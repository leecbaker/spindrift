use super::*;
use crate::document::paint::patterns::PaintPatternTiling;
use std::rc::Rc;

/// Resolves and tiles a CSS background image layer for any box-like area.
///
/// CSS Backgrounds and Borders defines background image sizing, positioning,
/// and repetition independently of the formatting context that produced the
/// box. This shared helper is used by document boxes, page boxes, and
/// page-margin boxes so generated page content paints backgrounds with the
/// same semantics as normal elements:
/// <https://www.w3.org/TR/css-backgrounds-3/#backgrounds>.
pub(in crate::layout) fn background_image_primitives_for_style(
    area: PaintBackgroundArea,
    style: &ComputedStyle,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    background_image_primitives_for_style_with_paint_areas_and_fixed_positioning_area(
        area,
        area,
        None,
        style.has_transform(),
        style,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
    )
}

pub(in crate::layout) fn background_image_primitives_for_style_with_paint_areas(
    positioning_border_area: PaintBackgroundArea,
    clip_border_area: PaintBackgroundArea,
    style: &ComputedStyle,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    background_image_primitives_for_style_with_paint_areas_and_fixed_positioning_area(
        positioning_border_area,
        clip_border_area,
        None,
        style.has_transform(),
        style,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
    )
}

/// Paint a structural table background image without assuming a separate
/// box-decoration phase will emit vector hard-stop gradients.
///
/// Table columns, rows, and row groups are painting layers, not independent
/// boxes, and therefore must emit every background-image layer themselves.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>.
pub(in crate::layout) fn structural_table_background_image_primitives(
    positioning_border_area: PaintBackgroundArea,
    clip_border_area: PaintBackgroundArea,
    style: &ComputedStyle,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    background_image_primitives_for_style_impl(
        BackgroundPaintAreas {
            positioning_border_area,
            clip_border_area,
            fixed_positioning_area: None,
            fixed_attachment_is_scrolled_by_transform: style.has_transform(),
        },
        style,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
        true,
        false,
        true,
    )
}

/// Resolve a table-root background across an internal sliced fragment edge.
/// Such an edge has no border paint to occlude the border-box background.
pub(in crate::layout) fn fragmented_table_root_background_image_primitives(
    positioning_border_area: PaintBackgroundArea,
    clip_border_area: PaintBackgroundArea,
    style: &ComputedStyle,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    background_image_primitives_for_style_impl(
        BackgroundPaintAreas {
            positioning_border_area,
            clip_border_area,
            fixed_positioning_area: None,
            fixed_attachment_is_scrolled_by_transform: style.has_transform(),
        },
        style,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
        true,
        true,
        false,
    )
}

/// Resolves background layers with a viewport-equivalent positioning area for
/// `background-attachment: fixed` layers.
///
/// A fixed layer still clips to its element's selected background-clip area,
/// but its positioning area is the viewport (or another fixed-position
/// containing block supplied by the caller). This keeps attachment separate
/// from origin and clip geometry.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-attachment>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn background_image_primitives_for_style_with_paint_areas_and_fixed_positioning_area(
    positioning_border_area: PaintBackgroundArea,
    clip_border_area: PaintBackgroundArea,
    fixed_positioning_area: Option<PaintBackgroundArea>,
    fixed_attachment_is_scrolled_by_transform: bool,
    style: &ComputedStyle,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    background_image_primitives_for_style_impl(
        BackgroundPaintAreas {
            positioning_border_area,
            clip_border_area,
            fixed_positioning_area,
            fixed_attachment_is_scrolled_by_transform,
        },
        style,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
        true,
        true,
        true,
    )
}

