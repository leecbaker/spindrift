use super::*;

pub(in crate::layout::flex) fn synthesis_writing_mode(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_line_axis: PhysicalAxis,
) -> WritingMode {
    if block_start_side(child_style.writing_mode).axis() != baseline_line_axis {
        return child_style.writing_mode;
    }
    if block_start_side(container_style.writing_mode).axis() != baseline_line_axis {
        return container_style.writing_mode;
    }
    match (child_style.writing_mode, child_style.direction) {
        (WritingMode::VerticalRl | WritingMode::VerticalLr, _) => WritingMode::HorizontalTb,
        (WritingMode::HorizontalTb, Direction::Ltr) => WritingMode::VerticalLr,
        (WritingMode::HorizontalTb, Direction::Rtl) => WritingMode::VerticalRl,
    }
}

pub(in crate::layout::flex) fn line_under_side(writing_mode: WritingMode) -> PhysicalSide {
    match writing_mode {
        WritingMode::HorizontalTb => PhysicalSide::Bottom,
        WritingMode::VerticalRl | WritingMode::VerticalLr => PhysicalSide::Left,
    }
}

pub(in crate::layout::flex) fn line_over_side(writing_mode: WritingMode) -> PhysicalSide {
    match writing_mode {
        WritingMode::HorizontalTb => PhysicalSide::Top,
        WritingMode::VerticalRl | WritingMode::VerticalLr => PhysicalSide::Right,
    }
}

/// Return a flex item's absolute baseline coordinate in the flex line cross
/// axis.
///
/// CSS Flexbox aligns row flex-line baseline sets in the row cross axis. For
/// horizontal writing modes that coordinate is physical y; for vertical
/// writing modes the row cross axis is physical x, so Quire uses the
/// vertical-text horizontal baseline estimates recorded from inline painting:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn measured_item_cross_axis_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> f32 {
    if physical_direction.is_row_axis() {
        return item.y()
            + style.margin.top
            + measured_item_border_box_baseline(
                item,
                estimate,
                style,
                container_style,
                baseline_set,
            );
    }
    item.x()
        + style.margin.left
        + measured_item_horizontal_border_box_baseline(
            item,
            estimate,
            style,
            container_style,
            baseline_set,
        )
}

/// Return a horizontal flex container's first exported baseline offset.
///
/// CSS Flexbox says a flex container's first main-axis baseline is generated
/// from the first flex item's first baseline set when that item has a baseline
/// parallel to the main axis; otherwise it is synthesized from the content box:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn flex_container_first_baseline(
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
) -> Option<f32> {
    if !container_style.flex_direction.is_row_axis() {
        return None;
    }

    let (item, estimate, child) = items
        .iter()
        .zip(estimates)
        .zip(children)
        .map(|((item, estimate), child)| (item, estimate, child))
        .next()?;

    Some(
        item.y()
            + estimate
                .first_baseline
                .unwrap_or_else(|| synthesized_item_baseline(item, &child.style))
            + child.style.margin.top,
    )
}

/// Recompute auto cross-size flex items against their final flex line cross size.
///
/// CSS Flexbox first lays out each item to determine its hypothetical cross size,
/// then determines each flex line's cross size, and finally aligns each item in
/// that line. Non-stretched auto cross-size items behave as fit-content in the
/// cross axis, so their available cross size is the resolved flex line cross
/// size, not only the container cross size used during line construction:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>,
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>, and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-align>.
pub(in crate::layout::flex) fn apply_line_cross_size_dependent_item_remeasurements(
    layout: &mut LayoutBuilder<'_>,
    items: &mut [FlexItemLayout],
    estimates: &mut [FlexItemEstimate],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    context: FlexLineCrossRemeasureContext<'_>,
) -> bool {
    let physical_direction = context.physical_direction;
    let axes = FlexAxes::from_physical_direction(physical_direction);
    let mut changed = false;

    for line in lines {
        let line_cross_size = line.cross_size();
        for &index in &line.item_indices {
            let child = &children[index];
            if !flex_item_needs_final_line_cross_remeasurement(
                &child.style,
                context.container_style,
                physical_direction,
            ) {
                continue;
            }

            let item_available = flex_item_final_line_cross_available_space(
                &child.style,
                physical_direction,
                context.available,
                line_cross_size,
            );
            let remeasured =
                layout.estimate_flex_item_size(child, context.stylesheets, item_available);
            let border_cross_size =
                estimated_flex_item_border_cross_size(&child.style, remeasured, physical_direction);

            if (items[index].cross_size(axes) - border_cross_size).abs() > 0.01 {
                items[index].set_cross_size(axes, border_cross_size);
                changed = true;
            }
            update_flex_item_estimate_cross_axis(
                &mut estimates[index],
                remeasured,
                physical_direction,
            );
        }
    }

    changed
}

pub(in crate::layout::flex) struct FlexLineCrossRemeasureContext<'a> {
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) stylesheets: &'a [Stylesheet],
    pub(in crate::layout::flex) physical_direction: FlexDirection,
    pub(in crate::layout::flex) available: FlexAvailableSpace,
}

