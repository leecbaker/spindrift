//! Table root clipping, stacking policy, and paint helpers.

use super::*;
/// Return the CSS overflow/paint-containment clip for a table box, excluding
/// wrapper captions.
///
/// CSS 2.1 errata makes `overflow` apply to the table box instead of the
/// table wrapper box, and defines `scroll`/`auto` as visible on table boxes.
/// The clipping edge therefore uses the table padding box around the grid, not
/// the wrapper area that contains captions:
/// <https://www.w3.org/Style/css2-updates/REC-CSS2-20110607-errata.html#s.11.1.1b>.
/// Paint containment uses the same table padding edge:
/// <https://www.w3.org/TR/css-contain-1/#containment-paint>.
pub(in crate::layout::table) fn table_box_overflow_clip(
    style: &ComputedStyle,
    padding_box: PaintClip,
    table_is_document_canvas: bool,
) -> Option<PaintClip> {
    if table_is_document_canvas {
        return None;
    }
    let clips = style.contain.paint
        || matches!(
            effective_overflow_for_style(style),
            css::Overflow::Hidden | css::Overflow::Clip
        );
    if !clips {
        return None;
    }
    let borders = used_border_widths(style);
    let border_box = paint_space_rect(
        padding_box.x() - borders.left,
        padding_box.y() - borders.bottom,
        padding_box.width() + borders.left + borders.right,
        padding_box.height() + borders.top + borders.bottom,
    );
    resolve_overflow_clip_edge(
        border_box,
        style,
        borders,
        UsedOverflowAxes::from_style(style),
        style.contain.paint,
        None,
    )
    .map(|edge| edge.clip.bounds)
}

pub(in crate::layout::table) fn table_padding_box_clip_from_border_box(
    border_box: PaintClip,
    table_width: UsedTableWidth,
) -> PaintClip {
    PaintClip::from_paint_rect(paint_space_rect(
        border_box.x() + table_width.border_widths.left,
        border_box.y() + table_width.border_widths.bottom,
        border_box.width() - table_width.border_widths.left - table_width.border_widths.right,
        border_box.height() - table_width.border_widths.top - table_width.border_widths.bottom,
    ))
}

/// Select the paint band for a table root from its computed outer display role.
///
/// CSS Tables defines `table` as block-level and `inline-table` as inline-level.
/// The table root's outer role therefore determines whether the atomic table
/// participates in the in-flow block or inline painting band; DOM ancestry,
/// including an enclosing table cell, does not change that role. Positioned and
/// relative tables are subsequently promoted by [`StackingContextPolicy`].
/// <https://drafts.csswg.org/css-display/#outer-role>;
/// <https://drafts.csswg.org/css-tables/#table-model>;
/// <https://www.w3.org/TR/CSS22/zindex.html>.
pub(in crate::layout::table) fn table_parent_paint_band(style: &ComputedStyle) -> PaintBand {
    debug_assert!(
        style.display.is_table(),
        "table paint-band classification requires a table root display"
    );
    if style.display.is_inline_level() {
        PaintBand::Inline
    } else {
        PaintBand::InFlowBlock
    }
}

pub(in crate::layout::table) fn table_atomic_stacking_policy(
    style: &ComputedStyle,
    parent_band: PaintBand,
    bounds: PaintClip,
    overflow_clip: Option<PaintClip>,
) -> StackingContextPolicy {
    let mut policy = StackingContextPolicy::for_atomic(style, parent_band, bounds);
    // Table layout records fragment-local paint structure, while the element
    // dispatcher owns the table element's principal effect context. Keeping
    // the transform here as well applies the same CTM once for the table
    // fragment and once for the owning element. Retain table-local overflow
    // clipping but let the enclosing context serialize the principal effect
    // exactly once.
    // <https://drafts.csswg.org/css-transforms-1/#transform-rendering>
    policy.effects.transform = None;
    policy.effects.suppress_paint = false;
    policy.effects.set_rectangular_overflow_clip(overflow_clip);
    policy
}

/// Whether this table fragment's structural outlines join the enclosing
/// normal-flow outline phase.
///
/// A static table is an atomic table-paint unit, but not an atomic *stacking*
/// context. Its row-group outlines therefore follow Spindrift's normal-flow
/// compatibility phase. Positioned and effect-owning tables retain a final
/// local outline phase instead.
pub(in crate::layout::table) fn table_outlines_use_in_flow_phase(
    style: &ComputedStyle,
    table_is_document_canvas: bool,
    policy: &StackingContextPolicy,
) -> bool {
    !table_is_document_canvas
        && !style.position.is_in_flow_positioned()
        && !policy.is_real_stacking_context
}

