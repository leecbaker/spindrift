use super::*;
use crate::document::paint::geometry::AxisSelectivePaintClip;
use crate::layout::flex::compute::{effective_align_self, flex_item_has_auto_cross_margin};

/// Treat auto margins as zero for an abspos flex static-position probe.
///
/// Absolutely positioned flex children do not participate in flex layout, but
/// Flexbox defines their static-position rectangle by laying out a
/// hypothetical sole flex item:
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
pub(in crate::layout::flex) fn zero_auto_margins_for_static_flex_probe(style: &mut ComputedStyle) {
    let zero = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(0.0),
    );
    if style.box_values.margin.left.is_auto() {
        style.box_values.margin.left = zero.clone();
        style.margin.left = 0.0;
    }
    if style.box_values.margin.right.is_auto() {
        style.box_values.margin.right = zero.clone();
        style.margin.right = 0.0;
    }
    if style.box_values.margin.top.is_auto() {
        style.box_values.margin.top = zero.clone();
        style.margin.top = 0.0;
    }
    if style.box_values.margin.bottom.is_auto() {
        style.box_values.margin.bottom = zero;
        style.margin.bottom = 0.0;
    }
}

/// Resolve distributed `justify-content` values for the hypothetical sole
/// flex item used to establish an absolutely positioned child's static
/// rectangle.
///
/// The static-position algorithm lays out exactly one hypothetical item.
/// CSS Box Alignment's fallback alignment for that item maps `space-between`
/// and `stretch` to start, and `space-around` and `space-evenly` to center.
/// Resolve that fallback before crossing the Taffy adapter, whose distributed
/// alignment does not model this flex static-position special case.
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
/// <https://www.w3.org/TR/css-align-3/#distribution-fallback>
pub(in crate::layout::flex) fn resolve_static_flex_probe_justify_content(
    style: &mut ComputedStyle,
) {
    style.justify_content.keyword = match style.justify_content.keyword {
        css::ContentAlignmentKeyword::Stretch | css::ContentAlignmentKeyword::SpaceBetween => {
            css::ContentAlignmentKeyword::FlexStart
        }
        css::ContentAlignmentKeyword::SpaceAround | css::ContentAlignmentKeyword::SpaceEvenly => {
            css::ContentAlignmentKeyword::Center
        }
        keyword => keyword,
    };
}

/// Resolves the definite main-axis size made available to flex line wrapping.
///
/// CSS Flexbox wraps lines against the flex container's used main size. When a
/// column flex container has `height:auto`, `max-height` still constrains that
/// used main size and must be visible to the flex algorithm, while `min-height`
/// only clamps the final auto height and should not force otherwise overflowing
/// content to wrap:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-line-break>.
pub(in crate::layout::flex) fn flex_available_content_height(
    style: &ComputedStyle,
    definite_content_height: Option<ContentBoxLength>,
    percentage_basis: BlockSizePercentageBasis,
) -> Option<ContentBoxLength> {
    if definite_content_height.is_some() || style.flex_wrap == FlexWrap::NoWrap {
        return definite_content_height;
    }
    if !physical_flex_direction(style).is_column_axis() {
        return definite_content_height;
    }
    used_max_height(style, percentage_basis)
}

/// Projects a block-level flex container's automatic logical inline size into
/// the physical-height input consumed by the Flex adapter.
///
/// In orthogonal writing modes, block formatting's automatic inline size fills
/// the containing block inline span. That span is physical height, not an
/// automatic physical block size, so keeping the distinction here prevents
/// fragment decoration from being truncated to the flex-content height.
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
/// <https://www.w3.org/TR/CSS2/visudet.html#blockwidth>
pub(in crate::layout::flex) fn orthogonal_block_flex_auto_inline_content_height(
    style: &ComputedStyle,
    participates_in_normal_flow: bool,
    available_physical_height: PhysicalContentHeight,
    vertical_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    participates_in_normal_flow
        .then_some(())
        .filter(|_| {
            WritingModeAxes::new(style.writing_mode, style.used_direction()).swaps_physical_axes()
        })
        .and_then(|_| style.box_values.height.is_auto().then_some(()))
        .map(|_| {
            content_box_pt(
                (available_physical_height.points() - vertical_non_content.points()).max(0.0),
            )
        })
}

/// Resolve a flex container's definite content height.
///
/// CSS Flexbox treats a flex container's post-flexing main size as definite,
/// and CSS Sizing lets a preferred aspect ratio transfer a definite width into
/// an automatic height. That ratio-derived height must therefore be visible to
/// flex item cross-size resolution:
/// <https://www.w3.org/TR/css-flexbox-1/#definite-sizes> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(in crate::layout::flex) fn definite_flex_container_content_height(
    style: &ComputedStyle,
    explicit_content_height: Option<ContentBoxLength>,
    content_width: ContentBoxLength,
    percentage_basis: BlockSizePercentageBasis,
    horizontal_non_content: NonContentLength,
    vertical_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    if explicit_content_height.is_some() || !style.box_values.height.is_auto() {
        return explicit_content_height;
    }

    let ratio = style.aspect_ratio.preferred_ratio_for_non_replaced(false)?;
    if ratio <= 0.0 || !ratio.is_finite() {
        return None;
    }

    let content_height = match style.box_sizing {
        BoxSizing::ContentBox => content_width / ratio,
        BoxSizing::BorderBox => {
            let border_box_width =
                content_box_to_border_box_length(content_width, horizontal_non_content);
            border_box_to_content_box_length(border_box_width / ratio, vertical_non_content)
        }
    };
    Some(constrain_content_height(
        style,
        content_height.max(content_box_pt(0.0)),
        percentage_basis,
    ))
}

