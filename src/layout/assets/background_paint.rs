use super::*;
use crate::document::paint::patterns::PaintPatternTiling;
use std::rc::Rc;

/// Resolved CSS background-image primitives plus their decoration-phase
/// eligibility.
///
/// CSS Backgrounds paints images below borders. An opaque square normal
/// border completely hides its border-area background, so normal box painting
/// may first resolve that hidden area away. A finite no-repeat image then has
/// only a zero-area boundary in common with the border, and PDF antialiasing
/// can deterministically assign that edge to the image phase.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-clip>
#[derive(Debug)]
pub(in crate::layout) struct ResolvedBackgroundImagePaint {
    pub(in crate::layout) primitives: Vec<PaintPrimitive>,
    pub(in crate::layout) border_disjoint_tile_geometry: bool,
}

#[derive(Debug)]
struct BackgroundImagePhaseEligibility {
    has_image_layer: bool,
    every_image_layer_is_border_disjoint: bool,
}

impl Default for BackgroundImagePhaseEligibility {
    fn default() -> Self {
        Self {
            has_image_layer: false,
            every_image_layer_is_border_disjoint: true,
        }
    }
}

impl BackgroundImagePhaseEligibility {
    fn note_tile(
        &mut self,
        padding_clip: Option<PaintBackgroundArea>,
        tile: &ResolvedBackgroundTile,
    ) {
        self.has_image_layer = true;
        self.every_image_layer_is_border_disjoint &=
            padding_clip.is_some_and(|padding| finite_no_repeat_tile_is_inside(tile, padding));
    }

    fn disqualify(&mut self) {
        self.has_image_layer = true;
        self.every_image_layer_is_border_disjoint = false;
    }

    fn finish(self) -> bool {
        self.has_image_layer && self.every_image_layer_is_border_disjoint
    }
}

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
    background_image_paint_for_style_with_paint_areas_and_fixed_positioning_area(
        area,
        area,
        None,
        style.has_transform(),
        style,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
    )
    .primitives
}

pub(in crate::layout) fn background_image_primitives_for_style_with_paint_areas(
    positioning_border_area: PaintBackgroundArea,
    clip_border_area: PaintBackgroundArea,
    style: &ComputedStyle,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    background_image_paint_for_style_with_paint_areas_and_fixed_positioning_area(
        positioning_border_area,
        clip_border_area,
        None,
        style.has_transform(),
        style,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
    )
    .primitives
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
        false,
    )
    .primitives
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
    .primitives
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
    background_image_paint_for_style_with_paint_areas_and_fixed_positioning_area(
        positioning_border_area,
        clip_border_area,
        fixed_positioning_area,
        fixed_attachment_is_scrolled_by_transform,
        style,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
    )
    .primitives
}

