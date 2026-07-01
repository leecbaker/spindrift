use super::*;

pub(super) fn load_image_source(
    src: &str,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
    resource_cache: &ResourceCache,
) -> Option<DecodedPngImage> {
    let src = dom::decode_entities_public(src);
    let bytes = if let Some(data) = src.strip_prefix("data:image/") {
        let (_, encoded) = data.split_once(";base64,")?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()?
    } else {
        let path = crate::resource::resolve_url_path(src.as_str(), base_url, root_url)?;
        resource_cache.get(&path)?.to_vec()
    };
    let image = image::load_from_memory(&bytes).ok()?;
    let (pixel_width, pixel_height) = image.dimensions();
    let rgba = image.to_rgba8();
    let mut rgb = Vec::with_capacity(pixel_width as usize * pixel_height as usize * 3);
    let mut alpha = Vec::with_capacity(pixel_width as usize * pixel_height as usize);
    let mut has_alpha = false;
    for pixel in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
        alpha.push(pixel[3]);
        has_alpha |= pixel[3] < 255;
    }
    Some(DecodedPngImage {
        pixel_width,
        pixel_height,
        rgb,
        alpha: has_alpha.then_some(alpha),
    })
}

/// Used CSS size and decoded pixels for an HTML image replaced element.
///
/// CSS Images defines raster images as replaced elements with intrinsic
/// dimensions, while CSS Sizing/Box Sizing define how `width`, `height`,
/// padding, and borders resolve to content-box and border-box sizes:
/// <https://www.w3.org/TR/css-images-3/#sizing> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(super) struct UsedImage {
    pub(super) decoded: DecodedPngImage,
    pub(super) content_width: f32,
    pub(super) content_height: f32,
    pub(super) border_box_width: f32,
    pub(super) border_box_height: f32,
}

/// Intrinsic dimensions and HTML presentational hints for an image element.
///
/// CSS Images treats raster images as replaced elements with intrinsic
/// dimensions and ratio. HTML `width`/`height` attributes participate as
/// presentational hints when present:
/// <https://www.w3.org/TR/css-images-3/#sizing> and
/// <https://html.spec.whatwg.org/multipage/rendering.html#attributes-for-embedded-content-and-images>.
pub(super) struct IntrinsicImageSize {
    pub(super) decoded: DecodedPngImage,
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) attr_width: Option<f32>,
    pub(super) attr_height: Option<f32>,
}

pub(super) fn intrinsic_image_size(
    element: &Element,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
    resource_cache: &ResourceCache,
) -> Option<IntrinsicImageSize> {
    let src = element.attrs.get("src")?;
    let decoded = load_image_source(src, base_url, root_url, resource_cache)?;
    let width = decoded.pixel_width as f32;
    let height = decoded.pixel_height as f32;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let attr_width = element.attrs.get("width").and_then(|value| {
        parse_html_length(value).filter(|width| *width > 0.0 && !value.contains('%'))
    });
    let attr_height = element.attrs.get("height").and_then(|value| {
        parse_html_length(value).filter(|height| *height > 0.0 && !value.contains('%'))
    });
    Some(IntrinsicImageSize {
        decoded,
        width,
        height,
        attr_width,
        attr_height,
    })
}

pub(super) fn used_image(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
    resource_cache: &ResourceCache,
) -> Option<UsedImage> {
    let intrinsic = intrinsic_image_size(element, base_url, root_url, resource_cache)?;
    let aspect_ratio = intrinsic.width / intrinsic.height;
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    let vertical_non_content =
        borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    let available_content_width = (available_width - horizontal_non_content).max(0.0);
    let width = used_content_width_or_auto(style, available_width, horizontal_non_content)
        .or(intrinsic.attr_width);
    let height = definite_image_content_height_without_percent(style, vertical_non_content)
        .or(intrinsic.attr_height);
    let width_is_auto = width.is_none();
    let height_is_auto = height.is_none();
    let (mut content_width, mut content_height) = match (width, height) {
        (Some(width_value), None) => (width_value, width_value / aspect_ratio),
        (None, Some(height_value)) => (height_value * aspect_ratio, height_value),
        (None, None) => (intrinsic.width, intrinsic.height),
        (Some(width_value), Some(height_value)) => (width_value, height_value),
    };
    constrain_replaced_size_with_aspect_ratio(
        &mut content_width,
        &mut content_height,
        aspect_ratio,
        ReplacedAutoAxes {
            width: width_is_auto,
            height: height_is_auto,
        },
        ReplacedSizeConstraints {
            min_width: used_min_width(style, available_width),
            max_width: used_max_width(style, available_width)
                .map(|width| width.min(available_content_width)),
            min_height: used_min_height(style, available_width),
            max_height: used_max_height(style, available_width),
        },
    );
    content_width = content_width.min(available_content_width);
    if width_is_auto && !height_is_auto {
        content_height = content_width / aspect_ratio;
    }
    Some(UsedImage {
        decoded: intrinsic.decoded,
        content_width,
        content_height,
        border_box_width: content_width + horizontal_non_content,
        border_box_height: content_height + vertical_non_content,
    })
}

