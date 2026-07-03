use super::*;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct BackgroundPaintArea {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
}

/// Resolves and tiles a CSS background image layer for any box-like area.
///
/// CSS Backgrounds and Borders defines background image sizing, positioning,
/// and repetition independently of the formatting context that produced the
/// box. This shared helper is used by document boxes, page boxes, and
/// page-margin boxes so generated page content paints backgrounds with the
/// same semantics as normal elements:
/// <https://www.w3.org/TR/css-backgrounds-3/#backgrounds>.
pub(in crate::layout) fn background_images_for_style(
    area: BackgroundPaintArea,
    style: &ComputedStyle,
    fallback_base_url: Option<&Path>,
    fallback_root_url: Option<&Path>,
    resource_cache: &ResourceCache,
) -> Vec<RenderedImage> {
    let mut images = Vec::new();
    for layer in background_layers_for_paint(style).iter().rev() {
        let positioning_area = background_paint_area_for_box(area, style, layer.origin);
        let clip_area = background_paint_area_for_box(area, style, layer.clip);
        let rounded_clip = rounded_background_clip_for_box(
            area.x,
            area.y,
            area.width,
            area.height,
            style,
            used_border_widths(style),
            layer.clip,
        );
        let Some(decoded) = background_layer_decoded_image(
            layer,
            positioning_area,
            fallback_base_url,
            fallback_root_url,
            resource_cache,
        ) else {
            continue;
        };
        let (image_width, image_height) = used_background_layer_size(
            &decoded,
            layer,
            positioning_area.width,
            positioning_area.height,
        );
        if image_width <= 0.0 || image_height <= 0.0 {
            continue;
        }
        let (offset_x, offset_y) = background_position(
            layer.position,
            positioning_area.width,
            positioning_area.height,
            image_width,
            image_height,
        );
        let tile_xs = background_tile_positions(
            positioning_area.x + offset_x,
            positioning_area.x,
            positioning_area.width,
            image_width,
            layer.repeat.repeats_x(),
        );
        let tile_ys = background_tile_positions(
            positioning_area.y + offset_y,
            positioning_area.y,
            positioning_area.height,
            image_height,
            layer.repeat.repeats_y(),
        );
        for tile_y in tile_ys {
            for tile_x in &tile_xs {
                let image = RenderedImage::from_paint_rect(
                    paint_space_rect(*tile_x, tile_y, image_width, image_height),
                    true,
                    decoded.pixel_width,
                    decoded.pixel_height,
                    None,
                    true,
                    decoded.rgb.clone(),
                    decoded.alpha.clone(),
                    None,
                );
                if let Some(mut image) = clip_background_image_to_area(image, clip_area) {
                    if let Some(clip) = rounded_clip.clone() {
                        image = image.with_clip(clip);
                    }
                    images.push(image);
                }
            }
        }
    }
    images
}

fn background_layer_decoded_image(
    layer: &css::BackgroundLayer,
    positioning_area: BackgroundPaintArea,
    fallback_base_url: Option<&Path>,
    fallback_root_url: Option<&Path>,
    resource_cache: &ResourceCache,
) -> Option<DecodedPngImage> {
    match layer.image.as_ref()? {
        BackgroundImage::Url {
            src,
            base_url,
            root_url,
        } => load_image_source(
            src.as_str(),
            base_url.as_deref().or(fallback_base_url),
            root_url.as_deref().or(fallback_root_url),
            resource_cache,
        ),
        BackgroundImage::LinearGradient(gradient) => {
            if gradient_can_skip_raster_vector_paints(gradient, layer, positioning_area) {
                None
            } else {
                rasterize_linear_gradient(gradient, positioning_area.width, positioning_area.height)
            }
        }
        BackgroundImage::RadialGradient(gradient) => {
            rasterize_radial_gradient(gradient, positioning_area.width, positioning_area.height)
        }
    }
}