pub(in crate::layout::table) fn table_horizontal_non_content_width(
    table_width: UsedTableWidth,
) -> f32 {
    table_width.inline_non_content().points()
}

pub(in crate::layout::table) fn table_content_width_clamped_to_min_content(
    style: &ComputedStyle,
    content_width: LogicalInlineContentSize,
    min_content: LogicalInlineContentSize,
) -> LogicalInlineContentSize {
    // CSS 2.2 permits the fixed layout algorithm only when the table has a
    // non-auto width. An auto-width `table-layout: fixed` table therefore
    // still needs the automatic table's intrinsic floor before the fixed
    // planner consumes its used grid width.
    // <https://www.w3.org/TR/CSS22/tables.html#fixed-table-layout>
    if style.table_layout == TableLayout::Auto || table_root_inline_size(style).is_auto() {
        LogicalInlineContentSize::new(content_box_pt(
            content_width.points().max(min_content.points()),
        ))
    } else {
        content_width
    }
}

pub(in crate::layout::table) fn table_displayed_horizontal_spacing(
    visible_columns: usize,
    table_metrics: TableMetrics,
) -> f32 {
    if visible_columns == 0 {
        0.0
    } else {
        table_metrics.spacing.horizontal.length_points() * (visible_columns + 1) as f32
    }
}

/// Return separated-border gutters inside a logical column span.
///
/// CSS 2.2 places horizontal `border-spacing` between adjacent column cells.
/// A cell spanning multiple visible columns includes those internal gutters in
/// its border box, so column width constraints derived from that cell must
/// remove them before distributing the remaining width to tracks:
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>.
pub(in crate::layout::table) fn table_internal_horizontal_spacing(
    start_column: usize,
    end_column: usize,
    collapsed_columns: &[bool],
    table_metrics: TableMetrics,
) -> f32 {
    let end_column = end_column.min(collapsed_columns.len());
    if start_column >= end_column {
        return 0.0;
    }
    let visible_columns = collapsed_columns[start_column..end_column]
        .iter()
        .filter(|collapsed| !**collapsed)
        .count();
    table_metrics.spacing.horizontal.length_points() * visible_columns.saturating_sub(1) as f32
}

pub(in crate::layout::table) fn table_column_background_primitives(
    table_x: f32,
    grid_top: f32,
    grid_height: f32,
    column_plan: &TableColumnPlan,
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
) -> Vec<PaintPrimitive> {
    let Some((paint_rect, _inline_bounds)) = table_column_background_rect(
        table_x,
        grid_top,
        grid_height,
        column_plan,
        start_column,
        end_column,
        style,
    ) else {
        return Vec::new();
    };
    table_column_background_primitives_with_clip(paint_rect, style, paint_rect)
}

/// Paint a column layer against the root table's projected logical grid.
///
/// A column's background spans the table grid's block extent.  In a vertical
/// table that extent is physical width, not the legacy row fragment's physical
/// height, so structural painting must retain [`TableGridPlacement`] until it
/// reaches the page boundary.
/// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_column_grid_background_primitives(
    projection: &TableGridFragmentProjection,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_bounds: &[TableRowBounds],
    row_heights: &[f32],
    row_offsets: &[f32],
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    if start_column >= end_column || start_column >= column_plan.column_count() {
        return Vec::new();
    }
    // `inline_bounds_for_span` is the table's direction-projected physical
    // span. The source paint view deliberately uses horizontal LTR page
    // coordinates, so this keeps a horizontal RTL column's background image
    // in its physical column without mirroring the CSS gradient itself.
    // <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
    let source_inline_edge = TableGridLength::new(0.0);
    let source_placement = projection.source_placement();
    let destination_placement = projection.destination_placement();
    let (Some(first_row), Some(last_row)) = (row_bounds.first(), row_bounds.last()) else {
        return Vec::new();
    };
    let block_start = TableGridLength::new(first_row.start);
    let block_size = TableGridLength::new(last_row.start + last_row.size - first_row.start);
    let inline_bounds = column_plan.inline_bounds_for_span(
        start_column,
        end_column.min(column_plan.column_count()) - start_column,
    );
    let positioning_rect = TableGridRect::new(
        TableGridPoint::from_lengths(inline_bounds.start, block_start),
        TableGridSize::from_lengths(inline_bounds.size, block_size),
    );
    let logical_positioning_area = PaintBackgroundArea::from_paint_rect(
        source_placement
            .overflow_clip_for(positioning_rect)
            .paint_rect(),
    );
    let mut primitives = Vec::new();
    let cell_clips = table_column_grid_cell_clips(
        projection,
        column_plan,
        table_grid,
        row_bounds,
        fragment_rows,
        row_heights,
        row_offsets,
        start_column,
        end_column,
        source_inline_edge,
    );
    for projection in cell_clips {
        let destination_clip = projection.destination_clip();
        let source_clip = projection.source_clip();
        primitives.extend(table_column_background_primitives_with_clip(
            destination_clip,
            style,
            destination_clip,
        ));
        let images = structural_table_background_image_primitives(
            logical_positioning_area,
            PaintBackgroundArea::from_paint_rect(source_clip),
            style,
            base_url,
            root_url,
            resource_cache,
        );
        if source_placement != destination_placement
            || source_placement.writing_mode().has_vertical_lines()
        {
            primitives.extend(images.into_iter().map(|primitive| {
                transform_table_column_image_primitive(primitive, projection.source_to_destination)
            }));
        } else {
            primitives.extend(images);
        }
    }
    primitives
}