pub(super) fn used_generated_image(
    src: &str,
    style: &ComputedStyle,
    available_width: f32,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
    resource_cache: &ResourceCache,
) -> Option<UsedImage> {
    let decoded = load_image_source(src, base_url, root_url, resource_cache)?;
    let intrinsic_width = decoded.pixel_width as f32;
    let intrinsic_height = decoded.pixel_height as f32;
    if intrinsic_width <= 0.0 || intrinsic_height <= 0.0 {
        return None;
    }
    let aspect_ratio = intrinsic_width / intrinsic_height;
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    let vertical_non_content =
        borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    let available_content_width = (available_width - horizontal_non_content).max(0.0);
    let width = used_content_width_or_auto(style, available_width, horizontal_non_content);
    let height = definite_image_content_height_without_percent(style, vertical_non_content);
    let width_is_auto = width.is_none();
    let height_is_auto = height.is_none();
    let (mut content_width, mut content_height) = match (width, height) {
        (Some(width_value), None) => (width_value, width_value / aspect_ratio),
        (None, Some(height_value)) => (height_value * aspect_ratio, height_value),
        (None, None) => (intrinsic_width, intrinsic_height),
        (Some(width_value), Some(height_value)) => (width_value, height_value),
    };
    constrain_replaced_size_with_aspect_ratio(
        &mut content_width,
        &mut content_height,
        aspect_ratio,
        ReplacedAutoAxes {
            width: width_is_auto,
            height: height_is_auto,
        },
        ReplacedSizeConstraints {
            min_width: used_min_width(style, available_width),
            max_width: used_max_width(style, available_width)
                .map(|width| width.min(available_content_width)),
            min_height: used_min_height(style, available_width),
            max_height: used_max_height(style, available_width),
        },
    );
    content_width = content_width.min(available_content_width);
    if width_is_auto && !height_is_auto {
        content_height = content_width / aspect_ratio;
    }
    Some(UsedImage {
        decoded,
        content_width,
        content_height,
        border_box_width: content_width + horizontal_non_content,
        border_box_height: content_height + vertical_non_content,
    })
}

