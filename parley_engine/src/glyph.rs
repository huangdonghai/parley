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
    /// Spacing and justification gaps are folded into this for text that advances along it, so it
    /// is the inline advance for horizontal and sideways text. For upright text in a vertical
    /// layout it stays the glyph's plain horizontal extent instead, because
    /// [`Self::vertical_advance`] carries the inline axis and the extent is what the block axis is
    /// centred on.
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
