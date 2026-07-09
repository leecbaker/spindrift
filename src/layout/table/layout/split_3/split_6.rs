use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::table) fn resolve_table_target_content_height(
        &self,
        table_style: &ComputedStyle,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        wrapper_border_box_block_size: Option<BorderBoxLength>,
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
            });
        if let Some(wrapper_border_box_block_size) = wrapper_border_box_block_size {
            return Some(content_box_pt(
                (wrapper_border_box_block_size.points()
                    - vertical_non_content.points()
                    - wrapper_non_grid_block_size.points())
                .max(0.0),
            ));
        }
        used_table_target_content_height(
            table_style,
            self.definite_block_size_stack
                .last()
                .cloned()
                .unwrap_or_else(PercentageBasis::indefinite),
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
                let required_height = if context.table_style.writing_mode.has_vertical_lines() {
                    // The table root's block track is physical width in a
                    // vertical writing mode. Reusing the physical-height
                    // cell metric here would grow the row during the
                    // reference pass after its logical block size was
                    // correctly established in the base pass.
                    // <https://drafts.csswg.org/css-tables-3/#row-layout>
                    // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
                    table_cell_content_max_width(
                        self,
                        cell,
                        &prepared.style,
                        context.stylesheets,
                        Some(prepared.borders),
                    )
                } else {
                    self.table_cell_border_box_height_with_basis(
                        &prepared.row_sizing_style,
                        prepared.metrics.content_height,
                        target_content_height_basis,
                        prepared.borders,
                    )
                };
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
        target_content_height: Option<ContentBoxLength>,
        table_metrics: TableMetrics,
    ) {
        for row in rows.iter_mut() {
            row.final_height = row.base;
        }
        let Some(target_content_height) = target_content_height else {
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
        stylesheets: &[Stylesheet],
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
    ) -> bool {
        if table_metrics.border_collapse == css::BorderCollapse::Collapse || placements.is_empty() {
            return false;
        }

        let mut saw_visible_column_cell = false;
        for placement in placements {
            let cell_width = column_plan.width_for_span(placement.column, placement.colspan);
            if cell_width <= 0.0 {
                continue;
            }
            saw_visible_column_cell = true;
            let cell = &row.cells[placement.cell];
            let mut cell_style = self.style_for_table_cell(cell, row, row_style, stylesheets);
            apply_table_cell_used_padding(
                &mut cell_style,
                table_cellpadding,
                PercentageBasis::definite(layout_pt(cell_width)),
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
        stylesheets: &[Stylesheet],
        table_width: f32,
        side: CaptionSide,
    ) -> f32 {
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
                    if caption_style.contain.size {
                        let mut used_style =
                            self.style_with_current_viewport_lengths(&caption_style);
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
        stylesheets: &[Stylesheet],
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> Option<f32> {
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
            })
            .reduce(f32::max)
    }

    pub(in crate::layout::table) fn table_cell_row_baseline_offset_for_alignment(
        &mut self,
        context: &TableCellBaselineAlignmentContext<'_>,
        placement: &TableCellPlacement,
        cell_style: &ComputedStyle,
    ) -> Option<f32> {
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
        Some((origin_top - target_top).max(0.0) + target_baseline)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_row_baseline_only_offset(
        &mut self,
        row_index: usize,
        row: &TableRow<'_>,
        placements: &[TableCellPlacement],
        row_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> Option<f32> {
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
            .reduce(f32::max)
    }

    pub(in crate::layout::table) fn table_cell_physical_y_row_baseline_candidate(
        &mut self,
        cell: &TableCell<'_>,
        prepared: &PreparedTableCell,
        stylesheets: &[Stylesheet],
    ) -> Option<f32> {
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
        stylesheets: &[Stylesheet],
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> Option<PreparedTableCell> {
        let mut style = self.style_for_table_cell(cell, row, row_style, stylesheets);
        let area = TableGridArea::from_placement(row_index, placement);
        let inline_bounds = column_plan.inline_bounds_for_area(area);
        let width = inline_bounds.size.max(0.0);
        // A table cell's percentage padding resolves against the final table
        // grid inline size, including only the border spacing between tracks.
        // It must not use the cell span width, which itself includes the
        // padding being resolved.
        // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
        let table_grid_inline_size = column_plan
            .width_for_span(0, column_plan.column_count())
            .max(0.0);
        apply_table_cell_used_padding(
            &mut style,
            table_cellpadding,
            PercentageBasis::definite(layout_pt(table_grid_inline_size)),
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
            style: style.as_computed().clone(),
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
        stylesheets: &[Stylesheet],
        cell_width: f32,
        border_insets: css::Edges,
    ) -> TableCellLayoutMetrics {
        let available_width = (cell_width
            - cell_style.padding.left
            - cell_style.padding.right
            - border_insets.left
            - border_insets.right)
            .max(0.0);
        let text_height = self.table_cell_text_content_height(cell, cell_style, available_width);
        let explicit_height_basis =
            table_cell_explicit_content_height_basis(cell_style, border_insets);
        let non_text_height = if explicit_height_basis.is_definite() {
            cell.children
                .as_deref()
                .map(|children| {
                    table_cell_replaced_content_height(cell).points().max(
                        self.table_cell_children_final_relayout_height(
                            children,
                            stylesheets,
                            available_width,
                            explicit_height_basis,
                        ),
                    )
                })
                .unwrap_or_else(|| table_cell_non_text_content_height(cell).points())
        } else {
            self.table_cell_non_text_content_height(cell, stylesheets, available_width)
        };
        let content_height = text_height.max(non_text_height);
        let border_box_height = table_cell_border_box_height_with_insets(
            row_sizing_style,
            content_height,
            border_insets,
        );
        let baseline_offset = self
            .table_cell_alignment_baseline_offset(
                cell,
                cell_style,
                stylesheets,
                available_width,
                border_insets,
            )
            .unwrap_or_else(|| {
                self.table_cell_content_bottom_baseline(cell_style, content_height, border_insets)
            });

        TableCellLayoutMetrics {
            content_height,
            border_box_height,
            baseline_offset,
        }
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
        stylesheets: &[Stylesheet],
        cell_width: f32,
        border_insets: css::Edges,
        first_pass: TableCellLayoutMetrics,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> TableCellLayoutMetrics {
        if !percentage_height_basis.is_definite() {
            return first_pass;
        }

        let available_width = (cell_width
            - cell_style.padding.left
            - cell_style.padding.right
            - border_insets.left
            - border_insets.right)
            .max(0.0);
        let text_height = self.table_cell_text_content_height(cell, cell_style, available_width);
        let non_text_height = cell
            .children
            .as_deref()
            .map(|children| {
                table_cell_replaced_content_height(cell).points().max(
                    self.table_cell_children_final_relayout_height(
                        children,
                        stylesheets,
                        available_width,
                        percentage_height_basis,
                    ),
                )
            })
            .unwrap_or_else(|| table_cell_non_text_content_height(cell).points());
        let content_height = text_height.max(non_text_height);
        let baseline_offset = if text_height > 0.0 && text_height >= non_text_height {
            self.table_cell_alignment_baseline_offset(
                cell,
                cell_style,
                stylesheets,
                available_width,
                border_insets,
            )
            .unwrap_or_else(|| {
                self.table_cell_content_bottom_baseline(cell_style, content_height, border_insets)
            })
        } else {
            self.table_cell_content_bottom_baseline(cell_style, content_height, border_insets)
        };

        TableCellLayoutMetrics {
            content_height,
            border_box_height: first_pass.border_box_height,
            baseline_offset,
        }
    }

    /// Resolve table-cell row-track constraints in the table row flow.
    ///
    /// A cell can establish a distinct writing mode for its contents. When
    /// that flow is orthogonal to the row, its physical `height` is not a
    /// physical row-track constraint: the table's column inline size supplies
    /// that orthogonal used size instead. The row-sizing surrogate preserves
    /// the original cell style for content layout, alignment, and paint.
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
            if !cell_style.box_values.height.is_auto() {
                set_style_used_height(&mut style, column_inline_size.max(0.0));
            } else {
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
        stylesheets: &[Stylesheet],
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
        stylesheets: &[Stylesheet],
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
                if let Some(float_bottom) = self.estimate_child_float_bottom(
                    &mut estimated_float_context,
                    child_element,
                    child_style,
                    stylesheets,
                    available_width,
                    Some(child_children),
                ) {
                    estimated_float_bottom = estimated_float_bottom.min(float_bottom);
                    continue;
                }
            }
            if let Some(inline_height) =
                self.table_cell_measured_inline_outer_height(child, stylesheets, available_width)
            {
                inline_line_height = inline_line_height.max(inline_height);
                continue;
            }
            if inline_line_height > 0.0 {
                block_flow.push_atomic_height(inline_line_height);
                inline_line_height = 0.0;
            }
            if let Some(collapsed_margin) =
                table_cell_self_collapsing_block_margin(child, self.document_canvas_overflow)
            {
                block_flow.push_collapsed_margin(collapsed_margin);
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
        stylesheets: &[Stylesheet],
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> Option<f32> {
        self.table_cell_nested_inline_sequence_for_children(
            style,
            children,
            stylesheets,
            None,
            available_width,
            percentage_height_basis,
        )
        .map(|plan| plan.sequence.total_height())
    }

    pub(in crate::layout::table) fn table_cell_measured_block_child_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        let has_parent_percentage =
            table_cell_formatting_child_has_parent_percentage_block_size(child);
        match child {
            box_tree::FormattingBox::Table(box_) => {
                if matches!(box_.style.position, Position::Absolute | Position::Fixed) {
                    return 0.0;
                }
                if !has_parent_percentage {
                    return self.estimate_table_height(
                        box_.element,
                        &box_.style,
                        stylesheets,
                        available_width,
                        &box_.fragment,
                    );
                }
                let style = self.table_cell_content_sizing_style(
                    &box_.style,
                    TableCellContentSizingPolicy::RowMinimum,
                );
                self.estimate_table_height(
                    box_.element,
                    &style,
                    stylesheets,
                    available_width,
                    &box_.fragment,
                )
            }
            box_tree::FormattingBox::Block(box_) => {
                if is_document_canvas_element(box_.element) {
                    return self.table_cell_measured_element_child_height(
                        box_.element,
                        &box_.style,
                        &box_.children,
                        stylesheets,
                        available_width,
                        child,
                    );
                }
                self.table_cell_row_minimum_element_outer_height(
                    box_.element,
                    &box_.style,
                    &box_.children,
                    stylesheets,
                    available_width,
                    child,
                )
            }
            box_tree::FormattingBox::Flex(box_) => self.table_cell_measured_element_child_height(
                box_.element,
                &box_.style,
                &box_.children,
                stylesheets,
                available_width,
                child,
            ),
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .table_cell_children_non_text_content_height(
                    &box_.children,
                    stylesheets,
                    available_width,
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .table_cell_children_non_text_content_height(
                    &box_.children,
                    stylesheets,
                    available_width,
                ),
            _ => table_cell_formatting_child_outer_height(child).points(),
        }
    }

    pub(in crate::layout::table) fn table_cell_final_relayout_child_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
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
        stylesheets: &[Stylesheet],
        available_width: f32,
        fallback_child: &box_tree::FormattingBox<'_>,
    ) -> f32 {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return 0.0;
        }
        if is_document_canvas_element(element) {
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
        stylesheets: &[Stylesheet],
        available_width: f32,
        fallback_child: &box_tree::FormattingBox<'_>,
    ) -> f32 {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return 0.0;
        }

        let cyclic_percentage_scroll_container =
            table_cell_block_size_depends_on_parent_percentage(style.box_values.height.clone())
                && style.box_values.min_height.length_if_no_percent().is_some()
                && matches!(
                    effective_overflow_for_style(style),
                    css::Overflow::Auto | css::Overflow::Scroll
                );
        let style =
            self.table_cell_content_sizing_style(style, TableCellContentSizingPolicy::RowMinimum);
        match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => {
                table_cell_canvas_first_pass_outer_height(element, &style, available_width)
            }
            Some(ReplacedElementKind::Image) => {
                self.estimate_image_height(element, &style, available_width)
            }
            Some(ReplacedElementKind::Svg) => estimate_svg_height(element, &style, available_width),
            None if style.display.is_table() || is_html_table_element(element) => self
                .estimate_element_height(
                    element,
                    &style,
                    stylesheets,
                    available_width,
                    Some(children),
                )
                .unwrap_or_else(|| {
                    table_cell_formatting_child_outer_height(fallback_child).points()
                }),
            None => self.table_cell_row_minimum_block_like_outer_height(
                element,
                &style,
                children,
                stylesheets,
                available_width,
                cyclic_percentage_scroll_container,
            ),
        }
    }

    pub(in crate::layout::table) fn table_cell_row_minimum_block_like_outer_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        cyclic_percentage_scroll_container: bool,
    ) -> f32 {
        let mut used_style = style.clone();
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_outer_width.max(0.0))),
        );
        let style = &used_style;
        if is_self_collapsing_block_box(element, style, children, self.document_canvas_overflow) {
            let descendant_start_margin = collapsible_first_child_start_margin_from_boxes(
                children,
                element,
                style,
                self.document_canvas_overflow,
            );
            return self_collapsing_block_margin_set_for_box(style, descendant_start_margin)
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
                    block_flow.push_atomic_height(inline_height);
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
                    Some((&box_.style, &box_.children))
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
                || is_document_canvas_element(child_element)
                || is_replaced_element(child_element)
            {
                if let Some(collapsed_margin) = table_cell_self_collapsing_block_margin(
                    child_box,
                    self.document_canvas_overflow,
                ) {
                    block_flow.push_collapsed_margin(collapsed_margin);
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
        stylesheets: &[Stylesheet],
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> f32 {
        let mut block_flow = TableCellBlockFlowHeight::default();
        let mut inline_line_height = 0.0_f32;

        for child in children {
            if formatting_box_can_only_create_phantom_line_boxes(child) {
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
            if let Some(inline_height) = self.table_cell_measured_inline_outer_height_with_basis(
                child,
                stylesheets,
                available_width,
                percentage_height_basis,
            ) {
                inline_line_height = inline_line_height.max(inline_height);
                continue;
            }
            if inline_line_height > 0.0 {
                block_flow.push_atomic_height(inline_line_height);
                inline_line_height = 0.0;
            }
            if let Some(collapsed_margin) =
                table_cell_self_collapsing_block_margin(child, self.document_canvas_overflow)
            {
                block_flow.push_collapsed_margin(collapsed_margin);
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

        block_flow.finish()
    }

    fn table_cell_measured_block_child_final_relayout_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
        percentage_height_basis: BlockSizePercentageBasis,
    ) -> f32 {
        match child {
            box_tree::FormattingBox::Table(box_) => {
                if matches!(box_.style.position, Position::Absolute | Position::Fixed) {
                    return 0.0;
                }
                self.estimate_table_height(
                    box_.element,
                    &box_.style,
                    stylesheets,
                    available_width,
                    &box_.fragment,
                )
            }
            box_tree::FormattingBox::Block(box_) => self
                .table_cell_final_relayout_element_outer_height(
                    box_.element,
                    &box_.style,
                    &box_.children,
                    stylesheets,
                    available_width,
                    percentage_height_basis,
                ),
            box_tree::FormattingBox::Flex(box_) => self
                .table_cell_final_relayout_element_outer_height(
                    box_.element,
                    &box_.style,
                    &box_.children,
                    stylesheets,
                    available_width,
                    percentage_height_basis,
                ),
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .table_cell_children_final_relayout_height(
                    &box_.children,
                    stylesheets,
                    available_width,
                    PercentageBasis::indefinite(),
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .table_cell_children_final_relayout_height(
                    &box_.children,
                    stylesheets,
                    available_width,
                    PercentageBasis::indefinite(),
                ),
            _ => table_cell_formatting_child_outer_height(child).points(),
        }
    }

    fn table_cell_final_relayout_element_outer_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        parent_percentage_height_basis: BlockSizePercentageBasis,
    ) -> f32 {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return 0.0;
        }
        if is_document_canvas_element(element) {
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
        if is_self_collapsing_block_box(element, style, children, self.document_canvas_overflow) {
            let descendant_start_margin = collapsible_first_child_start_margin_from_boxes(
                children,
                element,
                style,
                self.document_canvas_overflow,
            );
            return self_collapsing_block_margin_set_for_box(style, descendant_start_margin)
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
                    block_flow.push_atomic_height(inline_height);
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
            if let Some((box_style, box_children)) = match child_box {
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    Some((&box_.style, &box_.children))
                }
                box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                    Some((&box_.style, &box_.children))
                }
                _ => None,
            } {
                if has_non_inline_formatting_box(box_children) {
                    block_flow.push_zero_margin_child_height(
                        self.table_cell_children_final_relayout_height(
                            box_children,
                            stylesheets,
                            content_width,
                            block_size_percentage_basis_from_points(
                                specified_content_height,
                                BlockSizeBasisSource::TableCell,
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
                || is_document_canvas_element(child_element)
                || is_replaced_element(child_element)
            {
                if let Some(collapsed_margin) = table_cell_self_collapsing_block_margin(
                    child_box,
                    self.document_canvas_overflow,
                ) {
                    block_flow.push_collapsed_margin(collapsed_margin);
                } else {
                    let child_height = self.table_cell_final_relayout_element_outer_height(
                        child_element,
                        child_style,
                        child_children,
                        stylesheets,
                        content_width,
                        block_size_percentage_basis_from_points(
                            specified_content_height,
                            BlockSizeBasisSource::TableCell,
                        ),
                    );
                    block_flow.push_child_height(child_box, child_height);
                }
            }
        }
        let mut content_height = block_flow.finish();

        if !has_auto_height(style)
            || table_cell_used_min_height(style, parent_percentage_height_basis).is_some()
            || table_cell_used_max_height(style, parent_percentage_height_basis).is_some()
        {
            let requested_content_height = specified_content_height.unwrap_or(content_height);
            content_height = constrain(
                requested_content_height,
                table_cell_used_min_height(style, parent_percentage_height_basis),
                table_cell_used_max_height(style, parent_percentage_height_basis),
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
) -> Option<f32> {
    used_length_percentage_or_auto_with_basis(style.box_values.min_height.clone(), percentage_basis)
        .map(|value| value.points().max(0.0))
}

fn table_cell_used_max_height(
    style: &ComputedStyle,
    percentage_basis: BlockSizePercentageBasis,
) -> Option<f32> {
    used_length_percentage_or_auto_with_basis(style.box_values.max_height.clone(), percentage_basis)
        .map(|value| value.points().max(0.0))
}

#[derive(Default)]
struct TableCellBlockFlowHeight {
    height: f32,
    pending_margin: Option<f32>,
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
                .map(|pending| collapse_margins(layout_pt(pending), layout_pt(margin)).points())
                .unwrap_or(margin),
        );
    }

    fn push_child(&mut self, outer_height: f32, margin_top: f32, margin_bottom: f32) {
        let body_height = (outer_height - margin_top - margin_bottom).max(0.0);
        if let Some(pending) = self.pending_margin.take() {
            self.height += collapse_margins(layout_pt(pending), layout_pt(margin_top)).points();
        } else {
            self.height += margin_top;
        }
        self.height += body_height;
        self.pending_margin = Some(margin_bottom);
    }

    fn flush_pending_margin(&mut self) {
        if let Some(margin) = self.pending_margin.take() {
            self.height += margin;
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
    is_sibling_margin_collapsible_block_child(element, style)
        .then_some((style.margin.top, style.margin.bottom))
}

fn table_cell_self_collapsing_block_margin(
    child: &box_tree::FormattingBox<'_>,
    overflow_context: DocumentCanvasOverflowContext,
) -> Option<f32> {
    let (element, _, style, children) = child.element_parts()?;
    if !is_self_collapsing_block_box(element, style, children, overflow_context) {
        return None;
    }
    let descendant_start_margin =
        collapsible_first_child_start_margin_from_boxes(children, element, style, overflow_context);
    Some(self_collapsing_block_margin_set_for_box(
        style,
        descendant_start_margin,
    ))
}
