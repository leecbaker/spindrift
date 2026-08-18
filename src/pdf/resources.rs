use super::*;
use crate::document::paint::images::RasterSampling;
use crate::units::RasterPixelSize;

pub(super) fn image_source(image: &RenderedImage, resolution_dppx: f32) -> ImageResourceSource {
    let target_size = target_raster_size(image.paint_rect().size, resolution_dppx);
    match &image.source {
        crate::document::paint::images::RenderedImageSource::Stored {
            image_id,
            source_rect,
            ..
        } => ImageResourceSource::Stored {
            image_id: *image_id,
            source_rect: *source_rect,
            sampling: image.sampling,
            target_size,
        },
        crate::document::paint::images::RenderedImageSource::Inline {
            raster,
            source_rect,
        } => ImageResourceSource::Inline {
            pixel_width: raster.pixel_width,
            pixel_height: raster.pixel_height,
            natural_size: raster.natural_size,
            source_rect: *source_rect,
            sampling: image.sampling,
            target_size,
            color_space: raster.color_space.clone(),
            sample_depth: raster.sample_depth,
            rgb: Rc::clone(&raster.rgb),
            alpha: raster.alpha.clone(),
        },
    }
}

pub(super) fn image_pattern_source(
    pattern: &RenderedImagePattern,
    resolution_dppx: f32,
) -> ImageResourceSource {
    let target_size = target_raster_size(pattern.tiling.tile_size, resolution_dppx);
    match &pattern.source {
        crate::document::paint::images::RenderedImageSource::Stored {
            image_id,
            source_rect,
            ..
        } => ImageResourceSource::Stored {
            image_id: *image_id,
            source_rect: *source_rect,
            sampling: pattern.sampling,
            target_size,
        },
        crate::document::paint::images::RenderedImageSource::Inline {
            raster,
            source_rect,
        } => ImageResourceSource::Inline {
            pixel_width: raster.pixel_width,
            pixel_height: raster.pixel_height,
            natural_size: raster.natural_size,
            source_rect: *source_rect,
            sampling: pattern.sampling,
            target_size,
            color_space: raster.color_space.clone(),
            sample_depth: raster.sample_depth,
            rgb: Rc::clone(&raster.rgb),
            alpha: raster.alpha.clone(),
        },
    }
}

/// Expand one lightweight image source immediately before its PDF objects are
/// emitted. The resulting pixels must not escape the writer's per-image loop.
pub(super) fn materialize_image_resource(
    image_store: &crate::image_store::DocumentImageStore,
    source: &ImageResourceSource,
    color_mode: super::colors::PdfColorMode,
) -> ImageResource {
    match source {
        ImageResourceSource::Stored {
            image_id,
            source_rect,
            sampling,
            target_size,
        } => {
            // A PDF image XObject is scaled by its placement transform.  An
            // ordinary CSS `auto` image therefore does not need its encoded
            // JPEG samples resampled merely because its used CSS size differs
            // from its intrinsic pixel size.  Retaining the DCT stream is
            // both lossless and preserves an embedded ICC profile.
            let source_is_target_sized =
                target_size.width == source_rect.width && target_size.height == source_rect.height;
            if (source_is_target_sized || *sampling == RasterSampling::Auto)
                && let Some(resource) =
                    direct_jpeg_resource(image_store, *image_id, *source_rect, color_mode)
            {
                return resource;
            }
            image_store
                .with_rasterized(*image_id, |raster| {
                    let data = crop_image_resource_data(
                        raster.metadata.pixel_size.width,
                        raster.metadata.pixel_size.height,
                        raster.sample_depth,
                        raster.rgb,
                        raster.alpha,
                        *source_rect,
                    );
                    sampled_image_resource(
                        data,
                        raster.metadata.natural_size,
                        raster.metadata.pixel_size,
                        *source_rect,
                        *target_size,
                        *sampling,
                        raster.color_space,
                    )
                })
                .unwrap_or_else(transparent_fallback)
        }
        ImageResourceSource::Inline {
            pixel_width,
            pixel_height,
            natural_size,
            source_rect,
            sampling,
            target_size,
            color_space,
            sample_depth,
            rgb,
            alpha,
        } => {
            let data = crop_image_resource_data(
                *pixel_width,
                *pixel_height,
                *sample_depth,
                rgb.to_vec(),
                alpha.as_deref().map(ToOwned::to_owned),
                source_rect.unwrap_or(RenderedImageSourceRect {
                    x: 0,
                    y: 0,
                    width: *pixel_width,
                    height: *pixel_height,
                }),
            );
            sampled_image_resource(
                data,
                *natural_size,
                RasterPixelSize::new(*pixel_width, *pixel_height),
                source_rect.unwrap_or(RenderedImageSourceRect {
                    x: 0,
                    y: 0,
                    width: *pixel_width,
                    height: *pixel_height,
                }),
                *target_size,
                *sampling,
                color_space.clone(),
            )
        }
    }
}

/// Preserve an eligible JPEG only when sampling would be an exact identity.
///
/// Any CSS resampling mode, source crop, or output-profile conversion must
/// decode to samples first so the chosen algorithm is deterministic.
fn direct_jpeg_resource(
    image_store: &crate::image_store::DocumentImageStore,
    image_id: crate::image_store::ImageId,
    source_rect: RenderedImageSourceRect,
    color_mode: super::colors::PdfColorMode,
) -> Option<ImageResource> {
    let jpeg = image_store.direct_jpeg(image_id)?;
    let full_source = source_rect.x == 0
        && source_rect.y == 0
        && source_rect.width == jpeg.metadata.pixel_size.width
        && source_rect.height == jpeg.metadata.pixel_size.height;
    if !full_source
        || (color_mode == super::colors::PdfColorMode::SrgbOutputIntent
            && jpeg.color_space != crate::color::RasterColorSpace::SRGB)
    {
        return None;
    }
    Some(ImageResource {
        pixel_width: jpeg.metadata.pixel_size.width,
        pixel_height: jpeg.metadata.pixel_size.height,
        interpolate: false,
        color_space: jpeg.color_space,
        sample_depth: crate::image_store::RasterSampleDepth::Eight,
        payload: ImagePayload::Jpeg(jpeg.bytes),
    })
}

