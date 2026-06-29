use super::*;
use crate::timing::DebugTimer;
use pdf_writer::types::{
    ActionType, AnnotationType, BlendMode, CidFontType, FontFlags, SystemInfo,
};
use pdf_writer::{Name, Pdf, Rect, Ref, Settings, Str, TextStr};

pub(crate) fn write_document(document: &Document, variant: PdfVariant) -> Vec<u8> {
    let _timer = DebugTimer::start("serializing PDF document");
    let page_count = document.pages.len();
    let mut allocator = PdfObjectAllocator::new();

    let catalog_id = allocator.alloc_id();
    let pages_id = allocator.alloc_id();
    let font_id = allocator.alloc_id();
    let page_ids = allocator.alloc_ids(page_count);
    let content_ids = allocator.alloc_ids(page_count);

    let shaped_document = {
        let _timer = DebugTimer::start("shaping document text for PDF");
        shape_document_text(document)
    };
    let embedded_font_plans = {
        let first_embedded_font_id = allocator.peek_id();
        let _timer = DebugTimer::start(format!(
            "planning PDF font embedding for {} document font(s)",
            document.fonts.len()
        ));
        let profile = if variant.is_pdfa() {
            PdfFontValidationProfile::PdfA
        } else {
            PdfFontValidationProfile::Default
        };
        let plans = embedded_font_plans_with_profile(
            document,
            &shaped_document,
            first_embedded_font_id,
            profile,
        );
        allocator.advance_to(
            first_embedded_font_id + plans.fonts.len() * profile.embedded_font_object_count(),
        );
        plans
    };

    let (unique_images, page_image_unique_indexes) = deduplicate_images(document);
    let unique_image_ids = unique_images
        .iter()
        .map(|image| {
            let image_id = allocator.alloc_id();
            let alpha_mask_id = image.alpha.as_ref().map(|_| allocator.alloc_id());
            ImageObjectIds {
                image_id,
                alpha_mask_id,
            }
        })
        .collect::<Vec<_>>();
    let page_image_ids = page_image_unique_indexes
        .iter()
        .map(|page_images| {
            page_images
                .iter()
                .map(|index| unique_image_ids[*index].image_id)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let page_renders = {
        let _timer = DebugTimer::start(format!("building {page_count} page content stream(s)"));
        document
            .pages
            .iter()
            .enumerate()
            .map(|(index, page)| {
                let mut next_dynamic_object_id = allocator.peek_id();
                let render = page_content_render(
                    page,
                    &shaped_document.pages[index],
                    &embedded_font_plans,
                    &mut next_dynamic_object_id,
                );
                allocator.advance_to(next_dynamic_object_id);
                render
            })
            .collect::<Vec<_>>()
    };
    let page_ext_gstate_plans = document
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
        .collect::<Vec<_>>();

    let info_id = allocator.alloc_id();
    let metadata_id = allocator.alloc_id();
    let page_annotation_ids = document
        .pages
        .iter()
        .map(|page| page.links.iter().map(|_| allocator.alloc_id()).collect())
        .collect::<Vec<Vec<_>>>();
    let outline_plan = {
        let _timer = DebugTimer::start(format!(
            "planning {} bookmark outline item(s)",
            document.bookmarks.len()
        ));
        let plan = outline_plan(document, allocator.peek_id());
        if let Some(plan) = &plan {
            allocator.reserve_ids(1 + plan.nodes.len());
        }
        plan
    };

    let mut pdf = Pdf::with_settings(Settings::default());
    let (major_version, minor_version) = variant.pdf_version();
    pdf.set_version(major_version, minor_version);
    pdf.set_binary_marker(b"\xE2\xE3\xCF\xD3");

    write_catalog(
        &mut pdf,
        catalog_id,
        pages_id,
        metadata_id,
        outline_plan.as_ref(),
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
        &page_annotation_ids,
        &page_renders,
        &page_ext_gstate_plans,
    );
    write_embedded_fonts(&mut pdf, &embedded_font_plans.fonts);
    write_images(&mut pdf, &unique_images, &unique_image_ids);
    write_form_xobjects(
        &mut pdf,
        font_id,
        &page_image_ids,
        &page_renders,
        &page_ext_gstate_plans,
    );
    write_ext_gstate_objects(&mut pdf, &page_ext_gstate_plans);
    write_document_info(&mut pdf, pdf_ref(info_id), &document.metadata);
    write_document_xmp_metadata(&mut pdf, pdf_ref(metadata_id), &document.metadata, variant);
    write_annotations(&mut pdf, document, &page_annotation_ids);
    if let Some(outline_plan) = &outline_plan {
        write_outlines(&mut pdf, outline_plan, &page_ids, document);
    }
    pdf.set_file_id(pdf_file_identifier(
        document,
        &page_renders,
        &embedded_font_plans.fonts,
        &unique_images,
        &page_ext_gstate_plans,
    ));

    {
        let object_count = allocator.peek_id().saturating_sub(1);
        let _timer = DebugTimer::start(format!("assembling {object_count} PDF object(s)"));
        pdf.finish()
    }
}

fn deduplicate_images(document: &Document) -> (Vec<&RenderedImage>, Vec<Vec<usize>>) {
    let image_count = document
        .pages
        .iter()
        .map(|page| page.images.len())
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
                    let key = image_key(image);
                    if let Some(index) = image_lookup.get(&key) {
                        *index
                    } else {
                        let index = unique_images.len();
                        image_lookup.insert(key, index);
                        unique_images.push(image);
                        index
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    (unique_images, page_image_unique_indexes)
}

fn write_catalog(
    pdf: &mut Pdf,
    catalog_id: usize,
    pages_id: usize,
    metadata_id: usize,
    outline_plan: Option<&OutlinePlan>,
) {
    let mut catalog = pdf.catalog(pdf_ref(catalog_id));
    catalog.pages(pdf_ref(pages_id));
    catalog.metadata(pdf_ref(metadata_id));
    if let Some(plan) = outline_plan {
        catalog.outlines(pdf_ref(plan.root_id));
    }
}

fn write_pages(pdf: &mut Pdf, pages_id: usize, page_ids: &[usize]) {
    pdf.pages(pdf_ref(pages_id))
        .kids(page_ids.iter().copied().map(pdf_ref))
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
    page_image_ids: &[Vec<usize>],
    page_annotation_ids: &[Vec<usize>],
    page_renders: &[PageContentRender],
    page_ext_gstate_plans: &[Vec<ExtGStateObjectPlan>],
) {
    for (index, page) in document.pages.iter().enumerate() {
        let media_box = crate::document::paint_rect_to_pdf(crate::document::PaintRect::new(
            crate::document::PaintPoint::new(0.0, 0.0),
            page.paint_size(),
        ));
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
            page_writer.annotations(page_annotation_ids[index].iter().copied().map(pdf_ref));
        }
        {
            let mut resources = page_writer.resources();
            write_resource_dictionary(
                &mut resources,
                font_id,
                &page_image_ids[index],
                &page_renders[index],
                &page_ext_gstate_plans[index],
            );
        }
    }

    for (index, page_render) in page_renders.iter().enumerate() {
        pdf.stream(pdf_ref(content_ids[index]), &page_render.stream);
    }
}

fn write_resource_dictionary(
    resources: &mut pdf_writer::writers::Resources<'_>,
    font_id: usize,
    page_image_ids: &[usize],
    page_render: &PageContentRender,
    ext_gstate_plans: &[ExtGStateObjectPlan],
) {
    resources.pair(Name(b"Font"), pdf_ref(font_id));
    if !page_image_ids.is_empty() || !page_render.form_xobjects.is_empty() {
        let mut xobjects = resources.x_objects();
        for (image_index, id) in page_image_ids.iter().enumerate() {
            let name = format!("Im{}", image_index + 1);
            xobjects.pair(pdf_name(&name), pdf_ref(*id));
        }
        for form in &page_render.form_xobjects {
            xobjects.pair(pdf_name(&form.name), pdf_ref(form.id));
        }
    }
    write_ext_gstate_resources(resources, ext_gstate_plans);
}

fn write_ext_gstate_resources(
    resources: &mut pdf_writer::writers::Resources<'_>,
    plans: &[ExtGStateObjectPlan],
) {
    if plans.is_empty() {
        return;
    }
    let mut ext_gstates = resources.ext_g_states();
    for plan in plans {
        ext_gstates.pair(pdf_name(plan.resource.name()), pdf_ref(plan.id));
    }
}

fn write_embedded_fonts(pdf: &mut Pdf, embedded_fonts: &[EmbeddedFontPlan<'_>]) {
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

        let subtype = match font.font.program_kind {
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
                .default_width(font.default_width)
                .cid_to_gid_map_predefined(Name(b"Identity"));
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
            match font.font.program_kind {
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
        {
            let mut font_file = pdf.stream(pdf_ref(font.file_id), data);
            match font.font.program_kind {
                FontProgramKind::TrueType => {
                    font_file.pair(Name(b"Length1"), i32_from_usize(data.len()));
                }
                FontProgramKind::OpenTypeCff => {
                    font_file.pair(Name(b"Subtype"), Name(b"OpenType"));
                }
            }
        }

        let cmap = to_unicode_cmap(font);
        pdf.cmap(pdf_ref(font.to_unicode_id), &cmap);
        if let (Some(cid_set_id), Some(cid_set_data)) =
            (font.cid_set_id, font.cid_set_data.as_ref())
        {
            pdf.stream(pdf_ref(cid_set_id), cid_set_data);
        }
    }
}

fn write_images(
    pdf: &mut Pdf,
    unique_images: &[&RenderedImage],
    unique_image_ids: &[ImageObjectIds],
) {
    let _timer = DebugTimer::start(format!("building {} image object(s)", unique_images.len()));
    for (image_index, image) in unique_images.iter().enumerate() {
        let ids = unique_image_ids[image_index];
        let data = image_resource_data(image);
        {
            let mut image_writer = pdf.image_xobject(pdf_ref(ids.image_id), &data.rgb);
            image_writer
                .width(i32_from_u32(data.pixel_width))
                .height(i32_from_u32(data.pixel_height))
                .color_space_name(Name(b"DeviceRGB"))
                .bits_per_component(8)
                .interpolate(image.interpolate);
            if let Some(mask_id) = ids.alpha_mask_id {
                image_writer.s_mask(pdf_ref(mask_id));
            }
        }
        if let (Some(mask_id), Some(alpha)) = (ids.alpha_mask_id, data.alpha.as_deref()) {
            pdf.image_xobject(pdf_ref(mask_id), alpha)
                .width(i32_from_u32(data.pixel_width))
                .height(i32_from_u32(data.pixel_height))
                .color_space_name(Name(b"DeviceGray"))
                .bits_per_component(8)
                .interpolate(image.interpolate);
        }
    }
}

fn write_form_xobjects(
    pdf: &mut Pdf,
    font_id: usize,
    page_image_ids: &[Vec<usize>],
    page_renders: &[PageContentRender],
    page_ext_gstate_plans: &[Vec<ExtGStateObjectPlan>],
) {
    for (page_index, page_render) in page_renders.iter().enumerate() {
        for form in &page_render.form_xobjects {
            let mut form_writer = pdf.form_xobject(pdf_ref(form.id), &form.stream);
            form_writer.bbox(Rect::new(
                form.bbox.x(),
                form.bbox.y(),
                form.bbox.x() + form.bbox.width(),
                form.bbox.y() + form.bbox.height(),
            ));
            form_writer.group().transparency();
            {
                let mut resources = form_writer.resources();
                write_resource_dictionary(
                    &mut resources,
                    font_id,
                    &page_image_ids[page_index],
                    page_render,
                    &page_ext_gstate_plans[page_index],
                );
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
                ext_gstate.blend_mode(pdf_blend_mode(*mode));
            }
        }
    }
}

fn write_annotations(pdf: &mut Pdf, document: &Document, page_annotation_ids: &[Vec<usize>]) {
    for (page_index, page) in document.pages.iter().enumerate() {
        for (link_index, link) in page.links.iter().enumerate() {
            let mut annotation =
                pdf.annotation(pdf_ref(page_annotation_ids[page_index][link_index]));
            let rect = crate::document::paint_rect_to_pdf(link.paint_rect());
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
        if let Some(first) = top_level_ids.iter().min().copied() {
            outline.first(pdf_ref(first));
        }
        if let Some(last) = top_level_ids.iter().max().copied() {
            outline.last(pdf_ref(last));
        }
    }

    for node in &plan.nodes {
        let page_index = node
            .bookmark
            .page_index
            .min(document.pages.len().saturating_sub(1));
        let page_id = page_ids.get(page_index).copied().unwrap_or(0);
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
        let target = crate::document::paint_point_to_pdf(node.bookmark.target());
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
    unique_images: &[&RenderedImage],
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
        }
    }

    hash.write_usize(embedded_fonts.len());
    for font in embedded_fonts {
        hash.write_str(&font.resource_name);
        hash.write_str(&font.base_name);
        hash.write_bytes(&font.font_file_data);
        hash.write_usize(font.used_glyphs.len());
        for (glyph_id, unicode) in &font.used_glyphs {
            hash.write_u16(*glyph_id);
            hash.write_str(unicode);
        }
        hash.write_font_descriptor_metrics(&font.descriptor_metrics);
        hash.write_f32(font.default_width);
        hash.write_optional_bytes(font.cid_set_data.as_deref());
    }

    hash.write_usize(unique_images.len());
    for image in unique_images {
        let data = image_resource_data(image);
        hash.write_u32(data.pixel_width);
        hash.write_u32(data.pixel_height);
        hash.write_bool(image.interpolate);
        hash.write_bytes(&data.rgb);
        hash.write_optional_bytes(data.alpha.as_deref());
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
        self.write_str(&metadata.producer);
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

fn pdf_blend_mode(mode: crate::document::PaintBlendMode) -> BlendMode {
    match mode {
        crate::document::PaintBlendMode::Normal => BlendMode::Normal,
        crate::document::PaintBlendMode::Multiply => BlendMode::Multiply,
        crate::document::PaintBlendMode::Screen => BlendMode::Screen,
        crate::document::PaintBlendMode::Overlay => BlendMode::Overlay,
        crate::document::PaintBlendMode::Darken => BlendMode::Darken,
        crate::document::PaintBlendMode::Lighten => BlendMode::Lighten,
        crate::document::PaintBlendMode::ColorDodge => BlendMode::ColorDodge,
        crate::document::PaintBlendMode::ColorBurn => BlendMode::ColorBurn,
        crate::document::PaintBlendMode::HardLight => BlendMode::HardLight,
        crate::document::PaintBlendMode::SoftLight => BlendMode::SoftLight,
        crate::document::PaintBlendMode::Difference => BlendMode::Difference,
        crate::document::PaintBlendMode::Exclusion => BlendMode::Exclusion,
        crate::document::PaintBlendMode::Hue => BlendMode::Hue,
        crate::document::PaintBlendMode::Saturation => BlendMode::Saturation,
        crate::document::PaintBlendMode::Color => BlendMode::Color,
        crate::document::PaintBlendMode::Luminosity => BlendMode::Luminosity,
    }
}

fn i32_from_usize(value: usize) -> i32 {
    i32::try_from(value).expect("PDF object value exceeds i32 range")
}

fn i32_from_u32(value: u32) -> i32 {
    i32::try_from(value).expect("PDF object value exceeds i32 range")
}
