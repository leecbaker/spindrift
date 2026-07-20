use super::*;

pub(super) fn image_source(image: &RenderedImage) -> ImageResourceSource {
    match &image.source {
        crate::document::RenderedImageSource::Stored {
            image_id,
            source_rect,
            ..
        } => ImageResourceSource::Stored {
            image_id: *image_id,
            source_rect: *source_rect,
            interpolate: image.interpolate,
        },
        crate::document::RenderedImageSource::Inline { raster, .. } => {
            ImageResourceSource::Inline {
                pixel_width: raster.pixel_width,
                pixel_height: raster.pixel_height,
                interpolate: image.interpolate,
                color_space: raster.color_space.clone(),
                rgb: Rc::clone(&raster.rgb),
                alpha: raster.alpha.clone(),
            }
        }
    }
}

pub(super) fn image_pattern_source(pattern: &RenderedImagePattern) -> ImageResourceSource {
    match &pattern.source {
        crate::document::RenderedImageSource::Stored {
            image_id,
            source_rect,
            ..
        } => ImageResourceSource::Stored {
            image_id: *image_id,
            source_rect: *source_rect,
            interpolate: pattern.interpolate,
        },
        crate::document::RenderedImageSource::Inline { raster, .. } => {
            ImageResourceSource::Inline {
                pixel_width: raster.pixel_width,
                pixel_height: raster.pixel_height,
                interpolate: pattern.interpolate,
                color_space: raster.color_space.clone(),
                rgb: Rc::clone(&raster.rgb),
                alpha: raster.alpha.clone(),
            }
        }
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
            interpolate,
        } => direct_jpeg_resource(
            image_store,
            *image_id,
            *source_rect,
            *interpolate,
            color_mode,
        )
        .unwrap_or_else(|| {
            image_store
                .with_rasterized(*image_id, |raster| {
                    let data = crop_image_resource_data(
                        raster.metadata.pixel_width,
                        raster.metadata.pixel_height,
                        raster.rgb,
                        raster.alpha,
                        *source_rect,
                    );
                    ImageResource {
                        pixel_width: data.pixel_width,
                        pixel_height: data.pixel_height,
                        interpolate: *interpolate,
                        color_space: raster.color_space,
                        payload: ImagePayload::Samples {
                            rgb: data.rgb,
                            alpha: data.alpha,
                        },
                    }
                })
                .unwrap_or_else(|| transparent_fallback(*interpolate))
        }),
        ImageResourceSource::Inline {
            pixel_width,
            pixel_height,
            interpolate,
            color_space,
            rgb,
            alpha,
        } => ImageResource {
            pixel_width: *pixel_width,
            pixel_height: *pixel_height,
            interpolate: *interpolate,
            color_space: color_space.clone(),
            payload: ImagePayload::Samples {
                rgb: rgb.to_vec(),
                alpha: alpha.as_deref().map(ToOwned::to_owned),
            },
        },
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

/// Convert decoded image samples to the output image color space selected by
/// the document profile. JPEG passthrough intentionally retains its source
/// profile and therefore cannot become a direct graphics fill.
fn convert_image_resource_to_output_color(
    image: &mut ImageResource,
    color_mode: super::colors::PdfColorMode,
) {
    let target_space = match (color_mode, &image.color_space) {
        (super::colors::PdfColorMode::SrgbOutputIntent, _) => Some(crate::css::CssColorSpace::Srgb),
        (
            super::colors::PdfColorMode::PreserveCssSpace,
            crate::color::RasterColorSpace::BuiltIn(crate::css::CssColorSpace::XyzD50),
        ) => Some(crate::css::CssColorSpace::DisplayP3),
        (super::colors::PdfColorMode::PreserveCssSpace, _) => None,
    };
    if let (Some(target_space), ImagePayload::Samples { rgb, .. }) =
        (target_space, &mut image.payload)
    {
        let converted = match &image.color_space {
            crate::color::RasterColorSpace::BuiltIn(space) => {
                crate::color::convert_samples(rgb, *space, target_space)
            }
            crate::color::RasterColorSpace::EmbeddedRgb(profile) => {
                crate::color::convert_embedded_rgb_samples(rgb, profile, target_space)
            }
        };
        if let Some(converted) = converted {
            *rgb = converted;
            image.color_space = crate::color::RasterColorSpace::BuiltIn(target_space);
        }
    }
}

/// Return the exact direct-fill representation for an opaque uniform decoded
/// image. Embedded image profiles are deliberately excluded because page
/// graphics resources only expose Quire's built-in calibrated CSS spaces.
fn solid_fill_from_image_resource(image: &ImageResource) -> Option<SolidImageFill> {
    let ImageResource {
        pixel_width,
        pixel_height,
        color_space: crate::color::RasterColorSpace::BuiltIn(color_space),
        payload: ImagePayload::Samples { rgb, alpha },
        ..
    } = image
    else {
        return None;
    };
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
    rgb.chunks_exact(3)
        .all(|sample| sample == first)
        .then_some(SolidImageFill {
            color_space: *color_space,
            components: first.map(|sample| sample as f32 / 255.0),
        })
}

/// Use a JPEG's original DCT stream only when no source-pixel operation is
/// required. PDF/A output uses a tagged sRGB output condition, so a JPEG with
/// another embedded RGB profile must retain the decoded conversion path.
fn direct_jpeg_resource(
    image_store: &crate::image_store::DocumentImageStore,
    image_id: crate::image_store::ImageId,
    source_rect: RenderedImageSourceRect,
    interpolate: bool,
    color_mode: super::colors::PdfColorMode,
) -> Option<ImageResource> {
    let jpeg = image_store.direct_jpeg(image_id)?;
    let full_source = source_rect.x == 0
        && source_rect.y == 0
        && source_rect.width == jpeg.metadata.pixel_width
        && source_rect.height == jpeg.metadata.pixel_height;
    if !full_source
        || (color_mode == super::colors::PdfColorMode::SrgbOutputIntent
            && jpeg.color_space != crate::color::RasterColorSpace::SRGB)
    {
        return None;
    }
    Some(ImageResource {
        pixel_width: jpeg.metadata.pixel_width,
        pixel_height: jpeg.metadata.pixel_height,
        interpolate,
        color_space: jpeg.color_space,
        payload: ImagePayload::Jpeg(jpeg.bytes),
    })
}

#[cfg(test)]
pub(super) fn image_resource_data(
    image_store: &crate::image_store::DocumentImageStore,
    image: &RenderedImage,
) -> ImageResourceData {
    let (pixel_width, pixel_height, rgb, alpha) = match &image.source {
        crate::document::RenderedImageSource::Stored { image_id, .. } => {
            match image_store.with_rasterized(*image_id, |raster| {
                (
                    raster.metadata.pixel_width,
                    raster.metadata.pixel_height,
                    raster.rgb,
                    raster.alpha,
                )
            }) {
                Some(raster) => raster,
                None => {
                    return ImageResourceData {
                        pixel_width: 1,
                        pixel_height: 1,
                        rgb: vec![0, 0, 0],
                        alpha: Some(vec![0]),
                    };
                }
            }
        }
        crate::document::RenderedImageSource::Inline { raster, .. } => (
            raster.pixel_width,
            raster.pixel_height,
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
    crop_image_resource_data(pixel_width, pixel_height, rgb, alpha, source_rect)
}

fn crop_image_resource_data(
    pixel_width: u32,
    pixel_height: u32,
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
            rgb: vec![0, 0, 0],
            alpha: Some(vec![0]),
        };
    }

    let source_rgb = rgb;
    let source_alpha = alpha;
    let mut cropped_rgb = Vec::with_capacity(cropped_width as usize * cropped_height as usize * 3);
    let mut cropped_alpha = source_alpha
        .as_ref()
        .map(|_| Vec::with_capacity(cropped_width as usize * cropped_height as usize));
    for source_y in y0..y1 {
        let row_start = (source_y as usize * pixel_width as usize + x0 as usize) * 3;
        let row_end = row_start + cropped_width as usize * 3;
        cropped_rgb.extend_from_slice(&source_rgb[row_start..row_end]);
        if let (Some(source_alpha), Some(cropped_alpha)) = (&source_alpha, &mut cropped_alpha) {
            let alpha_row_start = source_y as usize * pixel_width as usize + x0 as usize;
            let alpha_row_end = alpha_row_start + cropped_width as usize;
            cropped_alpha.extend_from_slice(&source_alpha[alpha_row_start..alpha_row_end]);
        }
    }
    ImageResourceData {
        pixel_width: cropped_width,
        pixel_height: cropped_height,
        rgb: cropped_rgb,
        alpha: cropped_alpha,
    }
}

fn transparent_fallback(interpolate: bool) -> ImageResource {
    ImageResource {
        pixel_width: 1,
        pixel_height: 1,
        interpolate,
        color_space: crate::color::RasterColorSpace::SRGB,
        payload: ImagePayload::Samples {
            rgb: vec![0, 0, 0],
            alpha: Some(vec![0]),
        },
    }
}

pub(super) struct ImageResourceData {
    pub(super) pixel_width: u32,
    pub(super) pixel_height: u32,
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
        mode: crate::document::PaintBlendMode,
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
                crate::document::RenderedPathPaint::Solid(color) => {
                    collect_alpha_key(&mut alpha_keys, *color);
                }
                crate::document::RenderedPathPaint::SvgPattern(pattern) => {
                    collect_opacity_key(&mut alpha_keys, pattern.opacity);
                }
                crate::document::RenderedPathPaint::Gradient(_) => {}
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
    collect_alpha_key(alpha_keys, CssColor::TRANSPARENT.with_alpha(opacity));
}

fn collect_paint_tree_ext_gstates(
    alpha_keys: &mut BTreeMap<u16, ()>,
    blend_modes: &mut BTreeMap<crate::document::PaintBlendMode, ()>,
    context: &crate::document::PaintStackingContext,
) {
    collect_opacity_key(alpha_keys, context.effects.opacity);
    if context.effects.blend_mode != crate::document::PaintBlendMode::Normal {
        blend_modes.insert(context.effects.blend_mode, ());
    }
    for band in crate::document::PaintBand::ORDER {
        for item in &context.bands.bands[band.index()] {
            match item {
                crate::document::PaintDisplayItem::StackingContext(child) => {
                    collect_paint_tree_ext_gstates(alpha_keys, blend_modes, child);
                }
                crate::document::PaintDisplayItem::EffectScope(scope) => {
                    collect_effect_scope_ext_gstates(alpha_keys, blend_modes, scope);
                }
                crate::document::PaintDisplayItem::Operation(_)
                | crate::document::PaintDisplayItem::Primitive(_)
                | crate::document::PaintDisplayItem::Link(_) => {}
            }
        }
    }
}

fn collect_effect_scope_ext_gstates(
    alpha_keys: &mut BTreeMap<u16, ()>,
    blend_modes: &mut BTreeMap<crate::document::PaintBlendMode, ()>,
    scope: &crate::document::PaintEffectScope,
) {
    collect_opacity_key(alpha_keys, scope.effects.opacity);
    if scope.effects.blend_mode != crate::document::PaintBlendMode::Normal {
        blend_modes.insert(scope.effects.blend_mode, ());
    }
    for item in &scope.items {
        match item {
            crate::document::PaintDisplayItem::StackingContext(child) => {
                collect_paint_tree_ext_gstates(alpha_keys, blend_modes, child);
            }
            crate::document::PaintDisplayItem::EffectScope(child) => {
                collect_effect_scope_ext_gstates(alpha_keys, blend_modes, child);
            }
            crate::document::PaintDisplayItem::Operation(_)
            | crate::document::PaintDisplayItem::Primitive(_)
            | crate::document::PaintDisplayItem::Link(_) => {}
        }
    }
}

fn alpha_key(color: CssColor) -> Option<u16> {
    if color.is_visible() && !color.is_opaque() {
        Some((color.alpha() * 1000.0).round().clamp(1.0, 999.0) as u16)
    } else {
        None
    }
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
            crate::document::PaintRect::new(
                crate::document::PaintPoint::new(0.0, 0.0),
                crate::document::PaintSize::new(pixel_width as f32, pixel_height as f32),
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
            &image_source(&image),
            super::super::colors::PdfColorMode::SrgbOutputIntent,
            true,
        );

        assert_eq!(
            prepared,
            PreparedImageResource::SolidFill(SolidImageFill {
                color_space: crate::css::CssColorSpace::Srgb,
                components: [0.0, 128.0 / 255.0, 0.0],
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
            payload: ImagePayload::Samples {
                rgb: vec![0, 128, 0],
                alpha: Some(vec![254]),
            },
        };

        assert_eq!(solid_fill_from_image_resource(&non_uniform), None);
        assert_eq!(solid_fill_from_image_resource(&transparent), None);
    }

    #[test]
    fn cropped_jpeg_source_uses_decoded_samples() {
        let mut store = crate::image_store::DocumentImageStore::default();
        let (image_id, _) = store
            .resolve_data_url_with_orientation(
                "data:image/jpeg;base64,fixture",
                Rc::from(opaque_rgb_jpeg().into_boxed_slice()),
                false,
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
            interpolate: false,
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
                false,
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
            interpolate: false,
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
        let first_source = image_source(&first);
        let second_source = image_source(&second);

        assert_eq!(first_source, second_source);
    }
}
