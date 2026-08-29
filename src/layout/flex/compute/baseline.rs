use super::*;

pub(in crate::layout::flex) fn apply_baseline_self_alignment_fallback_offset(
    item: &mut FlexItemLayout,
    child_style: &ComputedStyle,
    line: &FlexLineLayout,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
    cross_bounds: Option<(FlexCrossOffset, FlexCrossOffset)>,
) {
    if flex_item_has_auto_cross_margin(child_style, physical_direction) {
        return;
    }

    let (cross_start, cross_end) = cross_bounds.unwrap_or((line.cross_start, line.cross_end));
    let cross_size = (cross_end - cross_start).non_negative_size();
    let subject_side = match baseline_set {
        // `wrap-reverse` reverses the flex cross axis, including the
        // first-baseline fallback edge. A non-participating singleton must
        // therefore remain attached to that reversed cross-start rather than
        // reverting to its unreversed logical self-start.
        // <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property>
        // <https://www.w3.org/TR/css-align-3/#baseline-align-self>
        FlexBaselineSet::First
            if container_style.flex_wrap.reverses_cross_axis()
                && container_style.writing_mode.has_vertical_lines() =>
        {
            flex_cross_start_side(container_style)
        }
        FlexBaselineSet::First => child_self_start_side(child_style, container_style),
        FlexBaselineSet::Last => child_self_end_side(child_style, container_style),
    };
    let outer_size = item_outer_cross_size(item, child_style, physical_direction);
    let target_side = if (cross_size - outer_size).is_negative() {
        flex_cross_start_side(container_style)
    } else {
        subject_side
    };
    let mut fallback_line = line.clone();
    fallback_line.cross_start = cross_start;
    fallback_line.cross_end = cross_end;
    align_item_cross_side(
        item,
        child_style,
        physical_direction,
        &fallback_line,
        target_side,
    );
}

/// Return the cross-axis slot used by baseline fallback.
///
/// `wrap-reverse` packs a wrapped column line against cross-end. Taffy
/// represents a stretched single line with the whole container cross size, so
/// align the fallback group in its packed cross-end slot instead of resetting
/// it to cross-start:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property> and
/// <https://www.w3.org/TR/css-align-3/#baseline-align-self>.
pub(super) fn baseline_fallback_cross_bounds(
    indices: &[usize],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    line: &FlexLineLayout,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> Option<(FlexCrossOffset, FlexCrossOffset)> {
    if !container_style.flex_wrap.reverses_cross_axis() || !physical_direction.is_column_axis() {
        return None;
    }
    let (group_start, group_end) = indices
        .iter()
        .cloned()
        .map(|index| {
            item_outer_cross_bounds(&items[index], &children[index].style, physical_direction)
        })
        .fold(
            None::<(FlexCrossOffset, FlexCrossOffset)>,
            |bounds, item_bounds| {
                Some(match bounds {
                    Some((start, end)) => (start.min(item_bounds.0), end.max(item_bounds.1)),
                    None => item_bounds,
                })
            },
        )?;
    let group_size = group_end - group_start;
    Some((
        (line.cross_end - group_size).max(line.cross_start),
        line.cross_end,
    ))
}

/// Align a flex line baseline-sharing group to the line's cross-axis edge.
///
/// CSS Flexbox baseline self-alignment places the participant with the largest
/// distance between its baseline and the relevant cross-start/end margin edge
/// flush with that line edge, then aligns the other participants to the same
/// baseline:
/// <https://www.w3.org/TR/css-flexbox-1/#valdef-align-items-baseline> and
/// <https://drafts.csswg.org/css-align-3/#baseline-align-self>.
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::flex) fn align_baseline_sharing_group_to_line(
    items: &mut [FlexItemLayout],
    line: &FlexLineLayout,
    participants: &[ResolvedFlexBaselineParticipant],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) {
    let side = flex_baseline_sharing_group_alignment_side(
        container_style,
        baseline_set,
        participants.len(),
    );
    let target_distance = participants
        .iter()
        .map(|participant| {
            let index = participant.index;
            item_baseline_distance_to_cross_side(
                &items[index],
                &estimates[index],
                &children[index].style,
                container_style,
                baseline_set,
                physical_direction,
                participant.source,
                side,
            )
        })
        .fold(FlexCrossSize::new(0.0), FlexCrossSize::max);
    let target_baseline = if side.is_start_edge() {
        line.cross_start + target_distance
    } else {
        line.cross_end - target_distance
    };
    for participant in participants {
        let index = participant.index;
        let baseline = resolved_item_cross_axis_border_box_baseline(
            &items[index],
            &estimates[index],
            &children[index].style,
            container_style,
            baseline_set,
            physical_direction,
            participant.source,
        );
        // The selected item baseline is an offset from the border-box cross
        // start, whereas the line target is an absolute cross-axis coordinate.
        // Resolve their difference into the item's new absolute cross start:
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>.
        items[index].set_cross_start(
            PhysicalFlexDirection::new(physical_direction),
            FlexItemBorderBoxCrossStart::from_border_box_offset(
                FlexCrossOffset::new(0.0) + (target_baseline - baseline),
            ),
        );
    }
}

