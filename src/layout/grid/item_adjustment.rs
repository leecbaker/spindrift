use super::model::{GridAxisTopology, GridItemArea, GridItemLayout};
use super::*;

/// Correct same-page grid self-alignment values outside Taffy's model.
///
/// Taffy 0.13 resolves `self-start`/`self-end` natively for horizontal-tb
/// alignment subjects. Spindrift still resolves them for vertical-writing subjects,
/// whose physical sides cannot be represented by Taffy's horizontal-tb model.
/// Physical `left`/`right` self-position values likewise bypass Taffy's
/// direction-sensitive start/end mapping. The correction uses effective
/// `justify-self` and `align-self` values, so container defaults follow the
/// same path:
/// <https://www.w3.org/TR/css-align-3/#self-alignment> and
/// <https://www.w3.org/TR/css-grid-1/#alignment>.
pub(super) fn apply_grid_self_alignment_corrections(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    container_width: PhysicalContentWidth,
    container_height: f32,
    columns: &GridAxisTopology,
    rows: &GridAxisTopology,
    items: &mut [GridItemLayout],
) {
    if WritingModeAxes::new(container_style.writing_mode, container_style.direction)
        .swaps_physical_axes()
    {
        return;
    }
    for (index, item) in items.iter_mut().enumerate() {
        let Some(area) = item.area else {
            continue;
        };
        let child_style = &children[index].style;
        let justify_self = effective_grid_justify_self(child_style, container_style);
        if let Some(x) = horizontal_self_alignment_offset(
            justify_self,
            child_style,
            container_style.justify_content,
            container_width,
            area,
            columns,
            item.width(),
        ) {
            item.set_axis_geometry(GridAxis::Column, x, item.width());
        }
        let align_self = effective_grid_align_self(child_style, container_style);
        if let Some(y) = vertical_self_alignment_offset(
            align_self,
            child_style,
            container_style.align_content,
            container_height,
            area,
            rows,
            item.height(),
        ) {
            item.set_axis_geometry(GridAxis::Row, y, item.height());
        }
    }
}

fn horizontal_self_alignment_offset(
    justify_self: JustifySelf,
    child_style: &ComputedStyle,
    justify_content: css::JustifyContent,
    container_width: PhysicalContentWidth,
    area: GridItemArea,
    columns: &GridAxisTopology,
    item_width: f32,
) -> Option<f32> {
    let side = match justify_self.keyword {
        SelfAlignmentKeyword::Left => Some(PhysicalSide::Left),
        SelfAlignmentKeyword::Right => Some(PhysicalSide::Right),
        SelfAlignmentKeyword::SelfStart if child_style.writing_mode.has_vertical_lines() => {
            grid_subject_self_start_side(child_style, PhysicalAxis::Horizontal)
        }
        SelfAlignmentKeyword::SelfEnd if child_style.writing_mode.has_vertical_lines() => {
            grid_subject_self_end_side(child_style, PhysicalAxis::Horizontal)
        }
        _ => None,
    }?;
    self_alignment_offset_for_side(
        side,
        SelfAlignmentAxisContext {
            axis: PhysicalAxis::Horizontal,
            content_alignment: justify_content,
            container_size: container_width.points(),
            topology: columns,
            start_line: area.column_start,
            end_line: area.column_end,
        },
        item_width,
    )
}

fn vertical_self_alignment_offset(
    align_self: AlignSelf,
    child_style: &ComputedStyle,
    align_content: css::AlignContent,
    container_height: f32,
    area: GridItemArea,
    rows: &GridAxisTopology,
    item_height: f32,
) -> Option<f32> {
    let side = match align_self.keyword {
        SelfAlignmentKeyword::SelfStart if child_style.writing_mode.has_vertical_lines() => {
            grid_subject_self_start_side(child_style, PhysicalAxis::Vertical)
        }
        SelfAlignmentKeyword::SelfEnd if child_style.writing_mode.has_vertical_lines() => {
            grid_subject_self_end_side(child_style, PhysicalAxis::Vertical)
        }
        _ => None,
    }?;
    self_alignment_offset_for_side(
        side,
        SelfAlignmentAxisContext {
            axis: PhysicalAxis::Vertical,
            content_alignment: align_content,
            container_size: container_height,
            topology: rows,
            start_line: area.row_start,
            end_line: area.row_end,
        },
        item_height,
    )
}

