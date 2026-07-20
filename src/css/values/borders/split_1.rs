use super::*;
use crate::css::{ParsedImage, parse_css_image};

#[derive(Debug, Clone, Copy)]
pub(crate) enum BorderSide {
    Top,
    Right,
    Bottom,
    Left,
}

impl From<PhysicalSide> for BorderSide {
    fn from(side: PhysicalSide) -> Self {
        match side {
            PhysicalSide::Top => Self::Top,
            PhysicalSide::Right => Self::Right,
            PhysicalSide::Bottom => Self::Bottom,
            PhysicalSide::Left => Self::Left,
        }
    }
}

/// Maps a logical border side to a physical side.
///
/// CSS Logical Properties maps block/inline sides through the computed
/// `writing-mode` and `direction` values:
/// <https://www.w3.org/TR/css-logical-1/#border-properties>.
pub(crate) fn logical_border_side(
    name: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<BorderSide> {
    let axes = WritingModeAxes::new(writing_mode, direction);
    match name {
        "border-block-start"
        | "border-block-start-width"
        | "border-block-start-style"
        | "border-block-start-color" => Some(axes.physical_side(LogicalSide::BlockStart).into()),
        "border-block-end"
        | "border-block-end-width"
        | "border-block-end-style"
        | "border-block-end-color" => Some(axes.physical_side(LogicalSide::BlockEnd).into()),
        "border-inline-start"
        | "border-inline-start-width"
        | "border-inline-start-style"
        | "border-inline-start-color" => Some(axes.physical_side(LogicalSide::InlineStart).into()),
        "border-inline-end"
        | "border-inline-end-width"
        | "border-inline-end-style"
        | "border-inline-end-color" => Some(axes.physical_side(LogicalSide::InlineEnd).into()),
        _ => None,
    }
}

/// Maps a logical corner radius property to a physical corner.
///
/// CSS Logical Properties defines flow-relative corner radius longhands that
/// combine one block side and one inline side:
/// <https://www.w3.org/TR/css-logical-1/#border-radius-properties>.
pub(crate) fn logical_corner_radius_longhand(
    name: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<&'static str> {
    let block_start = logical_border_side("border-block-start", direction, writing_mode)?;
    let block_end = logical_border_side("border-block-end", direction, writing_mode)?;
    let inline_start = logical_border_side("border-inline-start", direction, writing_mode)?;
    let inline_end = logical_border_side("border-inline-end", direction, writing_mode)?;
    let (block_side, inline_side) = match name {
        "border-start-start-radius" => (block_start, inline_start),
        "border-start-end-radius" => (block_start, inline_end),
        "border-end-start-radius" => (block_end, inline_start),
        "border-end-end-radius" => (block_end, inline_end),
        _ => return None,
    };
    physical_corner_radius_longhand(block_side, inline_side)
}

pub(in crate::css) fn physical_corner_radius_longhand(
    first: BorderSide,
    second: BorderSide,
) -> Option<&'static str> {
    match (first, second) {
        (BorderSide::Top, BorderSide::Left) | (BorderSide::Left, BorderSide::Top) => {
            Some("border-top-left-radius")
        }
        (BorderSide::Top, BorderSide::Right) | (BorderSide::Right, BorderSide::Top) => {
            Some("border-top-right-radius")
        }
        (BorderSide::Bottom, BorderSide::Right) | (BorderSide::Right, BorderSide::Bottom) => {
            Some("border-bottom-right-radius")
        }
        (BorderSide::Bottom, BorderSide::Left) | (BorderSide::Left, BorderSide::Bottom) => {
            Some("border-bottom-left-radius")
        }
        _ => None,
    }
}

pub(crate) fn apply_border(value: &str, style: &mut ComputedStyle, side: Option<BorderSide>) {
    let mut width = None;
    let mut border_style = None;
    let mut color = None;
    for part in split_css_component_values(value) {
        let mut recognized = false;
        if width.is_none() {
            width = parse_computed_border_width(part, style.font_size);
            recognized |= width.is_some();
        }
        if border_style.is_none() {
            border_style = parse_border_style(part);
            recognized |= border_style.is_some();
        }
        if color.is_none() {
            color = parse_border_color(part, style.color);
            recognized |= color.is_some();
        }
        if !recognized {
            return;
        }
    }

    let width = width.unwrap_or(ComputedLengthPercentage::from_points(3.0 * CSS_PX_TO_PT));
    let border_style = border_style.unwrap_or(BorderStyle::None);
    let color = color.unwrap_or(style.color);

    if let Some(side) = side {
        set_border_side_width(style, side, width);
        set_border_side_style_value(style, side, border_style);
        set_border_side_color(style, side, color);
    } else {
        let used_width = used_nonnegative_length(width.clone()).points();
        style.border_width = used_width;
        style.border_widths = edge_all(used_width);
        style.border_width_values = CssEdges::all(width);
        style.border_styles = border_styles_all(border_style);
        style.border_color = color;
        style.border_colors = border_colors_all(color);
    }
}

