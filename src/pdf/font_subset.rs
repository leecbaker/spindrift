use super::*;

pub(super) fn subset_font_file(
    font: &DocumentFont,
    used_glyphs: &BTreeMap<u16, String>,
    _profile: PdfFontValidationProfile,
) -> FontFilePlan {
    let original = font.data.as_ref();
    let source_program_kind =
        font_program_kind_from_data(original, font.face_index).unwrap_or(font.program_kind);
    let is_collection = matches!(original.get(..4), Some(b"ttcf") | Some(b"OTTC"));
    let fallback_len = if is_collection {
        match collection_face_size(original, font.face_index) {
            Some(size) => size,
            None => {
                return FontFilePlan::rejected(
                    format!(
                        "failed to inspect face {} from font collection",
                        font.face_index
                    ),
                    source_program_kind,
                    identity_glyph_mapping(used_glyphs),
                );
            }
        }
    } else if font.face_index != 0 {
        return fallback_after_subset_failure(
            &FallbackContext { font, used_glyphs },
            format!(
                "standalone font has unsupported face index {}",
                font.face_index
            ),
        );
    } else {
        original.len()
    };
    let source_glyphs = used_glyphs.keys().cloned().collect::<Vec<_>>();
    let remapper = subsetter::GlyphRemapper::new_from_glyphs_sorted(&source_glyphs);
    let source_gid_to_cid = compact_glyph_mapping(used_glyphs, &remapper);
    let fallback = FallbackContext { font, used_glyphs };
    let Some(source_gid_to_cid) = source_gid_to_cid else {
        return fallback_after_subset_failure(
            &fallback,
            "subsetter did not map every emitted glyph".to_string(),
        );
    };

    match subsetter::subset(original, font.face_index, &remapper) {
        Ok(data)
            if subset_font_is_valid(font, &data, &source_gid_to_cid)
                && data.len() < fallback_len =>
        {
            let program_kind = font_program_kind_from_data(&data, 0).unwrap_or(source_program_kind);
            FontFilePlan {
                data,
                embedding_kind: FontEmbeddingKind::SubsetCompactGids,
                fallback_reason: None,
                program_kind,
                source_gid_to_cid,
            }
        }
        Ok(data) if !subset_font_is_valid(font, &data, &source_gid_to_cid) => {
            fallback_after_subset_failure(
                &fallback,
                format!(
                    "subsetter output failed validation ({} byte(s))",
                    data.len()
                ),
            )
        }
        Ok(data) => fallback_after_subset_failure(
            &fallback,
            format!(
                "subsetter output was not smaller than original ({} >= {} byte(s))",
                data.len(),
                fallback_len
            ),
        ),
        Err(error) => {
            fallback_after_subset_failure(&fallback, format!("subsetter failed: {error}"))
        }
    }
}

struct FallbackContext<'a> {
    font: &'a DocumentFont,
    used_glyphs: &'a BTreeMap<u16, String>,
}

fn fallback_after_subset_failure(context: &FallbackContext<'_>, reason: String) -> FontFilePlan {
    let mut full = full_font_file(context.font, context.used_glyphs);
    if !matches!(full.embedding_kind, FontEmbeddingKind::Rejected { .. }) {
        full.fallback_reason = Some(reason);
    }
    full
}

/// Plans a complete font program with identity source-GID/CID mapping.
///
/// PDF CIDFontType2 resources address TrueType glyphs with an identity
/// `CIDToGIDMap`; a selected collection face must first be reconstructed as a
/// standalone SFNT. ISO 32000-2:2020, 9.7.4.3 and 9.9.2.
pub(super) fn full_font_file(
    font: &DocumentFont,
    used_glyphs: &BTreeMap<u16, String>,
) -> FontFilePlan {
    let original = font.data.as_ref();
    let source_program_kind =
        font_program_kind_from_data(original, font.face_index).unwrap_or(font.program_kind);
    let mut source_gid_to_cid = full_identity_glyph_mapping(font).unwrap_or_default();
    // A malformed font can report fewer glyphs than the shaping engine emits.
    // Keep every painted source GID addressable so the PDF text stream and
    // ToUnicode CMap stay synchronized even in that degraded case.
    for glyph_id in used_glyphs.keys() {
        source_gid_to_cid.entry(*glyph_id).or_insert(*glyph_id);
    }
    let is_collection = matches!(original.get(..4), Some(b"ttcf") | Some(b"OTTC"));
    let (data, embedding_kind) = if is_collection {
        match extract_collection_face(original, font.face_index) {
            Some(extracted) => (extracted, FontEmbeddingKind::ExtractedCollectionFace),
            None => {
                return FontFilePlan::rejected(
                    format!(
                        "failed to extract face {} from font collection",
                        font.face_index
                    ),
                    source_program_kind,
                    source_gid_to_cid,
                );
            }
        }
    } else if font.face_index != 0 {
        return FontFilePlan::rejected(
            format!(
                "standalone font has unsupported face index {}",
                font.face_index
            ),
            source_program_kind,
            source_gid_to_cid,
        );
    } else {
        (original.to_vec(), FontEmbeddingKind::FullStandaloneFont)
    };

    FontFilePlan {
        data,
        embedding_kind,
        fallback_reason: None,
        program_kind: source_program_kind,
        source_gid_to_cid,
    }
}

