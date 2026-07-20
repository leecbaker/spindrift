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
                && (line.cross_size() - outer_size).is_negative()
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
    container_cross_size: FlexCrossSize,
) -> Vec<FlexLineLayout> {
    if container_style.flex_wrap == FlexWrap::NoWrap {
        let (main_start, main_end) = flex_items_main_extent(items, children, physical_direction)
            .unwrap_or((FlexMainOffset::new(0.0), FlexMainOffset::new(0.0)));
        let item_indices = (0..items.len()).collect::<Vec<_>>();
        return vec![FlexLineLayout {
            item_indices: item_indices.clone(),
            source_start: 0,
            source_end: items.len(),
            main_start,
            main_end,
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(0.0) + container_cross_size,
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
    let overlap_tolerance = FlexCrossLength::new(0.01);
    for index in 0..items.len() {
        let (cross_start, cross_end) =
            item_outer_cross_bounds(&items[index], &children[index].style, physical_direction);
        let (main_start, main_end) =
            item_outer_main_bounds(&items[index], &children[index].style, physical_direction);
        if let Some(line) = lines.iter_mut().find(|line| {
            let cross_ranges_overlap = cross_start < line.cross_end - overlap_tolerance
                && cross_end > line.cross_start + overlap_tolerance;
            // A stretched item's hypothetical cross size may be zero before
            // CSS Flexbox's cross-size step. It still belongs to the source
            // line that precedes it in the main axis; grouping exclusively
            // by non-zero cross-range overlap would split that line and give
            // only its later siblings the stretched width.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-line-break>
            // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
            let shares_collapsed_cross_start = (cross_start - line.cross_start).abs() <= 0.01
                && ((cross_end - cross_start).abs() <= 0.01
                    || (line.cross_end - line.cross_start).abs() <= 0.01);
            let follows_line_in_main_axis = match physical_direction {
                FlexDirection::Row | FlexDirection::Column => {
                    main_start >= line.main_end - FlexMainLength::new(0.01)
                }
                FlexDirection::RowReverse | FlexDirection::ColumnReverse => {
                    main_end <= line.main_start + FlexMainLength::new(0.01)
                }
            };
            cross_ranges_overlap || (shares_collapsed_cross_start && follows_line_in_main_axis)
        }) {
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
    container_cross_size: FlexCrossSize,
) {
    let stretch_wrapped_lines = !lines.is_empty()
        && container_style.flex_wrap != FlexWrap::NoWrap
        && matches!(
            container_style.align_content.keyword,
            ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch
        );
    for line in &mut *lines {
        if container_style.flex_wrap == FlexWrap::NoWrap {
            line.cross_start = FlexCrossOffset::new(0.0);
            line.cross_end = FlexCrossOffset::new(0.0) + container_cross_size;
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
        let mut cross_bounds: Option<(FlexCrossOffset, FlexCrossOffset)> = None;
        let mut main_bounds: Option<(FlexMainOffset, FlexMainOffset)> = None;
        for &index in &line.item_indices {
            let (item_cross_start, item_cross_end) =
                item_outer_cross_bounds(&items[index], &children[index].style, physical_direction);
            let (item_main_start, item_main_end) =
                item_outer_main_bounds(&items[index], &children[index].style, physical_direction);
            cross_bounds = Some(match cross_bounds {
                Some((cross_start, cross_end)) => (
                    cross_start.min(item_cross_start),
                    cross_end.max(item_cross_end),
                ),
                None => (item_cross_start, item_cross_end),
            });
            main_bounds = Some(match main_bounds {
                Some((main_start, main_end)) => {
                    (main_start.min(item_main_start), main_end.max(item_main_end))
                }
                None => (item_main_start, item_main_end),
            });
        }
        let (cross_start, cross_end) = cross_bounds.expect("non-empty flex line has item bounds");
        line.cross_start = cross_start;
        line.cross_end = cross_end.max(cross_start + line.largest_collapsed_strut());
        let (main_start, main_end) = main_bounds.expect("non-empty flex line has item bounds");
        line.main_start = main_start;
        line.main_end = main_end;
    }
    if stretch_wrapped_lines {
        let PhysicalFlexGaps {
            horizontal: physical_gap_width,
            vertical: physical_gap_height,
        } = physical_flex_gaps(container_style);
        let cross_gap = used_flex_gap(
            if physical_direction.is_row_axis() {
                physical_gap_height
            } else {
                physical_gap_width
            },
            PercentageBasis::definite(flex_cross_content_box_length(container_cross_size)),
        );
        preserve_stretched_flex_line_cross_bounds(
            lines,
            container_cross_size,
            flex_cross_gap_size(cross_gap),
        );
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
    container_cross_size: FlexCrossSize,
    cross_gap: FlexCrossSize,
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
        line.cross_start = FlexCrossOffset::new(0.0);
        let cross_origin = FlexCrossOffset::new(0.0);
        line.cross_end = (cross_origin + container_cross_size)
            .max(line.cross_end)
            .max(line.cross_start + line.largest_collapsed_strut());
        return;
    }

    let cross_origin = FlexCrossOffset::new(0.0);
    for position in 0..line_order.len() {
        let line_index = line_order[position];
        if position == 0 && lines[line_index].cross_start > cross_origin {
            lines[line_index].cross_start = cross_origin;
        }
        let next_cross_start = line_order
            .get(position + 1)
            .map(|&next_index| {
                cross_origin
                    + ((lines[next_index].cross_start - cross_origin) - cross_gap)
                        .non_negative_size()
            })
            .unwrap_or(cross_origin + container_cross_size);
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
    container_cross_size: FlexCrossSize,
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
) -> (FlexCrossOffset, FlexCrossOffset) {
    let (start, end) = item.outer_cross_bounds(
        FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction)),
        style,
    );
    (start, end)
}

pub(in crate::layout::flex) fn item_outer_cross_size(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexCrossSize {
    let (cross_start, cross_end) = item_outer_cross_bounds(item, style, physical_direction);
    (cross_end - cross_start).non_negative_size()
}

pub(in crate::layout::flex) fn estimated_outer_cross_size(
    style: &ComputedStyle,
    estimate: FlexItemEstimate,
    physical_direction: FlexDirection,
) -> LayoutLength {
    let borders = used_border_widths(style);
    layout_pt(
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
        .max(0.0),
    )
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
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(0.0),
                cross_start: FlexCrossOffset::new(0.0),
                cross_end: FlexCrossOffset::new(0.0) + strut.cross_size,
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

    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
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
            lines[line_index].cross_start = lines[line_index].cross_start + delta;
            lines[line_index].cross_end = lines[line_index].cross_end + delta;
            for &item_index in &lines[line_index].item_indices {
                items[item_index].translate_cross(axes, delta);
            }
        }
        next_cross_start = lines[line_index].cross_end;
    }
}

/// Repartition a balanced flex container's already-sized items across its
/// normal-wrap line count.
///
/// CSS Flexbox Level 2 keeps the line count selected by ordinary wrapping, but
/// chooses a more even distribution of items where each candidate line still
/// fits the available main size. Taffy does not expose this draft algorithm,
/// so Quire preserves its flex sizing pass, changes only line membership, and
/// then reuses the normal main-axis repacking pass:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexBalanceContext<'a> {
    pub(in crate::layout::flex) physical_direction: FlexDirection,
    pub(in crate::layout::flex) requested_line_count: Option<usize>,
    pub(in crate::layout::flex) hypothetical_main_sizes: Option<&'a [FlexMainSize]>,
    pub(in crate::layout::flex) main_gap: FlexMainSize,
    pub(in crate::layout::flex) cross_gap: FlexCrossSize,
    pub(in crate::layout::flex) available_main_size: FlexMainSize,
}

pub(in crate::layout::flex) fn rebalance_flex_line_membership(
    lines: &mut Vec<FlexLineLayout>,
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    context: FlexBalanceContext<'_>,
) -> bool {
    if lines.is_empty() || !context.available_main_size.is_finite() {
        return false;
    }

    let ordered_items = lines
        .iter()
        .flat_map(|line| line.item_indices.iter().cloned())
        .collect::<Vec<_>>();
    if ordered_items.len() < lines.len() {
        return false;
    }

    let outer_main_sizes = ordered_items
        .iter()
        .map(|&index| {
            let item_main_size = context
                .hypothetical_main_sizes
                .and_then(|sizes| sizes.get(index))
                .cloned()
                .unwrap_or_else(|| item_main_size(&items[index], context.physical_direction));
            (item_main_size
                + fixed_main_before_margin(&children[index].style, context.physical_direction)
                + fixed_main_after_margin(&children[index].style, context.physical_direction))
            .non_negative_size()
        })
        .collect::<Vec<_>>();
    let inferred_line_count = balanced_flex_line_count(
        &outer_main_sizes,
        context.main_gap,
        context.available_main_size,
    );
    // `flex-line-count` is an explicit request for the number of balanced
    // lines.  It must therefore be allowed to reduce the provisional number
    // of ordinary-wrap lines as well as extend it.  Without an explicit
    // value, balance retains the ordinary line count (or the minimum count
    // required by oversized hypothetical main sizes).
    // <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
    let requested_line_count = context
        .requested_line_count
        .unwrap_or_else(|| lines.len().max(inferred_line_count))
        .clamp(1, ordered_items.len());
    if requested_line_count < 2 {
        return false;
    }
    while lines.len() < requested_line_count {
        let Some(last_line) = lines.last().cloned() else {
            return false;
        };
        let cross_size = last_line.cross_size();
        let cross_start = last_line.cross_end + context.cross_gap;
        lines.push(FlexLineLayout {
            item_indices: Vec::new(),
            source_start: last_line.source_end,
            source_end: last_line.source_end,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(0.0),
            cross_start,
            cross_end: cross_start + cross_size,
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        });
    }
    lines.truncate(requested_line_count);
    let line_count = lines.len();
    let Some(partitions) = balanced_flex_line_partitions(
        &ordered_items,
        &outer_main_sizes,
        line_count,
        context.main_gap,
        context.available_main_size,
    ) else {
        return false;
    };

    let axes =
        FlexAxes::from_physical_direction(PhysicalFlexDirection::new(context.physical_direction));
    let slots = lines
        .iter()
        .map(|line| (line.cross_start, line.cross_end))
        .collect::<Vec<_>>();
    let mut changed = false;
    for ((line, item_indices), (cross_start, cross_end)) in
        lines.iter_mut().zip(partitions).zip(slots)
    {
        changed |= line.item_indices != item_indices;
        for (position, &item_index) in item_indices.iter().enumerate() {
            let item = &mut items[item_index];
            let delta = cross_start - item.cross_start(axes);
            item.translate_cross(axes, delta);
            // Main-axis repacking sorts by physical position. A member moved
            // from a later normal-wrap line may retain a stale position, so
            // seed the new source-order partition before that shared pass.
            let provisional_main_start = if matches!(
                context.physical_direction,
                FlexDirection::RowReverse | FlexDirection::ColumnReverse
            ) {
                (item_indices.len() - position) as f32
            } else {
                position as f32
            };
            set_item_main_start(
                item,
                context.physical_direction,
                FlexMainOffset::new(provisional_main_start),
            );
        }
        line.source_start = item_indices
            .iter()
            .cloned()
            .min()
            .unwrap_or(line.source_start);
        line.source_end = item_indices
            .iter()
            .cloned()
            .max()
            .map(|index| index + 1)
            .unwrap_or(line.source_end);
        line.item_indices = item_indices;
        line.cross_start = cross_start;
        line.cross_end = cross_end;
    }
    changed
}

/// Returns the minimum balanced line count required by hypothetical outer sizes.
///
/// This mirrors the Level 2 sequence constraints when ordinary wrapping has
/// already folded a zero-sized item into a preceding overflowing item. An
/// overflowing item occupies its own sequence; a following zero-sized item is
/// assigned to the next sequence as required by the balance algorithm:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance>.
fn balanced_flex_line_count(
    outer_main_sizes: &[FlexMainSize],
    main_gap: FlexMainSize,
    available_main_size: FlexMainSize,
) -> usize {
    if outer_main_sizes.is_empty() {
        return 0;
    }
    let mut line_count = 1usize;
    let tolerance = FlexMainSize::new(0.01);
    let mut line_extent = outer_main_sizes[0];
    let mut line_overflows = line_extent > available_main_size + tolerance;
    for size in &outer_main_sizes[1..] {
        let candidate = line_extent + main_gap + *size;
        let must_break = candidate > available_main_size + tolerance
            || (line_overflows && *size == FlexMainSize::new(0.0));
        if must_break {
            line_count += 1;
            line_extent = *size;
            line_overflows = line_extent > available_main_size + tolerance;
        } else {
            line_extent = candidate;
            line_overflows = line_extent > available_main_size + tolerance;
        }
    }
    line_count
}

/// Find the source-order partition that minimizes the total squared line error.
///
/// The Level 2 balancing algorithm searches legal line partitions while
/// retaining the normal-wrap line count. Each line error is its hypothetical
/// outer main extent minus the container main size; the selected partition
/// minimizes the sum of squared errors. Automatic margins and flexible lengths
/// are resolved only after this hypothetical-size partition is selected:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance>.
fn balanced_flex_line_partitions(
    item_indices: &[usize],
    outer_main_sizes: &[FlexMainSize],
    line_count: usize,
    main_gap: FlexMainSize,
    available_main_size: FlexMainSize,
) -> Option<Vec<Vec<usize>>> {
    let item_count = item_indices.len();
    if line_count == 0 || line_count > item_count || item_count != outer_main_sizes.len() {
        return None;
    }

    let mut prefix = Vec::with_capacity(item_count + 1);
    prefix.push(FlexMainSize::new(0.0));
    for size in outer_main_sizes {
        prefix.push(*prefix.last().expect("prefix starts with zero") + *size);
    }
    let line_extent = |start: usize, end: usize| {
        (prefix[end] - prefix[start]).non_negative_size()
            + main_gap.scale(end.saturating_sub(start + 1) as f32)
    };

    let mut costs = vec![vec![f32::INFINITY; item_count + 1]; line_count + 1];
    let mut predecessors = vec![vec![None; item_count + 1]; line_count + 1];
    costs[0][0] = 0.0;
    for line in 1..=line_count {
        for end in line..=item_count {
            for start in (line - 1)..end {
                let extent = line_extent(start, end);
                // A sequence may overflow only when it contains one item.
                // This keeps a following zero-sized item out of an already
                // overflowing sequence.
                if (end - start > 1 && extent > available_main_size + FlexMainSize::new(0.01))
                    || !costs[line - 1][start].is_finite()
                {
                    continue;
                }
                let error = extent - available_main_size;
                // Dynamic-programming costs are dimensionless ordering
                // scores. The conversion happens only at this algorithmic
                // boundary; all line geometry remains typed above.
                let candidate = costs[line - 1][start] + error.points().powi(2);
                // Prefer the later break for equal errors. During reverse
                // reconstruction, this gives the draft algorithm's start bias:
                // assign as many items as possible to earlier lines.
                if candidate <= costs[line][end] {
                    costs[line][end] = candidate;
                    predecessors[line][end] = Some(start);
                }
            }
        }
    }
    if !costs[line_count][item_count].is_finite() {
        return None;
    }

    let mut partitions = Vec::with_capacity(line_count);
    let mut end = item_count;
    for line in (1..=line_count).rev() {
        let start = predecessors[line][end]?;
        partitions.push(item_indices[start..end].to_vec());
        end = start;
    }
    partitions.reverse();
    Some(partitions)
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
    container_main_size: FlexMainSize,
    container_main_percentage_basis: FlexAvailablePercentageBasis,
) {
    if !container_main_size.is_finite() {
        return;
    }

    let PhysicalFlexGaps {
        horizontal: physical_gap_width,
        vertical: physical_gap_height,
    } = physical_flex_gaps(container_style);
    let main_gap = flex_main_gap_size(used_flex_gap_with_basis(
        if physical_direction.is_row_axis() {
            physical_gap_width
        } else {
            physical_gap_height
        },
        container_main_percentage_basis,
    ));

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
            left_start
                .partial_cmp(&right_start)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let item_count = physical_order.len();
        let fixed_outer_size =
            physical_order
                .iter()
                .fold(FlexMainLength::new(0.0), |total, &index| {
                    total
                        + (item_main_size(&items[index], physical_direction)
                            + fixed_main_before_margin(&children[index].style, physical_direction)
                            + fixed_main_after_margin(&children[index].style, physical_direction))
                });
        let total_gap = main_gap.scale(item_count.saturating_sub(1) as f32);
        let free_space = container_main_size - fixed_outer_size - total_gap;
        let auto_margin_count = physical_order
            .iter()
            .map(|&index| main_auto_margin_count(&children[index].style, physical_direction))
            .sum::<usize>();
        let auto_margin = if free_space.is_positive() && auto_margin_count > 0 {
            free_space.divide(
                std::num::NonZeroUsize::new(auto_margin_count)
                    .expect("positive auto-margin count is non-zero"),
            )
        } else {
            FlexMainLength::new(0.0)
        };
        let justification = if auto_margin_count > 0 && free_space.is_positive() {
            FlexMainJustificationOffsets {
                initial: FlexMainLength::new(0.0),
                between: FlexMainLength::new(0.0),
            }
        } else {
            justify_content_offsets(
                container_style.justify_content,
                physical_direction,
                free_space,
                item_count,
            )
        };

        let mut cursor = FlexMainOffset::new(0.0) + justification.initial;
        for (position, &item_index) in physical_order.iter().enumerate() {
            cursor = cursor
                + main_before_margin(&children[item_index].style, physical_direction, auto_margin);
            set_item_main_start(&mut items[item_index], physical_direction, cursor);
            cursor = cursor + item_main_size(&items[item_index], physical_direction);
            cursor = cursor
                + main_after_margin(&children[item_index].style, physical_direction, auto_margin);
            if position + 1 < item_count {
                cursor = cursor + main_gap + justification.between;
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
) -> FlexMainSize {
    item.main_size(FlexAxes::from_physical_direction(
        PhysicalFlexDirection::new(physical_direction),
    ))
}

pub(in crate::layout::flex) fn set_item_main_start(
    item: &mut FlexItemLayout,
    physical_direction: FlexDirection,
    main_start: FlexMainOffset,
) {
    item.set_main_start(
        FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction)),
        main_start,
    );
}

pub(in crate::layout::flex) fn fixed_main_before_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.left.is_auto() {
            FlexMainLength::new(0.0)
        } else {
            FlexMainLength::new(style.margin.left)
        }
    } else if style.box_values.margin.top.is_auto() {
        FlexMainLength::new(0.0)
    } else {
        FlexMainLength::new(style.margin.top)
    }
}

pub(in crate::layout::flex) fn fixed_main_after_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.right.is_auto() {
            FlexMainLength::new(0.0)
        } else {
            FlexMainLength::new(style.margin.right)
        }
    } else if style.box_values.margin.bottom.is_auto() {
        FlexMainLength::new(0.0)
    } else {
        FlexMainLength::new(style.margin.bottom)
    }
}

