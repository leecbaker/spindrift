use super::*;

/// Inputs that define the stable flex-line topology and cross-axis geometry.
///
/// Grouping these independent container metrics prevents callers from mixing
/// a main-axis wrapping constraint with an unrelated cross-size slot.
pub(in crate::layout::flex) struct FlexLineCollectionContext<'a> {
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) physical_direction: FlexDirection,
    pub(in crate::layout::flex) cross_axis_layout: FlexLineCrossAxisLayout,
    pub(in crate::layout::flex) hypothetical_outer_main_sizes: &'a [FlexMainSize],
    pub(in crate::layout::flex) container_main_size: Option<FlexMainSize>,
    pub(in crate::layout::flex) main_gap: FlexMainSize,
}

/// The resolved cross-axis inputs shared by every flex-line sizing phase.
///
/// Percentage gutters use the flex container's cross-axis percentage basis.
/// Resolving that basis once at flex-line collection ensures initial stretch
/// and later line placement reserve the same CSS gap:
/// <https://www.w3.org/TR/css-align-3/#gap-percent>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::flex) struct FlexLineCrossAxisLayout {
    pub(in crate::layout::flex) constraint: FlexLineCrossConstraint,
    pub(in crate::layout::flex) gap: FlexCrossSize,
}

/// The source of a flex line's available cross size.
///
/// A numeric layout constraint does not itself make the container's cross
/// size definite. In particular, a page fragmentainer can cap an automatic
/// row's physical height while the row must still derive its content-based
/// cross size from its hypothetical item contributions. Keeping that state at
/// the line-collection boundary prevents a provisional Taffy root rectangle
/// from being treated as Flexbox's definite single-line slot:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-align>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::flex) enum FlexLineCrossConstraint {
    DefiniteInnerSize(FlexCrossSize),
    /// A final used inner size established by the container's min/max clamp.
    /// It constrains a single flex line, but does not make cyclic descendant
    /// percentages definite.
    ClampedInnerSize(FlexCrossSize),
    /// An explicit balanced line count reserves equal final line slots before
    /// item measurement. These slots are distinct from the container's
    /// percentage basis and must not be regenerated from normal-wrap
    /// hypothetical contributions.
    BalancedLineSlot(FlexCrossSize),
    ContentBased,
}

impl FlexLineCrossConstraint {
    /// Construct the line constraint from the container's resolved CSS cross
    /// size. A numeric available-space limit is insufficient: an automatic
    /// row within a definite fragmentainer remains content-based.
    pub(in crate::layout::flex) fn from_container(
        container_style: &ComputedStyle,
        available: FlexAvailableSpace,
        physical_direction: FlexDirection,
        used_inner_size: FlexCrossSize,
    ) -> Self {
        if container_style.flex_wrap.balances_lines() && container_style.flex_line_count.get() > 1 {
            let line_available = balanced_flex_item_measure_available_space(
                container_style,
                physical_direction,
                available,
            );
            let line_cross_size = if physical_direction.is_row_axis() {
                line_available
                    .height_constraint()
                    .map(|height| FlexCrossSize::new(height.points()))
            } else {
                Some(FlexCrossSize::new(line_available.width.points()))
            };
            if let Some(line_cross_size) = line_cross_size {
                return Self::BalancedLineSlot(line_cross_size);
            }
        }
        // This typed content-based path currently implements physical
        // horizontal rows only. Preserve the established Taffy reconciliation
        // for column and non-horizontal writing modes until their logical
        // cross-size constraints receive the same representation.
        if !physical_direction.is_row_axis()
            || container_style.writing_mode != WritingMode::HorizontalTb
        {
            return Self::DefiniteInnerSize(used_inner_size);
        }
        let used_cross_size = if physical_direction.is_row_axis() {
            used_length_percentage_or_auto_with_basis(
                container_style.box_values.height.value().clone(),
                available.height_basis,
            )
        } else {
            used_length_percentage_or_auto_with_basis(
                container_style.box_values.width.clone(),
                available.width_basis,
            )
        };
        if used_cross_size.is_some() {
            Self::DefiniteInnerSize(used_inner_size)
        } else if if physical_direction.is_row_axis() {
            !container_style.box_values.min_height.is_auto()
                || !container_style.box_values.max_height.is_auto()
        } else {
            !container_style.box_values.min_width.is_auto()
                || !container_style.box_values.max_width.is_auto()
        } {
            Self::ClampedInnerSize(used_inner_size)
        } else {
            Self::ContentBased
        }
    }

    /// The final slot available to a single flex line.
    ///
    /// This intentionally includes a min/max-clamped automatic size. A
    /// clamp constrains final line placement, but callers resolving
    /// descendant percentages must consult the separately preserved
    /// percentage basis instead.
    pub(super) fn single_line_slot_size(self) -> Option<FlexCrossSize> {
        match self {
            Self::DefiniteInnerSize(size) | Self::ClampedInnerSize(size) => Some(size),
            Self::BalancedLineSlot(_) | Self::ContentBased => None,
        }
    }