/// Returns whether an unfragmented flex container should prebreak to the next page.
///
/// CSS Fragmentation can move an unfragmented box to the next fragmentainer
/// when it fits there but not in the current remaining space. Flex containers
/// with item-level forced breaks skip this whole-box move so the break is
/// consumed at a flex boundary instead:
/// <https://www.w3.org/TR/css-break-3/#breaking-rules> and
/// <https://drafts.csswg.org/css-flexbox-1/#pagination>.
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
                    item.fragmentation_height().points(),
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
                    let content_slice = FlexFragmentSlice {
                        block_start: FlexFragmentBlockOffset::new(
                            (slice_start - item_block_start).max(0.0),
                        ),
                        block_end: FlexFragmentBlockOffset::new(
                            (slice_end - item_block_start).min(source_bounds.height().points()),
                        ),
                    };
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
        .filter_map(|fragment| fragment.destination_border_box)
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
pub(in crate::layout::flex) fn flex_gap_decoration_primitives_for_page(
    flex_layout: &FlexLayout,
    style: &ComputedStyle,
    context: FlexGapDecorationFragmentContext,
) -> Vec<PaintPrimitive> {
    let Some(fragment_block_bounds) = flex_page_fragment_block_range(
        &flex_layout.fragment_plan,
        context.page_index,
        !matches!(
            style.align_content.keyword,
            ContentAlignmentKeyword::Normal
                | ContentAlignmentKeyword::Start
                | ContentAlignmentKeyword::End
                | ContentAlignmentKeyword::FlexStart
                | ContentAlignmentKeyword::FlexEnd
                | ContentAlignmentKeyword::Left
                | ContentAlignmentKeyword::Right
                | ContentAlignmentKeyword::Center
                | ContentAlignmentKeyword::Baseline
                | ContentAlignmentKeyword::LastBaseline
        ),
    ) else {
        return Vec::new();
    };
    let block_start = fragment_block_bounds.start().points();
    let block_end = fragment_block_bounds.end().points();
    let fragment_height = (block_end - block_start).max(0.0);
    if fragment_height <= 0.01 {
        return Vec::new();
    }

    let mut gutters = flex_gap_decoration_gutters(
        flex_layout,
        style,
        PhysicalContentWidth::new(content_box_pt(context.content_inline_span.width())),
        context.content_height,
    );
    gutters.columns = gutters
        .columns
        .into_iter()
        .filter_map(|mut gutter| {
            let Some(segment) = gutter.segment_range else {
                return Some(gutter);
            };
            let start = segment.start.max(block_start);
            let end = segment.end.min(block_end);
            if end <= start + 0.01 {
                return None;
            }
            gutter.segment_range = Some(GapAxisSpan::new(start - block_start, end - block_start));
            Some(gutter)
        })
        .collect();
    gutters.rows = if context.has_forced_item_breaks {
        // Forced breaks between flex items/lines replace the intervening row
        // gutter with a fragmentainer boundary. No fragment owns that gutter,
        // so it contributes no row-rule segment on either side.
        Vec::new()
    } else {
        flex_fragment_gap_gutters(&gutters.rows, fragment_block_bounds)
    };
    // Visibility is defined by adjacency in the unfragmented flex layout.
    // Retain neighboring items across the fragment boundary when deciding
    // whether a page-local segment is `between` items; restricting metadata
    // to ink already replayed on this page incorrectly hides the rule in the
    // gap immediately before the next fragment.
    // https://drafts.csswg.org/css-gaps-1/#gap-rule-visibility
    let items = flex_layout
        .items
        .iter()
        .map(|item| {
            GapDecorationItem::from_rect(GapDecorationRect::new(
                GapDecorationPoint::new(item.x().points(), item.y().points() - block_start),
                GapDecorationSize::new(item.width().points(), item.height().points()),
            ))
        })
        .collect::<Vec<_>>();

    flex_gap_decoration_primitives_with_gutters(
        style,
        GapDecorationContainer::new(
            context.content_inline_span.left_x(),
            context.fragment_bounds.y() + context.fragment_bounds.height(),
            context.content_inline_span.width(),
            fragment_height,
        ),
        &items,
        &gutters,
    )
}

pub(in crate::layout::flex) fn flex_gap_decoration_items(
    flex_layout: &FlexLayout,
) -> Vec<GapDecorationItem> {
    flex_layout
        .items
        .iter()
        .map(|item| {
            GapDecorationItem::from_rect(GapDecorationRect::new(
                GapDecorationPoint::new(item.x().points(), item.y().points()),
                GapDecorationSize::new(item.width().points(), item.height().points()),
            ))
        })
        .collect()
}

pub(in crate::layout::flex) fn flex_fragment_gap_gutters(
    gutters: &[GapDecorationGutter],
    fragment_block_bounds: FlexFragmentBlockBounds,
) -> Vec<GapDecorationGutter> {
    // Gap decoration is a scalar paint adapter; keep source-range endpoints
    // typed until this projection.
    let block_start = fragment_block_bounds.start().points();
    let block_end = fragment_block_bounds.end().points();
    gutters
        .iter()
        .filter_map(|gutter| {
            let start = gutter.span.start.max(block_start);
            let end = gutter.span.end.min(block_end);
            (end > start + 0.01).then_some(GapDecorationGutter {
                span: GapAxisSpan::new(start - block_start, end - block_start),
                ..*gutter
            })
        })
        .collect()
}

pub(in crate::layout::flex) fn flex_item_line_range(
    flex_layout: &FlexLayout,
    item_index: usize,
) -> (usize, usize) {
    flex_layout
        .lines
        .iter()
        .enumerate()
        .find(|(_, line)| line.item_indices.contains(&item_index))
        .map(|(line_index, _)| (line_index, line_index + 1))
        .unwrap_or((0, 0))
}

pub(in crate::layout::flex) fn flex_item_block_bounds(
    item: &FlexItemLayout,
    use_fragmentation_height: bool,
) -> FlexFragmentBlockBounds {
    let height = if use_fragmentation_height {
        FlexFragmentBlockSize::new(item.fragmentation_height().points())
    } else {
        FlexFragmentBlockSize::new(item.height().points())
    };
    FlexFragmentBlockBounds::from_start_and_size(
        FlexFragmentBlockOffset::new(item.y().points()),
        height,
    )
}