/// Applies a logical border side shorthand using the style's flow direction.
///
/// CSS Logical Properties defines `border-block-start`, `border-block-end`,
/// `border-inline-start`, and `border-inline-end` as flow-relative aliases for
/// the physical side border shorthands:
/// <https://www.w3.org/TR/css-logical-1/#border-properties>.
pub(crate) fn apply_logical_border(value: &str, style: &mut ComputedStyle, name: &str) {
    if let Some(side) = logical_border_side(name, style.direction, style.writing_mode) {
        apply_border(value, style, Some(side));
    }
}

/// Applies `border-block` or `border-inline` using the style's flow direction.
///
/// The logical-axis border shorthands set both sides on the block or inline
/// axis after mapping those axes through `writing-mode` and `direction`:
/// <https://www.w3.org/TR/css-logical-1/#border-shorthands>.
pub(crate) fn apply_logical_border_axis(value: &str, style: &mut ComputedStyle, name: &str) {
    let logical_sides = match name {
        "border-block" => ["border-block-start", "border-block-end"],
        "border-inline" => ["border-inline-start", "border-inline-end"],
        _ => return,
    };
    for logical_side in logical_sides {
        if let Some(side) = logical_border_side(logical_side, style.direction, style.writing_mode) {
            apply_border(value, style, Some(side));
        }
    }
}

pub(crate) fn parse_border_width_with_font_size(value: &str, font_size: f32) -> Option<f32> {
    parse_computed_border_width(value, font_size)?.length_if_no_percent()
}

pub(crate) fn parse_computed_border_width(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "thin" => Some(ComputedLengthPercentage::from_points(CSS_PX_TO_PT)),
        "medium" => Some(ComputedLengthPercentage::from_points(3.0 * CSS_PX_TO_PT)),
        "thick" => Some(ComputedLengthPercentage::from_points(5.0 * CSS_PX_TO_PT)),
        _ => {
            let length = parse_computed_length_percentage(value, font_size)?;
            (!length.needs_percentage_basis() && !length.is_definitely_negative()).then_some(length)
        }
    }
}

pub(crate) fn parse_border_width_edges(
    value: &str,
    font_size: f32,
) -> Option<CssEdges<ComputedLengthPercentage>> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_computed_border_width(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some(CssEdges::all(all.clone())),
        [vertical, horizontal] => Some(CssEdges {
            top: vertical.clone(),
            right: horizontal.clone(),
            bottom: vertical.clone(),
            left: horizontal.clone(),
        }),
        [top, horizontal, bottom] => Some(CssEdges {
            top: top.clone(),
            right: horizontal.clone(),
            bottom: bottom.clone(),
            left: horizontal.clone(),
        }),
        [top, right, bottom, left] => Some(CssEdges {
            top: top.clone(),
            right: right.clone(),
            bottom: bottom.clone(),
            left: left.clone(),
        }),
        _ => None,
    }
}

/// Parse one or two logical-axis border width values.
///
/// CSS Logical Properties defines `border-block-width` and
/// `border-inline-width` as two-value shorthands for start/end widths:
/// <https://www.w3.org/TR/css-logical-1/#border-shorthands>.
pub(crate) fn parse_logical_border_widths(
    value: &str,
    font_size: f32,
) -> Option<[ComputedLengthPercentage; 2]> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_computed_border_width(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some([all.clone(), all.clone()]),
        [start, end] => Some([start.clone(), end.clone()]),
        _ => None,
    }
}

/// Parse one or two logical-axis border styles.
///
/// CSS Logical Properties defines `border-block-style` and
/// `border-inline-style` as two-value shorthands for start/end styles:
/// <https://www.w3.org/TR/css-logical-1/#border-shorthands>.
pub(crate) fn parse_logical_border_styles(value: &str) -> Option<[BorderStyle; 2]> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(parse_border_style)
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some([*all, *all]),
        [start, end] => Some([*start, *end]),
        _ => None,
    }
}

