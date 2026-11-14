// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Data structures to store the result of shaping.

use core::ops::Range;

use alloc::vec::Vec;
use icu_properties::props::EastAsianWidth;
use icu_properties::{CodePointMapData, props::VerticalOrientation};
use parlance::{BidiLevel, NormalizedCoord, TextOrientation};
use parley_data::Properties;
use skrifa::raw::TableProvider as _;
use skrifa::{
    GlyphId, MetadataProvider,
    instance::{LocationRef, Size},
    metrics::GlyphMetrics,
};

use crate::{
    CharInfo, FontInstance, Glyph,
    itemize::{Segment, TextRange},
};

use super::{
    Character, CharacterFlags, ShapedCluster, ShapedClusterFlags, SpacingTrimClass, Whitespace,
    atom::ShapedSlice,
    data::{auto_space_class, hanging_punctuation_class},
    shaper::ShapeOptions,
};

/// The per-style inputs `text-autospace` needs, resolved by Parley from each
/// style's computed properties.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct AutoSpaceStyle {
    /// Whether this style asks for automatic spacing.
    pub enabled: bool,
    /// The orientation of this style's non-ideographic characters in a column.
    pub orientation: TextOrientation,
    /// Whether this style's characters are a `text-combine-upright` composition.
    pub combined: bool,
    /// Whether this style asks for hanging punctuation.
    pub hanging: bool,
}

/// Metrics that apply to the glyphs of a font.
///
/// These metrics are scaled by the font size.
///
/// The `underline_*` and `strikethrough_*` fields are derived if not set by the font.
///
// TODO: Perhaps it'd be nicer if we exposed unscaled metrics (design units) instead. This currently
// just follows what `parley` used to do in its `RunMetrics`. If we go unscaled, we should then
// either also store units per em, or em-normalize the values like CSS does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    /// Distance from the baseline to the top of the alignment box.
    pub ascent: f32,
    /// Distance from the baseline to the bottom of the alignment box.
    pub descent: f32,
    /// Recommended additional spacing between lines.
    pub leading: f32,
    /// Offset of the top of underline decoration from the baseline.
    pub underline_offset: f32,
    /// Thickness of the underline decoration.
    pub underline_size: f32,
    /// Offset of the top of strikethrough decoration from the baseline.
    pub strikethrough_offset: f32,
    /// Thickness of the strikethrough decoration.
    pub strikethrough_size: f32,
    /// Distance from the baseline to the top of a typical English capital.
    pub cap_height: Option<f32>,
    /// Distance from the baseline to the top of the lowercase "x" or
    /// similar character.
    pub x_height: Option<f32>,
}

impl FontMetrics {
    /// Metrics to use when no font is available at all.
    ///
    /// These roughly match common Latin fonts, so that a layout without fonts still has plausible
    /// line heights.
    pub const fn fallback(font_size: f32) -> Self {
        Self {
            ascent: font_size * 0.8,
            descent: font_size * 0.2,
            leading: 0.,
            underline_offset: 0.,
            underline_size: 0.,
            strikethrough_offset: 0.,
            strikethrough_size: 0.,
            cap_height: None,
            x_height: None,
        }
    }

    /// Compute the metrics of `font` at `font_size`, positioned at the variable font location
    /// given by the `synthesis` and `variations` axis settings.
    ///
    /// Returns `None` if the font data cannot be parsed.
    pub fn from_font_instance(
        font: &FontInstance,
        font_size: f32,
        variations: &[parlance::FontVariation],
    ) -> Option<Self> {
        let font_ref =
            skrifa::FontRef::from_index(font.font.data.as_ref(), font.font.index).ok()?;
        let location = font_ref.axes().location(
            font.synthesis
                .variation_settings()
                .iter()
                .map(|(tag, value)| (skrifa::Tag::new(&tag.to_be_bytes()), *value))
                .chain(
                    variations
                        .iter()
                        .map(|v| (skrifa::Tag::new(&v.tag.to_bytes()), v.value)),
                ),
        );
        Some(Self::from_skrifa_metrics(
            &skrifa::metrics::Metrics::new(&font_ref, Size::new(font_size), &location),
            font_size,
        ))
    }

    fn from_skrifa_metrics(metrics: &skrifa::metrics::Metrics, font_size: f32) -> Self {
        // Default values from HarfBuzz: https://github.com/harfbuzz/harfbuzz/blob/12ee77ee00514c442bcd28f9aa9d56b708c37382/src/hb-ot-metrics.cc#L355-L372
        let fallback_decoration_size = font_size / 18.0;

        let (underline_offset, underline_size) = if let Some(underline) = metrics.underline {
            (underline.offset, underline.thickness)
        } else {
            (-fallback_decoration_size, fallback_decoration_size)
        };
        let (strikethrough_offset, strikethrough_size) = if let Some(strikeout) = metrics.strikeout
        {
            (strikeout.offset, strikeout.thickness)
        } else {
            (metrics.ascent / 2.0, fallback_decoration_size)
        };

        Self {
            ascent: metrics.ascent,
            descent: -metrics.descent,
            leading: metrics.leading,
            underline_offset,
            underline_size,
            strikethrough_offset,
            strikethrough_size,
            x_height: metrics.x_height,
            cap_height: metrics.cap_height,
        }
    }
}

/// The result of shaping.
///
/// After [analyzing][crate::Analysis] your text, [shape the text][crate::Shaper::shape_text],
/// writing the result into this [`ShapedText`]. This then holds your shaped paragraph of text.
///
/// This shaped text holds spans of the source text's characters that were shaped into
/// [`ShapedCluster`]s. Note that the boundaries of shaped clusters and graphemes need not coincide;
/// see [`ShapedCluster`]s documentation for more information.
///
/// Use [`Self::run_slice`] to get a run's [`ShapedSlice`]. From there, you can interact with slices
/// of shaped text, such as walking [atoms](crate::Atom) (e.g., [`ShapedSlice::atoms_start`]) and
/// [graphemes](crate::Grapheme) (e.g., [`ShapedSlice::graphemes_start`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedText {
    runs: Vec<ShapedRun>,
    characters: Vec<Character>,
    shaped_clusters: Vec<ShapedCluster>,
    glyphs: Vec<Glyph>,
    fonts: Vec<FontInstance>,
    normalized_coords: Vec<NormalizedCoord>,
}

impl ShapedText {
    /// Create a reusable [`ShapedText`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserve capacity for at least `additional_chars` more characters.
    ///
    /// This speculatively reserves capacity for `additional_chars` more clusters and glyphs.
    #[inline]
    pub(crate) fn reserve(&mut self, additional_chars: usize) {
        self.characters.reserve(additional_chars);
        self.shaped_clusters.reserve(additional_chars);
        self.glyphs.reserve(additional_chars);
    }

