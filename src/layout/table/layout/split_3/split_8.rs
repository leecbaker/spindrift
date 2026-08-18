use super::*;
use crate::layout::block::DefinitePhysicalContentHeight;
use crate::layout::block::child_available_space_for_formatting_context;

/// The intrinsic state contributed to one CSS table track distribution.
///
/// CSS Tables keeps a declared non-percentage width as a preferred track
/// contribution in addition to intrinsic min/max-content and percentage
/// contributions. A preferred cell width must not become a hard min-content
/// floor: an empty `width: 100px` cell may still shrink into a narrower float
/// exclusion band. Keeping these four values together also prevents `width:
/// 0` (or a small fixed width) from replacing an unbreakable cell's intrinsic
/// minimum:
/// <https://drafts.csswg.org/css-tables-3/#computing-column-measures>.
#[derive(Debug, Clone, Copy)]
struct TableTrackMeasure {
    min_content: f32,
    max_content: f32,
    declared_non_percentage_minimum: f32,
    percentage: f32,
}

impl TableTrackMeasure {
    fn min_target(self) -> f32 {
        self.min_content.max(self.declared_non_percentage_minimum)
    }

    fn max_target(self) -> f32 {
        self.max_content.max(self.min_target())
    }
}

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
    measure: TableTrackMeasure,
    explicit_non_percentage_width: bool,
    internal_spacing: f32,
}