/// Apply the final, grid-area-dependent sizing step for aspect-ratio items.
///
/// Grid track sizing first determines each grid area. An item's automatic
/// size is then resolved against that area and its effective self-alignment:
/// `normal` behaves as start for an aspect-ratio box, while explicit `stretch`
/// supplies the corresponding area dimension. This post-track step keeps that
/// distinction at the Grid-to-layout adapter boundary instead of encoding a
/// grid area's final dimensions as a synthetic CSS declaration.
/// <https://www.w3.org/TR/css-grid-1/#grid-item-sizing> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
pub(super) fn apply_grid_aspect_ratio_item_size_corrections(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    container_width: PhysicalContentWidth,
    container_height: f32,
    columns: &GridAxisTopology,
    rows: &GridAxisTopology,
    items: &mut [GridItemLayout],
) {
    for (child, item) in children.iter().zip(items) {
        let Some(area) = item.area else {
            continue;
        };
        let child_style = &child.style;
        let Some(_) = child_style
            .aspect_ratio
            .preferred_ratio_for_non_replaced(false)
            .filter(|ratio| *ratio > 0.0 && ratio.is_finite())
        else {
            continue;
        };
        let (
            horizontal_alignment,
            vertical_alignment,
            horizontal_content_alignment,
            vertical_content_alignment,
        ) = if !WritingModeAxes::new(container_style.writing_mode, container_style.direction)
            .swaps_physical_axes()
        {
            (
                effective_grid_justify_self(child_style, container_style),
                effective_grid_align_self(child_style, container_style),
                container_style.justify_content,
                container_style.align_content,
            )
        } else {
            (
                effective_grid_align_self(child_style, container_style),
                effective_grid_justify_self(child_style, container_style),
                container_style.align_content,
                container_style.justify_content,
            )
        };
        let Some((area_x, area_right)) = columns.aligned_area_bounds(
            horizontal_content_alignment,
            container_width.points(),
            area.column_start,
            area.column_end,
        ) else {
            continue;
        };
        let Some((area_y, area_bottom)) = rows.aligned_area_bounds(
            vertical_content_alignment,
            container_height,
            area.row_start,
            area.row_end,
        ) else {
            continue;
        };
        let area_width = (area_right - area_x).max(0.0);
        let area_height = (area_bottom - area_y).max(0.0);
        let metrics = item.used_box_metrics().unwrap_or_else(|| {
            used_box_metrics(
                child_style,
                PercentageBasis::definite(layout_pt(container_width.points())),
            )
        });
        let horizontal_non_content = metrics.horizontal_non_content_length();
        let vertical_non_content = metrics.vertical_non_content_length();
        let width_is_auto = child_style.box_values.width.is_auto();
        let height_is_auto = child_style.box_values.height.is_auto();
        let width_stretches =
            width_is_auto && grid_item_aspect_axis_stretches(horizontal_alignment.keyword);
        let height_stretches =
            height_is_auto && grid_item_aspect_axis_stretches(vertical_alignment.keyword);
        let mut content_width = used_content_box_size(
            child_style.box_values.width.clone(),
            child_style.box_sizing,
            PercentageBasis::definite(content_box_pt(area_width)),
            horizontal_non_content,
        )
        .map(SemanticLengthExt::points)
        .or_else(|| {
            width_stretches.then_some((area_width - horizontal_non_content.points()).max(0.0))
        });
        let mut content_height = used_content_box_size(
            child_style.box_values.height.value().clone(),
            child_style.box_sizing,
            PercentageBasis::definite(content_box_pt(area_height)),
            vertical_non_content,
        )
        .map(SemanticLengthExt::points)
        .or_else(|| {
            height_stretches.then_some((area_height - vertical_non_content.points()).max(0.0))
        });
        match (content_width, content_height) {
            (None, Some(height)) => {
                content_width = non_replaced_aspect_ratio_content_width(
                    child_style,
                    height,
                    horizontal_non_content.points(),
                    vertical_non_content.points(),
                )
            }
            (Some(width), None) => {
                content_height = non_replaced_aspect_ratio_content_height(
                    child_style,
                    width,
                    horizontal_non_content.points(),
                    vertical_non_content.points(),
                )
            }
            (None | Some(_), None | Some(_)) => {}
        }
        let (Some(content_width), Some(content_height)) = (content_width, content_height) else {
            continue;
        };
        let width = constrain_content_width(
            child_style,
            content_box_pt(content_width),
            PercentageBasis::definite(layout_pt(area_width)),
        )
        .points()
            + horizontal_non_content.points();
        let height = constrain_content_height(
            child_style,
            content_box_pt(content_height),
            PercentageBasis::definite(layout_pt(area_height)),
        )
        .points()
            + vertical_non_content.points();
        let width = width.max(0.0);
        let height = height.max(0.0);
        let x =
            grid_item_aspect_axis_position(area_x, area_right, width, horizontal_alignment.keyword);
        let y =
            grid_item_aspect_axis_position(area_y, area_bottom, height, vertical_alignment.keyword);
        item.set_axis_geometry(GridAxis::Column, x, width);
        item.set_axis_geometry(GridAxis::Row, y, height);
    }
}