    /// Clear the result while retaining capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.runs.clear();
        self.characters.clear();
        self.shaped_clusters.clear();
        self.glyphs.clear();
        self.fonts.clear();
        self.normalized_coords.clear();
    }

    /// Classify every character of `text` for `text-autospace` and `hanging-punctuation`.
    ///
    /// `styles` is indexed by each character's style id; a character is classified when its
    /// style enables either property, and marked upright when its style combines upright
    /// or when its `text-orientation` says so in a vertical writing mode. Calling this before
    /// line breaking keeps the character classification to one pass and lets the spacing
    /// calculation read it from packed flags instead of looking properties up again. It is
    /// cheap when neither property is enabled: every flag stays at its default.
    pub fn set_text_spacing_classes(&mut self, text: &str, styles: &[AutoSpaceStyle]) {
        if styles.is_empty()
            || !styles
                .iter()
                .any(|style| style.enabled || style.combined || style.hanging)
        {
            return;
        }

        let vertical_orientation = CodePointMapData::<VerticalOrientation>::new();
        let east_asian_width = CodePointMapData::<EastAsianWidth>::new();
        for (ch, character) in text.chars().zip(self.characters.iter_mut()) {
            let Some(style) = styles.get(character.style_index as usize) else {
                continue;
            };

            if style.enabled || style.hanging {
                let properties = Properties::get(ch);
                if style.enabled {
                    character.flags.set_auto_space_class(auto_space_class(
                        ch,
                        properties,
                        east_asian_width.get(ch),
                    ));
                }
                if style.hanging {
                    character
                        .flags
                        .set_hanging_punctuation_class(hanging_punctuation_class(ch, properties));
                }
            }

            if style.combined {
                character.flags.set_auto_space_upright(true);
                continue;
            }

            let upright = match style.orientation {
                TextOrientation::Upright => true,
                TextOrientation::Sideways => false,
                TextOrientation::Mixed => {
                    vertical_orientation.get(ch) != VerticalOrientation::Rotated
                }
            };
            if upright {
                character.flags.set_auto_space_upright(true);
            }
        }
    }

    /// The ideographic advance of the font at `font_index`, in horizontal and vertical layout
    /// units. A font without an ideograph is measured with its em square; likewise, a font
    /// without vertical metrics is measured with its em square for the vertical advance, which
    /// matches the fallback in [`Self::set_vertical_advances`].
    pub fn ideograph_advance(&self, font_index: usize, font_size: f32) -> (f32, f32) {
        let Some(font) = self.fonts.get(font_index) else {
            return (font_size, font_size);
        };
        let Ok(font_ref) = skrifa::FontRef::from_index(font.font.data.as_ref(), font.font.index)
        else {
            return (font_size, font_size);
        };

        let Some(glyph_id) = font_ref.charmap().map('水') else {
            return (font_size, font_size);
        };
        let Some(units_per_em) = font_ref.head().ok().map(|head| head.units_per_em() as f32) else {
            return (font_size, font_size);
        };

        let to_units = |advance: u16| (advance as f32 / units_per_em) * font_size;
        let horizontal = font_ref
            .hmtx()
            .ok()
            .and_then(|hmtx| hmtx.advance(glyph_id))
            .map(to_units)
            .unwrap_or(font_size);
        let vertical = font_ref
            .vmtx()
            .ok()
            .and_then(|vmtx| vmtx.advance(glyph_id))
            .map(to_units)
            .unwrap_or(font_size);

        (horizontal, vertical)
    }

    /// Get a [`ShapedSlice`] of the run at `run_index`.
    pub fn run_slice(&self, run_index: u32) -> ShapedSlice<'_> {
        let run = &self.runs[run_index as usize];
        ShapedSlice {
            characters: &self.characters,

            shaped_clusters: &self.shaped_clusters,
            glyphs: &self.glyphs,

            clusters: (
                run.shaped_clusters_range.start,
                run.shaped_clusters_range.end,
            ),
        }
    }

    /// The shaped runs.
    ///
    /// Each run contains glyphs that can be rendered with a single font.
    #[inline(always)]
    pub fn runs(&self) -> &[ShapedRun] {
        &self.runs
    }

    /// Resolve upright clusters' vertical origin and advance.
    ///
    /// A vertical writing mode advances upright text down the column by the font's vertical
    /// metrics (CSS Writing Modes 3 5.1.1), which are not the horizontal advances shaping
    /// produced: a font may set a CJK glyph's `hmtx` advance to less than one em while its
    /// `vmtx` advance is one em, and upright Latin is an em tall but only half an em wide.
    /// Line breaking, hit testing and selection all read the cluster advances, so they follow
    /// whatever this leaves behind. The same pass records every upright glyph's vertical origin
    /// for painting.
    ///
    /// `text` is the source text; `orientation` decides which characters are upright, and a
    /// cluster is upright when its first character is. Call this before breaking lines. It is
    /// idempotent: the metrics always come from the font, so calling it again gives the same
    /// result.
    pub fn set_vertical_advances(&mut self, text: &str, orientation: TextOrientation) {
        let vertical_orientation = CodePointMapData::<VerticalOrientation>::new();
        let runs: Vec<(usize, f32, Range<u32>, FontMetrics, VerticalAdvanceFeatures)> = self
            .runs
            .iter()
            .map(|run| {
                (
                    run.font_index,
                    run.font_size,
                    run.shaped_clusters_range.clone(),
                    run.font_metrics,
                    run.vertical_features,
                )
            })
            .collect();
        // (font index, advance features, glyph id) -> vertical origin and advance in ems. A page repeats a
        // few hundred glyphs over and over, so read the font tables once per glyph.
        let mut metrics_cache = hashbrown::HashMap::new();

        for (run_index, (font_index, font_size, cluster_range, font_metrics, vertical_features)) in
            runs.iter().enumerate()
        {
            let font = &self.fonts[*font_index].font;
            let Ok(font_ref) = skrifa::raw::FontRef::from_index(font.data.as_ref(), font.index)
            else {
                continue;
            };
            let Some(upem) = font_ref.head().ok().map(|head| head.units_per_em() as f32) else {
                continue;
            };
            let vmtx = font_ref.vmtx().ok();
            let vorg = font_ref.vorg().ok();
            // `vhal`/`vpal` replace the vertical advance, and HarfRust cannot apply
            // them because it never shapes in the vertical direction here.
            let vertical_deltas = if vertical_features.any() {
                vertical_advance_deltas(&font_ref, *vertical_features, upem)
            } else {
                Vec::new()
            };
            let vertical_delta = |glyph_id: u32| -> f32 {
                vertical_deltas
                    .binary_search_by_key(&glyph_id, |(glyph, _)| *glyph)
                    .map(|found| vertical_deltas[found].1)
                    .unwrap_or(0.)
            };
            let glyph_metrics = font_ref.glyph_metrics(Size::unscaled(), LocationRef::default());
            let fallback_origin = font_size * font_metrics.ascent
                / (font_metrics.ascent + font_metrics.descent).max(f32::EPSILON);

            let vertical_metrics = |glyph_id: u32| -> Option<(f32, f32)> {
                let glyph_id = GlyphId::new(glyph_id);
                let advance = (vmtx.as_ref()?.advance(glyph_id)? as f32 / upem
                    + vertical_delta(glyph_id.to_u32()))
                .max(0.);
                let origin = if let Some(vorg) = &vorg {
                    vorg.vertical_origin_y(glyph_id) as f32 / upem
                } else {
                    let bounds = glyph_metrics.bounds(glyph_id)?;
                    let top_side_bearing = vmtx.as_ref()?.side_bearing(glyph_id)?;
                    (bounds.y_max + top_side_bearing as f32) / upem
                };
                Some((origin, advance))
            };

            for cluster_index in cluster_range.clone() {
                let cluster = self.shaped_clusters[cluster_index as usize];
                // A cluster without glyphs (a soft hyphen, say) has nothing to advance.
                if cluster.glyph_len() == 0 {
                    continue;
                }
                let Some(ch) = text
                    .get(self.characters[cluster.chars_range().start as usize].text_byte_range())
                    .and_then(|character| character.chars().next())
                else {
                    continue;
                };
                let is_upright = match orientation {
                    TextOrientation::Sideways => false,
                    TextOrientation::Upright => true,
                    TextOrientation::Mixed => {
                        vertical_orientation.get(ch) != VerticalOrientation::Rotated
                    }
                };
                if !is_upright {
                    continue;
                }

                let mut vertical_for = |glyph_id: u32| {
                    let key = (*font_index, *vertical_features, glyph_id);
                    let metrics = *metrics_cache
                        .entry(key)
                        .or_insert_with(|| vertical_metrics(glyph_id));
                    metrics
                        .map(|(origin, advance)| (origin * font_size, advance * font_size))
                        .unwrap_or((fallback_origin, *font_size))
                };
                let trim_class = self.characters[cluster.chars_range().start as usize]
                    .flags
                    .spacing_trim_class();
                let scale = font_size / upem;

                if cluster.has_inline_glyph() {
                    let glyph_id = cluster.glyph_offset;
                    let (origin, advance) = vertical_for(glyph_id);
                    let trim_blanks = if trim_class != SpacingTrimClass::Other {
                        vertical_trim_blanks(&glyph_metrics, glyph_id, origin, advance, scale)
                    } else {
                        None
                    };
                    let cluster = &mut self.shaped_clusters[cluster_index as usize];
                    cluster.advance = advance;
                    cluster.vertical_origin = origin;
                    cluster.vertical_advance = advance;
                    if let Some((leading, trailing)) = trim_blanks {
                        cluster.trim_leading_blank = leading;
                        cluster.trim_trailing_blank = trailing;
                        cluster.flags = cluster.flags.with_trim_blanks(true);
                    }
                } else {
                    let start = cluster.glyph_offset as usize;
                    let end = start + usize::from(cluster.glyph_len());
                    let first_glyph_id = self.glyphs[start].id;
                    let mut advance = 0.;
                    let mut origin = 0.;
                    for (glyph_index, glyph) in self.glyphs[start..end].iter_mut().enumerate() {
                        let (glyph_origin, glyph_advance) = vertical_for(glyph.id);
                        glyph.vertical_origin = glyph_origin;
                        glyph.vertical_advance = glyph_advance;
                        if glyph_index == 0 {
                            origin = glyph_origin;
                        }
                        advance += glyph_advance;
                    }
                    let trim_blanks = if trim_class != SpacingTrimClass::Other {
                        vertical_trim_blanks(
                            &glyph_metrics,
                            first_glyph_id,
                            origin,
                            advance,
                            scale,
                        )
                    } else {
                        None
                    };
                    let cluster = &mut self.shaped_clusters[cluster_index as usize];
                    cluster.advance = advance;
                    cluster.vertical_origin = origin;
                    cluster.vertical_advance = advance;
                    if let Some((leading, trailing)) = trim_blanks {
                        cluster.trim_leading_blank = leading;
                        cluster.trim_trailing_blank = trailing;
                        cluster.flags = cluster.flags.with_trim_blanks(true);
                    }
                }
            }

            let cluster_slice =
                &self.shaped_clusters[cluster_range.start as usize..cluster_range.end as usize];
            self.runs[run_index].advance =
                cluster_slice.iter().map(|cluster| cluster.advance).sum();
        }
    }

    /// Classify the `text-spacing-trim` class of the ambiguous punctuation
    /// (fullwidth dot and colon punctuation: `U+3001 U+3002 U+FF0C U+FF0E
    /// U+FF1A U+FF1B`) from where the shaped glyph's ink sits in its advance
    /// box, per CSS Text 4 §8.5.2: a glyph whose ink is drawn in the middle of
    /// the box is middle-dot punctuation (a quarter is trimmed from each
    /// side), while ink that hugs the start of the box (the blank half is at
    /// the end) is closing punctuation (the trailing half is trimmed). A font
    /// with centred dot forms therefore gets a quarter trimmed from each side
    /// even without a `lang` to fall back on, and a font with start-hugging
    /// forms the closing treatment; no language guesswork is needed for fonts
    /// whose design already chose.
    ///
    /// `text` is the source text; `vertical` selects the metrics the glyph will
    /// be laid out with (`vmtx`/`VORG` in vertical writing modes, `hmtx`
    /// otherwise). The shaping features (`vert`) have already been applied, so
    /// the glyph measured is the one that will be drawn. Clusters whose glyph
    /// has no outline bounds keep whatever class the character properties (or
    /// a locale-based override) assigned.
    ///
    /// Call this once, after shaping and before breaking lines.
    pub fn resolve_trim_classes(&mut self, text: &str, vertical: bool) {
        if !text.chars().any(is_ambiguous_trim_punctuation) {
            return;
        }
        let runs: Vec<(usize, Range<u32>)> = self
            .runs
            .iter()
            .map(|run| (run.font_index, run.shaped_clusters_range.clone()))
            .collect();
        // (font index, glyph id) -> centre of the ink measured from the start
        // of the advance box, as a fraction of that advance.
        let mut centre_cache: Vec<((usize, u32), Option<f32>)> = Vec::new();

        for (font_index, cluster_range) in runs.iter() {
            let font = &self.fonts[*font_index].font;
            let Ok(font_ref) = skrifa::raw::FontRef::from_index(font.data.as_ref(), font.index)
            else {
                continue;
            };
            let Some(upem) = font_ref.head().ok().map(|head| head.units_per_em() as f32) else {
                continue;
            };
            if upem <= 0.0 {
                continue;
            }
            let vmtx = font_ref.vmtx().ok();
            let vorg = font_ref.vorg().ok();
            let glyph_metrics = font_ref.glyph_metrics(Size::unscaled(), LocationRef::default());

            // Centre of the glyph's ink measured from the start of its advance
            // box, as a fraction of that advance, along the axis it will be
            // laid out on.
            let ink_centre = |glyph_id: u32| -> Option<f32> {
                let glyph_id = GlyphId::new(glyph_id);
                let bounds = glyph_metrics.bounds(glyph_id)?;
                if vertical {
                    let vmtx = vmtx.as_ref()?;
                    let advance = vmtx.advance(glyph_id)? as f32;
                    if advance <= 0.0 {
                        return None;
                    }
                    // Distance from the top of the advance box to the glyph
                    // origin, in font units — the same `VORG`/top-side-bearing
                    // rule the vertical origin uses in `set_vertical_advances`.
                    let origin = if let Some(vorg) = &vorg {
                        vorg.vertical_origin_y(glyph_id) as f32
                    } else {
                        let top_side_bearing = vmtx.side_bearing(glyph_id)? as f32;
                        bounds.y_max + top_side_bearing
                    };
                    let centre = origin - (bounds.y_min + bounds.y_max) / 2.0;
                    Some(centre / advance)
                } else {
                    let advance = font_ref.hmtx().ok()?.advance(glyph_id)? as f32;
                    if advance <= 0.0 {
                        return None;
                    }
                    Some(((bounds.x_min + bounds.x_max) / 2.0) / advance)
                }
            };

            for cluster_index in cluster_range.clone() {
                let cluster = self.shaped_clusters[cluster_index as usize];
                if cluster.glyph_len() == 0 {
                    continue;
                }
                // The ambiguous punctuation inside this cluster (usually its
                // first character); the class belongs to the source character.
                let chars_range = cluster.chars_range();
                let ambiguous: Vec<u32> = (chars_range.start..chars_range.end)
                    .filter(|char_index| {
                        text.get(self.characters[*char_index as usize].text_byte_range())
                            .and_then(|s| s.chars().next())
                            .is_some_and(is_ambiguous_trim_punctuation)
                    })
                    .collect();
                if ambiguous.is_empty() {
                    continue;
                }
                let glyph_id = if cluster.has_inline_glyph() {
                    cluster.glyph_offset
                } else {
                    self.glyphs[cluster.glyph_offset as usize].id
                };
                let key = (*font_index, glyph_id);
                let centre = if let Some((_, centre)) =
                    centre_cache.iter().find(|(cached, _)| *cached == key)
                {
                    *centre
                } else {
                    let centre = ink_centre(glyph_id);
                    centre_cache.push((key, centre));
                    centre
                };
                let Some(centre) = centre else {
                    continue;
                };
                let class = if (0.36..=0.64).contains(&centre) {
                    SpacingTrimClass::MiddleDot
                } else {
                    SpacingTrimClass::Closing
                };
                for char_index in ambiguous {
                    self.characters[char_index as usize]
                        .flags
                        .set_spacing_trim_class(class);
                }
            }
        }
    }

    /// Zero the advance of every cluster that starts inside one of `ranges`.
    ///
    /// A ruby annotation is laid out with the base text but must not push it along: clearing
    /// the annotation's advances before line breaking leaves the base text where it would be
    /// without the annotation, which is what a page comparing against one without ruby sees.
    pub fn clear_advances(&mut self, text: &str, ranges: &[Range<usize>]) {
        if ranges.is_empty() {
            return;
        }
        let run_ranges: Vec<Range<u32>> = self
            .runs
            .iter()
            .map(|run| run.shaped_clusters_range.clone())
            .collect();

        for (run_index, cluster_range) in run_ranges.iter().enumerate() {
            for cluster_index in cluster_range.clone() {
                let cluster = self.shaped_clusters[cluster_index as usize];
                let character = &self.characters[cluster.chars_range().start as usize];
                let byte_range = character.text_byte_range();
                if text.get(byte_range.clone()).is_none() {
                    continue;
                }
                if ranges.iter().any(|range| range.contains(&byte_range.start)) {
                    self.shaped_clusters[cluster_index as usize].advance = 0.0;
                }
            }
            let cluster_slice =
                &self.shaped_clusters[cluster_range.start as usize..cluster_range.end as usize];
            self.runs[run_index].advance =
                cluster_slice.iter().map(|cluster| cluster.advance).sum();
        }
    }

    /// Escape hatch for `parley` tests that remap style indices to check whether layouts are
    /// otherwise identical. This is not for public consumption!
    #[doc(hidden)]
    #[inline(always)]
    pub fn remap_styles(&mut self, remap: &[u16]) {
        for character in self.characters.iter_mut() {
            character.style_index = remap[character.style_index as usize];
        }
        for shaped_cluster in self.shaped_clusters.iter_mut() {
            shaped_cluster.style_index = remap[shaped_cluster.style_index as usize];
        }
    }

    /// The character data of the text, in logical order.
    ///
    /// [`ShapedRun::characters_range`] indexes into this. The nth [`Character`] corresponds exactly
    /// to the nth character of the source text (in the Unicode scalar value sense). This contains
    /// every character of the source text unless shaping was aborted due to
    /// [`FontSelector::select_font`][crate::FontSelector::select_font] failing to return a font.
    #[inline(always)]
    pub fn characters(&self) -> &[Character] {
        &self.characters
    }

    /// The character data of the text, in logical order (mutable view).
    #[inline(always)]
    pub fn characters_mut(&mut self) -> &mut [Character] {
        &mut self.characters
    }

    /// The shaped clusters.
    ///
    /// [`ShapedRun::shaped_clusters_range`] indexes into this.
    #[inline(always)]
    pub fn shaped_clusters(&self) -> &[ShapedCluster] {
        &self.shaped_clusters
    }

    /// The shaped glyphs.
    ///
    /// [`ShapedRun::glyphs_range`] indexes into this.
    #[inline(always)]
    pub fn glyphs(&self) -> &[Glyph] {
        &self.glyphs
    }

    /// The fonts.
    ///
    /// [`ShapedRun::font_index`] indexes into this.
    #[inline(always)]
    pub fn fonts(&self) -> &[FontInstance] {
        &self.fonts
    }

    /// The normalized font coordinates used by runs in this shaped text.
    ///
    /// [`ShapedRun::normalized_coords_range`] splits this into per-run slices.
    #[inline(always)]
    pub fn normalized_coords(&self) -> &[NormalizedCoord] {
        &self.normalized_coords
    }

    #[expect(clippy::cast_possible_truncation, reason = "Deferred")]
    pub(crate) fn push_run(
        &mut self,
        text: &str,
        range: TextRange,
        segment: &Segment,
        options: &ShapeOptions<'_>,
        char_info: &[CharInfo],
        char_style_indices: &[u16],
        font: &FontInstance,
        glyph_buffer: &harfrust::GlyphBuffer,
        normalized_coords: &[harfrust::NormalizedCoord],
    ) {
        debug_assert_eq!(
            range.char_range.len(),
            text[range.byte_range.clone()].chars().count(),
            "The claimed character range and the text's covered byte range should agree"
        );

        let glyph_infos = glyph_buffer.glyph_infos();

        let normalized_coords_range = {
            let start = self.normalized_coords.len();
            self.normalized_coords.extend(
                normalized_coords
                    .iter()
                    .map(|c| NormalizedCoord::from_bits(c.to_bits())),
            );
            start..self.normalized_coords.len()
        };

        let font_index = self
            .fonts
            .iter()
            .position(|f| f == font)
            .unwrap_or_else(|| {
                let index = self.fonts.len();
                self.fonts.push(font.clone());
                index
            });

        let font = &self.fonts[font_index];
        let font_ref =
            skrifa::FontRef::from_index(font.font.data.as_ref(), font.font.index).unwrap();
        let metrics = skrifa::metrics::Metrics::new(
            &font_ref,
            Size::new(options.font_size),
            normalized_coords,
        );
        let units_per_em = metrics.units_per_em as f32;
        let font_metrics = FontMetrics::from_skrifa_metrics(&metrics, options.font_size);

        // `HarfRust` returns glyphs in visual order, so we need to process them as such while
        // maintaining logical ordering of clusters.

        let glyph_positions = glyph_buffer.glyph_positions();
        let scale_factor = options.font_size / units_per_em;
        let shaped_clusters_start = self.shaped_clusters.len();

        // Push all characters.
        let characters_start = self.characters.len();
        for (((byte_offset, ch), info), style_index) in text[range.byte_range.clone()]
            .char_indices()
            .zip(&char_info[range.char_range.clone()])
            .zip(&char_style_indices[range.char_range.clone()])
        {
            self.characters.push(Character {
                text_byte_start: (range.byte_range.start + byte_offset) as u32,
                style_index: *style_index,
                whitespace: Whitespace::from_char(ch),
                flags: CharacterFlags::new(
                    ch,
                    info.is_word_boundary(),
                    info.is_soft_wrap_opportunity(),
                ),
                grapheme_start: info.is_grapheme_start(),
            });
        }
        // TODO: we force a grapheme break at the start of the run, as itemization could have split
        // a grapheme into two. Ideally, grapheme segmentation should be performed after
        // itemization, at which case this will always be true.
        self.characters[characters_start].grapheme_start = true;

        let glyphs_start = self.glyphs.len();
        if segment.bidi_level.is_ltr() {
            process_shaped_clusters(
                &mut self.shaped_clusters,
                &mut self.glyphs,
                scale_factor,
                glyph_infos.iter(),
                glyph_positions.iter(),
                &self.characters,
                characters_start,
            );
        } else {
            process_shaped_clusters(
                &mut self.shaped_clusters,
                &mut self.glyphs,
                scale_factor,
                glyph_infos.iter().rev(),
                glyph_positions.iter().rev(),
                &self.characters,
                characters_start,
            );
            // Reverse each cluster's glyphs, such that they are in paint order.
            //
            // TODO: could this become a single `reverse` over the range?
            for cluster in &self.shaped_clusters[shaped_clusters_start..] {
                if !cluster.has_inline_glyph() && cluster.glyph_len() > 1 {
                    let start = cluster.glyph_offset as usize;
                    self.glyphs[start..start + cluster.glyph_len() as usize].reverse();
                }
            }
        }

        let glyph_metrics = font_ref.glyph_metrics(Size::unscaled(), LocationRef::default());
        let (shaped_clusters, glyphs, characters) = (
            &mut self.shaped_clusters,
            &self.glyphs,
            &self.characters,
        );
        for cluster in &mut shaped_clusters[shaped_clusters_start..] {
            if characters[cluster.chars_range().start as usize]
                .flags
                .spacing_trim_class()
                == SpacingTrimClass::Other
            {
                continue;
            }
            let Some((glyph_id, glyph_x)) = cluster_first_glyph(cluster, glyphs) else {
                continue;
            };
            let Some((leading, trailing)) = horizontal_trim_blanks(
                &glyph_metrics,
                glyph_id,
                glyph_x,
                cluster.advance,
                scale_factor,
            ) else {
                continue;
            };
            cluster.trim_leading_blank = leading;
            cluster.trim_trailing_blank = trailing;
            cluster.flags = cluster.flags.with_trim_blanks(true);
        }

        let shaped_clusters_range = shaped_clusters_start as u32..self.shaped_clusters.len() as u32;
        let run_advance = self.shaped_clusters[shaped_clusters_start..]
            .iter()
            .map(|cluster| cluster.advance)
            .sum();

        self.runs.push(ShapedRun {
            range,
            font_size: options.font_size,
            font_index,
            characters_range: characters_start as u32..self.characters.len() as u32,
            shaped_clusters_range,
            glyphs_range: glyphs_start..self.glyphs.len(),
            normalized_coords_range,
            bidi_level: segment.bidi_level,
            advance: run_advance,
            font_metrics,
            vertical_features: VerticalAdvanceFeatures::from_features(options.features),
        });
    }
}