/// Project the cell-derived paint regions through the retained table grid.
///
/// A structural column layer is positioned against its complete column span,
/// but CSS Tables exposes it only in cells participating in that span.  Keep
/// the source row tracks and the fragment's visible row pieces separate until
/// this final projection so `rowspan`, `colspan`, and vertical writing modes
/// share one clipping rule.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_column_grid_cell_clips(
    projection: &TableGridFragmentProjection,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    row_bounds: &[TableRowBounds],
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    start_column: usize,
    end_column: usize,
    source_inline_edge: TableGridLength,
) -> Vec<TableStructuralPaintProjection> {
    table_structural_originating_cell_projections(
        projection,
        row_bounds,
        column_plan,
        table_grid,
        fragment_rows,
        row_heights,
        row_offsets,
        TableStructuralOrigin::Columns {
            start: start_column,
            end: end_column,
        },
        source_inline_edge,
    )
}

/// Paint a table-root structural background through the visible source-row
/// pieces of one committed fragment.
///
/// The table grid remains the background positioning area under the default
/// `box-decoration-break: slice`; the fragment viewport only limits paint.
/// Keeping this at the table-grid boundary makes table-root images use the
/// same retained source geometry as row, row-group, and column layers.
/// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
#[allow(clippy::needless_collect)]
pub(in crate::layout::table) fn table_grid_fragment_background_primitives(
    projection: &TableGridFragmentProjection,
    row_bounds: &[TableRowBounds],
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    style: &ComputedStyle,
    collapsed_outer_insets: css::Edges,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let source_placement = projection.source_placement();
    let has_collapsed_outer_insets = collapsed_outer_insets != css::Edges::ZERO;
    let positioning_rect = source_placement.full_page_top_rect().paint_rect();
    let clips: Vec<_> = fragment_rows
        .iter()
        .enumerate()
        .filter_map(|(local_row, source_row)| {
            let _source = row_bounds.get(*source_row)?;
            let row_height = *row_heights.get(local_row)?;
            let slice = projection.source_row_slice(*source_row)?;
            if row_height <= 0.0 {
                return None;
            }
            let source_rect = TableGridRect::new(
                TableGridPoint::from_lengths(TableGridLength::new(0.0), slice.block_start.length()),
                TableGridSize::from_lengths(
                    source_placement.logical_inline_grid_extent(),
                    TableGridLength::new(row_height),
                ),
            );
            let destination_rect = TableGridRect::new(
                TableGridPoint::from_lengths(
                    TableGridLength::new(0.0),
                    slice.destination_block_start.length(),
                ),
                source_rect.size,
            );
            Some(projection.project_slice(source_rect, destination_rect, TableGridLength::new(0.0)))
        })
        .collect();
    let mut background_style = style.clone();
    if has_collapsed_outer_insets {
        // The structural helper clips root colors to row pieces. Collapsed
        // outer borders sit outside those pieces, so paint the color clips
        // below with their physical table-wrapper outsets instead.
        background_style.background.background_color = css::BackgroundColor::TRANSPARENT;
    }
    let mut primitives = table_grid_structural_background_primitives(
        positioning_rect,
        clips,
        &background_style,
        base_url,
        root_url,
        resource_cache,
    );
    if let Some(fill) = style.background.background_color.visible_color(style.color)
        && has_collapsed_outer_insets
    {
        let unfragmented_grid = fragment_rows.len() == row_bounds.len()
            && fragment_rows
                .iter()
                .enumerate()
                .all(|(row, source_row)| row == *source_row);
        if unfragmented_grid {
            let rect = source_placement.full_page_top_rect();
            let expanded = PageTopRect::new(
                rect.x() - collapsed_outer_insets.left,
                rect.top_y() + collapsed_outer_insets.top,
                rect.width() + collapsed_outer_insets.left + collapsed_outer_insets.right,
                rect.height() + collapsed_outer_insets.top + collapsed_outer_insets.bottom,
            )
            .paint_rect();
            primitives.push(PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                expanded,
                Some(fill),
            )));
        } else {
            for (local_row, source_row) in fragment_rows.iter().enumerate() {
                let (Some(source), Some(row_height), Some(row_offset)) = (
                    row_bounds.get(*source_row),
                    row_heights.get(local_row),
                    row_offsets.get(local_row),
                ) else {
                    continue;
                };
                if *row_height <= 0.0 {
                    continue;
                }
                let mut top = 0.0;
                let mut bottom = 0.0;
                if *source_row == 0 {
                    top = collapsed_outer_insets.top;
                }
                if *source_row + 1 == row_bounds.len() {
                    bottom = collapsed_outer_insets.bottom;
                }
                let rect = source_placement.page_top_rect_for(TableGridRect::new(
                    TableGridPoint::from_lengths(
                        TableGridLength::new(0.0),
                        TableGridLength::new(source.start + *row_offset),
                    ),
                    TableGridSize::from_lengths(
                        source_placement.logical_inline_grid_extent(),
                        TableGridLength::new(*row_height),
                    ),
                ));
                let expanded = PageTopRect::new(
                    rect.x() - collapsed_outer_insets.left,
                    rect.top_y() + top,
                    rect.width() + collapsed_outer_insets.left + collapsed_outer_insets.right,
                    rect.height() + top + bottom,
                )
                .paint_rect();
                primitives.push(PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                    expanded,
                    Some(fill),
                )));
            }
        }
    }
    primitives
}

