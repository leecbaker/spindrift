use std::time::{Duration, Instant};

use super::*;
use crate::Error;
use crate::timing::DebugTimer;

#[cfg(test)]
pub(super) fn embedded_font_plans_with_profile<'a>(
    document: &'a Document,
    first_embedded_font_id: usize,
    profile: PdfFontValidationProfile,
) -> EmbeddedFontPlans<'a> {
    timed_embedded_font_plans_with_profile(
        document,
        first_embedded_font_id,
        profile,
        crate::FontEmbeddingMode::Subset,
    )
    .expect("test font plan should be embeddable")
    .0
}

#[cfg(test)]
pub(super) fn embedded_font_plans_with_profile_and_mode<'a>(
    document: &'a Document,
    first_embedded_font_id: usize,
    profile: PdfFontValidationProfile,
    mode: crate::FontEmbeddingMode,
) -> EmbeddedFontPlans<'a> {
    timed_embedded_font_plans_with_profile(document, first_embedded_font_id, profile, mode)
        .expect("test font plan should be embeddable")
        .0
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct PdfFontPlanTimings {
    pub(super) used_glyph_collection: Duration,
    pub(super) font_resource_mapping: Duration,
    pub(super) font_audit_embedding: Duration,
}

pub(super) fn timed_embedded_font_plans_with_profile<'a>(
    document: &'a Document,
    first_embedded_font_id: usize,
    profile: PdfFontValidationProfile,
    mode: crate::FontEmbeddingMode,
) -> crate::Result<(EmbeddedFontPlans<'a>, PdfFontPlanTimings)> {
    let timer = DebugTimer::start("collecting PDF used glyphs");
    let used_glyphs = used_glyphs_for_painted_text(document);
    let used_glyph_collection = timer.finish();

    let timer = DebugTimer::start("deduplicating PDF document fonts and assigning resources");
    let mut pending_fonts = Vec::<PendingEmbeddedFont<'_>>::new();
    let mut document_font_to_embedded_font = vec![None; document.fonts.len()];
    let mut candidates_by_key = HashMap::<EmbeddedFontCandidateKey, Vec<usize>>::new();
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

        let key = embedded_font_candidate_key(font);
        let existing = candidates_by_key.get(&key).and_then(|candidates| {
            candidates.iter().copied().find(|index| {
                pending_fonts
                    .get(*index)
                    .is_some_and(|pending| same_embedded_font_program(pending.font, font))
            })
        });
        if let Some(index) = existing {
            duplicate_fonts += 1;
            let embedded_font = &mut pending_fonts[index];
            for (glyph_id, unicode) in font_glyphs {
                embedded_font.used_glyphs.entry(glyph_id).or_insert(unicode);
            }
            document_font_to_embedded_font[document_font_id] = Some(index);
            continue;
        }

        let index = pending_fonts.len();
        candidates_by_key.entry(key).or_default().push(index);
        document_font_to_embedded_font[document_font_id] = Some(index);
        pending_fonts.push(PendingEmbeddedFont {
            font,
            used_glyphs: font_glyphs,
        });
    }
    let font_resource_mapping = timer.finish();

    let timer = DebugTimer::start(format!(
        "auditing and planning {} PDF embedded font(s)",
        pending_fonts.len()
    ));
    let fonts = pending_fonts
        .into_iter()
        .enumerate()
        .map(|(index, pending)| -> crate::Result<EmbeddedFontPlan<'_>> {
            let started = Instant::now();
            let base_id = first_embedded_font_id + index * profile.embedded_font_object_count();
            let audit = audit_font_program(pending.font, &pending.used_glyphs, profile, mode);
            log::debug!(
                "audited and planned {:?} PDF font {} ({} used glyph(s)) in {:.3?}",
                mode,
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
            if let FontEmbeddingKind::Rejected { reason } = &audit.font_file.embedding_kind {
                return Err(Error::FontEmbedding {
                    font: pending.font.post_script_name.clone(),
                    reason: reason.clone(),
                });
            }
            let embedded_face = ttf_parser::Face::parse(&audit.font_file.data, 0).ok();
            let source_gid_to_width = pdf_text_space_widths(
                pending.font,
                embedded_face.as_ref(),
                &audit.font_file.source_gid_to_cid,
            );
            Ok(EmbeddedFontPlan {
                font: pending.font,
                resource_name: format!("RF{}", index + 1),
                base_name: audit.base_name,
                type0_id: base_id,
                cid_font_id: base_id + 1,
                descriptor_id: base_id + 2,
                file_id: base_id + 3,
                to_unicode_id: base_id + 4,
                cid_set_id: profile.emits_cid_set().then_some(base_id + 5),
                font_program_kind: audit.font_file.program_kind,
                source_gid_to_cid: audit.font_file.source_gid_to_cid,
                source_gid_to_width,
                used_cids: audit.used_cids,
                font_file_data: audit.font_file.data,
                embedding_kind: audit.font_file.embedding_kind,
                descriptor_metrics: audit.descriptor_metrics,
                default_width: audit.default_width,
                cid_set_data: audit.cid_set_data,
            })
        })
        .collect::<crate::Result<Vec<_>>>()?;
    let font_audit_embedding = timer.finish();

    log::debug!(
        "planned PDF font embedding: {} original font(s), {} used font(s), {} unique embedded font(s), {} pruned, {} duplicate(s) merged",
        document.fonts.len(),
        document.fonts.len().saturating_sub(pruned_fonts),
        fonts.len(),
        pruned_fonts,
        duplicate_fonts
    );

    Ok((
        EmbeddedFontPlans {
            fonts,
            document_font_to_embedded_font,
            document_font_synthesis: document.fonts.iter().map(|font| font.synthesis).collect(),
        },
        PdfFontPlanTimings {
            used_glyph_collection,
            font_resource_mapping,
            font_audit_embedding,
        },
    ))
}

