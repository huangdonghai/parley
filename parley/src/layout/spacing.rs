// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Additional advance applied on top of shaped advance.

use parley_engine::{
    Atom, Grapheme, ShapedSlice,
    shape::{AutoSpaceClass, Character, SpacingTrimClass, Whitespace},
};

use crate::{HangingPunctuation, TextAutospace, TextSpacingTrim};

/// Whether the whitespace is a word separator.
///
/// This mostly follows [CSS Text 3 § 7.1][css-word-separator], in that fixed-width spaces are not
/// considered word separators (and do not get stretched). Unicode's segmentation does consider such
/// fixed-width spaces to be word boundaries.
///
/// Note that the spec includes, e.g., the Ethiopic word space (U+1361) as a word separator (which
/// generally has a glyph), but we currently do not.
///
/// [css-word-separator]: https://www.w3.org/TR/css-text-3/#word-separator
pub(crate) fn is_word_separator(whitespace: Whitespace) -> bool {
    whitespace.is_space_or_nbsp()
}

/// Additional gaps to apply around a shaped advance.
///
/// This spacing is in visual order.
///
/// [CSS Text 4 § 8][css-spacing] says to apply spacing around word separators and "typographic
/// character units." Such a typographic character unit is an extended grapheme cluster, i.e.,
/// [`parley_engine::Grapheme`]. Word separators form their own graphemes. As ligatures should not
/// be broken up and we cannot tell `HarfRust` we want spacing between graphemes, you probably want
/// to apply spacing at the boundaries of an [`parley_engine::Atom`] (and in case of letter spacing,
/// disable optional ligation when shaping).
///
/// [css-spacing]: https://www.w3.org/TR/css-text-4/#spacing
#[derive(Copy, Clone, Debug)]
pub(crate) struct Gaps {
    /// For horizontal text, additional spacing visually to the left.
    ///
    /// For vertical text, this is to the top.
    ///
    /// Note: this field is wired up, but it's currently always zero as we follow Blink in assigning
    /// word and letter spacing to be visually after (see also docs on [`Spacing`]). However, e.g.,
    /// `text-justify: inter-character` would distribute spacing around graphemes, at which point
    /// `before` becomes used.
    pub(crate) before: f32,

    /// For horizontal text, additional spacing visually to the right.
    ///
    /// For vertical text, this is to the bottom.
    pub(crate) after: f32,
}

impl Gaps {
    pub(crate) const ZERO: Self = Self {
        before: 0.,
        after: 0.,
    };

    /// The total gap.
    #[inline(always)]
    pub(crate) const fn total(self) -> f32 {
        self.before + self.after
    }
}

/// Additional spacing to apply.
///
/// Word and letter spacing are applied visually after atoms. Note Blink and Gecko behave
/// differently: Blink applies spacing visually after, but Gecko applies it logically after. There
/// is [some discussion on whether the CSS specs should improve][css-reconsider-spacing].
///
/// [css-reconsider-spacing]: https://github.com/w3c/csswg-drafts/issues/10193#issue-2234554156
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Spacing {
    /// Additional spacing inserted after every word.
    ///
    /// See [CSS Text 4 § 8.1][css-word-spacing].
    ///
    /// [css-word-spacing]: https://www.w3.org/TR/css-text-4/#word-spacing-property
    pub(crate) word: f32,
    /// Additional spacing inserted after every atom.
    ///
    /// See [CSS Text 4 § 8.2][css-letter-spacing].
    ///
    /// [css-letter-spacing]: https://www.w3.org/TR/css-text-4/#letter-spacing-property
    pub(crate) letter: f32,
    /// CSS `text-spacing-trim`.
    pub(crate) text_spacing_trim: TextSpacingTrim,
    /// CSS `text-autospace`.
    pub(crate) text_autospace: TextAutospace,
    /// CSS `hanging-punctuation`.
    pub(crate) hanging_punctuation: HangingPunctuation,
    /// The amount `text-autospace` adds in horizontal and vertical writing modes.
    pub(crate) autospace_advance_horizontal: f32,
    pub(crate) autospace_advance_vertical: f32,
    /// The run's font size, used to tell half-width (already trimmed or
    /// proportional) punctuation glyphs apart from full-width ones: such glyphs
    /// must not be trimmed again (CSS Text 4 8.5.1).
    pub(crate) font_size: f32,
    /// Explicit width features override trim only in their corresponding axis.
    width_features: [bool; 2],
    pub(crate) vertical: bool,
}

