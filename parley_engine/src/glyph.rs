// Copyright 2025 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

/// Glyph with an offset and advance.
#[derive(Copy, Clone, Default, Debug, PartialEq)]
#[expect(missing_docs, reason = "Deferred")]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    /// Horizontal advance in layout units.
    ///
    /// This remains the glyph's horizontal extent in vertical layout, where
    /// [`Self::vertical_advance`] is used along the inline axis.
    pub advance: f32,
    /// Vertical origin in layout units, measured from the inline pen.
    ///
    /// This is zero for horizontal layout and for sideways glyphs.
    pub vertical_origin: f32,
    /// Vertical advance in layout units.
    ///
    /// This is zero for horizontal layout and for sideways glyphs.
    pub vertical_advance: f32,
}
