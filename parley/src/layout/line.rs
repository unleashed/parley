// Copyright 2021 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::layout::Style;
use crate::layout::data::BreakReason;
use crate::layout::data::{LayoutItemKind, LineData};
use crate::layout::glyph::Glyph;
use crate::layout::layout::Layout;
#[cfg(feature = "godot-wrap")]
use crate::layout::line_break::{godot_cluster_advance, godot_cluster_pen};
use crate::layout::run::Run;
use crate::style::Brush;
use crate::{InlineBox, InlineBoxKind};
use core::ops::Range;

/// Line in a text layout.
#[derive(Copy, Clone)]
pub struct Line<'a, B: Brush> {
    pub(crate) layout: &'a Layout<B>,
    pub(crate) index: u32,
    pub(crate) data: &'a LineData,
}

impl<'a, B: Brush> Line<'a, B> {
    /// Returns the metrics for the line.
    pub fn metrics(&self) -> &LineMetrics {
        &self.data.metrics
    }

    pub fn break_reason(&self) -> BreakReason {
        self.data.break_reason
    }

    /// Returns the range of text for the line.
    pub fn text_range(&self) -> Range<usize> {
        self.data.text_range.clone()
    }

    /// Returns the number of items in the line.
    pub fn len(&self) -> usize {
        self.data.item_range.len()
    }

    /// Returns `true` if the line is empty.
    pub fn is_empty(&self) -> bool {
        self.data.item_range.is_empty()
    }

    /// Returns the line item at the specified index.
    pub(crate) fn item(&self, index: usize) -> Option<LineItem<'a, B>> {
        let original_index = index;
        let index = self.data.item_range.start + index;
        if index >= self.data.item_range.end {
            return None;
        }
        let item = self.layout.data.line_items.get(index)?;

        Some(match item.kind {
            LayoutItemKind::TextRun => LineItem::Run(Run {
                layout: self.layout,
                line_index: self.index,
                index: original_index as u32,
                data: self.layout.data.runs.get(item.index)?,
                line_data: Some(item),
            }),
            LayoutItemKind::InlineBox => {
                LineItem::InlineBox(self.layout.data.inline_boxes.get(item.index)?)
            }
        })
    }

    /// Returns an iterator over the runs for the line.
    pub fn runs(&self) -> impl Iterator<Item = Run<'a, B>> + 'a + Clone {
        self.items_nonpositioned().filter_map(|item| item.run())
    }

    /// Returns an iterator over the non-glyph runs and inline boxes for the line.
    pub(crate) fn items_nonpositioned(&self) -> impl Iterator<Item = LineItem<'a, B>> + Clone {
        let copy = self.clone();
        let line_items = &copy.layout.data.line_items[self.data.item_range.clone()];
        line_items
            .iter()
            .enumerate()
            .map(move |(item_index, line_data)| match line_data.kind {
                LayoutItemKind::TextRun => LineItem::Run(Run {
                    layout: copy.layout,
                    line_index: copy.index,
                    index: item_index as u32,
                    data: &copy.layout.data.runs[line_data.index],
                    line_data: Some(line_data),
                }),
                LayoutItemKind::InlineBox => {
                    LineItem::InlineBox(&copy.layout.data.inline_boxes[line_data.index])
                }
            })
    }

    /// Returns an iterator over the glyph runs for the line.
    pub fn items(&self) -> impl Iterator<Item = PositionedLayoutItem<'a, B>> + 'a + Clone {
        GlyphRunIter {
            line: self.clone(),
            item_index: 0,
            glyph_start: 0,
            offset: 0.,
        }
    }
}