pub(in crate::layout) fn rasterize_generated_css_image(
    image: &BackgroundImage,
    width: f32,
    height: f32,
    fallback_base_url: Option<&Path>,
    fallback_root_url: Option<&Path>,
    resource_cache: &ResourceCache,
) -> Option<DecodedPngImage> {
    match image {
        BackgroundImage::Url {
            src,
            base_url,
            root_url,
        } => load_image_source(
            src.as_str(),
            base_url.as_deref().or(fallback_base_url),
            root_url.as_deref().or(fallback_root_url),
            resource_cache,
        ),
        BackgroundImage::LinearGradient(gradient) => {
            rasterize_linear_gradient(gradient, width, height)
        }
        BackgroundImage::RadialGradient(gradient) => {
            rasterize_radial_gradient(gradient, width, height)
        }
    }
}

fn gradient_can_skip_raster_vector_paints(
    gradient: &css::LinearGradient,
    layer: &css::BackgroundLayer,
    positioning_area: BackgroundPaintArea,
) -> bool {
    if !linear_gradient_can_paint_as_vector(gradient, layer) {
        return false;
    }
    let area = BackgroundRectArea {
        x: positioning_area.x,
        y: positioning_area.y,
        width: positioning_area.width,
        height: positioning_area.height,
    };
    let line = angled_gradient_line(gradient.direction, area);
    let Some(stops) = fixed_gradient_stops(gradient, line.axis_length) else {
        return false;
    };
    fixed_gradient_is_hard_stop(&stops)
}

fn used_background_layer_size(
    decoded: &DecodedPngImage,
    layer: &css::BackgroundLayer,
    area_width: f32,
    area_height: f32,
) -> (f32, f32) {
    if matches!(
        layer.image,
        Some(BackgroundImage::LinearGradient(_) | BackgroundImage::RadialGradient(_))
    ) {
        return used_generated_background_size(area_width, area_height, layer.size);
    }
    used_background_size(decoded, area_width, area_height, layer.size)
}

fn used_generated_background_size(
    area_width: f32,
    area_height: f32,
    value: css::BackgroundSize,
) -> (f32, f32) {
    match value {
        css::BackgroundSize::Auto | css::BackgroundSize::Cover | css::BackgroundSize::Contain => {
            (area_width, area_height)
        }
        css::BackgroundSize::Explicit { width, height } => {
            let used_width = used_background_size_axis(width, area_width).unwrap_or(area_width);
            let used_height = used_background_size_axis(height, area_height).unwrap_or(area_height);
            (used_width, used_height)
        }
    }
}

/// Rasterizes a CSS Images Level 3 linear gradient into a generated image.
///
/// Gradients are generated images with no intrinsic dimensions. The caller
/// supplies the concrete object size after CSS Backgrounds sizing, then this
/// samples the gradient in premultiplied sRGB as required by CSS Images 3:
/// <https://www.w3.org/TR/css-images-3/#coloring-gradient-line>.
fn rasterize_linear_gradient(
    gradient: &css::LinearGradient,
    width: f32,
    height: f32,
) -> Option<DecodedPngImage> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let (pixel_width, pixel_height) = generated_image_pixel_size(width, height);
    if pixel_width == 0 || pixel_height == 0 {
        return None;
    }
    let area = BackgroundRectArea {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    let line = angled_gradient_line(gradient.direction, area);
    let stops = fixed_gradient_stops(gradient, line.axis_length)?;
    let mut rgb = Vec::with_capacity(pixel_width as usize * pixel_height as usize * 3);
    let mut alpha = Vec::with_capacity(pixel_width as usize * pixel_height as usize);
    let mut has_alpha = false;
    for row in 0..pixel_height {
        let y = height - ((row as f32 + 0.5) * height / pixel_height as f32);
        for column in 0..pixel_width {
            let x = (column as f32 + 0.5) * width / pixel_width as f32;
            let position = gradient_axis_position((x, y), line);
            let color = sampled_gradient_color(gradient, &stops, position, line.axis_length);
            let a = (color.a * 255.0).round().clamp(0.0, 255.0) as u8;
            rgb.push((color.r * 255.0).round().clamp(0.0, 255.0) as u8);
            rgb.push((color.g * 255.0).round().clamp(0.0, 255.0) as u8);
            rgb.push((color.b * 255.0).round().clamp(0.0, 255.0) as u8);
            alpha.push(a);
            has_alpha |= a < 255;
        }
    }
    Some(DecodedPngImage {
        pixel_width,
        pixel_height,
        rgb,
        alpha: has_alpha.then_some(alpha),
    })
}

