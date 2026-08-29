use super::*;
use crate::document::paint::geometry::AxisSelectivePaintClip;
use crate::layout::flex::alignment::{effective_align_self, flex_item_has_auto_cross_margin};

mod model;
pub(in crate::layout::flex) use self::model::*;
pub(in crate::layout::flex) fn should_move_flex_container_to_next_page(
    block_top: PageTopBlockPosition,
    margin_block_start: LayoutLength,
    total_height: LayoutLength,
    page_top: PageTopBlockPosition,
    page_bottom: PageTopBlockPosition,
    page_area_height: LayoutLength,
) -> bool {
    let overflows_current_page = block_top.toward_block_end(total_height) < page_bottom;
    // `block_top` is the flex border-box start after normal-flow placement
    // has consumed the physical top margin.  Whole-box fragmentation must
    // instead ask whether the *margin box* starts at the fragmentainer edge:
    // otherwise a top margin makes an otherwise fitting box repeatedly move
    // to the next page and re-enter this layout path forever.
    // <https://www.w3.org/TR/css-break-3/#break-between>
    let margin_box_top = block_top.toward_block_start(margin_block_start);
    let starts_at_page_top = (margin_box_top.points() - page_top.points()).abs() < 0.01;
    overflows_current_page
        && !starts_at_page_top
        && total_height.points() <= page_area_height.points() + 0.01
}

/// Whether a flex container may create a whole-box page break before its
/// flex-item fragmentation is considered.
///
/// Isolated sizing replays deliberately suppress fragmentation: their purpose
/// is to measure one unfragmented formatting context.  Letting a flex
/// container prebreak in that scope both changes the measured size and can
/// repeatedly clone the replay's page state for floated orthogonal flexboxes.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
pub(in crate::layout::flex) fn flex_container_allows_whole_box_prebreak(
    fragmentainer_kind: FragmentainerKind,
    fragmentation_suppression_depth: usize,
    has_forced_item_breaks: bool,
) -> bool {
    fragmentainer_kind != FragmentainerKind::Column
        && fragmentation_suppression_depth == 0
        && !has_forced_item_breaks
}

/// Returns the physical block extent occupied by a fragmented single-line row
/// flex container.
///
/// Each continuation reruns cross-axis alignment in its own fragmentainer.
/// The flex container consequently occupies the complete content box of its
/// final continuation even when the remaining source content is shorter; a
/// stretched item shares that extent. This helper maps an unfragmented source
/// cross-size to those fragment-local content-box extents without guessing at
/// individual test geometry:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
pub(in crate::layout::flex) fn single_line_row_fragmented_cross_size(
    source_cross_size: FlexCrossSize,
    first_fragment_capacity: FlexFragmentBlockSize,
    continuation_fragment_capacity: FlexFragmentBlockSize,
) -> Option<FlexCrossSize> {
    if source_cross_size.points() <= first_fragment_capacity.points() + 0.01
        || continuation_fragment_capacity.points() <= 0.01
    {
        return None;
    }
    let continuation_count = ((source_cross_size.points() - first_fragment_capacity.points())
        / continuation_fragment_capacity.points())
    .ceil()
    .max(1.0);
    Some(FlexCrossSize::new(
        first_fragment_capacity.points()
            + continuation_count * continuation_fragment_capacity.points(),
    ))
}

/// Resolve the final fragmentainer boundary occupied by one flex item.
///
/// An item that crosses a fragmentainer boundary gains the remaining span of
/// its final fragmentainer. This is separate from the flex container's own
/// source size: a definite-height container can retain its used height while
/// an overflowing item's fragmented source extent continues.
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
pub(in crate::layout::flex) fn fragmented_flex_item_block_end(
    item_start: FlexFragmentBlockOffset,
    item_end: FlexFragmentBlockOffset,
    first_fragment_capacity: FlexFragmentBlockSize,
    continuation_fragment_capacity: FlexFragmentBlockSize,
) -> Option<FlexFragmentBlockOffset> {
    let first_capacity = first_fragment_capacity.points();
    let continuation_capacity = continuation_fragment_capacity.points();
    if item_end.points() <= first_capacity + 0.01 || continuation_capacity <= 0.01 {
        return None;
    }
    let crossed_boundary = if item_start.points() < first_capacity - 0.01 {
        first_capacity
    } else {
        let continuation_index = ((item_start.points() - first_capacity) / continuation_capacity)
            .floor()
            .max(0.0);
        first_capacity + (continuation_index + 1.0) * continuation_capacity
    };
    if item_end.points() <= crossed_boundary + 0.01 {
        return None;
    }
    let final_continuation_count = ((item_end.points() - first_capacity) / continuation_capacity)
        .ceil()
        .max(1.0);
    Some(FlexFragmentBlockOffset::new(
        first_capacity + final_continuation_count * continuation_capacity,
    ))
}