fn background_image_primitives_for_style_impl(
    paint_areas: BackgroundPaintAreas<PaintSpace>,
    style: &ComputedStyle,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    use_pdf_patterns_for_repeated_images: bool,
    box_decoration_paints_vector_gradients: bool,
    allow_border_box_occlusion_optimization: bool,
) -> Vec<PaintPrimitive> {
    let BackgroundPaintAreas {
        positioning_border_area,
        clip_border_area,
        fixed_positioning_area,
        fixed_attachment_is_scrolled_by_transform,
    } = paint_areas;
    let mut primitives = Vec::new();
    for layer in background_layers_for_paint(style).iter().rev() {
        let positioning_area = background_positioning_area_for_layer(
            positioning_border_area,
            fixed_positioning_area,
            fixed_attachment_is_scrolled_by_transform,
            style,
            layer,
        );
        let clip_box = if allow_border_box_occlusion_optimization
            && background_border_box_paint_is_occluded(style, layer.clip)
        {
            css::BackgroundBox::Padding
        } else {
            layer.clip
        };
        let clip_area = background_paint_area_for_box(clip_border_area, style, clip_box);
        let rounded_clip = rounded_background_clip_for_box(
            paint_space_rect(
                clip_border_area.x(),
                clip_border_area.y(),
                clip_border_area.width(),
                clip_border_area.height(),
            ),
            style,
            used_border_widths(style),
            clip_box,
        );
        let color_image = match layer.image.as_image().map(BackgroundImage::selected_image) {
            Some(BackgroundImage::CssColor(color)) => Some(color.resolve(style.color)),
            _ => None,
        };
        if let Some(BackgroundImage::Url {
            src,
            base_url,
            root_url,
            request_modifiers,
        }) = layer.image.as_image().map(BackgroundImage::selected_image)
            && let Some(ResolvedImageAsset::Svg(asset)) = load_resolved_image_source_with_request(
                src,
                base_url.as_ref().or(fallback_base_url),
                root_url.as_ref().or(fallback_root_url),
                resource_cache,
                raster_image_interpolation(style),
                request_modifiers,
            )
        {
            let image_size = used_svg_background_layer_size(&asset, layer, positioning_area.size());
            if image_size.width <= 0.0 || image_size.height <= 0.0 {
                continue;
            }
            let asset = Rc::new(asset.with_background_viewport(image_size));
            let tile = ResolvedBackgroundTile::new(
                positioning_area,
                clip_area,
                rounded_clip,
                layer,
                image_size,
            );
            if let Some(color) = asset.opaque_viewport_fill() {
                // A uniformly opaque SVG is geometrically equivalent to a
                // `image(<color>)` layer. Reuse the color-image painter so
                // repeat coalescing and CSS clipping remain identical while
                // avoiding unbounded vector coordinates from extreme SVG
                // viewBoxes.
                append_color_image_primitives(&mut primitives, color, &tile);
                continue;
            }
            if let Some((visible_area, source)) = non_repeating_svg_visible_area(&asset, &tile)
                && let Some(color) = asset.opaque_source_rect_fill(source)
            {
                primitives.push(uniform_background_rect_primitive(
                    visible_area,
                    color,
                    tile.rounded_clip.clone(),
                ));
                continue;
            }
            // A repeated SVG is one reusable vector cell.  Expanding it into
            // paths here turns a 0.2px CSS tile into 250,000 page operations;
            // represent the repetition in PDF instead.  The pattern's outer
            // paint is clipped to the CSS background area, so the cell itself
            // remains reusable for every occurrence.
            if tile.repeat.repeats_x() || tile.repeat.repeats_y() {
                let area = tile.clip_area;
                if area.width() <= 0.0 || area.height() <= 0.0 {
                    continue;
                }
                let origin_x = background_first_tile_position(
                    (f64::from(tile.positioning_area.x()) + tile.offset.x) as f32,
                    tile.positioning_area.x(),
                    tile.positioning_area.width(),
                    tile.size.width,
                    tile.repeat.x_axis(),
                );
                let origin_y = background_first_tile_position(
                    (f64::from(tile.positioning_area.y()) + tile.offset.y) as f32,
                    tile.positioning_area.y(),
                    tile.positioning_area.height(),
                    tile.size.height,
                    tile.repeat.y_axis(),
                );
                let step_width = background_pattern_step(
                    tile.size.width,
                    tile.positioning_area.width(),
                    tile.repeat.x_axis(),
                );
                let step_height = background_pattern_step(
                    tile.size.height,
                    tile.positioning_area.height(),
                    tile.repeat.y_axis(),
                );
                let paths = asset.paint_paths(PaintRect::new(PaintPoint::new(0.0, 0.0), tile.size));
                if step_width > 0.0 && step_height > 0.0 && !paths.is_empty() {
                    primitives.push(PaintPrimitive::SvgPattern(RenderedSvgPattern::new(
                        area.paint_rect(),
                        PaintPatternTiling::new(
                            tile.size,
                            PaintSize::new(step_width, step_height),
                            PaintPoint::new(origin_x, origin_y),
                        ),
                        paths,
                        tile.rounded_clip.clone(),
                    )));
                }
                continue;
            }
            for tile_area in tile.tiles() {
                for mut path in asset.paint_paths(tile_area.paint_rect()) {
                    let clip_area = tile.clip_area.paint_rect();
                    let needs_rectangular_clip = path.clip.is_some()
                        || path
                            .paint_bounds()
                            .is_none_or(|bounds| !paint_rect_contains(clip_area, bounds));
                    if needs_rectangular_clip {
                        let had_svg_clip = path.clip.is_some();
                        let clip = path.clip.get_or_insert_with(|| {
                            RenderedPathClip::new(
                                paint_rect_path_commands(clip_area),
                                RenderedPathFillRule::NonZero,
                                Vec::new(),
                            )
                        });
                        if had_svg_clip {
                            clip.additional_clips.push(RenderedPathClipPath::new(
                                paint_rect_path_commands(clip_area),
                                RenderedPathFillRule::NonZero,
                            ));
                        }
                    }
                    if let Some(rounded_clip) = &tile.rounded_clip {
                        let clip = path.clip.get_or_insert_with(|| rounded_clip.clone());
                        // A rounded background clip intersects (rather than
                        // replaces) an SVG root or CSS rectangular clip.
                        // The direct clone above is already the only clip.
                        if !clip.commands.eq(&rounded_clip.commands)
                            || clip.fill_rule != rounded_clip.fill_rule
                        {
                            clip.additional_clips.push(RenderedPathClipPath::new(
                                rounded_clip.commands.clone(),
                                rounded_clip.fill_rule,
                            ));
                        }
                    }
                    primitives.push(PaintPrimitive::Path(path));
                }
            }
            continue;
        }
        let Some(selected_image) = layer.image.as_image().map(BackgroundImage::selected_image)
        else {
            continue;
        };
        let generated_image = matches!(
            selected_image,
            BackgroundImage::LinearGradient(_)
                | BackgroundImage::RadialGradient(_)
                | BackgroundImage::ConicGradient(_)
                | BackgroundImage::CssColor(_)
        );
        // Generated CSS images have no intrinsic dimensions, so their used
        // background size is known before a raster recipe exists. This is the
        // key ordering required by CSS Backgrounds: raster fallbacks are made
        // at the used tile size, never at the positioning-area size.
        let decoded_for_size = (!generated_image)
            .then(|| {
                background_layer_decoded_image(
                    layer,
                    PaintSize::new(positioning_area.width(), positioning_area.height()),
                    fallback_base_url,
                    fallback_root_url,
                    resource_cache,
                    style.image_orientation == css::ImageOrientation::FromImage,
                    style.color,
                )
            })
            .flatten();
        let image_size = if generated_image {
            used_generated_background_layer_size(layer, positioning_area.size())
        } else {
            let Some(decoded) = decoded_for_size.as_ref() else {
                continue;
            };
            used_background_layer_size(decoded, layer, positioning_area.size())
        };
        if image_size.width <= 0.0 || image_size.height <= 0.0 {
            continue;
        }
        let tile = ResolvedBackgroundTile::new(
            positioning_area,
            clip_area,
            rounded_clip,
            layer,
            image_size,
        );
        // The box-decoration painter already emits this subset as exact
        // vector bands. Keeping it out of the generic image path prevents a
        // second paint of the same CSS layer.
        if box_decoration_paints_vector_gradients
            && matches!(selected_image, BackgroundImage::LinearGradient(gradient)
            if crate::layout::paint_helpers::linear_gradient_is_painted_by_box_decoration(
                gradient, layer, tile.size,
            ))
        {
            continue;
        }
        if let Some(color) = color_image {
            append_color_image_primitives(&mut primitives, color, &tile);
            continue;
        }
        if let Some(color) = uniform_gradient_color(selected_image, style.color) {
            append_color_image_primitives(&mut primitives, color, &tile);
            continue;
        }
        if let BackgroundImage::LinearGradient(gradient) = selected_image
            && !gradient
                .stops
                .iter()
                .any(|stop| stop.color.is_current_color())
        {
            let mut hard_stop_paths = Vec::new();
            let mut all_tiles_are_vector_hard_stops = true;
            for tile_area in tile.tiles() {
                let Some(clip) = tile_area.intersect(tile.clip_area) else {
                    continue;
                };
                let Some(paths) =
                    crate::layout::paint_helpers::linear_gradient_hard_stop_tile_paths(
                        gradient,
                        tile_area.paint_rect(),
                        clip.paint_rect(),
                        tile.rounded_clip.clone(),
                    )
                else {
                    all_tiles_are_vector_hard_stops = false;
                    break;
                };
                hard_stop_paths.extend(paths.into_iter().map(PaintPrimitive::Path));
            }
            if all_tiles_are_vector_hard_stops {
                primitives.extend(hard_stop_paths);
                continue;
            }
        }
        if append_native_css_gradient_primitives(
            &mut primitives,
            selected_image,
            &tile,
            style.color,
        ) {
            continue;
        }
        let Some(decoded) = decoded_for_size.or_else(|| {
            background_layer_decoded_image(
                layer,
                tile.size,
                fallback_base_url,
                fallback_root_url,
                resource_cache,
                style.image_orientation == css::ImageOrientation::FromImage,
                style.color,
            )
        }) else {
            continue;
        };
        if (generated_image || style.display.is_contents())
            && let Some(color) = opaque_uniform_raster_color(&decoded, resource_cache)
        {
            // A fully opaque uniform raster has no image-local state after
            // the CSS image has been decoded. Paint it through the same
            // vector path as `image(<color>)`: this preserves repetition and
            // `background-clip` while avoiding PDF pattern-edge coverage
            // differences from its equivalent CSS color.
            append_color_image_primitives(&mut primitives, color, &tile);
            continue;
        }
        // A repeated URL image is semantically a single image cell tiled
        // across its positioning area. Spatially constant generated images
        // have that same property, so they can use the pattern path too. In
        // particular, a tiny repeated solid-color gradient must not turn into
        // one PDF image placement per CSS tile.
        //
        // Non-constant gradients retain the existing individual-placement
        // path. PDF pattern image interpolation is not equivalent to the
        // generated-image raster path for those gradients at every scale.
        let can_emit_pattern = use_pdf_patterns_for_repeated_images
            && layer
                .image
                .as_image()
                .is_some_and(background_image_can_use_pdf_pattern)
            && (layer.repeat.repeats_x() || layer.repeat.repeats_y());
        if can_emit_pattern {
            // Repetition fills the background painting area, not only the
            // positioning area. This matters for a propagated root/body
            // background, whose images are positioned on the root box while
            // the canvas is its painting area.
            // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
            let pattern_area = tile.clip_area;
            if pattern_area.width() <= 0.0 || pattern_area.height() <= 0.0 {
                continue;
            }
            let origin_x = background_first_tile_position(
                (f64::from(tile.positioning_area.x()) + tile.offset.x) as f32,
                tile.positioning_area.x(),
                tile.positioning_area.width(),
                tile.size.width,
                tile.repeat.x_axis(),
            );
            let origin_y = background_first_tile_position(
                (f64::from(tile.positioning_area.y()) + tile.offset.y) as f32,
                tile.positioning_area.y(),
                tile.positioning_area.height(),
                tile.size.height,
                tile.repeat.y_axis(),
            );
            let step_width = background_pattern_step(
                tile.size.width,
                tile.positioning_area.width(),
                tile.repeat.x_axis(),
            );
            let step_height = background_pattern_step(
                tile.size.height,
                tile.positioning_area.height(),
                tile.repeat.y_axis(),
            );
            if step_width <= 0.0 || step_height <= 0.0 {
                continue;
            }
            let mut pattern = RenderedImagePattern::from_paint_rect(
                pattern_area.paint_rect(),
                true,
                PaintPatternTiling::new(
                    tile.size,
                    PaintSize::new(step_width, step_height),
                    PaintPoint::new(origin_x, origin_y),
                ),
                decoded.pixel_width,
                decoded.pixel_height,
                raster_image_interpolation(style),
                decoded.rgb.shared(),
                decoded.alpha.clone(),
            )
            .with_raster_color_space(decoded.color_space.clone())
            .with_image_id(decoded.image_id);
            if let Some(clip) = tile.rounded_clip.clone() {
                pattern = pattern.with_clip(clip);
            }
            primitives.push(PaintPrimitive::ImagePattern(pattern));
            continue;
        }
        for tile_area in tile.tiles() {
            let image = RenderedImage::from_paint_rect(
                tile_area.paint_rect(),
                true,
                decoded.pixel_width,
                decoded.pixel_height,
                None,
                raster_image_interpolation(style),
                decoded.rgb.shared(),
                decoded.alpha.clone(),
                None,
            )
            .with_raster_color_space(decoded.color_space.clone())
            .with_image_id(decoded.image_id);
            if let Some(image) = clip_background_image_to_paint_area(
                image,
                tile.clip_area,
                tile.rounded_clip.clone(),
            ) {
                primitives.push(PaintPrimitive::Image(image));
            }
        }
    }
    primitives
}

