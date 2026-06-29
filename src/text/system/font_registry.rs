use super::font_loading::post_script_name_for_face;
use super::*;

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
    ascender: i16,
    descender: i16,
    cap_height: i16,
    italic_angle: i16,
    bbox: [i16; 4],
}

impl DocumentFontRegistry {
    pub(super) fn new(
        registered_font_faces: HashMap<FontBlobFaceKey, RegisteredFontFaceMetadata>,
    ) -> Self {
        Self {
            fonts: Vec::new(),
            registered_font_faces,
            font_cache: HashMap::new(),
            font_blob_cache: HashMap::new(),
            parley_font_cache: HashMap::new(),
        }
    }

    pub(super) fn into_fonts(self) -> Vec<DocumentFont> {
        self.fonts
    }

    pub(super) fn get(&self, font_id: usize) -> Option<&DocumentFont> {
        self.fonts.get(font_id)
    }

    pub(super) fn font_has_character(&self, font_id: usize, character: char) -> bool {
        self.get(font_id)
            .is_some_and(|font| self.font_allows_character(font, character))
    }

    pub(super) fn font_has_unicode_range(&self, font_id: usize) -> bool {
        self.get(font_id)
            .and_then(|font| self.metadata_for_document_font(font))
            .is_some_and(|metadata| metadata.unicode_range.is_some())
    }

    pub(super) fn font_allows_character(&self, font: &DocumentFont, character: char) -> bool {
        font_has_character(font, character) && self.font_face_allows_character(font, character)
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
        self.registered_font_faces.get(&FontBlobFaceKey {
            blob_id: font.data.blob_id(),
            face_index: font.face_index,
        })
    }

    pub(super) fn font_query_has_character(font: &FontiqueQueryFont, character: char) -> bool {
        if standalone_font_program_kind(font.blob.as_ref()).is_none() {
            return false;
        }
        if font
            .charmap()
            .is_some_and(|charmap| charmap.map(character).is_some())
        {
            return true;
        }
        ttf_parser::Face::parse(font.blob.as_ref(), font.index)
            .ok()
            .and_then(|face| face.glyph_index(character))
            .is_some()
    }

    pub(super) fn document_font_from_query(
        &mut self,
        collection: &mut fontique::Collection,
        font: FontiqueQueryFont,
        family_override: Option<&str>,
        request: &FontRequest,
    ) -> Option<usize> {
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
            family_label: Some(family.clone()),
            request: Some(request.clone()),
        };
        if let Some(id) = self.font_blob_cache.get(&resolved_key) {
            return Some(*id);
        }

        let metadata = document_font_metadata(
            data,
            font.index,
            family,
            font.synthesis.embolden() || request.attributes.weight >= FontWeight::BOLD.0,
            font.synthesis.skew().is_some() || request.attributes.style != 0,
        )?;
        let id = self.push_document_font(metadata, font.blob, font.index);
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
        let face_key = FontBlobFaceKey {
            blob_id: font_data.data.id(),
            face_index: font_data.index,
        };
        let data = font_data.data.as_ref();
        let face = ttf_parser::Face::parse(data, font_data.index).ok()?;
        let family = self
            .registered_font_faces
            .get(&face_key)
            .map(|metadata| metadata.family.clone())
            .or_else(|| opentype_name(&face, ttf_parser::name_id::TYPOGRAPHIC_FAMILY))
            .or_else(|| opentype_name(&face, ttf_parser::name_id::FAMILY))
            .or_else(|| opentype_name(&face, ttf_parser::name_id::POST_SCRIPT_NAME))
            .unwrap_or_else(|| format!("FontBlob-{}", font_data.data.id()));
        let resolved_key = ResolvedFontFaceKey {
            blob_id: font_data.data.id(),
            face_index: font_data.index,
            family_label: Some(family.clone()),
            request: None,
        };
        if let Some(id) = self.font_blob_cache.get(&resolved_key) {
            return Some(*id);
        }

        let metadata = document_font_metadata(data, font_data.index, family, false, false)?;
        let id = self.push_document_font(metadata, font_data.data.clone(), font_data.index);
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
            .copied()
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

    fn push_document_font(
        &mut self,
        metadata: DocumentFontMetadata,
        blob: FontiqueBlob<u8>,
        face_index: u32,
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
            ascender: metadata.ascender,
            descender: metadata.descender,
            cap_height: metadata.cap_height,
            italic_angle: metadata.italic_angle,
            bbox: metadata.bbox,
        });
        id
    }
}

fn document_font_metadata(
    data: &[u8],
    face_index: u32,
    family: String,
    synthesize_bold: bool,
    synthesize_italic: bool,
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
    Some(DocumentFontMetadata {
        program_kind,
        family,
        post_script_name,
        units_per_em: face.units_per_em(),
        ascender: face.ascender(),
        descender: face.descender(),
        cap_height: face.capital_height().unwrap_or_else(|| face.ascender()),
        italic_angle: face.italic_angle().round() as i16,
        bbox,
    })
}

fn font_label_looks_emoji(font: &DocumentFont) -> bool {
    let family = font.family.to_ascii_lowercase();
    let post_script_name = font.post_script_name.to_ascii_lowercase();
    family.contains("emoji") || post_script_name.contains("emoji")
}