fn font_program_kind_from_data(font_data: &[u8], face_index: u32) -> Option<FontProgramKind> {
    let face = ttf_parser::Face::parse(font_data, face_index).ok()?;
    if face
        .raw_face()
        .table(ttf_parser::Tag::from_bytes(b"CFF "))
        .is_some()
    {
        Some(FontProgramKind::OpenTypeCff)
    } else if face
        .raw_face()
        .table(ttf_parser::Tag::from_bytes(b"glyf"))
        .is_some()
    {
        Some(FontProgramKind::TrueType)
    } else {
        None
    }
}

/// Build a deterministic mapping from shaped source GIDs to dense PDF CIDs.
///
/// `subsetter` uses the remapped GID as the CID in the CIDFont program; see
/// ISO 32000-2:2020, 9.7.6, "CIDFont Type 2", and 9.9.2, "CIDFont Type 0".
fn compact_glyph_mapping(
    used_glyphs: &BTreeMap<u16, String>,
    remapper: &subsetter::GlyphRemapper,
) -> Option<BTreeMap<u16, u16>> {
    used_glyphs
        .keys()
        .cloned()
        .map(|source_gid| remapper.get(source_gid).map(|cid| (source_gid, cid)))
        .collect()
}

fn subset_font_is_valid(
    font: &DocumentFont,
    subset_data: &[u8],
    source_gid_to_cid: &BTreeMap<u16, u16>,
) -> bool {
    let Ok(face) = ttf_parser::Face::parse(subset_data, 0) else {
        return false;
    };
    if face.units_per_em() != font.units_per_em {
        return false;
    }
    source_gid_to_cid
        .values()
        .all(|cid| face.glyph_hor_advance(ttf_parser::GlyphId(*cid)).is_some())
}

pub(super) fn identity_glyph_mapping(used_glyphs: &BTreeMap<u16, String>) -> BTreeMap<u16, u16> {
    used_glyphs
        .keys()
        .map(|glyph_id| (*glyph_id, *glyph_id))
        .collect()
}

fn full_identity_glyph_mapping(font: &DocumentFont) -> Option<BTreeMap<u16, u16>> {
    ttf_parser::Face::parse(font.data.as_ref(), font.face_index)
        .ok()
        .map(|face| {
            (0..face.number_of_glyphs())
                .map(|glyph_id| (glyph_id, glyph_id))
                .collect()
        })
}

/// Return the byte length of a standalone SFNT reconstructed from one TTC
/// face without copying its tables.  Successful subsetting only needs this
/// comparison length; allocating the full face before `subsetter` has decided
/// whether it is necessary doubles peak memory for large collections.
fn collection_face_size(font_data: &[u8], face_index: u32) -> Option<usize> {
    if !matches!(font_data.get(..4), Some(b"ttcf") | Some(b"OTTC")) {
        return None;
    }
    let face_count = read_u32(font_data, 8)?;
    if face_index >= face_count {
        return None;
    }
    let offset_entry = 12usize.checked_add(usize::try_from(face_index).ok()?.checked_mul(4)?)?;
    let face_offset = usize::try_from(read_u32(font_data, offset_entry)?).ok()?;
    let num_tables = usize::from(read_u16(font_data, face_offset.checked_add(4)?)?);
    let directory_len = 12usize.checked_add(num_tables.checked_mul(16)?)?;
    font_data.get(face_offset..face_offset.checked_add(directory_len)?)?;

    let mut next_table_offset = align4(directory_len)?;
    for index in 0..num_tables {
        let record_offset = face_offset
            .checked_add(12)?
            .checked_add(index.checked_mul(16)?)?;
        let old_offset =
            usize::try_from(read_u32(font_data, record_offset.checked_add(8)?)?).ok()?;
        let length = usize::try_from(read_u32(font_data, record_offset.checked_add(12)?)?).ok()?;
        font_data.get(old_offset..old_offset.checked_add(length)?)?;
        next_table_offset = align4(next_table_offset.checked_add(length)?)?;
    }
    Some(next_table_offset)
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
    pub(super) program_kind: FontProgramKind,
    pub(super) source_gid_to_cid: BTreeMap<u16, u16>,
}

impl FontFilePlan {
    pub(super) fn rejected(
        reason: String,
        program_kind: FontProgramKind,
        source_gid_to_cid: BTreeMap<u16, u16>,
    ) -> Self {
        Self {
            data: Vec::new(),
            embedding_kind: FontEmbeddingKind::Rejected {
                reason: reason.clone(),
            },
            fallback_reason: Some(reason),
            program_kind,
            source_gid_to_cid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_collection_face_without_reconstructing_it() {
        let mut collection = vec![0_u8; 68];
        collection[..4].copy_from_slice(b"ttcf");
        collection[8..12].copy_from_slice(&1_u32.to_be_bytes());
        collection[12..16].copy_from_slice(&16_u32.to_be_bytes());
        collection[16..20].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
        collection[20..22].copy_from_slice(&1_u16.to_be_bytes());
        collection[28..32].copy_from_slice(b"test");
        collection[36..40].copy_from_slice(&64_u32.to_be_bytes());
        collection[40..44].copy_from_slice(&4_u32.to_be_bytes());

        assert_eq!(collection_face_size(&collection, 0), Some(32));
        let extracted = extract_collection_face(&collection, 0).unwrap();
        assert_eq!(extracted.len(), 32);
    }
}