/// Restore the intrinsic used size of an automatically sized replaced Grid
/// item after track sizing. A replaced item with `align-self: normal` is not
/// stretch-fit like an ordinary block; its intrinsic dimensions may overflow a
/// zero-breadth `minmax(auto, 0)` track without contributing that size to the
/// track itself.
/// <https://www.w3.org/TR/css-grid-1/#algo-single-span-items>
/// <https://www.w3.org/TR/css-align-3/#valdef-justify-self-normal>
pub(super) fn apply_grid_replaced_item_size_corrections(
    container_style: &ComputedStyle,
    children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    items: &mut [GridItemLayout],
) {
    for ((child, estimate), item) in children.iter().zip(estimates).zip(items) {
        if !child.style.box_values.width.is_auto()
            || !child.style.box_values.height.value().is_auto()
        {
            continue;
        }
        let Some(used_size) = estimate.replaced_used_size else {
            continue;
        };
        // Unlike `normal`, an explicit (or inherited) stretch alignment
        // supplies the final grid-area size to a replaced item. An intrinsic
        // fallback must not restore the image after that zero-sized area was
        // deliberately selected by `minmax(auto, 0)`.
        // <https://www.w3.org/TR/css-align-3/#valdef-justify-self-stretch>
        if matches!(
            effective_grid_justify_self(&child.style, container_style).keyword,
            SelfAlignmentKeyword::Stretch
        ) || matches!(
            effective_grid_align_self(&child.style, container_style).keyword,
            SelfAlignmentKeyword::Stretch
        ) {
            continue;
        }
        let width = used_size.width.points().max(0.0);
        let height = used_size.height.points().max(0.0);
        if width == 0.0 || height == 0.0 || (item.width() > 0.0 && item.height() > 0.0) {
            continue;
        }
        // A zero-breadth track can omit its trailing Taffy line from the
        // detailed layout record. The item's resolved start position remains
        // authoritative, however, and is precisely where a non-stretch
        // replaced item is aligned by the preceding self-alignment phase.
        item.set_axis_geometry(GridAxis::Column, item.x(), width);
        item.set_axis_geometry(GridAxis::Row, item.y(), height);
    }
}

/// Resolve cyclic grid-item percentage sizes after final grid-area placement.
///
/// Grid must treat a percentage that depends on an intrinsic track as `auto`
/// while determining track contributions. Once tracks are placed, the same
/// preferred, minimum, and maximum size values resolve against the final grid
/// area. Keep that phase transition here rather than using the grid container
/// as Taffy's percentage basis.
/// <https://www.w3.org/TR/css-grid-1/#percentage-sizing>
/// <https://www.w3.org/TR/css-grid-1/#grid-item-sizing>
pub(super) struct GridFinalItemPercentagePlacement<'a> {
    pub(super) container_style: &'a ComputedStyle,
    pub(super) container_width: PhysicalContentWidth,
    pub(super) container_height: f32,
    pub(super) columns: &'a GridAxisTopology,
    pub(super) rows: &'a GridAxisTopology,
}

