use super::*;

pub(super) fn embedded_font_plans<'a>(
    document: &'a Document,
    shaped_document: &ShapedDocument,
    first_embedded_font_id: usize,
) -> EmbeddedFontPlans<'a> {
    let mut used_glyphs = vec![BTreeMap::<u16, String>::new(); document.fonts.len()];
    for (page_index, page) in shaped_document.pages.iter().enumerate() {
        for (line_index, shaped_line) in page.iter().enumerate() {
            let Some(shaped_line) = shaped_line else {
                continue;
            };
            for run in &shaped_line.runs {
                let Some(font_glyphs) = used_glyphs.get_mut(run.document_font_id) else {
                    continue;
                };
                for glyph in &run.glyphs {
                    font_glyphs
                        .entry(glyph.id)
                        .or_insert_with(|| glyph.unicode.clone());
                }
                if run.glyphs.is_empty()
                    && let Some(line) = document
                        .pages
                        .get(page_index)
                        .and_then(|page| page.lines.get(line_index))
                {
                    log::debug!("empty shaped text line {:?}", line.text);
                }
            }
        }
    }

    let mut fonts = Vec::<EmbeddedFontPlan<'_>>::new();
    let mut document_font_to_embedded_font = vec![None; document.fonts.len()];
    let mut key_to_embedded_font = HashMap::<EmbeddedFontKey, usize>::new();
    let mut pruned_fonts = 0usize;
    let mut duplicate_fonts = 0usize;

    for (document_font_id, font) in document.fonts.iter().enumerate() {
        let font_glyphs = used_glyphs
            .get(document_font_id)
            .cloned()
            .unwrap_or_default();
        if font_glyphs.is_empty() {
            pruned_fonts += 1;
            continue;
        }

        let key = embedded_font_key(font);
        if let Some(index) = key_to_embedded_font.get(&key).copied() {
            duplicate_fonts += 1;
            let embedded_font = &mut fonts[index];
            for (glyph_id, unicode) in font_glyphs {
                embedded_font.used_glyphs.entry(glyph_id).or_insert(unicode);
            }
            document_font_to_embedded_font[document_font_id] = Some(index);
            continue;
        }

        let index = fonts.len();
        let base_id = first_embedded_font_id + index * EMBEDDED_FONT_OBJECTS;
        key_to_embedded_font.insert(key, index);
        document_font_to_embedded_font[document_font_id] = Some(index);
        fonts.push(EmbeddedFontPlan {
            font,
            resource_name: format!("RF{}", index + 1),
            base_name: format!("REASYP+{}", font.post_script_name),
            type0_id: base_id,
            cid_font_id: base_id + 1,
            descriptor_id: base_id + 2,
            file_id: base_id + 3,
            to_unicode_id: base_id + 4,
            used_glyphs: font_glyphs,
        });
    }

    log::debug!(
        "planned PDF font embedding: {} original font(s), {} used font(s), {} unique embedded font(s), {} pruned, {} duplicate(s) merged",
        document.fonts.len(),
        document.fonts.len().saturating_sub(pruned_fonts),
        fonts.len(),
        pruned_fonts,
        duplicate_fonts
    );

    EmbeddedFontPlans {
        fonts,
        document_font_to_embedded_font,
    }
}

fn embedded_font_key(font: &DocumentFont) -> EmbeddedFontKey {
    EmbeddedFontKey {
        blob_id: font.data.blob_id(),
        face_index: font.face_index,
        program_kind: font.program_kind,
        post_script_name: font.post_script_name.clone(),
        units_per_em: font.units_per_em,
        ascender: font.ascender,
        descender: font.descender,
        cap_height: font.cap_height,
        italic_angle: font.italic_angle,
        bbox: font.bbox,
    }
}

pub(super) fn font_resource_dictionary(embedded_fonts: &[EmbeddedFontPlan<'_>]) -> String {
    let mut dictionary = String::from("<<");
    for font in embedded_fonts {
        dictionary.push_str(&format!(" /{} {} 0 R", font.resource_name, font.type0_id));
    }
    dictionary.push_str(" >>\n");
    dictionary
}

pub(super) fn embedded_type0_font_object(font: &EmbeddedFontPlan<'_>) -> String {
    format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont /{} /Encoding /Identity-H /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>\n",
        font.base_name, font.cid_font_id, font.to_unicode_id
    )
}

pub(super) fn embedded_cid_font_object(font: &EmbeddedFontPlan<'_>) -> String {
    let subtype = match font.font.program_kind {
        FontProgramKind::TrueType => "CIDFontType2",
        FontProgramKind::OpenTypeCff => "CIDFontType0",
    };
    format!(
        "<< /Type /Font /Subtype /{} /BaseFont /{} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {} 0 R /CIDToGIDMap /Identity /W [{}] >>\n",
        subtype,
        font.base_name,
        font.descriptor_id,
        cid_widths(font)
    )
}