#[derive(Debug, Clone, PartialEq)]
struct FontProgramAudit {
    base_name: String,
    font_file: FontFilePlan,
    descriptor_metrics: FontDescriptorMetrics,
    default_width: f32,
    cid_set_data: Option<Vec<u8>>,
    used_cids: BTreeMap<u16, String>,
    fallback_reason: Option<String>,
    warnings: Vec<String>,
}

fn audit_font_program(
    font: &DocumentFont,
    used_glyphs: &BTreeMap<u16, String>,
    profile: PdfFontValidationProfile,
    mode: crate::FontEmbeddingMode,
) -> FontProgramAudit {
    let mut warnings = Vec::new();
    let original = font.data.as_ref();
    let face = ttf_parser::Face::parse(original, font.face_index).ok();
    let mut rejection_reasons = Vec::new();

    if let Some(face) = &face {
        if !face.is_outline_embedding_allowed() {
            warnings.push("OS/2 fsType does not allow outline embedding".to_string());
            rejection_reasons.push("OS/2 fsType does not allow outline embedding");
        }
        if mode == crate::FontEmbeddingMode::Subset && !face.is_subsetting_allowed() {
            warnings.push("OS/2 fsType does not allow subset embedding".to_string());
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

    let mut font_file = match mode {
        crate::FontEmbeddingMode::Subset
            if face
                .as_ref()
                .is_some_and(|face| !face.is_subsetting_allowed()) =>
        {
            let mut full = full_font_file(font, used_glyphs);
            if !matches!(full.embedding_kind, FontEmbeddingKind::Rejected { .. }) {
                full.fallback_reason = Some(
                    "OS/2 fsType does not allow subset embedding; embedded full font".to_string(),
                );
            }
            full
        }
        crate::FontEmbeddingMode::Subset => subset_font_file(font, used_glyphs, profile),
        crate::FontEmbeddingMode::Full => full_font_file(font, used_glyphs),
    };
    if font_file.program_kind == FontProgramKind::OpenTypeCff {
        // A CIDFontType0 embeds a CFF program, not the surrounding OpenType
        // SFNT container. Compare the extracted programs rather than their
        // containers: a smaller SFNT is not useful if its CFF stream would
        // grow after CID conversion. ISO 32000-2:2020, 9.9.2, FontFile3 CFF
        // font programs.
        let original_cff = cff_table(original, font.face_index);
        let embedded_cff = cff_table(&font_file.data, 0);
        match (original_cff, embedded_cff) {
            (Some(original_cff), Some(embedded_cff))
                if matches!(
                    font_file.embedding_kind,
                    FontEmbeddingKind::SubsetCompactGids
                ) && embedded_cff.len() < original_cff.len() =>
            {
                font_file.data = embedded_cff.to_vec();
            }
            (Some(_original_cff), Some(_))
                if matches!(
                    font_file.embedding_kind,
                    FontEmbeddingKind::SubsetCompactGids
                ) =>
            {
                // ISO 32000-2:2020, 9.9.2 permits a complete embedded CFF
                // program.  A subset being larger is an optimization failure,
                // not a reason to serialize an empty font resource.
                let mut full = full_font_file(font, used_glyphs);
                if let Some(full_cff) = cff_table(&full.data, 0) {
                    full.data = full_cff.to_vec();
                    full.program_kind = FontProgramKind::OpenTypeCff;
                    full.fallback_reason = Some(
                        "subsetter CFF program was not smaller than original; embedded full CFF program"
                            .to_string(),
                    );
                    font_file = full;
                } else {
                    font_file = FontFilePlan::rejected(
                        "OpenType CFF font does not contain a CFF table".to_string(),
                        FontProgramKind::OpenTypeCff,
                        identity_glyph_mapping(used_glyphs),
                    );
                }
            }
            (Some(original_cff), Some(_)) if mode == crate::FontEmbeddingMode::Full => {
                font_file.data = original_cff.to_vec();
            }
            (Some(_), Some(embedded_cff)) => font_file.data = embedded_cff.to_vec(),
            _ => {
                font_file = FontFilePlan::rejected(
                    "OpenType CFF font does not contain a CFF table".to_string(),
                    FontProgramKind::OpenTypeCff,
                    identity_glyph_mapping(used_glyphs),
                );
            }
        }
    }
    if !rejection_reasons.is_empty() {
        font_file = FontFilePlan::rejected(
            rejection_reasons.join("; "),
            font_file.program_kind,
            identity_glyph_mapping(used_glyphs),
        );
    }
    let mut fallback_reason = font_file.fallback_reason.clone();
    if matches!(font_file.embedding_kind, FontEmbeddingKind::Rejected { .. }) {
        let reason = font_file
            .fallback_reason
            .clone()
            .unwrap_or_else(|| "strict font embedding rejected the font".to_string());
        fallback_reason = Some(reason);
    }

    let embedded_face = ttf_parser::Face::parse(&font_file.data, 0).ok();
    let base_name = pdf_font_base_name(font, embedded_face.as_ref(), &font_file.embedding_kind);
    let descriptor_metrics = font_descriptor_metrics_from_face(
        font,
        embedded_face.as_ref(),
        &font_file.source_gid_to_cid,
    );
    let default_width = descriptor_metrics.missing_width.unwrap_or(0.0);
    let full_program = matches!(
        font_file.embedding_kind,
        FontEmbeddingKind::InstantiatedFullCoverage
            | FontEmbeddingKind::FullStandaloneFont
            | FontEmbeddingKind::ExtractedCollectionFace
    );
    let mut used_cids = if full_program {
        unicode_cmap_for_full_font(face.as_ref(), &font_file.source_gid_to_cid)
    } else {
        BTreeMap::new()
    };
    for (source_gid, unicode) in used_glyphs {
        if let Some(cid) = font_file.source_gid_to_cid.get(source_gid) {
            used_cids.insert(*cid, unicode.clone());
        }
    }
    let missing = used_glyphs
        .keys()
        .filter(|source_gid| !font_file.source_gid_to_cid.contains_key(source_gid))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        warnings.push(format!(
            "font program did not map every emitted glyph to a CID: {missing:?}"
        ));
    }
    let cid_set_data = profile.emits_cid_set().then(|| {
        let glyph_ids = if full_program {
            font_file
                .source_gid_to_cid
                .values()
                .cloned()
                .collect::<Vec<_>>()
        } else {
            used_cids.keys().cloned().collect()
        };
        cid_set_stream_data(glyph_ids)
    });

    FontProgramAudit {
        base_name,
        font_file,
        descriptor_metrics,
        default_width,
        cid_set_data,
        used_cids,
        fallback_reason,
        warnings,
    }
}

fn cff_table(font_data: &[u8], face_index: u32) -> Option<&[u8]> {
    ttf_parser::Face::parse(font_data, face_index)
        .ok()
        .and_then(|face| face.raw_face().table(ttf_parser::Tag::from_bytes(b"CFF ")))
}

/// Builds the fallback ToUnicode mapping for every Unicode cmap entry in a
/// full embedded font. Shaped text mappings are inserted afterwards so that
/// ligatures and other multi-codepoint glyphs retain their authored text.
fn unicode_cmap_for_full_font(
    face: Option<&ttf_parser::Face<'_>>,
    source_gid_to_cid: &BTreeMap<u16, u16>,
) -> BTreeMap<u16, String> {
    let mut unicode_by_cid = BTreeMap::new();
    let Some(cmap) = face.and_then(|face| face.tables().cmap) else {
        return unicode_by_cid;
    };
    for subtable in cmap.subtables {
        if !subtable.is_unicode() {
            continue;
        }
        subtable.codepoints(|codepoint| {
            let Some(character) = char::from_u32(codepoint) else {
                return;
            };
            let Some(source_gid) = subtable.glyph_index(codepoint).map(|glyph| glyph.0) else {
                return;
            };
            let Some(cid) = source_gid_to_cid.get(&source_gid) else {
                return;
            };
            unicode_by_cid
                .entry(*cid)
                .or_insert_with(|| character.to_string());
        });
    }
    unicode_by_cid
}

struct PendingEmbeddedFont<'a> {
    font: &'a DocumentFont,
    used_glyphs: BTreeMap<u16, String>,
}