/// Project a cell-derived structural primitive into its destination table
/// fragment. These backgrounds are resolved for their originating structural
/// box before cell clipping, so their existing projection also moves the
/// pattern placement.
pub(in crate::layout::table) fn transform_table_column_image_primitive(
    primitive: PaintPrimitive,
    translation: PaintTranslation,
) -> PaintPrimitive {
    primitive.translated(translation)
}

#[allow(clippy::too_many_arguments)]
/// Paint a column or column-group background through cell-derived clips.
///
/// CSS Tables 3 renders column backgrounds as if each originating cell exposed
/// the column's background, so separated row spacing must remain unpainted
/// while the full column box remains the background positioning area:
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>.
pub(in crate::layout::table) fn table_column_fragment_background_primitives(
    table_x: f32,
    grid_top: f32,
    grid_height: f32,
    column_plan: &TableColumnPlan,
    table_grid: Option<&TableGrid>,
    fragment_rows: &[usize],
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
    row_tops: &[f32],
    row_heights: &[f32],
) -> Vec<PaintPrimitive> {
    if matches!(
        style.writing_mode,
        WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr
    ) {
        return table_column_background_primitives(
            table_x,
            grid_top,
            grid_height,
            column_plan,
            start_column,
            end_column,
            style,
        );
    }
    let Some((paint_rect, _inline_bounds)) = table_column_background_rect(
        table_x,
        grid_top,
        grid_height,
        column_plan,
        start_column,
        end_column,
        style,
    ) else {
        return Vec::new();
    };
    let cell_derived_clips = table_grid.map(|table_grid| {
        table_column_fragment_cell_clips(
            table_x,
            column_plan,
            table_grid,
            fragment_rows,
            row_tops,
            row_heights,
            start_column,
            end_column,
        )
    });
    let clips = cell_derived_clips.unwrap_or_else(|| {
        row_tops
            .iter()
            .cloned()
            .zip(row_heights.iter().cloned())
            .filter(|(_, row_height)| *row_height > 0.0)
            .map(|(row_top, row_height)| {
                intersect_paint_rect_or_empty(
                    paint_rect,
                    paint_space_rect(
                        paint_rect.origin.x,
                        row_top - row_height,
                        paint_rect.size.width,
                        row_height,
                    ),
                )
            })
            .collect()
    });
    let mut primitives = Vec::new();
    if let Some(fill) = style.background.background_color.visible_color(style.color) {
        primitives.extend(
            clips
                .into_iter()
                .map(|clip| PaintPrimitive::Rect(RenderedRect::from_paint_rect(clip, Some(fill)))),
        );
    }
    primitives
}

