use super::*;

mod empty_table;
mod rows;
mod split_1;
mod split_2;
mod split_4;
mod split_5;
mod split_6;
mod split_7;
mod split_8;
mod split_9;

pub(in crate::layout::table) struct TableBodyRowsInput<'table, 'ctx> {
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) rows: &'ctx [TableRow<'table>],
    pub(in crate::layout::table) grid: &'ctx TableGrid,
    pub(in crate::layout::table) columns: &'ctx [TableColumn<'table>],
    pub(in crate::layout::table) style: &'ctx ComputedStyle,
    pub(in crate::layout::table) stylesheets: &'ctx [Stylesheet],
    pub(in crate::layout::table) table_x: f32,
    pub(in crate::layout::table) used_table_width: f32,
    pub(in crate::layout::table) table_cellpadding: Option<f32>,
    pub(in crate::layout::table) column_plan: &'ctx TableColumnPlan,
    pub(in crate::layout::table) planned_row_heights: &'ctx [f32],
    pub(in crate::layout::table) planned_row_occupancy: &'ctx [bool],
    pub(in crate::layout::table) table_height_is_definite: bool,
    pub(in crate::layout::table) table_width: UsedTableWidth,
    pub(in crate::layout::table) table_metrics: TableMetrics,
    pub(in crate::layout::table) collapsed_geometry: Option<&'ctx CollapsedTableGeometry>,
    pub(in crate::layout::table) table_is_document_canvas: bool,
    pub(in crate::layout::table) repeating_header_rows: &'ctx [usize],
    pub(in crate::layout::table) repeating_footer_rows: &'ctx [usize],
    pub(in crate::layout::table) repeating_header_height: f32,
    pub(in crate::layout::table) repeating_footer_height: f32,
    pub(in crate::layout::table) avoid_break_row_groups: &'ctx [TableAvoidRowGroup],
    pub(in crate::layout::table) row_group_break_before: &'ctx [PageBreak],
    pub(in crate::layout::table) row_group_break_after: &'ctx [PageBreak],
}

pub(in crate::layout::table) struct TableBodyFragmentCommitContext<'table, 'ctx> {
    pub(in crate::layout::table) rows: &'ctx [TableRow<'table>],
    pub(in crate::layout::table) grid: &'ctx TableGrid,
    pub(in crate::layout::table) columns: &'ctx [TableColumn<'table>],
    pub(in crate::layout::table) style: &'ctx ComputedStyle,
    pub(in crate::layout::table) stylesheets: &'ctx [Stylesheet],
    pub(in crate::layout::table) table_x: f32,
    pub(in crate::layout::table) used_table_width: f32,
    pub(in crate::layout::table) table_cellpadding: Option<f32>,
    pub(in crate::layout::table) column_plan: &'ctx TableColumnPlan,
    pub(in crate::layout::table) planned_row_heights: &'ctx [f32],
    pub(in crate::layout::table) planned_row_occupancy: &'ctx [bool],
    pub(in crate::layout::table) table_width: UsedTableWidth,
    pub(in crate::layout::table) table_metrics: TableMetrics,
    pub(in crate::layout::table) collapsed_geometry: Option<&'ctx CollapsedTableGeometry>,
    pub(in crate::layout::table) table_is_document_canvas: bool,
    pub(in crate::layout::table) repeating_header_rows: &'ctx [usize],
    pub(in crate::layout::table) repeating_footer_rows: &'ctx [usize],
}