/// One shaped run, belonging to a [`ShapedText`].
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedRun {
    /// The range of text this run corresponds to.
    pub range: TextRange,
    /// Font size.
    pub font_size: f32,
    /// This run's font, as an index into [`ShapedText::fonts`].
    //
    // Note: We carry the font index instead of having `ShapedRun` carry `Arc<FontInstance>`,
    // meaning `ShapedRun` could become `Copy` once we have access to `core::range::Range`.
    pub font_index: usize,
    /// This run's characters, as a range into [`ShapedText::characters`].
    pub characters_range: Range<u32>,
    /// This run's shaped clusters, as a range into [`ShapedText::shaped_clusters`].
    pub shaped_clusters_range: Range<u32>,
    /// This run's glyphs, as a range into [`ShapedText::glyphs`].
    pub glyphs_range: Range<usize>,
    /// The normalized variation coords of this run, as a range into [`ShapedText::normalized_coords`].
    pub normalized_coords_range: Range<usize>,
    /// The bidi level of the run.
    pub bidi_level: BidiLevel,
    /// Total advance of the run.
    pub advance: f32,
    /// The font metrics of this run.
    pub font_metrics: FontMetrics,
    /// GPOS features this run asked for that adjust a glyph's *vertical*
    /// advance.
    ///
    /// Shaping always runs in the horizontal direction, so HarfRust never
    /// applies these: their value records carry a `YAdvance`, which only a
    /// vertical shaping pass reads. A vertical layout therefore has to fold
    /// them into the advance it takes from `vmtx` by itself.
    pub vertical_features: VerticalAdvanceFeatures,
}

