//! Catalog, page-tree, page-content, annotation, and outline serialization.

use super::super::*;
use super::primitives::{i32_from_usize, pdf_ref};
use super::resources::write_resource_dictionary;
use super::stream::encode_pdf_stream;
use crate::pdf::colors::{PdfColorMode, PdfColorPlan};
use pdf_writer::types::{ActionType, AnnotationType, OutputIntentSubtype};
use pdf_writer::{Filter, Pdf, Rect, Str, TextStr};

pub(crate) fn write_catalog(
    pdf: &mut Pdf,
    catalog_id: usize,
    pages_id: usize,
    metadata_id: Option<usize>,
    metadata: &DocumentMetadata,
    outline_plan: Option<&OutlinePlan>,
    color_plan: &PdfColorPlan,
) {
    let mut catalog = pdf.catalog(pdf_ref(catalog_id));
    catalog.pages(pdf_ref(pages_id));
    if let Some(metadata_id) = metadata_id {
        catalog.metadata(pdf_ref(metadata_id));
    }
    if let Some(language) = metadata.language.as_deref() {
        catalog.lang(TextStr(language));
    }
    if let Some(plan) = outline_plan {
        catalog.outlines(pdf_ref(plan.root_id));
    }
    if color_plan.mode() == PdfColorMode::SrgbOutputIntent {
        let mut intents = catalog.output_intents();
        let mut intent = intents.push();
        intent
            .subtype(OutputIntentSubtype::PDFA)
            .output_condition_identifier(TextStr("sRGB"))
            .info(TextStr("sRGB IEC 61966-2.1"))
            .dest_output_profile(pdf_ref(
                color_plan.profile_object_id(crate::css::CssColorSpace::Srgb),
            ));
    }
}

pub(crate) fn write_pages(pdf: &mut Pdf, pages_id: usize, pages: &[PdfPageProgram]) {
    pdf.pages(pdf_ref(pages_id))
        .kids(pages.iter().map(|page| pdf_ref(page.id)))
        .count(i32_from_usize(pages.len()));
}

pub(crate) fn write_pages_and_content(
    pdf: &mut Pdf,
    pages_id: usize,
    pages: &[PdfPageProgram],
    compression: crate::PdfCompression,
) {
    for page in pages {
        let media_box = crate::document::paint::geometry::paint_rect_to_pdf(
            crate::document::paint::geometry::PaintRect::new(
                crate::document::paint::geometry::PaintPoint::new(0.0, 0.0),
                page.size,
            ),
        );
        let mut page_writer = pdf.page(pdf_ref(page.id));
        page_writer.parent(pdf_ref(pages_id)).media_box(Rect::new(
            media_box.origin.x,
            media_box.origin.y,
            media_box.origin.x + media_box.size.width,
            media_box.origin.y + media_box.size.height,
        ));
        if let Some(content_id) = page.content_id {
            page_writer.contents(pdf_ref(content_id));
        }
        if page.rotation != 0 {
            page_writer.rotate(page.rotation);
        }
        if !page.annotations.is_empty() {
            page_writer.annotations(
                page.annotations
                    .iter()
                    .map(|annotation| pdf_ref(annotation.id)),
            );
        }
        let mut resources = page_writer.resources();
        if let Some(bindings) = &page.render.stream.resolved_resources
            && !bindings.is_empty()
        {
            write_resource_dictionary(&mut resources, bindings);
        }
    }

    for page in pages {
        let Some(content_id) = page.content_id else {
            continue;
        };
        let stream = encode_pdf_stream(compression, &page.render.stream.bytes);
        let mut writer = pdf.stream(pdf_ref(content_id), stream.bytes());
        if stream.uses_flate() {
            writer.filter(Filter::FlateDecode);
        }
    }
}

pub(crate) fn write_annotations(pdf: &mut Pdf, pages: &[PdfPageProgram]) {
    for page in pages {
        for link in &page.annotations {
            let mut annotation = pdf.annotation(pdf_ref(link.id));
            let rect = crate::document::paint::geometry::paint_rect_to_pdf(link.rect);
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

pub(crate) fn write_outlines(pdf: &mut Pdf, plan: &OutlinePlan, pages: &[PdfPageProgram]) {
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
        let page_id = pages.get(node.page_index).map(|page| page.id).unwrap_or(0);
        let mut item = pdf.outline_item(pdf_ref(node.id));
        item.title(TextStr(&node.label))
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
        let target = crate::document::paint::geometry::paint_point_to_pdf(node.target);
        item.dest()
            .page(pdf_ref(page_id))
            .xyz(target.x, target.y, Some(0.0));
    }
}
