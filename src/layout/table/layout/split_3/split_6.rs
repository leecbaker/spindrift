use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::table) fn resolve_table_target_content_height(
        &self,
        table_style: &ComputedStyle,
    ) -> Option<f32> {
        let vertical_non_content = if table_style.border_collapse == css::BorderCollapse::Collapse {
            0.0
        } else {
            let border_widths = used_border_widths(table_style);
            table_style.padding.top
                + table_style.padding.bottom
                + border_widths.top
                + border_widths.bottom
        };
        used_table_target_content_height(
            table_style,
            self.definite_block_size_stack.last().copied().flatten(),
            vertical_non_content,
        )
    }

    pub(in crate::layout::table) fn compute_table_reference_heights(
        &mut self,
        plan_rows: &mut [TableRowHeightPlan],
        context: &TableGridLayoutContext<'_, '_>,
        target_content_height: Option<f32>,
    ) {
        if plan_rows.is_empty() {
            return;
        }

        for (row_index, row) in context.rows.iter().enumerate() {
            if plan_rows[row_index].collapsed {
                continue;
            }
            let row_style = self.style_for_table_row(row, context.table_style, context.stylesheets);
            if let Some(row_height) = used_length_percentage_or_auto_with_optional_basis(
                row_style.box_values.height,
                target_content_height,
            ) {
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
                    context.table_metrics,
                    context.collapsed_geometry,
                ) else {
                    continue;
                };
                let required_height = self.table_cell_border_box_height_with_basis(
                    &prepared.row_sizing_style,
                    prepared.metrics.content_height,
                    target_content_height,
                    context.column_plan.total_width(),
                    prepared.borders,
                );
                distribute_table_span_constraint(
                    plan_rows,
                    row_index,
                    placement.rowspan,
                    required_height,
                    context.table_metrics,
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
            let Some(group_height) = used_length_percentage_or_auto_with_optional_basis(
                row_group_style.box_values.height,
                target_content_height,
            ) else {
                continue;
            };
            distribute_table_span_constraint(
                plan_rows,
                start,
                end - start,
                group_height,
                context.table_metrics,
                TableHeightTarget::Reference,
            );
        }
    }

    pub(in crate::layout::table) fn table_cell_border_box_height_with_basis(
        &self,
        style: &ComputedStyle,
        content_height: f32,
        percentage_basis: Option<f32>,
        width_basis: f32,
        border_insets: css::Edges,
    ) -> f32 {
        let vertical_non_content =
            style.padding.top + style.padding.bottom + border_insets.top + border_insets.bottom;
        let requested_content = used_content_height_or_auto_with_optional_basis(
            style,
            percentage_basis,
            vertical_non_content,
        )
        .unwrap_or(0.0)
        .max(content_height);
        constrain_height(style, requested_content, width_basis) + vertical_non_content
    }

    pub(in crate::layout::table) fn distribute_table_height_plan(
        &self,
        rows: &mut [TableRowHeightPlan],
        target_content_height: Option<f32>,
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

        let base = table_content_height_from_plan(rows, TableHeightTarget::Base, table_metrics);
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
            apply_table_cell_used_padding(&mut cell_style, table_cellpadding, cell_width);
            let cell_is_empty = table_cell_inline_text(cell).is_empty()
                && self.table_cell_non_text_content_height(
                    cell,
                    stylesheets,
                    (cell_width
                        - cell_style.padding.left
                        - cell_style.padding.right
                        - table_horizontal_borders(&cell_style))
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
                    self.estimate_element_height(
                        caption.element,
                        &caption_style,
                        stylesheets,
                        table_width,
                        caption.children.as_deref(),
                    )
                    .unwrap_or(0.0)
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
        // baseline` by the baseline of their first in-flow line box. This
        // lightweight fragment model uses the first text baseline approximation
        // already used for PDF text placement.
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
                    table_metrics,
                    collapsed_geometry,
                )?;
                if !table_cell_participates_in_physical_y_row_baseline(
                    &prepared.style,
                    row_style,
                    placement,
                ) {
                    return None;
                }
                Some(prepared.metrics.baseline_offset)
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
            context.table_metrics,
            context.collapsed_geometry,
        )?;
        let origin_top = table_row_top(
            0.0,
            context.planned_row_heights,
            context.planned_row_occupancy,
            context.table_metrics,
            context.row_index,
        );
        let target_top = table_row_top(
            0.0,
            context.planned_row_heights,
            context.planned_row_occupancy,
            context.table_metrics,
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
                    table_metrics,
                    collapsed_geometry,
                )?;
                if !table_cell_participates_in_physical_y_row_baseline(
                    &prepared.style,
                    row_style,
                    placement,
                ) {
                    return None;
                }
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
            })
            .reduce(f32::max)
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
        apply_table_cell_used_padding(&mut style, table_cellpadding, width);
        let borders = table_cell_border_insets(
            &style,
            placement,
            row_index,
            table_metrics,
            collapsed_geometry,
        );
        let row_sizing_style = self.table_cell_row_sizing_style(&style, row_style);
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
            style,
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
        let content_height = text_height.max(self.table_cell_non_text_content_height(
            cell,
            stylesheets,
            available_width,
        ));
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
        percentage_height_basis: Option<f32>,
    ) -> TableCellLayoutMetrics {
        let Some(percentage_height_basis) = percentage_height_basis else {
            return first_pass;
        };

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
                table_cell_replaced_content_height(cell).max(
                    self.table_cell_children_final_relayout_height(
                        children,
                        stylesheets,
                        available_width,
                        Some(percentage_height_basis),
                    ),
                )
            })
            .unwrap_or_else(|| table_cell_non_text_content_height(cell));
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

    /// Resolve table-cell physical row-size constraints in the row axis.
    ///
    /// CSS Tables consumes `height`/`min-height`/`max-height` on cells as row
    /// sizing constraints. Those constraints use the cell's selected font, but
    /// their font-relative units must follow the row/table axis instead of an
    /// orthogonal cell content writing mode.
    pub(in crate::layout::table) fn table_cell_row_sizing_style(
        &mut self,
        cell_style: &ComputedStyle,
        row_style: &ComputedStyle,
    ) -> ComputedStyle {
        let mut style = cell_style.clone();
        style.writing_mode = row_style.writing_mode;
        style.text_orientation = row_style.text_orientation;
        let ch_advance = self.font_system.ch_advance(&style);
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
            return table_cell_non_text_content_height(cell);
        };

        table_cell_replaced_content_height(cell).max(
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
        let mut height = 0.0_f32;
        let mut inline_line_height = 0.0_f32;
        let mut estimated_float_context = FloatContext { shapes: Vec::new() };
        let mut estimated_float_bottom = 0.0_f32;

        for child in children {
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
                height += inline_line_height;
                inline_line_height = 0.0;
            }
            height +=
                self.table_cell_measured_block_child_height(child, stylesheets, available_width);
        }

        (height + inline_line_height).max(-estimated_float_bottom)
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
            box_tree::FormattingBox::Block(box_) => self.table_cell_measured_element_child_height(
                box_.element,
                &box_.style,
                &box_.children,
                stylesheets,
                available_width,
                child,
            ),
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
            _ => table_cell_formatting_child_outer_height(child),
        }
    }

    pub(in crate::layout::table) fn table_cell_final_relayout_child_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
        percentage_height_basis: Option<f32>,
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
                .unwrap_or_else(|| table_cell_formatting_child_outer_height(fallback_child));
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

        let style =
            self.table_cell_content_sizing_style(style, TableCellContentSizingPolicy::RowMinimum);
        match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => {
                table_cell_canvas_first_pass_outer_height(element, &style, available_width)
            }
            Some(ReplacedElementKind::Image) => {
                self.estimate_image_height(element, &style, available_width)
            }
            Some(ReplacedElementKind::Svg) => estimate_svg_height(element, &style),
            None if style.display.is_table() || is_html_table_element(element) => self
                .estimate_element_height(
                    element,
                    &style,
                    stylesheets,
                    available_width,
                    Some(children),
                )
                .unwrap_or_else(|| table_cell_formatting_child_outer_height(fallback_child)),
            None => self.table_cell_row_minimum_block_like_outer_height(
                element,
                &style,
                children,
                stylesheets,
                available_width,
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
    ) -> f32 {
        let mut used_style = style.clone();
        let box_metrics = apply_used_box_metrics(&mut used_style, available_outer_width.max(0.0));
        let style = &used_style;
        let horizontal_extras = box_metrics.horizontal_non_content();
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
            intrinsic::content_width_from_intrinsic(
                style,
                available_outer_width,
                horizontal_extras,
                min_content,
                max_content,
                intrinsic::IntrinsicAutoWidth::FillAvailable,
            )
        } else {
            used_content_width(style, available_outer_width, horizontal_extras)
        };
        let content_width = constrain_width(style, requested_content_width, available_outer_width)
            .max(style.font_size);

        let mut content_height = 0.0;
        if !has_non_inline_formatting_box(children)
            && (has_direct_inline_content_box(children)
                || has_atomic_inline_formatting_box(children))
        {
            let text = inline_text_from_formatting_boxes(children);
            if !text.is_empty() {
                content_height += self.estimate_text_physical_height(
                    &text,
                    style,
                    content_width,
                    style.padding.left,
                    style.padding.right,
                );
            } else if has_atomic_inline_formatting_box(children) {
                content_height += style.line_height;
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
                    content_height += self.table_cell_children_non_text_content_height(
                        box_children,
                        stylesheets,
                        content_width,
                    );
                } else {
                    let text = inline_text_from_formatting_boxes(box_children);
                    if !text.is_empty() || has_atomic_inline_formatting_box(box_children) {
                        content_height += self
                            .estimate_text_physical_height(
                                &text,
                                box_style,
                                content_width,
                                box_style.padding.left,
                                box_style.padding.right,
                            )
                            .max(box_style.line_height);
                    }
                }
                continue;
            }
            let Some((child_element, _, child_style, child_children)) = child_box.element_parts()
            else {
                continue;
            };
            if child_style.display.is_block_level()
                || is_document_canvas_element(element)
                || is_replaced_element(child_element)
            {
                content_height += self.table_cell_row_minimum_element_outer_height(
                    child_element,
                    child_style,
                    child_children,
                    stylesheets,
                    content_width,
                    child_box,
                );
            }
        }

        if !has_auto_height(style)
            || used_min_height(style, content_width).is_some()
            || used_max_height(style, content_width).is_some()
        {
            let vertical_extras = box_metrics.vertical_non_content();
            let requested_content_height =
                used_content_height_or_auto(style, content_height, vertical_extras)
                    .unwrap_or(content_height);
            content_height = constrain_height(style, requested_content_height, content_width);
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
        percentage_height_basis: Option<f32>,
    ) -> f32 {
        let mut height = 0.0_f32;
        let mut inline_line_height = 0.0_f32;

        for child in children {
            if let Some(inline_height) =
                self.table_cell_measured_inline_outer_height(child, stylesheets, available_width)
            {
                inline_line_height = inline_line_height.max(inline_height);
                continue;
            }
            if inline_line_height > 0.0 {
                height += inline_line_height;
                inline_line_height = 0.0;
            }
            height += self.table_cell_measured_block_child_final_relayout_height(
                child,
                stylesheets,
                available_width,
                percentage_height_basis,
            );
        }

        height + inline_line_height
    }

    fn table_cell_measured_block_child_final_relayout_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
        percentage_height_basis: Option<f32>,
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
                    None,
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .table_cell_children_final_relayout_height(
                    &box_.children,
                    stylesheets,
                    available_width,
                    None,
                ),
            _ => table_cell_formatting_child_outer_height(child),
        }
    }

    fn table_cell_final_relayout_element_outer_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        parent_percentage_height_basis: Option<f32>,
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
        let box_metrics = apply_used_box_metrics(&mut used_style, available_outer_width.max(0.0));
        let style = &used_style;
        let horizontal_extras = box_metrics.horizontal_non_content();
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
            intrinsic::content_width_from_intrinsic(
                style,
                available_outer_width,
                horizontal_extras,
                min_content,
                max_content,
                intrinsic::IntrinsicAutoWidth::FillAvailable,
            )
        } else {
            used_content_width(style, available_outer_width, horizontal_extras)
        };
        let content_width = constrain_width(style, requested_content_width, available_outer_width)
            .max(style.font_size);
        let vertical_extras = box_metrics.vertical_non_content();
        let specified_content_height = used_content_height_or_auto_with_optional_basis(
            style,
            parent_percentage_height_basis,
            vertical_extras,
        );

        let mut content_height = 0.0;
        if !has_non_inline_formatting_box(children)
            && (has_direct_inline_content_box(children)
                || has_atomic_inline_formatting_box(children))
        {
            let text = inline_text_from_formatting_boxes(children);
            if !text.is_empty() {
                content_height += self.estimate_text_physical_height(
                    &text,
                    style,
                    content_width,
                    style.padding.left,
                    style.padding.right,
                );
            } else if has_atomic_inline_formatting_box(children) {
                content_height += style.line_height;
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
                    content_height += self.table_cell_children_final_relayout_height(
                        box_children,
                        stylesheets,
                        content_width,
                        None,
                    );
                } else {
                    let text = inline_text_from_formatting_boxes(box_children);
                    if !text.is_empty() || has_atomic_inline_formatting_box(box_children) {
                        content_height += self
                            .estimate_text_physical_height(
                                &text,
                                box_style,
                                content_width,
                                box_style.padding.left,
                                box_style.padding.right,
                            )
                            .max(box_style.line_height);
                    }
                }
                continue;
            }
            let Some((child_element, _, child_style, child_children)) = child_box.element_parts()
            else {
                continue;
            };
            if child_style.display.is_block_level()
                || is_document_canvas_element(element)
                || is_replaced_element(child_element)
            {
                content_height += self.table_cell_final_relayout_element_outer_height(
                    child_element,
                    child_style,
                    child_children,
                    stylesheets,
                    content_width,
                    specified_content_height,
                );
            }
        }

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

fn table_cell_used_min_height(style: &ComputedStyle, percentage_basis: Option<f32>) -> Option<f32> {
    used_length_percentage_or_auto_with_optional_basis(
        style.box_values.min_height,
        percentage_basis,
    )
    .map(|value| value.max(0.0))
}

fn table_cell_used_max_height(style: &ComputedStyle, percentage_basis: Option<f32>) -> Option<f32> {
    used_length_percentage_or_auto_with_optional_basis(
        style.box_values.max_height,
        percentage_basis,
    )
    .map(|value| value.max(0.0))
}