pub(super) fn apply_grid_deferred_percentage_item_size_corrections(
    placement: GridFinalItemPercentagePlacement<'_>,
    children: &[GridChild<'_>],
    estimates: &[GridItemEstimate],
    items: &mut [GridItemLayout],
) {
    for ((child, estimate), item) in children.iter().zip(estimates).zip(items) {
        let Some(area) = item.area else {
            continue;
        };
        let child_style = &child.style;
        let (
            horizontal_alignment,
            vertical_alignment,
            horizontal_content_alignment,
            vertical_content_alignment,
        ) = if WritingModeAxes::new(
            placement.container_style.writing_mode,
            placement.container_style.direction,
        )
        .swaps_physical_axes()
        {
            (
                effective_grid_align_self(child_style, placement.container_style),
                effective_grid_justify_self(child_style, placement.container_style),
                placement.container_style.align_content,
                placement.container_style.justify_content,
            )
        } else {
            (
                effective_grid_justify_self(child_style, placement.container_style),
                effective_grid_align_self(child_style, placement.container_style),
                placement.container_style.justify_content,
                placement.container_style.align_content,
            )
        };
        let Some((area_x, area_right)) = placement.columns.aligned_area_bounds(
            horizontal_content_alignment,
            placement.container_width.points(),
            area.column_start,
            area.column_end,
        ) else {
            continue;
        };
        let Some((area_y, area_bottom)) = placement.rows.aligned_area_bounds(
            vertical_content_alignment,
            placement.container_height,
            area.row_start,
            area.row_end,
        ) else {
            continue;
        };
        let area_width = (area_right - area_x).max(0.0);
        let area_height = (area_bottom - area_y).max(0.0);
        let final_size = resolve_grid_item_final_percentage_size(
            child,
            estimate,
            item,
            area_width,
            area_height,
            placement.container_width,
        );
        if let Some(width) = final_size.width {
            item.mark_final_percentage_axis(GridAxis::Column);
            item.set_axis_geometry(
                GridAxis::Column,
                grid_item_aspect_axis_position(
                    area_x,
                    area_right,
                    width.points(),
                    horizontal_alignment.keyword,
                ),
                width.points(),
            );
        }
        if let Some(height) = final_size.height {
            item.mark_final_percentage_axis(GridAxis::Row);
            item.set_axis_geometry(
                GridAxis::Row,
                grid_item_aspect_axis_position(
                    area_y,
                    area_bottom,
                    height.points(),
                    vertical_alignment.keyword,
                ),
                height.points(),
            );
        }
    }
}

/// Border-box dimensions resolved after a Grid item's final area is known.
///
/// A cyclic percentage contributes as `auto` while tracks are intrinsic-sized,
/// but its preferred/minimum/maximum constraint applies to the final area.
/// This value is deliberately independent of an item's final origin so Grid
/// Lanes can use it while determining its perpendicular packing extent.
/// <https://www.w3.org/TR/css-grid-1/#percentage-sizing>
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct GridItemFinalPercentageSize {
    pub(super) width: Option<BorderBoxLength>,
    pub(super) height: Option<BorderBoxLength>,
}

pub(super) fn resolve_grid_item_final_percentage_size(
    child: &GridChild<'_>,
    estimate: &GridItemEstimate,
    item: &GridItemLayout,
    area_width: f32,
    area_height: f32,
    container_width: PhysicalContentWidth,
) -> GridItemFinalPercentageSize {
    let child_style = &child.style;
    let metrics = item.used_box_metrics().unwrap_or_else(|| {
        used_box_metrics(
            child_style,
            PercentageBasis::definite(layout_pt(container_width.points())),
        )
    });
    let horizontal_non_content = metrics.horizontal_non_content_length();
    let vertical_non_content = metrics.vertical_non_content_length();
    let width = grid_item_axis_has_percentage(
        &child_style.box_values.width,
        &child_style.box_values.min_width,
        &child_style.box_values.max_width,
    )
    .then(|| {
        let content_width = used_content_box_size(
            child_style.box_values.width.clone(),
            child_style.box_sizing,
            PercentageBasis::definite(content_box_pt(area_width)),
            horizontal_non_content,
        )
        .map(SemanticLengthExt::points)
        .unwrap_or_else(|| {
            (estimate.physical_measurements().content_width.points()
                - horizontal_non_content.points())
            .max(0.0)
        });
        BorderBoxLength::new(
            constrain_content_width(
                child_style,
                content_box_pt(content_width),
                PercentageBasis::definite(layout_pt(area_width)),
            )
            .points()
                + horizontal_non_content.points(),
        )
    });
    let height = grid_item_axis_has_percentage(
        child_style.box_values.height.value(),
        &child_style.box_values.min_height,
        &child_style.box_values.max_height,
    )
    .then(|| {
        let content_height = used_content_box_size(
            child_style.box_values.height.value().clone(),
            child_style.box_sizing,
            PercentageBasis::definite(content_box_pt(area_height)),
            vertical_non_content,
        )
        .map(SemanticLengthExt::points)
        .unwrap_or_else(|| {
            (estimate.physical_measurements().content_height.points()
                - vertical_non_content.points())
            .max(0.0)
        });
        BorderBoxLength::new(
            constrain_content_height(
                child_style,
                content_box_pt(content_height),
                PercentageBasis::definite(layout_pt(area_height)),
            )
            .points()
                + vertical_non_content.points(),
        )
    });
    GridItemFinalPercentageSize { width, height }
}

fn grid_item_axis_has_percentage(
    preferred: &css::ComputedLengthPercentageOrAuto,
    minimum: &css::ComputedLengthPercentageOrAuto,
    maximum: &css::ComputedLengthPercentageOrAuto,
) -> bool {
    [preferred, minimum, maximum].into_iter().any(|value| {
        matches!(
            value,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.contains_percentage()
        )
    })
}

/// Whether an automatic grid-item axis receives its grid-area size.
///
/// For aspect-ratio boxes, `normal` falls back to start rather than stretch;
/// explicit `stretch` remains a definite sizing input.
fn grid_item_aspect_axis_stretches(alignment: SelfAlignmentKeyword) -> bool {
    matches!(alignment, SelfAlignmentKeyword::Stretch)
}

fn grid_item_aspect_axis_position(
    start: f32,
    end: f32,
    item_size: f32,
    alignment: SelfAlignmentKeyword,
) -> f32 {
    match alignment {
        SelfAlignmentKeyword::End
        | SelfAlignmentKeyword::FlexEnd
        | SelfAlignmentKeyword::SelfEnd
        | SelfAlignmentKeyword::Right => end - item_size,
        SelfAlignmentKeyword::Center => start + ((end - start - item_size) / 2.0),
        SelfAlignmentKeyword::Auto
        | SelfAlignmentKeyword::Normal
        | SelfAlignmentKeyword::Start
        | SelfAlignmentKeyword::FlexStart
        | SelfAlignmentKeyword::SelfStart
        | SelfAlignmentKeyword::Left
        | SelfAlignmentKeyword::Stretch
        | SelfAlignmentKeyword::Baseline
        | SelfAlignmentKeyword::LastBaseline => start,
    }
}

pub(super) fn grid_subject_self_start_side(
    child_style: &ComputedStyle,
    axis: PhysicalAxis,
) -> Option<PhysicalSide> {
    let block_start = block_start_side(child_style.writing_mode);
    if block_start.axis() == axis {
        Some(block_start)
    } else {
        let inline_start =
            inline_start_side(child_style.writing_mode, child_style.used_direction());
        (inline_start.axis() == axis).then_some(inline_start)
    }
}

pub(super) fn grid_subject_self_end_side(
    child_style: &ComputedStyle,
    axis: PhysicalAxis,
) -> Option<PhysicalSide> {
    let block_end = block_end_side(child_style.writing_mode);
    if block_end.axis() == axis {
        Some(block_end)
    } else {
        let inline_end = inline_end_side(child_style.writing_mode, child_style.used_direction());
        (inline_end.axis() == axis).then_some(inline_end)
    }
}

struct SelfAlignmentAxisContext<'a> {
    axis: PhysicalAxis,
    content_alignment: css::ContentAlignment,
    container_size: f32,
    topology: &'a GridAxisTopology,
    start_line: u16,
    end_line: u16,
}

