use read_fonts::tables::base::BaseCoord;
use read_fonts::tables::layout::DeviceOrVariationIndex;
use read_fonts::{FontRef, TableProvider};

use super::font_loading::post_script_name_for_face;
use super::*;
use crate::document::{
    CssFontVerticalMetrics, DocumentFontSynthesis, DocumentFontVariationCoordinates,
    OpenTypeBaselineAxis, OpenTypeBaselineCoordinate, OpenTypeBaselineScript,
    OpenTypeBaselineTable, OpenTypeVariationIndex, OpenTypeVerticalMetrics, SyntheticObliqueAngle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FontSupportKind {
    EmbeddableText,
    ColorOrEmojiOnlyFallback,
}

/// The presentation a font can provide for an emoji variation sequence.
///
/// This is deliberately derived from font program tables rather than a CSS
/// family alias. An `@font-face` rule is allowed to rename any font program,
/// so names such as `MonoEmojiFont` carry no presentation semantics.
/// <https://www.w3.org/TR/css-fonts-4/#font-variant-emoji-prop>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EmojiPresentationCapability {
    Text,
    Emoji,
}

struct DocumentFontMetadata {
    program_kind: FontProgramKind,
    family: String,
    post_script_name: String,
    units_per_em: u16,
    program_metrics: OpenTypeVerticalMetrics,
    layout_metrics: CssFontVerticalMetrics,
    cap_height: i16,
    italic_angle: i16,
    bbox: [i16; 4],
    baselines: OpenTypeBaselineTable,
    size_adjust: Option<f32>,
    variation_coordinates: DocumentFontVariationCoordinates,
    synthesis: DocumentFontSynthesis,
}

impl DocumentFontRegistry {
    pub(super) fn new(
        registered_font_faces: HashMap<RegisteredFontFaceKey, RegisteredFontFaceMetadata>,
    ) -> Self {
        Self {
            fonts: Vec::new(),
            registered_font_faces,
            document_font_faces: HashMap::new(),
            font_cache: HashMap::new(),
            font_blob_cache: HashMap::new(),
            parley_font_cache: HashMap::new(),
            font_size_adjust: HashMap::new(),
        }
    }

    pub(super) fn into_fonts(self) -> Vec<DocumentFont> {
        self.fonts
    }

    pub(crate) fn get(&self, font_id: usize) -> Option<&DocumentFont> {
        self.fonts.get(font_id)
    }

    pub(crate) fn font_has_character(&self, font_id: usize, character: char) -> bool {
        self.character_font_match(font_id, character).is_some()
    }

    /// Return a verified CSS-font match for one source character.
    ///
    /// The `unicode-range` descriptor and the OpenType cmap must both accept
    /// the scalar. A cmap mapping to glyph zero is `.notdef`, not coverage.
    pub(crate) fn character_font_match(
        &self,
        font_id: usize,
        character: char,
    ) -> Option<CharacterFontMatch> {
        let font = self.get(font_id)?;
        self.font_face_allows_character(font, character)
            .then(|| font_covered_glyph(font, character))
            .flatten()
            .map(|glyph_id| CharacterFontMatch { font_id, glyph_id })
    }

    pub(super) fn font_has_unicode_range(&self, font_id: usize) -> bool {
        self.get(font_id)
            .and_then(|font| self.metadata_for_document_font(font))
            .is_some_and(|metadata| metadata.unicode_range.is_some())
    }

    /// Return the `@font-face size-adjust` factor associated with a resolved
    /// document face. The descriptor is part of face selection metadata, not
    /// an intrinsic OpenType table value.
    /// <https://www.w3.org/TR/css-fonts-5/#descdef-font-face-size-adjust>
    pub(crate) fn font_size_adjust(&self, font_id: usize) -> Option<f32> {
        self.font_size_adjust.get(&font_id).cloned()
    }

    /// Return the descriptors of the exact `@font-face` selected for a
    /// document font. These descriptors cannot be derived from the authored
    /// family alone: a family can contain multiple faces with different
    /// feature defaults and `unicode-range` eligibility.
    pub(super) fn selected_face_features(
        &self,
        font_id: usize,
    ) -> Option<(String, FontFaceFeatureDefaults)> {
        let font = self.get(font_id)?;
        let metadata = self.metadata_for_document_font(font)?;
        Some((metadata.family.clone(), metadata.feature_defaults.clone()))
    }

