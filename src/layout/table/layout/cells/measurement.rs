//! Table-cell and table-row block-size measurement.

use crate::css::{
    self, BorderStyle, CaptionSide, ComputedStyle, EmptyCells, LayoutLength, PercentageBasis,
    Position, SemanticLengthExt, Stylesheets, layout_pt,
};
use crate::dom::Element;
use crate::layout::block::height_behaves_as_auto_for_margin_collapse;
use crate::layout::table::layout::{
    CollapsedTableGeometry, DefiniteTableCellBlockSizeBasis, PreparedTableCell,
    TableCellBaselineAlignmentContext, TableCellBaselineSet, TableCellContentPass,
    TableCellContentSizingPolicy, TableCellLayoutMetrics, TableGridLayoutContext,
    TableHeightDistributionTarget, TableHeightTarget, TableRowHeightPlan,
    distribute_table_height_extra, distribute_table_span_constraint,
    table_cell_alignment_baseline_set, table_cell_block_size_depends_on_parent_percentage,
    table_cell_border_box_height_with_insets, table_cell_border_insets,
    table_cell_can_consume_physical_y_row_baseline_for_alignment,
    table_cell_child_is_in_flow_float, table_cell_content_pass_from_committed_basis,
    table_cell_formatting_child_has_parent_percentage_block_size,
    table_cell_participates_in_physical_y_row_baseline, table_cell_row_sizing_border_box_height,
    table_content_height_from_plan, table_height_distribution_groups,
};
use crate::layout::table::{
    TableCaption, TableCell, TableCellBaselineOffset, TableCellContentBox, TableCellOuterBlockSize,
    TableCellPadding, TableCellPlacement, TableColumnPlan, TableGridArea, TableMetrics, TableRow,
    TableRowBaselineOffset, apply_table_cell_used_padding,
    table_cell_formatting_child_outer_height, table_cell_inline_text,
    table_cell_non_text_content_height, table_cell_replaced_content_height,
    table_cell_root_block_track_contribution, table_horizontal_borders, table_root_block_size,
    table_row_top, used_table_target_content_height,
};
use crate::layout::{
    BlockSizeBasisSource, BlockSizePercentageBasis, DocumentCanvasResolution, FloatContext,
    LayoutBuilder, LogicalBlockContentSize, LogicalInlineContentSize, PhysicalContentWidth,
    apply_used_box_metrics, block_size_percentage_basis_from_points, box_tree, collapse_margins,
    collapsible_first_child_start_margin_from_boxes, constrain, constrain_content_height,
    constrain_content_width, effective_overflow_for_style,
    formatting_box_can_only_create_phantom_line_boxes, has_atomic_inline_formatting_box,
    has_auto_height, has_direct_inline_content_box, has_non_inline_formatting_box,
    inline_text_from_formatting_boxes, intrinsic, is_replaced_element,
    is_self_collapsing_block_box, outer_margins_adjoin_block_siblings, replaced_element_kind,
    self_collapsing_block_margin_set_for_box, set_style_auto_height, set_style_used_height,
    used_border_widths, used_content_box_height_or_auto,
    used_content_box_height_or_auto_with_basis, used_content_box_width, used_length_percentage,
    used_length_percentage_or_auto_with_basis, used_max_height, used_min_height,
    used_property_containment,
};
use crate::units::{BorderBoxLength, ContentBoxLength, content_box_pt, non_content_pt};