/// Rasterizes a CSS Images Level 3 radial gradient into a generated image.
///
/// Radial gradients are generated images with no intrinsic dimensions. The
/// concrete background tile size determines the center point, ending radii,
/// color-stop percentage basis, and repeating period:
/// <https://www.w3.org/TR/css-images-3/#radial-gradients>.
fn rasterize_radial_gradient(
    gradient: &css::RadialGradient,
    width: f32,
    height: f32,
) -> Option<DecodedPngImage> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let (pixel_width, pixel_height) = generated_image_pixel_size(width, height);
    if pixel_width == 0 || pixel_height == 0 {
        return None;
    }
    let geometry = used_radial_gradient_geometry(gradient, width, height)?;
    let stops = fixed_radial_gradient_stops(gradient, geometry.axis_length)?;
    let mut rgb = Vec::with_capacity(pixel_width as usize * pixel_height as usize * 3);
    let mut alpha = Vec::with_capacity(pixel_width as usize * pixel_height as usize);
    let mut has_alpha = false;
    for row in 0..pixel_height {
        let y = height - ((row as f32 + 0.5) * height / pixel_height as f32);
        for column in 0..pixel_width {
            let x = (column as f32 + 0.5) * width / pixel_width as f32;
            let position = radial_gradient_axis_position((x, y), geometry);
            let color =
                sampled_radial_gradient_color(gradient, &stops, position, geometry.axis_length);
            let a = (color.a * 255.0).round().clamp(0.0, 255.0) as u8;
            rgb.push((color.r * 255.0).round().clamp(0.0, 255.0) as u8);
            rgb.push((color.g * 255.0).round().clamp(0.0, 255.0) as u8);
            rgb.push((color.b * 255.0).round().clamp(0.0, 255.0) as u8);
            alpha.push(a);
            has_alpha |= a < 255;
        }
    }
    Some(DecodedPngImage {
        pixel_width,
        pixel_height,
        rgb,
        alpha: has_alpha.then_some(alpha),
    })
}

#[derive(Debug, Clone, Copy)]
struct UsedRadialGradientGeometry {
    center_x: f32,
    center_y: f32,
    radius_x: f32,
    radius_y: f32,
    axis_length: f32,
}

fn used_radial_gradient_geometry(
    gradient: &css::RadialGradient,
    width: f32,
    height: f32,
) -> Option<UsedRadialGradientGeometry> {
    let center_x = used_background_position_axis(gradient.position.x, width, false);
    let center_y = used_background_position_axis(gradient.position.y, height, true);
    let (radius_x, radius_y) = match gradient.size {
        css::RadialGradientSize::CircleRadius(radius) => {
            let radius = used_length_percentage(radius, width.max(height)).max(0.0);
            (radius, radius)
        }
        css::RadialGradientSize::EllipseRadii { x, y } => (
            used_length_percentage(x, width).max(0.0),
            used_length_percentage(y, height).max(0.0),
        ),
        css::RadialGradientSize::Extent(extent) => used_radial_gradient_extent_radii(
            gradient.shape,
            extent,
            center_x,
            center_y,
            width,
            height,
        ),
    };
    if radius_x <= 0.0 || radius_y <= 0.0 {
        return None;
    }
    Some(UsedRadialGradientGeometry {
        center_x,
        center_y,
        radius_x,
        radius_y,
        axis_length: radius_x.max(radius_y),
    })
}