    /// Return the fixed CSS descriptors of the selected `@font-face`.
    /// Variable weight and width descriptors keep their requested axis value;
    /// a fixed face is the only case where CSS `font-synthesis` may need to
    /// substitute the face's own descriptor before shaping.
    pub(super) fn selected_face_fixed_attributes(
        &self,
        font_id: usize,
    ) -> Option<(Option<FontWeight>, FontStyle)> {
        let font = self.get(font_id)?;
        let metadata = self.metadata_for_document_font(font)?;
        Some((
            (!metadata.weight_is_variable).then_some(metadata.weight),
            metadata.style,
        ))
    }

    pub(super) fn has_registered_css_family(&self, family: &str) -> bool {
        self.registered_font_faces
            .values()
            .any(|metadata| metadata.family.eq_ignore_ascii_case(family))
    }

    pub(crate) fn selected_face_variations(&self, font_id: usize) -> Option<FontVariationSettings> {
        let font = self.get(font_id)?;
        Some(
            self.metadata_for_document_font(font)?
                .font_variation_settings
                .clone(),
        )
    }

    fn font_face_allows_character(&self, font: &DocumentFont, character: char) -> bool {
        self.metadata_for_document_font(font)
            .and_then(|metadata| metadata.unicode_range.as_deref())
            .is_none_or(|ranges| ranges.iter().any(|range| range.contains(character)))
    }

    fn metadata_for_document_font(
        &self,
        font: &DocumentFont,
    ) -> Option<&RegisteredFontFaceMetadata> {
        self.document_font_faces
            .get(&font.id)
            .and_then(|key| self.registered_font_faces.get(key))
            .or_else(|| {
                // Parley can hand a document font back through a separate
                // query path whose collection index is not retained in the
                // registry cache. The CSS family override remains stable for
                // that path, so recover the sole matching face metadata
                // instead of silently dropping unicode-range and descriptor
                // behavior.
                // <https://www.w3.org/TR/css-fonts-4/#font-face-src-desc>
                let mut matching = self
                    .registered_font_faces
                    .values()
                    .filter(|metadata| metadata.family.eq_ignore_ascii_case(&font.family));
                let metadata = matching.next()?;
                matching.next().is_none().then_some(metadata)
            })
    }

    pub(super) fn font_query_has_character(font: &FontiqueQueryFont, character: char) -> bool {
        if standalone_font_program_kind(font.blob.as_ref()).is_none() {
            return false;
        }
        ttf_parser::Face::parse(font.blob.as_ref(), font.index)
            .ok()
            .and_then(|face| CoveredGlyphId::from_face(&face, character))
            .is_some()
    }