pub(in crate::layout::flex) fn main_before_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    auto_margin: FlexMainLength,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.left.is_auto() {
            auto_margin
        } else {
            FlexMainLength::new(style.margin.left)
        }
    } else if style.box_values.margin.top.is_auto() {
        auto_margin
    } else {
        FlexMainLength::new(style.margin.top)
    }
}

pub(in crate::layout::flex) fn main_after_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    auto_margin: FlexMainLength,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.right.is_auto() {
            auto_margin
        } else {
            FlexMainLength::new(style.margin.right)
        }
    } else if style.box_values.margin.bottom.is_auto() {
        auto_margin
    } else {
        FlexMainLength::new(style.margin.bottom)
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
    free_space: FlexMainLength,
    item_count: usize,
) -> FlexMainJustificationOffsets {
    let keyword = justify_content_fallback_keyword(justify_content, free_space, item_count);
    let reversed = matches!(
        physical_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let first = match keyword {
        ContentAlignmentKeyword::Normal
        | ContentAlignmentKeyword::Stretch
        | ContentAlignmentKeyword::Start => FlexMainLength::new(0.0),
        ContentAlignmentKeyword::FlexStart => {
            if reversed {
                free_space
            } else {
                FlexMainLength::new(0.0)
            }
        }
        ContentAlignmentKeyword::End => free_space,
        ContentAlignmentKeyword::FlexEnd => {
            if reversed {
                FlexMainLength::new(0.0)
            } else {
                free_space
            }
        }
        ContentAlignmentKeyword::Left => FlexMainLength::new(0.0),
        ContentAlignmentKeyword::Right => free_space,
        ContentAlignmentKeyword::Center => free_space.half(),
        ContentAlignmentKeyword::SpaceBetween => FlexMainLength::new(0.0),
        ContentAlignmentKeyword::SpaceAround => {
            if !free_space.is_negative() {
                free_space
                    .divide(std::num::NonZeroUsize::new(item_count).expect("non-zero item count"))
                    .half()
            } else {
                free_space.half()
            }
        }
        ContentAlignmentKeyword::SpaceEvenly => {
            if !free_space.is_negative() {
                free_space.divide(
                    std::num::NonZeroUsize::new(item_count + 1)
                        .expect("item count plus one is non-zero"),
                )
            } else {
                free_space.half()
            }
        }
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            FlexMainLength::new(0.0)
        }
    };
    let positive_free_space = free_space.max(FlexMainLength::new(0.0));
    let between = match keyword {
        ContentAlignmentKeyword::SpaceBetween if item_count > 1 => positive_free_space.divide(
            std::num::NonZeroUsize::new(item_count - 1)
                .expect("more than one item leaves a non-zero divisor"),
        ),
        ContentAlignmentKeyword::SpaceAround if item_count > 0 => positive_free_space
            .divide(std::num::NonZeroUsize::new(item_count).expect("non-zero item count")),
        ContentAlignmentKeyword::SpaceEvenly => positive_free_space.divide(
            std::num::NonZeroUsize::new(item_count + 1).expect("item count plus one is non-zero"),
        ),
        _ => FlexMainLength::new(0.0),
    };
    FlexMainJustificationOffsets {
        initial: first,
        between,
    }
}

