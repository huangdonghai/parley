// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Tests for spacing style properties.

use crate::test_name;
use crate::util::{ColorBrush, TestEnv, samples};
use parley::layout::Alignment;
use parley::style::{FontFamily, LineHeight, StyleProperty};
use parley::{
    AlignmentOptions, HangingPunctuation, Layout, PositionedLayoutItem, TextOrientation,
    TextSpacingTrim,
};

/// Helper to build a layout with line height applied
fn build_with_line_height(
    env: &mut TestEnv,
    text: &str,
    line_height: LineHeight,
) -> Layout<ColorBrush> {
    let mut builder = env.ranged_builder(text);
    builder.push_default(StyleProperty::LineHeight(line_height));
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout
}

/// Helper to build a layout with letter spacing applied
fn build_with_letter_spacing(env: &mut TestEnv, text: &str, spacing: f32) -> Layout<ColorBrush> {
    let mut builder = env.ranged_builder(text);
    builder.push_default(StyleProperty::LetterSpacing(spacing));
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout
}

/// Helper to build a layout with word spacing applied
fn build_with_word_spacing(env: &mut TestEnv, text: &str, spacing: f32) -> Layout<ColorBrush> {
    let mut builder = env.ranged_builder(text);
    builder.push_default(StyleProperty::WordSpacing(spacing));
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout
}

// ============================================================================
// LineHeight Tests
// ============================================================================

#[test]
fn style_line_height_absolute() {
    let mut env = TestEnv::new(test_name!(), None);
    let text = samples::LATIN_MULTILINE;

    for height in [16.0, 24.0, 32.0, 48.0] {
        let layout = build_with_line_height(&mut env, text, LineHeight::Absolute(height));

        env.with_name(&format!("abs_{height}"))
            .check_layout_snapshot(&layout);
    }
}

#[test]
fn style_line_height_font_relative() {
    let mut env = TestEnv::new(test_name!(), None);
    let text = samples::LATIN_MULTILINE;

    for factor in [1.0, 1.2, 1.5, 2.0] {
        let layout = build_with_line_height(&mut env, text, LineHeight::FontSizeRelative(factor));

        // Use underscore in name to avoid dots in filename
        let name = format!("rel_{}", (factor * 10.0) as i32);
        env.with_name(&name).check_layout_snapshot(&layout);
    }
}

#[test]
fn style_line_height_metrics_relative() {
    let mut env = TestEnv::new(test_name!(), None);
    let text = samples::LATIN_MULTILINE;

    for factor in [0.8, 1.0, 1.2, 1.5] {
        let layout = build_with_line_height(&mut env, text, LineHeight::MetricsRelative(factor));

        let name = format!("metrics_{}", (factor * 10.0) as i32);
        env.with_name(&name).check_layout_snapshot(&layout);
    }
}

// ============================================================================
// LetterSpacing Tests
// ============================================================================

#[test]
fn style_letter_spacing_positive() {
    let mut env = TestEnv::new(test_name!(), None);
    let text = samples::LATIN;

    for spacing in [1.0, 2.0, 4.0, 8.0] {
        let layout = build_with_letter_spacing(&mut env, text, spacing);

        env.with_name(&format!("pos_{spacing}"))
            .check_layout_snapshot(&layout);
    }
}

#[test]
fn style_letter_spacing_negative() {
    let mut env = TestEnv::new(test_name!(), None);
    let text = samples::LATIN;

    for spacing in [-1.0, -2.0] {
        let layout = build_with_letter_spacing(&mut env, text, spacing);

        let name = format!("neg_{}", (-spacing) as i32);
        env.with_name(&name).check_layout_snapshot(&layout);
    }
}

#[test]
fn style_letter_spacing_with_ligatures() {
    let mut env = TestEnv::new(test_name!(), None);
    let text = samples::LIGATURES;

    // Without letter spacing - ligatures may form
    let layout_no_spacing = build_with_letter_spacing(&mut env, text, 0.0);
    env.with_name("no_spacing")
        .check_layout_snapshot(&layout_no_spacing);

    // With letter spacing - ligatures should break
    let layout_with_spacing = build_with_letter_spacing(&mut env, text, 2.0);
    env.with_name("with_spacing")
        .check_layout_snapshot(&layout_with_spacing);
}

// ============================================================================
// WordSpacing Tests
// ============================================================================

#[test]
fn style_word_spacing_positive() {
    let mut env = TestEnv::new(test_name!(), None);
    let text = samples::SPACED;

    for spacing in [2.0, 4.0, 8.0, 16.0] {
        let layout = build_with_word_spacing(&mut env, text, spacing);

        env.with_name(&format!("pos_{spacing}"))
            .check_layout_snapshot(&layout);
    }
}

#[test]
fn style_word_spacing_negative() {
    let mut env = TestEnv::new(test_name!(), None);
    let text = samples::SPACED;

    for spacing in [-2.0, -4.0] {
        let layout = build_with_word_spacing(&mut env, text, spacing);

        let name = format!("neg_{}", (-spacing) as i32);
        env.with_name(&name).check_layout_snapshot(&layout);
    }
}

