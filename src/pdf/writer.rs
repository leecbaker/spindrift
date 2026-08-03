use super::colors::{PdfColorPlan, PdfPaintColor};
use super::*;
use crate::timing::DebugTimer;
use pdf_writer::types::{
    ActionType, AnnotationType, CidFontType, ColorSpaceOperand, FontFlags, FunctionShadingType,
    MaskType, PaintType, SystemInfo, TilingType,
};
use pdf_writer::{Content, Filter, Name, Pdf, Rect, Ref, Settings, Str, TextStr};
use std::time::Duration;

pub(crate) fn write_document(
    document: &Document,
    options: &crate::PdfOptions,
) -> crate::Result<Vec<u8>> {
    let profile = options.profile;
    let font_embedding = options.font_embedding;
    let compression = options.compression;
    let total_timer = DebugTimer::start("serializing PDF document");
    let mut timings = PdfTimingSummary::new();
    let page_count = document.pages.len();
    let mut allocator = PdfObjectAllocator::new();

    let catalog_id = allocator.alloc_id();
    let pages_id = allocator.alloc_id();
    let font_id = allocator.alloc_id();
    let page_ids = allocator.alloc_ids(page_count);
    let content_ids = allocator.alloc_ids(page_count);
    let image_plan = timings.measure("deduplicating and preparing PDF image resources", || {
        deduplicate_images(document)
    });
    let vector_paint_colors = document.pages.iter().flat_map(Page::vector_paint_colors);
    let color_plan = PdfColorPlan::new(
        profile,
        allocator.peek_id(),
        vector_paint_colors,
        image_plan.built_in_color_spaces(&document.image_store),
        image_plan.embedded_rgb_profiles(&document.image_store),
    )?;
    allocator.reserve_ids(color_plan.object_count());
    let solid_fill_eligibility = image_plan.solid_fill_eligibility(document);
    let prepared_images = timings.measure("materializing PDF image paint representations", || {
        image_plan
            .unique_images
            .iter()
            .zip(solid_fill_eligibility)
            .map(|(source, eligible)| {
                prepare_image_resource(&document.image_store, source, color_plan.mode(), eligible)
            })
            .collect::<Vec<_>>()
    });

    let (embedded_font_plans, font_timings) = timings.measure(
        format!(
            "planning PDF font embedding for {} document font(s)",
            document.fonts.len()
        ),
        || {
            let first_embedded_font_id = allocator.peek_id();
            let font_validation_profile = if profile.is_pdfa() {
                PdfFontValidationProfile::PdfA
            } else {
                PdfFontValidationProfile::Default
            };
            let (plans, font_timings) = timed_embedded_font_plans_with_profile(
                document,
                first_embedded_font_id,
                font_validation_profile,
                font_embedding,
            )?;
            allocator.advance_to(
                first_embedded_font_id
                    + plans.fonts.len() * font_validation_profile.embedded_font_object_count(),
            );
            Ok::<_, crate::Error>((plans, font_timings))
        },
    )?;
    timings.record(
        "PDF font embedding: used glyph collection",
        font_timings.used_glyph_collection,
    );
    timings.record(
        "PDF font embedding: document font resource mapping",
        font_timings.font_resource_mapping,
    );
    timings.record(
        "PDF font embedding: audit and planning",
        font_timings.font_audit_embedding,
    );

    let (unique_image_ids, page_image_ids) =
        timings.measure("assigning PDF image object IDs", || {
            let unique_image_ids = prepared_images
                .iter()
                .map(|image| match image {
                    PreparedImageResource::Transparent | PreparedImageResource::SolidFill(_) => {
                        None
                    }
                    PreparedImageResource::Raster(_) => {
                        let image_id = PdfImageObjectId(allocator.alloc_id());
                        // Alpha is only known after decoding encoded sources. Reserve
                        // a mask object ID so object planning remains lightweight;
                        // the ID is simply unused for opaque images.
                        let alpha_mask_id = Some(PdfImageObjectId(allocator.alloc_id()));
                        Some(ImageObjectIds {
                            image_id,
                            alpha_mask_id,
                        })
                    }
                })
                .collect::<Vec<_>>();
            let page_image_ids = image_plan
                .page_image_unique_indexes
                .iter()
                .map(|page_images| {
                    page_images
                        .iter()
                        .map(|index| unique_image_ids[index.0].map(|ids| ids.image_id))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            (unique_image_ids, page_image_ids)
        });
    let page_image_pattern_plans = timings.measure("planning PDF image pattern objects", || {
        image_plan
            .page_pattern_tile_unique_indexes
            .iter()
            .enumerate()
            .map(|(page_index, tile_indexes)| {
                document.pages[page_index]
                    .image_patterns
                    .iter()
                    .zip(tile_indexes)
                    .enumerate()
                    .filter_map(|(pattern_index, (pattern, tile_index))| {
                        let tile_image_id = unique_image_ids[tile_index.0].map(|ids| ids.image_id);
                        match (&prepared_images[tile_index.0], tile_image_id) {
                            (PreparedImageResource::Transparent, None) => None,
                            (PreparedImageResource::Raster(_), Some(tile_image_id)) => {
                                Some(PageImagePatternPlan {
                                    id: allocator.alloc_id(),
                                    name: format!("P{}", pattern_index + 1),
                                    tile_image_id,
                                    pattern: pattern.clone(),
                                })
                            }
                            (PreparedImageResource::SolidFill(_), None) => {
                                unreachable!("image patterns cannot use solid-fill emission")
                            }
                            _ => {
                                unreachable!("image resource IDs match their paint representation")
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    });

    let page_renders = timings.measure(
        format!("building {page_count} page content stream(s)"),
        || {
            document
                .pages
                .iter()
                .enumerate()
                .map(|(page_index, page)| {
                    let mut next_dynamic_object_id = allocator.peek_id();
                    let render = page_content_render(
                        page,
                        &embedded_font_plans,
                        &mut next_dynamic_object_id,
                        &color_plan,
                        &prepared_images,
                        &image_plan.page_image_unique_indexes[page_index],
                        &image_plan.page_pattern_tile_unique_indexes[page_index],
                    );
                    allocator.advance_to(next_dynamic_object_id);
                    render
                })
                .collect::<Vec<_>>()
        },
    );
    let page_ext_gstate_plans = timings.measure("planning PDF page ExtGState resources", || {
        document
            .pages
            .iter()
            .map(|page| {
                page_ext_gstate_resources(page)
                    .into_iter()
                    .map(|resource| ExtGStateObjectPlan {
                        id: allocator.alloc_id(),
                        resource,
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    });

    let info_id = allocator.alloc_id();
    let metadata_id = allocator.alloc_id();
    let page_annotation_ids = timings.measure("planning PDF annotation IDs", || {
        document
            .pages
            .iter()
            .map(|page| page.links.iter().map(|_| allocator.alloc_id()).collect())
            .collect::<Vec<Vec<_>>>()
    });
    let outline_plan = timings.measure(
        format!(
            "planning {} bookmark outline item(s)",
            document.bookmarks.len()
        ),
        || {
            let plan = outline_plan(document, allocator.peek_id());
            if let Some(plan) = &plan {
                allocator.reserve_ids(1 + plan.nodes.len());
            }
            plan
        },
    );

    let mut pdf = Pdf::with_settings(Settings::default());
    let (major_version, minor_version) = profile.pdf_version();
    pdf.set_version(major_version, minor_version);
    pdf.set_binary_marker(b"\xE2\xE3\xCF\xD3");

    timings.measure("writing PDF page tree and content objects", || {
        write_catalog(
            &mut pdf,
            catalog_id,
            pages_id,
            metadata_id,
            outline_plan.as_ref(),
            &color_plan,
        );
        write_pages(&mut pdf, pages_id, &page_ids);
        write_font_resources(&mut pdf, font_id, &embedded_font_plans.fonts);
        write_pages_and_content(
            &mut pdf,
            document,
            pages_id,
            font_id,
            &page_ids,
            &content_ids,
            &page_image_ids,
            &page_image_pattern_plans,
            &page_annotation_ids,
            &page_renders,
            &page_ext_gstate_plans,
            &color_plan,
            compression,
        );
    });
    timings.measure("writing PDF embedded font objects", || {
        write_embedded_fonts(&mut pdf, &embedded_font_plans.fonts, compression);
    });
    timings.measure("writing PDF image objects", || {
        write_images(
            &mut pdf,
            &prepared_images,
            &unique_image_ids,
            &color_plan,
            compression,
        );
    });
    timings.measure("writing PDF ICC color profiles", || {
        color_plan.write_profiles(&mut pdf, compression);
    });
    timings.measure("writing PDF image pattern objects", || {
        write_image_patterns(&mut pdf, &page_image_pattern_plans, compression);
    });
    timings.measure("writing SVG gradient shading patterns", || {
        write_gradient_patterns(&mut pdf, &page_renders, &color_plan, compression);
    });
    timings.measure("writing SVG gradient alpha-mask forms", || {
        write_gradient_alpha_forms(&mut pdf, &page_renders, compression);
    });
    timings.measure("writing PDF form XObjects", || {
        write_form_xobjects(
            &mut pdf,
            font_id,
            &page_image_ids,
            &page_image_pattern_plans,
            &page_renders,
            &page_ext_gstate_plans,
            &color_plan,
            compression,
        );
    });
    timings.measure("writing PDF ExtGState objects", || {
        write_ext_gstate_objects(&mut pdf, &page_ext_gstate_plans);
        write_gradient_alpha_ext_gstates(&mut pdf, &page_renders);
    });
    timings.measure("writing PDF metadata, annotations, and outlines", || {
        write_document_info(
            &mut pdf,
            pdf_ref(info_id),
            &document.metadata,
            &options.producer,
        );
        write_document_xmp_metadata(
            &mut pdf,
            pdf_ref(metadata_id),
            &document.metadata,
            profile,
            compression,
            &options.producer,
        );
        write_annotations(&mut pdf, document, &page_annotation_ids);
        if let Some(outline_plan) = &outline_plan {
            write_outlines(&mut pdf, outline_plan, &page_ids, document);
        }
    });
    timings.measure("building deterministic PDF file identifier", || {
        pdf.set_file_id(pdf_file_identifier(
            document,
            &page_renders,
            &embedded_font_plans.fonts,
            &image_plan.unique_images,
            &page_image_pattern_plans,
            &page_ext_gstate_plans,
        ));
    });

    let object_count = allocator.peek_id().saturating_sub(1);
    let bytes = timings.measure(format!("assembling {object_count} PDF object(s)"), || {
        pdf.finish()
    });
    let total = total_timer.finish();
    timings.log_summary(total);
    Ok(bytes)
}

#[derive(Debug, Default)]
struct PdfTimingSummary {
    stages: Vec<PdfTimingStage>,
}

#[derive(Debug)]
struct PdfTimingStage {
    label: String,
    elapsed: Duration,
}

impl PdfTimingSummary {
    fn new() -> Self {
        Self::default()
    }

    fn measure<T>(&mut self, label: impl Into<String>, work: impl FnOnce() -> T) -> T {
        let label = label.into();
        let timer = DebugTimer::start(label.clone());
        let output = work();
        let elapsed = timer.finish();
        self.record(label, elapsed);
        output
    }

    fn record(&mut self, label: impl Into<String>, elapsed: Duration) {
        self.stages.push(PdfTimingStage {
            label: label.into(),
            elapsed,
        });
    }

    fn log_summary(&self, total: Duration) {
        let total_seconds = total.as_secs_f64();
        log::debug!(
            "PDF timing summary: total {:.3?}; nested stages included, percentages are of total and do not sum to 100%",
            total
        );
        for stage in &self.stages {
            let percent = if total_seconds > 0.0 {
                stage.elapsed.as_secs_f64() * 100.0 / total_seconds
            } else {
                0.0
            };
            log::debug!(
                "PDF timing summary: {:>6.2}% {:>10.3?} {}",
                percent,
                stage.elapsed,
                stage.label
            );
        }
    }
}

fn deduplicate_images(document: &Document) -> ImageResourcePlan {
    let image_count = document
        .pages
        .iter()
        .map(|page| page.images.len() + page.image_patterns.len())
        .sum::<usize>();
    let _timer = DebugTimer::start(format!("deduplicating {image_count} image reference(s)"));
    let mut image_lookup = HashMap::new();
    let mut unique_images = Vec::new();
    let page_image_unique_indexes = document
        .pages
        .iter()
        .map(|page| {
            page.images
                .iter()
                .map(|image| {
                    let source = image_source(image);
                    if let Some(index) = image_lookup.get(&source) {
                        *index
                    } else {
                        let index = unique_images.len();
                        image_lookup.insert(source.clone(), PlannedImageIndex(index));
                        unique_images.push(source);
                        PlannedImageIndex(index)
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let page_pattern_tile_unique_indexes = document
        .pages
        .iter()
        .map(|page| {
            page.image_patterns
                .iter()
                .map(|pattern| {
                    let source = image_pattern_source(pattern);
                    if let Some(index) = image_lookup.get(&source) {
                        *index
                    } else {
                        let index = unique_images.len();
                        image_lookup.insert(source.clone(), PlannedImageIndex(index));
                        unique_images.push(source);
                        PlannedImageIndex(index)
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    ImageResourcePlan {
        unique_images,
        page_image_unique_indexes,
        page_pattern_tile_unique_indexes,
    }
}

fn write_catalog(
    pdf: &mut Pdf,
    catalog_id: usize,
    pages_id: usize,
    metadata_id: usize,
    outline_plan: Option<&OutlinePlan>,
    color_plan: &PdfColorPlan,
) {
    let mut catalog = pdf.catalog(pdf_ref(catalog_id));
    catalog.pages(pdf_ref(pages_id));
    catalog.metadata(pdf_ref(metadata_id));
    if let Some(plan) = outline_plan {
        catalog.outlines(pdf_ref(plan.root_id));
    }
    if color_plan.mode() == super::colors::PdfColorMode::SrgbOutputIntent {
        let mut intents = catalog.output_intents();
        let mut intent = intents.push();
        intent
            .subtype(pdf_writer::types::OutputIntentSubtype::PDFA)
            .output_condition_identifier(TextStr("sRGB"))
            .info(TextStr("sRGB IEC 61966-2.1"))
            .dest_output_profile(pdf_ref(color_plan.srgb_profile_object_id()));
    }
}

fn write_pages(pdf: &mut Pdf, pages_id: usize, page_ids: &[usize]) {
    pdf.pages(pdf_ref(pages_id))
        .kids(page_ids.iter().cloned().map(pdf_ref))
        .count(i32_from_usize(page_ids.len()));
}

fn write_font_resources(pdf: &mut Pdf, font_id: usize, embedded_fonts: &[EmbeddedFontPlan<'_>]) {
    let mut fonts = pdf.indirect(pdf_ref(font_id)).dict();
    for font in embedded_fonts {
        fonts.pair(pdf_name(&font.resource_name), pdf_ref(font.type0_id));
    }
}

#[allow(clippy::too_many_arguments)]
fn write_pages_and_content(
    pdf: &mut Pdf,
    document: &Document,
    pages_id: usize,
    font_id: usize,
    page_ids: &[usize],
    content_ids: &[usize],
    page_image_ids: &[Vec<Option<PdfImageObjectId>>],
    page_image_pattern_plans: &[Vec<PageImagePatternPlan>],
    page_annotation_ids: &[Vec<usize>],
    page_renders: &[PageContentRender],
    page_ext_gstate_plans: &[Vec<ExtGStateObjectPlan>],
    color_plan: &PdfColorPlan,
    compression: crate::PdfCompression,
) {
    for (index, page) in document.pages.iter().enumerate() {
        let media_box = crate::document::paint::geometry::paint_rect_to_pdf(
            crate::document::paint::geometry::PaintRect::new(
                crate::document::paint::geometry::PaintPoint::new(0.0, 0.0),
                page.paint_size(),
            ),
        );
        let mut page_writer = pdf.page(pdf_ref(page_ids[index]));
        page_writer
            .parent(pdf_ref(pages_id))
            .media_box(Rect::new(
                media_box.origin.x,
                media_box.origin.y,
                media_box.origin.x + media_box.size.width,
                media_box.origin.y + media_box.size.height,
            ))
            .contents(pdf_ref(content_ids[index]));
        if page.rotation != 0 {
            page_writer.rotate(page.rotation);
        }
        if !page_annotation_ids[index].is_empty() {
            page_writer.annotations(page_annotation_ids[index].iter().cloned().map(pdf_ref));
        }
        {
            let mut resources = page_writer.resources();
            write_resource_dictionary(
                &mut resources,
                font_id,
                &page_image_ids[index],
                &page_image_pattern_plans[index],
                &page_renders[index],
                &page_ext_gstate_plans[index],
                page_renders[index]
                    .form_xobjects
                    .iter()
                    .map(|form| (form.name.as_str(), form.id)),
                color_plan,
            );
        }
    }

    for (index, page_render) in page_renders.iter().enumerate() {
        let stream = encode_pdf_stream(compression, &page_render.stream);
        let mut writer = pdf.stream(pdf_ref(content_ids[index]), stream.bytes());
        if stream.uses_flate() {
            writer.filter(Filter::FlateDecode);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_resource_dictionary<'a>(
    resources: &mut pdf_writer::writers::Resources<'_>,
    font_id: usize,
    page_image_ids: &[Option<PdfImageObjectId>],
    page_image_pattern_plans: &[PageImagePatternPlan],
    page_render: &PageContentRender,
    ext_gstate_plans: &[ExtGStateObjectPlan],
    form_xobjects: impl Iterator<Item = (&'a str, usize)>,
    color_plan: &PdfColorPlan,
) {
    resources.pair(Name(b"Font"), pdf_ref(font_id));
    color_plan.write_page_resources(resources);
    let form_xobjects = form_xobjects.collect::<Vec<_>>();
    if !page_image_ids.is_empty() || !form_xobjects.is_empty() {
        let mut xobjects = resources.x_objects();
        for (image_index, id) in page_image_ids.iter().enumerate() {
            let Some(id) = id else {
                continue;
            };
            let name = format!("Im{}", image_index + 1);
            xobjects.pair(pdf_name(&name), pdf_ref(id.0));
        }
        for (name, id) in form_xobjects {
            xobjects.pair(pdf_name(name), pdf_ref(id));
        }
    }
    if !page_image_pattern_plans.is_empty()
        || !page_render.gradient_patterns.is_empty()
        || !page_render.gradient_tiling_patterns.is_empty()
        || !page_render.svg_tiling_patterns.is_empty()
        || !page_render.svg_path_tiling_patterns.is_empty()
    {
        let mut patterns = resources.patterns();
        for plan in page_image_pattern_plans {
            patterns.pair(pdf_name(&plan.name), pdf_ref(plan.id));
        }
        for plan in &page_render.gradient_patterns {
            patterns.pair(pdf_name(&plan.name), pdf_ref(plan.id));
            if let Some(alpha) = &plan.alpha {
                patterns.pair(pdf_name(&alpha.pattern_name), pdf_ref(alpha.pattern_id));
            }
        }
        for plan in &page_render.gradient_tiling_patterns {
            patterns.pair(pdf_name(&plan.name), pdf_ref(plan.id));
        }
        for plan in &page_render.svg_tiling_patterns {
            patterns.pair(pdf_name(&plan.name), pdf_ref(plan.id));
        }
        for plan in &page_render.svg_path_tiling_patterns {
            patterns.pair(pdf_name(&plan.name), pdf_ref(plan.id));
        }
    }
    write_ext_gstate_resources(resources, ext_gstate_plans, &page_render.gradient_patterns);
}

fn write_ext_gstate_resources(
    resources: &mut pdf_writer::writers::Resources<'_>,
    plans: &[ExtGStateObjectPlan],
    gradients: &[GradientPatternPlan],
) {
    if plans.is_empty() && gradients.iter().all(|gradient| gradient.alpha.is_none()) {
        return;
    }
    let mut ext_gstates = resources.ext_g_states();
    for plan in plans {
        ext_gstates.pair(pdf_name(plan.resource.name()), pdf_ref(plan.id));
    }
    for alpha in gradients
        .iter()
        .filter_map(|gradient| gradient.alpha.as_ref())
    {
        ext_gstates.pair(
            pdf_name(&alpha.ext_gstate_name),
            pdf_ref(alpha.ext_gstate_id),
        );
    }
}

fn write_embedded_fonts(
    pdf: &mut Pdf,
    embedded_fonts: &[EmbeddedFontPlan<'_>],
    compression: crate::PdfCompression,
) {
    let _timer = DebugTimer::start(format!(
        "building {} embedded font object set(s)",
        embedded_fonts.len()
    ));
    for font in embedded_fonts {
        pdf.type0_font(pdf_ref(font.type0_id))
            .base_font(pdf_name(&font.base_name))
            .encoding_predefined(Name(b"Identity-H"))
            .descendant_font(pdf_ref(font.cid_font_id))
            .to_unicode(pdf_ref(font.to_unicode_id));

        let subtype = match font.font_program_kind {
            FontProgramKind::TrueType => CidFontType::Type2,
            FontProgramKind::OpenTypeCff => CidFontType::Type0,
        };
        {
            let mut cid_font = pdf.cid_font(pdf_ref(font.cid_font_id));
            cid_font
                .subtype(subtype)
                .base_font(pdf_name(&font.base_name))
                .system_info(SystemInfo {
                    registry: Str(b"Adobe"),
                    ordering: Str(b"Identity"),
                    supplement: 0,
                })
                .font_descriptor(pdf_ref(font.descriptor_id))
                .default_width(font.default_width);
            // CIDToGIDMap belongs to CIDFontType2 (TrueType) only. OpenType
            // CFF programs use the CFF character identifiers directly; adding
            // an Identity map makes PDF consumers address unrelated outlines.
            // ISO 32000-2:2020, 9.7.4.3, CIDFontType2 dictionaries.
            if font.font_program_kind == FontProgramKind::TrueType {
                cid_font.cid_to_gid_map_predefined(Name(b"Identity"));
            }
            {
                let mut widths = cid_font.widths();
                for (glyph_id, width) in cid_width_entries(font) {
                    widths.consecutive(glyph_id, [width]);
                }
            }
        }

        let metrics = &font.descriptor_metrics;
        {
            let mut descriptor = pdf.font_descriptor(pdf_ref(font.descriptor_id));
            descriptor
                .name(pdf_name(&font.base_name))
                .flags(FontFlags::from_bits_retain(metrics.flags))
                .bbox(Rect::new(
                    metrics.bbox[0] as f32,
                    metrics.bbox[1] as f32,
                    metrics.bbox[2] as f32,
                    metrics.bbox[3] as f32,
                ))
                .italic_angle(metrics.italic_angle)
                .ascent(metrics.ascent)
                .descent(metrics.descent)
                .cap_height(metrics.cap_height)
                .stem_v(metrics.stem_v);
            if let Some(x_height) = metrics.x_height {
                descriptor.x_height(x_height);
            }
            if let Some(avg_width) = metrics.avg_width {
                descriptor.avg_width(avg_width);
            }
            if let Some(max_width) = metrics.max_width {
                descriptor.max_width(max_width);
            }
            if let Some(missing_width) = metrics.missing_width {
                descriptor.missing_width(missing_width);
            }
            if let Some(cid_set_id) = font.cid_set_id {
                descriptor.cid_set(pdf_ref(cid_set_id));
            }
            match font.font_program_kind {
                FontProgramKind::TrueType => {
                    descriptor.font_file2(pdf_ref(font.file_id));
                }
                FontProgramKind::OpenTypeCff => {
                    descriptor.font_file3(pdf_ref(font.file_id));
                }
            }
        }

        log_embedded_font_file(font);
        let data = font.font_file_data.as_slice();
        let stream = encode_pdf_stream(compression, data);
        {
            let mut font_file = pdf.stream(pdf_ref(font.file_id), stream.bytes());
            if stream.uses_flate() {
                font_file.filter(Filter::FlateDecode);
            }
            match font.font_program_kind {
                FontProgramKind::TrueType => {
                    font_file.pair(Name(b"Length1"), i32_from_usize(data.len()));
                }
                FontProgramKind::OpenTypeCff => {
                    font_file.pair(Name(b"Subtype"), Name(b"CIDFontType0C"));
                }
            }
        }

        let cmap = to_unicode_cmap(font);
        let cmap_stream = encode_pdf_stream(compression, &cmap);
        {
            let mut cmap_writer = pdf.cmap(pdf_ref(font.to_unicode_id), cmap_stream.bytes());
            if cmap_stream.uses_flate() {
                cmap_writer.filter(Filter::FlateDecode);
            }
        }
        if let (Some(cid_set_id), Some(cid_set_data)) =
            (font.cid_set_id, font.cid_set_data.as_ref())
        {
            let cid_set_stream = encode_pdf_stream(compression, cid_set_data);
            let mut cid_set_writer = pdf.stream(pdf_ref(cid_set_id), cid_set_stream.bytes());
            if cid_set_stream.uses_flate() {
                cid_set_writer.filter(Filter::FlateDecode);
            }
        }
    }
}

fn write_images(
    pdf: &mut Pdf,
    prepared_images: &[PreparedImageResource],
    unique_image_ids: &[Option<ImageObjectIds>],
    color_plan: &PdfColorPlan,
    compression: crate::PdfCompression,
) {
    let raster_count = prepared_images
        .iter()
        .filter(|image| matches!(image, PreparedImageResource::Raster(_)))
        .count();
    let _timer = DebugTimer::start(format!("building {raster_count} image object(s)"));
    for (image, ids) in prepared_images.iter().zip(unique_image_ids) {
        let PreparedImageResource::Raster(image) = image else {
            continue;
        };
        let ids = ids.expect("raster images receive PDF object IDs");
        let sample_stream = match &image.payload {
            ImagePayload::Samples { rgb, .. } => Some(encode_pdf_stream(compression, rgb)),
            ImagePayload::Jpeg(_) => None,
        };
        {
            let image_bytes = match (&sample_stream, &image.payload) {
                (Some(stream), ImagePayload::Samples { .. }) => stream.bytes(),
                (None, ImagePayload::Jpeg(bytes)) => bytes.as_ref(),
                _ => unreachable!("PDF image payload and stream plan must agree"),
            };
            let mut image_writer = pdf.image_xobject(pdf_ref(ids.image_id.0), image_bytes);
            image_writer
                .width(i32_from_u32(image.pixel_width))
                .height(i32_from_u32(image.pixel_height))
                .bits_per_component(8)
                .interpolate(image.interpolate);
            image_writer.color_space().icc_based(pdf_ref(
                color_plan.image_profile_object_id(&image.color_space),
            ));
            if sample_stream
                .as_ref()
                .is_some_and(|stream| stream.uses_flate())
            {
                image_writer.filter(Filter::FlateDecode);
            }
            if matches!(&image.payload, ImagePayload::Jpeg(_)) {
                image_writer.filter(Filter::DctDecode);
            }
            if let (Some(mask_id), ImagePayload::Samples { alpha: Some(_), .. }) =
                (ids.alpha_mask_id, &image.payload)
            {
                image_writer.s_mask(pdf_ref(mask_id.0));
            }
        }
        if let (
            Some(mask_id),
            ImagePayload::Samples {
                alpha: Some(alpha), ..
            },
        ) = (ids.alpha_mask_id, &image.payload)
        {
            let alpha_stream = encode_pdf_stream(compression, alpha);
            let mut alpha_writer = pdf.image_xobject(pdf_ref(mask_id.0), alpha_stream.bytes());
            alpha_writer
                .width(i32_from_u32(image.pixel_width))
                .height(i32_from_u32(image.pixel_height))
                .color_space_name(Name(b"DeviceGray"))
                .bits_per_component(8)
                .interpolate(image.interpolate);
            if alpha_stream.uses_flate() {
                alpha_writer.filter(Filter::FlateDecode);
            }
        }
    }
}

/// Emit PDF tiling pattern streams for repeated raster CSS backgrounds.
///
/// ISO 32000-1:2008, 8.7.3 defines tiling patterns as reusable cells painted
/// by the PDF consumer. Each Quire pattern cell paints one image XObject at the
/// used CSS background tile size; the page content stream clips and fills the
/// background painting area with the pattern color space.
fn write_image_patterns(
    pdf: &mut Pdf,
    page_image_pattern_plans: &[Vec<PageImagePatternPlan>],
    compression: crate::PdfCompression,
) {
    for plan in page_image_pattern_plans.iter().flatten() {
        let pattern = &plan.pattern;
        if pattern.tiling.tile_size.width <= 0.0
            || pattern.tiling.tile_size.height <= 0.0
            || pattern.tiling.step.width <= 0.0
            || pattern.tiling.step.height <= 0.0
        {
            continue;
        }

        let mut content = Content::new();
        content
            .transform([
                pattern.tiling.tile_size.width,
                0.0,
                0.0,
                pattern.tiling.tile_size.height,
                0.0,
                0.0,
            ])
            .x_object(pdf_name("Im1"));
        let stream = content.finish().into_vec();
        let stream = encode_pdf_stream(compression, &stream);
        let mut pattern_writer = pdf.tiling_pattern(pdf_ref(plan.id), stream.bytes());
        if stream.uses_flate() {
            pattern_writer.filter(Filter::FlateDecode);
        }
        pattern_writer
            .paint_type(PaintType::Colored)
            .tiling_type(TilingType::ConstantSpacing)
            .bbox(Rect::new(
                0.0,
                0.0,
                pattern.tiling.tile_size.width,
                pattern.tiling.tile_size.height,
            ))
            .x_step(pattern.tiling.step.width)
            .y_step(pattern.tiling.step.height)
            .matrix([
                1.0,
                0.0,
                0.0,
                1.0,
                pattern.tiling.origin.x,
                pattern.tiling.origin.y,
            ]);
        {
            let mut resources = pattern_writer.resources();
            let mut xobjects = resources.x_objects();
            xobjects.pair(pdf_name("Im1"), pdf_ref(plan.tile_image_id.0));
        }
    }
}

/// Emit native PDF shading patterns for SVG and CSS linear/radial gradients.
///
/// SVG 2, 13.2 maps to PDF axial/radial shadings. Each stop interval is a
/// Type 2 exponential interpolation; three or more intervals use a Type 3
/// stitching function as defined by ISO 32000-2:2020, 8.9.1-8.9.3.
fn write_gradient_patterns(
    pdf: &mut Pdf,
    page_renders: &[PageContentRender],
    color_plan: &PdfColorPlan,
    compression: crate::PdfCompression,
) {
    for plan in page_renders
        .iter()
        .flat_map(|render| &render.gradient_patterns)
    {
        let output_space = color_plan.gradient_output_space(&plan.gradient);
        if let Some(periodic) = &plan.gradient.periodic {
            write_periodic_gradient_color_function(
                pdf,
                plan.function_ids[0],
                periodic,
                color_plan,
                output_space,
            );
            write_gradient_shading_pattern(
                pdf,
                plan,
                color_plan,
                plan.function_ids[0],
                output_space,
            );
            if let Some(alpha) = &plan.alpha {
                write_periodic_gradient_alpha_pattern(pdf, plan, alpha, periodic);
            }
            continue;
        }
        let interval_count = plan.gradient.stops.len().saturating_sub(1);
        if interval_count == 0 {
            continue;
        }
        let (interval_ids, stitching_id) = if interval_count == 1 {
            (&plan.function_ids[..1], None)
        } else {
            (
                &plan.function_ids[..interval_count],
                plan.function_ids.get(interval_count).cloned(),
            )
        };
        for (interval, id) in plan.gradient.stops.windows(2).zip(interval_ids) {
            let from = color_plan.gradient_color(interval[0].color, output_space);
            let to = color_plan.gradient_color(interval[1].color, output_space);
            if (from.alpha() - to.alpha()).abs() > 0.0001 {
                write_premultiplied_gradient_color_function(
                    pdf,
                    *id,
                    from,
                    to,
                    interval[0].interpolation_exponent,
                );
            } else {
                pdf.exponential_function(pdf_ref(*id))
                    .domain([0.0, 1.0])
                    .c0([
                        from.components()[0],
                        from.components()[1],
                        from.components()[2],
                    ])
                    .c1([to.components()[0], to.components()[1], to.components()[2]])
                    .n(interval[0].interpolation_exponent);
            }
        }
        let function_id = if let Some(stitching_id) = stitching_id {
            let bounds = plan.gradient.stops[1..plan.gradient.stops.len() - 1]
                .iter()
                .map(|stop| stop.offset);
            pdf.stitching_function(pdf_ref(stitching_id))
                .domain([0.0, 1.0])
                .functions(interval_ids.iter().cloned().map(pdf_ref))
                .bounds(bounds)
                .encode((0..interval_count).flat_map(|_| [0.0, 1.0]));
            stitching_id
        } else {
            interval_ids[0]
        };
        write_gradient_shading_pattern(pdf, plan, color_plan, function_id, output_space);
        if let Some(alpha) = &plan.alpha {
            write_gradient_alpha_pattern(pdf, plan, alpha);
        }
    }
    for plan in page_renders
        .iter()
        .flat_map(|render| &render.gradient_tiling_patterns)
    {
        let pattern = &plan.pattern;
        let mut content = Content::new();
        content.save_state();
        if let Some(name) = &plan.alpha_gstate_name {
            content.set_parameters(pdf_name(name));
        }
        content
            .set_fill_color_space(ColorSpaceOperand::Pattern)
            .set_fill_pattern([], pdf_name(&plan.shading_pattern_name))
            .rect(
                0.0,
                0.0,
                pattern.tiling.tile_size.width,
                pattern.tiling.tile_size.height,
            )
            .fill_nonzero()
            .restore_state();
        let stream = content.finish().into_vec();
        let stream = encode_pdf_stream(compression, &stream);
        let mut writer = pdf.tiling_pattern(pdf_ref(plan.id), stream.bytes());
        if stream.uses_flate() {
            writer.filter(Filter::FlateDecode);
        }
        writer
            .paint_type(PaintType::Colored)
            .tiling_type(TilingType::ConstantSpacing)
            .bbox(Rect::new(
                0.0,
                0.0,
                pattern.tiling.tile_size.width,
                pattern.tiling.tile_size.height,
            ))
            .x_step(pattern.tiling.step.width)
            .y_step(pattern.tiling.step.height)
            .matrix([
                1.0,
                0.0,
                0.0,
                1.0,
                pattern.tiling.origin.x,
                pattern.tiling.origin.y,
            ]);
        let mut resources = writer.resources();
        resources.patterns().pair(
            pdf_name(&plan.shading_pattern_name),
            pdf_ref(find_gradient_pattern_id(
                page_renders,
                &plan.shading_pattern_name,
            )),
        );
        if let Some(name) = &plan.alpha_gstate_name {
            resources.ext_g_states().pair(
                pdf_name(name),
                pdf_ref(find_gradient_alpha_gstate_id(
                    page_renders,
                    &plan.shading_pattern_name,
                )),
            );
        }
    }
    for plan in page_renders
        .iter()
        .flat_map(|render| &render.svg_tiling_patterns)
    {
        let pattern = &plan.pattern;
        let mut content = Content::new();
        content.x_object(pdf_name(&plan.form_name));
        let stream = content.finish().into_vec();
        let stream = encode_pdf_stream(compression, &stream);
        let mut writer = pdf.tiling_pattern(pdf_ref(plan.id), stream.bytes());
        if stream.uses_flate() {
            writer.filter(Filter::FlateDecode);
        }
        writer
            .paint_type(PaintType::Colored)
            .tiling_type(TilingType::ConstantSpacing)
            .bbox(Rect::new(
                0.0,
                0.0,
                pattern.tiling.tile_size.width,
                pattern.tiling.tile_size.height,
            ))
            .x_step(pattern.tiling.step.width)
            .y_step(pattern.tiling.step.height)
            .matrix(plan.transform.pdf_components());
        writer
            .resources()
            .x_objects()
            .pair(pdf_name(&plan.form_name), pdf_ref(plan.form_id));
    }
    for plan in page_renders
        .iter()
        .flat_map(|render| &render.svg_path_tiling_patterns)
    {
        let pattern = &plan.pattern;
        let stream = svg_path_pattern_tile_content(pattern, color_plan.mode());
        let stream = encode_pdf_stream(compression, &stream);
        let mut writer = pdf.tiling_pattern(pdf_ref(plan.id), stream.bytes());
        if stream.uses_flate() {
            writer.filter(Filter::FlateDecode);
        }
        let translation = crate::document::paint::geometry::PaintTransform::translate(
            crate::document::paint::geometry::PaintTranslation::new(
                pattern.origin.x,
                pattern.origin.y,
            ),
        );
        let matrix = pattern.transform.multiply(translation);
        writer
            .paint_type(PaintType::Colored)
            .tiling_type(TilingType::ConstantSpacing)
            .bbox(Rect::new(
                0.0,
                0.0,
                pattern.tile_size.width,
                pattern.tile_size.height,
            ))
            .x_step(pattern.tile_size.width)
            .y_step(pattern.tile_size.height)
            .matrix(matrix.pdf_components());
        // A tiling pattern has a Resources entry even when its supported
        // solid-vector cell needs no named resources.  Some PDF consumers
        // reject an omitted dictionary rather than treating it as empty.
        color_plan.write_page_resources(&mut writer.resources());
    }
}

fn find_gradient_pattern_id(page_renders: &[PageContentRender], name: &str) -> usize {
    page_renders
        .iter()
        .flat_map(|render| &render.gradient_patterns)
        .find(|plan| plan.name == name)
        .map(|plan| plan.id)
        .expect("tiling gradient must reference a page shading pattern")
}

fn write_gradient_shading_pattern(
    pdf: &mut Pdf,
    plan: &GradientPatternPlan,
    color_plan: &PdfColorPlan,
    function_id: usize,
    output_space: crate::css::CssColorSpace,
) {
    let mut pattern = pdf.shading_pattern(pdf_ref(plan.id));
    pattern.matrix(plan.gradient.transform.pdf_components());
    let mut shading = pattern.function_shading();
    shading
        .color_space()
        .icc_based(pdf_ref(color_plan.profile_object_id(output_space)));
    shading.function(pdf_ref(function_id)).extend([true, true]);
    match &plan.gradient.kind {
        crate::document::paint::paths::RenderedGradientKind::Linear { start, end } => {
            shading
                .shading_type(FunctionShadingType::Axial)
                .coords([start.x, start.y, end.x, end.y]);
        }
        crate::document::paint::paths::RenderedGradientKind::Radial {
            start_center,
            start_radius,
            end_center,
            end_radius,
        } => {
            shading.shading_type(FunctionShadingType::Radial).coords([
                start_center.x,
                start_center.y,
                *start_radius,
                end_center.x,
                end_center.y,
                *end_radius,
            ]);
        }
    }
}

/// Find the soft-mask graphics state paired with a shading resource. The
/// enclosing Type 1 tiling pattern has its own resource dictionary, so it
/// cannot rely on the page-level ExtGState entry used by a direct path paint.
fn find_gradient_alpha_gstate_id(page_renders: &[PageContentRender], name: &str) -> usize {
    page_renders
        .iter()
        .flat_map(|render| &render.gradient_patterns)
        .find(|plan| plan.name == name)
        .and_then(|plan| plan.alpha.as_ref())
        .map(|alpha| alpha.ext_gstate_id)
        .expect("tiling gradient alpha resource must have an ExtGState")
}

/// Emit a Type 4 function for an interval whose opacity changes.
///
/// CSS interpolates premultiplied colors, while PDF applies the color shading
/// and its soft mask independently. This calculator unpremultiplies each
/// interpolated component before the paired soft mask composites it.
///
/// CSS Images 3, 3.4.1: <https://www.w3.org/TR/css-images-3/#coloring-gradient-line>
/// ISO 32000-2:2020, 8.9.4 (Type 4 calculator functions).
fn write_premultiplied_gradient_color_function(
    pdf: &mut Pdf,
    id: usize,
    from: PdfPaintColor,
    to: PdfPaintColor,
    exponent: f32,
) {
    let alpha_delta = to.alpha() - from.alpha();
    let channel = |start: f32, end: f32| {
        let start = start * from.alpha();
        let delta = end * to.alpha() - start;
        // Copy the saved t^N below the preceding output, then use a second
        // copy for alpha. The original remains for the next channel.
        format!(
            "1 index {delta} mul {start} add 1 index {alpha_delta} mul {} add dup 0 eq {{ pop pop 0 }} {{ div }} ifelse",
            from.alpha(),
        )
    };
    // Keep four copies of t^N, one beneath each output component. After the
    // three channel calculations, rotate those scratch values to the top and
    // discard them, leaving R G B as the function's outputs.
    let code = format!(
        "{{ {exponent} exp dup dup dup {} {} {} 7 3 roll pop pop pop pop }}",
        channel(from.components()[0], to.components()[0]),
        channel(from.components()[1], to.components()[1]),
        channel(from.components()[2], to.components()[2]),
    );
    pdf.post_script_function(pdf_ref(id), code.as_bytes())
        .domain([0.0, 1.0])
        .range([0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
}

fn write_periodic_gradient_color_function(
    pdf: &mut Pdf,
    id: usize,
    periodic: &crate::document::paint::paths::RenderedPeriodicGradient,
    color_plan: &PdfColorPlan,
    output_space: crate::css::CssColorSpace,
) {
    let stops = periodic
        .stops
        .iter()
        .map(|stop| PdfPeriodicGradientStop {
            offset: stop.offset,
            interpolation_exponent: stop.interpolation_exponent,
            color: color_plan.gradient_color(stop.color, output_space),
        })
        .collect::<Vec<_>>();
    let code = periodic_gradient_pdf_function_code(periodic, &stops);
    pdf.post_script_function(pdf_ref(id), code.as_bytes())
        .domain([0.0, 1.0])
        .range([0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
}

#[derive(Clone, Copy)]
struct PdfPeriodicGradientStop {
    offset: f32,
    interpolation_exponent: f32,
    color: PdfPaintColor,
}

fn periodic_gradient_pdf_function_code(
    periodic: &crate::document::paint::paths::RenderedPeriodicGradient,
    stops: &[PdfPeriodicGradientStop],
) -> String {
    fn interval_code(left: &PdfPeriodicGradientStop, right: &PdfPeriodicGradientStop) -> String {
        let span = right.offset - left.offset;
        let progress = format!(
            "{} sub {} div {} exp",
            left.offset, span, left.interpolation_exponent
        );
        let alpha_delta = right.color.alpha() - left.color.alpha();
        let channel = |start: f32, end: f32| {
            let start = start * left.color.alpha();
            let delta = end * right.color.alpha() - start;
            format!(
                "1 index {delta} mul {start} add 1 index {alpha_delta} mul {} add dup 0 eq {{ pop pop 0 }} {{ div }} ifelse",
                left.color.alpha()
            )
        };
        let left_components = left.color.components();
        let right_components = right.color.components();
        format!(
            "{progress} dup dup dup {} {} {} 7 3 roll pop pop pop pop",
            channel(left_components[0], right_components[0]),
            channel(left_components[1], right_components[1]),
            channel(left_components[2], right_components[2])
        )
    }
    let intervals = stops
        .windows(2)
        .filter(|pair| pair[1].offset > pair[0].offset)
        .collect::<Vec<_>>();
    debug_assert!(!intervals.is_empty());
    let last = intervals.last().expect("non-degenerate periodic gradient");
    let mut selection = interval_code(&last[0], &last[1]);
    for pair in intervals.iter().rev().skip(1) {
        selection = format!(
            "dup {} lt {{ {} }} {{ {} }} ifelse",
            pair[1].offset,
            interval_code(&pair[0], &pair[1]),
            selection
        );
    }
    format!(
        "{{ {} mul {} sub dup {} div floor {} mul sub {} add {} }}",
        periodic.domain_length,
        periodic.start,
        periodic.period,
        periodic.period,
        periodic.start,
        selection
    )
}

fn periodic_gradient_function_code(
    periodic: &crate::document::paint::paths::RenderedPeriodicGradient,
    stops: &[crate::document::paint::paths::RenderedGradientStop],
    alpha_only: bool,
) -> String {
    fn interval_code(
        left: &crate::document::paint::paths::RenderedGradientStop,
        right: &crate::document::paint::paths::RenderedGradientStop,
        alpha_only: bool,
    ) -> String {
        let span = right.offset - left.offset;
        let progress = format!(
            "{} sub {} div {} exp",
            left.offset, span, left.interpolation_exponent
        );
        if alpha_only {
            return format!(
                "{progress} {} mul {} add",
                right.color.alpha() - left.color.alpha(),
                left.color.alpha()
            );
        }
        let alpha_delta = right.color.alpha() - left.color.alpha();
        let channel = |start: f32, end: f32| {
            let start = start * left.color.alpha();
            let delta = end * right.color.alpha() - start;
            format!(
                "1 index {delta} mul {start} add 1 index {alpha_delta} mul {} add dup 0 eq {{ pop pop 0 }} {{ div }} ifelse",
                left.color.alpha()
            )
        };
        format!(
            "{progress} dup dup dup {} {} {} 7 3 roll pop pop pop pop",
            channel(left.color.components()[0], right.color.components()[0]),
            channel(left.color.components()[1], right.color.components()[1]),
            channel(left.color.components()[2], right.color.components()[2])
        )
    }
    // Coincident stops are CSS hard stops, not zero-length interpolation
    // intervals. Selecting only positive-width intervals both avoids a
    // division by zero in the calculator and gives the later stop precedence
    // at its position: `lt` falls through to the next interval at a shared
    // boundary.
    let intervals = stops
        .windows(2)
        .filter(|pair| pair[1].offset > pair[0].offset)
        .collect::<Vec<_>>();
    debug_assert!(
        !intervals.is_empty(),
        "non-degenerate periodic gradients must contain an interval"
    );
    let last = intervals
        .last()
        .expect("non-degenerate periodic gradients must contain an interval");
    let mut selection = interval_code(&last[0], &last[1], alpha_only);
    for pair in intervals.iter().rev().skip(1) {
        selection = format!(
            "dup {} lt {{ {} }} {{ {} }} ifelse",
            pair[1].offset,
            interval_code(&pair[0], &pair[1], alpha_only),
            selection
        );
    }
    format!(
        "{{ {} mul {} sub dup {} div floor {} mul sub {} add {} }}",
        periodic.domain_length,
        periodic.start,
        periodic.period,
        periodic.period,
        periodic.start,
        selection
    )
}

fn write_periodic_gradient_alpha_pattern(
    pdf: &mut Pdf,
    plan: &GradientPatternPlan,
    alpha: &GradientAlphaPlan,
    periodic: &crate::document::paint::paths::RenderedPeriodicGradient,
) {
    let code = periodic_gradient_function_code(periodic, &periodic.stops, true);
    pdf.post_script_function(pdf_ref(alpha.function_ids[0]), code.as_bytes())
        .domain([0.0, 1.0])
        .range([0.0, 1.0]);
    let mut pattern = pdf.shading_pattern(pdf_ref(alpha.pattern_id));
    pattern.matrix(plan.gradient.transform.pdf_components());
    let mut shading = pattern.function_shading();
    shading.color_space().device_gray();
    shading
        .function(pdf_ref(alpha.function_ids[0]))
        .extend([true, true]);
    match &plan.gradient.kind {
        crate::document::paint::paths::RenderedGradientKind::Linear { start, end } => {
            shading
                .shading_type(FunctionShadingType::Axial)
                .coords([start.x, start.y, end.x, end.y]);
        }
        crate::document::paint::paths::RenderedGradientKind::Radial {
            start_center,
            start_radius,
            end_center,
            end_radius,
        } => {
            shading.shading_type(FunctionShadingType::Radial).coords([
                start_center.x,
                start_center.y,
                *start_radius,
                end_center.x,
                end_center.y,
                *end_radius,
            ]);
        }
    }
}

fn write_gradient_alpha_pattern(
    pdf: &mut Pdf,
    plan: &GradientPatternPlan,
    alpha: &GradientAlphaPlan,
) {
    let interval_count = plan.gradient.stops.len().saturating_sub(1);
    if interval_count == 0 {
        return;
    }
    let (interval_ids, stitching_id) = if interval_count == 1 {
        (&alpha.function_ids[..1], None)
    } else {
        (
            &alpha.function_ids[..interval_count],
            alpha.function_ids.get(interval_count).cloned(),
        )
    };
    for (interval, id) in plan.gradient.stops.windows(2).zip(interval_ids) {
        pdf.exponential_function(pdf_ref(*id))
            .domain([0.0, 1.0])
            .c0([interval[0].color.alpha()])
            .c1([interval[1].color.alpha()])
            .n(interval[0].interpolation_exponent);
    }
    let function_id = if let Some(stitching_id) = stitching_id {
        let bounds = plan.gradient.stops[1..plan.gradient.stops.len() - 1]
            .iter()
            .map(|stop| stop.offset);
        pdf.stitching_function(pdf_ref(stitching_id))
            .domain([0.0, 1.0])
            .functions(interval_ids.iter().cloned().map(pdf_ref))
            .bounds(bounds)
            .encode((0..interval_count).flat_map(|_| [0.0, 1.0]));
        stitching_id
    } else {
        interval_ids[0]
    };
    let transform = plan.gradient.transform;
    let mut pattern = pdf.shading_pattern(pdf_ref(alpha.pattern_id));
    pattern.matrix(transform.pdf_components());
    let mut shading = pattern.function_shading();
    shading.color_space().device_gray();
    shading.function(pdf_ref(function_id)).extend([true, true]);
    match &plan.gradient.kind {
        crate::document::paint::paths::RenderedGradientKind::Linear { start, end } => {
            shading
                .shading_type(FunctionShadingType::Axial)
                .coords([start.x, start.y, end.x, end.y]);
        }
        crate::document::paint::paths::RenderedGradientKind::Radial {
            start_center,
            start_radius,
            end_center,
            end_radius,
        } => {
            shading.shading_type(FunctionShadingType::Radial).coords([
                start_center.x,
                start_center.y,
                *start_radius,
                end_center.x,
                end_center.y,
                *end_radius,
            ]);
        }
    }
}

fn write_gradient_alpha_forms(
    pdf: &mut Pdf,
    page_renders: &[PageContentRender],
    compression: crate::PdfCompression,
) {
    for plan in page_renders
        .iter()
        .flat_map(|render| &render.gradient_patterns)
    {
        let Some(alpha) = &plan.alpha else {
            continue;
        };
        let mut content = Content::new();
        content
            .set_fill_color_space(ColorSpaceOperand::Pattern)
            .set_fill_pattern([], pdf_name(&alpha.pattern_name))
            .rect(0.0, 0.0, alpha.page_size.width, alpha.page_size.height)
            .fill_nonzero();
        let stream = content.finish().into_vec();
        let stream = encode_pdf_stream(compression, &stream);
        let mut form = pdf.form_xobject(pdf_ref(alpha.form_id), stream.bytes());
        if stream.uses_flate() {
            form.filter(Filter::FlateDecode);
        }
        form.bbox(Rect::new(
            0.0,
            0.0,
            alpha.page_size.width,
            alpha.page_size.height,
        ));
        {
            let mut group = form.group();
            group.transparency();
            group.color_space().device_gray();
        }
        let mut resources = form.resources();
        let mut patterns = resources.patterns();
        patterns.pair(pdf_name(&alpha.pattern_name), pdf_ref(alpha.pattern_id));
    }
}

fn write_gradient_alpha_ext_gstates(pdf: &mut Pdf, page_renders: &[PageContentRender]) {
    for alpha in page_renders
        .iter()
        .flat_map(|render| &render.gradient_patterns)
        .filter_map(|gradient| gradient.alpha.as_ref())
    {
        let mut state = pdf.ext_graphics(pdf_ref(alpha.ext_gstate_id));
        state
            .soft_mask()
            .subtype(MaskType::Luminosity)
            .group(pdf_ref(alpha.form_id));
    }
}

#[allow(clippy::too_many_arguments)]
fn write_form_xobjects(
    pdf: &mut Pdf,
    font_id: usize,
    page_image_ids: &[Vec<Option<PdfImageObjectId>>],
    page_image_pattern_plans: &[Vec<PageImagePatternPlan>],
    page_renders: &[PageContentRender],
    page_ext_gstate_plans: &[Vec<ExtGStateObjectPlan>],
    color_plan: &PdfColorPlan,
    compression: crate::PdfCompression,
) {
    for (page_index, page_render) in page_renders.iter().enumerate() {
        for form in &page_render.form_xobjects {
            let stream = encode_pdf_stream(compression, &form.stream);
            let mut form_writer = pdf.form_xobject(pdf_ref(form.id), stream.bytes());
            if stream.uses_flate() {
                form_writer.filter(Filter::FlateDecode);
            }
            form_writer.bbox(Rect::new(
                form.bbox.x(),
                form.bbox.y(),
                form.bbox.x() + form.bbox.width(),
                form.bbox.y() + form.bbox.height(),
            ));
            if form.transparency_group {
                form_writer.group().transparency();
            }
            {
                let mut resources = form_writer.resources();
                if form.transparency_group {
                    write_resource_dictionary(
                        &mut resources,
                        font_id,
                        &page_image_ids[page_index],
                        &page_image_pattern_plans[page_index],
                        page_render,
                        &page_ext_gstate_plans[page_index],
                        form.form_dependencies
                            .iter()
                            .map(|dependency| (dependency.name.as_str(), dependency.id)),
                        color_plan,
                    );
                } else {
                    // A tile form is called by its owning tiling pattern.
                    // Giving it the page's complete XObject dictionary would
                    // make the form reference itself, which PDF consumers
                    // correctly treat as a recursive resource. Keep only
                    // resources that a vector tile path can actually use.
                    write_svg_tile_resource_dictionary(
                        &mut resources,
                        &page_image_ids[page_index],
                        &page_image_pattern_plans[page_index],
                        page_render,
                        &page_ext_gstate_plans[page_index],
                        color_plan,
                    );
                }
            }
        }
    }
}

fn write_svg_tile_resource_dictionary(
    resources: &mut pdf_writer::writers::Resources<'_>,
    page_image_ids: &[Option<PdfImageObjectId>],
    page_image_pattern_plans: &[PageImagePatternPlan],
    page_render: &PageContentRender,
    ext_gstate_plans: &[ExtGStateObjectPlan],
    color_plan: &PdfColorPlan,
) {
    // SVG text is intentionally unsupported, so a tile Form never needs the
    // document's text font. Omitting the empty page-font dictionary also
    // keeps PDF validators from treating it as an invalid font resource.
    color_plan.write_page_resources(resources);
    if !page_image_ids.is_empty() {
        let mut xobjects = resources.x_objects();
        for (index, image_id) in page_image_ids.iter().enumerate() {
            let Some(image_id) = image_id else {
                continue;
            };
            xobjects.pair(pdf_name(&format!("Im{}", index + 1)), pdf_ref(image_id.0));
        }
    }
    if !page_image_pattern_plans.is_empty()
        || !page_render.gradient_patterns.is_empty()
        || !page_render.gradient_tiling_patterns.is_empty()
        || !page_render.svg_path_tiling_patterns.is_empty()
    {
        let mut patterns = resources.patterns();
        for plan in page_image_pattern_plans {
            patterns.pair(pdf_name(&plan.name), pdf_ref(plan.id));
        }
        for plan in &page_render.gradient_patterns {
            patterns.pair(pdf_name(&plan.name), pdf_ref(plan.id));
            if let Some(alpha) = &plan.alpha {
                patterns.pair(pdf_name(&alpha.pattern_name), pdf_ref(alpha.pattern_id));
            }
        }
        for plan in &page_render.gradient_tiling_patterns {
            patterns.pair(pdf_name(&plan.name), pdf_ref(plan.id));
        }
        for plan in &page_render.svg_path_tiling_patterns {
            patterns.pair(pdf_name(&plan.name), pdf_ref(plan.id));
        }
    }
    write_ext_gstate_resources(resources, ext_gstate_plans, &page_render.gradient_patterns);
}

fn write_ext_gstate_objects(pdf: &mut Pdf, page_ext_gstate_plans: &[Vec<ExtGStateObjectPlan>]) {
    for plan in page_ext_gstate_plans.iter().flatten() {
        let mut ext_gstate = pdf.ext_graphics(pdf_ref(plan.id));
        match &plan.resource {
            ExtGStateResource::Alpha { alpha, .. } => {
                ext_gstate.non_stroking_alpha(*alpha).stroking_alpha(*alpha);
            }
            ExtGStateResource::Blend { mode, .. } => {
                ext_gstate.blend_mode((*mode).into());
            }
        }
    }
}

fn write_annotations(pdf: &mut Pdf, document: &Document, page_annotation_ids: &[Vec<usize>]) {
    for (page_index, page) in document.pages.iter().enumerate() {
        for (link_index, link) in page.links.iter().enumerate() {
            let mut annotation =
                pdf.annotation(pdf_ref(page_annotation_ids[page_index][link_index]));
            let rect = crate::document::paint::geometry::paint_rect_to_pdf(link.paint_rect());
            annotation
                .subtype(AnnotationType::Link)
                .rect(Rect::new(
                    rect.origin.x,
                    rect.origin.y,
                    rect.origin.x + rect.size.width,
                    rect.origin.y + rect.size.height,
                ))
                .border(0.0, 0.0, 0.0, None);
            annotation
                .action()
                .action_type(ActionType::Uri)
                .uri(Str(link.target.as_bytes()));
        }
    }
}

fn write_outlines(pdf: &mut Pdf, plan: &OutlinePlan, page_ids: &[usize], document: &Document) {
    let top_level_ids = plan
        .nodes
        .iter()
        .filter(|node| node.parent_id == plan.root_id)
        .map(|node| node.id)
        .collect::<Vec<_>>();
    {
        let mut outline = pdf.outline(pdf_ref(plan.root_id));
        outline.count(plan.visible_count);
        if let Some(first) = top_level_ids.iter().min().cloned() {
            outline.first(pdf_ref(first));
        }
        if let Some(last) = top_level_ids.iter().max().cloned() {
            outline.last(pdf_ref(last));
        }
    }

    for node in &plan.nodes {
        let page_index = node
            .bookmark
            .page_index
            .min(document.pages.len().saturating_sub(1));
        let page_id = page_ids.get(page_index).cloned().unwrap_or(0);
        let mut item = pdf.outline_item(pdf_ref(node.id));
        item.title(TextStr(&node.bookmark.label))
            .parent(pdf_ref(node.parent_id))
            .count(node.child_count);
        if let Some(id) = node.prev_id {
            item.prev(pdf_ref(id));
        }
        if let Some(id) = node.next_id {
            item.next(pdf_ref(id));
        }
        if let Some(id) = node.first_child_id {
            item.first(pdf_ref(id));
        }
        if let Some(id) = node.last_child_id {
            item.last(pdf_ref(id));
        }
        let target = crate::document::paint::geometry::paint_point_to_pdf(node.bookmark.target());
        item.dest()
            .page(pdf_ref(page_id))
            .xyz(target.x, target.y, Some(0.0));
    }
}

/// Build the two trailer `/ID` byte strings for a newly written PDF file.
///
/// ISO 32000-1:2008, 14.4 "File Identifiers" defines `/ID` as two byte
/// strings in the file trailer. The first identifies the original file and the
/// second identifies the current revision; PDF/A-1 requires the array to be
/// present. Quire writes complete files rather than incremental updates, so a
/// deterministic identifier derived from the generated document artifacts is
/// used for both entries.
fn pdf_file_identifier(
    document: &Document,
    page_renders: &[PageContentRender],
    embedded_fonts: &[EmbeddedFontPlan<'_>],
    unique_images: &[ImageResourceSource],
    page_image_pattern_plans: &[Vec<PageImagePatternPlan>],
    page_ext_gstate_plans: &[Vec<ExtGStateObjectPlan>],
) -> (Vec<u8>, Vec<u8>) {
    let mut hash = PdfFileIdentifierHash::new();
    hash.write_bytes(b"quire-pdf-file-id-v1");
    hash.write_document_metadata(&document.metadata);

    hash.write_usize(document.pages.len());
    for page in &document.pages {
        let page_size = page.paint_size();
        hash.write_f32(page_size.width);
        hash.write_f32(page_size.height);
        hash.write_i32(page.rotation);
        hash.write_usize(page.links.len());
        for link in &page.links {
            hash.write_f32(link.x());
            hash.write_f32(link.y());
            hash.write_f32(link.width());
            hash.write_f32(link.height());
            hash.write_str(&link.target);
        }
    }

    hash.write_usize(document.bookmarks.len());
    for bookmark in &document.bookmarks {
        hash.write_u32(bookmark.level);
        hash.write_str(&bookmark.label);
        hash.write_usize(bookmark.page_index);
        let target = bookmark.target();
        hash.write_f32(target.x);
        hash.write_f32(target.y);
        hash.write_u8(match bookmark.state {
            BookmarkState::Open => 1,
            BookmarkState::Closed => 2,
        });
    }

    hash.write_usize(page_renders.len());
    for render in page_renders {
        hash.write_bytes(&render.stream);
        hash.write_usize(render.form_xobjects.len());
        for form in &render.form_xobjects {
            hash.write_str(&form.name);
            hash.write_f32(form.bbox.x());
            hash.write_f32(form.bbox.y());
            hash.write_f32(form.bbox.width());
            hash.write_f32(form.bbox.height());
            hash.write_bytes(&form.stream);
            hash.write_bool(form.transparency_group);
            hash.write_usize(form.form_dependencies.len());
            for dependency in &form.form_dependencies {
                hash.write_usize(dependency.id);
                hash.write_str(&dependency.name);
            }
        }
        hash.write_usize(render.gradient_patterns.len());
        for plan in &render.gradient_patterns {
            hash.write_gradient(&plan.gradient);
            match &plan.alpha {
                Some(alpha) => {
                    hash.write_bool(true);
                    hash.write_f32(alpha.page_size.width);
                    hash.write_f32(alpha.page_size.height);
                }
                None => hash.write_bool(false),
            }
        }
    }

    hash.write_usize(embedded_fonts.len());
    for font in embedded_fonts {
        hash.write_str(&font.resource_name);
        hash.write_str(&font.base_name);
        hash.write_bytes(&font.font_file_data);
        hash.write_usize(font.source_gid_to_cid.len());
        for (source_gid, cid) in &font.source_gid_to_cid {
            hash.write_u16(*source_gid);
            hash.write_u16(*cid);
        }
        hash.write_usize(font.used_cids.len());
        for (cid, unicode) in &font.used_cids {
            hash.write_u16(*cid);
            hash.write_str(unicode);
        }
        hash.write_font_descriptor_metrics(&font.descriptor_metrics);
        hash.write_f32(font.default_width);
        hash.write_optional_bytes(font.cid_set_data.as_deref());
    }

    hash.write_usize(unique_images.len());
    for image in unique_images {
        match image {
            ImageResourceSource::Stored {
                image_id,
                source_rect,
                interpolate,
            } => {
                hash.write_u8(1);
                hash.write_bool(*interpolate);
                hash.write_source_rect(*source_rect);
                document
                    .image_store
                    .write_asset_identity(*image_id, |bytes| hash.write_bytes(bytes));
            }
            ImageResourceSource::Inline {
                pixel_width,
                pixel_height,
                interpolate,
                rgb,
                alpha,
                color_space,
            } => {
                hash.write_u8(2);
                hash.write_u32(*pixel_width);
                hash.write_u32(*pixel_height);
                hash.write_bool(*interpolate);
                match color_space {
                    crate::color::RasterColorSpace::BuiltIn(space) => {
                        hash.write_u8(1);
                        hash.write_u8(*space as u8);
                    }
                    crate::color::RasterColorSpace::EmbeddedRgb(profile) => {
                        hash.write_u8(2);
                        hash.write_bytes(profile);
                    }
                }
                hash.write_bytes(rgb);
                hash.write_optional_bytes(alpha.as_deref());
            }
        }
    }

    hash.write_usize(page_image_pattern_plans.len());
    for plans in page_image_pattern_plans {
        hash.write_usize(plans.len());
        for plan in plans {
            hash.write_str(&plan.name);
            hash.write_usize(plan.tile_image_id.0);
            let pattern = &plan.pattern;
            let rect = pattern.paint_rect();
            hash.write_bool(pattern.background);
            hash.write_f32(rect.origin.x);
            hash.write_f32(rect.origin.y);
            hash.write_f32(rect.size.width);
            hash.write_f32(rect.size.height);
            hash.write_f32(pattern.tiling.tile_size.width);
            hash.write_f32(pattern.tiling.tile_size.height);
            hash.write_f32(pattern.tiling.step.width);
            hash.write_f32(pattern.tiling.step.height);
            hash.write_f32(pattern.tiling.origin.x);
            hash.write_f32(pattern.tiling.origin.y);
            hash.write_bool(pattern.interpolate);
            hash.write_bool(pattern.clip().is_some());
        }
    }

    hash.write_usize(page_ext_gstate_plans.len());
    for plans in page_ext_gstate_plans {
        hash.write_usize(plans.len());
        for plan in plans {
            match &plan.resource {
                ExtGStateResource::Alpha { name, alpha } => {
                    hash.write_u8(1);
                    hash.write_str(name);
                    hash.write_f32(*alpha);
                }
                ExtGStateResource::Blend { name, mode } => {
                    hash.write_u8(2);
                    hash.write_str(name);
                    hash.write_str(&format!("{mode:?}"));
                }
            }
        }
    }

    let id = hash.finish().to_vec();
    (id.clone(), id)
}

#[derive(Debug, Clone, Copy)]
struct PdfFileIdentifierHash {
    first: u64,
    second: u64,
}

impl PdfFileIdentifierHash {
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self {
            first: 0xcbf2_9ce4_8422_2325,
            second: 0x6c62_272e_07bb_0142,
        }
    }

    fn finish(self) -> [u8; 16] {
        let first = Self::avalanche(self.first);
        let second = Self::avalanche(self.second);
        let mut id = [0; 16];
        id[..8].copy_from_slice(&first.to_be_bytes());
        id[8..].copy_from_slice(&second.to_be_bytes());
        id
    }

    fn write_document_metadata(&mut self, metadata: &DocumentMetadata) {
        self.write_optional_str(metadata.title.as_deref());
        self.write_optional_str(metadata.author.as_deref());
        self.write_optional_str(metadata.creator.as_deref());
    }

    fn write_source_rect(&mut self, rect: RenderedImageSourceRect) {
        self.write_u32(rect.x());
        self.write_u32(rect.y());
        self.write_u32(rect.width());
        self.write_u32(rect.height());
    }

    fn write_font_descriptor_metrics(&mut self, metrics: &FontDescriptorMetrics) {
        self.write_u32(metrics.flags);
        for value in metrics.bbox {
            self.write_i32(value);
        }
        self.write_f32(metrics.italic_angle);
        self.write_f32(metrics.ascent);
        self.write_f32(metrics.descent);
        self.write_f32(metrics.cap_height);
        self.write_optional_f32(metrics.x_height);
        self.write_f32(metrics.stem_v);
        self.write_optional_f32(metrics.avg_width);
        self.write_optional_f32(metrics.max_width);
        self.write_optional_f32(metrics.missing_width);
    }

    fn write_gradient(&mut self, gradient: &crate::document::paint::paths::RenderedGradient) {
        match &gradient.kind {
            crate::document::paint::paths::RenderedGradientKind::Linear { start, end } => {
                self.write_u8(1);
                self.write_f32(start.x);
                self.write_f32(start.y);
                self.write_f32(end.x);
                self.write_f32(end.y);
            }
            crate::document::paint::paths::RenderedGradientKind::Radial {
                start_center,
                start_radius,
                end_center,
                end_radius,
            } => {
                self.write_u8(2);
                self.write_f32(start_center.x);
                self.write_f32(start_center.y);
                self.write_f32(*start_radius);
                self.write_f32(end_center.x);
                self.write_f32(end_center.y);
                self.write_f32(*end_radius);
            }
        }
        let transform = gradient.transform;
        for component in transform.pdf_components() {
            self.write_f32(component);
        }
        self.write_usize(gradient.stops.len());
        for stop in &gradient.stops {
            self.write_f32(stop.offset);
            self.write_f32(stop.interpolation_exponent);
            self.write_u8(stop.color.space().cache_key());
            self.write_f32(stop.color.components()[0]);
            self.write_f32(stop.color.components()[1]);
            self.write_f32(stop.color.components()[2]);
            self.write_f32(stop.color.alpha());
        }
    }

    fn write_optional_str(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.write_bool(true);
                self.write_str(value);
            }
            None => self.write_bool(false),
        }
    }

    fn write_optional_bytes(&mut self, value: Option<&[u8]>) {
        match value {
            Some(value) => {
                self.write_bool(true);
                self.write_bytes(value);
            }
            None => self.write_bool(false),
        }
    }

    fn write_optional_f32(&mut self, value: Option<f32>) {
        match value {
            Some(value) => {
                self.write_bool(true);
                self.write_f32(value);
            }
            None => self.write_bool(false),
        }
    }

    fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_usize(bytes.len());
        for byte in bytes {
            self.write_u8(*byte);
        }
    }

    fn write_bool(&mut self, value: bool) {
        self.write_u8(u8::from(value));
    }

    fn write_u8(&mut self, value: u8) {
        self.first = Self::hash_byte(self.first, value);
        self.second = Self::hash_byte(self.second, value.rotate_left(1));
    }

    fn write_u16(&mut self, value: u16) {
        self.write_bytes_unframed(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.write_bytes_unframed(&value.to_le_bytes());
    }

    fn write_i32(&mut self, value: i32) {
        self.write_bytes_unframed(&value.to_le_bytes());
    }

    fn write_usize(&mut self, value: usize) {
        self.write_bytes_unframed(&(value as u64).to_le_bytes());
    }

    fn write_f32(&mut self, value: f32) {
        let value = if value == 0.0 { 0.0 } else { value };
        self.write_u32(value.to_bits());
    }

    fn write_bytes_unframed(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u8(*byte);
        }
    }

    fn hash_byte(state: u64, byte: u8) -> u64 {
        (state ^ u64::from(byte)).wrapping_mul(Self::FNV_PRIME)
    }

    fn avalanche(mut value: u64) -> u64 {
        value ^= value >> 33;
        value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
        value ^= value >> 33;
        value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        value ^ (value >> 33)
    }
}

#[derive(Debug, Clone, Copy)]
struct PdfObjectAllocator {
    next_id: usize,
}

impl PdfObjectAllocator {
    fn new() -> Self {
        Self { next_id: 1 }
    }

    fn alloc_id(&mut self) -> usize {
        self.reserve_ids(1)
    }

    fn alloc_ids(&mut self, count: usize) -> Vec<usize> {
        let first = self.reserve_ids(count);
        (first..first + count).collect()
    }

    fn reserve_ids(&mut self, count: usize) -> usize {
        let first = self.next_id;
        self.next_id += count;
        first
    }

    fn peek_id(&self) -> usize {
        self.next_id
    }

    fn advance_to(&mut self, next_id: usize) {
        assert!(
            next_id >= self.next_id,
            "PDF object allocator cannot move backwards"
        );
        self.next_id = next_id;
    }
}

fn pdf_ref(id: usize) -> Ref {
    Ref::new(i32_from_usize(id))
}

fn pdf_name(name: &str) -> Name<'_> {
    Name(name.as_bytes())
}

fn i32_from_usize(value: usize) -> i32 {
    i32::try_from(value).expect("PDF object value exceeds i32 range")
}

fn i32_from_u32(value: u32) -> i32 {
    i32::try_from(value).expect("PDF object value exceeds i32 range")
}
