use super::*;
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
    definite_content_height: Option<f32>,
    percentage_basis: PercentageBasis<LayoutLength>,
) -> Option<f32> {
    if definite_content_height.is_some() || style.flex_wrap == FlexWrap::NoWrap {
        return definite_content_height;
    }
    if !style.flex_direction.is_column_axis() {
        return definite_content_height;
    }
    used_max_height(style, percentage_basis).map(SemanticLengthExt::points)
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
    explicit_content_height: Option<f32>,
    content_width: f32,
    percentage_basis: PercentageBasis<LayoutLength>,
    horizontal_non_content: f32,
    vertical_non_content: f32,
) -> Option<f32> {
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
            let border_box_width = content_width + horizontal_non_content;
            (border_box_width / ratio) - vertical_non_content
        }
    };
    Some(
        constrain_content_height(
            style,
            content_box_pt(content_height.max(0.0)),
            percentage_basis,
        )
        .points(),
    )
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
    block_top: f32,
    total_height: f32,
    page_top: f32,
    page_bottom: f32,
    page_area_height: f32,
) -> bool {
    let overflows_current_page = block_top - total_height < page_bottom;
    let starts_at_page_top = (block_top - page_top).abs() < 0.01;
    overflows_current_page && !starts_at_page_top && total_height <= page_area_height + 0.01
}

