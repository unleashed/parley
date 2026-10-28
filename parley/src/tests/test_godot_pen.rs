// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The `godot-wrap` feature's glyph placement: each glyph on Godot's pen.

use std::{borrow::Cow, vec::Vec};

use parlance::FontFamilyName;
use peniko::color::palette;

use super::test_builders::create_font_context;
use super::utils::ColorBrush;
use crate::{FontFamily, Layout, LayoutContext, PositionedLayoutItem, StyleProperty};

const TEXT: &str = "Start a New Run?";
const SCALES: [f32; 3] = [1.0, 5.0 / 3.0, 5.0 / 6.0];

/// `TEXT` in Roboto at `size` design px laid out at `scale`, an MSDF import's or not; with
/// `split`, "New Run?" in a span of its own colour.
fn layout(size: f32, scale: f32, msdf: bool, split: bool) -> Layout<ColorBrush> {
    let mut fcx = create_font_context();
    let mut lcx = LayoutContext::new();
    let mut builder = lcx.ranged_builder(&mut fcx, TEXT, scale, false);
    builder.push_default(FontFamily::from(
        &[FontFamilyName::Named(Cow::Borrowed("Roboto"))][..],
    ));
    builder.push_default(StyleProperty::FontSize(size));
    builder.push_default(StyleProperty::GodotMsdf(msdf));
    if split {
        builder.push(
            StyleProperty::Brush(ColorBrush::new(palette::css::RED)),
            8..TEXT.len(),
        );
    }
    let mut layout = builder.build(TEXT);
    layout.break_all_lines(None);
    layout
}

/// Each glyph's pen, design px from the line's start, and the line's measured advance.
fn pens(layout: &Layout<ColorBrush>, scale: f32) -> (Vec<f32>, f32) {
    let line = layout.lines().next().unwrap();
    let mut pens = Vec::new();
    for item in line.items() {
        if let PositionedLayoutItem::GlyphRun(run) = item {
            pens.extend(run.positioned_glyphs().map(|glyph| glyph.x / scale));
            let last = run.positioned_glyphs().last().unwrap();
            assert!(
                (last.x + last.advance - run.offset() - run.advance()).abs() < 1e-3,
                "a run's last pen is its advance"
            );
        }
    }
    (pens, line.metrics().advance / scale)
}

/// Godot's pens over the exact cluster advances (design px): `step` per cluster (whole px or
/// not), the remainder reset at a blank and at `reset` (a span's start).
fn godot_pens(layout: &Layout<ColorBrush>, scale: f32, whole: bool, reset: usize) -> Vec<f32> {
    let line = layout.lines().next().unwrap();
    let mut pens = Vec::new();
    let (mut pen, mut remainder) = (0.0_f64, 0.0_f64);
    let mut at = 0;
    for item in line.items() {
        if let PositionedLayoutItem::GlyphRun(run) = item {
            for cluster in run.run().clusters() {
                if cluster.is_space_or_nbsp() || at == reset {
                    remainder = 0.0;
                }
                let step = f64::from(cluster.advance() / scale * 64.0 + 0.5 + 1e-3).floor() / 64.0;
                pens.push(pen as f32);
                if whole {
                    let full = remainder + step;
                    pen += full.round();
                    remainder = full - full.round();
                } else {
                    pen += step;
                }
                at += 1;
            }
            break;
        }
    }
    pens
}

/// Above 20 px a plain font's glyphs sit on the whole-px pen with the remainder carried, the
/// remainder reset at each blank, at every scale.
#[test]
fn a_plain_run_above_20_px_places_glyphs_on_the_carried_pen() {
    for scale in SCALES {
        let layout = layout(24.0, scale, false, false);
        let (pens, _) = pens(&layout, scale);
        let godot = godot_pens(&layout, scale, true, usize::MAX);
        assert_eq!(pens.len(), godot.len());
        for (pen, godot) in pens.iter().zip(&godot) {
            assert!(
                (pen - godot).abs() < 1e-3,
                "{pens:?} for {godot:?} at {scale}"
            );
            assert!((pen - pen.round()).abs() < 1e-3, "{pen} whole at {scale}");
        }
    }
}

/// Up to 20 px a plain font's pen is the sum of the 26.6 steps, no whole-px rounding.
#[test]
fn a_plain_run_up_to_20_px_places_glyphs_on_the_26_6_steps() {
    for scale in SCALES {
        let layout = layout(20.0, scale, false, false);
        let (pens, _) = pens(&layout, scale);
        let godot = godot_pens(&layout, scale, false, usize::MAX);
        for (pen, godot) in pens.iter().zip(&godot) {
            assert!(
                (pen - godot).abs() < 1e-3,
                "{pens:?} for {godot:?} at {scale}"
            );
        }
    }
}

/// An MSDF import keeps the exact positions: each pen the exact advances' sum.
#[test]
fn an_msdf_run_keeps_exact_glyph_positions() {
    for scale in SCALES {
        let layout = layout(24.0, scale, true, false);
        let (pens, _) = pens(&layout, scale);
        let line = layout.lines().next().unwrap();
        let mut exact = Vec::new();
        let mut pen = 0.0;
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                for cluster in run.run().clusters() {
                    exact.push(pen);
                    pen += cluster.advance() / scale;
                }
                break;
            }
        }
        for (pen, exact) in pens.iter().zip(&exact) {
            assert!(
                (pen - exact).abs() < 1e-3,
                "{pens:?} for {exact:?} at {scale}"
            );
        }
    }
}

/// The last glyph's pen plus its advance is the line breaker's measure for every rule, and a
/// coloured span starts where the run before it ends, its remainder reset.
#[test]
fn the_last_pen_is_the_measured_advance() {
    for scale in SCALES {
        for (size, msdf) in [(24.0, false), (20.0, false), (24.0, true)] {
            for split in [false, true] {
                let layout = layout(size, scale, msdf, split);
                let line = layout.lines().next().unwrap();
                let mut end = 0.0;
                for item in line.items() {
                    if let PositionedLayoutItem::GlyphRun(run) = item {
                        assert!(
                            (run.offset() - end).abs() < 1e-3,
                            "a span starts at the pen"
                        );
                        end = run.offset() + run.advance();
                    }
                }
                let (pens, advance) = pens(&layout, scale);
                assert!(
                    (end / scale - advance).abs() < 1e-3,
                    "{end} for {advance} at {scale}, {size} px, msdf {msdf}, split {split}"
                );
                if split && !msdf && size > 20.0 {
                    let godot = godot_pens(&layout, scale, true, 8);
                    assert_eq!(pens.len(), godot.len());
                    for (pen, godot) in pens.iter().zip(&godot) {
                        assert!((pen - godot).abs() < 1e-3, "{pens:?} for {godot:?}");
                    }
                }
            }
        }
    }
}