/// Map a used paint extent to Quire's static CSS device grid.
///
/// A PDF has no inherent raster density. The density selected for CSS media
/// queries and `image-set()` is therefore the deterministic output grid for
/// CSS Images sampling as well.
fn target_raster_size(
    size: crate::document::paint::geometry::PaintSize,
    resolution_dppx: f32,
) -> RasterPixelSize {
    let axis = |extent: f32| {
        let css_pixels = extent / crate::css::CSS_PX_TO_PT * resolution_dppx;
        if !css_pixels.is_finite() || css_pixels <= 0.0 {
            1
        } else {
            css_pixels.round().clamp(1.0, u32::MAX as f32) as u32
        }
    };
    RasterPixelSize::new(axis(size.width), axis(size.height))
}

/// Materialize the selected raster source on the final CSS device grid.
///
/// `pixelated` first scales the selected image to the closest positive integer
/// multiple of its natural CSS size with nearest-neighbor, then applies the
/// smooth pass to the final target. `crisp-edges` is nearest-neighbor at the
/// final target. Other values choose Quire's deterministic smooth policy.
/// <https://drafts.csswg.org/css-images-3/#the-image-rendering>
fn sampled_image_resource(
    data: ImageResourceData,
    natural_size: crate::units::CssPixelSize,
    full_pixel_size: RasterPixelSize,
    _source_rect: RenderedImageSourceRect,
    target_size: RasterPixelSize,
    sampling: RasterSampling,
    color_space: crate::color::RasterColorSpace,
) -> ImageResource {
    let selected_natural_width =
        natural_size.width as f32 * data.pixel_width as f32 / full_pixel_size.width.max(1) as f32;
    let selected_natural_height = natural_size.height as f32 * data.pixel_height as f32
        / full_pixel_size.height.max(1) as f32;
    let data = match sampling {
        RasterSampling::CrispEdges => {
            resize_image_resource_data(data, target_size, RasterResampling::Nearest)
        }
        RasterSampling::Pixelated => {
            let multiplier = |target: u32, natural: f32| {
                if !natural.is_finite() || natural <= 0.0 {
                    1
                } else {
                    // Positive half-way values choose the larger integer.
                    ((target as f32 / natural) + 0.5).floor().max(1.0) as u32
                }
            };
            let intermediate = RasterPixelSize::new(
                data.pixel_width
                    .saturating_mul(multiplier(target_size.width, selected_natural_width)),
                data.pixel_height
                    .saturating_mul(multiplier(target_size.height, selected_natural_height)),
            );
            let data = resize_image_resource_data(data, intermediate, RasterResampling::Nearest);
            resize_image_resource_data(data, target_size, RasterResampling::Linear)
        }
        // `auto` leaves an already-rasterized CSS image at its resolved
        // source grid.  In particular, generated gradients choose that grid
        // while resolving their tile; resampling it again to the placement
        // size loses the requested tile resolution.
        RasterSampling::Auto => data,
        RasterSampling::Smooth | RasterSampling::HighQuality => {
            resize_image_resource_data(data, target_size, RasterResampling::Cubic)
        }
    };
    ImageResource {
        pixel_width: data.pixel_width,
        pixel_height: data.pixel_height,
        // Explicit sampling modes have already selected samples. `auto`
        // retains the source grid and lets the PDF consumer interpolate it.
        interpolate: sampling == RasterSampling::Auto,
        color_space,
        sample_depth: data.sample_depth,
        payload: ImagePayload::Samples {
            rgb: data.rgb,
            alpha: data.alpha,
        },
    }
}

/// Resample separated RGB and alpha planes without losing 16-bit precision.
///
/// Smooth resampling works in premultiplied alpha so transparent colors cannot
/// introduce a fringe. The caller has already cropped the CSS image source,
/// which keeps border-image slices sampling-contained.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RasterResampling {
    Nearest,
    Linear,
    Cubic,
}

