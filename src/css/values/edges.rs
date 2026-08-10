use super::*;
use crate::css::component_values::split_css_component_values;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxSide {
    Top,
    Right,
    Bottom,
    Left,
}

impl From<PhysicalSide> for BoxSide {
    fn from(side: PhysicalSide) -> Self {
        match side {
            PhysicalSide::Top => Self::Top,
            PhysicalSide::Right => Self::Right,
            PhysicalSide::Bottom => Self::Bottom,
            PhysicalSide::Left => Self::Left,
        }
    }
}

/// Maps a logical box edge to a physical side.
///
/// CSS Logical Properties defines flow-relative margin and padding properties
/// whose physical side is resolved from the computed `writing-mode` and
/// `direction`:
/// <https://www.w3.org/TR/css-logical-1/#box>.
pub(crate) fn logical_box_side(
    name: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<BoxSide> {
    let axes = WritingModeAxes::new(writing_mode, direction);
    match name {
        "block-start"
        | "margin-block-start"
        | "padding-block-start"
        | "scroll-padding-block-start"
        | "scroll-margin-block-start"
        | "inset-block-start" => Some(axes.physical_side(LogicalSide::BlockStart).into()),
        "block-end"
        | "margin-block-end"
        | "padding-block-end"
        | "scroll-padding-block-end"
        | "scroll-margin-block-end"
        | "inset-block-end" => Some(axes.physical_side(LogicalSide::BlockEnd).into()),
        "inline-start"
        | "margin-inline-start"
        | "padding-inline-start"
        | "scroll-padding-inline-start"
        | "scroll-margin-inline-start"
        | "inset-inline-start" => Some(axes.physical_side(LogicalSide::InlineStart).into()),
        "inline-end"
        | "margin-inline-end"
        | "padding-inline-end"
        | "scroll-padding-inline-end"
        | "scroll-margin-inline-end"
        | "inset-inline-end" => Some(axes.physical_side(LogicalSide::InlineEnd).into()),
        _ => None,
    }
}

/// Returns the physical longhand for a logical margin side.
///
/// CSS Logical Properties maps `margin-block-*` and `margin-inline-*` aliases
/// to the corresponding physical margin longhand:
/// <https://www.w3.org/TR/css-logical-1/#margin-properties>.
pub(crate) fn physical_margin_side_longhand(side: BoxSide) -> &'static str {
    match side {
        BoxSide::Top => "margin-top",
        BoxSide::Right => "margin-right",
        BoxSide::Bottom => "margin-bottom",
        BoxSide::Left => "margin-left",
    }
}

/// Returns the physical longhand for a logical padding side.
///
/// CSS Logical Properties maps `padding-block-*` and `padding-inline-*`
/// aliases to the corresponding physical padding longhand:
/// <https://www.w3.org/TR/css-logical-1/#padding-properties>.
pub(crate) fn physical_padding_side_longhand(side: BoxSide) -> &'static str {
    match side {
        BoxSide::Top => "padding-top",
        BoxSide::Right => "padding-right",
        BoxSide::Bottom => "padding-bottom",
        BoxSide::Left => "padding-left",
    }
}

pub(crate) fn parse_edge_values(
    value: &str,
    font_size: f32,
) -> Option<CssEdges<ComputedLengthPercentage>> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_computed_length_percentage(part, font_size))
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

pub(crate) fn parse_margin_edge_values(
    value: &str,
    font_size: f32,
) -> Option<CssEdges<ComputedLengthPercentageOrAuto>> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_computed_length_percentage_auto(part, font_size))
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

pub(crate) fn parse_border_spacing(value: &str, font_size: f32) -> Option<BorderSpacing> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_non_negative_length(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [both] => Some(BorderSpacing {
            horizontal: both.clone(),
            vertical: both.clone(),
        }),
        [horizontal, vertical] => Some(BorderSpacing {
            horizontal: horizontal.clone(),
            vertical: vertical.clone(),
        }),
        _ => None,
    }
}

fn parse_non_negative_length(value: &str, font_size: f32) -> Option<ComputedLengthPercentage> {
    let length = parse_computed_length_percentage(value, font_size)?;
    (!length.contains_percentage() && !length_percentage_is_definitely_negative(&length))
        .then_some(length)
}

fn length_percentage_is_definitely_negative(value: &ComputedLengthPercentage) -> bool {
    value.is_definitely_absolute() && value.length_points() < 0.0
}

pub(crate) fn edge_all(value: f32) -> Edges {
    Edges {
        top: value,
        right: value,
        bottom: value,
        left: value,
    }
}

pub(crate) fn border_colors_all(color: CssColorOrCurrentColor) -> BorderColors {
    BorderColors {
        top: color,
        right: color,
        bottom: color,
        left: color,
    }
}

pub(crate) fn border_styles_all(style: BorderStyle) -> BorderStyles {
    BorderStyles {
        top: style,
        right: style,
        bottom: style,
        left: style,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_shorthand_keeps_calc_with_internal_whitespace_as_one_component() {
        let edges = parse_edge_values("calc(20px * 2)", 12.0).expect("calc edge shorthand parses");

        assert_eq!(edges.top.length_points(), 30.0);
        assert_eq!(edges.right.length_points(), 30.0);
        assert_eq!(edges.bottom.length_points(), 30.0);
        assert_eq!(edges.left.length_points(), 30.0);
    }
}