/// Paint CSS background-image layers for a column or column group through the
/// cell-derived clips exposed by the current row fragment.
///
/// The structural background's positioning area is the complete column box,
/// while each participating row exposes only its cell-height slice. Reusing
/// the normal background painter keeps gradients, URL images, sizing,
/// positioning, and repetition consistent with ordinary boxes.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_column_fragment_background_image_primitives(
    table_x: f32,
    grid_top: f32,
    grid_height: f32,
    column_plan: &TableColumnPlan,
    table_grid: Option<&TableGrid>,
    fragment_rows: &[usize],
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
    row_tops: &[f32],
    row_heights: &[f32],
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let Some((paint_rect, _inline_bounds)) = table_column_background_rect(
        table_x,
        grid_top,
        grid_height,
        column_plan,
        start_column,
        end_column,
        style,
    ) else {
        return Vec::new();
    };
    let positioning_area = PaintBackgroundArea::from_paint_rect(paint_rect);
    let clips = if matches!(
        style.writing_mode,
        WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr
    ) {
        vec![paint_rect]
    } else if let Some(table_grid) = table_grid {
        table_column_fragment_cell_clips(
            table_x,
            column_plan,
            table_grid,
            fragment_rows,
            row_tops,
            row_heights,
            start_column,
            end_column,
        )
    } else {
        row_tops
            .iter()
            .cloned()
            .zip(row_heights.iter().cloned())
            .filter(|(_, row_height)| *row_height > 0.0)
            .map(|(row_top, row_height)| {
                intersect_paint_rect_or_empty(
                    paint_rect,
                    paint_space_rect(
                        paint_rect.origin.x,
                        row_top - row_height,
                        paint_rect.size.width,
                        row_height,
                    ),
                )
            })
            .collect()
    };
    clips
        .into_iter()
        .filter(|clip| clip.size.width > 0.0 && clip.size.height > 0.0)
        .flat_map(|clip| {
            structural_table_background_image_primitives(
                positioning_area,
                PaintBackgroundArea::from_paint_rect(clip),
                style,
                base_url,
                root_url,
                resource_cache,
            )
        })
        .collect()
}

/// Return the exposed cell slices for a structural column background.
///
/// A column background is positioned against the complete column box, but it
/// is painted only through cells that overlap that column. In particular, a
/// `colspan` must not expose a column image in its other grid columns, and a
/// `rowspan` keeps its cell clip continuous across the rows it occupies.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_column_fragment_cell_clips(
    table_x: f32,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_tops: &[f32],
    row_heights: &[f32],
    start_column: usize,
    end_column: usize,
) -> Vec<PaintRect> {
    let mut clips = Vec::new();
    for source_row in fragment_rows.iter().cloned() {
        let Some(placements) = table_grid.rows.get(source_row) else {
            continue;
        };
        for placement in placements {
            let cell_end = placement.column.saturating_add(placement.colspan);
            if placement.column >= end_column || cell_end <= start_column {
                continue;
            }
            let mut cell_top = None;
            let mut cell_bottom = None;
            for (covered_local_row, covered_source_row) in fragment_rows.iter().cloned().enumerate()
            {
                if covered_source_row < source_row
                    || covered_source_row >= source_row.saturating_add(placement.rowspan)
                {
                    continue;
                }
                let (Some(row_top), Some(row_height)) = (
                    row_tops.get(covered_local_row).cloned(),
                    row_heights.get(covered_local_row).cloned(),
                ) else {
                    continue;
                };
                if row_height <= 0.0 {
                    continue;
                }
                cell_top = Some(cell_top.map_or(row_top, |top: f32| top.max(row_top)));
                let row_bottom = row_top - row_height;
                cell_bottom =
                    Some(cell_bottom.map_or(row_bottom, |bottom: f32| bottom.min(row_bottom)));
            }
            let (Some(cell_top), Some(cell_bottom)) = (cell_top, cell_bottom) else {
                continue;
            };
            let cell_inline =
                column_plan.inline_bounds_for_span(placement.column, placement.colspan);
            let cell_rect = paint_space_rect(
                table_x + cell_inline.logical_start().get(),
                cell_bottom,
                cell_inline.logical_size().get(),
                (cell_top - cell_bottom).max(0.0),
            );
            if cell_rect.size.width > 0.0 && cell_rect.size.height > 0.0 {
                clips.push(cell_rect);
            }
        }
    }
    clips
}

