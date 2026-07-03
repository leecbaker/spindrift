use super::*;

/// Corrects Taffy's placeholder placement for `self-start` and `self-end`.
///
/// CSS Box Alignment defines these keywords from the alignment subject's own
/// writing mode, while flex cross-axis placement aligns the subject within the
/// current flex line. Taffy exposes only the container-axis keyword, so this
/// pass keeps Taffy responsible for sizing and line construction and adjusts
/// only the final cross-axis offset for values that need subject-axis mapping:
/// <https://www.w3.org/TR/css-align-3/#self-position> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-align>.
pub(in crate::layout::flex) fn apply_subject_axis_self_alignment_offsets(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    if !children.iter().any(|child| {
        matches!(
            effective_align_self(&child.style, container_style).keyword,
            SelfAlignmentKeyword::SelfStart | SelfAlignmentKeyword::SelfEnd
        )
    }) {
        return;
    }

    for line in lines {
        for &index in &line.item_indices {
            let child_style = &children[index].style;
            let alignment = effective_align_self(child_style, container_style);
            let subject_side = match alignment.keyword {
                SelfAlignmentKeyword::SelfStart => {
                    child_self_start_side(child_style, container_style)
                }
                SelfAlignmentKeyword::SelfEnd => child_self_end_side(child_style, container_style),
                _ => continue,
            };
            if flex_item_has_auto_cross_margin(child_style, physical_direction) {
                continue;
            }
            let outer_size = item_outer_cross_size(&items[index], child_style, physical_direction);
            let target_side = if alignment.safety == AlignmentSafety::Safe
                && line.cross_size() - outer_size < 0.0
            {
                flex_cross_start_side(container_style)
            } else {
                subject_side
            };
            align_item_cross_side(
                &mut items[index],
                child_style,
                physical_direction,
                line,
                target_side,
            );
        }
    }
}

pub(in crate::layout::flex) fn flex_lines_from_items(
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    estimates: &[FlexItemEstimate],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) -> Vec<FlexLineLayout> {
    if container_style.flex_wrap == FlexWrap::NoWrap {
        let (main_start, main_end) =
            flex_items_main_extent(items, children, physical_direction).unwrap_or((0.0, 0.0));
        let item_indices = (0..items.len()).collect::<Vec<_>>();
        return vec![FlexLineLayout {
            item_indices: item_indices.clone(),
            source_start: 0,
            source_end: items.len(),
            main_start,
            main_end,
            cross_start: 0.0,
            cross_end: container_cross_size.max(0.0),
            first_baseline: flex_line_baseline(
                &item_indices,
                items,
                estimates,
                children,
                container_style,
                FlexBaselineSet::First,
                physical_direction,
            ),
            last_baseline: flex_line_baseline(
                &item_indices,
                items,
                estimates,
                children,
                container_style,
                FlexBaselineSet::Last,
                physical_direction,
            ),
            collapsed_struts: Vec::new(),
        }];
    }

    let mut lines: Vec<FlexLineLayout> = Vec::new();
    for index in 0..items.len() {
        let (cross_start, cross_end) =
            item_outer_cross_bounds(&items[index], &children[index].style, physical_direction);
        let (main_start, main_end) =
            item_outer_main_bounds(&items[index], &children[index].style, physical_direction);
        if let Some(line) = lines
            .iter_mut()
            .find(|line| cross_start < line.cross_end - 0.01 && cross_end > line.cross_start + 0.01)
        {
            line.cross_start = line.cross_start.min(cross_start);
            line.cross_end = line.cross_end.max(cross_end);
            line.main_start = line.main_start.min(main_start);
            line.main_end = line.main_end.max(main_end);
            line.source_start = line.source_start.min(index);
            line.source_end = line.source_end.max(index + 1);
            line.item_indices.push(index);
        } else {
            lines.push(FlexLineLayout {
                item_indices: vec![index],
                source_start: index,
                source_end: index + 1,
                main_start,
                main_end,
                cross_start,
                cross_end,
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            });
        }
    }
    refresh_flex_line_metadata(
        &mut lines,
        items,
        estimates,
        children,
        container_style,
        physical_direction,
        container_cross_size,
    );
    lines
}

