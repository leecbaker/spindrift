use super::*;

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
        style.box_values.margin.left = zero;
        style.margin.left = 0.0;
    }
    if style.box_values.margin.right.is_auto() {
        style.box_values.margin.right = zero;
        style.margin.right = 0.0;
    }
    if style.box_values.margin.top.is_auto() {
        style.box_values.margin.top = zero;
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
    percentage_basis: f32,
) -> Option<f32> {
    if definite_content_height.is_some() || style.flex_wrap == FlexWrap::NoWrap {
        return definite_content_height;
    }
    if !style.flex_direction.is_column_axis() {
        return definite_content_height;
    }
    used_max_height(style, percentage_basis)
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
    percentage_basis: f32,
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
    Some(constrain_height(
        style,
        content_height.max(0.0),
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

pub(in crate::layout::flex) fn flex_break_units(
    flex_layout: &FlexLayout,
    children: &[StyledChild<'_>],
    style: &ComputedStyle,
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
                    .copied()
                    .filter(|&index| {
                        children
                            .get(index)
                            .is_some_and(|child| !flex_item_is_collapsed(&child.style))
                    })
                    .collect::<Vec<_>>();
                (!item_indices.is_empty()).then(|| FlexBreakUnit {
                    line_start: line_index,
                    line_end: line_index + 1,
                    block_start: line.cross_start,
                    block_end: line.cross_end,
                    break_before: flex_unit_break_before(&item_indices, children),
                    break_after: flex_unit_break_after(&item_indices, children),
                    break_inside_avoid: item_indices
                        .iter()
                        .any(|&index| children[index].style.break_inside_avoid),
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

    let mut units = flex_layout
        .items
        .iter()
        .enumerate()
        .filter(|(index, _)| !flex_item_is_collapsed(&children[*index].style))
        .map(|(index, item)| {
            let (block_start, block_end) = flex_item_block_bounds(item);
            let (line_start, line_end) = flex_item_line_range(flex_layout, index);
            FlexBreakUnit {
                item_indices: vec![index],
                line_start,
                line_end,
                block_start,
                block_end,
                break_before: children[index].style.break_before,
                break_after: children[index].style.break_after,
                break_inside_avoid: children[index].style.break_inside_avoid,
            }
        })
        .collect::<Vec<_>>();
    units.sort_by(|a, b| {
        a.block_start
            .partial_cmp(&b.block_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    units
}

pub(in crate::layout::flex) fn flex_fragment_from_break_unit(
    unit: &FlexBreakUnit,
    items: &[FlexItemLayout],
    context: FlexFragmentBuildContext,
) -> FlexFragmentLayout {
    let fragment_height = unit.block_size();
    let fragment_bottom = context.content_top - (unit.block_end - context.block_offset);
    FlexFragmentLayout {
        page_index: context.page_index,
        line_start: unit.line_start,
        line_end: unit.line_end,
        block_start: unit.block_start,
        block_end: unit.block_end,
        items: unit
            .item_indices
            .iter()
            .filter_map(|&item_index| {
                let item = items.get(item_index)?;
                let (item_block_start, item_block_end) = flex_item_block_bounds(item);
                let slice_start = item_block_start.max(unit.block_start);
                let slice_end = item_block_end.min(unit.block_end);
                if slice_end <= slice_start + 0.01 {
                    return None;
                }
                let mut bounds = item.clone();
                bounds.set_y(slice_start);
                bounds.set_height((slice_end - slice_start).max(0.0));
                let content_slice = FlexFragmentSlice {
                    block_start: (slice_start - item_block_start).max(0.0),
                    block_end: (slice_end - item_block_start).min(item.height().max(0.0)),
                };
                Some(FlexItemFragmentLayout {
                    item_index,
                    source_item_index: item_index,
                    original_bounds: item.clone(),
                    bounds,
                    content_slice,
                    decoration_slice: content_slice,
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
) -> Option<(f32, f32)> {
    plan.fragments
        .iter()
        .filter(|fragment| fragment.page_index == page_index)
        .fold(None, |range, fragment| {
            Some(match range {
                Some((start, end)) => (
                    f32::min(start, fragment.block_start),
                    f32::max(end, fragment.block_end),
                ),
                None => (fragment.block_start, fragment.block_end),
            })
        })
}

pub(in crate::layout::flex) fn flex_gap_decoration_primitives_for_page(
    flex_layout: &FlexLayout,
    style: &ComputedStyle,
    page_index: usize,
    inner_x: f32,
    inner_width: f32,
    total_content_height: f32,
    fragment_bounds: PaintClip,
) -> Vec<PaintPrimitive> {
    let Some((block_start, block_end)) =
        flex_page_fragment_block_range(&flex_layout.fragment_plan, page_index)
    else {
        return Vec::new();
    };
    let fragment_height = (block_end - block_start).max(0.0);
    if fragment_height <= 0.01 {
        return Vec::new();
    }

    let mut gutters =
        flex_gap_decoration_gutters(flex_layout, style, inner_width, total_content_height);
    gutters.rows = flex_fragment_gap_gutters(&gutters.rows, block_start, block_end);
    let items = flex_layout
        .fragment_plan
        .fragments
        .iter()
        .filter(|fragment| fragment.page_index == page_index)
        .flat_map(|fragment| &fragment.items)
        .filter_map(|item| {
            let bounds = &item.bounds;
            (bounds.height() > 0.01).then(|| {
                GapDecorationItem::new(
                    bounds.x(),
                    bounds.y() - block_start,
                    bounds.width(),
                    bounds.height(),
                )
            })
        })
        .collect::<Vec<_>>();

    flex_gap_decoration_primitives_with_gutters(
        style,
        inner_x,
        fragment_bounds.y() + fragment_bounds.height(),
        inner_width,
        fragment_height,
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
        .map(|item| GapDecorationItem::new(item.x(), item.y(), item.width(), item.height()))
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
            let start = gutter.start.max(block_start);
            let end = gutter.end.min(block_end);
            (end > start + 0.01)
                .then(|| GapDecorationGutter::new(start - block_start, end - block_start))
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

pub(in crate::layout::flex) fn flex_item_block_bounds(item: &FlexItemLayout) -> (f32, f32) {
    (item.y(), item.y() + item.height())
}

pub(in crate::layout::flex) fn flex_gap_decoration_gutters(
    flex_layout: &FlexLayout,
    style: &ComputedStyle,
    content_width: f32,
    content_height: f32,
) -> GapDecorationGutters {
    let axes = FlexAxes::for_style(style);
    let (physical_gap_width, physical_gap_height) = physical_flex_gaps(style);
    let used_physical_gap_width = used_flex_gap(physical_gap_width, content_width);
    let used_physical_gap_height = used_flex_gap(physical_gap_height, content_height);
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
    let main_gutters = flex_main_axis_gap_gutters(flex_layout, axes, main_gap);
    let cross_gutters = flex_cross_axis_gap_gutters(flex_layout, cross_gap);
    if axes.is_main_row_axis() {
        GapDecorationGutters {
            columns: main_gutters,
            rows: cross_gutters,
        }
    } else {
        GapDecorationGutters {
            columns: cross_gutters,
            rows: main_gutters,
        }
    }
}

pub(in crate::layout::flex) fn flex_main_axis_gap_gutters(
    flex_layout: &FlexLayout,
    axes: FlexAxes,
    used_gap: f32,
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
        for pair in line_items.windows(2) {
            let start = pair[0].main_start(axes) + pair[0].main_size(axes);
            let end = pair[1].main_start(axes);
            push_unique_flex_gap_gutter(&mut gutters, start, end, used_gap);
        }
    }
    gutters.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.end
                    .partial_cmp(&b.end)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    gutters
}

pub(in crate::layout::flex) fn flex_cross_axis_gap_gutters(
    flex_layout: &FlexLayout,
    used_gap: f32,
) -> Vec<GapDecorationGutter> {
    let mut lines = flex_layout.lines.iter().collect::<Vec<_>>();
    lines.sort_by(|a, b| {
        a.cross_start
            .partial_cmp(&b.cross_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut gutters = Vec::new();
    for pair in lines.windows(2) {
        push_unique_flex_gap_gutter(
            &mut gutters,
            pair[0].cross_end,
            pair[1].cross_start,
            used_gap,
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
    let size = used_gap.min(available).max(0.0);
    let start = start + (available - size) * 0.5;
    let end = start + size;
    if gutters
        .iter()
        .any(|gutter| (gutter.start - start).abs() <= 0.01 && (gutter.end - end).abs() <= 0.01)
    {
        return;
    }
    gutters.push(GapDecorationGutter::new(start, end));
}

pub(in crate::layout::flex) fn flex_unit_break_before(
    item_indices: &[usize],
    children: &[StyledChild<'_>],
) -> PageBreak {
    item_indices
        .iter()
        .map(|&index| children[index].style.break_before)
        .fold(PageBreak::Auto, combine_flex_break)
}

pub(in crate::layout::flex) fn flex_unit_break_after(
    item_indices: &[usize],
    children: &[StyledChild<'_>],
) -> PageBreak {
    item_indices
        .iter()
        .map(|&index| children[index].style.break_after)
        .fold(PageBreak::Auto, combine_flex_break)
}

pub(in crate::layout::flex) fn combine_flex_break(
    current: PageBreak,
    candidate: PageBreak,
) -> PageBreak {
    if current.is_forced() {
        current
    } else if candidate.is_forced() || candidate.avoids_page() {
        candidate
    } else {
        current
    }
}

/// Consume flex item break requests at the flex-container layer.
///
/// CSS Flexbox fragmentation handles forced breaks from flex items as breaks
/// between flex lines/container fragments. They must not be re-applied when
/// each item is laid out through the block-layout entrypoint, or a
/// `break-before`/`page-break-before` on an item can incorrectly push that item
/// to a standalone PDF page:
/// <https://drafts.csswg.org/css-flexbox-1/#pagination>.
pub(in crate::layout::flex) fn suppress_flex_item_fragmentation_breaks(style: &mut ComputedStyle) {
    style.break_before = PageBreak::Auto;
    style.break_after = PageBreak::Auto;
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
                FlexItemLayout::new(0.0, 0.0, 30.0, 20.0),
                FlexItemLayout::new(40.0, 0.0, 30.0, 20.0),
                FlexItemLayout::new(0.0, 30.0, 40.0, 20.0),
                FlexItemLayout::new(50.0, 30.0, 30.0, 20.0),
            ],
            lines: vec![
                test_flex_line(vec![0, 1], 0.0, 70.0, 0.0, 20.0),
                test_flex_line(vec![2, 3], 0.0, 80.0, 30.0, 50.0),
            ],
            fragment_plan: FlexFragmentPlan::default(),
        };

        let gutters = flex_gap_decoration_gutters(&flex_layout, &style, 100.0, 50.0);

        assert_eq!(gutters.columns.len(), 2);
        assert_eq!(gutters.columns[0].start, 30.0);
        assert_eq!(gutters.columns[0].end, 40.0);
        assert_eq!(gutters.columns[1].start, 40.0);
        assert_eq!(gutters.columns[1].end, 50.0);
        assert_eq!(gutters.rows.len(), 1);
        assert_eq!(gutters.rows[0].start, 20.0);
        assert_eq!(gutters.rows[0].end, 30.0);

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
        let left = FlexItemLayout::new(0.0, 20.0, 30.0, 50.0);
        let right = FlexItemLayout::new(40.0, 20.0, 30.0, 50.0);
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
                    block_start: 20.0,
                    block_end: 70.0,
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
        );
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].x1(), 35.0);
        assert_eq!(strokes[0].y1(), 150.0);
        assert_eq!(strokes[0].y2(), 100.0);
        assert_eq!(strokes[0].width, 4.0);
    }

    fn test_flex_item_fragment(item_index: usize, item: FlexItemLayout) -> FlexItemFragmentLayout {
        FlexItemFragmentLayout {
            item_index,
            source_item_index: item_index,
            original_bounds: item.clone(),
            bounds: item.clone(),
            content_slice: FlexFragmentSlice::full(item.height()),
            decoration_slice: FlexFragmentSlice::full(item.height()),
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
            source_start: item_indices.iter().copied().min().unwrap_or(0),
            source_end: item_indices
                .iter()
                .copied()
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