/// Used size for an invalid CSS `content: url(...)` replacement image.
///
/// CSS Content Level 3 requires invalid replacement images to render as a
/// transparent image with no natural dimensions rather than suppressing the
/// replacement box:
/// <https://www.w3.org/TR/css-content-3/#content-property>.
pub(super) fn used_invalid_replacement_image(
    style: &ComputedStyle,
    available_width: f32,
) -> UsedImage {
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    let vertical_non_content =
        borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    let available_content_width = (available_width - horizontal_non_content).max(0.0);
    let mut content_width =
        used_content_width_or_auto(style, available_width, horizontal_non_content).unwrap_or(0.0);
    let content_height =
        definite_image_content_height_without_percent(style, vertical_non_content).unwrap_or(0.0);
    content_width = content_width.min(available_content_width);
    UsedImage {
        decoded: DecodedPngImage {
            pixel_width: 1,
            pixel_height: 1,
            rgb: vec![0, 0, 0],
            alpha: Some(vec![0]),
        },
        content_width,
        content_height,
        border_box_width: content_width + horizontal_non_content,
        border_box_height: content_height + vertical_non_content,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ReplacedAutoAxes {
    pub(super) width: bool,
    pub(super) height: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ReplacedSizeConstraints {
    pub(super) min_width: Option<f32>,
    pub(super) max_width: Option<f32>,
    pub(super) min_height: Option<f32>,
    pub(super) max_height: Option<f32>,
}

/// Applies min/max constraints to a replaced element while preserving ratio when possible.
///
/// CSS Sizing defines preferred aspect ratio transfer for boxes with an
/// intrinsic ratio. When one axis is automatic, constraints in the other axis
/// can transfer through that ratio instead of distorting the replaced content:
/// <https://www.w3.org/TR/css-sizing-3/#aspect-ratio> and
/// <https://www.w3.org/TR/css-sizing-3/#min-size-properties>.
pub(super) fn constrain_replaced_size_with_aspect_ratio(
    width: &mut f32,
    height: &mut f32,
    aspect_ratio: f32,
    auto_axes: ReplacedAutoAxes,
    constraints: ReplacedSizeConstraints,
) {
    if aspect_ratio <= 0.0 {
        return;
    }
    if let Some(min_width) = constraints.min_width
        && *width < min_width
    {
        *width = min_width;
        if auto_axes.height {
            *height = *width / aspect_ratio;
        }
    }
    if let Some(max_width) = constraints.max_width
        && *width > max_width
    {
        *width = max_width;
        if auto_axes.height {
            *height = *width / aspect_ratio;
        }
    }
    if let Some(min_height) = constraints.min_height
        && *height < min_height
    {
        *height = min_height;
        if auto_axes.width {
            *width = *height * aspect_ratio;
        }
    }
    if let Some(max_height) = constraints.max_height
        && *height > max_height
    {
        *height = max_height;
        if auto_axes.width {
            *width = *height * aspect_ratio;
        }
    }
}

/// Resolve a definite image content height while preserving indefinite percentages as auto.
///
/// CSS 2.2 treats percentage heights as auto when the containing block block
/// size is not definite. Image layout currently reaches this helper only in
/// those indefinite block-axis contexts, so percentage heights remain unresolved
/// here while absolute lengths still honor `box-sizing`:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-height-property> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(super) fn definite_image_content_height_without_percent(
    style: &ComputedStyle,
    vertical_non_content: f32,
) -> Option<f32> {
    let height = style.box_values.height.length_if_no_percent()?;
    Some(match style.box_sizing {
        BoxSizing::BorderBox => (height - vertical_non_content).max(0.0),
        BoxSizing::ContentBox => height.max(0.0),
    })
}

/// Resolve the used content size of an HTML `<canvas>` replaced element.
///
/// HTML gives canvas a default intrinsic size of 300 by 150 CSS pixels, while
/// CSS Images/Sizing resolves replaced auto sizes from intrinsic dimensions and
/// ratio:
/// <https://html.spec.whatwg.org/multipage/canvas.html#attr-canvas-width>,
/// <https://www.w3.org/TR/css-images-3/#default-sizing>.
pub(super) fn used_canvas_size(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
) -> (f32, f32) {
    used_canvas_size_with_height_basis(element, style, available_width, None)
}

/// Resolve the used content size of an HTML `<canvas>` with an optional
/// containing-block height for percentage resolution.
///
/// CSS 2.2 treats percentage heights as auto unless the containing block has a
/// definite block size. Table-cell content relayout can provide that definite
/// basis after row height distribution:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-height-property> and
/// <https://drafts.csswg.org/css-tables-3/#table-cell-content-relayout>.
pub(super) fn used_canvas_size_with_height_basis(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
    height_basis: Option<f32>,
) -> (f32, f32) {
    let intrinsic_width = element
        .attrs
        .get("width")
        .and_then(|value| parse_html_length(value))
        .filter(|value| *value > 0.0)
        .unwrap_or(300.0 * css::CSS_PX_TO_PT);
    let intrinsic_height = element
        .attrs
        .get("height")
        .and_then(|value| parse_html_length(value))
        .filter(|value| *value > 0.0)
        .unwrap_or(150.0 * css::CSS_PX_TO_PT);
    let aspect_ratio = intrinsic_width / intrinsic_height;
    let mut width = used_length_percentage_or_auto(style.box_values.width, available_width);
    let vertical_non_content =
        style.padding.top + style.padding.bottom + vertical_border_width(style);
    let mut height =
        used_content_height_or_auto_with_optional_basis(style, height_basis, vertical_non_content);
    let width_was_auto = width.is_none();
    match (width, height) {
        (Some(width_value), None) => height = Some(width_value / aspect_ratio),
        (None, Some(height_value)) => width = Some(height_value * aspect_ratio),
        (None, None) => {
            width = Some(intrinsic_width);
            height = Some(intrinsic_height);
        }
        (Some(_), Some(_)) => {}
    }
    let mut width = width.unwrap_or(intrinsic_width);
    let mut height = height.unwrap_or(intrinsic_height);
    constrain_canvas_height(
        style,
        height_basis,
        vertical_non_content,
        aspect_ratio,
        width_was_auto,
        &mut width,
        &mut height,
    );
    (width, height)
}

fn constrain_canvas_height(
    style: &ComputedStyle,
    height_basis: Option<f32>,
    vertical_non_content: f32,
    aspect_ratio: f32,
    width_was_auto: bool,
    width: &mut f32,
    height: &mut f32,
) {
    let constraints = CanvasHeightConstraints {
        min_height: used_canvas_content_height_constraint(
            style.box_values.min_height,
            style.box_sizing,
            height_basis,
            vertical_non_content,
        ),
        max_height: used_canvas_content_height_constraint(
            style.box_values.max_height,
            style.box_sizing,
            height_basis,
            vertical_non_content,
        ),
    };
    if let Some(min_height) = constraints.min_height
        && *height < min_height
    {
        *height = min_height;
        if width_was_auto {
            *width = *height * aspect_ratio;
        }
    }
    if let Some(max_height) = constraints.max_height
        && *height > max_height
    {
        *height = max_height;
        if width_was_auto {
            *width = *height * aspect_ratio;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CanvasHeightConstraints {
    min_height: Option<f32>,
    max_height: Option<f32>,
}

fn used_canvas_content_height_constraint(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    height_basis: Option<f32>,
    vertical_non_content: f32,
) -> Option<f32> {
    let specified = used_length_percentage_or_auto_with_optional_basis(value, height_basis)?;
    Some(match box_sizing {
        BoxSizing::BorderBox => (specified - vertical_non_content).max(0.0),
        BoxSizing::ContentBox => specified.max(0.0),
    })
}

pub(super) fn used_background_size(
    image: &DecodedPngImage,
    area_width: f32,
    area_height: f32,
    value: css::BackgroundSize,
) -> (f32, f32) {
    let intrinsic_width = image.pixel_width as f32;
    let intrinsic_height = image.pixel_height as f32;
    if intrinsic_width <= 0.0 || intrinsic_height <= 0.0 {
        return (0.0, 0.0);
    }
    let aspect_ratio = intrinsic_width / intrinsic_height;
    if value == css::BackgroundSize::Auto {
        return (intrinsic_width, intrinsic_height);
    }
    if value == css::BackgroundSize::Cover {
        let scale = (area_width / intrinsic_width).max(area_height / intrinsic_height);
        return (intrinsic_width * scale, intrinsic_height * scale);
    }
    if value == css::BackgroundSize::Contain {
        let scale = (area_width / intrinsic_width).min(area_height / intrinsic_height);
        return (intrinsic_width * scale, intrinsic_height * scale);
    }
    let css::BackgroundSize::Explicit { width, height } = value else {
        return (intrinsic_width, intrinsic_height);
    };
    let first = used_background_size_axis(width, area_width);
    let second = used_background_size_axis(height, area_height);
    match (first, second) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (width, width / aspect_ratio),
        (None, Some(height)) => (height * aspect_ratio, height),
        (None, None) => (intrinsic_width, intrinsic_height),
    }
}

/// Resolves one computed `background-size` axis.
///
/// CSS Backgrounds and Borders resolves explicit size percentages against the
/// background positioning area:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>.
pub(super) fn used_background_size_axis(
    value: css::BackgroundSizeAxis,
    reference: f32,
) -> Option<f32> {
    match value {
        css::BackgroundSizeAxis::Auto => None,
        css::BackgroundSizeAxis::LengthPercentage(value) => {
            Some(used_length_percentage(value, reference).max(0.0))
        }
    }
}

pub(super) fn background_position(
    value: css::BackgroundPosition,
    area_width: f32,
    area_height: f32,
    image_width: f32,
    image_height: f32,
) -> (f32, f32) {
    let free_x = area_width - image_width;
    let free_y = area_height - image_height;
    (
        used_background_position_axis(value.x, free_x, false),
        used_background_position_axis(value.y, free_y, true),
    )
}

#[derive(Debug, Clone, Copy)]
pub(super) struct UsedBorderImageSlices {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

/// Resolve `border-image-slice` against the source image dimensions.
///
/// CSS Backgrounds and Borders resolves percentages against image dimensions
/// and proportionally reduces opposing slices when their sum exceeds the image
/// size:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>.
pub(super) fn used_border_image_slices(
    values: css::BorderImageSliceOffsets,
    image_width: u32,
    image_height: u32,
) -> UsedBorderImageSlices {
    let mut top = used_border_image_slice_value(values.top, image_height);
    let mut right = used_border_image_slice_value(values.right, image_width);
    let mut bottom = used_border_image_slice_value(values.bottom, image_height);
    let mut left = used_border_image_slice_value(values.left, image_width);
    reduce_opposing_slices(&mut top, &mut bottom, image_height);
    reduce_opposing_slices(&mut left, &mut right, image_width);
    UsedBorderImageSlices {
        top,
        right,
        bottom,
        left,
    }
}

fn used_border_image_slice_value(value: css::BorderImageSliceValue, reference: u32) -> u32 {
    let resolved = match value {
        css::BorderImageSliceValue::Number(value) => value,
        css::BorderImageSliceValue::Percent(value) => value * reference as f32,
    };
    resolved.max(0.0).round() as u32
}

fn reduce_opposing_slices(first: &mut u32, second: &mut u32, reference: u32) {
    let sum = first.saturating_add(*second);
    if sum <= reference || sum == 0 {
        return;
    }
    let scale = reference as f32 / sum as f32;
    *first = ((*first as f32) * scale).round() as u32;
    *second = reference.saturating_sub(*first);
}

/// Resolve `border-image-width` to destination border-image side widths.
///
/// Numeric values multiply the corresponding used border width; lengths and
/// percentages resolve against the border image area dimensions. `auto` uses
/// the intrinsic size of the corresponding image slice, falling back to the
/// used border width only when that slice dimension is unavailable:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-width>.
pub(super) fn used_border_image_widths(
    style: &ComputedStyle,
    border_widths: css::Edges,
    border_box_width: f32,
    border_box_height: f32,
    slices: UsedBorderImageSlices,
) -> css::Edges {
    css::Edges {
        top: used_border_image_width_value(
            style.border_image.width.top,
            border_widths.top,
            border_box_height,
            slices.top,
        ),
        right: used_border_image_width_value(
            style.border_image.width.right,
            border_widths.right,
            border_box_width,
            slices.right,
        ),
        bottom: used_border_image_width_value(
            style.border_image.width.bottom,
            border_widths.bottom,
            border_box_height,
            slices.bottom,
        ),
        left: used_border_image_width_value(
            style.border_image.width.left,
            border_widths.left,
            border_box_width,
            slices.left,
        ),
    }
}

fn used_border_image_width_value(
    value: css::BorderImageWidthValue,
    border_width: f32,
    reference: f32,
    slice_width: u32,
) -> f32 {
    match value {
        css::BorderImageWidthValue::Auto => {
            if slice_width > 0 {
                slice_width as f32
            } else {
                border_width
            }
        }
        css::BorderImageWidthValue::Number(value) => border_width * value,
        css::BorderImageWidthValue::LengthPercentage(value) => {
            used_length_percentage(value, reference)
        }
    }
    .max(0.0)
}

/// Proportionally fit border-image widths inside the border-image area.
///
/// CSS Backgrounds and Borders scales all four used `border-image-width`
/// values down by a common factor when opposite sides would overlap:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>.
pub(super) fn fit_border_image_widths_to_area(
    widths: css::Edges,
    area_width: f32,
    area_height: f32,
) -> css::Edges {
    let horizontal_sum = widths.left + widths.right;
    let vertical_sum = widths.top + widths.bottom;
    let mut factor = 1.0_f32;
    if horizontal_sum > area_width && horizontal_sum > 0.0 {
        factor = factor.min(area_width / horizontal_sum);
    }
    if vertical_sum > area_height && vertical_sum > 0.0 {
        factor = factor.min(area_height / vertical_sum);
    }
    if factor >= 1.0 {
        widths
    } else {
        css::Edges {
            top: widths.top * factor,
            right: widths.right * factor,
            bottom: widths.bottom * factor,
            left: widths.left * factor,
        }
    }
}

/// Resolve `border-image-outset` to physical outsets.
///
/// Numeric values multiply the corresponding used border width; length values
/// are absolute:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-outset>.
pub(super) fn used_border_image_outsets(
    style: &ComputedStyle,
    border_widths: css::Edges,
) -> css::Edges {
    css::Edges {
        top: used_border_image_outset_value(style.border_image.outset.top, border_widths.top),
        right: used_border_image_outset_value(style.border_image.outset.right, border_widths.right),
        bottom: used_border_image_outset_value(
            style.border_image.outset.bottom,
            border_widths.bottom,
        ),
        left: used_border_image_outset_value(style.border_image.outset.left, border_widths.left),
    }
}

fn used_border_image_outset_value(value: css::BorderImageOutsetValue, border_width: f32) -> f32 {
    match value {
        css::BorderImageOutsetValue::Number(value) => value * border_width,
        css::BorderImageOutsetValue::Length(value) => value.length,
    }
    .max(0.0)
}

/// Resolves one computed `background-position` axis to the renderer's PDF
/// coordinate space.
///
/// CSS Backgrounds and Borders positions images in the positioning area; the
/// vertical result is inverted here because PDF page coordinates in this
/// renderer are top-origin for rectangles but image placement uses bottom
/// offsets:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-position>.
pub(super) fn used_background_position_axis(
    axis: css::BackgroundPositionAxis,
    free_space: f32,
    invert_start_end: bool,
) -> f32 {
    let offset = used_length_percentage(axis.offset, free_space);
    match (axis.origin, invert_start_end) {
        (css::BackgroundPositionOrigin::Start, false) => offset,
        (css::BackgroundPositionOrigin::Start, true) => free_space - offset,
        (css::BackgroundPositionOrigin::Center, _) => free_space / 2.0 + offset,
        (css::BackgroundPositionOrigin::End, false) => free_space - offset,
        (css::BackgroundPositionOrigin::End, true) => offset,
    }
}

pub(super) fn inline_replaced_descent(style: &ComputedStyle) -> f32 {
    // CSS inline replaced elements align to the text baseline by default.
    // Reserve a conservative descender area below the image until line layout
    // has real font ascent/descent metrics.
    (style.line_height * 0.25).max(0.0)
}

pub(super) fn svg_rect(element: &Element) -> Option<(f32, f32, Color)> {
    let svg_width = element
        .attrs
        .get("width")
        .and_then(|value| parse_html_length(value));
    let svg_height = element
        .attrs
        .get("height")
        .and_then(|value| parse_html_length(value));
    for child in &element.children {
        let NodeKind::Element(rect) = &child.kind else {
            continue;
        };
        if rect.tag != "rect" {
            continue;
        }
        let width = rect
            .attrs
            .get("width")
            .and_then(|value| parse_html_length(value))
            .or(svg_width)?;
        let height = rect
            .attrs
            .get("height")
            .and_then(|value| parse_html_length(value))
            .or(svg_height)?;
        let fill = rect
            .attrs
            .get("fill")
            .and_then(|value| css::parse_color(value))
            .unwrap_or(Color::BLACK);
        return Some((width, height, fill));
    }
    None
}

pub(super) fn estimate_svg_height(element: &Element, style: &ComputedStyle) -> f32 {
    let (_, height, _) =
        svg_rect(element).unwrap_or((style.font_size, style.line_height, style.color));
    style.margin.top + height + style.margin.bottom
}

pub(super) fn parse_html_length(value: &str) -> Option<f32> {
    let value = value.trim();
    if let Some(number) = value.strip_suffix("px") {
        return number
            .trim()
            .parse::<f32>()
            .ok()
            .map(|value| value * css::CSS_PX_TO_PT);
    }
    if let Some(number) = value.strip_suffix("pt") {
        return number.trim().parse().ok();
    }
    value
        .parse::<f32>()
        .ok()
        .map(|value| value * css::CSS_PX_TO_PT)
}
