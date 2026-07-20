use super::*;

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
    if style.flex_wrap.reverses_cross_axis() {
        flex_cross_end_side(style)
    } else {
        flex_cross_start_side(style)
    }
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

pub(in crate::layout::flex) fn align_flex_line_group_cross_side(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
    side: PhysicalSide,
    container_cross_size: FlexCrossSize,
) {
    let Some((group_start, group_end)) = flex_line_group_cross_bounds(lines) else {
        return;
    };
    align_flex_line_group_cross_side_from_bounds(
        lines,
        items,
        physical_direction,
        side,
        container_cross_size,
        group_start,
        group_end,
    );
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
    let side = flex_baseline_alignment_side(container_style, baseline_set);
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
        items[index].set_cross_start(
            FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction)),
            FlexCrossOffset::new(0.0) + (target_baseline - baseline),
        );
    }
}
pub(in crate::layout::flex) fn shift_flex_line_cross_axis(
    line: &mut FlexLineLayout,
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
    delta: FlexCrossLength,
) {
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
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
    let (start, end) = item.outer_main_bounds(
        FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction)),
        style,
    );
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
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    let cross_start = match (
        physical_direction.is_row_axis(),
        side.is_start_edge(),
        side.is_end_edge(),
    ) {
        (true, true, false) => Some(line.cross_start + FlexCrossLength::new(style.margin.top)),
        (true, false, true) => {
            Some(line.cross_end - FlexCrossLength::new(style.margin.bottom) - item.cross_size(axes))
        }
        (false, true, false) => Some(line.cross_start + FlexCrossLength::new(style.margin.left)),
        (false, false, true) => {
            Some(line.cross_end - FlexCrossLength::new(style.margin.right) - item.cross_size(axes))
        }
        _ => None,
    };
    if let Some(cross_start) = cross_start {
        item.set_cross_start(axes, cross_start);
    }
}

pub(in crate::layout::flex) fn taffy_justify_content(
    justify_content: JustifyContent,
    flex_direction: FlexDirection,
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
        ContentAlignmentKeyword::Left => Some(
            if matches!(
                flex_direction,
                FlexDirection::RowReverse | FlexDirection::ColumnReverse
            ) {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                    safety,
                }
            } else {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexStart,
                    safety,
                }
            },
        ),
        ContentAlignmentKeyword::Right => {
            Some(if matches!(flex_direction, FlexDirection::Column) {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexStart,
                    safety,
                }
            } else if matches!(flex_direction, FlexDirection::ColumnReverse) {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                    safety,
                }
            } else if matches!(flex_direction, FlexDirection::RowReverse) {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexStart,
                    safety,
                }
            } else {
                taffy_layout::JustifyContent {
                    keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                    safety,
                }
            })
        }
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

/// Maps a CSS flex container's physical axes to Taffy's writing direction.
///
/// Taffy has physical row/column flex directions plus an LTR/RTL switch. For
/// horizontal writing modes that switch is CSS `direction`. For vertical
/// writing modes whose CSS row axis becomes Taffy's physical column axis, the
/// switch must instead represent the horizontal cross-axis start side, so
/// `vertical-rl` row flex lines start at the physical right edge. Vertical
/// writing modes whose CSS column axis becomes Taffy's physical row axis have
/// their right-to-left or left-to-right block flow already encoded in
/// `physical_flex_direction`, so they use LTR here.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>,
/// <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property>, and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(in crate::layout::flex) fn taffy_flex_layout_direction(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> ::taffy::Direction {
    if style.writing_mode == WritingMode::HorizontalTb {
        return taffy_direction(style.direction);
    }
    if physical_direction.is_column_axis() && flex_cross_start_side(style) == PhysicalSide::Right {
        ::taffy::Direction::Rtl
    } else {
        ::taffy::Direction::Ltr
    }
}