/// Returns the physical block extent occupied by a fragmented single-line row
/// flex container.
///
/// Each continuation reruns cross-axis alignment in its own fragmentainer.
/// A stretched item therefore occupies the complete content box of its final
/// continuation even when its remaining source content is shorter. This
/// helper maps an unfragmented source cross-size to those fragment-local
/// content-box extents without guessing at individual test geometry:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
pub(in crate::layout::flex) fn single_line_row_fragmented_cross_size(
    source_cross_size: f32,
    first_fragment_capacity: f32,
    continuation_fragment_capacity: f32,
) -> Option<f32> {
    let first_fragment_capacity = first_fragment_capacity.max(0.0);
    let continuation_fragment_capacity = continuation_fragment_capacity.max(0.0);
    if source_cross_size <= first_fragment_capacity + 0.01 || continuation_fragment_capacity <= 0.01
    {
        return None;
    }
    let continuation_count = ((source_cross_size - first_fragment_capacity)
        / continuation_fragment_capacity)
        .ceil()
        .max(1.0);
    Some(first_fragment_capacity + continuation_count * continuation_fragment_capacity)
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
    let physical_direction = physical_flex_direction(style);
    if physical_direction.is_row_axis() {
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
                (!item_indices.is_empty()).then(|| FlexBreakUnit {
                    line_start: line_index,
                    line_end: line_index + 1,
                    block_start: line.cross_start.points(),
                    block_end: line.cross_end.points(),
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
        return units;
    }

    // Wrapped column flex lines can overlap in the physical block direction:
    // each line has its own physical horizontal cross-axis position while its
    // items share the same vertical main-axis range. A fragmentainer must
    // slice all coincident ranges together, rather than serializing one flex
    // line after another and incorrectly consuming columns for each item.
    // <https://www.w3.org/TR/css-flexbox-1/#pagination>
    let mut units = Vec::<FlexBreakUnit>::new();
    for (index, item) in flex_layout.items.iter().enumerate() {
        if flex_item_is_collapsed(&children[index].style) {
            continue;
        }
        let (block_start, block_end) = flex_item_block_bounds(item, use_fragmentation_height);
        let (line_start, line_end) = flex_item_line_range(flex_layout, index);
        if let Some(unit) = units.iter_mut().find(|unit| {
            (unit.block_start - block_start).abs() <= 0.01
                && (unit.block_end - block_end).abs() <= 0.01
        }) {
            unit.item_indices.push(index);
            unit.line_start = unit.line_start.min(line_start);
            unit.line_end = unit.line_end.max(line_end);
            unit.break_before = fragmentainer_kind
                .combine_break(unit.break_before, children[index].style.break_before);
            unit.break_after = fragmentainer_kind
                .combine_break(unit.break_after, children[index].style.break_after);
            unit.break_inside_avoid |=
                fragmentainer_kind.avoids_break_inside(&children[index].style);
        } else {
            units.push(FlexBreakUnit {
                item_indices: vec![index],
                line_start,
                line_end,
                block_start,
                block_end,
                break_before: children[index].style.break_before,
                break_after: children[index].style.break_after,
                break_inside_avoid: fragmentainer_kind.avoids_break_inside(&children[index].style),
            });
        }
    }
    units.sort_by(|a, b| {
        a.block_start
            .partial_cmp(&b.block_start)
            .unwrap_or(std::cmp::Ordering::Equal)
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
    items: &[FlexItemLayout],
    context: FlexFragmentBuildContext,
    use_fragmentation_height: bool,
) -> FlexFragmentLayout {
    let fragment_height = unit.block_size();
    let fragment_bottom = context.content_top - (unit.block_end - context.block_offset);
    FlexFragmentLayout {
        page_index: context.page_index,
        line_start: unit.line_start,
        line_end: unit.line_end,
        block_start: FlexFragmentBlockOffset::new(unit.block_start),
        block_end: FlexFragmentBlockOffset::new(unit.block_end),
        items: unit
            .item_indices
            .iter()
            .filter_map(|&item_index| {
                let item = items.get(item_index)?;
                let (item_block_start, item_block_end) =
                    flex_item_block_bounds(item, use_fragmentation_height);
                let slice_start = item_block_start.max(unit.block_start);
                let slice_end = item_block_end.min(unit.block_end);
                if slice_end <= slice_start + 0.01 {
                    return None;
                }
                let mut source_bounds = item.clone();
                if use_fragmentation_height {
                    source_bounds.set_height(item.fragmentation_height());
                }
                let mut bounds = source_bounds.clone();
                bounds.set_y(slice_start);
                bounds.set_height((slice_end - slice_start).max(0.0));
                let content_slice = FlexFragmentSlice {
                    block_start: FlexFragmentBlockOffset::new(
                        (slice_start - item_block_start).max(0.0),
                    ),
                    block_end: FlexFragmentBlockOffset::new(
                        (slice_end - item_block_start).min(item.height().max(0.0)),
                    ),
                };
                Some(FlexItemFragmentLayout {
                    item_index,
                    source_item_index: item_index,
                    original_bounds: source_bounds,
                    bounds,
                    content_slice,
                    decoration_slice: content_slice,
                    continuation: FlexItemContinuation {
                        source_content_slice: content_slice,
                        decoration_slice: content_slice,
                        first_fragmentainer_capacity: context.first_fragmentainer_capacity,
                        continuation_fragmentainer_capacity: context
                            .continuation_fragmentainer_capacity,
                        fragmentainer_index: context.page_index,
                        continuation_ordinal: 0,
                    },
                    metadata: FragmentPageMetadata::empty(context.page_index),
                })
            })
            .collect(),
        metadata: FragmentPageMetadata::new(
            context.page_index,
            Some(PaintClip::from_paint_rect(paint_space_rect(
                context.outer_x,
                fragment_bottom,
                context.outer_width,
                fragment_height,
            ))),
            context.starts_page_fragment,
        ),
    }
}

pub(in crate::layout::flex) fn flex_container_page_fragment_bounds(
    plan: &FlexFragmentPlan,
    page_index: usize,
) -> Option<PaintClip> {
    plan.fragments
        .iter()
        .filter(|fragment| fragment.page_index == page_index)
        .filter_map(|fragment| fragment.metadata.source_border_box)
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

pub(in crate::layout::flex) fn flex_page_fragment_block_range(
    plan: &FlexFragmentPlan,
    page_index: usize,
    retain_distributed_gap: bool,
) -> Option<(f32, f32)> {
    let range = plan
        .fragments
        .iter()
        .filter(|fragment| fragment.page_index == page_index)
        .fold(None, |range, fragment| {
            Some(match range {
                Some((start, end)) => (
                    f32::min(start, fragment.block_start.points()),
                    f32::max(end, fragment.block_end.points()),
                ),
                None => (fragment.block_start.points(), fragment.block_end.points()),
            })
        });
    range.map(|(start, end)| {
        if !retain_distributed_gap {
            return (start, end);
        }
        let next_start = plan
            .fragments
            .iter()
            .filter(|fragment| fragment.page_index > page_index)
            .map(|fragment| fragment.block_start.points())
            .filter(|next_start| *next_start > end + 0.01)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        (start, next_start.unwrap_or(end))
    })
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout::flex) fn flex_gap_decoration_primitives_for_page(
    flex_layout: &FlexLayout,
    style: &ComputedStyle,
    page_index: usize,
    inner_x: f32,
    inner_width: f32,
    total_content_height: f32,
    fragment_bounds: PaintClip,
    has_forced_item_breaks: bool,
) -> Vec<PaintPrimitive> {
    let Some((block_start, block_end)) = flex_page_fragment_block_range(
        &flex_layout.fragment_plan,
        page_index,
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
    let fragment_height = (block_end - block_start).max(0.0);
    if fragment_height <= 0.01 {
        return Vec::new();
    }

    let mut gutters =
        flex_gap_decoration_gutters(flex_layout, style, inner_width, total_content_height);
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
    gutters.rows = if has_forced_item_breaks {
        // Forced breaks between flex items/lines replace the intervening row
        // gutter with a fragmentainer boundary. No fragment owns that gutter,
        // so it contributes no row-rule segment on either side.
        Vec::new()
    } else {
        flex_fragment_gap_gutters(&gutters.rows, block_start, block_end)
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
                GapDecorationPoint::new(item.x(), item.y() - block_start),
                GapDecorationSize::new(item.width(), item.height()),
            ))
        })
        .collect::<Vec<_>>();

    flex_gap_decoration_primitives_with_gutters(
        style,
        GapDecorationContainer::new(
            inner_x,
            fragment_bounds.y() + fragment_bounds.height(),
            inner_width,
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
                GapDecorationPoint::new(item.x(), item.y()),
                GapDecorationSize::new(item.width(), item.height()),
            ))
        })
        .collect()
}

pub(in crate::layout::flex) fn flex_fragment_gap_gutters(
    gutters: &[GapDecorationGutter],
    block_start: f32,
    block_end: f32,
) -> Vec<GapDecorationGutter> {
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
) -> (f32, f32) {
    let height = if use_fragmentation_height {
        item.fragmentation_height()
    } else {
        item.height()
    };
    (item.y(), item.y() + height)
}

pub(in crate::layout::flex) fn flex_gap_decoration_gutters(
    flex_layout: &FlexLayout,
    style: &ComputedStyle,
    content_width: f32,
    content_height: f32,
) -> GapDecorationGutters {
    let axes = FlexAxes::for_style(style);
    let PhysicalFlexGaps {
        horizontal: physical_gap_width,
        vertical: physical_gap_height,
    } = physical_flex_gaps(style);
    let used_physical_gap_width = used_flex_gap(
        physical_gap_width,
        PercentageBasis::definite(content_box_pt(content_width)),
    )
    .points();
    let used_physical_gap_height = used_flex_gap(
        physical_gap_height,
        PercentageBasis::definite(content_box_pt(content_height)),
    )
    .points();
    let main_gap = if axes.is_main_row_axis() {
        used_physical_gap_width
    } else {
        used_physical_gap_height
    };
    let cross_gap = if axes.is_main_row_axis() {
        used_physical_gap_height
    } else {
        used_physical_gap_width
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
        main_gap <= 0.01
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
    used_gap: f32,
    cross_gap: f32,
    has_distributed_gutters: bool,
    distribute_fractional_remainder: bool,
) -> Vec<GapDecorationGutter> {
    let mut gutters = Vec::new();
    for line in &flex_layout.lines {
        let mut line_items = line
            .item_indices
            .iter()
            .filter_map(|&index| flex_layout.items.get(index))
            .filter(|item| item.main_size(axes) > 0.01)
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
            let start = pair[0].main_start(axes) + pair[0].main_size(axes) + remainder_offset;
            let end = pair[1].main_start(axes) + remainder_offset;
            if let Some((segment_start, segment_end)) =
                line_cross_range.map(|(start, end)| (start.points(), end.points()))
            {
                push_unique_flex_gap_gutter_with_segment(
                    &mut gutters,
                    start,
                    end,
                    if has_distributed_gutters {
                        f32::INFINITY
                    } else {
                        used_gap
                    },
                    segment_start,
                    segment_end,
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
            && segment.start <= previous_segment.end + cross_gap + 0.01
            && previous_segment.start <= segment.end + cross_gap + 0.01
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
    used_gap: f32,
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
                        let start = item.cross_start(axes);
                        let end = start + item.cross_size(axes);
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
            pair[0].1,
            pair[1].0,
            if has_distributed_gutters {
                f32::INFINITY
            } else {
                used_gap
            },
        );
    }
    gutters
}

pub(in crate::layout::flex) fn push_unique_flex_gap_gutter(
    gutters: &mut Vec<GapDecorationGutter>,
    start: f32,
    end: f32,
    used_gap: f32,
) {
    if end <= start + 0.01 || used_gap <= 0.01 {
        return;
    }
    let available = end - start;
    let size = if used_gap.is_infinite() {
        available
    } else {
        used_gap.min(available).max(0.0)
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

pub(in crate::layout::flex) fn push_unique_flex_gap_gutter_with_segment(
    gutters: &mut Vec<GapDecorationGutter>,
    start: f32,
    end: f32,
    used_gap: f32,
    segment_start: f32,
    segment_end: f32,
) {
    if end <= start + 0.01 || used_gap <= 0.01 || segment_end <= segment_start + 0.01 {
        return;
    }
    let available = end - start;
    // Distributed alignment increases the effective gutter between adjacent
    // items; the decoration is centered in that entire resolved gutter.
    // https://drafts.csswg.org/css-align-3/#gap-legacy
    let size = if used_gap.is_infinite() {
        available
    } else {
        used_gap.min(available).max(0.0)
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
    fn flex_gap_gutters_use_line_local_main_axis_gaps() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Row;
        style.column_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        style.row_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        let flex_layout = FlexLayout {
            height: 50.0,
            first_baseline: Some(0.0),
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
                test_flex_line(vec![0, 1], 0.0, 70.0, 0.0, 20.0),
                test_flex_line(vec![2, 3], 0.0, 80.0, 30.0, 50.0),
            ],
            fragment_plan: FlexFragmentPlan::default(),
        };

        let gutters = flex_gap_decoration_gutters(&flex_layout, &style, 100.0, 50.0);

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
        let no_gap_gutters = flex_gap_decoration_gutters(&flex_layout, &no_gap_style, 100.0, 50.0);
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
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        let left = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 20.0),
            ContainerSize::new(30.0, 50.0),
        ));
        let right = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(40.0, 20.0),
            ContainerSize::new(30.0, 50.0),
        ));
        let flex_layout = FlexLayout {
            height: 70.0,
            first_baseline: Some(0.0),
            items: vec![left.clone(), right.clone()],
            lines: vec![test_flex_line(vec![0, 1], 0.0, 70.0, 20.0, 70.0)],
            fragment_plan: FlexFragmentPlan {
                fragments: vec![FlexFragmentLayout {
                    page_index: 0,
                    line_start: 0,
                    line_end: 1,
                    block_start: FlexFragmentBlockOffset::new(20.0),
                    block_end: FlexFragmentBlockOffset::new(70.0),
                    items: vec![
                        test_flex_item_fragment(0, left),
                        test_flex_item_fragment(1, right),
                    ],
                    metadata: FragmentPageMetadata::empty(0),
                }],
            },
        };

        let primitives = flex_gap_decoration_primitives_for_page(
            &flex_layout,
            &style,
            0,
            0.0,
            70.0,
            70.0,
            PaintClip::new(0.0, 100.0, 70.0, 50.0),
            false,
        );
        let strokes = solid_gap_rule_centerlines(&primitives);

        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].x1(), 35.0);
        assert_eq!(strokes[0].y1(), 150.0);
        assert_eq!(strokes[0].y2(), 100.0);
        assert_eq!(strokes[0].width, 4.0);
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
        let current_fragmentainer = Fragmentainer::new(100.0, 10.0);

        let page_decision = FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
            fragmentainer_kind: FragmentainerKind::Page,
            break_is_applicable: true,
            unit_is_oversized: false,
            has_prior_unit: false,
            has_later_unit: false,
            cursor: FlexFragmentCursor::new(0.0, 0.0),
            unit_block_start: 20.0,
            unit_block_end: 40.0,
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
            cursor: FlexFragmentCursor::new(0.0, 0.0),
            unit_block_start: 20.0,
            unit_block_end: 40.0,
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
    fn flex_fragment_transition_page_cursor_gate_is_target_specific() {
        let page_transition = FlexFragmentTransitionDecision::forced(FragmentainerKind::Page, 40.0);
        let column_transition =
            FlexFragmentTransitionDecision::forced(FragmentainerKind::Column, 40.0);

        assert!(page_transition.materializes_page_cursor());
        assert!(!column_transition.materializes_page_cursor());
        assert_eq!(
            column_transition.cursor_after_fragmentainer_advance(200.0),
            FlexFragmentCursor::new(200.0, 40.0)
        );
    }

    #[test]
    fn single_line_row_continuation_fills_its_final_fragment() {
        assert_eq!(
            single_line_row_fragmented_cross_size(112.5, 100.0, 100.0),
            Some(200.0)
        );
        assert_eq!(
            single_line_row_fragmented_cross_size(100.0, 100.0, 100.0),
            None
        );
    }

    fn test_flex_item_fragment(item_index: usize, item: FlexItemLayout) -> FlexItemFragmentLayout {
        FlexItemFragmentLayout {
            item_index,
            source_item_index: item_index,
            original_bounds: item.clone(),
            bounds: item.clone(),
            content_slice: FlexFragmentSlice {
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(item.height()),
            },
            decoration_slice: FlexFragmentSlice {
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(item.height()),
            },
            continuation: FlexItemContinuation::default(),
            metadata: FragmentPageMetadata::empty(0),
        }
    }

    fn test_flex_line(
        item_indices: Vec<usize>,
        main_start: f32,
        main_end: f32,
        cross_start: f32,
        cross_end: f32,
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
            main_start: FlexMainOffset::new(main_start),
            main_end: FlexMainOffset::new(main_end),
            cross_start: FlexCrossOffset::new(cross_start),
            cross_end: FlexCrossOffset::new(cross_end),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        }
    }
}