/// Paint one row's structural background through the cells it originates.
///
/// CSS Tables draws a row background in its originating cells. A cell that
/// spans later rows therefore continues to expose that row's background, while
/// the image still positions against the originating row box.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_row_fragment_background_primitives(
    table_x: f32,
    positioning_rect: PaintRect,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_tops: &[f32],
    row_heights: &[f32],
    row_offsets: &[f32],
    original_row_heights: &[f32],
    row_index: usize,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let clips = table_row_fragment_cell_clips(
        table_x,
        column_plan,
        table_grid,
        fragment_rows,
        row_tops,
        row_heights,
        row_index,
    );
    // `box-decoration-break` defaults to `slice`, so a row background is
    // positioned against the unfragmented source row even though each table
    // fragment exposes it only through the cells visible in that fragment.
    // In particular, a repeating image must not restart at a column/page
    // boundary.  The row plan retains the amount already consumed from the
    // source row and its original height precisely for this projection:
    // <https://www.w3.org/TR/css-break-3/#break-decoration>.
    let positioning_rect = fragment_rows
        .iter()
        .position(|source_row| *source_row == row_index)
        .and_then(|local_row| {
            let top = *row_tops.get(local_row)? + *row_offsets.get(local_row)?;
            let height = *original_row_heights.get(local_row)?;
            (height > 0.0).then_some(paint_space_rect(
                positioning_rect.origin.x,
                top - height,
                positioning_rect.size.width,
                height,
            ))
        })
        .unwrap_or(positioning_rect);
    let positioning_area = PaintBackgroundArea::from_paint_rect(positioning_rect);
    let mut primitives = Vec::new();
    if let Some(fill) = style.background.background_color.visible_color(style.color) {
        primitives.extend(
            clips
                .iter()
                .cloned()
                .map(|clip| PaintPrimitive::Rect(RenderedRect::from_paint_rect(clip, Some(fill)))),
        );
    }
    primitives.extend(clips.into_iter().flat_map(|clip| {
        structural_table_background_image_primitives(
            positioning_area,
            PaintBackgroundArea::from_paint_rect(clip),
            style,
            base_url,
            root_url,
            resource_cache,
        )
    }));
    primitives
}

/// Paint one row background from source table-grid geometry.
///
/// Unlike fragment-local `row_top` values, `row_bounds` identifies the whole
/// source row.  The positioning rectangle therefore remains continuous under
/// the default `box-decoration-break: slice`, while the generated primitives
/// are visible only through originating cell pieces in this fragment.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds> and
/// <https://www.w3.org/TR/css-break-3/#break-decoration>.
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_row_grid_background_primitives(
    projection: &TableGridFragmentProjection,
    row_bounds: &[TableRowBounds],
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    row_index: usize,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let source_placement = projection.source_placement();
    let Some(source_row) = row_bounds.get(row_index).copied() else {
        return Vec::new();
    };
    let inline_rect = column_plan
        .logical_occupied_inline_rect()
        .unwrap_or_else(|| {
            TableGridRect::new(
                TableGridPoint::from_lengths(TableGridLength::new(0.0), TableGridLength::new(0.0)),
                TableGridSize::from_lengths(
                    source_placement.logical_inline_grid_extent(),
                    TableGridLength::new(0.0),
                ),
            )
        });
    let positioning_rect = source_placement
        .page_top_rect_for(TableGridRect::new(
            TableGridPoint::from_lengths(
                TableGridLength::new(inline_rect.origin.x),
                TableGridLength::new(source_row.start),
            ),
            TableGridSize::from_lengths(
                TableGridLength::new(inline_rect.size.width),
                TableGridLength::new(source_row.size),
            ),
        ))
        .paint_rect();
    let clips = table_originating_cell_grid_clips(
        projection,
        row_bounds,
        column_plan,
        table_grid,
        fragment_rows,
        row_heights,
        row_offsets,
        row_index,
        row_index,
        row_index.saturating_add(1),
    );
    table_grid_structural_background_primitives(
        positioning_rect,
        clips,
        style,
        base_url,
        root_url,
        resource_cache,
    )
}