fn resize_image_resource_data(
    source: ImageResourceData,
    target: RasterPixelSize,
    method: RasterResampling,
) -> ImageResourceData {
    if source.pixel_width == target.width && source.pixel_height == target.height {
        return source;
    }
    let component_bytes = source.sample_depth.bytes_per_component();
    let Some(pixel_count) = usize::try_from(target.width).ok().and_then(|width| {
        usize::try_from(target.height)
            .ok()
            .and_then(|height| width.checked_mul(height))
    }) else {
        return source;
    };
    let Some(rgb_len) = pixel_count
        .checked_mul(3)
        .and_then(|value| value.checked_mul(component_bytes))
    else {
        return source;
    };
    let alpha_len = source
        .alpha
        .as_ref()
        .and_then(|_| pixel_count.checked_mul(component_bytes));
    let Some(total_bytes) = (match alpha_len {
        Some(length) => rgb_len.checked_add(length),
        None => Some(rgb_len),
    }) else {
        return source;
    };
    // A CSS paint rectangle can be arbitrarily large while a PDF resource is
    // necessarily finite. Refuse an excessive intermediate deterministically
    // instead of overflowing or attempting an unbounded allocation.
    const MAX_MATERIALIZED_RASTER_BYTES: usize = 512 * 1024 * 1024;
    if total_bytes > MAX_MATERIALIZED_RASTER_BYTES {
        return source;
    }
    let mut rgb = Vec::new();
    if rgb.try_reserve_exact(rgb_len).is_err() {
        return source;
    }
    rgb.resize(rgb_len, 0);
    let mut alpha = if let Some(length) = alpha_len {
        let mut samples = Vec::new();
        if samples.try_reserve_exact(length).is_err() {
            return source;
        }
        samples.resize(length, 0);
        Some(samples)
    } else {
        None
    };
    let source_width = source.pixel_width as usize;
    let source_height = source.pixel_height as usize;
    let target_width = target.width as usize;
    let target_height = target.height as usize;
    for target_y in 0..target_height {
        for target_x in 0..target_width {
            let mut output_alpha = 0.0;
            let mut components = [0.0; 3];
            let mut contribute = |source_x: usize, source_y: usize, weight: f32| {
                let source_index = source_y * source_width + source_x;
                let source_alpha = source.alpha.as_ref().map_or(1.0, |samples| {
                    read_component(samples, source_index, component_bytes)
                });
                output_alpha += source_alpha * weight;
                for (component, output) in components.iter_mut().enumerate() {
                    *output +=
                        read_component(&source.rgb, source_index * 3 + component, component_bytes)
                            * source_alpha
                            * weight;
                }
            };
            match method {
                RasterResampling::Nearest | RasterResampling::Linear => {
                    let nearest = method == RasterResampling::Nearest;
                    let (y0, y1, fy) =
                        resample_axis(target_y, target_height, source_height, nearest);
                    let (x0, x1, fx) = resample_axis(target_x, target_width, source_width, nearest);
                    for (source_x, source_y, weight) in [
                        (x0, y0, (1.0 - fx) * (1.0 - fy)),
                        (x1, y0, fx * (1.0 - fy)),
                        (x0, y1, (1.0 - fx) * fy),
                        (x1, y1, fx * fy),
                    ] {
                        contribute(source_x, source_y, weight);
                    }
                }
                RasterResampling::Cubic => {
                    for (source_y, y_weight) in
                        cubic_resample_axis(target_y, target_height, source_height)
                    {
                        for (source_x, x_weight) in
                            cubic_resample_axis(target_x, target_width, source_width)
                        {
                            contribute(source_x, source_y, x_weight * y_weight);
                        }
                    }
                }
            }
            let target_index = target_y * target_width + target_x;
            if output_alpha > 0.0 {
                for (component, value) in components.into_iter().enumerate() {
                    write_component(
                        &mut rgb,
                        target_index * 3 + component,
                        component_bytes,
                        value / output_alpha,
                    );
                }
            }
            if let Some(alpha) = &mut alpha {
                write_component(alpha, target_index, component_bytes, output_alpha);
            }
        }
    }
    ImageResourceData {
        pixel_width: target.width,
        pixel_height: target.height,
        sample_depth: source.sample_depth,
        rgb,
        alpha,
    }
}

/// Smooth cubic B-spline weights for Quire's deterministic `auto` policy.
///
/// The four-tap filter is deliberately distinct from the final linear pass
/// required by `pixelated`: the property asks the UA to choose its ordinary
/// smooth algorithm, while pixelated's first nearest-neighbor stage is fixed.
fn cubic_resample_axis(index: usize, target_len: usize, source_len: usize) -> [(usize, f32); 4] {
    let position = (index as f32 + 0.5) * source_len as f32 / target_len as f32 - 0.5;
    let base = position.floor() as isize;
    std::array::from_fn(|offset| {
        let source =
            (base + offset as isize - 1).clamp(0, source_len.saturating_sub(1) as isize) as usize;
        let distance = (position - (base + offset as isize - 1) as f32).abs();
        let weight = if distance < 1.0 {
            (4.0 - 6.0 * distance * distance + 3.0 * distance * distance * distance) / 6.0
        } else if distance < 2.0 {
            (2.0 - distance).powi(3) / 6.0
        } else {
            0.0
        };
        (source, weight)
    })
}

fn resample_axis(
    index: usize,
    target_len: usize,
    source_len: usize,
    nearest: bool,
) -> (usize, usize, f32) {
    let position = (index as f32 + 0.5) * source_len as f32 / target_len as f32 - 0.5;
    if nearest {
        let pixel = position
            .floor()
            .clamp(0.0, source_len.saturating_sub(1) as f32) as usize;
        return (pixel, pixel, 0.0);
    }
    let start = position
        .floor()
        .clamp(0.0, source_len.saturating_sub(1) as f32) as usize;
    let end = (start + 1).min(source_len.saturating_sub(1));
    (start, end, (position - position.floor()).clamp(0.0, 1.0))
}

fn read_component(samples: &[u8], index: usize, component_bytes: usize) -> f32 {
    let offset = index * component_bytes;
    match component_bytes {
        1 => samples[offset] as f32 / 255.0,
        2 => u16::from_be_bytes([samples[offset], samples[offset + 1]]) as f32 / 65535.0,
        _ => unreachable!("raster samples have one or two bytes per component"),
    }
}

