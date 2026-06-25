use super::*;
use crate::timing::DebugTimer;

pub(crate) fn write_document(document: &Document) -> Vec<u8> {
    let _timer = DebugTimer::start("serializing PDF document");
    let page_count = document.pages.len();
    let catalog_id = 1;
    let pages_id = 2;
    let font_id = 3;
    let first_page_id = 4;
    let first_content_id = first_page_id + page_count;
    let shaped_document = {
        let _timer = DebugTimer::start("shaping document text for PDF");
        shape_document_text(document)
    };
    let first_embedded_font_id = first_content_id + page_count;
    let embedded_font_plans = {
        let _timer = DebugTimer::start(format!(
            "planning PDF font embedding for {} document font(s)",
            document.fonts.len()
        ));
        embedded_font_plans(document, &shaped_document, first_embedded_font_id)
    };
    let first_image_id =
        first_embedded_font_id + embedded_font_plans.fonts.len() * EMBEDDED_FONT_OBJECTS;
    let mut image_lookup = HashMap::new();
    let mut unique_images = Vec::new();
    let page_image_unique_indexes = {
        let image_count = document
            .pages
            .iter()
            .map(|page| page.images.len())
            .sum::<usize>();
        let _timer = DebugTimer::start(format!("deduplicating {image_count} image reference(s)"));
        document
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
            .collect::<Vec<_>>()
    };
    let mut next_image_object_id = first_image_id;
    let unique_image_ids = unique_images
        .iter()
        .map(|image| {
            let image_id = next_image_object_id;
            next_image_object_id += 1;
            let alpha_mask_id = image.alpha.as_ref().map(|_| {
                let mask_id = next_image_object_id;
                next_image_object_id += 1;
                mask_id
            });
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
    let mut next_dynamic_object_id = next_image_object_id;
    let page_renders = {
        let _timer = DebugTimer::start(format!("building {page_count} page content stream(s)"));
        document
            .pages
            .iter()
            .enumerate()
            .map(|(index, page)| {
                page_content_render(
                    page,
                    &shaped_document.pages[index],
                    &embedded_font_plans,
                    &mut next_dynamic_object_id,
                )
            })
            .collect::<Vec<_>>()
    };
    let info_id = next_dynamic_object_id;
    let first_annotation_id = info_id + 1;
    let mut next_annotation_id = first_annotation_id;
    let page_annotation_ids = document
        .pages
        .iter()
        .map(|page| {
            page.links
                .iter()
                .map(|_| {
                    let id = next_annotation_id;
                    next_annotation_id += 1;
                    id
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let annotation_count = page_annotation_ids.iter().map(Vec::len).sum::<usize>();
    let first_outline_id = first_annotation_id + annotation_count;
    let outline_plan = {
        let _timer = DebugTimer::start(format!(
            "planning {} bookmark outline item(s)",
            document.bookmarks.len()
        ));
        outline_plan(document, first_outline_id)
    };

    let mut objects = Vec::new();
    objects.push((
        catalog_id,
        catalog_dictionary(outline_plan.as_ref()).into_bytes(),
    ));

    let kids = (0..page_count)
        .map(|index| format!("{} 0 R", first_page_id + index))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push((
        pages_id,
        format!("<< /Type /Pages /Kids [{kids}] /Count {page_count} >>\n").into_bytes(),
    ));

    objects.push((
        font_id,
        font_resource_dictionary(&embedded_font_plans.fonts).into_bytes(),
    ));

    for (index, page) in document.pages.iter().enumerate() {
        let page_id = first_page_id + index;
        let content_id = first_content_id + index;
        let xobject_entries = page_image_ids[index]
            .iter()
            .enumerate()
            .map(|(image_index, id)| format!("/Im{} {id} 0 R", image_index + 1))
            .chain(
                page_renders[index]
                    .form_xobjects
                    .iter()
                    .map(|form| format!("/{} {} 0 R", form.name, form.id)),
            )
            .collect::<Vec<_>>();
        let xobjects = if xobject_entries.is_empty() {
            String::new()
        } else {
            format!(" /XObject << {} >>", xobject_entries.join(" "))
        };
        let annots = if page_annotation_ids[index].is_empty() {
            String::new()
        } else {
            format!(
                " /Annots [{}]",
                page_annotation_ids[index]
                    .iter()
                    .map(|id| format!("{id} 0 R"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        let ext_gstates = page_ext_gstate_resource_dictionary(page);
        let rotate = if page.rotation == 0 {
            String::new()
        } else {
            format!(" /Rotate {}", page.rotation)
        };
        objects.push((
            page_id,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.3} {:.3}]{rotate} /Resources << /Font 3 0 R{xobjects}{ext_gstates} >> /Contents {content_id} 0 R{annots} >>\n",
                page.width, page.height
            )
            .into_bytes(),
        ));
    }

    {
        for (index, _page) in document.pages.iter().enumerate() {
            let content_id = first_content_id + index;
            let stream = &page_renders[index].stream;
            objects.push((
                content_id,
                format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes(),
            ));
            objects.last_mut().unwrap().1.extend_from_slice(stream);
            objects
                .last_mut()
                .unwrap()
                .1
                .extend_from_slice(b"\nendstream\n");
        }
    }

    {
        let _timer = DebugTimer::start(format!(
            "building {} embedded font object set(s)",
            embedded_font_plans.fonts.len()
        ));
        for (index, font) in embedded_font_plans.fonts.iter().enumerate() {
            let base_id = first_embedded_font_id + index * EMBEDDED_FONT_OBJECTS;
            objects.push((base_id, embedded_type0_font_object(font).into_bytes()));
            objects.push((base_id + 1, embedded_cid_font_object(font).into_bytes()));
            objects.push((
                base_id + 2,
                embedded_font_descriptor_object(font).into_bytes(),
            ));
            objects.push((base_id + 3, embedded_font_file_object(font)));
            objects.push((base_id + 4, to_unicode_object(font).into_bytes()));
        }
    }

    {
        let _timer = DebugTimer::start(format!("building {} image object(s)", unique_images.len()));
        for (image_index, image) in unique_images.iter().enumerate() {
            if let Some(ids) = unique_image_ids.get(image_index) {
                let data = image_resource_data(image);
                objects.push((
                    ids.image_id,
                    image_object_dictionary(
                        data.pixel_width,
                        data.pixel_height,
                        image.interpolate,
                        &data.rgb,
                        ids.alpha_mask_id,
                    ),
                ));
                if let (Some(mask_id), Some(alpha)) = (ids.alpha_mask_id, data.alpha.as_deref()) {
                    objects.push((
                        mask_id,
                        image_alpha_mask_object(
                            data.pixel_width,
                            data.pixel_height,
                            image.interpolate,
                            alpha,
                        ),
                    ));
                }
            }
        }
    }

    for (page_index, page_render) in page_renders.iter().enumerate() {
        for form in &page_render.form_xobjects {
            objects.push((
                form.id,
                form_xobject_object(
                    form,
                    &document.pages[page_index],
                    &page_image_ids[page_index],
                    page_render,
                ),
            ));
        }
    }

    objects.push((info_id, info_dictionary(document).into_bytes()));
    for (page_index, page) in document.pages.iter().enumerate() {
        for (link_index, link) in page.links.iter().enumerate() {
            objects.push((
                page_annotation_ids[page_index][link_index],
                annotation_dictionary(link).into_bytes(),
            ));
        }
    }
    if let Some(outline_plan) = &outline_plan {
        objects.extend(outline_objects(outline_plan, first_page_id, document));
    }

    {
        objects.sort_by_key(|(id, _)| *id);
        let _timer = DebugTimer::start(format!("assembling {} PDF object(s)", objects.len()));
        let mut output = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = vec![0usize];
        for (id, body) in &objects {
            offsets.push(output.len());
            output.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
            output.extend_from_slice(body);
            output.extend_from_slice(b"endobj\n");
        }

        let xref_offset = output.len();
        output.extend_from_slice(format!("xref\n0 {}\n", offsets.len()).as_bytes());
        output.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            output.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        output.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R /Info {info_id} 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                offsets.len(),
            )
            .as_bytes(),
        );
        output
    }
}

fn form_xobject_object(
    form: &FormXObjectRender,
    page: &Page,
    page_image_ids: &[usize],
    page_render: &PageContentRender,
) -> Vec<u8> {
    let image_entries = page_image_ids
        .iter()
        .enumerate()
        .map(|(image_index, id)| format!("/Im{} {id} 0 R", image_index + 1));
    let form_entries = page_render
        .form_xobjects
        .iter()
        .map(|candidate| format!("/{} {} 0 R", candidate.name, candidate.id));
    let xobject_entries = image_entries.chain(form_entries).collect::<Vec<_>>();
    let xobjects = if xobject_entries.is_empty() {
        String::new()
    } else {
        format!(" /XObject << {} >>", xobject_entries.join(" "))
    };
    let ext_gstates = page_ext_gstate_resource_dictionary(page);
    let mut object = format!(
        "<< /Type /XObject /Subtype /Form /BBox [{:.3} {:.3} {:.3} {:.3}] /Group << /S /Transparency >> /Resources << /Font 3 0 R{xobjects}{ext_gstates} >> /Length {} >>\nstream\n",
        form.bbox.x,
        form.bbox.y,
        form.bbox.x + form.bbox.width,
        form.bbox.y + form.bbox.height,
        form.stream.len()
    )
    .into_bytes();
    object.extend_from_slice(&form.stream);
    object.extend_from_slice(b"\nendstream\n");
    object
}