/// Metrics information for a line.
#[derive(Copy, Clone, Default, Debug, PartialEq)]
pub struct LineMetrics {
    /// Typographic ascent.
    pub ascent: f32,
    /// Typographic descent.
    pub descent: f32,
    /// Typographic leading.
    pub leading: f32,
    /// The absolute line height (in layout units).
    pub line_height: f32,
    /// Offset to the baseline.
    pub baseline: f32,
    /// Offset for alignment.
    pub offset: f32,
    /// Full advance of the line, including trailing whitespace.
    pub advance: f32,
    /// Advance of trailing whitespace.
    pub trailing_whitespace: f32,
    /// Minimum coordinate in the line direction.
    ///
    /// For horizontal text, this would be the left of the line.
    pub inline_min_coord: f32,
    /// Maximum coordinate in the line direction.
    ///
    /// For horizontal text, this would be the right of the line.
    pub inline_max_coord: f32,
    /// Minimum coordinate in the direction orthogonal to line
    /// direction.
    ///
    /// For horizontal text, this would be the top of the line.
    pub block_min_coord: f32,
    /// Maximum coordinate in the direction orthogonal to line
    /// direction.
    ///
    /// For horizontal text, this would be the bottom of the line.
    pub block_max_coord: f32,
}

impl LineMetrics {
    /// Returns the size of the line
    pub fn size(&self) -> f32 {
        self.line_height
    }
}

/// A line item and its corresponding data (a run or inline box). Unlike a
/// [`PositionedLayoutItem`], runs are not guaranteed to be split by style.
pub(crate) enum LineItem<'a, B: Brush> {
    Run(Run<'a, B>),
    InlineBox(&'a InlineBox),
}

impl<'a, B: Brush> LineItem<'a, B> {
    pub(crate) fn run(self) -> Option<Run<'a, B>> {
        match self {
            LineItem::Run(run) => Some(run),
            _ => None,
        }
    }
}

/// The computed result of an item (glyph run or inline box) within a layout
#[derive(Clone)]
pub enum PositionedLayoutItem<'a, B: Brush> {
    GlyphRun(GlyphRun<'a, B>),
    InlineBox(PositionedInlineBox),
}

/// The computed position of an inline box within a layout
#[derive(Debug, Clone)]
pub struct PositionedInlineBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub id: u64,
    pub kind: InlineBoxKind,
}

/// Sequence of fully positioned glyphs with the same style.
#[derive(Clone)]
pub struct GlyphRun<'a, B: Brush> {
    run: Run<'a, B>,
    style: &'a Style<B>,
    glyph_start: usize,
    glyph_count: usize,
    offset: f32,
    baseline: f32,
    advance: f32,
}

impl<'a, B: Brush> GlyphRun<'a, B> {
    /// Returns the underlying run.
    pub fn run(&self) -> &Run<'a, B> {
        &self.run
    }

    /// Returns the associated style.
    pub fn style(&self) -> &Style<B> {
        self.style
    }

    /// Returns the offset to the baseline.
    pub fn baseline(&self) -> f32 {
        self.baseline
    }

    /// Returns the offset to the first glyph along the baseline.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// Returns the total advance of the run.
    pub fn advance(&self) -> f32 {
        self.advance
    }

    /// Returns an iterator over the glyphs in the run.
    pub fn glyphs(&'a self) -> impl Iterator<Item = Glyph> + 'a + Clone {
        run_glyphs(&self.run)
            .skip(self.glyph_start)
            .take(self.glyph_count)
    }

    /// Returns an iterator over the fully positioned glyphs in the run.
    pub fn positioned_glyphs(&'a self) -> impl Iterator<Item = Glyph> + 'a + Clone {
        let mut offset = self.offset;
        let baseline = self.baseline;
        self.glyphs().map(move |mut g| {
            g.x += offset;
            g.y += baseline;
            offset += g.advance;
            g
        })
    }
}

/// The glyphs of `run` in visual order, as [`GlyphRun::glyphs`] and the line's glyph runs take
/// them.
#[cfg(not(feature = "godot-wrap"))]
fn run_glyphs<'a, B: Brush>(run: &'a Run<'a, B>) -> impl Iterator<Item = Glyph> + 'a + Clone {
    run.visual_clusters().flat_map(|cluster| cluster.glyphs())
}

/// The glyphs of `run` in visual order, as [`GlyphRun::glyphs`] and the line's glyph runs take
/// them (the `godot-wrap` feature): each glyph at Godot's x offset and each cluster Godot's
/// advance (`line_break::GodotPen`), so a glyph sits on Godot's pen (a plain font above 20 px: whole px with
/// the remainder carried; up to 20 px: 26.6 steps; an MSDF import: exact) and a style run's
/// glyphs sum to the line breaker's measure. A cluster's advance goes to its last glyph, the
/// others keep theirs; a ligature's components, which carry no glyph, add theirs to it.
#[cfg(feature = "godot-wrap")]
fn run_glyphs<'a, B: Brush>(run: &'a Run<'a, B>) -> impl Iterator<Item = Glyph> + 'a + Clone {
    run.visual_clusters().flat_map(|cluster| {
        let data = &cluster.run.layout.data;
        let run_data = cluster.run.data;
        let range = cluster.run.cluster_range();
        let index = range.start + cluster.path.logical_index();
        let mut pen = godot_cluster_pen(data, run_data, index);
        let mut advance = pen.advance(data.clusters[index].advance);
        advance += (index + 1..range.end)
            .take_while(|&next| {
                data.clusters[next].is_ligature_component() && data.clusters[next].glyph_len == 0
            })
            .map(|next| godot_cluster_advance(data, run_data, next))
            .sum::<f32>();
        let count = cluster.glyphs().count();
        let mut before = 0.0;
        cluster.glyphs().enumerate().map(move |(at, mut glyph)| {
            glyph.x = pen.offset(glyph.x);
            if at + 1 == count {
                glyph.advance = advance - before;
            } else {
                before += glyph.advance;
            }
            glyph
        })
    })
}

#[derive(Clone)]
struct GlyphRunIter<'a, B: Brush> {
    line: Line<'a, B>,
    item_index: usize,
    glyph_start: usize,
    offset: f32,
}