impl Spacing {
    #[inline(always)]
    pub(crate) const fn new(
        word: f32,
        letter: f32,
        text_spacing_trim: TextSpacingTrim,
        font_size: f32,
    ) -> Self {
        Self {
            word,
            letter,
            text_spacing_trim,
            text_autospace: TextAutospace::NoAutospace,
            hanging_punctuation: HangingPunctuation::NONE,
            autospace_advance_horizontal: 0.0,
            autospace_advance_vertical: 0.0,
            font_size,
            width_features: [false; 2],
            vertical: false,
        }
    }

    /// Set whether this run uses `text-autospace`, and the width it adds.
    pub(crate) const fn with_autospace(
        mut self,
        text_autospace: TextAutospace,
        horizontal: f32,
        vertical: f32,
    ) -> Self {
        self.text_autospace = text_autospace;
        self.autospace_advance_horizontal = horizontal;
        self.autospace_advance_vertical = vertical;
        self
    }

    /// Set whether this run uses `hanging-punctuation`.
    pub(crate) const fn with_hanging(mut self, hanging_punctuation: HangingPunctuation) -> Self {
        self.hanging_punctuation = hanging_punctuation;
        self
    }

    /// The width `text-autospace` adds at the next boundary.
    #[inline(always)]
    pub(crate) fn autospace_width(self) -> f32 {
        if self.vertical {
            self.autospace_advance_vertical
        } else {
            self.autospace_advance_horizontal
        }
    }

    pub(crate) const fn with_width_features(mut self, horizontal: bool, vertical: bool) -> Self {
        self.width_features = [horizontal, vertical];
        self
    }

    #[inline(always)]
    pub(crate) const fn text_spacing_trim(self) -> TextSpacingTrim {
        if self.width_features[self.vertical as usize] {
            // Suppress both trimming and restoring space to an explicitly
            // half-width glyph. `Auto` currently leaves the shaped width alone.
            TextSpacingTrim::Auto
        } else {
            self.text_spacing_trim
        }
    }

    #[inline(always)]
    pub(crate) const fn is_zero(self) -> bool {
        self.word == 0.
            && self.letter == 0.
            && self.hanging_punctuation.is_none()
            && match self.text_autospace {
                TextAutospace::NoAutospace => true,
                TextAutospace::Normal => {
                    (self.vertical && self.autospace_advance_vertical == 0.0)
                        || (!self.vertical && self.autospace_advance_horizontal == 0.0)
                }
            }
            && matches!(
                self.text_spacing_trim(),
                TextSpacingTrim::Auto
                    | TextSpacingTrim::Triplet(
                        crate::TrimStart::Space,
                        crate::TrimAdjacent::Space,
                        crate::TrimEnd::Space
                    )
            )
    }

    /// The gaps around an atom whose first character has the given whitespace class.
    ///
    /// This is [`EffectiveSpacing::gaps`] under [`Justification::NONE`].
    #[inline(always)]
    pub(crate) fn gaps(self, whitespace: Whitespace) -> Gaps {
        if whitespace == Whitespace::Newline {
            return Gaps::ZERO;
        }

        let mut gaps = Gaps {
            before: 0.,
            after: self.letter,
        };

        if is_word_separator(whitespace) {
            gaps.after += self.word;
        }

        gaps
    }
}

