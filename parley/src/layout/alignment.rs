// Copyright 2024 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use super::{BreakReason, Line, PositionedLayoutItem};
use crate::InlineBoxKind;
use crate::Layout;
use crate::style::Brush;
use alloc::vec::Vec;

/// Alignment of a layout.
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Alignment {
    /// This is [`Alignment::Left`] for LTR text and [`Alignment::Right`] for RTL text.
    #[default]
    Start,
    /// This is [`Alignment::Right`] for LTR text and [`Alignment::Left`] for RTL text.
    End,
    /// Align content to the left edge.
    ///
    /// For alignment that should be aware of text direction, use [`Alignment::Start`] or
    /// [`Alignment::End`] instead.
    Left,
    /// Align each line centered within the container.
    Center,
    /// Align content to the right edge.
    ///
    /// For alignment that should be aware of text direction, use [`Alignment::Start`] or
    /// [`Alignment::End`] instead.
    Right,
    /// Justify each line by spacing out content.
    ///
    /// The last line of a paragraph is start-aligned instead, unless
    /// [`AlignmentOptions::last_line_alignment`] is set.
    Justify,
}

/// Additional options to fine tune alignment
#[derive(Debug, Clone, Copy)]
pub struct AlignmentOptions {
    /// If set to `true`, "end" and "center" alignment will apply even if the line contents are
    /// wider than the alignment width. If it is set to `false`, all overflowing lines will be
    /// [`Alignment::Start`] aligned.
    pub align_when_overflowing: bool,
    /// The alignment of the last line of each paragraph: the last line of the layout and every
    /// line that ends in an explicit line break. This corresponds to the CSS `text-align-last`
    /// property.
    ///
    /// If set to `None`, those lines use the alignment passed to [`Layout::align`], except that
    /// [`Alignment::Justify`] falls back to [`Alignment::Start`]. This is the behavior of
    /// `text-align-last: auto`.
    ///
    /// [`Layout::align`]: crate::Layout::align
    pub last_line_alignment: Option<Alignment>,
}

#[expect(
    clippy::derivable_impls,
    reason = "Make default values explicit rather than relying on the implicit default value of bool"
)]
impl Default for AlignmentOptions {
    fn default() -> Self {
        Self {
            align_when_overflowing: false,
            last_line_alignment: None,
        }
    }
}

