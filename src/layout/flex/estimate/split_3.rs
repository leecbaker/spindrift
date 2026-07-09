use super::*;

/// Estimate a row flex container's exported baselines from flex lines.
///
/// CSS Flexbox generates a row flex container's first and last main-axis
/// baseline sets from the first and last flex lines, using the startmost or
/// endmost item on those lines when that item has a parallel baseline. In
/// vertical writing modes the CSS row axis is physical y, so the exported
/// baseline is a horizontal x-axis offset:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn estimate_row_flex_container_line_metrics(
    style: &ComputedStyle,
    available: FlexAvailableSpace,
    items: &[EstimatedFlexBaselineItem],
) -> Option<EstimatedFlexLineMetrics> {
    if items.is_empty() {
        return None;
    }

    if !style.flex_direction.is_row_axis() {
        return None;
    }

    let physical_direction = physical_flex_direction(style);
    let PhysicalFlexGaps {
        horizontal: physical_gap_width,
        vertical: physical_gap_height,
    } = physical_flex_gaps(style);
    let main_gap_value = if physical_direction.is_row_axis() {
        physical_gap_width.clone()
    } else {
        physical_gap_height.clone()
    };
    let cross_gap_value = if physical_direction.is_row_axis() {
        physical_gap_height
    } else {
        physical_gap_width
    };
    let available_main_size = if physical_direction.is_row_axis() {
        available.width.points()
    } else {
        available
            .height
            .map(PhysicalContentHeight::points)
            .unwrap_or_else(|| available.width.points())
    };
    let available_cross_size = if physical_direction.is_row_axis() {
        available
            .height
            .map(PhysicalContentHeight::points)
            .unwrap_or(0.0)
    } else {
        available.width.points()
    };
    let intrinsic_main_gap = estimated_intrinsic_flex_gap(main_gap_value.clone()).points();
    let main_size =
        estimated_row_flex_container_main_size(style, available, items, intrinsic_main_gap);
    let main_gap = used_flex_gap(
        main_gap_value,
        PercentageBasis::definite(content_box_pt(main_size.unwrap_or(available_main_size))),
    )
    .points();
    let cross_gap = used_flex_gap(
        cross_gap_value,
        PercentageBasis::definite(content_box_pt(available_cross_size)),
    )
    .points();
    let mut lines = if style.flex_wrap == FlexWrap::NoWrap {
        vec![estimated_flex_line(0, items.len(), 0.0, items)]
    } else if let Some(main_size) = main_size {
        estimate_wrapped_row_flex_lines(items, main_size.max(0.0), main_gap, cross_gap)
    } else {
        vec![estimated_flex_line(0, items.len(), 0.0, items)]
    };
    let container_cross_size =
        estimated_row_flex_container_cross_size(style, available, physical_direction);
    if style.flex_wrap.wraps()
        && matches!(
            style.align_content.keyword,
            ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch
        )
        && let Some(container_cross_size) = container_cross_size
    {
        stretch_estimated_flex_line_cross_positions(
            &mut lines,
            container_cross_size.max(0.0),
            cross_gap,
        );
    }
    if style.flex_wrap.reverses_cross_axis() {
        reverse_estimated_flex_line_cross_positions(&mut lines, container_cross_size);
    }
    let first_line = lines.first()?;
    let last_line = lines.last()?;
    let cross_size = lines
        .iter()
        .map(|line| line.cross_start + line.cross_size)
        .fold(0.0f32, f32::max);

    Some(EstimatedFlexLineMetrics {
        line_count: lines.len(),
        cross_size,
        first_baseline: estimated_flex_line_baseline(
            first_line,
            items,
            style.flex_direction,
            EstimatedFlexBaselineSet::First,
        ),
        last_baseline: estimated_flex_line_baseline(
            last_line,
            items,
            style.flex_direction,
            EstimatedFlexBaselineSet::Last,
        ),
    })
}