/// Whether CSS 2.2's fixed table-layout algorithm applies to this table.
///
/// The algorithm requires a non-auto table width; otherwise table layout
/// falls back to its automatic intrinsic sizing path.
/// <https://www.w3.org/TR/CSS22/tables.html#fixed-table-layout>
fn fixed_table_layout_algorithm_applies(table_style: &ComputedStyle) -> bool {
    table_style.table_layout == TableLayout::Fixed && !table_root_inline_size(table_style).is_auto()
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
        stylesheets: &Stylesheets<'_>,
        columns: &[TableColumn<'_>],
        column_count: usize,
    ) -> css::Edges {
        self.collapsed_table_geometry(rows, grid, table_style, stylesheets, columns, column_count)
            .outer_insets
    }

    /// Resolve the collapsed table wrapper's conflict-resolved outer insets.
    ///
    /// CSS Positioned Layout consumes the wrapper border box, while CSS 2.2
    /// collapsed borders contribute the resolved outer grid insets rather than
    /// the authored full table border widths:
    /// <https://www.w3.org/TR/css-position-3/#abs-non-replaced-height> and
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
    pub(in crate::layout) fn resolved_collapsed_table_wrapper_insets(
        &mut self,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> Option<ResolvedTableWrapperInsets> {
        if style.border_collapse != css::BorderCollapse::Collapse {
            return None;
        }
        let fragment = fragment?;
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.row_ordering.rows.as_slice();
        if rows.is_empty() {
            return Some(ResolvedTableWrapperInsets::ZERO);
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
        Some(ResolvedTableWrapperInsets {
            border_widths: insets,
        })
    }

    /// Resolve vertical border-box insets for absolutely positioned collapsed tables.
    pub(in crate::layout) fn collapsed_table_outer_vertical_insets(
        &mut self,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> Option<f32> {
        self.resolved_collapsed_table_wrapper_insets(style, stylesheets, fragment)
            .map(ResolvedTableWrapperInsets::vertical_non_content)
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
        stylesheets: &Stylesheets<'_>,
        fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> Option<f32> {
        self.resolved_collapsed_table_wrapper_insets(style, stylesheets, fragment)
            .map(ResolvedTableWrapperInsets::horizontal_non_content)
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
        stylesheets: &Stylesheets<'_>,
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
                    PercentageBasis::definite(LogicalInlineContentSize::new(content_box_pt(0.0))),
                );

                // A cell's physical width only constrains a table column when
                // the table root inline axis is physical horizontal. For an
                // orthogonal table it contributes to the row/block track.
                let table_inline_is_physical_width =
                    !WritingModeAxes::new(table_style.writing_mode, table_style.used_direction())
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
                let min_content_width = track_inline_size.min_content.points();
                let max_content_width = track_inline_size.max_content.points();
                let width_floor = explicit_width
                    .clone()
                    .map(|width| {
                        declared_table_cell_width_length_floor(&cell_style, width, border_insets)
                            .points()
                    })
                    .unwrap_or(0.0);
                let min_content =
                    constrain_table_intrinsic_width_with_floor(&cell_style, min_content_width, 0.0);
                // A definite cell width is a minimum table-track constraint.
                // A zero width cannot replace the cell's intrinsic minimum,
                // while an unbreakable item can still require a wider track.
                // <https://www.w3.org/TR/CSS22/tables.html#auto-table-layout>
                let max_content = constrain_table_intrinsic_width_with_floor(
                    &cell_style,
                    max_content_width.max(min_content),
                    width_floor,
                );
                let percentage = intrinsic_percentage_contribution(&cell_style).max(
                    explicit_width
                        .clone()
                        .map(declared_table_track_size_percentage)
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
                    measure: TableTrackMeasure {
                        min_content,
                        max_content,
                        declared_non_percentage_minimum: width_floor,
                        percentage,
                    },
                    explicit_non_percentage_width: explicit_width
                        .is_some_and(declared_table_track_size_is_non_percentage),
                    internal_spacing,
                });
            }
        }

        for contribution in cell_contributions
            .iter()
            .filter(|contribution| contribution.colspan == 1)
        {
            measures.min_content_widths[contribution.start] = measures.min_content_widths
                [contribution.start]
                .max(contribution.measure.min_target());
            measures.max_content_widths[contribution.start] = measures.max_content_widths
                [contribution.start]
                .max(contribution.measure.max_target());
            measures.intrinsic_percentages[contribution.start] = measures.intrinsic_percentages
                [contribution.start]
                .max(contribution.measure.percentage);
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
                contribution.measure.percentage,
            );
            distribute_spanned_measure(
                &mut measures,
                contribution.start,
                contribution.end,
                (contribution.measure.min_target() - contribution.internal_spacing).max(0.0),
                true,
            );
            distribute_spanned_measure(
                &mut measures,
                contribution.start,
                contribution.end,
                (contribution.measure.max_target() - contribution.internal_spacing).max(0.0),
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
    pub(in crate::layout::table) fn resolve_table_used_content_inline_size(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        columns: &[TableColumn<'_>],
        available_outer_inline: f32,
        table_cellpadding: Option<f32>,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        table_geometry: &mut UsedTableWrapperGeometry,
    ) {
        let inline_non_content = table_geometry.inline_non_content();
        let mut content_inline = table_geometry.grid_inline.content_box_length();
        // `used_table_wrapper_geometry` starts an auto table from the available inline
        // span so the ordinary intrinsic path can shrink it. A non-zero
        // wrapper min-inline constraint instead establishes the grid's
        // definite target; preserve that target before distributing tracks.
        if table_root_inline_size(table_style).is_auto()
            && table_root_distributes_extra_inline_space(table_style)
            && let Some(min_inline_size) = table_root_inline_content_box_size(
                table_root_min_inline_size(table_style),
                table_style.box_sizing,
                PercentageBasis::definite(content_box_pt(available_outer_inline)),
                inline_non_content,
            )
        {
            content_inline = min_inline_size;
        }

        // CSS 2.2's fixed table-layout algorithm needs the used table width,
        // declared column widths, and at most the first row.  It must not
        // inspect every cell merely to derive an automatic-layout
        // min-content floor.  Keep the auto-width case on the existing
        // measuring path: CSS 2.2 falls back to automatic table layout when
        // a fixed-layout table has no specified width.
        // <https://www.w3.org/TR/CSS22/tables.html#fixed-table-layout>
        if fixed_table_layout_algorithm_applies(table_style) {
            table_geometry.set_grid_inline(LogicalInlineContentSize::new(content_inline));
            return;
        }

        let measures = self.table_column_measures(
            rows,
            grid,
            table_style,
            stylesheets,
            columns,
            content_inline.points(),
            table_cellpadding,
            table_metrics,
            collapsed_geometry,
        );
        let min_content = measures.table_min_content_width().max(0.0);
        let max_content = measures.table_max_content_width().max(min_content);
        if let Some(width) = intrinsic::intrinsic_content_box_width_keyword(
            table_root_inline_size(table_style),
            content_box_pt(min_content),
            content_box_pt(max_content),
            layout_pt(available_outer_inline),
            inline_non_content,
        ) {
            content_inline = constrain_table_root_inline_size(
                table_style,
                width,
                PercentageBasis::definite(content_box_pt(available_outer_inline)),
                inline_non_content,
            )
            .content_box_length();
        }
        let content_inline = table_content_width_clamped_to_min_content(
            table_style,
            LogicalInlineContentSize::new(content_inline),
            LogicalInlineContentSize::new(content_box_pt(min_content)),
        );
        table_geometry.set_grid_inline(content_inline);
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_column_plan(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        columns: &[TableColumn<'_>],
        table_width: LogicalInlineContentSize,
        distribute_extra_width: bool,
        table_cellpadding: Option<f32>,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> TableColumnPlan {
        let table_inline_size = table_width;
        let table_width = table_inline_size.points();
        let column_count = grid.column_count;
        // CSS 2.2 only defines the fixed table-layout algorithm when the
        // table's inline size is not `auto`. An auto-width table keeps the
        // automatic track-measurement path, including legacy HTML column and
        // cell width constraints.
        // <https://www.w3.org/TR/CSS22/tables.html#fixed-table-layout>
        if table_style.table_layout == TableLayout::Fixed
            && !table_root_inline_size(table_style).is_auto()
        {
            return self.fixed_table_column_plan(
                rows,
                grid,
                table_style,
                stylesheets,
                columns,
                table_inline_size,
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
            widths.into_iter().map(TableGridLength::new).collect(),
            TableGridLength::new(table_metrics.spacing.horizontal.length_points()),
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
        stylesheets: &Stylesheets<'_>,
        columns: &[TableColumn<'_>],
        table_width: LogicalInlineContentSize,
        distribute_extra_width: bool,
        table_cellpadding: Option<f32>,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        column_count: usize,
    ) -> TableColumnPlan {
        let table_width = table_width.points();
        let table_inline_track = TableInlineTrackSizing::for_table(table_style);
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
            TableGridLength::new((table_width - total_horizontal_spacing).max(0.0));
        let mut widths = vec![TableGridLength::new(0.0); column_count];
        let mut declared = vec![false; column_count];
        let mut column_index = 0;
        for column in columns {
            if column_index >= column_count {
                break;
            }
            let span = column.span.min(column_count - column_index).max(1);
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            let column_flow_style = table_internal_flow_style(&column_style, table_style);
            if let Some(width) =
                declared_table_column_track_size(table_inline_track, &column_flow_style)
            {
                let width = constrain_declared_table_track_size(
                    table_inline_track,
                    &column_flow_style,
                    width,
                    content_box_pt(content_table_width.get()),
                );
                // Fixed-table column distribution is scalar coordinate
                // arithmetic over the table grid.
                distribute_fixed_width(
                    &mut widths,
                    &mut declared,
                    column_index,
                    span,
                    TableGridLength::new(width.points()),
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
                    PercentageBasis::definite(LogicalInlineContentSize::new(content_box_pt(
                        content_table_width.get(),
                    ))),
                );
                if let Some(explicit_width) = cell.element.and_then(|element| {
                    declared_table_cell_track_size(table_inline_track, element, &cell_style)
                }) {
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
                    let width = TableGridLength::new(
                        declared_table_cell_track_border_box_size(
                            table_inline_track,
                            &cell_style,
                            explicit_width,
                            content_table_width.get(),
                            border_insets,
                        )
                        .points(),
                    );
                    let width = if colspan > 1 {
                        let end = (placement.column + colspan).min(collapsed_columns.len());
                        let internal_spacing = table_internal_horizontal_spacing(
                            placement.column,
                            end,
                            &collapsed_columns,
                            table_metrics.clone(),
                        );
                        (width - TableGridLength::new(internal_spacing))
                            .max(TableGridLength::new(0.0))
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

        let used_width = widths
            .iter()
            .copied()
            .fold(TableGridLength::new(0.0), |sum, width| sum + width);
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
            let extra = TableGridLength::new(remaining.get() / receivers.len() as f32);
            for index in receivers {
                widths[index] += extra;
            }
        }

        TableColumnPlan::with_collapsed(
            widths,
            TableGridLength::new(table_metrics.spacing.horizontal.length_points()),
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
        stylesheets: &Stylesheets<'_>,
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
        &mut self,
        source: &ComputedStyle,
    ) -> TableUsedStyle {
        let used = self.style_with_current_used_lengths(source);
        TableUsedStyle::from_source_and_normalized(source.clone(), used)
    }

    pub(in crate::layout::table) fn style_for_table_row(
        &mut self,
        row: &TableRow<'_>,
        table_style: &(impl TableStyleSource + ?Sized),
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
            style.background.background_color = css::BackgroundColor::TRANSPARENT;
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
        stylesheets: &Stylesheets<'_>,
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
        overflow_clip: Option<OverflowClip>,
        ancestors: Vec<ElementSignature>,
        definite_block_size: BlockSizePercentageBasis,
    ) -> TableCellContentScope {
        let scope = TableCellContentScope {
            content_left: self.content_left,
            content_right: self.content_right,
            table_cell_content_coordinate_contexts: self
                .table_cell_content_coordinate_contexts
                .clone(),
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
            if WritingModeAxes::new(cell_style.writing_mode, cell_style.used_direction())
                .swaps_physical_axes()
            {
                content_height
            } else {
                content_width
            };
        self.content_left = content_box.left();
        self.content_right = content_box.right();
        self.table_cell_content_coordinate_contexts
            .push(TableCellContentCoordinateContext {
                origin: PageTopPoint::new(content_box.left(), content_box.top_y()),
                writing_mode: cell_style.writing_mode,
                direction: cell_style.used_direction(),
                overflow_clip,
            });
        self.cursor_y = content_box.top_y();
        self.ancestors = ancestors;
        self.containing_block_direction = cell_style.used_direction();
        self.containing_block_writing_mode = cell_style.writing_mode;
        self.content_logical_inline_size_stack
            .push(content_logical_inline_size.max(1.0));
        let inherited_orthogonal_available_height = self
            .current_child_available_space()
            .orthogonal_available_height;
        self.child_available_space_stack
            .push(child_available_space_for_formatting_context(
                cell_style,
                PhysicalContentWidth::new(content_box_pt(content_width)),
                Some(DefinitePhysicalContentHeight::new(
                    PhysicalContentHeight::new(content_box_pt(content_height)),
                )),
                inherited_orthogonal_available_height,
                PhysicalContentHeight::new(content_box_pt(content_height)),
            ));
        self.definite_block_size_stack.push(definite_block_size);
        scope
    }

    pub(in crate::layout::table) fn enter_table_cell_content_scope_for_rect(
        &mut self,
        cell_style: &ComputedStyle,
        content_rect: PageTopRect,
        overflow_clip: Option<OverflowClip>,
        ancestors: Vec<ElementSignature>,
        definite_block_size: Option<f32>,
    ) -> TableCellContentScope {
        self.enter_table_cell_content_scope(
            cell_style,
            TableCellContentBox::from_page_top_rect(content_rect),
            overflow_clip,
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
        self.table_cell_content_coordinate_contexts = scope.table_cell_content_coordinate_contexts;
        self.cursor_y = scope.cursor_y;
        self.ancestors = scope.ancestors;
        self.containing_block_direction = scope.containing_block_direction;
        self.containing_block_writing_mode = scope.containing_block_writing_mode;
        self.content_logical_inline_size_stack = scope.content_logical_inline_size_stack;
        self.child_available_space_stack = scope.child_available_space_stack;
        self.definite_block_size_stack = scope.definite_block_size_stack;
    }

    /// Whether an enclosing table cell clips descendant visual overflow in the
    /// physical block axis used by page and column fragmentation.
    pub(in crate::layout) fn table_cell_context_clips_block_fragmentation(&self) -> bool {
        self.table_cell_content_coordinate_contexts
            .iter()
            .any(|context| context.overflow_clip.is_some_and(|clip| clip.clips_y))
    }

    /// Resolve table-cell content alignment as a distance toward the cell's
    /// logical block end.
    ///
    /// The caller projects this displacement through `TableCellAxisAdapter`.
    /// Keeping it logical prevents a vertical cell's legacy `vertical-align`
    /// from being treated as a physical-Y adjustment merely because its table
    /// root happens to be horizontal:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn table_cell_content_block_offset(
        &self,
        cell_style: &ComputedStyle,
        content_geometry: TableCellContentGeometry,
        subject_block_size: f32,
        row_baseline_offset: Option<f32>,
        cell_baseline_offset: f32,
    ) -> f32 {
        let free_space = content_geometry.block_size().points() - subject_block_size;
        if cell_style.align_content.keyword == ContentAlignmentKeyword::Normal {
            let extra = free_space.max(0.0);
            return match cell_style.vertical_align.table_cell_align {
                TableCellVerticalAlign::Top => 0.0,
                TableCellVerticalAlign::Middle => extra / 2.0,
                TableCellVerticalAlign::Bottom => extra,
                TableCellVerticalAlign::Baseline => row_baseline_offset
                    .map(|baseline| (baseline - cell_baseline_offset).max(0.0))
                    .unwrap_or(0.0)
                    .min(extra),
            };
        }
        if matches!(
            cell_style.align_content.keyword,
            ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline
        ) && let Some(baseline) = row_baseline_offset
        {
            return (baseline - cell_baseline_offset).max(0.0);
        }
        content_alignment_offset_toward_end(
            cell_style.align_content,
            free_space,
            block_align_content_defaults_to_safe_overflow(cell_style),
        )
    }

    pub(in crate::layout::table) fn table_cell_content_alignment_subject_width(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        border_insets: css::Edges,
        available_block_width: f32,
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
        if !WritingModeAxes::new(cell_style.writing_mode, cell_style.used_direction())
            .swaps_physical_axes()
        {
            return fallback_width;
        }

        // The alignment subject is the fragment produced with the same
        // definite logical inline constraint used for the cell's intrinsic
        // table measure. For a vertical cell that constraint is physical
        // height, so `max-height` can wrap text into several physical-width
        // columns before `vertical-align` distributes the remaining block
        // space. Measuring it unconstrained would center one column inside a
        // multi-column fragment.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
        let available_inline_size = table_cell_inline_intrinsic_measure(cell_style)
            .map(LogicalInlineContentSize::points)
            .unwrap_or(f32::MAX);
        let inline_measurement = if let Some(children) = cell.children.as_deref() {
            Some(self.intrinsic_inline_measurement_for_boxes(
                children,
                cell_style,
                stylesheets,
                available_inline_size,
            ))
        } else {
            cell.element.map(|element| {
                self.intrinsic_inline_measurement_for_element(
                    element,
                    cell_style,
                    stylesheets,
                    None,
                    available_inline_size,
                )
            })
        };
        let inline_subject_width = inline_measurement
            // In a vertical cell, table-cell `vertical-align` distributes
            // content along the cell's logical block axis. Its subject is the
            // selected line stack's logical block span, not the inline
            // progression extent contributed by `line-height`.
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
            // <https://www.w3.org/TR/CSS22/tables.html#height-layout>
            .map(|measurement| measurement.logical_block_span(cell_style))
            .unwrap_or(0.0);
        let block_subject_width = cell
            .children
            .as_deref()
            .map(|children| {
                self.table_cell_block_children_alignment_subject_width(
                    children,
                    available_block_width,
                )
            })
            .unwrap_or(0.0);
        let measured_subject_width = inline_subject_width.max(block_subject_width);
        if measured_subject_width > 0.0 {
            measured_subject_width
        } else {
            fallback_width
        }
    }

    /// Return the physical-width contribution of in-flow block descendants.
    ///
    /// A vertical table cell aligns content on its logical block axis, which
    /// is physical width. Inline intrinsic measurement deliberately omits
    /// block children, so using only it makes a `width: 40pt` child appear to
    /// be no wider than a `width: 20pt` cell and incorrectly suppresses
    /// unsafe overflow alignment. This measures the same used horizontal box
    /// geometry that the later block-child pass paints.
    fn table_cell_block_children_alignment_subject_width(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        containing_width: f32,
    ) -> f32 {
        children
            .iter()
            .map(|child| {
                self.table_cell_block_child_alignment_subject_width(child, containing_width)
            })
            .fold(0.0, f32::max)
    }

    fn table_cell_block_child_alignment_subject_width(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        containing_width: f32,
    ) -> f32 {
        match child {
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .table_cell_block_children_alignment_subject_width(
                    &box_.children,
                    containing_width,
                ),
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .table_cell_block_children_alignment_subject_width(
                    &box_.core.children,
                    containing_width,
                ),
            box_tree::FormattingBox::Text(_) | box_tree::FormattingBox::Inline(_) => 0.0,
            _ if !table_cell_has_in_flow_layout_child(child) => 0.0,
            _ => {
                let mut style = self.style_with_current_used_lengths(child.style());
                let percentage_basis = PercentageBasis::definite(layout_pt(containing_width));
                let metrics = apply_used_box_metrics(&mut style, percentage_basis);
                let horizontal_non_content = metrics.horizontal_non_content_length();
                let available_outer_width =
                    normal_flow_block_available_outer_width(&style, layout_pt(containing_width));
                let content_width =
                    used_content_box_width(&style, available_outer_width, horizontal_non_content);
                content_width.points()
                    + horizontal_non_content.points()
                    + metrics.margin.left.points()
                    + metrics.margin.right.points()
            }
        }
    }

    pub(in crate::layout::table) fn layout_table_cell_replaced_children(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        content_box: TableCellContentBox,
    ) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_track_measure_keeps_intrinsic_minimum_above_a_zero_preference() {
        let measure = TableTrackMeasure {
            min_content: 12.0,
            max_content: 20.0,
            declared_non_percentage_minimum: 0.0,
            percentage: 0.0,
        };
        assert_eq!(measure.min_target(), 12.0);
        assert_eq!(measure.max_target(), 20.0);
    }

    #[test]
    fn table_track_measure_includes_a_definite_cell_width_in_its_minimum() {
        let measure = TableTrackMeasure {
            min_content: 12.0,
            max_content: 20.0,
            declared_non_percentage_minimum: 16.0,
            percentage: 0.5,
        };
        assert_eq!(measure.min_target(), 16.0);
        assert_eq!(measure.max_target(), 20.0);
        assert_eq!(measure.percentage, 0.5);
    }

    #[test]
    fn fixed_table_layout_with_auto_width_uses_automatic_tracks() {
        let mut style = ComputedStyle::initial();
        style.table_layout = TableLayout::Fixed;
        assert!(!fixed_table_layout_algorithm_applies(&style));

        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(100.0),
        );
        assert!(fixed_table_layout_algorithm_applies(&style));
    }
}
