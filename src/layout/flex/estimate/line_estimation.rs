use super::*;
use crate::layout::flex::compute::FlexBaselineSet;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct EstimatedFlexBaselineItem {
    pub(in crate::layout::flex) outer_main_size: FlexMainSize,
    pub(in crate::layout::flex) outer_cross_size: FlexCrossSize,
    pub(in crate::layout::flex) margin_cross_start: FlexCrossLength,
    pub(in crate::layout::flex) cross_alignment: EstimatedFlexItemCrossAlignment,
    /// The baseline-sharing set this item can contribute to. Baseline-aligned
    /// items with an orthogonal inline axis or an auto cross margin use CSS
    /// Align's fallback alignment and do not establish the flex line's shared
    /// baseline.
    pub(in crate::layout::flex) baseline_set: Option<FlexBaselineSet>,
    pub(in crate::layout::flex) first_baseline: Option<FlexCrossOffset>,
    pub(in crate::layout::flex) last_baseline: Option<FlexCrossOffset>,
    pub(in crate::layout::flex) synthesized_first_baseline: FlexCrossOffset,
    pub(in crate::layout::flex) synthesized_last_baseline: FlexCrossOffset,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) enum EstimatedFlexItemCrossAlignment {
    Side(PhysicalSide),
    Center,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct EstimatedFlexLineMetrics {
    pub(in crate::layout::flex) cross_size: FlexCrossSize,
    pub(in crate::layout::flex) first_baseline: Option<FlexCrossOffset>,
    pub(in crate::layout::flex) last_baseline: Option<FlexCrossOffset>,
}

#[derive(Debug, Clone)]
pub(in crate::layout::flex) struct EstimatedFlexLine {
    pub(in crate::layout::flex) item_indices: Vec<usize>,
    pub(in crate::layout::flex) cross_start: FlexCrossOffset,
    pub(in crate::layout::flex) cross_size: FlexCrossSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EstimatedFlexContainerBaselineSource {
    Shared {
        line_index: usize,
        baseline_set: FlexBaselineSet,
    },
    Item {
        index: usize,
        baseline_set: FlexBaselineSet,
    },
    SynthesizedItem {
        index: usize,
        baseline_set: FlexBaselineSet,
    },
}

pub(in crate::layout::flex) fn estimated_flex_item_cross_axis_baselines(
    size: FlexItemEstimate,
    physical_direction: FlexDirection,
) -> (Option<FlexCrossOffset>, Option<FlexCrossOffset>) {
    if physical_direction.is_row_axis() {
        (
            size.baselines
                .vertical
                .first
                .map(|baseline| FlexCrossOffset::new(baseline.points())),
            size.baselines
                .vertical
                .last
                .map(|baseline| FlexCrossOffset::new(baseline.points())),
        )
    } else {
        (
            size.baselines
                .horizontal
                .first
                .map(|baseline| FlexCrossOffset::new(baseline.points())),
            size.baselines
                .horizontal
                .last
                .map(|baseline| FlexCrossOffset::new(baseline.points())),
        )
    }
}

/// Estimate a row flex container's exported baselines from flex lines.
///
/// CSS Flexbox generates a row flex container's first and last baseline sets
/// from the first and last flex lines. A line's shared baseline takes
/// precedence over the startmost or endmost item's parallel baseline. In
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
    let available_main_size = available
        .definite_main_size(physical_direction)
        // Baseline estimation retains its established physical-width fallback
        // when a column container has an indefinite main axis.
        .unwrap_or_else(|| flex_main_size_from_content_box(available.width.content_box_length()));
    let available_cross_size = available
        .definite_cross_size(physical_direction)
        .unwrap_or_else(|| FlexCrossSize::new(0.0));
    let intrinsic_main_gap =
        flex_main_gap_size(estimated_intrinsic_flex_gap(main_gap_value.clone()));
    let main_size =
        estimated_row_flex_container_main_size(style, available, items, intrinsic_main_gap);
    let main_gap = used_flex_gap(
        main_gap_value,
        PercentageBasis::definite(content_box_pt(
            main_size.unwrap_or(available_main_size).points(),
        )),
    );
    let cross_gap = used_flex_gap(
        cross_gap_value,
        PercentageBasis::definite(content_box_pt(available_cross_size.points())),
    );
    let mut lines = if style.flex_wrap == FlexWrap::NoWrap {
        vec![estimated_flex_line(
            0,
            items.len(),
            FlexCrossOffset::new(0.0),
            items,
        )]
    } else if let Some(main_size) = main_size {
        estimate_wrapped_row_flex_lines(
            items,
            main_size,
            flex_main_gap_size(main_gap),
            flex_cross_gap_size(cross_gap),
        )
    } else {
        vec![estimated_flex_line(
            0,
            items.len(),
            FlexCrossOffset::new(0.0),
            items,
        )]
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
            container_cross_size,
            flex_cross_gap_size(cross_gap),
        );
    }
    if style.flex_wrap.reverses_cross_axis() {
        reverse_estimated_flex_line_cross_positions(&mut lines, container_cross_size);
    }
    let (first_line, last_line) = estimated_flex_container_baseline_lines(&lines, style)?;
    let cross_size = lines
        .iter()
        .map(|line| line.cross_start + line.cross_size)
        .fold(FlexCrossOffset::new(0.0), FlexCrossOffset::max)
        .relative_to(FlexCrossOffset::new(0.0))
        .non_negative_size();

    Some(EstimatedFlexLineMetrics {
        cross_size,
        first_baseline: estimated_flex_container_main_axis_baseline(
            &lines,
            first_line,
            items,
            physical_direction,
            FlexBaselineSet::First,
        ),
        last_baseline: estimated_flex_container_main_axis_baseline(
            &lines,
            last_line,
            items,
            physical_direction,
            FlexBaselineSet::Last,
        ),
    })
}