/// Line-position information for `text-spacing-trim`.
///
/// This is `Copy` so it can live in [`EffectiveSpacing`] without allocation.
/// `first_cluster`/`last_cluster` are shaped-cluster indices of the first and
/// last text atoms on the line; `None` means "not at that edge" (or, during
/// line breaking measurement for the last atom, "not yet known").
#[derive(Copy, Clone, Debug)]
pub(crate) struct TrimLineContext {
    /// Shaped-cluster index where the line's first text atom starts.
    pub(crate) first_cluster: Option<u32>,
    /// Shaped-cluster index where the line's last text atom ends (exclusive).
    pub(crate) last_cluster: Option<u32>,
    /// Whether this line is the first line of the block or follows a forced break.
    pub(crate) first_line: bool,
    /// Whether the line's inline base direction is right-to-left: the start
    /// edge is then the visually right (bottom in vertical) one.
    pub(crate) line_is_rtl: bool,
    /// Whether this context positions glyphs for painting, as opposed to
    /// measuring advances while breaking lines.
    pub(crate) hanging_for_paint: bool,
    /// Whether the line's last closing atom was trimmed to fit (`allow-end`).
    pub(crate) end_trimmed: bool,
    /// Whether the end of the line is not yet known (line-breaking measurement).
    pub(crate) end_unknown: bool,
    /// Whether the preceding atom on this line triggers adjacent collapsing.
    pub(crate) prev_trigger: bool,
    /// Whether the following atom on this line triggers adjacent collapsing.
    pub(crate) next_trigger: bool,
    /// The `text-autospace` class of the preceding text on this line.
    pub(crate) prev_autospace: Option<AutoSpaceClass>,
    /// Whether the first atom of a sliced note row must ignore atoms before its range.
    pub(crate) isolate_first_autospace: bool,
    /// Shaped-cluster index of the hanging opening punctuation, if any.
    pub(crate) first_punctuation_hanging_cluster: Option<u32>,
    /// Shaped-cluster index of the hanging punctuation at the line end, if any.
    pub(crate) last_punctuation_hanging_cluster: Option<u32>,
}

impl TrimLineContext {
    /// No line information: mid-line behavior with slice neighbors.
    pub(crate) const NONE: Self = Self {
        first_cluster: None,
        last_cluster: None,
        first_line: false,
        line_is_rtl: false,
        hanging_for_paint: false,
        end_trimmed: false,
        end_unknown: false,
        prev_trigger: false,
        next_trigger: false,
        prev_autospace: None,
        isolate_first_autospace: false,
        first_punctuation_hanging_cluster: None,
        last_punctuation_hanging_cluster: None,
    };
}

/// Justification to apply to a single line.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Justification {
    /// The amount of additional spacing to apply per justification opportunity.
    pub(crate) amount_per_opportunity: f32,

    /// One past the line's logically last shaped cluster eligible for justification. Shaped
    /// clusters at or beyond this index are not justified, as they hang past the line's end, or are
    /// part of the last atom that doesn't fully hang (e.g., an NBSP doesn't hang, but if it closes
    /// the line, it shouldn't get justified).
    pub(crate) justification_end_cluster: u32,
}

impl Justification {
    /// No justification.
    pub(crate) const NONE: Self = Self {
        amount_per_opportunity: 0.,
        justification_end_cluster: u32::MAX,
    };
}

impl Default for Justification {
    fn default() -> Self {
        Self::NONE
    }
}

/// Additional spacing to apply, including justification (if any).
#[derive(Copy, Clone, Debug)]
pub(crate) struct EffectiveSpacing {
    spacing: Spacing,
    justification: Justification,
    trim_line: TrimLineContext,
}

impl EffectiveSpacing {
    /// The combined [`Spacing`] and [`Justification`] effective for a run.
    ///
    /// To measure text for purposes like line breaking, pass [`Justification::NONE`].
    #[inline(always)]
    pub(crate) const fn new(spacing: Spacing, justification: Justification) -> Self {
        Self {
            spacing,
            justification,
            trim_line: TrimLineContext::NONE,
        }
    }

