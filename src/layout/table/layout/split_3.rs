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

/// Return the decoration style for a table cell in the collapsed-border model.
///
/// Collapsed grid borders paint in a separate, later table-border phase. A
/// cell background with the initial `border-box` clip must therefore stop at
/// the cell padding edge: otherwise the background occupies the half-rule
/// area that belongs to a neighbouring collapsed border. This also keeps the
/// colour and image layers aligned when a winning rule has unequal widths on
/// its two sides.
///
/// <https://drafts.csswg.org/css-tables-3/#collapsed-borders>
fn collapsed_cell_decoration_style(style: &ComputedStyle, collapsed: bool) -> ComputedStyle {
    let mut decoration_style = style.clone();
    if !collapsed {
        return decoration_style;
    }

    if decoration_style.background.background_clip == css::BackgroundBox::Border {
        decoration_style.background.background_clip = css::BackgroundBox::Padding;
    }
    for layer in &mut decoration_style.background.background_layers {
        if layer.clip == css::BackgroundBox::Border {
            layer.clip = css::BackgroundBox::Padding;
        }
    }
    decoration_style
}

/// Whether source-logical column order runs opposite the final page paint
/// order.  Structural column backgrounds are a single table painting layer;
/// using this at that layer keeps adjacent opaque spans from taking ownership
/// of each other's fractional device-pixel edge after a writing-mode
/// projection.
///
/// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
fn table_columns_paint_in_reverse_page_order(style: &ComputedStyle) -> bool {
    matches!(
        WritingModeAxes::new(style.writing_mode, style.used_direction())
            .physical_side(LogicalSide::InlineStart),
        PhysicalSide::Right | PhysicalSide::Bottom
    )
}

/// Whether a column-group interval contains an explicit `col` layer that
/// must remain above the group background in CSS table paint order.
fn table_column_group_has_explicit_columns(
    columns: &[TableColumn<'_>],
    start_column: usize,
    end_column: usize,
    column_count: usize,
) -> bool {
    let mut column_index = 0;
    for column in columns {
        if column_index >= column_count {
            break;
        }
        let span = column.span.min(column_count - column_index).max(1);
        let column_end = column_index + span;
        let overlaps_group = column_index < end_column && column_end > start_column;
        let is_group_placeholder = column
            .group
            .as_ref()
            .is_some_and(|group| group.signature == column.signature);
        if overlaps_group && !is_group_placeholder {
            return true;
        }
        column_index = column_end;
    }
    false
}

pub(in crate::layout::table) struct TableBodyRowsInput<'table, 'ctx> {
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) rows: &'ctx [TableRow<'table>],
    pub(in crate::layout::table) grid: &'ctx TableGrid,
    pub(in crate::layout::table) columns: &'ctx [TableColumn<'table>],
    pub(in crate::layout::table) style: &'ctx ComputedStyle,
    pub(in crate::layout::table) stylesheets: &'ctx Stylesheets<'ctx>,
    pub(in crate::layout::table) table_x: f32,
    pub(in crate::layout::table) wrapper_table_x: PageInlinePosition,
    /// Immutable unfragmented table grid used as the source paint space.
    pub(in crate::layout::table) source_grid_placement: TableGridPlacement,
    /// Canonical grid-local source placement for the table root's own
    /// background positioning area. Structural row and column backgrounds
    /// retain `source_grid_placement` instead.
    pub(in crate::layout::table) root_background_source_grid_placement: TableGridPlacement,
    /// First destination grid placement after wrapper-owned top-caption
    /// progress. This can differ from the source placement when the caption
    /// crosses a page or column boundary.
    pub(in crate::layout::table) initial_destination_grid_placement: TableGridPlacement,
    /// Retained wrapper source/destination progress shared by every body
    /// fragment. This carries caption progress without making captions part
    /// of table-root background geometry.
    pub(in crate::layout::table) wrapper_timeline: TableWrapperFragmentTimeline,
    pub(in crate::layout::table) logical_inline_extent: LogicalInlineContentSize,
    pub(in crate::layout::table) physical_grid_width: PhysicalContentWidth,
    pub(in crate::layout::table) table_cellpadding: Option<f32>,
    pub(in crate::layout::table) column_plan: &'ctx TableColumnPlan,
    pub(in crate::layout::table) planned_row_heights: &'ctx [f32],
    pub(in crate::layout::table) source_row_heights: &'ctx [f32],
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

/// A table wrapper's physical X offset within its active content column.
///
/// Page-area origins are stable across synthetic multicolumn fragmentainers,
/// whereas the content-column origin changes. Retaining only this signed local
/// offset prevents continuation painting from accidentally anchoring to the
/// page area instead of its destination column.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableContinuationInlineOffset(f32);

impl TableContinuationInlineOffset {
    pub(in crate::layout::table) fn capture(table_x: f32, content_left: f32) -> Self {
        Self(table_x - content_left)
    }

    pub(in crate::layout::table) fn resolve(self, content_left: f32) -> f32 {
        content_left + self.0
    }
}

pub(in crate::layout::table) struct TableBodyFragmentCommitContext<'table, 'ctx> {
    pub(in crate::layout::table) rows: &'ctx [TableRow<'table>],
    pub(in crate::layout::table) grid: &'ctx TableGrid,
    pub(in crate::layout::table) columns: &'ctx [TableColumn<'table>],
    pub(in crate::layout::table) style: &'ctx ComputedStyle,
    pub(in crate::layout::table) stylesheets: &'ctx Stylesheets<'ctx>,
    pub(in crate::layout::table) table_x: f32,
    pub(in crate::layout::table) wrapper_table_x: PageInlinePosition,
    /// Physical inline origin of the table wrapper, retained when later body
    /// slices rebase their logical block coordinate into another fragmentainer.
    pub(in crate::layout::table) table_inline_origin: PageTopBlockPosition,
    pub(in crate::layout::table) continuation_inline_offset: TableContinuationInlineOffset,
    pub(in crate::layout::table) logical_inline_extent: LogicalInlineContentSize,
    pub(in crate::layout::table) physical_grid_width: PhysicalContentWidth,
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

impl TableBodyFragmentCommitContext<'_, '_> {
    pub(in crate::layout::table) fn rebase_to_content_left(&mut self, content_left: f32) {
        self.table_x = self.continuation_inline_offset.resolve(content_left);
    }
}

#[cfg(test)]
mod tests {
    use super::TableContinuationInlineOffset;

    #[test]
    fn table_continuation_offset_preserves_local_inline_placement() {
        let offset = TableContinuationInlineOffset::capture(28.0, 20.0);

        // A page continuation can retain the same active content origin.
        let same_page_x = offset.resolve(20.0);
        assert_eq!(same_page_x, 28.0);
        // A multicolumn continuation must use its new column origin.
        assert_eq!(offset.resolve(120.0), 128.0);
    }
}
