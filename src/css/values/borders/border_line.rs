use super::*;
use crate::css::component_values::split_css_component_values;

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