#[test]
fn cluster_spacing_gaps_match_advance_delta() {
    let mut env = TestEnv::new(test_name!(), None);
    let layout = build_with_letter_spacing(&mut env, "abc", 2.0);

    let mut saw_cluster = false;
    for line in layout.lines() {
        for run in line.runs() {
            for cluster in run.clusters() {
                let gap = cluster.advance() - cluster.shaped_advance();
                let split = cluster.spacing_before() + cluster.spacing_after();
                assert!(
                    (gap - split).abs() < f32::EPSILON * 8.0,
                    "cluster spacing must account for the full advance delta"
                );
                saw_cluster = true;
            }
        }
    }
    assert!(saw_cluster);
}

#[test]
fn vertical_metrics_are_scaled_per_run() {
    let mut env = TestEnv::new(test_name!(), None);
    let text = "AA";
    let mut builder = env.ranged_builder(text);
    builder.push_default(StyleProperty::FontFamily(FontFamily::named(
        "Noto Sans CJK SC",
    )));
    builder.push(StyleProperty::FontSize(16.0), 0..1);
    builder.push(StyleProperty::FontSize(32.0), 1..2);
    let mut layout = builder.build(text);
    layout.set_vertical_advances(text, TextOrientation::Upright);
    layout.break_all_lines(None);

    let mut ratios = Vec::new();
    for line in layout.lines() {
        for run in line.runs() {
            let font_size = run.font_size();
            for cluster in run.clusters() {
                let glyph_advance: f32 = cluster
                    .glyphs()
                    .map(|glyph| glyph.vertical_advance)
                    .sum();
                assert!((cluster.advance() - glyph_advance).abs() < 0.001);
                for glyph in cluster.glyphs() {
                    if glyph.vertical_advance > 0.0 {
                        ratios.push(glyph.vertical_advance / font_size);
                    }
                }
            }
        }
    }

    assert!(!ratios.is_empty());
    let first = ratios[0];
    for ratio in ratios {
        assert!((ratio - first).abs() < 0.001);
    }
}

fn build_trimmed(env: &mut TestEnv, text: &str, trim: parley::TextSpacingTrim) -> Layout<ColorBrush> {
    let mut builder = env.ranged_builder(text);
    builder.push_default(StyleProperty::FontFamily(FontFamily::named(
        "Noto Sans CJK SC",
    )));
    builder.push_default(StyleProperty::TextSpacingTrim(trim));
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    layout
}

/// The `spacing_after` + `shaped_advance` deltas per cluster, split by side.
fn spacing_sides(layout: &Layout<ColorBrush>) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for line in layout.lines() {
        for run in line.runs() {
            for cluster in run.clusters() {
                out.push((cluster.spacing_before(), cluster.spacing_after()));
            }
        }
    }
    out
}

#[test]
fn text_spacing_trim_all_trims_every_punctuation_side() {
    let mut env = TestEnv::new(test_name!(), None);
    let layout = build_trimmed(&mut env, "国（国）国", TextSpacingTrim::TrimAll);
    let sides = spacing_sides(&layout);
    assert_eq!(sides.len(), 5);
    assert!(sides[1].0 < 0., "horizontal opening trims its leading half: {sides:?}");
    assert_eq!(sides[1].1, 0.);
    assert!(sides[3].1 < 0., "closing trims its trailing half: {sides:?}");
}

#[test]
fn text_spacing_trim_normal_collapses_only_adjacent_pairs() {
    let mut env = TestEnv::new(test_name!(), None);
    // ）（ adjacent: both collapse under the default policy.
    let layout = build_trimmed(&mut env, "国）（国", TextSpacingTrim::normal());
    let sides = spacing_sides(&layout);
    assert_eq!(sides[1], (0., 0.), "closing before opening stays full: {sides:?}");
    assert!(sides[2].0 < 0., "opening after closing collapses: {sides:?}");
    // Mid-line, non-adjacent punctuation stays spaced.
    let layout = build_trimmed(&mut env, "国（国", TextSpacingTrim::normal());
    let sides = spacing_sides(&layout);
    assert_eq!(sides, vec![(0., 0.), (0., 0.), (0., 0.)]);
}

#[test]
fn text_spacing_trim_start_and_end_policies() {
    let mut env = TestEnv::new(test_name!(), None);
    // `normal` (space-start): line-start opening is not trimmed.
    let layout = build_trimmed(&mut env, "（国", TextSpacingTrim::normal());
    let sides = spacing_sides(&layout);
    assert_eq!(sides[0].0, 0., "space-start must not trim: {sides:?}");
    // `trim-start` policy trims the line-start opening.
    let trim_start = TextSpacingTrim::new(
        parley::TrimStart::Trim,
        parley::TrimAdjacent::Trim,
        parley::TrimEnd::Allow,
    );
    let layout = build_trimmed(&mut env, "（国", trim_start);
    let sides = spacing_sides(&layout);
    assert!(sides[0].0 < 0., "trim-start must trim: {sides:?}");
    // `trim-end` trims the line-final closing punctuation.
    let trim_end = TextSpacingTrim::new(
        parley::TrimStart::Space,
        parley::TrimAdjacent::Trim,
        parley::TrimEnd::Trim,
    );
    let layout = build_trimmed(&mut env, "国）", trim_end);
    let sides = spacing_sides(&layout);
    assert!(sides[1].1 < 0., "trim-end must trim: {sides:?}");
}

