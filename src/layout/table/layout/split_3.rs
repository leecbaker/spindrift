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

/// A horizontal table wrapper's physical inline offset within its active
/// content column.
///
/// Page-area origins are stable across synthetic multicolumn fragmentainers,
/// whereas the content-column origin changes. Retaining only this signed local
/// offset prevents continuation painting from accidentally anchoring to the
/// page area instead of its destination column.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct HorizontalTableContinuationInlineOffset(f32);

impl HorizontalTableContinuationInlineOffset {
    pub(in crate::layout::table) fn capture(table_x: f32, content_left: f32) -> Self {
        Self(table_x - content_left)
    }

    pub(in crate::layout::table) fn resolve(self, content_left: f32) -> f32 {
        content_left + self.0
    }
}

/// The physical page origin selected for a complete destination cell grid.
///
/// Unlike a fragmentainer edge, this is the top-left corner of the grid's
/// whole projected rectangle.  In particular, a `vertical-rl` continuation
/// must subtract the complete grid block extent from its logical block-start
/// edge before it can be handed to [`TableGridPlacement`].  Keeping this
/// conversion in one named adapter prevents row paint, structural paint, and
/// wrapper decoration from independently treating `content_left` as a table
/// origin.
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableFragmentainerGridOrigin(PageTopPoint);

impl TableFragmentainerGridOrigin {
    /// Construct the first grid frame from the wrapper position resolved by
    /// normal flow. Subsequent frames use [`Self::for_continuation`], whose
    /// fragmentainer-edge projection is intentionally different for vertical
    /// tables.
    ///
    /// <https://www.w3.org/TR/css-tables-3/#table-layout>
    /// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
    pub(in crate::layout::table) fn for_initial(
        table_x: f32,
        inline_top: PageTopBlockPosition,
    ) -> Self {
        Self(PageTopPoint::new(table_x, inline_top.points()))
    }

    fn for_continuation(
        style: &ComputedStyle,
        content_left: f32,
        content_right: f32,
        horizontal_inline_offset: HorizontalTableContinuationInlineOffset,
        cell_grid_block_extent: TableGridLength,
        inline_top: PageTopBlockPosition,
    ) -> Self {
        let x = match style.writing_mode {
            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                content_right - cell_grid_block_extent.get()
            }
            WritingMode::VerticalLr | WritingMode::SidewaysLr => content_left,
            WritingMode::HorizontalTb => horizontal_inline_offset.resolve(content_left),
        };
        Self(PageTopPoint::new(x, inline_top.points()))
    }

    pub(in crate::layout::table) fn x(self) -> f32 {
        self.0.x()
    }

    pub(in crate::layout::table) fn page_top_point(self) -> PageTopPoint {
        self.0
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
    pub(in crate::layout::table) continuation_inline_offset:
        HorizontalTableContinuationInlineOffset,
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
    pub(in crate::layout::table) fn rebase_destination_grid_to_fragmentainer(
        &mut self,
        style: &ComputedStyle,
        content_left: f32,
        content_right: f32,
    ) {
        // A continuation owns the entire destination cell grid, not merely
        // the physical X coordinate of its fragmentainer. In a vertical-RL
        // root `TableGridPlacement::origin` is the physical *left* edge of
        // that complete grid, while logical block-start is the fragmentainer
        // content-right edge. Reusing `content_left` here makes every row
        // project to the right of its new column. Vertical-LR begins at the
        // physical left edge; horizontal roots retain their inline offset.
        let cell_grid_block_extent = TableGridLength::new(table_grid_height(
            self.planned_row_heights,
            self.planned_row_occupancy,
            self.table_metrics.clone(),
        ));
        self.table_x = TableFragmentainerGridOrigin::for_continuation(
            style,
            content_left,
            content_right,
            self.continuation_inline_offset,
            cell_grid_block_extent,
            self.table_inline_origin,
        )
        .x();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_continuation_offset_preserves_local_inline_placement() {
        let offset = HorizontalTableContinuationInlineOffset::capture(28.0, 20.0);

        // A page continuation can retain the same active content origin.
        let same_page_x = offset.resolve(20.0);
        assert_eq!(same_page_x, 28.0);
        // A multicolumn continuation must use its new column origin.
        assert_eq!(offset.resolve(120.0), 128.0);
    }

    #[test]
    fn continuation_grid_origin_uses_the_logical_block_start_edge() {
        let offset = HorizontalTableContinuationInlineOffset::capture(28.0, 20.0);
        let extent = TableGridLength::new(255.0);
        let inline_top = PageTopBlockPosition::new(400.0);
        let style = |writing_mode| ComputedStyle {
            writing_mode,
            ..ComputedStyle::initial()
        };

        assert_eq!(
            TableFragmentainerGridOrigin::for_continuation(
                &style(WritingMode::HorizontalTb),
                120.0,
                220.0,
                offset,
                extent,
                inline_top,
            )
            .x(),
            128.0
        );
        assert_eq!(
            TableFragmentainerGridOrigin::for_continuation(
                &style(WritingMode::VerticalLr),
                120.0,
                220.0,
                offset,
                extent,
                inline_top,
            )
            .x(),
            120.0
        );
        assert_eq!(
            TableFragmentainerGridOrigin::for_continuation(
                &style(WritingMode::VerticalRl),
                120.0,
                220.0,
                offset,
                extent,
                inline_top,
            )
            .x(),
            -35.0
        );
    }
}
