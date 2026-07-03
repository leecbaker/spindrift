use super::*;
use crate::timing::DebugTimer;
use std::time::{Duration, Instant};

#[cfg(test)]
pub(super) fn embedded_font_plans_with_profile<'a>(
    document: &'a Document,
    first_embedded_font_id: usize,
    profile: PdfFontValidationProfile,
) -> EmbeddedFontPlans<'a> {
    timed_embedded_font_plans_with_profile(document, first_embedded_font_id, profile).0
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct PdfFontPlanTimings {
    pub(super) used_glyph_collection: Duration,
    pub(super) font_resource_mapping: Duration,
    pub(super) font_audit_subsetting: Duration,
}

pub(super) fn timed_embedded_font_plans_with_profile<'a>(
    document: &'a Document,
    first_embedded_font_id: usize,
    profile: PdfFontValidationProfile,
) -> (EmbeddedFontPlans<'a>, PdfFontPlanTimings) {
    let timer = DebugTimer::start("collecting PDF used glyphs");
    let used_glyphs = used_glyphs_for_painted_text(document);
    let used_glyph_collection = timer.finish();

    let timer = DebugTimer::start("deduplicating PDF document fonts and assigning resources");
    let mut pending_fonts = Vec::<PendingEmbeddedFont<'_>>::new();
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
            let embedded_font = &mut pending_fonts[index];
            for (glyph_id, unicode) in font_glyphs {
                embedded_font.used_glyphs.entry(glyph_id).or_insert(unicode);
            }
            document_font_to_embedded_font[document_font_id] = Some(index);
            continue;
        }

        let index = pending_fonts.len();
        key_to_embedded_font.insert(key, index);
        document_font_to_embedded_font[document_font_id] = Some(index);
        pending_fonts.push(PendingEmbeddedFont {
            font,
            used_glyphs: font_glyphs,
        });
    }
    let font_resource_mapping = timer.finish();

    let timer = DebugTimer::start(format!(
        "auditing and subsetting {} PDF embedded font(s)",
        pending_fonts.len()
    ));
    let fonts = pending_fonts
        .into_iter()
        .enumerate()
        .map(|(index, pending)| {
            let started = Instant::now();
            let base_id = first_embedded_font_id + index * profile.embedded_font_object_count();
            let audit = audit_font_program(pending.font, &pending.used_glyphs, profile);
            log::debug!(
                "audited and subset PDF font {} ({} used glyph(s)) in {:.3?}",
                pending.font.post_script_name,
                pending.used_glyphs.len(),
                started.elapsed()
            );
            if let Some(reason) = &audit.fallback_reason {
                log::debug!(
                    "using full font fallback for {}: {}",
                    pending.font.post_script_name,
                    reason
                );
            }
            if !audit.warnings.is_empty() {
                for warning in &audit.warnings {
                    log::warn!(
                        "PDF font audit for {}: {warning}",
                        pending.font.post_script_name
                    );
                }
            }
            EmbeddedFontPlan {
                font: pending.font,
                resource_name: format!("RF{}", index + 1),
                base_name: audit.base_name,
                type0_id: base_id,
                cid_font_id: base_id + 1,
                descriptor_id: base_id + 2,
                file_id: base_id + 3,
                to_unicode_id: base_id + 4,
                cid_set_id: profile.emits_cid_set().then_some(base_id + 5),
                used_glyphs: pending.used_glyphs,
                font_file_data: audit.font_file.data,
                embedding_kind: audit.font_file.embedding_kind,
                descriptor_metrics: audit.descriptor_metrics,
                default_width: audit.default_width,
                cid_set_data: audit.cid_set_data,
            }
        })
        .collect::<Vec<_>>();
    let font_audit_subsetting = timer.finish();

    log::debug!(
        "planned PDF font embedding: {} original font(s), {} used font(s), {} unique embedded font(s), {} pruned, {} duplicate(s) merged",
        document.fonts.len(),
        document.fonts.len().saturating_sub(pruned_fonts),
        fonts.len(),
        pruned_fonts,
        duplicate_fonts
    );

    (
        EmbeddedFontPlans {
            fonts,
            document_font_to_embedded_font,
        },
        PdfFontPlanTimings {
            used_glyph_collection,
            font_resource_mapping,
            font_audit_subsetting,
        },
    )
}

