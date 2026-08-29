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
pub(in crate::layout::flex) fn place_item_with_final_auto_cross_margins(
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
pub(in crate::layout::flex) fn align_item_cross_center(
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
