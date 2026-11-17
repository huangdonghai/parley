// Copyright 2024 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Query support.

use crate::{Charmap, CharmapIndex};

use super::super::{Collection, SourceCache, matching::match_fonts};

use alloc::vec::Vec;
use parlance::Script;
use smallvec::SmallVec;

use super::{
    super::{
        Attributes, Blob, FallbackKey, FamilyId, FamilyInfo, FontFeature, FontUnicodeRange,
        GenericFamily, Synthesis,
    },
    Inner,
};

#[derive(Clone, Default)]
pub(super) struct QueryState {
    families: Vec<CachedFamily>,
    fallback_families: Vec<CachedFamily>,
}

impl QueryState {
    fn clear(&mut self) {
        self.families.clear();
        self.fallback_families.clear();
    }
}

/// State for font selection.
///
/// Instances of this can be obtained from [`Collection::query`].
pub struct Query<'a> {
    collection: &'a mut Inner,
    state: &'a mut QueryState,
    source_cache: &'a mut SourceCache,
    attributes: Attributes,
    fallbacks: Option<FallbackKey>,
}

impl<'a> Query<'a> {
    pub(super) fn new(collection: &'a mut Collection, source_cache: &'a mut SourceCache) -> Self {
        collection.query_state.clear();
        Self {
            collection: &mut collection.inner,
            state: &mut collection.query_state,
            source_cache,
            attributes: Attributes::default(),
            fallbacks: None,
        }
    }

    /// Sets the ordered sequence of families to match against.
    pub fn set_families<'f, I>(&mut self, families: I)
    where
        I: IntoIterator,
        I::Item: Into<QueryFamily<'f>>,
    {
        self.state.families.clear();
        for family in families {
            let family = family.into();
            match family {
                QueryFamily::Named(name) => {
                    if let Some(id) = self.collection.family_id(name) {
                        self.state.families.push(CachedFamily::new(id));
                    }
                }
                QueryFamily::Id(id) => {
                    self.state.families.push(CachedFamily::new(id));
                }
                QueryFamily::Generic(generic) => {
                    for id in self.collection.generic_families(generic) {
                        self.state.families.push(CachedFamily::new(id));
                    }
                }
            }
        }
    }

    /// Sets the primary attributes to match against.
    pub fn set_attributes(&mut self, attributes: Attributes) {
        if self.attributes != attributes {
            for family in &mut self.state.families {
                family.clear_fonts();
            }
            for family in &mut self.state.fallback_families {
                family.clear_fonts();
            }
            self.attributes = attributes;
        }
    }

    /// Sets the script and locale for fallback fonts.
    pub fn set_fallbacks(&mut self, key: impl Into<FallbackKey>) {
        let key = key.into();
        if self.fallbacks != Some(key) {
            self.state.fallback_families.clear();
            self.state.fallback_families.extend(
                self.collection
                    .fallback_families(key)
                    .map(CachedFamily::new),
            );
            // HACK: always add a Han font to the fallback list to capture
            // punctuation in the common script
            // See <https://github.com/linebender/parley/issues/597>
            self.state.fallback_families.extend(
                self.collection
                    .fallback_families(FallbackKey::new(Script::from_bytes(*b"Hani"), None))
                    .map(CachedFamily::new),
            );
            self.fallbacks = Some(key);
        }
    }

    /// Invokes the given callback with all fonts that match the current
    /// settings.
    ///
    /// Return [`QueryStatus::Stop`] to end iterating over the matching
    /// fonts or [`QueryStatus::Continue`] to continue iterating.
    pub fn matches_with<'q>(&'q mut self, mut f: impl FnMut(&'q QueryFont) -> QueryStatus) {
        for family in self
            .state
            .families
            .iter_mut()
            .chain(self.state.fallback_families.iter_mut())
        {
            match &mut family.family {
                Entry::Error => continue,
                Entry::Ok(..) => {}
                status @ Entry::Vacant => {
                    if let Some(info) = self.collection.family(family.id) {
                        *status = Entry::Ok(info);
                    } else {
                        *status = Entry::Error;
                        continue;
                    }
                }
            }
            let Entry::Ok(family_info) = &family.family else {
                continue;
            };
            let bucket = load_bucket(
                family_info,
                self.attributes,
                &mut family.best,
                self.source_cache,
            );
            let mut yielded_default = false;
            for font in bucket {
                if font.family.1 == family_info.default_font_index() {
                    yielded_default = true;
                }
                if f(font) == QueryStatus::Stop {
                    return;
                }
            }
            if yielded_default {
                continue;
            }
            if let Some(font) = load_font(
                family_info,
                self.attributes,
                &mut family.default,
                self.source_cache,
            ) && f(font) == QueryStatus::Stop
            {
                return;
            }
        }
    }