pub(in crate::layout::flex) fn refresh_flex_line_cross_bounds(
    lines: &mut [FlexLineLayout],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) {
    let stretch_wrapped_lines = !lines.is_empty()
        && container_style.flex_wrap != FlexWrap::NoWrap
        && matches!(
            container_style.align_content.keyword,
            ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch
        );
    for line in &mut *lines {
        if container_style.flex_wrap == FlexWrap::NoWrap {
            line.cross_start = 0.0;
            line.cross_end = container_cross_size.max(0.0);
            if let Some(main_extent) = flex_items_main_extent(items, children, physical_direction) {
                line.main_start = main_extent.0;
                line.main_end = main_extent.1;
            }
            line.cross_end = line
                .cross_end
                .max(line.cross_start + line.largest_collapsed_strut());
            continue;
        }
        if line.item_indices.is_empty() {
            line.cross_end = line
                .cross_end
                .max(line.cross_start + line.largest_collapsed_strut());
            continue;
        }
        let mut cross_start = f32::INFINITY;
        let mut cross_end = f32::NEG_INFINITY;
        let mut main_start = f32::INFINITY;
        let mut main_end = f32::NEG_INFINITY;
        for &index in &line.item_indices {
            let (item_cross_start, item_cross_end) =
                item_outer_cross_bounds(&items[index], &children[index].style, physical_direction);
            let (item_main_start, item_main_end) =
                item_outer_main_bounds(&items[index], &children[index].style, physical_direction);
            cross_start = cross_start.min(item_cross_start);
            cross_end = cross_end.max(item_cross_end);
            main_start = main_start.min(item_main_start);
            main_end = main_end.max(item_main_end);
        }
        line.cross_start = cross_start;
        line.cross_end = cross_end.max(cross_start + line.largest_collapsed_strut());
        line.main_start = main_start;
        line.main_end = main_end;
    }
    if stretch_wrapped_lines {
        preserve_stretched_flex_line_cross_bounds(lines, container_cross_size);
    }
}