/// Mirror final flex cross-axis geometry when CSS's cross-start is the
/// physical bottom edge.
///
/// Taffy's `Direction` can express a horizontal start side, but it has no
/// top-to-bottom/bottom-to-top equivalent. A vertical-writing column flex
/// container therefore needs a final coordinate conversion when its inline
/// axis is RTL. Taffy still forms and sizes the lines; this maps its top-origin
/// cross coordinates to CSS's bottom-origin inline axis for both items and
/// line metadata.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>
/// <https://www.w3.org/TR/css-writing-modes-4/#inline-flow>
pub(in crate::layout::flex) fn mirror_vertical_cross_axis_for_rtl_inline_flow(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: FlexCrossSize,
) {
    if !matches!(style.writing_mode, WritingMode::VerticalRl | WritingMode::VerticalLr | WritingMode::SidewaysRl | WritingMode::SidewaysLr)
        || !physical_direction.is_row_axis()
        || flex_cross_start_side(style) != PhysicalSide::Bottom
        // Baseline line packing has its own physical-edge conversion. Leave
        // that specialized path intact until it can mirror both baseline
        // sharing groups and their fallback alignment together.
        || matches!(
            style.align_items.keyword,
            SelfAlignmentKeyword::Baseline | SelfAlignmentKeyword::LastBaseline
        )
    {
        return;
    }
    let cross_origin = FlexCrossOffset::new(0.0);
    let container_cross_end = cross_origin + container_cross_size;
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    for item in items {
        item.set_cross_start(
            axes,
            container_cross_end - (item.cross_start(axes) - cross_origin) - item.cross_size(axes),
        );
    }
    for line in lines {
        let cross_start = line.cross_start;
        let cross_end = line.cross_end;
        line.cross_start = container_cross_end - (cross_end - cross_origin);
        line.cross_end = container_cross_end - (cross_start - cross_origin);
        line.first_baseline = line
            .first_baseline
            .map(|baseline| container_cross_end - (baseline - cross_origin));
        line.last_baseline = line
            .last_baseline
            .map(|baseline| container_cross_end - (baseline - cross_origin));
    }
}

/// Maps CSS `direction` to Taffy's physical LTR/RTL switch.
pub(in crate::layout::flex) fn taffy_direction(direction: Direction) -> ::taffy::Direction {
    match direction {
        Direction::Ltr => ::taffy::Direction::Ltr,
        Direction::Rtl => ::taffy::Direction::Rtl,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexBaselineSet {
    First,
    Last,
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
    AutoCrossMargin,
}

/// An item resolved for one first- or last-baseline set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) struct ResolvedFlexBaselineParticipant {
    pub(in crate::layout::flex) index: usize,
    pub(in crate::layout::flex) source: FlexBaselineSource,
    pub(in crate::layout::flex) participation: FlexBaselineParticipation,
}

/// The complete first- or last-baseline decision for a single flex line.
///
/// The physical line axis is derived from authored `flex-direction` and the
/// container writing mode.  It is deliberately retained separately from the
/// physical Taffy direction, which is only an adapter for geometry:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout::flex) struct FlexBaselineResolution {
    pub(in crate::layout::flex) baseline_set: FlexBaselineSet,
    pub(in crate::layout::flex) line_axis: PhysicalAxis,
    pub(in crate::layout::flex) participants: Vec<ResolvedFlexBaselineParticipant>,
}