    /// Returns the [first available font] for the current settings: the first
    /// font that [`matches_with`](Self::matches_with) would yield, if any.
    ///
    /// CSS Fonts requires the first available font's `unicode-range` to include U+0020 SPACE.
    /// The face need not actually contain a space glyph.
    ///
    /// [first available font]: https://drafts.csswg.org/css-fonts-4/#first-available-font
    pub fn first_available_font(&mut self) -> Option<QueryFont> {
        let mut found = None;
        self.matches_with(|font| {
            if !font.matches_unicode_range(0x20) {
                return QueryStatus::Continue;
            }
            found = Some(font.clone());
            QueryStatus::Stop
        });
        found
    }
}

impl Drop for Query<'_> {
    fn drop(&mut self) {
        self.state.clear();
    }
}

/// Determines whether a font query operation will continue.
///
/// See [`Query::matches_with`].
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum QueryStatus {
    /// Query should continue with the next font.
    Continue,
    /// Query should stop.
    Stop,
}

/// Family descriptor for a font query.
///
/// This allows [`Query::set_families`] to
/// take a variety of family types.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum QueryFamily<'a> {
    /// A named font family.
    Named(&'a str),
    /// An identifier for a font family.
    Id(FamilyId),
    /// A generic font family.
    Generic(GenericFamily),
}

impl<'a> From<&'a str> for QueryFamily<'a> {
    fn from(value: &'a str) -> Self {
        Self::Named(value)
    }
}

impl From<FamilyId> for QueryFamily<'static> {
    fn from(value: FamilyId) -> Self {
        Self::Id(value)
    }
}

impl From<GenericFamily> for QueryFamily<'static> {
    fn from(value: GenericFamily) -> Self {
        Self::Generic(value)
    }
}

/// Candidate font generated by a [`Query`].
#[derive(Clone, Debug)]
pub struct QueryFont {
    /// Family identifier and index of the font in the family font list.
    pub family: (FamilyId, usize),
    /// Blob containing the font data.
    pub blob: Blob<u8>,
    /// Index of a font in a font collection (`ttc`) file.
    pub index: u32,
    /// Synthesis suggestions for this font based on the requested attributes.
    pub synthesis: Synthesis,
    /// Data used for constructing a character map for this font.
    pub charmap_index: CharmapIndex,
    /// CSS `unicode-range` intervals, or `None` when the face has no range restriction.
    pub unicode_ranges: Option<Vec<FontUnicodeRange>>,
    /// `@font-face` feature defaults for this exact face.
    pub font_features: Vec<FontFeature>,
}

impl QueryFont {
    /// Whether this face is eligible for a codepoint under its `unicode-range` descriptor.
    pub fn matches_unicode_range(&self, codepoint: u32) -> bool {
        self.unicode_ranges
            .as_ref()
            .is_none_or(|ranges| ranges.iter().any(|range| range.contains(codepoint)))
    }

    /// The face's OpenType feature defaults.
    pub fn font_features(&self) -> &[FontFeature] {
        &self.font_features
    }

    /// Attempts to construct a [Charmap] for this font.
    pub fn charmap(&self) -> Option<Charmap<'_>> {
        self.charmap_index.charmap(self.blob.as_ref())
    }
}

/// Loads every font in `family` matching `attributes` (all coverage variants of the best match).
fn load_bucket<'a>(
    family: &FamilyInfo,
    attributes: Attributes,
    bucket: &'a mut Entry<SmallVec<[QueryFont; 1]>>,
    source_cache: &mut SourceCache,
) -> &'a [QueryFont] {
    match bucket {
        Entry::Error => &[],
        Entry::Ok(fonts) => fonts,
        status @ Entry::Vacant => {
            *status = Entry::Error;
            let indices = match_fonts(
                family.fonts(),
                attributes.width,
                attributes.style,
                attributes.weight,
                true,
            );
            let mut fonts: SmallVec<[QueryFont; 1]> = SmallVec::new();
            for index in indices {
                let Some(font_info) = family.fonts().get(index) else {
                    continue;
                };
                let Some(blob) = font_info.load(Some(source_cache)) else {
                    continue;
                };
                fonts.push(QueryFont {
                    family: (family.id(), index),
                    blob: blob.clone(),
                    index: font_info.index(),
                    synthesis: font_info.synthesis(
                        attributes.width,
                        attributes.style,
                        attributes.weight,
                    ),
                    charmap_index: font_info.charmap_index(),
                    unicode_ranges: font_info.unicode_ranges().map(<[_]>::to_vec),
                    font_features: font_info.font_features().to_vec(),
                });
            }
            if fonts.is_empty() {
                return &[];
            }
            *status = Entry::Ok(fonts);
            let Entry::Ok(fonts) = status else {
                unreachable!()
            };
            fonts
        }
    }
}