/// Expand wrapped physical-column flex items through their final committed
/// fragmentainer span.
///
/// A wrapped column line owns an independent main-axis sequence. When one of
/// its items crosses a fragmentainer boundary, its used block size acquires
/// the remaining span of the final fragmentainer; otherwise its background
/// and border stop at the original source tail while the container continues
/// into the next column. Later items in that same committed flex line advance
/// in its source main-axis sequence; overlapping cross-axis lines remain
/// independent.
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
/// <https://www.w3.org/TR/css-break-3/#box-splitting>
pub(in crate::layout::flex) fn expand_wrapped_column_items_through_fragmentainers(
    items: &mut [FlexItemLayout],
    lines: &[FlexLineLayout],
    first_fragment_capacity: FlexFragmentBlockSize,
    continuation_fragment_capacity: FlexFragmentBlockSize,
) -> bool {
    let mut expanded = false;
    for line in lines {
        for (line_position, &item_index) in line.item_indices.iter().enumerate() {
            let item_bounds = flex_item_block_bounds(&items[item_index], true);
            let Some(final_block_end) = fragmented_flex_item_block_end(
                item_bounds.start(),
                item_bounds.end(),
                first_fragment_capacity,
                continuation_fragment_capacity,
            ) else {
                continue;
            };
            let expansion = final_block_end - item_bounds.end();
            if expansion.points() <= 0.01 {
                continue;
            }
            let expanded_height = (final_block_end - item_bounds.start()).non_negative_size();
            items[item_index].set_height(FlexPhysicalVerticalSize::new(expanded_height.points()));
            items[item_index].set_fragmentation_height(PhysicalContentHeight::new(content_box_pt(
                expanded_height.points(),
            )));
            let item_y = items[item_index].y().points();
            for &following_index in &line.item_indices[line_position + 1..] {
                if items[following_index].y().points() >= item_y - 0.01 {
                    items[following_index].set_y(FlexPhysicalVerticalOffset::new(
                        items[following_index].y().points() + expansion.points(),
                    ));
                }
            }
            expanded = true;
        }
    }
    expanded
}