    /// Attach line-position information for `text-spacing-trim`.
    #[inline(always)]
    pub(crate) const fn with_trim_line(mut self, trim_line: TrimLineContext) -> Self {
        self.trim_line = trim_line;
        self
    }

    /// Reinterprets the line edges for a range of pre-shaped note content.
    #[inline(always)]
    pub(crate) const fn with_note_line_range(
        mut self,
        first_cluster: u32,
        last_cluster: u32,
        first_line: bool,
        line_start: bool,
        line_end: bool,
    ) -> Self {
        if line_start {
            self.trim_line.first_cluster = Some(first_cluster);
            self.trim_line.first_line = first_line;
            self.trim_line.prev_trigger = false;
            self.trim_line.prev_autospace = None;
            self.trim_line.isolate_first_autospace = true;
            self.trim_line.first_punctuation_hanging_cluster = None;
        } else {
            self.trim_line.first_cluster = None;
            self.trim_line.first_line = false;
            self.trim_line.isolate_first_autospace = false;
            self.trim_line.first_punctuation_hanging_cluster = None;
        }
        if line_end {
            self.trim_line.last_cluster = Some(last_cluster);
            self.trim_line.end_trimmed = false;
            self.trim_line.next_trigger = false;
            self.trim_line.last_punctuation_hanging_cluster = None;
        } else {
            self.trim_line.last_cluster = None;
            self.trim_line.last_punctuation_hanging_cluster = None;
        }
        self.trim_line.end_unknown = false;
        self
    }

    #[inline(always)]
    pub(crate) const fn with_note_line_hanging(
        mut self,
        first_punctuation: Option<u32>,
        last_punctuation: Option<u32>,
        hanging_for_paint: bool,
    ) -> Self {
        self.trim_line.first_punctuation_hanging_cluster = first_punctuation;
        self.trim_line.last_punctuation_hanging_cluster = last_punctuation;
        self.trim_line.hanging_for_paint = hanging_for_paint;
        self
    }

    #[inline(always)]
    pub(crate) const fn is_zero(self) -> bool {
        self.spacing.is_zero() && self.justification.amount_per_opportunity == 0.
    }