impl<'a, B: Brush> Iterator for GlyphRunIter<'a, B> {
    type Item = PositionedLayoutItem<'a, B>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let item = self.line.item(self.item_index)?;
            match item {
                LineItem::InlineBox(inline_box) => {
                    let x = self.offset
                        + self.line.data.metrics.inline_min_coord
                        + self.line.data.metrics.offset;

                    self.item_index += 1;
                    self.glyph_start = 0;
                    if matches!(
                        inline_box.kind,
                        InlineBoxKind::InFlow | InlineBoxKind::InFlowCentered
                    ) {
                        self.offset += inline_box.width;
                    }
                    let metrics = &self.line.data.metrics;
                    let bottom = if inline_box.kind == InlineBoxKind::InFlowCentered {
                        metrics.baseline
                            - (metrics.ascent - metrics.descent - inline_box.height) * 0.5
                    } else {
                        metrics.baseline
                    };
                    return Some(PositionedLayoutItem::InlineBox(PositionedInlineBox {
                        x,
                        y: bottom - inline_box.height,
                        width: inline_box.width,
                        height: inline_box.height,
                        id: inline_box.id,
                        kind: inline_box.kind,
                    }));
                }
                LineItem::Run(run) => {
                    let mut iter = run_glyphs(&run).skip(self.glyph_start);

                    if let Some(first) = iter.next() {
                        let mut advance = first.advance;
                        let style_index = first.style_index();
                        let mut glyph_count = 1;
                        for glyph in iter.take_while(|g| g.style_index() == style_index) {
                            glyph_count += 1;
                            advance += glyph.advance;
                        }
                        let style = run.layout.data.styles.get(style_index)?;
                        let glyph_start = self.glyph_start;
                        self.glyph_start += glyph_count;
                        let offset = self.offset;
                        self.offset += advance;
                        return Some(PositionedLayoutItem::GlyphRun(GlyphRun {
                            run,
                            style,
                            glyph_start,
                            glyph_count,
                            offset: offset
                                + self.line.data.metrics.inline_min_coord
                                + self.line.data.metrics.offset,
                            baseline: self.line.data.metrics.baseline,
                            advance,
                        }));
                    }
                    self.item_index += 1;
                    self.glyph_start = 0;
                }
            }
        }
    }
}