/// Return the edge used to place one baseline-sharing group.
///
/// `wrap-reverse` changes the cross-start attachment of a compatible
/// singleton, whose baseline fallback would otherwise select the item's
/// self-start edge. A multi-item sharing group instead retains the
/// unreversed first/last baseline edge while the line stack itself is
/// reversed. Keeping this decision at the group boundary prevents the
/// singleton correction from changing the shared baseline coordinate of an
/// ordinary line.
/// <https://www.w3.org/TR/css-flexbox-1/#valdef-align-items-baseline>
pub(in crate::layout::flex) fn flex_baseline_sharing_group_alignment_side(
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    participant_count: usize,
) -> PhysicalSide {
    let first_baseline_side =
        if participant_count == 1 && container_style.flex_wrap.reverses_cross_axis() {
            flex_cross_start_side(container_style)
        } else {
            flex_unreversed_cross_start_side(container_style)
        };
    match baseline_set {
        FlexBaselineSet::First => first_baseline_side,
        FlexBaselineSet::Last => opposite_physical_side(first_baseline_side),
    }
}

pub(in crate::layout::flex) fn shift_flex_line_cross_axis(
    line: &mut FlexLineLayout,
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
    delta: FlexCrossLength,
) {
    let axes = PhysicalFlexDirection::new(physical_direction);
    line.cross_start = line.cross_start + delta;
    line.cross_end = line.cross_end + delta;
    for &item_index in &line.item_indices {
        items[item_index].translate_cross(axes, delta);
    }
}

pub(in crate::layout::flex) fn collapsed_strut_line_overlap(
    strut: &FlexCollapsedStrut,
    line: &FlexLineLayout,
) -> usize {
    let start = strut.source_start.max(line.source_start);
    let end = strut.source_end.min(line.source_end);
    end.saturating_sub(start)
}