    /// Returns whether a system font program can be emitted as a PDF outline
    /// font. CSS generic-family selection is user-agent-defined, so Quire
    /// excludes candidates whose OS/2 embedding permissions prohibit the PDF
    /// outline program that its writer emits.
    ///
    /// <https://learn.microsoft.com/en-us/typography/opentype/spec/os2#fstype>
    pub(super) fn font_query_allows_outline_embedding(font: &FontiqueQueryFont) -> bool {
        ttf_parser::Face::parse(font.blob.as_ref(), font.index)
            .is_ok_and(|face| face.is_outline_embedding_allowed())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn document_font_from_query(
        &mut self,
        collection: &mut fontique::Collection,
        font: FontiqueQueryFont,
        family_override: Option<&str>,
        request: &FontRequest,
        synthesize_weight: bool,
        synthesize_style: bool,
        variation_coordinates: DocumentFontVariationCoordinates,
    ) -> Option<usize> {
        let registered_face = RegisteredFontFaceKey {
            family_id: font.family.0.to_u64(),
            family_index: font.family.1,
        };
        let registered_face = self
            .registered_font_faces
            .contains_key(&registered_face)
            .then_some(registered_face);
        let key = FontKey {
            family_id: font.family.0,
            family_index: font.family.1,
            face_index: font.index,
            request: request.attributes,
            synthesize_weight,
            synthesize_style,
            variation_coordinates: variation_coordinates.clone(),
        };
        if family_override.is_none()
            && let Some(id) = self.font_cache.get(&key)
        {
            return Some(*id);
        }

        let data = font.blob.as_ref();
        let face = ttf_parser::Face::parse(data, font.index).ok()?;
        let family_name = collection
            .family_name(font.family.0)
            .map(str::to_string)
            .or_else(|| opentype_name(&face, ttf_parser::name_id::TYPOGRAPHIC_FAMILY))
            .or_else(|| opentype_name(&face, ttf_parser::name_id::FAMILY))
            .or_else(|| opentype_name(&face, ttf_parser::name_id::POST_SCRIPT_NAME))
            .unwrap_or_else(|| format!("Font-{}", font.family.0.to_u64()));
        let family = family_override.unwrap_or(&family_name).to_string();
        let resolved_key = ResolvedFontFaceKey {
            blob_id: font.blob.id(),
            face_index: font.index,
            registered_face,
            family_label: Some(family.clone()),
            request: Some(request.clone()),
            synthesize_weight,
            synthesize_style,
            variation_coordinates: variation_coordinates.clone(),
        };
        if let Some(id) = self.font_blob_cache.get(&resolved_key) {
            return Some(*id);
        }

        let face_metadata = registered_face.and_then(|key| self.registered_font_faces.get(&key));
        let size_adjust = face_metadata
            .and_then(|metadata| metadata.size_adjust)
            .map(f32::from_bits);
        let mut metadata = document_font_metadata(
            data,
            font.index,
            family,
            synthesize_weight && font.synthesis.embolden(),
            synthesize_style
                .then(|| font.synthesis.skew())
                .flatten()
                .and_then(fontique_synthetic_oblique_angle),
            size_adjust,
            variation_coordinates,
        )?;
        if let Some(face_metadata) = face_metadata {
            apply_metric_overrides(&mut metadata, face_metadata);
        }
        let id = self.push_document_font(metadata, font.blob, font.index, registered_face);
        self.font_blob_cache.insert(resolved_key, id);
        if family_override.is_none() {
            self.font_cache.insert(key, id);
        }
        Some(id)
    }

    pub(super) fn document_font_from_parley(
        &mut self,
        font_data: &parley::FontData,
        variation_coordinates: DocumentFontVariationCoordinates,
    ) -> Option<usize> {
        let data = font_data.data.as_ref();
        let face = ttf_parser::Face::parse(data, font_data.index).ok()?;
        let family = opentype_name(&face, ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
            .or_else(|| opentype_name(&face, ttf_parser::name_id::FAMILY))
            .or_else(|| opentype_name(&face, ttf_parser::name_id::POST_SCRIPT_NAME))
            .unwrap_or_else(|| format!("FontBlob-{}", font_data.data.id()));
        let resolved_key = ResolvedFontFaceKey {
            blob_id: font_data.data.id(),
            face_index: font_data.index,
            registered_face: None,
            family_label: Some(family.clone()),
            request: None,
            synthesize_weight: false,
            synthesize_style: false,
            variation_coordinates: variation_coordinates.clone(),
        };
        if let Some(id) = self.font_blob_cache.get(&resolved_key) {
            return Some(*id);
        }

        let metadata = document_font_metadata(
            data,
            font_data.index,
            family,
            false,
            None,
            None,
            variation_coordinates,
        )?;
        let id = self.push_document_font(metadata, font_data.data.clone(), font_data.index, None);
        self.font_blob_cache.insert(resolved_key, id);
        Some(id)
    }

    pub(super) fn cached_parley_font(
        &self,
        font_data: &parley::FontData,
        request: &FontRequest,
        synthesize_weight: bool,
        synthesize_style: bool,
        variation_coordinates: &DocumentFontVariationCoordinates,
    ) -> Option<usize> {
        self.parley_font_cache
            .get(&ParleyFontRequestKey {
                blob_id: font_data.data.id(),
                face_index: font_data.index,
                request: request.clone(),
                synthesize_weight,
                synthesize_style,
                variation_coordinates: variation_coordinates.clone(),
            })
            .cloned()
    }

    pub(super) fn cache_parley_font(
        &mut self,
        font_data: &parley::FontData,
        request: &FontRequest,
        synthesize_weight: bool,
        synthesize_style: bool,
        variation_coordinates: &DocumentFontVariationCoordinates,
        font_id: usize,
    ) {
        self.parley_font_cache.insert(
            ParleyFontRequestKey {
                blob_id: font_data.data.id(),
                face_index: font_data.index,
                request: request.clone(),
                synthesize_weight,
                synthesize_style,
                variation_coordinates: variation_coordinates.clone(),
            },
            font_id,
        );
    }

    pub(super) fn support_kind_for_run(&self, font_id: usize, text: &str) -> FontSupportKind {
        let Some(font) = self.get(font_id) else {
            return FontSupportKind::EmbeddableText;
        };
        let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
            return FontSupportKind::EmbeddableText;
        };

        let mut has_visible_glyph = false;
        let mut has_text_outline = false;
        for glyph in text
            .chars()
            .filter(|character| !character_is_default_ignorable_code_point(*character))
            .filter_map(|character| face.glyph_index(character))
        {
            has_visible_glyph = true;
            has_text_outline |= face.glyph_bounding_box(glyph).is_some();
            if face.is_color_glyph(glyph) {
                return FontSupportKind::ColorOrEmojiOnlyFallback;
            }
        }

        if !has_visible_glyph {
            return FontSupportKind::EmbeddableText;
        }
        if !has_text_outline || face_has_color_presentation_tables(&face) {
            FontSupportKind::ColorOrEmojiOnlyFallback
        } else {
            FontSupportKind::EmbeddableText
        }
    }

    /// Return whether a face can serve an emoji presentation request for a
    /// base scalar and optional variation selector.
    ///
    /// A cmap format 14 mapping is preferred when present; its registered
    /// default UVS is coverage for that sequence. Fonts without such a mapping
    /// still participate through base coverage, allowing the user agent's
    /// presentation policy to select a color or text face for the cluster.
    pub(super) fn emoji_presentation_capability(
        &self,
        font_id: usize,
        base: char,
        selector: Option<char>,
    ) -> Option<EmojiPresentationCapability> {
        let font = self.get(font_id)?;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        let glyph = selector
            .and_then(|selector| face.glyph_variation_index(base, selector))
            .or_else(|| face.glyph_index(base))?;
        (glyph.0 != 0).then_some(())?;
        Some(if face_has_color_presentation_tables(&face) {
            EmojiPresentationCapability::Emoji
        } else {
            EmojiPresentationCapability::Text
        })
    }

    /// Whether a run has at least one color-capable glyph. This remains a
    /// paint/fallback safeguard; presentation selection itself uses the
    /// base-plus-selector query above.
    pub(super) fn run_has_emoji_presentation_glyph(&self, font_id: usize, text: &str) -> bool {
        text.chars()
            .filter(|character| !character_is_default_ignorable_code_point(*character))
            .any(|character| {
                self.emoji_presentation_capability(font_id, character, None)
                    == Some(EmojiPresentationCapability::Emoji)
            })
    }

    fn push_document_font(
        &mut self,
        metadata: DocumentFontMetadata,
        blob: FontiqueBlob<u8>,
        face_index: u32,
        registered_face: Option<RegisteredFontFaceKey>,
    ) -> usize {
        let id = self.fonts.len();
        self.fonts.push(DocumentFont {
            id,
            family: metadata.family,
            post_script_name: metadata.post_script_name,
            program_kind: metadata.program_kind,
            data: crate::document::DocumentFontData::from_blob(blob),
            face_index,
            units_per_em: metadata.units_per_em,
            program_metrics: metadata.program_metrics,
            layout_metrics: metadata.layout_metrics,
            cap_height: metadata.cap_height,
            italic_angle: metadata.italic_angle,
            bbox: metadata.bbox,
            baselines: metadata.baselines,
            variation_coordinates: metadata.variation_coordinates,
            synthesis: metadata.synthesis,
        });
        if let Some(size_adjust) = metadata.size_adjust {
            self.font_size_adjust.insert(id, size_adjust);
        }
        if let Some(registered_face) = registered_face {
            self.document_font_faces.insert(id, registered_face);
        }
        id
    }
}

fn document_font_metadata(
    data: &[u8],
    face_index: u32,
    family: String,
    synthesize_bold: bool,
    synthetic_oblique: Option<SyntheticObliqueAngle>,
    size_adjust: Option<f32>,
    variation_coordinates: DocumentFontVariationCoordinates,
) -> Option<DocumentFontMetadata> {
    let program_kind = standalone_font_program_kind(data)?;
    let face = ttf_parser::Face::parse(data, face_index).ok()?;
    let post_script_name =
        post_script_name_for_face(&face, &family, synthesize_bold, synthetic_oblique.is_some());
    let bbox = [
        face.global_bounding_box().x_min,
        face.global_bounding_box().y_min,
        face.global_bounding_box().x_max,
        face.global_bounding_box().y_max,
    ];
    // Pango uses the OpenType typographic vertical metrics when a font
    // supplies them.  These metrics define CSS `line-height: normal` and the
    // baseline used by WeasyPrint; the hhea ascender/descender can include a
    // much larger legacy line box.  Keep the hhea pair as the fallback for
    // older or incomplete faces.
    // <https://learn.microsoft.com/en-us/typography/opentype/spec/os2#stypoascender>
    let (ascender, descender) = match (face.typographic_ascender(), face.typographic_descender()) {
        (Some(ascender), Some(descender)) if ascender != 0 && descender != 0 => {
            (ascender, descender)
        }
        _ => (face.ascender(), face.descender()),
    };
    let program_metrics = OpenTypeVerticalMetrics {
        ascender,
        descender,
        // Preserve Quire's existing normal-line-height policy for unmodified
        // faces. A CSS `line-gap-override` supplies an explicit replacement
        // below; raw OpenType line-gap adoption remains a separate policy
        // decision.
        line_gap: 0,
    };
    Some(DocumentFontMetadata {
        program_kind,
        family,
        post_script_name,
        units_per_em: face.units_per_em(),
        program_metrics,
        layout_metrics: CssFontVerticalMetrics {
            ascender: program_metrics.ascender,
            descender: program_metrics.descender,
            line_gap: program_metrics.line_gap,
        },
        cap_height: face.capital_height().unwrap_or_else(|| face.ascender()),
        italic_angle: face.italic_angle().round() as i16,
        bbox,
        baselines: baseline_table_for_font(data, face_index),
        size_adjust,
        variation_coordinates,
        synthesis: DocumentFontSynthesis {
            embolden: synthesize_bold,
            oblique: synthetic_oblique,
        },
    })
}

/// Fontique stores faux-oblique synthesis as an `i8`, exposed through its
/// public API as `f32`. Preserve the source's discrete value for the PDF
/// paint state rather than rounding arbitrary authored CSS angles here.
fn fontique_synthetic_oblique_angle(degrees: f32) -> Option<SyntheticObliqueAngle> {
    let integer = degrees as i8;
    (degrees.is_finite() && f32::from(integer) == degrees)
        .then(|| SyntheticObliqueAngle::from_fontique_degrees(integer))
        .flatten()
}

/// Extract immutable design-unit baseline information from OpenType `BASE`.
///
/// CSS Inline defines font baseline tables independently for horizontal and
/// vertical typography, while OpenType stores those axes in the corresponding
/// `BASE` Axis tables. Format 2 device corrections are deliberately not
/// retained: PDF layout is resolution-independent and unhinted. Format 3
/// variation indices are retained for resolution at the exact CSS variation
/// instance during layout.
/// <https://drafts.csswg.org/css-inline-3/#baseline-tables>
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/base>
fn baseline_table_for_font(data: &[u8], face_index: u32) -> OpenTypeBaselineTable {
    let Ok(font) = FontRef::from_index(data, face_index) else {
        return OpenTypeBaselineTable::default();
    };
    let Ok(base) = font.base() else {
        return OpenTypeBaselineTable::default();
    };
    OpenTypeBaselineTable {
        horizontal: base
            .horiz_axis()
            .and_then(Result::ok)
            .map(parse_baseline_axis)
            .unwrap_or_default(),
        vertical: base
            .vert_axis()
            .and_then(Result::ok)
            .map(parse_baseline_axis)
            .unwrap_or_default(),
    }
}

fn parse_baseline_axis(axis: read_fonts::tables::base::Axis<'_>) -> OpenTypeBaselineAxis {
    let Some(Ok(tag_list)) = axis.base_tag_list() else {
        return OpenTypeBaselineAxis::default();
    };
    let Ok(script_list) = axis.base_script_list() else {
        return OpenTypeBaselineAxis::default();
    };
    let baseline_tags: Vec<_> = tag_list
        .baseline_tags()
        .iter()
        .map(|tag| tag.get().to_be_bytes())
        .collect();
    let mut scripts = Vec::new();
    for record in script_list.base_script_records() {
        let Ok(script) = record.base_script(script_list.offset_data()) else {
            continue;
        };
        let Some(Ok(values)) = script.base_values() else {
            continue;
        };
        let coordinates = values.base_coords();
        let mut parsed_coordinates = Vec::new();
        for (index, tag) in baseline_tags.iter().enumerate() {
            let Ok(coord) = coordinates.get(index) else {
                continue;
            };
            parsed_coordinates.push((*tag, parse_baseline_coordinate(coord)));
        }
        let default_baseline = baseline_tags
            .get(usize::from(values.default_baseline_index()))
            .copied();
        scripts.push(OpenTypeBaselineScript {
            script: record.base_script_tag().to_be_bytes(),
            default_baseline,
            coordinates: parsed_coordinates,
        });
    }
    OpenTypeBaselineAxis { scripts }
}

fn parse_baseline_coordinate(coord: BaseCoord<'_>) -> OpenTypeBaselineCoordinate {
    let variation_index = match coord {
        BaseCoord::Format3(ref format) => {
            format
                .device()
                .and_then(Result::ok)
                .and_then(|device| match device {
                    DeviceOrVariationIndex::VariationIndex(index) => Some(OpenTypeVariationIndex {
                        outer: index.delta_set_outer_index(),
                        inner: index.delta_set_inner_index(),
                    }),
                    DeviceOrVariationIndex::Device(_) => None,
                })
        }
        BaseCoord::Format1(_) | BaseCoord::Format2(_) => None,
    };
    OpenTypeBaselineCoordinate {
        design_units: coord.coordinate(),
        variation_index,
    }
}

fn apply_metric_overrides(metadata: &mut DocumentFontMetadata, face: &RegisteredFontFaceMetadata) {
    let units_per_em = f32::from(metadata.units_per_em);
    if let Some(value) = face.ascent_override {
        metadata.layout_metrics.ascender = metric_override_units(value, units_per_em);
    }
    if let Some(value) = face.descent_override {
        metadata.layout_metrics.descender = -metric_override_units(value, units_per_em);
    }
    if let Some(value) = face.line_gap_override {
        metadata.layout_metrics.line_gap = metric_override_units(value, units_per_em);
    }
}

fn metric_override_units(value: u32, units_per_em: f32) -> i16 {
    (f32::from_bits(value) * units_per_em)
        .round()
        .clamp(0.0, i16::MAX as f32) as i16
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod baseline_table_tests {
    use read_fonts::tables::variations::{DeltaSetIndex, ItemVariationStore};
    use read_fonts::types::F2Dot14;
    use read_fonts::{FontData, FontRead};

    use super::*;

    #[test]
    fn base_coord_formats_retain_design_coordinates_and_only_format_three_variations() {
        let format_1 = BaseCoord::read(FontData::new(&[0, 1, 0, 50])).unwrap();
        let format_2 = BaseCoord::read(FontData::new(&[0, 2, 0, 50, 0, 1, 0, 0])).unwrap();
        // Format 3's device offset resolves a VariationIndex with outer=1 and
        // inner=2. The delta-format high bit distinguishes it from a device
        // table. Device corrections in format 2 deliberately remain unused.
        let format_3 =
            BaseCoord::read(FontData::new(&[0, 3, 0, 50, 0, 6, 0, 1, 0, 2, 0x80, 0])).unwrap();

        assert_eq!(
            parse_baseline_coordinate(format_1),
            OpenTypeBaselineCoordinate {
                design_units: 50,
                variation_index: None,
            }
        );
        assert_eq!(
            parse_baseline_coordinate(format_2),
            OpenTypeBaselineCoordinate {
                design_units: 50,
                variation_index: None,
            }
        );
        assert_eq!(
            parse_baseline_coordinate(format_3),
            OpenTypeBaselineCoordinate {
                design_units: 50,
                variation_index: Some(OpenTypeVariationIndex { outer: 1, inner: 2 }),
            }
        );
    }

    #[test]
    fn base_axis_preserves_script_default_and_tagged_coordinates() {
        // Axis → BaseTagList(`romn`, `hang`) → BaseScriptList(`DFLT`) →
        // BaseValues. This is intentionally a table-local fixture: it keeps
        // parser coverage in the repository without depending on a host font.
        let bytes = [
            0, 4, 0, 14, // Axis offsets
            0, 2, b'r', b'o', b'm', b'n', b'h', b'a', b'n', b'g', // BaseTagList
            0, 1, b'D', b'F', b'L', b'T', 0, 8, // BaseScriptList
            0, 6, 0, 0, 0, 0, // BaseScript
            0, 0, 0, 2, 0, 8, 0, 12, // BaseValues
            0, 1, 0, 50, // `romn` BaseCoord format 1
            0, 2, 2, 138, 0, 1, 0, 0, // `hang` BaseCoord format 2
        ];
        let axis = read_fonts::tables::base::Axis::read(FontData::new(&bytes)).unwrap();

        assert_eq!(
            parse_baseline_axis(axis),
            OpenTypeBaselineAxis {
                scripts: vec![OpenTypeBaselineScript {
                    script: *b"DFLT",
                    default_baseline: Some(*b"romn"),
                    coordinates: vec![
                        (
                            *b"romn",
                            OpenTypeBaselineCoordinate {
                                design_units: 50,
                                variation_index: None,
                            },
                        ),
                        (
                            *b"hang",
                            OpenTypeBaselineCoordinate {
                                design_units: 650,
                                variation_index: None,
                            },
                        ),
                    ],
                }],
            }
        );
    }

    #[test]
    fn format_three_variation_index_selects_the_normalized_instance_delta() {
        // A one-axis ItemVariationStore: its only region rises from zero to
        // one and contributes +50 design units at normalized coordinate 1.
        // This is the delta store addressed by the Format 3 coordinate above.
        let store = ItemVariationStore::read(FontData::new(&[
            0, 1, // format
            0, 0, 0, 12, // VariationRegionList offset
            0, 1, // ItemVariationData count
            0, 0, 0, 22, // ItemVariationData offset
            0, 1, 0, 1, // one axis, one region
            0, 0, 0x40, 0, 0x40, 0, // start=0, peak=1, end=1
            0, 1, 0, 1, 0, 1, // one item, one word delta, one region
            0, 0, // region index
            0, 50, // delta set 0
        ]))
        .unwrap();
        let delta = DeltaSetIndex { outer: 0, inner: 0 };
        assert_eq!(store.compute_delta(delta, &[F2Dot14::ZERO]).unwrap(), 0);
        assert_eq!(store.compute_delta(delta, &[F2Dot14::ONE]).unwrap(), 50);
    }
}

fn face_has_color_presentation_tables(face: &ttf_parser::Face<'_>) -> bool {
    // Use raw table presence rather than parsed table availability. A color
    // table may be valid for a PDF consumer but not expose a high-level
    // `ttf-parser` representation; it still establishes the face as a
    // color-presentation candidate for CSS font fallback.
    let raw = face.raw_face();
    [b"COLR", b"CBDT", b"CBLC", b"sbix", b"SVG "]
        .into_iter()
        .any(|tag| raw.table(ttf_parser::Tag::from_bytes(tag)).is_some())
}