/// Parse one or two logical-axis border colors.
///
/// CSS Logical Properties defines `border-block-color` and
/// `border-inline-color` as two-value shorthands for start/end colors:
/// <https://www.w3.org/TR/css-logical-1/#border-shorthands>.
pub(crate) fn parse_logical_border_colors(
    value: &str,
    current_color: CssColor,
) -> Option<[CssColor; 2]> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_border_color(part, current_color))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some([*all, *all]),
        [start, end] => Some([*start, *end]),
        _ => None,
    }
}

pub(crate) fn parse_border_style(value: &str) -> Option<BorderStyle> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "none" => Some(BorderStyle::None),
        "hidden" => Some(BorderStyle::Hidden),
        "dotted" => Some(BorderStyle::Dotted),
        "dashed" => Some(BorderStyle::Dashed),
        "solid" => Some(BorderStyle::Solid),
        "double" => Some(BorderStyle::Double),
        "groove" => Some(BorderStyle::Groove),
        "ridge" => Some(BorderStyle::Ridge),
        "inset" => Some(BorderStyle::Inset),
        "outset" => Some(BorderStyle::Outset),
        _ => None,
    }
}

/// Parses one border color component, including `currentColor`.
///
/// CSS Backgrounds and Borders defines the initial border color as
/// `currentColor`, and CSS CssColor defines the keyword as the element's computed
/// `color` value:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-color> and
/// <https://www.w3.org/TR/css-color-4/#currentcolor-color>.
pub(crate) fn parse_border_color(value: &str, current_color: CssColor) -> Option<CssColor> {
    if trim_css_value(value).eq_ignore_ascii_case("currentcolor") {
        Some(current_color)
    } else {
        parse_color(value)
    }
}