pub(in crate::layout::flex) fn estimate_wrapped_row_flex_lines(
    items: &[EstimatedFlexBaselineItem],
    main_size: f32,
    main_gap: f32,
    cross_gap: f32,
) -> Vec<EstimatedFlexLine> {
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut line_main_size = 0.0f32;

    for (index, item) in items.iter().enumerate() {
        let item_outer_main = item.outer_main_size.max(0.0);
        let candidate_main_size = if index == line_start {
            item_outer_main
        } else {
            line_main_size + main_gap + item_outer_main
        };
        if index > line_start && candidate_main_size > main_size + 0.01 {
            let cross_start = estimated_next_flex_line_cross_start(&lines, cross_gap);
            lines.push(estimated_flex_line(line_start, index, cross_start, items));
            line_start = index;
            line_main_size = item_outer_main;
        } else {
            line_main_size = candidate_main_size;
        }
    }

    let cross_start = estimated_next_flex_line_cross_start(&lines, cross_gap);
    lines.push(estimated_flex_line(
        line_start,
        items.len(),
        cross_start,
        items,
    ));
    lines
}

pub(in crate::layout::flex) fn estimated_row_flex_container_main_size(
    style: &ComputedStyle,
    available: FlexAvailableSpace,
    items: &[EstimatedFlexBaselineItem],
    intrinsic_main_gap: f32,
) -> Option<f32> {
    let physical_direction = physical_flex_direction(style);
    let (size_property, min_size_property, max_size_property, percentage_basis) =
        if physical_direction.is_row_axis() {
            (
                style.box_values.width.clone(),
                style.box_values.min_width.clone(),
                style.box_values.max_width.clone(),
                available.width_basis,
            )
        } else {
            (
                style.box_values.height.clone(),
                style.box_values.min_height.clone(),
                style.box_values.max_height.clone(),
                available.height_basis,
            )
        };
    let (min_content, max_content) =
        estimated_row_flex_container_intrinsic_main_sizes(items, intrinsic_main_gap);
    let size = estimated_intrinsic_length_percentage_or_auto(
        size_property,
        percentage_basis,
        min_content,
        max_content,
    )
    .or_else(|| percentage_basis.points());
    let max_size = estimated_intrinsic_length_percentage_or_auto(
        max_size_property,
        percentage_basis,
        min_content,
        max_content,
    );
    let min_size = estimated_intrinsic_length_percentage_or_auto(
        min_size_property,
        percentage_basis,
        min_content,
        max_content,
    );

    match (size, max_size) {
        (Some(size), Some(max_size)) => Some(
            min_size
                .map_or(size, |min_size| size.max(min_size))
                .min(max_size),
        ),
        (Some(size), None) => Some(min_size.map_or(size, |min_size| size.max(min_size))),
        (None, Some(max_size)) => {
            Some(min_size.map_or(max_size, |min_size| max_size.max(min_size)))
        }
        (None, None) => None,
    }
}

pub(in crate::layout::flex) fn estimated_row_flex_container_cross_size(
    style: &ComputedStyle,
    available: FlexAvailableSpace,
    physical_direction: FlexDirection,
) -> Option<f32> {
    // Intrinsic flex measurement may already have resolved the container's
    // cross size and replaced that axis with a `DefiniteCrossSize` basis.
    // Reapplying a percentage preferred size to that replacement would resolve
    // the percentage against itself (for example, a 50% width of 60pt becoming
    // 30pt), which moves wrapped lines and exports the wrong baseline.
    // https://www.w3.org/TR/css-flexbox-1/#definite-sizes
    let resolved_cross_size = if physical_direction.is_row_axis() {
        match available.height_basis {
            PercentageBasis::Definite {
                source: FlexAvailableSizeSource::DefiniteCrossSize,
                ..
            } => available.height.map(PhysicalContentHeight::points),
            _ => None,
        }
    } else {
        match available.width_basis {
            PercentageBasis::Definite {
                source: FlexAvailableSizeSource::DefiniteCrossSize,
                ..
            } => Some(available.width.points()),
            _ => None,
        }
    };
    if resolved_cross_size.is_some() {
        return resolved_cross_size;
    }
    if physical_direction.is_row_axis() {
        estimated_intrinsic_length_percentage_or_auto(
            style.box_values.height.clone(),
            available.height_basis,
            0.0,
            0.0,
        )
        .or_else(|| available.height_basis_points())
    } else {
        estimated_intrinsic_length_percentage_or_auto(
            style.box_values.width.clone(),
            available.width_basis,
            0.0,
            0.0,
        )
        .or_else(|| available.width_basis_points())
    }
}