pub(super) fn cid_widths(font: &EmbeddedFontPlan<'_>) -> String {
    let Ok(face) = ttf_parser::Face::parse(&font.font.data, font.font.face_index) else {
        return String::new();
    };
    let units_per_em = font.font.units_per_em.max(1) as f32;
    font.used_glyphs
        .keys()
        .map(|glyph_id| {
            let width = face
                .glyph_hor_advance(ttf_parser::GlyphId(*glyph_id))
                .map(|width| (width as f32 * 1000.0 / units_per_em).round() as i32)
                .unwrap_or(0);
            format!("{glyph_id} [{width}]")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn embedded_font_descriptor_object(font: &EmbeddedFontPlan<'_>) -> String {
    let units_per_em = font.font.units_per_em.max(1) as f32;
    let scale = |value: i16| (value as f32 * 1000.0 / units_per_em).round() as i32;
    let bbox = font_descriptor_bbox(font, &scale);
    let font_file_key = match font.font.program_kind {
        FontProgramKind::TrueType => "FontFile2",
        FontProgramKind::OpenTypeCff => "FontFile3",
    };
    format!(
        "<< /Type /FontDescriptor /FontName /{} /Flags {} /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV 80 /{} {} 0 R >>\n",
        font.base_name,
        font_descriptor_flags(font.font),
        bbox[0],
        bbox[1],
        bbox[2],
        bbox[3],
        font.font.italic_angle,
        scale(font.font.ascender),
        scale(font.font.descender),
        bbox[3],
        font_file_key,
        font.file_id
    )
}

pub(super) fn font_descriptor_bbox<F>(font: &EmbeddedFontPlan<'_>, scale: &F) -> [i32; 4]
where
    F: Fn(i16) -> i32,
{
    let ascent = scale(font.font.ascender);
    let descent = scale(font.font.descender);
    let max_width = max_used_glyph_width(font).unwrap_or_else(|| scale(font.font.bbox[2]));
    [0, descent, max_width, ascent]
}

pub(super) fn max_used_glyph_width(font: &EmbeddedFontPlan<'_>) -> Option<i32> {
    let face = ttf_parser::Face::parse(&font.font.data, font.font.face_index).ok()?;
    let units_per_em = font.font.units_per_em.max(1) as f32;
    font.used_glyphs
        .keys()
        .filter_map(|glyph_id| {
            face.glyph_hor_advance(ttf_parser::GlyphId(*glyph_id))
                .map(|width| (width as f32 * 1000.0 / units_per_em).round() as i32)
        })
        .max()
}

pub(super) fn font_descriptor_flags(font: &DocumentFont) -> u32 {
    let mut flags = 32;
    if font.family.to_ascii_lowercase().contains("courier")
        || font.family.to_ascii_lowercase().contains("mono")
    {
        flags |= 1;
    }
    flags
}

pub(super) fn embedded_font_file_object(font: &EmbeddedFontPlan<'_>) -> Vec<u8> {
    let used_glyphs = font.used_glyphs.keys().copied().collect::<Vec<_>>();
    let source_len = font.font.data.len();
    let (data, embedding_mode) = match font.font.program_kind {
        FontProgramKind::TrueType => {
            match subset::subset_font(font.font.data.as_ref(), font.font.face_index, &used_glyphs) {
                Some(data) => (data, "subset"),
                None => (font.font.data.as_ref().to_vec(), "full TrueType fallback"),
            }
        }
        FontProgramKind::OpenTypeCff => (font.font.data.as_ref().to_vec(), "full OpenType CFF"),
    };
    log::debug!(
        "embedding font {} ({:?}, {} used glyph(s), {embedding_mode}): {} byte source, {} byte PDF stream",
        font.font.post_script_name,
        font.font.program_kind,
        used_glyphs.len(),
        source_len,
        data.len()
    );
    let dictionary = match font.font.program_kind {
        FontProgramKind::TrueType => format!(
            "<< /Length {} /Length1 {} >>\nstream\n",
            data.len(),
            data.len()
        ),
        FontProgramKind::OpenTypeCff => {
            format!("<< /Length {} /Subtype /OpenType >>\nstream\n", data.len())
        }
    };
    let mut object = dictionary.into_bytes();
    object.extend_from_slice(&data);
    object.extend_from_slice(b"\nendstream\n");
    object
}

pub(super) fn to_unicode_object(font: &EmbeddedFontPlan<'_>) -> String {
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> def\n/CMapName /ReasyPrint-ToUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries = font
        .used_glyphs
        .iter()
        .filter(|(_, unicode)| !unicode.is_empty())
        .map(|(glyph_id, unicode)| format!("<{glyph_id:04X}> <{}>", utf16be_hex(unicode)))
        .collect::<Vec<_>>();
    for chunk in entries.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for entry in chunk {
            cmap.push_str(entry);
            cmap.push('\n');
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    format!("<< /Length {} >>\nstream\n{}endstream\n", cmap.len(), cmap)
}

pub(super) fn utf16be_hex(text: &str) -> String {
    text.encode_utf16()
        .flat_map(u16::to_be_bytes)
        .map(|byte| format!("{byte:02X}"))
        .collect()
}