pub(in crate::layout::flex) fn estimate_wrapped_row_flex_lines(
    items: &[EstimatedFlexBaselineItem],
    main_size: FlexMainSize,
    main_gap: FlexMainSize,
    cross_gap: FlexCrossSize,
) -> Vec<EstimatedFlexLine> {
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut line_main_size = FlexMainSize::new(0.0);

    for (index, item) in items.iter().enumerate() {
        let item_outer_main = item.outer_main_size;
        let candidate_main_size = if index == line_start {
            item_outer_main
        } else {
            line_main_size + main_gap + item_outer_main
        };
        if index > line_start && candidate_main_size.points() > main_size.points() + 0.01 {
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
    intrinsic_main_gap: FlexMainSize,
) -> Option<FlexMainSize> {
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
                style.box_values.height.value().clone(),
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
        flex_main_content_box_length(min_content),
        flex_main_content_box_length(max_content),
    )
    .map(flex_main_size_from_content_box)
    .or_else(|| {
        percentage_basis
            .value()
            .map(flex_main_size_from_content_box)
    });
    let max_size = estimated_intrinsic_length_percentage_or_auto(
        max_size_property,
        percentage_basis,
        flex_main_content_box_length(min_content),
        flex_main_content_box_length(max_content),
    );
    let max_size = max_size.map(flex_main_size_from_content_box);
    let min_size = estimated_intrinsic_length_percentage_or_auto(
        min_size_property,
        percentage_basis,
        flex_main_content_box_length(min_content),
        flex_main_content_box_length(max_content),
    );
    let min_size = min_size.map(flex_main_size_from_content_box);

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
) -> Option<FlexCrossSize> {
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
            } => available
                .height
                .map(PhysicalContentHeight::content_box_length)
                .map(flex_cross_size_from_content_box),
            _ => None,
        }
    } else {
        match available.width_basis {
            PercentageBasis::Definite {
                source: FlexAvailableSizeSource::DefiniteCrossSize,
                ..
            } => Some(flex_cross_size_from_content_box(
                available.width.content_box_length(),
            )),
            _ => None,
        }
    };
    if resolved_cross_size.is_some() {
        return resolved_cross_size;
    }
    if physical_direction.is_row_axis() {
        estimated_intrinsic_length_percentage_or_auto(
            style.box_values.height.value().clone(),
            available.height_basis,
            content_box_pt(0.0),
            content_box_pt(0.0),
        )
        .map(flex_cross_size_from_content_box)
        .or_else(|| {
            available
                .height_basis
                .value()
                .map(flex_cross_size_from_content_box)
        })
    } else {
        estimated_intrinsic_length_percentage_or_auto(
            style.box_values.width.clone(),
            available.width_basis,
            content_box_pt(0.0),
            content_box_pt(0.0),
        )
        .map(flex_cross_size_from_content_box)
        .or_else(|| {
            available
                .width_basis
                .value()
                .map(flex_cross_size_from_content_box)
        })
    }
}