/// Paint one row-group background from source table-grid geometry.
///
/// Row groups and rows deliberately share originating-cell clipping so cells
/// spanning later source rows expose the correct structural background in a
/// fragmented table.
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_row_group_grid_background_primitives(
    projection: &TableGridFragmentProjection,
    row_bounds: &[TableRowBounds],
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    start_row: usize,
    end_row: usize,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let source_placement = projection.source_placement();
    let Some(start) = row_bounds.get(start_row).copied() else {
        return Vec::new();
    };
    let Some(end) = end_row
        .checked_sub(1)
        .and_then(|index| row_bounds.get(index))
        .copied()
    else {
        return Vec::new();
    };
    let inline_rect = column_plan
        .logical_occupied_inline_rect()
        .unwrap_or_else(|| {
            TableGridRect::new(
                TableGridPoint::from_lengths(TableGridLength::new(0.0), TableGridLength::new(0.0)),
                TableGridSize::from_lengths(
                    source_placement.logical_inline_grid_extent(),
                    TableGridLength::new(0.0),
                ),
            )
        });
    let positioning_rect = source_placement
        .page_top_rect_for(TableGridRect::new(
            TableGridPoint::from_lengths(
                TableGridLength::new(inline_rect.origin.x),
                TableGridLength::new(start.start),
            ),
            TableGridSize::from_lengths(
                TableGridLength::new(inline_rect.size.width),
                TableGridLength::new((end.start + end.size - start.start).max(0.0)),
            ),
        ))
        .paint_rect();
    let clips = table_structural_originating_cell_projections(
        projection,
        row_bounds,
        column_plan,
        table_grid,
        fragment_rows,
        row_heights,
        row_offsets,
        TableStructuralOrigin::Rows {
            start: start_row,
            end: end_row,
        },
        TableGridLength::new(0.0),
    );
    table_grid_structural_background_primitives(
        positioning_rect,
        clips,
        style,
        base_url,
        root_url,
        resource_cache,
    )
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_originating_cell_grid_clips(
    projection: &TableGridFragmentProjection,
    row_bounds: &[TableRowBounds],
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    _originating_row: usize,
    structural_start_row: usize,
    structural_end_row: usize,
) -> Vec<TableStructuralPaintProjection> {
    table_structural_originating_cell_projections(
        projection,
        row_bounds,
        column_plan,
        table_grid,
        fragment_rows,
        row_heights,
        row_offsets,
        TableStructuralOrigin::Rows {
            start: structural_start_row,
            end: structural_end_row,
        },
        TableGridLength::new(0.0),
    )
}

/// Paint table structural layers from source-grid geometry into the cell
/// regions exposed by a single destination fragment. CSS background colors
/// use the physical destination clips; images resolve in the unfragmented
/// source positioning area and are then transformed once into the root table's
/// writing mode.
/// <https://www.w3.org/TR/css-backgrounds-3/#background-position>
/// <https://www.w3.org/TR/CSS22/tables.html#table-layers>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_grid_structural_background_primitives(
    source_positioning_rect: PaintRect,
    clips: Vec<TableStructuralPaintProjection>,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let positioning_area = PaintBackgroundArea::from_paint_rect(source_positioning_rect);
    let mut primitives = Vec::new();
    if let Some(fill) = style.background.background_color.visible_color(style.color) {
        primitives.extend(clips.iter().map(|projection| {
            PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                projection.destination_clip(),
                Some(fill),
            ))
        }));
    }
    for projection in clips {
        let images = structural_table_background_image_primitives(
            positioning_area,
            PaintBackgroundArea::from_paint_rect(projection.source_clip()),
            style,
            base_url,
            root_url,
            resource_cache,
        );
        primitives.extend(images.into_iter().map(|primitive| {
            transform_table_column_image_primitive(primitive, projection.source_to_destination)
        }));
    }
    primitives
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_row_fragment_cell_clips(
    table_x: f32,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_tops: &[f32],
    row_heights: &[f32],
    row_index: usize,
) -> Vec<PaintRect> {
    let Some(placements) = table_grid.rows.get(row_index) else {
        return Vec::new();
    };
    let mut clips = Vec::new();
    for placement in placements {
        let mut cell_top = None;
        let mut cell_bottom = None;
        for (local_row, source_row) in fragment_rows.iter().cloned().enumerate() {
            if source_row < row_index || source_row >= row_index.saturating_add(placement.rowspan) {
                continue;
            }
            let (Some(row_top), Some(row_height)) = (
                row_tops.get(local_row).cloned(),
                row_heights.get(local_row).cloned(),
            ) else {
                continue;
            };
            if row_height <= 0.0 {
                continue;
            }
            cell_top = Some(cell_top.map_or(row_top, |top: f32| top.max(row_top)));
            let row_bottom = row_top - row_height;
            cell_bottom =
                Some(cell_bottom.map_or(row_bottom, |bottom: f32| bottom.min(row_bottom)));
        }
        let (Some(cell_top), Some(cell_bottom)) = (cell_top, cell_bottom) else {
            continue;
        };
        let cell_inline = column_plan.inline_bounds_for_span(placement.column, placement.colspan);
        clips.push(paint_space_rect(
            table_x + cell_inline.logical_start().get(),
            cell_bottom,
            cell_inline.logical_size().get(),
            (cell_top - cell_bottom).max(0.0),
        ));
    }
    clips
}