pub(in crate::layout::flex) fn estimated_row_flex_container_intrinsic_main_sizes(
    items: &[EstimatedFlexBaselineItem],
    intrinsic_main_gap: f32,
) -> (f32, f32) {
    let min_content = items
        .iter()
        .map(|item| item.outer_main_size.max(0.0))
        .fold(0.0f32, f32::max);
    let max_content_items = items
        .iter()
        .map(|item| item.outer_main_size.max(0.0))
        .sum::<f32>();
    let max_content_gaps = intrinsic_main_gap.max(0.0) * items.len().saturating_sub(1) as f32;
    let max_content = max_content_items + max_content_gaps;
    (min_content, max_content.max(min_content))
}

/// Returns the flex gap contribution used by intrinsic max-content estimates.
///
/// CSS Box Alignment resolves cyclic percentage gaps against zero for
/// intrinsic size contributions, while preserving any non-percentage length
/// component:
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(in crate::layout::flex) fn estimated_intrinsic_flex_gap(
    value: css::ComputedGap,
) -> LayoutLength {
    match value {
        css::ComputedGap::Normal => layout_pt(0.0),
        css::ComputedGap::LengthPercentage(value) => value.length_max_zero(),
    }
}

pub(in crate::layout::flex) fn estimated_intrinsic_length_percentage_or_auto(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: FlexAvailablePercentageBasis,
    min_content: f32,
    max_content: f32,
) -> Option<f32> {
    let percentage_basis = percentage_basis.points();
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::Stretch => {
            percentage_basis.map(|basis| basis.max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.is_definitely_absolute() {
                Some(value.length_max_zero().points())
            } else {
                let basis = percentage_basis?;
                value
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        basis.max(0.0),
                    )))
                    .map(|length| length.points().max(0.0))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent => Some(min_content.max(0.0)),
        css::ComputedLengthPercentageOrAuto::MaxContent => {
            Some(max_content.max(min_content).max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .clone()
                .and_then(|limit| {
                    percentage_basis.map(|basis| {
                        used_length_percentage(limit, PercentageBasis::definite(layout_pt(basis)))
                            .points()
                    })
                })
                .or_else(|| {
                    limit
                        .filter(|limit| !limit.needs_percentage_basis())
                        .map(|limit| limit.length_points())
                })
                .or(percentage_basis)
                .unwrap_or(max_content);
            Some(
                max_content
                    .max(min_content)
                    .max(0.0)
                    .min(min_content.max(0.0).max(stretch.max(0.0))),
            )
        }
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => {
            let percentage_basis = percentage_basis.unwrap_or(0.0);
            let stretch = percentage_basis.max(0.0);
            let fit_content = max_content.max(min_content).min(min_content.max(stretch));
            Some(
                value
                    .used_value(
                        max_content,
                        min_content,
                        max_content,
                        fit_content,
                        stretch,
                        PercentageBasis::definite(layout_pt(percentage_basis)),
                    )
                    .max(layout_pt(0.0))
                    .points(),
            )
        }
    }
}

/// Reverse estimated wrapped line positions for `flex-wrap: wrap-reverse`.
///
/// CSS Flexbox swaps cross-start and cross-end for `wrap-reverse`. If a
/// definite cross size is available, overflowed line stacks reverse against
/// that container size and may fall outside the flex container's cross-start
/// edge; otherwise intrinsic exports reverse against the line stack itself:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
pub(in crate::layout::flex) fn reverse_estimated_flex_line_cross_positions(
    lines: &mut [EstimatedFlexLine],
    container_cross_size: Option<f32>,
) {
    let line_stack_cross_size = estimated_flex_line_stack_cross_size(lines);
    let cross_size = container_cross_size
        .unwrap_or(line_stack_cross_size)
        .max(0.0);
    for line in lines {
        line.cross_start = cross_size - line.cross_start - line.cross_size;
    }
}