/// The first row baseline exported by an inline table together with the
/// selected cell font's paint-coordinate adjustment.
///
/// Table sizing consumes `offset`, while an atomic `inline-table` must also
/// translate that CSS-layout coordinate into its captured paint fragment's
/// coordinate system.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRowBaseline {
    pub(in crate::layout::table) offset: TableRowBaselineOffset,
    pub(in crate::layout::table) rendered_font_adjustment: f32,
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::table) fn resolve_table_target_content_height(
        &self,
        table_style: &ComputedStyle,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        wrapper_border_box_block_size: Option<BorderBoxLength>,
        positioned_table_block_content_size: Option<LogicalBlockContentSize>,
        wrapper_non_grid_block_size: LayoutLength,
    ) -> Option<ContentBoxLength> {
        let vertical_non_content =
            non_content_pt(if let Some(collapsed_geometry) = collapsed_geometry {
                // Collapsed grid-edge borders are not ordinary wrapper borders,
                // but their conflict-resolved outer halves still consume space in
                // the used table border box. CSS `box-sizing: border-box` must
                // remove those actual insets before row-height distribution.
                // <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
                // <https://www.w3.org/TR/css-sizing-3/#box-sizing>
                collapsed_geometry.outer_insets.top + collapsed_geometry.outer_insets.bottom
            } else {
                let border_widths = used_border_widths(table_style);
                table_style.padding.top
                    + table_style.padding.bottom
                    + border_widths.top
                    + border_widths.bottom
                // Separated-border edge spacing is part of the table
                // content box.  A table's definite height constrains that
                // whole content box, including its edge spacing; only
                // padding and ordinary borders lie outside it.
                // <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
            });
        if let Some(wrapper_border_box_block_size) = wrapper_border_box_block_size {
            let assigned_grid_content_size = content_box_pt(
                (wrapper_border_box_block_size.points()
                    - vertical_non_content.points()
                    - wrapper_non_grid_block_size.points())
                .max(0.0),
            );
            // Grid and flex alignment assign a used wrapper border-box span,
            // but that span still participates in the table wrapper's own
            // min/max constraints.  In particular, a stretched auto-height
            // table must not bypass `max-height`; conversely, an authored
            // max-height alone must not manufacture a target for intrinsic
            // rows when no parent supplies a definite wrapper size.
            // <https://drafts.csswg.org/css-tables/#computing-the-table-height>
            return Some(
                used_table_target_content_height(
                    table_style,
                    self.block_percentage_context_stack
                        .current_percentage_basis(),
                    vertical_non_content,
                )
                .map_or(assigned_grid_content_size, |constraint_target| {
                    assigned_grid_content_size.min(constraint_target)
                }),
            );
        }
        if let Some(positioned_table_block_content_size) = positioned_table_block_content_size {
            // Absolute positioning has already resolved this definite size in
            // content-box space. Preserve that result as the table grid's
            // distribution target instead of re-measuring the intrinsic row
            // height from the cell contents.
            return Some(positioned_table_block_content_size.content_box_length());
        }
        used_table_target_content_height(
            table_style,
            self.block_percentage_context_stack
                .current_percentage_basis(),
            vertical_non_content,
        )
    }

    pub(in crate::layout::table) fn compute_table_reference_heights(
        &mut self,
        plan_rows: &mut [TableRowHeightPlan],
        context: &TableGridLayoutContext<'_, '_>,
        target_content_height_basis: BlockSizePercentageBasis,
    ) {
        if plan_rows.is_empty() {
            return;
        }

        for (row_index, row) in context.rows.iter().enumerate() {
            if plan_rows[row_index].collapsed {
                continue;
            }
            let row_style = self.style_for_table_row(row, context.table_style, context.stylesheets);
            if let Some(row_height) = used_length_percentage_or_auto_with_basis(
                table_root_block_size(&row_style),
                target_content_height_basis,
            )
            .map(|height| height.points())
            {
                plan_rows[row_index].reference = plan_rows[row_index].reference.max(row_height);
            }
            for placement in &context.grid.rows[row_index] {
                let cell = &row.cells[placement.cell];
                let Some(prepared) = self.prepare_table_cell(
                    cell,
                    row,
                    &row_style,
                    placement,
                    row_index,
                    0.0,
                    context.stylesheets,
                    context.table_cellpadding,
                    context.column_plan,
                    context.table_metrics.clone(),
                    context.collapsed_geometry,
                ) else {
                    continue;
                };
                let physical_height = self.table_cell_border_box_height_with_basis(
                    &prepared.row_sizing_style,
                    prepared.metrics.content_height,
                    target_content_height_basis,
                    prepared.borders,
                );
                let required_height = table_cell_root_block_track_contribution(
                    self,
                    cell,
                    &prepared.style,
                    context.table_style,
                    context.stylesheets,
                    Some(prepared.borders),
                    physical_height,
                );
                distribute_table_span_constraint(
                    plan_rows,
                    row_index,
                    placement.rowspan,
                    required_height,
                    context.table_metrics.clone(),
                    TableHeightTarget::Reference,
                );
            }
        }

        let groups = table_height_distribution_groups(context.rows);
        for (start, end) in groups {
            let Some(row_group) = context.rows[start].row_groups.last() else {
                continue;
            };
            let row_group_style =
                self.style_for_table_row_group(row_group, context.table_style, context.stylesheets);
            let Some(group_height) = used_length_percentage_or_auto_with_basis(
                table_root_block_size(&row_group_style),
                target_content_height_basis,
            )
            .map(|height| height.points()) else {
                continue;
            };
            distribute_table_span_constraint(
                plan_rows,
                start,
                end - start,
                group_height,
                context.table_metrics.clone(),
                TableHeightTarget::Reference,
            );
        }
    }

    pub(in crate::layout::table) fn table_cell_border_box_height_with_basis(
        &self,
        style: &ComputedStyle,
        content_height: f32,
        percentage_basis: BlockSizePercentageBasis,
        border_insets: css::Edges,
    ) -> f32 {
        table_cell_row_sizing_border_box_height(
            style,
            content_height,
            percentage_basis,
            border_insets,
        )
    }

    pub(in crate::layout::table) fn distribute_table_height_plan(
        &self,
        rows: &mut [TableRowHeightPlan],
        target: TableHeightDistributionTarget,
        table_metrics: TableMetrics,
    ) {
        for row in rows.iter_mut() {
            row.final_height = row.base;
        }
        let Some(target_content_height) = target.definite_content_height() else {
            for row in rows.iter_mut() {
                row.final_height = row.reference;
            }
            return;
        };
        // Row distribution is coordinate arithmetic over scalar row intervals;
        // preserve the table target's content-box type until this boundary.
        let target_content_height = target_content_height.points();

        let base =
            table_content_height_from_plan(rows, TableHeightTarget::Base, table_metrics.clone());
        let reference =
            table_content_height_from_plan(rows, TableHeightTarget::Reference, table_metrics);
        if target_content_height <= base + 0.01 {
            return;
        }

        if target_content_height <= reference + 0.01 {
            let extra = target_content_height - base;
            let capacity = rows
                .iter()
                .filter(|row| !row.collapsed)
                .map(|row| (row.reference - row.base).max(0.0))
                .sum::<f32>();
            if capacity <= 0.01 {
                distribute_table_height_extra(rows, extra, |row| !row.collapsed);
                return;
            }
            for row in rows.iter_mut().filter(|row| !row.collapsed) {
                let share = (row.reference - row.base).max(0.0) / capacity;
                row.final_height = row.base + extra * share;
            }
            return;
        }

        for row in rows.iter_mut() {
            row.final_height = row.reference;
        }
        let extra = target_content_height - reference;
        let distributed =
            distribute_table_height_extra(rows, extra, |row| !row.collapsed && row.auto);
        if distributed <= 0.0 {
            distribute_table_height_extra(rows, extra, |row| !row.collapsed);
        }
    }

    /// Return whether a separated-border row has only hidden empty cells.
    ///
    /// CSS 2.2 `empty-cells: hide` suppresses empty cell borders/backgrounds
    /// in separated-border tables; if all cells in a row are hidden and empty,
    /// the row has zero height and vertical border spacing on only one side.
    /// https://www.w3.org/TR/CSS22/tables.html#empty-cells
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_row_is_hidden_empty(
        &mut self,
        row: &TableRow<'_>,
        placements: &[TableCellPlacement],
        row_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_cellpadding: Option<TableCellPadding>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
    ) -> bool {
        if table_metrics.border_collapse == css::BorderCollapse::Collapse || placements.is_empty() {
            return false;
        }

        let mut saw_visible_column_cell = false;
        for placement in placements {
            let cell_inline =
                column_plan.inline_bounds_for_span(placement.column, placement.colspan);
            let cell_width = cell_inline.logical_size().get();
            if cell_width <= 0.0 {
                continue;
            }
            saw_visible_column_cell = true;
            let cell = &row.cells[placement.cell];
            let mut cell_style = self.style_for_table_cell(cell, row, row_style, stylesheets);
            apply_table_cell_used_padding(
                &mut cell_style,
                table_cellpadding,
                PercentageBasis::definite(LogicalInlineContentSize::new(content_box_pt(
                    cell_width,
                ))),
            );
            let cell_is_empty = table_cell_inline_text(cell).is_empty()
                && self.table_cell_non_text_content_height(
                    cell,
                    stylesheets,
                    (cell_width
                        - cell_style.padding.left
                        - cell_style.padding.right
                        - table_horizontal_borders(&cell_style).points())
                    .max(0.0),
                ) <= 0.0;
            if cell_style.empty_cells == EmptyCells::Show || !cell_is_empty {
                return false;
            }
        }
        saw_visible_column_cell
    }

    pub(in crate::layout::table) fn estimate_table_captions_height(
        &mut self,
        captions: &[TableCaption<'_>],
        table_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_width: PhysicalContentWidth,
        side: CaptionSide,
    ) -> f32 {
        let table_width = table_width.points();
        captions
            .iter()
            .filter_map(|caption| {
                let caption_style = self.style_for_table_caption(caption, table_style, stylesheets);
                (caption_style.caption_side == side).then(|| {
                    // A size-contained caption contributes its own principal
                    // box, not the height of the text that subsequently
                    // overflows from that box.  This is deliberately kept in
                    // the caption measurement boundary: the normal caption
                    // layout below still formats and paints the descendants.
                    // <https://www.w3.org/TR/css-contain-1/#containment-size>
                    if used_property_containment(caption.element, &caption_style).size {
                        let mut used_style = caption_style.used_style().clone();
                        let metrics = apply_used_box_metrics(
                            &mut used_style,
                            PercentageBasis::definite(layout_pt(table_width.max(0.0))),
                        );
                        let vertical_non_content = metrics.vertical_non_content_length().points();
                        let content_height = used_content_box_height_or_auto(
                            &used_style,
                            layout_pt(table_width.max(0.0)),
                            non_content_pt(vertical_non_content),
                        )
                        .map(SemanticLengthExt::points)
                        .or_else(|| {
                            used_style
                                .contain_intrinsic_size
                                .height
                                .clone()
                                .map(|height| {
                                    used_length_percentage(
                                        height,
                                        PercentageBasis::definite(layout_pt(table_width.max(0.0))),
                                    )
                                    .points()
                                })
                        })
                        .unwrap_or(0.0);
                        content_height.max(0.0) + vertical_non_content
                    } else {
                        self.estimate_element_height(
                            caption.element,
                            &caption_style,
                            stylesheets,
                            table_width,
                            caption.children.as_deref(),
                        )
                        .unwrap_or(0.0)
                    }
                })
            })
            .sum()
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_row_baseline_offset(
        &mut self,
        row_index: usize,
        row: &TableRow<'_>,
        placements: &[TableCellPlacement],
        row_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_cellpadding: Option<TableCellPadding>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> Option<TableRowBaseline> {
        // CSS 2.2 table height layout aligns cells with `vertical-align:
        // baseline` by an actual in-flow cell baseline, falling back to the
        // bottom content edge when the cell has non-text content but no
        // baseline. Empty cells must not grow the row as if they had a line.
        // <https://www.w3.org/TR/CSS22/tables.html#height-layout>
        placements
            .iter()
            .filter_map(|placement| {
                let cell = &row.cells[placement.cell];
                let prepared = self.prepare_table_cell(
                    cell,
                    row,
                    row_style,
                    placement,
                    row_index,
                    0.0,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics.clone(),
                    collapsed_geometry,
                )?;
                if !table_cell_participates_in_physical_y_row_baseline(
                    &prepared.style,
                    row_style,
                    placement,
                ) {
                    return None;
                }
                self.table_cell_physical_y_row_baseline_candidate(cell, &prepared, stylesheets)
                    .map(|offset| TableRowBaseline {
                        offset: TableRowBaselineOffset::new(layout_pt(offset.points())),
                        rendered_font_adjustment: self
                            .font_system
                            .rendered_first_line_baseline_offset(&prepared.style)
                            .points(),
                    })
            })
            .max_by(|left, right| left.offset.points().total_cmp(&right.offset.points()))
    }

    /// Return the content baseline exposed by a table row to an enclosing
    /// `inline-table`.
    ///
    /// This intentionally does not require a cell to have `vertical-align:
    /// baseline`: row-cell alignment controls table height layout, whereas CSS
    /// 2.2 defines an inline-table's exported baseline as the first row's
    /// content baseline.
    /// <https://www.w3.org/TR/CSS22/tables.html#table-display>
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_row_inline_baseline_offset(
        &mut self,
        row_index: usize,
        row: &TableRow<'_>,
        placements: &[TableCellPlacement],
        row_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_cellpadding: Option<TableCellPadding>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> Option<TableRowBaseline> {
        placements
            .iter()
            .filter_map(|placement| {
                let cell = &row.cells[placement.cell];
                let prepared = self.prepare_table_cell(
                    cell,
                    row,
                    row_style,
                    placement,
                    row_index,
                    0.0,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics.clone(),
                    collapsed_geometry,
                )?;
                // This baseline belongs to the inline-table's own logical
                // block axis.  Row-height planning has a distinct physical-Y
                // eligibility gate, but reusing that gate here suppresses a
                // perfectly valid vertical-writing table baseline before its
                // atom can project it into the parent line.
                // <https://www.w3.org/TR/CSS22/tables.html#table-display>
                self.table_cell_physical_y_row_baseline_candidate(cell, &prepared, stylesheets)
                    .map(|offset| TableRowBaseline {
                        offset: TableRowBaselineOffset::new(layout_pt(offset.points())),
                        rendered_font_adjustment: self
                            .font_system
                            .rendered_first_line_baseline_offset(&prepared.style)
                            .points(),
                    })
            })
            .max_by(|left, right| left.offset.points().total_cmp(&right.offset.points()))
    }

    pub(in crate::layout::table) fn table_cell_row_baseline_offset_for_alignment(
        &mut self,
        context: &TableCellBaselineAlignmentContext<'_>,
        placement: &TableCellPlacement,
        cell_style: &ComputedStyle,
    ) -> Option<TableRowBaselineOffset> {
        if !table_cell_can_consume_physical_y_row_baseline_for_alignment(
            cell_style,
            context.row_style,
        ) {
            return None;
        }
        if table_cell_alignment_baseline_set(cell_style) == TableCellBaselineSet::First {
            return context.row_baseline_offset;
        }
        let target_row_index = (context.row_index + placement.rowspan.saturating_sub(1))
            .min(context.rows.len().saturating_sub(1));
        if target_row_index == context.row_index {
            return context.row_baseline_offset;
        }
        let target_row = context.rows.get(target_row_index)?;
        let target_row_style =
            self.style_for_table_row(target_row, context.table_style, context.stylesheets);
        let target_baseline = self.table_row_baseline_offset(
            target_row_index,
            target_row,
            context.grid.rows.get(target_row_index)?,
            &target_row_style,
            context.stylesheets,
            context.table_cellpadding,
            context.column_plan,
            context.table_metrics.clone(),
            context.collapsed_geometry,
        )?;
        let origin_top = table_row_top(
            0.0,
            context.planned_row_heights,
            context.planned_row_occupancy,
            context.table_metrics.clone(),
            context.row_index,
        );
        let target_top = table_row_top(
            0.0,
            context.planned_row_heights,
            context.planned_row_occupancy,
            context.table_metrics.clone(),
            target_row_index,
        );
        Some(TableRowBaselineOffset::new(layout_pt(
            (origin_top - target_top).max(0.0) + target_baseline.offset.points(),
        )))
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_row_baseline_only_offset(
        &mut self,
        row_index: usize,
        row: &TableRow<'_>,
        placements: &[TableCellPlacement],
        row_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_cellpadding: Option<TableCellPadding>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> Option<TableRowBaselineOffset> {
        placements
            .iter()
            .filter_map(|placement| {
                let cell = &row.cells[placement.cell];
                let prepared = self.prepare_table_cell(
                    cell,
                    row,
                    row_style,
                    placement,
                    row_index,
                    0.0,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics.clone(),
                    collapsed_geometry,
                )?;
                if !table_cell_participates_in_physical_y_row_baseline(
                    &prepared.style,
                    row_style,
                    placement,
                ) {
                    return None;
                }
                self.table_cell_physical_y_row_baseline_candidate(cell, &prepared, stylesheets)
            })
            .map(|offset| TableRowBaselineOffset::new(layout_pt(offset.points())))
            .reduce(|left, right| {
                if left.points() >= right.points() {
                    left
                } else {
                    right
                }
            })
    }

    pub(in crate::layout::table) fn table_cell_physical_y_row_baseline_candidate(
        &mut self,
        cell: &TableCell<'_>,
        prepared: &PreparedTableCell,
        stylesheets: &Stylesheets<'_>,
    ) -> Option<TableCellBaselineOffset> {
        let available_width = (prepared.width()
            - prepared.style.padding.left
            - prepared.style.padding.right
            - prepared.borders.left
            - prepared.borders.right)
            .max(0.0);
        self.table_cell_alignment_baseline_offset(
            cell,
            &prepared.style,
            stylesheets,
            available_width,
            prepared.borders,
        )
        .or_else(|| {
            let replaced_height = table_cell_replaced_content_height(cell);
            let fallback_content_height = if replaced_height.points() > 0.0 {
                replaced_height.points()
            } else {
                prepared.metrics.content_height
            };
            (fallback_content_height > 0.0).then(|| {
                self.table_cell_content_bottom_baseline(
                    &prepared.style,
                    fallback_content_height,
                    prepared.borders,
                )
            })
        })
    }

    /// Measure the content, border box, and table-cell baseline for row layout.
    ///
    /// CSS 2.2 and CSS Tables align table cells by the first in-flow line-box
    /// baseline or first in-flow table-row baseline in the cell; if neither
    /// exists, the baseline is the bottom content edge. Anonymous table object
    /// construction can leave cells with no intrinsic column width when their
    /// only contents are out-of-flow; those cells still have descendants that
    /// must be processed for positioned layout:
    /// <https://www.w3.org/TR/CSS22/tables.html#anonymous-boxes>,
    /// <https://www.w3.org/TR/CSS22/tables.html#height-layout> and
    /// <https://drafts.csswg.org/css-tables-3/#row-layout>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn prepare_table_cell(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        placement: &TableCellPlacement,
        row_index: usize,
        _table_x: f32,
        stylesheets: &Stylesheets<'_>,
        table_cellpadding: Option<TableCellPadding>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> Option<PreparedTableCell> {
        let mut style = self.style_for_table_cell(cell, row, row_style, stylesheets);
        let area = TableGridArea::from_placement(row_index, placement);
        let inline_bounds = column_plan.inline_bounds_for_area(area);
        let width = inline_bounds.logical_size().get().max(0.0);
        // A table cell's percentage padding resolves against the final table
        // grid inline size, including only the border spacing between tracks.
        // It must not use the cell span width, which itself includes the
        // padding being resolved.
        // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
        let table_grid_inline_size = column_plan.total_width();
        apply_table_cell_used_padding(
            &mut style,
            table_cellpadding,
            PercentageBasis::definite(table_grid_inline_size),
        );
        let borders = table_cell_border_insets(
            &style,
            placement,
            row_index,
            table_metrics.clone(),
            collapsed_geometry,
        );
        if table_metrics.border_collapse == css::BorderCollapse::Collapse {
            apply_resolved_collapsed_border_layout_edges(&mut style, borders);
        }
        let row_sizing_style = self.table_cell_row_sizing_style(&style, row_style, width);
        let metrics = self.table_cell_layout_metrics(
            cell,
            &style,
            &row_sizing_style,
            stylesheets,
            width,
            borders,
        );
        let text = table_cell_inline_text(cell);
        Some(PreparedTableCell {
            style: style.used_style().clone(),
            row_sizing_style,
            area,
            inline_bounds,
            borders,
            metrics,
            text,
        })
    }

    pub(in crate::layout::table) fn table_cell_layout_metrics(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        row_sizing_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        cell_width: f32,
        border_insets: css::Edges,
    ) -> TableCellLayoutMetrics {
        self.with_positioned_layout_suppressed(|layout| {
            let available_width = (cell_width
                - cell_style.padding.left
                - cell_style.padding.right
                - border_insets.left
                - border_insets.right)
                .max(0.0);
            let text_height =
                layout.table_cell_text_content_height(cell, cell_style, available_width);
            // Anonymous table cells still form an inline formatting context. A
            // separately measured non-text atomic inline only captures the
            // tallest individual atom; the prepared sequence is what preserves
            // intervening whitespace and therefore a wrapped second line.
            // <https://www.w3.org/TR/CSS22/tables.html#anonymous-boxes>
            // <https://www.w3.org/TR/css-inline-3/#line-layout>
            let inline_sequence_height = cell.children.as_deref().and_then(|children| {
                layout.table_cell_inline_sequence_height(
                    cell_style,
                    children,
                    stylesheets,
                    available_width,
                    PercentageBasis::indefinite(),
                )
            });
            let explicit_content_pass = DefiniteTableCellBlockSizeBasis::from_explicit_cell_height(
                cell_style,
                border_insets,
            )
            .map(TableCellContentPass::FinalRelayout);
            let non_text_height = if let Some(content_pass) = explicit_content_pass {
                cell.children
                    .as_deref()
                    .map(|children| {
                        table_cell_replaced_content_height(cell).points().max(
                            layout.table_cell_children_final_relayout_height(
                                children,
                                stylesheets,
                                available_width,
                                content_pass,
                            ),
                        )
                    })
                    .unwrap_or_else(|| table_cell_non_text_content_height(cell).points())
            } else {
                layout.table_cell_non_text_content_height(cell, stylesheets, available_width)
            };
            let content_height = text_height.max(non_text_height).max(
                inline_sequence_height
                    .map(TableCellOuterBlockSize::points)
                    .unwrap_or(0.0),
            );
            debug_assert!(content_height >= 0.0);
            let border_box_height = table_cell_border_box_height_with_insets(
                row_sizing_style,
                content_height,
                border_insets,
            );
            let baseline_offset = layout
                .table_cell_alignment_baseline_offset(
                    cell,
                    cell_style,
                    stylesheets,
                    available_width,
                    border_insets,
                )
                .unwrap_or_else(|| {
                    layout.table_cell_content_bottom_baseline(
                        cell_style,
                        content_height,
                        border_insets,
                    )
                });

            TableCellLayoutMetrics {
                content_height,
                border_box_height,
                baseline_offset,
            }
        })
    }

    /// Remeasure table-cell content after row height distribution.
    ///
    /// CSS Tables 3 lays out cell contents a second time when table or cell
    /// heights make descendant percentage block sizes definite. The first-pass
    /// row minimum must still ignore those percentages, while this final pass
    /// supplies the used cell content height as the containing-block basis for
    /// table-cell alignment and fragmentation:
    /// <https://drafts.csswg.org/css-tables-3/#table-cell-content-relayout> and
    /// <https://www.w3.org/TR/CSS22/tables.html#height-layout>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_cell_final_relayout_metrics(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        content_box: TableCellContentBox,
        border_insets: css::Edges,
        first_pass: TableCellLayoutMetrics,
        content_pass: TableCellContentPass,
    ) -> TableCellLayoutMetrics {
        let Some(final_basis) = content_pass.final_basis() else {
            return first_pass;
        };
        let percentage_height_basis = final_basis.percentage_basis();

        self.with_positioned_layout_suppressed(|layout| {
            // This box was projected through the table root's writing mode
            // before entering this physical measurement API. Its width is
            // therefore the actual horizontal content measure, not a root
            // logical inline track that happens to be named `width`.
            let available_width = content_box.width();
            let text_height =
                layout.table_cell_text_content_height(cell, cell_style, available_width);
            let inline_sequence_height = cell.children.as_deref().and_then(|children| {
                layout.table_cell_inline_sequence_height(
                    cell_style,
                    children,
                    stylesheets,
                    available_width,
                    percentage_height_basis,
                )
            });
            let non_text_height = cell
                .children
                .as_deref()
                .map(|children| {
                    table_cell_replaced_content_height(cell).points().max(
                        layout.table_cell_children_final_relayout_height(
                            children,
                            stylesheets,
                            available_width,
                            content_pass,
                        ),
                    )
                })
                .unwrap_or_else(|| table_cell_non_text_content_height(cell).points());
            let content_height = text_height.max(non_text_height).max(
                inline_sequence_height
                    .map(TableCellOuterBlockSize::points)
                    .unwrap_or(0.0),
            );
            let baseline_offset = if text_height > 0.0 && text_height >= non_text_height {
                layout
                    .table_cell_alignment_baseline_offset(
                        cell,
                        cell_style,
                        stylesheets,
                        available_width,
                        border_insets,
                    )
                    .unwrap_or_else(|| {
                        layout.table_cell_content_bottom_baseline(
                            cell_style,
                            content_height,
                            border_insets,
                        )
                    })
            } else {
                layout.table_cell_content_bottom_baseline(cell_style, content_height, border_insets)
            };
            TableCellLayoutMetrics {
                content_height,
                border_box_height: first_pass.border_box_height,
                baseline_offset,
            }
        })
    }

    /// Resolve table-cell row-track constraints in the table row flow.
    ///
    /// A cell can establish a distinct writing mode for its contents. A
    /// physical-height `ch` term is resolved by table row sizing in the table
    /// track context, whereas an already definite logical `inline-size`
    /// remains a real physical row constraint. Keep the cell's original style
    /// for content layout, alignment, and paint, while using this row-flow
    /// surrogate solely for row measurement.
    /// <https://drafts.csswg.org/css-tables-3/#row-layout>
    /// <https://drafts.csswg.org/css-writing-modes-4/#writing-mode>
    pub(in crate::layout::table) fn table_cell_row_sizing_style(
        &mut self,
        cell_style: &ComputedStyle,
        row_style: &ComputedStyle,
        column_inline_size: f32,
    ) -> ComputedStyle {
        let mut style = cell_style.clone();
        if cell_style.writing_mode != row_style.writing_mode {
            if cell_style.box_values.height.is_deferred_font_metric() {
                set_style_used_height(&mut style, column_inline_size.max(0.0));
            } else if cell_style.box_values.height.is_auto() {
                set_style_auto_height(&mut style);
                style.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
                style.box_values.max_height = css::ComputedLengthPercentageOrAuto::Auto;
            }
            return style;
        }
        style.writing_mode = row_style.writing_mode;
        style.direction = row_style.direction;
        style.text_orientation = row_style.text_orientation;
        let ch_advance = self.ch_advance_for_style(&style, style.requires_ch_advance());
        style
            .box_values
            .height
            .resolve_font_metric_lengths(ch_advance);
        style
            .box_values
            .min_height
            .resolve_font_metric_lengths(ch_advance);
        style
            .box_values
            .max_height
            .resolve_font_metric_lengths(ch_advance);
        style
    }

    /// Measure non-text content for table row sizing using durable child boxes.
    ///
    /// CSS table row height depends on the minimum height required by cell
    /// content. Nested table boxes already carry a table fragment with row,
    /// caption, spacing, and span information, so row sizing must reuse the
    /// table measurement path instead of approximating from the child list:
    /// <https://www.w3.org/TR/CSS22/tables.html#height-layout> and
    /// <https://drafts.csswg.org/css-tables-3/#row-layout>.
    pub(in crate::layout::table) fn table_cell_non_text_content_height(
        &mut self,
        cell: &TableCell<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> f32 {
        let Some(children) = cell.children.as_deref() else {
            return table_cell_non_text_content_height(cell).points();
        };

        table_cell_replaced_content_height(cell).points().max(
            self.table_cell_children_non_text_content_height(
                children,
                stylesheets,
                available_width,
            ),
        )
    }

    /// Measure table-cell non-text content, including direct floated children.
    ///
    /// CSS 2.2 table cells establish a block formatting context for their
    /// contents, and floated descendants are blockified before float placement.
    /// Auto row height must therefore include the lowest direct float margin-box
    /// edge, matching the normal block auto-height estimator:
    /// <https://www.w3.org/TR/CSS22/tables.html#model>,
    /// <https://www.w3.org/TR/CSS22/visuren.html#dis-pos-flo>, and
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height>.
    pub(in crate::layout::table) fn table_cell_children_non_text_content_height(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> f32 {
        let mut block_flow = TableCellBlockFlowHeight::default();
        let mut inline_line_height = 0.0_f32;
        let mut estimated_float_context = FloatContext { shapes: Vec::new() };
        let mut estimated_float_bottom = 0.0_f32;
        let has_block_flow_child = has_non_inline_formatting_box(children);

        for child in children {
            // Whitespace-only inline content between block children produces
            // phantom line boxes. It must not become an in-flow line that
            // terminates adjoining margin collapse in the table-cell BFC.
            // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
            // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
            if has_block_flow_child && formatting_box_can_only_create_phantom_line_boxes(child) {
                continue;
            }
            if let box_tree::FormattingBox::Text(box_) = child {
                inline_line_height = inline_line_height.max(self.estimate_text_physical_height(
                    &box_.text,
                    &box_.style,
                    available_width,
                    0.0,
                    0.0,
                ));
                continue;
            }
            if table_cell_child_is_in_flow_float(child) {
                let Some((child_element, _, child_style, child_children)) = child.element_parts()
                else {
                    continue;
                };
                if let Some(float_bottom) = self.table_cell_float_bottom_for_content_pass(
                    &mut estimated_float_context,
                    child_element,
                    child_style,
                    stylesheets,
                    available_width,
                    Some(child_children),
                    PercentageBasis::indefinite(),
                ) {
                    estimated_float_bottom = estimated_float_bottom.min(float_bottom);
                    continue;
                }
            }
            if let box_tree::FormattingBox::Inline(box_) = child {
                if matches!(
                    box_.core.style.position,
                    Position::Absolute | Position::Fixed
                ) {
                    continue;
                }
                // Inline content before a block child occupies its own line in
                // the table-cell normal-flow sequence. It therefore adds to
                // the row minimum instead of competing with that block's
                // height through a `max` calculation.
                // <https://www.w3.org/TR/CSS22/tables.html#height-layout>
                inline_line_height = inline_line_height.max(
                    self.table_cell_children_text_content_height(
                        &box_.core.children,
                        available_width,
                    )
                    .max(box_.core.style.line_height),
                );
                continue;
            }
            if let Some(inline_height) =
                self.table_cell_measured_inline_outer_height(child, stylesheets, available_width)
            {
                inline_line_height = inline_line_height.max(inline_height.points());
                continue;
            }
            if inline_line_height > 0.0 {
                block_flow.push_atomic_height(inline_line_height);
                inline_line_height = 0.0;
            }
            if let Some(collapsed_margin) = table_cell_self_collapsing_block_margin(
                child,
                PercentageBasis::indefinite(),
                self.document_canvas_overflow,
            ) {
                block_flow.push_collapsed_margin(collapsed_margin.points());
                continue;
            }
            let child_height =
                self.table_cell_measured_block_child_height(child, stylesheets, available_width);
            block_flow.push_child_height(child, child_height);
        }

        if inline_line_height > 0.0 {
            block_flow.push_atomic_height(inline_line_height);
        }

        block_flow.finish().max(-estimated_float_bottom)
    }

    /// Measure inline content containing atomic inline boxes for table row sizing.
    ///
    /// CSS Inline includes atomic inline boxes in line box sizing, even when
    /// the containing block's own `line-height` is zero. Reusing the prepared
    /// inline sequence keeps table-cell row minimum sizing aligned with final
    /// table-cell painting:
    /// <https://www.w3.org/TR/css-inline-3/#atomic-inline> and
    /// <https://drafts.csswg.org/css-tables-3/#row-layout>.
    pub(in crate::layout::table) fn table_cell_inline_sequence_height(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> Option<TableCellOuterBlockSize> {
        if !percentage_height_basis.is_definite()
            && children.iter().all(|child| {
                matches!(
                    child,
                    box_tree::FormattingBox::AtomicInline(_) | box_tree::FormattingBox::Replaced(_)
                )
            })
            && children
                .iter()
                .any(table_cell_formatting_child_has_parent_percentage_block_size)
        {
            // A direct atomic-only cell still forms an inline sequence, but
            // CSS Tables measures each percentage-dependent atom as `auto`
            // during row-minimum sizing. The general inline collector has no
            // table-pass policy, so use the pass-aware atom measurement here
            // before the final cell content height is committed.
            // <https://drafts.csswg.org/css-tables-3/#row-layout>
            return children
                .iter()
                .filter_map(|child| {
                    self.table_cell_measured_inline_outer_height(
                        child,
                        stylesheets,
                        available_width,
                    )
                })
                .reduce(|left, right| {
                    if left.points() >= right.points() {
                        left
                    } else {
                        right
                    }
                });
        }
        self.table_cell_nested_inline_sequence_for_children(
            style,
            children,
            stylesheets,
            None,
            available_width,
            percentage_height_basis,
        )
        .map(|plan| TableCellOuterBlockSize::new(layout_pt(plan.sequence.total_height())))
    }

    pub(in crate::layout::table) fn table_cell_measured_block_child_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> f32 {
        let has_parent_percentage =
            table_cell_formatting_child_has_parent_percentage_block_size(child);
        match child {
            box_tree::FormattingBox::Table(box_) => {
                if matches!(
                    box_.core.style.position,
                    Position::Absolute | Position::Fixed
                ) {
                    return 0.0;
                }
                if !has_parent_percentage {
                    return self.estimate_table_height(
                        box_.core.element,
                        &box_.core.style,
                        stylesheets,
                        available_width,
                        &box_.fragment,
                    );
                }
                let style = self.table_cell_content_sizing_style(
                    &box_.core.style,
                    TableCellContentSizingPolicy::RowMinimum,
                );
                self.estimate_table_height(
                    box_.core.element,
                    &style,
                    stylesheets,
                    available_width,
                    &box_.fragment,
                )
            }
            box_tree::FormattingBox::Block(box_) => {
                // A block descendant without a percentage block-size dependency
                // can use the ordinary block formatter.  Besides avoiding an
                // unnecessary row-minimum approximation, this preserves the
                // inline line box's descent below atomic inline children.
                // The row-minimum policy is only needed to break a percentage
                // dependency cycle while determining the row's minimum height.
                if !has_parent_percentage {
                    return self.table_cell_measured_element_child_height(
                        box_.core.element,
                        &box_.core.style,
                        &box_.core.children,
                        stylesheets,
                        available_width,
                        child,
                    );
                }
                if self
                    .document_canvas_overflow
                    .is_document_canvas_flow_element(box_.core.element)
                {
                    return self.table_cell_measured_element_child_height(
                        box_.core.element,
                        &box_.core.style,
                        &box_.core.children,
                        stylesheets,
                        available_width,
                        child,
                    );
                }
                self.table_cell_row_minimum_element_outer_height(
                    box_.core.element,
                    &box_.core.style,
                    &box_.core.children,
                    stylesheets,
                    available_width,
                    child,
                )
            }
            box_tree::FormattingBox::Flex(box_) => self.table_cell_measured_element_child_height(
                box_.core.element,
                &box_.core.style,
                &box_.core.children,
                stylesheets,
                available_width,
                child,
            ),
            box_tree::FormattingBox::AtomicInline(_) | box_tree::FormattingBox::Replaced(_) => {
                // CSS Tables row minimum sizing treats percentage-dependent
                // atomic and replaced content as auto. The inline helper owns
                // that pass-specific measurement; using the durable outer
                // height here would feed fallback replaced geometry back into
                // the row plan before the cell has a committed height.
                // <https://drafts.csswg.org/css-tables-3/#row-layout>
                self.table_cell_measured_inline_outer_height(child, stylesheets, available_width)
                    .map(TableCellOuterBlockSize::points)
                    .unwrap_or_else(|| table_cell_formatting_child_outer_height(child).points())
            }
            // An anonymous block is one contiguous inline run generated by
            // CSS 2.2's block-in-inline transformation. Measure its prepared
            // line sequence directly: recursively taking the maximum child
            // line height loses cross-inline vertical alignment (for example
            // a `vertical-align: top` ordinal beside a smaller inline), and
            // can make the following block overlap the next table row.
            // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
            // <https://drafts.csswg.org/css-tables-3/#row-layout>
            box_tree::FormattingBox::AnonymousBlock(box_)
                if !has_non_inline_formatting_box(&box_.children) =>
            {
                self.table_cell_inline_sequence_height(
                    &box_.style,
                    &box_.children,
                    stylesheets,
                    available_width,
                    PercentageBasis::indefinite(),
                )
                .map(TableCellOuterBlockSize::points)
                .unwrap_or(0.0)
            }
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .table_cell_children_non_text_content_height(
                    &box_.children,
                    stylesheets,
                    available_width,
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .table_cell_children_non_text_content_height(
                    &box_.core.children,
                    stylesheets,
                    available_width,
                ),
            _ => table_cell_formatting_child_outer_height(child).points(),
        }
    }

    pub(in crate::layout::table) fn table_cell_final_relayout_child_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> f32 {
        self.table_cell_measured_block_child_final_relayout_height(
            child,
            stylesheets,
            available_width,
            percentage_height_basis,
        )
    }

    pub(in crate::layout::table) fn table_cell_measured_element_child_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        fallback_child: &box_tree::FormattingBox<'_>,
    ) -> f32 {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return 0.0;
        }
        if self
            .document_canvas_overflow
            .is_document_canvas_flow_element(element)
        {
            return self.table_cell_measured_document_canvas_child_height(
                style,
                children,
                stylesheets,
                available_width,
            );
        }
        if !table_cell_formatting_child_has_parent_percentage_block_size(fallback_child) {
            return self
                .estimate_element_height(
                    element,
                    style,
                    stylesheets,
                    available_width,
                    Some(children),
                )
                .unwrap_or_else(|| {
                    table_cell_formatting_child_outer_height(fallback_child).points()
                });
        }
        self.table_cell_row_minimum_element_outer_height(
            element,
            style,
            children,
            stylesheets,
            available_width,
            fallback_child,
        )
    }

    /// Measure a table-cell descendant for first-pass row minimum sizing.
    ///
    /// CSS Tables 3 excludes heights that depend on the final parent cell
    /// block-size from the first pass. This estimator mirrors normal block-like
    /// height estimation, but applies the table row-minimum sizing policy
    /// recursively so nested percentage heights do not accidentally become
    /// definite by using their own intrinsic content as a percentage basis:
    /// <https://drafts.csswg.org/css-tables-3/#row-layout>.
    pub(in crate::layout::table) fn table_cell_row_minimum_element_outer_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        fallback_child: &box_tree::FormattingBox<'_>,
    ) -> f32 {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return 0.0;
        }

        let cyclic_percentage_scroll_container =
            table_cell_block_size_depends_on_parent_percentage(
                style.box_values.height.value().clone(),
            ) && style.box_values.min_height.length_if_no_percent().is_some()
                && matches!(
                    effective_overflow_for_style(style),
                    css::Overflow::Auto | css::Overflow::Scroll
                );
        let style =
            self.table_cell_content_sizing_style(style, TableCellContentSizingPolicy::RowMinimum);
        if replaced_element_kind(element).is_some() {
            return self
                .estimate_element_height(
                    element,
                    &style,
                    stylesheets,
                    available_width,
                    Some(children),
                )
                .unwrap_or_else(|| {
                    table_cell_formatting_child_outer_height(fallback_child).points()
                });
        }
        if style.display.is_table() {
            self.estimate_element_height(
                element,
                &style,
                stylesheets,
                available_width,
                Some(children),
            )
            .unwrap_or_else(|| table_cell_formatting_child_outer_height(fallback_child).points())
        } else {
            self.table_cell_row_minimum_block_like_outer_height(
                element,
                &style,
                children,
                stylesheets,
                available_width,
                cyclic_percentage_scroll_container,
            )
        }
    }

    pub(in crate::layout::table) fn table_cell_row_minimum_block_like_outer_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_outer_width: f32,
        cyclic_percentage_scroll_container: bool,
    ) -> f32 {
        let mut used_style = style.clone();
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_outer_width.max(0.0))),
        );
        let style = &used_style;
        // The computed height remains necessary for final table-cell sizing,
        // but CSS Sizing values that behave as `auto` use an auto proxy for
        // this self-collapsing predicate.
        // <https://drafts.csswg.org/css-sizing-3/#behave-auto>
        let mut margin_collapse_style = None;
        if height_behaves_as_auto_for_margin_collapse(style, PercentageBasis::indefinite()) {
            let mut style_for_margin_collapse = style.clone();
            style_for_margin_collapse
                .box_values
                .height
                .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
            margin_collapse_style = Some(style_for_margin_collapse);
        }
        let margin_collapse_style = margin_collapse_style.as_ref().unwrap_or(style);
        if is_self_collapsing_block_box(
            element,
            margin_collapse_style,
            children,
            self.document_canvas_overflow,
        ) {
            let descendant_start_margin = collapsible_first_child_start_margin_from_boxes(
                children,
                element,
                style,
                self.document_canvas_overflow,
            );
            return self_collapsing_block_margin_set_for_box(style, descendant_start_margin)
                .collapsed()
                .points()
                .max(0.0);
        }
        let horizontal_extras = box_metrics.horizontal_non_content_length().points();
        let requested_content_width = if matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        ) {
            let (min_content, max_content) = self.block_intrinsic_content_widths(
                element,
                style,
                stylesheets,
                Some(children),
                available_outer_width,
            );
            intrinsic::content_box_width_from_intrinsic(
                style,
                layout_pt(available_outer_width),
                non_content_pt(horizontal_extras),
                content_box_pt(min_content),
                content_box_pt(max_content),
                intrinsic::IntrinsicAutoWidth::FillAvailable,
            )
            .points()
        } else {
            used_content_box_width(
                style,
                layout_pt(available_outer_width),
                non_content_pt(horizontal_extras),
            )
            .points()
        };
        let content_width = constrain_content_width(
            style,
            content_box_pt(requested_content_width),
            PercentageBasis::definite(layout_pt(available_outer_width)),
        )
        .points()
        .max(style.font_size);

        let mut block_flow = TableCellBlockFlowHeight::default();
        if !has_non_inline_formatting_box(children)
            && (has_direct_inline_content_box(children)
                || has_atomic_inline_formatting_box(children))
        {
            if has_atomic_inline_formatting_box(children) {
                if let Some(inline_height) = self.table_cell_inline_sequence_height(
                    style,
                    children,
                    stylesheets,
                    content_width,
                    PercentageBasis::indefinite(),
                ) {
                    block_flow.push_atomic_height(inline_height.points());
                }
            } else {
                let text = inline_text_from_formatting_boxes(children);
                if !text.is_empty() {
                    block_flow.push_atomic_height(self.estimate_text_physical_height(
                        &text,
                        style,
                        content_width,
                        style.padding.left,
                        style.padding.right,
                    ));
                }
            }
        }

        for child_box in children {
            // A scroll container with a percentage block size participates in
            // table row-minimum sizing through its own constraints, not the
            // overflowing descendants that the second pass will clip. This
            // breaks the otherwise circular `height: 100%` dependency while
            // preserving explicit minimum sizes.
            // <https://drafts.csswg.org/css-tables-3/#table-cell-content-relayout>
            if cyclic_percentage_scroll_container {
                break;
            }
            if let Some((box_style, box_children)) = match child_box {
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    Some((&box_.style, &box_.children))
                }
                box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                    Some((&box_.core.style, &box_.core.children))
                }
                _ => None,
            } {
                if has_non_inline_formatting_box(box_children) {
                    block_flow.push_zero_margin_child_height(
                        self.table_cell_children_non_text_content_height(
                            box_children,
                            stylesheets,
                            content_width,
                        ),
                    );
                } else {
                    let text = inline_text_from_formatting_boxes(box_children);
                    if !text.is_empty() || has_atomic_inline_formatting_box(box_children) {
                        let inline_height = if has_atomic_inline_formatting_box(box_children) {
                            self.table_cell_inline_sequence_height(
                                box_style,
                                box_children,
                                stylesheets,
                                content_width,
                                PercentageBasis::indefinite(),
                            )
                            .map(TableCellOuterBlockSize::points)
                            .unwrap_or(box_style.line_height)
                        } else {
                            self.estimate_text_physical_height(
                                &text,
                                box_style,
                                content_width,
                                box_style.padding.left,
                                box_style.padding.right,
                            )
                        };
                        block_flow.push_atomic_height(inline_height);
                    }
                }
                continue;
            }
            let Some((child_element, _, child_style, child_children)) = child_box.element_parts()
            else {
                continue;
            };
            if child_style.display.is_block_level()
                || self
                    .document_canvas_overflow
                    .is_document_canvas_flow_element(child_element)
                || is_replaced_element(child_element)
            {
                if let Some(collapsed_margin) = table_cell_self_collapsing_block_margin(
                    child_box,
                    PercentageBasis::indefinite(),
                    self.document_canvas_overflow,
                ) {
                    block_flow.push_collapsed_margin(collapsed_margin.points());
                } else {
                    let child_height = self.table_cell_row_minimum_element_outer_height(
                        child_element,
                        child_style,
                        child_children,
                        stylesheets,
                        content_width,
                        child_box,
                    );
                    block_flow.push_child_height(child_box, child_height);
                }
            }
        }
        let mut content_height = block_flow.finish();

        if !has_auto_height(style)
            || used_min_height(style, PercentageBasis::definite(layout_pt(content_width))).is_some()
            || used_max_height(style, PercentageBasis::definite(layout_pt(content_width))).is_some()
        {
            let vertical_extras = box_metrics.vertical_non_content_length().points();
            let requested_content_height = used_content_box_height_or_auto(
                style,
                layout_pt(content_height),
                non_content_pt(vertical_extras),
            )
            .map(SemanticLengthExt::points)
            .unwrap_or(content_height);
            content_height = constrain_content_height(
                style,
                content_box_pt(requested_content_height),
                PercentageBasis::definite(layout_pt(content_width)),
            )
            .points();
        }

        let borders = used_border_widths(style);
        style.margin.top
            + borders.top
            + style.padding.top
            + content_height
            + style.padding.bottom
            + borders.bottom
            + style.margin.bottom
    }

    fn table_cell_children_final_relayout_height(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        content_pass: TableCellContentPass,
    ) -> f32 {
        let percentage_height_basis = content_pass.percentage_basis();
        let mut block_flow = TableCellBlockFlowHeight::default();
        let mut inline_line_height = 0.0_f32;
        let mut estimated_float_context = FloatContext { shapes: Vec::new() };
        let mut estimated_float_bottom = 0.0_f32;
        let has_block_flow_child = has_non_inline_formatting_box(children);

        for child in children {
            // A float-only inline wrapper can look like a phantom line, but
            // its margin-box bottom still contributes to table-cell height.
            // <https://www.w3.org/TR/CSS22/tables.html#height-layout>
            if has_block_flow_child && formatting_box_can_only_create_phantom_line_boxes(child) {
                continue;
            }
            if let box_tree::FormattingBox::Text(box_) = child {
                inline_line_height = inline_line_height.max(self.estimate_text_physical_height(
                    &box_.text,
                    &box_.style,
                    available_width,
                    0.0,
                    0.0,
                ));
                continue;
            }
            if table_cell_child_is_in_flow_float(child) {
                let Some((child_element, _, child_style, child_children)) = child.element_parts()
                else {
                    continue;
                };
                if let Some(float_bottom) = self.table_cell_float_bottom_for_content_pass(
                    &mut estimated_float_context,
                    child_element,
                    child_style,
                    stylesheets,
                    available_width,
                    Some(child_children),
                    percentage_height_basis,
                ) {
                    estimated_float_bottom = estimated_float_bottom.min(float_bottom);
                    continue;
                }
            }
            if let box_tree::FormattingBox::Inline(box_) = child {
                if matches!(
                    box_.core.style.position,
                    Position::Absolute | Position::Fixed
                ) {
                    continue;
                }
                inline_line_height = inline_line_height.max(
                    self.table_cell_children_text_content_height(
                        &box_.core.children,
                        available_width,
                    )
                    .max(box_.core.style.line_height),
                );
                continue;
            }
            if let Some(inline_height) = self.table_cell_measured_inline_outer_height_with_basis(
                child,
                stylesheets,
                available_width,
                content_pass,
            ) {
                inline_line_height = inline_line_height.max(inline_height.points());
                continue;
            }
            if inline_line_height > 0.0 {
                block_flow.push_atomic_height(inline_line_height);
                inline_line_height = 0.0;
            }
            if let Some(collapsed_margin) = table_cell_self_collapsing_block_margin(
                child,
                percentage_height_basis,
                self.document_canvas_overflow,
            ) {
                block_flow.push_collapsed_margin(collapsed_margin.points());
                continue;
            }
            let child_height = self.table_cell_measured_block_child_final_relayout_height(
                child,
                stylesheets,
                available_width,
                percentage_height_basis,
            );
            block_flow.push_child_height(child, child_height);
        }

        if inline_line_height > 0.0 {
            block_flow.push_atomic_height(inline_line_height);
        }

        block_flow.finish().max(-estimated_float_bottom)
    }

    /// Measure a table-cell float for one table-content sizing pass.
    ///
    /// CSS Tables uses the same float margin-box contribution for row-minimum
    /// sizing and definite cell relayout; only the descendant percentage basis
    /// changes. Sharing this boundary prevents cell alignment from observing a
    /// different subject size than row sizing.
    /// <https://drafts.csswg.org/css-tables-3/#row-layout>
    /// <https://drafts.csswg.org/css-tables-3/#table-cell-content-relayout>
    #[allow(clippy::too_many_arguments)]
    fn table_cell_float_bottom_for_content_pass(
        &mut self,
        float_context: &mut FloatContext,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> Option<f32> {
        self.block_percentage_context_stack
            .push_percentage_basis(percentage_height_basis);
        let bottom = self.estimate_child_float_bottom(
            float_context,
            element,
            style,
            stylesheets,
            available_width,
            child_boxes,
        );
        self.block_percentage_context_stack.pop();
        bottom
    }

    fn table_cell_measured_block_child_final_relayout_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> f32 {
        match child {
            box_tree::FormattingBox::Table(box_) => {
                if matches!(
                    box_.core.style.position,
                    Position::Absolute | Position::Fixed
                ) {
                    return 0.0;
                }
                self.estimate_table_height(
                    box_.core.element,
                    &box_.core.style,
                    stylesheets,
                    available_width,
                    &box_.fragment,
                )
            }
            box_tree::FormattingBox::Block(box_) => self
                .table_cell_final_relayout_element_outer_height(
                    box_.core.element,
                    &box_.core.style,
                    &box_.core.children,
                    stylesheets,
                    available_width,
                    percentage_height_basis,
                ),
            box_tree::FormattingBox::Flex(box_) => self
                .table_cell_final_relayout_element_outer_height(
                    box_.core.element,
                    &box_.core.style,
                    &box_.core.children,
                    stylesheets,
                    available_width,
                    percentage_height_basis,
                ),
            box_tree::FormattingBox::AnonymousBlock(box_)
                if !has_non_inline_formatting_box(&box_.children) =>
            {
                self.table_cell_inline_sequence_height(
                    &box_.style,
                    &box_.children,
                    stylesheets,
                    available_width,
                    percentage_height_basis,
                )
                .map(TableCellOuterBlockSize::points)
                .unwrap_or(0.0)
            }
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .table_cell_children_final_relayout_height(
                    &box_.children,
                    stylesheets,
                    available_width,
                    TableCellContentPass::RowMinimum,
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .table_cell_children_final_relayout_height(
                    &box_.core.children,
                    stylesheets,
                    available_width,
                    TableCellContentPass::RowMinimum,
                ),
            _ => table_cell_formatting_child_outer_height(child).points(),
        }
    }

    fn table_cell_final_relayout_element_outer_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_outer_width: f32,
        parent_percentage_height_basis: BlockSizePercentageBasis,
    ) -> f32 {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return 0.0;
        }
        if self
            .document_canvas_overflow
            .is_document_canvas_flow_element(element)
        {
            return self.table_cell_measured_document_canvas_child_height(
                style,
                children,
                stylesheets,
                available_outer_width,
            );
        }

        let mut used_style = self
            .table_cell_content_sizing_style(style, TableCellContentSizingPolicy::FinalRelayout);
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_outer_width.max(0.0))),
        );
        let style = &used_style;
        // The computed height remains necessary for final table-cell sizing,
        // but CSS Sizing values that behave as `auto` use an auto proxy for
        // this self-collapsing predicate.
        // <https://drafts.csswg.org/css-sizing-3/#behave-auto>
        let mut margin_collapse_style = None;
        if height_behaves_as_auto_for_margin_collapse(style, parent_percentage_height_basis) {
            let mut style_for_margin_collapse = style.clone();
            style_for_margin_collapse
                .box_values
                .height
                .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
            margin_collapse_style = Some(style_for_margin_collapse);
        }
        let margin_collapse_style = margin_collapse_style.as_ref().unwrap_or(style);
        if is_self_collapsing_block_box(
            element,
            margin_collapse_style,
            children,
            self.document_canvas_overflow,
        ) {
            let descendant_start_margin = collapsible_first_child_start_margin_from_boxes(
                children,
                element,
                style,
                self.document_canvas_overflow,
            );
            return self_collapsing_block_margin_set_for_box(style, descendant_start_margin)
                .collapsed()
                .points()
                .max(0.0);
        }
        let horizontal_extras = box_metrics.horizontal_non_content_length().points();
        let requested_content_width = if matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        ) {
            let (min_content, max_content) = self.block_intrinsic_content_widths(
                element,
                style,
                stylesheets,
                Some(children),
                available_outer_width,
            );
            intrinsic::content_box_width_from_intrinsic(
                style,
                layout_pt(available_outer_width),
                non_content_pt(horizontal_extras),
                content_box_pt(min_content),
                content_box_pt(max_content),
                intrinsic::IntrinsicAutoWidth::FillAvailable,
            )
            .points()
        } else {
            used_content_box_width(
                style,
                layout_pt(available_outer_width),
                non_content_pt(horizontal_extras),
            )
            .points()
        };
        let content_width = constrain_content_width(
            style,
            content_box_pt(requested_content_width),
            PercentageBasis::definite(layout_pt(available_outer_width)),
        )
        .points()
        .max(style.font_size);
        let vertical_extras = box_metrics.vertical_non_content_length().points();
        let specified_content_height = used_content_box_height_or_auto_with_basis(
            style,
            parent_percentage_height_basis,
            non_content_pt(vertical_extras),
        )
        .map(SemanticLengthExt::points);

        let mut block_flow = TableCellBlockFlowHeight::default();
        let mut inline_line_height = 0.0_f32;
        if !has_non_inline_formatting_box(children)
            && (has_direct_inline_content_box(children)
                || has_atomic_inline_formatting_box(children))
        {
            if has_atomic_inline_formatting_box(children) {
                if let Some(inline_height) = self.table_cell_inline_sequence_height(
                    style,
                    children,
                    stylesheets,
                    content_width,
                    parent_percentage_height_basis,
                ) {
                    block_flow.push_atomic_height(inline_height.points());
                }
            } else {
                let text = inline_text_from_formatting_boxes(children);
                if !text.is_empty() {
                    block_flow.push_atomic_height(self.estimate_text_physical_height(
                        &text,
                        style,
                        content_width,
                        style.padding.left,
                        style.padding.right,
                    ));
                }
            }
        }

        for child_box in children {
            if has_non_inline_formatting_box(children) {
                match child_box {
                    box_tree::FormattingBox::Text(box_) => {
                        inline_line_height =
                            inline_line_height.max(self.estimate_text_physical_height(
                                &box_.text,
                                &box_.style,
                                content_width,
                                0.0,
                                0.0,
                            ));
                        continue;
                    }
                    box_tree::FormattingBox::Inline(box_)
                        if !matches!(
                            box_.core.style.position,
                            Position::Absolute | Position::Fixed
                        ) =>
                    {
                        inline_line_height = inline_line_height.max(
                            self.table_cell_children_text_content_height(
                                &box_.core.children,
                                content_width,
                            )
                            .max(box_.core.style.line_height),
                        );
                        continue;
                    }
                    _ => {}
                }
            }
            if inline_line_height > 0.0 {
                block_flow.push_atomic_height(inline_line_height);
                inline_line_height = 0.0;
            }
            if let Some((box_style, box_children)) = match child_box {
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    Some((&box_.style, &box_.children))
                }
                box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                    Some((&box_.core.style, &box_.core.children))
                }
                _ => None,
            } {
                if has_non_inline_formatting_box(box_children) {
                    block_flow.push_zero_margin_child_height(
                        self.table_cell_children_final_relayout_height(
                            box_children,
                            stylesheets,
                            content_width,
                            table_cell_content_pass_from_committed_basis(
                                block_size_percentage_basis_from_points(
                                    specified_content_height,
                                    BlockSizeBasisSource::TableCell,
                                ),
                            ),
                        ),
                    );
                } else {
                    let text = inline_text_from_formatting_boxes(box_children);
                    if !text.is_empty() || has_atomic_inline_formatting_box(box_children) {
                        let inline_height = if has_atomic_inline_formatting_box(box_children) {
                            self.table_cell_inline_sequence_height(
                                box_style,
                                box_children,
                                stylesheets,
                                content_width,
                                block_size_percentage_basis_from_points(
                                    specified_content_height,
                                    BlockSizeBasisSource::TableCell,
                                ),
                            )
                            .map(TableCellOuterBlockSize::points)
                            .unwrap_or(box_style.line_height)
                        } else {
                            self.estimate_text_physical_height(
                                &text,
                                box_style,
                                content_width,
                                box_style.padding.left,
                                box_style.padding.right,
                            )
                        };
                        block_flow.push_atomic_height(inline_height);
                    }
                }
                continue;
            }
            let Some((child_element, _, child_style, child_children)) = child_box.element_parts()
            else {
                continue;
            };
            if child_style.display.is_block_level()
                || self
                    .document_canvas_overflow
                    .is_document_canvas_flow_element(child_element)
                || is_replaced_element(child_element)
            {
                let child_percentage_height_basis = block_size_percentage_basis_from_points(
                    specified_content_height,
                    BlockSizeBasisSource::TableCell,
                );
                if let Some(collapsed_margin) = table_cell_self_collapsing_block_margin(
                    child_box,
                    child_percentage_height_basis,
                    self.document_canvas_overflow,
                ) {
                    block_flow.push_collapsed_margin(collapsed_margin.points());
                } else {
                    let child_height = self.table_cell_final_relayout_element_outer_height(
                        child_element,
                        child_style,
                        child_children,
                        stylesheets,
                        content_width,
                        child_percentage_height_basis,
                    );
                    block_flow.push_child_height(child_box, child_height);
                }
            }
        }
        if inline_line_height > 0.0 {
            block_flow.push_atomic_height(inline_line_height);
        }
        let mut content_height = block_flow.finish();

        if !has_auto_height(style)
            || table_cell_used_min_height(style, parent_percentage_height_basis).is_some()
            || table_cell_used_max_height(style, parent_percentage_height_basis).is_some()
        {
            let requested_content_height = specified_content_height.unwrap_or(content_height);
            content_height = constrain(
                requested_content_height,
                table_cell_used_min_height(style, parent_percentage_height_basis)
                    .map(ContentBoxLength::points),
                table_cell_used_max_height(style, parent_percentage_height_basis)
                    .map(ContentBoxLength::points),
            );
        }

        let borders = used_border_widths(style);
        style.margin.top
            + borders.top
            + style.padding.top
            + content_height
            + style.padding.bottom
            + borders.bottom
            + style.margin.bottom
    }
}