fn write_component(samples: &mut [u8], index: usize, component_bytes: usize, value: f32) {
    let offset = index * component_bytes;
    match component_bytes {
        1 => samples[offset] = (value.clamp(0.0, 1.0) * 255.0).round() as u8,
        2 => samples[offset..offset + 2]
            .copy_from_slice(&((value.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes()),
        _ => unreachable!("raster samples have one or two bytes per component"),
    }
}

/// Resolve one image source into its final PDF paint representation.
///
/// The solid-fill classification happens only after source cropping and the
/// selected PDF output conversion. Consequently a promoted fill selects the
/// same calibrated components an image XObject would have carried.
/// ISO 32000-2:2020, 8.6.5 and 8.9.5.
pub(super) fn prepare_image_resource(
    image_store: &crate::image_store::DocumentImageStore,
    source: &ImageResourceSource,
    color_mode: super::colors::PdfColorMode,
    solid_fill_eligible: bool,
) -> PreparedImageResource {
    let mut image = materialize_image_resource(image_store, source, color_mode);
    if image_resource_is_fully_transparent(&image) {
        return PreparedImageResource::Transparent;
    }
    convert_image_resource_to_output_color(&mut image, color_mode);
    if super::PROMOTE_SOLID_RASTER_IMAGES_TO_VECTOR_FILLS
        && solid_fill_eligible
        && let Some(fill) = solid_fill_from_image_resource(&image)
    {
        PreparedImageResource::SolidFill(fill)
    } else {
        PreparedImageResource::Raster(image)
    }
}

/// Return whether no source sample contributes paint after source cropping.
///
/// Emitting an interpolated all-zero PDF soft mask can create a rasterizer
/// fringe despite having no visible CSS paint. Model that state explicitly so
/// the writer can omit the image and any pattern that would reference it.
fn image_resource_is_fully_transparent(image: &ImageResource) -> bool {
    matches!(
        &image.payload,
        ImagePayload::Samples {
            alpha: Some(alpha),
            ..
        } if alpha.iter().all(|alpha| *alpha == 0)
    )
}

/// Convert decoded image samples to the output image color space selected by
/// the document profile. JPEG passthrough intentionally retains its source
/// profile and therefore cannot become a direct graphics fill.
fn convert_image_resource_to_output_color(
    image: &mut ImageResource,
    color_mode: super::colors::PdfColorMode,
) {
    let target_space = match (color_mode, &image.color_space) {
        (super::colors::PdfColorMode::SrgbOutputIntent, _) => Some(crate::css::CssColorSpace::Srgb),
        (super::colors::PdfColorMode::PreserveCssSpace, _) => None,
    };
    if let (Some(target_space), ImagePayload::Samples { rgb, .. }) =
        (target_space, &mut image.payload)
    {
        let converted = match &image.color_space {
            crate::color::RasterColorSpace::BuiltIn(space) => {
                crate::color::convert_samples_at_depth(
                    rgb,
                    image.sample_depth,
                    *space,
                    target_space,
                )
            }
            crate::color::RasterColorSpace::EmbeddedRgb(profile) => {
                crate::color::convert_embedded_rgb_samples_at_depth(
                    rgb,
                    image.sample_depth,
                    profile,
                    target_space,
                )
            }
        };
        if let Some(converted) = converted {
            *rgb = converted;
            image.color_space = crate::color::RasterColorSpace::BuiltIn(target_space);
        }
    }
}

/// Return the exact direct-fill representation for an opaque uniform decoded
/// image. Its retained component space is emitted through the matching PDF
/// ICCBased resource, including an ordinary-PDF embedded source profile.
fn solid_fill_from_image_resource(image: &ImageResource) -> Option<SolidImageFill> {
    let ImageResource {
        pixel_width,
        pixel_height,
        color_space,
        sample_depth,
        payload: ImagePayload::Samples { rgb, alpha },
        ..
    } = image
    else {
        return None;
    };
    if *sample_depth != crate::image_store::RasterSampleDepth::Eight {
        return None;
    }
    let pixel_count = (*pixel_width as usize).checked_mul(*pixel_height as usize)?;
    if pixel_count == 0 || rgb.len() != pixel_count.checked_mul(3)? {
        return None;
    }
    if alpha
        .as_ref()
        .is_some_and(|alpha| alpha.len() != pixel_count || alpha.iter().any(|alpha| *alpha != 255))
    {
        return None;
    }
    let first = [rgb[0], rgb[1], rgb[2]];
    rgb.as_chunks::<3>()
        .0
        .iter()
        .all(|sample| sample == &first)
        .then_some(SolidImageFill {
            color_space: color_space.clone(),
            components: first,
        })
}

/// Use a JPEG's original DCT stream only when no source-pixel operation is
/// required. PDF/A output uses a tagged sRGB output condition, so a JPEG with
/// another embedded RGB profile must retain the decoded conversion path.
#[cfg(test)]
pub(super) fn image_resource_data(
    image_store: &crate::image_store::DocumentImageStore,
    image: &RenderedImage,
) -> ImageResourceData {
    let (pixel_width, pixel_height, sample_depth, rgb, alpha) = match &image.source {
        crate::document::paint::images::RenderedImageSource::Stored { image_id, .. } => {
            match image_store.with_rasterized(*image_id, |raster| {
                (
                    raster.metadata.pixel_size.width,
                    raster.metadata.pixel_size.height,
                    raster.sample_depth,
                    raster.rgb,
                    raster.alpha,
                )
            }) {
                Some(raster) => raster,
                None => {
                    return ImageResourceData {
                        pixel_width: 1,
                        pixel_height: 1,
                        sample_depth: crate::image_store::RasterSampleDepth::Eight,
                        rgb: vec![0, 0, 0],
                        alpha: Some(vec![0]),
                    };
                }
            }
        }
        crate::document::paint::images::RenderedImageSource::Inline { raster, .. } => (
            raster.pixel_width,
            raster.pixel_height,
            raster.sample_depth,
            raster.rgb.to_vec(),
            raster.alpha.as_deref().map(ToOwned::to_owned),
        ),
    };
    let source_rect = image.source_rect().unwrap_or(RenderedImageSourceRect {
        x: 0,
        y: 0,
        width: pixel_width,
        height: pixel_height,
    });
    crop_image_resource_data(
        pixel_width,
        pixel_height,
        sample_depth,
        rgb,
        alpha,
        source_rect,
    )
}

fn crop_image_resource_data(
    pixel_width: u32,
    pixel_height: u32,
    sample_depth: crate::image_store::RasterSampleDepth,
    rgb: Vec<u8>,
    alpha: Option<Vec<u8>>,
    source_rect: RenderedImageSourceRect,
) -> ImageResourceData {
    if source_rect.x == 0
        && source_rect.y == 0
        && source_rect.width == pixel_width
        && source_rect.height == pixel_height
    {
        return ImageResourceData {
            pixel_width,
            pixel_height,
            sample_depth,
            rgb,
            alpha,
        };
    }
    let x0 = source_rect.x.min(pixel_width);
    let y0 = source_rect.y.min(pixel_height);
    let x1 = x0.saturating_add(source_rect.width).min(pixel_width);
    let y1 = y0.saturating_add(source_rect.height).min(pixel_height);
    let cropped_width = x1.saturating_sub(x0);
    let cropped_height = y1.saturating_sub(y0);
    if cropped_width == 0 || cropped_height == 0 {
        return ImageResourceData {
            pixel_width: 1,
            pixel_height: 1,
            sample_depth: crate::image_store::RasterSampleDepth::Eight,
            rgb: vec![0, 0, 0],
            alpha: Some(vec![0]),
        };
    }

    let source_rgb = rgb;
    let source_alpha = alpha;
    let component_bytes = sample_depth.bytes_per_component();
    let mut cropped_rgb =
        Vec::with_capacity(cropped_width as usize * cropped_height as usize * 3 * component_bytes);
    let mut cropped_alpha = source_alpha.as_ref().map(|_| {
        Vec::with_capacity(cropped_width as usize * cropped_height as usize * component_bytes)
    });
    for source_y in y0..y1 {
        let row_start =
            (source_y as usize * pixel_width as usize + x0 as usize) * 3 * component_bytes;
        let row_end = row_start + cropped_width as usize * 3 * component_bytes;
        cropped_rgb.extend_from_slice(&source_rgb[row_start..row_end]);
        if let (Some(source_alpha), Some(cropped_alpha)) = (&source_alpha, &mut cropped_alpha) {
            let alpha_row_start =
                (source_y as usize * pixel_width as usize + x0 as usize) * component_bytes;
            let alpha_row_end = alpha_row_start + cropped_width as usize * component_bytes;
            cropped_alpha.extend_from_slice(&source_alpha[alpha_row_start..alpha_row_end]);
        }
    }
    ImageResourceData {
        pixel_width: cropped_width,
        pixel_height: cropped_height,
        sample_depth,
        rgb: cropped_rgb,
        alpha: cropped_alpha,
    }
}

fn transparent_fallback() -> ImageResource {
    ImageResource {
        pixel_width: 1,
        pixel_height: 1,
        interpolate: false,
        color_space: crate::color::RasterColorSpace::SRGB,
        sample_depth: crate::image_store::RasterSampleDepth::Eight,
        payload: ImagePayload::Samples {
            rgb: vec![0, 0, 0],
            alpha: Some(vec![0]),
        },
    }
}

#[derive(Clone)]
pub(super) struct ImageResourceData {
    pub(super) pixel_width: u32,
    pub(super) pixel_height: u32,
    pub(super) sample_depth: crate::image_store::RasterSampleDepth,
    pub(super) rgb: Vec<u8>,
    pub(super) alpha: Option<Vec<u8>>,
}

/// Return the PDF graphics-state resource name for a semi-transparent color.
///
/// PDF 1.4 transparency uses ExtGState dictionaries with stroking (`CA`) and
/// nonstroking (`ca`) alpha constants:
/// ISO 32000-1:2008, 11.7.4.3 "Constant Shape and Opacity".
pub(super) fn paint_alpha_resource_name(color: CssColor) -> Option<String> {
    alpha_key(color).map(|key| format!("GSalpha{key:03}"))
}

/// Return the PDF graphics-state resource name for CSS group opacity.
///
/// Unlike an individual transparent paint color, `opacity: 0` still needs a
/// graphics state: it suppresses an otherwise paintable transparency group.
/// ISO 32000 permits zero for both constant-alpha entries:
/// ISO 32000-1:2008, 11.7.4.3 "Constant Shape and Opacity".
pub(super) fn paint_opacity_resource_name(opacity: f32) -> Option<String> {
    opacity_key(opacity).map(|key| format!("GSalpha{key:03}"))
}

/// Plan a page-local `/ExtGState` resource for alpha paints.
///
/// PDF page resource dictionaries name ExtGState resources, and content streams
/// activate them with the `gs` operator:
/// ISO 32000-1:2008, 7.8.3 "Resource Dictionaries" and 8.4.5 "Graphics State
/// Parameter Dictionaries".
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ExtGStateResource {
    Alpha {
        name: String,
        alpha: f32,
    },
    Blend {
        name: String,
        mode: crate::document::paint::effects::PaintBlendMode,
    },
}

impl ExtGStateResource {
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Alpha { name, .. } | Self::Blend { name, .. } => name,
        }
    }
}

