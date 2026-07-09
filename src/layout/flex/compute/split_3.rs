use super::*;

/// Applies `align-content: stretch` fallback when stretched flex lines overflow.
///
/// CSS Align defines `stretch` as falling back to `flex-start`, not
/// `safe flex-start`, so an overflowing wrapped line remains packed against
/// the flex cross-start side. Taffy 0.11 applies the older generic
/// distribution fallback, so Quire corrects only the overflow case after
/// recovering flex line metadata:
/// <https://drafts.csswg.org/css-align/#valdef-align-content-stretch> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
pub(in crate::layout::flex) fn apply_stretch_align_content_overflow_fallback_offsets(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
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
    let group_size = (group_end - group_start).max(0.0);
    if group_size <= container_cross_size.max(0.0) + 0.01 {
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
) -> Option<(f32, f32)> {
    lines
        .iter()
        .filter_map(|line| {
            if line.item_indices.is_empty() {
                return Some((line.cross_start.points(), line.cross_end.points()));
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
                .fold(None::<(f32, f32)>, |bounds, item_bounds| {
                    Some(match bounds {
                        Some((start, end)) => (start.min(item_bounds.0), end.max(item_bounds.1)),
                        None => item_bounds,
                    })
                })
                .map(|(start, end)| {
                    (
                        start.min(line.cross_start.points()),
                        end.max(
                            line.cross_start.points() + line.largest_collapsed_strut().points(),
                        ),
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
    container_cross_size: f32,
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
    container_cross_size: f32,
    group_start: f32,
    group_end: f32,
) {
    let delta = if side.is_start_edge() {
        -group_start
    } else if side.is_end_edge() {
        container_cross_size.max(0.0) - group_end
    } else {
        0.0
    };
    if delta.abs() <= 0.01 {
        return;
    }
    for line in lines {
        shift_flex_line_cross_axis(line, items, physical_direction, delta);
    }
}

/// Applies baseline self-alignment fallback when a sharing group is unavailable.
///
/// CSS Box Alignment falls back from first-baseline self-alignment to
/// `safe self-start` and from last-baseline self-alignment to `safe self-end`
/// when no compatible baseline-sharing group can be formed. Row flex lines can
/// share compatible baselines, so this fallback only handles single-participant
/// groups there. Flexbox then aligns that fallback in the flex line's cross
/// axis:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-self> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-align>.
pub(in crate::layout::flex) fn apply_baseline_self_alignment_fallback_offsets(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    if !container_style.flex_direction.is_row_axis() {
        return;
    }

    for line in lines {
        for baseline_set in [FlexBaselineSet::First, FlexBaselineSet::Last] {
            let baseline_indices = line
                .item_indices
                .iter()
                .cloned()
                .filter(|&candidate| {
                    flex_baseline_set(&children[candidate].style, container_style)
                        == Some(baseline_set)
                })
                .collect::<Vec<_>>();
            if baseline_indices.is_empty() {
                continue;
            }

            let compatible_indices = baseline_indices
                .iter()
                .cloned()
                .filter(|&candidate| {
                    flex_item_baseline_axis_is_parallel_to_main_axis(
                        &children[candidate].style,
                        physical_direction,
                    ) && !flex_item_has_auto_cross_margin(
                        &children[candidate].style,
                        physical_direction,
                    )
                })
                .collect::<Vec<_>>();
            let fallback_cross_bounds = baseline_fallback_cross_bounds(
                &baseline_indices,
                items,
                children,
                line,
                container_style,
                physical_direction,
            );
            if compatible_indices.len() == 1 {
                apply_baseline_self_alignment_fallback_offset(
                    &mut items[compatible_indices[0]],
                    &children[compatible_indices[0]].style,
                    line,
                    container_style,
                    baseline_set,
                    physical_direction,
                    fallback_cross_bounds,
                );
            }

            for index in baseline_indices {
                if compatible_indices.contains(&index) {
                    continue;
                }
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
        inline_start_side(child_style.writing_mode, child_style.direction).axis();
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
    cross_bounds: Option<(f32, f32)>,
) {
    if flex_item_has_auto_cross_margin(child_style, physical_direction) {
        return;
    }

    let (cross_start, cross_end) =
        cross_bounds.unwrap_or((line.cross_start.points(), line.cross_end.points()));
    let cross_size = (cross_end - cross_start).max(0.0);
    let subject_side = match baseline_set {
        FlexBaselineSet::First => child_self_start_side(child_style, container_style),
        FlexBaselineSet::Last => child_self_end_side(child_style, container_style),
    };
    let outer_size = item_outer_cross_size(item, child_style, physical_direction);
    let target_side = if cross_size - outer_size < 0.0 {
        flex_cross_start_side(container_style)
    } else {
        subject_side
    };
    let mut fallback_line = line.clone();
    fallback_line.cross_start = FlexCrossOffset::new(cross_start);
    fallback_line.cross_end = FlexCrossOffset::new(cross_end);
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
) -> Option<(f32, f32)> {
    if !container_style.flex_wrap.reverses_cross_axis() || !physical_direction.is_column_axis() {
        return None;
    }
    let (group_start, group_end) = indices
        .iter()
        .cloned()
        .map(|index| {
            item_outer_cross_bounds(&items[index], &children[index].style, physical_direction)
        })
        .fold(None::<(f32, f32)>, |bounds, item_bounds| {
            Some(match bounds {
                Some((start, end)) => (start.min(item_bounds.0), end.max(item_bounds.1)),
                None => item_bounds,
            })
        })?;
    let group_size = (group_end - group_start).max(0.0);
    Some((
        (line.cross_end.points() - group_size).max(line.cross_start.points()),
        line.cross_end.points(),
    ))
}

pub(in crate::layout::flex) struct FlexBaselineSharingContext<'a, 'dom> {
    pub(in crate::layout::flex) estimates: &'a [FlexItemEstimate],
    pub(in crate::layout::flex) children: &'a [StyledChild<'dom>],
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) baseline_set: FlexBaselineSet,
    pub(in crate::layout::flex) physical_direction: FlexDirection,
}

/// Align a flex line baseline-sharing group to the line's cross-axis edge.
///
/// CSS Flexbox baseline self-alignment places the participant with the largest
/// distance between its baseline and the relevant cross-start/end margin edge
/// flush with that line edge, then aligns the other participants to the same
/// baseline:
/// <https://www.w3.org/TR/css-flexbox-1/#valdef-align-items-baseline> and
/// <https://drafts.csswg.org/css-align-3/#baseline-align-self>.
pub(in crate::layout::flex) fn align_baseline_sharing_group_to_line(
    items: &mut [FlexItemLayout],
    line: &FlexLineLayout,
    line_indices: &[usize],
    context: FlexBaselineSharingContext<'_, '_>,
) {
    let side = flex_baseline_alignment_side(context.container_style, context.baseline_set);
    let target_distance = line_indices
        .iter()
        .map(|&index| {
            item_baseline_distance_to_cross_side(
                &items[index],
                &context.estimates[index],
                &context.children[index].style,
                context.container_style,
                context.baseline_set,
                context.physical_direction,
                side,
            )
        })
        .fold(0.0f32, f32::max);
    let target_baseline = if side.is_start_edge() {
        line.cross_start.points() + target_distance
    } else {
        line.cross_end.points() - target_distance
    };

    for &index in line_indices {
        let baseline = measured_item_cross_axis_border_box_baseline(
            &items[index],
            &context.estimates[index],
            &context.children[index].style,
            context.container_style,
            context.baseline_set,
            context.physical_direction,
        );
        if context.physical_direction.is_row_axis() {
            items[index].set_y(target_baseline - baseline);
        } else {
            items[index].set_x(target_baseline - baseline);
        }
    }
}

/// Applies column-axis flex baseline self-alignment.
///
/// CSS Flexbox aligns baseline-sharing flex items by placing the item with the
/// largest distance from its baseline to the relevant cross-start/end margin
/// edge flush with that cross edge; missing baselines are synthesized from the
/// border box:
/// <https://www.w3.org/TR/css-flexbox-1/#valdef-align-items-baseline> and
/// <https://drafts.csswg.org/css-align-3/#synthesize-baseline>.
pub(in crate::layout::flex) fn apply_column_baseline_self_alignment_offsets(
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    if container_style.flex_direction.is_row_axis() {
        return;
    }

    for line in lines {
        for baseline_set in [FlexBaselineSet::First, FlexBaselineSet::Last] {
            let baseline_indices = line
                .item_indices
                .iter()
                .cloned()
                .filter(|&candidate| {
                    flex_baseline_set(&children[candidate].style, container_style)
                        == Some(baseline_set)
                })
                .collect::<Vec<_>>();
            if baseline_indices.is_empty() {
                continue;
            }

            let compatible_indices = baseline_indices
                .iter()
                .cloned()
                .filter(|&candidate| {
                    flex_item_baseline_axis_is_parallel_to_main_axis(
                        &children[candidate].style,
                        physical_direction,
                    ) && !flex_item_has_auto_cross_margin(
                        &children[candidate].style,
                        physical_direction,
                    )
                })
                .collect::<Vec<_>>();
            let fallback_cross_bounds = baseline_fallback_cross_bounds(
                &baseline_indices,
                items,
                children,
                line,
                container_style,
                physical_direction,
            );
            if compatible_indices.len() == 1 {
                apply_baseline_self_alignment_fallback_offset(
                    &mut items[compatible_indices[0]],
                    &children[compatible_indices[0]].style,
                    line,
                    container_style,
                    baseline_set,
                    physical_direction,
                    fallback_cross_bounds,
                );
            } else if compatible_indices.len() > 1 {
                align_baseline_sharing_group_to_line(
                    items,
                    line,
                    &compatible_indices,
                    FlexBaselineSharingContext {
                        estimates,
                        children,
                        container_style,
                        baseline_set,
                        physical_direction,
                    },
                );
            }

            for index in baseline_indices {
                if compatible_indices.contains(&index) {
                    continue;
                }
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
pub(in crate::layout::flex) fn shift_flex_line_cross_axis(
    line: &mut FlexLineLayout,
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
    delta: f32,
) {
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    line.cross_start = FlexCrossOffset::new(line.cross_start.points() + delta);
    line.cross_end = FlexCrossOffset::new(line.cross_end.points() + delta);
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
) -> (f32, f32) {
    item.outer_main_bounds(
        FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction)),
        style,
    )
}

pub(in crate::layout::flex) fn flex_items_main_extent(
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) -> Option<(f32, f32)> {
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
) -> Option<(f32, f32)> {
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
) -> Option<f32> {
    if !container_style.flex_direction.is_row_axis() {
        return None;
    }

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
        .reduce(f32::max)
}

pub(in crate::layout::flex) fn flex_line_content_baseline(
    line: &FlexLineLayout,
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> Option<f32> {
    if !container_style.flex_direction.is_row_axis() {
        return None;
    }

    let line_baseline = match baseline_set {
        FlexBaselineSet::First => line.first_baseline,
        FlexBaselineSet::Last => line.last_baseline,
    };
    if line_baseline.is_some() {
        return line_baseline.map(FlexCrossOffset::points);
    }

    flex_line_baseline_item_index(line, container_style, baseline_set).map(|index| {
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
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> Option<usize> {
    match (baseline_set, container_style.flex_direction) {
        (FlexBaselineSet::First, FlexDirection::Row | FlexDirection::RowReverse) => {
            line.item_indices.iter().cloned().min()
        }
        (FlexBaselineSet::First, FlexDirection::Column) => line.item_indices.iter().cloned().min(),
        (FlexBaselineSet::First, FlexDirection::ColumnReverse) => {
            line.item_indices.iter().cloned().max()
        }
        (FlexBaselineSet::Last, FlexDirection::Row | FlexDirection::RowReverse) => {
            line.item_indices.iter().cloned().max()
        }
        (FlexBaselineSet::Last, FlexDirection::Column) => line.item_indices.iter().cloned().max(),
        (FlexBaselineSet::Last, FlexDirection::ColumnReverse) => {
            line.item_indices.iter().cloned().min()
        }
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
    match (
        physical_direction.is_row_axis(),
        side.is_start_edge(),
        side.is_end_edge(),
    ) {
        (true, true, false) => item.set_y(line.cross_start.points() + style.margin.top),
        (true, false, true) => {
            item.set_y(line.cross_end.points() - style.margin.bottom - item.height())
        }
        (false, true, false) => item.set_x(line.cross_start.points() + style.margin.left),
        (false, false, true) => {
            item.set_x(line.cross_end.points() - style.margin.right - item.width())
        }
        _ => {}
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
    container_cross_size: f32,
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
    let container_cross_size = container_cross_size.max(0.0);
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    for item in items {
        item.set_cross_start(
            axes,
            container_cross_size - item.cross_start(axes) - item.cross_size(axes),
        );
    }
    for line in lines {
        let cross_start = line.cross_start.points();
        let cross_end = line.cross_end.points();
        line.cross_start = FlexCrossOffset::new(container_cross_size - cross_end);
        line.cross_end = FlexCrossOffset::new(container_cross_size - cross_start);
        line.first_baseline = line
            .first_baseline
            .map(|baseline| FlexCrossOffset::new(container_cross_size - baseline.points()));
        line.last_baseline = line
            .last_baseline
            .map(|baseline| FlexCrossOffset::new(container_cross_size - baseline.points()));
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

/// Apply row flex baseline self-alignment using renderer text baselines.
///
/// Taffy 0.11's public measure callback returns only leaf sizes, so baseline
/// aligned flex leaves are initially positioned with the CSS fallback baseline
/// synthesized from the item border box. CSS Flexbox aligns participating row
/// flex items by their first or last baseline set; after layout we keep each
/// flex line's cross-axis slot and reapply measured baselines from the same
/// intrinsic estimates used for flex sizing:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line> and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn replace_synthesized_baseline_offsets(
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    if !container_style.flex_direction.is_row_axis() {
        return;
    }

    for line in lines {
        for baseline_set in [FlexBaselineSet::First, FlexBaselineSet::Last] {
            let line_indices = line
                .item_indices
                .iter()
                .cloned()
                .filter(|&candidate| {
                    flex_baseline_set(&children[candidate].style, container_style)
                        == Some(baseline_set)
                        && !flex_item_has_auto_cross_margin(
                            &children[candidate].style,
                            physical_direction,
                        )
                })
                .collect::<Vec<_>>();
            if line_indices.len() <= 1 {
                continue;
            }

            align_baseline_sharing_group_to_line(
                items,
                line,
                &line_indices,
                FlexBaselineSharingContext {
                    estimates,
                    children,
                    container_style,
                    baseline_set,
                    physical_direction,
                },
            );
        }
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
    let first_baseline_side = if container_style.flex_wrap.reverses_cross_axis() {
        flex_cross_end_side(container_style)
    } else {
        flex_cross_start_side(container_style)
    };
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

pub(in crate::layout::flex) fn item_baseline_distance_to_cross_side(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
    side: PhysicalSide,
) -> f32 {
    debug_assert_eq!(
        side.axis(),
        if physical_direction.is_row_axis() {
            PhysicalAxis::Vertical
        } else {
            PhysicalAxis::Horizontal
        }
    );
    let baseline = measured_item_cross_axis_border_box_baseline(
        item,
        estimate,
        child_style,
        container_style,
        baseline_set,
        physical_direction,
    );
    let size = if physical_direction.is_row_axis() {
        item.height()
    } else {
        item.width()
    };
    match side {
        PhysicalSide::Top => child_style.margin.top + baseline,
        PhysicalSide::Right => child_style.margin.right + size - baseline,
        PhysicalSide::Bottom => child_style.margin.bottom + size - baseline,
        PhysicalSide::Left => child_style.margin.left + baseline,
    }
    .max(0.0)
}

pub(in crate::layout::flex) fn measured_item_border_box_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> f32 {
    let measured = match baseline_set {
        FlexBaselineSet::First => estimate.first_baseline,
        FlexBaselineSet::Last => estimate.last_baseline,
    };
    measured.unwrap_or_else(|| {
        synthesized_item_border_box_baseline(
            item,
            child_style,
            container_style,
            baseline_set,
            flex_baseline_line_axis(container_style),
        )
    })
}

pub(in crate::layout::flex) fn measured_item_horizontal_border_box_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> f32 {
    let measured = match baseline_set {
        FlexBaselineSet::First => estimate.first_horizontal_baseline,
        FlexBaselineSet::Last => estimate.last_horizontal_baseline,
    };
    measured.unwrap_or_else(|| {
        synthesized_item_border_box_baseline(
            item,
            child_style,
            container_style,
            baseline_set,
            flex_baseline_line_axis(container_style),
        )
    })
}

pub(in crate::layout::flex) fn measured_item_cross_axis_border_box_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> f32 {
    if physical_direction.is_row_axis() {
        measured_item_border_box_baseline(
            item,
            estimate,
            child_style,
            container_style,
            baseline_set,
        )
    } else {
        measured_item_horizontal_border_box_baseline(
            item,
            estimate,
            child_style,
            container_style,
            baseline_set,
        )
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
) -> f32 {
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
        PhysicalSide::Top => 0.0,
        PhysicalSide::Right => item.width(),
        PhysicalSide::Bottom => item.height(),
        PhysicalSide::Left => 0.0,
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
) -> f32 {
    match baseline_line_axis {
        PhysicalAxis::Horizontal => item.height() / 2.0,
        PhysicalAxis::Vertical => item.width() / 2.0,
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