/// The GPOS features that replace a glyph's vertical advance with a different
/// one, and so have to be applied on top of `vmtx` when the layout is vertical.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VerticalAdvanceFeatures {
    /// `vhal`: alternate vertical half metrics, which is how a font offers the
    /// half-width form `text-spacing-trim` asks for.
    pub vhal: bool,
    /// `vpal`: proportional alternate vertical metrics.
    pub vpal: bool,
}

impl VerticalAdvanceFeatures {
    /// Which of the two a style's font features turn on.
    ///
    /// `font-feature-settings` lists features in order and the last value for a
    /// tag wins, which the resolver has already applied by the time the list
    /// reaches shaping.
    pub fn from_features(features: &[parlance::FontFeature]) -> Self {
        let mut on = Self::default();
        for feature in features {
            match &feature.tag.to_bytes() {
                b"vhal" => on.vhal = feature.value != 0,
                b"vpal" => on.vpal = feature.value != 0,
                _ => {}
            }
        }
        on
    }

    /// Whether either feature is on, i.e. whether the font has to be consulted.
    #[inline(always)]
    pub fn any(self) -> bool {
        self.vhal || self.vpal
    }
}

/// The first glyph of a cluster and its horizontal paint offset.
fn cluster_first_glyph(cluster: &ShapedCluster, glyphs: &[Glyph]) -> Option<(u32, f32)> {
    if cluster.glyph_len() == 0 {
        None
    } else if cluster.has_inline_glyph() {
        Some((cluster.glyph_offset, 0.))
    } else {
        let glyph = glyphs.get(cluster.glyph_offset as usize)?;
        Some((glyph.id, glyph.x))
    }
}