/// Select the PDF sampling hint for a raster CSS background image.
///
/// CSS Images leaves `image-rendering: auto`'s concrete algorithm to the user
/// agent. Quire's normal replaced-image painter emits a non-interpolating PDF
/// image for that default; background images use the same policy so one source
/// has identical sampling at the same used CSS size. `pixelated` likewise
/// forbids interpolation.
/// <https://drafts.csswg.org/css-images-4/#propdef-image-rendering>
pub(in crate::layout) fn raster_image_interpolation(_style: &ComputedStyle) -> bool {
    false
}

fn used_svg_background_layer_size(
    asset: &SharedSvgAsset,
    layer: &css::BackgroundLayer,
    positioning_area: PaintSize,
) -> PaintSize {
    if asset.has_degenerate_view_box() {
        return PaintSize::new(0.0, 0.0);
    }
    let intrinsic = asset.intrinsic_dimensions();
    let mut size = used_background_size_from_intrinsic_dimensions(
        positioning_area,
        layer.size.clone(),
        CssImageNaturalDimensions::from_layout_axes(
            intrinsic.width,
            intrinsic.height,
            intrinsic.aspect_ratio,
        ),
    );
    if layer.repeat.x_axis() == css::BackgroundRepeatAxis::Round {
        size.width = rounded_background_tile_size(size.width, positioning_area.width);
        if matches!(
            layer.size,
            css::BackgroundSize::Auto
                | css::BackgroundSize::Explicit {
                    height: css::BackgroundSizeAxis::Auto,
                    ..
                }
        ) && let Some(ratio) = intrinsic.aspect_ratio.filter(|ratio| *ratio > 0.0)
        {
            size.height = size.width / ratio;
        }
    }
    if layer.repeat.y_axis() == css::BackgroundRepeatAxis::Round {
        size.height = rounded_background_tile_size(size.height, positioning_area.height);
        if matches!(
            layer.size,
            css::BackgroundSize::Auto
                | css::BackgroundSize::Explicit {
                    width: css::BackgroundSizeAxis::Auto,
                    ..
                }
        ) && let Some(ratio) = intrinsic.aspect_ratio.filter(|ratio| *ratio > 0.0)
        {
            size.width = size.height * ratio;
        }
    }
    size
}