/// Resolve CSS image primitives and retain the physical tile relationship
/// needed by normal-box decoration phases.
#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn background_image_paint_for_style_with_paint_areas_and_fixed_positioning_area(
    positioning_border_area: PaintBackgroundArea,
    clip_border_area: PaintBackgroundArea,
    fixed_positioning_area: Option<PaintBackgroundArea>,
    fixed_attachment_is_scrolled_by_transform: bool,
    style: &ComputedStyle,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> ResolvedBackgroundImagePaint {
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

#[allow(clippy::too_many_arguments)]
fn background_image_primitives_for_style_impl(
    paint_areas: BackgroundPaintAreas<PaintSpace>,
    style: &ComputedStyle,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    use_pdf_patterns_for_repeated_images: bool,
    box_decoration_paints_vector_gradients: bool,
    collect_border_disjoint_phase_eligibility: bool,
) -> ResolvedBackgroundImagePaint {
    let BackgroundPaintAreas {
        positioning_border_area,
        clip_border_area,
        fixed_positioning_area,
        fixed_attachment_is_scrolled_by_transform,
    } = paint_areas;
    let mut primitives = Vec::new();
    let mut phase_eligibility = BackgroundImagePhaseEligibility::default();
    for layer in background_layers_for_paint(style).iter().rev() {
        let positioning_area = background_positioning_area_for_layer(
            positioning_border_area,
            fixed_positioning_area,
            fixed_attachment_is_scrolled_by_transform,
            style,
            layer,
        );
        // This remains the CSS painting area.  The padding-box rectangle is
        // consulted separately below as a PDF phase-eligibility probe; it
        // must never substitute for `background-clip: border-box`.
        let resolved_clip = layer.clip;
        let clip_area = background_paint_area_for_box(clip_border_area, style, resolved_clip);
        let rounded_clip = rounded_background_clip_for_box(
            paint_space_rect(
                clip_border_area.x(),
                clip_border_area.y(),
                clip_border_area.width(),
                clip_border_area.height(),
            ),
            style,
            used_border_widths(style),
            resolved_clip,
        );
        let padding_clip_for_phase = (collect_border_disjoint_phase_eligibility
            && layer.clip == css::BackgroundBox::Border
            && has_opaque_square_normal_border(style))
        .then(|| {
            background_paint_area_for_box(clip_border_area, style, css::BackgroundBox::Padding)
        });
        let selected_for_source = layer.image.as_image().map(BackgroundImage::selected_image);
        let image_function_source =
            matches!(selected_for_source, Some(BackgroundImage::ImageFunction(_))).then(|| {
                resolve_css_image_source(
                    selected_for_source.expect("selected image is present"),
                    ImageResolutionContext {
                        base_url: fallback_base_url,
                        root_url: fallback_root_url,
                        current_color: style.color,
                        orientation: crate::layout::asset_helpers::raster_orientation_policy(
                            style.image_orientation,
                        ),
                        svg_context: crate::svg::SvgImageContext::from_used_color_scheme(
                            style.used_color_scheme,
                        ),
                        resource_cache,
                    },
                )
            });
        let color_image = match selected_for_source {
            Some(BackgroundImage::CssColor(color)) => Some(color.resolve(style.color)),
            _ => match image_function_source.as_ref() {
                Some(ResolvedCssImage::SolidColor(color)) => Some(*color),
                _ => None,
            },
        };
        let svg_asset = match selected_for_source {
            Some(BackgroundImage::Url(url)) => load_resolved_image_source_with_request(
                &url.href,
                url.base_url.as_ref().or(fallback_base_url),
                url.root_url.as_ref().or(fallback_root_url),
                resource_cache,
                crate::layout::asset_helpers::raster_orientation_policy(style.image_orientation),
                crate::svg::SvgImageContext::from_used_color_scheme(style.used_color_scheme),
                &url.request_modifiers,
            ),
            _ => match image_function_source.as_ref() {
                Some(ResolvedCssImage::External(asset)) => Some(asset.clone()),
                _ => None,
            },
        };
        if let Some(ResolvedImageAsset::Svg(asset)) = svg_asset {
            let image_size = used_svg_background_layer_size(&asset, layer, positioning_area.size());
            if image_size.width <= 0.0 || image_size.height <= 0.0 {
                phase_eligibility.disqualify();
                continue;
            }
            let asset = Rc::new(asset.with_css_image_viewport(image_size));
            let tile = ResolvedBackgroundTile::new(
                positioning_area,
                clip_area,
                rounded_clip,
                layer,
                image_size,
            );
            // A finite tile wholly inside the padding box has no CSS-visible
            // overlap with the border.  It uses the established post-border
            // replay path; backing is exclusively for cover tiles that still
            // need a continuation beneath the border clip.
            let border_disjoint_tile = padding_clip_for_phase
                .is_some_and(|padding| finite_no_repeat_tile_is_inside(&tile, padding));
            phase_eligibility.note_tile(padding_clip_for_phase, &tile);
            if let Some(slice) = non_repeating_svg_visible_area(&asset, &tile) {
                let viewport_fill = asset.opaque_viewport_fill();
                if !border_disjoint_tile
                    && let Some(backing) = PdfOpaqueBorderBacking::new(style, layer, &tile, slice)
                {
                    // Backing belongs below the normal border, so this layer
                    // cannot take the border-disjoint post-border replay.
                    phase_eligibility.disqualify();
                    append_svg_border_backing_primitives(
                        &mut primitives,
                        &asset,
                        viewport_fill,
                        backing,
                        tile.rounded_clip.as_ref(),
                    );
                }
                if let Some(color) =
                    viewport_fill.or_else(|| asset.opaque_source_rect_fill(slice.source))
                {
                    // A spatially uniform visible image has the same PDF
                    // edge coverage as the normal CSS background-color
                    // reference path. Keep it in the ordinary under-border
                    // phase; only non-uniform finite images need the
                    // deterministic padding-edge replay.
                    phase_eligibility.disqualify();
                    primitives.push(uniform_background_rect_primitive(
                        slice.destination_area,
                        color,
                        tile.rounded_clip.clone(),
                    ));
                } else {
                    // A non-repeating SVG tile has one finite visible area.
                    // Crop its root source viewport before producing PDF paths
                    // instead of clipping the full tile with a second PDF
                    // rectangle. That preserves CSS background-clip while
                    // avoiding an antialiased seam at the crop edge.
                    for mut path in asset.paint_paths_for_source_rect(
                        slice.destination_area.paint_rect(),
                        slice.source,
                    ) {
                        append_rounded_background_clip(&mut path, tile.rounded_clip.as_ref());
                        primitives.push(PaintPrimitive::Path(path));
                    }
                }
                continue;
            }
            if let Some(color) = asset.opaque_viewport_fill() {
                // A uniformly opaque SVG is geometrically equivalent to a
                // `image(<color>)` layer. Reuse the color-image painter so
                // repeat coalescing and CSS clipping remain identical while
                // avoiding unbounded vector coordinates from extreme SVG
                // viewBoxes.
                phase_eligibility.disqualify();
                append_color_image_primitives(&mut primitives, color, &tile);
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
                    append_rounded_background_clip(&mut path, tile.rounded_clip.as_ref());
                    primitives.push(PaintPrimitive::Path(path));
                }
            }
            continue;
        }
        let Some(selected_image) = layer.image.as_image().map(BackgroundImage::selected_image)
        else {
            continue;
        };
        let generated_image = color_image.is_some()
            || matches!(
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
                    crate::layout::asset_helpers::raster_orientation_policy(
                        style.image_orientation,
                    ),
                    style.color,
                )
            })
            .flatten();
        let image_size = if generated_image {
            used_generated_background_layer_size(layer, positioning_area.size())
        } else {
            let Some(decoded) = decoded_for_size.as_ref() else {
                phase_eligibility.disqualify();
                continue;
            };
            used_background_layer_size(decoded, layer, positioning_area.size())
        };
        if image_size.width <= 0.0 || image_size.height <= 0.0 {
            phase_eligibility.disqualify();
            continue;
        }
        let tile = ResolvedBackgroundTile::new(
            positioning_area,
            clip_area,
            rounded_clip,
            layer,
            image_size,
        );
        phase_eligibility.note_tile(padding_clip_for_phase, &tile);
        // The box-decoration painter already emits this subset as exact
        // vector bands. Keeping it out of the generic image path prevents a
        // second paint of the same CSS layer.
        if box_decoration_paints_vector_gradients
            && matches!(selected_image, BackgroundImage::LinearGradient(gradient)
            if crate::layout::paint_helpers::linear_gradient_is_painted_by_box_decoration(
                gradient, layer, tile.size,
            ))
        {
            phase_eligibility.disqualify();
            continue;
        }
        if let Some(color) = color_image {
            phase_eligibility.disqualify();
            append_color_image_primitives(&mut primitives, color, &tile);
            continue;
        }
        if let Some(color) = uniform_gradient_color(selected_image, style.color) {
            phase_eligibility.disqualify();
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
                crate::layout::asset_helpers::raster_orientation_policy(style.image_orientation),
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
            phase_eligibility.disqualify();
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
                decoded.pixel_size.width,
                decoded.pixel_size.height,
                raster_image_sampling(style),
                decoded.rgb.shared(),
                decoded.alpha.clone(),
            )
            .with_raster_color_space(decoded.color_space.clone())
            .with_source_rect(decoded.source_rect)
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
                decoded.pixel_size.width,
                decoded.pixel_size.height,
                decoded.source_rect,
                raster_image_sampling(style),
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
    ResolvedBackgroundImagePaint {
        primitives,
        border_disjoint_tile_geometry: phase_eligibility.finish(),
    }
}

/// Whether an image has one finite no-repeat tile. A later opaque-border pass
/// may resolve its hidden border area away; only non-uniform finite tiles need
/// that second resolved geometry for deterministic PDF edge coverage.
fn finite_no_repeat_tile_is_inside(
    tile: &ResolvedBackgroundTile,
    padding: PaintBackgroundArea,
) -> bool {
    if !matches!(tile.repeat.x_axis(), css::BackgroundRepeatAxis::NoRepeat)
        || !matches!(tile.repeat.y_axis(), css::BackgroundRepeatAxis::NoRepeat)
    {
        return false;
    }
    let tile_x = f64::from(tile.positioning_area.x()) + tile.offset.x;
    let tile_y = f64::from(tile.positioning_area.y()) + tile.offset.y;
    tile_x >= f64::from(padding.x())
        && tile_y >= f64::from(padding.y())
        && tile_x + f64::from(tile.size.width) <= f64::from(padding.x() + padding.width())
        && tile_y + f64::from(tile.size.height) <= f64::from(padding.y() + padding.height())
}

