// Copyright 2025 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

/// The paragraph's base direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BaseDirection {
    /// Choose direction automatically (commonly "first-strong").
    #[default]
    Auto,
    /// Left-to-right.
    Ltr,
    /// Right-to-left.
    Rtl,
}

/// Control over word breaking, named for the CSS property.
///
/// See: <https://www.w3.org/TR/css-text-3/#word-break-property>
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WordBreak {
    /// Customary rules.
    #[default]
    Normal,
    /// Breaking is allowed within "words".
    BreakAll,
    /// Breaking is forbidden within "words".
    KeepAll,
}

/// Strictness of line-breaking rules, named for the CSS property.
///
/// See: <https://www.w3.org/TR/css-text-3/#line-break-property>
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineBreak {
    /// The least restrictive set of line-breaking rules.
    Loose,
    /// The most common set of line-breaking rules.
    Normal,
    /// The most stringent set of line-breaking rules. This is the default.
    #[default]
    Strict,
    /// A soft wrap opportunity around every typographic character unit (grapheme cluster).
    ///
    /// This disregards the prohibitions of the Unicode line breaking algorithm (such as around
    /// U+00A0 NO-BREAK SPACE, U+2060 WORD JOINER and punctuation) and of [`WordBreak::KeepAll`].
    /// There is still no opportunity directly before a mandatory break, soft wrapping must be
    /// enabled by [`TextWrapMode`], and a line break override can still suppress opportunities.
    Anywhere,
}

/// Control over "emergency" line-breaking.
///
/// See: <https://www.w3.org/TR/css-text-3/#overflow-wrap-property>
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverflowWrap {
    /// Even with extremely long words, lines can only break at places specified in [`WordBreak`].
    #[default]
    Normal,
    /// Words may be broken at an arbitrary point if needed.
    Anywhere,
    /// Like `Anywhere`, but treated differently for min-content sizing in some engines.
    BreakWord,
}

/// Control over non-"emergency" line-breaking.
///
/// See: <https://www.w3.org/TR/css-text-4/#text-wrap-mode>
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextWrapMode {
    /// Wrap as needed to prevent overflow.
    #[default]
    Wrap,
    /// Do not wrap at soft-wrap opportunities.
    NoWrap,
}

/// How characters are oriented within a vertical line.
///
/// This is the text-layout vocabulary for CSS `text-orientation`. The
/// per-character result for [`TextOrientation::Mixed`] is defined by
/// [UAX #50](https://www.unicode.org/reports/tr50/).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextOrientation {
    /// Follow UAX #50 per character.
    #[default]
    Mixed,
    /// Keep every character upright.
    Upright,
    /// Rotate every character 90 degrees clockwise.
    Sideways,
}

/// How opening punctuation is spaced at the start of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TrimStart {
    /// Set opening punctuation half-width at line start (flush).
    Trim,
    /// Set opening punctuation full-width at line start (spaced).
    #[default]
    Space,
    /// Full-width on the first line, otherwise as [`TrimStart::Trim`].
    SpaceFirst,
}

/// How punctuation is spaced between adjacent pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TrimAdjacent {
    /// Collapse spacing between punctuation glyphs.
    #[default]
    Trim,
    /// Set punctuation full-width when not at a line edge.
    Space,
}

/// How closing punctuation is spaced at the end of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TrimEnd {
    /// Set closing punctuation half-width at line end (flush).
    Trim,
    /// Set closing punctuation full-width at line end (spaced).
    Space,
    /// Half-width only when the full-width glyph does not otherwise fit.
    #[default]
    Allow,
}

/// How fullwidth CJK punctuation is spaced.
///
/// This is the computed value of CSS `text-spacing-trim`: shorthand values
/// (`normal`, `space-all`, `trim-both`) have already been expanded to their
/// per-position keywords by the style system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSpacingTrim {
    /// Let the user agent choose a high-quality spacing policy.
    Auto,
    /// All opening, closing, and middle-dot punctuation is half-width.
    TrimAll,
    /// Per-position keywords, one per controlled position (start, adjacent, end).
    Triplet(TrimStart, TrimAdjacent, TrimEnd),
}

impl Default for TextSpacingTrim {
    fn default() -> Self {
        Self::normal()
    }
}

impl TextSpacingTrim {
    /// The CSS initial value: `space-start trim-adjacent allow-end`.
    pub const fn normal() -> Self {
        Self::new(TrimStart::Space, TrimAdjacent::Trim, TrimEnd::Allow)
    }

    /// `space-all`: every position is spaced.
    pub const fn space_all() -> Self {
        Self::new(TrimStart::Space, TrimAdjacent::Space, TrimEnd::Space)
    }

    /// `trim-both`: trim both line edges and collapse adjacent pairs.
    pub const fn trim_both() -> Self {
        Self::new(TrimStart::Trim, TrimAdjacent::Trim, TrimEnd::Trim)
    }

    /// Build from per-position keywords.
    pub const fn new(start: TrimStart, adjacent: TrimAdjacent, end: TrimEnd) -> Self {
        Self::Triplet(start, adjacent, end)
    }

    /// Whether this value trims all punctuation regardless of context.
    pub fn trims_all(&self) -> bool {
        matches!(self, Self::TrimAll)
    }

    /// Whether opening punctuation is trimmed at line start.
    ///
    /// `first_line` is whether this line is the first line of the block
    /// container or follows a forced break.
    pub fn start_trims(&self, first_line: bool) -> bool {
        match self {
            Self::TrimAll => true,
            Self::Auto => false,
            Self::Triplet(start, _, _) => match start {
                TrimStart::Trim => true,
                TrimStart::Space => false,
                TrimStart::SpaceFirst => !first_line,
            },
        }
    }

    /// Whether adjacent punctuation pairs are collapsed.
    pub fn adjacent_trims(&self) -> bool {
        match self {
            Self::TrimAll => true,
            Self::Auto => false,
            Self::Triplet(_, adjacent, _) => matches!(adjacent, TrimAdjacent::Trim),
        }
    }

    /// Whether closing punctuation is always trimmed at line end.
    pub fn end_trims(&self) -> bool {
        match self {
            Self::TrimAll => true,
            Self::Auto => false,
            Self::Triplet(_, _, end) => matches!(end, TrimEnd::Trim),
        }
    }

    /// Whether closing punctuation may be trimmed at line end when needed to fit.
    pub fn end_allows(&self) -> bool {
        match self {
            Self::TrimAll => true,
            Self::Auto => false,
            Self::Triplet(_, _, end) => matches!(end, TrimEnd::Allow),
        }
    }

    /// Whether closing punctuation is ever trimmed at line end.
    pub fn end_can_trim(&self) -> bool {
        self.end_trims() || self.end_allows()
    }
}