    pub(in crate::layout::flex) fn reserved_balanced_line_slot(self) -> Option<FlexCrossSize> {
        match self {
            Self::BalancedLineSlot(size) => Some(size),
            Self::DefiniteInnerSize(_) | Self::ClampedInnerSize(_) | Self::ContentBased => None,
        }
    }
}

pub(in crate::layout::flex) fn flex_lines_from_items(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    estimates: &[FlexItemEstimate],
    context: FlexLineCollectionContext<'_>,
) -> Vec<FlexLineLayout> {
    let topology = collect_flex_line_topology(
        items.len(),
        context.container_style.flex_wrap,
        context.hypothetical_outer_main_sizes,
        context.container_main_size,
        context.main_gap,
    );
    let mut lines = topology
        .into_iter()
        .enumerate()
        .map(|(logical_cross_start_rank, topology)| {
            flex_line_layout_from_topology(
                topology,
                logical_cross_start_rank,
                items,
                children,
                context.physical_direction,
            )
        })
        .collect::<Vec<_>>();
    refresh_flex_line_metadata(
        &mut lines,
        items,
        estimates,
        children,
        context.container_style,
        context.physical_direction,
        context.cross_axis_layout,
    );
    lines
}

/// Collect immutable ordinary-wrap line membership before item rectangles are
/// adjusted by baseline alignment, stretch, or fragmentation.
///
/// The input sequence is already order-modified by the flex adapter.  A
/// wrapping container with an indefinite main size has a single available
/// line; otherwise this mirrors Flexbox's consecutive hypothetical-size line
/// collection, including the required oversized-first-item behavior:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>.
pub(in crate::layout::flex) fn collect_flex_line_topology(
    item_count: usize,
    flex_wrap: FlexWrap,
    hypothetical_outer_main_sizes: &[FlexMainSize],
    container_main_size: Option<FlexMainSize>,
    main_gap: FlexMainSize,
) -> Vec<FlexLineTopology> {
    if item_count == 0 {
        return Vec::new();
    }
    if flex_wrap == FlexWrap::NoWrap || container_main_size.is_none() {
        return vec![FlexLineTopology {
            item_indices: (0..item_count).collect(),
            source_start: 0,
            source_end: item_count,
        }];
    }

    let available_main_size = container_main_size.expect("checked definite main size");
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut current_size = FlexMainSize::new(0.0);
    let tolerance = FlexMainSize::new(0.01);
    for index in 0..item_count {
        let item_size = hypothetical_outer_main_sizes
            .get(index)
            .copied()
            .unwrap_or_else(|| FlexMainSize::new(0.0));
        let candidate = if current.is_empty() {
            item_size
        } else {
            current_size + main_gap + item_size
        };
        if !current.is_empty() && candidate > available_main_size + tolerance {
            let source_start = *current.first().expect("non-empty line");
            let source_end = current.last().expect("non-empty line") + 1;
            lines.push(FlexLineTopology {
                item_indices: std::mem::take(&mut current),
                source_start,
                source_end,
            });
            current_size = item_size;
        } else {
            current_size = candidate;
        }
        current.push(index);
    }
    let source_start = *current.first().expect("items produce a final line");
    let source_end = current.last().expect("items produce a final line") + 1;
    lines.push(FlexLineTopology {
        item_indices: current,
        source_start,
        source_end,
    });
    lines
}

/// Materialize one stable line record from its canonical membership.
///
/// Taffy supplies initial coordinates, but only to seed the line's slot and
/// item bounds.  It never determines membership after this boundary.
fn flex_line_layout_from_topology(
    topology: FlexLineTopology,
    logical_cross_start_rank: usize,
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) -> FlexLineLayout {
    let mut cross_bounds: Option<(FlexCrossOffset, FlexCrossOffset)> = None;
    let mut main_bounds: Option<(FlexMainOffset, FlexMainOffset)> = None;
    for &index in &topology.item_indices {
        let cross =
            item_outer_cross_bounds(&items[index], &children[index].style, physical_direction);
        let main =
            item_outer_main_bounds(&items[index], &children[index].style, physical_direction);
        cross_bounds = Some(match cross_bounds {
            Some((start, end)) => (start.min(cross.0), end.max(cross.1)),
            None => cross,
        });
        main_bounds = Some(match main_bounds {
            Some((start, end)) => (start.min(main.0), end.max(main.1)),
            None => main,
        });
    }
    let (cross_start, cross_end) = cross_bounds.expect("line topology is non-empty");
    let (main_start, main_end) = main_bounds.expect("line topology is non-empty");
    FlexLineLayout {
        item_indices: topology.item_indices,
        logical_cross_start_rank,
        source_start: topology.source_start,
        source_end: topology.source_end,
        main_start,
        main_end,
        cross_start,
        cross_end,
        first_baseline: None,
        last_baseline: None,
        collapsed_struts: Vec::new(),
    }
}

pub(in crate::layout::flex) fn item_outer_cross_bounds(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> (FlexCrossOffset, FlexCrossOffset) {
    let (start, end) =
        item.outer_cross_bounds(PhysicalFlexDirection::new(physical_direction), style);
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
                logical_cross_start_rank: 0,
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

    let axes = PhysicalFlexDirection::new(physical_direction);
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