/// Blank space before and after a glyph's ink on the horizontal inline axis.
fn horizontal_trim_blanks(
    glyph_metrics: &GlyphMetrics<'_>,
    glyph_id: u32,
    glyph_x: f32,
    advance: f32,
    scale: f32,
) -> Option<(f32, f32)> {
    let bounds = glyph_metrics.bounds(GlyphId::new(glyph_id))?;
    let leading = (glyph_x + bounds.x_min * scale).clamp(0., advance);
    let trailing = (advance - glyph_x - bounds.x_max * scale).clamp(0., advance);
    Some((leading, trailing))
}

/// Blank space before and after a glyph's ink on the vertical inline axis.
///
/// The vertical origin is the pen position, measured downwards. Font bounds
/// use a Y-up coordinate system, so the ink's top is `origin - y_max`.
fn vertical_trim_blanks(
    glyph_metrics: &GlyphMetrics<'_>,
    glyph_id: u32,
    origin: f32,
    advance: f32,
    scale: f32,
) -> Option<(f32, f32)> {
    let bounds = glyph_metrics.bounds(GlyphId::new(glyph_id))?;
    let leading = (origin - bounds.y_max * scale).clamp(0., advance);
    let trailing = (advance - origin + bounds.y_min * scale).clamp(0., advance);
    Some((leading, trailing))
}