pub(in crate::layout::table) fn table_column_background_rect(
    table_x: f32,
    grid_top: f32,
    grid_height: f32,
    column_plan: &TableColumnPlan,
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
) -> Option<(PaintRect, TableInlineBounds)> {
    if start_column >= end_column || start_column >= column_plan.column_count() {
        return None;
    }
    let clamped_end = end_column.min(column_plan.column_count());
    let inline_bounds =
        column_plan.inline_bounds_for_span(start_column, clamped_end - start_column);
    let block_size = if matches!(
        style.writing_mode,
        WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr
    ) {
        used_length_percentage_or_auto(
            style.box_values.height.value().clone(),
            PercentageBasis::definite(layout_pt(grid_height)),
        )
        .map(|height| height.points())
        .unwrap_or(grid_height)
        .max(grid_height)
    } else {
        grid_height
    };
    let rect = TableGridRect::new(
        TableGridPoint::from_lengths(inline_bounds.start, TableGridLength::new(0.0)),
        TableGridSize::from_lengths(inline_bounds.size, TableGridLength::new(block_size)),
    );
    let placement = TableGridPlacement::with_axes(
        TableGridContentBoxTopLeft::new(PageTopPoint::new(table_x, grid_top)),
        column_plan.axes,
        TableGridLogicalSize::new(
            column_plan.total_width(),
            LogicalBlockContentSize::new(content_box_pt(block_size)),
        ),
    );
    let paint_rect = placement.overflow_clip_for(rect).paint_rect();
    Some((paint_rect, inline_bounds))
}

pub(in crate::layout::table) fn table_column_background_primitives_with_clip(
    paint_rect: PaintRect,
    style: &ComputedStyle,
    clip: PaintRect,
) -> Vec<PaintPrimitive> {
    let mut rects = Vec::new();
    if paint_rect.size.width <= 0.0
        || paint_rect.size.height <= 0.0
        || clip.size.width <= 0.0
        || clip.size.height <= 0.0
    {
        return Vec::new();
    }
    if let Some(fill) = style.background.background_color.visible_color(style.color) {
        let area = background_rect_clip_area_for_box(
            paint_rect,
            style,
            css::Edges::ZERO,
            style.background.background_clip,
            Some(clip),
        );
        if area.size.width > 0.0 && area.size.height > 0.0 {
            rects.push(RenderedRect::from_paint_rect(area, Some(fill)));
        }
    }
    rects.into_iter().map(PaintPrimitive::Rect).collect()
}

pub(in crate::layout::table) fn visible_column_span(
    start_column: usize,
    end_column: usize,
    collapsed_columns: &[bool],
) -> Option<(usize, usize)> {
    let clamped_end = end_column.min(collapsed_columns.len());
    let visible_start = (start_column..clamped_end).find(|index| !collapsed_columns[*index])?;
    let visible_end = (visible_start + 1..clamped_end)
        .rfind(|index| !collapsed_columns[*index])
        .map(|index| index + 1)
        .unwrap_or(visible_start + 1);
    Some((visible_start, visible_end))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn push_table_fragment_row_span_background(
    primitives: &mut Vec<PaintPrimitive>,
    inline_span: PageInlineSpan,
    row_tops: &[f32],
    row_heights: &[f32],
    start: usize,
    end: usize,
    fill: CssColor,
) {
    if let Some(bounds) =
        table_fragment_row_span_bounds(inline_span, row_tops, row_heights, start, end)
    {
        primitives.push(PaintPrimitive::Rect(RenderedRect::from_paint_rect(
            bounds.paint_rect(),
            Some(fill),
        )));
    }
}

pub(in crate::layout::table) fn table_fragment_row_span_bounds(
    inline_span: PageInlineSpan,
    row_tops: &[f32],
    row_heights: &[f32],
    start: usize,
    end: usize,
) -> Option<PaintClip> {
    if start >= end || end > row_tops.len() || end > row_heights.len() {
        return None;
    }
    let top = row_tops[start];
    let last = end - 1;
    let bottom = row_tops[last] - row_heights[last];
    let height = (top - bottom).max(0.0);
    (height > 0.0).then_some(
        PageTopRect::new(inline_span.left_x(), top, inline_span.width(), height).paint_clip(),
    )
}
