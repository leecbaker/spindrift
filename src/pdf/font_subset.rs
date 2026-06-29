use super::*;

pub(super) fn subset_font_file(
    font: &DocumentFont,
    used_glyphs: &BTreeMap<u16, String>,
    profile: PdfFontValidationProfile,
) -> FontFilePlan {
    let original = font.data.as_ref();
    let is_collection = matches!(original.get(..4), Some(b"ttcf") | Some(b"OTTC"));
    let (source, fallback_kind) = if is_collection {
        match extract_collection_face(original, font.face_index) {
            Some(extracted) => (extracted, FontEmbeddingKind::ExtractedCollectionFace),
            None => {
                return FontFilePlan::rejected(format!(
                    "failed to extract face {} from font collection",
                    font.face_index
                ));
            }
        }
    } else if font.face_index != 0 {
        return fallback_or_reject(
            original,
            profile,
            FontEmbeddingKind::FullStandaloneFont,
            format!(
                "standalone font has unsupported face index {}",
                font.face_index
            ),
        );
    } else {
        (original.to_vec(), FontEmbeddingKind::FullStandaloneFont)
    };
    let source = source.as_slice();

    let subset = subset_font_file_with_retained_gids(source, used_glyphs);
    match subset {
        Ok(data) if subset_font_is_valid(font, &data, used_glyphs) && data.len() < source.len() => {
            FontFilePlan {
                data,
                embedding_kind: FontEmbeddingKind::SubsetRetainedGids,
                fallback_reason: None,
            }
        }
        Ok(data) if !subset_font_is_valid(font, &data, used_glyphs) => fallback_or_reject(
            source,
            profile,
            fallback_kind,
            format!(
                "fontcull subset output failed validation ({} byte(s))",
                data.len()
            ),
        ),
        Ok(data) => fallback_or_reject(
            source,
            profile,
            fallback_kind,
            format!(
                "fontcull subset was not smaller than original ({} >= {} byte(s))",
                data.len(),
                source.len()
            ),
        ),
        Err(error) => fallback_or_reject(
            source,
            profile,
            fallback_kind,
            format!("fontcull subset failed: {error}"),
        ),
    }
}

fn fallback_or_reject(
    font_data: &[u8],
    profile: PdfFontValidationProfile,
    embedding_kind: FontEmbeddingKind,
    reason: String,
) -> FontFilePlan {
    if profile.allows_full_font_fallback() {
        FontFilePlan::fallback(font_data, embedding_kind, reason)
    } else {
        FontFilePlan::rejected(reason)
    }
}

fn subset_font_file_with_retained_gids(
    font_data: &[u8],
    used_glyphs: &BTreeMap<u16, String>,
) -> Result<Vec<u8>, fontcull_klippa::SubsetError> {
    use fontcull_klippa::{DEFAULT_LAYOUT_FEATURES, Plan, SubsetFlags, subset_font};
    use fontcull_write_fonts::read::FontRef;
    use fontcull_write_fonts::read::collections::IntSet;
    use fontcull_write_fonts::types::{GlyphId, NameId, Tag};

    let font = FontRef::new(font_data)
        .map_err(|error| fontcull_klippa::SubsetError::InvalidId(format!("{error:?}")))?;
    let mut gids = IntSet::<GlyphId>::empty();
    gids.insert(GlyphId::NOTDEF);
    for glyph_id in used_glyphs.keys() {
        gids.insert(GlyphId::from(*glyph_id as u32));
    }

    let mut unicodes = IntSet::<u32>::empty();
    for unicode in used_glyphs.values() {
        for character in unicode.chars() {
            unicodes.insert(character as u32);
        }
    }

    let drop_tables = IntSet::<Tag>::empty();
    let layout_scripts = IntSet::<Tag>::all();
    let layout_features = IntSet::<Tag>::from_iter(DEFAULT_LAYOUT_FEATURES.iter().copied());
    let name_ids = IntSet::<NameId>::empty();
    let name_languages = IntSet::<u16>::empty();
    let plan = Plan::new(
        &gids,
        &unicodes,
        &font,
        SubsetFlags::SUBSET_FLAGS_RETAIN_GIDS,
        &drop_tables,
        &layout_scripts,
        &layout_features,
        &name_ids,
        &name_languages,
    );
    subset_font(&font, &plan)
}