/// Parse one to four `border-color` components.
///
/// CSS Backgrounds and Borders Level 3 defines `border-color` as the
/// one-to-four-value box-edge shorthand for the physical border color
/// longhands:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-color>.
pub(crate) fn parse_border_colors(value: &str, current_color: CssColor) -> Option<BorderColors> {
    let colors = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_border_color(part, current_color))
        .collect::<Option<Vec<_>>>()?;
    match colors.as_slice() {
        [all] => Some(border_colors_all(*all)),
        [vertical, horizontal] => Some(BorderColors {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(BorderColors {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(BorderColors {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

/// Parse one to four `border-style` components.
///
/// CSS Backgrounds and Borders Level 3 defines `border-style` as the
/// one-to-four-value box-edge shorthand for the physical border style
/// longhands:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-style>.
pub(crate) fn parse_border_styles(value: &str) -> Option<BorderStyles> {
    let styles = split_css_component_values(value)
        .into_iter()
        .map(parse_border_style)
        .collect::<Option<Vec<_>>>()?;
    match styles.as_slice() {
        [all] => Some(border_styles_all(*all)),
        [vertical, horizontal] => Some(BorderStyles {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(BorderStyles {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(BorderStyles {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

/// Parse `border-image-source`.
///
/// CSS Backgrounds and Borders defines the initial value as `none` and accepts
/// image values:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-source>.
pub(crate) fn parse_border_image_source(value: &str) -> Option<ComputedImage> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        Some(ComputedImage::None)
    } else {
        match parse_css_image(value, None, None) {
            ParsedImage::Image(image) => Some(image),
            ParsedImage::NotAnImage | ParsedImage::SyntaxError => None,
        }
    }
}

/// Parse the `border-image` shorthand.
///
/// CSS Backgrounds and Borders Level 3 defines `border-image` as a shorthand
/// for source, slice, width, outset, and repeat. Unspecified longhands reset to
/// their initial values, while slash-separated width/outset groups are only
/// valid when a slice group is present:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-image>.
pub(crate) fn parse_border_image(value: &str, font_size: f32) -> Option<BorderImage> {
    let groups = split_css_top_level_slashes(value);
    if groups.is_empty() || groups.len() > 3 {
        return None;
    }

    let mut image = BorderImage::initial();
    let mut source_seen = false;
    let mut slice_tokens = Vec::new();
    let mut repeat_tokens = Vec::new();
    let mut width_tokens = Vec::new();
    let mut outset_tokens = Vec::new();

    for part in split_css_component_values(groups[0]) {
        if parse_border_image_source_token(part, &mut image, &mut source_seen)? {
            continue;
        }
        if parse_border_image_repeat_keyword(part).is_some() {
            repeat_tokens.push(part);
            continue;
        }
        slice_tokens.push(part);
    }

    for part in groups
        .get(1)
        .into_iter()
        .flat_map(|group| split_css_component_values(group))
    {
        if parse_border_image_source_token(part, &mut image, &mut source_seen)? {
            continue;
        }
        if parse_border_image_repeat_keyword(part).is_some() {
            repeat_tokens.push(part);
            continue;
        }
        width_tokens.push(part);
    }

    for part in groups
        .get(2)
        .into_iter()
        .flat_map(|group| split_css_component_values(group))
    {
        if parse_border_image_source_token(part, &mut image, &mut source_seen)? {
            continue;
        }
        if parse_border_image_repeat_keyword(part).is_some() {
            repeat_tokens.push(part);
            continue;
        }
        outset_tokens.push(part);
    }

    if groups.len() > 1 && slice_tokens.is_empty() {
        return None;
    }
    if !slice_tokens.is_empty() {
        image.slice = parse_border_image_slice(&slice_tokens.join(" "))?;
    }
    if !width_tokens.is_empty() {
        image.width = parse_border_image_width(&width_tokens.join(" "), font_size)?;
    }
    if !outset_tokens.is_empty() {
        image.outset = parse_border_image_outset(&outset_tokens.join(" "), font_size)?;
    }
    if !repeat_tokens.is_empty() {
        image.repeat = parse_border_image_repeat(&repeat_tokens.join(" "))?;
    }

    Some(image)
}

pub(in crate::css) fn parse_border_image_source_token(
    token: &str,
    image: &mut BorderImage,
    source_seen: &mut bool,
) -> Option<bool> {
    let source = parse_border_image_source(token);
    if let Some(source) = source {
        if *source_seen {
            return None;
        }
        image.source = source;
        *source_seen = true;
        Some(true)
    } else {
        Some(false)
    }
}

/// Parse `border-image-slice`.
///
/// Unitless numbers and percentages are stored until the source image size is
/// known at used-value time; an optional `fill` keyword is preserved:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>.
pub(crate) fn parse_border_image_slice(value: &str) -> Option<BorderImageSlice> {
    let mut fill = false;
    let mut offsets = Vec::new();
    for part in split_css_component_values(value) {
        if part.eq_ignore_ascii_case("fill") {
            if fill {
                return None;
            }
            fill = true;
            continue;
        }
        offsets.push(parse_border_image_slice_value(part)?);
    }
    Some(BorderImageSlice {
        offsets: expand_border_image_slice_offsets(&offsets)?,
        fill,
    })
}

pub(in crate::css) fn parse_border_image_slice_value(value: &str) -> Option<BorderImageSliceValue> {
    parse_percentage(value)
        .map(BorderImageSliceValue::Percent)
        .or_else(|| parse_non_negative_number(value).map(BorderImageSliceValue::Number))
}

pub(in crate::css) fn expand_border_image_slice_offsets(
    values: &[BorderImageSliceValue],
) -> Option<BorderImageSliceOffsets> {
    match values {
        [all] => Some(BorderImageSliceOffsets {
            top: *all,
            right: *all,
            bottom: *all,
            left: *all,
        }),
        [vertical, horizontal] => Some(BorderImageSliceOffsets {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(BorderImageSliceOffsets {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(BorderImageSliceOffsets {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

/// Parse `border-image-width`.
///
/// Numeric values multiply border widths; explicit lengths and percentages are
/// represented as computed length-percentage values:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-width>.
pub(crate) fn parse_border_image_width(value: &str, font_size: f32) -> Option<BorderImageWidth> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_border_image_width_value(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some(BorderImageWidth {
            top: all.clone(),
            right: all.clone(),
            bottom: all.clone(),
            left: all.clone(),
        }),
        [vertical, horizontal] => Some(BorderImageWidth {
            top: vertical.clone(),
            right: horizontal.clone(),
            bottom: vertical.clone(),
            left: horizontal.clone(),
        }),
        [top, horizontal, bottom] => Some(BorderImageWidth {
            top: top.clone(),
            right: horizontal.clone(),
            bottom: bottom.clone(),
            left: horizontal.clone(),
        }),
        [top, right, bottom, left] => Some(BorderImageWidth {
            top: top.clone(),
            right: right.clone(),
            bottom: bottom.clone(),
            left: left.clone(),
        }),
        _ => None,
    }
}

pub(in crate::css) fn parse_border_image_width_value(
    value: &str,
    font_size: f32,
) -> Option<BorderImageWidthValue> {
    if trim_css_value(value).eq_ignore_ascii_case("auto") {
        return Some(BorderImageWidthValue::Auto);
    }
    parse_non_negative_number(value)
        .map(BorderImageWidthValue::Number)
        .or_else(|| {
            parse_computed_length_percentage(value, font_size)
                .map(BorderImageWidthValue::LengthPercentage)
        })
}

/// Parse `border-image-outset`.
///
/// Numeric values multiply border widths; lengths are used directly:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-outset>.
pub(crate) fn parse_border_image_outset(value: &str, font_size: f32) -> Option<BorderImageOutset> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_border_image_outset_value(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some(BorderImageOutset {
            top: all.clone(),
            right: all.clone(),
            bottom: all.clone(),
            left: all.clone(),
        }),
        [vertical, horizontal] => Some(BorderImageOutset {
            top: vertical.clone(),
            right: horizontal.clone(),
            bottom: vertical.clone(),
            left: horizontal.clone(),
        }),
        [top, horizontal, bottom] => Some(BorderImageOutset {
            top: top.clone(),
            right: horizontal.clone(),
            bottom: bottom.clone(),
            left: horizontal.clone(),
        }),
        [top, right, bottom, left] => Some(BorderImageOutset {
            top: top.clone(),
            right: right.clone(),
            bottom: bottom.clone(),
            left: left.clone(),
        }),
        _ => None,
    }
}

pub(in crate::css) fn parse_border_image_outset_value(
    value: &str,
    font_size: f32,
) -> Option<BorderImageOutsetValue> {
    parse_non_negative_number(value)
        .map(BorderImageOutsetValue::Number)
        .or_else(|| {
            parse_computed_length_percentage(value, font_size).and_then(|value| {
                (!value.needs_percentage_basis() && !value.is_definitely_negative())
                    .then_some(BorderImageOutsetValue::Length(value))
            })
        })
}

/// Parse `border-image-repeat`.
///
/// One keyword applies to both axes; two keywords are horizontal then vertical:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-repeat>.
pub(crate) fn parse_border_image_repeat(value: &str) -> Option<BorderImageRepeat> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(parse_border_image_repeat_keyword)
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some(BorderImageRepeat {
            horizontal: *all,
            vertical: *all,
        }),
        [horizontal, vertical] => Some(BorderImageRepeat {
            horizontal: *horizontal,
            vertical: *vertical,
        }),
        _ => None,
    }
}

pub(in crate::css) fn parse_border_image_repeat_keyword(
    value: &str,
) -> Option<BorderImageRepeatKeyword> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "stretch" => Some(BorderImageRepeatKeyword::Stretch),
        "repeat" => Some(BorderImageRepeatKeyword::Repeat),
        "round" => Some(BorderImageRepeatKeyword::Round),
        "space" => Some(BorderImageRepeatKeyword::Space),
        _ => None,
    }
}

pub(in crate::css) fn parse_non_negative_number(value: &str) -> Option<f32> {
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    let value = parser.expect_number().ok()?;
    (value >= 0.0 && parser.is_exhausted()).then_some(value)
}

/// Splits a CSS value into top-level component values.
///
/// CSS Syntax tokenization treats function contents as nested component
/// values, so whitespace inside `rgb(255 0 0)` must not split a border
/// shorthand component:
/// <https://www.w3.org/TR/css-syntax-3/#component-value>.
pub(crate) fn split_css_component_values(value: &str) -> Vec<&str> {
    crate::css::component_values::split_css_component_values(trim_css_value(value))
}

pub(in crate::css) fn split_css_top_level_slashes(value: &str) -> Vec<&str> {
    crate::css::component_values::split_css_top_level_delimiter(trim_css_value(value), '/')
}

/// Parse `border-radius` using the CSS Backgrounds and Borders shorthand grammar.
///
/// See CSS Backgrounds and Borders Level 3, §5.1 "Curve Radii: the
/// border-radius properties". Percentages are preserved until used-value time
/// because horizontal radii resolve against border-box width and vertical radii
/// resolve against border-box height.
pub(crate) fn parse_border_radius(value: &str, font_size: f32) -> Option<BorderRadius> {
    let value = trim_css_value(value);
    let (horizontal_value, vertical_value) =
        crate::css::component_values::split_css_top_level_once(value, '/')
            .map(|(horizontal, vertical)| (horizontal, Some(vertical)))
            .unwrap_or((value, None));
    let horizontal = parse_radius_components(horizontal_value.trim(), font_size)?;
    let vertical = if let Some(group) = vertical_value {
        parse_radius_components(group.trim(), font_size)?
    } else {
        horizontal.clone()
    };
    Some(BorderRadius {
        top_left: CornerRadius {
            x: horizontal.top,
            y: vertical.top,
        },
        top_right: CornerRadius {
            x: horizontal.right,
            y: vertical.right,
        },
        bottom_right: CornerRadius {
            x: horizontal.bottom,
            y: vertical.bottom,
        },
        bottom_left: CornerRadius {
            x: horizontal.left,
            y: vertical.left,
        },
    })
}

/// Parse a `border-*-*-radius` longhand.
///
/// CSS Backgrounds and Borders Level 3 defines corner radius longhands as one
/// or two non-negative `<length-percentage>` values. The first value is the
/// horizontal radius, and the second value, when present, is the vertical
/// radius:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-radius>.
pub(crate) fn parse_corner_radius(value: &str, font_size: f32) -> Option<CornerRadius> {
    let radii = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_radius_value(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    match radii.as_slice() {
        [all] => Some(CornerRadius {
            x: all.clone(),
            y: all.clone(),
        }),
        [x, y] => Some(CornerRadius {
            x: x.clone(),
            y: y.clone(),
        }),
        _ => None,
    }
}

/// Parse one `corner-*-shape` value.
///
/// CSS Borders and Box Decorations Level 4 defines `superellipse()` and
/// keyword aliases for common superellipse corner shapes:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape>.
pub(crate) fn parse_corner_shape(value: &str) -> Option<CornerShape> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    match lower.as_str() {
        "round" => Some(CornerShape::ROUND),
        "squircle" => Some(CornerShape::SQUIRCLE),
        "square" => Some(CornerShape::SQUARE),
        "bevel" => Some(CornerShape::BEVEL),
        "scoop" => Some(CornerShape::SCOOP),
        "notch" => Some(CornerShape::NOTCH),
        _ => parse_superellipse_function(&lower),
    }
}

fn parse_superellipse_function(value: &str) -> Option<CornerShape> {
    let argument = value
        .strip_prefix("superellipse(")?
        .strip_suffix(')')
        .map(trim_css_value)?;
    let parameter = match argument {
        "infinity" | "+infinity" => SuperellipseParameter::Infinity,
        "-infinity" => SuperellipseParameter::NegativeInfinity,
        _ => {
            let mut input = ParserInput::new(argument);
            let mut parser = Parser::new(&mut input);
            let number = parser.expect_number().ok()?;
            if !number.is_finite() || !parser.is_exhausted() {
                return None;
            }
            SuperellipseParameter::Number(number)
        }
    };
    Some(CornerShape::superellipse(parameter))
}

/// Parse the `corner-shape` shorthand's one-to-four physical corner values.
///
/// The expansion order matches `border-radius`: top-left, top-right,
/// bottom-right, bottom-left:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape-shorthand>.
pub(crate) fn parse_corner_shapes(value: &str) -> Option<CornerShapes> {
    let shapes = split_css_component_values(value)
        .into_iter()
        .map(parse_corner_shape)
        .collect::<Option<Vec<_>>>()?;
    match shapes.as_slice() {
        [all] => Some(CornerShapes {
            top_left: *all,
            top_right: *all,
            bottom_right: *all,
            bottom_left: *all,
        }),
        [vertical, horizontal] => Some(CornerShapes {
            top_left: *vertical,
            top_right: *horizontal,
            bottom_right: *vertical,
            bottom_left: *horizontal,
        }),
        [top_left, horizontal, bottom_right] => Some(CornerShapes {
            top_left: *top_left,
            top_right: *horizontal,
            bottom_right: *bottom_right,
            bottom_left: *horizontal,
        }),
        [top_left, top_right, bottom_right, bottom_left] => Some(CornerShapes {
            top_left: *top_left,
            top_right: *top_right,
            bottom_right: *bottom_right,
            bottom_left: *bottom_left,
        }),
        _ => None,
    }
}
