use super::*;

/// A table-cell contribution to CSS Tables auto column measures.
///
/// CSS Tables 3 defines column min/max-content and intrinsic percentage widths
/// in layers: first columns and single-column cells, then cells with larger
/// spans in increasing span order. Keeping contributions explicit lets the
/// layout pass build the correct baseline widths before distributing spanning
/// cell excess:
/// <https://drafts.csswg.org/css-tables-3/#computing-column-measures>.
#[derive(Debug, Clone, Copy)]
struct TableCellColumnContribution {
    start: usize,
    end: usize,
    colspan: usize,
    min_target_width: f32,
    max_target_width: f32,
    percentage: f32,
    explicit_non_percentage_width: bool,
    internal_spacing: f32,
}

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    /// Resolve the wrapper insets contributed by collapsed outer table borders.
    ///
    /// CSS 2.2 collapsed border conflict resolution produces grid-edge border
    /// winners before table wrapper layout consumes the outer half-widths as
    /// used table border insets.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
    pub(in crate::layout::table) fn collapsed_border_outer_insets(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        columns: &[TableColumn<'_>],
        column_count: usize,
    ) -> css::Edges {
        self.collapsed_table_geometry(rows, grid, table_style, stylesheets, columns, column_count)
            .outer_insets
    }

    /// Resolve vertical border-box insets for absolutely positioned collapsed tables.
    ///
    /// CSS Positioned Layout uses the border box in vertical inset equations,
    /// while CSS 2.2 collapsed borders contribute resolved outer grid insets
    /// rather than the authored full table border widths:
    /// <https://www.w3.org/TR/css-position-3/#abs-non-replaced-height> and
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
    pub(in crate::layout) fn collapsed_table_outer_vertical_insets(
        &mut self,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> Option<f32> {
        if style.border_collapse != css::BorderCollapse::Collapse {
            return None;
        }
        let fragment = fragment?;
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        if rows.is_empty() {
            return Some(vertical_border_width(style));
        }
        let grid = table_grid(rows);
        let column_count = grid.column_count.max(1);
        let insets = self.collapsed_border_outer_insets(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            column_count,
        );
        Some(insets.top + insets.bottom)
    }

    /// Resolve horizontal border-box insets for parent-facing collapsed table sizing.
    ///
    /// CSS 2.2 centers collapsed borders on the table grid edges; those
    /// resolved outer half-widths are part of the table wrapper's visual border
    /// box even though they are not ordinary separated table padding or border.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
    pub(in crate::layout) fn collapsed_table_outer_horizontal_insets(
        &mut self,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> Option<f32> {
        if style.border_collapse != css::BorderCollapse::Collapse {
            return None;
        }
        let fragment = fragment?;
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        if rows.is_empty() {
            return Some(horizontal_border_width(style));
        }
        let grid = table_grid(rows);
        let column_count = grid.column_count.max(1);
        let insets = self.collapsed_border_outer_insets(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            column_count,
        );
        Some(insets.left + insets.right)
    }

    #[allow(clippy::too_many_arguments)]
    /// Collect CSS Tables column measures before final width distribution.
    ///
    /// CSS Tables 3 computes min-content widths, max-content widths,
    /// intrinsic percentage widths, and constrainedness before resolving the
    /// table's used width:
    /// <https://drafts.csswg.org/css-tables-3/#computing-column-measures>.
    pub(in crate::layout::table) fn table_column_measures(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        columns: &[TableColumn<'_>],
        _table_width: f32,
        table_cellpadding: Option<f32>,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> TableColumnMeasures {
        let column_count = grid.column_count;
        let collapsed_columns =
            self.collapsed_table_columns(columns, table_style, stylesheets, column_count);
        let visible_columns = collapsed_columns
            .iter()
            .filter(|collapsed| !**collapsed)
            .count();
        let total_horizontal_spacing =
            table_displayed_horizontal_spacing(visible_columns, table_metrics.clone());
        let mut measures = TableColumnMeasures {
            min_content_widths: vec![0.0_f32; column_count],
            max_content_widths: vec![0.0_f32; column_count],
            intrinsic_percentages: vec![0.0_f32; column_count],
            constrained: vec![false; column_count],
            occupied: vec![false; column_count],
            total_horizontal_spacing,
        };

        let mut column_index = 0;
        for column in columns {
            if column_index >= column_count {
                break;
            }
            let span = column.span.min(column_count - column_index).max(1);
            if let Some(group) = &column.group {
                let group_style =
                    self.style_for_table_column_group(group, table_style, stylesheets);
                let group_flow_style = table_internal_flow_style(&group_style, table_style);
                apply_table_column_style_measures(
                    &mut measures,
                    column_index,
                    span,
                    &group_flow_style,
                );
            }
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            let column_flow_style = table_internal_flow_style(&column_style, table_style);
            apply_table_column_style_measures(
                &mut measures,
                column_index,
                span,
                &column_flow_style,
            );
            column_index += span;
        }

        let mut cell_contributions = Vec::new();
        for (row_index, row) in rows.iter().enumerate() {
            let row_style = self.style_for_table_row(row, table_style, stylesheets);
            for placement in &grid.rows[row_index] {
                if placement.column >= column_count {
                    break;
                }
                let cell = &row.cells[placement.cell];
                let colspan = placement
                    .colspan
                    .min(column_count - placement.column)
                    .max(1);
                let end = placement.column + colspan;
                let mut cell_style = self.style_for_table_cell(cell, row, &row_style, stylesheets);
                apply_table_cell_used_padding(
                    &mut cell_style,
                    table_cellpadding,
                    // Intrinsic table measures cannot depend on the table's
                    // eventual inline size. CSS Tables resolves percentage
                    // cell padding as zero at this stage, then resolves it
                    // after the track widths have been finalized.
                    // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
                    PercentageBasis::definite(layout_pt(0.0)),
                );

                // A cell's physical width only constrains a table column when
                // the table root inline axis is physical horizontal. For an
                // orthogonal table it contributes to the row/block track.
                let table_inline_is_physical_width =
                    !WritingModeAxes::new(table_style.writing_mode, table_style.direction)
                        .swaps_physical_axes();
                let explicit_width = table_inline_is_physical_width
                    .then(|| {
                        cell.element
                            .and_then(|element| declared_table_cell_width(element, &cell_style))
                    })
                    .flatten();
                let border_insets =
                    (table_metrics.border_collapse == css::BorderCollapse::Collapse).then(|| {
                        table_cell_border_insets(
                            &cell_style,
                            placement,
                            row_index,
                            table_metrics.clone(),
                            collapsed_geometry,
                        )
                    });
                let track_inline_size = table_cell_content_table_inline_size(
                    self,
                    cell,
                    &cell_style,
                    table_style,
                    stylesheets,
                    border_insets,
                );
                let min_content_width = track_inline_size.min_content;
                let max_content_width = track_inline_size.max_content;
                let width_floor = explicit_width
                    .clone()
                    .map(|width| {
                        declared_table_cell_width_length_floor(&cell_style, width, border_insets)
                            .points()
                    })
                    .unwrap_or(0.0);
                let min_target_width = constrain_table_intrinsic_width_with_floor(
                    &cell_style,
                    min_content_width,
                    width_floor,
                );
                // A non-percentage specified cell width constrains an auto
                // table's preferred track width. Its min-content contribution
                // can still exceed that width for unbreakable content, but
                // optional CSS Text breaks (including `break-spaces`) must not
                // make the table choose the cell's unwrapped max-content size.
                // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
                let max_target_width = if explicit_width
                    .as_ref()
                    .is_some_and(|width| declared_table_width_is_non_percentage(width.clone()))
                {
                    min_target_width
                } else {
                    constrain_table_intrinsic_width_with_floor(
                        &cell_style,
                        max_content_width.max(min_target_width),
                        width_floor,
                    )
                };
                let percentage = intrinsic_percentage_contribution(&cell_style).max(
                    explicit_width
                        .clone()
                        .map(declared_table_width_percentage)
                        .unwrap_or(0.0),
                );

                for index in placement.column..end {
                    measures.occupied[index] = true;
                }
                let internal_spacing = if colspan > 1 {
                    table_internal_horizontal_spacing(
                        placement.column,
                        end,
                        &collapsed_columns,
                        table_metrics.clone(),
                    )
                } else {
                    0.0
                };
                cell_contributions.push(TableCellColumnContribution {
                    start: placement.column,
                    end,
                    colspan,
                    min_target_width,
                    max_target_width,
                    percentage,
                    explicit_non_percentage_width: explicit_width
                        .is_some_and(declared_table_width_is_non_percentage),
                    internal_spacing,
                });
            }
        }

        for contribution in cell_contributions
            .iter()
            .filter(|contribution| contribution.colspan == 1)
        {
            measures.min_content_widths[contribution.start] =
                measures.min_content_widths[contribution.start].max(contribution.min_target_width);
            measures.max_content_widths[contribution.start] =
                measures.max_content_widths[contribution.start].max(contribution.max_target_width);
            measures.intrinsic_percentages[contribution.start] =
                measures.intrinsic_percentages[contribution.start].max(contribution.percentage);
            if contribution.explicit_non_percentage_width {
                measures.constrained[contribution.start] = true;
            }
        }

        let mut spanning_contributions = cell_contributions
            .iter()
            .filter(|contribution| contribution.colspan > 1)
            .cloned()
            .collect::<Vec<_>>();
        spanning_contributions.sort_by_key(|contribution| contribution.colspan);
        for contribution in spanning_contributions {
            distribute_spanned_percentage(
                &mut measures,
                contribution.start,
                contribution.end,
                contribution.percentage,
            );
            distribute_spanned_measure(
                &mut measures,
                contribution.start,
                contribution.end,
                (contribution.min_target_width - contribution.internal_spacing).max(0.0),
                true,
            );
            distribute_spanned_measure(
                &mut measures,
                contribution.start,
                contribution.end,
                (contribution.max_target_width - contribution.internal_spacing).max(0.0),
                false,
            );
        }

        cap_intrinsic_percentages(&mut measures.intrinsic_percentages);
        measures
    }

    #[allow(clippy::too_many_arguments)]
    /// Resolve the table wrapper's used grid/content inline size.
    ///
    /// CSS Tables 3 computes the table grid minimum before resolving the used
    /// table width; auto-layout tables cannot use a content-box width smaller
    /// than that grid min-content contribution:
    /// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>.
    pub(in crate::layout::table) fn resolve_table_used_content_width(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        columns: &[TableColumn<'_>],
        available_outer_width: f32,
        table_cellpadding: Option<f32>,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        table_width: &mut UsedTableWidth,
    ) {
        let measures = self.table_column_measures(
            rows,
            grid,
            table_style,
            stylesheets,
            columns,
            table_width.content_width.points(),
            table_cellpadding,
            table_metrics,
            collapsed_geometry,
        );
        let min_content = measures.table_min_content_width().max(0.0);
        let max_content = measures.table_max_content_width().max(min_content);
        let horizontal_border_non_content =
            if table_style.border_collapse == css::BorderCollapse::Collapse {
                0.0
            } else {
                table_width.border_widths.left + table_width.border_widths.right
            };
        let horizontal_non_content =
            horizontal_border_non_content + table_width.padding.left + table_width.padding.right;
        let mut content_width = table_width.content_width.points();
        if let Some(width) = intrinsic::intrinsic_content_box_width_keyword(
            table_root_inline_size(table_style),
            content_box_pt(min_content),
            content_box_pt(max_content),
            layout_pt(available_outer_width),
            non_content_pt(horizontal_non_content),
        ) {
            content_width = constrain_content_width(
                table_style,
                width,
                PercentageBasis::definite(layout_pt(available_outer_width)),
            )
            .points()
            .max(table_style.font_size);
        }
        content_width =
            table_content_width_clamped_to_min_content(table_style, content_width, min_content);
        table_width.content_width = content_box_pt(content_width);
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_column_plan(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        columns: &[TableColumn<'_>],
        table_width: f32,
        distribute_extra_width: bool,
        table_cellpadding: Option<f32>,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> TableColumnPlan {
        let column_count = grid.column_count;
        if table_style.table_layout == TableLayout::Fixed {
            return self.fixed_table_column_plan(
                rows,
                grid,
                table_style,
                stylesheets,
                columns,
                table_width,
                distribute_extra_width,
                table_cellpadding,
                table_metrics,
                collapsed_geometry,
                column_count,
            );
        }

        let measures = self.table_column_measures(
            rows,
            grid,
            table_style,
            stylesheets,
            columns,
            table_width,
            table_cellpadding,
            table_metrics.clone(),
            collapsed_geometry,
        );
        let table_min_content_width = measures.table_min_content_width();
        let table_max_content_width = measures
            .table_max_content_width()
            .max(table_min_content_width);
        let used_table_width = if distribute_extra_width {
            table_width.max(table_min_content_width)
        } else if table_width <= table_min_content_width {
            table_min_content_width
        } else if table_width < table_max_content_width {
            table_width
        } else {
            table_max_content_width
        };
        let assignable_width = (used_table_width - measures.total_horizontal_spacing).max(0.0);
        let widths = auto_table_column_widths(&measures, assignable_width);

        let collapsed_columns =
            self.collapsed_table_columns(columns, table_style, stylesheets, column_count);
        TableColumnPlan::with_collapsed(
            widths,
            table_metrics.spacing.horizontal.length_points(),
            collapsed_columns,
            TableAxes::for_style(table_style),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn fixed_table_column_plan(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        columns: &[TableColumn<'_>],
        table_width: f32,
        distribute_extra_width: bool,
        table_cellpadding: Option<f32>,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        column_count: usize,
    ) -> TableColumnPlan {
        // CSS 2.2 fixed table layout uses column widths from column elements,
        // then first-row cells, then divides remaining space equally.
        let collapsed_columns =
            self.collapsed_table_columns(columns, table_style, stylesheets, column_count);
        let visible_columns = collapsed_columns
            .iter()
            .filter(|collapsed| !**collapsed)
            .count();
        let total_horizontal_spacing =
            table_displayed_horizontal_spacing(visible_columns, table_metrics.clone());
        let content_table_width =
            (table_width - total_horizontal_spacing).max(table_style.font_size);
        let mut widths = vec![0.0_f32; column_count];
        let mut declared = vec![false; column_count];
        let mut column_index = 0;
        for column in columns {
            if column_index >= column_count {
                break;
            }
            let span = column.span.min(column_count - column_index).max(1);
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            let column_flow_style = table_internal_flow_style(&column_style, table_style);
            if let Some(width) = declared_table_column_width(&column_flow_style) {
                let width = constrain_declared_table_width(
                    &column_flow_style,
                    width,
                    content_box_pt(content_table_width),
                );
                // Fixed-table column distribution is scalar coordinate
                // arithmetic over the table grid.
                distribute_fixed_width(
                    &mut widths,
                    &mut declared,
                    column_index,
                    span,
                    width.points(),
                );
            }
            column_index += span;
        }

        if let Some(first_row) = rows.first() {
            let row_style = self.style_for_table_row(first_row, table_style, stylesheets);
            for placement in &grid.rows[0] {
                if placement.column >= column_count {
                    break;
                }
                let cell = &first_row.cells[placement.cell];
                let colspan = placement
                    .colspan
                    .min(column_count - placement.column)
                    .max(1);
                let mut cell_style =
                    self.style_for_table_cell(cell, first_row, &row_style, stylesheets);
                apply_table_cell_used_padding(
                    &mut cell_style,
                    table_cellpadding,
                    PercentageBasis::definite(layout_pt(content_table_width)),
                );
                if let Some(explicit_width) = cell
                    .element
                    .and_then(|element| declared_table_cell_width(element, &cell_style))
                {
                    let border_insets = (table_metrics.border_collapse
                        == css::BorderCollapse::Collapse)
                        .then(|| {
                            table_cell_border_insets(
                                &cell_style,
                                placement,
                                0,
                                table_metrics.clone(),
                                collapsed_geometry,
                            )
                        });
                    let width = declared_table_cell_border_box_width(
                        &cell_style,
                        explicit_width,
                        content_table_width,
                        border_insets,
                    )
                    .points();
                    let width = if colspan > 1 {
                        let end = (placement.column + colspan).min(collapsed_columns.len());
                        let internal_spacing = table_internal_horizontal_spacing(
                            placement.column,
                            end,
                            &collapsed_columns,
                            table_metrics.clone(),
                        );
                        (width - internal_spacing).max(0.0)
                    } else {
                        width
                    };
                    distribute_first_row_fixed_width(
                        &mut widths,
                        &mut declared,
                        placement.column,
                        colspan,
                        width,
                    );
                }
            }
        }

        let used_width = widths.iter().sum::<f32>();
        if distribute_extra_width && used_width < content_table_width {
            let remaining = content_table_width - used_width;
            let receivers = declared
                .iter()
                .enumerate()
                .filter_map(|(index, is_declared)| (!is_declared).then_some(index))
                .collect::<Vec<_>>();
            let receivers = if receivers.is_empty() {
                (0..column_count).collect::<Vec<_>>()
            } else {
                receivers
            };
            let extra = remaining / receivers.len() as f32;
            for index in receivers {
                widths[index] += extra;
            }
        }

        TableColumnPlan::with_collapsed(
            widths,
            table_metrics.spacing.horizontal.length_points(),
            collapsed_columns,
            TableAxes::for_style(table_style),
        )
    }

    /// Compute which table grid columns are suppressed by `visibility: collapse`.
    ///
    /// CSS 2.2 defines `visibility: collapse` for table columns and column
    /// groups. Collapsed columns are removed from the displayed inline width
    /// without recomputing the table's column constraints.
    /// https://www.w3.org/TR/CSS22/tables.html#dynamic-effects
    pub(in crate::layout::table) fn collapsed_table_columns(
        &mut self,
        columns: &[TableColumn<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        column_count: usize,
    ) -> Vec<bool> {
        let mut collapsed = vec![false; column_count];
        let mut column_index = 0;
        for column in columns {
            if column_index >= column_count {
                break;
            }
            let span = column.span.min(column_count - column_index).max(1);
            let group_collapsed = column
                .group
                .as_ref()
                .map(|group| self.style_for_table_column_group(group, table_style, stylesheets))
                .is_some_and(|group_style| group_style.visibility == Visibility::Collapse);
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            if group_collapsed || column_style.visibility == Visibility::Collapse {
                for value in &mut collapsed[column_index..column_index + span] {
                    *value = true;
                }
            }
            column_index += span;
        }
        collapsed
    }

    /// Cross the table used-value boundary without changing the style retained
    /// by the table fragment for later cascade reconstruction.
    pub(in crate::layout::table) fn table_used_style(
        &self,
        source: &ComputedStyle,
    ) -> TableUsedStyle {
        let used = if source.zoom_applied {
            source.clone()
        } else {
            self.style_with_current_viewport_lengths(source)
        };
        TableUsedStyle::from_source_and_normalized(source.clone(), used)
    }

    pub(in crate::layout::table) fn style_for_table_row(
        &mut self,
        row: &TableRow<'_>,
        table_style: &(impl TableStyleSource + ?Sized),
        stylesheets: &[Stylesheet],
    ) -> TableUsedStyle {
        if let Some(style) = &row.style {
            return self.table_used_style(style.as_ref());
        }
        let mut ancestors = self.ancestors.clone();
        ancestors.extend(row.ancestors.iter().cloned());
        let parent_style = row
            .row_groups
            .last()
            .map(|group| self.style_for_table_row_group(group, table_style, stylesheets))
            .unwrap_or_else(|| self.table_used_style(table_style.table_source()));
        let source = if let Some(element) = row.element {
            self.style_for_layout_element_with_parent_font_metrics_and_ancestors(
                element,
                row.signature.clone(),
                stylesheets,
                Some(parent_style.source()),
                &ancestors,
            )
        } else {
            self.style_for_signature_with_parent_font_metrics(
                row.signature.clone(),
                None,
                stylesheets,
                Some(parent_style.source()),
                &ancestors,
            )
        };
        self.table_used_style(&source)
    }

    pub(in crate::layout::table) fn style_for_table_row_group(
        &mut self,
        row_group: &TableRowGroup<'_>,
        table_style: &(impl TableStyleSource + ?Sized),
        stylesheets: &[Stylesheet],
    ) -> TableUsedStyle {
        if let Some(style) = &row_group.style {
            return self.table_used_style(style.as_ref());
        }
        let ancestors = self.ancestors.clone();
        let source = self.style_for_layout_element_with_parent_font_metrics_and_ancestors(
            row_group.element,
            row_group.signature.clone(),
            stylesheets,
            Some(table_style.table_source()),
            &ancestors,
        );
        self.table_used_style(&source)
    }

    pub(in crate::layout::table) fn style_for_table_column(
        &mut self,
        column: &TableColumn<'_>,
        table_style: &(impl TableStyleSource + ?Sized),
        stylesheets: &[Stylesheet],
    ) -> TableUsedStyle {
        if let Some(style) = &column.style {
            return self.table_used_style(style.as_ref());
        }
        let mut ancestors = self.ancestors.clone();
        let parent_style = if let Some(group) = &column.group {
            let group_style = self.style_for_table_column_group(group, table_style, stylesheets);
            ancestors.push(group.signature.clone());
            group_style
        } else {
            self.table_used_style(table_style.table_source())
        };
        let source = self.style_for_layout_element_with_parent_font_metrics_and_ancestors(
            column.element,
            column.signature.clone(),
            stylesheets,
            Some(parent_style.source()),
            &ancestors,
        );
        self.table_used_style(&source)
    }

    pub(in crate::layout::table) fn style_for_table_column_group(
        &mut self,
        group: &TableColumnGroup<'_>,
        table_style: &(impl TableStyleSource + ?Sized),
        stylesheets: &[Stylesheet],
    ) -> TableUsedStyle {
        if let Some(style) = &group.style {
            return self.table_used_style(style.as_ref());
        }
        let ancestors = self.ancestors.clone();
        let source = self.style_for_layout_element_with_parent_font_metrics_and_ancestors(
            group.element,
            group.signature.clone(),
            stylesheets,
            Some(table_style.table_source()),
            &ancestors,
        );
        self.table_used_style(&source)
    }

    pub(in crate::layout::table) fn style_for_table_cell(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        row_style: &(impl TableStyleSource + ?Sized),
        stylesheets: &[Stylesheet],
    ) -> TableUsedStyle {
        if cell.anonymous {
            let mut style = cell
                .style
                .as_deref()
                .cloned()
                .unwrap_or_else(|| row_style.table_source().clone());
            style.display = Display::TABLE_CELL;
            style.margin = css::Edges::ZERO;
            style.padding = css::Edges::ZERO;
            style.border_width = 0.0;
            style.border_widths = css::Edges::ZERO;
            style.border_width_values = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
            style.border_styles = css::BorderStyles::NONE;
            style.background_color = None;
            set_style_auto_width(&mut style);
            set_style_auto_height(&mut style);
            style.box_values.min_width = css::ComputedLengthPercentageOrAuto::Auto;
            style.box_values.max_width = css::ComputedLengthPercentageOrAuto::Auto;
            style.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
            style.box_values.max_height = css::ComputedLengthPercentageOrAuto::Auto;
            return self.table_used_style(&style);
        }
        if let Some(style) = &cell.style {
            return self.table_used_style(style.as_ref());
        }
        let mut ancestors = self.ancestors.clone();
        ancestors.extend(row.ancestors.iter().cloned());
        ancestors.push(row.signature.clone());
        let source = if let Some(element) = cell.element {
            self.style_for_layout_element_with_parent_font_metrics_and_ancestors(
                element,
                cell.signature.clone(),
                stylesheets,
                Some(row_style.table_source()),
                &ancestors,
            )
        } else {
            self.style_for_signature_with_parent_font_metrics(
                cell.signature.clone(),
                None,
                stylesheets,
                Some(row_style.table_source()),
                &ancestors,
            )
        };
        self.table_used_style(&source)
    }

    pub(in crate::layout::table) fn style_for_table_caption(
        &mut self,
        caption: &TableCaption<'_>,
        table_style: &(impl TableStyleSource + ?Sized),
        stylesheets: &[Stylesheet],
    ) -> TableUsedStyle {
        if let Some(style) = &caption.style {
            return self.table_used_style(style.as_ref());
        }
        let ancestors = self.ancestors.clone();
        let source = self.style_for_layout_element_with_parent_font_metrics_and_ancestors(
            caption.element,
            caption.signature.clone(),
            stylesheets,
            Some(table_style.table_source()),
            &ancestors,
        );
        self.table_used_style(&source)
    }

    pub(in crate::layout::table) fn enter_table_cell_content_scope(
        &mut self,
        cell_style: &ComputedStyle,
        content_box: TableCellContentBox,
        ancestors: Vec<ElementSignature>,
        definite_block_size: BlockSizePercentageBasis,
    ) -> TableCellContentScope {
        let scope = TableCellContentScope {
            content_left: self.content_left,
            content_right: self.content_right,
            cursor_y: self.cursor_y,
            ancestors: self.ancestors.clone(),
            containing_block_direction: self.containing_block_direction,
            containing_block_writing_mode: self.containing_block_writing_mode,
            content_logical_inline_size_stack: self.content_logical_inline_size_stack.clone(),
            child_available_space_stack: self.child_available_space_stack.clone(),
            definite_block_size_stack: self.definite_block_size_stack.clone(),
        };
        let content_width = content_box.width().max(0.0);
        let content_height = content_box.height().max(0.0);
        let content_logical_inline_size =
            if WritingModeAxes::new(cell_style.writing_mode, cell_style.direction)
                .swaps_physical_axes()
            {
                content_height
            } else {
                content_width
            };

        self.content_left = content_box.left();
        self.content_right = content_box.right();
        self.cursor_y = content_box.top_y();
        self.ancestors = ancestors;
        self.containing_block_direction = cell_style.direction;
        self.containing_block_writing_mode = cell_style.writing_mode;
        self.content_logical_inline_size_stack
            .push(content_logical_inline_size.max(1.0));
        self.child_available_space_stack
            .push(ChildAvailableSpace::new(
                cell_style.writing_mode,
                PhysicalContentWidth::new(content_box_pt(content_width)),
                Some(PhysicalContentHeight::new(content_box_pt(content_height))),
                PhysicalContentHeight::new(content_box_pt(content_height)),
            ));
        self.definite_block_size_stack.push(definite_block_size);
        scope
    }

    pub(in crate::layout::table) fn enter_table_cell_content_scope_for_rect(
        &mut self,
        cell_style: &ComputedStyle,
        content_rect: PageTopRect,
        ancestors: Vec<ElementSignature>,
        definite_block_size: Option<f32>,
    ) -> TableCellContentScope {
        self.enter_table_cell_content_scope(
            cell_style,
            TableCellContentBox::from_page_top_rect(content_rect),
            ancestors,
            block_size_percentage_basis_from_points(
                definite_block_size,
                BlockSizeBasisSource::TableCell,
            ),
        )
    }

    pub(in crate::layout::table) fn restore_table_cell_content_scope(
        &mut self,
        scope: TableCellContentScope,
    ) {
        self.content_left = scope.content_left;
        self.content_right = scope.content_right;
        self.cursor_y = scope.cursor_y;
        self.ancestors = scope.ancestors;
        self.containing_block_direction = scope.containing_block_direction;
        self.containing_block_writing_mode = scope.containing_block_writing_mode;
        self.content_logical_inline_size_stack = scope.content_logical_inline_size_stack;
        self.child_available_space_stack = scope.child_available_space_stack;
        self.definite_block_size_stack = scope.definite_block_size_stack;
    }

    pub(in crate::layout::table) fn table_cell_content_x_offset(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        cell_width: f32,
        border_insets: css::Edges,
    ) -> f32 {
        if !WritingModeAxes::new(cell_style.writing_mode, cell_style.direction)
            .swaps_physical_axes()
            || cell_style.align_content.keyword == ContentAlignmentKeyword::Normal
        {
            return 0.0;
        }

        let content_box_width = (cell_width
            - border_insets.left
            - border_insets.right
            - cell_style.padding.left
            - cell_style.padding.right)
            .max(0.0);
        let subject_width = self.table_cell_content_alignment_subject_width(
            cell,
            cell_style,
            stylesheets,
            border_insets,
        );
        let free_space = content_box_width - subject_width;
        let toward_block_end = content_alignment_offset_toward_end(
            cell_style.align_content,
            free_space,
            block_align_content_defaults_to_safe_overflow(cell_style),
        );

        match block_start_side(cell_style.writing_mode) {
            PhysicalSide::Left => toward_block_end,
            PhysicalSide::Right => free_space - toward_block_end,
            PhysicalSide::Top | PhysicalSide::Bottom => 0.0,
        }
    }

    pub(in crate::layout::table) fn table_cell_content_alignment_subject_width(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        border_insets: css::Edges,
    ) -> f32 {
        let non_content = cell_style.padding.left
            + cell_style.padding.right
            + border_insets.left
            + border_insets.right;
        let fallback_width = (table_cell_content_max_width(
            self,
            cell,
            cell_style,
            stylesheets,
            Some(border_insets),
        ) - non_content)
            .max(0.0);
        if !WritingModeAxes::new(cell_style.writing_mode, cell_style.direction)
            .swaps_physical_axes()
        {
            return fallback_width;
        }

        let inline_measurement = if let Some(children) = cell.children.as_deref() {
            Some(self.intrinsic_inline_measurement_for_boxes(
                children,
                cell_style,
                stylesheets,
                f32::MAX,
            ))
        } else {
            cell.element.map(|element| {
                self.intrinsic_inline_measurement_for_element(
                    element,
                    cell_style,
                    stylesheets,
                    None,
                    f32::MAX,
                )
            })
        };
        inline_measurement
            .map(|measurement| measurement.physical_height(cell_style))
            .filter(|width| *width > 0.0)
            .unwrap_or(fallback_width)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn layout_table_cell_replaced_children(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        cell_borders: css::Edges,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        content_offset: f32,
        content_x_offset: f32,
    ) {
        let content_box = border_box.content_box(
            placement,
            cell_style.padding,
            cell_borders,
            content_offset,
            content_x_offset,
        );
        let content_bounds = content_box.page_top_rect();
        let mut x = content_bounds.x();
        let y_top = content_bounds.top_y();
        if let Some(children) = cell.children.as_deref() {
            for child_box in children {
                let Some((child, _, _, _)) = child_box.element_parts() else {
                    continue;
                };
                if replaced_element_kind(child) == Some(ReplacedElementKind::Svg)
                    && let Some(asset) = self.resource_cache.inline_svg_asset(child)
                {
                    let size = asset.intrinsic_size();
                    if cell_style.visibility == Visibility::Visible {
                        let rect = PageTopRect::new(x, y_top, size.width, size.height).paint_rect();
                        for path in asset.paint_paths(rect) {
                            self.push_path_in_band(PaintBand::InFlowBlock, path);
                        }
                    }
                    x += size.width;
                }
            }
            return;
        }

        let Some(element) = cell.element else {
            return;
        };
        for child in &element.children {
            let NodeKind::Element(child) = &child.kind else {
                continue;
            };
            if replaced_element_kind(child) == Some(ReplacedElementKind::Svg)
                && let Some(asset) = self.resource_cache.inline_svg_asset(child)
            {
                let size = asset.intrinsic_size();
                let rect = PageTopRect::new(x, y_top, size.width, size.height).paint_rect();
                for path in asset.paint_paths(rect) {
                    self.push_path_in_band(PaintBand::InFlowBlock, path);
                }
                x += size.width;
            }
        }
    }
}

/// Return the style values used for table-internal track geometry.
///
/// Columns and column groups do not establish independent writing-mode flows:
/// their track direction and the physical property selected for a declared
/// size come from the table root.  Preserve all other column styling (notably
/// width, visibility, borders, and backgrounds), while making this boundary
/// explicit so a `writing-mode` or `direction` declaration on `<col>` cannot
/// alter track measurement.
/// <https://drafts.csswg.org/css-writing-modes-4/#writing-mode>
/// <https://drafts.csswg.org/css-tables-3/#table-layout>
fn table_internal_flow_style(style: &ComputedStyle, table_style: &ComputedStyle) -> ComputedStyle {
    let mut flow_style = style.clone();
    flow_style.writing_mode = table_style.writing_mode;
    flow_style.direction = table_style.direction;
    flow_style.text_orientation = table_style.text_orientation;
    flow_style
}