fn subset_font_is_valid(
    font: &DocumentFont,
    subset_data: &[u8],
    used_glyphs: &BTreeMap<u16, String>,
) -> bool {
    let Ok(face) = ttf_parser::Face::parse(subset_data, 0) else {
        return false;
    };
    if face.units_per_em() != font.units_per_em {
        return false;
    }
    used_glyphs.keys().all(|glyph_id| {
        face.glyph_hor_advance(ttf_parser::GlyphId(*glyph_id))
            .is_some()
    })
}

fn extract_collection_face(font_data: &[u8], face_index: u32) -> Option<Vec<u8>> {
    if !matches!(font_data.get(..4), Some(b"ttcf") | Some(b"OTTC")) {
        return None;
    }
    let face_count = read_u32(font_data, 8)?;
    if face_index >= face_count {
        return None;
    }
    let offset_entry = 12usize.checked_add(usize::try_from(face_index).ok()?.checked_mul(4)?)?;
    let face_offset = usize::try_from(read_u32(font_data, offset_entry)?).ok()?;
    let sfnt_version = font_data.get(face_offset..face_offset.checked_add(4)?)?;
    let num_tables = read_u16(font_data, face_offset.checked_add(4)?)?;
    let table_count = usize::from(num_tables);
    let directory_len = 12usize.checked_add(table_count.checked_mul(16)?)?;
    font_data.get(face_offset..face_offset.checked_add(directory_len)?)?;

    let mut next_table_offset = align4(directory_len)?;
    let mut records = Vec::with_capacity(table_count);
    for index in 0..table_count {
        let record_offset = face_offset
            .checked_add(12)?
            .checked_add(index.checked_mul(16)?)?;
        let tag = font_data.get(record_offset..record_offset.checked_add(4)?)?;
        let checksum = read_u32(font_data, record_offset.checked_add(4)?)?;
        let old_offset =
            usize::try_from(read_u32(font_data, record_offset.checked_add(8)?)?).ok()?;
        let length = usize::try_from(read_u32(font_data, record_offset.checked_add(12)?)?).ok()?;
        font_data.get(old_offset..old_offset.checked_add(length)?)?;
        records.push((
            [tag[0], tag[1], tag[2], tag[3]],
            checksum,
            old_offset,
            length,
            next_table_offset,
        ));
        next_table_offset = align4(next_table_offset.checked_add(length)?)?;
    }

    let mut standalone = vec![0u8; next_table_offset];
    standalone[0..4].copy_from_slice(sfnt_version);
    standalone[4..6].copy_from_slice(&num_tables.to_be_bytes());
    let search_range = read_u16(font_data, face_offset.checked_add(6)?)?;
    let entry_selector = read_u16(font_data, face_offset.checked_add(8)?)?;
    let range_shift = read_u16(font_data, face_offset.checked_add(10)?)?;
    standalone[6..8].copy_from_slice(&search_range.to_be_bytes());
    standalone[8..10].copy_from_slice(&entry_selector.to_be_bytes());
    standalone[10..12].copy_from_slice(&range_shift.to_be_bytes());

    for (index, (tag, checksum, old_offset, length, new_offset)) in records.into_iter().enumerate()
    {
        let record_offset = 12 + index * 16;
        standalone[record_offset..record_offset + 4].copy_from_slice(&tag);
        standalone[record_offset + 4..record_offset + 8].copy_from_slice(&checksum.to_be_bytes());
        standalone[record_offset + 8..record_offset + 12]
            .copy_from_slice(&(new_offset as u32).to_be_bytes());
        standalone[record_offset + 12..record_offset + 16]
            .copy_from_slice(&(length as u32).to_be_bytes());
        standalone[new_offset..new_offset + length]
            .copy_from_slice(&font_data[old_offset..old_offset + length]);
    }
    Some(standalone)
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|value| value & !3)
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct FontFilePlan {
    pub(super) data: Vec<u8>,
    pub(super) embedding_kind: FontEmbeddingKind,
    pub(super) fallback_reason: Option<String>,
}

impl FontFilePlan {
    pub(super) fn fallback(
        font_data: &[u8],
        embedding_kind: FontEmbeddingKind,
        reason: String,
    ) -> Self {
        Self {
            data: font_data.to_vec(),
            embedding_kind,
            fallback_reason: Some(reason),
        }
    }

    pub(super) fn rejected(reason: String) -> Self {
        Self {
            data: Vec::new(),
            embedding_kind: FontEmbeddingKind::Rejected {
                reason: reason.clone(),
            },
            fallback_reason: Some(reason),
        }
    }
}