    /// The gaps around `atom`.
    #[inline(always)]
    pub(crate) fn gaps(self, atom: &Atom<'_>) -> Gaps {
        let whitespace = atom.characters()[0].whitespace;
        let mut gaps = self.spacing.gaps(whitespace);
        let range = atom.shaped_clusters_range();

        // Justification expands word separators and, for scripts that do not separate words with
        // spaces, the boundary between two typographic character units (CSS Text 3 6.4.5).
        let is_justification_opportunity =
            is_word_separator(whitespace) || atom.characters()[0].flags.is_cjk_typographic();
        let starts_at_hanging =
            self.trim_line.first_punctuation_hanging_cluster == Some(range.start);
        let ends_at_hanging = self.trim_line.last_punctuation_hanging_cluster == Some(range.end);
        if is_justification_opportunity
            && !starts_at_hanging
            && !ends_at_hanging
            && range.end <= self.justification.justification_end_cluster
        {
            gaps.after += self.justification.amount_per_opportunity;
        }

        self.apply_text_spacing_trim(atom, &mut gaps);

        // A hanging glyph still takes the trimmed glyph shape: `text-spacing-trim`
        // applies at the line edge just as it does elsewhere (CSS Text 4 §8.5).
        // The character advance that becomes a negative margin is therefore the
        // trimmed one, not the punctuation's full-width advance (CSS Text 3 §8.2).
        let hanging_class = atom.characters()[0].flags.hanging_punctuation_class();
        if self.trim_line.first_punctuation_hanging_cluster == Some(range.start)
            && matches!(
                hanging_class,
                parley_engine::shape::HangingPunctuationClass::Opening
                    | parley_engine::shape::HangingPunctuationClass::AmbiguousQuote
            )
        {
            let advance = self.trimmed_advance(atom);
            if self.trim_line.line_is_rtl {
                gaps.after -= advance;
            } else {
                gaps.before -= advance;
            }
        }
        if self.trim_line.last_punctuation_hanging_cluster == Some(range.start)
            && matches!(
                hanging_class,
                parley_engine::shape::HangingPunctuationClass::StopOrComma
                    | parley_engine::shape::HangingPunctuationClass::Closing
                    | parley_engine::shape::HangingPunctuationClass::AmbiguousQuote
            )
        {
            // In RTL text the hanging glyph is the visually first atom on the line,
            // so shifting it by a negative gap would also pull every atom after it
            // along. At paint time it needs no gap: the line offset already places
            // it past the line's end edge, and its own trimmed width carries the
            // atoms after it to the line's edge. Only the measuring path subtracts it.
            if self.trim_line.line_is_rtl && self.trim_line.hanging_for_paint {
                // Keep the trim gaps so the painted run still covers the glyph.
            } else {
                gaps.after -= self.trimmed_advance(atom);
            }
        }

        if self.spacing.text_autospace == TextAutospace::Normal {
            let previous = if self.trim_line.isolate_first_autospace
                && self.trim_line.first_cluster == Some(range.start)
            {
                self.trim_line.prev_autospace
            } else {
                atom.cursor_before()
                    .prev()
                    .map(|previous| {
                        effective_auto_space_class(
                            &previous.characters()[0],
                            self.spacing.vertical,
                        )
                    })
                    .or(self.trim_line.prev_autospace)
            };
            let Some(previous) = previous else {
                return gaps;
            };

            let current = effective_auto_space_class(&atom.characters()[0], self.spacing.vertical);
            if separates_autospace(previous, current) {
                gaps.before += self.spacing.autospace_width();
            }
        }

        gaps
    }

    /// The atom's advance after `text-spacing-trim`, but before justification,
    /// text-autospace, and hanging.
    ///
    /// This is the advance a hanging glyph gives up at the line edge, and what a
    /// hanging glyph's painted run still covers.
    pub(crate) fn trimmed_advance(self, atom: &Atom<'_>) -> f32 {
        let mut gaps = self.spacing.gaps(atom.characters()[0].whitespace);
        self.apply_text_spacing_trim(atom, &mut gaps);
        atom.advance() + gaps.total()
    }

