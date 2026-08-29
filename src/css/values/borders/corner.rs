use super::*;
use crate::css::component_values::{split_css_component_values, split_css_top_level_delimiter};

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
            horizontal: horizontal.top,
            vertical: vertical.top,
        },
        top_right: CornerRadius {
            horizontal: horizontal.right,
            vertical: vertical.right,
        },
        bottom_right: CornerRadius {
            horizontal: horizontal.bottom,
            vertical: vertical.bottom,
        },
        bottom_left: CornerRadius {
            horizontal: horizontal.left,
            vertical: vertical.left,
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
            horizontal: all.clone(),
            vertical: all.clone(),
        }),
        [x, y] => Some(CornerRadius {
            horizontal: x.clone(),
            vertical: y.clone(),
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
    let groups = split_css_top_level_delimiter(value, '/');
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

pub(crate) fn parse_radius_components(
    value: &str,
    font_size: f32,
) -> Option<EdgesOf<CornerRadiusComponent>> {
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

pub(crate) fn parse_radius_value(value: &str, font_size: f32) -> Option<CornerRadiusComponent> {
    let value = parse_computed_length_percentage(value, font_size)?;
    // CSS accepts a calculation whose computed result is outside this
    // property's range, then clamps it at used-value time.  Rejecting it here
    // would incorrectly retain an earlier declaration such as `10px` for
    // `border-radius: calc(-10px)`.
    // <https://drafts.csswg.org/css-values-4/#calc-range>
    Some(CornerRadiusComponent { value })
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgesOf<T> {
    pub(in crate::css) top: T,
    pub(in crate::css) right: T,
    pub(in crate::css) bottom: T,
    pub(in crate::css) left: T,
}