/// Collect page-local `/ExtGState` resource entries for alpha and blend modes.
///
/// PDF 1.4 transparency uses ExtGState dictionaries with stroking (`CA`) and
/// nonstroking (`ca`) alpha constants, and blend modes are selected with the
/// `/BM` graphics-state parameter:
/// ISO 32000-1:2008, 8.4.5 "Graphics State Parameter Dictionaries" and
/// 11.3.5 "Blend Mode".
pub(super) fn page_ext_gstate_resources(page: &Page) -> Vec<ExtGStateResource> {
    let mut alpha_keys = BTreeMap::new();
    let mut blend_modes = BTreeMap::new();
    for rect in &page.rects {
        if let Some(fill) = rect.fill {
            collect_alpha_key(&mut alpha_keys, fill);
        }
        if let Some(stroke) = rect.stroke {
            collect_alpha_key(&mut alpha_keys, stroke);
        }
    }
    for rect in &page.rounded_rects {
        if let Some(fill) = rect.fill {
            collect_alpha_key(&mut alpha_keys, fill);
        }
        if let Some(stroke) = rect.stroke {
            collect_alpha_key(&mut alpha_keys, stroke);
        }
    }
    for stroke in &page.strokes {
        collect_alpha_key(&mut alpha_keys, stroke.color);
    }
    for path in &page.paths {
        for paint in [path.fill_paint.as_ref(), path.stroke_paint.as_ref()]
            .into_iter()
            .flatten()
        {
            match paint {
                crate::document::paint::paths::RenderedPathPaint::Solid(color) => {
                    collect_alpha_key(&mut alpha_keys, *color);
                }
                crate::document::paint::paths::RenderedPathPaint::SvgPattern(pattern) => {
                    collect_opacity_key(&mut alpha_keys, pattern.opacity);
                }
                crate::document::paint::paths::RenderedPathPaint::Gradient(_) => {}
            }
        }
    }
    for line in &page.lines {
        collect_alpha_key(&mut alpha_keys, line.color);
    }
    collect_paint_tree_ext_gstates(&mut alpha_keys, &mut blend_modes, &page.paint_tree().root);
    if alpha_keys.is_empty() && blend_modes.is_empty() {
        return Vec::new();
    }
    let mut entries = alpha_keys
        .into_keys()
        .map(|key| {
            let alpha = key as f32 / 1000.0;
            ExtGStateResource::Alpha {
                name: format!("GSalpha{key:03}"),
                alpha,
            }
        })
        .collect::<Vec<_>>();
    entries.extend(blend_modes.into_keys().filter_map(|mode| {
        Some(ExtGStateResource::Blend {
            name: mode.resource_name()?,
            mode,
        })
    }));
    entries
}