pub(in crate::layout::flex) fn estimated_flex_line_stack_cross_size(
    lines: &[EstimatedFlexLine],
) -> f32 {
    lines
        .iter()
        .map(|line| line.cross_start + line.cross_size)
        .fold(0.0f32, f32::max)
}

pub(in crate::layout::flex) fn stretch_estimated_flex_line_cross_positions(
    lines: &mut [EstimatedFlexLine],
    container_cross_size: f32,
    cross_gap: f32,
) {
    if lines.is_empty() {
        return;
    }
    let total_line_cross_size = lines.iter().map(|line| line.cross_size).sum::<f32>();
    let total_gap = cross_gap.max(0.0) * lines.len().saturating_sub(1) as f32;
    let extra_per_line =
        ((container_cross_size - total_line_cross_size - total_gap) / lines.len() as f32).max(0.0);
    let mut cross_start = 0.0;
    for line in lines {
        line.cross_start = cross_start;
        line.cross_size += extra_per_line;
        cross_start += line.cross_size + cross_gap.max(0.0);
    }
}

pub(in crate::layout::flex) fn estimated_next_flex_line_cross_start(
    lines: &[EstimatedFlexLine],
    cross_gap: f32,
) -> f32 {
    lines
        .last()
        .map(|line| line.cross_start + line.cross_size + cross_gap)
        .unwrap_or(0.0)
}