/// Replace a collapsed table cell's authored border metrics with its resolved
/// grid-edge metrics for subsequent content layout.
///
/// Collapsed-border conflict resolution chooses one border per shared grid
/// edge; losing `none` and 3D candidates must not retain box-model space while
/// cell content is sized and positioned.  The stored widths remain full edge
/// widths because ordinary used-border helpers apply the collapsed half-width
/// projection at their layout boundary:
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
fn apply_resolved_collapsed_border_layout_edges(style: &mut ComputedStyle, insets: css::Edges) {
    let full = css::Edges {
        top: insets.top * 2.0,
        right: insets.right * 2.0,
        bottom: insets.bottom * 2.0,
        left: insets.left * 2.0,
    };
    style.border_widths = full;
    style.border_styles = css::BorderStyles {
        top: resolved_collapsed_border_layout_style(full.top),
        right: resolved_collapsed_border_layout_style(full.right),
        bottom: resolved_collapsed_border_layout_style(full.bottom),
        left: resolved_collapsed_border_layout_style(full.left),
    };
}

fn resolved_collapsed_border_layout_style(width: f32) -> BorderStyle {
    if width > 0.0 {
        BorderStyle::Solid
    } else {
        BorderStyle::None
    }
}

fn table_cell_used_min_height(
    style: &ComputedStyle,
    percentage_basis: BlockSizePercentageBasis,
) -> Option<ContentBoxLength> {
    used_length_percentage_or_auto_with_basis(style.box_values.min_height.clone(), percentage_basis)
        .map(|value| content_box_pt(value.points().max(0.0)))
}