/// Preserve stretched wrapped flex line boxes after item-bound refresh.
///
/// Taffy owns flex line construction and initial packing, but Quire refreshes
/// line metadata from item bounds for baseline and fragmentation passes. CSS
/// Flexbox stretches flex lines, not just their items, so post-layout
/// alignment corrections need the full line cross-size:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-line-stretch> and
/// <https://www.w3.org/TR/css-align-3/#align-content-property>.
pub(in crate::layout::flex) fn preserve_stretched_flex_line_cross_bounds(
    lines: &mut [FlexLineLayout],
    container_cross_size: f32,
) {
    let mut line_order = (0..lines.len()).collect::<Vec<_>>();
    line_order.sort_by(|&a, &b| {
        lines[a]
            .cross_start
            .partial_cmp(&lines[b].cross_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if line_order.len() == 1 {
        let line = &mut lines[line_order[0]];
        line.cross_start = 0.0;
        line.cross_end = container_cross_size
            .max(0.0)
            .max(line.cross_end)
            .max(line.cross_start + line.largest_collapsed_strut());
        return;
    }

    let container_cross_size = container_cross_size.max(0.0);
    for position in 0..line_order.len() {
        let line_index = line_order[position];
        if position == 0 && lines[line_index].cross_start > 0.0 {
            lines[line_index].cross_start = 0.0;
        }
        let next_cross_start = line_order
            .get(position + 1)
            .map(|&next_index| lines[next_index].cross_start)
            .unwrap_or(container_cross_size);
        lines[line_index].cross_end = lines[line_index]
            .cross_end
            .max(next_cross_start)
            .max(lines[line_index].cross_start + lines[line_index].largest_collapsed_strut());
    }
}

pub(in crate::layout::flex) fn refresh_flex_line_metadata(
    lines: &mut [FlexLineLayout],
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) {
    refresh_flex_line_cross_bounds(
        lines,
        items,
        children,
        container_style,
        physical_direction,
        container_cross_size,
    );
    for line in lines {
        line.first_baseline = flex_line_baseline(
            &line.item_indices,
            items,
            estimates,
            children,
            container_style,
            FlexBaselineSet::First,
            physical_direction,
        );
        line.last_baseline = flex_line_baseline(
            &line.item_indices,
            items,
            estimates,
            children,
            container_style,
            FlexBaselineSet::Last,
            physical_direction,
        );
    }
}

pub(in crate::layout::flex) fn item_outer_cross_bounds(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> (f32, f32) {
    item.outer_cross_bounds(FlexAxes::from_physical_direction(physical_direction), style)
}

pub(in crate::layout::flex) fn item_outer_cross_size(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> f32 {
    let (cross_start, cross_end) = item_outer_cross_bounds(item, style, physical_direction);
    (cross_end - cross_start).max(0.0)
}

pub(in crate::layout::flex) fn estimated_outer_cross_size(
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
            + style.margin.top
            + style.margin.bottom
    } else {
        estimate.width.points()
            + style.padding.left
            + style.padding.right
            + borders.left
            + borders.right
            + style.margin.left
            + style.margin.right
    }
    .max(0.0)
}

pub(in crate::layout::flex) fn collapsed_struts_from_visible_layout(
    children: &[StyledChild<'_>],
    style: &ComputedStyle,
    visible_layout: &FlexLayout,
) -> Vec<FlexCollapsedStrut> {
    let physical_direction = physical_flex_direction(style);
    children
        .iter()
        .enumerate()
        .filter(|(_, child)| flex_item_is_collapsed(&child.style))
        .map(|(item_index, child)| {
            let item = &visible_layout.items[item_index];
            let line = visible_layout
                .lines
                .iter()
                .find(|line| line.item_indices.contains(&item_index));
            FlexCollapsedStrut {
                item_index,
                cross_size: item_outer_cross_size(item, &child.style, physical_direction),
                source_start: line.map(|line| line.source_start).unwrap_or(item_index),
                source_end: line.map(|line| line.source_end).unwrap_or(item_index + 1),
            }
        })
        .collect()
}

pub(in crate::layout::flex) fn attach_collapsed_struts_to_active_lines(
    lines: &mut Vec<FlexLineLayout>,
    source_indices: &[usize],
    collapsed_struts: &[FlexCollapsedStrut],
) {
    if collapsed_struts.is_empty() {
        return;
    }

    for line in lines.iter_mut() {
        let mut source_start = usize::MAX;
        let mut source_end = 0usize;
        for &active_index in &line.item_indices {
            let Some(&source_index) = source_indices.get(active_index) else {
                continue;
            };
            source_start = source_start.min(source_index);
            source_end = source_end.max(source_index + 1);
        }
        if source_start != usize::MAX {
            line.source_start = source_start;
            line.source_end = source_end;
        }
    }

    for strut in collapsed_struts {
        if lines.is_empty() {
            lines.push(FlexLineLayout {
                item_indices: Vec::new(),
                source_start: strut.item_index,
                source_end: strut.item_index + 1,
                main_start: 0.0,
                main_end: 0.0,
                cross_start: 0.0,
                cross_end: strut.cross_size,
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: vec![strut.clone()],
            });
            continue;
        }

        let target_index = lines
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| collapsed_strut_line_overlap(strut, line))
            .filter(|(_, line)| collapsed_strut_line_overlap(strut, line) > 0)
            .map(|(index, _)| index)
            .or_else(|| {
                lines
                    .iter()
                    .position(|line| strut.item_index < line.source_end)
            })
            .unwrap_or(lines.len() - 1);
        let line = &mut lines[target_index];
        line.source_start = line.source_start.min(strut.source_start);
        line.source_end = line.source_end.max(strut.source_end);
        line.collapsed_struts.push(strut.clone());
        line.cross_end = line
            .cross_end
            .max(line.cross_start + line.largest_collapsed_strut());
    }
}

pub(in crate::layout::flex) fn repack_lines_after_collapsed_struts(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
) {
    if lines.len() <= 1 || !lines.iter().any(|line| !line.collapsed_struts.is_empty()) {
        return;
    }

    let axes = FlexAxes::from_physical_direction(physical_direction);
    let mut line_order = (0..lines.len()).collect::<Vec<_>>();
    line_order.sort_by(|&a, &b| {
        lines[a]
            .cross_start
            .partial_cmp(&lines[b].cross_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut next_cross_start = lines[line_order[0]].cross_start;
    for line_index in line_order {
        let delta = next_cross_start - lines[line_index].cross_start;
        if delta.abs() > 0.01 {
            lines[line_index].cross_start += delta;
            lines[line_index].cross_end += delta;
            for &item_index in &lines[line_index].item_indices {
                items[item_index].translate_cross(axes, delta);
            }
        }
        next_cross_start = lines[line_index].cross_end;
    }
}

/// Repack flex lines after Quire-side main-size corrections.
///
/// Taffy performs flexible length resolution and `justify-content` packing
/// before Quire applies the final automatic minimum-size guard for edge cases
/// Taffy cannot represent. When that guard changes a main size, CSS Flexbox's
/// main-axis alignment must be recomputed from the corrected outer sizes:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-align> and
/// <https://www.w3.org/TR/css-align-3/#distribution-values>.
pub(in crate::layout::flex) fn repack_lines_after_main_size_adjustment(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_main_size: f32,
) {
    if !container_main_size.is_finite() {
        return;
    }

    let (physical_gap_width, physical_gap_height) = physical_flex_gaps(container_style);
    let main_gap = used_flex_gap(
        if physical_direction.is_row_axis() {
            physical_gap_width
        } else {
            physical_gap_height
        },
        container_main_size,
    )
    .max(0.0);

    for line in lines {
        if line.item_indices.is_empty() {
            continue;
        }

        let mut physical_order = line.item_indices.clone();
        physical_order.sort_by(|&left, &right| {
            let left_start =
                item_outer_main_bounds(&items[left], &children[left].style, physical_direction).0;
            let right_start =
                item_outer_main_bounds(&items[right], &children[right].style, physical_direction).0;
            left_start.total_cmp(&right_start)
        });

        let item_count = physical_order.len();
        let fixed_outer_size = physical_order
            .iter()
            .map(|&index| {
                item_main_size(&items[index], physical_direction)
                    + fixed_main_before_margin(&children[index].style, physical_direction)
                    + fixed_main_after_margin(&children[index].style, physical_direction)
            })
            .sum::<f32>();
        let total_gap = main_gap * item_count.saturating_sub(1) as f32;
        let free_space = container_main_size - fixed_outer_size - total_gap;
        let auto_margin_count = physical_order
            .iter()
            .map(|&index| main_auto_margin_count(&children[index].style, physical_direction))
            .sum::<usize>();
        let auto_margin = if free_space > 0.0 && auto_margin_count > 0 {
            free_space / auto_margin_count as f32
        } else {
            0.0
        };
        let (initial_offset, extra_gap) = if auto_margin_count > 0 && free_space > 0.0 {
            (0.0, 0.0)
        } else {
            justify_content_offsets(
                container_style.justify_content,
                physical_direction,
                free_space,
                item_count,
            )
        };

        let mut cursor = initial_offset;
        for (position, &item_index) in physical_order.iter().enumerate() {
            cursor +=
                main_before_margin(&children[item_index].style, physical_direction, auto_margin);
            set_item_main_start(&mut items[item_index], physical_direction, cursor);
            cursor += item_main_size(&items[item_index], physical_direction);
            cursor +=
                main_after_margin(&children[item_index].style, physical_direction, auto_margin);
            if position + 1 < item_count {
                cursor += main_gap + extra_gap;
            }
        }

        if let Some((main_start, main_end)) =
            flex_line_items_main_extent(line, items, children, physical_direction)
        {
            line.main_start = main_start;
            line.main_end = main_end;
        }
    }
}

pub(in crate::layout::flex) fn item_main_size(
    item: &FlexItemLayout,
    physical_direction: FlexDirection,
) -> f32 {
    item.main_size(FlexAxes::from_physical_direction(physical_direction))
        .max(0.0)
}

pub(in crate::layout::flex) fn set_item_main_start(
    item: &mut FlexItemLayout,
    physical_direction: FlexDirection,
    main_start: f32,
) {
    item.set_main_start(
        FlexAxes::from_physical_direction(physical_direction),
        main_start,
    );
}

pub(in crate::layout::flex) fn fixed_main_before_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> f32 {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.left.is_auto() {
            0.0
        } else {
            style.margin.left
        }
    } else if style.box_values.margin.top.is_auto() {
        0.0
    } else {
        style.margin.top
    }
}

pub(in crate::layout::flex) fn fixed_main_after_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> f32 {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.right.is_auto() {
            0.0
        } else {
            style.margin.right
        }
    } else if style.box_values.margin.bottom.is_auto() {
        0.0
    } else {
        style.margin.bottom
    }
}

pub(in crate::layout::flex) fn main_before_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    auto_margin: f32,
) -> f32 {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.left.is_auto() {
            auto_margin
        } else {
            style.margin.left
        }
    } else if style.box_values.margin.top.is_auto() {
        auto_margin
    } else {
        style.margin.top
    }
}

pub(in crate::layout::flex) fn main_after_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    auto_margin: f32,
) -> f32 {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.right.is_auto() {
            auto_margin
        } else {
            style.margin.right
        }
    } else if style.box_values.margin.bottom.is_auto() {
        auto_margin
    } else {
        style.margin.bottom
    }
}

pub(in crate::layout::flex) fn main_auto_margin_count(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> usize {
    if physical_direction.is_row_axis() {
        style.box_values.margin.left.is_auto() as usize
            + style.box_values.margin.right.is_auto() as usize
    } else {
        style.box_values.margin.top.is_auto() as usize
            + style.box_values.margin.bottom.is_auto() as usize
    }
}

pub(in crate::layout::flex) fn justify_content_offsets(
    justify_content: JustifyContent,
    physical_direction: FlexDirection,
    free_space: f32,
    item_count: usize,
) -> (f32, f32) {
    let keyword = justify_content_fallback_keyword(justify_content, free_space, item_count);
    let reversed = matches!(
        physical_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let first = match keyword {
        ContentAlignmentKeyword::Normal
        | ContentAlignmentKeyword::Stretch
        | ContentAlignmentKeyword::Start => 0.0,
        ContentAlignmentKeyword::FlexStart => {
            if reversed {
                free_space
            } else {
                0.0
            }
        }
        ContentAlignmentKeyword::End => free_space,
        ContentAlignmentKeyword::FlexEnd => {
            if reversed {
                0.0
            } else {
                free_space
            }
        }
        ContentAlignmentKeyword::Left => 0.0,
        ContentAlignmentKeyword::Right => free_space,
        ContentAlignmentKeyword::Center => free_space / 2.0,
        ContentAlignmentKeyword::SpaceBetween => 0.0,
        ContentAlignmentKeyword::SpaceAround => {
            if free_space >= 0.0 {
                (free_space / item_count as f32) / 2.0
            } else {
                free_space / 2.0
            }
        }
        ContentAlignmentKeyword::SpaceEvenly => {
            if free_space >= 0.0 {
                free_space / (item_count + 1) as f32
            } else {
                free_space / 2.0
            }
        }
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => 0.0,
    };
    let positive_free_space = free_space.max(0.0);
    let between = match keyword {
        ContentAlignmentKeyword::SpaceBetween if item_count > 1 => {
            positive_free_space / (item_count - 1) as f32
        }
        ContentAlignmentKeyword::SpaceAround if item_count > 0 => {
            positive_free_space / item_count as f32
        }
        ContentAlignmentKeyword::SpaceEvenly => positive_free_space / (item_count + 1) as f32,
        _ => 0.0,
    };
    (first, between)
}

pub(in crate::layout::flex) fn justify_content_fallback_keyword(
    justify_content: JustifyContent,
    free_space: f32,
    item_count: usize,
) -> ContentAlignmentKeyword {
    let mut keyword = justify_content.keyword;
    let mut safe = justify_content.safety == AlignmentSafety::Safe;
    if item_count <= 1 || free_space <= 0.0 {
        (keyword, safe) = match keyword {
            ContentAlignmentKeyword::Stretch | ContentAlignmentKeyword::SpaceBetween => {
                (ContentAlignmentKeyword::FlexStart, true)
            }
            ContentAlignmentKeyword::SpaceAround | ContentAlignmentKeyword::SpaceEvenly => {
                (ContentAlignmentKeyword::Center, true)
            }
            other => (other, safe),
        };
    }
    if free_space <= 0.0 && safe {
        ContentAlignmentKeyword::Start
    } else {
        keyword
    }
}

/// Apply CSS Box Alignment baseline content-alignment to wrapped row flex lines.
///
/// Taffy 0.11 maps `align-content: baseline` to start packing. CSS Align
/// instead treats flex lines as the alignment subjects and aligns their
/// compatible baseline sets when those sets are available:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-content>.
pub(in crate::layout::flex) fn apply_baseline_align_content_offsets(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
) {
    if lines.is_empty() || container_style.flex_wrap == FlexWrap::NoWrap {
        return;
    }

    let baseline_set = match container_style.align_content.keyword {
        ContentAlignmentKeyword::Baseline => FlexBaselineSet::First,
        ContentAlignmentKeyword::LastBaseline => FlexBaselineSet::Last,
        _ => return,
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
            container_style,
            physical_direction,
            container_cross_size,
            baseline_set,
        );
        return;
    }

    if !container_style.flex_direction.is_row_axis() {
        apply_baseline_align_content_fallback_offset(
            items,
            lines,
            container_style,
            physical_direction,
            container_cross_size,
            baseline_set,
        );
        return;
    }

    let target_baseline = line_baselines
        .iter()
        .flatten()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    if !target_baseline.is_finite() {
        return;
    }

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
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: f32,
    baseline_set: FlexBaselineSet,
) {
    let Some((group_start, group_end)) = flex_line_group_cross_bounds(lines) else {
        return;
    };
    let group_size = (group_end - group_start).max(0.0);
    let target_side = if group_size > container_cross_size.max(0.0) {
        flex_cross_start_side(container_style)
    } else {
        match baseline_set {
            FlexBaselineSet::First => flex_cross_start_side(container_style),
            FlexBaselineSet::Last => flex_cross_end_side(container_style),
        }
    };
    align_flex_line_group_cross_side(
        lines,
        items,
        physical_direction,
        target_side,
        container_cross_size,
    );
}

pub(in crate::layout::flex) fn flex_line_group_cross_bounds(
    lines: &[FlexLineLayout],
) -> Option<(f32, f32)> {
    lines
        .iter()
        .map(|line| (line.cross_start, line.cross_end))
        .fold(None, |bounds, line_bounds| {
            Some(match bounds {
                Some((start, end)) => (start.min(line_bounds.0), end.max(line_bounds.1)),
                None => line_bounds,
            })
        })
}