pub(in crate::layout::flex) fn estimated_flex_line(
    start: usize,
    end: usize,
    cross_start: f32,
    items: &[EstimatedFlexBaselineItem],
) -> EstimatedFlexLine {
    let item_indices = (start..end).collect::<Vec<_>>();
    let cross_size = item_indices
        .iter()
        .cloned()
        .map(|index| items[index].outer_cross_size)
        .fold(0.0f32, f32::max);
    EstimatedFlexLine {
        item_indices,
        cross_start,
        cross_size,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum EstimatedFlexBaselineSet {
    First,
    Last,
}

pub(in crate::layout::flex) fn estimated_flex_line_baseline(
    line: &EstimatedFlexLine,
    items: &[EstimatedFlexBaselineItem],
    direction: FlexDirection,
    baseline_set: EstimatedFlexBaselineSet,
) -> Option<f32> {
    estimated_flex_line_baseline_item_index(line, direction, baseline_set).and_then(|index| {
        let item = items[index];
        let baseline = match baseline_set {
            EstimatedFlexBaselineSet::First => item.first_baseline,
            EstimatedFlexBaselineSet::Last => item.last_baseline,
        }?;
        Some(
            line.cross_start
                + estimated_flex_item_cross_start_offset(line, item)
                + item.margin_cross_start
                + baseline,
        )
    })
}

pub(in crate::layout::flex) fn estimated_flex_item_cross_start_offset(
    line: &EstimatedFlexLine,
    item: EstimatedFlexBaselineItem,
) -> f32 {
    let free_space = (line.cross_size - item.outer_cross_size).max(0.0);
    match item.cross_alignment {
        EstimatedFlexItemCrossAlignment::Side(side) if side.is_end_edge() => free_space,
        EstimatedFlexItemCrossAlignment::Side(_) => 0.0,
        EstimatedFlexItemCrossAlignment::Center => free_space / 2.0,
    }
}

pub(in crate::layout::flex) fn estimated_flex_item_cross_alignment(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> EstimatedFlexItemCrossAlignment {
    match estimated_effective_align_self(child_style, container_style).keyword {
        SelfAlignmentKeyword::Center => EstimatedFlexItemCrossAlignment::Center,
        SelfAlignmentKeyword::End => EstimatedFlexItemCrossAlignment::Side(
            estimated_flex_base_cross_end_side(container_style),
        ),
        SelfAlignmentKeyword::FlexEnd => {
            EstimatedFlexItemCrossAlignment::Side(estimated_flex_cross_end_side(container_style))
        }
        SelfAlignmentKeyword::SelfStart => EstimatedFlexItemCrossAlignment::Side(
            estimated_child_self_start_side(child_style, container_style),
        ),
        SelfAlignmentKeyword::SelfEnd => EstimatedFlexItemCrossAlignment::Side(
            estimated_child_self_end_side(child_style, container_style),
        ),
        SelfAlignmentKeyword::Auto
        | SelfAlignmentKeyword::Normal
        | SelfAlignmentKeyword::Start
        | SelfAlignmentKeyword::FlexStart
        | SelfAlignmentKeyword::Left
        | SelfAlignmentKeyword::Right
        | SelfAlignmentKeyword::Stretch
        | SelfAlignmentKeyword::Baseline
        | SelfAlignmentKeyword::LastBaseline => {
            EstimatedFlexItemCrossAlignment::Side(estimated_flex_cross_start_side(container_style))
        }
    }
}

pub(in crate::layout::flex) fn estimated_effective_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> AlignSelf {
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.align_items
    } else {
        child_style.align_self
    }
}

pub(in crate::layout::flex) fn estimated_flex_item_available_space(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> FlexItemAvailableSpace {
    let mut item_available = FlexItemAvailableSpace::from_container(available);
    let Some(stretched_cross_size) = estimated_stretched_flex_item_cross_size(
        child_style,
        container_style,
        physical_direction,
        available,
    ) else {
        return item_available;
    };

    if physical_direction.is_row_axis() {
        item_available.set_definite_height(
            PhysicalContentHeight::new(content_box_pt(stretched_cross_size)),
            FlexAvailableSizeSource::DefiniteCrossSize,
        );
        item_available.stretched_height = Some(PhysicalContentHeight::new(content_box_pt(
            stretched_cross_size,
        )));
    } else {
        item_available.set_definite_width(
            PhysicalContentWidth::new(content_box_pt(stretched_cross_size)),
            FlexAvailableSizeSource::DefiniteCrossSize,
        );
        item_available.stretched_width = Some(PhysicalContentWidth::new(content_box_pt(
            stretched_cross_size,
        )));
    }
    item_available
}

pub(in crate::layout::flex) fn estimated_stretched_flex_item_cross_size(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
    if !matches!(
        estimated_effective_align_self(child_style, container_style).keyword,
        SelfAlignmentKeyword::Auto | SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    ) || estimated_flex_item_has_auto_cross_margin(child_style, physical_direction)
    {
        return None;
    }

    if physical_direction.is_row_axis() {
        if !child_style.box_values.height.is_auto() {
            return None;
        }
        let container_cross_size = available.height_basis_points()?;
        Some((container_cross_size - child_style.margin.top - child_style.margin.bottom).max(0.0))
    } else {
        if !child_style.box_values.width.is_auto() {
            return None;
        }
        let container_cross_size = available.width_basis_points()?;
        Some((container_cross_size - child_style.margin.left - child_style.margin.right).max(0.0))
    }
}

pub(in crate::layout::flex) fn estimated_flex_item_has_auto_cross_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> bool {
    if physical_direction.is_row_axis() {
        style.box_values.margin.top.is_auto() || style.box_values.margin.bottom.is_auto()
    } else {
        style.box_values.margin.left.is_auto() || style.box_values.margin.right.is_auto()
    }
}

pub(in crate::layout::flex) fn estimated_flex_base_cross_start_side(
    style: &ComputedStyle,
) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_start_side(style.writing_mode)
    } else {
        inline_start_side(style.writing_mode, style.direction)
    }
}

pub(in crate::layout::flex) fn estimated_flex_base_cross_end_side(
    style: &ComputedStyle,
) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_end_side(style.writing_mode)
    } else {
        inline_end_side(style.writing_mode, style.direction)
    }
}

pub(in crate::layout::flex) fn estimated_flex_cross_start_side(
    style: &ComputedStyle,
) -> PhysicalSide {
    if style.flex_wrap.reverses_cross_axis() {
        estimated_flex_base_cross_end_side(style)
    } else {
        estimated_flex_base_cross_start_side(style)
    }
}