    /// Apply the punctuation half-width adjustments of `text-spacing-trim` to `gaps`.
    fn apply_text_spacing_trim(self, atom: &Atom<'_>, gaps: &mut Gaps) {
        let trim = self.spacing.text_spacing_trim();
        if trim.trims_all() {
            let advance = atom.advance();
            if !is_fullwidth(advance, self.spacing.font_size) {
                return;
            }
            match atom.characters()[0].flags.spacing_trim_class() {
                SpacingTrimClass::Opening => gaps.before -= advance / 2.,
                SpacingTrimClass::Closing => gaps.after -= advance / 2.,
                SpacingTrimClass::MiddleDot => {
                    gaps.before -= advance / 4.;
                    gaps.after -= advance / 4.;
                }
                SpacingTrimClass::Other => {}
            }
        } else if !matches!(trim, TextSpacingTrim::Auto) {
            let advance = atom.advance();
            let class = atom.characters()[0].flags.spacing_trim_class();
            if class != SpacingTrimClass::Other {
                let range = atom.shaped_clusters_range();
                let is_first = self.trim_line.first_cluster == Some(range.start);
                let is_last = self.trim_line.last_cluster == Some(range.end);
                let trimmed = self.trim_decision(class, atom);
                if trimmed && is_fullwidth(advance, self.spacing.font_size) {
                    match class {
                        // The blank half of opening punctuation is the one
                        // before the glyph — left in horizontal text, top in
                        // vertical — so the trim goes on the start side in
                        // both writing modes. Kerning the start pulls the
                        // glyph's ink and everything after it back by half an
                        // em, which is the same geometry as swapping in the
                        // font's half-width form (`halt`/`vhal`) and shifting
                        // the glyph, without needing the font to have one.
                        SpacingTrimClass::Opening => gaps.before -= advance / 2.,
                        SpacingTrimClass::Closing => gaps.after -= advance / 2.,
                        SpacingTrimClass::MiddleDot => {
                            // Middle dots are only trimmed by `trim-all`, never
                            // by position; `trimmed` is false for them here.
                        }
                        SpacingTrimClass::Other => {}
                    }
                } else if !trimmed
                    && !is_first
                    && !is_last
                    && is_halfwidth(advance, self.spacing.font_size)
                {
                    // Mid-line glyph given half-width by the font (e.g. via
                    // `halt`) but set full-width at this position: add the
                    // space back. At line edges the glyph is left as given.
                    let extra = self.spacing.font_size - advance;
                    match class {
                        SpacingTrimClass::Opening => gaps.before += extra,
                        SpacingTrimClass::Closing => gaps.after += extra,
                        SpacingTrimClass::MiddleDot => {
                            gaps.before += extra / 2.;
                            gaps.after += extra / 2.;
                        }
                        SpacingTrimClass::Other => {}
                    }
                }
            }
        }
    }

    /// Whether this atom should be set half-width at its position on the line.
    fn trim_decision(&self, class: SpacingTrimClass, atom: &Atom<'_>) -> bool {
        let trim = self.spacing.text_spacing_trim();
        let range = atom.shaped_clusters_range();
        let is_first = self.trim_line.first_cluster == Some(range.start);
        let is_last = self.trim_line.last_cluster == Some(range.end);
        match class {
            SpacingTrimClass::Opening => {
                if is_first {
                    trim.start_trims(self.trim_line.first_line)
                } else {
                    trim.adjacent_trims()
                        && opening_is_adjacent(
                            atom,
                            self.trim_line.prev_trigger,
                            self.spacing.font_size,
                        )
                }
            }
            SpacingTrimClass::Closing => {
                if is_last {
                    trim.end_trims() || self.trim_line.end_trimmed
                } else {
                    !self.trim_line.end_unknown
                        && trim.adjacent_trims()
                        && (closing_is_adjacent(atom, self.trim_line.next_trigger))
                }
            }
            // Middle dots are only trimmed by `trim-all`.
            SpacingTrimClass::MiddleDot => false,
            SpacingTrimClass::Other => false,
        }
    }

    /// The total advance of `atom`, i.e., the sum of the shaped advance and gaps.
    #[inline(always)]
    pub(crate) fn atom_advance(self, atom: &Atom<'_>) -> f32 {
        atom.advance() + self.gaps(atom).total()
    }

    /// The gaps owned by `grapheme` at its atom boundaries.
    ///
    /// `grapheme_advance` uses the same ownership rule, so summing the returned
    /// values over an atom's graphemes gives exactly [`Self::gaps`].
    pub(crate) fn grapheme_gaps(self, atom: &Atom<'_>, grapheme: Grapheme, is_rtl: bool) -> Gaps {
        let gaps = self.gaps(atom);
        let (owns_before, owns_after) = if is_rtl {
            (grapheme.is_atom_end(), grapheme.is_atom_start())
        } else {
            (grapheme.is_atom_start(), grapheme.is_atom_end())
        };
        Gaps {
            before: if owns_before { gaps.before } else { 0. },
            after: if owns_after { gaps.after } else { 0. },
        }
    }