/// Paint an `image(<color>)` layer after the normal background geometry has
/// resolved its used size, position, repeat, and clip. A color image is
/// spatially uniform, so a vector fill is an exact representation and avoids
/// manufacturing a raster image resource solely to paint one color.
/// <https://drafts.csswg.org/css-images-4/#image-notation>
fn append_color_image_primitives(
    primitives: &mut Vec<PaintPrimitive>,
    color: CssColor,
    resolved: &ResolvedBackgroundTile,
) {
    let x_tiles = color_image_axis_tiles(
        resolved.positioning_area.x(),
        resolved.offset.x,
        resolved.size.width,
        resolved.repeat.x_axis(),
        resolved.tile_xs(),
        resolved.clip_area.x(),
        resolved.clip_area.width(),
    );
    let y_tiles = color_image_axis_tiles(
        resolved.positioning_area.y(),
        resolved.offset.y,
        resolved.size.height,
        resolved.repeat.y_axis(),
        resolved.tile_ys(),
        resolved.clip_area.y(),
        resolved.clip_area.height(),
    );
    // A constant image has no tile-local state. Along each continuously
    // repeated axis its painted coverage is the whole positioning area, so
    // coalesce tiles before clipping instead of expanding tiny CSS tiles into
    // page-content paths.
    for (y, height) in &y_tiles {
        for (x, width) in &x_tiles {
            let Some((x, width)) = intersect_background_axis(
                *x,
                *width,
                resolved.clip_area.x(),
                resolved.clip_area.width(),
            ) else {
                continue;
            };
            let Some((y, height)) = intersect_background_axis(
                *y,
                *height,
                resolved.clip_area.y(),
                resolved.clip_area.height(),
            ) else {
                continue;
            };
            primitives.push(uniform_background_rect_primitive(
                PaintBackgroundArea::new(PaintPoint::new(x, y), PaintSize::new(width, height)),
                color,
                resolved.rounded_clip.clone(),
            ));
        }
    }
}

