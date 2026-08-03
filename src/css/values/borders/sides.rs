use super::*;

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
    materialize_border_width_for_visible_side(style, side);
}

/// Materialize resolved border widths for sides whose line style can paint.
///
/// The CSS initial border width is `medium`, while its initial line style is
/// `none`.  Keeping the specified value separately lets a later
/// `border-style` declaration expose that initial width without making an
/// unstyled border contribute to layout.
/// <https://www.w3.org/TR/css-backgrounds-3/#border-width>
pub(crate) fn materialize_visible_border_widths(style: &mut ComputedStyle) {
    for side in [
        BorderSide::Top,
        BorderSide::Right,
        BorderSide::Bottom,
        BorderSide::Left,
    ] {
        materialize_border_width_for_visible_side(style, side);
    }
}

fn materialize_border_width_for_visible_side(style: &mut ComputedStyle, side: BorderSide) {
    let (border_style, value) = match side {
        BorderSide::Top => (
            style.border_styles.top,
            style.border_width_values.top.clone(),
        ),
        BorderSide::Right => (
            style.border_styles.right,
            style.border_width_values.right.clone(),
        ),
        BorderSide::Bottom => (
            style.border_styles.bottom,
            style.border_width_values.bottom.clone(),
        ),
        BorderSide::Left => (
            style.border_styles.left,
            style.border_width_values.left.clone(),
        ),
    };
    if border_style.suppresses_used_width() {
        return;
    }

    let used = used_nonnegative_length(value).points();
    match side {
        BorderSide::Top => style.border_widths.top = used,
        BorderSide::Right => style.border_widths.right = used,
        BorderSide::Bottom => style.border_widths.bottom = used,
        BorderSide::Left => style.border_widths.left = used,
    }
    style.border_width = max_edge(style.border_widths);
}

pub(crate) fn set_border_side_color(style: &mut ComputedStyle, side: BorderSide, color: CssColor) {
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

pub(in crate::css) fn used_nonnegative_length(value: ComputedLengthPercentage) -> LayoutLength {
    value.length_max_zero()
}

pub(crate) fn max_edge(edges: Edges) -> f32 {
    edges.top.max(edges.right).max(edges.bottom).max(edges.left)
}