fn collect_alpha_key(alpha_keys: &mut BTreeMap<u16, ()>, color: CssColor) {
    if let Some(key) = alpha_key(color) {
        alpha_keys.insert(key, ());
    }
}

fn collect_opacity_key(alpha_keys: &mut BTreeMap<u16, ()>, opacity: f32) {
    if let Some(key) = opacity_key(opacity) {
        alpha_keys.insert(key, ());
    }
}

fn collect_paint_tree_ext_gstates(
    alpha_keys: &mut BTreeMap<u16, ()>,
    blend_modes: &mut BTreeMap<crate::document::paint::effects::PaintBlendMode, ()>,
    context: &crate::document::paint::stacking::PaintStackingContext,
) {
    collect_opacity_key(alpha_keys, context.effects.opacity);
    if context.effects.blend_mode != crate::document::paint::effects::PaintBlendMode::Normal {
        blend_modes.insert(context.effects.blend_mode, ());
    }
    for band in crate::document::paint::display_list::PaintBand::ORDER {
        for item in &context.bands.bands[band.index()] {
            match item {
                crate::document::paint::display_list::PaintDisplayItem::StackingContext(child) => {
                    collect_paint_tree_ext_gstates(alpha_keys, blend_modes, child);
                }
                crate::document::paint::display_list::PaintDisplayItem::EffectScope(scope) => {
                    collect_effect_scope_ext_gstates(alpha_keys, blend_modes, scope);
                }
                crate::document::paint::display_list::PaintDisplayItem::Operation(_)
                | crate::document::paint::display_list::PaintDisplayItem::Primitive(_)
                | crate::document::paint::display_list::PaintDisplayItem::Link(_) => {}
            }
        }
    }
}

fn collect_effect_scope_ext_gstates(
    alpha_keys: &mut BTreeMap<u16, ()>,
    blend_modes: &mut BTreeMap<crate::document::paint::effects::PaintBlendMode, ()>,
    scope: &crate::document::paint::effects::PaintEffectScope,
) {
    collect_opacity_key(alpha_keys, scope.effects.opacity);
    if scope.effects.blend_mode != crate::document::paint::effects::PaintBlendMode::Normal {
        blend_modes.insert(scope.effects.blend_mode, ());
    }
    for item in &scope.items {
        match item {
            crate::document::paint::display_list::PaintDisplayItem::StackingContext(child) => {
                collect_paint_tree_ext_gstates(alpha_keys, blend_modes, child);
            }
            crate::document::paint::display_list::PaintDisplayItem::EffectScope(child) => {
                collect_effect_scope_ext_gstates(alpha_keys, blend_modes, child);
            }
            crate::document::paint::display_list::PaintDisplayItem::Operation(_)
            | crate::document::paint::display_list::PaintDisplayItem::Primitive(_)
            | crate::document::paint::display_list::PaintDisplayItem::Link(_) => {}
        }
    }
}

fn alpha_key(color: CssColor) -> Option<u16> {
    // Fully transparent colors still issue normal paint operations, so they
    // need an explicit zero-alpha graphics state. Omitting it makes CSS
    // `color: transparent` paint with the previously active PDF color.
    // <https://www.w3.org/TR/css-color-4/#transparency> and ISO 32000-1:2008,
    // 11.7.4.3 "Constant Shape and Opacity".
    opacity_key(color.alpha())
}