/// Emit a spatially uniform background area with PDF's native rectangle
/// operator when no curved CSS clip is needed.  A rounded clip still requires
/// a general path clipping scope.
fn uniform_background_rect_primitive(
    area: PaintBackgroundArea,
    color: CssColor,
    rounded_clip: Option<RenderedPathClip>,
) -> PaintPrimitive {
    if rounded_clip.is_none() {
        PaintPrimitive::Rect(RenderedRect::from_paint_rect(
            area.paint_rect(),
            Some(color),
        ))
    } else {
        PaintPrimitive::Path(RenderedPath::new(
            paint_rect_path_commands(area.paint_rect()),
            Some(color),
            RenderedPathFillRule::NonZero,
            None,
            PaintStrokeWidth::ZERO,
            rounded_clip,
        ))
    }
}

/// Return a CSS color only when decoded raster samples are exactly one opaque
/// built-in CSS RGB color.
///
/// An embedded ICC profile remains an image resource because it has no
/// equivalent page graphics color-space object at this layout boundary.
/// <https://www.w3.org/TR/css-color-4/#predefined>.
pub(in crate::layout) fn opaque_uniform_raster_color(
    decoded: &DecodedPngImage,
    resource_cache: &ResourceCache,
) -> Option<CssColor> {
    if let Some(image_id) = decoded.image_id {
        return resource_cache
            .with_rasterized_image(image_id, |raster| {
                opaque_uniform_raster_samples_color(
                    raster.metadata.pixel_width,
                    raster.metadata.pixel_height,
                    &raster.rgb,
                    raster.alpha.as_deref(),
                    &raster.color_space,
                )
            })
            .flatten();
    }
    opaque_uniform_raster_samples_color(
        decoded.pixel_width,
        decoded.pixel_height,
        &decoded.rgb,
        decoded.alpha.as_deref(),
        &decoded.color_space,
    )
}

