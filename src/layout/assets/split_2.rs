use super::*;
use crate::document::PaintPatternTiling;
use std::rc::Rc;

/// A CSS background positioning or clipping area in one bottom-left-origin
/// coordinate space.
///
/// CSS Backgrounds resolves positioning, clipping, and repeat geometry in a
/// common coordinate system.  Keeping the marker on that rectangle prevents
/// document-canvas geometry from reaching page-local primitive emission before
/// the page-projection boundary:
/// <https://www.w3.org/TR/css-backgrounds-3/#backgrounds>.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(transparent)]
pub(in crate::layout) struct BackgroundArea<Space>(euclid::Rect<f32, Space>);

/// A background area ready for page-local paint primitive emission.
pub(in crate::layout) type PaintBackgroundArea = BackgroundArea<PaintSpace>;
/// A background area positioned on the assembled document canvas.
pub(in crate::layout) type DocumentCanvasBackgroundArea = BackgroundArea<DocumentCanvasSpace>;

impl<Space> BackgroundArea<Space> {
    pub(in crate::layout) fn new(
        origin: euclid::Point2D<f32, Space>,
        size: euclid::Size2D<f32, Space>,
    ) -> Self {
        Self(euclid::Rect::new(
            origin,
            euclid::Size2D::new(size.width.max(0.0), size.height.max(0.0)),
        ))
    }

    pub(in crate::layout) fn x(&self) -> f32 {
        self.0.origin.x
    }

    pub(in crate::layout) fn y(&self) -> f32 {
        self.0.origin.y
    }

    pub(in crate::layout) fn width(&self) -> f32 {
        self.0.size.width
    }

    pub(in crate::layout) fn height(&self) -> f32 {
        self.0.size.height
    }

    pub(in crate::layout) fn size(&self) -> euclid::Size2D<f32, Space> {
        self.0.size
    }

    pub(in crate::layout) fn inset(self, edges: css::Edges) -> Self {
        Self::new(
            euclid::Point2D::new(self.x() + edges.left, self.y() + edges.bottom),
            euclid::Size2D::new(
                (self.width() - edges.left - edges.right).max(0.0),
                (self.height() - edges.top - edges.bottom).max(0.0),
            ),
        )
    }

    pub(in crate::layout) fn intersect(self, other: Self) -> Option<Self> {
        self.0.intersection(&other.0).map(Self)
    }
}

impl BackgroundArea<PaintSpace> {
    pub(in crate::layout) fn from_paint_rect(rect: PaintRect) -> Self {
        Self(rect)
    }

    pub(in crate::layout) fn paint_rect(self) -> PaintRect {
        self.0
    }
}

impl BackgroundArea<DocumentCanvasSpace> {
    pub(in crate::layout) fn from_document_canvas_rect(rect: DocumentCanvasRect) -> Self {
        Self(rect)
    }

    /// Project a document-canvas area onto one page before page-local
    /// primitive emission.
    pub(in crate::layout) fn project_to_paint(
        self,
        page_document_bottom: f32,
    ) -> PaintBackgroundArea {
        PaintBackgroundArea::new(
            PaintPoint::new(self.x(), self.y() - page_document_bottom),
            PaintSize::new(self.width(), self.height()),
        )
    }
}

/// The distinct coordinate areas used to resolve a background image.
///
/// The selected `background-origin` area positions a normal layer, its
/// `background-clip` area bounds painting, and a fixed layer may instead use
/// a viewport-equivalent positioning area. CSS defines these independently:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin>,
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-attachment>.
#[derive(Debug, Clone, Copy)]
struct BackgroundPaintAreas<Space> {
    positioning_border_area: BackgroundArea<Space>,
    clip_border_area: BackgroundArea<Space>,
    fixed_positioning_area: Option<BackgroundArea<Space>>,
    fixed_attachment_is_scrolled_by_transform: bool,
}

/// Fully resolved geometry for one CSS background layer.
///
/// CSS Backgrounds resolves the positioning area, used image size, position,
/// and repeat behavior before painting the image. Keeping that result together
/// ensures vector and raster image sources use identical tile geometry:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-image>.
#[derive(Debug, Clone)]
struct ResolvedBackgroundTile {
    positioning_area: PaintBackgroundArea,
    clip_area: PaintBackgroundArea,
    rounded_clip: Option<RenderedPathClip>,
    size: PaintSize,
    offset: PaintBackgroundOffset,
    repeat: css::BackgroundRepeat,
}

impl ResolvedBackgroundTile {
    fn new(
        positioning_area: PaintBackgroundArea,
        clip_area: PaintBackgroundArea,
        rounded_clip: Option<RenderedPathClip>,
        layer: &css::BackgroundLayer,
        size: PaintSize,
    ) -> Self {
        let offset = background_position(layer.position.clone(), positioning_area.size(), size);
        Self {
            positioning_area,
            clip_area,
            rounded_clip,
            size,
            offset,
            repeat: layer.repeat,
        }
    }

    fn tile_xs(&self) -> Vec<f32> {
        let repeat = self.repeat.x_axis();
        let (area_start, area_size) = if repeat == css::BackgroundRepeatAxis::Space {
            (self.positioning_area.x(), self.positioning_area.width())
        } else {
            (self.clip_area.x(), self.clip_area.width())
        };
        background_tile_positions(
            (f64::from(self.positioning_area.x()) + self.offset.x) as f32,
            area_start,
            area_size,
            self.size.width,
            repeat,
        )
    }

    fn tile_ys(&self) -> Vec<f32> {
        let repeat = self.repeat.y_axis();
        let (area_start, area_size) = if repeat == css::BackgroundRepeatAxis::Space {
            (self.positioning_area.y(), self.positioning_area.height())
        } else {
            (self.clip_area.y(), self.clip_area.height())
        };
        background_tile_positions(
            (f64::from(self.positioning_area.y()) + self.offset.y) as f32,
            area_start,
            area_size,
            self.size.height,
            repeat,
        )
    }

    fn tiles(&self) -> Vec<PaintBackgroundArea> {
        let tile_xs = self.tile_xs();
        self.tile_ys()
            .into_iter()
            .flat_map(|y| {
                tile_xs
                    .iter()
                    .cloned()
                    .map(move |x| PaintBackgroundArea::new(PaintPoint::new(x, y), self.size))
            })
            .collect()
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
    )
}