fn table_cell_used_max_height(
    style: &ComputedStyle,
    percentage_basis: BlockSizePercentageBasis,
) -> Option<ContentBoxLength> {
    used_length_percentage_or_auto_with_basis(style.box_values.max_height.clone(), percentage_basis)
        .map(|value| content_box_pt(value.points().max(0.0)))
}

#[derive(Default)]
struct TableCellBlockFlowHeight {
    height: f32,
    pending_margin: Option<LayoutLength>,
}

impl TableCellBlockFlowHeight {
    fn push_atomic_height(&mut self, height: f32) {
        self.flush_pending_margin();
        self.height += height;
    }

    fn push_zero_margin_child_height(&mut self, height: f32) {
        self.push_child(height, 0.0, 0.0);
    }

    /// Stack one normal-flow block child in a table-cell block formatting context.
    ///
    /// CSS table cells establish block formatting contexts, so child margins do
    /// not collapse through the cell itself. Adjoining margins inside the cell
    /// still collapse between normal-flow block siblings and through
    /// self-collapsing blocks:
    /// <https://www.w3.org/TR/CSS22/tables.html#model>,
    /// <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>, and
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
    fn push_child_height(&mut self, child: &box_tree::FormattingBox<'_>, outer_height: f32) {
        if let Some((margin_top, margin_bottom)) = table_cell_sibling_collapsible_margins(child) {
            self.push_child(outer_height, margin_top, margin_bottom);
        } else {
            self.push_atomic_height(outer_height);
        }
    }