fn self_alignment_offset_for_side(
    side: PhysicalSide,
    context: SelfAlignmentAxisContext<'_>,
    item_size: f32,
) -> Option<f32> {
    if side.axis() != context.axis {
        return None;
    }
    let (area_start, area_end) = context.topology.aligned_area_bounds(
        context.content_alignment,
        context.container_size,
        context.start_line,
        context.end_line,
    )?;
    let item_size = item_size.max(0.0);
    match (context.axis, side) {
        (PhysicalAxis::Horizontal, PhysicalSide::Left)
        | (PhysicalAxis::Vertical, PhysicalSide::Top) => Some(area_start),
        (PhysicalAxis::Horizontal, PhysicalSide::Right)
        | (PhysicalAxis::Vertical, PhysicalSide::Bottom) => Some(area_end - item_size),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn anonymous_grid_child_with_style(style: ComputedStyle) -> GridChild<'static> {
        let source = FormattingContextChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: style.clone(),
        };
        let used_style = css::LayoutStyle::from_computed(&style).into_zoomed();
        GridUsedItem::from_source(source, used_style)
    }

    #[test]
    fn final_grid_area_resolves_mixed_percentage_sizes_before_constraints() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_affine(layout_pt(2.0), 1.0, true),
        );
        style.box_values.min_width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );
        style.box_values.max_width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.75),
        );
        let child = anonymous_grid_child_with_style(style);
        let item = GridItemLayout::new(
            GridRect::new(GridPoint::new(0.0, 0.0), GridSize::new(10.0, 10.0)),
            None,
        );

        let size = resolve_grid_item_final_percentage_size(
            &child,
            &GridItemEstimate::fixed(10.0, 10.0),
            &item,
            100.0,
            80.0,
            PhysicalContentWidth::new(content_box_pt(100.0)),
        );

        // `calc(2pt + 100%)` resolves to 102pt against the final area, then
        // the final 75% maximum constrains the used border-box width.
        assert_eq!(size.width.map(SemanticLengthExt::points), Some(75.0));
        assert_eq!(size.height, None);
    }
}