/// Resolve and apply every flex baseline self-alignment exactly once.
///
/// Taffy's measure callback has no baseline channel, so its output is used for
/// flex sizing and line construction only.  Quire then resolves CSS Flexbox
/// baseline-sharing eligibility, fallback, and measured/synthesized baselines
/// together from final item geometry.  Keeping these phases together avoids
/// row, column, and fallback passes disagreeing about the same item:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line> and
/// <https://drafts.csswg.org/css-align-3/#baseline-align-self>.
pub(in crate::layout::flex) fn resolve_flex_baseline_self_alignment(
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    for line in lines {
        for baseline_set in [FlexBaselineSet::First, FlexBaselineSet::Last] {
            let resolution = resolve_flex_line_baseline_set(
                line,
                estimates,
                children,
                container_style,
                baseline_set,
                physical_direction,
            );
            if resolution.participants.is_empty() {
                continue;
            }

            let mut sharing_participants = resolution
                .participants
                .iter()
                .filter(|participant| {
                    participant.participation == FlexBaselineParticipation::Shares
                })
                .copied()
                .collect::<Vec<_>>();
            let mut fallback_indices = resolution
                .participants
                .iter()
                .filter(|participant| {
                    participant.participation == FlexBaselineParticipation::Fallback
                })
                .map(|participant| participant.index)
                .collect::<Vec<_>>();

            // CSS Align requires at least two compatible participants for a
            // baseline-sharing group.  A sole otherwise-compatible item uses
            // the same safe self-alignment fallback as an incompatible item.
            if sharing_participants.len() <= 1 {
                fallback_indices.extend(
                    sharing_participants
                        .drain(..)
                        .map(|participant| participant.index),
                );
            } else {
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
}

pub(in crate::layout::flex) fn resolve_flex_line_baseline_set(
    line: &FlexLineLayout,
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> FlexBaselineResolution {
    let line_axis = flex_baseline_line_axis(container_style);
    let participants = line
        .item_indices
        .iter()
        .copied()
        .filter(|&index| {
            flex_baseline_set(&children[index].style, container_style) == Some(baseline_set)
        })
        .map(|index| {
            let child_style = &children[index].style;
            let participation = if flex_item_has_auto_cross_margin(child_style, physical_direction)
            {
                FlexBaselineParticipation::AutoCrossMargin
            } else if flex_item_baseline_axis_is_parallel_to_main_axis(
                child_style,
                physical_direction,
            ) {
                FlexBaselineParticipation::Shares
            } else {
                FlexBaselineParticipation::Fallback
            };
            ResolvedFlexBaselineParticipant {
                index,
                source: flex_item_baseline_source(&estimates[index], baseline_set, line_axis),
                participation,
            }
        })
        .collect();
    FlexBaselineResolution {
        baseline_set,
        line_axis,
        participants,
    }
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
    // `wrap-reverse` reverses flex-line placement, but it does not reverse
    // the baseline set inside each line. First-baseline sharing is therefore
    // anchored to the container's ordinary cross-start side; the line's
    // already-final physical position supplies the wrap-reversed coordinate.
    // Reversing this side pulled a padded first-baseline group toward the
    // opposite edge of its packed line and made its exported baseline differ
    // from the equivalent nested single-line flex construction.
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
    let size = item.cross_size(FlexAxes::from_physical_direction(
        PhysicalFlexDirection::new(physical_direction),
    ));
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
            flex_baseline_line_axis(container_style),
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
            FlexPhysicalBaselineOffset::Vertical(FlexVerticalBaselineOffset::new(0.0))
        }
        PhysicalSide::Right => FlexPhysicalBaselineOffset::Horizontal(
            flex_horizontal_baseline_from_physical_width(item.width()),
        ),
        PhysicalSide::Bottom => FlexPhysicalBaselineOffset::Vertical(
            flex_vertical_baseline_from_physical_height(item.height()),
        ),
        PhysicalSide::Left => {
            FlexPhysicalBaselineOffset::Horizontal(FlexHorizontalBaselineOffset::new(0.0))
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
    fn baseline_resolution_keeps_sharing_fallback_and_auto_margin_distinct() {
        let line = FlexLineLayout {
            item_indices: vec![0, 1, 2],
            source_start: 0,
            source_end: 3,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(30.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(20.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
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
        measured.baselines.vertical.first = Some(FlexVerticalBaselineOffset::new(5.0));
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

        let resolution = resolve_flex_line_baseline_set(
            &line,
            &estimates,
            &children,
            &container,
            FlexBaselineSet::First,
            FlexDirection::Row,
        );

        assert_eq!(resolution.line_axis, PhysicalAxis::Horizontal);
        assert_eq!(
            resolution.participants,
            vec![
                ResolvedFlexBaselineParticipant {
                    index: 0,
                    source: FlexBaselineSource::Measured,
                    participation: FlexBaselineParticipation::Shares,
                },
                ResolvedFlexBaselineParticipant {
                    index: 1,
                    source: FlexBaselineSource::Synthesized,
                    participation: FlexBaselineParticipation::Fallback,
                },
                ResolvedFlexBaselineParticipant {
                    index: 2,
                    source: FlexBaselineSource::Synthesized,
                    participation: FlexBaselineParticipation::AutoCrossMargin,
                },
            ]
        );
    }
}