/// Processes shaped glyphs from `HarfRust` and converts them into `ShapedCluster` and `Glyph`.
///
/// # Parameters
///
/// ## Output Parameters (mutated by this function):
/// * `shaped_clusters` - Vector where new `ShapedCluster` entries will be pushed.
/// * `glyphs` - Vector where new `Glyph` entries will be pushed. Note: single-glyph clusters
///   with zero offsets are inlined directly into `ShapedCluster`.
///
/// ## Input Parameters:
/// * `scale_factor` - Scaling factor used to convert font units to the target size.
/// * `glyph_infos` - `HarfRust` glyph information in logical order (i.e., reversed for RTL runs).
/// * `glyph_positions` - `HarfRust` glyph positioning data in logical order (i.e., reversed for RTL
///   runs).
/// * `characters` must contain the shaped characters whose clusters we're now processing, starting at
///   index `characters_start`.
/// * `characters_start` - See `characters`.
fn process_shaped_clusters<'a>(
    shaped_clusters: &mut Vec<ShapedCluster>,
    glyphs: &mut Vec<Glyph>,
    scale_factor: f32,
    glyph_infos: impl Iterator<Item = &'a harfrust::GlyphInfo>,
    glyph_positions: impl Iterator<Item = &'a harfrust::GlyphPosition>,
    characters: &[Character],
    characters_start: usize,
) {
    struct Cluster {
        id: u32,
        characters_start: usize,
        glyphs_start: usize,
        glyphs: u32,
        inline_glyph: Option<Glyph>,
        advance: f32,
    }

    /// Flush `cluster`, whose characters end at `char_end`, onto `shaped_clusters`.
    #[expect(clippy::cast_possible_truncation, reason = "Deferred")]
    fn flush(
        cluster: &mut Cluster,
        char_end: usize,
        characters: &[Character],
        shaped_clusters: &mut Vec<ShapedCluster>,
    ) {
        let first_character = &characters[cluster.characters_start];
        let is_newline = first_character.whitespace == Whitespace::Newline;
        let horizontal_advance = cluster
            .inline_glyph
            .map(|inline_glyph| inline_glyph.advance)
            .unwrap_or(0.);
        let (glyph_offset, glyph_len, inline_glyph, advance) = if is_newline {
            // Elide glyphs of newlines.
            (cluster.glyphs_start as u32, 0, false, 0.)
        } else if let Some(inline_glyph) = cluster.inline_glyph.take() {
            (inline_glyph.id, 1, true, cluster.advance)
        } else {
            (
                cluster.glyphs_start as u32,
                cluster.glyphs as u8,
                false,
                cluster.advance,
            )
        };

        shaped_clusters.push(ShapedCluster {
            chars_range: (cluster.characters_start as u32, char_end as u32),
            style_index: first_character.style_index,
            flags: ShapedClusterFlags::new(glyph_len)
                .with_grapheme_start(first_character.grapheme_start)
                // TODO: fill with actual shaping data (`parley` currently just ignores this)
                .with_safe_to_break_before(false)
                .with_inline_glyph(inline_glyph)
                .with_first_char(
                    first_character.flags.is_soft_wrap_opportunity(),
                    first_character.whitespace,
                ),
            glyph_offset,
            advance,
            vertical_origin: 0.,
            vertical_advance: 0.,
            horizontal_advance,
            trim_leading_blank: 0.,
            trim_trailing_blank: 0.,
        });
    }

    let mut cluster = Cluster {
        id: 0,
        characters_start,
        glyphs_start: glyphs.len(),
        glyphs: 0,
        inline_glyph: None,
        advance: 0.,
    };

    for (glyph_info, glyph_pos) in glyph_infos.zip(glyph_positions) {
        if glyph_info.cluster != cluster.id {
            let char_end = characters_start + glyph_info.cluster as usize;
            flush(&mut cluster, char_end, characters, shaped_clusters);

            cluster = Cluster {
                id: glyph_info.cluster,
                characters_start: char_end,
                glyphs_start: glyphs.len(),
                glyphs: 0,
                inline_glyph: None,
                advance: 0.,
            };
        }

        let glyph = Glyph {
            id: glyph_info.glyph_id,
            x: (glyph_pos.x_offset as f32) * scale_factor,
            // Convert from font space (Y-up) to layout space (Y-down)
            y: -(glyph_pos.y_offset as f32) * scale_factor,
            advance: (glyph_pos.x_advance as f32) * scale_factor,
            vertical_origin: 0.,
            vertical_advance: 0.,
        };
        if cluster.glyphs == 0 && glyph.x == 0. && glyph.y == 0. {
            // Defer this potential zero-offset, single glyph cluster
            cluster.inline_glyph = Some(glyph);
        } else {
            if let Some(pending_glyph) = cluster.inline_glyph.take() {
                glyphs.push(pending_glyph);
            }
            glyphs.push(glyph);
        }
        cluster.advance += glyph.advance;
        cluster.glyphs += 1;
    }

    // Flush the final cluster.
    flush(&mut cluster, characters.len(), characters, shaped_clusters);
}

/// Whether `ch` is fullwidth dot or colon punctuation, whose `text-spacing-trim`
/// class is decided by where its ink is drawn rather than by the character.
fn is_ambiguous_trim_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '\u{3001}' | '\u{3002}' | '\u{FF0C}' | '\u{FF0E}' | '\u{FF1A}' | '\u{FF1B}'
    )
}

