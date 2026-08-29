use super::*;

/// Resolve one candidate balanced line through the same flex-length algorithm
/// used by the initial Taffy layout.
///
/// Balancing chooses contiguous item sequences from their hypothetical outer
/// main sizes, but each selected sequence then needs its own flexible-length
/// resolution. Keeping the Taffy item styles intact preserves flex factors,
/// automatic minimum sizes, min/max clamping, auto margins, and
/// `justify-content`; only wrapping is disabled because the caller supplies
/// exactly one line:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance> and
/// <https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths>.
pub(super) struct BalancedTaffyLayoutContext<'a> {
    pub(super) template_tree: &'a taffy_layout::TaffyTree<FlexItemEstimate>,
    pub(super) template_root: taffy_layout::NodeId,
    pub(super) template_nodes: &'a [taffy_layout::NodeId],
    pub(super) estimates: &'a [FlexItemEstimate],
    pub(super) flex_axes: FlexAxes,
    pub(super) available: FlexAvailableSpace,
}

pub(super) fn balanced_taffy_line_layouts(
    context: &BalancedTaffyLayoutContext<'_>,
    item_indices: &[usize],
    hypothetical_sizes: bool,
) -> Option<Vec<FlexItemLayout>> {
    let mut tree = taffy_layout::TaffyTree::new();
    tree.disable_rounding();
    let mut nodes = Vec::with_capacity(item_indices.len());
    for &item_index in item_indices {
        let template_node = *context.template_nodes.get(item_index)?;
        let estimate = *context.estimates.get(item_index)?;
        let mut style = context.template_tree.style(template_node).ok()?.clone();
        if hypothetical_sizes {
            style.flex_grow = 0.0;
            style.flex_shrink = 0.0;
        }
        nodes.push(tree.new_leaf_with_context(style, estimate).ok()?);
    }

    let mut root_style = context
        .template_tree
        .style(context.template_root)
        .ok()?
        .clone();
    root_style.flex_wrap = taffy_layout::FlexWrap::NoWrap;
    // The template root was built for the full container.  A balanced plan
    // resolves one no-wrap Taffy tree per final line, so its cross axis must
    // instead use the same reserved line slot that was used to estimate the
    // items.  Child percentage dimensions have already been resolved against
    // their original container bases while constructing the template styles;
    // this changes only the line's layout constraint.
    // <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
    root_style.size.width = taffy_layout::Dimension::length(context.available.width.points());
    root_style.size.height = context
        .available
        .height_constraint()
        .map(PhysicalContentHeight::points)
        .map(taffy_layout::Dimension::length)
        .unwrap_or_else(taffy_layout::Dimension::auto);
    let root = tree.new_with_children(root_style, &nodes).ok()?;
    tree.compute_layout_with_measure(
        root,
        taffy_layout::Size {
            width: taffy_layout::AvailableSpace::Definite(context.available.width.points()),
            height: context
                .available
                .height
                .map(PhysicalContentHeight::points)
                .map(taffy_layout::AvailableSpace::Definite)
                .unwrap_or(taffy_layout::AvailableSpace::MaxContent),
        },
        |input, _node_id, node_context, _style| taffy_flex_measurement(input, node_context),
    )
    .ok()?;
    nodes
        .iter()
        .map(|&node| {
            tree.layout(node)
                .ok()
                .map(taffy_rect_from_layout)
                .map(FlexItemLayout::from_taffy_rect)
        })
        .collect()
}