fn opaque_uniform_raster_samples_color(
    pixel_width: u32,
    pixel_height: u32,
    rgb: &[u8],
    alpha: Option<&[u8]>,
    color_space: &crate::color::RasterColorSpace,
) -> Option<CssColor> {
    let crate::color::RasterColorSpace::BuiltIn(color_space) = color_space else {
        return None;
    };
    let pixel_count = (pixel_width as usize).checked_mul(pixel_height as usize)?;
    if pixel_count == 0 || rgb.len() != pixel_count.checked_mul(3)? {
        return None;
    }
    if alpha.is_some_and(|alpha| {
        alpha.len() != pixel_count || alpha.iter().any(|sample| *sample != 255)
    }) {
        return None;
    }
    let first = [rgb[0], rgb[1], rgb[2]];
    rgb.as_chunks::<3>()
        .0
        .iter()
        .all(|sample| sample == &first)
        .then(|| {
            CssColor::in_space(
                *color_space,
                first[0] as f32 / 255.0,
                first[1] as f32 / 255.0,
                first[2] as f32 / 255.0,
                1.0,
            )
        })
}

/// Return the tiles of one uniform-image axis in sufficiently precise page
/// coordinates to clip an extremely large non-repeating image.
pub(in crate::layout) fn color_image_axis_tiles(
    area_start: f32,
    offset: f64,
    tile_size: f32,
    repeat: css::BackgroundRepeatAxis,
    ordinary_positions: Vec<f32>,
    clip_start: f32,
    clip_size: f32,
) -> Vec<(f64, f64)> {
    match repeat {
        css::BackgroundRepeatAxis::Repeat | css::BackgroundRepeatAxis::Round => {
            // A spatially uniform repeated image has no tile-local state: its
            // union covers every point on the repeated axis that survives
            // background clipping, including the part of a clip area outside
            // the positioning area.
            vec![(f64::from(clip_start), f64::from(clip_size))]
        }
        css::BackgroundRepeatAxis::NoRepeat => {
            vec![(f64::from(area_start) + offset, f64::from(tile_size))]
        }
        css::BackgroundRepeatAxis::Space => ordinary_positions
            .into_iter()
            .map(|position| (f64::from(position), f64::from(tile_size)))
            .collect(),
    }
}

/// Intersect a tile and a clip interval before reducing the visible result to
/// the renderer's f32 paint coordinates.
fn intersect_background_axis(
    tile_start: f64,
    tile_size: f64,
    clip_start: f32,
    clip_size: f32,
) -> Option<(f32, f32)> {
    let (start, end) = intersect_background_axis_precise(
        tile_start,
        tile_size,
        f64::from(clip_start),
        f64::from(clip_size),
    )?;
    (end > start).then_some((start as f32, (end - start) as f32))
}

fn intersect_background_axis_precise(
    tile_start: f64,
    tile_size: f64,
    clip_start: f64,
    clip_size: f64,
) -> Option<(f64, f64)> {
    let start = tile_start.max(clip_start);
    let end = (tile_start + tile_size).min(clip_start + clip_size);
    (end > start).then_some((start, end))
}