fn used_glyphs_for_painted_text(document: &Document) -> Vec<BTreeMap<u16, String>> {
    let mut used_glyphs = vec![BTreeMap::<u16, String>::new(); document.fonts.len()];
    for page in &document.pages {
        collect_context_used_glyphs(
            page,
            document.fonts.len(),
            &page.paint_tree().root,
            &mut used_glyphs,
        );
    }
    used_glyphs
}

fn collect_context_used_glyphs(
    page: &Page,
    document_font_count: usize,
    context: &crate::document::paint::stacking::PaintStackingContext,
    used_glyphs: &mut [BTreeMap<u16, String>],
) {
    for band in crate::document::paint::display_list::PaintBand::ORDER {
        for item in &context.bands.bands[band.index()] {
            match item {
                crate::document::paint::display_list::PaintDisplayItem::Operation(operation) => {
                    collect_operation_used_glyphs(
                        page,
                        document_font_count,
                        operation,
                        used_glyphs,
                    );
                }
                crate::document::paint::display_list::PaintDisplayItem::StackingContext(
                    context,
                ) => {
                    collect_context_used_glyphs(page, document_font_count, context, used_glyphs);
                }
                crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope) => {
                    collect_effect_scope_used_glyphs(page, document_font_count, scope, used_glyphs);
                }
                crate::document::paint::display_list::PaintDisplayItem::Primitive(_)
                | crate::document::paint::display_list::PaintDisplayItem::Link(_) => {}
            }
        }
    }
}