#[derive(Debug, Clone, PartialEq)]
struct FontProgramAudit {
    base_name: String,
    font_file: FontFilePlan,
    descriptor_metrics: FontDescriptorMetrics,
    default_width: f32,
    cid_set_data: Option<Vec<u8>>,
    fallback_reason: Option<String>,
    warnings: Vec<String>,
}

fn audit_font_program(
    font: &DocumentFont,
    used_glyphs: &BTreeMap<u16, String>,
    profile: PdfFontValidationProfile,
) -> FontProgramAudit {
    let mut warnings = Vec::new();
    let original = font.data.as_ref();
    let face = ttf_parser::Face::parse(original, font.face_index).ok();
    let mut rejection_reasons = Vec::new();

    if let Some(face) = &face {
        if !face.is_outline_embedding_allowed() {
            warnings.push("OS/2 fsType does not allow outline embedding".to_string());
            if !profile.allows_full_font_fallback() {
                rejection_reasons.push("OS/2 fsType does not allow outline embedding");
            }
        }
        if !face.is_subsetting_allowed() {
            warnings.push("OS/2 fsType does not allow subset embedding".to_string());
            if !profile.allows_full_font_fallback() {
                rejection_reasons.push("OS/2 fsType does not allow subset embedding");
            }
        }
        if face.tables().sbix.is_some()
            || face.tables().cbdt.is_some()
            || face.tables().bdat.is_some()
            || face.tables().ebdt.is_some()
        {
            warnings.push(
                "bitmap/color font tables are not represented by PDF Type 0 outline embedding"
                    .to_string(),
            );
        }
    } else {
        warnings.push("font program could not be parsed for PDF audit".to_string());
    }

    let missing_unicode_count = used_glyphs
        .values()
        .filter(|unicode| unicode.is_empty())
        .count();
    if missing_unicode_count > 0 {
        warnings.push(format!(
            "{missing_unicode_count} emitted glyph(s) have no ToUnicode mapping"
        ));
        if !profile.allows_full_font_fallback() {
            rejection_reasons.push("emitted glyphs are missing ToUnicode mappings");
        }
    }

    let mut font_file = subset_font_file(font, used_glyphs, profile);
    if !rejection_reasons.is_empty() {
        font_file = FontFilePlan::rejected(rejection_reasons.join("; "));
    }
    let mut fallback_reason = font_file.fallback_reason.clone();
    if matches!(font_file.embedding_kind, FontEmbeddingKind::Rejected { .. }) {
        let reason = font_file
            .fallback_reason
            .clone()
            .unwrap_or_else(|| "strict font embedding rejected the font".to_string());
        fallback_reason = Some(reason);
    }

    let base_name = pdf_font_base_name(font, &font_file.embedding_kind);
    let descriptor_metrics = font_descriptor_metrics_from_face(font, face.as_ref(), used_glyphs);
    let default_width = descriptor_metrics.missing_width.unwrap_or(0.0);
    let cid_set_data = profile
        .emits_cid_set()
        .then(|| cid_set_stream_data(used_glyphs.keys().copied()));

    FontProgramAudit {
        base_name,
        font_file,
        descriptor_metrics,
        default_width,
        cid_set_data,
        fallback_reason,
        warnings,
    }
}

struct PendingEmbeddedFont<'a> {
    font: &'a DocumentFont,
    used_glyphs: BTreeMap<u16, String>,
}

fn used_glyphs_for_painted_text(document: &Document) -> Vec<BTreeMap<u16, String>> {
    let mut used_glyphs = vec![BTreeMap::<u16, String>::new(); document.fonts.len()];
    for page in &document.pages {
        if let Some(tree) = page.paint_tree() {
            collect_context_used_glyphs(page, document.fonts.len(), &tree.root, &mut used_glyphs);
        } else {
            for operation in page.paint_operations().iter() {
                collect_operation_used_glyphs(
                    page,
                    document.fonts.len(),
                    operation,
                    &mut used_glyphs,
                );
            }
        }
    }
    used_glyphs
}

