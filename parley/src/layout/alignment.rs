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
    for line in &mut layout.data.lines {
        line.justification.amount_per_opportunity = 0.;
        line.metrics.offset = 0.;
    }
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
                                InlineBoxKind::InFlow | InlineBoxKind::InlineEdge
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
        let line_alignment = if !is_last_line {
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

                // Justified alignment doesn't apply if there are no whitespace gaps to adjust. In
                // that case, start-align, i.e., left-align for LTR text and right-align for RTL
                // text.
                if line.num_justification_opportunities == 0 {
                    if is_rtl {
                        line.metrics.offset += free_space;
                    }
                    continue;
                }

                line.justification.amount_per_opportunity =
                    free_space / line.num_justification_opportunities as f32;
            }
        }
    }
}
