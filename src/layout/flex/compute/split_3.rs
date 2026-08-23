use super::*;
use crate::layout::taffy_bridge;

/// Applies `align-content: stretch` fallback when stretched flex lines overflow.
///
/// CSS Align defines `stretch` as falling back to `flex-start`, not
/// `safe flex-start`, so an overflowing wrapped line remains packed against
/// the flex cross-start side. Taffy's generic distribution fallback differs,
/// so Quire corrects only the overflow case after recovering flex line
/// metadata:
/// <https://drafts.csswg.org/css-align/#valdef-align-content-stretch> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
pub(in crate::layout::flex) fn apply_stretch_align_content_overflow_fallback_offsets(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: FlexCrossSize,
) {
    if container_style.flex_wrap == FlexWrap::NoWrap
        || !matches!(
            container_style.align_content.keyword,
            ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch
        )
    {
        return;
    }

    let Some((group_start, group_end)) =
        flex_line_alignment_subject_cross_bounds(lines, items, children, physical_direction)
    else {
        return;
    };
    let group_size = (group_end - group_start).non_negative_size();
    if group_size <= container_cross_size + FlexCrossSize::new(0.01) {
        return;
    }

    align_flex_line_group_cross_side_from_bounds(
        lines,
        items,
        physical_direction,
        flex_line_packing_flex_start_side(container_style),
        container_cross_size,
        group_start,
        group_end,
    );
}

pub(in crate::layout::flex) fn flex_line_packing_flex_start_side(
    style: &ComputedStyle,
) -> PhysicalSide {
    // `flex_cross_start_side` already applies `wrap-reverse`; line packing
    // must not invert the axis a second time.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property>
    flex_cross_start_side(style)
}

pub(in crate::layout::flex) fn flex_line_alignment_subject_cross_bounds(
    lines: &[FlexLineLayout],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) -> Option<(FlexCrossOffset, FlexCrossOffset)> {
    lines
        .iter()
        .filter_map(|line| {
            if line.item_indices.is_empty() {
                return Some((line.cross_start, line.cross_end));
            }
            line.item_indices
                .iter()
                .map(|&index| {
                    item_outer_cross_bounds(
                        &items[index],
                        &children[index].style,
                        physical_direction,
                    )
                })
                .fold(
                    None::<(FlexCrossOffset, FlexCrossOffset)>,
                    |bounds, item_bounds| {
                        Some(match bounds {
                            Some((start, end)) => {
                                (start.min(item_bounds.0), end.max(item_bounds.1))
                            }
                            None => item_bounds,
                        })
                    },
                )
                .map(|(start, end)| {
                    (
                        start.min(line.cross_start),
                        end.max(line.cross_start + line.largest_collapsed_strut()),
                    )
                })
        })
        .fold(None, |bounds, line_bounds| {
            Some(match bounds {
                Some((start, end)) => (start.min(line_bounds.0), end.max(line_bounds.1)),
                None => line_bounds,
            })
        })
}

pub(in crate::layout::flex) fn align_flex_line_group_cross_side_from_bounds(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
    side: PhysicalSide,
    container_cross_size: FlexCrossSize,
    group_start: FlexCrossOffset,
    group_end: FlexCrossOffset,
) {
    let cross_origin = FlexCrossOffset::new(0.0);
    let delta = if side.is_start_edge() {
        cross_origin - group_start
    } else if side.is_end_edge() {
        (cross_origin + container_cross_size) - group_end
    } else {
        FlexCrossLength::new(0.0)
    };
    if delta.abs() <= 0.01 {
        return;
    }
    for line in lines {
        shift_flex_line_cross_axis(line, items, physical_direction, delta);
    }
}