pub(in crate::layout::flex) fn item_outer_main_bounds(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> (FlexMainOffset, FlexMainOffset) {
    let (start, end) =
        item.outer_main_bounds(PhysicalFlexDirection::new(physical_direction), style);
    (start, end)
}

pub(in crate::layout::flex) fn flex_items_main_extent(
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) -> Option<(FlexMainOffset, FlexMainOffset)> {
    items
        .iter()
        .zip(children)
        .map(|(item, child)| item_outer_main_bounds(item, &child.style, physical_direction))
        .fold(None, |bounds, item_bounds| {
            Some(match bounds {
                Some((start, end)) => (start.min(item_bounds.0), end.max(item_bounds.1)),
                None => item_bounds,
            })
        })
}

pub(in crate::layout::flex) fn flex_line_items_main_extent(
    line: &FlexLineLayout,
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) -> Option<(FlexMainOffset, FlexMainOffset)> {
    line.item_indices
        .iter()
        .cloned()
        .map(|index| {
            item_outer_main_bounds(&items[index], &children[index].style, physical_direction)
        })
        .fold(None, |bounds, item_bounds| {
            Some(match bounds {
                Some((start, end)) => (start.min(item_bounds.0), end.max(item_bounds.1)),
                None => item_bounds,
            })
        })
}

pub(in crate::layout::flex) fn flex_line_baseline(
    line_indices: &[usize],
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> Option<FlexCrossOffset> {
    line_indices
        .iter()
        .cloned()
        .filter(|&index| {
            flex_baseline_set(&children[index].style, container_style) == Some(baseline_set)
        })
        .map(|index| {
            measured_item_cross_axis_baseline(
                &items[index],
                &estimates[index],
                &children[index].style,
                container_style,
                baseline_set,
                physical_direction,
            )
        })
        .reduce(FlexCrossOffset::max)
}

pub(in crate::layout::flex) fn flex_line_content_baseline(
    line: &FlexLineLayout,
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> Option<FlexCrossOffset> {
    let line_baseline = match baseline_set {
        FlexBaselineSet::First => line.first_baseline,
        FlexBaselineSet::Last => line.last_baseline,
    };
    if line_baseline.is_some() {
        return line_baseline;
    }

    flex_line_baseline_item_index(line, physical_direction, baseline_set).map(|index| {
        measured_item_cross_axis_baseline(
            &items[index],
            &estimates[index],
            &children[index].style,
            container_style,
            baseline_set,
            physical_direction,
        )
    })
}

pub(in crate::layout::flex) fn flex_line_baseline_item_index(
    line: &FlexLineLayout,
    _physical_direction: FlexDirection,
    baseline_set: FlexBaselineSet,
) -> Option<usize> {
    // `item_indices` is already in order-modified flex-line order. Flex
    // direction determines where that first item is physically placed, not
    // which order-modified item is main-start. Reversing this sequence again
    // for `*-reverse` therefore selects the main-end item instead.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
    match baseline_set {
        FlexBaselineSet::First => line.item_indices.first().copied(),
        FlexBaselineSet::Last => line.item_indices.last().copied(),
    }
}

/// The origin of an item baseline used for flex baseline alignment.
///
/// Keeping this distinction through final placement makes it explicit that a
/// missing measured baseline participates through CSS Align's border-box
/// synthesis, rather than through Taffy's provisional fallback geometry:
/// <https://drafts.csswg.org/css-align-3/#synthesize-baseline>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexBaselineSource {
    Measured,
    Synthesized,
}

/// A baseline-aligned item's final role in one flex line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexBaselineParticipation {
    Shares,
    Fallback,
}

/// An item resolved for one first- or last-baseline set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) struct ResolvedFlexBaselineParticipant {
    pub(in crate::layout::flex) index: usize,
    pub(in crate::layout::flex) source: FlexBaselineSource,
    pub(in crate::layout::flex) participation: FlexBaselineParticipation,
}
pub(in crate::layout::flex) fn flex_item_baseline_source(
    estimate: &FlexItemEstimate,
    baseline_set: FlexBaselineSet,
    line_axis: PhysicalAxis,
) -> FlexBaselineSource {
    let has_baseline = match (line_axis, baseline_set) {
        (PhysicalAxis::Horizontal, FlexBaselineSet::First) => {
            estimate.baselines.vertical.first.is_some()
        }
        (PhysicalAxis::Horizontal, FlexBaselineSet::Last) => {
            estimate.baselines.vertical.last.is_some()
        }
        (PhysicalAxis::Vertical, FlexBaselineSet::First) => {
            estimate.baselines.horizontal.first.is_some()
        }
        (PhysicalAxis::Vertical, FlexBaselineSet::Last) => {
            estimate.baselines.horizontal.last.is_some()
        }
    };
    if has_baseline {
        FlexBaselineSource::Measured
    } else {
        FlexBaselineSource::Synthesized
    }
}

