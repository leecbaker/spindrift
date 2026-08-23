//! Raster-image and raster-tiling-pattern serialization.

use pdf_writer::types::{PaintType, TilingType};
use pdf_writer::{Filter, Name, Pdf, Rect};

use super::super::*;
use super::primitives::{i32_from_u32, pdf_ref};
use super::resources::write_resource_dictionary;
use super::stream::encode_pdf_stream;
use crate::pdf::colors::PdfColorPlan;
use crate::timing::DebugTimer;

pub(crate) fn write_images(
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

/// Emit local-resource, colored tiling patterns for repeated raster backgrounds.
/// ISO 32000-1:2008, 8.7.3.
pub(crate) fn write_image_patterns(
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