fn collect_effect_scope_used_glyphs(
    page: &Page,
    document_font_count: usize,
    scope: &crate::document::paint::effects::PaintEffectScope,
    used_glyphs: &mut [BTreeMap<u16, String>],
) {
    for item in &scope.items {
        match item {
            crate::document::paint::display_list::PaintDisplayItem::Operation(operation) => {
                collect_operation_used_glyphs(page, document_font_count, operation, used_glyphs);
            }
            crate::document::paint::display_list::PaintDisplayItem::StackingContext(context) => {
                collect_context_used_glyphs(page, document_font_count, context, used_glyphs);
            }
            crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope) => {
                collect_effect_scope_used_glyphs(page, document_font_count, scope, used_glyphs);
            }
            crate::document::paint::display_list::PaintDisplayItem::Primitive(_)
            | crate::document::paint::display_list::PaintDisplayItem::Link(_) => {}
        }
    }
}

fn collect_operation_used_glyphs(
    page: &Page,
    document_font_count: usize,
    operation: &crate::document::paint::page::PaintOperation,
    used_glyphs: &mut [BTreeMap<u16, String>],
) {
    let line_index = match operation {
        crate::document::paint::page::PaintOperation::Line(index) => *index,
        crate::document::paint::page::PaintOperation::OpaqueTextCoverage(index) => {
            let Some(coverage) = page.opaque_text_coverages.get(*index) else {
                return;
            };
            coverage.line_index
        }
        _ => return,
    };
    let Some(line) = page.lines.get(line_index) else {
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
            let Some(glyph_id) = glyph.painted_id() else {
                continue;
            };
            let unicode = if glyph.unicode.is_empty() {
                run.actual_text.unwrap_or_default().to_owned()
            } else {
                glyph.unicode.clone()
            };
            font_glyphs.entry(glyph_id).or_insert(unicode);
        }
        if run.glyphs.is_empty() {
            log::debug!("empty shaped text line {:?}", line.text);
        }
    }
    if !saw_text_run && !line.text.is_empty() && line.runs.is_empty() {
        log::warn!(
            "skipping unshaped text line without a resolved embedded font: {:?}",
            line.text
        );
    }
}