/// Retain the selected CSS raster-sampling behavior until PDF preparation.
///
/// CSS Images applies `image-rendering` to decorative images as well as
/// replaced content. The final sampling operation cannot be chosen here: an
/// object-fit adjustment or a pattern tile can still change its used size.
/// <https://drafts.csswg.org/css-images-4/#propdef-image-rendering>
pub(in crate::layout) fn raster_image_sampling(
    style: &ComputedStyle,
) -> crate::document::paint::images::RasterSampling {
    use crate::document::paint::images::RasterSampling;

    match style.image_rendering {
        css::ImageRendering::Auto => RasterSampling::Auto,
        css::ImageRendering::Smooth => RasterSampling::Smooth,
        css::ImageRendering::HighQuality => RasterSampling::HighQuality,
        css::ImageRendering::Pixelated => RasterSampling::Pixelated,
        css::ImageRendering::CrispEdges => RasterSampling::CrispEdges,
    }
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

/// A private covered backdrop must remain in the PDF display list so the
/// later border's antialiased edge samples it rather than the page canvas.
fn opaque_border_backing_rect_primitive(
    area: PaintBackgroundArea,
    color: CssColor,
) -> PaintPrimitive {
    PaintPrimitive::Rect(
        RenderedRect::from_paint_rect(area.paint_rect(), Some(color))
            .with_opaque_backdrop_preservation(),
    )
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
                    raster.metadata.pixel_size.width,
                    raster.metadata.pixel_size.height,
                    &raster.rgb,
                    raster.alpha.as_deref(),
                    &raster.color_space,
                )
            })
            .flatten();
    }
    opaque_uniform_raster_samples_color(
        decoded.pixel_size.width,
        decoded.pixel_size.height,
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

/// One finite visible part of a non-repeating SVG tile, represented in both
/// CSS paint and root SVG source coordinates.
///
/// CSS background clipping selects the visible destination area while SVG
/// viewport coordinates select the corresponding source rectangle. Keeping
/// them paired avoids accidentally applying a source crop with an unrelated
/// destination transform. The destination is deliberately one of the
/// original CSS rectangles whenever one contains the other: reconstructing an
/// equal rectangle from an f64 intersection changes its f32 raster edge.
/// <https://drafts.csswg.org/css-backgrounds-4/#background-clip>
#[derive(Debug, Clone, Copy)]
struct ResolvedNonRepeatingSvgTile {
    destination_area: PaintBackgroundArea,
    source: crate::svg::SvgSourceRect,
}

/// Resolve the visible part of a non-repeating SVG tile and the matching SVG
/// source rectangle without first collapsing an enormous tile to f32 page
/// coordinates.
fn non_repeating_svg_visible_area(
    asset: &SharedSvgAsset,
    tile: &ResolvedBackgroundTile,
) -> Option<ResolvedNonRepeatingSvgTile> {
    if !matches!(tile.repeat.x_axis(), css::BackgroundRepeatAxis::NoRepeat)
        || !matches!(tile.repeat.y_axis(), css::BackgroundRepeatAxis::NoRepeat)
        || tile.size.width <= 0.0
        || tile.size.height <= 0.0
    {
        return None;
    }
    resolve_non_repeating_svg_visible_tile(
        PaintPoint::new(
            (f64::from(tile.positioning_area.x()) + tile.offset.x) as f32,
            (f64::from(tile.positioning_area.y()) + tile.offset.y) as f32,
        ),
        (
            f64::from(tile.positioning_area.x()) + tile.offset.x,
            f64::from(tile.positioning_area.y()) + tile.offset.y,
        ),
        tile.size,
        tile.clip_area,
        asset.source_viewport_size(),
    )
}

/// Resolve the finite paint and source rectangles for a no-repeat SVG tile.
///
/// `precise_tile_origin` preserves the precision used while resolving CSS
/// background position. `tile_origin` is the original f32 geometry supplied
/// to the vector painter. A contained tile must retain the latter unchanged,
/// while a contained clip must retain its own original CSS geometry.
fn resolve_non_repeating_svg_visible_tile(
    tile_origin: PaintPoint,
    precise_tile_origin: (f64, f64),
    tile_size: PaintSize,
    clip_area: PaintBackgroundArea,
    source_size: crate::svg::SvgSourceSize,
) -> Option<ResolvedNonRepeatingSvgTile> {
    let tile_x = precise_tile_origin.0;
    let tile_y = precise_tile_origin.1;
    let tile_width = f64::from(tile_size.width);
    let tile_height = f64::from(tile_size.height);
    let clip_x = f64::from(clip_area.x());
    let clip_y = f64::from(clip_area.y());
    let clip_width = f64::from(clip_area.width());
    let clip_height = f64::from(clip_area.height());
    let (x1, x2) = intersect_background_axis_precise(tile_x, tile_width, clip_x, clip_width)?;
    let (y1, y2) = intersect_background_axis_precise(tile_y, tile_height, clip_y, clip_height)?;

    let tile_area = PaintBackgroundArea::new(tile_origin, tile_size);
    let tile_is_inside_clip = clip_x <= tile_x
        && tile_x + tile_width <= clip_x + clip_width
        && clip_y <= tile_y
        && tile_y + tile_height <= clip_y + clip_height;
    let clip_is_inside_tile = tile_x <= clip_x
        && clip_x + clip_width <= tile_x + tile_width
        && tile_y <= clip_y
        && clip_y + clip_height <= tile_y + tile_height;
    let destination_area = if tile_is_inside_clip {
        tile_area
    } else if clip_is_inside_tile {
        clip_area
    } else {
        PaintBackgroundArea::new(
            PaintPoint::new(x1 as f32, y1 as f32),
            PaintSize::new((x2 - x1) as f32, (y2 - y1) as f32),
        )
    };
    let source = if tile_is_inside_clip {
        crate::svg::SvgSourceRect::new(crate::svg::SvgSourcePoint::new(0.0, 0.0), source_size)
    } else {
        svg_source_rect_for_visible_tile(
            tile_x,
            tile_y,
            tile_width,
            tile_height,
            x1,
            x2,
            y1,
            y2,
            source_size,
        )
    };
    Some(ResolvedNonRepeatingSvgTile {
        destination_area,
        source,
    })
}

/// Map an already-resolved CSS destination crop back into the top-left SVG
/// source viewport. This is only used when CSS clipping genuinely removes a
/// part of the tile.
#[allow(clippy::too_many_arguments)]
fn svg_source_rect_for_visible_tile(
    tile_x: f64,
    tile_y: f64,
    tile_width: f64,
    tile_height: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
    source_size: crate::svg::SvgSourceSize,
) -> crate::svg::SvgSourceRect {
    let source_x = ((x1 - tile_x) / tile_width * f64::from(source_size.width)) as f32;
    let source_y =
        ((tile_y + tile_height - y2) / tile_height * f64::from(source_size.height)) as f32;
    let source_width = ((x2 - x1) / tile_width * f64::from(source_size.width)) as f32;
    let source_height = ((y2 - y1) / tile_height * f64::from(source_size.height)) as f32;
    crate::svg::SvgSourceRect::new(
        crate::svg::SvgSourcePoint::new(source_x, source_y),
        crate::svg::SvgSourceSize::new(source_width, source_height),
    )
}

/// PDF-private source-edge continuations beneath a fully opaque square
/// border. The main CSS tile remains untouched; these strips are all outside
/// its positioning area and below later border ink.
#[derive(Debug, Clone)]
struct PdfOpaqueBorderBacking {
    strips: Vec<FullTileSvgBorderBackingStrip>,
}

#[derive(Debug, Clone, Copy)]
struct FullTileSvgBorderBackingStrip {
    source: crate::svg::SvgSourceRect,
    source_destination: PaintBackgroundArea,
    destination: PaintBackgroundArea,
    reflection: PaintTransform,
}

#[derive(Debug, Clone, Copy)]
enum FullTileSvgSourceEdge {
    Start,
    End,
    Full,
}

impl PdfOpaqueBorderBacking {
    fn new(
        style: &ComputedStyle,
        layer: &css::BackgroundLayer,
        tile: &ResolvedBackgroundTile,
        visible: ResolvedNonRepeatingSvgTile,
    ) -> Option<Self> {
        if layer.clip != css::BackgroundBox::Border
            || !matches!(&layer.size, css::BackgroundSize::Cover)
            || !tile_covers_positioning_area(tile)
            || !has_opaque_square_normal_border(style)
        {
            return None;
        }
        let strips = full_tile_svg_border_backing_strips(tile.clip_area, visible)?;
        (!strips.is_empty()).then_some(Self { strips })
    }
}

fn tile_covers_positioning_area(tile: &ResolvedBackgroundTile) -> bool {
    let start_x = f64::from(tile.positioning_area.x()) + tile.offset.x;
    let start_y = f64::from(tile.positioning_area.y()) + tile.offset.y;
    start_x <= f64::from(tile.positioning_area.x())
        && start_y <= f64::from(tile.positioning_area.y())
        && start_x + f64::from(tile.size.width)
            >= f64::from(tile.positioning_area.x() + tile.positioning_area.width())
        && start_y + f64::from(tile.size.height)
            >= f64::from(tile.positioning_area.y() + tile.positioning_area.height())
}

/// Repaint the finite padding-box portion of a cover SVG after the border.
///
/// The original border-clipped SVG remains in the normal CSS background
/// phase.  This bounded replay matches a reference child whose colored box
/// begins at the padding edge, without enlarging the CSS tile or exposing the
/// image over the border.
/// Partition the border-only continuation into disjoint side and corner
/// strips. The temporary source paint remains inside the visible tile, then
/// reflects about its respective edge.
fn full_tile_svg_border_backing_strips(
    clip: PaintBackgroundArea,
    visible: ResolvedNonRepeatingSvgTile,
) -> Option<Vec<FullTileSvgBorderBackingStrip>> {
    let inner = visible.destination_area;
    if !paint_background_area_contains(clip, inner) {
        return None;
    }
    let left = inner.x() - clip.x();
    let right = clip.x() + clip.width() - inner.x() - inner.width();
    let bottom = inner.y() - clip.y();
    let top = clip.y() + clip.height() - inner.y() - inner.height();
    let mut strips = Vec::with_capacity(8);
    let left_x = inner.x();
    let right_x = inner.x() + inner.width();
    let bottom_y = inner.y();
    let top_y = inner.y() + inner.height();
    for (destination, horizontal, vertical, anchor_x, anchor_y) in [
        (
            PaintBackgroundArea::new(
                PaintPoint::new(clip.x(), bottom_y),
                PaintSize::new(left, inner.height()),
            ),
            FullTileSvgSourceEdge::Start,
            FullTileSvgSourceEdge::Full,
            left_x,
            bottom_y,
        ),
        (
            PaintBackgroundArea::new(
                PaintPoint::new(right_x, bottom_y),
                PaintSize::new(right, inner.height()),
            ),
            FullTileSvgSourceEdge::End,
            FullTileSvgSourceEdge::Full,
            right_x,
            bottom_y,
        ),
        (
            PaintBackgroundArea::new(
                PaintPoint::new(left_x, clip.y()),
                PaintSize::new(inner.width(), bottom),
            ),
            FullTileSvgSourceEdge::Full,
            FullTileSvgSourceEdge::End,
            left_x,
            bottom_y,
        ),
        (
            PaintBackgroundArea::new(
                PaintPoint::new(left_x, top_y),
                PaintSize::new(inner.width(), top),
            ),
            FullTileSvgSourceEdge::Full,
            FullTileSvgSourceEdge::Start,
            left_x,
            top_y,
        ),
        (
            PaintBackgroundArea::new(
                PaintPoint::new(clip.x(), clip.y()),
                PaintSize::new(left, bottom),
            ),
            FullTileSvgSourceEdge::Start,
            FullTileSvgSourceEdge::End,
            left_x,
            bottom_y,
        ),
        (
            PaintBackgroundArea::new(
                PaintPoint::new(right_x, clip.y()),
                PaintSize::new(right, bottom),
            ),
            FullTileSvgSourceEdge::End,
            FullTileSvgSourceEdge::End,
            right_x,
            bottom_y,
        ),
        (
            PaintBackgroundArea::new(PaintPoint::new(clip.x(), top_y), PaintSize::new(left, top)),
            FullTileSvgSourceEdge::Start,
            FullTileSvgSourceEdge::Start,
            left_x,
            top_y,
        ),
        (
            PaintBackgroundArea::new(PaintPoint::new(right_x, top_y), PaintSize::new(right, top)),
            FullTileSvgSourceEdge::End,
            FullTileSvgSourceEdge::Start,
            right_x,
            top_y,
        ),
    ] {
        push_full_tile_svg_border_backing_strip(
            &mut strips,
            visible,
            destination,
            horizontal,
            vertical,
            anchor_x,
            anchor_y,
        );
    }
    Some(strips)
}

#[allow(clippy::too_many_arguments)]
fn push_full_tile_svg_border_backing_strip(
    strips: &mut Vec<FullTileSvgBorderBackingStrip>,
    visible: ResolvedNonRepeatingSvgTile,
    destination: PaintBackgroundArea,
    horizontal: FullTileSvgSourceEdge,
    vertical: FullTileSvgSourceEdge,
    anchor_x: f32,
    anchor_y: f32,
) {
    if destination.width() <= 0.0 || destination.height() <= 0.0 {
        return;
    }
    let source_destination = PaintBackgroundArea::new(
        PaintPoint::new(
            if matches!(horizontal, FullTileSvgSourceEdge::End) {
                anchor_x - destination.width()
            } else {
                anchor_x
            },
            if matches!(vertical, FullTileSvgSourceEdge::Start) {
                anchor_y - destination.height()
            } else {
                anchor_y
            },
        ),
        destination.size(),
    );
    let source = full_tile_svg_source_edge_rect(
        visible.source,
        horizontal,
        vertical,
        destination.size(),
        visible.destination_area.size(),
    );
    let reflect_x = !matches!(horizontal, FullTileSvgSourceEdge::Full);
    let reflect_y = !matches!(vertical, FullTileSvgSourceEdge::Full);
    strips.push(FullTileSvgBorderBackingStrip {
        source,
        source_destination,
        destination,
        reflection: PaintTransform::new(
            if reflect_x { -1.0 } else { 1.0 },
            0.0,
            0.0,
            if reflect_y { -1.0 } else { 1.0 },
            if reflect_x { 2.0 * anchor_x } else { 0.0 },
            if reflect_y { 2.0 * anchor_y } else { 0.0 },
        ),
    });
}

fn full_tile_svg_source_edge_rect(
    source: crate::svg::SvgSourceRect,
    horizontal: FullTileSvgSourceEdge,
    vertical: FullTileSvgSourceEdge,
    destination: PaintSize,
    visible: PaintSize,
) -> crate::svg::SvgSourceRect {
    let axis = |start: f32, size: f32, destination_size: f32, visible_size: f32, edge| {
        if matches!(edge, FullTileSvgSourceEdge::Full) {
            (start, size)
        } else {
            let extent = (size * destination_size / visible_size)
                .min(size)
                .max(f32::MIN_POSITIVE);
            match edge {
                FullTileSvgSourceEdge::Start => (start, extent),
                FullTileSvgSourceEdge::End => (start + size - extent, extent),
                FullTileSvgSourceEdge::Full => unreachable!(),
            }
        }
    };
    let (x, width) = axis(
        source.origin.x,
        source.size.width,
        destination.width,
        visible.width,
        horizontal,
    );
    let (y, height) = axis(
        source.origin.y,
        source.size.height,
        destination.height,
        visible.height,
        vertical,
    );
    crate::svg::SvgSourceRect::new(
        crate::svg::SvgSourcePoint::new(x, y),
        crate::svg::SvgSourceSize::new(width, height),
    )
}

fn append_svg_border_backing_primitives(
    primitives: &mut Vec<PaintPrimitive>,
    asset: &SharedSvgAsset,
    viewport_fill: Option<CssColor>,
    backing: PdfOpaqueBorderBacking,
    rounded_clip: Option<&RenderedPathClip>,
) {
    for strip in backing.strips {
        if let Some(color) = viewport_fill.or_else(|| asset.opaque_source_rect_fill(strip.source)) {
            primitives.push(opaque_border_backing_rect_primitive(
                strip.destination,
                color,
            ));
        } else {
            for mut path in asset
                .paint_paths_for_source_rect(strip.source_destination.paint_rect(), strip.source)
            {
                path = path.transformed(strip.reflection);
                append_rounded_background_clip(&mut path, rounded_clip);
                primitives.push(PaintPrimitive::Path(path));
            }
        }
    }
}

fn paint_background_area_contains(outer: PaintBackgroundArea, inner: PaintBackgroundArea) -> bool {
    outer.x() <= inner.x()
        && outer.y() <= inner.y()
        && outer.x() + outer.width() >= inner.x() + inner.width()
        && outer.y() + outer.height() >= inner.y() + inner.height()
}

#[cfg(any())]
mod obsolete_pdf_opaque_border_backing {
    use super::*;

    fn ignored() {
        if touches_left {
            push_svg_border_backing_strip(
                &mut strips,
                visible,
                positioning,
                PaintBackgroundArea::new(
                    PaintPoint::new(clip.x(), inside_y),
                    PaintSize::new(left_width, inside_height),
                ),
                SvgSourceEdge::Start,
                SvgSourceEdge::Full,
            );
        }
        if touches_right {
            push_svg_border_backing_strip(
                &mut strips,
                visible,
                positioning,
                PaintBackgroundArea::new(
                    PaintPoint::new(position_right, inside_y),
                    PaintSize::new(right_width, inside_height),
                ),
                SvgSourceEdge::End,
                SvgSourceEdge::Full,
            );
        }
        if touches_bottom {
            push_svg_border_backing_strip(
                &mut strips,
                visible,
                positioning,
                PaintBackgroundArea::new(
                    PaintPoint::new(inside_x, clip.y()),
                    PaintSize::new(inside_width, bottom_height),
                ),
                SvgSourceEdge::Full,
                SvgSourceEdge::End,
            );
        }
        if touches_top {
            push_svg_border_backing_strip(
                &mut strips,
                visible,
                positioning,
                PaintBackgroundArea::new(
                    PaintPoint::new(inside_x, position_top),
                    PaintSize::new(inside_width, top_height),
                ),
                SvgSourceEdge::Full,
                SvgSourceEdge::Start,
            );
        }

        for (horizontal, vertical, x, y, width, height, enabled) in [
            (
                SvgSourceEdge::Start,
                SvgSourceEdge::End,
                clip.x(),
                clip.y(),
                left_width,
                bottom_height,
                touches_left && touches_bottom,
            ),
            (
                SvgSourceEdge::End,
                SvgSourceEdge::End,
                position_right,
                clip.y(),
                right_width,
                bottom_height,
                touches_right && touches_bottom,
            ),
            (
                SvgSourceEdge::Start,
                SvgSourceEdge::Start,
                clip.x(),
                position_top,
                left_width,
                top_height,
                touches_left && touches_top,
            ),
            (
                SvgSourceEdge::End,
                SvgSourceEdge::Start,
                position_right,
                position_top,
                right_width,
                top_height,
                touches_right && touches_top,
            ),
        ] {
            if enabled {
                push_svg_border_backing_strip(
                    &mut strips,
                    visible,
                    positioning,
                    PaintBackgroundArea::new(PaintPoint::new(x, y), PaintSize::new(width, height)),
                    horizontal,
                    vertical,
                );
            }
        }
        Some(strips)
    }

    fn push_svg_border_backing_strip(
        strips: &mut Vec<SvgBorderBackingStrip>,
        visible: ResolvedNonRepeatingSvgTile,
        positioning: PaintBackgroundArea,
        destination: PaintBackgroundArea,
        horizontal: SvgSourceEdge,
        vertical: SvgSourceEdge,
    ) {
        if destination.width() <= 0.0 || destination.height() <= 0.0 {
            return;
        }
        let source_destination = PaintBackgroundArea::new(
            PaintPoint::new(
                svg_backing_source_destination_axis(
                    destination.x(),
                    destination.width(),
                    positioning.x(),
                    positioning.width(),
                    horizontal,
                ),
                svg_backing_source_destination_axis(
                    destination.y(),
                    destination.height(),
                    positioning.y(),
                    positioning.height(),
                    vertical,
                ),
            ),
            destination.size(),
        );
        let source = svg_source_edge_rect(visible, positioning, destination, horizontal, vertical);
        let reflection = PaintTransform::new(
            if matches!(horizontal, SvgSourceEdge::Full) {
                1.0
            } else {
                -1.0
            },
            0.0,
            0.0,
            if matches!(vertical, SvgSourceEdge::Full) {
                1.0
            } else {
                -1.0
            },
            if matches!(horizontal, SvgSourceEdge::Full) {
                0.0
            } else {
                2.0 * svg_backing_reflection_axis(positioning.x(), positioning.width(), horizontal)
            },
            if matches!(vertical, SvgSourceEdge::Full) {
                0.0
            } else {
                2.0 * svg_backing_reflection_axis(positioning.y(), positioning.height(), vertical)
            },
        );
        strips.push(SvgBorderBackingStrip {
            source,
            source_destination,
            destination,
            reflection,
        });
    }

    fn svg_source_edge_rect(
        visible: ResolvedNonRepeatingSvgTile,
        positioning: PaintBackgroundArea,
        destination: PaintBackgroundArea,
        horizontal: SvgSourceEdge,
        vertical: SvgSourceEdge,
    ) -> crate::svg::SvgSourceRect {
        let (x, width) = svg_source_edge_axis(
            visible.source.origin.x,
            visible.source.size.width,
            visible.destination_area.x(),
            visible.destination_area.width(),
            positioning.x(),
            positioning.width(),
            destination.x(),
            destination.width(),
            horizontal,
        );
        let (y, height) = svg_source_edge_axis(
            visible.source.origin.y,
            visible.source.size.height,
            visible.destination_area.y(),
            visible.destination_area.height(),
            positioning.y(),
            positioning.height(),
            destination.y(),
            destination.height(),
            vertical,
        );
        crate::svg::SvgSourceRect::new(
            crate::svg::SvgSourcePoint::new(x, y),
            crate::svg::SvgSourceSize::new(width, height),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn svg_source_edge_axis(
        source_start: f32,
        source_size: f32,
        visible_destination_start: f32,
        visible_destination_size: f32,
        positioning_start: f32,
        positioning_size: f32,
        destination_start: f32,
        destination_size: f32,
        edge: SvgSourceEdge,
    ) -> (f32, f32) {
        let source_at = |destination_position: f32| {
            source_start
                + (destination_position - visible_destination_start) * source_size
                    / visible_destination_size
        };
        let extent = (source_size * destination_size / visible_destination_size)
            .min(source_size)
            .max(f32::MIN_POSITIVE);
        match edge {
            SvgSourceEdge::Start => (source_at(positioning_start), extent),
            SvgSourceEdge::End => (
                source_at(positioning_start + positioning_size) - extent,
                extent,
            ),
            SvgSourceEdge::Full => (
                source_at(destination_start),
                source_size * destination_size / visible_destination_size,
            ),
        }
    }

    fn svg_backing_source_destination_axis(
        destination_start: f32,
        destination_size: f32,
        positioning_start: f32,
        positioning_size: f32,
        edge: SvgSourceEdge,
    ) -> f32 {
        match edge {
            SvgSourceEdge::Start => positioning_start,
            SvgSourceEdge::End => positioning_start + positioning_size - destination_size,
            SvgSourceEdge::Full => destination_start,
        }
    }

    fn svg_backing_reflection_axis(
        positioning_start: f32,
        positioning_size: f32,
        edge: SvgSourceEdge,
    ) -> f32 {
        match edge {
            SvgSourceEdge::Start => positioning_start,
            SvgSourceEdge::End => positioning_start + positioning_size,
            SvgSourceEdge::Full => unreachable!(),
        }
    }

    fn append_svg_border_backing_primitives(
        primitives: &mut Vec<PaintPrimitive>,
        asset: &SharedSvgAsset,
        viewport_fill: Option<CssColor>,
        backing: PdfOpaqueBorderBacking,
        rounded_clip: Option<&RenderedPathClip>,
    ) {
        for strip in backing.strips {
            if let Some(color) =
                viewport_fill.or_else(|| asset.opaque_source_rect_fill(strip.source))
            {
                primitives.push(opaque_border_backing_rect_primitive(
                    strip.destination,
                    color,
                ));
            } else {
                for mut path in asset.paint_paths_for_source_rect(
                    strip.source_destination.paint_rect(),
                    strip.source,
                ) {
                    path = path.transformed(strip.reflection);
                    append_rounded_background_clip(&mut path, rounded_clip);
                    primitives.push(PaintPrimitive::Path(path));
                }
            }
        }
    }

    fn paint_background_area_contains(
        outer: PaintBackgroundArea,
        inner: PaintBackgroundArea,
    ) -> bool {
        outer.x() <= inner.x()
            && outer.y() <= inner.y()
            && outer.x() + outer.width() >= inner.x() + inner.width()
            && outer.y() + outer.height() >= inner.y() + inner.height()
    }
}

pub(in crate::layout) fn has_opaque_square_normal_border(style: &ComputedStyle) -> bool {
    if !style.border_radius.clone().is_zero() || style.border_image.source.is_image() {
        return false;
    }
    let widths = used_border_widths(style);
    let styles = style.border_styles;
    let colors = style.border_colors.resolve(style.color);
    widths.top > 0.0
        && widths.right > 0.0
        && widths.bottom > 0.0
        && widths.left > 0.0
        && styles.top == css::BorderStyle::Solid
        && styles.right == css::BorderStyle::Solid
        && styles.bottom == css::BorderStyle::Solid
        && styles.left == css::BorderStyle::Solid
        && colors.top.is_opaque()
        && colors.right.is_opaque()
        && colors.bottom.is_opaque()
        && colors.left.is_opaque()
}

fn append_rounded_background_clip(
    path: &mut RenderedPath,
    rounded_clip: Option<&RenderedPathClip>,
) {
    let Some(rounded_clip) = rounded_clip else {
        return;
    };
    let clip = path.clip.get_or_insert_with(|| rounded_clip.clone());
    // A rounded background clip intersects (rather than replaces) an SVG
    // root or CSS rectangular clip. The direct clone above is already the
    // only clip.
    if !clip.commands.eq(&rounded_clip.commands) || clip.fill_rule != rounded_clip.fill_rule {
        clip.additional_clips.push(RenderedPathClipPath::new(
            rounded_clip.commands.clone(),
            rounded_clip.fill_rule,
        ));
    }
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
        BackgroundImage::LightDark(_) => {
            unreachable!("light-dark() must resolve before background paint")
        }
        BackgroundImage::ImageSet(_)
        | BackgroundImage::SelectedImageSet { .. }
        | BackgroundImage::Url(_)
        | BackgroundImage::ImageFunction(_)
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
        BackgroundImage::LightDark(_) => {
            unreachable!("light-dark() must resolve before background paint")
        }
        BackgroundImage::ImageSet(_) | BackgroundImage::SelectedImageSet { .. } => {
            unreachable!("selected image-set source is unwrapped")
        }
        BackgroundImage::Url(_) | BackgroundImage::ImageFunction(_) => return true,
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
    orientation_policy: crate::image_store::RasterOrientationPolicy,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    match layer.image.as_image()?.selected_image() {
        BackgroundImage::LightDark(_) => {
            unreachable!("light-dark() must resolve before background paint")
        }
        BackgroundImage::ImageSet(_) | BackgroundImage::SelectedImageSet { .. } => {
            unreachable!("selected image-set source is unwrapped")
        }
        BackgroundImage::Url(url) => load_image_source_with_request(
            &url.href,
            url.base_url.as_ref().or(fallback_base_url),
            url.root_url.as_ref().or(fallback_root_url),
            resource_cache,
            orientation_policy,
            &url.request_modifiers,
        ),
        BackgroundImage::ImageFunction(_) => match resolve_css_image_source(
            layer.image.as_image()?.selected_image(),
            ImageResolutionContext {
                base_url: fallback_base_url,
                root_url: fallback_root_url,
                current_color,
                orientation: orientation_policy,
                svg_context: crate::svg::SvgImageContext::default(),
                resource_cache,
            },
        ) {
            ResolvedCssImage::External(ResolvedImageAsset::Raster(image)) => Some(image),
            ResolvedCssImage::SolidColor(color) => Some(solid_color_image(color)),
            ResolvedCssImage::External(ResolvedImageAsset::Svg(_)) | ResolvedCssImage::Invalid => {
                None
            }
        },
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
        BackgroundImage::LightDark(_) => {
            unreachable!("light-dark() must resolve before background paint")
        }
        BackgroundImage::ImageSet(_) | BackgroundImage::SelectedImageSet { .. } => {
            unreachable!("selected image-set source is unwrapped")
        }
        BackgroundImage::Url(url) => load_image_source_with_request(
            &url.href,
            url.base_url.as_ref().or(fallback_base_url),
            url.root_url.as_ref().or(fallback_root_url),
            resource_cache,
            crate::image_store::RasterOrientationPolicy::FromImage,
            &url.request_modifiers,
        ),
        BackgroundImage::ImageFunction(_) => match resolve_css_image_source(
            image,
            ImageResolutionContext {
                base_url: fallback_base_url,
                root_url: fallback_root_url,
                current_color,
                orientation: crate::image_store::RasterOrientationPolicy::FromImage,
                svg_context: crate::svg::SvgImageContext::default(),
                resource_cache,
            },
        ) {
            ResolvedCssImage::External(ResolvedImageAsset::Raster(image)) => Some(image),
            ResolvedCssImage::SolidColor(color) => Some(solid_color_image(color)),
            ResolvedCssImage::External(ResolvedImageAsset::Svg(_)) | ResolvedCssImage::Invalid => {
                None
            }
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cropped_non_repeating_svg_paths_end_at_the_visible_destination_edge() {
        let asset = crate::svg::parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="32">
                <rect width="8" height="16" fill="lime"/>
                <rect y="16" width="8" height="16" fill="aqua"/>
            </svg>"#,
        )
        .unwrap()
        .with_css_image_viewport(PaintSize::new(256.0, 1024.0));
        let slice = ResolvedNonRepeatingSvgTile {
            destination_area: PaintBackgroundArea::new(
                PaintPoint::new(0.0, 0.0),
                PaintSize::new(256.0, 768.0),
            ),
            source: crate::svg::SvgSourceRect::new(
                crate::svg::SvgSourcePoint::new(0.0, 8.0),
                crate::svg::SvgSourceSize::new(8.0, 24.0),
            ),
        };

        let paths =
            asset.paint_paths_for_source_rect(slice.destination_area.paint_rect(), slice.source);

        assert_eq!(paths.len(), 2);
        assert!(paths.iter().all(|path| path.clip.is_none()));
        assert!(paths.iter().all(|path| {
            path.paint_bounds().is_some_and(|bounds| {
                paint_rect_contains(slice.destination_area.paint_rect(), bounds)
            })
        }));
    }

    #[test]
    fn no_repeat_svg_tile_inside_clip_keeps_original_tile_coordinates() {
        let tile_origin = PaintPoint::new(13.125, 27.25);
        let tile_size = PaintSize::new(41.5, 19.75);
        let slice = resolve_non_repeating_svg_visible_tile(
            tile_origin,
            (13.125_f64, 27.25_f64),
            tile_size,
            PaintBackgroundArea::new(
                PaintPoint::new(-100.0, -100.0),
                PaintSize::new(500.0, 500.0),
            ),
            crate::svg::SvgSourceSize::new(83.0, 79.0),
        )
        .unwrap();

        assert_eq!(
            slice.destination_area,
            PaintBackgroundArea::new(tile_origin, tile_size)
        );
        assert_eq!(
            slice.source,
            crate::svg::SvgSourceRect::new(
                crate::svg::SvgSourcePoint::new(0.0, 0.0),
                crate::svg::SvgSourceSize::new(83.0, 79.0),
            )
        );
    }

    #[test]
    fn no_repeat_svg_clip_inside_cover_tile_keeps_original_clip_coordinates() {
        let clip =
            PaintBackgroundArea::new(PaintPoint::new(5.25, 11.75), PaintSize::new(9.5, 7.25));
        let slice = resolve_non_repeating_svg_visible_tile(
            PaintPoint::new(1.0, 2.0),
            (1.0, 2.0),
            PaintSize::new(32.0, 24.0),
            clip,
            crate::svg::SvgSourceSize::new(64.0, 48.0),
        )
        .unwrap();

        assert_eq!(slice.destination_area, clip);
        assert_eq!(
            slice.source,
            crate::svg::SvgSourceRect::new(
                crate::svg::SvgSourcePoint::new(8.5, 14.0),
                crate::svg::SvgSourceSize::new(19.0, 14.5),
            )
        );
    }

    #[test]
    fn no_repeat_svg_partial_intersection_keeps_matching_source_mapping() {
        let slice = resolve_non_repeating_svg_visible_tile(
            PaintPoint::new(0.0, 0.0),
            (0.0, 0.0),
            PaintSize::new(10.0, 10.0),
            PaintBackgroundArea::new(PaintPoint::new(5.0, -5.0), PaintSize::new(10.0, 10.0)),
            crate::svg::SvgSourceSize::new(10.0, 10.0),
        )
        .unwrap();

        assert_eq!(
            slice.destination_area,
            PaintBackgroundArea::new(PaintPoint::new(5.0, 0.0), PaintSize::new(5.0, 5.0),)
        );
        assert_eq!(
            slice.source,
            crate::svg::SvgSourceRect::new(
                crate::svg::SvgSourcePoint::new(5.0, 5.0),
                crate::svg::SvgSourceSize::new(5.0, 5.0),
            )
        );
    }

    fn no_repeat_tile(
        positioning_area: PaintBackgroundArea,
        clip_area: PaintBackgroundArea,
        size: PaintSize,
        offset: PaintBackgroundOffset,
    ) -> ResolvedBackgroundTile {
        ResolvedBackgroundTile {
            positioning_area,
            clip_area,
            rounded_clip: None,
            size,
            offset,
            repeat: css::BackgroundRepeat::NoRepeat,
        }
    }

    #[test]
    fn border_disjoint_phase_requires_one_finite_non_repeating_tile() {
        let clip = PaintBackgroundArea::new(PaintPoint::new(0.0, 0.0), PaintSize::new(12.0, 12.0));
        let contained = no_repeat_tile(
            PaintBackgroundArea::new(PaintPoint::new(1.0, 1.0), PaintSize::new(10.0, 10.0)),
            clip,
            PaintSize::new(10.0, 10.0),
            PaintBackgroundOffset::new(0.0, 0.0),
        );
        let padding =
            PaintBackgroundArea::new(PaintPoint::new(1.0, 1.0), PaintSize::new(10.0, 10.0));
        assert!(finite_no_repeat_tile_is_inside(&contained, padding));

        let oversized = no_repeat_tile(
            PaintBackgroundArea::new(PaintPoint::new(1.0, 1.0), PaintSize::new(10.0, 10.0)),
            clip,
            PaintSize::new(20.0, 20.0),
            PaintBackgroundOffset::new(-5.0, -5.0),
        );
        assert!(!finite_no_repeat_tile_is_inside(&oversized, padding));

        let partial = no_repeat_tile(
            PaintBackgroundArea::new(PaintPoint::new(1.0, 1.0), PaintSize::new(10.0, 10.0)),
            clip,
            PaintSize::new(10.0, 10.0),
            PaintBackgroundOffset::new(-5.0, 0.0),
        );
        assert!(!finite_no_repeat_tile_is_inside(&partial, padding));

        let repeated = ResolvedBackgroundTile {
            repeat: css::BackgroundRepeat::Repeat,
            ..contained.clone()
        };
        assert!(!finite_no_repeat_tile_is_inside(&repeated, padding));

        let mut eligible_layers = BackgroundImagePhaseEligibility::default();
        eligible_layers.note_tile(Some(padding), &contained);
        eligible_layers.note_tile(Some(padding), &contained);
        assert!(eligible_layers.finish());

        let mut mixed_layers = BackgroundImagePhaseEligibility::default();
        mixed_layers.note_tile(Some(padding), &contained);
        mixed_layers.note_tile(Some(padding), &repeated);
        assert!(!mixed_layers.finish());
    }

    #[test]
    fn cropped_non_repeating_svg_paths_retain_a_rounded_background_clip() {
        let mut path = RenderedPath::new(
            paint_rect_path_commands(PaintRect::new(
                PaintPoint::new(0.0, 0.0),
                PaintSize::new(10.0, 10.0),
            )),
            Some(CssColor::new(0, 255, 255)),
            RenderedPathFillRule::NonZero,
            None,
            PaintStrokeWidth::ZERO,
            None,
        );
        let rounded_clip = RenderedPathClip::new(
            paint_rect_path_commands(PaintRect::new(
                PaintPoint::new(1.0, 1.0),
                PaintSize::new(8.0, 8.0),
            )),
            RenderedPathFillRule::NonZero,
            Vec::new(),
        );

        append_rounded_background_clip(&mut path, Some(&rounded_clip));

        assert_eq!(path.clip, Some(rounded_clip));
    }

    #[cfg(any())]
    #[test]
    fn svg_border_backing_partitions_full_and_partial_edge_contact() {
        let clip = PaintBackgroundArea::new(PaintPoint::new(0.0, 0.0), PaintSize::new(12.0, 12.0));
        let positioning =
            PaintBackgroundArea::new(PaintPoint::new(1.0, 1.0), PaintSize::new(10.0, 10.0));
        let source = crate::svg::SvgSourceRect::new(
            crate::svg::SvgSourcePoint::new(0.0, 0.0),
            crate::svg::SvgSourceSize::new(100.0, 100.0),
        );
        let full = svg_border_backing_strips(
            clip,
            positioning,
            ResolvedNonRepeatingSvgTile {
                destination_area: positioning,
                source,
            },
        )
        .unwrap();
        assert_eq!(full.len(), 8);
        assert_eq!(
            full.iter()
                .map(|strip| strip.destination.width() * strip.destination.height())
                .sum::<f32>(),
            44.0
        );

        let partial = svg_border_backing_strips(
            clip,
            positioning,
            ResolvedNonRepeatingSvgTile {
                destination_area: PaintBackgroundArea::new(
                    PaintPoint::new(1.0, 7.0),
                    PaintSize::new(4.0, 4.0),
                ),
                source,
            },
        )
        .unwrap();
        assert_eq!(partial.len(), 3);
        assert!(partial.iter().all(|strip| {
            strip.destination.x() + strip.destination.width() <= positioning.x()
                || strip.destination.y() + strip.destination.height() <= positioning.y()
                || strip.destination.x() >= positioning.x() + positioning.width()
                || strip.destination.y() >= positioning.y() + positioning.height()
        }));
    }
}