/// Read the `vhal`/`vpal` value records that apply to each glyph, in ems.
///
/// The result is sorted by glyph id so a caller can binary search it.
fn vertical_advance_deltas(
    font_ref: &skrifa::raw::FontRef<'_>,
    features: VerticalAdvanceFeatures,
    upem: f32,
) -> Vec<(u32, f32)> {
    use skrifa::raw::tables::gpos::{PositionSubtables, SinglePos};

    let Ok(gpos) = font_ref.gpos() else {
        return Vec::new();
    };
    let Ok(lookups) = gpos.lookup_list() else {
        return Vec::new();
    };
    let Ok(feature_list) = gpos.feature_list() else {
        return Vec::new();
    };
    let mut deltas: Vec<(u32, f32)> = Vec::new();
    for record in feature_list.feature_records() {
        let tag = record.feature_tag();
        let wanted = (features.vhal && tag == skrifa::Tag::new(b"vhal"))
            || (features.vpal && tag == skrifa::Tag::new(b"vpal"));
        if !wanted {
            continue;
        }
        // A `FeatureRecord`'s offset is measured from the FeatureList table, not
        // from the GPOS header, so the wrong base silently resolves the record
        // to a different feature (and thus the wrong lookups).
        let Ok(feature) = record.feature(feature_list.offset_data()) else {
            continue;
        };
        for index in feature.lookup_list_indices() {
            let Ok(lookup) = lookups.lookups().get(usize::from(index.get())) else {
                continue;
            };
            let Ok(PositionSubtables::Single(subtables)) = lookup.subtables() else {
                continue;
            };
            for subtable in 0..subtables.len() {
                let Ok(subtable) = subtables.get(subtable) else {
                    continue;
                };
                match subtable {
                    SinglePos::Format1(single) => {
                        let Ok(coverage) = single.coverage() else {
                            continue;
                        };
                        let Some(delta) = single.value_record().y_advance() else {
                            continue;
                        };
                        deltas.extend(
                            coverage
                                .iter()
                                .map(|glyph| (glyph.to_u32(), f32::from(delta) / upem)),
                        );
                    }
                    SinglePos::Format2(single) => {
                        let Ok(coverage) = single.coverage() else {
                            continue;
                        };
                        for (glyph, value) in coverage.iter().zip(single.value_records().iter()) {
                            let Ok(value) = value else {
                                continue;
                            };
                            if let Some(delta) = value.y_advance() {
                                deltas.push((glyph.to_u32(), f32::from(delta) / upem));
                            }
                        }
                    }
                }
            }
        }
    }
    deltas.sort_unstable_by_key(|(glyph, _)| *glyph);
    deltas
}

#[cfg(test)]
mod tests {
    use alloc::{sync::Arc, vec, vec::Vec};

    use fontique::Synthesis;
    use linebender_resource_handle::{Blob, FontData};

    use crate::{
        Analysis, AnalysisOptions, Analyzer, FontInstance, FontInstanceRef, FontSelector,
        ShapeOptions, Shaper,
        itemize::{Item, Segment},
        shape::CharCluster,
    };

    use super::{ShapedText, VerticalAdvanceFeatures, vertical_advance_deltas};

    /// `vhal` is the only place a font can offer the half-width vertical
    /// punctuation `text-spacing-trim` asks for, and HarfRust never applies it
    /// because shaping here never runs in the vertical direction. This checks
    /// the lookups read by hand against the subset font the text-spacing-trim
    /// references use.
    #[test]
    fn vertical_advance_deltas_cover_the_vertical_forms() {
        let path = r"D:\_git0\conformance\fonts\noto\cjk\NotoSansCJKjp-Regular-subset-halt.otf";
        if !std::path::Path::new(path).exists() {
            std::eprintln!("skipping: the halt-font subset is not present");
            return;
        }
        let bytes = std::fs::read(path).expect("read the halt font");
        let font = skrifa::raw::FontRef::from_index(&bytes, 0).expect("parse the halt font");
        let upem = skrifa::raw::TableProvider::head(&font)
            .expect("head")
            .units_per_em() as f32;
        let deltas = vertical_advance_deltas(
            &font,
            VerticalAdvanceFeatures {
                vhal: true,
                vpal: false,
            },
            upem,
        );
        // `vhal` in this subset is three single-positioning subtables covering
        // 19, 7 and 15 vertical forms, each halving the advance. Resolving the
        // feature to the wrong offsets used to produce a different set of
        // glyphs entirely, so the count is worth pinning.
        assert_eq!(
            deltas.len(),
            41,
            "`vhal` covers 19 + 7 + 15 glyphs in this subset, got {}",
            deltas.len()
        );
        assert!(
            deltas.iter().all(|(_, delta)| *delta == -0.5),
            "every `vhal` record halves the vertical advance"
        );
    }

    const ROBOTO: &[u8] =
        include_bytes!("../../../parley_dev/assets/fonts/roboto_fonts/Roboto-Regular.ttf");

    #[test]
    fn vertical_metrics_cache_distinguishes_width_features() {
        let path = r"D:\_git0\conformance\fonts\noto\cjk\NotoSansCJKjp-Regular-subset-halt.otf";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let font = FontInstance {
            font: FontData::new(Blob::new(Arc::new(bytes)), 0),
            synthesis: Synthesis::default(),
        };
        let vert = parlance::FontFeature {
            tag: parlance::Tag::new(b"vert"),
            value: 1,
        };
        let vhal = parlance::FontFeature {
            tag: parlance::Tag::new(b"vhal"),
            value: 1,
        };
        let plain = [vert];
        let half = [vert, vhal];
        let text = "（（";
        let mut shaped = ShapedText::new();
        Shaper::default().shape_text(
            text,
            &analyze(text),
            &[0, 1],
            [
                Item {
                    char_end: 1,
                    options: ShapeOptions {
                        font_size: 20.,
                        features: &plain,
                        language: None,
                        variations: &[],
                    },
                },
                Item {
                    char_end: 2,
                    options: ShapeOptions {
                        font_size: 20.,
                        features: &half,
                        language: None,
                        variations: &[],
                    },
                },
            ],
            SingleFont(font),
            &mut shaped,
        );
        for _ in 0..2 {
            shaped.set_vertical_advances(text, parlance::TextOrientation::Upright);
            let advances: Vec<_> = shaped
                .shaped_clusters()
                .iter()
                .map(|cluster| cluster.advance)
                .collect();
            assert_eq!(advances, [20., 10.]);
        }
    }
    const NOTO_KUFI_ARABIC: &[u8] =
        include_bytes!("../../../parley_dev/assets/fonts/noto_fonts/NotoKufiArabic-Regular.otf");
    const NOTO_COLOR_EMOJI: &[u8] = include_bytes!(
        "../../../parley_dev/assets/fonts/noto_color_emoji/NotoColorEmoji-Subset.ttf"
    );

    /// A [`FontSelector`] shaping everything with a single font.
    struct SingleFont(FontInstance);