#[test]
fn text_spacing_trim_space_all_never_trims() {
    let mut env = TestEnv::new(test_name!(), None);
    let layout = build_trimmed(&mut env, "国（（）国", TextSpacingTrim::space_all());
    for (before, after) in spacing_sides(&layout) {
        assert_eq!((before, after), (0., 0.));
    }
}

#[test]
fn hanging_punctuation_gives_up_its_trimmed_advance() {
    let mut env = TestEnv::new(test_name!(), None);
    let text = "国。";
    let mut builder = env.ranged_builder(text);
    builder.push_default(StyleProperty::FontFamily(FontFamily::named(
        "Noto Sans CJK SC",
    )));
    builder.push_default(StyleProperty::TextSpacingTrim(TextSpacingTrim::TrimAll));
    builder.push_default(StyleProperty::HangingPunctuation(
        HangingPunctuation::FORCE_END,
    ));
    let mut layout = builder.build(text);
    layout.set_vertical(true);
    layout.break_all_lines(Some(200.0));

    let line = layout.get(0).unwrap();
    let mut stop = None;
    let mut hanging = None;
    for item in line.items() {
        let PositionedLayoutItem::GlyphRun(run) = item else {
            continue;
        };
        for cluster in run.clusters() {
            if cluster.text_range().start == "国".len() {
                stop = Some(cluster);
                hanging = Some(run.hanging_punctuation_advance().1);
            }
        }
    }

    let stop = stop.expect("the stop");
    let hanging = hanging.expect("the hanging stop's run");
    let half = stop.shaped_advance() / 2.;
    assert!(
        (hanging - half).abs() < 0.01,
        "a hung stop must give up its trimmed advance ({hanging}), not its full-width one ({})",
        stop.shaped_advance()
    );
    let body = line
        .runs()
        .flat_map(|run| run.clusters())
        .find(|cluster| cluster.text_range().start == 0)
        .expect("the first character")
        .advance();
    let laid_out_body = line.metrics().advance - line.metrics().hanging_advance;
    assert!(
        (laid_out_body - body).abs() < 0.01,
        "the line minus the hung advance must be the body advance ({laid_out_body} vs {body})"
    );
}

// ============================================================================
// Run Splitting Tests
// ============================================================================

#[test]
fn spacing_causes_style_run_breaks() {
    let mut env = TestEnv::new(test_name!(), None);

    let text = "foo bar";
    let mut builder = env.ranged_builder(text);
    builder.push(StyleProperty::WordSpacing(2.0), 3..text.len());
    builder.push(StyleProperty::LetterSpacing(1.5), 3..text.len());

    let mut layout = builder.build(text);
    layout.break_all_lines(None);

    // Collect all runs from the layout
    let runs: Vec<_> = layout.lines().flat_map(|line| line.runs()).collect();

    assert_eq!(
        runs.len(),
        2,
        "expected two runs after spacing property changes"
    );

    // Verify text ranges are split correctly
    assert_eq!(&text[runs[0].text_range()], "foo",);
    assert_eq!(&text[runs[1].text_range()], " bar",);

    env.with_name("spacing_style_run_breaks")
        .check_layout_snapshot(&layout);
}

// ============================================================================
// Grapheme tests
// ============================================================================

#[test]
fn spacing_and_multi_shaped_cluster_graphemes() {
    let mut env = TestEnv::new(test_name!(), None);

    // The combining acute accent `U+0301` combines with the character to form a grapheme, including
    // if that previous character is a space. Our Roboto font doesn't carry a glyph for `sp+U+0301`
    // or `h+U+0301`, so both become two shaped clusters, but form one grapheme.
    //
    // Compare with the following HTML.
    //
    // ```html
    // <p style="letter-spacing: 10px; background: #eee; width: fit-content;">a &#x0301;bh&#x0301;d</p>
    // <p style="word-spacing: 10px; background: #eee; width: fit-content;">a &#x0301;bh&#x0301;d</p>
    // ```
    let text = "a \u{0301}bh\u{0301}d";

    {
        let mut builder = env.ranged_builder(text);
        builder.push(StyleProperty::LetterSpacing(10.), 0..text.len());

        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        env.with_name("letter").check_layout_snapshot(&layout);
    }

    {
        let mut builder = env.ranged_builder(text);
        builder.push(StyleProperty::WordSpacing(10.), 0..text.len());

        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        env.with_name("word").check_layout_snapshot(&layout);
    }
}
