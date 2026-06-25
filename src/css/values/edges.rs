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
        "block-start" | "margin-block-start" | "padding-block-start" => Some(block_start),
        "block-end" | "margin-block-end" | "padding-block-end" => Some(block_end),
        "inline-start" | "margin-inline-start" | "padding-inline-start" => Some(inline_start),
        "inline-end" | "margin-inline-end" | "padding-inline-end" => Some(inline_end),
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

pub(crate) fn parse_edges_with_font_size(value: &str, font_size: f32) -> Option<Edges> {
    let typed = parse_edge_values(value, font_size)?;
    let values = [
        typed.top.length_if_no_percent()?,
        typed.right.length_if_no_percent()?,
        typed.bottom.length_if_no_percent()?,
        typed.left.length_if_no_percent()?,
    ];
    Some(Edges {
        top: values[0],
        right: values[1],
        bottom: values[2],
        left: values[3],
    })
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

pub(crate) fn parse_border_spacing(value: &str) -> Option<BorderSpacing> {
    let values = value
        .split_whitespace()
        .filter_map(parse_length)
        .collect::<Vec<_>>();
    match values.as_slice() {
        [both] => Some(BorderSpacing {
            horizontal: (*both).max(0.0),
            vertical: (*both).max(0.0),
        }),
        [horizontal, vertical] => Some(BorderSpacing {
            horizontal: (*horizontal).max(0.0),
            vertical: (*vertical).max(0.0),
        }),
        _ => None,
    }
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