    fn push_collapsed_margin(&mut self, margin: f32) {
        self.pending_margin = Some(
            self.pending_margin
                .map(|pending| collapse_margins(pending, layout_pt(margin)))
                .unwrap_or_else(|| layout_pt(margin)),
        );
    }

    fn push_child(&mut self, outer_height: f32, margin_top: f32, margin_bottom: f32) {
        let body_height = (outer_height - margin_top - margin_bottom).max(0.0);
        if let Some(pending) = self.pending_margin.take() {
            self.height += collapse_margins(pending, layout_pt(margin_top)).points();
        } else {
            self.height += margin_top;
        }
        self.height += body_height;
        self.pending_margin = Some(layout_pt(margin_bottom));
    }

    fn flush_pending_margin(&mut self) {
        if let Some(margin) = self.pending_margin.take() {
            self.height += margin.points();
        }
    }

    fn finish(mut self) -> f32 {
        self.flush_pending_margin();
        self.height.max(0.0)
    }
}

fn table_cell_sibling_collapsible_margins(
    child: &box_tree::FormattingBox<'_>,
) -> Option<(f32, f32)> {
    let (element, _, style, _) = child.element_parts()?;
    outer_margins_adjoin_block_siblings(element, style)
        .then_some((style.margin.top, style.margin.bottom))
}

fn table_cell_self_collapsing_block_margin(
    child: &box_tree::FormattingBox<'_>,
    percentage_height_basis: BlockSizePercentageBasis,
    overflow_context: DocumentCanvasResolution,
) -> Option<LayoutLength> {
    let (element, _, style, children) = child.element_parts()?;
    let mut margin_collapse_style = None;
    if height_behaves_as_auto_for_margin_collapse(style, percentage_height_basis) {
        let mut style_for_margin_collapse = style.clone();
        style_for_margin_collapse
            .box_values
            .height
            .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
        margin_collapse_style = Some(style_for_margin_collapse);
    }
    let margin_collapse_style = margin_collapse_style.as_ref().unwrap_or(style);
    if !is_self_collapsing_block_box(element, margin_collapse_style, children, overflow_context) {
        return None;
    }
    let descendant_start_margin =
        collapsible_first_child_start_margin_from_boxes(children, element, style, overflow_context);
    Some(self_collapsing_block_margin_set_for_box(style, descendant_start_margin).collapsed())
}
