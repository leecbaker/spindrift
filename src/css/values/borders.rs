use super::*;

#[derive(Debug, Clone, Copy)]
pub(crate) enum BorderSide {
    Top,
    Right,
    Bottom,
    Left,
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
    let block_start = match writing_mode {
        WritingMode::HorizontalTb => BorderSide::Top,
        WritingMode::VerticalRl => BorderSide::Right,
        WritingMode::VerticalLr => BorderSide::Left,
    };
    let block_end = match writing_mode {
        WritingMode::HorizontalTb => BorderSide::Bottom,
        WritingMode::VerticalRl => BorderSide::Left,
        WritingMode::VerticalLr => BorderSide::Right,
    };
    let inline_start = match (writing_mode, direction) {
        (WritingMode::HorizontalTb, Direction::Ltr) => BorderSide::Left,
        (WritingMode::HorizontalTb, Direction::Rtl) => BorderSide::Right,
        (_, Direction::Ltr) => BorderSide::Top,
        (_, Direction::Rtl) => BorderSide::Bottom,
    };
    let inline_end = match (writing_mode, direction) {
        (WritingMode::HorizontalTb, Direction::Ltr) => BorderSide::Right,
        (WritingMode::HorizontalTb, Direction::Rtl) => BorderSide::Left,
        (_, Direction::Ltr) => BorderSide::Bottom,
        (_, Direction::Rtl) => BorderSide::Top,
    };
    match name {
        "border-block-start"
        | "border-block-start-width"
        | "border-block-start-style"
        | "border-block-start-color" => Some(block_start),
        "border-block-end"
        | "border-block-end-width"
        | "border-block-end-style"
        | "border-block-end-color" => Some(block_end),
        "border-inline-start"
        | "border-inline-start-width"
        | "border-inline-start-style"
        | "border-inline-start-color" => Some(inline_start),
        "border-inline-end"
        | "border-inline-end-width"
        | "border-inline-end-style"
        | "border-inline-end-color" => Some(inline_end),
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

fn physical_corner_radius_longhand(first: BorderSide, second: BorderSide) -> Option<&'static str> {
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

    let width = width.unwrap_or(ComputedLengthPercentage::from_length(3.0 * CSS_PX_TO_PT));
    let border_style = border_style.unwrap_or(BorderStyle::None);
    let color = color.unwrap_or(style.color);

    if let Some(side) = side {
        set_border_side_width(style, side, width);
        set_border_side_style_value(style, side, border_style);
        set_border_side_color(style, side, color);
    } else {
        let used_width = used_nonnegative_length(width);
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
        "thin" => Some(ComputedLengthPercentage::from_length(CSS_PX_TO_PT)),
        "medium" => Some(ComputedLengthPercentage::from_length(3.0 * CSS_PX_TO_PT)),
        "thick" => Some(ComputedLengthPercentage::from_length(5.0 * CSS_PX_TO_PT)),
        _ => {
            let length = parse_computed_length_percentage(value, font_size)?;
            (length.percent == 0.0 && !length_percentage_is_definitely_negative(length))
                .then_some(length)
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
        [all] => Some(CssEdges::all(*all)),
        [vertical, horizontal] => Some(CssEdges {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(CssEdges {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(CssEdges {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
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
        [all] => Some([*all, *all]),
        [start, end] => Some([*start, *end]),
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
pub(crate) fn parse_logical_border_colors(value: &str, current_color: Color) -> Option<[Color; 2]> {
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
/// `currentColor`, and CSS Color defines the keyword as the element's computed
/// `color` value:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-color> and
/// <https://www.w3.org/TR/css-color-4/#currentcolor-color>.
pub(crate) fn parse_border_color(value: &str, current_color: Color) -> Option<Color> {
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
pub(crate) fn parse_border_colors(value: &str, current_color: Color) -> Option<BorderColors> {
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
/// image values. This implementation currently supports URL image sources:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-source>.
pub(crate) fn parse_border_image_source(value: &str) -> Option<Option<String>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        Some(None)
    } else {
        parse_first_css_url(value).map(Some)
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

fn parse_border_image_source_token(
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

fn parse_border_image_slice_value(value: &str) -> Option<BorderImageSliceValue> {
    parse_percentage(value)
        .map(BorderImageSliceValue::Percent)
        .or_else(|| parse_non_negative_number(value).map(BorderImageSliceValue::Number))
}

fn expand_border_image_slice_offsets(
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
            top: *all,
            right: *all,
            bottom: *all,
            left: *all,
        }),
        [vertical, horizontal] => Some(BorderImageWidth {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(BorderImageWidth {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(BorderImageWidth {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

fn parse_border_image_width_value(value: &str, font_size: f32) -> Option<BorderImageWidthValue> {
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
            top: *all,
            right: *all,
            bottom: *all,
            left: *all,
        }),
        [vertical, horizontal] => Some(BorderImageOutset {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(BorderImageOutset {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(BorderImageOutset {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

fn parse_border_image_outset_value(value: &str, font_size: f32) -> Option<BorderImageOutsetValue> {
    parse_non_negative_number(value)
        .map(BorderImageOutsetValue::Number)
        .or_else(|| {
            parse_computed_length_percentage(value, font_size).and_then(|value| {
                (value.percent == 0.0 && !length_percentage_is_definitely_negative(value))
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

fn parse_border_image_repeat_keyword(value: &str) -> Option<BorderImageRepeatKeyword> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "stretch" => Some(BorderImageRepeatKeyword::Stretch),
        "repeat" => Some(BorderImageRepeatKeyword::Repeat),
        "round" => Some(BorderImageRepeatKeyword::Round),
        "space" => Some(BorderImageRepeatKeyword::Space),
        _ => None,
    }
}

fn parse_non_negative_number(value: &str) -> Option<f32> {
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
    let value = trim_css_value(value);
    let mut parts = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if start.is_none() && !ch.is_whitespace() {
            start = Some(index);
        }
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if ch.is_whitespace() && depth == 0 => {
                if let Some(component_start) = start.take() {
                    parts.push(value[component_start..index].trim());
                }
            }
            _ => {}
        }
    }
    if let Some(component_start) = start {
        parts.push(value[component_start..].trim());
    }
    parts.retain(|part| !part.is_empty());
    parts
}

fn split_css_top_level_slashes(value: &str) -> Vec<&str> {
    let value = trim_css_value(value);
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            '/' if depth == 0 => {
                parts.push(value[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts
}

/// Parse `border-radius` using the CSS Backgrounds and Borders shorthand grammar.
///
/// See CSS Backgrounds and Borders Level 3, §5.1 "Curve Radii: the
/// border-radius properties". Percentages are preserved until used-value time
/// because horizontal radii resolve against border-box width and vertical radii
/// resolve against border-box height.
pub(crate) fn parse_border_radius(value: &str, font_size: f32) -> Option<BorderRadius> {
    let value = trim_css_value(value);
    let mut groups = value.splitn(2, '/');
    let horizontal = parse_radius_components(groups.next()?.trim(), font_size)?;
    let vertical = if let Some(group) = groups.next() {
        parse_radius_components(group.trim(), font_size)?
    } else {
        horizontal
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
        [all] => Some(CornerRadius { x: *all, y: *all }),
        [x, y] => Some(CornerRadius { x: *x, y: *y }),
        _ => None,
    }
}

/// Parse one `corner-*-shape` keyword.
///
/// CSS Borders and Box Decorations Level 4 defines `round`, `bevel`, `scoop`,
/// and `notch` as keyword aliases for common superellipse corner shapes:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape>.
pub(crate) fn parse_corner_shape(value: &str) -> Option<CornerShape> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "round" => Some(CornerShape::Round),
        "bevel" => Some(CornerShape::Bevel),
        "scoop" => Some(CornerShape::Scoop),
        "notch" => Some(CornerShape::Notch),
        _ => None,
    }
}

/// Parse the `corner-shape` shorthand's one-to-four physical corner keywords.
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

/// Parse one component of a `corner` shorthand.
///
/// CSS Borders and Box Decorations Level 4 defines `corner` as setting both
/// `border-*-radius` and `corner-*-shape` longhands. This helper accepts the
/// WPT-covered order `<border-*-radius> <corner-*-shape>` and also permits the
/// shape before the radius because the grammar uses `||`:
/// <https://drafts.csswg.org/css-borders-4/#corner-shorthand>.
pub(crate) fn parse_corner_radius_and_shape(
    value: &str,
    font_size: f32,
) -> Option<(CornerRadius, CornerShape)> {
    let mut shape = None;
    let mut radius_parts = Vec::new();
    for part in split_css_component_values(value) {
        if shape.is_none() {
            shape = parse_corner_shape(part);
            if shape.is_some() {
                continue;
            }
        }
        radius_parts.push(part);
    }
    let radius = parse_corner_radius(&radius_parts.join(" "), font_size)?;
    Some((radius, shape.unwrap_or(CornerShape::Round)))
}

/// Parse the all-corner `corner` shorthand.
///
/// The shorthand follows the physical corner order top-left, top-right,
/// bottom-right, bottom-left when slash-separated per-corner components are
/// used:
/// <https://drafts.csswg.org/css-borders-4/#corner-shorthand>.
pub(crate) fn parse_corner_shorthand(
    value: &str,
    font_size: f32,
) -> Option<(BorderRadius, CornerShapes)> {
    let groups = split_css_top_level_slashes(value);
    if groups.is_empty() || groups.len() > 4 {
        return None;
    }
    let parsed = groups
        .iter()
        .map(|group| parse_corner_radius_and_shape(group, font_size))
        .collect::<Option<Vec<_>>>()?;
    let expanded = match parsed.as_slice() {
        [all] => [*all, *all, *all, *all],
        [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
        [top_left, horizontal, bottom_right] => {
            [*top_left, *horizontal, *bottom_right, *horizontal]
        }
        [top_left, top_right, bottom_right, bottom_left] => {
            [*top_left, *top_right, *bottom_right, *bottom_left]
        }
        _ => return None,
    };
    Some((
        BorderRadius {
            top_left: expanded[0].0,
            top_right: expanded[1].0,
            bottom_right: expanded[2].0,
            bottom_left: expanded[3].0,
        },
        CornerShapes {
            top_left: expanded[0].1,
            top_right: expanded[1].1,
            bottom_right: expanded[2].1,
            bottom_left: expanded[3].1,
        },
    ))
}

pub(crate) fn parse_radius_components(value: &str, font_size: f32) -> Option<EdgesOf<CssRadius>> {
    let radii = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_radius_value(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    match radii.as_slice() {
        [all] => Some(EdgesOf {
            top: *all,
            right: *all,
            bottom: *all,
            left: *all,
        }),
        [vertical, horizontal] => Some(EdgesOf {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(EdgesOf {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(EdgesOf {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

pub(crate) fn parse_radius_value(value: &str, font_size: f32) -> Option<CssRadius> {
    let value = parse_computed_length_percentage(value, font_size)?;
    (!length_percentage_is_definitely_negative(value)).then_some(CssRadius { value })
}

fn length_percentage_is_definitely_negative(value: ComputedLengthPercentage) -> bool {
    let components = [
        value.length,
        value.percent,
        value.ch,
        value.vw,
        value.vh,
        value.vmin,
        value.vmax,
        value.vi,
        value.vb,
    ];
    components.iter().any(|component| *component < 0.0)
        && components.iter().all(|component| *component <= 0.0)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgesOf<T> {
    top: T,
    right: T,
    bottom: T,
    left: T,
}

fn used_nonnegative_length(value: ComputedLengthPercentage) -> f32 {
    value
        .length_if_no_percent()
        .unwrap_or(value.length)
        .max(0.0)
}

pub(crate) fn set_border_side_width(
    style: &mut ComputedStyle,
    side: BorderSide,
    length: ComputedLengthPercentage,
) {
    let used = used_nonnegative_length(length);
    match side {
        BorderSide::Top => {
            style.border_width_values.top = length;
            style.border_widths.top = used;
        }
        BorderSide::Right => {
            style.border_width_values.right = length;
            style.border_widths.right = used;
        }
        BorderSide::Bottom => {
            style.border_width_values.bottom = length;
            style.border_widths.bottom = used;
        }
        BorderSide::Left => {
            style.border_width_values.left = length;
            style.border_widths.left = used;
        }
    }
    style.border_width = max_edge(style.border_widths);
}

pub(crate) fn set_border_side_style(style: &mut ComputedStyle, side: BorderSide, value: &str) {
    if let Some(border_style) = parse_border_style(value) {
        set_border_side_style_value(style, side, border_style);
    }
}

pub(crate) fn set_border_side_style_value(
    style: &mut ComputedStyle,
    side: BorderSide,
    border_style: BorderStyle,
) {
    match side {
        BorderSide::Top => style.border_styles.top = border_style,
        BorderSide::Right => style.border_styles.right = border_style,
        BorderSide::Bottom => style.border_styles.bottom = border_style,
        BorderSide::Left => style.border_styles.left = border_style,
    }
}

pub(crate) fn set_border_side_color(style: &mut ComputedStyle, side: BorderSide, color: Color) {
    match side {
        BorderSide::Top => {
            style.border_colors.top = color;
            style.border_color = color;
        }
        BorderSide::Right => style.border_colors.right = color,
        BorderSide::Bottom => style.border_colors.bottom = color,
        BorderSide::Left => style.border_colors.left = color,
    }
}

pub(crate) fn max_edge(edges: Edges) -> f32 {
    edges.top.max(edges.right).max(edges.bottom).max(edges.left)
}