pub(in crate::layout::flex) fn flex_baseline_alignment_side(
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> PhysicalSide {
    // Flexbox uses its cross-start edge for first-baseline placement. That
    // edge is reversed by `wrap-reverse`; a compatible baseline group must
    // use the same edge as the final flex-line slot.
    // <https://www.w3.org/TR/css-flexbox-1/#valdef-align-items-baseline>
    let first_baseline_side = flex_cross_start_side(container_style);
    match baseline_set {
        FlexBaselineSet::First => first_baseline_side,
        FlexBaselineSet::Last => opposite_physical_side(first_baseline_side),
    }
}

pub(in crate::layout::flex) fn opposite_physical_side(side: PhysicalSide) -> PhysicalSide {
    match side {
        PhysicalSide::Top => PhysicalSide::Bottom,
        PhysicalSide::Right => PhysicalSide::Left,
        PhysicalSide::Bottom => PhysicalSide::Top,
        PhysicalSide::Left => PhysicalSide::Right,
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout::flex) fn item_baseline_distance_to_cross_side(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
    source: FlexBaselineSource,
    side: PhysicalSide,
) -> FlexCrossSize {
    debug_assert_eq!(
        side.axis(),
        if physical_direction.is_row_axis() {
            PhysicalAxis::Vertical
        } else {
            PhysicalAxis::Horizontal
        }
    );
    let baseline = FlexCrossLength::new(
        resolved_item_cross_axis_border_box_baseline(
            item,
            estimate,
            child_style,
            container_style,
            baseline_set,
            physical_direction,
            source,
        )
        .points(),
    );
    let size = item.cross_size(PhysicalFlexDirection::new(physical_direction));
    let distance = match side {
        PhysicalSide::Top => FlexCrossLength::new(child_style.margin.top) + baseline,
        PhysicalSide::Right => FlexCrossLength::new(child_style.margin.right) + (size - baseline),
        PhysicalSide::Bottom => FlexCrossLength::new(child_style.margin.bottom) + (size - baseline),
        PhysicalSide::Left => FlexCrossLength::new(child_style.margin.left) + baseline,
    };
    distance.non_negative_size()
}

pub(in crate::layout::flex) fn measured_item_border_box_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> FlexVerticalBaselineOffset {
    measured_item_vertical_border_box_baseline_for_line_axis(
        item,
        estimate,
        child_style,
        container_style,
        baseline_set,
        flex_baseline_line_axis(container_style),
    )
}

/// Return an item's physical vertical baseline, synthesizing against an
/// explicitly selected baseline-line axis when content did not produce one.
///
/// Flexbox's internal baseline axis can differ from the physical line axis
/// needed when an inline flex container exports its main-axis baseline. The
/// latter must not fall back through the former, or a physical horizontal
/// synthesized baseline is incorrectly treated as a vertical offset.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
/// <https://drafts.csswg.org/css-align-3/#synthesize-baseline>
pub(in crate::layout::flex) fn measured_item_vertical_border_box_baseline_for_line_axis(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    baseline_line_axis: PhysicalAxis,
) -> FlexVerticalBaselineOffset {
    let measured = match baseline_set {
        FlexBaselineSet::First => estimate.baselines.vertical.first,
        FlexBaselineSet::Last => estimate.baselines.vertical.last,
    };
    measured.unwrap_or_else(|| {
        synthesized_item_border_box_baseline(
            item,
            child_style,
            container_style,
            baseline_set,
            baseline_line_axis,
        )
        .vertical()
    })
}

pub(in crate::layout::flex) fn measured_item_horizontal_border_box_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> FlexHorizontalBaselineOffset {
    measured_item_horizontal_border_box_baseline_for_line_axis(
        item,
        estimate,
        child_style,
        container_style,
        baseline_set,
        flex_baseline_line_axis(container_style),
    )
}

/// Return an item's physical horizontal baseline for the supplied baseline
/// line axis. Flex container baseline export can request an axis different
/// from the line's self-alignment axis, notably for a column flex container.
pub(in crate::layout::flex) fn measured_item_horizontal_border_box_baseline_for_line_axis(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    baseline_line_axis: PhysicalAxis,
) -> FlexHorizontalBaselineOffset {
    let measured = match baseline_set {
        FlexBaselineSet::First => estimate.baselines.horizontal.first,
        FlexBaselineSet::Last => estimate.baselines.horizontal.last,
    };
    measured.unwrap_or_else(|| {
        synthesized_item_border_box_baseline(
            item,
            child_style,
            container_style,
            baseline_set,
            baseline_line_axis,
        )
        .horizontal()
    })
}

/// Return the baseline selected by the final resolution record.
///
/// The source is carried to this boundary so measured and synthesized values
/// cannot be confused after flex-line construction has completed.
pub(in crate::layout::flex) fn resolved_item_cross_axis_border_box_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
    source: FlexBaselineSource,
) -> FlexCrossOffset {
    let axis = flex_baseline_line_axis(container_style);
    match (physical_direction.is_row_axis(), source) {
        (true, FlexBaselineSource::Measured) => {
            let baseline = match baseline_set {
                FlexBaselineSet::First => estimate.baselines.vertical.first,
                FlexBaselineSet::Last => estimate.baselines.vertical.last,
            }
            .expect("resolved measured vertical flex baseline must exist");
            FlexCrossOffset::new(0.0) + flex_cross_length_from_vertical_baseline(baseline)
        }
        (false, FlexBaselineSource::Measured) => {
            let baseline = match baseline_set {
                FlexBaselineSet::First => estimate.baselines.horizontal.first,
                FlexBaselineSet::Last => estimate.baselines.horizontal.last,
            }
            .expect("resolved measured horizontal flex baseline must exist");
            FlexCrossOffset::new(0.0) + flex_cross_length_from_horizontal_baseline(baseline)
        }
        (true, FlexBaselineSource::Synthesized) => {
            let baseline = synthesized_item_border_box_baseline(
                item,
                child_style,
                container_style,
                baseline_set,
                axis,
            )
            .vertical();
            FlexCrossOffset::new(0.0) + flex_cross_length_from_vertical_baseline(baseline)
        }
        (false, FlexBaselineSource::Synthesized) => {
            let baseline = synthesized_item_border_box_baseline(
                item,
                child_style,
                container_style,
                baseline_set,
                axis,
            )
            .horizontal();
            FlexCrossOffset::new(0.0) + flex_cross_length_from_horizontal_baseline(baseline)
        }
    }
}

/// Synthesizes a missing flex-item baseline from its border box.
///
/// CSS Align synthesizes an alphabetic baseline from the line-under edge of
/// the rectangle and a central baseline by averaging the rectangle edges; CSS
/// Flexbox says flex items synthesize from border edges. CSS Writing Modes
/// makes the central baseline dominant in vertical typographic mode for
/// `text-orientation:mixed` and `upright`, while sideways text keeps the
/// alphabetic baseline:
/// <https://drafts.csswg.org/css-align-3/#synthesize-baseline>,
/// <https://www.w3.org/TR/css-writing-modes-4/#text-baselines>, and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn synthesized_item_border_box_baseline(
    item: &FlexItemLayout,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    baseline_line_axis: PhysicalAxis,
) -> FlexPhysicalBaselineOffset {
    let synthesis_writing_mode =
        synthesis_writing_mode(child_style, container_style, baseline_line_axis);
    if vertical_typographic_mode_uses_central_baseline(child_style, synthesis_writing_mode) {
        return synthesized_item_central_border_box_baseline(item, baseline_line_axis);
    }
    let side = match baseline_set {
        FlexBaselineSet::First => line_under_side(synthesis_writing_mode),
        FlexBaselineSet::Last => line_over_side(synthesis_writing_mode),
    };
    match side {
        PhysicalSide::Top => {
            FlexPhysicalBaselineOffset::Vertical(flex_vertical_baseline_from_points(0.0))
        }
        PhysicalSide::Right => FlexPhysicalBaselineOffset::Horizontal(
            flex_horizontal_baseline_from_physical_width(item.width()),
        ),
        PhysicalSide::Bottom => FlexPhysicalBaselineOffset::Vertical(
            flex_vertical_baseline_from_physical_height(item.height()),
        ),
        PhysicalSide::Left => {
            FlexPhysicalBaselineOffset::Horizontal(flex_horizontal_baseline_from_points(0.0))
        }
    }
}

