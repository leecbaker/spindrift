use super::font_loading::post_script_name_for_face;
use super::*;
use crate::document::{CssFontVerticalMetrics, OpenTypeVerticalMetrics};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FontSupportKind {
    EmbeddableText,
    ColorOrEmojiOnlyFallback,
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
    size_adjust: Option<f32>,
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

    pub(super) fn selected_face_variations(&self, font_id: usize) -> Option<FontVariationSettings> {
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

    pub(super) fn document_font_from_query(
        &mut self,
        collection: &mut fontique::Collection,
        font: FontiqueQueryFont,
        family_override: Option<&str>,
        request: &FontRequest,
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
            font.synthesis.embolden() || request.attributes.weight >= FontWeight::BOLD.0,
            font.synthesis.skew().is_some() || request.attributes.style != 0,
            size_adjust,
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
        };
        if let Some(id) = self.font_blob_cache.get(&resolved_key) {
            return Some(*id);
        }

        let metadata = document_font_metadata(data, font_data.index, family, false, false, None)?;
        let id = self.push_document_font(metadata, font_data.data.clone(), font_data.index, None);
        self.font_blob_cache.insert(resolved_key, id);
        Some(id)
    }

    pub(super) fn cached_parley_font(
        &self,
        font_data: &parley::FontData,
        request: &FontRequest,
    ) -> Option<usize> {
        self.parley_font_cache
            .get(&ParleyFontRequestKey {
                blob_id: font_data.data.id(),
                face_index: font_data.index,
                request: request.clone(),
            })
            .cloned()
    }

    pub(super) fn cache_parley_font(
        &mut self,
        font_data: &parley::FontData,
        request: &FontRequest,
        font_id: usize,
    ) {
        self.parley_font_cache.insert(
            ParleyFontRequestKey {
                blob_id: font_data.data.id(),
                face_index: font_data.index,
                request: request.clone(),
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

        let visible_glyphs = text
            .chars()
            .filter(|character| !character_is_default_ignorable_code_point(*character))
            .filter_map(|character| face.glyph_index(character))
            .collect::<Vec<_>>();
        if visible_glyphs.is_empty() {
            return FontSupportKind::EmbeddableText;
        }

        let has_text_outline = visible_glyphs
            .iter()
            .any(|glyph| face.glyph_bounding_box(*glyph).is_some());
        let has_color_glyph = visible_glyphs
            .iter()
            .any(|glyph| face.is_color_glyph(*glyph));
        if !has_text_outline || has_color_glyph || font_label_looks_emoji(font) {
            FontSupportKind::ColorOrEmojiOnlyFallback
        } else {
            FontSupportKind::EmbeddableText
        }
    }

    /// Whether a run contains a COLR glyph that the layout stage can paint as
    /// explicit colored outline layers rather than replacing with fallback.
    pub(super) fn run_has_color_glyph(&self, font_id: usize, text: &str) -> bool {
        let Some(font) = self.get(font_id) else {
            return false;
        };
        let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
            return false;
        };
        text.chars().any(|character| {
            face.glyph_index(character)
                .is_some_and(|glyph| face.is_color_glyph(glyph))
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
    synthesize_italic: bool,
    size_adjust: Option<f32>,
) -> Option<DocumentFontMetadata> {
    let program_kind = standalone_font_program_kind(data)?;
    let face = ttf_parser::Face::parse(data, face_index).ok()?;
    let post_script_name =
        post_script_name_for_face(&face, &family, synthesize_bold, synthesize_italic);
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
        size_adjust,
    })
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

fn font_label_looks_emoji(font: &DocumentFont) -> bool {
    let family = font.family.to_ascii_lowercase();
    let post_script_name = font.post_script_name.to_ascii_lowercase();
    family.contains("emoji") || post_script_name.contains("emoji")
}