/// Replace the selected balanced lines' main-axis geometry with a fresh flex
/// resolution for each line.
///
/// This deliberately keeps the cross-axis geometry selected by the outer flex
/// layout. The balancing phase changes only line membership; cross sizing and
/// line packing continue through the regular flex pipeline afterward:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance>.
pub(super) fn resolve_balanced_line_flexible_lengths(
    context: &BalancedTaffyLayoutContext<'_>,
    lines: &[FlexLineLayout],
    items: &mut [FlexItemLayout],
) {
    for line in lines {
        let Some(layouts) = balanced_taffy_line_layouts(context, &line.item_indices, false) else {
            continue;
        };
        for (&item_index, layout) in line.item_indices.iter().zip(layouts) {
            let Some(item) = items.get_mut(item_index) else {
                continue;
            };
            item.set_main_size(context.flex_axes, layout.main_size(context.flex_axes));
            item.set_main_start(context.flex_axes, layout.main_start(context.flex_axes));
            // This no-wrap tree is the final balanced line, not a probe.
            // Preserve its resolved cross size as well as its flexible main
            // size so downstream line measurement and CSS Align placement do
            // not retain a full-container normal-wrap box.
            item.set_cross_size(context.flex_axes, layout.cross_size(context.flex_axes));
        }
    }
}

/// Record the source block extent that fragmented replay must cover.
///
/// Flex layout keeps an item's used border-box height even when descendants
/// visibly overflow it. Fragmentation must nevertheless keep replaying the
/// item until that descendant content has been consumed by fragmentainers:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination> and
/// <https://www.w3.org/TR/css-break-3/#box-splitting>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexBalanceContext<'a> {
    pub(in crate::layout::flex) physical_direction: FlexDirection,
    pub(in crate::layout::flex) minimum_line_count: usize,
    pub(in crate::layout::flex) hypothetical_main_sizes: Option<&'a [FlexMainSize]>,
    pub(in crate::layout::flex) main_gap: FlexMainSize,
    pub(in crate::layout::flex) cross_gap: FlexCrossSize,
    /// Equal cross-axis slot reserved by an explicit balanced line count.
    /// It is a layout constraint, not a replacement percentage basis.
    pub(in crate::layout::flex) reserved_line_cross_size: Option<FlexCrossSize>,
    pub(in crate::layout::flex) available_main_size: FlexMainSize,
}

/// The complete Level 2 balance topology selected before final flex-line
/// sizing and placement.  Its item contributions are margin-inclusive
/// hypothetical outer main sizes, while its partitions retain source order.
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance>
#[derive(Debug, Clone)]
pub(in crate::layout::flex) struct BalancedFlexLinePlan {
    pub(in crate::layout::flex) partitions: Vec<Vec<usize>>,
    pub(in crate::layout::flex) outer_main_sizes: Vec<FlexMainSize>,
    pub(in crate::layout::flex) main_gap: FlexMainSize,
    pub(in crate::layout::flex) cross_gap: FlexCrossSize,
    pub(in crate::layout::flex) reserved_line_cross_size: Option<FlexCrossSize>,
    pub(in crate::layout::flex) available_main_size: FlexMainSize,
}

