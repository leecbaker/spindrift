//! Shared table-cell layout state and sizing-policy types.

use super::*;
/// Structural table paint owned by a relatively positioned row or row group.
///
/// Table layout creates row and row-group backgrounds after row content has
/// been measured. Retaining those primitives with their originating style
/// lets finalization place them in the positioned auto stack rather than
/// flattening them into the table's in-flow background band.
/// <https://drafts.csswg.org/css-position-3/#relative-positioning>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct RelativeTablePartStructuralPaint {
    /// Unscaled source retained for paint identity and any deferred cascade
    /// reconstruction.
    pub(in crate::layout::table) source_style: ComputedStyle,
    /// Used style that selected the captured paint geometry.
    pub(in crate::layout::table) style: css::ZoomedLayoutStyle,
    pub(in crate::layout::table) bounds: PaintClip,
    pub(in crate::layout::table) primitives: Vec<PaintPrimitive>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableCellLayoutMetrics {
    pub(in crate::layout::table) content_height: f32,
    pub(in crate::layout::table) border_box_height: f32,
    pub(in crate::layout::table) baseline_offset: TableCellBaselineOffset,
}

pub(in crate::layout::table) struct PreparedTableCell {
    pub(in crate::layout::table) style: css::ZoomedLayoutStyle,
    pub(in crate::layout::table) row_sizing_style: ComputedStyle,
    pub(in crate::layout::table) area: TableGridArea,
    pub(in crate::layout::table) inline_bounds: TableInlineBounds,
    pub(in crate::layout::table) borders: css::Edges,
    pub(in crate::layout::table) metrics: TableCellLayoutMetrics,
    pub(in crate::layout::table) text: String,
}

impl PreparedTableCell {
    pub(in crate::layout::table) fn width(&self) -> f32 {
        self.inline_bounds.logical_size().get()
    }
}

/// The final coordinate context established by one table-cell content scope.
///
/// Inline atomic fragments can outlive the immediate cell-layout call. They
/// therefore must retain the cell's page origin and logical flow rather than
/// infer a sideways projection from a nesting counter during replay:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://drafts.csswg.org/css-tables-3/#table-cell-content-layout-second-pass>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct TableCellContentCoordinateContext {
    pub(in crate::layout) origin: PageTopPoint,
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
    /// The resolved cell-descendant clip visible to nested layout decisions.
    ///
    /// Table layout retains the actual PDF clip after child paint collection,
    /// but a nested formatting context still needs to know whether its source
    /// overflow is bounded in the table cell's physical block axis.
    pub(in crate::layout) overflow_clip: Option<OverflowClip>,
}

pub(in crate::layout::table) struct TableCellContentScope {
    pub(in crate::layout::table) content_left: f32,
    pub(in crate::layout::table) content_right: f32,
    pub(in crate::layout::table) table_cell_content_coordinate_contexts:
        Vec<TableCellContentCoordinateContext>,
    pub(in crate::layout::table) cursor_y: f32,
    pub(in crate::layout::table) ancestors: Vec<ElementSignature>,
    pub(in crate::layout::table) containing_block_direction: Direction,
    pub(in crate::layout::table) containing_block_writing_mode: WritingMode,
    pub(in crate::layout::table) content_logical_inline_size_stack: Vec<f32>,
    pub(in crate::layout::table) child_available_space_stack: Vec<ChildAvailableSpace>,
    pub(in crate::layout::table) block_percentage_context_stack: BlockPercentageContextStack,
}

pub(in crate::layout::table) struct TableGridLayoutContext<'table, 'ctx> {
    pub(in crate::layout::table) rows: &'ctx [TableRow<'table>],
    pub(in crate::layout::table) grid: &'ctx TableGrid,
    pub(in crate::layout::table) table_style: &'ctx TableUsedStyle,
    pub(in crate::layout::table) stylesheets: &'ctx Stylesheets<'ctx>,
    pub(in crate::layout::table) table_cellpadding: Option<TableCellPadding>,
    pub(in crate::layout::table) column_plan: &'ctx TableColumnPlan,
    pub(in crate::layout::table) table_metrics: TableMetrics,
    pub(in crate::layout::table) collapsed_geometry: Option<&'ctx CollapsedTableGeometry>,
    /// A flex/grid-assigned table-wrapper border-box block size. This is
    /// separate from the CSS `height` property, which sizes the table grid.
    /// <https://drafts.csswg.org/css-tables/#computing-the-table-height>
    pub(in crate::layout::table) wrapper_border_box_block_size: Option<BorderBoxLength>,
    /// A definite content-box block size resolved by absolute positioning.
    /// This is separate from the flex/grid wrapper override because an
    /// authored table block size targets the table grid and must be distributed
    /// to rows by the table algorithm.
    pub(in crate::layout::table) positioned_table_block_content_size:
        Option<LogicalBlockContentSize>,
    /// Top and bottom caption block sizes, excluded from the table grid when
    /// a flex/grid item supplies a wrapper size.
    pub(in crate::layout::table) wrapper_non_grid_block_size: LayoutLength,
}

pub(in crate::layout::table) struct TableCellBaselineAlignmentContext<'a> {
    pub(in crate::layout::table) row_index: usize,
    pub(in crate::layout::table) row_style: &'a ComputedStyle,
    pub(in crate::layout::table) table_style: &'a ComputedStyle,
    pub(in crate::layout::table) rows: &'a [TableRow<'a>],
    pub(in crate::layout::table) grid: &'a TableGrid,
    pub(in crate::layout::table) stylesheets: &'a Stylesheets<'a>,
    pub(in crate::layout::table) table_cellpadding: Option<TableCellPadding>,
    pub(in crate::layout::table) column_plan: &'a TableColumnPlan,
    pub(in crate::layout::table) planned_row_heights: &'a [f32],
    pub(in crate::layout::table) planned_row_occupancy: &'a [bool],
    pub(in crate::layout::table) table_metrics: TableMetrics,
    pub(in crate::layout::table) collapsed_geometry: Option<&'a CollapsedTableGeometry>,
    pub(in crate::layout::table) row_baseline_offset: Option<TableRowBaselineOffset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableCellBaselineSet {
    First,
    Last,
}

/// Table-cell content sizing mode for CSS Tables row layout.
///
/// CSS Tables 3 first measures row minimum heights with cell-percentage
/// dependent descendants treated as `auto`, then relays out cell content
/// against the final cell content box height:
/// <https://drafts.csswg.org/css-tables-3/#row-layout> and
/// <https://drafts.csswg.org/css-tables-3/#table-cell-content-relayout>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableCellContentSizingPolicy {
    RowMinimum,
    FinalRelayout,
}