pub(in crate::layout::flex) fn estimated_row_flex_container_intrinsic_main_sizes(
    items: &[EstimatedFlexBaselineItem],
    intrinsic_main_gap: FlexMainSize,
) -> (FlexMainSize, FlexMainSize) {
    let min_content = items
        .iter()
        .map(|item| item.outer_main_size)
        .fold(FlexMainSize::new(0.0), FlexMainSize::max);
    let max_content_items = items
        .iter()
        .map(|item| item.outer_main_size)
        .fold(FlexMainSize::new(0.0), |sum, size| sum + size);
    let max_content_gaps = intrinsic_main_gap.scale(items.len().saturating_sub(1) as f32);
    let max_content = max_content_items + max_content_gaps;
    (min_content, max_content.max(min_content))
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
    container_cross_size: Option<FlexCrossSize>,
) {
    let line_stack_cross_size = estimated_flex_line_stack_cross_size(lines);
    let cross_size = container_cross_size.unwrap_or(line_stack_cross_size);
    let cross_origin = FlexCrossOffset::new(0.0);
    let cross_end = cross_origin + cross_size;
    for line in lines {
        line.cross_start = cross_end - line.cross_start.relative_to(cross_origin) - line.cross_size;
    }
}

pub(in crate::layout::flex) fn estimated_flex_line_stack_cross_size(
    lines: &[EstimatedFlexLine],
) -> FlexCrossSize {
    lines
        .iter()
        .map(|line| line.cross_start + line.cross_size)
        .fold(FlexCrossOffset::new(0.0), FlexCrossOffset::max)
        .relative_to(FlexCrossOffset::new(0.0))
        .non_negative_size()
}

pub(in crate::layout::flex) fn stretch_estimated_flex_line_cross_positions(
    lines: &mut [EstimatedFlexLine],
    container_cross_size: FlexCrossSize,
    cross_gap: FlexCrossSize,
) {
    if lines.is_empty() {
        return;
    }
    let total_line_cross_size = lines
        .iter()
        .map(|line| line.cross_size)
        .fold(FlexCrossSize::new(0.0), |sum, size| sum + size);
    let total_gap = cross_gap.scale(lines.len().saturating_sub(1) as f32);
    let extra_per_line = (container_cross_size - total_line_cross_size - total_gap)
        .non_negative_size()
        .divide(std::num::NonZeroUsize::new(lines.len()).expect("non-empty flex lines"));
    let mut cross_start = FlexCrossOffset::new(0.0);
    for line in lines {
        line.cross_start = cross_start;
        line.cross_size = line.cross_size + extra_per_line;
        cross_start = cross_start + line.cross_size + cross_gap;
    }
}

pub(in crate::layout::flex) fn estimated_next_flex_line_cross_start(
    lines: &[EstimatedFlexLine],
    cross_gap: FlexCrossSize,
) -> FlexCrossOffset {
    lines
        .last()
        .map(|line| line.cross_start + line.cross_size + cross_gap)
        .unwrap_or(FlexCrossOffset::new(0.0))
}

pub(in crate::layout::flex) fn estimated_flex_line(
    start: usize,
    end: usize,
    cross_start: FlexCrossOffset,
    items: &[EstimatedFlexBaselineItem],
) -> EstimatedFlexLine {
    let item_indices = (start..end).collect::<Vec<_>>();
    let cross_size = estimated_flex_line_cross_size(&item_indices, items);
    EstimatedFlexLine {
        item_indices,
        cross_start,
        cross_size,
    }
}