/// Select the final source-order topology for a balanced flex container.
pub(in crate::layout::flex) fn balanced_flex_line_plan(
    lines: &[FlexLineLayout],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    context: FlexBalanceContext<'_>,
) -> Option<BalancedFlexLinePlan> {
    if lines.is_empty() || !context.available_main_size.is_finite() {
        return None;
    }
    let ordered_items = lines
        .iter()
        .flat_map(|line| line.item_indices.iter().copied())
        .collect::<Vec<_>>();
    if ordered_items.is_empty() {
        return None;
    }
    let outer_main_sizes = ordered_items
        .iter()
        .map(|&index| {
            let item_main_size = context
                .hypothetical_main_sizes
                .and_then(|sizes| sizes.get(index))
                .copied()
                .unwrap_or_else(|| item_main_size(&items[index], context.physical_direction));
            // CSS Flexbox Level 2 floors the margin-inclusive hypothetical
            // outer size only at the line-breaking boundary.
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
    let line_count = lines
        .len()
        .max(inferred_line_count)
        .max(context.minimum_line_count)
        .clamp(1, ordered_items.len());
    let partitions = balanced_flex_line_partitions(
        &ordered_items,
        &outer_main_sizes,
        line_count,
        context.main_gap,
        context.available_main_size,
    )?;
    Some(BalancedFlexLinePlan {
        partitions,
        outer_main_sizes,
        main_gap: context.main_gap,
        cross_gap: context.cross_gap,
        reserved_line_cross_size: context.reserved_line_cross_size,
        available_main_size: context.available_main_size,
    })
}

pub(in crate::layout::flex) fn rebalance_flex_line_membership(
    lines: &mut Vec<FlexLineLayout>,
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    context: FlexBalanceContext<'_>,
) -> bool {
    let Some(plan) = balanced_flex_line_plan(lines, items, children, context) else {
        return false;
    };
    debug_assert_eq!(plan.main_gap, context.main_gap);
    debug_assert_eq!(plan.available_main_size, context.available_main_size);
    debug_assert_eq!(
        plan.partitions.iter().map(Vec::len).sum::<usize>(),
        plan.outer_main_sizes.len(),
    );
    if let Some(line_cross_size) = plan.reserved_line_cross_size {
        // `flex-line-count` reserves final equal slots before the item
        // measurements that selected this plan. Rebuild those slots from the
        // plan rather than inheriting normal-wrap geometry from Taffy.
        // <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
        let Some(first_cross_start) = lines
            .iter()
            .map(|line| line.cross_start)
            .min_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
        else {
            return false;
        };
        *lines = plan
            .partitions
            .iter()
            .enumerate()
            .map(|(index, item_indices)| {
                let cross_start =
                    first_cross_start + (line_cross_size + plan.cross_gap).scale(index as f32);
                let source_start = item_indices.iter().copied().min().unwrap_or(0);
                let source_end = item_indices
                    .iter()
                    .copied()
                    .max()
                    .map(|item_index| item_index + 1)
                    .unwrap_or(source_start);
                FlexLineLayout {
                    item_indices: item_indices.clone(),
                    logical_cross_start_rank: index,
                    source_start,
                    source_end,
                    main_start: FlexMainOffset::new(0.0),
                    main_end: FlexMainOffset::new(0.0),
                    cross_start,
                    cross_end: cross_start + line_cross_size,
                    first_baseline: None,
                    last_baseline: None,
                    collapsed_struts: Vec::new(),
                }
            })
            .collect();
    } else {
        while lines.len() < plan.partitions.len() {
            let Some(last_line) = lines.last().cloned() else {
                return false;
            };
            let cross_size = last_line.cross_size();
            let cross_start = last_line.cross_end + plan.cross_gap;
            lines.push(FlexLineLayout {
                item_indices: Vec::new(),
                logical_cross_start_rank: lines.len(),
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
        lines.truncate(plan.partitions.len());
    }

    let axes = PhysicalFlexDirection::new(context.physical_direction);
    let slots = lines
        .iter()
        .map(|line| (line.cross_start, line.cross_end))
        .collect::<Vec<_>>();
    let mut changed = false;
    for ((line, item_indices), (cross_start, cross_end)) in
        lines.iter_mut().zip(plan.partitions).zip(slots)
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
pub(in crate::layout::flex) fn balanced_flex_line_count(
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
pub(in crate::layout::flex) fn balanced_flex_line_partitions(
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

    // Balancing costs are dimensionless ordering scores at the dynamic
    // programming boundary. Use f64 so equivalent partitions remain tied
    // after PDF-point layout values have passed through several f32 adapters.
    // A tiny absolute tolerance then preserves the Level 2 start-biased tie
    // break instead of letting summation order choose the final line.
    const COST_TIE_EPSILON: f64 = 1e-8;
    let mut costs = vec![vec![f64::INFINITY; item_count + 1]; line_count + 1];
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
                let candidate = costs[line - 1][start] + f64::from(error.points()).powi(2);
                // Prefer the later break for equal errors. During reverse
                // reconstruction, this gives the draft algorithm's start bias:
                // assign as many items as possible to earlier lines.
                if candidate <= costs[line][end] + COST_TIE_EPSILON {
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