/// Main-axis offsets selected by CSS `justify-content` for one flex line.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexMainJustificationOffsets {
    pub(in crate::layout::flex) initial: FlexMainLength,
    pub(in crate::layout::flex) between: FlexMainLength,
}

pub(in crate::layout::flex) fn justify_content_fallback_keyword(
    justify_content: JustifyContent,
    free_space: FlexMainLength,
    item_count: usize,
) -> ContentAlignmentKeyword {
    let mut keyword = justify_content.keyword;
    let mut safe = justify_content.safety == AlignmentSafety::Safe;
    if item_count <= 1 || free_space.is_non_positive() {
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
    if free_space.is_non_positive() && safe {
        ContentAlignmentKeyword::Start
    } else {
        keyword
    }
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

    // CSS Flexbox defines line baseline sets for a row main axis. A logical
    // row can project to a physical column in vertical writing, so this must
    // use the authored logical flex direction rather than `physical_direction`.
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
    container_cross_size: FlexCrossSize,
    baseline_set: FlexBaselineSet,
) {
    let Some((group_start, group_end)) = flex_line_group_cross_bounds(lines) else {
        return;
    };
    let group_size = (group_end - group_start).non_negative_size();
    let target_side = if group_size > container_cross_size {
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
) -> Option<(FlexCrossOffset, FlexCrossOffset)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_line(
        item_indices: Vec<usize>,
        cross_start: FlexCrossOffset,
        cross_end: FlexCrossOffset,
    ) -> FlexLineLayout {
        FlexLineLayout {
            source_start: item_indices.iter().cloned().min().unwrap_or(0),
            source_end: item_indices
                .iter()
                .cloned()
                .max()
                .map(|index| index + 1)
                .unwrap_or(0),
            item_indices,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(0.0),
            cross_start,
            cross_end,
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        }
    }

    fn test_child() -> StyledChild<'static> {
        StyledChild {
            kind: FormattingContextChildKind::AnonymousContent { children: vec![] },
            style: ComputedStyle::initial(),
        }
    }

    #[test]
    fn balance_context_uses_cross_gap_when_adding_requested_lines() {
        let mut lines = vec![test_line(
            vec![0, 1],
            FlexCrossOffset::new(0.0),
            FlexCrossOffset::new(10.0),
        )];
        let mut items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                ContainerSize::new(20.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(20.0, 0.0),
                ContainerSize::new(20.0, 10.0),
            )),
        ];
        let children = vec![test_child(), test_child()];

        assert!(rebalance_flex_line_membership(
            &mut lines,
            &mut items,
            &children,
            FlexBalanceContext {
                physical_direction: FlexDirection::Row,
                requested_line_count: Some(2),
                hypothetical_main_sizes: None,
                main_gap: FlexMainSize::new(0.0),
                cross_gap: FlexCrossSize::new(15.0),
                available_main_size: FlexMainSize::new(100.0),
            },
        ));

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].cross_start, FlexCrossOffset::new(25.0));
        assert_eq!(lines[1].cross_end, FlexCrossOffset::new(35.0));
    }

    #[test]
    fn balance_outer_main_sizes_keep_negative_margin_clamp() {
        let outer_main_sizes = [FlexMainSize::new(20.0 - 30.0), FlexMainSize::new(30.0)];

        assert_eq!(outer_main_sizes[0], FlexMainSize::new(0.0));
        assert_eq!(
            balanced_flex_line_count(
                &outer_main_sizes,
                FlexMainSize::new(0.0),
                FlexMainSize::new(30.0),
            ),
            1
        );
    }

    #[test]
    fn balance_partitions_overflowing_main_sizes_at_available_boundary() {
        let item_indices = [0, 1, 2];
        let outer_main_sizes = [
            FlexMainSize::new(60.0),
            FlexMainSize::new(60.0),
            FlexMainSize::new(40.0),
        ];

        assert_eq!(
            balanced_flex_line_partitions(
                &item_indices,
                &outer_main_sizes,
                2,
                FlexMainSize::new(0.0),
                FlexMainSize::new(100.0),
            ),
            Some(vec![vec![0], vec![1, 2]])
        );
    }
}