pub(super) fn embedded_font_candidate_key(font: &DocumentFont) -> EmbeddedFontCandidateKey {
    EmbeddedFontCandidateKey {
        program_len: font.data.len(),
        face_index: font.face_index,
        program_kind: font.program_kind,
        variation_coordinates: font.variation_coordinates.clone(),
    }
}

/// Returns whether two document-font records originate from the same complete
/// program. Blob identity handles the common shared-source case without
/// touching mapped font bytes; a byte comparison preserves deduplication for
/// separately loaded but identical font programs.
pub(super) fn same_embedded_font_program(left: &DocumentFont, right: &DocumentFont) -> bool {
    left.variation_coordinates == right.variation_coordinates
        && (left.data.blob_id() == right.data.blob_id()
            || left.data.as_ref() == right.data.as_ref())
}

fn pdf_text_space_widths(
    font: &DocumentFont,
    embedded_face: Option<&ttf_parser::Face<'_>>,
    source_gid_to_cid: &BTreeMap<u16, u16>,
) -> BTreeMap<u16, PdfTextSpaceWidth> {
    let Some(face) = embedded_face else {
        return BTreeMap::new();
    };
    source_gid_to_cid
        .iter()
        .filter_map(|(source_gid, cid)| {
            face.glyph_hor_advance(ttf_parser::GlyphId(*cid))
                .map(|advance| {
                    (
                        *source_gid,
                        PdfTextSpaceWidth::from_font_units(advance, font.units_per_em),
                    )
                })
        })
        .collect()
}

pub(super) fn cid_width_entries(font: &EmbeddedFontPlan<'_>) -> Vec<(u16, f32)> {
    font.source_gid_to_cid
        .iter()
        .filter_map(|(source_gid, cid)| {
            font.source_gid_to_width
                .get(source_gid)
                .map(|width| (*cid, width.as_pdf_number()))
        })
        .collect()
}

fn font_descriptor_metrics_from_face(
    font: &DocumentFont,
    face: Option<&ttf_parser::Face<'_>>,
    source_gid_to_cid: &BTreeMap<u16, u16>,
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
        || scale_f32(font.program_metrics.ascender),
        |face| scale_f32(face.ascender()),
    );
    let descent = face.map_or_else(
        || scale_f32(font.program_metrics.descender),
        |face| scale_f32(face.descender()),
    );
    let cap_height = face
        .and_then(ttf_parser::Face::capital_height)
        .map_or_else(|| scale_f32(font.cap_height), scale_f32);
    let x_height = face
        .and_then(ttf_parser::Face::x_height)
        .map(scale_f32)
        .or_else(|| glyph_height(face, 'x'));
    let widths = mapped_glyph_widths(font, face, source_gid_to_cid);
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
        max_width: widths.iter().cloned().max().map(|width| width as f32),
        missing_width: face
            .and_then(|face| face.glyph_hor_advance(ttf_parser::GlyphId(0)))
            .map(|width| (width as f32 * 1000.0 / units_per_em).round()),
    }
}