pub(in crate::layout::flex) fn synthesized_item_central_border_box_baseline(
    item: &FlexItemLayout,
    baseline_line_axis: PhysicalAxis,
) -> FlexPhysicalBaselineOffset {
    match baseline_line_axis {
        PhysicalAxis::Horizontal => FlexPhysicalBaselineOffset::Vertical(
            flex_vertical_baseline_from_physical_height(item.height()).half(),
        ),
        PhysicalAxis::Vertical => FlexPhysicalBaselineOffset::Horizontal(
            flex_horizontal_baseline_from_physical_width(item.width()).half(),
        ),
    }
}

/// Return the physical axis that flex baseline lines are parallel to.
///
/// CSS Flexbox derives row flex baselines from item baseline sets parallel to
pub(in crate::layout::flex) fn measured_item_cross_axis_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> FlexCrossOffset {
    if physical_direction.is_row_axis() {
        let baseline = flex_item_vertical_border_box_baseline_coordinate(
            item,
            measured_item_border_box_baseline(item, estimate, style, container_style, baseline_set),
        );
        return FlexCrossOffset::new(baseline.points());
    }
    let baseline = flex_item_horizontal_border_box_baseline_coordinate(
        item,
        measured_item_horizontal_border_box_baseline(
            item,
            estimate,
            style,
            container_style,
            baseline_set,
        ),
    );
    FlexCrossOffset::new(baseline.points())
}