fn load_font<'a>(
    family: &FamilyInfo,
    attributes: Attributes,
    font: &'a mut Entry<QueryFont>,
    source_cache: &mut SourceCache,
) -> Option<&'a QueryFont> {
    match font {
        Entry::Error => None,
        Entry::Ok(font) => Some(font),
        status @ Entry::Vacant => {
            // Set to error in case we fail. This simplifies
            // the following code.
            *status = Entry::Error;
            let family_index = family.default_font_index();
            let font_info = family.fonts().get(family_index)?;
            let blob = font_info.load(Some(source_cache))?;
            let blob_index = font_info.index();
            let synthesis =
                font_info.synthesis(attributes.width, attributes.style, attributes.weight);
            *status = Entry::Ok(QueryFont {
                family: (family.id(), family_index),
                blob,
                index: blob_index,
                synthesis,
                charmap_index: font_info.charmap_index(),
                unicode_ranges: font_info.unicode_ranges().map(<[_]>::to_vec),
                font_features: font_info.font_features().to_vec(),
            });
            if let Entry::Ok(font) = status {
                Some(font)
            } else {
                None
            }
        }
    }
}

#[derive(Clone)]
struct CachedFamily {
    id: FamilyId,
    family: Entry<FamilyInfo>,
    best: Entry<SmallVec<[QueryFont; 1]>>,
    default: Entry<QueryFont>,
}

impl CachedFamily {
    fn new(id: FamilyId) -> Self {
        Self {
            id,
            family: Entry::Vacant,
            best: Entry::Vacant,
            default: Entry::Vacant,
        }
    }

    fn clear_fonts(&mut self) {
        self.best = Entry::Vacant;
        self.default = Entry::Vacant;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::vec::Vec;

    use super::QueryStatus;
    use crate::{
        Blob, Collection, CollectionOptions, FontFeature, FontInfoOverride, FontUnicodeRange,
        QueryFamily, SourceCache,
    };
    use parlance::Tag;

    const ROBOTO: &[u8] =
        include_bytes!("../../../parley_dev/assets/fonts/roboto_fonts/Roboto-Regular.ttf");

    #[test]
    fn same_family_faces_keep_distinct_feature_settings() {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let first_range = [FontUnicodeRange::new(0xFF08, 0xFF08)];
        let second_range = [FontUnicodeRange::new(0x56FD, 0x56FD)];
        let first_features = [FontFeature::new(Tag::new(b"halt"), 1)];
        let second_features = [FontFeature::new(Tag::new(b"halt"), 0)];
        let data = Blob::new(Arc::new(ROBOTO.to_vec()));
        collection.register_fonts(
            data.clone(),
            Some(FontInfoOverride {
                family_name: Some("SubsetFamily"),
                unicode_ranges: Some(&first_range),
                font_features: Some(&first_features),
                ..Default::default()
            }),
        );
        collection.register_fonts(
            data,
            Some(FontInfoOverride {
                family_name: Some("SubsetFamily"),
                unicode_ranges: Some(&second_range),
                font_features: Some(&second_features),
                ..Default::default()
            }),
        );

        let family = collection.family_id("SubsetFamily").unwrap();
        let mut source_cache = SourceCache::default();
        let mut query = collection.query(&mut source_cache);
        query.set_families([QueryFamily::Id(family)]);
        let mut faces = Vec::new();
        query.matches_with(|font| {
            faces.push((font.unicode_ranges.clone(), font.font_features.clone()));
            QueryStatus::Continue
        });

        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0], (Some(first_range.to_vec()), first_features.to_vec()));
        assert_eq!(faces[1], (Some(second_range.to_vec()), second_features.to_vec()));
    }

    #[test]
    fn first_available_skips_faces_without_space_in_unicode_range() {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let ascii_letters = [FontUnicodeRange::new(0x41, 0x5A)];
        let with_space = [FontUnicodeRange::new(0x20, 0x7E)];
        let data = Blob::new(Arc::new(ROBOTO.to_vec()));
        collection.register_fonts(
            data.clone(),
            Some(FontInfoOverride {
                family_name: Some("LettersOnly"),
                unicode_ranges: Some(&ascii_letters),
                ..Default::default()
            }),
        );
        collection.register_fonts(
            data,
            Some(FontInfoOverride {
                family_name: Some("HasSpace"),
                unicode_ranges: Some(&with_space),
                ..Default::default()
            }),
        );
        let letters_id = collection.family_id("LettersOnly").unwrap();
        let space_id = collection.family_id("HasSpace").unwrap();
        let mut source_cache = SourceCache::default();
        let mut query = collection.query(&mut source_cache);
        query.set_families([QueryFamily::Id(letters_id), QueryFamily::Id(space_id)]);

        let first_available = query.first_available_font().unwrap();
        assert_eq!(first_available.family.0, space_id);
        assert!(first_available.matches_unicode_range(0x20));
        assert!(!first_available.matches_unicode_range(0x7F));

        query.set_families([QueryFamily::Id(letters_id)]);
        assert!(query.first_available_font().is_none());
    }
}

#[derive(Clone)]
enum Entry<T> {
    Ok(T),
    Vacant,
    Error,
}