fn collect_context_used_glyphs(
    page: &Page,
    document_font_count: usize,
    context: &crate::document::PaintStackingContext,
    used_glyphs: &mut [BTreeMap<u16, String>],
) {
    for band in crate::document::PaintBand::ORDER {
        for item in &context.bands.bands[band.index()] {
            match item {
                crate::document::PaintDisplayItem::Operation(operation) => {
                    collect_operation_used_glyphs(
                        page,
                        document_font_count,
                        operation,
                        used_glyphs,
                    );
                }
                crate::document::PaintDisplayItem::StackingContext(context) => {
                    collect_context_used_glyphs(page, document_font_count, context, used_glyphs);
                }
                crate::document::PaintDisplayItem::EffectScope(scope) => {
                    collect_effect_scope_used_glyphs(page, document_font_count, scope, used_glyphs);
                }
                crate::document::PaintDisplayItem::Primitive(_)
                | crate::document::PaintDisplayItem::Link(_) => {}
            }
        }
    }
}

fn collect_effect_scope_used_glyphs(
    page: &Page,
    document_font_count: usize,
    scope: &crate::document::PaintEffectScope,
    used_glyphs: &mut [BTreeMap<u16, String>],
) {
    for item in &scope.items {
        match item {
            crate::document::PaintDisplayItem::Operation(operation) => {
                collect_operation_used_glyphs(page, document_font_count, operation, used_glyphs);
            }
            crate::document::PaintDisplayItem::StackingContext(context) => {
                collect_context_used_glyphs(page, document_font_count, context, used_glyphs);
            }
            crate::document::PaintDisplayItem::EffectScope(scope) => {
                collect_effect_scope_used_glyphs(page, document_font_count, scope, used_glyphs);
            }
            crate::document::PaintDisplayItem::Primitive(_)
            | crate::document::PaintDisplayItem::Link(_) => {}
        }
    }
}