fn background_image_primitives_for_style_impl(
    paint_areas: BackgroundPaintAreas<PaintSpace>,
    style: &ComputedStyle,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    use_pdf_patterns_for_repeated_images: bool,
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
        let clip_box = if background_border_box_paint_is_occluded(style, layer.clip) {
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
                    if let Some(clip) = &mut path.clip {
                        clip.additional_clips.push(RenderedPathClipPath::new(
                            paint_rect_path_commands(tile.clip_area.paint_rect()),
                            RenderedPathFillRule::NonZero,
                        ));
                        if let Some(rounded_clip) = &tile.rounded_clip {
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
        if matches!(selected_image, BackgroundImage::LinearGradient(gradient)
        if crate::layout::paint_helpers::linear_gradient_is_painted_by_box_decoration(
            gradient, layer, tile.size,
        )) {
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
                let Some(paths) = crate::layout::paint_helpers::linear_gradient_hard_stop_paths(
                    gradient,
                    layer,
                    tile_area.paint_rect(),
                    clip.paint_rect(),
                    tile.rounded_clip.clone(),
                ) else {
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

/// Return the tiles of one uniform-image axis in sufficiently precise page
/// coordinates to clip an extremely large non-repeating image.
fn color_image_axis_tiles(
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
        BackgroundImage::ImageSet { .. }
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
fn uniform_gradient_stop_color(
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
fn append_native_css_gradient_primitives(
    primitives: &mut Vec<PaintPrimitive>,
    image: &BackgroundImage,
    resolved: &ResolvedBackgroundTile,
    current_color: CssColor,
) -> bool {
    if !gradient_interpolation_can_use_native_shading(image) {
        return false;
    }
    let gradient = match image {
        BackgroundImage::LinearGradient(gradient) => linear_gradient_paint(
            &gradient.resolve_current_color(current_color),
            resolved.size,
        ),
        BackgroundImage::RadialGradient(gradient) => radial_gradient_paint(
            &gradient.resolve_current_color(current_color),
            resolved.size,
        ),
        _ => None,
    };
    let Some(gradient) = gradient else {
        return false;
    };
    if resolved.repeat.repeats_x() || resolved.repeat.repeats_y() {
        let area = resolved.clip_area;
        if area.width() <= 0.0 || area.height() <= 0.0 {
            return true;
        }
        let origin_x = background_first_tile_position(
            (f64::from(resolved.positioning_area.x()) + resolved.offset.x) as f32,
            resolved.positioning_area.x(),
            resolved.positioning_area.width(),
            resolved.size.width,
            resolved.repeat.x_axis(),
        );
        let origin_y = background_first_tile_position(
            (f64::from(resolved.positioning_area.y()) + resolved.offset.y) as f32,
            resolved.positioning_area.y(),
            resolved.positioning_area.height(),
            resolved.size.height,
            resolved.repeat.y_axis(),
        );
        let step_width = background_pattern_step(
            resolved.size.width,
            resolved.positioning_area.width(),
            resolved.repeat.x_axis(),
        );
        let step_height = background_pattern_step(
            resolved.size.height,
            resolved.positioning_area.height(),
            resolved.repeat.y_axis(),
        );
        if step_width > 0.0 && step_height > 0.0 {
            primitives.push(PaintPrimitive::GradientPattern(
                RenderedGradientPattern::new(
                    area.paint_rect(),
                    PaintPatternTiling::new(
                        resolved.size,
                        PaintSize::new(step_width, step_height),
                        PaintPoint::new(origin_x, origin_y),
                    ),
                    gradient,
                    resolved.rounded_clip.clone(),
                ),
            ));
        }
        return true;
    }
    for tile_area in resolved.tiles() {
        let Some(tile) = tile_area.intersect(resolved.clip_area) else {
            continue;
        };
        // Keep non-repeating gradients in a local PDF cell too. A direct
        // shading pattern has a matrix in the page coordinate system, while
        // CSS gradients are defined in the background tile's local image
        // coordinate system. The local cell preserves that distinction for
        // every tile, including a single `no-repeat` occurrence.
        primitives.push(PaintPrimitive::GradientPattern(
            RenderedGradientPattern::new(
                tile.paint_rect(),
                PaintPatternTiling::new(
                    resolved.size,
                    resolved.size,
                    PaintPoint::new(tile_area.x(), tile_area.y()),
                ),
                gradient.clone(),
                resolved.rounded_clip.clone(),
            ),
        ));
    }
    true
}

fn linear_gradient_paint(
    gradient: &css::LinearGradient,
    size: PaintSize,
) -> Option<RenderedGradient> {
    let line = angled_gradient_line(
        gradient.direction,
        PaintRect::new(PaintPoint::new(0.0, 0.0), size),
    );
    let mut fixed_stops = fixed_gradient_stops(gradient, line.axis_length)?;
    let color_space = resolve_fixed_gradient_colors(&mut fixed_stops, gradient.interpolation);
    let program = resolve_gradient_program(
        fixed_stops,
        &gradient.hints,
        line.axis_length,
        gradient.repeating,
        gradient.interpolation,
    )?;
    let periodic = periodic_pdf_gradient(&program, line.axis_length);
    let stops = repeating_gradient_average_color(&program, line.axis_length).map_or_else(
        || {
            periodic
                .as_ref()
                .map(|periodic| periodic.stops.clone())
                .or_else(|| normalized_pdf_gradient_stops(&program, line.axis_length))
        },
        |color| {
            Some(vec![
                RenderedGradientStop {
                    offset: 0.0,
                    color,
                    interpolation_exponent: 1.0,
                },
                RenderedGradientStop {
                    offset: 1.0,
                    color,
                    interpolation_exponent: 1.0,
                },
            ])
        },
    )?;
    let (start, end) = line.endpoints();
    Some(RenderedGradient {
        kind: RenderedGradientKind::Linear { start, end },
        color_space,
        stops,
        periodic,
        transform: PaintTransform::identity(),
    })
}

fn radial_gradient_paint(
    gradient: &css::RadialGradient,
    size: PaintSize,
) -> Option<RenderedGradient> {
    let geometry = used_radial_gradient_geometry(gradient, size)?;
    let mut fixed_stops = fixed_radial_gradient_stops(gradient, geometry.axis_length)?;
    let color_space = resolve_fixed_gradient_colors(&mut fixed_stops, gradient.interpolation);
    let domain_scale = if gradient.repeating {
        radial_gradient_paint_domain_scale(geometry, size)
    } else {
        1.0
    };
    let domain_length = geometry.axis_length * domain_scale;
    let program = resolve_gradient_program(
        fixed_stops,
        &gradient.hints,
        geometry.axis_length,
        gradient.repeating,
        gradient.interpolation,
    )?;
    let periodic = periodic_pdf_gradient(&program, domain_length);
    let stops = repeating_gradient_average_color(&program, domain_length).map_or_else(
        || {
            periodic
                .as_ref()
                .map(|periodic| periodic.stops.clone())
                .or_else(|| normalized_pdf_gradient_stops(&program, domain_length))
        },
        |color| {
            Some(vec![
                RenderedGradientStop {
                    offset: 0.0,
                    color,
                    interpolation_exponent: 1.0,
                },
                RenderedGradientStop {
                    offset: 1.0,
                    color,
                    interpolation_exponent: 1.0,
                },
            ])
        },
    )?;
    // A PDF radial shading has circular geometry. Scaling its local coordinate
    // system creates the CSS ellipse without rasterizing it.
    let transform = PaintTransform::scale(geometry.radii.width, geometry.radii.height);
    let center = PaintPoint::new(
        geometry.center.x / geometry.radii.width,
        geometry.center.y / geometry.radii.height,
    );
    Some(RenderedGradient {
        kind: RenderedGradientKind::Radial {
            start_center: center,
            start_radius: 0.0,
            end_center: center,
            end_radius: domain_scale,
        },
        color_space,
        stops,
        periodic,
        transform,
    })
}

/// Returns the CSS Images gradient-average color for a zero-period repeating
/// gradient. The average is the integral of premultiplied components across
/// the color line; with coincident stops CSS distributes them evenly first.
/// <https://www.w3.org/TR/css-images-3/#gradient-average-color>
const MAX_PDF_REPEATING_GRADIENT_STOPS: f32 = 4096.0;

/// Fixed CSS gradient color line shared by raster sampling and native-PDF
/// preparation. It preserves the repeated cycle rather than baking repetition
/// into image pixels or CSS-background tiles.
#[derive(Debug, Clone)]
struct ResolvedGradientProgram {
    stops: Vec<FixedGradientStop>,
    interval_exponents: Vec<f32>,
    repeat_period: Option<f32>,
    interpolation: css::GradientInterpolationMethod,
}

fn resolve_gradient_program(
    stops: Vec<FixedGradientStop>,
    hints: &[css::GradientColorHint],
    color_line_length: f32,
    repeating: bool,
    interpolation: css::GradientInterpolationMethod,
) -> Option<ResolvedGradientProgram> {
    let first = stops.first()?;
    let last = stops.last()?;
    let repeat_period = repeating.then_some(last.position - first.position);
    Some(ResolvedGradientProgram {
        interval_exponents: gradient_interval_exponents(&stops, hints, color_line_length),
        stops,
        repeat_period,
        interpolation,
    })
}

fn periodic_pdf_gradient(
    program: &ResolvedGradientProgram,
    domain_length: f32,
) -> Option<Box<crate::document::RenderedPeriodicGradient>> {
    let period = program.repeat_period?;
    if period <= 0.001 || repeating_gradient_average_color(program, domain_length).is_some() {
        return None;
    }
    Some(Box::new(crate::document::RenderedPeriodicGradient {
        stops: program
            .stops
            .iter()
            .enumerate()
            .map(|(index, stop)| RenderedGradientStop {
                offset: stop.position,
                color: stop.color,
                interpolation_exponent: program.interval_exponents[index],
            })
            .collect(),
        start: program.stops.first()?.position,
        period,
        domain_length,
    }))
}

fn repeating_gradient_average_color(
    program: &ResolvedGradientProgram,
    domain_length: f32,
) -> Option<CssColor> {
    if program.repeat_period.is_none() || program.stops.len() < 2 {
        return None;
    }
    let stops = &program.stops;
    let first = stops.first()?;
    let period = program.repeat_period?;
    let degenerate = period <= 0.001;
    let estimated_stop_count = if degenerate {
        f32::INFINITY
    } else {
        ((domain_length / period).ceil() + 3.0) * stops.len() as f32
    };
    if !degenerate && estimated_stop_count <= MAX_PDF_REPEATING_GRADIENT_STOPS {
        return None;
    }

    let total_length = if degenerate {
        (stops.len() - 1) as f32
    } else {
        period
    };
    let (red, green, blue, alpha) = stops.windows(2).enumerate().fold(
        (0.0, 0.0, 0.0, 0.0),
        |(red, green, blue, alpha), (index, pair)| {
            let length = if degenerate {
                1.0
            } else {
                pair[1].position - pair[0].position
            };
            // Integrate CSS's `t^N` transition-hint interpolation rather
            // than assuming a midpoint transition. The average is defined in
            // premultiplied color space by CSS Images 3.
            let progress_average = if degenerate {
                0.5
            } else {
                1.0 / (program.interval_exponents[index] + 1.0)
            };
            let weight = length / total_length;
            let interpolate =
                |start: f32, end: f32| (start + (end - start) * progress_average) * weight;
            (
                red + interpolate(
                    pair[0].color.components()[0] * pair[0].color.alpha(),
                    pair[1].color.components()[0] * pair[1].color.alpha(),
                ),
                green
                    + interpolate(
                        pair[0].color.components()[1] * pair[0].color.alpha(),
                        pair[1].color.components()[1] * pair[1].color.alpha(),
                    ),
                blue + interpolate(
                    pair[0].color.components()[2] * pair[0].color.alpha(),
                    pair[1].color.components()[2] * pair[1].color.alpha(),
                ),
                alpha + interpolate(pair[0].color.alpha(), pair[1].color.alpha()),
            )
        },
    );
    Some(if alpha <= 0.0 {
        CssColor::in_space(first.color.space(), 0.0, 0.0, 0.0, 0.0)
    } else {
        CssColor::in_space(
            first.color.space(),
            red / alpha,
            green / alpha,
            blue / alpha,
            alpha,
        )
    })
}

fn normalized_pdf_gradient_stops(
    program: &ResolvedGradientProgram,
    domain_length: f32,
) -> Option<Vec<RenderedGradientStop>> {
    let stops = &program.stops;
    let first = stops.first()?;
    let last = stops.last()?;
    if domain_length <= 0.0 {
        return None;
    }
    if program.repeat_period.is_none() {
        if (first.position).abs() > 0.001 || (last.position - domain_length).abs() > 0.001 {
            return None;
        }
        return Some(
            stops
                .iter()
                .enumerate()
                .map(|(index, stop)| RenderedGradientStop {
                    offset: (stop.position / domain_length).clamp(0.0, 1.0),
                    color: stop.color,
                    interpolation_exponent: program.interval_exponents[index],
                })
                .collect(),
        );
    }

    // CSS Images 3 repeats the fixed-up stop list in both directions. The
    // zero-period case was handled as a gradient-average color above.
    let period = program.repeat_period?;
    if period <= 0.001 {
        return None;
    }
    let mut rendered = Vec::new();
    let first_cycle = ((-first.position) / period).floor() as i32 - 1;
    let last_cycle = ((domain_length - first.position) / period).ceil() as i32 + 1;
    for cycle in first_cycle..=last_cycle {
        let shift = cycle as f32 * period;
        for (index, stop) in stops.iter().enumerate() {
            let position = stop.position + shift;
            if position < -0.001 || position > domain_length + 0.001 {
                continue;
            }
            let offset = (position / domain_length).clamp(0.0, 1.0);
            // Keep coincident boundary stops: their order encodes the sharp
            // transition required when a repeat's last and first colors differ.
            rendered.push(RenderedGradientStop {
                offset,
                color: stop.color,
                interpolation_exponent: program.interval_exponents[index],
            });
        }
    }
    rendered.sort_by(|left, right| left.offset.total_cmp(&right.offset));
    if rendered.len() < 2 {
        return None;
    }
    // The finite PDF function domain must have endpoint values. Insert the
    // sampled CSS color when a cycle has no stop exactly on an endpoint.
    if rendered.first().is_some_and(|stop| stop.offset > 0.001) {
        rendered.insert(
            0,
            RenderedGradientStop {
                offset: 0.0,
                color: sampled_gradient_program_color(program, 0.0),
                interpolation_exponent: 1.0,
            },
        );
    }
    if rendered.last().is_some_and(|stop| stop.offset < 0.999) {
        rendered.push(RenderedGradientStop {
            offset: 1.0,
            color: sampled_gradient_program_color(program, domain_length),
            interpolation_exponent: 1.0,
        });
    }
    Some(rendered)
}

fn gradient_interval_exponents(
    stops: &[FixedGradientStop],
    hints: &[css::GradientColorHint],
    color_line_length: f32,
) -> Vec<f32> {
    stops
        .windows(2)
        .enumerate()
        .map(|(index, pair)| {
            let Some(hint) = hints.iter().find(|hint| hint.after_stop == index) else {
                return 1.0;
            };
            let Some(position) = hint
                .position
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                    color_line_length,
                )))
                .map(layout_points)
                .or(Some(hint.position.length_points()))
            else {
                return 1.0;
            };
            let fraction = (position - pair[0].position) / (pair[1].position - pair[0].position);
            if fraction > 0.001 && fraction < 0.999 {
                (0.5_f32.ln() / fraction.ln()).clamp(0.01, 100.0)
            } else {
                1.0
            }
        })
        .chain(std::iter::once(1.0))
        .collect()
}

fn sampled_gradient_program_color(
    program: &ResolvedGradientProgram,
    mut position: f32,
) -> CssColor {
    let stops = &program.stops;
    if let Some(period) = program.repeat_period {
        if period <= 0.001 {
            return repeating_gradient_average_color(program, f32::INFINITY)
                .unwrap_or(stops.last().expect("non-empty stops").color);
        }
        position = (position - stops[0].position).rem_euclid(period) + stops[0].position;
    }
    if position <= stops[0].position {
        return stops[0].color;
    }
    for (index, pair) in stops.windows(2).enumerate() {
        if position <= pair[1].position {
            let span = pair[1].position - pair[0].position;
            if span <= 0.001 {
                return pair[1].color;
            }
            let progress = ((position - pair[0].position) / span)
                .clamp(0.0, 1.0)
                .powf(program.interval_exponents[index]);
            return crate::color::interpolate_color_with_missing(
                pair[0].color,
                pair[1].color,
                program.interpolation,
                progress,
                pair[0].missing_components.bits(),
                pair[1].missing_components.bits(),
            );
        }
    }
    stops.last().expect("non-empty stops").color
}

fn radial_gradient_paint_domain_scale(
    geometry: UsedRadialGradientGeometry,
    size: PaintSize,
) -> f32 {
    [
        PaintPoint::new(0.0, 0.0),
        PaintPoint::new(size.width, 0.0),
        PaintPoint::new(0.0, size.height),
        PaintPoint::new(size.width, size.height),
    ]
    .into_iter()
    .map(|point| radial_gradient_axis_position(point, geometry) / geometry.axis_length)
    .fold(1.0, f32::max)
}

/// Whether repeating this generated image through a PDF tiling pattern
/// preserves Quire's image-emission semantics.
///
/// A gradient with identical stop colors is spatially constant after CSS
/// Images color-stop fixup, regardless of its direction, hint, or stop
/// positions. It can therefore share the repeated pattern path. Raster URL
/// images deliberately remain individual placements: PDF viewers can sample
/// a tiling-pattern cell using the same local source rectangle and CSS tile
/// geometry as an individual placement. Reusing that cell keeps repeated
/// raster backgrounds bounded without changing their positioning or clip.
/// <https://www.w3.org/TR/css-images-3/#coloring-gradient-line>
fn background_image_can_use_pdf_pattern(image: &BackgroundImage) -> bool {
    let stops = match image.selected_image() {
        BackgroundImage::ImageSet { .. } => unreachable!("selected image-set source is unwrapped"),
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
        BackgroundImage::ImageSet { .. } => unreachable!("selected image-set source is unwrapped"),
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
        BackgroundImage::ImageSet { .. } => unreachable!("selected image-set source is unwrapped"),
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

fn used_background_layer_size(
    decoded: &DecodedPngImage,
    layer: &css::BackgroundLayer,
    positioning_area: PaintSize,
) -> PaintSize {
    let Some(image) = layer.image.as_image() else {
        return PaintSize::new(0.0, 0.0);
    };
    let selected_image = image.selected_image();
    let generated_image = matches!(
        selected_image,
        BackgroundImage::LinearGradient(_)
            | BackgroundImage::RadialGradient(_)
            | BackgroundImage::ConicGradient(_)
            | BackgroundImage::CssColor(_)
    );
    let mut size = if generated_image {
        used_generated_background_size(positioning_area, layer.size.clone())
    } else {
        used_background_size(
            decoded,
            positioning_area,
            layer.size.clone(),
            image.intrinsic_resolution(),
        )
    };

    let (width_is_auto, height_is_auto) = match &layer.size {
        css::BackgroundSize::Auto => (true, true),
        css::BackgroundSize::Explicit { width, height } => (
            matches!(width, css::BackgroundSizeAxis::Auto),
            matches!(height, css::BackgroundSizeAxis::Auto),
        ),
        css::BackgroundSize::Cover | css::BackgroundSize::Contain => (false, false),
    };
    let aspect_ratio = (!generated_image && decoded.pixel_height > 0)
        .then(|| decoded.pixel_width as f32 / decoded.pixel_height as f32);
    if layer.repeat.x_axis() == css::BackgroundRepeatAxis::Round {
        size.width = rounded_background_tile_size(size.width, positioning_area.width);
        if height_is_auto && let Some(aspect_ratio) = aspect_ratio {
            size.height = size.width / aspect_ratio;
        }
    }
    if layer.repeat.y_axis() == css::BackgroundRepeatAxis::Round {
        size.height = rounded_background_tile_size(size.height, positioning_area.height);
        if width_is_auto && let Some(aspect_ratio) = aspect_ratio {
            size.width = size.height * aspect_ratio;
        }
    }
    size
}

fn used_generated_background_layer_size(
    layer: &css::BackgroundLayer,
    positioning_area: PaintSize,
) -> PaintSize {
    let mut size = used_generated_background_size(positioning_area, layer.size.clone());
    if layer.repeat.x_axis() == css::BackgroundRepeatAxis::Round {
        size.width = rounded_background_tile_size(size.width, positioning_area.width);
    }
    if layer.repeat.y_axis() == css::BackgroundRepeatAxis::Round {
        size.height = rounded_background_tile_size(size.height, positioning_area.height);
    }
    size
}

fn rounded_background_tile_size(tile_size: f32, area_size: f32) -> f32 {
    if tile_size <= 0.0 || area_size <= 0.0 {
        return tile_size;
    }
    let count = (area_size / tile_size).round().max(1.0);
    area_size / count
}

fn used_generated_background_size(
    positioning_area: PaintSize,
    value: css::BackgroundSize,
) -> PaintSize {
    match value {
        css::BackgroundSize::Auto | css::BackgroundSize::Cover | css::BackgroundSize::Contain => {
            positioning_area
        }
        css::BackgroundSize::Explicit { width, height } => {
            let used_width = used_background_size_axis(width, positioning_area.width)
                .unwrap_or(positioning_area.width);
            let used_height = used_background_size_axis(height, positioning_area.height)
                .unwrap_or(positioning_area.height);
            PaintSize::new(used_width, used_height)
        }
    }
}

fn generated_linear_gradient_image(
    gradient: &css::LinearGradient,
    size: PaintSize,
    resource_cache: &ResourceCache,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    let gradient = gradient.resolve_current_color(current_color);
    let pixel_size = generated_image_pixel_size(size);
    (size.width > 0.0 && size.height > 0.0 && pixel_size.width > 0 && pixel_size.height > 0).then(
        || {
            let image_id = resource_cache.register_generated_image_recipe(
                crate::image_store::GeneratedRasterImage::Linear {
                    gradient,
                    size,
                    metadata: crate::image_store::ImageMetadata {
                        pixel_width: pixel_size.width,
                        pixel_height: pixel_size.height,
                    },
                },
            );
            DecodedPngImage {
                image_id: Some(image_id),
                pixel_width: pixel_size.width,
                pixel_height: pixel_size.height,
                rgb: EncodedRasterRgbSamples::from_shared(resource_cache.image_placeholder_rgb()),
                alpha: None,
                color_space: crate::color::RasterColorSpace::SRGB,
            }
        },
    )
}

fn generated_radial_gradient_image(
    gradient: &css::RadialGradient,
    size: PaintSize,
    resource_cache: &ResourceCache,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    let gradient = gradient.resolve_current_color(current_color);
    let pixel_size = generated_image_pixel_size(size);
    (size.width > 0.0 && size.height > 0.0 && pixel_size.width > 0 && pixel_size.height > 0).then(
        || {
            let image_id = resource_cache.register_generated_image_recipe(
                crate::image_store::GeneratedRasterImage::Radial {
                    gradient,
                    size,
                    metadata: crate::image_store::ImageMetadata {
                        pixel_width: pixel_size.width,
                        pixel_height: pixel_size.height,
                    },
                },
            );
            DecodedPngImage {
                image_id: Some(image_id),
                pixel_width: pixel_size.width,
                pixel_height: pixel_size.height,
                rgb: EncodedRasterRgbSamples::from_shared(resource_cache.image_placeholder_rgb()),
                alpha: None,
                color_space: crate::color::RasterColorSpace::SRGB,
            }
        },
    )
}

/// Rasterizes a CSS Images Level 3 linear gradient into a generated image.
///
/// Gradients are generated images with no intrinsic dimensions. The caller
/// supplies the concrete object size after CSS Backgrounds sizing, then this
/// samples the gradient in a resolved common CSS CssColor 4 space. CSS Images 3
/// stop positions, hints, and premultiplied-component interpolation are
/// otherwise unchanged:
/// <https://www.w3.org/TR/css-images-3/#coloring-gradient-line>.
pub(crate) fn rasterize_linear_gradient(
    gradient: &css::LinearGradient,
    size: PaintSize,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    let gradient = gradient.resolve_current_color(current_color);
    let width = size.width;
    let height = size.height;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let pixel_size = generated_image_pixel_size(size);
    let (pixel_width, pixel_height) = (pixel_size.width, pixel_size.height);
    if pixel_width == 0 || pixel_height == 0 {
        return None;
    }
    let area = paint_space_rect(0.0, 0.0, width, height);
    let line = angled_gradient_line(gradient.direction, area);
    let mut stops = fixed_gradient_stops(&gradient, line.axis_length)?;
    let color_space = resolve_fixed_gradient_colors(&mut stops, gradient.interpolation);
    let program = resolve_gradient_program(
        stops,
        &gradient.hints,
        line.axis_length,
        gradient.repeating,
        gradient.interpolation,
    )?;
    let mut rgb = Vec::with_capacity(pixel_width as usize * pixel_height as usize * 3);
    let mut alpha = Vec::with_capacity(pixel_width as usize * pixel_height as usize);
    let mut has_alpha = false;
    for row in 0..pixel_height {
        let y = height - ((row as f32 + 0.5) * height / pixel_height as f32);
        for column in 0..pixel_width {
            let x = (column as f32 + 0.5) * width / pixel_width as f32;
            let position = gradient_axis_position(PaintPoint::new(x, y), line);
            let color = sampled_gradient_program_color(&program, position);
            let a = (color.alpha() * 255.0).round().clamp(0.0, 255.0) as u8;
            rgb.push((color.components()[0] * 255.0).round().clamp(0.0, 255.0) as u8);
            rgb.push((color.components()[1] * 255.0).round().clamp(0.0, 255.0) as u8);
            rgb.push((color.components()[2] * 255.0).round().clamp(0.0, 255.0) as u8);
            alpha.push(a);
            has_alpha |= a < 255;
        }
    }
    Some(
        DecodedPngImage::new(pixel_width, pixel_height, rgb, has_alpha.then_some(alpha))
            .in_color_space(color_space),
    )
}

/// Rasterizes a CSS Images Level 3 radial gradient into a generated image.
///
/// Radial gradients are generated images with no intrinsic dimensions. The
/// concrete background tile size determines the center point, ending radii,
/// color-stop percentage basis, and repeating period:
/// <https://www.w3.org/TR/css-images-3/#radial-gradients>.
pub(crate) fn rasterize_radial_gradient(
    gradient: &css::RadialGradient,
    size: PaintSize,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    let gradient = gradient.resolve_current_color(current_color);
    let width = size.width;
    let height = size.height;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let pixel_size = generated_image_pixel_size(size);
    let (pixel_width, pixel_height) = (pixel_size.width, pixel_size.height);
    if pixel_width == 0 || pixel_height == 0 {
        return None;
    }
    let geometry = used_radial_gradient_geometry(&gradient, size)?;
    let mut stops = fixed_radial_gradient_stops(&gradient, geometry.axis_length)?;
    let color_space = resolve_fixed_gradient_colors(&mut stops, gradient.interpolation);
    let program = resolve_gradient_program(
        stops,
        &gradient.hints,
        geometry.axis_length,
        gradient.repeating,
        gradient.interpolation,
    )?;
    let mut rgb = Vec::with_capacity(pixel_width as usize * pixel_height as usize * 3);
    let mut alpha = Vec::with_capacity(pixel_width as usize * pixel_height as usize);
    let mut has_alpha = false;
    for row in 0..pixel_height {
        let y = height - ((row as f32 + 0.5) * height / pixel_height as f32);
        for column in 0..pixel_width {
            let x = (column as f32 + 0.5) * width / pixel_width as f32;
            let position = radial_gradient_axis_position(PaintPoint::new(x, y), geometry);
            let color = sampled_gradient_program_color(&program, position);
            let a = (color.alpha() * 255.0).round().clamp(0.0, 255.0) as u8;
            rgb.push((color.components()[0] * 255.0).round().clamp(0.0, 255.0) as u8);
            rgb.push((color.components()[1] * 255.0).round().clamp(0.0, 255.0) as u8);
            rgb.push((color.components()[2] * 255.0).round().clamp(0.0, 255.0) as u8);
            alpha.push(a);
            has_alpha |= a < 255;
        }
    }
    Some(
        DecodedPngImage::new(pixel_width, pixel_height, rgb, has_alpha.then_some(alpha))
            .in_color_space(color_space),
    )
}

/// Rasterize a CSS Images Level 4 conic gradient using its clockwise angular
/// color line. CSS zero degrees points toward the top of the gradient box;
/// increasing angles turn clockwise.
/// <https://drafts.csswg.org/css-images-4/#conic-gradients>
pub(crate) fn rasterize_conic_gradient(
    gradient: &css::ConicGradient,
    size: PaintSize,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    let gradient = gradient.resolve_current_color(current_color);
    let width = size.width;
    let height = size.height;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let pixel_size = generated_image_pixel_size(size);
    let (pixel_width, pixel_height) = (pixel_size.width, pixel_size.height);
    let mut stops = fixed_conic_gradient_stops(&gradient)?;
    let color_space = resolve_fixed_gradient_colors(&mut stops, gradient.interpolation);
    let center_x = used_background_position_axis(gradient.position.x.clone(), width, false);
    let center_y = used_background_position_axis(gradient.position.y.clone(), height, true);
    let mut rgb = Vec::with_capacity(pixel_width as usize * pixel_height as usize * 3);
    let mut alpha = Vec::with_capacity(pixel_width as usize * pixel_height as usize);
    let mut has_alpha = false;
    for row in 0..pixel_height {
        let y = height - ((row as f32 + 0.5) * height / pixel_height as f32);
        for column in 0..pixel_width {
            let x = (column as f32 + 0.5) * width / pixel_width as f32;
            let angle = ((x - center_x).atan2(y - center_y).to_degrees() - gradient.start_angle)
                .rem_euclid(360.0);
            let color = sampled_conic_gradient_color(
                gradient.repeating,
                &stops,
                angle,
                gradient.interpolation,
            );
            rgb.extend([
                (color.components()[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                (color.components()[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                (color.components()[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            ]);
            let opacity = (color.alpha() * 255.0).round().clamp(0.0, 255.0) as u8;
            alpha.push(opacity);
            has_alpha |= opacity < 255;
        }
    }
    Some(
        DecodedPngImage::new(pixel_width, pixel_height, rgb, has_alpha.then_some(alpha))
            .in_color_space(color_space),
    )
}

/// Apply the same common-space selection used by vector PDF gradients before
/// sampling generated CSS-gradient images.
fn gradient_interpolation_can_use_native_shading(image: &BackgroundImage) -> bool {
    let (method, has_missing_components) = match image {
        BackgroundImage::LinearGradient(gradient) => (
            gradient.interpolation,
            gradient.stops.iter().any(|stop| {
                !stop
                    .color
                    .missing_components_for(gradient.interpolation)
                    .is_empty()
            }),
        ),
        BackgroundImage::RadialGradient(gradient) => (
            gradient.interpolation,
            gradient.stops.iter().any(|stop| {
                !stop
                    .color
                    .missing_components_for(gradient.interpolation)
                    .is_empty()
            }),
        ),
        _ => return false,
    };
    // PDF Type 2 functions interpolate encoded components directly. That is
    // exact only for CSS's encoded rectangular spaces (and D50 XYZ); all
    // perceptual, polar, and linear-light methods must go through the shared
    // raster sampler.
    !has_missing_components
        && matches!(
            method.space,
            css::GradientInterpolationSpace::Srgb
                | css::GradientInterpolationSpace::DisplayP3
                | css::GradientInterpolationSpace::A98Rgb
                | css::GradientInterpolationSpace::ProphotoRgb
                | css::GradientInterpolationSpace::Rec2020
                | css::GradientInterpolationSpace::XyzD50
        )
}

fn gradient_interpolation_output_space(
    method: css::GradientInterpolationMethod,
) -> crate::css::CssColorSpace {
    match method.space {
        css::GradientInterpolationSpace::Srgb
        | css::GradientInterpolationSpace::SrgbLinear
        | css::GradientInterpolationSpace::Hsl
        | css::GradientInterpolationSpace::Hwb => crate::css::CssColorSpace::Srgb,
        css::GradientInterpolationSpace::DisplayP3
        | css::GradientInterpolationSpace::DisplayP3Linear => crate::css::CssColorSpace::DisplayP3,
        css::GradientInterpolationSpace::A98Rgb => crate::css::CssColorSpace::A98Rgb,
        css::GradientInterpolationSpace::ProphotoRgb => crate::css::CssColorSpace::ProphotoRgb,
        css::GradientInterpolationSpace::Rec2020 => crate::css::CssColorSpace::Rec2020,
        css::GradientInterpolationSpace::XyzD50
        | css::GradientInterpolationSpace::XyzD65
        | css::GradientInterpolationSpace::Lab
        | css::GradientInterpolationSpace::Oklab
        | css::GradientInterpolationSpace::Lch
        | css::GradientInterpolationSpace::Oklch => crate::css::CssColorSpace::XyzD50,
    }
}

fn resolve_fixed_gradient_colors(
    stops: &mut [FixedGradientStop],
    interpolation: css::GradientInterpolationMethod,
) -> crate::css::CssColorSpace {
    let space = gradient_interpolation_output_space(interpolation);
    if stops.iter().all(|stop| stop.color.space() == space) {
        return space;
    }
    if stops.iter_mut().all(|stop| {
        if let Some(color) = crate::color::convert_color(stop.color, space) {
            stop.color = color;
            true
        } else {
            false
        }
    }) {
        space
    } else {
        for stop in stops {
            stop.color =
                crate::css::color_to_predefined_rgb(stop.color, crate::css::CssColorSpace::Srgb)
                    .expect("sRGB is a predefined CSS RGB space");
        }
        crate::css::CssColorSpace::Srgb
    }
}

fn fixed_conic_gradient_stops(gradient: &css::ConicGradient) -> Option<Vec<FixedGradientStop>> {
    if gradient.stops.is_empty() {
        return None;
    }
    let mut positions = gradient
        .stops
        .iter()
        .map(|stop| stop.position)
        .collect::<Vec<_>>();
    positions[0].get_or_insert(0.0);
    let last = positions.len() - 1;
    positions[last].get_or_insert(360.0);
    let mut previous = positions[0]?;
    for position in positions.iter_mut().skip(1).flatten() {
        *position = position.max(previous);
        previous = *position;
    }
    let mut index = 0;
    while index < positions.len() {
        if positions[index].is_some() {
            index += 1;
            continue;
        }
        let start = index;
        while index < positions.len() && positions[index].is_none() {
            index += 1;
        }
        let before = positions[start - 1]?;
        let after = positions[index]?;
        let slots = (index - start + 1) as f32;
        for (offset, position) in positions[start..index].iter_mut().enumerate() {
            *position = Some(before + (after - before) * (offset + 1) as f32 / slots);
        }
    }
    gradient
        .stops
        .iter()
        .zip(positions)
        .map(|(stop, position)| {
            Some(FixedGradientStop {
                color: stop.color.as_color()?,
                missing_components: stop.color.missing_components_for(gradient.interpolation),
                position: position.unwrap_or(0.0),
            })
        })
        .collect()
}

fn sampled_conic_gradient_color(
    repeating: bool,
    stops: &[FixedGradientStop],
    mut position: f32,
    interpolation: css::GradientInterpolationMethod,
) -> CssColor {
    let first = stops.first().expect("non-empty conic stops");
    let last = stops.last().expect("non-empty conic stops");
    if repeating {
        let period = last.position - first.position;
        if period.abs() <= 0.001 {
            return last.color;
        }
        position = (position - first.position).rem_euclid(period) + first.position;
    }
    if position <= first.position {
        return first.color;
    }
    for pair in stops.windows(2) {
        if position <= pair[1].position {
            let span = pair[1].position - pair[0].position;
            if span.abs() <= 0.001 {
                return pair[1].color;
            }
            return crate::color::interpolate_color_with_missing(
                pair[0].color,
                pair[1].color,
                interpolation,
                (position - pair[0].position) / span,
                pair[0].missing_components.bits(),
                pair[1].missing_components.bits(),
            );
        }
    }
    last.color
}

#[derive(Debug, Clone, Copy)]
struct UsedRadialGradientGeometry {
    center: PaintPoint,
    radii: PaintSize,
    axis_length: f32,
}

fn used_radial_gradient_geometry(
    gradient: &css::RadialGradient,
    size: PaintSize,
) -> Option<UsedRadialGradientGeometry> {
    let width = size.width;
    let height = size.height;
    let center = PaintPoint::new(
        used_background_position_axis(gradient.position.x.clone(), width, false),
        used_background_position_axis(gradient.position.y.clone(), height, true),
    );
    let radii = match &gradient.size {
        css::RadialGradientSize::CircleRadius(radius) => {
            let radius = used_length_percentage(
                radius.clone(),
                PercentageBasis::definite(layout_pt(width.max(height).max(0.0))),
            )
            .points();
            PaintSize::new(radius, radius)
        }
        css::RadialGradientSize::EllipseRadii { x, y } => PaintSize::new(
            used_length_percentage(
                x.clone(),
                PercentageBasis::definite(layout_pt(width.max(0.0))),
            )
            .points(),
            used_length_percentage(
                y.clone(),
                PercentageBasis::definite(layout_pt(height.max(0.0))),
            )
            .points(),
        ),
        css::RadialGradientSize::Extent(extent) => {
            used_radial_gradient_extent_radii(gradient.shape, *extent, center, size)
        }
    };
    if radii.width <= 0.0 || radii.height <= 0.0 {
        return None;
    }
    Some(UsedRadialGradientGeometry {
        center,
        radii,
        axis_length: radii.width.max(radii.height),
    })
}

fn used_radial_gradient_extent_radii(
    shape: css::RadialGradientShape,
    extent: css::RadialGradientExtent,
    center: PaintPoint,
    size: PaintSize,
) -> PaintSize {
    let width = size.width;
    let height = size.height;
    let left = center.x.max(0.0);
    let right = (width - center.x).max(0.0);
    let bottom = center.y.max(0.0);
    let top = (height - center.y).max(0.0);
    match shape {
        css::RadialGradientShape::Circle => {
            let corners = [
                (left * left + bottom * bottom).sqrt(),
                (left * left + top * top).sqrt(),
                (right * right + bottom * bottom).sqrt(),
                (right * right + top * top).sqrt(),
            ];
            let radius = match extent {
                css::RadialGradientExtent::ClosestSide => left.min(right).min(bottom).min(top),
                css::RadialGradientExtent::FarthestSide => left.max(right).max(bottom).max(top),
                css::RadialGradientExtent::ClosestCorner => {
                    corners.into_iter().fold(f32::INFINITY, f32::min)
                }
                css::RadialGradientExtent::FarthestCorner => {
                    corners.into_iter().fold(0.0, f32::max)
                }
            };
            PaintSize::new(radius, radius)
        }
        css::RadialGradientShape::Ellipse => {
            let side_radii = match extent {
                css::RadialGradientExtent::ClosestSide
                | css::RadialGradientExtent::ClosestCorner => {
                    PaintSize::new(left.min(right), bottom.min(top))
                }
                css::RadialGradientExtent::FarthestSide
                | css::RadialGradientExtent::FarthestCorner => {
                    PaintSize::new(left.max(right), bottom.max(top))
                }
            };
            if matches!(
                extent,
                css::RadialGradientExtent::ClosestSide | css::RadialGradientExtent::FarthestSide
            ) {
                return side_radii;
            }
            scaled_ellipse_corner_radii(side_radii, extent, left, right, bottom, top)
        }
    }
}

fn scaled_ellipse_corner_radii(
    radii: PaintSize,
    extent: css::RadialGradientExtent,
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
) -> PaintSize {
    if radii.width <= 0.0 || radii.height <= 0.0 {
        return radii;
    }
    let corner_scales = [
        ((left / radii.width).powi(2) + (bottom / radii.height).powi(2)).sqrt(),
        ((left / radii.width).powi(2) + (top / radii.height).powi(2)).sqrt(),
        ((right / radii.width).powi(2) + (bottom / radii.height).powi(2)).sqrt(),
        ((right / radii.width).powi(2) + (top / radii.height).powi(2)).sqrt(),
    ];
    let scale = match extent {
        css::RadialGradientExtent::ClosestCorner => {
            corner_scales.into_iter().fold(f32::INFINITY, f32::min)
        }
        css::RadialGradientExtent::FarthestCorner => corner_scales.into_iter().fold(0.0, f32::max),
        css::RadialGradientExtent::ClosestSide | css::RadialGradientExtent::FarthestSide => 1.0,
    };
    PaintSize::new(radii.width * scale, radii.height * scale)
}

fn radial_gradient_axis_position(point: PaintPoint, geometry: UsedRadialGradientGeometry) -> f32 {
    let dx = (point.x - geometry.center.x) / geometry.radii.width;
    let dy = (point.y - geometry.center.y) / geometry.radii.height;
    (dx * dx + dy * dy).sqrt() * geometry.axis_length
}

fn fixed_radial_gradient_stops(
    gradient: &css::RadialGradient,
    axis_length: f32,
) -> Option<Vec<FixedGradientStop>> {
    fixed_gradient_stops_from_color_stops(&gradient.stops, axis_length, gradient.interpolation)
}

fn fixed_gradient_stops_from_color_stops(
    stops: &[css::GradientColorStop],
    axis_length: f32,
    interpolation: css::GradientInterpolationMethod,
) -> Option<Vec<FixedGradientStop>> {
    if axis_length <= 0.0 || stops.len() < 2 {
        return None;
    }
    let mut positions = stops
        .iter()
        .map(|stop| {
            stop.position
                .as_ref()
                .and_then(|position| {
                    position
                        .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                            axis_length,
                        )))
                        .map(layout_points)
                })
                .or_else(|| {
                    stop.position
                        .as_ref()
                        .map(|position| position.length_points())
                })
        })
        .collect::<Vec<_>>();
    positions[0].get_or_insert(0.0);
    let last_index = positions.len() - 1;
    positions[last_index].get_or_insert(axis_length);

    let mut previous = positions[0].expect("defaulted first stop");
    for position in positions.iter_mut().skip(1).flatten() {
        if *position < previous {
            *position = previous;
        }
        previous = *position;
    }

    let mut index = 0usize;
    while index < positions.len() {
        if positions[index].is_some() {
            index += 1;
            continue;
        }
        let run_start = index;
        while index < positions.len() && positions[index].is_none() {
            index += 1;
        }
        let before = positions[run_start - 1].expect("first stop defaulted");
        let after = positions[index].expect("last stop defaulted");
        let slots = (index - run_start + 1) as f32;
        for (offset, position) in positions[run_start..index].iter_mut().enumerate() {
            let step = (offset + 1) as f32 / slots;
            *position = Some(before + (after - before) * step);
        }
    }

    stops
        .iter()
        .zip(positions)
        .map(|(stop, position)| {
            Some(FixedGradientStop {
                color: stop.color.as_color()?,
                missing_components: stop.color.missing_components_for(interpolation),
                position: position.expect("all positions fixed up"),
            })
        })
        .collect()
}

fn generated_image_pixel_size(size: PaintSize) -> RasterPixelSize {
    const PIXELS_PER_PT: f32 = 2.0;
    const MAX_EDGE: f32 = 4096.0;
    let mut pixel_width = (size.width * PIXELS_PER_PT).ceil().max(1.0);
    let mut pixel_height = (size.height * PIXELS_PER_PT).ceil().max(1.0);
    let scale = (MAX_EDGE / pixel_width.max(pixel_height)).min(1.0);
    pixel_width = (pixel_width * scale).ceil().max(1.0);
    pixel_height = (pixel_height * scale).ceil().max(1.0);
    RasterPixelSize::new(pixel_width as u32, pixel_height as u32)
}

pub(in crate::layout) fn background_layers_for_paint(
    style: &ComputedStyle,
) -> Vec<css::BackgroundLayer> {
    if !style.background_layers.is_empty() {
        return style.background_layers.clone();
    }
    vec![css::BackgroundLayer {
        image: style.background_image.clone(),
        position: style.background_position.clone(),
        size: style.background_size.clone(),
        repeat: style.background_repeat,
        attachment: style.background_attachment,
        origin: style.background_origin,
        clip: style.background_clip,
    }]
}

pub(in crate::layout) fn background_paint_area_for_box<Space>(
    area: BackgroundArea<Space>,
    style: &ComputedStyle,
    box_: css::BackgroundBox,
) -> BackgroundArea<Space> {
    let border = used_border_widths(style);
    match box_ {
        css::BackgroundBox::Border | css::BackgroundBox::BorderArea => area,
        css::BackgroundBox::Padding => area.inset(border),
        css::BackgroundBox::Content => area.inset(border).inset(style.padding),
    }
}

/// Resolve the background positioning area for one layer.
///
/// `background-origin` selects an element box for scroll/local layers. A
/// fixed layer instead uses the viewport-equivalent area supplied by layout,
/// so its origin keyword has no effect.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin> and
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-attachment>
fn background_positioning_area_for_layer<Space>(
    positioning_border_area: BackgroundArea<Space>,
    fixed_positioning_area: Option<BackgroundArea<Space>>,
    fixed_attachment_is_scrolled_by_transform: bool,
    style: &ComputedStyle,
    layer: &css::BackgroundLayer,
) -> BackgroundArea<Space> {
    match (layer.attachment, fixed_positioning_area) {
        // A transform turns a non-root fixed background into a scroll
        // background. Its image is part of the transformed element's paint
        // subtree rather than a viewport-fixed source.
        // <https://drafts.csswg.org/css-transforms-1/#transform-rendering>
        (css::BackgroundAttachment::Fixed, _) if fixed_attachment_is_scrolled_by_transform => {
            background_paint_area_for_box(positioning_border_area, style, layer.origin)
        }
        (css::BackgroundAttachment::Fixed, Some(area)) => area,
        _ => background_paint_area_for_box(positioning_border_area, style, layer.origin),
    }
}

/// Returns whether painting a border-box-clipped background below the border
/// cannot affect the composited result.
///
/// A nonzero, opaque, square `solid` border completely covers the border area,
/// so CSS Backgrounds and Borders' normal painting order makes a background
/// below it unobservable. Restricting the emitted background to the padding
/// box is therefore an equivalent paint elimination; it also avoids PDF
/// rasterizers exposing a subpixel seam between two otherwise opaque paints.
///
/// <https://www.w3.org/TR/css-backgrounds-3/#layering> and
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-clip>.
fn background_border_box_paint_is_occluded(
    style: &ComputedStyle,
    clip_box: css::BackgroundBox,
) -> bool {
    if clip_box != css::BackgroundBox::Border
        || !style.border_radius.clone().is_zero()
        || style.border_image.source.is_image()
    {
        return false;
    }

    let widths = used_border_widths(style);
    let styles = style.border_styles;
    let colors = style.border_colors;
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

/// Clip a raster background image to its destination-space paint area.
///
/// A CSS clip constrains the image's output; it must not change the mapping
/// between source pixels and its original destination tile. In particular,
/// converting a fractional source-pixel edge to an integer PDF source rect
/// would rescale the retained pixels. Keep the image geometry intact and
/// express any partial tile through a PDF destination clip instead.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-clip>
fn clip_background_image_to_paint_area(
    mut image: RenderedImage,
    clip: PaintBackgroundArea,
    rounded_clip: Option<RenderedPathClip>,
) -> Option<RenderedImage> {
    let image_rect = image.paint_rect();
    let visible = image_rect.intersection(&clip.paint_rect())?;
    let clip = if visible != image_rect {
        let mut rectangular_clip = RenderedPathClip::new(
            paint_rect_path_commands(visible),
            RenderedPathFillRule::NonZero,
            Vec::new(),
        );
        if let Some(rounded_clip) = rounded_clip {
            rectangular_clip
                .additional_clips
                .push(RenderedPathClipPath::new(
                    rounded_clip.commands,
                    rounded_clip.fill_rule,
                ));
            rectangular_clip
                .additional_clips
                .extend(rounded_clip.additional_clips);
        }
        Some(rectangular_clip)
    } else {
        rounded_clip
    };
    if let Some(clip) = clip {
        image = image.with_clip(clip);
    }
    Some(image)
}

pub(in crate::layout) fn clear_position_insets(style: &mut ComputedStyle) {
    clear_style_insets(style);
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct RenderedImageTileRect {
    /// Destination tile region in page-local CSS paint coordinates.
    ///
    /// CSS Backgrounds and Borders slices `border-image` into destination
    /// regions that are painted into the border-image area. At this stage the
    /// layout box has already been projected into paint space, so the rectangle
    /// uses the same bottom-left-origin coordinate system as rendered images:
    /// <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>.
    pub(in crate::layout) rect: PaintRect,
}

impl RenderedImageTileRect {
    pub(in crate::layout) fn from_paint_rect(rect: PaintRect) -> Self {
        Self { rect }
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.rect.origin.x
    }

    pub(in crate::layout) fn y(self) -> f32 {
        self.rect.origin.y
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.rect.size.width
    }

    pub(in crate::layout) fn height(self) -> f32 {
        self.rect.size.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BorderImageTileSegment {
    pub(in crate::layout) destination_offset: f32,
    pub(in crate::layout) destination_size: f32,
    pub(in crate::layout) source_offset: u32,
    pub(in crate::layout) source_size: u32,
}

/// Emits the repeated image tiles for one border-image slice region.
///
/// CSS Backgrounds and Borders Level 3 applies `border-image-repeat` after the
/// source image has been sliced into a 3x3 grid. Corners are stretched, edge
/// regions repeat only along their long axis, and the optional center region
/// repeats on both axes:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>.
pub(in crate::layout) fn push_border_image_tiles(
    images: &mut Vec<RenderedImage>,
    decoded: &DecodedPngImage,
    destination: RenderedImageTileRect,
    source: RenderedImageSourceRect,
    repeat_x: css::BorderImageRepeatKeyword,
    repeat_y: css::BorderImageRepeatKeyword,
    interpolate: bool,
) {
    let tile_size = border_image_base_tile_size(destination, source, repeat_x, repeat_y);
    let x_segments =
        border_image_tile_segments(repeat_x, destination.width(), tile_size.width, source.width);
    let y_segments = border_image_tile_segments(
        repeat_y,
        destination.height(),
        tile_size.height,
        source.height,
    );
    for y_segment in &y_segments {
        for x_segment in &x_segments {
            if x_segment.destination_size <= 0.0
                || y_segment.destination_size <= 0.0
                || x_segment.source_size == 0
                || y_segment.source_size == 0
            {
                continue;
            }
            images.push(
                RenderedImage::from_paint_rect(
                    paint_space_rect(
                        destination.x() + x_segment.destination_offset,
                        destination.y() + y_segment.destination_offset,
                        x_segment.destination_size,
                        y_segment.destination_size,
                    ),
                    true,
                    decoded.pixel_width,
                    decoded.pixel_height,
                    Some(RenderedImageSourceRect {
                        x: source.x + x_segment.source_offset,
                        y: source.y + y_segment.source_offset,
                        width: x_segment.source_size,
                        height: y_segment.source_size,
                    }),
                    interpolate,
                    decoded.rgb.shared(),
                    decoded.alpha.clone(),
                    None,
                )
                .with_raster_color_space(decoded.color_space.clone())
                .with_image_id(decoded.image_id),
            );
        }
    }
}

pub(in crate::layout) fn border_image_base_tile_size(
    destination: RenderedImageTileRect,
    source: RenderedImageSourceRect,
    repeat_x: css::BorderImageRepeatKeyword,
    repeat_y: css::BorderImageRepeatKeyword,
) -> PaintSize {
    // Raster-image dimensions are CSS pixels, while the destination is laid
    // out in PDF points. Convert before deriving cross-axis tile scaling.
    let mut tile_width = source.width as f32 * css::CSS_PX_TO_PT;
    let mut tile_height = source.height as f32 * css::CSS_PX_TO_PT;
    if repeat_x != css::BorderImageRepeatKeyword::Stretch
        && repeat_y == css::BorderImageRepeatKeyword::Stretch
        && source.height > 0
    {
        let scale = destination.height() / (source.height as f32 * css::CSS_PX_TO_PT);
        tile_width *= scale;
    }
    if repeat_y != css::BorderImageRepeatKeyword::Stretch
        && repeat_x == css::BorderImageRepeatKeyword::Stretch
        && source.width > 0
    {
        let scale = destination.width() / (source.width as f32 * css::CSS_PX_TO_PT);
        tile_height *= scale;
    }
    if repeat_x == css::BorderImageRepeatKeyword::Stretch {
        tile_width = destination.width();
    }
    if repeat_y == css::BorderImageRepeatKeyword::Stretch {
        tile_height = destination.height();
    }
    PaintSize::new(tile_width.max(0.0), tile_height.max(0.0))
}

/// Computes destination/source segments for one `border-image-repeat` axis.
///
/// The CSS border-image process defines four repeat modes: `stretch` scales one
/// image to the region, `repeat` clips repeated tiles at the ends, `round`
/// adjusts the tile size to fit an integer number of tiles, and `space`
/// distributes whole tiles with gaps:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-repeat>.
pub(in crate::layout) fn border_image_tile_segments(
    repeat: css::BorderImageRepeatKeyword,
    destination_size: f32,
    base_tile_size: f32,
    source_size: u32,
) -> Vec<BorderImageTileSegment> {
    if destination_size <= 0.0 || source_size == 0 {
        return Vec::new();
    }
    if repeat == css::BorderImageRepeatKeyword::Stretch || base_tile_size <= 0.0 {
        return vec![BorderImageTileSegment {
            destination_offset: 0.0,
            destination_size,
            source_offset: 0,
            source_size,
        }];
    }
    match repeat {
        css::BorderImageRepeatKeyword::Repeat => {
            repeat_border_image_tile_segments(destination_size, base_tile_size, source_size)
        }
        css::BorderImageRepeatKeyword::Round => {
            let count = (destination_size / base_tile_size).round().max(1.0) as usize;
            let tile_size = destination_size / count as f32;
            (0..count)
                .map(|index| BorderImageTileSegment {
                    destination_offset: index as f32 * tile_size,
                    destination_size: tile_size,
                    source_offset: 0,
                    source_size,
                })
                .collect()
        }
        css::BorderImageRepeatKeyword::Space => {
            let count = (destination_size / base_tile_size).floor() as usize;
            let count = count.max(1);
            let tile_size = base_tile_size.min(destination_size);
            // Border-image `space` distributes its leftover space around the
            // complete tiles: before the first tile, between every pair, and
            // after the last. This is intentionally different from
            // background-repeat: space, whose outer edges have no gap.
            // <https://www.w3.org/TR/css-backgrounds-3/#border-image-repeat>
            let spacing = (destination_size - tile_size * count as f32) / (count + 1) as f32;
            (0..count)
                .map(|index| BorderImageTileSegment {
                    destination_offset: spacing + index as f32 * (tile_size + spacing),
                    destination_size: tile_size,
                    source_offset: 0,
                    source_size,
                })
                .collect()
        }
        css::BorderImageRepeatKeyword::Stretch => unreachable!(),
    }
}

pub(in crate::layout) fn repeat_border_image_tile_segments(
    destination_size: f32,
    tile_size: f32,
    source_size: u32,
) -> Vec<BorderImageTileSegment> {
    if destination_size <= 0.0 || tile_size <= 0.0 || source_size == 0 {
        return Vec::new();
    }

    // `repeat` centers the integer sequence of whole tiles in the edge
    // region, then clips the equal overhang at either end. Starting at the
    // leading edge instead selects a different part of a source tile whenever
    // the region is not an exact multiple (including the common one-tile
    // case). CSS Border Images describes the centered, symmetrically clipped
    // placement in its border-image process:
    // <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>.
    let tile_count = (destination_size / tile_size).ceil().max(1.0) as usize;
    let sequence_size = tile_count as f32 * tile_size;
    let first_offset = (destination_size - sequence_size) / 2.0;
    let mut segments = Vec::with_capacity(tile_count);

    for index in 0..tile_count {
        let tile_start = first_offset + index as f32 * tile_size;
        let tile_end = tile_start + tile_size;
        let visible_start = tile_start.max(0.0);
        let visible_end = tile_end.min(destination_size);
        if visible_end <= visible_start {
            continue;
        }
        let source_start = ((visible_start - tile_start) * source_size as f32 / tile_size)
            .round()
            .clamp(0.0, source_size as f32) as u32;
        let source_end = ((visible_end - tile_start) * source_size as f32 / tile_size)
            .round()
            .clamp(source_start as f32, source_size as f32) as u32;
        if source_end <= source_start {
            continue;
        }
        segments.push(BorderImageTileSegment {
            destination_offset: visible_start,
            destination_size: visible_end - visible_start,
            source_offset: source_start,
            source_size: source_end - source_start,
        });
    }
    segments
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedAxis {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) size: f32,
    pub(in crate::layout) margin_start: f32,
    pub(in crate::layout) margin_end: f32,
}

impl PositionedAxis {
    pub(in crate::layout) fn new(
        start: f32,
        size: f32,
        margin_start: f32,
        margin_end: f32,
    ) -> Self {
        Self {
            start,
            size,
            margin_start,
            margin_end,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum AbsoluteAxisDirection {
    HorizontalLtr,
    HorizontalRtl,
    Vertical,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AbsoluteDefiniteAxis {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) size: f32,
    pub(in crate::layout) end: f32,
    pub(in crate::layout) margin_start: f32,
    pub(in crate::layout) margin_end: f32,
    pub(in crate::layout) non_content: f32,
    pub(in crate::layout) containing_size: f32,
}

/// Resolve auto margins for a fully definite absolutely positioned axis.
///
/// CSS 2.2 defines absolute-position sizing by a constraint equation over
/// start inset, margins, padding, borders, content size, and end inset. Auto
/// margins remain zero for the other non-replaced absolute-position cases, but
/// when both insets and the used size are definite, auto margins absorb the
/// equation's remaining space before overconstraint handling:
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width> and
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height>.
pub(in crate::layout) fn resolve_absolute_definite_axis_auto_margins(
    start_auto: bool,
    end_auto: bool,
    axis: AbsoluteDefiniteAxis,
    direction: AbsoluteAxisDirection,
) -> PositionedAxis {
    let remaining = axis.containing_size
        - axis.start
        - axis.margin_start
        - axis.non_content
        - axis.size
        - axis.margin_end
        - axis.end;

    match (start_auto, end_auto) {
        (true, true) => {
            if matches!(direction, AbsoluteAxisDirection::HorizontalLtr) && remaining < 0.0 {
                return PositionedAxis::new(axis.start, axis.size, 0.0, remaining);
            }
            if matches!(direction, AbsoluteAxisDirection::HorizontalRtl) && remaining < 0.0 {
                return PositionedAxis::new(axis.start, axis.size, remaining, 0.0);
            }
            PositionedAxis::new(
                axis.start,
                axis.size,
                axis.margin_start + remaining / 2.0,
                axis.margin_end + remaining / 2.0,
            )
        }
        (true, false) => PositionedAxis::new(
            axis.start,
            axis.size,
            axis.margin_start + remaining,
            axis.margin_end,
        ),
        (false, true) => PositionedAxis::new(
            axis.start,
            axis.size,
            axis.margin_start,
            axis.margin_end + remaining,
        ),
        (false, false) => match direction {
            AbsoluteAxisDirection::HorizontalRtl => PositionedAxis::new(
                axis.containing_size
                    - axis.end
                    - axis.margin_start
                    - axis.margin_end
                    - axis.non_content
                    - axis.size,
                axis.size,
                axis.margin_start,
                axis.margin_end,
            ),
            AbsoluteAxisDirection::HorizontalLtr | AbsoluteAxisDirection::Vertical => {
                PositionedAxis::new(axis.start, axis.size, axis.margin_start, axis.margin_end)
            }
        },
    }
}

/// Returns tile origins that intersect a background positioning area.
///
/// CSS Backgrounds and Borders repeats from the positioned first tile in both
/// directions as needed, but PDF emission needs a finite set of image
/// placements for the current painted area:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
pub(in crate::layout) fn background_tile_positions(
    positioned_start: f32,
    area_start: f32,
    area_size: f32,
    tile_size: f32,
    repeat: css::BackgroundRepeatAxis,
) -> Vec<f32> {
    if tile_size <= 0.0 {
        return Vec::new();
    }
    match repeat {
        css::BackgroundRepeatAxis::NoRepeat => return vec![positioned_start],
        css::BackgroundRepeatAxis::Space => {
            let count = spaced_background_tile_count(tile_size, area_size);
            if count < 2 {
                return vec![positioned_start];
            }
            let step = spaced_background_tile_step(tile_size, area_size, count);
            return (0..count)
                .map(|index| area_start + index as f32 * step)
                .collect();
        }
        css::BackgroundRepeatAxis::Repeat | css::BackgroundRepeatAxis::Round => {}
    }
    if area_size <= 0.0 {
        return Vec::new();
    }

    let area_end = area_start + area_size;
    let mut first = positioned_start;
    while first > area_start {
        first -= tile_size;
    }
    while first + tile_size <= area_start {
        first += tile_size;
    }

    let mut positions = Vec::new();
    let mut current = first;
    while current < area_end {
        positions.push(current);
        current += tile_size;
    }
    positions
}

fn background_first_tile_position(
    positioned_start: f32,
    area_start: f32,
    area_size: f32,
    tile_size: f32,
    repeat: css::BackgroundRepeatAxis,
) -> f32 {
    match repeat {
        css::BackgroundRepeatAxis::NoRepeat => return positioned_start,
        css::BackgroundRepeatAxis::Space => {
            if spaced_background_tile_count(tile_size, area_size) >= 2 {
                return area_start;
            }
            return positioned_start;
        }
        css::BackgroundRepeatAxis::Repeat | css::BackgroundRepeatAxis::Round => {}
    }
    if tile_size <= 0.0 {
        return positioned_start;
    }
    let mut first = positioned_start;
    while first > area_start {
        first -= tile_size;
    }
    while first + tile_size <= area_start {
        first += tile_size;
    }
    first
}

fn background_pattern_step(
    tile_size: f32,
    area_size: f32,
    repeat: css::BackgroundRepeatAxis,
) -> f32 {
    match repeat {
        css::BackgroundRepeatAxis::Repeat | css::BackgroundRepeatAxis::Round => tile_size,
        css::BackgroundRepeatAxis::Space => {
            let count = spaced_background_tile_count(tile_size, area_size);
            if count >= 2 {
                spaced_background_tile_step(tile_size, area_size, count)
            } else {
                non_repeating_pattern_step(tile_size, area_size)
            }
        }
        css::BackgroundRepeatAxis::NoRepeat => non_repeating_pattern_step(tile_size, area_size),
    }
}

fn spaced_background_tile_count(tile_size: f32, area_size: f32) -> usize {
    if tile_size <= 0.0 || area_size <= 0.0 {
        return 0;
    }
    (area_size / tile_size).floor().max(1.0) as usize
}

fn spaced_background_tile_step(tile_size: f32, area_size: f32, count: usize) -> f32 {
    if count < 2 {
        return non_repeating_pattern_step(tile_size, area_size);
    }
    (area_size - tile_size) / (count - 1) as f32
}

fn non_repeating_pattern_step(tile_size: f32, area_size: f32) -> f32 {
    tile_size.max(area_size.abs() * 2.0 + tile_size)
}

#[cfg(test)]
pub(in crate::layout) fn resolve_absolute_horizontal(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_or_intrinsic_width: f32,
    static_position: StaticHorizontalPosition,
    containing_direction: Direction,
) -> PositionedAxis {
    resolve_absolute_horizontal_with_non_content(
        style,
        containing_block,
        auto_or_intrinsic_width,
        None,
        static_position,
        containing_direction,
        style.padding.left + style.padding.right + horizontal_border_width(style),
    )
}

/// Resolve the horizontal absolute-position equation with the used box-model
/// inset supplied by the formatting context.
///
/// Ordinary boxes use padding plus border widths. Collapsed tables supply
/// zero because their resolved edge borders belong to the grid rather than to
/// the table wrapper's CSS sizing conversion.
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width>
/// <https://www.w3.org/TR/css-tables-3/#table-wrapper-box>
///
/// `containing_direction` names the physical start side of the containing
/// block's horizontal axis. In a vertical writing mode that axis is the
/// logical block axis, so it is determined by `vertical-rl` versus
/// `vertical-lr`, not the inline `direction` value.
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
pub(in crate::layout) fn resolve_absolute_horizontal_with_non_content(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_or_intrinsic_width: f32,
    automatic_minimum_width: Option<f32>,
    static_position: StaticHorizontalPosition,
    containing_direction: Direction,
    horizontal_non_content: f32,
) -> PositionedAxis {
    // CSS 2.2 10.3.7, non-replaced absolutely positioned elements. The
    // static position has separate physical left and right distances; RTL
    // static-position containing blocks seed auto horizontal positioning from
    // the static right side before solving for the used left.
    let left = used_inset_left(style, containing_block);
    let right = used_inset_right(style, containing_block);
    let width = used_content_box_width_or_auto(
        style,
        layout_pt(containing_block.width()),
        non_content_pt(horizontal_non_content),
    )
    .or_else(|| {
        matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        )
        .then_some(content_box_pt(auto_or_intrinsic_width))
    })
    .map(|width| {
        constrain_content_width(
            style,
            width,
            PercentageBasis::definite(layout_pt(containing_block.width())),
        )
        .points()
    })
    .map(|width| automatic_minimum_width.map_or(width, |minimum| width.max(minimum)));
    let shrink_to_fit_width = constrain_content_width(
        style,
        content_box_pt(auto_or_intrinsic_width),
        PercentageBasis::definite(layout_pt(containing_block.width())),
    )
    .points();
    let static_left = if static_position.can_fall_outside {
        static_position.left
    } else {
        static_position.left.clamp(0.0, containing_block.width())
    };
    let static_right = if static_position.can_fall_outside {
        static_position.right
    } else {
        static_position.right.clamp(0.0, containing_block.width())
    };
    let margin_start = style.margin.left;
    let margin_end = style.margin.right;
    let non_content = horizontal_non_content;
    let fill_between = |start: f32, end: f32| {
        (containing_block.width() - start - margin_start - non_content - margin_end - end).max(0.0)
    };
    let border_box_size = |content_size: f32| content_size + non_content;
    let start_for_end = |content_size: f32, end: f32| {
        containing_block.width() - end - margin_start - margin_end - border_box_size(content_size)
    };

    match (left, width, right) {
        (Some(start), Some(size), Some(end)) => match containing_direction {
            Direction::Ltr => resolve_absolute_definite_axis_auto_margins(
                style.box_values.margin.left.is_auto(),
                style.box_values.margin.right.is_auto(),
                AbsoluteDefiniteAxis {
                    start,
                    size,
                    end,
                    margin_start,
                    margin_end,
                    non_content,
                    containing_size: containing_block.width(),
                },
                AbsoluteAxisDirection::HorizontalLtr,
            ),
            Direction::Rtl => resolve_absolute_definite_axis_auto_margins(
                style.box_values.margin.left.is_auto(),
                style.box_values.margin.right.is_auto(),
                AbsoluteDefiniteAxis {
                    start,
                    size,
                    end,
                    margin_start,
                    margin_end,
                    non_content,
                    containing_size: containing_block.width(),
                },
                AbsoluteAxisDirection::HorizontalRtl,
            ),
        },
        (Some(start), Some(size), None) => {
            PositionedAxis::new(start, size, margin_start, margin_end)
        }
        (Some(start), None, Some(end)) if style.display.is_table() => {
            let axis = AbsoluteDefiniteAxis {
                start,
                size: shrink_to_fit_width,
                end,
                margin_start,
                margin_end,
                non_content,
                containing_size: containing_block.width(),
            };
            match containing_direction {
                Direction::Ltr => resolve_absolute_definite_axis_auto_margins(
                    style.box_values.margin.left.is_auto(),
                    style.box_values.margin.right.is_auto(),
                    axis,
                    AbsoluteAxisDirection::HorizontalLtr,
                ),
                Direction::Rtl => resolve_absolute_definite_axis_auto_margins(
                    style.box_values.margin.left.is_auto(),
                    style.box_values.margin.right.is_auto(),
                    axis,
                    AbsoluteAxisDirection::HorizontalRtl,
                ),
            }
        }
        (Some(start), None, Some(end)) => PositionedAxis::new(
            start,
            constrain_content_width(
                style,
                content_box_pt(fill_between(start, end)),
                PercentageBasis::definite(layout_pt(containing_block.width())),
            )
            .points(),
            margin_start,
            margin_end,
        ),
        (Some(start), None, None) => {
            PositionedAxis::new(start, shrink_to_fit_width, margin_start, margin_end)
        }
        (None, Some(size), Some(end)) => {
            PositionedAxis::new(start_for_end(size, end), size, margin_start, margin_end)
        }
        (None, Some(size), None) => match containing_direction {
            Direction::Ltr => PositionedAxis::new(static_left, size, margin_start, margin_end),
            Direction::Rtl => PositionedAxis::new(
                start_for_end(size, static_right),
                size,
                margin_start,
                margin_end,
            ),
        },
        (None, None, Some(end)) => PositionedAxis::new(
            start_for_end(shrink_to_fit_width, end),
            shrink_to_fit_width,
            margin_start,
            margin_end,
        ),
        (None, None, None) => match containing_direction {
            Direction::Ltr => {
                PositionedAxis::new(static_left, shrink_to_fit_width, margin_start, margin_end)
            }
            Direction::Rtl => PositionedAxis::new(
                start_for_end(shrink_to_fit_width, static_right),
                shrink_to_fit_width,
                margin_start,
                margin_end,
            ),
        },
    }
}

/// Return the start direction for physical horizontal inset equations.
///
/// CSS `left` and `right` are physical, while `direction` reverses only a
/// horizontal writing mode's inline axis. Vertical writing modes project the
/// logical block axis onto physical horizontal, whose start side is fixed by
/// the writing mode.
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
pub(in crate::layout) fn physical_horizontal_axis_direction(
    writing_mode: WritingMode,
    direction: Direction,
) -> Direction {
    match block_start_side(writing_mode) {
        PhysicalSide::Left => Direction::Ltr,
        PhysicalSide::Right => Direction::Rtl,
        PhysicalSide::Top | PhysicalSide::Bottom => direction,
    }
}

/// Return the definite content-height basis an absolutely positioned box can
/// expose to descendants before intrinsic width measurement.
///
/// CSS Positioned Layout makes an absolutely positioned box's own containing
/// block definite for percentage resolution, and CSS 2.2's vertical equation
/// can also make `height: auto` definite when both physical insets are set:
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height> and
/// <https://drafts.csswg.org/css-sizing-3/#definite>.
pub(in crate::layout) fn absolute_positioned_content_height_percentage_basis(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    vertical_border_width: f32,
) -> BlockSizePercentageBasis {
    let vertical_non_content = style.padding.top + style.padding.bottom + vertical_border_width;
    if let Some(height) = used_content_box_height_or_auto(
        style,
        layout_pt(containing_block.height()),
        non_content_pt(vertical_non_content),
    ) {
        return PercentageBasis::definite_from(
            constrain_content_height(
                style,
                height,
                PercentageBasis::definite(layout_pt(containing_block.height())),
            ),
            BlockSizeBasisSource::AbsolutePositioned,
        );
    }

    let Some(top) = used_inset_top(style, containing_block) else {
        return PercentageBasis::indefinite();
    };
    let Some(bottom) = used_inset_bottom(style, containing_block) else {
        return PercentageBasis::indefinite();
    };
    let content_height = (containing_block.height()
        - top
        - style.margin.top
        - vertical_non_content
        - style.margin.bottom
        - bottom)
        .max(0.0);
    PercentageBasis::definite_from(
        constrain_content_height(
            style,
            content_box_pt(content_height),
            PercentageBasis::definite(layout_pt(containing_block.height())),
        ),
        BlockSizeBasisSource::AbsolutePositioned,
    )
}

pub(in crate::layout) fn resolve_absolute_vertical(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_height: f32,
    automatic_minimum_height: Option<f32>,
    static_start: f32,
    vertical_border_width: f32,
) -> PositionedAxis {
    // CSS 2.1 10.6.4, non-replaced absolutely positioned elements. Static
    // position is approximated from the layout cursor at the element's source
    // position until layout carries explicit placeholders.
    let top = used_inset_top(style, containing_block);
    let bottom = used_inset_bottom(style, containing_block);
    let height = used_content_box_height_or_auto(
        style,
        layout_pt(containing_block.height()),
        non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width),
    )
    .map(|height| {
        constrain_content_height(
            style,
            height,
            PercentageBasis::definite(layout_pt(containing_block.height())),
        )
        .points()
    })
    .map(|height| automatic_minimum_height.map_or(height, |minimum| height.max(minimum)));
    let auto_height = constrain_content_height(
        style,
        content_box_pt(auto_height),
        PercentageBasis::definite(layout_pt(containing_block.height())),
    )
    .points();
    // CSS 2.2 defines the static position as the hypothetical normal-flow
    // position. It can fall outside the containing block, especially while a
    // nested formatting context is measured in temporary coordinates.
    let margin_start = style.margin.top;
    let margin_end = style.margin.bottom;
    let non_content = style.padding.top + style.padding.bottom + vertical_border_width;
    let fill_between = |start: f32, end: f32| {
        (containing_block.height() - start - margin_start - non_content - margin_end - end).max(0.0)
    };
    let border_box_size = |content_size: f32| content_size + non_content;
    let start_for_end = |content_size: f32, end: f32| {
        containing_block.height() - end - margin_start - margin_end - border_box_size(content_size)
    };

    match (top, height, bottom) {
        (Some(start), Some(size), Some(end)) => resolve_absolute_definite_axis_auto_margins(
            style.box_values.margin.top.is_auto(),
            style.box_values.margin.bottom.is_auto(),
            AbsoluteDefiniteAxis {
                start,
                size,
                end,
                margin_start,
                margin_end,
                non_content,
                containing_size: containing_block.height(),
            },
            AbsoluteAxisDirection::Vertical,
        ),
        (Some(start), Some(size), None) => {
            PositionedAxis::new(start, size, margin_start, margin_end)
        }
        (Some(start), None, Some(end)) if style.display.is_table() => {
            resolve_absolute_definite_axis_auto_margins(
                style.box_values.margin.top.is_auto(),
                style.box_values.margin.bottom.is_auto(),
                AbsoluteDefiniteAxis {
                    start,
                    size: auto_height,
                    end,
                    margin_start,
                    margin_end,
                    non_content,
                    containing_size: containing_block.height(),
                },
                AbsoluteAxisDirection::Vertical,
            )
        }
        (Some(start), None, Some(end)) => PositionedAxis::new(
            start,
            constrain_content_height(
                style,
                content_box_pt(fill_between(start, end)),
                PercentageBasis::definite(layout_pt(containing_block.height())),
            )
            .points(),
            margin_start,
            margin_end,
        ),
        (Some(start), None, None) => {
            PositionedAxis::new(start, auto_height, margin_start, margin_end)
        }
        (None, Some(size), Some(end)) => {
            PositionedAxis::new(start_for_end(size, end), size, margin_start, margin_end)
        }
        (None, Some(size), None) => {
            PositionedAxis::new(static_start, size, margin_start, margin_end)
        }
        (None, None, Some(end)) => PositionedAxis::new(
            start_for_end(auto_height, end),
            auto_height,
            margin_start,
            margin_end,
        ),
        (None, None, None) => {
            PositionedAxis::new(static_start, auto_height, margin_start, margin_end)
        }
    }
}

pub(in crate::layout) fn paint_effects_for_element_box(
    element: &Element,
    style: &ComputedStyle,
    border_box: PaintClip,
) -> PaintEffects {
    paint_effects_for_box_with_overflow_clip(
        style,
        border_box,
        used_overflow_clips_element(element, style)
            || (style.contain.paint && property_containment_applies_to_element(element, style)),
    )
}

pub(in crate::layout) fn paint_effects_for_box(
    style: &ComputedStyle,
    border_box: PaintClip,
) -> PaintEffects {
    paint_effects_for_box_with_overflow_clip(
        style,
        border_box,
        style_clips_overflow(style) || style.contain.paint,
    )
}

pub(in crate::layout) fn paint_effects_for_box_with_overflow_clip(
    style: &ComputedStyle,
    border_box: PaintClip,
    clips_overflow: bool,
) -> PaintEffects {
    let borders = used_border_widths(style);
    let transform = paint_transform_for_box(style, border_box.paint_rect());
    let suppress_3d = transform_3d_suppresses_paint(style, border_box.paint_rect());
    PaintEffects {
        opacity: style.opacity,
        transform,
        suppress_paint: suppress_3d
            || transform.is_some_and(|transform| !transform.is_invertible()),
        overflow_clip: clips_overflow.then_some(PaintClip::from_paint_rect(paint_space_rect(
            border_box.x() + borders.left,
            border_box.y() + borders.bottom,
            border_box.width() - borders.left - borders.right,
            border_box.height() - borders.top - borders.bottom,
        ))),
        overflow_clip_union: None,
        rounded_overflow_clip: clips_overflow
            .then(|| {
                rounded_clip_rect_for_box(
                    paint_space_rect(
                        border_box.x(),
                        border_box.y(),
                        border_box.width(),
                        border_box.height(),
                    ),
                    style,
                    borders,
                    css::BackgroundBox::Padding,
                )
            })
            .flatten(),
        absolute_clip: None,
        clip_path: paint_clip_path_effect(style, border_box),
        mask: paint_mask_effect(style),
        filter: paint_filter_effect(style),
        blend_mode: paint_blend_mode(style.mix_blend_mode),
        isolation: style.isolation == Isolation::Isolate || style.will_change.isolation,
    }
}

pub(in crate::layout) fn paint_clip_path_effect(
    style: &ComputedStyle,
    border_box: PaintClip,
) -> PaintClipPathEffect {
    match &style.clip_path {
        ClipPath::None if style.will_change.clip_path => PaintClipPathEffect::WillChange,
        ClipPath::None => PaintClipPathEffect::None,
        ClipPath::Polygon(points) => {
            let border_box = border_box.paint_rect();
            let resolve = |value: &css::ComputedLengthPercentage, basis: f32| {
                value
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
                    .map(layout_points)
                    .unwrap_or_else(|| value.length_points())
            };
            let points = points
                .iter()
                .map(|point| {
                    PaintPoint::new(
                        border_box.min_x() + resolve(&point.x, border_box.width()),
                        // CSS basic-shape coordinates use the geometry box's
                        // top-left origin, while page paint coordinates use a
                        // bottom-left origin.
                        border_box.max_y() - resolve(&point.y, border_box.height()),
                    )
                })
                .collect::<Vec<_>>();
            RenderedClipPathPolygon::new(&points)
                .map(|polygon| PaintClipPathEffect::Polygon(Box::new(polygon)))
                .unwrap_or(PaintClipPathEffect::Shape)
        }
        ClipPath::Inset {
            top,
            right,
            bottom,
            left,
        } => {
            let border_box = border_box.paint_rect();
            let resolve = |value: &css::ComputedLengthPercentage, basis: f32| {
                value
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
                    .map(layout_points)
                    .unwrap_or_else(|| value.length_points())
            };
            let left = resolve(left, border_box.width());
            let right = resolve(right, border_box.width());
            let top = resolve(top, border_box.height());
            let bottom = resolve(bottom, border_box.height());
            let min_x = border_box.min_x() + left;
            let max_x = border_box.max_x() - right;
            let min_y = border_box.min_y() + bottom;
            let max_y = border_box.max_y() - top;
            RenderedClipPathPolygon::new(&[
                PaintPoint::new(min_x, min_y),
                PaintPoint::new(max_x, min_y),
                PaintPoint::new(max_x, max_y),
                PaintPoint::new(min_x, max_y),
            ])
            .map(|polygon| PaintClipPathEffect::Polygon(Box::new(polygon)))
            .unwrap_or(PaintClipPathEffect::Shape)
        }
        ClipPath::Shape => PaintClipPathEffect::Shape,
        ClipPath::Url => PaintClipPathEffect::Url,
    }
}

pub(in crate::layout) fn paint_mask_effect(style: &ComputedStyle) -> PaintMaskEffect {
    if !matches!(style.mask, MaskValue::None) {
        PaintMaskEffect::MaskImage
    } else if style.will_change.mask {
        PaintMaskEffect::WillChange
    } else {
        PaintMaskEffect::None
    }
}

pub(in crate::layout) fn paint_filter_effect(style: &ComputedStyle) -> PaintFilterEffect {
    if !matches!(style.filter, FilterValue::None) {
        PaintFilterEffect::FilterList
    } else if style.will_change.filter {
        PaintFilterEffect::WillChange
    } else {
        PaintFilterEffect::None
    }
}

pub(in crate::layout) fn paint_blend_mode(mode: MixBlendMode) -> PaintBlendMode {
    match mode {
        MixBlendMode::Normal => PaintBlendMode::Normal,
        MixBlendMode::Multiply => PaintBlendMode::Multiply,
        MixBlendMode::Screen => PaintBlendMode::Screen,
        MixBlendMode::Overlay => PaintBlendMode::Overlay,
        MixBlendMode::Darken => PaintBlendMode::Darken,
        MixBlendMode::Lighten => PaintBlendMode::Lighten,
        MixBlendMode::ColorDodge => PaintBlendMode::ColorDodge,
        MixBlendMode::ColorBurn => PaintBlendMode::ColorBurn,
        MixBlendMode::HardLight => PaintBlendMode::HardLight,
        MixBlendMode::SoftLight => PaintBlendMode::SoftLight,
        MixBlendMode::Difference => PaintBlendMode::Difference,
        MixBlendMode::Exclusion => PaintBlendMode::Exclusion,
        MixBlendMode::Hue => PaintBlendMode::Hue,
        MixBlendMode::Saturation => PaintBlendMode::Saturation,
        MixBlendMode::Color => PaintBlendMode::Color,
        MixBlendMode::Luminosity => PaintBlendMode::Luminosity,
    }
}

pub(in crate::layout) fn positioned_applicable_overflow_clips(
    clips: &[OverflowClip],
    containing_block: ContainingBlock,
) -> Vec<OverflowClip> {
    let containing_block_rect = PageTopRect::new(
        containing_block.x(),
        containing_block.top_y(),
        containing_block.width(),
        containing_block.height(),
    )
    .paint_rect();
    clips
        .iter()
        .cloned()
        .filter(|clip| paint_rect_contains(clip.paint_rect(), containing_block_rect))
        .collect()
}

pub(in crate::layout) fn paint_rect_contains(outer: PaintRect, inner: PaintRect) -> bool {
    const EPSILON: f32 = 0.01;
    let outer_left = outer.origin.x;
    let outer_right = outer.origin.x + outer.size.width;
    let outer_bottom = outer.origin.y;
    let outer_top = outer.origin.y + outer.size.height;
    let inner_left = inner.origin.x;
    let inner_right = inner.origin.x + inner.size.width;
    let inner_bottom = inner.origin.y;
    let inner_top = inner.origin.y + inner.size.height;
    outer_left <= inner_left + EPSILON
        && outer_right + EPSILON >= inner_right
        && outer_bottom <= inner_bottom + EPSILON
        && outer_top + EPSILON >= inner_top
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_gradient_pixel_size_preserves_asymmetric_paint_size() {
        assert_eq!(
            generated_image_pixel_size(PaintSize::new(30.0, 11.0)),
            RasterPixelSize::new(60, 22)
        );
    }

    #[test]
    fn fractional_background_clip_preserves_source_mapping_and_intersects_rounded_clip() {
        let image = RenderedImage::from_paint_rect(
            paint_space_rect(10.0, 20.0, 100.0, 50.0),
            true,
            200,
            100,
            Some(RenderedImageSourceRect {
                x: 10,
                y: 20,
                width: 200,
                height: 100,
            }),
            true,
            Rc::from(Vec::new().into_boxed_slice()),
            None,
            None,
        );

        let rounded_clip = RenderedPathClip::new(
            paint_rect_path_commands(paint_space_rect(35.0, 35.0, 40.0, 10.0)),
            RenderedPathFillRule::EvenOdd,
            vec![RenderedPathClipPath::new(
                paint_rect_path_commands(paint_space_rect(40.0, 35.0, 20.0, 10.0)),
                RenderedPathFillRule::NonZero,
            )],
        );

        let clipped = clip_background_image_to_paint_area(
            image,
            PaintBackgroundArea::from_paint_rect(paint_space_rect(30.0, 30.0, 50.0, 20.0)),
            Some(rounded_clip.clone()),
        )
        .expect("overlapping paint rectangles should retain an image");

        assert_eq!(
            clipped.paint_rect(),
            paint_space_rect(10.0, 20.0, 100.0, 50.0)
        );
        assert_eq!(
            clipped.source_rect(),
            Some(RenderedImageSourceRect {
                x: 10,
                y: 20,
                width: 200,
                height: 100,
            }),
        );
        let clip = clipped
            .clip()
            .expect("partial tile installs a destination clip");
        assert_eq!(
            clip.commands,
            paint_rect_path_commands(paint_space_rect(30.0, 30.0, 50.0, 20.0))
        );
        assert_eq!(
            clip.additional_clips,
            vec![
                RenderedPathClipPath::new(rounded_clip.commands, rounded_clip.fill_rule),
                rounded_clip.additional_clips.into_iter().next().unwrap(),
            ]
        );
    }

    #[test]
    fn contained_background_tile_keeps_only_its_rounded_clip() {
        let image = RenderedImage::from_paint_rect(
            paint_space_rect(30.0, 30.0, 20.0, 20.0),
            true,
            20,
            20,
            None,
            true,
            Rc::from(Vec::new().into_boxed_slice()),
            None,
            None,
        );
        let rounded_clip = RenderedPathClip::new(
            paint_rect_path_commands(paint_space_rect(30.0, 30.0, 20.0, 20.0)),
            RenderedPathFillRule::NonZero,
            Vec::new(),
        );

        let clipped = clip_background_image_to_paint_area(
            image,
            PaintBackgroundArea::from_paint_rect(paint_space_rect(20.0, 20.0, 40.0, 40.0)),
            Some(rounded_clip.clone()),
        )
        .expect("contained tile remains paintable");

        assert_eq!(
            clipped.paint_rect(),
            paint_space_rect(30.0, 30.0, 20.0, 20.0)
        );
        assert_eq!(clipped.clip(), Some(&rounded_clip));
    }

    fn containing_block(width: f32) -> ContainingBlock {
        ContainingBlock::from_page_top_rect(PageTopRect::new(0.0, 100.0, width, 100.0))
    }

    fn length(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    #[test]
    fn physical_horizontal_axis_uses_block_start_for_vertical_writing_modes() {
        assert_eq!(
            physical_horizontal_axis_direction(WritingMode::HorizontalTb, Direction::Rtl),
            Direction::Rtl
        );
        assert_eq!(
            physical_horizontal_axis_direction(WritingMode::VerticalLr, Direction::Rtl),
            Direction::Ltr
        );
        assert_eq!(
            physical_horizontal_axis_direction(WritingMode::VerticalRl, Direction::Ltr),
            Direction::Rtl
        );
    }

    #[test]
    fn absolute_positioned_height_basis_uses_explicit_height() {
        let mut style = ComputedStyle::initial();
        style.box_values.height = length(40.0);

        let basis = absolute_positioned_content_height_percentage_basis(
            &style,
            containing_block(100.0),
            0.0,
        );

        assert!(basis.is_definite());
        assert!((basis.points().unwrap() - 40.0).abs() < 0.01);
    }

    #[test]
    fn absolute_positioned_height_basis_uses_top_bottom_fill() {
        let mut style = ComputedStyle::initial();
        style.box_values.inset_top = length(10.0);
        style.box_values.inset_bottom = length(20.0);

        let basis = absolute_positioned_content_height_percentage_basis(
            &style,
            containing_block(100.0),
            0.0,
        );

        assert!(basis.is_definite());
        assert!((basis.points().unwrap() - 70.0).abs() < 0.01);
    }

    #[test]
    fn absolute_positioned_height_basis_keeps_one_inset_auto_height_indefinite() {
        let mut style = ComputedStyle::initial();
        style.box_values.inset_top = length(10.0);

        let basis = absolute_positioned_content_height_percentage_basis(
            &style,
            containing_block(100.0),
            0.0,
        );

        assert!(!basis.is_definite());
        assert_eq!(basis.points(), None);
    }

    #[test]
    fn rtl_auto_width_absolute_horizontal_uses_static_right() {
        let style = ComputedStyle::initial();
        let axis = resolve_absolute_horizontal(
            &style,
            containing_block(100.0),
            30.0,
            StaticHorizontalPosition::new(0.0, 0.0),
            Direction::Rtl,
        );

        assert!((axis.start - 70.0).abs() < 0.01, "{axis:?}");
        assert!((axis.size - 30.0).abs() < 0.01, "{axis:?}");
    }

    #[test]
    fn rtl_definite_width_absolute_horizontal_uses_static_right() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(25.0),
        );
        let axis = resolve_absolute_horizontal(
            &style,
            containing_block(100.0),
            30.0,
            StaticHorizontalPosition::new(0.0, 0.0),
            Direction::Rtl,
        );

        assert!((axis.start - 75.0).abs() < 0.01, "{axis:?}");
        assert!((axis.size - 25.0).abs() < 0.01, "{axis:?}");
    }

    #[test]
    fn repeated_border_image_tiles_share_decoded_pixel_storage() {
        let decoded = DecodedPngImage::new(1, 1, vec![20, 40, 60], Some(vec![255]));
        let mut images = Vec::new();

        push_border_image_tiles(
            &mut images,
            &decoded,
            RenderedImageTileRect::from_paint_rect(paint_space_rect(0.0, 0.0, 3.0, 1.0)),
            RenderedImageSourceRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            css::BorderImageRepeatKeyword::Repeat,
            css::BorderImageRepeatKeyword::Stretch,
            true,
        );

        assert!(images.len() > 1, "{images:?}");
        assert!(
            images[1..]
                .iter()
                .all(|image| images[0].pixel_storage_ptr_eq(image))
        );
    }

    #[test]
    fn repeated_border_image_tiles_center_a_clipped_single_tile() {
        assert_eq!(
            repeat_border_image_tile_segments(10.0, 16.0, 160),
            vec![BorderImageTileSegment {
                destination_offset: 0.0,
                destination_size: 10.0,
                source_offset: 30,
                source_size: 100,
            }],
        );
    }

    #[test]
    fn spaced_border_image_tiles_include_gaps_at_both_edges() {
        assert_eq!(
            border_image_tile_segments(css::BorderImageRepeatKeyword::Space, 296.0, 100.0, 50),
            vec![
                BorderImageTileSegment {
                    destination_offset: 32.0,
                    destination_size: 100.0,
                    source_offset: 0,
                    source_size: 50,
                },
                BorderImageTileSegment {
                    destination_offset: 164.0,
                    destination_size: 100.0,
                    source_offset: 0,
                    source_size: 50,
                },
            ],
        );
    }

    #[test]
    fn round_repeat_rescales_an_auto_opposite_background_size_axis() {
        let decoded = DecodedPngImage::new(100, 100, vec![0; 100 * 100 * 3], None);
        let mut layer = css::BackgroundLayer::initial();
        layer.image = css::ComputedImage::image(css::BackgroundImage::Url {
            src: "image.png".to_string(),
            base_url: None,
            root_url: None,
            request_modifiers: css::RequestUrlModifiers::default(),
        });
        layer.size = css::BackgroundSize::Explicit {
            width: css::BackgroundSizeAxis::LengthPercentage(
                css::ComputedLengthPercentage::from_points(52.0),
            ),
            height: css::BackgroundSizeAxis::Auto,
        };
        layer.repeat = css::BackgroundRepeat::new(
            css::BackgroundRepeatAxis::Round,
            css::BackgroundRepeatAxis::Repeat,
        );

        let size = used_background_layer_size(&decoded, &layer, PaintSize::new(180.0, 180.0));

        assert!((size.width - 60.0).abs() < 0.01, "{}", size.width);
        assert!((size.height - 60.0).abs() < 0.01, "{}", size.height);
    }

    #[test]
    fn no_repeat_retains_the_tile_for_a_zero_sized_positioning_area() {
        assert_eq!(
            background_tile_positions(25.0, 25.0, 0.0, 50.0, css::BackgroundRepeatAxis::NoRepeat,),
            vec![25.0],
        );
        assert!(
            background_tile_positions(25.0, 25.0, 0.0, 50.0, css::BackgroundRepeatAxis::Repeat,)
                .is_empty()
        );
    }

    #[test]
    fn fixed_background_positioning_uses_viewport_and_ignores_origin() {
        let mut style = ComputedStyle::initial();
        style.padding = css::Edges {
            top: 10.0,
            right: 10.0,
            bottom: 10.0,
            left: 10.0,
        };
        style.border_widths = css::Edges {
            top: 5.0,
            right: 5.0,
            bottom: 5.0,
            left: 5.0,
        };
        let border_area =
            PaintBackgroundArea::new(PaintPoint::new(100.0, 50.0), PaintSize::new(80.0, 60.0));
        let viewport =
            PaintBackgroundArea::new(PaintPoint::new(0.0, 0.0), PaintSize::new(500.0, 700.0));
        let mut layer = css::BackgroundLayer::initial();
        layer.origin = css::BackgroundBox::Content;
        layer.attachment = css::BackgroundAttachment::Fixed;

        assert_eq!(
            background_positioning_area_for_layer(
                border_area,
                Some(viewport),
                false,
                &style,
                &layer,
            ),
            viewport,
        );

        style
            .transform
            .push(css::TransformFunction::Scale(css::CssScaleFactors {
                x: 1.0,
                y: 1.0,
            }));
        assert_eq!(
            background_positioning_area_for_layer(
                border_area,
                Some(viewport),
                true,
                &style,
                &layer,
            ),
            PaintBackgroundArea::new(PaintPoint::new(110.0, 60.0), PaintSize::new(60.0, 40.0),),
        );
        style.transform.clear();

        layer.attachment = css::BackgroundAttachment::Scroll;
        assert_eq!(
            background_positioning_area_for_layer(
                border_area,
                Some(viewport),
                false,
                &style,
                &layer,
            ),
            PaintBackgroundArea::new(PaintPoint::new(110.0, 60.0), PaintSize::new(60.0, 40.0),),
        );
    }

    #[test]
    fn background_areas_preserve_bottom_left_insets_and_disjoint_intersections() {
        let area =
            PaintBackgroundArea::new(PaintPoint::new(10.0, 20.0), PaintSize::new(100.0, 80.0));

        assert_eq!(
            area.inset(css::Edges {
                top: 7.0,
                right: 11.0,
                bottom: 13.0,
                left: 17.0,
            })
            .paint_rect(),
            paint_space_rect(27.0, 33.0, 72.0, 60.0),
        );
        assert!(
            area.intersect(PaintBackgroundArea::new(
                PaintPoint::new(200.0, 300.0),
                PaintSize::new(10.0, 10.0),
            ))
            .is_none()
        );
    }

    #[test]
    fn document_canvas_background_projection_preserves_tile_phase_per_page() {
        let canvas_tile = DocumentCanvasBackgroundArea::new(
            DocumentCanvasPoint::new(25.0, 240.0),
            DocumentCanvasSize::new(40.0, 30.0),
        );
        let first_page = canvas_tile.project_to_paint(200.0);
        let second_page = canvas_tile.project_to_paint(100.0);

        assert_eq!(
            first_page.paint_rect(),
            paint_space_rect(25.0, 40.0, 40.0, 30.0)
        );
        assert_eq!(
            second_page.paint_rect(),
            paint_space_rect(25.0, 140.0, 40.0, 30.0)
        );
        assert_eq!(
            second_page.y() - first_page.y(),
            100.0,
            "projecting positioning, clip, and fixed areas by the same page bottom preserves their relative phase"
        );
    }

    #[test]
    fn repeated_uniform_background_covers_the_clip_beyond_its_positioning_area() {
        assert_eq!(
            color_image_axis_tiles(
                100.0,
                0.0,
                10.0,
                css::BackgroundRepeatAxis::Repeat,
                Vec::new(),
                0.0,
                300.0,
            ),
            vec![(0.0, 300.0)],
        );
    }

    #[test]
    fn ltr_absolute_horizontal_static_left_can_fall_after_containing_block() {
        let style = ComputedStyle::initial();
        let axis = resolve_absolute_horizontal(
            &style,
            containing_block(100.0),
            30.0,
            StaticHorizontalPosition::new_unclamped(130.0, -30.0),
            Direction::Ltr,
        );

        assert!((axis.start - 130.0).abs() < 0.01, "{axis:?}");
        assert!((axis.size - 30.0).abs() < 0.01, "{axis:?}");
    }

    #[test]
    fn rtl_absolute_horizontal_static_right_can_fall_after_containing_block() {
        let style = ComputedStyle::initial();
        let axis = resolve_absolute_horizontal(
            &style,
            containing_block(100.0),
            30.0,
            StaticHorizontalPosition::new_unclamped(-60.0, 130.0),
            Direction::Rtl,
        );

        assert!((axis.start + 60.0).abs() < 0.01, "{axis:?}");
        assert!((axis.size - 30.0).abs() < 0.01, "{axis:?}");
    }

    #[test]
    fn raster_hsl_decreasing_matches_longer_hue() {
        let gradient = |hue| css::LinearGradient {
            direction: css::LinearGradientDirection::Angle(90.0),
            interpolation: css::GradientInterpolationMethod {
                space: css::GradientInterpolationSpace::Hsl,
                hue,
            },
            repeating: false,
            stops: vec![
                css::GradientColorStop {
                    color: css::GradientColor::CssColor(CssColor::new(255, 0, 0)),
                    position: Some(css::ComputedLengthPercentage::from_percent(0.0)),
                },
                css::GradientColorStop {
                    color: css::GradientColor::CssColor(CssColor::new(255, 165, 0)),
                    position: Some(css::ComputedLengthPercentage::from_percent(1.0)),
                },
            ],
            hints: Vec::new(),
        };
        let size = PaintSize::new(100.0, 20.0);
        let decreasing = rasterize_linear_gradient(
            &gradient(css::HueInterpolationMethod::Decreasing),
            size,
            CssColor::TRANSPARENT,
        )
        .unwrap();
        let longer = rasterize_linear_gradient(
            &gradient(css::HueInterpolationMethod::Longer),
            size,
            CssColor::TRANSPARENT,
        )
        .unwrap();
        assert_eq!(decreasing.rgb, longer.rgb);
    }

    #[test]
    fn uniform_gradient_detection_resolves_missing_srgb_components() {
        let yellow = CssColor::new(255, 255, 0);
        let uniform = uniform_gradient_stop_color(
            &[
                css::GradientColor::ColorWithMissing {
                    color: CssColor::new(0, 255, 0),
                    missing: css::GradientMissingComponents::new(0b0101),
                    source: css::GradientMissingComponentSpace::Rgb,
                },
                css::GradientColor::CssColor(yellow),
            ],
            css::GradientInterpolationMethod {
                space: css::GradientInterpolationSpace::Srgb,
                hue: css::HueInterpolationMethod::Shorter,
            },
            CssColor::BLACK,
        );
        assert_eq!(uniform, Some(yellow));
    }
}