/// The spec-selected source of one main-axis flex-container baseline.
///
/// Keeping shared, measured-item, and synthesized-item sources distinct makes
/// the priority order in CSS Flexbox 8.5 explicit. In particular, an absent
/// requested sharing group must check the opposite sharing group before an
/// item baseline or border-edge synthesis is considered:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexContainerMainAxisBaselineSource {
    Shared {
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

/// Return the first and last baseline sets exported by a flex container.
///
/// Flexbox first identifies the startmost/endmost finalized flex line. When
/// the compatible exported baseline is item-derived rather than a shared
/// main-axis line baseline, item selection remains scoped to that selected
/// line. Both stages operate after `order`, `flex-direction`, and final line
/// placement:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn flex_container_baselines(
    lines: &[FlexLineLayout],
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexContainerBaselineSets {
    let Some((first_line, last_line)) = flex_container_baseline_lines(lines, container_style)
    else {
        return FlexContainerBaselineSets::default();
    };
    let inline_axis = inline_start_side(
        container_style.writing_mode,
        container_style.used_direction(),
    )
    .axis();
    let main_axis = flex_baseline_line_axis(container_style);
    let first = if inline_axis == main_axis {
        flex_container_main_axis_baseline(
            first_line,
            items,
            estimates,
            children,
            container_style,
            FlexBaselineSet::First,
            physical_direction,
        )
        .map(|baseline| flex_cross_offset_as_physical_baseline(baseline, physical_direction))
    } else {
        flex_container_baseline_item(
            first_line,
            items,
            children,
            container_style,
            physical_direction,
            FlexBaselineSet::First,
        )
        .map(|index| {
            flex_item_baseline_for_container_axis(
                index,
                items,
                estimates,
                children,
                container_style,
                FlexBaselineSet::First,
                inline_axis,
            )
        })
    };
    let last = if inline_axis == main_axis {
        flex_container_main_axis_baseline(
            last_line,
            items,
            estimates,
            children,
            container_style,
            FlexBaselineSet::Last,
            physical_direction,
        )
        .map(|baseline| flex_cross_offset_as_physical_baseline(baseline, physical_direction))
    } else {
        flex_container_baseline_item(
            last_line,
            items,
            children,
            container_style,
            physical_direction,
            FlexBaselineSet::Last,
        )
        .map(|index| {
            flex_item_baseline_for_container_axis(
                index,
                items,
                estimates,
                children,
                container_style,
                FlexBaselineSet::Last,
                inline_axis,
            )
        })
    };
    FlexContainerBaselineSets {
        vertical: FlexItemBaselinePair {
            first: match first {
                Some(FlexPhysicalBaselineOffset::Vertical(baseline)) => Some(baseline),
                _ => None,
            },
            last: match last {
                Some(FlexPhysicalBaselineOffset::Vertical(baseline)) => Some(baseline),
                _ => None,
            },
        },
        horizontal: FlexItemBaselinePair {
            first: match first {
                Some(FlexPhysicalBaselineOffset::Horizontal(baseline)) => Some(baseline),
                _ => None,
            },
            last: match last {
                Some(FlexPhysicalBaselineOffset::Horizontal(baseline)) => Some(baseline),
                _ => None,
            },
        },
        vertical_metric: flex_container_baseline_metric(container_style),
        horizontal_metric: flex_container_baseline_metric(container_style),
    }
}

#[allow(clippy::too_many_arguments)]
fn flex_container_main_axis_baseline(
    fallback_line: &FlexLineLayout,
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> Option<FlexCrossOffset> {
    let source = flex_container_main_axis_baseline_source(
        fallback_line,
        estimates,
        children,
        container_style,
        baseline_set,
    )?;
    match source {
        FlexContainerMainAxisBaselineSource::Shared { baseline_set } => match baseline_set {
            FlexBaselineSet::First => fallback_line.first_baseline,
            FlexBaselineSet::Last => fallback_line.last_baseline,
        },
        FlexContainerMainAxisBaselineSource::Item {
            index,
            baseline_set,
        }
        | FlexContainerMainAxisBaselineSource::SynthesizedItem {
            index,
            baseline_set,
        } => Some(measured_item_cross_axis_baseline(
            &items[index],
            &estimates[index],
            &children[index].style,
            container_style,
            baseline_set,
            physical_direction,
        )),
    }
}

pub(in crate::layout::flex) fn flex_container_main_axis_baseline_source(
    fallback_line: &FlexLineLayout,
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> Option<FlexContainerMainAxisBaselineSource> {
    let shared_baseline = |line: &FlexLineLayout, set| match set {
        FlexBaselineSet::First => line.first_baseline,
        FlexBaselineSet::Last => line.last_baseline,
    };
    // Baseline-sharing priority belongs to the selected finalized
    // startmost/endmost flex line. `wrap-reverse` changes which physical line
    // occupies that edge, so order-modified line rank must not replace the
    // line selected by `flex_container_baseline_lines`.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
    if shared_baseline(fallback_line, baseline_set).is_some() {
        return Some(FlexContainerMainAxisBaselineSource::Shared { baseline_set });
    }
    let opposite_set = baseline_set.opposite();
    if shared_baseline(fallback_line, opposite_set).is_some() {
        return Some(FlexContainerMainAxisBaselineSource::Shared {
            baseline_set: opposite_set,
        });
    }

    let baseline_line_axis = flex_baseline_line_axis(container_style);
    let measured_set = |index: usize| {
        [baseline_set, opposite_set]
            .into_iter()
            .find(|&set| {
                flex_item_baseline_source(&estimates[index], set, baseline_line_axis)
                    == FlexBaselineSource::Measured
            })
            .map(|set| FlexContainerMainAxisBaselineSource::Item {
                index,
                baseline_set: set,
            })
    };
    let active_indices = || {
        fallback_line.item_indices.iter().copied().filter(|&index| {
            children
                .get(index)
                .is_some_and(|child| !flex_item_is_collapsed(&child.style))
        })
    };
    let item_source = match baseline_set {
        FlexBaselineSet::First => active_indices().find_map(measured_set),
        FlexBaselineSet::Last => active_indices().rev().find_map(measured_set),
    };
    if item_source.is_some() {
        return item_source;
    }

    let index = match baseline_set {
        FlexBaselineSet::First => active_indices().next(),
        FlexBaselineSet::Last => active_indices().next_back(),
    }?;
    Some(FlexContainerMainAxisBaselineSource::SynthesizedItem {
        index,
        baseline_set,
    })
}

fn flex_cross_offset_as_physical_baseline(
    baseline: FlexCrossOffset,
    physical_direction: FlexDirection,
) -> FlexPhysicalBaselineOffset {
    if physical_direction.is_row_axis() {
        FlexPhysicalBaselineOffset::Vertical(flex_vertical_baseline_from_points(baseline.points()))
    } else {
        FlexPhysicalBaselineOffset::Horizontal(flex_horizontal_baseline_from_points(
            baseline.points(),
        ))
    }
}

fn flex_container_baseline_item(
    line: &FlexLineLayout,
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    baseline_set: FlexBaselineSet,
) -> Option<usize> {
    let axes = WritingModeAxes::new(
        container_style.writing_mode,
        container_style.used_direction(),
    );
    let main_start = match container_style.flex_direction {
        FlexDirection::Row => LogicalSide::InlineStart,
        FlexDirection::RowReverse => LogicalSide::InlineEnd,
        FlexDirection::Column => LogicalSide::BlockStart,
        FlexDirection::ColumnReverse => LogicalSide::BlockEnd,
    };
    let main_start = axes.physical_side(main_start);

    // `FlexLineLayout::item_indices` preserves order-modified order.  Final
    // main-axis edges select the CSS startmost/endmost item, while this rank
    // resolves geometrically coincident items without falling back to DOM
    // order.  In particular, a reverse direction changes the physical edge
    // which is startmost; it must not be implemented by reversing a source
    // list after layout.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
    let ordered_items = line
        .item_indices
        .iter()
        .copied()
        .filter(|&index| {
            children
                .get(index)
                .is_some_and(|child| !flex_item_is_collapsed(&child.style))
        })
        .enumerate()
        .collect::<Vec<_>>();
    let main_progress = |index: usize| {
        let (start, end) = item_outer_main_bounds(
            items.get(index).expect("flex line item has final geometry"),
            &children[index].style,
            physical_direction,
        );
        if main_start.is_start_edge() {
            start.points()
        } else {
            -end.points()
        }
    };
    let compare = |left: &(usize, usize), right: &(usize, usize)| {
        main_progress(left.1)
            .partial_cmp(&main_progress(right.1))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    };
    match baseline_set {
        FlexBaselineSet::First => ordered_items
            .iter()
            .min_by(|left, right| compare(left, right))
            .map(|(_, index)| *index),
        FlexBaselineSet::Last => ordered_items
            .iter()
            .max_by(|left, right| compare(left, right))
            .map(|(_, index)| *index),
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout::flex) fn flex_item_baseline_for_container_axis(
    index: usize,
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    baseline_line_axis: PhysicalAxis,
) -> FlexPhysicalBaselineOffset {
    let item = &items[index];
    let child_style = &children[index].style;
    match baseline_line_axis {
        PhysicalAxis::Horizontal => {
            FlexPhysicalBaselineOffset::Vertical(flex_item_vertical_border_box_baseline_coordinate(
                item,
                measured_item_vertical_border_box_baseline_for_line_axis(
                    item,
                    &estimates[index],
                    child_style,
                    container_style,
                    baseline_set,
                    baseline_line_axis,
                ),
            ))
        }
        PhysicalAxis::Vertical => FlexPhysicalBaselineOffset::Horizontal(
            flex_item_horizontal_border_box_baseline_coordinate(
                item,
                measured_item_horizontal_border_box_baseline_for_line_axis(
                    item,
                    &estimates[index],
                    child_style,
                    container_style,
                    baseline_set,
                    baseline_line_axis,
                ),
            ),
        ),
    }
}

/// Select the startmost and endmost finalized flex lines for baseline export.
///
/// This is intentionally based on final physical line geometry, rather than
/// the order-modified line membership. The startmost/endmost terms are relative
/// to the container's ordinary writing-mode cross axis: `wrap-reverse` changes
/// flex-line stacking, but does not exchange those baseline-export edges.
/// `align-content` translations still have to be reflected before export:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn flex_container_baseline_lines<'a>(
    lines: &'a [FlexLineLayout],
    container_style: &ComputedStyle,
) -> Option<(&'a FlexLineLayout, &'a FlexLineLayout)> {
    let cross_start = flex_unreversed_cross_start_side(container_style);
    let (first, last) = if cross_start.is_start_edge() {
        (
            lines.iter().min_by(|left, right| {
                left.cross_start
                    .partial_cmp(&right.cross_start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            lines.iter().max_by(|left, right| {
                left.cross_end
                    .partial_cmp(&right.cross_end)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        )
    } else {
        (
            lines.iter().max_by(|left, right| {
                left.cross_end
                    .partial_cmp(&right.cross_end)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            lines.iter().min_by(|left, right| {
                left.cross_start
                    .partial_cmp(&right.cross_start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        )
    };
    let first = first?;
    let last = last?;
    Some((first, last))
}

/// Apply CSS Box Alignment baseline content-alignment to wrapped flex lines.
///
/// Taffy's `AlignContentKeyword` has no baseline keywords, so the adapter maps
/// `align-content: baseline` to start packing. CSS Align instead treats flex
/// lines as the alignment subjects and aligns their compatible baseline sets
/// when those sets are available. `FlexCrossOffset` is projected by the
/// physical flex direction, so this also covers a logical row in vertical
/// writing, whose cross axis is physically horizontal:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-content>.
pub(in crate::layout::flex) fn apply_baseline_align_content_offsets(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: FlexCrossSize,
) -> bool {
    if lines.is_empty() || container_style.flex_wrap == FlexWrap::NoWrap {
        return false;
    }

    let baseline_set = match container_style.align_content.keyword {
        ContentAlignmentKeyword::Baseline => FlexBaselineSet::First,
        ContentAlignmentKeyword::LastBaseline => FlexBaselineSet::Last,
        _ => return false,
    };

    let line_baselines = lines
        .iter()
        .map(|line| {
            flex_line_content_baseline(
                line,
                items,
                estimates,
                children,
                container_style,
                baseline_set,
                physical_direction,
            )
        })
        .collect::<Vec<_>>();
    if line_baselines
        .iter()
        .filter(|baseline| baseline.is_some())
        .count()
        <= 1
    {
        apply_baseline_align_content_fallback_offset(
            items,
            lines,
            children,
            container_style,
            physical_direction,
            container_cross_size,
            baseline_set,
        );
        return true;
    }

    // CSS Flexbox defines line baseline sets for a row main axis. A logical
    // row can project to a physical column in vertical writing, so this must
    // use the authored logical flex direction rather than `physical_direction`.
    if !container_style.flex_direction.is_row_axis() {
        apply_baseline_align_content_fallback_offset(
            items,
            lines,
            children,
            container_style,
            physical_direction,
            container_cross_size,
            baseline_set,
        );
        return true;
    }

    let target_baseline = line_baselines
        .iter()
        .flatten()
        .copied()
        .reduce(FlexCrossOffset::max)
        .expect("more than one baseline participant has a baseline");

    for (line_index, baseline) in line_baselines.into_iter().enumerate() {
        let Some(baseline) = baseline else {
            continue;
        };
        let delta = target_baseline - baseline;
        if delta.abs() <= 0.01 {
            continue;
        }
        shift_flex_line_cross_axis(&mut lines[line_index], items, physical_direction, delta);
    }
    true
}

/// Applies content-alignment fallback when line baselines cannot be shared.
///
/// CSS Align defines first-baseline content alignment fallback as safe logical
/// start, and last-baseline content alignment fallback as safe logical end.
/// The fallback moves the flex-line group in the flex container cross axis:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-content> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
pub(in crate::layout::flex) fn apply_baseline_align_content_fallback_offset(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: FlexCrossSize,
    baseline_set: FlexBaselineSet,
) {
    // A stretched single line occupies the whole cross-size, but baseline
    // content-alignment falls back by positioning the alignment subjects, not
    // by treating that stretch slot as an already-positioned subject.
    let Some((group_start, group_end)) =
        flex_line_alignment_subject_cross_bounds(lines, items, children, physical_direction)
    else {
        return;
    };
    let group_size = (group_end - group_start).non_negative_size();
    let target_side = if group_size > container_cross_size {
        baseline_content_alignment_fallback_side(container_style, FlexBaselineSet::First)
    } else {
        baseline_content_alignment_fallback_side(container_style, baseline_set)
    };
    align_flex_line_group_cross_side_from_bounds(
        lines,
        items,
        physical_direction,
        target_side,
        container_cross_size,
        group_start,
        group_end,
    );
}

/// Baseline content-alignment fallbacks use logical start/end. `wrap-reverse`
/// reverses flex-line packing, but does not reverse the fallback defined by
/// CSS Box Alignment.
fn baseline_content_alignment_fallback_side(
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> PhysicalSide {
    let logical_start = if container_style.flex_wrap.reverses_cross_axis() {
        flex_cross_end_side(container_style)
    } else {
        flex_cross_start_side(container_style)
    };
    match baseline_set {
        FlexBaselineSet::First => logical_start,
        FlexBaselineSet::Last => logical_start.opposite(),
    }
}