fn mapped_glyph_widths(
    font: &DocumentFont,
    face: Option<&ttf_parser::Face<'_>>,
    source_gid_to_cid: &BTreeMap<u16, u16>,
) -> Vec<i32> {
    let Some(face) = face else {
        return Vec::new();
    };
    let units_per_em = font.units_per_em.max(1) as f32;
    source_gid_to_cid
        .iter()
        .filter_map(|(_source_gid, cid)| {
            face.glyph_hor_advance(ttf_parser::GlyphId(*cid))
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
    let label = pdf_font_program_label(font, face).to_ascii_lowercase();
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
        FontEmbeddingKind::SubsetCompactGids => "subsetted with compact CIDs",
        FontEmbeddingKind::InstantiatedFullCoverage => {
            "static variable instance with full coverage"
        }
        FontEmbeddingKind::FullStandaloneFont => "full standalone font",
        FontEmbeddingKind::ExtractedCollectionFace => "extracted collection face",
        FontEmbeddingKind::Rejected { .. } => "rejected",
    };
    log::debug!(
        "embedding font {} ({:?}, {} used glyph(s), {}): {} byte PDF stream from {} byte original",
        font.font.post_script_name,
        font.font_program_kind,
        font.used_cids.len(),
        kind,
        embedded_len,
        original_len
    );
}

fn pdf_font_base_name(
    font: &DocumentFont,
    face: Option<&ttf_parser::Face<'_>>,
    embedding_kind: &FontEmbeddingKind,
) -> String {
    let post_script_name = sanitize_pdf_font_name(&pdf_font_program_post_script_name(font, face));
    match embedding_kind {
        FontEmbeddingKind::SubsetCompactGids => {
            format!(
                "{}+{}",
                subset_prefix(font, &post_script_name),
                post_script_name
            )
        }
        FontEmbeddingKind::InstantiatedFullCoverage => format!(
            "{}+{}",
            subset_prefix(font, &post_script_name),
            post_script_name
        ),
        FontEmbeddingKind::FullStandaloneFont | FontEmbeddingKind::ExtractedCollectionFace => {
            post_script_name
        }
        FontEmbeddingKind::Rejected { .. } => format!("REJECT+{post_script_name}"),
    }
}

fn pdf_font_program_post_script_name(
    font: &DocumentFont,
    face: Option<&ttf_parser::Face<'_>>,
) -> String {
    face.and_then(|face| {
        crate::text::font_program_opentype_name(face, ttf_parser::name_id::POST_SCRIPT_NAME)
    })
    .or_else(|| {
        face.and_then(|face| {
            crate::text::font_program_opentype_name(face, ttf_parser::name_id::FULL_NAME)
        })
    })
    .unwrap_or_else(|| font.post_script_name.clone())
}

fn pdf_font_program_label(font: &DocumentFont, face: Option<&ttf_parser::Face<'_>>) -> String {
    let Some(face) = face else {
        return format!("{} {}", font.family, font.post_script_name);
    };
    let family =
        crate::text::font_program_opentype_name(face, ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
            .or_else(|| crate::text::font_program_opentype_name(face, ttf_parser::name_id::FAMILY))
            .unwrap_or_default();
    format!(
        "{family} {}",
        pdf_font_program_post_script_name(font, Some(face))
    )
}

fn subset_prefix(font: &DocumentFont, post_script_name: &str) -> String {
    let hash = font.data.blob_id()
        ^ ((font.face_index as u64) << 32)
        ^ post_script_name.bytes().fold(0u64, |hash, byte| {
            hash.wrapping_mul(33).wrapping_add(byte as u64)
        })
        ^ font
            .variation_coordinates
            .0
            .iter()
            .fold(0u64, |hash, (tag, value)| {
                tag.iter().fold(hash, |hash, byte| {
                    hash.wrapping_mul(33).wrapping_add(u64::from(*byte))
                }) ^ u64::from(*value)
            });
    std::iter::successors(Some(hash), |hash| Some(hash / 26))
        .take(6)
        .map(|hash| (b'A' + (hash % 26) as u8) as char)
        .collect()
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
    let Some(max_gid) = glyph_ids.iter().cloned().max() else {
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
        pdf_writer::Name(b"Quire-ToUnicode"),
        pdf_writer::types::SystemInfo {
            registry: pdf_writer::Str(b"Adobe"),
            ordering: pdf_writer::Str(b"Identity"),
            supplement: 0,
        },
    );
    for (cid, unicode) in &font.used_cids {
        let codepoints = unicode.chars().collect::<Vec<_>>();
        if codepoints.is_empty() {
            continue;
        }
        cmap.pair_with_multiple(*cid, codepoints);
    }
    cmap.finish().into_vec()
}