/// Return whether a flex item can join the container's baseline-sharing group.
///
/// CSS Flexbox only collects baseline-aligned flex items whose inline axis is
/// parallel to the flex container's main axis. Items with an orthogonal inline
/// axis fall back through CSS Align's first/last-baseline self-alignment
/// fallback instead:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line> and
/// <https://drafts.csswg.org/css-align-3/#baseline-align-self>.
pub(in crate::layout::flex) fn flex_item_baseline_axis_is_parallel_to_main_axis(
    child_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> bool {
    let item_inline_axis =
        inline_start_side(child_style.writing_mode, child_style.used_direction()).axis();
    item_inline_axis
        == if physical_direction.is_row_axis() {
            PhysicalAxis::Horizontal
        } else {
            PhysicalAxis::Vertical
        }
}

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
fn baseline_fallback_cross_bounds(
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

pub(in crate::layout::flex) fn align_item_cross_side(
    item: &mut FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    line: &FlexLineLayout,
    side: PhysicalSide,
) {
    debug_assert_eq!(
        side.axis(),
        if physical_direction.is_row_axis() {
            PhysicalAxis::Vertical
        } else {
            PhysicalAxis::Horizontal
        }
    );
    let axes = PhysicalFlexDirection::new(physical_direction);
    let border_box_cross_start = match (
        physical_direction.is_row_axis(),
        side.is_start_edge(),
        side.is_end_edge(),
    ) {
        (true, true, false) => Some(FlexItemBorderBoxCrossStart::from_line_cross_start_margin(
            line.cross_start,
            FlexCrossLength::new(style.margin.top),
        )),
        (true, false, true) => Some(FlexItemBorderBoxCrossStart::from_line_cross_end_margin(
            line.cross_end,
            FlexCrossLength::new(style.margin.bottom),
            item.cross_size(axes),
        )),
        (false, true, false) => Some(FlexItemBorderBoxCrossStart::from_line_cross_start_margin(
            line.cross_start,
            FlexCrossLength::new(style.margin.left),
        )),
        (false, false, true) => Some(FlexItemBorderBoxCrossStart::from_line_cross_end_margin(
            line.cross_end,
            FlexCrossLength::new(style.margin.right),
            item.cross_size(axes),
        )),
        _ => None,
    };
    if let Some(border_box_cross_start) = border_box_cross_start {
        item.set_cross_start(axes, border_box_cross_start);
    }
}

pub(in crate::layout::flex) fn taffy_justify_content(
    justify_content: JustifyContent,
    axes: FlexAxes,
) -> Option<taffy_layout::JustifyContent> {
    let safety = taffy_safety(justify_content.safety);
    match justify_content.keyword {
        ContentAlignmentKeyword::Normal
        | ContentAlignmentKeyword::FlexStart
        | ContentAlignmentKeyword::Stretch => Some(taffy_layout::JustifyContent {
            keyword: taffy_layout::AlignContentKeyword::FlexStart,
            safety,
        }),
        ContentAlignmentKeyword::FlexEnd => Some(taffy_layout::JustifyContent {
            keyword: taffy_layout::AlignContentKeyword::FlexEnd,
            safety,
        }),
        ContentAlignmentKeyword::Start => Some(taffy_layout::JustifyContent {
            keyword: taffy_layout::AlignContentKeyword::Start,
            safety,
        }),
        ContentAlignmentKeyword::End => Some(taffy_layout::JustifyContent {
            keyword: taffy_layout::AlignContentKeyword::End,
            safety,
        }),
        // `left` and `right` are physical horizontal alignment keywords. They
        // are positional whenever the flex main axis is physical horizontal,
        // including a column flex container in vertical or sideways writing
        // modes; otherwise they compute to `start`.
        // <https://drafts.csswg.org/css-align-3/#justify-content-property>
        ContentAlignmentKeyword::Left | ContentAlignmentKeyword::Right
            if !axes.is_main_row_axis() =>
        {
            Some(taffy_layout::JustifyContent {
                keyword: taffy_layout::AlignContentKeyword::Start,
                safety,
            })
        }
        ContentAlignmentKeyword::Left => Some(if PhysicalSide::Left == axes.main_start_side() {
            taffy_layout::JustifyContent {
                keyword: taffy_layout::AlignContentKeyword::FlexStart,
                safety,
            }
        } else {
            debug_assert_eq!(PhysicalSide::Left, axes.main_end_side());
            taffy_layout::JustifyContent {
                keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                safety,
            }
        }),
        ContentAlignmentKeyword::Right => Some(if PhysicalSide::Right == axes.main_start_side() {
            taffy_layout::JustifyContent {
                keyword: taffy_layout::AlignContentKeyword::FlexStart,
                safety,
            }
        } else {
            debug_assert_eq!(PhysicalSide::Right, axes.main_end_side());
            taffy_layout::JustifyContent {
                keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                safety,
            }
        }),
        ContentAlignmentKeyword::Center => Some(taffy_layout::JustifyContent {
            keyword: taffy_layout::AlignContentKeyword::Center,
            safety,
        }),
        ContentAlignmentKeyword::SpaceBetween => Some(taffy_layout::JustifyContent::SPACE_BETWEEN),
        ContentAlignmentKeyword::SpaceAround => Some(taffy_layout::JustifyContent::SPACE_AROUND),
        ContentAlignmentKeyword::SpaceEvenly => Some(taffy_layout::JustifyContent::SPACE_EVENLY),
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            Some(taffy_layout::JustifyContent::FLEX_START)
        }
    }
}

/// Reproject Taffy's flex-item cross-axis rectangles when CSS cross-start is
/// the physical bottom edge.
///
/// Taffy's `Direction` can express a horizontal start side, but it has no
/// top-to-bottom/bottom-to-top equivalent. A vertical-writing column flex
/// container therefore needs a coordinate conversion when its inline axis is
/// RTL. Taffy still forms and sizes the lines; this maps its top-origin cross
/// coordinates to CSS's bottom-origin inline axis before Quire constructs line
/// metadata or performs any CSS Align placement.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>
/// <https://www.w3.org/TR/css-writing-modes-4/#inline-flow>
pub(in crate::layout::flex) fn reproject_taffy_item_cross_axis_coordinates(
    items: &mut [FlexItemLayout],
    axes: FlexAxes,
    container_cross_size: FlexCrossSize,
) {
    if axes.taffy_cross_axis_projection() != TaffyCrossAxisProjection::Reflect {
        return;
    }
    let cross_origin = FlexCrossOffset::new(0.0);
    let container_cross_end = cross_origin + container_cross_size;
    for item in items {
        item.set_cross_start(
            axes,
            FlexItemBorderBoxCrossStart::from_border_box_offset(
                container_cross_end
                    - (item.cross_start(axes) - cross_origin)
                    - item.cross_size(axes),
            ),
        );
    }
}

/// Maps CSS `direction` to Taffy's physical LTR/RTL switch.
pub(in crate::layout::flex) fn taffy_direction(direction: Direction) -> ::taffy::Direction {
    taffy_bridge::direction(direction)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexBaselineSet {
    First,
    Last,
}

impl FlexBaselineSet {
    pub(in crate::layout::flex) fn opposite(self) -> Self {
        match self {
            Self::First => Self::Last,
            Self::Last => Self::First,
        }
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

/// The final cross-axis behavior of one flex item after CSS Align has resolved
/// `align-self:auto` and the item's writing-mode-dependent sides.
///
/// Taffy receives only a sizing-compatible placeholder for several of these
/// values. This record preserves the CSS decision until final flex-line slots
/// exist, preventing later remeasurement from mixing a stale Taffy position
/// with Quire's baseline or subject-axis correction.
/// <https://www.w3.org/TR/css-align-3/#self-alignment> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-align>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexCrossPlacementMode {
    AutoCrossMargin,
    Stretch,
    Side(PhysicalSide),
    Center,
    Baseline {
        set: FlexBaselineSet,
        source: FlexBaselineSource,
        participation: FlexBaselineParticipation,
    },
}

/// Final CSS cross-axis alignment data for an active flex item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) struct ResolvedFlexCrossAlignment {
    pub(in crate::layout::flex) mode: FlexCrossPlacementMode,
    pub(in crate::layout::flex) safety: AlignmentSafety,
    pub(in crate::layout::flex) flex_cross_start: PhysicalSide,
    pub(in crate::layout::flex) flex_cross_end: PhysicalSide,
    pub(in crate::layout::flex) self_start: PhysicalSide,
    pub(in crate::layout::flex) self_end: PhysicalSide,
}

/// Derive the CSS cross-axis alignment decision once for each active item.
pub(in crate::layout::flex) fn resolve_flex_cross_alignments(
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> Vec<ResolvedFlexCrossAlignment> {
    let flex_cross_start = flex_cross_start_side(container_style);
    let flex_cross_end = flex_cross_end_side(container_style);
    let baseline_line_axis = flex_baseline_line_axis(container_style);
    children
        .iter()
        .zip(estimates)
        .map(|(child, estimate)| {
            let child_style = &child.style;
            let alignment = effective_align_self(child_style, container_style);
            let self_start = child_self_start_side(child_style, container_style);
            let self_end = child_self_end_side(child_style, container_style);
            let mode = if flex_item_has_auto_cross_margin(child_style, physical_direction) {
                FlexCrossPlacementMode::AutoCrossMargin
            } else {
                match alignment.keyword {
                    SelfAlignmentKeyword::Baseline | SelfAlignmentKeyword::LastBaseline => {
                        let set = if alignment.keyword == SelfAlignmentKeyword::Baseline {
                            FlexBaselineSet::First
                        } else {
                            FlexBaselineSet::Last
                        };
                        let participation = if flex_item_baseline_axis_is_parallel_to_main_axis(
                            child_style,
                            physical_direction,
                        ) {
                            FlexBaselineParticipation::Shares
                        } else {
                            // The fallback is resolved by the final baseline
                            // placement phase, which preserves the packed
                            // `wrap-reverse` line slot before aligning the
                            // subject to safe self-start/self-end.
                            FlexBaselineParticipation::Fallback
                        };
                        FlexCrossPlacementMode::Baseline {
                            set,
                            source: flex_item_baseline_source(estimate, set, baseline_line_axis),
                            participation,
                        }
                    }
                    SelfAlignmentKeyword::Center => FlexCrossPlacementMode::Center,
                    SelfAlignmentKeyword::SelfStart => FlexCrossPlacementMode::Side(self_start),
                    SelfAlignmentKeyword::SelfEnd => FlexCrossPlacementMode::Side(self_end),
                    SelfAlignmentKeyword::End | SelfAlignmentKeyword::FlexEnd => {
                        FlexCrossPlacementMode::Side(flex_cross_end)
                    }
                    SelfAlignmentKeyword::Normal
                    | SelfAlignmentKeyword::Stretch
                    | SelfAlignmentKeyword::Auto => FlexCrossPlacementMode::Stretch,
                    SelfAlignmentKeyword::Start
                    | SelfAlignmentKeyword::FlexStart
                    | SelfAlignmentKeyword::Left
                    | SelfAlignmentKeyword::Right => FlexCrossPlacementMode::Side(flex_cross_start),
                }
            };
            ResolvedFlexCrossAlignment {
                mode,
                safety: alignment.safety,
                flex_cross_start,
                flex_cross_end,
                self_start,
                self_end,
            }
        })
        .collect()
}

/// Resolve every flex cross-axis placement exactly once.
///
/// Taffy's measure callback has no baseline channel, so its output is used for
/// flex sizing and line construction only.  Quire then resolves CSS Flexbox
/// baseline-sharing eligibility, fallback, and measured/synthesized baselines
/// together from final item geometry.  Keeping these phases together avoids
/// row, column, and fallback passes disagreeing about the same item:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line> and
/// <https://drafts.csswg.org/css-align-3/#baseline-align-self>.
pub(in crate::layout::flex) fn finalize_flex_cross_axis_placement(
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    lines: &mut [FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    alignments: &[ResolvedFlexCrossAlignment],
) {
    for line in &*lines {
        for &index in &line.item_indices {
            let alignment = alignments[index];
            let child_style = &children[index].style;
            match alignment.mode {
                FlexCrossPlacementMode::AutoCrossMargin => {
                    place_item_with_final_auto_cross_margins(
                        &mut items[index],
                        child_style,
                        physical_direction,
                        line,
                    );
                }
                FlexCrossPlacementMode::Stretch => {}
                FlexCrossPlacementMode::Side(side) => {
                    let outer_size =
                        item_outer_cross_size(&items[index], child_style, physical_direction);
                    let target_side = if alignment.safety == AlignmentSafety::Safe
                        && (line.cross_size() - outer_size).is_negative()
                    {
                        alignment.flex_cross_start
                    } else {
                        side
                    };
                    align_item_cross_side(
                        &mut items[index],
                        child_style,
                        physical_direction,
                        line,
                        target_side,
                    );
                }
                FlexCrossPlacementMode::Center => {
                    align_item_cross_center(
                        &mut items[index],
                        child_style,
                        physical_direction,
                        line,
                        alignment,
                    );
                }
                FlexCrossPlacementMode::Baseline { .. } => {}
            }
        }
        for baseline_set in [FlexBaselineSet::First, FlexBaselineSet::Last] {
            let participants = line
                .item_indices
                .iter()
                .filter_map(|&index| match alignments[index].mode {
                    FlexCrossPlacementMode::Baseline {
                        set,
                        source,
                        participation,
                    } if set == baseline_set => Some(ResolvedFlexBaselineParticipant {
                        index,
                        source,
                        participation,
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if participants.is_empty() {
                continue;
            }

            let sharing_participants = participants
                .iter()
                .filter(|participant| {
                    participant.participation == FlexBaselineParticipation::Shares
                })
                .copied()
                .collect::<Vec<_>>();
            let mut fallback_indices = participants
                .iter()
                .filter(|participant| {
                    participant.participation == FlexBaselineParticipation::Fallback
                })
                .map(|participant| participant.index)
                .collect::<Vec<_>>();

            // CSS Align's ordinary singleton fallback is retained for an
            // unreversed line. With `wrap-reverse`, however, a compatible
            // sole participant remains attached to the line's reversed Flex
            // cross-start edge; applying the generic self-alignment fallback
            // incorrectly moves it to self-start.
            // <https://www.w3.org/TR/css-flexbox-1/#valdef-align-items-baseline>
            if sharing_participants.len() > 1
                || (sharing_participants.len() == 1
                    && container_style.flex_wrap.reverses_cross_axis())
            {
                align_baseline_sharing_group_to_line(
                    items,
                    line,
                    &sharing_participants,
                    estimates,
                    children,
                    container_style,
                    baseline_set,
                    physical_direction,
                );
            } else {
                fallback_indices.extend(
                    sharing_participants
                        .iter()
                        .map(|participant| participant.index),
                );
            }

            if fallback_indices.is_empty() {
                continue;
            }
            let fallback_cross_bounds = baseline_fallback_cross_bounds(
                &fallback_indices,
                items,
                children,
                line,
                container_style,
                physical_direction,
            );
            for index in fallback_indices {
                apply_baseline_self_alignment_fallback_offset(
                    &mut items[index],
                    &children[index].style,
                    line,
                    container_style,
                    baseline_set,
                    physical_direction,
                    fallback_cross_bounds,
                );
            }
        }
    }
    // Placement changes the absolute baseline coordinates but never the
    // resolved line slots. Refresh this derived export at the same boundary
    // so callers cannot accidentally retain pre-placement baseline metadata.
    refresh_flex_line_baselines(
        lines,
        items,
        estimates,
        children,
        container_style,
        physical_direction,
    );
}

/// Resolve automatic cross-axis margins from a flex line's final slot.
///
/// Taffy distributes automatic margins while initially allocating a line, but
/// Flex can subsequently replace that provisional cross slot after final
/// normal-flow measurement and `align-content` distribution.  CSS Flexbox
/// resolves an item's auto cross margins against that final line size; leaving
/// the provisional location in place makes `margin-inline:auto` behave like
/// cross-start in stretched wrapped columns.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-margins>
fn place_item_with_final_auto_cross_margins(
    item: &mut FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    line: &FlexLineLayout,
) {
    let (before_is_auto, after_is_auto, before_margin, after_margin) =
        if physical_direction.is_row_axis() {
            (
                style.box_values.margin.top.is_auto(),
                style.box_values.margin.bottom.is_auto(),
                FlexCrossLength::new(style.margin.top),
                FlexCrossLength::new(style.margin.bottom),
            )
        } else {
            (
                style.box_values.margin.left.is_auto(),
                style.box_values.margin.right.is_auto(),
                FlexCrossLength::new(style.margin.left),
                FlexCrossLength::new(style.margin.right),
            )
        };
    let auto_count = before_is_auto as usize + after_is_auto as usize;
    debug_assert!(auto_count > 0);

    let axes = PhysicalFlexDirection::new(physical_direction);
    let zero = FlexCrossLength::new(0.0);
    let fixed_outer_size = item.cross_size(axes)
        + if before_is_auto { zero } else { before_margin }
        + if after_is_auto { zero } else { after_margin };
    let free_space = (line.cross_size() - fixed_outer_size).max(zero);
    let auto_margin = free_space.divide(
        std::num::NonZeroUsize::new(auto_count)
            .expect("a cross-axis auto margin selected this placement"),
    );
    let border_box_cross_start = line.cross_start
        + if before_is_auto {
            auto_margin
        } else {
            before_margin
        };
    item.set_cross_start(
        axes,
        FlexItemBorderBoxCrossStart::from_border_box_offset(border_box_cross_start),
    );
}

/// Align a margin box to the center of one final flex-line slot.
fn align_item_cross_center(
    item: &mut FlexItemLayout,
    child_style: &ComputedStyle,
    physical_direction: FlexDirection,
    line: &FlexLineLayout,
    alignment: ResolvedFlexCrossAlignment,
) {
    let outer_size = item_outer_cross_size(item, child_style, physical_direction);
    if alignment.safety == AlignmentSafety::Safe && (line.cross_size() - outer_size).is_negative() {
        align_item_cross_side(
            item,
            child_style,
            physical_direction,
            line,
            alignment.flex_cross_start,
        );
        return;
    }
    let axes = PhysicalFlexDirection::new(physical_direction);
    let free_space = line.cross_size() - outer_size;
    let border_box_cross_start = if physical_direction.is_row_axis() {
        line.cross_start + free_space.half() + FlexCrossLength::new(child_style.margin.top)
    } else {
        line.cross_start + free_space.half() + FlexCrossLength::new(child_style.margin.left)
    };
    item.set_cross_start(
        axes,
        FlexItemBorderBoxCrossStart::from_border_box_offset(border_box_cross_start),
    );
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

pub(in crate::layout::flex) fn flex_baseline_set(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> Option<FlexBaselineSet> {
    match child_style.align_self.keyword {
        SelfAlignmentKeyword::Baseline => Some(FlexBaselineSet::First),
        SelfAlignmentKeyword::LastBaseline => Some(FlexBaselineSet::Last),
        SelfAlignmentKeyword::Auto => match container_style.align_items.keyword {
            SelfAlignmentKeyword::Baseline => Some(FlexBaselineSet::First),
            SelfAlignmentKeyword::LastBaseline => Some(FlexBaselineSet::Last),
            _ => None,
        },
        _ => None,
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

pub(in crate::layout::flex) fn vertical_typographic_mode_uses_central_baseline(
    child_style: &ComputedStyle,
    synthesis_writing_mode: WritingMode,
) -> bool {
    matches!(
        synthesis_writing_mode.text_layout_policy(child_style.text_orientation),
        css::TextLayoutPolicy::Vertical(
            css::TextOrientation::Mixed | css::TextOrientation::Upright
        )
    )
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
/// the flex container's main axis, and CSS Writing Modes maps that CSS axis
/// into physical page coordinates. Keeping this as CSS-axis metadata prevents
/// baseline synthesis from depending on Taffy's row/column adapter encoding:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(in crate::layout::flex) fn flex_baseline_line_axis(
    container_style: &ComputedStyle,
) -> PhysicalAxis {
    match (
        container_style.flex_direction.is_row_axis(),
        container_style.writing_mode,
    ) {
        (true, WritingMode::HorizontalTb)
        | (
            false,
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr,
        ) => PhysicalAxis::Horizontal,
        (
            true,
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr,
        )
        | (false, WritingMode::HorizontalTb) => PhysicalAxis::Vertical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_baseline_selection_uses_order_modified_flex_line_order() {
        let line = FlexLineLayout {
            item_indices: vec![2, 5],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 2,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(0.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(0.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };

        assert_eq!(
            flex_line_baseline_item_index(
                &line,
                FlexDirection::ColumnReverse,
                FlexBaselineSet::First,
            ),
            Some(2),
        );
        assert_eq!(
            flex_line_baseline_item_index(
                &line,
                FlexDirection::ColumnReverse,
                FlexBaselineSet::Last,
            ),
            Some(5),
        );
    }

    #[test]
    fn wrap_reverse_reverses_the_singleton_baseline_edge() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Row;
        style.flex_wrap = FlexWrap::WrapReverse;

        assert_eq!(
            flex_baseline_alignment_side(&style, FlexBaselineSet::First),
            PhysicalSide::Bottom,
        );
        assert_eq!(
            flex_baseline_alignment_side(&style, FlexBaselineSet::Last),
            PhysicalSide::Top,
        );
        assert_eq!(
            flex_baseline_sharing_group_alignment_side(&style, FlexBaselineSet::First, 1),
            PhysicalSide::Bottom,
        );
        assert_eq!(
            flex_baseline_sharing_group_alignment_side(&style, FlexBaselineSet::Last, 1),
            PhysicalSide::Top,
        );
        assert_eq!(
            flex_baseline_sharing_group_alignment_side(&style, FlexBaselineSet::First, 2),
            PhysicalSide::Top,
        );
        assert_eq!(
            flex_baseline_sharing_group_alignment_side(&style, FlexBaselineSet::Last, 2),
            PhysicalSide::Bottom,
        );
    }

    #[test]
    fn baseline_resolution_keeps_sharing_fallback_and_auto_margin_distinct() {
        let mut measured_style = ComputedStyle::initial();
        measured_style.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Baseline);
        let mut orthogonal_style = measured_style.clone();
        orthogonal_style.writing_mode = WritingMode::VerticalRl;
        let mut auto_margin_style = measured_style.clone();
        auto_margin_style.box_values.margin.top = css::ComputedLengthPercentageOrAuto::Auto;
        let children = vec![
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: measured_style,
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: orthogonal_style,
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: auto_margin_style,
            },
        ];
        let mut measured = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        measured.baselines.vertical.first = Some(flex_vertical_baseline_from_points(5.0));
        let estimates = vec![
            measured,
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            ),
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            ),
        ];
        let container = ComputedStyle::initial();

        let alignments =
            resolve_flex_cross_alignments(&estimates, &children, &container, FlexDirection::Row);

        assert_eq!(
            alignments[0].mode,
            FlexCrossPlacementMode::Baseline {
                set: FlexBaselineSet::First,
                source: FlexBaselineSource::Measured,
                participation: FlexBaselineParticipation::Shares,
            }
        );
        assert_eq!(
            alignments[1].mode,
            FlexCrossPlacementMode::Baseline {
                set: FlexBaselineSet::First,
                source: FlexBaselineSource::Synthesized,
                participation: FlexBaselineParticipation::Fallback,
            }
        );
        assert_eq!(alignments[2].mode, FlexCrossPlacementMode::AutoCrossMargin);
    }

    #[test]
    fn cross_alignment_resolution_preserves_subject_sides_and_safe_centering() {
        let mut self_end_style = ComputedStyle::initial();
        self_end_style.align_self = css::SelfAlignment::safe(SelfAlignmentKeyword::SelfEnd);

        let mut center_style = ComputedStyle::initial();
        center_style.align_self = css::SelfAlignment::safe(SelfAlignmentKeyword::Center);

        let mut auto_margin_style = ComputedStyle::initial();
        auto_margin_style.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Center);
        auto_margin_style.box_values.margin.top = css::ComputedLengthPercentageOrAuto::Auto;

        let children = vec![
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: self_end_style,
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: center_style,
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: auto_margin_style,
            },
        ];
        let estimates = vec![
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            );
            3
        ];
        let container = ComputedStyle::initial();
        let alignments =
            resolve_flex_cross_alignments(&estimates, &children, &container, FlexDirection::Row);

        assert_eq!(
            alignments[0].mode,
            FlexCrossPlacementMode::Side(PhysicalSide::Bottom)
        );
        assert_eq!(alignments[0].safety, AlignmentSafety::Safe);
        assert_eq!(alignments[0].self_start, PhysicalSide::Top);
        assert_eq!(alignments[0].self_end, PhysicalSide::Bottom);
        assert_eq!(alignments[1].mode, FlexCrossPlacementMode::Center);
        assert_eq!(alignments[1].safety, AlignmentSafety::Safe);
        assert_eq!(alignments[2].mode, FlexCrossPlacementMode::AutoCrossMargin);
    }

    #[test]
    fn centered_cross_placement_uses_margin_box_geometry_and_safe_start() {
        let mut child_style = ComputedStyle::initial();
        child_style.margin.top = -5.0;
        child_style.margin.bottom = 15.0;
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(100.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let mut item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(10.0, 10.0),
        ));
        let centered = ResolvedFlexCrossAlignment {
            mode: FlexCrossPlacementMode::Center,
            safety: AlignmentSafety::Default,
            flex_cross_start: PhysicalSide::Top,
            flex_cross_end: PhysicalSide::Bottom,
            self_start: PhysicalSide::Top,
            self_end: PhysicalSide::Bottom,
        };

        align_item_cross_center(&mut item, &child_style, FlexDirection::Row, &line, centered);
        // The margin box is 20px tall, so its 40px cross start is centered
        // in the 100px line slot; the border box starts 5px before it.
        assert_eq!(item.y(), FlexPhysicalVerticalOffset::new(35.0));

        let mut overflowing_line = line.clone();
        overflowing_line.cross_end = FlexCrossOffset::new(10.0);
        let mut safe_item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(10.0, 10.0),
        ));
        align_item_cross_center(
            &mut safe_item,
            &child_style,
            FlexDirection::Row,
            &overflowing_line,
            ResolvedFlexCrossAlignment {
                safety: AlignmentSafety::Safe,
                ..centered
            },
        );
        assert_eq!(safe_item.y(), FlexPhysicalVerticalOffset::new(-5.0));
    }

    #[test]
    fn final_line_slot_redistributes_auto_cross_margins_for_a_column_item() {
        let mut child_style = ComputedStyle::initial();
        child_style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::Auto;
        child_style.box_values.margin.right = css::ComputedLengthPercentageOrAuto::Auto;
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(10.0),
            cross_end: FlexCrossOffset::new(110.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let mut item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(10.0, 0.0),
            ContainerSize::new(20.0, 10.0),
        ));

        place_item_with_final_auto_cross_margins(
            &mut item,
            &child_style,
            FlexDirection::Column,
            &line,
        );

        assert_eq!(item.x(), FlexPhysicalHorizontalOffset::new(50.0));
    }

    #[test]
    fn baseline_fallback_preserves_negative_cross_start_margins() {
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(10.0),
            cross_end: FlexCrossOffset::new(50.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let row_container = ComputedStyle::initial();

        let mut row_style = ComputedStyle::initial();
        row_style.margin.top = -4.0;
        let mut row_item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 10.0),
            ContainerSize::new(10.0, 10.0),
        ));
        apply_baseline_self_alignment_fallback_offset(
            &mut row_item,
            &row_style,
            &line,
            &row_container,
            FlexBaselineSet::First,
            FlexDirection::Row,
            None,
        );
        assert_eq!(row_item.y(), FlexPhysicalVerticalOffset::new(6.0));

        let mut column_style = ComputedStyle::initial();
        column_style.margin.left = -4.0;
        let mut column_container = ComputedStyle::initial();
        column_container.flex_direction = FlexDirection::Column;
        let mut column_item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(10.0, 0.0),
            ContainerSize::new(10.0, 10.0),
        ));
        apply_baseline_self_alignment_fallback_offset(
            &mut column_item,
            &column_style,
            &line,
            &column_container,
            FlexBaselineSet::First,
            FlexDirection::Column,
            None,
        );
        assert_eq!(column_item.x(), FlexPhysicalHorizontalOffset::new(6.0));
    }

    #[test]
    fn baseline_sharing_resolves_absolute_cross_starts_from_line_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0, 1],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 2,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(20.0),
            cross_start: FlexCrossOffset::new(50.0),
            cross_end: FlexCrossOffset::new(62.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let mut first = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        first.baselines.vertical.first = Some(flex_vertical_baseline_from_points(4.0));
        let mut second = first;
        second.baselines.vertical.first = Some(flex_vertical_baseline_from_points(8.0));
        let children = vec![
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            },
        ];
        let mut items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 40.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(10.0, 42.0),
                ContainerSize::new(10.0, 10.0),
            )),
        ];
        let container = ComputedStyle::initial();
        let participants = vec![
            ResolvedFlexBaselineParticipant {
                index: 0,
                source: FlexBaselineSource::Measured,
                participation: FlexBaselineParticipation::Shares,
            },
            ResolvedFlexBaselineParticipant {
                index: 1,
                source: FlexBaselineSource::Measured,
                participation: FlexBaselineParticipation::Shares,
            },
        ];

        align_baseline_sharing_group_to_line(
            &mut items,
            &line,
            &participants,
            &[first, second],
            &children,
            &container,
            FlexBaselineSet::First,
            FlexDirection::Row,
        );

        assert_eq!(items[0].y(), FlexPhysicalVerticalOffset::new(54.0));
        assert_eq!(items[1].y(), FlexPhysicalVerticalOffset::new(50.0));
    }

    #[test]
    fn taffy_cross_projection_precedes_line_reconciliation_for_vertical_rtl_columns() {
        for flex_wrap in [FlexWrap::Wrap, FlexWrap::WrapReverse] {
            let mut style = ComputedStyle::initial();
            style.writing_mode = WritingMode::VerticalLr;
            style.direction = Direction::Rtl;
            style.flex_direction = FlexDirection::Column;
            style.flex_wrap = flex_wrap;
            let axes = FlexAxes::for_style(&style);
            assert_eq!(
                axes.taffy_cross_axis_projection(),
                TaffyCrossAxisProjection::Reflect
            );

            let mut items = vec![
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 0.0),
                    ContainerSize::new(20.0, 30.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(20.0, 0.0),
                    ContainerSize::new(20.0, 30.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 30.0),
                    ContainerSize::new(20.0, 30.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(20.0, 30.0),
                    ContainerSize::new(20.0, 30.0),
                )),
            ];
            reproject_taffy_item_cross_axis_coordinates(&mut items, axes, FlexCrossSize::new(60.0));

            // The physical row main axis leaves Y as the cross coordinate.
            // Taffy's first top-origin line is therefore the CSS
            // bottom-origin line, independently of wrap-reverse. This is the
            // four-item wrapped geometry that later Flex line reconciliation
            // receives.
            assert_eq!(items[0].x(), FlexPhysicalHorizontalOffset::new(0.0));
            assert_eq!(items[1].x(), FlexPhysicalHorizontalOffset::new(20.0));
            assert_eq!(
                items.iter().map(|item| item.y()).collect::<Vec<_>>(),
                vec![
                    FlexPhysicalVerticalOffset::new(30.0),
                    FlexPhysicalVerticalOffset::new(30.0),
                    FlexPhysicalVerticalOffset::new(0.0),
                    FlexPhysicalVerticalOffset::new(0.0),
                ],
            );
            assert!(
                items
                    .iter()
                    .all(|item| item.height() == FlexPhysicalVerticalSize::new(30.0))
            );
        }
    }

    #[test]
    fn physical_right_justifies_a_sideways_column_on_its_horizontal_main_axis() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::SidewaysLr;
        style.direction = Direction::Ltr;
        style.flex_direction = FlexDirection::Column;
        let axes = FlexAxes::for_style(&style);
        assert!(axes.is_main_row_axis());
        assert_eq!(axes.main_start_side(), PhysicalSide::Left);

        let right =
            taffy_justify_content(JustifyContent::new(ContentAlignmentKeyword::Right), axes)
                .expect("justify-content always has a Taffy fallback");
        let left = taffy_justify_content(JustifyContent::new(ContentAlignmentKeyword::Left), axes)
            .expect("justify-content always has a Taffy fallback");

        assert_eq!(right.keyword, taffy_layout::AlignContentKeyword::FlexEnd);
        assert_eq!(left.keyword, taffy_layout::AlignContentKeyword::FlexStart);
    }
}