/// Resolve the visible part of a non-repeating SVG tile and the matching SVG
/// source rectangle without first collapsing an enormous tile to f32 page
/// coordinates.
fn non_repeating_svg_visible_area(
    asset: &SharedSvgAsset,
    tile: &ResolvedBackgroundTile,
) -> Option<(PaintBackgroundArea, crate::svg::SvgSourceRect)> {
    if !matches!(tile.repeat.x_axis(), css::BackgroundRepeatAxis::NoRepeat)
        || !matches!(tile.repeat.y_axis(), css::BackgroundRepeatAxis::NoRepeat)
        || tile.size.width <= 0.0
        || tile.size.height <= 0.0
    {
        return None;
    }
    let tile_x = f64::from(tile.positioning_area.x()) + tile.offset.x;
    let tile_y = f64::from(tile.positioning_area.y()) + tile.offset.y;
    let (x1, x2) = intersect_background_axis_precise(
        tile_x,
        f64::from(tile.size.width),
        f64::from(tile.clip_area.x()),
        f64::from(tile.clip_area.width()),
    )?;
    let (y1, y2) = intersect_background_axis_precise(
        tile_y,
        f64::from(tile.size.height),
        f64::from(tile.clip_area.y()),
        f64::from(tile.clip_area.height()),
    )?;
    let source_size = asset.source_viewport_size();
    let source_x =
        ((x1 - tile_x) / f64::from(tile.size.width) * f64::from(source_size.width)) as f32;
    let source_y = ((tile_y + f64::from(tile.size.height) - y2) / f64::from(tile.size.height)
        * f64::from(source_size.height)) as f32;
    let source_width =
        ((x2 - x1) / f64::from(tile.size.width) * f64::from(source_size.width)) as f32;
    let source_height =
        ((y2 - y1) / f64::from(tile.size.height) * f64::from(source_size.height)) as f32;
    Some((
        PaintBackgroundArea::new(
            PaintPoint::new(x1 as f32, y1 as f32),
            PaintSize::new((x2 - x1) as f32, (y2 - y1) as f32),
        ),
        crate::svg::SvgSourceRect::new(
            crate::svg::SvgSourcePoint::new(source_x, source_y),
            crate::svg::SvgSourceSize::new(source_width, source_height),
        ),
    ))
}

/// Returns the exact paint for a spatially uniform generated gradient.
///
/// CssColor-stop fixup changes positions but not stop colors, so identical source
/// colors are uniform irrespective of omitted positions, hints, or direction.
/// <https://www.w3.org/TR/css-images-3/#coloring-gradient-line>
fn uniform_gradient_color(image: &BackgroundImage, current_color: CssColor) -> Option<CssColor> {
    let (stops, interpolation) = match image {
        BackgroundImage::LinearGradient(gradient) => (
            gradient
                .stops
                .iter()
                .map(|stop| stop.color)
                .collect::<Vec<_>>(),
            gradient.interpolation,
        ),
        BackgroundImage::RadialGradient(gradient) => (
            gradient
                .stops
                .iter()
                .map(|stop| stop.color)
                .collect::<Vec<_>>(),
            gradient.interpolation,
        ),
        BackgroundImage::ConicGradient(gradient) => (
            gradient
                .stops
                .iter()
                .map(|stop| stop.color)
                .collect::<Vec<_>>(),
            gradient.interpolation,
        ),
        BackgroundImage::ImageSet(_)
        | BackgroundImage::SelectedImageSet { .. }
        | BackgroundImage::Url { .. }
        | BackgroundImage::CssColor(_) => {
            return None;
        }
    };
    uniform_gradient_stop_color(&stops, interpolation, current_color)
}

/// Determines whether a color line is constant after CSS CssColor's
/// missing-component and analogous-component fixup. Comparing specified stop
/// colors alone is insufficient: `rgb(none 255 none), yellow` is a uniform
/// yellow gradient even though its computed stop coordinates differ.
pub(in crate::layout) fn uniform_gradient_stop_color(
    stops: &[css::GradientColor],
    interpolation: css::GradientInterpolationMethod,
    current_color: CssColor,
) -> Option<CssColor> {
    let first = *stops.first()?;
    if stops.len() == 1 {
        return Some(first.resolve(current_color));
    }
    let first_color = first.resolve(current_color);
    if stops.iter().all(|stop| {
        stop.missing_components_for(interpolation).is_empty()
            && stop.resolve(current_color) == first_color
    }) {
        return Some(first_color);
    }
    let mut uniform = None;
    for pair in stops.windows(2) {
        let start = pair[0];
        let end = pair[1];
        let start_color = crate::color::interpolate_color_with_missing(
            start.resolve(current_color),
            end.resolve(current_color),
            interpolation,
            0.0,
            start.missing_components_for(interpolation).bits(),
            end.missing_components_for(interpolation).bits(),
        );
        let end_color = crate::color::interpolate_color_with_missing(
            start.resolve(current_color),
            end.resolve(current_color),
            interpolation,
            1.0,
            start.missing_components_for(interpolation).bits(),
            end.missing_components_for(interpolation).bits(),
        );
        if start_color != end_color || uniform.is_some_and(|color| color != start_color) {
            return None;
        }
        uniform = Some(start_color);
    }
    uniform
}

