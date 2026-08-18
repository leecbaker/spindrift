use super::colors::{
    PdfBlendColorSpace, PdfColorPlan, PdfColorRequirements, PdfLoweringColorPolicy, PdfPaintColor,
};
use super::*;
use crate::timing::DebugTimer;
use pdf_writer::types::{FunctionShadingType, MaskType, PaintType, TilingType};
use pdf_writer::{Content, Filter, Pdf, Rect, Settings};
use std::io::Write;
use std::time::Duration;

pub(crate) fn write_document<W: Write>(
    document: &Document,
    options: &crate::PdfOptions,
    writer: &mut W,
) -> crate::Result<()> {
    let profile = options.profile;
    let font_embedding = options.font_embedding;
    let compression = options.compression;
    let total_timer = DebugTimer::start("serializing PDF document");
    let mut timings = PdfTimingSummary::new();
    let page_count = document.pages.len();
    let mut planner = PdfResourcePlanner::new();

    let catalog_id = planner.alloc_id();
    let pages_id = planner.alloc_id();
    let page_ids = planner.alloc_ids(page_count);
    let image_plan = timings.measure("deduplicating and preparing PDF image resources", || {
        deduplicate_images(document)
    });
    // Colour requirements are semantic program inputs, not writer-time
    // fallbacks.  In particular, a colourless isolated Form still needs its
    // explicitly selected sRGB blending space.
    let mut color_requirements = PdfColorRequirements::from_paint_and_image_sources(
        document.pages.iter().flat_map(Page::vector_paint_colors),
        image_plan.built_in_color_spaces(&document.image_store),
        image_plan.embedded_rgb_profiles(&document.image_store),
    );
    if document.pages.iter().any(Page::has_transparency_group) {
        color_requirements.require_blending_space(PdfBlendColorSpace::Srgb);
    }
    let lowering_color_policy = PdfLoweringColorPolicy::new(profile, &color_requirements);
    let solid_fill_eligibility = image_plan.solid_fill_eligibility(document);
    let prepared_images = timings.measure("materializing PDF image paint representations", || {
        image_plan
            .unique_images
            .iter()
            .zip(solid_fill_eligibility)
            .map(|(source, eligible)| {
                prepare_image_resource(
                    &document.image_store,
                    source,
                    lowering_color_policy.mode(),
                    eligible,
                )
            })
            .collect::<Vec<_>>()
    });

    let font_validation_profile = if profile.is_pdfa() {
        PdfFontValidationProfile::PdfA
    } else {
        PdfFontValidationProfile::Default
    };
    let (mut embedded_font_plans, font_timings) = timings.measure(
        format!(
            "planning PDF font embedding for {} document font(s)",
            document.fonts.len()
        ),
        || {
            let (plans, font_timings) = timed_embedded_font_plans_with_profile(
                document,
                0,
                font_validation_profile,
                font_embedding,
            )?;
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
    let mut dynamic_resources = PdfResourceRegistry::default();
    let page_renders = timings.measure(
        format!("building {page_count} page content stream(s)"),
        || {
            document
                .pages
                .iter()
                .enumerate()
                .map(|(page_index, page)| {
                    page_content_render(
                        page,
                        &embedded_font_plans,
                        super::content::PageContentRenderInputs {
                            resources: &mut dynamic_resources,
                            color_policy: &lowering_color_policy,
                            image_resources: &prepared_images,
                            page_image_sources: &image_plan.page_image_unique_indexes[page_index],
                            page_svg_pattern_image_sources: &image_plan
                                .page_svg_pattern_image_unique_indexes[page_index],
                            page_image_pattern_sources: &image_plan
                                .page_pattern_tile_unique_indexes[page_index],
                            raster_resolution_dppx: document.image_store.output_resolution_dppx(),
                        },
                    )
                })
                .collect::<Vec<_>>()
        },
    );
    let lowered_program = PdfLoweredDocumentProgram {
        pages: page_renders,
        dynamic_resources,
    };
    lowered_program.debug_assert_well_formed();
    let color_requirements = final_color_requirements_from_lowering(
        &lowering_color_policy,
        &lowered_program.pages,
        &image_plan,
        &document.image_store,
    );
    let color_plan = PdfColorPlan::new(profile, planner.peek_id(), color_requirements)?;
    planner.reserve_ids(color_plan.object_count());
    let first_embedded_font_id = planner.peek_id();
    embedded_font_plans.resolve_object_ids(first_embedded_font_id, font_validation_profile);
    planner.advance_to(
        first_embedded_font_id
            + embedded_font_plans.fonts.len()
                * font_validation_profile.embedded_font_object_count(),
    );
    let unique_image_ids = timings.measure("assigning PDF image object IDs", || {
        prepared_images
            .iter()
            .map(|image| match image {
                PreparedImageResource::Transparent | PreparedImageResource::SolidFill(_) => None,
                PreparedImageResource::Raster(_) => Some(ImageObjectIds {
                    image_id: PdfImageObjectId(planner.alloc_id()),
                    alpha_mask_id: Some(PdfImageObjectId(planner.alloc_id())),
                }),
            })
            .collect::<Vec<_>>()
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
                                Some(PageImagePatternPlan {
                                    handle: PdfImagePatternHandle {
                                        page_index,
                                        pattern_index,
                                    },
                                    id: planner.alloc_id(),
                                    name: format!("P{}", pattern_index + 1),
                                    tile_image_id,
                                    pattern: pattern.clone(),
                                    stream: PdfStreamProgram {
                                        bytes: content.finish().into_vec(),
                                        resource_uses: PdfStreamResourceUses {
                                            xobjects: [(
                                                "Im1".into(),
                                                PdfXObjectHandle::Image(PdfImageHandle(
                                                    tile_index.0,
                                                )),
                                            )]
                                            .into(),
                                            ..PdfStreamResourceUses::default()
                                        },
                                        resolved_resources: Some(PdfResolvedStreamResources {
                                            xobjects: [(
                                                "Im1".into(),
                                                PdfResolvedReference(tile_image_id.0),
                                            )]
                                            .into(),
                                            ..PdfResolvedStreamResources::default()
                                        }),
                                    },
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
    // Lowering has now completed.  Resolve symbolic handles in their stable
    // encounter order; no content writer can allocate an indirect object.
    planner.plan_dynamic_resources(&lowered_program.dynamic_resources);
    let mut page_renders = lowered_program.pages;
    let content_ids = page_renders
        .iter()
        .map(|render| (!render.stream.bytes.is_empty()).then(|| planner.alloc_id()))
        .collect::<Vec<_>>();
    let page_ext_gstate_plans = timings.measure("planning PDF page ExtGState resources", || {
        document
            .pages
            .iter()
            .map(|page| {
                page_ext_gstate_resources(page)
                    .into_iter()
                    .map(|resource| ExtGStateObjectPlan {
                        id: planner.alloc_id(),
                        resource,
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    });
    timings.measure("recording exact PDF stream resource uses", || {
        for (page_index, page_render) in page_renders.iter_mut().enumerate() {
            let bindings = PdfPageResourceBindings {
                xobjects: page_render
                    .form_xobjects
                    .iter()
                    .map(|form| (form.name.clone(), PdfXObjectHandle::Form(form.id)))
                    .chain(
                        image_plan.page_image_unique_indexes[page_index]
                            .iter()
                            .enumerate()
                            .filter(|(_, image)| unique_image_ids[image.0].is_some())
                            .map(|(index, image)| {
                                (
                                    format!("Im{}", index + 1),
                                    PdfXObjectHandle::Image(PdfImageHandle(image.0)),
                                )
                            }),
                    )
                    .chain(
                        image_plan.page_svg_pattern_image_unique_indexes[page_index]
                            .iter()
                            .filter(|image| unique_image_ids[image.0].is_some())
                            .map(|image| {
                                (
                                    format!("Im{}", image.0 + 1),
                                    PdfXObjectHandle::Image(PdfImageHandle(image.0)),
                                )
                            }),
                    )
                    .collect(),
                fonts: embedded_font_plans
                    .fonts
                    .iter()
                    .enumerate()
                    .map(|(index, font)| (font.resource_name.clone(), PdfFontHandle(index)))
                    .collect(),
                patterns: page_image_pattern_plans[page_index]
                    .iter()
                    .map(|plan| {
                        (
                            plan.name.clone(),
                            PdfPatternResourceHandle::Image(plan.handle),
                        )
                    })
                    .chain(page_render.gradient_patterns.iter().flat_map(|plan| {
                        std::iter::once((
                            plan.name.clone(),
                            PdfPatternResourceHandle::Dynamic(plan.id),
                        ))
                        .chain(plan.alpha.iter().map(|alpha| {
                            (
                                alpha.pattern_name.clone(),
                                PdfPatternResourceHandle::Dynamic(alpha.pattern_id),
                            )
                        }))
                    }))
                    .chain(page_render.gradient_tiling_patterns.iter().map(|plan| {
                        (
                            plan.name.clone(),
                            PdfPatternResourceHandle::Dynamic(plan.id),
                        )
                    }))
                    .chain(page_render.svg_tiling_patterns.iter().map(|plan| {
                        (
                            plan.name.clone(),
                            PdfPatternResourceHandle::Dynamic(plan.id),
                        )
                    }))
                    .chain(page_render.svg_path_tiling_patterns.iter().map(|plan| {
                        (
                            plan.name.clone(),
                            PdfPatternResourceHandle::Dynamic(plan.id),
                        )
                    }))
                    .collect(),
                ext_gstates: page_ext_gstate_plans[page_index]
                    .iter()
                    .enumerate()
                    .map(|(resource_index, plan)| {
                        (
                            plan.resource.name().to_owned(),
                            PdfExtGStateResourceHandle::Page(PdfPageExtGStateHandle {
                                page_index,
                                resource_index,
                            }),
                        )
                    })
                    .chain(
                        page_render
                            .gradient_patterns
                            .iter()
                            .filter_map(|plan| plan.alpha.as_ref())
                            .map(|alpha| {
                                (
                                    alpha.ext_gstate_name.clone(),
                                    PdfExtGStateResourceHandle::Dynamic(alpha.ext_gstate_id),
                                )
                            }),
                    )
                    .collect(),
                color_spaces: color_plan.resource_handles().into_iter().collect(),
            };
            bindings.record_uses(&mut page_render.stream);
            for form in &mut page_render.form_xobjects {
                bindings.record_uses(&mut form.stream);
            }
            for plan in &mut page_render.gradient_tiling_patterns {
                bindings.record_uses(&mut plan.stream);
            }
            for plan in &mut page_render.svg_tiling_patterns {
                bindings.record_uses(&mut plan.stream);
            }
            for plan in &mut page_render.svg_path_tiling_patterns {
                bindings.record_uses(&mut plan.stream);
            }
            for plan in &mut page_render.gradient_patterns {
                if let Some(alpha) = &mut plan.alpha {
                    bindings.record_uses(&mut alpha.stream);
                }
            }
            planner.resolve_stream_bindings(
                &mut page_render.stream,
                &embedded_font_plans.fonts,
                &unique_image_ids,
                &page_image_pattern_plans,
                &page_ext_gstate_plans,
                &color_plan,
            );
            for form in &mut page_render.form_xobjects {
                planner.resolve_stream_bindings(
                    &mut form.stream,
                    &embedded_font_plans.fonts,
                    &unique_image_ids,
                    &page_image_pattern_plans,
                    &page_ext_gstate_plans,
                    &color_plan,
                );
            }
            for plan in &mut page_render.gradient_tiling_patterns {
                planner.resolve_stream_bindings(
                    &mut plan.stream,
                    &embedded_font_plans.fonts,
                    &unique_image_ids,
                    &page_image_pattern_plans,
                    &page_ext_gstate_plans,
                    &color_plan,
                );
            }
            for plan in &mut page_render.svg_tiling_patterns {
                planner.resolve_stream_bindings(
                    &mut plan.stream,
                    &embedded_font_plans.fonts,
                    &unique_image_ids,
                    &page_image_pattern_plans,
                    &page_ext_gstate_plans,
                    &color_plan,
                );
            }
            for plan in &mut page_render.svg_path_tiling_patterns {
                planner.resolve_stream_bindings(
                    &mut plan.stream,
                    &embedded_font_plans.fonts,
                    &unique_image_ids,
                    &page_image_pattern_plans,
                    &page_ext_gstate_plans,
                    &color_plan,
                );
            }
            for plan in &mut page_render.gradient_patterns {
                if let Some(alpha) = &mut plan.alpha {
                    planner.resolve_stream_bindings(
                        &mut alpha.stream,
                        &embedded_font_plans.fonts,
                        &unique_image_ids,
                        &page_image_pattern_plans,
                        &page_ext_gstate_plans,
                        &color_plan,
                    );
                }
            }
        }
    });

    let info_id = planner.alloc_id();
    let metadata_id =
        (profile.is_pdfa() || document.metadata.has_source_metadata()).then(|| planner.alloc_id());
    let page_annotation_ids = timings.measure("planning PDF annotation IDs", || {
        document
            .pages
            .iter()
            .map(|page| page.links.iter().map(|_| planner.alloc_id()).collect())
            .collect::<Vec<Vec<_>>>()
    });
    let outline_plan = timings.measure(
        format!(
            "planning {} bookmark outline item(s)",
            document.bookmarks.len()
        ),
        || {
            let plan = outline_plan(document, planner.peek_id());
            if let Some(plan) = &plan {
                planner.reserve_ids(1 + plan.nodes.len());
            }
            plan
        },
    );
    let page_annotations = document
        .pages
        .iter()
        .zip(&page_annotation_ids)
        .map(|(page, ids)| {
            page.links
                .iter()
                .zip(ids)
                .map(|(link, id)| PdfAnnotationProgram {
                    id: *id,
                    rect: link.paint_rect(),
                    target: link.target.to_string(),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let file_id = pdf_file_identifier(
        document,
        &page_renders,
        &embedded_font_plans.fonts,
        &image_plan.unique_images,
        &page_image_pattern_plans,
        &page_ext_gstate_plans,
    );
    let pages = document
        .pages
        .iter()
        .zip(page_ids)
        .zip(content_ids)
        .zip(page_annotations)
        .zip(page_renders)
        .map(
            |((((page, id), content_id), annotations), render)| PdfPageProgram {
                id,
                content_id,
                size: page.paint_size(),
                rotation: page.rotation,
                annotations,
                render,
            },
        )
        .collect();

    // The writer receives one complete resolved program.  All remaining
    // serializers consume these entries rather than allocating or discovering
    // static resources from the source document.
    let program = PdfDocumentProgram {
        catalog_id,
        pages_id,
        pages,
        dynamic_resources: planner,
        color_plan,
        fonts: embedded_font_plans.fonts,
        images: PdfImageProgram {
            prepared: prepared_images,
            unique_object_ids: unique_image_ids,
            page_patterns: page_image_pattern_plans,
        },
        page_ext_gstates: page_ext_gstate_plans,
        metadata: PdfMetadataProgram {
            info_id,
            xmp_id: metadata_id,
            source: document.metadata.clone(),
            producer: options.producer.clone(),
        },
        outline: outline_plan,
        file_id,
    };
    program.debug_assert_resolved_references_are_unique();

    let bytes = serialize_program(
        &program,
        PdfSerializationOptions {
            profile,
            compression,
        },
        &mut timings,
    );
    let total = total_timer.finish();
    timings.log_summary(total);
    writer.write_all(&bytes)?;
    Ok(())
}

/// Pure serialization boundary for the resolved private PDF program.
///
/// The source [`Document`] has already been consumed by lowering and planning;
/// this function can only read resolved object references and serialization
/// options.
#[derive(Debug, Clone, Copy)]
struct PdfSerializationOptions {
    profile: crate::PdfProfile,
    compression: crate::PdfCompression,
}

fn serialize_program(
    program: &PdfDocumentProgram<'_>,
    options: PdfSerializationOptions,
    timings: &mut PdfTimingSummary,
) -> Vec<u8> {
    let profile = options.profile;
    let compression = options.compression;
    let mut pdf = Pdf::with_settings(Settings::default());
    let (major_version, minor_version) = profile.pdf_version();
    pdf.set_version(major_version, minor_version);
    pdf.set_binary_marker(b"\xE2\xE3\xCF\xD3");

    timings.measure("writing PDF page tree and content objects", || {
        write_catalog(
            &mut pdf,
            program.catalog_id,
            program.pages_id,
            program.metadata.xmp_id,
            &program.metadata.source,
            program.outline.as_ref(),
            &program.color_plan,
        );
        write_pages(&mut pdf, program.pages_id, &program.pages);
        write_pages_and_content(&mut pdf, program.pages_id, &program.pages, compression);
    });
    timings.measure("writing PDF embedded font objects", || {
        write_embedded_fonts(&mut pdf, &program.fonts, compression);
    });
    timings.measure("writing PDF image objects", || {
        write_images(
            &mut pdf,
            &program.images.prepared,
            &program.images.unique_object_ids,
            &program.color_plan,
            compression,
        );
    });
    timings.measure("writing PDF ICC color profiles", || {
        program.color_plan.write_profiles(&mut pdf, compression);
    });
    timings.measure("writing PDF image pattern objects", || {
        write_image_patterns(&mut pdf, &program.images.page_patterns, compression);
    });
    timings.measure("writing SVG gradient shading patterns", || {
        write_gradient_patterns(
            &mut pdf,
            program.pages.iter().map(|page| &page.render),
            &program.color_plan,
            &program.dynamic_resources,
            compression,
        );
    });
    timings.measure("writing SVG gradient alpha-mask forms", || {
        write_gradient_alpha_forms(
            &mut pdf,
            program.pages.iter().map(|page| &page.render),
            &program.dynamic_resources,
            compression,
        );
    });
    timings.measure("writing PDF form XObjects", || {
        write_form_xobjects(
            &mut pdf,
            program.pages.iter().map(|page| &page.render),
            &program.color_plan,
            &program.dynamic_resources,
            compression,
        );
    });
    timings.measure("writing PDF ExtGState objects", || {
        write_ext_gstate_objects(&mut pdf, &program.page_ext_gstates);
        write_gradient_alpha_ext_gstates(
            &mut pdf,
            program.pages.iter().map(|page| &page.render),
            &program.dynamic_resources,
        );
    });
    timings.measure("writing PDF metadata, annotations, and outlines", || {
        write_document_info(
            &mut pdf,
            pdf_ref(program.metadata.info_id),
            &program.metadata.source,
            &program.metadata.producer,
        );
        if let Some(metadata_id) = program.metadata.xmp_id {
            write_document_xmp_metadata(
                &mut pdf,
                pdf_ref(metadata_id),
                &program.metadata.source,
                profile,
                compression,
                &program.metadata.producer,
            );
        }
        write_annotations(&mut pdf, &program.pages);
        if let Some(outline_plan) = &program.outline {
            write_outlines(&mut pdf, outline_plan, &program.pages);
        }
    });
    timings.measure("building deterministic PDF file identifier", || {
        pdf.set_file_id(program.file_id.clone());
    });

    let object_count = program.dynamic_resources.peek_id().saturating_sub(1);
    timings.measure(format!("assembling {object_count} PDF object(s)"), || {
        pdf.finish()
    })
}

/// Derive calibrated PDF colour requirements from final content programs.
///
/// This is deliberately after semantic lowering: filter transforms, gradient
/// normalization, and SVG Form construction have already selected the actual
/// PDF `cs`/`CS` names. CSS source colours are used only to build the
/// provisional lowering policy and never to retain the final ICC table.
fn final_color_requirements_from_lowering(
    lowering_policy: &PdfLoweringColorPolicy,
    page_renders: &[PageContentRender],
    image_plan: &ImageResourcePlan,
    image_store: &crate::image_store::DocumentImageStore,
) -> PdfColorRequirements {
    let used_raster_images = final_raster_image_indexes(page_renders, image_plan);
    let (built_in_image_spaces, embedded_image_profiles) =
        image_plan.color_sources_for_indexes(&used_raster_images, image_store);
    let mut requirements = PdfColorRequirements::from_paint_and_image_sources(
        std::iter::empty(),
        built_in_image_spaces,
        embedded_image_profiles,
    );
    let color_names = lowering_policy.resource_names();
    for page_render in page_renders {
        for stream in std::iter::once(&page_render.stream)
            .chain(page_render.form_xobjects.iter().map(|form| &form.stream))
            .chain(
                page_render
                    .gradient_tiling_patterns
                    .iter()
                    .map(|pattern| &pattern.stream),
            )
            .chain(
                page_render
                    .svg_tiling_patterns
                    .iter()
                    .map(|pattern| &pattern.stream),
            )
            .chain(
                page_render
                    .svg_path_tiling_patterns
                    .iter()
                    .map(|pattern| &pattern.stream),
            )
            .chain(
                page_render
                    .gradient_patterns
                    .iter()
                    .filter_map(|pattern| pattern.alpha.as_ref().map(|alpha| &alpha.stream)),
            )
        {
            for name in &color_names {
                if stream_uses_named_operator(&stream.bytes, name, b"cs")
                    || stream_uses_named_operator(&stream.bytes, name, b"CS")
                {
                    if let Some(space) = lowering_policy.output_space_for_resource_name(name) {
                        requirements.require_final_output_space(space);
                    } else if let Some(profile) =
                        lowering_policy.embedded_rgb_profile_for_resource_name(name)
                    {
                        requirements.require_embedded_rgb_profile(profile);
                    }
                }
            }
        }
        for gradient in &page_render.gradient_patterns {
            requirements.require_final_output_space(
                lowering_policy.gradient_output_space(&gradient.gradient),
            );
        }
        for form in &page_render.form_xobjects {
            if matches!(
                form.kind,
                PdfFormKind::TransparencyGroup {
                    blending_space: PdfBlendColorSpace::Srgb
                }
            ) {
                requirements.require_blending_space(PdfBlendColorSpace::Srgb);
            }
        }
    }
    requirements
}

/// Discover raster XObjects selected by the final page and Form streams.
///
/// Image source collection happens before lowering so that content painting
/// has stable `/ImN` and `/PN` names.  This pass deliberately goes the other
/// direction: it retains a raster source's ICC profile only if a final `Do`
/// or image-pattern selection can reach the serialized program.  Direct
/// solid-image fills are covered instead by their emitted colour-space
/// operators in `final_color_requirements_from_lowering`.
fn final_raster_image_indexes(
    page_renders: &[PageContentRender],
    image_plan: &ImageResourcePlan,
) -> Vec<PlannedImageIndex> {
    let mut indexes = Vec::new();
    for (page_index, page_render) in page_renders.iter().enumerate() {
        let streams = std::iter::once(&page_render.stream)
            .chain(page_render.form_xobjects.iter().map(|form| &form.stream));
        for stream in streams {
            for (image_index, planned) in image_plan.page_image_unique_indexes[page_index]
                .iter()
                .enumerate()
            {
                let name = format!("Im{}", image_index + 1);
                if stream_uses_named_operator(&stream.bytes, &name, b"Do")
                    && !indexes.contains(planned)
                {
                    indexes.push(*planned);
                }
            }
            for (pattern_index, planned) in image_plan.page_pattern_tile_unique_indexes[page_index]
                .iter()
                .enumerate()
            {
                let name = format!("P{}", pattern_index + 1);
                if (stream_uses_named_operator(&stream.bytes, &name, b"scn")
                    || stream_uses_named_operator(&stream.bytes, &name, b"SCN"))
                    && !indexes.contains(planned)
                {
                    indexes.push(*planned);
                }
            }
        }
    }
    indexes
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
    let raster_resolution_dppx = document.image_store.output_resolution_dppx();
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
                    let source = image_source(image, raster_resolution_dppx);
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
                    let source = image_pattern_source(pattern, raster_resolution_dppx);
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
    let page_svg_pattern_image_unique_indexes = document
        .pages
        .iter()
        .map(|page| {
            page.svg_pattern_images
                .iter()
                .map(|image| {
                    let source = image_source(image, raster_resolution_dppx);
                    if let Some(index) = image_lookup.get(&source) {
                        *index
                    } else {
                        let index = PlannedImageIndex(unique_images.len());
                        image_lookup.insert(source.clone(), index);
                        unique_images.push(source);
                        index
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    ImageResourcePlan {
        unique_images,
        page_image_unique_indexes,
        page_svg_pattern_image_unique_indexes,
        page_pattern_tile_unique_indexes,
    }
}

#[cfg(any())]
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
                .bits_per_component(image.sample_depth.bits_per_component())
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
                .bits_per_component(image.sample_depth.bits_per_component())
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
#[cfg(any())]
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

        let stream = encode_pdf_stream(compression, &plan.stream.bytes);
        let mut pattern_writer = pdf.tiling_pattern(pdf_ref(plan.id), stream.bytes());
        if stream.uses_flate() {
            pattern_writer.filter(Filter::FlateDecode);
        }
        let matrix = crate::document::paint::geometry::PaintTransform::translate(
            crate::document::paint::geometry::PaintTranslation::new(
                pattern.tiling.origin.x,
                pattern.tiling.origin.y,
            ),
        );
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
            .matrix(matrix.pdf_components());
        let bindings = plan
            .stream
            .resolved_resources
            .as_ref()
            .expect("image-pattern stream is resolved during planning");
        write_resource_dictionary(&mut pattern_writer.resources(), bindings);
    }
}

/// Emit native PDF shading patterns for SVG and CSS linear/radial gradients.
///
/// SVG 2, 13.2 maps to PDF axial/radial shadings. Each stop interval is a
/// Type 2 exponential interpolation; three or more intervals use a Type 3
/// stitching function as defined by ISO 32000-2:2020, 8.9.1-8.9.3.
fn write_gradient_patterns<'a>(
    pdf: &mut Pdf,
    page_renders: impl IntoIterator<Item = &'a PageContentRender>,
    color_plan: &PdfColorPlan,
    dynamic_resources: &PdfResourcePlanner,
    compression: crate::PdfCompression,
) {
    let page_renders = page_renders.into_iter().collect::<Vec<_>>();
    for plan in page_renders
        .iter()
        .flat_map(|render| &render.gradient_patterns)
    {
        let output_space = color_plan.gradient_output_space(&plan.gradient);
        if let Some(periodic) = &plan.gradient.periodic {
            write_periodic_gradient_color_function(
                pdf,
                dynamic_resources.function(plan.function_ids[0]),
                periodic,
                color_plan,
                output_space,
            );
            write_gradient_shading_pattern(
                pdf,
                plan,
                color_plan,
                dynamic_resources.function(plan.function_ids[0]),
                output_space,
                dynamic_resources,
            );
            if let Some(alpha) = &plan.alpha {
                write_periodic_gradient_alpha_pattern(
                    pdf,
                    plan,
                    alpha,
                    periodic,
                    dynamic_resources,
                );
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
                    dynamic_resources.function(*id),
                    from,
                    to,
                    interval[0].interpolation_exponent,
                );
            } else {
                pdf.exponential_function(pdf_ref(dynamic_resources.function(*id)))
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
            pdf.stitching_function(pdf_ref(dynamic_resources.function(stitching_id)))
                .domain([0.0, 1.0])
                .functions(
                    interval_ids
                        .iter()
                        .copied()
                        .map(|id| pdf_ref(dynamic_resources.function(id))),
                )
                .bounds(bounds)
                .encode((0..interval_count).flat_map(|_| [0.0, 1.0]));
            stitching_id
        } else {
            interval_ids[0]
        };
        write_gradient_shading_pattern(
            pdf,
            plan,
            color_plan,
            dynamic_resources.function(function_id),
            output_space,
            dynamic_resources,
        );
        if let Some(alpha) = &plan.alpha {
            write_gradient_alpha_pattern(pdf, plan, alpha, dynamic_resources);
        }
    }
    for plan in page_renders
        .iter()
        .flat_map(|render| &render.gradient_tiling_patterns)
    {
        let pattern = &plan.pattern;
        let stream = encode_pdf_stream(compression, &plan.stream.bytes);
        let mut writer =
            pdf.tiling_pattern(pdf_ref(dynamic_resources.pattern(plan.id)), stream.bytes());
        if stream.uses_flate() {
            writer.filter(Filter::FlateDecode);
        }
        let matrix = crate::document::paint::geometry::PaintTransform::translate(
            crate::document::paint::geometry::PaintTranslation::new(
                pattern.tiling.origin.x,
                pattern.tiling.origin.y,
            ),
        );
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
            .matrix(matrix.pdf_components());
        let bindings = plan
            .stream
            .resolved_resources
            .as_ref()
            .expect("gradient tiling stream is resolved during planning");
        write_resource_dictionary(&mut writer.resources(), bindings);
    }
    for plan in page_renders
        .iter()
        .flat_map(|render| &render.svg_tiling_patterns)
    {
        let pattern = &plan.pattern;
        let stream = encode_pdf_stream(compression, &plan.stream.bytes);
        let mut writer =
            pdf.tiling_pattern(pdf_ref(dynamic_resources.pattern(plan.id)), stream.bytes());
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
        let bindings = plan
            .stream
            .resolved_resources
            .as_ref()
            .expect("SVG tiling stream is resolved during planning");
        write_resource_dictionary(&mut writer.resources(), bindings);
    }
    for plan in page_renders
        .iter()
        .flat_map(|render| &render.svg_path_tiling_patterns)
    {
        let pattern = &plan.pattern;
        let stream = encode_pdf_stream(compression, &plan.stream.bytes);
        let mut writer =
            pdf.tiling_pattern(pdf_ref(dynamic_resources.pattern(plan.id)), stream.bytes());
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
        let bindings = plan
            .stream
            .resolved_resources
            .as_ref()
            .expect("SVG path tiling stream is resolved during planning");
        write_resource_dictionary(&mut writer.resources(), bindings);
    }
}

fn write_gradient_shading_pattern(
    pdf: &mut Pdf,
    plan: &GradientPatternPlan,
    color_plan: &PdfColorPlan,
    function_id: usize,
    output_space: crate::css::CssColorSpace,
    dynamic_resources: &PdfResourcePlanner,
) {
    let mut pattern = pdf.shading_pattern(pdf_ref(dynamic_resources.pattern(plan.id)));
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
    dynamic_resources: &PdfResourcePlanner,
) {
    let code = periodic_gradient_function_code(periodic, &periodic.stops, true);
    pdf.post_script_function(
        pdf_ref(dynamic_resources.function(alpha.function_ids[0])),
        code.as_bytes(),
    )
    .domain([0.0, 1.0])
    .range([0.0, 1.0]);
    let mut pattern = pdf.shading_pattern(pdf_ref(dynamic_resources.pattern(alpha.pattern_id)));
    pattern.matrix(plan.gradient.transform.pdf_components());
    let mut shading = pattern.function_shading();
    shading.color_space().device_gray();
    shading
        .function(pdf_ref(dynamic_resources.function(alpha.function_ids[0])))
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
    dynamic_resources: &PdfResourcePlanner,
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
        pdf.exponential_function(pdf_ref(dynamic_resources.function(*id)))
            .domain([0.0, 1.0])
            .c0([interval[0].color.alpha()])
            .c1([interval[1].color.alpha()])
            .n(interval[0].interpolation_exponent);
    }
    let function_id = if let Some(stitching_id) = stitching_id {
        let bounds = plan.gradient.stops[1..plan.gradient.stops.len() - 1]
            .iter()
            .map(|stop| stop.offset);
        pdf.stitching_function(pdf_ref(dynamic_resources.function(stitching_id)))
            .domain([0.0, 1.0])
            .functions(
                interval_ids
                    .iter()
                    .copied()
                    .map(|id| pdf_ref(dynamic_resources.function(id))),
            )
            .bounds(bounds)
            .encode((0..interval_count).flat_map(|_| [0.0, 1.0]));
        stitching_id
    } else {
        interval_ids[0]
    };
    let transform = plan.gradient.transform;
    let mut pattern = pdf.shading_pattern(pdf_ref(dynamic_resources.pattern(alpha.pattern_id)));
    pattern.matrix(transform.pdf_components());
    let mut shading = pattern.function_shading();
    shading.color_space().device_gray();
    shading
        .function(pdf_ref(dynamic_resources.function(function_id)))
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

fn write_gradient_alpha_forms<'a>(
    pdf: &mut Pdf,
    page_renders: impl IntoIterator<Item = &'a PageContentRender>,
    dynamic_resources: &PdfResourcePlanner,
    compression: crate::PdfCompression,
) {
    let page_renders = page_renders.into_iter().collect::<Vec<_>>();
    for plan in page_renders
        .iter()
        .flat_map(|render| &render.gradient_patterns)
    {
        let Some(alpha) = &plan.alpha else {
            continue;
        };
        let stream = encode_pdf_stream(compression, &alpha.stream.bytes);
        let mut form = pdf.form_xobject(
            pdf_ref(dynamic_resources.form(alpha.form_id)),
            stream.bytes(),
        );
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
        let bindings = alpha
            .stream
            .resolved_resources
            .as_ref()
            .expect("gradient alpha stream is resolved during planning");
        write_resource_dictionary(&mut form.resources(), bindings);
    }
}

fn write_gradient_alpha_ext_gstates<'a>(
    pdf: &mut Pdf,
    page_renders: impl IntoIterator<Item = &'a PageContentRender>,
    dynamic_resources: &PdfResourcePlanner,
) {
    let page_renders = page_renders.into_iter().collect::<Vec<_>>();
    for alpha in page_renders
        .iter()
        .flat_map(|render| &render.gradient_patterns)
        .filter_map(|gradient| gradient.alpha.as_ref())
    {
        let mut state =
            pdf.ext_graphics(pdf_ref(dynamic_resources.ext_gstate(alpha.ext_gstate_id)));
        state
            .soft_mask()
            .subtype(MaskType::Luminosity)
            .group(pdf_ref(dynamic_resources.form(alpha.form_id)));
    }
}

#[allow(clippy::too_many_arguments)]
fn write_form_xobjects<'a>(
    pdf: &mut Pdf,
    page_renders: impl IntoIterator<Item = &'a PageContentRender>,
    color_plan: &PdfColorPlan,
    dynamic_resources: &PdfResourcePlanner,
    compression: crate::PdfCompression,
) {
    let page_renders = page_renders.into_iter().collect::<Vec<_>>();
    for page_render in page_renders {
        for form in &page_render.form_xobjects {
            let stream = encode_pdf_stream(compression, &form.stream.bytes);
            let mut form_writer =
                pdf.form_xobject(pdf_ref(dynamic_resources.form(form.id)), stream.bytes());
            if stream.uses_flate() {
                form_writer.filter(Filter::FlateDecode);
            }
            form_writer.bbox(Rect::new(
                form.bbox.x(),
                form.bbox.y(),
                form.bbox.x() + form.bbox.width(),
                form.bbox.y() + form.bbox.height(),
            ));
            if let PdfFormKind::TransparencyGroup { blending_space } = form.kind {
                let mut group = form_writer.group();
                group.transparency().isolated(true);
                match blending_space {
                    PdfBlendColorSpace::Srgb => group.color_space().icc_based(pdf_ref(
                        color_plan.profile_object_id(crate::css::CssColorSpace::Srgb),
                    )),
                };
            }
            {
                let mut resources = form_writer.resources();
                let bindings = form
                    .stream
                    .resolved_resources
                    .as_ref()
                    .expect("every Form stream is resolved before serialization");
                write_resource_dictionary(&mut resources, bindings);
            }
        }
    }
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
        hash.write_bytes(&render.stream.bytes);
        hash.write_usize(render.form_xobjects.len());
        for form in &render.form_xobjects {
            hash.write_str(&form.name);
            hash.write_f32(form.bbox.x());
            hash.write_f32(form.bbox.y());
            hash.write_f32(form.bbox.width());
            hash.write_f32(form.bbox.height());
            hash.write_bytes(&form.stream.bytes);
            hash.write_bool(matches!(form.kind, PdfFormKind::TransparencyGroup { .. }));
            let form_dependencies = form
                .stream
                .resource_uses
                .xobjects
                .iter()
                .filter_map(|(name, handle)| match handle {
                    PdfXObjectHandle::Form(handle) => Some((name, handle)),
                    PdfXObjectHandle::Image(_) => None,
                })
                .collect::<Vec<_>>();
            hash.write_usize(form_dependencies.len());
            for (name, dependency) in form_dependencies {
                hash.write_usize(dependency.0.0);
                hash.write_str(name);
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
                sampling,
                target_size,
            } => {
                hash.write_u8(1);
                hash.write_u8(*sampling as u8);
                hash.write_u32(target_size.width);
                hash.write_u32(target_size.height);
                hash.write_source_rect(*source_rect);
                document
                    .image_store
                    .write_asset_identity(*image_id, |bytes| hash.write_bytes(bytes));
            }
            ImageResourceSource::Inline {
                pixel_width,
                pixel_height,
                natural_size,
                source_rect,
                sampling,
                target_size,
                rgb,
                alpha,
                color_space,
                sample_depth,
            } => {
                hash.write_u8(2);
                hash.write_u32(*pixel_width);
                hash.write_u32(*pixel_height);
                hash.write_u32(natural_size.width);
                hash.write_u32(natural_size.height);
                match source_rect {
                    Some(source_rect) => {
                        hash.write_bool(true);
                        hash.write_source_rect(*source_rect);
                    }
                    None => hash.write_bool(false),
                }
                hash.write_u8(*sampling as u8);
                hash.write_u32(target_size.width);
                hash.write_u32(target_size.height);
                hash.write_i32(sample_depth.bits_per_component());
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
            hash.write_u8(pattern.sampling as u8);
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
        self.write_optional_str(metadata.language.as_deref());
        self.write_optional_str(metadata.description.as_deref());
        self.write_usize(metadata.keywords.len());
        for keyword in &metadata.keywords {
            self.write_str(keyword);
        }
        self.write_optional_str(metadata.created.as_ref().map(crate::DocumentDate::as_str));
        self.write_optional_str(metadata.modified.as_ref().map(crate::DocumentDate::as_str));
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