fn collect_operation_used_glyphs(
    page: &Page,
    document_font_count: usize,
    operation: &crate::PaintOperation,
    used_glyphs: &mut [BTreeMap<u16, String>],
) {
    let crate::PaintOperation::Line(index) = operation else {
        return;
    };
    let Some(line) = page.lines.get(*index) else {
        return;
    };
    if !line.color.is_visible() {
        return;
    }
    let mut saw_text_run = false;
    for run in pdf_text_runs(line, document_font_count) {
        saw_text_run = true;
        let Some(font_glyphs) = used_glyphs.get_mut(run.document_font_id) else {
            continue;
        };
        for glyph in run.glyphs {
            font_glyphs
                .entry(glyph.id)
                .or_insert_with(|| glyph.unicode.clone());
        }
        if run.glyphs.is_empty() {
            log::debug!("empty shaped text line {:?}", line.text);
        }
    }
    if !saw_text_run && !line.text.is_empty() {
        log::warn!(
            "skipping unshaped text line without a resolved embedded font: {:?}",
            line.text
        );
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

pub(super) fn cid_width_entries(font: &EmbeddedFontPlan<'_>) -> Vec<(u16, f32)> {
    let Ok(face) = ttf_parser::Face::parse(&font.font.data, font.font.face_index) else {
        return Vec::new();
    };
    let units_per_em = font.font.units_per_em.max(1) as f32;
    font.used_glyphs
        .keys()
        .map(|glyph_id| {
            let width = face
                .glyph_hor_advance(ttf_parser::GlyphId(*glyph_id))
                .map(|width| (width as f32 * 1000.0 / units_per_em).round() as i32)
                .unwrap_or(0);
            (*glyph_id, width as f32)
        })
        .collect()
}

fn font_descriptor_metrics_from_face(
    font: &DocumentFont,
    face: Option<&ttf_parser::Face<'_>>,
    used_glyphs: &BTreeMap<u16, String>,
) -> FontDescriptorMetrics {
    let units_per_em = font.units_per_em.max(1) as f32;
    let scale_i16 = |value: i16| (value as f32 * 1000.0 / units_per_em).round() as i32;
    let scale_f32 = |value: i16| scale_i16(value) as f32;
    let bbox = face
        .map(|face| {
            let bbox = face.global_bounding_box();
            [
                scale_i16(bbox.x_min),
                scale_i16(bbox.y_min),
                scale_i16(bbox.x_max),
                scale_i16(bbox.y_max),
            ]
        })
        .unwrap_or_else(|| {
            [
                scale_i16(font.bbox[0]),
                scale_i16(font.bbox[1]),
                scale_i16(font.bbox[2]),
                scale_i16(font.bbox[3]),
            ]
        });
    let ascent = face.map_or_else(
        || scale_f32(font.ascender),
        |face| scale_f32(face.ascender()),
    );
    let descent = face.map_or_else(
        || scale_f32(font.descender),
        |face| scale_f32(face.descender()),
    );
    let cap_height = face
        .and_then(ttf_parser::Face::capital_height)
        .map_or_else(|| scale_f32(font.cap_height), scale_f32);
    let x_height = face
        .and_then(ttf_parser::Face::x_height)
        .map(scale_f32)
        .or_else(|| glyph_height(face, 'x'));
    let widths = used_glyph_widths(font, face, used_glyphs);
    FontDescriptorMetrics {
        flags: font_descriptor_flags(font, face),
        bbox,
        italic_angle: face.map_or(font.italic_angle as f32, ttf_parser::Face::italic_angle),
        ascent,
        descent,
        cap_height,
        x_height,
        stem_v: stem_v_estimate(face),
        avg_width: average_width(&widths),
        max_width: widths.iter().copied().max().map(|width| width as f32),
        missing_width: face
            .and_then(|face| face.glyph_hor_advance(ttf_parser::GlyphId(0)))
            .map(|width| (width as f32 * 1000.0 / units_per_em).round()),
    }
}

fn used_glyph_widths(
    font: &DocumentFont,
    face: Option<&ttf_parser::Face<'_>>,
    used_glyphs: &BTreeMap<u16, String>,
) -> Vec<i32> {
    let Some(face) = face else {
        return Vec::new();
    };
    let units_per_em = font.units_per_em.max(1) as f32;
    used_glyphs
        .keys()
        .filter_map(|glyph_id| {
            face.glyph_hor_advance(ttf_parser::GlyphId(*glyph_id))
                .map(|width| (width as f32 * 1000.0 / units_per_em).round() as i32)
        })
        .collect()
}

fn average_width(widths: &[i32]) -> Option<f32> {
    (!widths.is_empty()).then(|| {
        let total = widths.iter().sum::<i32>() as f32;
        (total / widths.len() as f32).round()
    })
}

fn glyph_height(face: Option<&ttf_parser::Face<'_>>, character: char) -> Option<f32> {
    let face = face?;
    let units_per_em = face.units_per_em().max(1) as f32;
    let glyph = face.glyph_index(character)?;
    let bbox = face.glyph_bounding_box(glyph)?;
    Some(((bbox.y_max - bbox.y_min) as f32 * 1000.0 / units_per_em).round())
}

fn stem_v_estimate(face: Option<&ttf_parser::Face<'_>>) -> f32 {
    let Some(face) = face else {
        return 80.0;
    };
    let weight = face.weight().to_number();
    ((weight as f32 / 700.0) * 80.0).clamp(50.0, 220.0).round()
}

pub(super) fn font_descriptor_flags(
    font: &DocumentFont,
    face: Option<&ttf_parser::Face<'_>>,
) -> u32 {
    let mut flags = 0;
    let family = font.family.to_ascii_lowercase();
    let post_script_name = font.post_script_name.to_ascii_lowercase();
    let label = format!("{family} {post_script_name}");
    if face.is_some_and(ttf_parser::Face::is_monospaced)
        || label.contains("courier")
        || label.contains("mono")
    {
        flags |= 1;
    }
    if label.contains("serif") && !label.contains("sans") {
        flags |= 2;
    }
    if face.is_some_and(|face| face.glyph_index('A').is_some() || face.glyph_index('a').is_some()) {
        flags |= 32;
    } else {
        flags |= 4;
    }
    if label.contains("script") || label.contains("cursive") {
        flags |= 8;
    }
    if face.is_some_and(ttf_parser::Face::is_italic) || font.italic_angle != 0 {
        flags |= 64;
    }
    if label.contains("allcap") || label.contains("all-cap") || label.contains("all cap") {
        flags |= 1 << 16;
    }
    if label.contains("smallcap") || label.contains("small-cap") || label.contains("small cap") {
        flags |= 1 << 17;
    }
    if face.is_some_and(ttf_parser::Face::is_bold) || label.contains("bold") {
        flags |= 1 << 18;
    }
    flags
}

pub(super) fn log_embedded_font_file(font: &EmbeddedFontPlan<'_>) {
    let original_len = font.font.data.len();
    let embedded_len = font.font_file_data.len();
    let kind = match &font.embedding_kind {
        FontEmbeddingKind::SubsetRetainedGids => "subsetted with retained GIDs",
        FontEmbeddingKind::FullStandaloneFont => "full standalone font",
        FontEmbeddingKind::ExtractedCollectionFace => "extracted collection face",
        FontEmbeddingKind::Rejected { .. } => "rejected",
    };
    log::debug!(
        "embedding font {} ({:?}, {} used glyph(s), {}): {} byte PDF stream from {} byte original",
        font.font.post_script_name,
        font.font.program_kind,
        font.used_glyphs.len(),
        kind,
        embedded_len,
        original_len
    );
}

fn pdf_font_base_name(font: &DocumentFont, embedding_kind: &FontEmbeddingKind) -> String {
    let post_script_name = sanitize_pdf_font_name(&font.post_script_name);
    match embedding_kind {
        FontEmbeddingKind::SubsetRetainedGids => {
            format!("{}+{}", subset_prefix(font), post_script_name)
        }
        FontEmbeddingKind::FullStandaloneFont | FontEmbeddingKind::ExtractedCollectionFace => {
            post_script_name
        }
        FontEmbeddingKind::Rejected { .. } => format!("REJECT+{post_script_name}"),
    }
}

fn subset_prefix(font: &DocumentFont) -> String {
    let mut hash = font.data.blob_id()
        ^ ((font.face_index as u64) << 32)
        ^ font.post_script_name.bytes().fold(0u64, |hash, byte| {
            hash.wrapping_mul(33).wrapping_add(byte as u64)
        });
    let mut prefix = String::with_capacity(6);
    for _ in 0..6 {
        prefix.push((b'A' + (hash % 26) as u8) as char);
        hash /= 26;
    }
    prefix
}

fn sanitize_pdf_font_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '+') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "QuireFont".to_string()
    } else {
        sanitized
    }
}