/// Calculate an estimated flex line's used cross size with the same
/// baseline-sharing contribution used by final layout.
///
/// CSS Flexbox 9.4 sums the greatest distance before and after the shared
/// baseline, then compares that sum with every non-participant's hypothetical
/// outer cross size and zero:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>.
fn estimated_flex_line_cross_size(
    item_indices: &[usize],
    items: &[EstimatedFlexBaselineItem],
) -> FlexCrossSize {
    let mut largest_baseline_start = FlexCrossSize::new(0.0);
    let mut largest_baseline_end = FlexCrossSize::new(0.0);
    let mut has_baseline_participant = false;
    let mut largest_other = FlexCrossSize::new(0.0);

    for &index in item_indices {
        let item = items[index];
        let Some(baseline_set) = item.baseline_set else {
            largest_other = largest_other.max(item.outer_cross_size);
            continue;
        };
        has_baseline_participant = true;
        let baseline = estimated_flex_item_baseline(item, baseline_set);
        let distance_from_start = (item.margin_cross_start
            + baseline.relative_to(FlexCrossOffset::new(0.0)))
        .non_negative_size();
        let distance_from_end = (item.outer_cross_size - distance_from_start).non_negative_size();
        largest_baseline_start = largest_baseline_start.max(distance_from_start);
        largest_baseline_end = largest_baseline_end.max(distance_from_end);
    }

    let baseline_size = if has_baseline_participant {
        largest_baseline_start + largest_baseline_end
    } else {
        FlexCrossSize::new(0.0)
    };
    baseline_size.max(largest_other)
}

fn estimated_flex_item_baseline(
    item: EstimatedFlexBaselineItem,
    baseline_set: FlexBaselineSet,
) -> FlexCrossOffset {
    match baseline_set {
        FlexBaselineSet::First => item
            .first_baseline
            .unwrap_or(item.synthesized_first_baseline),
        FlexBaselineSet::Last => item.last_baseline.unwrap_or(item.synthesized_last_baseline),
    }
}