/// Whether an auto-height row flex item is stretched in a continuation
/// fragment. Auto cross-axis margins suppress stretch per CSS Flexbox, while
/// `normal` computes to stretch for flex items:
/// <https://www.w3.org/TR/css-flexbox-1/#align-items-property>.
pub(in crate::layout::flex) fn row_flex_item_stretches_in_fragment(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> bool {
    child_style.box_values.height.is_auto()
        && !flex_item_has_auto_cross_margin(child_style, FlexDirection::Row)
        && matches!(
            effective_align_self(child_style, container_style).keyword,
            SelfAlignmentKeyword::Auto
                | SelfAlignmentKeyword::Normal
                | SelfAlignmentKeyword::Stretch
        )
}

pub(in crate::layout::flex) fn flex_break_units(
    fragmentainer_kind: FragmentainerKind,
    flex_layout: &FlexLayout,
    children: &[StyledChild<'_>],
    style: &ComputedStyle,
    use_fragmentation_height: bool,
) -> Vec<FlexBreakUnit> {
    let boundary_projection = FlexFragmentationBoundaryProjection::for_style(style);
    if boundary_projection == FlexFragmentationBoundaryProjection::LineCrossAxis {
        let mut units = flex_layout
            .lines
            .iter()
            .enumerate()
            .filter_map(|(line_index, line)| {
                let item_indices = line
                    .item_indices
                    .iter()
                    .cloned()
                    .filter(|&index| {
                        children
                            .get(index)
                            .is_some_and(|child| !flex_item_is_collapsed(&child.style))
                    })
                    .collect::<Vec<_>>();
                let line_block_bounds = boundary_projection.line_cross_block_bounds(line);
                let block_start = line_block_bounds.start();
                let mut block_end = line_block_bounds.end();
                if use_fragmentation_height {
                    for &item_index in &item_indices {
                        let item_bounds =
                            flex_item_block_bounds(&flex_layout.items[item_index], true);
                        // A negative cross-axis margin is paint overflow from
                        // the flex line, not an earlier fragmentable source
                        // range. Moving the line start to that margin would
                        // shift every ordinary stretched peer down in the
                        // first fragment. The line itself owns its source
                        // start; item overflow can only extend its end.
                        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                        if item_bounds.end().points() > block_end.points() {
                            block_end = item_bounds.end();
                        }
                    }
                }
                (!item_indices.is_empty()).then(|| FlexBreakUnit {
                    topology: FlexReplayTopology::Fragmented,
                    line_start: line_index,
                    line_end: line_index + 1,
                    block_start,
                    block_end,
                    break_before: flex_unit_break_before(
                        fragmentainer_kind,
                        &item_indices,
                        children,
                    ),
                    break_after: flex_unit_break_after(fragmentainer_kind, &item_indices, children),
                    break_inside_avoid: item_indices.iter().any(|&index| {
                        fragmentainer_kind.avoids_break_inside(&children[index].style)
                    }),
                    item_indices,
                })
            })
            .collect::<Vec<_>>();
        units.sort_by(|a, b| {
            a.block_start
                .partial_cmp(&b.block_start)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if use_fragmentation_height
            && units.iter().enumerate().any(|(index, unit)| {
                units[index + 1..]
                    .iter()
                    .any(|later| unit.block_end.points() > later.block_start.points() + 0.01)
            })
        {
            let mut boundaries = units
                .iter()
                .flat_map(|unit| [unit.block_start.points(), unit.block_end.points()])
                .collect::<Vec<_>>();
            boundaries.sort_by(|left, right| {
                left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
            });
            boundaries.dedup_by(|left, right| (*left - *right).abs() <= 0.01);
            let mut partitions = Vec::new();
            for pair in boundaries.windows(2) {
                let block_start = FlexFragmentBlockOffset::new(pair[0]);
                let block_end = FlexFragmentBlockOffset::new(pair[1]);
                let active = units
                    .iter()
                    .filter(|unit| {
                        unit.block_start.points() < block_end.points() - 0.01
                            && unit.block_end.points() > block_start.points() + 0.01
                    })
                    .collect::<Vec<_>>();
                let mut item_indices = active
                    .iter()
                    .flat_map(|unit| unit.item_indices.iter().copied())
                    .collect::<Vec<_>>();
                item_indices.sort_unstable();
                item_indices.dedup();
                if item_indices.is_empty() {
                    continue;
                }
                partitions.push(FlexBreakUnit {
                    topology: FlexReplayTopology::Fragmented,
                    line_start: active.iter().map(|unit| unit.line_start).min().unwrap_or(0),
                    line_end: active.iter().map(|unit| unit.line_end).max().unwrap_or(0),
                    block_start,
                    block_end,
                    break_before: PageBreak::Auto,
                    break_after: PageBreak::Auto,
                    break_inside_avoid: active.iter().any(|unit| unit.break_inside_avoid),
                    item_indices,
                });
            }
            return partitions;
        }
        return units;
    }

    // Main-axis item intervals can overlap in the physical block direction:
    // each
    // line has its own physical horizontal cross-axis position while its
    // items share some of the vertical main-axis range. Partition that axis
    // at every item edge and emit the set of items active in each interval.
    // Grouping only equal item ranges loses the shorter line at a fragment
    // boundary, while serializing lines incorrectly consumes a separate
    // fragmentainer for each cross-axis line.
    // <https://www.w3.org/TR/css-flexbox-1/#pagination>
    let mut item_ranges = Vec::new();
    let mut boundaries = Vec::new();
    for (index, item) in flex_layout.items.iter().enumerate() {
        if flex_item_is_collapsed(&children[index].style) {
            continue;
        }
        let block_bounds =
            boundary_projection.item_main_block_bounds(item, use_fragmentation_height);
        if block_bounds.end().points() <= block_bounds.start().points() + 0.01 {
            continue;
        }
        boundaries.push(block_bounds.start().points());
        boundaries.push(block_bounds.end().points());
        item_ranges.push((index, block_bounds));
    }
    boundaries.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    boundaries.dedup_by(|left, right| (*left - *right).abs() <= 0.01);

    let mut units = Vec::new();
    for boundary_pair in boundaries.windows(2) {
        let block_start = FlexFragmentBlockOffset::new(boundary_pair[0]);
        let block_end = FlexFragmentBlockOffset::new(boundary_pair[1]);
        if block_end.points() <= block_start.points() + 0.01 {
            continue;
        }
        let item_indices = item_ranges
            .iter()
            .filter_map(|(index, bounds)| {
                (bounds.start().points() < block_end.points() - 0.01
                    && bounds.end().points() > block_start.points() + 0.01)
                    .then_some(*index)
            })
            .collect::<Vec<_>>();
        if item_indices.is_empty() {
            continue;
        }
        let starts_at_boundary = item_ranges
            .iter()
            .filter_map(|(index, bounds)| {
                ((bounds.start() - block_start).abs() <= 0.01).then_some(*index)
            })
            .collect::<Vec<_>>();
        let ends_at_boundary = item_ranges
            .iter()
            .filter_map(|(index, bounds)| {
                ((bounds.end() - block_end).abs() <= 0.01).then_some(*index)
            })
            .collect::<Vec<_>>();
        let (line_start, line_end) =
            item_indices
                .iter()
                .fold((usize::MAX, 0usize), |(line_start, line_end), &index| {
                    let (item_line_start, item_line_end) = flex_item_line_range(flex_layout, index);
                    (line_start.min(item_line_start), line_end.max(item_line_end))
                });
        let break_inside_avoid = item_indices
            .iter()
            .any(|&index| fragmentainer_kind.avoids_break_inside(&children[index].style));
        units.push(FlexBreakUnit {
            topology: FlexReplayTopology::Fragmented,
            item_indices,
            line_start,
            line_end,
            break_before: flex_unit_break_before(fragmentainer_kind, &starts_at_boundary, children),
            break_after: flex_unit_break_after(fragmentainer_kind, &ends_at_boundary, children),
            break_inside_avoid,
            block_start,
            block_end,
        });
    }
    units
}

/// Builds the sole replay unit for a flex container that remains in one
/// fragmentainer.
///
/// Flex lines can overlap on the physical block axis (notably wrapped
/// physical columns), but that is ordinary two-dimensional flex placement,
/// not fragmentation. Replaying them through physical block intervals would
/// turn a `column-reverse` container into an incorrectly ordered source
/// sequence. The line membership is already order-modified, so flatten it
/// without sorting by physical geometry.
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
pub(in crate::layout::flex) fn unfragmented_flex_container_break_unit(
    flex_layout: &FlexLayout,
    children: &[StyledChild<'_>],
    total_content_height: LayoutLength,
) -> FlexBreakUnit {
    let mut item_indices = Vec::with_capacity(flex_layout.items.len());
    let mut included = vec![false; flex_layout.items.len()];
    for line in &flex_layout.lines {
        for &item_index in &line.item_indices {
            if !included[item_index] && !flex_item_is_collapsed(&children[item_index].style) {
                included[item_index] = true;
                item_indices.push(item_index);
            }
        }
    }
    // A zero-sized or otherwise exceptional item can have final geometry
    // without appearing in a line record. It remains an in-flow flex item and
    // must have exactly one replay opportunity.
    for (item_index, child) in children.iter().enumerate() {
        if !included[item_index] && !flex_item_is_collapsed(&child.style) {
            item_indices.push(item_index);
        }
    }
    FlexBreakUnit {
        topology: FlexReplayTopology::Unfragmented,
        item_indices,
        line_start: 0,
        line_end: flex_layout.lines.len(),
        block_start: FlexFragmentBlockOffset::new(0.0),
        block_end: FlexFragmentBlockOffset::new(total_content_height.points()),
        break_before: PageBreak::Auto,
        break_after: PageBreak::Auto,
        break_inside_avoid: false,
    }
}

/// Builds the fragmentation units for a flex container, including its own
/// generated box when it has no in-flow flex items.
///
/// An empty flex container still has a fragmentable principal box: its
/// background, border, padding, and definite block-size are not conditional on
/// producing a flex line. Keeping this invariant at the unit-construction
/// boundary ensures a later fragmented-layout recomputation cannot discard
/// the container's paint range.
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
pub(in crate::layout::flex) fn flex_container_break_units(
    fragmentainer_kind: FragmentainerKind,
    flex_layout: &FlexLayout,
    children: &[StyledChild<'_>],
    style: &ComputedStyle,
    use_fragmentation_height: bool,
    total_content_height: LayoutLength,
) -> Vec<FlexBreakUnit> {
    let mut units = flex_break_units(
        fragmentainer_kind,
        flex_layout,
        children,
        style,
        use_fragmentation_height,
    );
    // A line can exist solely to hold zero-sized flex items. It has no
    // fragmentable block range and therefore cannot substitute for the
    // flex container's own fixed-height principal box. Without removing
    // such units, the fallback below never creates the container slices
    // that own its background, border, and padding in later columns/pages.
    // <https://www.w3.org/TR/css-flexbox-1/#pagination>
    // <https://www.w3.org/TR/css-break-3/#box-splitting>
    units.retain(|unit| unit.block_end.points() > unit.block_start.points() + 0.01);
    if units.is_empty() && total_content_height > layout_pt(0.01) {
        units.push(FlexBreakUnit {
            topology: FlexReplayTopology::Fragmented,
            item_indices: Vec::new(),
            line_start: 0,
            line_end: 0,
            block_start: FlexFragmentBlockOffset::new(0.0),
            block_end: FlexFragmentBlockOffset::new(total_content_height.points()),
            break_before: PageBreak::Auto,
            break_after: PageBreak::Auto,
            break_inside_avoid: false,
        });
    }
    // A single flex break unit represents the entire flex container,
    // including an empty fixed-height flex container. Preserve a container
    // `break-inside: avoid` at this sole break opportunity.
    // <https://www.w3.org/TR/css-break-3/#break-within>
    if units.len() == 1 && fragmentainer_kind.avoids_break_inside(style) {
        units[0].break_inside_avoid = true;
    }
    units
}

/// Visible descendant overflow from a flex container's used content box.
///
/// The source end is distinct from the container's used block size: it can
/// require a later page/column continuation without changing the principal
/// flex box that participates in its parent formatting context.
/// <https://drafts.csswg.org/css-flexbox-1/#pagination>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::flex) struct FlexVisibleOverflow {
    source_block_end: FlexFragmentBlockOffset,
    used_content_block_size: LayoutLength,
}

impl FlexVisibleOverflow {
    pub(in crate::layout::flex) fn new(
        source_block_end: FlexFragmentBlockOffset,
        used_content_block_size: LayoutLength,
    ) -> Self {
        debug_assert!(source_block_end.points() > used_content_block_size.points() + 0.01);
        Self {
            source_block_end,
            used_content_block_size,
        }
    }

    /// Whether the visual-overflow source interval reaches this fragmentainer.
    ///
    /// A local overflow interval inside a page/column is ordinary visual
    /// overflow, not a flex container fragment.  Fragmentation begins only
    /// once that source interval reaches a fragmentainer boundary.
    pub(in crate::layout::flex) fn reaches_fragmentainer(
        self,
        available_content_block_size: FlexFragmentBlockSize,
    ) -> bool {
        debug_assert!(self.source_block_end.points() > self.used_content_block_size.points());
        self.source_block_end.points() > available_content_block_size.points() + 0.01
    }
}

/// Return the visible descendant source extent that may require a flex
/// continuation after the container's used block box.
///
/// Wrapped physical rows, physical columns, and nested flex items can retain
/// source content beyond the used flex box.  The caller must still test the
/// resulting source extent against the actual fragmentainer before enabling
/// flex fragmentation.
/// <https://drafts.csswg.org/css-flexbox-1/#pagination>
pub(in crate::layout::flex) fn flex_visible_overflow(
    flex_layout: &FlexLayout,
    children: &[StyledChild<'_>],
    style: &ComputedStyle,
    used_content_block_size: LayoutLength,
) -> Option<FlexVisibleOverflow> {
    let source_block_end = flex_layout
        .items
        .iter()
        .zip(children)
        .filter_map(|(item, child)| {
            let source_end = flex_item_block_bounds(item, true).end();
            let extends_used_box = source_end.points() > used_content_block_size.points() + 0.01;
            let may_continue = physical_flex_direction(style).is_column_axis()
                || (physical_flex_direction(style).is_row_axis() && style.flex_wrap.wraps())
                || child.style.display.is_flex();
            (extends_used_box && may_continue).then_some(source_end)
        })
        .max_by(|left, right| left.points().total_cmp(&right.points()))?;
    Some(FlexVisibleOverflow::new(
        source_block_end,
        used_content_block_size,
    ))
}

/// Return whether a flex container's visible overflow is clipped on the
/// physical block axis used by page and column fragmentainers.
///
/// A horizontal-only clip must not suppress a vertical continuation. Paint
/// containment clips both axes.
/// <https://drafts.csswg.org/css-overflow-3/#overflow-properties>
pub(in crate::layout::flex) fn flex_overflow_is_clipped_in_fragmentation_axis(
    overflow_axes: UsedOverflowAxes,
    paint_containment_applies: bool,
) -> bool {
    overflow_axes.clips_y() || paint_containment_applies
}

/// Split an exhausted zero-height column's physical row into its atomic item
/// overflow subjects.
///
/// The one-CSS-pixel capacity used to guarantee fragmentation progress is not
/// a usable line-fragmentation space. Each atomic item therefore keeps its
/// own source interval: the first one overflows the originating column, while
/// a later item may take the next anonymous column at its ordinary flex-item
/// boundary. Treating the entire row as one atomic line would move every item
/// together and changes their committed inline positions.
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>
pub(in crate::layout::flex) fn flex_zero_capacity_column_item_break_units(
    flex_layout: &FlexLayout,
    children: &[StyledChild<'_>],
    style: &ComputedStyle,
) -> Vec<FlexBreakUnit> {
    debug_assert!(physical_flex_direction(style).is_row_axis());
    let mut units = flex_layout
        .items
        .iter()
        .enumerate()
        .filter(|(item_index, _)| !flex_item_is_collapsed(&children[*item_index].style))
        .map(|(item_index, item)| {
            let bounds = flex_item_block_bounds(item, true);
            let (line_start, line_end) = flex_item_line_range(flex_layout, item_index);
            FlexBreakUnit {
                topology: FlexReplayTopology::Fragmented,
                item_indices: vec![item_index],
                line_start,
                line_end,
                block_start: bounds.start(),
                block_end: bounds.end(),
                break_before: flex_unit_break_before(
                    FragmentainerKind::Column,
                    &[item_index],
                    children,
                ),
                break_after: flex_unit_break_after(
                    FragmentainerKind::Column,
                    &[item_index],
                    children,
                ),
                break_inside_avoid: FragmentainerKind::Column
                    .avoids_break_inside(&children[item_index].style),
            }
        })
        .filter(|unit| unit.block_end.points() > unit.block_start.points() + 0.01)
        .collect::<Vec<_>>();
    units.sort_by(|left, right| {
        left.block_start
            .partial_cmp(&right.block_start)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.item_indices.cmp(&right.item_indices))
    });
    units
}

/// Whether an item's nested normal-flow formatting boxes own a forced break.
///
/// A flex container plans boundaries between its own flex units, but it must
/// leave an independently forced descendant boundary to the descendant's
/// formatting context. In particular, an auto-sized row that merely touches a
/// column boundary cannot synthesize an empty continuation around that nested
/// boundary:
/// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
pub(in crate::layout::flex) fn flex_item_contents_have_forced_break_in(
    boxes: &[box_tree::FormattingBox<'_>],
    fragmentainer_kind: FragmentainerKind,
) -> bool {
    boxes.iter().any(|box_| {
        box_.element_parts().is_some_and(|(_, _, style, children)| {
            fragmentainer_kind.is_forced_break(style.break_before)
                || fragmentainer_kind.is_forced_break(style.break_after)
                || flex_item_contents_have_forced_break_in(children, fragmentainer_kind)
        }) || flex_item_contents_have_forced_break_in(box_.children(), fragmentainer_kind)
    })
}

pub(in crate::layout::flex) fn flex_fragment_from_break_unit(
    unit: &FlexBreakUnit,
    flex_layout: &FlexLayout,
    context: FlexFragmentBuildContext,
    use_fragmentation_height: bool,
) -> FlexFragmentLayout {
    let fragment_height = unit.block_size();
    let fragment_bottom = context
        .content_top
        .toward_block_end(layout_pt((unit.block_end - context.block_offset).points()))
        .points();
    let items = unit
        .item_indices
        .iter()
        .filter_map(|&item_index| {
            let item = flex_layout.items.get(item_index)?;
            let line_index = flex_layout
                .lines
                .iter()
                .position(|line| line.item_indices.contains(&item_index))
                .unwrap_or(unit.line_start);
            let used_bounds = item.clone();
            let mut source_bounds = used_bounds.clone();
            if use_fragmentation_height {
                source_bounds.set_height(FlexPhysicalVerticalSize::new(
                    item.fragmentation_source_height().points(),
                ));
            }
            let (bounds, content_slice, replay_origin) = match unit.topology {
                FlexReplayTopology::Unfragmented => (
                    used_bounds.clone(),
                    FlexFragmentSlice {
                        block_start: FlexFragmentBlockOffset::new(0.0),
                        block_end: FlexFragmentBlockOffset::new(used_bounds.height().points()),
                    },
                    FlexItemReplayOrigin::ChildFragment,
                ),
                FlexReplayTopology::Fragmented => {
                    let item_block_bounds = flex_item_block_bounds(item, use_fragmentation_height);
                    let item_block_start = item_block_bounds.start().points();
                    let item_block_end = item_block_bounds.end().points();
                    let slice_start = item_block_start.max(unit.block_start.points());
                    let slice_end = item_block_end.min(unit.block_end.points());
                    if slice_end <= slice_start + 0.01 {
                        return None;
                    }
                    let mut bounds = source_bounds.clone();
                    bounds.set_y(FlexPhysicalVerticalOffset::new(slice_start));
                    bounds.set_height(FlexPhysicalVerticalSize::new(
                        (slice_end - slice_start).max(0.0),
                    ));
                    let destination_slice = FlexFragmentSlice {
                        block_start: FlexFragmentBlockOffset::new(
                            (slice_start - item_block_start).max(0.0),
                        ),
                        block_end: FlexFragmentBlockOffset::new(
                            (slice_end - item_block_start).min(item_block_end - item_block_start),
                        ),
                    };
                    let content_slice = item
                        .source_slice_for_destination_slice(destination_slice)
                        .unwrap_or(destination_slice);
                    // The selected interval, rather than the source box's
                    // current height, identifies descendant-overflow replay.
                    // <https://www.w3.org/TR/css-flexbox-1/#pagination>
                    // <https://www.w3.org/TR/css-break-3/#box-splitting>
                    let replay_origin = if content_slice.block_end.points()
                        > used_bounds.height().points() + 0.01
                    {
                        FlexItemReplayOrigin::SourceSlice
                    } else {
                        FlexItemReplayOrigin::ChildFragment
                    };
                    (bounds, content_slice, replay_origin)
                }
            };
            // Descendant overflow can outlive the flex item's used border
            // box. Preserve its source content range, but project box
            // decoration independently onto the used border box so a
            // trailing item border never becomes a synthetic overflow
            // continuation.
            // <https://www.w3.org/TR/css-break-3/#break-decoration>
            let used_border_box_end = FlexFragmentBlockOffset::new(used_bounds.height().points());
            let decoration_slice = FlexFragmentSlice {
                block_start: FlexFragmentBlockOffset::new(
                    content_slice
                        .block_start
                        .points()
                        .min(used_border_box_end.points()),
                ),
                block_end: FlexFragmentBlockOffset::new(
                    content_slice
                        .block_end
                        .points()
                        .min(used_border_box_end.points()),
                ),
            };
            Some(FlexItemFragmentLayout {
                item_index,
                source_item_index: item_index,
                line_index,
                source_bounds,
                used_bounds,
                bounds,
                content_slice,
                decoration_slice,
                continuation: FlexItemContinuation {
                    source_content_slice: content_slice,
                    // Materialization commits this from the selected source
                    // slice and frozen used border-box origin. The planner
                    // deliberately has no child style, so it must not infer
                    // the replay offset from flex direction here.
                    source_canvas_block_start: FlexFragmentBlockOffset::new(0.0),
                    decoration_slice,
                    replay_origin,
                    first_fragmentainer_capacity: FlexFragmentBlockSize::new(
                        context.first_fragmentainer_capacity.points(),
                    ),
                    continuation_fragmentainer_capacity: FlexFragmentBlockSize::new(
                        context.continuation_fragmentainer_capacity.points(),
                    ),
                    fragmentainer_index: context.page_index,
                    fragment_start: FlexItemFragmentStart::ItemStart,
                    child_fragment_ordinal: None,
                },
                metadata: FragmentPageMetadata::empty(context.page_index),
            })
        })
        .collect::<Vec<_>>();
    let line_fragments = flex_layout
        .lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let line_items = items
                .iter()
                .filter(|item| line.item_indices.contains(&item.item_index))
                .collect::<Vec<_>>();
            let start = line_items
                .iter()
                .map(|item| item.bounds.y().points())
                .min_by(|left, right| {
                    left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
                })?;
            let end = line_items
                .iter()
                .map(|item| item.bounds.y().points() + item.bounds.height().points())
                .max_by(|left, right| {
                    left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
                })?;
            Some(FlexLineFragmentLayout {
                line_index,
                source_bounds: FlexFragmentBlockBounds::new(
                    FlexFragmentBlockOffset::new(start),
                    FlexFragmentBlockOffset::new(end),
                ),
                item_indices: line_items.iter().map(|item| item.item_index).collect(),
            })
        })
        .collect();
    FlexFragmentLayout {
        page_index: context.page_index,
        line_start: unit.line_start,
        line_end: unit.line_end,
        block_start: unit.block_start,
        block_end: unit.block_end,
        line_fragments,
        items,
        metadata: FragmentPageMetadata::new(
            context.page_index,
            Some(PaintClip::from_paint_rect(paint_space_rect(
                context.outer_inline_span.left_x(),
                fragment_bottom,
                context.outer_inline_span.width(),
                fragment_height.points(),
            ))),
            context.starts_page_fragment,
        ),
    }
}

pub(in crate::layout::flex) fn flex_container_page_fragment_bounds(
    plan: &FlexFragmentPlan,
    page_index: usize,
) -> Option<PaintClip> {
    // Container painting consumes the committed fragment records, not the
    // provisional source-layout metadata. In particular, decoration ownership
    // can enlarge a destination border box without changing an item's source
    // range.
    // <https://www.w3.org/TR/css-break-3/#break-decoration>
    plan.materialized_fragments
        .iter()
        .filter(|fragment| fragment.page_index == page_index)
        .filter_map(|fragment| {
            fragment
                .principal_box()
                .map(DecoratedBoxFragment::border_box)
        })
        .fold(None, |bounds, fragment_box| {
            Some(match bounds {
                Some(bounds) => {
                    let bottom = bounds.y().min(fragment_box.y());
                    let top = (bounds.y() + bounds.height())
                        .max(fragment_box.y() + fragment_box.height());
                    let left = bounds.x().min(fragment_box.x());
                    PaintClip::from_paint_rect(paint_space_rect(
                        left,
                        bottom,
                        (bounds.x() + bounds.width()).max(fragment_box.x() + fragment_box.width())
                            - left,
                        top - bottom,
                    ))
                }
                None => fragment_box,
            })
        })
}

/// Returns the resolved container overflow clip for one committed page
/// fragment.
///
/// The clip is materialized only after the container's final used block size
/// is known. Multiple source slices may share a destination page, so their
/// local clips are unioned at this paint adapter boundary.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
pub(in crate::layout::flex) fn flex_container_page_contents_overflow_clip(
    plan: &FlexFragmentPlan,
    page_index: usize,
) -> Option<AxisSelectivePaintClip> {
    plan.materialized_fragments
        .iter()
        .filter(|fragment| fragment.page_index == page_index)
        .filter_map(|fragment| fragment.contents_overflow_clip)
        .fold(None, |clip, fragment_clip| {
            Some(match clip {
                Some(clip) => {
                    debug_assert_eq!(clip.clips_x(), fragment_clip.clips_x());
                    debug_assert_eq!(clip.clips_y(), fragment_clip.clips_y());
                    let clip_bounds = clip.bounds();
                    let fragment_bounds = fragment_clip.bounds();
                    let left = clip_bounds.x().min(fragment_bounds.x());
                    let bottom = clip_bounds.y().min(fragment_bounds.y());
                    let right = (clip_bounds.x() + clip_bounds.width())
                        .max(fragment_bounds.x() + fragment_bounds.width());
                    let top = (clip_bounds.y() + clip_bounds.height())
                        .max(fragment_bounds.y() + fragment_bounds.height());
                    AxisSelectivePaintClip::new(
                        PaintClip::new(left, bottom, right - left, top - bottom),
                        clip.clips_x(),
                        clip.clips_y(),
                    )
                }
                None => fragment_clip,
            })
        })
}

/// Extends a content-slice paint bound with the flex container decorations
/// owned by that fragment.
///
/// Flex fragmentation slices the container's content box at flex-line or item
/// boundaries, while its block-start and block-end padding/borders belong to
/// the first and last box fragments respectively (`box-decoration-break:
/// slice`). The fragment plan stores content ranges for item replay, so this
/// adapter derives the corresponding border-box paint range without letting
/// callers reconstruct its coordinate arithmetic.
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
pub(in crate::layout::flex) fn flex_container_fragment_border_box(
    content_bounds: PaintClip,
    owns_block_start_decoration: bool,
    owns_block_end_decoration: bool,
    block_start_decoration: LayoutLength,
    block_end_decoration: LayoutLength,
) -> PaintClip {
    let block_start_decoration = if owns_block_start_decoration {
        block_start_decoration
    } else {
        layout_pt(0.0)
    };
    let block_end_decoration = if owns_block_end_decoration {
        block_end_decoration
    } else {
        layout_pt(0.0)
    };
    PaintClip::from_paint_rect(paint_space_rect(
        content_bounds.x(),
        content_bounds.y() - block_end_decoration.points(),
        content_bounds.width(),
        content_bounds.height() + block_start_decoration.points() + block_end_decoration.points(),
    ))
}

pub(in crate::layout::flex) fn flex_page_fragment_block_range(
    plan: &FlexFragmentPlan,
    page_index: usize,
    retain_distributed_gap: bool,
) -> Option<FlexFragmentBlockBounds> {
    let range = plan
        .fragments
        .iter()
        .filter(|fragment| fragment.page_index == page_index)
        .fold(None::<FlexFragmentBlockBounds>, |range, fragment| {
            Some(match range {
                Some(range) => FlexFragmentBlockBounds::new(
                    if range.start() <= fragment.block_start {
                        range.start()
                    } else {
                        fragment.block_start
                    },
                    if range.end() >= fragment.block_end {
                        range.end()
                    } else {
                        fragment.block_end
                    },
                ),
                None => FlexFragmentBlockBounds::new(fragment.block_start, fragment.block_end),
            })
        });
    range.map(|range| {
        if !retain_distributed_gap {
            return range;
        }
        let next_start = plan
            .fragments
            .iter()
            .filter(|fragment| fragment.page_index > page_index)
            .map(|fragment| fragment.block_start)
            .filter(|next_start| next_start.points() > range.end().points() + 0.01)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        FlexFragmentBlockBounds::new(range.start(), next_start.unwrap_or(range.end()))
    })
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout::flex) fn flex_unit_break_before(
    fragmentainer_kind: FragmentainerKind,
    item_indices: &[usize],
    children: &[StyledChild<'_>],
) -> PageBreak {
    flex_unit_break_before_for_styles(
        fragmentainer_kind,
        item_indices.iter().map(|&index| &children[index].style),
    )
}

pub(in crate::layout::flex) fn flex_unit_break_after(
    fragmentainer_kind: FragmentainerKind,
    item_indices: &[usize],
    children: &[StyledChild<'_>],
) -> PageBreak {
    flex_unit_break_after_for_styles(
        fragmentainer_kind,
        item_indices.iter().map(|&index| &children[index].style),
    )
}

pub(in crate::layout::flex) fn flex_unit_break_before_for_styles<'a>(
    fragmentainer_kind: FragmentainerKind,
    styles: impl IntoIterator<Item = &'a ComputedStyle>,
) -> PageBreak {
    styles
        .into_iter()
        .map(|style| style.break_before)
        .fold(PageBreak::Auto, |current, candidate| {
            fragmentainer_kind.combine_break(current, candidate)
        })
}

pub(in crate::layout::flex) fn flex_unit_break_after_for_styles<'a>(
    fragmentainer_kind: FragmentainerKind,
    styles: impl IntoIterator<Item = &'a ComputedStyle>,
) -> PageBreak {
    styles
        .into_iter()
        .map(|style| style.break_after)
        .fold(PageBreak::Auto, |current, candidate| {
            fragmentainer_kind.combine_break(current, candidate)
        })
}