pub(in crate::layout::flex) fn estimated_flex_cross_end_side(
    style: &ComputedStyle,
) -> PhysicalSide {
    if style.flex_wrap.reverses_cross_axis() {
        estimated_flex_base_cross_start_side(style)
    } else {
        estimated_flex_base_cross_end_side(style)
    }
}

pub(in crate::layout::flex) fn estimated_child_self_start_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_axis = estimated_flex_base_cross_start_side(container_style).axis();
    let block_start = block_start_side(child_style.writing_mode);
    if block_start.axis() == cross_axis {
        block_start
    } else {
        inline_start_side(child_style.writing_mode, child_style.direction)
    }
}

pub(in crate::layout::flex) fn estimated_child_self_end_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_axis = estimated_flex_base_cross_start_side(container_style).axis();
    let block_end = block_end_side(child_style.writing_mode);
    if block_end.axis() == cross_axis {
        block_end
    } else {
        inline_end_side(child_style.writing_mode, child_style.direction)
    }
}

pub(in crate::layout::flex) fn estimated_flex_line_baseline_item_index(
    line: &EstimatedFlexLine,
    direction: FlexDirection,
    baseline_set: EstimatedFlexBaselineSet,
) -> Option<usize> {
    match (baseline_set, direction) {
        (EstimatedFlexBaselineSet::First, FlexDirection::Row | FlexDirection::RowReverse) => {
            line.item_indices.first().cloned()
        }
        (EstimatedFlexBaselineSet::First, FlexDirection::Column) => {
            line.item_indices.first().cloned()
        }
        (EstimatedFlexBaselineSet::First, FlexDirection::ColumnReverse) => {
            line.item_indices.last().cloned()
        }
        (EstimatedFlexBaselineSet::Last, FlexDirection::Row | FlexDirection::RowReverse) => {
            line.item_indices.last().cloned()
        }
        (EstimatedFlexBaselineSet::Last, FlexDirection::Column) => {
            line.item_indices.last().cloned()
        }
        (EstimatedFlexBaselineSet::Last, FlexDirection::ColumnReverse) => {
            line.item_indices.first().cloned()
        }
    }
}

pub(in crate::layout::flex) fn estimated_flex_main_content_size(
    style: &ComputedStyle,
    size: FlexItemEstimate,
    direction: FlexDirection,
    percentage_basis: FlexAvailablePercentageBasis,
) -> f32 {
    let percentage_basis_points = percentage_basis.points();
    let (preferred_size, min_size, specified_size) = if direction.is_row_axis() {
        (
            size.content_width,
            size.min_width,
            style.box_values.width.clone(),
        )
    } else {
        (
            size.content_height,
            size.min_height,
            style.box_values.height.clone(),
        )
    };

    match &style.flex_basis {
        css::ComputedFlexBasis::LengthPercentage(length) => {
            if length.contains_percentage() && percentage_basis_points.is_none() {
                preferred_size.points()
            } else {
                used_length_percentage(
                    length.value.clone(),
                    PercentageBasis::definite(layout_pt(percentage_basis_points.unwrap_or(0.0))),
                )
                .points()
            }
        }
        css::ComputedFlexBasis::Content | css::ComputedFlexBasis::MaxContent => {
            preferred_size.points()
        }
        css::ComputedFlexBasis::MinContent => min_size.points(),
        css::ComputedFlexBasis::FitContent(limit) => {
            let limit = limit
                .clone()
                .map(|limit| {
                    used_length_percentage(
                        limit,
                        PercentageBasis::definite(layout_pt(
                            percentage_basis_points.unwrap_or(0.0),
                        )),
                    )
                    .points()
                })
                .or(percentage_basis_points)
                .unwrap_or_else(|| preferred_size.points());
            preferred_size
                .points()
                .max(0.0)
                .min(min_size.points().max(0.0).max(limit.max(0.0)))
        }
        css::ComputedFlexBasis::Auto => {
            used_length_percentage_or_auto_with_basis(specified_size, percentage_basis)
                .map(|size| size.points())
                .unwrap_or_else(|| preferred_size.points())
        }
    }
}