/// Align the layout.
pub(crate) fn align<B: Brush>(
    layout: &mut Layout<B>,
    alignment: Alignment,
    options: AlignmentOptions,
) {
    layout.data.alignment = Some(alignment);

    let is_rtl = layout.data.base_level.is_rtl();
    // Alignment can be requested more than once with different values. Custom
    // compositions are expanded below, so restore their natural widths before
    // measuring each pass.
    for layout_box in &mut layout.data.inline_boxes {
        if let Some((_, _, _, _, natural_box_width)) = layout_box.justification_profile {
            layout_box.inline_box.width = natural_box_width;
        }
    }
    for line in &mut layout.data.lines {
        line.justification.amount_per_opportunity = 0.;
        line.metrics.offset = 0.;
    }
    let custom_justification_by_line: Vec<Vec<(usize, f32, u32, f32, u32)>> = layout
        .data
        .lines
        .iter()
        .map(|line| {
            layout.data.line_items[line.item_range.clone()]
                .iter()
                .filter_map(|item| {
                    if item.kind != super::LayoutItemKind::InlineBox {
                        return None;
                    }
                    let layout_box = &layout.data.inline_boxes[item.index];
                    let (first_width, first_count, second_width, second_count, _) =
                        layout_box.justification_profile?;
                    Some((
                        item.index,
                        first_width,
                        first_count,
                        second_width,
                        second_count,
                    ))
                })
                .collect()
        })
        .collect();
    // The breaker's running x does not include every contextual spacing
    // adjustment (notably interior text-spacing-trim), while the painted runs
    // do. Reconstruct the unpainted line advance from those runs so
    // justification distributes only the room that actually remains.
    let visual_advances: Vec<Option<f32>> = {
        let layout_ref: &Layout<B> = layout;
        (0..layout_ref.data.lines.len())
            .map(|index| {
                let data = &layout_ref.data.lines[index];
                if is_rtl || !data.end_hanging {
                    return None;
                }
                let line = Line {
                    layout: layout_ref,
                    index: index as u32,
                    data,
                };
                let mut advance = 0.0;
                for item in line.items() {
                    match item {
                        PositionedLayoutItem::GlyphRun(run) => advance += run.advance(),
                        PositionedLayoutItem::InlineBox(inline_box)
                            if matches!(
                                inline_box.kind,
                                InlineBoxKind::InFlow
                                    | InlineBoxKind::CustomInFlow
                                    | InlineBoxKind::CustomInFlowCentered
                                    | InlineBoxKind::InlineEdge
                            ) =>
                        {
                            advance += inline_box.width;
                        }
                        PositionedLayoutItem::InlineBox(_) => {}
                    }
                }
                Some(advance + data.metrics.hanging_advance)
            })
            .collect()
    };

    // Apply alignment to line items
    let mut custom_width_expansions = Vec::new();
    for (index, line) in layout.data.lines.iter_mut().enumerate() {
        if let Some(advance) = visual_advances[index] {
            line.metrics.advance = advance;
        }

        let indent = line.metrics.indent;

        if is_rtl {
            // In RTL text, trailing whitespace is on the left. As we hang that whitespace, offset
            // the line to the left. Note: indent is not subtracted here because `free_space` below
            // already accounts for it.
            line.metrics.offset = -line.metrics.hanging_advance;
        } else {
            line.metrics.offset = indent;
        }

        // Compute free space.
        let line_width = line.metrics.inline_max_coord - line.metrics.inline_min_coord;
        // `advance` already excludes a hanging start glyph (CSS Text 3 §8.2: it is not
        // considered when measuring for alignment), but still includes a hanging end
        // glyph, which is added back below.
        let free_space = line_width - indent - line.metrics.advance + line.metrics.hanging_advance;

        if !options.align_when_overflowing && free_space <= 0.0 {
            if is_rtl {
                // In RTL text, right-align on overflow.
                line.metrics.offset += free_space;
            }
            continue;
        }

        let is_last_line = matches!(line.break_reason, BreakReason::None | BreakReason::Explicit);
        let line_alignment = if line.suppress_justify {
            Alignment::Start
        } else if !is_last_line {
            alignment
        } else if let Some(last_line_alignment) = options.last_line_alignment {
            last_line_alignment
        } else if alignment == Alignment::Justify {
            Alignment::Start
        } else {
            alignment
        };

        match (line_alignment, is_rtl) {
            (Alignment::Left, _) | (Alignment::Start, false) | (Alignment::End, true) => {
                // Do nothing
            }
            (Alignment::Right, _) | (Alignment::Start, true) | (Alignment::End, false) => {
                line.metrics.offset += free_space;
            }
            (Alignment::Center, _) => {
                line.metrics.offset += free_space * 0.5;
            }
            (Alignment::Justify, _) => {
                // Justified alignment doesn't have any effect if free_space is negative or zero
                if free_space <= 0.0 {
                    continue;
                }

                let custom_profiles = &custom_justification_by_line[index];
                if custom_profiles.is_empty() {
                    if line.num_justification_opportunities == 0 {
                        if is_rtl {
                            line.metrics.offset += free_space;
                        }
                    } else {
                        line.justification.amount_per_opportunity =
                            free_space / line.num_justification_opportunities as f32;
                    }
                    continue;
                }
                let custom_opportunities: u32 = custom_profiles
                    .iter()
                    .map(|(_, _, first, _, second)| (*first).max(*second))
                    .sum();
                let total_opportunities = line
                    .num_justification_opportunities
                    .saturating_add(custom_opportunities);
                // Justified alignment doesn't apply if there are no text or
                // composition gaps to adjust.
                if total_opportunities == 0 {
                    if is_rtl {
                        line.metrics.offset += free_space;
                    }
                    continue;
                }

                let total_growth = |amount: f32| {
                    let text_growth = line.num_justification_opportunities as f32 * amount;
                    custom_profiles.iter().fold(text_growth, |growth, profile| {
                        let (_, first_width, first_count, second_width, second_count) = *profile;
                        let natural_width = first_width.max(second_width);
                        let justified_width = (first_width + first_count as f32 * amount)
                            .max(second_width + second_count as f32 * amount);
                        growth + (justified_width - natural_width)
                    })
                };
                let max_natural_width = custom_profiles
                    .iter()
                    .map(|(_, first_width, _, second_width, _)| {
                        (*first_width).max(*second_width)
                    })
                    .sum::<f32>();
                let mut low = 0.0;
                let mut high = free_space + max_natural_width + 1.0;
                for _ in 0..32 {
                    let middle = (low + high) * 0.5;
                    if total_growth(middle) < free_space {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                let amount = high;
                line.justification.amount_per_opportunity = amount;
                for (box_index, first_width, first_count, second_width, second_count) in
                    custom_profiles
                {
                    let natural_width = first_width.max(*second_width);
                    let justified_width = (*first_width + *first_count as f32 * amount)
                        .max(*second_width + *second_count as f32 * amount);
                    custom_width_expansions.push((*box_index, justified_width - natural_width));
                }
            }
        }
    }

    for (index, expansion) in custom_width_expansions {
        if let Some(layout_box) = layout.data.inline_boxes.get_mut(index) {
            layout_box.inline_box.width += expansion;
        }
    }
}