fn cid_set_stream_data(glyph_ids: impl IntoIterator<Item = u16>) -> Vec<u8> {
    let glyph_ids = glyph_ids.into_iter().collect::<Vec<_>>();
    let Some(max_gid) = glyph_ids.iter().copied().max() else {
        return Vec::new();
    };
    let mut bytes = vec![0u8; usize::from(max_gid / 8) + 1];
    for glyph_id in glyph_ids {
        let byte_index = usize::from(glyph_id / 8);
        let bit = 7 - (glyph_id % 8);
        bytes[byte_index] |= 1 << bit;
    }
    bytes
}

pub(super) fn to_unicode_cmap(font: &EmbeddedFontPlan<'_>) -> Vec<u8> {
    let mut cmap = pdf_writer::types::UnicodeCmap::<u16>::new(
        pdf_writer::Name(b"ReasyPrint-ToUnicode"),
        pdf_writer::types::SystemInfo {
            registry: pdf_writer::Str(b"Adobe"),
            ordering: pdf_writer::Str(b"Identity"),
            supplement: 0,
        },
    );
    for (glyph_id, unicode) in &font.used_glyphs {
        let codepoints = unicode.chars().collect::<Vec<_>>();
        if codepoints.is_empty() {
            continue;
        }
        cmap.pair_with_multiple(*glyph_id, codepoints);
    }
    cmap.finish().into_vec()
}