/// Emits CSS linear and radial gradients as PDF shading patterns. Repeating
/// color lines are expanded only over the finite painted gradient domain;
/// outer CSS background repetition remains a native PDF tiling pattern.
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>
/// <https://www.w3.org/TR/css-images-3/#radial-gradients>
fn background_image_can_use_pdf_pattern(image: &BackgroundImage) -> bool {
    let stops = match image.selected_image() {
        BackgroundImage::ImageSet(_) | BackgroundImage::SelectedImageSet { .. } => {
            unreachable!("selected image-set source is unwrapped")
        }
        BackgroundImage::Url { .. } => return true,
        BackgroundImage::LinearGradient(gradient) => &gradient.stops,
        BackgroundImage::RadialGradient(gradient) => &gradient.stops,
        BackgroundImage::ConicGradient(_) => return false,
        BackgroundImage::CssColor(_) => return true,
    };
    stops
        .first()
        .is_some_and(|first| stops.iter().all(|stop| stop.color == first.color))
}

fn background_layer_decoded_image(
    layer: &css::BackgroundLayer,
    size: PaintSize,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    apply_orientation: bool,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    match layer.image.as_image()?.selected_image() {
        BackgroundImage::ImageSet(_) | BackgroundImage::SelectedImageSet { .. } => {
            unreachable!("selected image-set source is unwrapped")
        }
        BackgroundImage::Url {
            src,
            base_url,
            root_url,
            request_modifiers,
        } => load_image_source_with_request(
            src.as_str(),
            base_url.as_ref().or(fallback_base_url),
            root_url.as_ref().or(fallback_root_url),
            resource_cache,
            apply_orientation,
            request_modifiers,
        ),
        BackgroundImage::LinearGradient(gradient) => {
            generated_linear_gradient_image(gradient, size, resource_cache, current_color)
        }
        BackgroundImage::RadialGradient(gradient) => {
            generated_radial_gradient_image(gradient, size, resource_cache, current_color)
        }
        BackgroundImage::ConicGradient(gradient) => {
            rasterize_conic_gradient(gradient, size, current_color)
        }
        BackgroundImage::CssColor(_) => None,
    }
}

pub(in crate::layout) fn rasterize_generated_css_image(
    image: &BackgroundImage,
    size: PaintSize,
    current_color: CssColor,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Option<DecodedPngImage> {
    match image.selected_image() {
        BackgroundImage::ImageSet(_) | BackgroundImage::SelectedImageSet { .. } => {
            unreachable!("selected image-set source is unwrapped")
        }
        BackgroundImage::Url {
            src,
            base_url,
            root_url,
            request_modifiers,
        } => load_image_source_with_request(
            src.as_str(),
            base_url.as_ref().or(fallback_base_url),
            root_url.as_ref().or(fallback_root_url),
            resource_cache,
            true,
            request_modifiers,
        ),
        BackgroundImage::LinearGradient(gradient) => {
            generated_linear_gradient_image(gradient, size, resource_cache, current_color)
        }
        BackgroundImage::RadialGradient(gradient) => {
            generated_radial_gradient_image(gradient, size, resource_cache, current_color)
        }
        BackgroundImage::ConicGradient(gradient) => {
            rasterize_conic_gradient(gradient, size, current_color)
        }
        BackgroundImage::CssColor(color) => Some(solid_color_image(color.resolve(current_color))),
    }
}

fn solid_color_image(color: CssColor) -> DecodedPngImage {
    // Generated PNGs are an explicit sRGB encoding boundary.
    let color = crate::css::color_to_predefined_rgb(color, crate::css::CssColorSpace::Srgb)
        .expect("sRGB is a predefined CSS RGB space");
    DecodedPngImage::new(
        1,
        1,
        vec![
            (color.components()[0] * 255.0).round().clamp(0.0, 255.0) as u8,
            (color.components()[1] * 255.0).round().clamp(0.0, 255.0) as u8,
            (color.components()[2] * 255.0).round().clamp(0.0, 255.0) as u8,
        ],
        (color.alpha() < 1.0)
            .then_some(vec![(color.alpha() * 255.0).round().clamp(0.0, 255.0) as u8]),
    )
}
