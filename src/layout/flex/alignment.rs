use super::*;

pub(in crate::layout::flex) fn effective_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> AlignSelf {
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.align_items
    } else {
        child_style.align_self
    }
}
pub(in crate::layout::flex) fn flex_cross_start_side(style: &ComputedStyle) -> PhysicalSide {
    FlexAxes::for_style(style).cross_start_side()
}

/// Return the flex cross-start side before `wrap-reverse` changes line
/// stacking.
///
/// A flex line's first/last baseline alignment edge is defined by the
/// container's ordinary cross axis. `wrap-reverse` changes the direction in
/// which whole flex lines are stacked, but does not reverse first/last
/// baseline alignment inside each line:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property> and
/// <https://www.w3.org/TR/css-flexbox-1/#valdef-align-items-baseline>.
pub(in crate::layout::flex) fn flex_unreversed_cross_start_side(
    style: &ComputedStyle,
) -> PhysicalSide {
    FlexAxes::for_style(style).unreversed_cross_start_side()
}

pub(in crate::layout::flex) fn flex_cross_end_side(style: &ComputedStyle) -> PhysicalSide {
    FlexAxes::for_style(style).cross_end_side()
}

pub(in crate::layout::flex) fn child_self_start_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_start = flex_cross_start_side(container_style);
    let cross_axis = cross_start.axis();
    let child_axes = FlowAxes::for_style(child_style);
    let block_start = child_axes.block_start_side();
    if block_start.axis() == cross_axis {
        block_start
    } else {
        child_axes.inline_start_side()
    }
}

pub(in crate::layout::flex) fn child_self_end_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_start = flex_cross_start_side(container_style);
    let cross_axis = cross_start.axis();
    let child_axes = FlowAxes::for_style(child_style);
    let block_end = child_axes.block_start_side().opposite();
    if block_end.axis() == cross_axis {
        block_end
    } else {
        child_axes.inline_start_side().opposite()
    }
}

pub(in crate::layout::flex) fn flex_item_has_auto_cross_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> bool {
    if physical_direction.is_row_axis() {
        style.box_values.margin.top.is_auto() || style.box_values.margin.bottom.is_auto()
    } else {
        style.box_values.margin.left.is_auto() || style.box_values.margin.right.is_auto()
    }
}
