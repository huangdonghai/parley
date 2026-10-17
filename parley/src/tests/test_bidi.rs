// Copyright 2024 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Bidi tests for lines built from inline boxes only.

use alloc::vec::Vec;

use super::utils::ColorBrush;
use super::utils::fonts::{FONT_FAMILY_LIST, create_font_context};
use crate::{
    Alignment, AlignmentOptions, BaseDirection, FontFamily, InlineBox, InlineBoxKind, LayoutContext,
    PositionedLayoutItem, TextStyle, VerticalAlign,
};

fn root_style() -> TextStyle<'static, 'static, ColorBrush> {
    TextStyle {
        font_family: FontFamily::from(FONT_FAMILY_LIST),
        font_size: 20.,
        ..Default::default()
    }
}

fn inline_box_xs(base_direction: BaseDirection) -> Vec<(u64, f32)> {
    let mut fcx = create_font_context();
    let mut lcx: LayoutContext<ColorBrush> = LayoutContext::new();
    let root = root_style();
    let mut builder = lcx.tree_builder(&mut fcx, 1., false, &root);
    builder.set_base_direction(base_direction);
    for (id, width) in [(0_u64, 40.), (1, 20.)] {
        builder.push_inline_box(InlineBox {
            id,
            kind: InlineBoxKind::InFlow,
            index: 0,
            width,
            height: 20.,
            baseline: Some(20.),
            vertical_align: VerticalAlign::BASELINE,
        });
    }
    let (mut layout, _) = builder.build();
    assert_eq!(layout.is_rtl(), base_direction == BaseDirection::Rtl);
    layout.break_all_lines(None);
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout
        .lines()
        .next()
        .unwrap()
        .items()
        .filter_map(|item| match item {
            PositionedLayoutItem::InlineBox(inline_box) => Some((inline_box.id, inline_box.x)),
            _ => None,
        })
        .collect()
}

/// A line with no text at all still has to follow the paragraph direction: in an RTL paragraph the
/// first inline box is the right-most one.
#[test]
fn inline_boxes_only_line_follows_base_direction() {
    let ltr = inline_box_xs(BaseDirection::Ltr);
    assert_eq!(ltr, [(0, 0.), (1, 40.)]);

    let rtl = inline_box_xs(BaseDirection::Rtl);
    assert_eq!(rtl, [(1, 0.), (0, 20.)]);
}