/// Resolve auto cross sizes that depend on the final flexed main size.
///
/// CSS Flexbox determines each item's hypothetical cross size after flexing has
/// produced a used main size. CSS Sizing says a preferred aspect ratio makes
/// the auto axis ratio-dependent when the opposite axis is definite:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(in crate::layout::flex) fn apply_main_size_aspect_ratio_cross_size_corrections(
    items: &mut [FlexItemLayout],
    estimates: &mut [FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> bool {
    let axes = FlexAxes::from_physical_direction(physical_direction);
    let mut changed = false;
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        let child_style = &child.style;
        if flex_item_has_auto_cross_margin(child_style, physical_direction) {
            continue;
        }
        let cross_size_is_auto = if physical_direction.is_row_axis() {
            child_style.box_values.height.is_auto()
        } else {
            child_style.box_values.width.is_auto()
        };
        if !cross_size_is_auto {
            continue;
        }
        let stretch_with_definite_cross_size = matches!(
            effective_align_self(child_style, container_style).keyword,
            SelfAlignmentKeyword::Auto
                | SelfAlignmentKeyword::Normal
                | SelfAlignmentKeyword::Stretch
        ) && if physical_direction.is_row_axis() {
            available.height_is_definite
        } else {
            available.width_is_definite
        };
        if stretch_with_definite_cross_size {
            continue;
        }
        let Some(ratio) = child_style
            .aspect_ratio
            .preferred_ratio(child.is_replaced_element(), estimate.preferred_aspect_ratio)
        else {
            continue;
        };
        let borders = used_border_widths(child_style);
        let (main_non_content, cross_non_content) = if physical_direction.is_row_axis() {
            (
                child_style.padding.left + child_style.padding.right + borders.left + borders.right,
                child_style.padding.top + child_style.padding.bottom + borders.top + borders.bottom,
            )
        } else {
            (
                child_style.padding.top + child_style.padding.bottom + borders.top + borders.bottom,
                child_style.padding.left + child_style.padding.right + borders.left + borders.right,
            )
        };
        let main_content_size = (item.main_size(axes) - main_non_content).max(0.0);
        let mut cross_content_size = if physical_direction.is_row_axis() {
            main_content_size / ratio
        } else {
            main_content_size * ratio
        };
        let percentage_basis = available.width;
        let (min_cross, max_cross) = if physical_direction.is_row_axis() {
            (
                used_min_height(child_style, percentage_basis),
                used_max_height(child_style, percentage_basis),
            )
        } else {
            (
                used_min_width(child_style, percentage_basis),
                used_max_width(child_style, percentage_basis),
            )
        };
        if let Some(min_cross) = min_cross {
            cross_content_size = cross_content_size.max(min_cross);
        }
        if let Some(max_cross) = max_cross {
            cross_content_size = cross_content_size.min(max_cross);
        }
        let border_cross_size = (cross_content_size + cross_non_content).max(0.0);
        if (item.cross_size(axes) - border_cross_size).abs() > 0.01 {
            item.set_cross_size(axes, border_cross_size);
            changed = true;
        }
        if physical_direction.is_row_axis() {
            estimate.height = content_box_pt(cross_content_size);
            estimate.content_height = content_box_pt(cross_content_size);
        } else {
            estimate.width = content_box_pt(cross_content_size);
            estimate.content_width = content_box_pt(cross_content_size);
        }
    }
    changed
}

pub(in crate::layout::flex) fn flex_item_needs_final_line_cross_remeasurement(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> bool {
    if flex_item_has_auto_cross_margin(child_style, physical_direction) {
        return false;
    }
    if matches!(
        effective_align_self(child_style, container_style).keyword,
        SelfAlignmentKeyword::Auto | SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    ) {
        return false;
    }
    if physical_direction.is_row_axis() {
        child_style.box_values.height.is_auto()
    } else {
        child_style.box_values.width.is_auto()
    }
}

pub(in crate::layout::flex) fn flex_item_final_line_cross_available_space(
    child_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
    line_cross_size: f32,
) -> FlexItemAvailableSpace {
    let mut item_available = FlexItemAvailableSpace::from_container(available);
    if physical_direction.is_row_axis() {
        let cross_size =
            (line_cross_size - child_style.margin.top - child_style.margin.bottom).max(0.0);
        item_available.height = Some(cross_size);
        item_available.height_is_definite = true;
    } else {
        let cross_size =
            (line_cross_size - child_style.margin.left - child_style.margin.right).max(0.0);
        item_available.width = cross_size;
        item_available.width_is_definite = true;
    }
    item_available
}

pub(in crate::layout::flex) fn estimated_flex_item_border_cross_size(
    style: &ComputedStyle,
    estimate: FlexItemEstimate,
    physical_direction: FlexDirection,
) -> f32 {
    let borders = used_border_widths(style);
    if physical_direction.is_row_axis() {
        estimate.height.points()
            + style.padding.top
            + style.padding.bottom
            + borders.top
            + borders.bottom
    } else {
        estimate.width.points()
            + style.padding.left
            + style.padding.right
            + borders.left
            + borders.right
    }
    .max(0.0)
}

pub(in crate::layout::flex) fn update_flex_item_estimate_cross_axis(
    estimate: &mut FlexItemEstimate,
    remeasured: FlexItemEstimate,
    physical_direction: FlexDirection,
) {
    if physical_direction.is_row_axis() {
        estimate.height = remeasured.height;
        estimate.min_height = remeasured.min_height;
        estimate.content_height = remeasured.content_height;
    } else {
        estimate.width = remeasured.width;
        estimate.min_width = remeasured.min_width;
        estimate.content_width = remeasured.content_width;
    }
    estimate.first_baseline = remeasured.first_baseline;
    estimate.last_baseline = remeasured.last_baseline;
    estimate.first_horizontal_baseline = remeasured.first_horizontal_baseline;
    estimate.last_horizontal_baseline = remeasured.last_horizontal_baseline;
}