fn opacity_key(opacity: f32) -> Option<u16> {
    (opacity.is_finite() && (0.0..1.0).contains(&opacity))
        .then(|| (opacity * 1000.0).round().clamp(0.0, 999.0) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;
    use std::rc::Rc;

    fn opaque_rgb_jpeg() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
            .write_image(
                &[240, 32, 16, 16, 192, 64, 32, 64, 240, 224, 224, 32],
                2,
                2,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        bytes
    }

    fn test_image(
        pixel_width: u32,
        pixel_height: u32,
        rgb: Rc<[u8]>,
        alpha: Option<Rc<[u8]>>,
        source_rect: Option<RenderedImageSourceRect>,
    ) -> RenderedImage {
        RenderedImage::from_paint_rect(
            crate::document::paint::geometry::PaintRect::new(
                crate::document::paint::geometry::PaintPoint::new(0.0, 0.0),
                crate::document::paint::geometry::PaintSize::new(
                    pixel_width as f32,
                    pixel_height as f32,
                ),
            ),
            false,
            pixel_width,
            pixel_height,
            source_rect,
            false,
            rgb,
            alpha,
            None,
        )
    }

    #[test]
    fn uncropped_image_resource_data_copies_inline_pixels_for_emission() {
        let rgb: Rc<[u8]> = Rc::from(vec![1, 2, 3, 4, 5, 6].into_boxed_slice());
        let alpha: Rc<[u8]> = Rc::from(vec![255, 127].into_boxed_slice());
        let image = test_image(2, 1, Rc::clone(&rgb), Some(Rc::clone(&alpha)), None);

        let data = image_resource_data(&crate::image_store::DocumentImageStore::default(), &image);

        assert_eq!(data.pixel_width, 2);
        assert_eq!(data.pixel_height, 1);
        assert_eq!(data.rgb, rgb.as_ref());
        assert_eq!(data.alpha.as_deref(), Some(alpha.as_ref()));
    }

    #[test]
    fn cropped_image_resource_data_contains_source_rect_pixels() {
        let rgb: Rc<[u8]> = Rc::from(
            vec![
                1, 2, 3, 4, 5, 6, //
                7, 8, 9, 10, 11, 12,
            ]
            .into_boxed_slice(),
        );
        let alpha: Rc<[u8]> = Rc::from(vec![10, 20, 30, 40].into_boxed_slice());
        let image = test_image(
            2,
            2,
            rgb,
            Some(alpha),
            Some(RenderedImageSourceRect {
                x: 1,
                y: 0,
                width: 1,
                height: 2,
            }),
        );

        let data = image_resource_data(&crate::image_store::DocumentImageStore::default(), &image);

        assert_eq!(data.pixel_width, 1);
        assert_eq!(data.pixel_height, 2);
        assert_eq!(data.rgb.as_slice(), &[4, 5, 6, 10, 11, 12]);
        assert_eq!(data.alpha.as_deref(), Some([20, 40].as_slice()));
    }

    #[test]
    fn inline_source_crop_is_preserved_for_pdf_resource_emission() {
        let image = test_image(
            2,
            2,
            Rc::from(
                vec![
                    255, 0, 0, 0, 128, 0, // red beside selected green
                    255, 0, 0, 0, 128, 0,
                ]
                .into_boxed_slice(),
            ),
            None,
            Some(RenderedImageSourceRect {
                x: 1,
                y: 0,
                width: 1,
                height: 2,
            }),
        );

        let resource = materialize_image_resource(
            &crate::image_store::DocumentImageStore::default(),
            &image_source(&image, 1.0),
            super::super::colors::PdfColorMode::SrgbOutputIntent,
        );

        // PDF preparation samples on the final 1dppx CSS grid. The selected
        // crop is still isolated before that resize, so no neighboring red
        // samples may enter the result.
        assert_eq!(resource.pixel_width, 3);
        assert_eq!(resource.pixel_height, 3);
        let ImagePayload::Samples { rgb, alpha } = resource.payload else {
            panic!("inline raster should materialize as samples");
        };
        assert!(rgb.chunks_exact(3).all(|pixel| pixel == [0, 128, 0]));
        assert_eq!(alpha, None);
    }

    #[test]
    fn cropped_sixteen_bit_samples_keep_component_boundaries() {
        let image = test_image(
            2,
            1,
            Rc::from([
                0, 1, 0, 2, 0, 3, // first RGB sample
                1, 1, 1, 2, 1, 3, // second RGB sample
            ]),
            Some(Rc::from([0xff, 0xff, 0x12, 0x34])),
            Some(RenderedImageSourceRect {
                x: 1,
                y: 0,
                width: 1,
                height: 1,
            }),
        )
        .with_raster_sample_depth(crate::image_store::RasterSampleDepth::Sixteen);

        let data = image_resource_data(&crate::image_store::DocumentImageStore::default(), &image);

        assert_eq!(
            data.sample_depth,
            crate::image_store::RasterSampleDepth::Sixteen
        );
        assert_eq!(data.rgb, [1, 1, 1, 2, 1, 3]);
        assert_eq!(data.alpha, Some(vec![0x12, 0x34]));
    }

    #[test]
    fn cropped_opaque_uniform_samples_promote_to_their_final_fill_color() {
        let image = test_image(
            2,
            2,
            Rc::from(vec![0, 128, 0, 0, 128, 0, 0, 128, 0, 0, 128, 0].into_boxed_slice()),
            Some(Rc::from(vec![255, 255, 255, 255].into_boxed_slice())),
            Some(RenderedImageSourceRect {
                x: 1,
                y: 0,
                width: 1,
                height: 2,
            }),
        );
        let prepared = prepare_image_resource(
            &crate::image_store::DocumentImageStore::default(),
            &image_source(&image, 1.0),
            super::super::colors::PdfColorMode::SrgbOutputIntent,
            true,
        );

        assert_eq!(
            prepared,
            PreparedImageResource::SolidFill(SolidImageFill {
                color_space: crate::color::RasterColorSpace::SRGB,
                components: [0, 128, 0],
            })
        );
    }

    #[test]
    fn opaque_uniform_embedded_profile_samples_promote_in_ordinary_pdf() {
        let profile = Rc::from(
            crate::color::icc_profile_bytes(crate::css::CssColorSpace::DisplayP3)
                .unwrap()
                .into_boxed_slice(),
        );
        let source = ImageResourceSource::Inline {
            pixel_width: 2,
            pixel_height: 1,
            natural_size: crate::units::CssPixelSize::new(2, 1),
            source_rect: None,
            sampling: RasterSampling::CrispEdges,
            target_size: RasterPixelSize::new(2, 1),
            color_space: crate::color::RasterColorSpace::EmbeddedRgb(Rc::clone(&profile)),
            sample_depth: crate::image_store::RasterSampleDepth::Eight,
            rgb: Rc::from([153_u8, 0, 0, 153, 0, 0]),
            alpha: None,
        };

        let prepared = prepare_image_resource(
            &crate::image_store::DocumentImageStore::default(),
            &source,
            super::super::colors::PdfColorMode::PreserveCssSpace,
            true,
        );

        assert_eq!(
            prepared,
            PreparedImageResource::SolidFill(SolidImageFill {
                color_space: crate::color::RasterColorSpace::EmbeddedRgb(profile),
                components: [153, 0, 0],
            })
        );
    }

    #[test]
    fn non_uniform_or_transparent_samples_remain_raster_images() {
        let non_uniform = ImageResource {
            pixel_width: 2,
            pixel_height: 1,
            interpolate: false,
            color_space: crate::color::RasterColorSpace::SRGB,
            sample_depth: crate::image_store::RasterSampleDepth::Eight,
            payload: ImagePayload::Samples {
                rgb: vec![0, 128, 0, 0, 129, 0],
                alpha: None,
            },
        };
        let transparent = ImageResource {
            pixel_width: 1,
            pixel_height: 1,
            interpolate: false,
            color_space: crate::color::RasterColorSpace::SRGB,
            sample_depth: crate::image_store::RasterSampleDepth::Eight,
            payload: ImagePayload::Samples {
                rgb: vec![0, 128, 0],
                alpha: Some(vec![254]),
            },
        };

        assert_eq!(solid_fill_from_image_resource(&non_uniform), None);
        assert_eq!(solid_fill_from_image_resource(&transparent), None);
    }

    #[test]
    fn fully_transparent_interpolated_samples_are_omitted_from_pdf_paint() {
        let image = test_image(
            1,
            1,
            Rc::from([255_u8, 255, 255]),
            Some(Rc::from([0_u8])),
            None,
        );

        assert_eq!(
            prepare_image_resource(
                &crate::image_store::DocumentImageStore::default(),
                &image_source(&image, 1.0),
                super::super::colors::PdfColorMode::SrgbOutputIntent,
                false,
            ),
            PreparedImageResource::Transparent
        );
    }

    #[test]
    fn cropped_jpeg_source_uses_decoded_samples() {
        let mut store = crate::image_store::DocumentImageStore::default();
        let (image_id, _) = store
            .resolve_data_url_with_orientation(
                "data:image/jpeg;base64,fixture",
                Rc::from(opaque_rgb_jpeg().into_boxed_slice()),
                crate::image_store::RasterOrientationPolicy::Encoded,
            )
            .unwrap();
        let source = ImageResourceSource::Stored {
            image_id,
            source_rect: RenderedImageSourceRect {
                x: 1,
                y: 0,
                width: 1,
                height: 2,
            },
            sampling: RasterSampling::CrispEdges,
            target_size: RasterPixelSize::new(1, 2),
        };

        let image = materialize_image_resource(
            &store,
            &source,
            super::super::colors::PdfColorMode::SrgbOutputIntent,
        );

        assert_eq!((image.pixel_width, image.pixel_height), (1, 2));
        assert!(matches!(image.payload, ImagePayload::Samples { .. }));
    }

    #[test]
    fn direct_jpeg_sources_remain_raster_images() {
        let mut store = crate::image_store::DocumentImageStore::default();
        let (image_id, _) = store
            .resolve_data_url_with_orientation(
                "data:image/jpeg;base64,fixture",
                Rc::from(opaque_rgb_jpeg().into_boxed_slice()),
                crate::image_store::RasterOrientationPolicy::Encoded,
            )
            .unwrap();
        let source = ImageResourceSource::Stored {
            image_id,
            source_rect: RenderedImageSourceRect {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
            sampling: RasterSampling::CrispEdges,
            target_size: RasterPixelSize::new(2, 2),
        };

        assert!(matches!(
            prepare_image_resource(
                &store,
                &source,
                super::super::colors::PdfColorMode::PreserveCssSpace,
                true,
            ),
            PreparedImageResource::Raster(ImageResource {
                payload: ImagePayload::Jpeg(_),
                ..
            })
        ));
    }

    #[test]
    fn repeated_inline_image_sources_are_deduplicated_without_copying_pixels() {
        let rgb: Rc<[u8]> = Rc::from(
            vec![
                1, 2, 3, 4, 5, 6, //
                7, 8, 9, 10, 11, 12,
            ]
            .into_boxed_slice(),
        );
        let source_rect = Some(RenderedImageSourceRect {
            x: 1,
            y: 0,
            width: 1,
            height: 2,
        });
        let first = test_image(2, 2, Rc::clone(&rgb), None, source_rect);
        let second = test_image(2, 2, rgb, None, source_rect);
        let first_source = image_source(&first, 1.0);
        let second_source = image_source(&second, 1.0);

        assert_eq!(first_source, second_source);
    }

    #[test]
    fn crisp_edges_resampling_never_blends_source_colors() {
        let image = sampled_image_resource(
            ImageResourceData {
                pixel_width: 2,
                pixel_height: 1,
                sample_depth: crate::image_store::RasterSampleDepth::Eight,
                rgb: vec![255, 0, 0, 0, 0, 255],
                alpha: None,
            },
            crate::units::CssPixelSize::new(2, 1),
            RasterPixelSize::new(2, 1),
            RenderedImageSourceRect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
            RasterPixelSize::new(3, 1),
            RasterSampling::CrispEdges,
            crate::color::RasterColorSpace::SRGB,
        );
        let ImagePayload::Samples { rgb, .. } = image.payload else {
            panic!("resampling produces decoded samples");
        };
        assert_eq!(rgb, vec![255, 0, 0, 255, 0, 0, 0, 0, 255]);
    }

    #[test]
    fn pixelated_performs_a_smooth_second_stage_for_non_integer_targets() {
        let source = ImageResourceData {
            pixel_width: 2,
            pixel_height: 1,
            sample_depth: crate::image_store::RasterSampleDepth::Eight,
            rgb: vec![255, 0, 0, 0, 0, 255],
            alpha: None,
        };
        let resource = |sampling| {
            sampled_image_resource(
                source.clone(),
                crate::units::CssPixelSize::new(2, 1),
                RasterPixelSize::new(2, 1),
                RenderedImageSourceRect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                RasterPixelSize::new(3, 1),
                sampling,
                crate::color::RasterColorSpace::SRGB,
            )
        };
        let ImagePayload::Samples { rgb: pixelated, .. } =
            resource(RasterSampling::Pixelated).payload
        else {
            unreachable!();
        };
        let ImagePayload::Samples { rgb: crisp, .. } = resource(RasterSampling::CrispEdges).payload
        else {
            unreachable!();
        };
        assert_ne!(pixelated, crisp);
        assert!(
            pixelated
                .chunks_exact(3)
                .any(|pixel| pixel[0] > 0 && pixel[2] > 0)
        );
    }

    #[test]
    fn smooth_resampling_preserves_sixteen_bit_alpha_planes() {
        let data = resize_image_resource_data(
            ImageResourceData {
                pixel_width: 1,
                pixel_height: 1,
                sample_depth: crate::image_store::RasterSampleDepth::Sixteen,
                rgb: vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc],
                alpha: Some(vec![0xab, 0xcd]),
            },
            RasterPixelSize::new(2, 1),
            RasterResampling::Linear,
        );
        assert_eq!(
            data.sample_depth,
            crate::image_store::RasterSampleDepth::Sixteen
        );
        assert_eq!(data.rgb.len(), 2 * 3 * 2);
        assert_eq!(data.alpha.as_deref().map(<[u8]>::len), Some(2 * 2));
    }
}