    /// The total advance of `grapheme`, i.e., the sum of the shaped advance and gaps.
    ///
    /// This only adds the gap that the grapheme owns, i.e., if that grapheme boundary is also an
    /// atom boundary.
    pub(crate) fn grapheme_advance(self, atom: &Atom<'_>, grapheme: Grapheme, is_rtl: bool) -> f32 {
        // TODO: perhaps spacing should not be applied if the grapheme `continues_before` /
        // `continues_after`. The same then should hold true in `gaps` computed for an atom.
        let gaps = self.grapheme_gaps(atom, grapheme, is_rtl);
        grapheme.advance() + gaps.total()
    }

    /// The total advance of `slice`, i.e., the sum of shaped advances and gaps.
    #[inline]
    pub(crate) fn slice_advance(self, slice: ShapedSlice<'_>) -> f32 {
        if self.is_zero() {
            slice
                .shaped_clusters()
                .iter()
                .map(|cluster| cluster.advance)
                .sum()
        } else {
            slice
                .atoms_start()
                .map(|atom| self.atom_advance(&atom))
                .sum()
        }
    }
}

#[inline(always)]
fn atom_trim_class(atom: &Atom<'_>) -> SpacingTrimClass {
    atom.characters()[0].flags.spacing_trim_class()
}

/// A neighbour's `text-autospace` class in vertical writing.
///
/// A non-ideographic character set upright is not a neighbour the property spaces.
#[inline(always)]
pub(crate) fn effective_auto_space_class(character: &Character, vertical: bool) -> AutoSpaceClass {
    let class = character.flags.auto_space_class();
    if class == AutoSpaceClass::NonIdeograph && vertical && character.flags.is_auto_space_upright()
    {
        AutoSpaceClass::Other
    } else {
        class
    }
}

#[inline(always)]
fn separates_autospace(left: AutoSpaceClass, right: AutoSpaceClass) -> bool {
    matches!(
        (left, right),
        (AutoSpaceClass::Ideograph, AutoSpaceClass::NonIdeograph)
            | (AutoSpaceClass::NonIdeograph, AutoSpaceClass::Ideograph)
    )
}

/// Whether `advance` is (approximately) the font's fullwidth advance.
#[inline(always)]
fn is_fullwidth(advance: f32, font_size: f32) -> bool {
    advance > font_size * 0.75
}

/// Whether `advance` is (approximately) the font's halfwidth advance.
///
/// This is how a glyph given half-width by the font (e.g. via `halt`/`vhal`)
/// is told apart from a proportional glyph: only exactly-half glyphs have a
/// full-width setting to restore; proportional glyphs are left alone.
#[inline(always)]
fn is_halfwidth(advance: f32, font_size: f32) -> bool {
    (advance - font_size / 2.).abs() <= font_size * 0.02
}

#[inline(always)]
fn atom_is_ideographic_space(atom: &Atom<'_>) -> bool {
    atom.characters()[0].whitespace == Whitespace::IdeographicSpace
}

#[inline(always)]
fn opening_is_adjacent(atom: &Atom<'_>, cross_run_trigger: bool, font_size: f32) -> bool {
    if let Some(previous) = atom.cursor_before().prev() {
        return matches!(
            atom_trim_class(&previous),
            SpacingTrimClass::Opening | SpacingTrimClass::MiddleDot
        ) || (atom_trim_class(&previous) == SpacingTrimClass::Closing
            && (previous.characters()[0].flags.is_cjk_typographic()
                || is_fullwidth(previous.advance(), font_size)))
            || atom_is_ideographic_space(&previous);
    }
    cross_run_trigger
}

#[inline(always)]
fn closing_is_adjacent(atom: &Atom<'_>, cross_run_trigger: bool) -> bool {
    if let Some(next) = atom.cursor_after().next() {
        return matches!(
            atom_trim_class(&next),
            SpacingTrimClass::Closing | SpacingTrimClass::MiddleDot
        ) || atom_is_ideographic_space(&next);
    }
    cross_run_trigger
}