fn used_radial_gradient_extent_radii(
    shape: css::RadialGradientShape,
    extent: css::RadialGradientExtent,
    center_x: f32,
    center_y: f32,
    width: f32,
    height: f32,
) -> (f32, f32) {
    let left = center_x.max(0.0);
    let right = (width - center_x).max(0.0);
    let bottom = center_y.max(0.0);
    let top = (height - center_y).max(0.0);
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
            (radius, radius)
        }
        css::RadialGradientShape::Ellipse => {
            let side_radii = match extent {
                css::RadialGradientExtent::ClosestSide
                | css::RadialGradientExtent::ClosestCorner => (left.min(right), bottom.min(top)),
                css::RadialGradientExtent::FarthestSide
                | css::RadialGradientExtent::FarthestCorner => (left.max(right), bottom.max(top)),
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
    (radius_x, radius_y): (f32, f32),
    extent: css::RadialGradientExtent,
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
) -> (f32, f32) {
    if radius_x <= 0.0 || radius_y <= 0.0 {
        return (radius_x, radius_y);
    }
    let corner_scales = [
        ((left / radius_x).powi(2) + (bottom / radius_y).powi(2)).sqrt(),
        ((left / radius_x).powi(2) + (top / radius_y).powi(2)).sqrt(),
        ((right / radius_x).powi(2) + (bottom / radius_y).powi(2)).sqrt(),
        ((right / radius_x).powi(2) + (top / radius_y).powi(2)).sqrt(),
    ];
    let scale = match extent {
        css::RadialGradientExtent::ClosestCorner => {
            corner_scales.into_iter().fold(f32::INFINITY, f32::min)
        }
        css::RadialGradientExtent::FarthestCorner => corner_scales.into_iter().fold(0.0, f32::max),
        css::RadialGradientExtent::ClosestSide | css::RadialGradientExtent::FarthestSide => 1.0,
    };
    (radius_x * scale, radius_y * scale)
}

fn radial_gradient_axis_position((x, y): (f32, f32), geometry: UsedRadialGradientGeometry) -> f32 {
    let dx = (x - geometry.center_x) / geometry.radius_x;
    let dy = (y - geometry.center_y) / geometry.radius_y;
    (dx * dx + dy * dy).sqrt() * geometry.axis_length
}

fn fixed_radial_gradient_stops(
    gradient: &css::RadialGradient,
    axis_length: f32,
) -> Option<Vec<FixedGradientStop>> {
    fixed_gradient_stops_from_color_stops(&gradient.stops, axis_length)
}

fn fixed_gradient_stops_from_color_stops(
    stops: &[css::GradientColorStop],
    axis_length: f32,
) -> Option<Vec<FixedGradientStop>> {
    if axis_length <= 0.0 || stops.len() < 2 {
        return None;
    }
    let mut positions = stops
        .iter()
        .copied()
        .map(|stop| {
            stop.position
                .and_then(|position| position.used_length_with_percentage_basis(axis_length))
                .or_else(|| {
                    stop.position
                        .map(|position| position.length_with_percentage_basis(axis_length))
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

    Some(
        stops
            .iter()
            .zip(positions)
            .map(|(stop, position)| FixedGradientStop {
                color: stop.color,
                position: position.expect("all positions fixed up"),
            })
            .collect(),
    )
}

fn sampled_radial_gradient_color(
    gradient: &css::RadialGradient,
    stops: &[FixedGradientStop],
    position: f32,
    axis_length: f32,
) -> Color {
    sampled_gradient_color_with_hints(
        gradient.repeating,
        &gradient.hints,
        stops,
        position,
        axis_length,
    )
}

fn generated_image_pixel_size(width: f32, height: f32) -> (u32, u32) {
    const PIXELS_PER_PT: f32 = 2.0;
    const MAX_EDGE: f32 = 4096.0;
    let mut pixel_width = (width * PIXELS_PER_PT).ceil().max(1.0);
    let mut pixel_height = (height * PIXELS_PER_PT).ceil().max(1.0);
    let scale = (MAX_EDGE / pixel_width.max(pixel_height)).min(1.0);
    pixel_width = (pixel_width * scale).ceil().max(1.0);
    pixel_height = (pixel_height * scale).ceil().max(1.0);
    (pixel_width as u32, pixel_height as u32)
}

fn sampled_gradient_color(
    gradient: &css::LinearGradient,
    stops: &[FixedGradientStop],
    position: f32,
    axis_length: f32,
) -> Color {
    sampled_gradient_color_with_hints(
        gradient.repeating,
        &gradient.hints,
        stops,
        position,
        axis_length,
    )
}

fn sampled_gradient_color_with_hints(
    repeating: bool,
    hints: &[css::GradientColorHint],
    stops: &[FixedGradientStop],
    mut position: f32,
    axis_length: f32,
) -> Color {
    if repeating {
        let first = stops.first().map(|stop| stop.position).unwrap_or(0.0);
        let last = stops
            .last()
            .map(|stop| stop.position)
            .unwrap_or(axis_length);
        let period = last - first;
        if period.abs() <= 0.001 {
            return stops
                .last()
                .map(|stop| stop.color)
                .unwrap_or(Color::TRANSPARENT);
        }
        position = (position - first).rem_euclid(period) + first;
    }
    if position <= stops[0].position {
        return stops[0].color;
    }
    for (index, pair) in stops.windows(2).enumerate() {
        if position <= pair[1].position {
            if (pair[1].position - pair[0].position).abs() <= 0.001 {
                return pair[1].color;
            }
            let mut t = (position - pair[0].position) / (pair[1].position - pair[0].position);
            if let Some(hint) =
                hints
                    .iter()
                    .find(|hint| hint.after_stop == index)
                    .and_then(|hint| {
                        hint.position
                            .used_length_with_percentage_basis(axis_length)
                            .or(Some(
                                hint.position.length_with_percentage_basis(axis_length),
                            ))
                    })
            {
                t = hinted_gradient_progress(position, pair[0].position, pair[1].position, hint);
            }
            return interpolate_gradient_color(pair[0].color, pair[1].color, t);
        }
    }
    stops
        .last()
        .map(|stop| stop.color)
        .unwrap_or(Color::TRANSPARENT)
}

fn hinted_gradient_progress(position: f32, start: f32, end: f32, hint: f32) -> f32 {
    if hint <= start || hint >= end {
        return (position - start) / (end - start);
    }
    if position <= hint {
        0.5 * (position - start) / (hint - start)
    } else {
        0.5 + 0.5 * (position - hint) / (end - hint)
    }
    .clamp(0.0, 1.0)
}

fn interpolate_gradient_color(start: Color, end: Color, progress: f32) -> Color {
    let t = progress.clamp(0.0, 1.0);
    let start_r = start.r * start.a;
    let start_g = start.g * start.a;
    let start_b = start.b * start.a;
    let end_r = end.r * end.a;
    let end_g = end.g * end.a;
    let end_b = end.b * end.a;
    let alpha = start.a + (end.a - start.a) * t;
    if alpha <= 0.0 {
        return Color::TRANSPARENT;
    }
    Color::srgb(
        (start_r + (end_r - start_r) * t) / alpha,
        (start_g + (end_g - start_g) * t) / alpha,
        (start_b + (end_b - start_b) * t) / alpha,
        alpha,
    )
}

pub(in crate::layout) fn background_layers_for_paint(
    style: &ComputedStyle,
) -> Vec<css::BackgroundLayer> {
    if !style.background_layers.is_empty() {
        return style.background_layers.clone();
    }
    vec![css::BackgroundLayer {
        image: style.background_image.clone(),
        position: style.background_position,
        size: style.background_size,
        repeat: style.background_repeat,
        origin: style.background_origin,
        clip: style.background_clip,
    }]
}

pub(in crate::layout) fn background_paint_area_for_box(
    area: BackgroundPaintArea,
    style: &ComputedStyle,
    box_: css::BackgroundBox,
) -> BackgroundPaintArea {
    let border = used_border_widths(style);
    match box_ {
        css::BackgroundBox::Border => area,
        css::BackgroundBox::Padding => area.inset(border),
        css::BackgroundBox::Content => area.inset(border).inset(style.padding),
    }
}

pub(in crate::layout) fn clip_background_image_to_area(
    mut image: RenderedImage,
    clip: BackgroundPaintArea,
) -> Option<RenderedImage> {
    let image_x = image.x();
    let image_y = image.y();
    let image_width = image.width();
    let image_height = image.height();
    let x1 = image_x.max(clip.x);
    let y1 = image_y.max(clip.y);
    let x2 = (image_x + image_width).min(clip.x + clip.width);
    let y2 = (image_y + image_height).min(clip.y + clip.height);
    if x2 <= x1 || y2 <= y1 || image_width <= 0.0 || image_height <= 0.0 {
        return None;
    }
    let source = image.source_rect.unwrap_or(RenderedImageSourceRect {
        x: 0,
        y: 0,
        width: image.pixel_width,
        height: image.pixel_height,
    });
    let source_x = source.x as f32 + ((x1 - image_x) / image_width) * source.width as f32;
    let source_y = source.y as f32 + ((y1 - image_y) / image_height) * source.height as f32;
    let source_width = ((x2 - x1) / image_width) * source.width as f32;
    let source_height = ((y2 - y1) / image_height) * source.height as f32;
    image.set_paint_rect(paint_space_rect(x1, y1, x2 - x1, y2 - y1));
    image.source_rect = Some(RenderedImageSourceRect {
        x: source_x.floor().max(0.0) as u32,
        y: source_y.floor().max(0.0) as u32,
        width: source_width.ceil().max(1.0) as u32,
        height: source_height.ceil().max(1.0) as u32,
    });
    Some(image)
}

impl BackgroundPaintArea {
    pub(in crate::layout) fn inset(self, edges: css::Edges) -> Self {
        Self {
            x: self.x + edges.left,
            y: self.y + edges.bottom,
            width: (self.width - edges.left - edges.right).max(0.0),
            height: (self.height - edges.top - edges.bottom).max(0.0),
        }
    }
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
    pub(in crate::layout) fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            rect: paint_space_rect(x, y, width, height),
        }
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

#[derive(Debug, Clone, Copy)]
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
) {
    let (tile_width, tile_height) =
        border_image_base_tile_size(destination, source, repeat_x, repeat_y);
    let x_segments =
        border_image_tile_segments(repeat_x, destination.width(), tile_width, source.width);
    let y_segments =
        border_image_tile_segments(repeat_y, destination.height(), tile_height, source.height);
    for y_segment in &y_segments {
        for x_segment in &x_segments {
            if x_segment.destination_size <= 0.0
                || y_segment.destination_size <= 0.0
                || x_segment.source_size == 0
                || y_segment.source_size == 0
            {
                continue;
            }
            images.push(RenderedImage::from_paint_rect(
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
                true,
                decoded.rgb.clone(),
                decoded.alpha.clone(),
                None,
            ));
        }
    }
}

pub(in crate::layout) fn border_image_base_tile_size(
    destination: RenderedImageTileRect,
    source: RenderedImageSourceRect,
    repeat_x: css::BorderImageRepeatKeyword,
    repeat_y: css::BorderImageRepeatKeyword,
) -> (f32, f32) {
    let mut tile_width = source.width as f32;
    let mut tile_height = source.height as f32;
    if repeat_x != css::BorderImageRepeatKeyword::Stretch
        && repeat_y == css::BorderImageRepeatKeyword::Stretch
        && source.height > 0
    {
        let scale = destination.height() / source.height as f32;
        tile_width *= scale;
    }
    if repeat_y != css::BorderImageRepeatKeyword::Stretch
        && repeat_x == css::BorderImageRepeatKeyword::Stretch
        && source.width > 0
    {
        let scale = destination.width() / source.width as f32;
        tile_height *= scale;
    }
    if repeat_x == css::BorderImageRepeatKeyword::Stretch {
        tile_width = destination.width();
    }
    if repeat_y == css::BorderImageRepeatKeyword::Stretch {
        tile_height = destination.height();
    }
    (tile_width.max(0.0), tile_height.max(0.0))
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
            if count <= 1 {
                let tile_size = base_tile_size.min(destination_size);
                return vec![BorderImageTileSegment {
                    destination_offset: (destination_size - tile_size) / 2.0,
                    destination_size: tile_size,
                    source_offset: 0,
                    source_size,
                }];
            }
            let spacing = (destination_size - base_tile_size * count as f32) / (count - 1) as f32;
            (0..count)
                .map(|index| BorderImageTileSegment {
                    destination_offset: index as f32 * (base_tile_size + spacing),
                    destination_size: base_tile_size,
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
    let mut segments = Vec::new();
    let mut offset = 0.0;
    while offset < destination_size - f32::EPSILON {
        let visible_size = tile_size.min(destination_size - offset);
        let source_visible = ((source_size as f32) * (visible_size / tile_size))
            .round()
            .clamp(1.0, source_size as f32) as u32;
        segments.push(BorderImageTileSegment {
            destination_offset: offset,
            destination_size: visible_size,
            source_offset: 0,
            source_size: source_visible,
        });
        offset += tile_size;
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
    repeats: bool,
) -> Vec<f32> {
    if area_size <= 0.0 || tile_size <= 0.0 {
        return Vec::new();
    }
    if !repeats {
        return vec![positioned_start];
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

pub(in crate::layout) fn resolve_absolute_horizontal(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_or_intrinsic_width: f32,
    static_position: StaticHorizontalPosition,
    containing_direction: Direction,
) -> PositionedAxis {
    // CSS 2.2 10.3.7, non-replaced absolutely positioned elements. The
    // static position has separate physical left and right distances; RTL
    // static-position containing blocks seed auto horizontal positioning from
    // the static right side before solving for the used left.
    let left = used_inset_left(style, containing_block);
    let right = used_inset_right(style, containing_block);
    let width = used_content_width_or_auto(
        style,
        containing_block.width(),
        style.padding.left + style.padding.right + horizontal_border_width(style),
    )
    .or_else(|| {
        matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        )
        .then_some(auto_or_intrinsic_width)
    })
    .map(|width| constrain_width(style, width, containing_block.width()));
    let shrink_to_fit_width =
        constrain_width(style, auto_or_intrinsic_width, containing_block.width());
    let static_left = static_position.left.clamp(0.0, containing_block.width());
    let static_right = static_position.right.clamp(0.0, containing_block.width());
    let margin_start = style.margin.left;
    let margin_end = style.margin.right;
    let non_content = style.padding.left + style.padding.right + horizontal_border_width(style);
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
        (Some(start), None, Some(end)) => PositionedAxis::new(
            start,
            constrain_width(style, fill_between(start, end), containing_block.width()),
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

pub(in crate::layout) fn resolve_absolute_vertical(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_height: f32,
    static_start: f32,
    vertical_border_width: f32,
) -> PositionedAxis {
    // CSS 2.1 10.6.4, non-replaced absolutely positioned elements. Static
    // position is approximated from the layout cursor at the element's source
    // position until layout carries explicit placeholders.
    let top = used_inset_top(style, containing_block);
    let bottom = used_inset_bottom(style, containing_block);
    let height = used_content_height_or_auto(
        style,
        containing_block.height(),
        style.padding.top + style.padding.bottom + vertical_border_width,
    )
    .map(|height| constrain_height(style, height, containing_block.height()));
    let auto_height = constrain_height(style, auto_height, containing_block.height());
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
        (Some(start), None, Some(end)) => PositionedAxis::new(
            start,
            constrain_height(style, fill_between(start, end), containing_block.height()),
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

/// Returns the fill color for a decoded image that is exactly one opaque color.
///
/// CSS Images paints replaced raster content into the element's concrete object
/// size. When every source pixel is the same opaque color, a filled PDF
/// rectangle is visually equivalent and avoids raster-image boundary
/// antialiasing seams at adjacent same-color edges:
/// <https://www.w3.org/TR/css-images-3/#concrete-object-size> and
/// ISO 32000-1:2008 section 8.9.
pub(in crate::layout) fn solid_opaque_image_fill(image: &DecodedPngImage) -> Option<Color> {
    if image.pixel_width <= 1 && image.pixel_height <= 1 {
        return None;
    }
    if image.alpha.is_some() || image.rgb.len() < 3 {
        return None;
    }
    let first = &image.rgb[..3];
    image
        .rgb
        .chunks_exact(3)
        .all(|pixel| pixel == first)
        .then(|| Color::new(first[0], first[1], first[2]))
}

pub(in crate::layout) fn paint_effects_for_element_box(
    element: &Element,
    style: &ComputedStyle,
    border_box: PaintClip,
) -> PaintEffects {
    paint_effects_for_box_with_overflow_clip(
        style,
        border_box,
        used_overflow_clips_element(element, style),
    )
}

pub(in crate::layout) fn paint_effects_for_box(
    style: &ComputedStyle,
    border_box: PaintClip,
) -> PaintEffects {
    paint_effects_for_box_with_overflow_clip(style, border_box, style.overflow.clips_overflow())
}

pub(in crate::layout) fn paint_effects_for_box_with_overflow_clip(
    style: &ComputedStyle,
    border_box: PaintClip,
    clips_overflow: bool,
) -> PaintEffects {
    let borders = used_border_widths(style);
    PaintEffects {
        opacity: style.opacity,
        transform: paint_transform_for_box(style, border_box),
        overflow_clip: clips_overflow.then_some(PaintClip::from_paint_rect(paint_space_rect(
            border_box.x() + borders.left,
            border_box.y() + borders.bottom,
            border_box.width() - borders.left - borders.right,
            border_box.height() - borders.top - borders.bottom,
        ))),
        absolute_clip: None,
        clip_path: paint_clip_path_effect(style),
        mask: paint_mask_effect(style),
        filter: paint_filter_effect(style),
        blend_mode: paint_blend_mode(style.mix_blend_mode),
        isolation: style.isolation == Isolation::Isolate || style.will_change.isolation,
    }
}

pub(in crate::layout) fn paint_clip_path_effect(style: &ComputedStyle) -> PaintClipPathEffect {
    match style.clip_path {
        ClipPath::None if style.will_change.clip_path => PaintClipPathEffect::WillChange,
        ClipPath::None => PaintClipPathEffect::None,
        ClipPath::Inset => PaintClipPathEffect::Inset,
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
        .copied()
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

pub(in crate::layout) fn positioned_box_is_orthogonal_to_containing_block(
    containing: WritingMode,
    positioned: WritingMode,
) -> bool {
    matches!(
        (containing, positioned),
        (
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl | WritingMode::VerticalLr
        ) | (
            WritingMode::VerticalRl | WritingMode::VerticalLr,
            WritingMode::HorizontalTb
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn containing_block(width: f32) -> ContainingBlock {
        ContainingBlock::from_page_top_rect(PageTopRect::new(0.0, 100.0, width, 100.0))
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
}