/// Select physical startmost/endmost estimated lines after packing.
/// `wrap-reverse` changes their positions, but CSS baseline export still uses
/// the container's ordinary writing-mode cross-start/end edges.
fn estimated_flex_container_baseline_lines<'a>(
    lines: &'a [EstimatedFlexLine],
    style: &ComputedStyle,
) -> Option<(&'a EstimatedFlexLine, &'a EstimatedFlexLine)> {
    let cross_start = estimated_flex_base_cross_start_side(style);
    let (first, last) = if cross_start.is_start_edge() {
        (
            lines.iter().min_by(|left, right| {
                left.cross_start
                    .partial_cmp(&right.cross_start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            lines.iter().max_by(|left, right| {
                (left.cross_start + left.cross_size)
                    .partial_cmp(&(right.cross_start + right.cross_size))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        )
    } else {
        (
            lines.iter().max_by(|left, right| {
                (left.cross_start + left.cross_size)
                    .partial_cmp(&(right.cross_start + right.cross_size))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            lines.iter().min_by(|left, right| {
                left.cross_start
                    .partial_cmp(&right.cross_start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        )
    };
    Some((first?, last?))
}

#[cfg(test)]
fn estimated_flex_line_baseline(
    line: &EstimatedFlexLine,
    items: &[EstimatedFlexBaselineItem],
    _physical_direction: FlexDirection,
    baseline_set: FlexBaselineSet,
) -> Option<FlexCrossOffset> {
    estimated_flex_container_main_axis_baseline(
        std::slice::from_ref(line),
        line,
        items,
        _physical_direction,
        baseline_set,
    )
}

fn estimated_flex_container_main_axis_baseline(
    lines: &[EstimatedFlexLine],
    fallback_line: &EstimatedFlexLine,
    items: &[EstimatedFlexBaselineItem],
    _physical_direction: FlexDirection,
    baseline_set: FlexBaselineSet,
) -> Option<FlexCrossOffset> {
    let source =
        estimated_flex_container_baseline_source(lines, fallback_line, items, baseline_set)?;
    match source {
        EstimatedFlexContainerBaselineSource::Shared {
            line_index,
            baseline_set: source_set,
        } => lines[line_index]
            .item_indices
            .iter()
            .copied()
            .filter(|&index| items[index].baseline_set == Some(source_set))
            .map(|index| {
                estimated_flex_item_line_baseline(&lines[line_index], items[index], source_set)
            })
            .reduce(FlexCrossOffset::max),
        EstimatedFlexContainerBaselineSource::Item {
            index,
            baseline_set,
        } => Some(estimated_flex_item_line_baseline(
            fallback_line,
            items[index],
            baseline_set,
        )),
        EstimatedFlexContainerBaselineSource::SynthesizedItem {
            index,
            baseline_set,
        } => Some(estimated_flex_item_synthesized_line_baseline(
            fallback_line,
            items[index],
            baseline_set,
        )),
    }
}

fn estimated_flex_container_baseline_source(
    lines: &[EstimatedFlexLine],
    fallback_line: &EstimatedFlexLine,
    items: &[EstimatedFlexBaselineItem],
    baseline_set: FlexBaselineSet,
) -> Option<EstimatedFlexContainerBaselineSource> {
    let has_participants = |line: &EstimatedFlexLine, set| {
        line.item_indices
            .iter()
            .any(|&index| items[index].baseline_set == Some(set))
    };
    // Estimated lines retain order-modified line order even when their final
    // cross positions are reversed. Sharing-group priority is confined to
    // that first/last line, while `fallback_line` independently represents
    // the ordinary finalized cross-start/end fallback used by final layout.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
    let participant_line_index = match baseline_set {
        FlexBaselineSet::First => (!lines.is_empty()).then_some(0),
        FlexBaselineSet::Last => lines.len().checked_sub(1),
    };
    if let Some(line_index) = participant_line_index
        && has_participants(&lines[line_index], baseline_set)
    {
        return Some(EstimatedFlexContainerBaselineSource::Shared {
            line_index,
            baseline_set,
        });
    }
    let opposite_set = baseline_set.opposite();
    if let Some(line_index) = participant_line_index
        && has_participants(&lines[line_index], opposite_set)
    {
        return Some(EstimatedFlexContainerBaselineSource::Shared {
            line_index,
            baseline_set: opposite_set,
        });
    }

    let measured_set = |index: usize| {
        [baseline_set, opposite_set]
            .into_iter()
            .find(|set| match set {
                FlexBaselineSet::First => items[index].first_baseline.is_some(),
                FlexBaselineSet::Last => items[index].last_baseline.is_some(),
            })
            .map(|set| EstimatedFlexContainerBaselineSource::Item {
                index,
                baseline_set: set,
            })
    };
    let item_source = match baseline_set {
        FlexBaselineSet::First => fallback_line
            .item_indices
            .iter()
            .copied()
            .find_map(measured_set),
        FlexBaselineSet::Last => fallback_line
            .item_indices
            .iter()
            .rev()
            .copied()
            .find_map(measured_set),
    };
    if item_source.is_some() {
        return item_source;
    }

    let index =
        estimated_flex_line_baseline_item_index(fallback_line, FlexDirection::Row, baseline_set)?;
    Some(EstimatedFlexContainerBaselineSource::SynthesizedItem {
        index,
        baseline_set,
    })
}

fn estimated_flex_item_line_baseline(
    line: &EstimatedFlexLine,
    item: EstimatedFlexBaselineItem,
    baseline_set: FlexBaselineSet,
) -> FlexCrossOffset {
    let baseline = estimated_flex_item_baseline(item, baseline_set);
    let position = line.cross_start
        + estimated_flex_item_cross_start_offset(line, item)
        + item.margin_cross_start;
    position + baseline.relative_to(FlexCrossOffset::new(0.0))
}

fn estimated_flex_item_synthesized_line_baseline(
    line: &EstimatedFlexLine,
    item: EstimatedFlexBaselineItem,
    baseline_set: FlexBaselineSet,
) -> FlexCrossOffset {
    let baseline = match baseline_set {
        FlexBaselineSet::First => item.synthesized_first_baseline,
        FlexBaselineSet::Last => item.synthesized_last_baseline,
    };
    let position = line.cross_start
        + estimated_flex_item_cross_start_offset(line, item)
        + item.margin_cross_start;
    position + baseline.relative_to(FlexCrossOffset::new(0.0))
}

pub(in crate::layout::flex) fn estimated_flex_item_cross_start_offset(
    line: &EstimatedFlexLine,
    item: EstimatedFlexBaselineItem,
) -> FlexCrossSize {
    let free_space = (line.cross_size - item.outer_cross_size).non_negative_size();
    match item.cross_alignment {
        EstimatedFlexItemCrossAlignment::Side(side) if side.is_end_edge() => free_space,
        EstimatedFlexItemCrossAlignment::Side(_) => FlexCrossSize::new(0.0),
        EstimatedFlexItemCrossAlignment::Center => free_space.scale(0.5),
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
    let premeasure_cross_size = if container_style.flex_wrap.balances_lines()
        && container_style.flex_line_count.get() > 1
    {
        estimated_stretched_flex_item_cross_size(
            child_style,
            container_style,
            physical_direction,
            available,
        )
        .map(FlexPremeasureCrossSize::BalancedLineSlot)
    } else if !container_style.flex_wrap.wraps() {
        // A single-line container with a definite cross size establishes a
        // definite stretched item cross size before flex-base calculation.
        // This is distinct from a final stretch replay because the result is
        // required to determine the base size itself.
        // <https://drafts.csswg.org/css-flexbox/#algo-main-item>
        estimated_stretched_flex_item_cross_size(
            child_style,
            container_style,
            physical_direction,
            available,
        )
        .map(FlexPremeasureCrossSize::DefiniteSingleLineContainer)
    } else {
        None
    };
    let Some(premeasure_cross_size) = premeasure_cross_size else {
        return item_available;
    };
    let stretched_cross_size = premeasure_cross_size.size();

    item_available.set_definite_cross_size(
        physical_direction,
        stretched_cross_size,
        premeasure_cross_size.available_size_source(),
    );
    item_available.set_stretched_cross_size(physical_direction, stretched_cross_size);
    item_available
}

pub(in crate::layout::flex) fn estimated_stretched_flex_item_cross_size(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexCrossSize> {
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
        let container_cross_size =
            flex_cross_size_from_content_box(available.height_basis_content_box_length()?);
        let stretch_size = (container_cross_size
            - FlexCrossLength::new(child_style.margin.top + child_style.margin.bottom))
        .non_negative_size();
        Some(flex_cross_size_from_content_box(
            constrain_flex_item_estimated_height(
                child_style,
                flex_cross_content_box_length(stretch_size),
                flex_cross_content_box_length(stretch_size),
                flex_cross_content_box_length(stretch_size),
                available.height_basis,
                non_content_pt(
                    child_style.padding.top
                        + child_style.padding.bottom
                        + vertical_border_width(child_style),
                ),
            ),
        ))
    } else {
        if !child_style.box_values.width.is_auto() {
            return None;
        }
        let container_cross_size =
            flex_cross_size_from_content_box(available.width_basis_content_box_length()?);
        let stretch_size = (container_cross_size
            - FlexCrossLength::new(child_style.margin.left + child_style.margin.right))
        .non_negative_size();
        Some(flex_cross_size_from_content_box(constrain_content_width(
            child_style,
            flex_cross_content_box_length(stretch_size),
            available.width_basis,
        )))
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
        inline_start_side(style.writing_mode, style.used_direction())
    }
}

pub(in crate::layout::flex) fn estimated_flex_base_cross_end_side(
    style: &ComputedStyle,
) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_end_side(style.writing_mode)
    } else {
        inline_end_side(style.writing_mode, style.used_direction())
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
        inline_start_side(child_style.writing_mode, child_style.used_direction())
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
        inline_end_side(child_style.writing_mode, child_style.used_direction())
    }
}

pub(in crate::layout::flex) fn estimated_flex_line_baseline_item_index(
    line: &EstimatedFlexLine,
    _physical_direction: FlexDirection,
    baseline_set: FlexBaselineSet,
) -> Option<usize> {
    // Estimated lines preserve the same order-modified sequence as final
    // layout. A reversed physical main axis must not reverse it a second
    // time while choosing the startmost/endmost fallback item.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
    match baseline_set {
        FlexBaselineSet::First => line.item_indices.first().copied(),
        FlexBaselineSet::Last => line.item_indices.last().copied(),
    }
}

pub(in crate::layout::flex) fn estimated_flex_main_content_size(
    style: &ComputedStyle,
    size: FlexItemEstimate,
    direction: FlexDirection,
    percentage_basis: FlexAvailablePercentageBasis,
) -> ContentBoxLength {
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
            style.box_values.height.value().clone(),
        )
    };

    match &style.flex_basis {
        css::ComputedFlexBasis::LengthPercentage(length) => {
            if length.contains_percentage() && percentage_basis_points.is_none() {
                preferred_size
            } else {
                crate::units::layout_to_content_box_length(used_length_percentage(
                    length.value.clone(),
                    PercentageBasis::definite(layout_pt(percentage_basis_points.unwrap_or(0.0))),
                ))
            }
        }
        css::ComputedFlexBasis::Content | css::ComputedFlexBasis::MaxContent => preferred_size,
        css::ComputedFlexBasis::MinContent => min_size,
        css::ComputedFlexBasis::FitContent(limit) => {
            let limit = limit
                .clone()
                .map(|limit| {
                    crate::units::layout_to_content_box_length(used_length_percentage(
                        limit,
                        PercentageBasis::definite(layout_pt(
                            percentage_basis_points.unwrap_or(0.0),
                        )),
                    ))
                })
                .or_else(|| percentage_basis_points.map(content_box_pt))
                .unwrap_or(preferred_size);
            let lower = if min_size >= limit { min_size } else { limit };
            if preferred_size > lower {
                lower
            } else {
                preferred_size
            }
        }
        css::ComputedFlexBasis::Auto => {
            used_length_percentage_or_auto_with_basis(specified_size, percentage_basis)
                .map(crate::units::layout_to_content_box_length)
                .unwrap_or(preferred_size)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stretched_cross_estimate_is_a_typed_cross_size() {
        let mut child_style = ComputedStyle::initial();
        child_style.margin.top = 12.0;
        child_style.margin.bottom = 8.0;
        let container_style = ComputedStyle::initial();
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(300.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(300.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: Some(PhysicalContentHeight::new(content_box_pt(100.0))),
            height_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
        };

        assert_eq!(
            estimated_stretched_flex_item_cross_size(
                &child_style,
                &container_style,
                FlexDirection::Row,
                available,
            ),
            Some(FlexCrossSize::new(80.0))
        );
    }

    #[test]
    fn single_line_definite_stretch_is_the_only_non_balanced_premeasure_basis() {
        let child_style = ComputedStyle::initial();
        let container_style = ComputedStyle::initial();
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(300.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(300.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: Some(PhysicalContentHeight::new(content_box_pt(100.0))),
            height_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
        };

        let stretched = estimated_flex_item_available_space(
            &child_style,
            &container_style,
            FlexDirection::Row,
            available,
        );
        assert_eq!(
            stretched.height_basis,
            PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::DefiniteSingleLineStretch,
            )
        );

        let mut multiline_container = container_style.clone();
        multiline_container.flex_wrap = FlexWrap::Wrap;
        assert!(
            estimated_flex_item_available_space(
                &child_style,
                &multiline_container,
                FlexDirection::Row,
                available,
            )
            .stretched_height
            .is_none()
        );

        let mut non_stretch_container = container_style.clone();
        non_stretch_container.align_items.keyword = SelfAlignmentKeyword::Start;
        assert!(
            estimated_flex_item_available_space(
                &child_style,
                &non_stretch_container,
                FlexDirection::Row,
                available,
            )
            .stretched_height
            .is_none()
        );

        let mut explicit_cross_size = child_style.clone();
        *explicit_cross_size.box_values.height =
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(10.0),
            );
        assert!(
            estimated_flex_item_available_space(
                &explicit_cross_size,
                &container_style,
                FlexDirection::Row,
                available,
            )
            .stretched_height
            .is_none()
        );

        let mut auto_cross_margin = child_style;
        auto_cross_margin.box_values.margin.top = css::ComputedLengthPercentageOrAuto::Auto;
        assert!(
            estimated_flex_item_available_space(
                &auto_cross_margin,
                &container_style,
                FlexDirection::Row,
                available,
            )
            .stretched_height
            .is_none()
        );
    }

    #[test]
    fn estimated_baseline_selection_uses_order_modified_flex_line_order() {
        let line = EstimatedFlexLine {
            item_indices: vec![3, 7],
            cross_start: FlexCrossOffset::new(0.0),
            cross_size: FlexCrossSize::new(0.0),
        };

        // `row` in vertical-rl with RTL inline progression projects to a
        // physical column-reverse, but the order-modified first item remains
        // the first fallback baseline item.
        assert_eq!(
            estimated_flex_line_baseline_item_index(
                &line,
                FlexDirection::ColumnReverse,
                FlexBaselineSet::First,
            ),
            Some(3),
        );
        assert_eq!(
            estimated_flex_line_baseline_item_index(
                &line,
                FlexDirection::ColumnReverse,
                FlexBaselineSet::Last,
            ),
            Some(7),
        );
    }

    #[test]
    fn estimated_line_shared_baseline_wins_over_startmost_item_baseline() {
        let line = EstimatedFlexLine {
            item_indices: vec![0, 1],
            cross_start: FlexCrossOffset::new(0.0),
            cross_size: FlexCrossSize::new(16.0),
        };
        let item = |baseline_set, first_baseline| EstimatedFlexBaselineItem {
            outer_main_size: FlexMainSize::new(20.0),
            outer_cross_size: FlexCrossSize::new(16.0),
            margin_cross_start: FlexCrossLength::new(0.0),
            cross_alignment: EstimatedFlexItemCrossAlignment::Side(PhysicalSide::Top),
            baseline_set,
            first_baseline: Some(FlexCrossOffset::new(first_baseline)),
            last_baseline: Some(FlexCrossOffset::new(first_baseline)),
            synthesized_first_baseline: FlexCrossOffset::new(16.0),
            synthesized_last_baseline: FlexCrossOffset::new(0.0),
        };

        // The first item has a 9px baseline, but the second participates in
        // first-baseline alignment and establishes the line's 12px shared
        // baseline. This mirrors final flex-line export for nested flexes.
        assert_eq!(
            estimated_flex_line_baseline(
                &line,
                &[item(None, 9.0), item(Some(FlexBaselineSet::First), 12.0),],
                FlexDirection::Row,
                FlexBaselineSet::First,
            ),
            Some(FlexCrossOffset::new(12.0)),
        );
    }

    #[test]
    fn estimated_line_cross_size_includes_baseline_depths() {
        let item = |outer_cross_size, baseline, baseline_set| EstimatedFlexBaselineItem {
            outer_main_size: FlexMainSize::new(20.0),
            outer_cross_size: FlexCrossSize::new(outer_cross_size),
            margin_cross_start: FlexCrossLength::new(0.0),
            cross_alignment: EstimatedFlexItemCrossAlignment::Side(PhysicalSide::Top),
            baseline_set,
            first_baseline: Some(FlexCrossOffset::new(baseline)),
            last_baseline: Some(FlexCrossOffset::new(baseline)),
            synthesized_first_baseline: FlexCrossOffset::new(outer_cross_size),
            synthesized_last_baseline: FlexCrossOffset::new(0.0),
        };
        let items = [
            item(16.0, 12.0, Some(FlexBaselineSet::First)),
            item(28.0, 6.0, Some(FlexBaselineSet::First)),
        ];

        let line = estimated_flex_line(0, items.len(), FlexCrossOffset::new(0.0), &items);

        assert_eq!(line.cross_size, FlexCrossSize::new(34.0));
    }
}
