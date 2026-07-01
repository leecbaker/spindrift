use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxSide {
    Top,
    Right,
    Bottom,
    Left,
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
    let block_start = match writing_mode {
        WritingMode::HorizontalTb => BoxSide::Top,
        WritingMode::VerticalRl => BoxSide::Right,
        WritingMode::VerticalLr => BoxSide::Left,
    };
    let block_end = match writing_mode {
        WritingMode::HorizontalTb => BoxSide::Bottom,
        WritingMode::VerticalRl => BoxSide::Left,
        WritingMode::VerticalLr => BoxSide::Right,
    };
    let inline_start = match (writing_mode, direction) {
        (WritingMode::HorizontalTb, Direction::Ltr) => BoxSide::Left,
        (WritingMode::HorizontalTb, Direction::Rtl) => BoxSide::Right,
        (_, Direction::Ltr) => BoxSide::Top,
        (_, Direction::Rtl) => BoxSide::Bottom,
    };
    let inline_end = match (writing_mode, direction) {
        (WritingMode::HorizontalTb, Direction::Ltr) => BoxSide::Right,
        (WritingMode::HorizontalTb, Direction::Rtl) => BoxSide::Left,
        (_, Direction::Ltr) => BoxSide::Bottom,
        (_, Direction::Rtl) => BoxSide::Top,
    };
    match name {
        "block-start" | "margin-block-start" | "padding-block-start" | "inset-block-start" => {
            Some(block_start)
        }
        "block-end" | "margin-block-end" | "padding-block-end" | "inset-block-end" => {
            Some(block_end)
        }
        "inline-start" | "margin-inline-start" | "padding-inline-start" | "inset-inline-start" => {
            Some(inline_start)
        }
        "inline-end" | "margin-inline-end" | "padding-inline-end" | "inset-inline-end" => {
            Some(inline_end)
        }
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
    let values = value
        .split_whitespace()
        .map(|part| parse_computed_length_percentage(part, font_size))
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

pub(crate) fn parse_margin_edge_values(
    value: &str,
    font_size: f32,
) -> Option<CssEdges<ComputedLengthPercentageOrAuto>> {
    let values = value
        .split_whitespace()
        .map(|part| parse_computed_length_percentage_auto(part, font_size))
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

pub(crate) fn parse_border_spacing(value: &str, font_size: f32) -> Option<BorderSpacing> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_non_negative_length(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [both] => Some(BorderSpacing {
            horizontal: *both,
            vertical: *both,
        }),
        [horizontal, vertical] => Some(BorderSpacing {
            horizontal: *horizontal,
            vertical: *vertical,
        }),
        _ => None,
    }
}

fn parse_non_negative_length(value: &str, font_size: f32) -> Option<ComputedLengthPercentage> {
    let length = parse_computed_length_percentage(value, font_size)?;
    (length.percent == 0.0 && !length_percentage_is_definitely_negative(length)).then_some(length)
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

pub(crate) fn edge_all(value: f32) -> Edges {
    Edges {
        top: value,
        right: value,
        bottom: value,
        left: value,
    }
}

pub(crate) fn border_colors_all(color: Color) -> BorderColors {
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