pub(in crate::layout::flex) fn flex_gap_decoration_gutters(
    flex_layout: &FlexLayout,
    style: &ComputedStyle,
    content_width: PhysicalContentWidth,
    content_height: PhysicalContentHeight,
) -> GapDecorationGutters {
    let axes = FlexAxes::for_style(style);
    let PhysicalFlexGaps {
        horizontal: physical_gap_width,
        vertical: physical_gap_height,
    } = physical_flex_gaps(style);
    let used_physical_gap_width = used_flex_gap(
        physical_gap_width,
        PercentageBasis::definite(content_width.content_box_length()),
    );
    let used_physical_gap_height = used_flex_gap(
        physical_gap_height,
        PercentageBasis::definite(content_height.content_box_length()),
    );
    let main_gap = if axes.is_main_row_axis() {
        flex_main_gap_size(used_physical_gap_width)
    } else {
        flex_main_gap_size(used_physical_gap_height)
    };
    let cross_gap = if axes.is_main_row_axis() {
        flex_cross_gap_size(used_physical_gap_height)
    } else {
        flex_cross_gap_size(used_physical_gap_width)
    };
    let cross_gutters = flex_cross_axis_gap_gutters(
        flex_layout,
        axes,
        cross_gap,
        matches!(
            style.align_content.keyword,
            ContentAlignmentKeyword::SpaceBetween
                | ContentAlignmentKeyword::SpaceAround
                | ContentAlignmentKeyword::SpaceEvenly
        ),
    );
    let main_gutters = flex_main_axis_gap_gutters(
        flex_layout,
        axes,
        main_gap,
        cross_gap,
        matches!(
            style.justify_content.keyword,
            ContentAlignmentKeyword::SpaceBetween
                | ContentAlignmentKeyword::SpaceAround
                | ContentAlignmentKeyword::SpaceEvenly
        ),
        main_gap.points() <= 0.01
            && style.justify_content.keyword == ContentAlignmentKeyword::SpaceBetween
            && style.column_rule.rule_break == css::GapRuleBreak::Normal
            && style.row_rule.rule_break == css::GapRuleBreak::Normal
            && !flex_layout
                .fragment_plan
                .fragments
                .iter()
                .any(|fragment| fragment.page_index > 0),
    );
    let mut gutters = if axes.is_main_row_axis() {
        GapDecorationGutters {
            columns: main_gutters,
            rows: cross_gutters,
        }
    } else {
        GapDecorationGutters {
            columns: cross_gutters,
            rows: main_gutters,
        }
    };
    let reverse_physical_columns = if style.writing_mode == WritingMode::HorizontalTb {
        style.direction == Direction::Rtl
    } else {
        matches!(
            style.writing_mode,
            WritingMode::VerticalRl | WritingMode::SidewaysRl
        )
    };
    assign_flex_gap_rule_indices(&mut gutters.columns, reverse_physical_columns);
    assign_flex_gap_rule_indices(
        &mut gutters.rows,
        style.writing_mode.ltr_inline_progresses_upward(),
    );
    gutters
}

fn assign_flex_gap_rule_indices(gutters: &mut [GapDecorationGutter], reverse: bool) {
    let mut positions = gutters
        .iter()
        .map(|gutter| gutter.span.start)
        .collect::<Vec<_>>();
    positions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    positions.dedup_by(|a, b| (*a - *b).abs() <= 0.01);
    let count = positions.len();
    for gutter in gutters {
        let physical_index = positions
            .iter()
            .position(|position| (*position - gutter.span.start).abs() <= 0.01)
            .unwrap_or(0);
        gutter.rule_index = Some(if reverse {
            count.saturating_sub(1).saturating_sub(physical_index)
        } else {
            physical_index
        });
    }
}

