use super::*;

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
    Some((radius, shape.unwrap_or(CornerShape::ROUND)))
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
        [all] => [all.clone(), all.clone(), all.clone(), all.clone()],
        [vertical, horizontal] => [
            vertical.clone(),
            horizontal.clone(),
            vertical.clone(),
            horizontal.clone(),
        ],
        [top_left, horizontal, bottom_right] => [
            top_left.clone(),
            horizontal.clone(),
            bottom_right.clone(),
            horizontal.clone(),
        ],
        [top_left, top_right, bottom_right, bottom_left] => [
            top_left.clone(),
            top_right.clone(),
            bottom_right.clone(),
            bottom_left.clone(),
        ],
        _ => return None,
    };
    Some((
        BorderRadius {
            top_left: expanded[0].0.clone(),
            top_right: expanded[1].0.clone(),
            bottom_right: expanded[2].0.clone(),
            bottom_left: expanded[3].0.clone(),
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
            top: all.clone(),
            right: all.clone(),
            bottom: all.clone(),
            left: all.clone(),
        }),
        [vertical, horizontal] => Some(EdgesOf {
            top: vertical.clone(),
            right: horizontal.clone(),
            bottom: vertical.clone(),
            left: horizontal.clone(),
        }),
        [top, horizontal, bottom] => Some(EdgesOf {
            top: top.clone(),
            right: horizontal.clone(),
            bottom: bottom.clone(),
            left: horizontal.clone(),
        }),
        [top, right, bottom, left] => Some(EdgesOf {
            top: top.clone(),
            right: right.clone(),
            bottom: bottom.clone(),
            left: left.clone(),
        }),
        _ => None,
    }
}

pub(crate) fn parse_radius_value(value: &str, font_size: f32) -> Option<CssRadius> {
    let value = parse_computed_length_percentage(value, font_size)?;
    (!length_percentage_is_definitely_negative(value.clone())).then_some(CssRadius { value })
}

pub(in crate::css) fn length_percentage_is_definitely_negative(
    value: ComputedLengthPercentage,
) -> bool {
    value.is_definitely_negative()
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgesOf<T> {
    pub(in crate::css) top: T,
    pub(in crate::css) right: T,
    pub(in crate::css) bottom: T,
    pub(in crate::css) left: T,
}

pub(in crate::css) fn used_nonnegative_length(value: ComputedLengthPercentage) -> LayoutLength {
    value.length_max_zero()
}

pub(crate) fn set_border_side_width(
    style: &mut ComputedStyle,
    side: BorderSide,
    length: ComputedLengthPercentage,
) {
    let used = used_nonnegative_length(length.clone()).points();
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