    impl FontSelector for SingleFont {
        fn select_font(
            &mut self,
            _item: &Segment,
            _options: &ShapeOptions<'_>,
            _cluster: &mut CharCluster,
        ) -> Option<FontInstanceRef<'_>> {
            Some(self.0.as_ref())
        }
    }

    /// A [`FontSelector`] shaping clusters starting with a particular character with one font, and
    /// all other clusters with another, to split the text into runs at that character.
    struct FontForChar {
        ch: char,
        font: FontInstance,
        other_font: FontInstance,
    }

    impl FontSelector for FontForChar {
        fn select_font(
            &mut self,
            _segment: &Segment,
            _options: &ShapeOptions<'_>,
            cluster: &mut CharCluster,
        ) -> Option<FontInstanceRef<'_>> {
            if cluster.chars()[0].ch == self.ch {
                Some(self.font.as_ref())
            } else {
                Some(self.other_font.as_ref())
            }
        }
    }

    fn analyze(text: &str) -> Analysis {
        let mut analysis = Analysis::new();
        Analyzer::new().analyze(
            text,
            &AnalysisOptions {
                line_break: &[],
                line_break_override: None,
                ..AnalysisOptions::default()
            },
            &mut analysis,
        );
        analysis
    }

    fn font_instance(font_data: &'static [u8]) -> FontInstance {
        FontInstance {
            font: FontData::new(Blob::new(Arc::new(font_data)), 0),
            synthesis: Synthesis::default(),
        }
    }

    fn shape_with_font(text: &str, font_data: &'static [u8]) -> ShapedText {
        shape_with_font_selector(text, SingleFont(font_instance(font_data)))
    }

    fn shape_with_font_selector(text: &str, select_font: impl FontSelector) -> ShapedText {
        let analysis = analyze(text);
        let mut shaper = Shaper::default();
        let mut shaped = ShapedText::new();

        let char_style_indices = vec![0; text.chars().count()];
        let items = [Item {
            char_end: text.chars().count().try_into().unwrap(),
            options: ShapeOptions {
                font_size: 32.0,
                language: None,
                features: &[],
                variations: &[],
            },
        }];
        shaper.shape_text(
            text,
            &analysis,
            &char_style_indices,
            items,
            select_font,
            &mut shaped,
        );
        shaped
    }

    /// A run can shape into no glyphs at all: `HarfRust` normally shapes `default-ignorable`
    /// characters (like a soft-hyphen) into a space glyph with zero advance, but if the space glyph
    /// is not available, it just deletes the character.
    ///
    /// This checks whether we handle that.
    #[test]
    fn run_without_glyphs_is_kept() {
        // The soft hyphen is `default-ignorable`, and is a grapheme of its own, so font selection
        // gives it a run of its own.
        let shaped = shape_with_font_selector(
            "a\u{00AD}b",
            FontForChar {
                ch: '\u{00AD}',
                // Our Noto Color Emoji subset has no space character.
                font: font_instance(NOTO_COLOR_EMOJI),
                other_font: font_instance(ROBOTO),
            },
        );

        assert_eq!(
            shaped.characters().iter().count(),
            3,
            "the characters of a run without glyphs are shaped"
        );
        let [a, soft_hyphen, b] = shaped.runs() else {
            panic!("expected three runs, got {}", shaped.runs().len());
        };
        assert_eq!(a.characters_range, 0..1);
        assert_eq!(soft_hyphen.characters_range, 1..2);
        assert_eq!(b.characters_range, 2..3);

        assert!(soft_hyphen.glyphs_range.is_empty());
        assert_eq!(soft_hyphen.advance, 0.);

        // The soft-hyphen's run should have a single cluster, and it must be without glyphs.
        let clusters = shaped.run_slice(1).shaped_clusters();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].glyph_len(), 0);
        assert_eq!(clusters[0].chars_range(), 1..2);
    }

    #[test]
    fn single_cluster_run_advance_matches_its_cluster() {
        let shaped = shape_with_font("A", ROBOTO);
        let run = &shaped.runs()[0];
        let cluster = &shaped.shaped_clusters()[run.shaped_clusters_range.start as usize];

        assert!(cluster.advance > 0.0);
        assert_eq!(run.advance, cluster.advance);
    }

    /// [U+0600 U+0623 U+064F] is a single grapheme in text-wide segmentation, but U+0600 has
    /// bidi class AN while the following characters resolve to a different bidi level, so
    /// itemization splits the grapheme into the items [U+0600] [U+0623 U+064F]. The item
    /// boundary forces a grapheme start on U+0623.
    #[test]
    fn one_grapheme_split_across_items() {
        let shaped = shape_with_font("\u{0600}\u{0623}\u{064F}", NOTO_KUFI_ARABIC);

        let grapheme_starts: Vec<bool> =
            shaped.characters.iter().map(|c| c.grapheme_start).collect();
        assert_eq!(grapheme_starts, [true, true, false]);

        let clusters: Vec<(u32, u32, bool)> = shaped
            .shaped_clusters
            .iter()
            .map(|c| {
                (
                    c.chars_range().start,
                    c.chars_range().end,
                    c.is_grapheme_start(),
                )
            })
            .collect();
        assert_eq!(clusters, [(0, 1, true), (1, 3, true)]);
    }

    /// "ffi" is three graphemes but shapes into one cluster.
    #[test]
    fn three_graphemes_one_cluster() {
        let shaped = shape_with_font("ffi", ROBOTO);

        let grapheme_starts: Vec<bool> =
            shaped.characters.iter().map(|c| c.grapheme_start).collect();
        assert_eq!(grapheme_starts, [true, true, true]);

        let clusters: Vec<(u32, u32, bool)> = shaped
            .shaped_clusters
            .iter()
            .map(|c| {
                (
                    c.chars_range().start,
                    c.chars_range().end,
                    c.is_grapheme_start(),
                )
            })
            .collect();
        assert_eq!(clusters, [(0, 3, true)]);
    }

    #[test]
    fn newlines_removed() {
        for newline in ["\n", "\r", "\u{2028}", "\u{2029}"] {
            let shaped = shape_with_font(&alloc::format!("a{newline}b"), ROBOTO);

            let index = shaped
                .shaped_clusters
                .iter()
                .position(|cluster| cluster.chars_range().start == 1)
                .unwrap();
            let newline_cluster = &shaped.shaped_clusters[index];
            assert_eq!(newline_cluster.glyph_len(), 0, "newline glyphs are removed");
            assert_eq!(newline_cluster.advance, 0., "newline advance is set to 0");
        }
    }

    /// An itemization boundary forces a cluster break in shaping, and must force a grapheme start.
    #[test]
    fn item_boundaries_force_grapheme_start() {
        // Two regional indicators forming a single flag grapheme...
        let text = "\u{1F1E6}\u{1F1E7}";
        let char_style_indices = vec![0; text.chars().count()];
        let analysis = analyze(text);
        assert_eq!(
            analysis
                .char_info()
                .iter()
                .map(|info| info.is_grapheme_start())
                .collect::<Vec<_>>(),
            [true, false]
        );

        let font = font_instance(ROBOTO);
        let mut shaper = Shaper::default();
        let mut shaped = ShapedText::new();

        // ...split over two items.
        let items = [
            Item {
                char_end: 1,
                options: ShapeOptions {
                    font_size: 32.0,
                    language: None,
                    features: &[],
                    variations: &[],
                },
            },
            Item {
                char_end: text.chars().count().try_into().unwrap(),
                options: ShapeOptions {
                    font_size: 32.0,
                    language: None,
                    features: &[],
                    variations: &[],
                },
            },
        ];
        shaper.shape_text(
            text,
            &analysis,
            &char_style_indices,
            items,
            SingleFont(font),
            &mut shaped,
        );

        let grapheme_starts: Vec<bool> =
            shaped.characters.iter().map(|c| c.grapheme_start).collect();
        assert_eq!(grapheme_starts, [true, true]);

        // But note that, had we had three regional indicator symbols, like `[R0 R1 R2]` itemized as
        // `[R0] [R1 R2]`, the last pair should form a single grapheme. We don't do that currently.
    }
}