pub(in crate::layout::flex) fn flex_main_axis_gap_gutters(
    flex_layout: &FlexLayout,
    axes: FlexAxes,
    used_gap: FlexMainSize,
    cross_gap: FlexCrossSize,
    has_distributed_gutters: bool,
    distribute_fractional_remainder: bool,
) -> Vec<GapDecorationGutter> {
    let mut gutters = Vec::new();
    for line in &flex_layout.lines {
        let mut line_items = line
            .item_indices
            .iter()
            .filter_map(|&index| flex_layout.items.get(index))
            .filter(|item| item.main_size(axes).points() > 0.01)
            .collect::<Vec<_>>();
        line_items.sort_by(|a, b| {
            a.main_start(axes)
                .partial_cmp(&b.main_start(axes))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // Main-axis rule segments occupy the flex line's allocated cross-size,
        // including space added by `align-content: stretch`; the item margin
        // boxes determine gutter centers but do not truncate the line segment.
        // <https://drafts.csswg.org/css-gaps-1/#flex-gaps>
        let line_cross_range = Some((
            line.cross_start.min(line.cross_end),
            line.cross_start.max(line.cross_end),
        ));
        for (gap_index, pair) in line_items.windows(2).enumerate() {
            // Space-between distributes an indivisible CSS-pixel remainder
            // across successive gaps. Keep rule centers on the same alternating
            // half-pixel sequence as that distribution instead of repeatedly
            // truncating toward one edge.
            let remainder_offset = if distribute_fractional_remainder {
                css::CSS_PX_TO_PT / 2.0 * if gap_index % 2 == 0 { 1.0 } else { -1.0 }
            } else {
                0.0
            };
            let start = (pair[0].main_start(axes)
                + pair[0].main_size(axes)
                + FlexMainLength::new(remainder_offset))
            .points();
            let end = (pair[1].main_start(axes) + FlexMainLength::new(remainder_offset)).points();
            if let Some((segment_start, segment_end)) =
                line_cross_range.map(|(start, end)| (start.points(), end.points()))
            {
                push_unique_flex_gap_gutter_with_segment(
                    &mut gutters,
                    GapAxisSpan::new(start, end),
                    if has_distributed_gutters {
                        FlexGapDecorationGutterWidth::FillAvailable
                    } else {
                        FlexGapDecorationGutterWidth::Fixed(layout_pt(used_gap.points()))
                    },
                    GapAxisSpan::new(segment_start, segment_end),
                );
            }
        }
    }
    gutters.sort_by(|a, b| {
        a.span
            .start
            .partial_cmp(&b.span.start)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.span
                    .end
                    .partial_cmp(&b.span.end)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let mut merged: Vec<GapDecorationGutter> = Vec::with_capacity(gutters.len());
    for gutter in gutters {
        if let Some(previous) = merged.last_mut()
            && (previous.span.start - gutter.span.start).abs() <= 0.01
            && (previous.span.end - gutter.span.end).abs() <= 0.01
            && let (Some(previous_segment), Some(segment)) =
                (previous.segment_range, gutter.segment_range)
            && segment.start <= previous_segment.end + cross_gap.points() + 0.01
            && previous_segment.start <= segment.end + cross_gap.points() + 0.01
        {
            // Flex lines may be stored in either physical cross-axis order
            // (notably vertical-rl and wrap-reverse). Union adjacent aligned
            // main-axis gutters without assuming the next segment starts
            // after the previous one.
            previous.segment_range = Some(GapAxisSpan::new(
                previous_segment.start.min(segment.start),
                previous_segment.end.max(segment.end),
            ));
        } else {
            merged.push(gutter);
        }
    }
    merged
}

pub(in crate::layout::flex) fn flex_cross_axis_gap_gutters(
    flex_layout: &FlexLayout,
    axes: FlexAxes,
    used_gap: FlexCrossSize,
    has_distributed_gutters: bool,
) -> Vec<GapDecorationGutter> {
    // Cross-axis gutters are the spaces between resolved flex line boxes.
    // Line metadata excludes the authored gap while retaining distributed and
    // stretched line allocation, so these boundaries are the authoritative
    // used gutter edges.
    // <https://drafts.csswg.org/css-gaps-1/#flex-gaps>
    let is_fragmented = flex_layout
        .fragment_plan
        .fragments
        .iter()
        .any(|fragment| fragment.page_index > 0);
    let mut line_bands = if is_fragmented {
        flex_layout
            .lines
            .iter()
            .filter_map(|line| {
                line.item_indices
                    .iter()
                    .filter_map(|&index| flex_layout.items.get(index))
                    .fold(None, |band, item| {
                        let start = item.cross_start(axes).points();
                        let end = (item.cross_start(axes) + item.cross_size(axes)).points();
                        Some(match band {
                            Some((band_start, band_end)) => {
                                (f32::min(band_start, start), f32::max(band_end, end))
                            }
                            None => (start, end),
                        })
                    })
            })
            .collect::<Vec<_>>()
    } else {
        flex_layout
            .lines
            .iter()
            .map(|line| {
                (
                    line.cross_start.points().min(line.cross_end.points()),
                    line.cross_start.points().max(line.cross_end.points()),
                )
            })
            .collect::<Vec<_>>()
    };
    line_bands.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut gutters = Vec::new();
    for pair in line_bands.windows(2) {
        push_unique_flex_gap_gutter(
            &mut gutters,
            GapAxisSpan::new(pair[0].1, pair[1].0),
            if has_distributed_gutters {
                FlexGapDecorationGutterWidth::FillAvailable
            } else {
                FlexGapDecorationGutterWidth::Fixed(layout_pt(used_gap.points()))
            },
        );
    }
    gutters
}

/// The rule extent selected for a gap-decoration paint primitive.
///
/// Distributed alignment fills the resolved gutter, while an authored gap
/// retains its used layout extent. This is deliberately separate from either
/// Flex axis because the next boundary is the axis-neutral gap-decoration
/// painter.
#[derive(Debug, Clone, Copy)]
enum FlexGapDecorationGutterWidth {
    Fixed(LayoutLength),
    FillAvailable,
}

fn push_unique_flex_gap_gutter(
    gutters: &mut Vec<GapDecorationGutter>,
    span: GapAxisSpan,
    used_gap: FlexGapDecorationGutterWidth,
) {
    let start = span.start;
    let end = span.end;
    if end <= start + 0.01
        || matches!(used_gap, FlexGapDecorationGutterWidth::Fixed(width) if width.points() <= 0.01)
    {
        return;
    }
    let available = end - start;
    let size = match used_gap {
        FlexGapDecorationGutterWidth::Fixed(width) => width.points().min(available).max(0.0),
        FlexGapDecorationGutterWidth::FillAvailable => available,
    };
    let start = start + (available - size) * 0.5;
    let end = start + size;
    if gutters.iter().any(|gutter| {
        (gutter.span.start - start).abs() <= 0.01 && (gutter.span.end - end).abs() <= 0.01
    }) {
        return;
    }
    gutters.push(GapDecorationGutter::new(start, end));
}

fn push_unique_flex_gap_gutter_with_segment(
    gutters: &mut Vec<GapDecorationGutter>,
    span: GapAxisSpan,
    used_gap: FlexGapDecorationGutterWidth,
    segment: GapAxisSpan,
) {
    let start = span.start;
    let end = span.end;
    let segment_start = segment.start;
    let segment_end = segment.end;
    if end <= start + 0.01
        || matches!(used_gap, FlexGapDecorationGutterWidth::Fixed(width) if width.points() <= 0.01)
        || segment_end <= segment_start + 0.01
    {
        return;
    }
    let available = end - start;
    // Distributed alignment increases the effective gutter between adjacent
    // items; the decoration is centered in that entire resolved gutter.
    // https://drafts.csswg.org/css-align-3/#gap-legacy
    let size = match used_gap {
        FlexGapDecorationGutterWidth::Fixed(width) => width.points().min(available).max(0.0),
        FlexGapDecorationGutterWidth::FillAvailable => available,
    };
    let start = start + (available - size) * 0.5;
    let end = start + size;
    if gutters.iter().any(|gutter| {
        (gutter.span.start - start).abs() <= 0.01
            && (gutter.span.end - end).abs() <= 0.01
            && gutter.segment_range.is_some_and(|existing| {
                (existing.start - segment_start).abs() <= 0.01
                    && (existing.end - segment_end).abs() <= 0.01
            })
    }) {
        return;
    }
    gutters.push(GapDecorationGutter::with_segment_range(
        start,
        end,
        segment_start,
        segment_end,
    ));
}

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

fn flex_unit_break_before_for_styles<'a>(
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

fn flex_unit_break_after_for_styles<'a>(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sole_item_static_probe_resolves_distributed_justify_content_fallbacks() {
        let cases = [
            (
                css::ContentAlignmentKeyword::SpaceBetween,
                css::ContentAlignmentKeyword::FlexStart,
            ),
            (
                css::ContentAlignmentKeyword::Stretch,
                css::ContentAlignmentKeyword::FlexStart,
            ),
            (
                css::ContentAlignmentKeyword::SpaceAround,
                css::ContentAlignmentKeyword::Center,
            ),
            (
                css::ContentAlignmentKeyword::SpaceEvenly,
                css::ContentAlignmentKeyword::Center,
            ),
        ];

        for (authored, expected) in cases {
            let mut style = ComputedStyle::initial();
            style.justify_content.keyword = authored;
            resolve_static_flex_probe_justify_content(&mut style);
            assert_eq!(style.justify_content.keyword, expected);
        }
    }

    #[test]
    fn flex_prebreak_recognizes_a_margin_box_at_page_top() {
        assert!(!should_move_flex_container_to_next_page(
            PageTopBlockPosition::new(980.0),
            layout_pt(20.0),
            layout_pt(990.0),
            PageTopBlockPosition::new(1000.0),
            PageTopBlockPosition::new(0.0),
            layout_pt(1000.0),
        ));
    }

    #[test]
    fn flex_prebreak_still_moves_a_margin_box_that_starts_mid_page() {
        assert!(should_move_flex_container_to_next_page(
            PageTopBlockPosition::new(780.0),
            layout_pt(20.0),
            layout_pt(990.0),
            PageTopBlockPosition::new(1000.0),
            PageTopBlockPosition::new(0.0),
            layout_pt(1000.0),
        ));
    }

    #[test]
    fn isolated_flex_measurement_cannot_whole_box_prebreak() {
        assert!(!flex_container_allows_whole_box_prebreak(
            FragmentainerKind::Page,
            1,
            false,
        ));
        assert!(flex_container_allows_whole_box_prebreak(
            FragmentainerKind::Page,
            0,
            false,
        ));
    }

    #[test]
    fn flex_fragment_materializes_overlapping_wrapped_column_line_slices() {
        let first = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(20.0, 100.0),
        ));
        let second = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(20.0, 0.0),
            ContainerSize::new(20.0, 50.0),
        ));
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(100.0)),
            main_gap: FlexMainSize::new(0.0),
            baselines: FlexContainerBaselineSets::default(),
            items: vec![first, second],
            lines: vec![
                test_flex_line(
                    vec![0],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(100.0),
                    FlexCrossOffset::new(0.0),
                    FlexCrossOffset::new(20.0),
                ),
                test_flex_line(
                    vec![1],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(50.0),
                    FlexCrossOffset::new(20.0),
                    FlexCrossOffset::new(40.0),
                ),
            ],
            fragment_plan: FlexFragmentPlan::default(),
        };
        let fragment = flex_fragment_from_break_unit(
            &FlexBreakUnit {
                topology: FlexReplayTopology::Fragmented,
                item_indices: vec![0, 1],
                line_start: 0,
                line_end: 2,
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(75.0),
                break_before: PageBreak::Auto,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
            &flex_layout,
            FlexFragmentBuildContext {
                page_index: 0,
                outer_inline_span: PageInlineSpan::new(0.0, 40.0),
                content_top: PageTopBlockPosition::new(100.0),
                block_offset: FlexFragmentBlockOffset::new(0.0),
                first_fragmentainer_capacity: layout_pt(75.0),
                continuation_fragmentainer_capacity: layout_pt(75.0),
                starts_page_fragment: true,
            },
            false,
        );

        assert_eq!(fragment.line_fragments.len(), 2);
        assert_eq!(fragment.line_fragments[0].item_indices, vec![0]);
        assert_eq!(fragment.items[0].line_index, 0);
        assert_eq!(
            fragment.line_fragments[0].source_bounds,
            FlexFragmentBlockBounds::new(
                FlexFragmentBlockOffset::new(0.0),
                FlexFragmentBlockOffset::new(75.0),
            )
        );
        assert_eq!(fragment.line_fragments[1].item_indices, vec![1]);
        assert_eq!(fragment.items[1].line_index, 1);
        assert_eq!(
            fragment.line_fragments[1].source_bounds,
            FlexFragmentBlockBounds::new(
                FlexFragmentBlockOffset::new(0.0),
                FlexFragmentBlockOffset::new(50.0),
            )
        );

        let unfragmented = flex_fragment_from_break_unit(
            &FlexBreakUnit {
                topology: FlexReplayTopology::Unfragmented,
                // The physical-Y order of these wrapped column lines is not
                // the replay order for `column-reverse`.
                item_indices: vec![1, 0],
                line_start: 0,
                line_end: 2,
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(100.0),
                break_before: PageBreak::Auto,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
            &flex_layout,
            FlexFragmentBuildContext {
                page_index: 0,
                outer_inline_span: PageInlineSpan::new(0.0, 40.0),
                content_top: PageTopBlockPosition::new(100.0),
                block_offset: FlexFragmentBlockOffset::new(0.0),
                first_fragmentainer_capacity: layout_pt(100.0),
                continuation_fragmentainer_capacity: layout_pt(100.0),
                starts_page_fragment: true,
            },
            false,
        );
        assert_eq!(
            unfragmented
                .items
                .iter()
                .map(|item| item.item_index)
                .collect::<Vec<_>>(),
            vec![1, 0]
        );
        assert_eq!(
            unfragmented.items[0].bounds.height(),
            FlexPhysicalVerticalSize::new(50.0)
        );
        assert_eq!(
            unfragmented.items[1].bounds.height(),
            FlexPhysicalVerticalSize::new(100.0)
        );
        assert!(unfragmented.items.iter().all(|item| {
            !item.continuation.continues_from_previous_fragment()
                && (item.continuation.source_content_slice.block_end.points()
                    - item.source_bounds.height().points())
                .abs()
                    <= 0.01
        }));
    }

    #[test]
    fn wrapped_column_item_growth_stays_within_its_own_line() {
        let mut items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                ContainerSize::new(20.0, 120.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 120.0),
                ContainerSize::new(20.0, 20.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(20.0, 0.0),
                ContainerSize::new(20.0, 120.0),
            )),
        ];
        let lines = vec![
            test_flex_line(
                vec![0, 1],
                FlexMainOffset::new(0.0),
                FlexMainOffset::new(140.0),
                FlexCrossOffset::new(0.0),
                FlexCrossOffset::new(20.0),
            ),
            test_flex_line(
                vec![2],
                FlexMainOffset::new(0.0),
                FlexMainOffset::new(120.0),
                FlexCrossOffset::new(20.0),
                FlexCrossOffset::new(40.0),
            ),
        ];

        assert!(expand_wrapped_column_items_through_fragmentainers(
            &mut items,
            &lines,
            FlexFragmentBlockSize::new(100.0),
            FlexFragmentBlockSize::new(100.0),
        ));
        assert_eq!(items[0].height().points(), 200.0);
        assert_eq!(items[1].y().points(), 200.0);
        assert_eq!(items[2].height().points(), 200.0);
        assert_eq!(items[0].y().points(), 0.0);
        assert_eq!(items[2].y().points(), 0.0);
    }

    #[test]
    fn orthogonal_block_flex_auto_inline_size_projects_to_physical_height() {
        let mut vertical = ComputedStyle::initial();
        vertical.writing_mode = WritingMode::VerticalRl;
        assert_eq!(
            orthogonal_block_flex_auto_inline_content_height(
                &vertical,
                true,
                PhysicalContentHeight::new(content_box_pt(100.0)),
                non_content_pt(12.0),
            ),
            Some(content_box_pt(88.0))
        );

        let horizontal = ComputedStyle::initial();
        assert_eq!(
            orthogonal_block_flex_auto_inline_content_height(
                &horizontal,
                true,
                PhysicalContentHeight::new(content_box_pt(100.0)),
                non_content_pt(12.0),
            ),
            None
        );

        assert_eq!(
            orthogonal_block_flex_auto_inline_content_height(
                &vertical,
                false,
                PhysicalContentHeight::new(content_box_pt(100.0)),
                non_content_pt(12.0),
            ),
            None,
            "floats use shrink-to-fit sizing rather than normal-flow block fill"
        );
    }

    fn containing_block_height_basis(height: PhysicalContentHeight) -> BlockSizePercentageBasis {
        PercentageBasis::definite_from(
            height.content_box_length(),
            BlockSizeBasisSource::ContainingBlock,
        )
    }

    #[test]
    fn definite_flex_container_height_transfers_content_box_aspect_ratio() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::ContentBox;
        style.aspect_ratio = css::AspectRatio::from_ratio(2.0).unwrap();

        let height = definite_flex_container_content_height(
            &style,
            None,
            content_box_pt(120.0),
            containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(100.0))),
            non_content_pt(20.0),
            non_content_pt(10.0),
        );

        assert_eq!(height, Some(content_box_pt(60.0)));
    }

    #[test]
    fn definite_flex_container_height_transfers_border_box_aspect_ratio() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.aspect_ratio = css::AspectRatio::from_ratio(2.0).unwrap();

        let height = definite_flex_container_content_height(
            &style,
            None,
            content_box_pt(100.0),
            containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(100.0))),
            non_content_pt(20.0),
            non_content_pt(10.0),
        );

        assert_eq!(height, Some(content_box_pt(50.0)));
    }

    #[test]
    fn definite_flex_container_height_keeps_explicit_height_and_rejects_invalid_ratio() {
        let explicit_height = content_box_pt(45.0);
        let style = ComputedStyle::initial();
        assert_eq!(
            definite_flex_container_content_height(
                &style,
                Some(explicit_height),
                content_box_pt(120.0),
                containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(100.0))),
                non_content_pt(0.0),
                non_content_pt(0.0),
            ),
            Some(explicit_height)
        );

        let invalid_ratio_style = ComputedStyle::initial();
        assert!(css::AspectRatio::from_ratio(f32::NAN).is_none());
        assert_eq!(
            definite_flex_container_content_height(
                &invalid_ratio_style,
                None,
                content_box_pt(120.0),
                containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(100.0))),
                non_content_pt(0.0),
                non_content_pt(0.0),
            ),
            None
        );
    }

    #[test]
    fn wrapped_column_flex_uses_max_height_as_available_height() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Column;
        style.flex_wrap = FlexWrap::Wrap;
        style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(60.0),
        );

        assert_eq!(
            flex_available_content_height(
                &style,
                None,
                containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(100.0))),
            ),
            Some(content_box_pt(60.0))
        );
    }

    #[test]
    fn flex_source_block_end_projects_typed_capacity_into_local_offset() {
        assert_eq!(
            flex_source_block_end_after_available_capacity(
                FlexFragmentBlockOffset::new(70.0),
                Fragmentainer::new(layout_pt(120.0), layout_pt(30.0)),
            ),
            FlexFragmentBlockOffset::new(100.0)
        );
    }

    #[test]
    fn flex_gap_gutters_use_line_local_main_axis_gaps() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Row;
        style.column_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        style.row_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(50.0)),
            main_gap: FlexMainSize::new(10.0),
            baselines: FlexContainerBaselineSets {
                vertical: FlexItemBaselinePair {
                    first: Some(flex_vertical_baseline_from_points(0.0)),
                    last: None,
                },
                horizontal: FlexItemBaselinePair::default(),
            },
            items: vec![
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 0.0),
                    ContainerSize::new(30.0, 20.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(40.0, 0.0),
                    ContainerSize::new(30.0, 20.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 30.0),
                    ContainerSize::new(40.0, 20.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(50.0, 30.0),
                    ContainerSize::new(30.0, 20.0),
                )),
            ],
            lines: vec![
                test_flex_line(
                    vec![0, 1],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(70.0),
                    FlexCrossOffset::new(0.0),
                    FlexCrossOffset::new(20.0),
                ),
                test_flex_line(
                    vec![2, 3],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(80.0),
                    FlexCrossOffset::new(30.0),
                    FlexCrossOffset::new(50.0),
                ),
            ],
            fragment_plan: FlexFragmentPlan::default(),
        };

        let gutters = flex_gap_decoration_gutters(
            &flex_layout,
            &style,
            PhysicalContentWidth::new(content_box_pt(100.0)),
            PhysicalContentHeight::new(content_box_pt(50.0)),
        );

        assert_eq!(gutters.columns.len(), 2);
        assert_eq!(gutters.columns[0].span.start, 30.0);
        assert_eq!(gutters.columns[0].span.end, 40.0);
        assert_eq!(gutters.columns[1].span.start, 40.0);
        assert_eq!(gutters.columns[1].span.end, 50.0);
        assert_eq!(gutters.rows.len(), 1);
        assert_eq!(gutters.rows[0].span.start, 20.0);
        assert_eq!(gutters.rows[0].span.end, 30.0);

        let mut no_gap_style = style;
        no_gap_style.column_gap = css::ComputedGap::Normal;
        no_gap_style.row_gap = css::ComputedGap::Normal;
        let no_gap_gutters = flex_gap_decoration_gutters(
            &flex_layout,
            &no_gap_style,
            PhysicalContentWidth::new(content_box_pt(100.0)),
            PhysicalContentHeight::new(content_box_pt(50.0)),
        );
        assert!(no_gap_gutters.columns.is_empty());
        assert!(no_gap_gutters.rows.is_empty());
    }

    #[test]
    fn flex_gap_decorations_are_projected_into_page_fragments() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.flex_direction = FlexDirection::Row;
        style.column_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        style.column_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(CssColor::new(255, 0, 0));
        let left = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 20.0),
            ContainerSize::new(30.0, 50.0),
        ));
        let right = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(40.0, 20.0),
            ContainerSize::new(30.0, 50.0),
        ));
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(70.0)),
            main_gap: FlexMainSize::new(10.0),
            baselines: FlexContainerBaselineSets {
                vertical: FlexItemBaselinePair {
                    first: Some(flex_vertical_baseline_from_points(0.0)),
                    last: None,
                },
                horizontal: FlexItemBaselinePair::default(),
            },
            items: vec![left.clone(), right.clone()],
            lines: vec![test_flex_line(
                vec![0, 1],
                FlexMainOffset::new(0.0),
                FlexMainOffset::new(70.0),
                FlexCrossOffset::new(20.0),
                FlexCrossOffset::new(70.0),
            )],
            fragment_plan: FlexFragmentPlan {
                fragments: vec![FlexFragmentLayout {
                    page_index: 0,
                    line_start: 0,
                    line_end: 1,
                    block_start: FlexFragmentBlockOffset::new(20.0),
                    block_end: FlexFragmentBlockOffset::new(70.0),
                    line_fragments: Vec::new(),
                    items: vec![
                        test_flex_item_fragment(0, left),
                        test_flex_item_fragment(1, right),
                    ],
                    metadata: FragmentPageMetadata::empty(0),
                }],
                materialized_fragments: Vec::new(),
            },
        };

        let primitives = flex_gap_decoration_primitives_for_page(
            &flex_layout,
            &style,
            FlexGapDecorationFragmentContext {
                page_index: 0,
                content_inline_span: PageInlineSpan::new(0.0, 70.0),
                content_height: PhysicalContentHeight::new(content_box_pt(70.0)),
                fragment_bounds: PaintClip::new(0.0, 100.0, 70.0, 50.0),
                has_forced_item_breaks: false,
            },
        );
        let strokes = solid_gap_rule_centerlines(&primitives);

        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].x1(), 35.0);
        assert_eq!(strokes[0].y1(), 150.0);
        assert_eq!(strokes[0].y2(), 100.0);
        assert_eq!(strokes[0].stroke_width, PaintStrokeWidth::new(4.0));
    }

    #[test]
    fn flex_break_combiner_ignores_other_fragmentainer_values() {
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Auto, PageBreak::Column),
            PageBreak::Auto
        );
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Auto, PageBreak::AvoidColumn),
            PageBreak::Auto
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Auto, PageBreak::Column),
            PageBreak::Column
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Auto, PageBreak::AvoidColumn),
            PageBreak::AvoidColumn
        );
    }

    #[test]
    fn flex_break_combiner_keeps_existing_target_forced_break() {
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Left, PageBreak::Page),
            PageBreak::Left
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Column, PageBreak::Avoid),
            PageBreak::Column
        );
    }

    #[test]
    fn flex_unit_break_aggregation_scopes_forced_values_to_fragmentainer_kind() {
        let mut first = ComputedStyle::initial();
        first.break_before = PageBreak::Column;
        first.break_after = PageBreak::AvoidColumn;
        let mut second = ComputedStyle::initial();
        second.break_before = PageBreak::Auto;
        second.break_after = PageBreak::Page;
        let styles = [&first, &second];

        assert_eq!(
            flex_unit_break_before_for_styles(FragmentainerKind::Page, styles),
            PageBreak::Auto
        );
        assert_eq!(
            flex_unit_break_before_for_styles(FragmentainerKind::Column, styles),
            PageBreak::Column
        );
        assert_eq!(
            flex_unit_break_after_for_styles(FragmentainerKind::Page, styles),
            PageBreak::Page
        );
        assert_eq!(
            flex_unit_break_after_for_styles(FragmentainerKind::Column, styles),
            PageBreak::AvoidColumn
        );
    }

    #[test]
    fn flex_unit_prebreak_scopes_avoid_to_fragmentainer_kind() {
        let opportunity = FragmentBreakOpportunity {
            source_block_offset: 20.0,
            break_before: PageBreak::Auto,
            break_after: PageBreak::AvoidColumn,
            break_inside_avoid: false,
        };
        let current_fragmentainer = Fragmentainer::new(layout_pt(100.0), layout_pt(10.0));

        let page_decision = FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
            fragmentainer_kind: FragmentainerKind::Page,
            break_is_applicable: true,
            unit_is_oversized: false,
            has_prior_unit: false,
            has_later_unit: false,
            cursor: FlexFragmentCursor::new(
                PageTopBlockPosition::new(0.0),
                FlexFragmentBlockOffset::new(0.0),
            ),
            unit_block_start: FlexFragmentBlockOffset::new(20.0),
            unit_block_end: FlexFragmentBlockOffset::new(40.0),
            current_fragmentainer,
            break_opportunity: opportunity,
            can_advance: true,
        });
        let column_decision = FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
            fragmentainer_kind: FragmentainerKind::Column,
            break_is_applicable: true,
            unit_is_oversized: false,
            has_prior_unit: false,
            has_later_unit: false,
            cursor: FlexFragmentCursor::new(
                PageTopBlockPosition::new(0.0),
                FlexFragmentBlockOffset::new(0.0),
            ),
            unit_block_start: FlexFragmentBlockOffset::new(20.0),
            unit_block_end: FlexFragmentBlockOffset::new(40.0),
            current_fragmentainer,
            break_opportunity: opportunity,
            can_advance: true,
        });

        assert!(page_decision.transition_before_unit.is_none());
        let column_transition = column_decision
            .transition_before_unit
            .expect("column avoid should advance before the flex unit");
        assert_eq!(
            column_transition.fragmentainer_kind,
            FragmentainerKind::Column
        );
    }

    #[test]
    fn flex_unit_prebreak_advances_sole_unit_from_exhausted_fragmentainer() {
        let decision = FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
            fragmentainer_kind: FragmentainerKind::Column,
            break_is_applicable: true,
            unit_is_oversized: false,
            has_prior_unit: false,
            has_later_unit: false,
            cursor: FlexFragmentCursor::new(
                PageTopBlockPosition::new(0.0),
                FlexFragmentBlockOffset::new(0.0),
            ),
            unit_block_start: FlexFragmentBlockOffset::new(0.0),
            unit_block_end: FlexFragmentBlockOffset::new(75.0),
            current_fragmentainer: Fragmentainer::new(layout_pt(75.0), layout_pt(0.0)),
            break_opportunity: FragmentBreakOpportunity::before_box_boundary(
                FragmentainerKind::Column,
                0.0,
                FragmentBreakContext::new(
                    PageBreak::Auto,
                    PageBreak::Auto,
                    PageBreak::Auto,
                    PageBreak::Auto,
                ),
                PageBreak::Auto,
                false,
            ),
            can_advance: true,
        });

        assert_eq!(
            decision
                .transition_before_unit
                .expect("a sole flex unit advances out of an exhausted column")
                .reason,
            FlexFragmentBreakReason::OverflowOrAvoid
        );
    }

    #[test]
    fn flex_unit_prebreak_preserves_remaining_source_gap() {
        let decision = FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
            fragmentainer_kind: FragmentainerKind::Page,
            break_is_applicable: true,
            // The next item itself fits in an empty fragmentainer. Its source
            // start is separated from the preceding item by a gap, of which
            // 18pt remain after this fragmentainer's 18pt capacity.
            unit_is_oversized: false,
            has_prior_unit: true,
            has_later_unit: true,
            cursor: FlexFragmentCursor::new(
                PageTopBlockPosition::new(0.0),
                FlexFragmentBlockOffset::new(90.0),
            ),
            unit_block_start: FlexFragmentBlockOffset::new(126.0),
            unit_block_end: FlexFragmentBlockOffset::new(162.0),
            current_fragmentainer: Fragmentainer::new(layout_pt(144.0), layout_pt(18.0)),
            break_opportunity: FragmentBreakOpportunity::before_box_boundary(
                FragmentainerKind::Page,
                126.0,
                FragmentBreakContext::new(
                    PageBreak::Auto,
                    PageBreak::Auto,
                    PageBreak::Auto,
                    PageBreak::Auto,
                ),
                PageBreak::Auto,
                false,
            ),
            can_advance: true,
        });

        assert_eq!(
            decision
                .transition_before_unit
                .expect("the gap-separated item moves to the next fragmentainer")
                .next_block_offset,
            FlexFragmentBlockOffset::new(108.0),
        );
    }

    #[test]
    fn flex_fragment_transition_page_cursor_gate_is_target_specific() {
        let page_transition = FlexFragmentTransitionDecision::forced(
            FragmentainerKind::Page,
            FlexFragmentBlockOffset::new(40.0),
        );
        let column_transition = FlexFragmentTransitionDecision::forced(
            FragmentainerKind::Column,
            FlexFragmentBlockOffset::new(40.0),
        );

        assert!(page_transition.materializes_page_cursor());
        assert!(!column_transition.materializes_page_cursor());
        assert_eq!(
            column_transition.cursor_after_fragmentainer_advance(PageTopBlockPosition::new(200.0)),
            FlexFragmentCursor::new(
                PageTopBlockPosition::new(200.0),
                FlexFragmentBlockOffset::new(40.0)
            )
        );
    }

    #[test]
    fn single_line_row_continuation_fills_its_final_fragment() {
        assert_eq!(
            single_line_row_fragmented_cross_size(
                FlexCrossSize::new(112.5),
                FlexFragmentBlockSize::new(100.0),
                FlexFragmentBlockSize::new(100.0),
            ),
            Some(FlexCrossSize::new(200.0))
        );
        assert_eq!(
            single_line_row_fragmented_cross_size(
                FlexCrossSize::new(100.0),
                FlexFragmentBlockSize::new(100.0),
                FlexFragmentBlockSize::new(100.0),
            ),
            None
        );
    }

    fn test_flex_item_fragment(item_index: usize, item: FlexItemLayout) -> FlexItemFragmentLayout {
        FlexItemFragmentLayout {
            item_index,
            source_item_index: item_index,
            line_index: 0,
            source_bounds: item.clone(),
            used_bounds: item.clone(),
            bounds: item.clone(),
            content_slice: FlexFragmentSlice {
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(item.height().points()),
            },
            decoration_slice: FlexFragmentSlice {
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(item.height().points()),
            },
            continuation: FlexItemContinuation::default(),
            metadata: FragmentPageMetadata::empty(0),
        }
    }

    fn test_flex_line(
        item_indices: Vec<usize>,
        main_start: FlexMainOffset,
        main_end: FlexMainOffset,
        cross_start: FlexCrossOffset,
        cross_end: FlexCrossOffset,
    ) -> FlexLineLayout {
        FlexLineLayout {
            logical_cross_start_rank: 0,
            source_start: item_indices.iter().cloned().min().unwrap_or(0),
            source_end: item_indices
                .iter()
                .cloned()
                .max()
                .map(|index| index + 1)
                .unwrap_or(0),
            item_indices,
            main_start,
            main_end,
            cross_start,
            cross_end,
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        }
    }
}
