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
        (
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr,
            _,
        ) => WritingMode::HorizontalTb,
        (WritingMode::HorizontalTb, Direction::Ltr) => WritingMode::VerticalLr,
        (WritingMode::HorizontalTb, Direction::Rtl) => WritingMode::VerticalRl,
    }
}

pub(in crate::layout::flex) fn line_under_side(writing_mode: WritingMode) -> PhysicalSide {
    css::line_under_side(writing_mode)
}

pub(in crate::layout::flex) fn line_over_side(writing_mode: WritingMode) -> PhysicalSide {
    css::line_over_side(writing_mode)
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

/// Return a row flex container's first exported main-axis baseline offset.
///
/// CSS Flexbox first uses the shared first baseline of baseline-aligned items
/// on the startmost flex line. When no items on that line participate in
/// baseline alignment, its startmost item instead contributes (or
/// synthesizes) the baseline:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn flex_container_first_baseline(
    lines: &[FlexLineLayout],
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> Option<f32> {
    if !container_style.flex_direction.is_row_axis() {
        return None;
    }

    flex_line_content_baseline(
        lines.first()?,
        items,
        estimates,
        children,
        container_style,
        FlexBaselineSet::First,
        physical_direction,
    )
}

/// Recompute auto cross-size flex items that depend on their flex line.
///
/// CSS Flexbox uses each item's hypothetical cross size for non-stretch
/// alignments, but that hypothetical size is still measured from block layout
/// with the flex line's available cross size. For column flex items this can
/// change shrink-to-fit auto widths and therefore float layout. Stretch items
/// also relayout against the resolved line cross size. Quire's wrapped-line
/// metadata may span to the next line start for alignment and fragmentation, so
/// this pass excludes the following cross-axis gap from the item stretch slot:
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
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    let mut changed = false;
    if context.container_style.flex_wrap == FlexWrap::NoWrap
        && !context.container_cross_size_basis.is_definite()
        && !lines.iter().any(|line| !line.collapsed_struts.is_empty())
    {
        return false;
    }

    for line in lines {
        let line_cross_size = flex_line_item_stretch_cross_size(
            line,
            lines,
            FlexLineItemStretchContext {
                estimates,
                children,
                physical_direction: context.physical_direction,
                container_style: context.container_style,
                container_cross_size_basis: context.container_cross_size_basis,
                line_cross_gap: context.line_cross_gap,
            },
        );
        for &index in &line.item_indices {
            let child = &children[index];
            let remeasure_kind = flex_item_line_cross_remeasurement_kind(
                &child.style,
                context.container_style,
                physical_direction,
            );
            if remeasure_kind == FlexLineCrossRemeasureKind::None {
                continue;
            }

            let item_available = flex_item_line_cross_available_space(
                &child.style,
                physical_direction,
                context.available,
                line_cross_size,
            );
            let mut remeasured = layout.estimate_flex_item_size(
                child,
                context.stylesheets,
                item_available,
                physical_direction,
            );
            let mut border_cross_size = match remeasure_kind {
                FlexLineCrossRemeasureKind::Stretch => stretched_flex_item_line_cross_border_size(
                    &child.style,
                    physical_direction,
                    line_cross_size,
                    context.available.width_basis,
                ),
                FlexLineCrossRemeasureKind::ColumnShrinkToFit => {
                    remeasured_flex_item_cross_border_size(
                        &child.style,
                        remeasured,
                        physical_direction,
                    )
                    .points()
                }
                FlexLineCrossRemeasureKind::None => continue,
            };

            // Taffy's stretch adapter accepts only numeric maxima. Preserve
            // CSS intrinsic maxima here, then remeasure auto main sizes at the
            // used cross size so float and inline wrapping observe the same
            // constraint as final replay:
            // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes> and
            // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>.
            let intrinsic_max_cross_size = match physical_direction.is_row_axis() {
                true => match child.style.box_values.max_height {
                    css::ComputedLengthPercentageOrAuto::MinContent => Some(
                        remeasured_flex_item_cross_border_size(
                            &child.style,
                            FlexItemEstimate::fixed(
                                remeasured.width.points(),
                                remeasured.min_height.points(),
                            ),
                            physical_direction,
                        )
                        .points(),
                    ),
                    css::ComputedLengthPercentageOrAuto::MaxContent => Some(
                        remeasured_flex_item_cross_border_size(
                            &child.style,
                            FlexItemEstimate::fixed(
                                remeasured.width.points(),
                                remeasured.content_height.points(),
                            ),
                            physical_direction,
                        )
                        .points(),
                    ),
                    _ => None,
                },
                false => match child.style.box_values.max_width {
                    css::ComputedLengthPercentageOrAuto::MinContent => Some(
                        remeasured_flex_item_cross_border_size(
                            &child.style,
                            FlexItemEstimate::fixed(
                                remeasured.min_width.points(),
                                remeasured.height.points(),
                            ),
                            physical_direction,
                        )
                        .points(),
                    ),
                    css::ComputedLengthPercentageOrAuto::MaxContent => Some(
                        remeasured_flex_item_cross_border_size(
                            &child.style,
                            FlexItemEstimate::fixed(
                                remeasured.content_width.points(),
                                remeasured.height.points(),
                            ),
                            physical_direction,
                        )
                        .points(),
                    ),
                    _ => None,
                },
            };
            if let Some(max_cross_size) = intrinsic_max_cross_size
                && max_cross_size + 0.01 < border_cross_size
            {
                border_cross_size = max_cross_size;
            }

            // A stretch-fit size is clamped by definite min/max constraints
            // before the flex item's contents are laid out. Remeasure an
            // automatic main size against that final cross size; otherwise a
            // narrower `max-width` can leave inline content measured at the
            // unconstrained stretch width and incorrectly avoid wrapping.
            // <https://drafts.csswg.org/css-flexbox-1/#algo-stretch> and
            // <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>.
            let used_line_cross_size = if physical_direction.is_row_axis() {
                border_cross_size + child.style.margin.top + child.style.margin.bottom
            } else {
                border_cross_size + child.style.margin.left + child.style.margin.right
            };
            if (used_line_cross_size - line_cross_size).abs() > 0.01 {
                remeasured = layout.estimate_flex_item_size(
                    child,
                    context.stylesheets,
                    flex_item_line_cross_available_space(
                        &child.style,
                        physical_direction,
                        context.available,
                        used_line_cross_size,
                    ),
                    physical_direction,
                );
                if remeasure_kind == FlexLineCrossRemeasureKind::Stretch
                    && matches!(child.style.flex_basis, css::ComputedFlexBasis::Auto)
                {
                    let borders = used_border_widths(&child.style);
                    let automatic_main_size = if physical_direction.is_row_axis() {
                        child.style.box_values.width.is_auto().then(|| {
                            remeasured.width.points()
                                + child.style.padding.left
                                + child.style.padding.right
                                + borders.left
                                + borders.right
                        })
                    } else {
                        child.style.box_values.height.is_auto().then(|| {
                            remeasured.height.points()
                                + child.style.padding.top
                                + child.style.padding.bottom
                                + borders.top
                                + borders.bottom
                        })
                    };
                    if let Some(automatic_main_size) = automatic_main_size
                        && (items[index].main_size(axes) - automatic_main_size).abs() > 0.01
                    {
                        items[index].set_main_size(axes, automatic_main_size);
                        changed = true;
                    }
                }
            }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexLineCrossRemeasureKind {
    None,
    Stretch,
    ColumnShrinkToFit,
}

pub(in crate::layout::flex) struct FlexLineCrossRemeasureContext<'a> {
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) stylesheets: &'a [Stylesheet],
    pub(in crate::layout::flex) physical_direction: FlexDirection,
    pub(in crate::layout::flex) available: FlexAvailableSpace,
    pub(in crate::layout::flex) container_cross_size_basis: FlexAvailablePercentageBasis,
    pub(in crate::layout::flex) line_cross_gap: f32,
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
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
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
            // The percentage basis inherited from the containing block does
            // not make an auto-height flex container's own cross size
            // definite. Only its resolved used content height can suppress
            // the item's ratio-dependent automatic cross size here.
            // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
            available.height.is_some()
        } else {
            available.width_basis.is_definite()
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
        let main_size = item.main_size(axes).max(0.0);
        let main_content_size = (main_size - main_non_content).max(0.0);
        let mut cross_content_size = if child_style.aspect_ratio.uses_content_box_for_non_replaced()
            || child_style.box_sizing == BoxSizing::ContentBox
        {
            if physical_direction.is_row_axis() {
                main_content_size / ratio
            } else {
                main_content_size * ratio
            }
        } else if physical_direction.is_row_axis() {
            (main_size / ratio - cross_non_content).max(0.0)
        } else {
            (main_size * ratio - cross_non_content).max(0.0)
        };
        let percentage_basis = available.width.points();
        let (min_cross, max_cross) = if physical_direction.is_row_axis() {
            (
                used_min_height(
                    child_style,
                    PercentageBasis::definite(layout_pt(percentage_basis)),
                )
                .map(SemanticLengthExt::points),
                used_max_height(
                    child_style,
                    PercentageBasis::definite(layout_pt(percentage_basis)),
                )
                .map(SemanticLengthExt::points),
            )
        } else {
            (
                used_min_width(
                    child_style,
                    PercentageBasis::definite(layout_pt(percentage_basis)),
                )
                .map(SemanticLengthExt::points),
                used_max_width(
                    child_style,
                    PercentageBasis::definite(layout_pt(percentage_basis)),
                )
                .map(SemanticLengthExt::points),
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

pub(in crate::layout::flex) fn flex_item_line_cross_remeasurement_kind(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexLineCrossRemeasureKind {
    if flex_item_has_auto_cross_margin(child_style, physical_direction) {
        return FlexLineCrossRemeasureKind::None;
    }
    let align_stretches = matches!(
        effective_align_self(child_style, container_style).keyword,
        SelfAlignmentKeyword::Auto | SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    );
    let cross_size_is_auto = if physical_direction.is_row_axis() {
        child_style.box_values.height.is_auto()
    } else {
        child_style.box_values.width.is_auto()
    };
    if !cross_size_is_auto {
        return FlexLineCrossRemeasureKind::None;
    }
    if align_stretches {
        return FlexLineCrossRemeasureKind::Stretch;
    }
    if physical_direction.is_column_axis() {
        return FlexLineCrossRemeasureKind::ColumnShrinkToFit;
    }
    FlexLineCrossRemeasureKind::None
}

pub(in crate::layout::flex) fn flex_line_cross_gap(
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
    physical_gap_width: css::ComputedGap,
    physical_gap_height: css::ComputedGap,
) -> f32 {
    if container_style.flex_wrap == FlexWrap::NoWrap {
        return 0.0;
    }
    if physical_direction.is_row_axis() {
        used_flex_gap_with_basis(physical_gap_height, available.height_basis).points()
    } else {
        used_flex_gap_with_basis(physical_gap_width, available.width_basis).points()
    }
}

pub(in crate::layout::flex) fn flex_line_item_stretch_cross_size(
    line: &FlexLineLayout,
    lines: &[FlexLineLayout],
    context: FlexLineItemStretchContext<'_, '_>,
) -> f32 {
    let cross_size = if context.container_style.flex_wrap == FlexWrap::NoWrap
        && !context.container_cross_size_basis.is_definite()
        && line.collapsed_struts.is_empty()
    {
        flex_line_estimated_outer_cross_extent(
            line,
            context.estimates,
            context.children,
            context.physical_direction,
        )
        .unwrap_or_else(|| line.cross_size().points())
    } else {
        line.cross_size().points()
    };
    if context.line_cross_gap <= 0.0 {
        return cross_size;
    }
    let has_following_adjacent_line = lines.iter().any(|other| {
        other.cross_start.points() > line.cross_start.points() + 0.01
            && other.cross_start.points() <= line.cross_end.points() + 0.01
    });
    if has_following_adjacent_line {
        (cross_size - context.line_cross_gap).max(0.0)
    } else {
        cross_size
    }
}

pub(in crate::layout::flex) struct FlexLineItemStretchContext<'a, 'dom> {
    pub(in crate::layout::flex) estimates: &'a [FlexItemEstimate],
    pub(in crate::layout::flex) children: &'a [StyledChild<'dom>],
    pub(in crate::layout::flex) physical_direction: FlexDirection,
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) container_cross_size_basis: FlexAvailablePercentageBasis,
    pub(in crate::layout::flex) line_cross_gap: f32,
}

pub(in crate::layout::flex) fn flex_line_estimated_outer_cross_extent(
    line: &FlexLineLayout,
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) -> Option<f32> {
    line.item_indices
        .iter()
        .cloned()
        .map(|index| {
            estimated_outer_cross_size(&children[index].style, estimates[index], physical_direction)
                .points()
        })
        .reduce(f32::max)
}

pub(in crate::layout::flex) fn flex_item_line_cross_available_space(
    child_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
    line_cross_size: f32,
) -> FlexItemAvailableSpace {
    let mut item_available = FlexItemAvailableSpace::from_container(available);
    if physical_direction.is_row_axis() {
        let cross_size =
            (line_cross_size - child_style.margin.top - child_style.margin.bottom).max(0.0);
        item_available.set_definite_height(
            PhysicalContentHeight::new(content_box_pt(cross_size)),
            FlexAvailableSizeSource::DefiniteCrossSize,
        );
    } else {
        let cross_size =
            (line_cross_size - child_style.margin.left - child_style.margin.right).max(0.0);
        item_available.set_definite_width(
            PhysicalContentWidth::new(content_box_pt(cross_size)),
            FlexAvailableSizeSource::DefiniteCrossSize,
        );
    }
    item_available
}

pub(in crate::layout::flex) fn remeasured_flex_item_cross_border_size(
    style: &ComputedStyle,
    estimate: FlexItemEstimate,
    physical_direction: FlexDirection,
) -> BorderBoxLength {
    let borders = used_border_widths(style);
    border_box_pt(
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
        .max(0.0),
    )
}

pub(in crate::layout::flex) fn stretched_flex_item_line_cross_border_size(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    line_cross_size: f32,
    percentage_basis: FlexAvailablePercentageBasis,
) -> f32 {
    let borders = used_border_widths(style);
    if physical_direction.is_row_axis() {
        let non_content = style.padding.top + style.padding.bottom + borders.top + borders.bottom;
        let outer_cross_size = (line_cross_size - style.margin.top - style.margin.bottom).max(0.0);
        constrain_content_height(
            style,
            content_box_pt((outer_cross_size - non_content).max(0.0)),
            percentage_basis,
        )
        .points()
            + non_content
    } else {
        let non_content = style.padding.left + style.padding.right + borders.left + borders.right;
        let outer_cross_size = (line_cross_size - style.margin.left - style.margin.right).max(0.0);
        constrain_content_width(
            style,
            content_box_pt((outer_cross_size - non_content).max(0.0)),
            percentage_basis,
        )
        .points()
            + non_content
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
