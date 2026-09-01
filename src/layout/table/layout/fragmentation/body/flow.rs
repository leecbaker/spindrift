//! Table body-row fragmentation and fragment transitions.

use crate::css::{
    self, ComputedStyle, PageBreak, SemanticLengthExt, Stylesheets, WritingMode, layout_pt,
};
use crate::layout::table::layout::fragmentation::{
    HorizontalTableContinuationInlineOffset, TableBodyFragmentCommitContext, TableBodyRowsInput,
    TableBodyRowsOutcome, TableRootFinalBodyFragment,
};
use crate::layout::table::layout::{
    CollapsedTableGeometry, PendingTableBreakCandidate,
    TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE, TableAvoidBreakCandidateState, TableAvoidRowGroup,
    TableAvoidRowGroupKeepState, TableAvoidRunBreakDecision, TableAvoidRunBreakInput,
    TableBodyPaintFragment, TableBreakCandidateMeta, TableForcedBreakCarryState,
    TableForcedBreakDecision, TableForcedBreakInput, TableFragmentBoundaryDecision,
    TableFragmentBreakReason, TableFragmentChromeContext, TableFragmentFooterAction,
    TableFragmentRepeatPolicy, TableFragmentStartDecision, TableFragmentTransitionDecision,
    TableFragmentTransitionInput, TableFragmentainerBlockStart, TableFragmentainerPlacement,
    TableNamedPageBreakDecision, TableNamedPageBreakInput, TableOuterFragmentainerPlacement,
    TableOversizedRowSliceDecision, TableOversizedRowSliceInput, TableRowFragmentDecision,
    TableRowFragmentMode, TableRowGroupAvoidDecision, TableRowGroupAvoidDecisionInput,
    TableRowGroupFragmentRequirement, TableRowOverflowBreakDecision, TableRowOverflowBreakInput,
    TableRowSourceFragment, TableWrapperFragmentChrome, TableWrapperFragmentTimeline,
    table_fragment_repeat_policy,
};
use crate::layout::table::{
    TableCellPadding, TableColumnPlan, TableGrid, TableGridPlacement, TableMetrics, TableRow,
    TableRowBaselineOffset, UsedTableWidth, table_row_group_end_indices, table_row_is_collapsed,
    table_vertical_edge_spacing,
};
use crate::layout::{
    AssignmentPlacement, FlowAxes, FragmentBreakOpportunity, FragmentainerAdvance,
    FragmentainerKind, LayoutBuilder, LogicalBlockContentSize, PageInlinePosition,
    PageTopBlockPosition, PageTopPoint, PageTopRect, ResolvedPageBoundaryValues,
    html_table_rowspan, page_value_sources_from_style_and_children,
    resolved_page_boundary_values_from_style_and_children, style_is_in_normal_flow,
};
use crate::units::{NonContentLength, content_box_pt, non_content_pt};

/// Geometry consumed at the start of one destination table fragment.
///
/// A table's page transition bypasses ordinary block-flow re-entry, so the
/// table owns this fragment-local placement explicitly. In particular,
/// separated-border leading edge spacing belongs to the grid fragment that
/// receives an occupied source row. A continuation therefore retains that
/// edge before replaying its row or repeated header chrome.
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy)]
struct TableFragmentStartPlacement {
    fragmentainer: TableFragmentainerPlacement,
    wrapper_block_start: NonContentLength,
    leading_grid_edge_spacing: NonContentLength,
    has_repeated_header: bool,
}

impl TableFragmentStartPlacement {
    fn for_destination(
        context: &TableBodyFragmentCommitContext<'_, '_>,
        start: TableFragmentStartDecision,
        fragmentainer: TableFragmentainerPlacement,
        source_row_has_occupied_cell: bool,
    ) -> Self {
        let repeated_header_rows = start.repeated_header_rows(context.repeating_header_rows);
        let has_repeated_header = !repeated_header_rows.is_empty();
        debug_assert!(
            fragmentainer.paint_top().points().is_finite()
                && fragmentainer.block_start().points().is_finite(),
            "a destination table fragmentainer must have finite physical and logical origins"
        );
        Self {
            fragmentainer,
            wrapper_block_start: TableWrapperFragmentChrome::for_table(
                context.style,
                context.table_width,
            )
            .continuation_block_start(),
            leading_grid_edge_spacing: source_row_has_occupied_cell
                .then(|| {
                    LayoutBuilder::table_logical_block_edge_spacing(
                        context.style,
                        context.planned_row_occupancy,
                        &context.table_metrics,
                    )
                })
                .map(non_content_pt)
                .unwrap_or_else(|| non_content_pt(0.0)),
            has_repeated_header,
        }
    }

    fn cursor_for_start(self) -> f32 {
        // A separated grid exposes its edge before every materialized table
        // fragment that owns an occupied source row. Wrapper decoration is
        // independently added only for `box-decoration-break: clone`.
        // <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
        self.fragmentainer.block_start().points()
            - self.leading_grid_edge_spacing.points()
            - if self.has_repeated_header {
                0.0
            } else {
                self.wrapper_block_start.points()
            }
    }

    fn fragmentainer(self) -> TableFragmentainerPlacement {
        // The destination cell grid is derived from the fragmentainer's
        // wrapper grid exactly once.  Applying this continuation-only inset
        // here used to shift every separated-border continuation a second
        // time, after `TableGridFrames` had already removed the outer edge.
        self.fragmentainer
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Materialize the source fragmentainers skipped by a wrapper sibling.
    ///
    /// A vertical caption may be retained as one source paint fragment while
    /// still consuming several outer multicolumn intervals.  Its following
    /// grid must be recorded in the ordinal selected by that wrapper
    /// progress; merely storing the ordinal on a table placement leaves the
    /// grid on an earlier scratch page, where parent replay exposes it in a
    /// column already consumed by the caption.
    ///
    /// This is the one bridge from a table-facing outer placement to the
    /// scratch `PageContext` backend.  The sequence owns the ordinal; table
    /// grid axes never participate in selecting it.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    /// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
    fn materialize_table_outer_fragmentainer_placement(
        &mut self,
        placement: TableFragmentainerPlacement,
    ) {
        let Some(outer) = placement.outer_fragmentainer() else {
            return;
        };
        let target_ordinal = outer.ordinal();
        debug_assert!(
            self.pages.len() <= target_ordinal,
            "table wrapper continuation cannot select an already-materialized outer fragmentainer"
        );
        while self.pages.len() < target_ordinal {
            self.materialize_column_continuation();
        }
        debug_assert_eq!(self.pages.len(), target_ordinal);
    }

    /// The active outer column placement, if table layout is running in an
    /// anonymous multicol fragmentainer.
    pub(in crate::layout::table) fn active_table_fragmentainer_placement(
        &self,
    ) -> Option<TableOuterFragmentainerPlacement> {
        self.fragmentainer_override
            .filter(|override_| override_.kind == FragmentainerKind::Column)
            .map(|override_| {
                TableOuterFragmentainerPlacement::from_outer(
                    override_.sequence.current_placement(self.pages.len()),
                )
            })
    }

    /// Return the block-flow axes of the context that owns this table's
    /// fragmentainers.
    ///
    /// A table grid can be orthogonal to its multicol container, but CSS
    /// Fragmentation still advances that table through the container's one
    /// fragmentainer sequence. Table-specific axes remain reserved for grid
    /// geometry and paint projection.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    fn table_fragmentainer_flow_axes(&self, table_style: &ComputedStyle) -> FlowAxes {
        self.active_table_fragmentainer_placement()
            .map(TableOuterFragmentainerPlacement::axes)
            .unwrap_or_else(|| FlowAxes::for_style(table_style))
    }

    /// Return the table's logical block-axis size in the current fragmentainer.
    ///
    /// A vertical table fragments along physical X.  `page_area_height()` is
    /// therefore the wrong capacity for it: in a multicolumn fragmentainer it
    /// describes the table's logical inline measure, not the available row
    /// track span.  Keep this conversion at the table fragmentation boundary
    /// so row decisions continue to use one logical block coordinate system:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout::table) fn table_fragmentainer_block_size(
        &self,
        style: &ComputedStyle,
    ) -> f32 {
        self.active_table_fragmentainer_placement()
            .map(TableOuterFragmentainerPlacement::logical_block_capacity)
            .unwrap_or_else(|| {
                if self
                    .table_fragmentainer_flow_axes(style)
                    .writing_mode()
                    .has_vertical_lines()
                {
                    (self.content_right - self.content_left).max(0.0)
                } else {
                    self.page_area_height()
                }
            })
    }

    /// Return the scalar origin of the current fragmentainer's logical block
    /// axis.  The scalar decreases toward block-end for every writing mode so
    /// it can be consumed by [`PageTopBlockPosition`] without exposing a
    /// physical-X special case to break decisions.
    pub(in crate::layout::table) fn table_fragmentainer_block_start(
        &self,
        style: &ComputedStyle,
    ) -> f32 {
        if let Some(placement) = self.active_table_fragmentainer_placement() {
            return placement.block_start().points();
        }
        match self.table_fragmentainer_flow_axes(style).writing_mode() {
            WritingMode::VerticalRl | WritingMode::SidewaysRl => self.content_right,
            WritingMode::VerticalLr | WritingMode::SidewaysLr => -self.content_left,
            WritingMode::HorizontalTb => self.current_page_context.top(),
        }
    }

    fn table_fragmentainer_block_end(&self, style: &ComputedStyle) -> f32 {
        if let Some(placement) = self.active_table_fragmentainer_placement() {
            return placement.block_end().points();
        }
        match self.table_fragmentainer_flow_axes(style).writing_mode() {
            WritingMode::VerticalRl | WritingMode::SidewaysRl => self.content_left,
            WritingMode::VerticalLr | WritingMode::SidewaysLr => -self.content_right,
            WritingMode::HorizontalTb => self.page_bottom(),
        }
    }

    /// Capture the destination geometry before the fragment's first row,
    /// repeated header, or wrapper paint is replayed. The physical table X and
    /// logical block origin belong to the fragmentainer, not to the source
    /// row that happens to start it.
    pub(in crate::layout::table) fn table_fragmentainer_placement(
        &self,
        style: &ComputedStyle,
        table_x: f32,
        wrapper_table_x: PageInlinePosition,
        paint_top: f32,
    ) -> TableFragmentainerPlacement {
        let outer_fragmentainer = self.active_table_fragmentainer_placement();
        let table_x = PageInlinePosition::new(table_x);
        let paint_top = PageTopBlockPosition::new(paint_top);
        let block_span = LogicalBlockContentSize::new(content_box_pt(
            self.table_fragmentainer_block_size(style),
        ));
        let placement = match self.table_fragmentainer_flow_axes(style).writing_mode() {
            WritingMode::HorizontalTb => {
                TableFragmentainerPlacement::horizontal(table_x, paint_top, block_span)
            }
            WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                TableFragmentainerPlacement::vertical_lr(
                    table_x,
                    paint_top,
                    TableFragmentainerBlockStart::new(self.table_fragmentainer_block_start(style)),
                    block_span,
                )
            }
            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                TableFragmentainerPlacement::vertical_rl(
                    table_x,
                    paint_top,
                    TableFragmentainerBlockStart::new(self.table_fragmentainer_block_start(style)),
                    block_span,
                )
            }
        };
        placement
            .with_wrapper_table_x(wrapper_table_x)
            .with_outer_fragmentainer(outer_fragmentainer)
    }

    fn table_block_cursor_is_at_start(
        &self,
        cursor: f32,
        style: &ComputedStyle,
        source_leading_grid_edge_spacing: f32,
    ) -> bool {
        // The first source fragment consumes its leading grid edge before its
        // first row. Continuations start directly at their fragmentainer
        // boundary. Both positions are legal no-progress boundaries.
        // <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
        let fragment_start = self.table_fragmentainer_block_start(style);
        (cursor - fragment_start).abs() <= 0.01
            || (source_leading_grid_edge_spacing > 0.0
                && (cursor - (fragment_start - source_leading_grid_edge_spacing)).abs() <= 0.01)
    }

    /// Synchronize the row-fragmentation cursor after entering a destination
    /// fragmentainer. Horizontal table chrome consumes `cursor_y` while it is
    /// replayed; vertical tables instead use their independent physical-X
    /// block coordinate.
    fn table_block_cursor_after_fragment_start(
        &self,
        style: &ComputedStyle,
        _source_leading_grid_edge_spacing: f32,
    ) -> f32 {
        if style.writing_mode.has_vertical_lines() {
            self.table_fragmentainer_block_start(style)
        } else {
            self.cursor_y
        }
    }

    fn table_logical_block_spacing(style: &ComputedStyle, table_metrics: &TableMetrics) -> f32 {
        if style.writing_mode.has_vertical_lines() {
            table_metrics.spacing.horizontal.length_points()
        } else {
            table_metrics.spacing.vertical.length_points()
        }
    }

    fn table_logical_block_edge_spacing(
        style: &ComputedStyle,
        row_occupancy: &[bool],
        table_metrics: &TableMetrics,
    ) -> f32 {
        if table_metrics.border_collapse == css::BorderCollapse::Separate
            && row_occupancy.iter().any(|occupied| *occupied)
        {
            Self::table_logical_block_spacing(style, table_metrics)
        } else {
            0.0
        }
    }

    /// Return wrapper chrome at the logical block end of this table root.
    ///
    /// Row-fit decisions consume logical block capacity. Using physical
    /// `bottom` unconditionally leaves the right table edge outside the last
    /// fragmentainer in `vertical-lr` (and the left edge in `vertical-rl`).
    /// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    fn table_logical_block_end_chrome(style: &ComputedStyle, table_width: UsedTableWidth) -> f32 {
        match style.writing_mode {
            WritingMode::HorizontalTb => {
                table_width.padding.bottom + table_width.border_widths.bottom
            }
            WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                table_width.padding.right + table_width.border_widths.right
            }
            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                table_width.padding.left + table_width.border_widths.left
            }
        }
    }

    pub(in crate::layout::table) fn layout_table_body_rows(
        &mut self,
        input: TableBodyRowsInput<'_, '_>,
    ) -> TableBodyRowsOutcome {
        let TableBodyRowsInput {
            fragmentainer_kind,
            rows,
            grid,
            columns,
            style,
            stylesheets,
            table_x,
            wrapper_table_x,
            source_grid_placement,
            root_background_source_grid_placement,
            initial_destination_grid_placement,
            initial_fragmentainer_placement,
            initial_grid_content_top,
            wrapper_timeline,
            logical_inline_extent,
            physical_grid_width,
            table_cellpadding,
            column_plan,
            planned_row_heights,
            source_row_heights,
            planned_row_occupancy,
            table_height_is_definite,
            table_width,
            table_metrics,
            collapsed_geometry,
            table_is_document_canvas,
            repeating_header_rows,
            repeating_footer_rows,
            repeating_header_height,
            repeating_footer_height,
            avoid_break_row_groups,
            row_group_break_before,
            row_group_break_after,
            ..
        } = input;
        if matches!(
            style.writing_mode,
            WritingMode::VerticalRl | WritingMode::SidewaysRl
        ) {
            self.materialize_table_outer_fragmentainer_placement(initial_fragmentainer_placement);
        }
        let physical_grid_width_points = physical_grid_width.points();
        // The source grid is a virtual unfragmented table coordinate system.
        // Its page-top position must not leak into the initial destination
        // fragmentainer after a top caption has fragmented. In vertical
        // writing modes this value is the physical inline-axis origin used by
        // the body fragment placement.
        let table_inline_origin = if style.writing_mode.has_vertical_lines() {
            initial_grid_content_top
        } else {
            PageTopBlockPosition::new(
                initial_destination_grid_placement
                    .full_page_top_rect()
                    .top_y(),
            )
        };
        debug_assert!(
            (logical_inline_extent.points() - column_plan.total_width().points()).abs() <= 0.01,
            "body rows must retain the column plan's logical inline extent"
        );
        let fragmentainer_uses_vertical_block_axis = self
            .table_fragmentainer_flow_axes(style)
            .writing_mode()
            .has_vertical_lines();
        // The wrapper selects the first destination placement after captions
        // have completed. Table-grid geometry must consume that placement;
        // recomputing a vertical origin from the full grid extent restarts it
        // at the opening column and loses wrapper-flow progress.
        let mut table_x = table_x;
        let continuation_inline_offset = HorizontalTableContinuationInlineOffset::capture(
            if style.writing_mode.has_vertical_lines() {
                table_x
            } else {
                wrapper_table_x.points()
            },
            self.content_left,
        );
        let mut table_body_fragment_started = false;
        let logical_block_edge_spacing =
            Self::table_logical_block_edge_spacing(style, planned_row_occupancy, &table_metrics);
        let mut current_fragment_repeat_policy = table_fragment_repeat_policy(
            layout_pt(0.01),
            layout_pt(self.table_fragmentainer_block_size(style)),
            layout_pt(0.0),
            layout_pt(repeating_footer_height),
            false,
            true,
        );
        let mut pending_fragment_start = TableFragmentStartDecision::new(
            TableFragmentBreakReason::TableStart,
            current_fragment_repeat_policy,
            false,
        );
        let mut pending_fragmentainer_placement = initial_fragmentainer_placement
            .with_destination_grid_origin(PageTopPoint::new(
                table_x,
                if fragmentainer_uses_vertical_block_axis {
                    table_inline_origin.points()
                } else {
                    self.cursor_y
                },
            ));
        // Caption layout belongs to the wrapper's source progression. The
        // first grid row must begin after that already-consumed interval in
        // the *outer* fragmentainer sequence, not at the table grid's local
        // block zero. This is most visible in vertical multicolumn layout,
        // where resetting to a physical page cursor replays the grid back
        // into columns consumed by the caption.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        // <https://drafts.csswg.org/css-tables-3/#table-root>
        let mut logical_block_cursor = if fragmentainer_uses_vertical_block_axis
            && pending_fragmentainer_placement
                .outer_fragmentainer()
                .is_some()
        {
            pending_fragmentainer_placement.block_start().points()
                - pending_fragmentainer_placement
                    .grid_block_progress(initial_destination_grid_placement)
                    .length()
                    .get()
                - logical_block_edge_spacing
        } else if fragmentainer_uses_vertical_block_axis {
            // A paged vertical table's grid position is table-local. It must
            // not be interpreted as progress already consumed in the page
            // fragmentainer; only an enclosing multicol placement carries
            // that wrapper-flow progression.
            self.table_fragmentainer_block_start(style) - logical_block_edge_spacing
        } else {
            self.cursor_y
        };
        let mut table_body_fragment: Option<TableBodyPaintFragment> = None;
        let mut forced_break_carry = TableForcedBreakCarryState::new(fragmentainer_kind);
        let mut avoid_break_candidates = TableAvoidBreakCandidateState::new(fragmentainer_kind);
        let mut previous_row_page_end: Option<Option<String>> = None;
        let mut avoid_row_group_keep_state = TableAvoidRowGroupKeepState::default();
        let row_group_end_indices = table_row_group_end_indices(rows);
        // The table grid starts after its top edge spacing and must leave its
        // matching bottom edge spacing, padding, and border before the table
        // can end. Reserve that trailing non-content when deciding whether the
        // final row fits; otherwise a final row may be accepted only to place
        // the table's closing edge outside the fragmentainer.
        // <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
        let trailing_table_non_content = non_content_pt(
            table_vertical_edge_spacing(planned_row_occupancy, table_metrics.clone())
                + Self::table_logical_block_end_chrome(style, table_width),
        );
        let wrapper_chrome = TableWrapperFragmentChrome::for_table(style, table_width);
        let mut fragment_commit_context = TableBodyFragmentCommitContext {
            rows,
            grid,
            columns,
            style,
            stylesheets,
            table_x,
            wrapper_table_x,
            table_inline_origin,
            continuation_inline_offset,
            logical_inline_extent,
            physical_grid_width,
            table_cellpadding,
            column_plan,
            planned_row_heights,
            planned_row_occupancy,
            table_width,
            table_metrics: table_metrics.clone(),
            collapsed_geometry,
            table_is_document_canvas,
            repeating_header_rows,
            repeating_footer_rows,
        };
        let mut row_index = 0usize;
        while row_index < rows.len() {
            let row = &rows[row_index];
            let row_style = self.style_for_table_row(row, style, stylesheets);
            let row_is_repeating_header = repeating_header_rows.contains(&row_index);
            let row_is_repeating_footer = repeating_footer_rows.contains(&row_index);
            let row_height = planned_row_heights[row_index];
            let row_is_running = row_style.position.is_running();
            let row_collapsed = table_row_is_collapsed(&row_style);
            let source_row_has_occupied_cell = !row_is_running
                && !row_collapsed
                && planned_row_occupancy
                    .get(row_index)
                    .copied()
                    .unwrap_or(false);
            let row_chrome_context = TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(self.table_fragmentainer_block_size(style)),
                header_height: layout_pt(repeating_header_height),
                footer_height: layout_pt(repeating_footer_height),
                wrapper_chrome,
                allow_header: !row_is_repeating_header,
                allow_footer: !row_is_repeating_footer,
            };
            let row_fragment_required_height =
                if row_height > self.table_fragmentainer_block_size(style) + 0.01 {
                    0.01
                } else {
                    row_height
                        + if row_index + 1 == rows.len() && !row_is_repeating_header {
                            trailing_table_non_content.points()
                        } else {
                            0.0
                        }
                };
            let row_page_values = if row_is_running {
                ResolvedPageBoundaryValues {
                    start: None,
                    end: None,
                }
            } else {
                self.table_row_page_boundary_values(
                    row_index,
                    row_group_end_indices[row_index],
                    row,
                    &row_style,
                    style,
                    stylesheets,
                )
            };
            let row_page_start = row_page_values.start;
            let row_page_end = row_page_values.end;
            // A table's first source row is also a class-A page boundary.
            // Without a preceding row there is no `previous_row_page_end` to
            // trigger the normal transition below, but a row-group's `page`
            // value still selects the page box on which that first row (and
            // any repeated copy) is laid out.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            if previous_row_page_end.is_none()
                && !self.current_page_has_content()
                && row_page_start != self.active_page_value_scope(style)
            {
                self.switch_page_name_at_class_a_boundary(row_page_start.as_deref());
                fragment_commit_context.rebase_destination_grid_to_fragmentainer(
                    self.table_fragmentainer_flow_axes(style),
                    self.content_left,
                    self.content_right,
                );
                table_x = fragment_commit_context.table_x;
                logical_block_cursor = self.table_block_cursor_after_fragment_start(style, 0.0);
                pending_fragmentainer_placement = self.table_fragmentainer_placement(
                    style,
                    table_x,
                    wrapper_table_x,
                    if style.writing_mode.has_vertical_lines() {
                        table_inline_origin.points()
                    } else {
                        self.cursor_y
                    },
                );
            }
            if let Some(previous_page_end) = previous_row_page_end.clone()
                && let Some(named_page_break) =
                    TableNamedPageBreakDecision::choose(TableNamedPageBreakInput {
                        previous_page_end,
                        row_page_start: row_page_start.clone(),
                        outgoing_repeat_policy: current_fragment_repeat_policy,
                        row_required_height: row_fragment_required_height,
                        chrome_context: row_chrome_context,
                        paint_repeated_footer: !row_is_repeating_footer
                            && !self.table_block_cursor_is_at_start(
                                logical_block_cursor,
                                style,
                                logical_block_edge_spacing,
                            ),
                    })
            {
                self.commit_table_body_fragment_boundary(
                    &mut table_body_fragment,
                    &fragment_commit_context,
                    named_page_break.boundary,
                );
                self.switch_page_name_at_class_a_boundary(named_page_break.page_name.as_deref());
                fragment_commit_context.rebase_destination_grid_to_fragmentainer(
                    self.table_fragmentainer_flow_axes(style),
                    self.content_left,
                    self.content_right,
                );
                table_x = fragment_commit_context.table_x;
                logical_block_cursor =
                    self.table_block_cursor_after_fragment_start(style, logical_block_edge_spacing);
                pending_fragmentainer_placement = self.table_fragmentainer_placement(
                    style,
                    table_x,
                    wrapper_table_x,
                    if style.writing_mode.has_vertical_lines() {
                        table_inline_origin.points()
                    } else {
                        self.cursor_y
                    },
                );
                current_fragment_repeat_policy = named_page_break.start.repeat_policy;
                pending_fragment_start = named_page_break.start;
            }
            if !row_is_running {
                previous_row_page_end = Some(row_page_end);
            }
            let break_before = Self::effective_table_row_break_before(
                row_index,
                &row_style,
                row_group_break_before,
            );
            let break_after =
                Self::effective_table_row_break_after(row_index, &row_style, row_group_break_after);
            let next_break_before = if row_index + 1 < rows.len() {
                let next_row_style =
                    self.style_for_table_row(&rows[row_index + 1], style, stylesheets);
                Self::effective_table_row_break_before(
                    row_index + 1,
                    &next_row_style,
                    row_group_break_before,
                )
            } else {
                PageBreak::Auto
            };
            let row_breaks =
                forced_break_carry.take_box_context(break_before, break_after, next_break_before);
            let forced_break_before = row_breaks.forced_break_before_in(fragmentainer_kind);
            let mut broke_before_row = forced_break_before.is_some();
            let wrapper_timeline_checkpoint = table_body_fragment
                .as_ref()
                .and_then(TableBodyPaintFragment::wrapper_timeline_checkpoint);
            let pending_row_start_candidate = PendingTableBreakCandidate {
                meta: TableBreakCandidateMeta {
                    row_index,
                    table_body_fragment: table_body_fragment.clone(),
                    wrapper_timeline_checkpoint,
                    repeat_policy: current_fragment_repeat_policy,
                    height: 0.0,
                },
            };
            let row_start_candidate = avoid_break_candidates
                .row_start_may_be_rollback_target(row_collapsed, row_is_running, row_breaks)
                .then(|| pending_row_start_candidate.arm(self));
            let mut start_chrome_replayed_after_break = false;
            if let Some(page_break) = forced_break_before {
                let forced_break = Self::table_body_forced_break(
                    current_fragment_repeat_policy,
                    fragmentainer_kind,
                    page_break,
                    row_fragment_required_height,
                    row_chrome_context,
                    !row_is_repeating_footer
                        && !self.table_block_cursor_is_at_start(
                            logical_block_cursor,
                            style,
                            logical_block_edge_spacing,
                        ),
                );
                current_fragment_repeat_policy = forced_break.start.repeat_policy;
                pending_fragment_start = forced_break.start;
                if let Some(destination_cursor) = self.apply_table_body_forced_break(
                    &mut table_body_fragment,
                    &mut fragment_commit_context,
                    &mut pending_fragmentainer_placement,
                    forced_break,
                    source_row_has_occupied_cell,
                ) {
                    logical_block_cursor = destination_cursor.points();
                    start_chrome_replayed_after_break = true;
                } else {
                    logical_block_cursor = self
                        .table_block_cursor_after_fragment_start(style, logical_block_edge_spacing);
                }
                table_x = fragment_commit_context.table_x;
            }
            // Rows and row groups are both table fragmentation containers.
            // Represent a row-level `break-inside: avoid` as a one-row range
            // so it shares the measured keep-together decision used for an
            // authored row group rather than relying on the later generic row
            // overflow path.
            // <https://www.w3.org/TR/css-break-3/#break-within>
            let authored_avoid_row_group = avoid_break_row_groups
                .iter()
                .find(|avoid_row_group| avoid_row_group.start == row_index)
                .copied();
            let row_level_avoid = authored_avoid_row_group.is_none()
                && fragmentainer_kind.avoids_break_inside(&row_style);
            let avoid_row_group = authored_avoid_row_group.or_else(|| {
                row_level_avoid.then(|| TableAvoidRowGroup::new(row_index, row_index + 1))
            });
            let mut row_requires_avoid_group_slice = false;
            if let Some(avoid_row_group) = avoid_row_group {
                let authored_group_has_forced_internal_break = authored_avoid_row_group
                    .is_some_and(|group| {
                        (group.start + 1..group.end).any(|candidate_index| {
                            let candidate_style = self.style_for_table_row(
                                &rows[candidate_index],
                                style,
                                stylesheets,
                            );
                            Self::effective_table_row_break_before(
                                candidate_index,
                                &candidate_style,
                                row_group_break_before,
                            ) != PageBreak::Auto
                        })
                    });
                let group_requirement = TableRowGroupFragmentRequirement::from_row_group(
                    avoid_row_group,
                    planned_row_heights,
                    planned_row_occupancy,
                    table_metrics.clone(),
                );
                // A one-row group can still exceed a fresh fragmentainer when
                // separated-border edges are included. Its row track itself
                // is smaller than the page, so the ordinary oversized-row
                // branch would otherwise paint past the trailing edge instead
                // of splitting at a cell-child boundary.
                let no_repeat_fragmentainer = row_chrome_context
                    .without_repeats()
                    .fresh_fragmentainer(TableFragmentRepeatPolicy {
                        repeat_header: false,
                        repeat_footer: false,
                    });
                row_requires_avoid_group_slice = authored_avoid_row_group.is_some()
                    && avoid_row_group.row_span() == 1
                    && group_requirement.block_size().points()
                        > no_repeat_fragmentainer.body_capacity.points()
                            + TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE;
                let current_fragmentainer = row_chrome_context.current_fragmentainer(
                    PageTopBlockPosition::new(logical_block_cursor),
                    PageTopBlockPosition::new(self.table_fragmentainer_block_end(style)),
                    current_fragment_repeat_policy,
                    !row_is_repeating_footer,
                );
                // CSS Fragmentation 3 treats `break-inside: avoid` as a
                // preference to keep a fragmentation container together when
                // possible. For table row groups, use the measured group height
                // so rows are moved as a unit when their complete table
                // fragment footprint fits on a fresh page but not in the
                // current remaining fragmentainer.
                // https://www.w3.org/TR/css-break-3/#break-within
                let row_group_avoid_decision =
                    TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
                        group: avoid_row_group,
                        required_block_size: group_requirement.block_size(),
                        current_fragmentainer,
                        chrome_context: row_chrome_context,
                        can_advance: !self.table_block_cursor_is_at_start(
                            logical_block_cursor,
                            style,
                            logical_block_edge_spacing,
                        ),
                    });
                let oversized_authored_group_needs_fresh_start = authored_avoid_row_group.is_some()
                    && !authored_group_has_forced_internal_break
                    && row_group_avoid_decision.is_none()
                    && group_requirement.block_size().points()
                        > current_fragmentainer.available_block_size().points() + 0.01;
                // When an avoided group cannot remain whole after repeated
                // chrome is accounted for, move its first source row to a
                // fresh table fragment before relaxing the constraint at a
                // legal child boundary. Starting the relaxed split in the
                // previous fragment loses the group-level avoid opportunity
                // and makes its first atomic child share unrelated header or
                // footer chrome.
                // <https://www.w3.org/TR/css-break-3/#break-within>
                if let Some(decision) = row_group_avoid_decision
                    && !(row_level_avoid && decision.keeps_with_overflow())
                {
                    // The decision has already selected the only repeat
                    // policy that preserves this avoided group. Recomputing
                    // it here reintroduces chrome after a bounded-overflow
                    // decision deliberately suppressed it.
                    let incoming_repeat_policy = decision.repeat_policy;
                    debug_assert!(
                        decision.required_block_size.points()
                            > current_fragmentainer.available_block_size().points() + 0.01
                    );
                    let transition = Self::table_body_fragment_transition(
                        fragmentainer_kind,
                        current_fragment_repeat_policy,
                        TableFragmentBreakReason::AvoidedOverflow,
                        incoming_repeat_policy,
                        !row_is_repeating_header,
                        !row_is_repeating_footer,
                    );
                    current_fragment_repeat_policy = transition.start.repeat_policy;
                    // Only `KeptByChromeOverflow` records a range, but the
                    // state owns that distinction. Committing every decision
                    // here keeps the pagination decision and the row-paint
                    // mode coupled without duplicating its predicate at the
                    // call site.
                    avoid_row_group_keep_state.commit(decision);
                    pending_fragment_start = transition.start;
                    logical_block_cursor = self.apply_table_body_fragment_transition(
                        &mut table_body_fragment,
                        &mut fragment_commit_context,
                        &mut pending_fragmentainer_placement,
                        transition,
                        !table_body_fragment_started,
                        source_row_has_occupied_cell,
                    );
                    table_body_fragment_started = true;
                    table_x = fragment_commit_context.table_x;
                    start_chrome_replayed_after_break = true;
                    broke_before_row = true;
                }
                if !broke_before_row && oversized_authored_group_needs_fresh_start {
                    let incoming_repeat_policy =
                        row_chrome_context.repeat_policy(layout_pt(row_height));
                    let transition = Self::table_body_fragment_transition(
                        fragmentainer_kind,
                        current_fragment_repeat_policy,
                        TableFragmentBreakReason::AvoidedOverflow,
                        incoming_repeat_policy,
                        !row_is_repeating_header,
                        !row_is_repeating_footer,
                    );
                    current_fragment_repeat_policy = transition.start.repeat_policy;
                    pending_fragment_start = transition.start;
                    logical_block_cursor = self.apply_table_body_fragment_transition(
                        &mut table_body_fragment,
                        &mut fragment_commit_context,
                        &mut pending_fragmentainer_placement,
                        transition,
                        !table_body_fragment_started,
                        source_row_has_occupied_cell,
                    );
                    table_body_fragment_started = true;
                    table_x = fragment_commit_context.table_x;
                    start_chrome_replayed_after_break = true;
                    broke_before_row = true;
                }
            }
            let current_fragmentainer = row_chrome_context.current_fragmentainer(
                PageTopBlockPosition::new(logical_block_cursor),
                PageTopBlockPosition::new(self.table_fragmentainer_block_end(style)),
                current_fragment_repeat_policy,
                !row_is_repeating_footer,
            );
            let row_break_opportunity = FragmentBreakOpportunity::before_box_boundary(
                fragmentainer_kind,
                row_index as f32,
                row_breaks,
                avoid_break_candidates.previous_break_after,
                false,
            );
            let avoid_boundary = row_break_opportunity.avoids_break_in(fragmentainer_kind);
            let avoid_candidate = avoid_break_candidates.boundary_candidate(row_breaks);
            if avoid_boundary
                && let Some(candidate) = avoid_candidate
                && let Some(decision) =
                    TableAvoidRunBreakDecision::choose(TableAvoidRunBreakInput {
                        candidate,
                        row_height,
                        current_fragmentainer,
                        chrome_context: row_chrome_context,
                        can_advance: !self.table_block_cursor_is_at_start(
                            logical_block_cursor,
                            style,
                            logical_block_edge_spacing,
                        ),
                    })
            {
                debug_assert!(
                    decision.avoid_run_height
                        > current_fragmentainer.available_block_size().points() + 0.01
                );
                let candidate_meta = decision.candidate.restore(self);
                if let (Some(fragment), Some(checkpoint)) = (
                    candidate_meta.table_body_fragment.as_ref(),
                    candidate_meta.wrapper_timeline_checkpoint,
                ) {
                    fragment.rewind_wrapper_timeline(checkpoint);
                }
                table_body_fragment = candidate_meta.table_body_fragment;
                current_fragment_repeat_policy = candidate_meta.repeat_policy;
                let transition = Self::table_body_fragment_transition(
                    fragmentainer_kind,
                    current_fragment_repeat_policy,
                    TableFragmentBreakReason::AvoidedOverflow,
                    decision.incoming_repeat_policy,
                    !row_is_repeating_header,
                    !row_is_repeating_footer
                        && !self.table_block_cursor_is_at_start(
                            logical_block_cursor,
                            style,
                            logical_block_edge_spacing,
                        ),
                );
                current_fragment_repeat_policy = transition.start.repeat_policy;
                pending_fragment_start = transition.start;
                logical_block_cursor = self.apply_table_body_fragment_transition(
                    &mut table_body_fragment,
                    &mut fragment_commit_context,
                    &mut pending_fragmentainer_placement,
                    transition,
                    !table_body_fragment_started,
                    source_row_has_occupied_cell,
                );
                table_body_fragment_started = true;
                table_x = fragment_commit_context.table_x;
                row_index = candidate_meta.row_index;
                avoid_break_candidates.reset();
                continue;
            }
            let row_kept_by_avoid_group = avoid_row_group_keep_state.contains_row(row_index);
            if let Some(overflow_break) =
                TableRowOverflowBreakDecision::choose(TableRowOverflowBreakInput {
                    row_height,
                    row_required_height: row_fragment_required_height,
                    current_fragmentainer,
                    row_kept_by_avoid_group,
                    prefer_fresh_fragment: row_level_avoid,
                    can_break: !self.table_block_cursor_is_at_start(
                        logical_block_cursor,
                        style,
                        logical_block_edge_spacing,
                    ) && self.out_of_flow_prebreak_suppression_depth == 0,
                    chrome_context: row_chrome_context,
                })
            {
                debug_assert_eq!(overflow_break.row_height, row_height);
                let transition = Self::table_body_fragment_transition(
                    fragmentainer_kind,
                    current_fragment_repeat_policy,
                    TableFragmentBreakReason::Overflow,
                    overflow_break.incoming_repeat_policy,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                current_fragment_repeat_policy = transition.start.repeat_policy;
                pending_fragment_start = transition.start;
                logical_block_cursor = self.apply_table_body_fragment_transition(
                    &mut table_body_fragment,
                    &mut fragment_commit_context,
                    &mut pending_fragmentainer_placement,
                    transition,
                    !table_body_fragment_started,
                    source_row_has_occupied_cell,
                );
                table_body_fragment_started = true;
                table_x = fragment_commit_context.table_x;
                start_chrome_replayed_after_break = true;
                broke_before_row = true;
            }
            if broke_before_row {
                if !start_chrome_replayed_after_break {
                    self.replay_table_fragment_start_chrome(
                        &fragment_commit_context,
                        pending_fragment_start,
                    );
                    logical_block_cursor = self
                        .table_block_cursor_after_fragment_start(style, logical_block_edge_spacing);
                }
                let after_header_fragmentainer = row_chrome_context.current_fragmentainer(
                    PageTopBlockPosition::new(logical_block_cursor),
                    PageTopBlockPosition::new(self.table_fragmentainer_block_end(style)),
                    current_fragment_repeat_policy,
                    !row_is_repeating_footer,
                );
                if let Some(overflow_break) =
                    TableRowOverflowBreakDecision::choose(TableRowOverflowBreakInput {
                        row_height,
                        row_required_height: row_fragment_required_height,
                        current_fragmentainer: after_header_fragmentainer,
                        row_kept_by_avoid_group,
                        prefer_fresh_fragment: false,
                        can_break: !self.table_block_cursor_is_at_start(
                            logical_block_cursor,
                            style,
                            logical_block_edge_spacing,
                        ),
                        chrome_context: TableFragmentChromeContext {
                            allow_header: false,
                            ..row_chrome_context
                        },
                    })
                {
                    debug_assert_eq!(overflow_break.row_height, row_height);
                    let transition = Self::table_body_fragment_transition(
                        fragmentainer_kind,
                        current_fragment_repeat_policy,
                        TableFragmentBreakReason::Overflow,
                        overflow_break.incoming_repeat_policy,
                        false,
                        !row_is_repeating_footer,
                    );
                    current_fragment_repeat_policy = transition.start.repeat_policy;
                    pending_fragment_start = transition.start;
                    logical_block_cursor = self.apply_table_body_fragment_transition(
                        &mut table_body_fragment,
                        &mut fragment_commit_context,
                        &mut pending_fragmentainer_placement,
                        transition,
                        !table_body_fragment_started,
                        source_row_has_occupied_cell,
                    );
                    table_body_fragment_started = true;
                    table_x = fragment_commit_context.table_x;
                }
            }

            let row_top = self.cursor_y;
            let fragment_placement = pending_fragmentainer_placement;
            self.ensure_committed_table_body_fragment(
                &mut table_body_fragment,
                fragmentainer_kind,
                fragment_placement,
                &mut pending_fragment_start,
                current_fragment_repeat_policy,
                repeating_header_rows,
            );
            if row_collapsed {
                let decision = self.table_row_fragment_decision(
                    table_body_fragment.as_ref(),
                    table_x,
                    physical_grid_width_points,
                    row_index,
                    row_top,
                    0.0,
                    0.0,
                    0.0,
                    true,
                    TableRowFragmentMode::Whole,
                );
                self.capture_table_row_fragment_decision_assignments(
                    row,
                    &row_style,
                    stylesheets,
                    table_x,
                    decision,
                );
                if let Some(fragment) = &mut table_body_fragment {
                    fragment.push_row_decision(decision);
                }
                avoid_break_candidates.finish_non_content_row(row_breaks, row_start_candidate);
                row_index += 1;
                continue;
            }
            if row_is_running {
                let placement = self.table_row_running_assignment_placement(table_x, row_top);
                if let Some(element) = row.element {
                    self.capture_assignments_for_fragment_source(element, &row_style, placement);
                }
                avoid_break_candidates.finish_non_content_row(row_breaks, row_start_candidate);
                row_index += 1;
                continue;
            }
            let row_baseline_offset = self
                .table_row_baseline_offset(
                    row_index,
                    row,
                    &grid.rows[row_index],
                    &row_style,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics.clone(),
                    collapsed_geometry,
                )
                .map(|baseline| baseline.offset);
            let row_has_fragmentable_cell_content =
                self.table_row_has_fragmentable_cell_content(row, grid, row_index);
            let row_exceeds_fresh_body = row_height
                > row_chrome_context
                    .fresh_fragmentainer(row_chrome_context.repeat_policy(layout_pt(row_height)))
                    .body_capacity
                    .points()
                    + 0.01;
            if row_has_fragmentable_cell_content
                && (row_exceeds_fresh_body || row_requires_avoid_group_slice)
                && !row_kept_by_avoid_group
            {
                let mut remaining = row_height;
                let mut piece_offset = 0.0;
                while remaining > 0.01 {
                    let current_fragmentainer = row_chrome_context.current_fragmentainer(
                        PageTopBlockPosition::new(logical_block_cursor),
                        PageTopBlockPosition::new(self.table_fragmentainer_block_end(style)),
                        current_fragment_repeat_policy,
                        !row_is_repeating_footer,
                    );
                    let mut slice_decision =
                        TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
                            remaining_height: remaining,
                            row_required_height: row_fragment_required_height,
                            current_fragmentainer,
                            chrome_context: row_chrome_context,
                            can_advance: !self.table_block_cursor_is_at_start(
                                logical_block_cursor,
                                style,
                                logical_block_edge_spacing,
                            ),
                        });
                    if slice_decision.paints_slice() {
                        let fresh_body_capacity = row_chrome_context
                            .fresh_fragmentainer(slice_decision.incoming_repeat_policy)
                            .body_capacity
                            .points();
                        // Child-boundary measurement reuses final table-cell
                        // relayout, whose nested formatting-context probes can
                        // otherwise append provisional paint. It is only a
                        // break-choice query, so restore its complete page and
                        // side-effect state before committing the selected row
                        // piece.
                        let child_boundary_snapshot = self.snapshot();
                        let child_boundary_piece_height = self
                            .table_row_child_boundary_piece_height(
                                row,
                                &row_style,
                                grid,
                                row_index,
                                stylesheets,
                                table_cellpadding,
                                column_plan,
                                table_metrics.clone(),
                                collapsed_geometry,
                                piece_offset,
                                slice_decision.piece_height,
                                remaining,
                                fresh_body_capacity,
                            );
                        self.restore(child_boundary_snapshot);
                        // A zero result is a potential pre-break decision.
                        // The no-progress check below accepts it only when a
                        // destination fragmentainer can offer more body
                        // capacity; otherwise this first source piece stays
                        // intact in the current fragmentainer.
                        slice_decision =
                            slice_decision.at_child_boundary(child_boundary_piece_height);
                    }
                    // A zero-height child boundary may legally pre-break an
                    // oversized row only when a destination fragmentainer can
                    // consume more of that source row.  At a row's first
                    // source piece, retrying an equally small column/page
                    // would otherwise create an unbounded sequence of empty
                    // table fragments.  Keep the row intact and let it
                    // overflow the current fragmentainer instead.
                    //
                    // <https://drafts.csswg.org/css-tables/#table-fragmentation>
                    // <https://www.w3.org/TR/css-break-3/#unforced-breaks>
                    if piece_offset <= 0.01 {
                        let next_body_capacity = row_chrome_context
                            .fresh_fragmentainer(slice_decision.incoming_repeat_policy)
                            .body_capacity
                            .points();
                        if slice_decision.needs_unfragmented_overflow(next_body_capacity) {
                            slice_decision =
                                slice_decision.as_unfragmented_overflow(next_body_capacity);
                        }
                    }
                    if !slice_decision.paints_slice() {
                        debug_assert!(
                            slice_decision.available_body_size <= 0.01
                                || !self.table_block_cursor_is_at_start(
                                    logical_block_cursor,
                                    style,
                                    logical_block_edge_spacing,
                                ),
                            "a child-aware table slice may only pre-break away from a nonempty fragmentainer"
                        );
                        let transition = Self::table_body_fragment_transition(
                            fragmentainer_kind,
                            current_fragment_repeat_policy,
                            TableFragmentBreakReason::OversizedRowSlice,
                            slice_decision.incoming_repeat_policy,
                            !row_is_repeating_header,
                            !row_is_repeating_footer,
                        );
                        current_fragment_repeat_policy = transition.start.repeat_policy;
                        pending_fragment_start = transition.start;
                        logical_block_cursor = self.apply_table_body_fragment_transition(
                            &mut table_body_fragment,
                            &mut fragment_commit_context,
                            &mut pending_fragmentainer_placement,
                            transition,
                            !table_body_fragment_started,
                            source_row_has_occupied_cell,
                        );
                        table_body_fragment_started = true;
                        table_x = fragment_commit_context.table_x;
                        let fragment_placement = pending_fragmentainer_placement;
                        self.ensure_committed_table_body_fragment(
                            &mut table_body_fragment,
                            fragmentainer_kind,
                            fragment_placement,
                            &mut pending_fragment_start,
                            current_fragment_repeat_policy,
                            repeating_header_rows,
                        );
                        continue;
                    }

                    let piece_height = slice_decision.piece_height;
                    let piece_top = self.cursor_y;
                    let fragment_placement = pending_fragmentainer_placement;
                    self.ensure_committed_table_body_fragment(
                        &mut table_body_fragment,
                        fragmentainer_kind,
                        fragment_placement,
                        &mut pending_fragment_start,
                        current_fragment_repeat_policy,
                        repeating_header_rows,
                    );
                    let decision = self.table_row_fragment_decision(
                        table_body_fragment.as_ref(),
                        table_x,
                        physical_grid_width_points,
                        row_index,
                        piece_top,
                        piece_height,
                        piece_offset,
                        row_height,
                        !planned_row_occupancy
                            .get(row_index)
                            .cloned()
                            .unwrap_or(false),
                        if slice_decision.is_unfragmented_overflow() {
                            TableRowFragmentMode::Whole
                        } else {
                            TableRowFragmentMode::Sliced
                        },
                    );
                    self.capture_table_row_fragment_decision_assignments(
                        row,
                        &row_style,
                        stylesheets,
                        table_x,
                        decision,
                    );
                    self.paint_committed_table_row_fragment(
                        &mut table_body_fragment,
                        decision,
                        row,
                        &row_style,
                        rows,
                        grid,
                        style,
                        stylesheets,
                        table_x,
                        physical_grid_width_points,
                        table_cellpadding,
                        column_plan,
                        planned_row_heights,
                        source_row_heights,
                        planned_row_occupancy,
                        table_height_is_definite,
                        table_metrics.clone(),
                        collapsed_geometry,
                        row_baseline_offset,
                        source_grid_placement,
                        root_background_source_grid_placement,
                        wrapper_timeline.clone(),
                    );
                    logical_block_cursor -= piece_height;
                    if !style.writing_mode.has_vertical_lines() {
                        self.cursor_y -= piece_height;
                    }
                    remaining -= piece_height;
                    piece_offset += piece_height;

                    if slice_decision.continues_after_slice() {
                        let transition = Self::table_body_fragment_transition(
                            fragmentainer_kind,
                            current_fragment_repeat_policy,
                            TableFragmentBreakReason::OversizedRowSlice,
                            slice_decision.incoming_repeat_policy,
                            !row_is_repeating_header,
                            !row_is_repeating_footer,
                        );
                        current_fragment_repeat_policy = transition.start.repeat_policy;
                        pending_fragment_start = transition.start;
                        logical_block_cursor = self.apply_table_body_fragment_transition(
                            &mut table_body_fragment,
                            &mut fragment_commit_context,
                            &mut pending_fragmentainer_placement,
                            transition,
                            !table_body_fragment_started,
                            source_row_has_occupied_cell,
                        );
                        table_body_fragment_started = true;
                        table_x = fragment_commit_context.table_x;
                        let fragment_placement = pending_fragmentainer_placement;
                        self.ensure_committed_table_body_fragment(
                            &mut table_body_fragment,
                            fragmentainer_kind,
                            fragment_placement,
                            &mut pending_fragment_start,
                            current_fragment_repeat_policy,
                            repeating_header_rows,
                        );
                    }
                }
            } else if !style.writing_mode.has_vertical_lines()
                && !row_has_fragmentable_cell_content
                && row_height
                    > row_chrome_context
                        .current_fragmentainer(
                            PageTopBlockPosition::new(logical_block_cursor),
                            PageTopBlockPosition::new(self.table_fragmentainer_block_end(style)),
                            current_fragment_repeat_policy,
                            !row_is_repeating_footer,
                        )
                        .available_block_size()
                        .points()
                        + 0.01
            {
                // The row's empty cells have no internal break opportunity,
                // yet its table-root decoration must still be clipped by each
                // outer fragmentainer that its fixed source interval crosses.
                // This mode records only those structural slices; it never
                // replays cell content as independently fragmented.
                // <https://www.w3.org/TR/css-break-3/#box-splitting>
                let mut remaining = row_height;
                let mut source_offset = 0.0;
                while remaining > 0.01 {
                    let fragmentainer = row_chrome_context.current_fragmentainer(
                        PageTopBlockPosition::new(logical_block_cursor),
                        PageTopBlockPosition::new(self.table_fragmentainer_block_end(style)),
                        current_fragment_repeat_policy,
                        !row_is_repeating_footer,
                    );
                    let available = fragmentainer.available_block_size().points();
                    if available <= 0.01 {
                        let transition = Self::table_body_fragment_transition(
                            fragmentainer_kind,
                            current_fragment_repeat_policy,
                            TableFragmentBreakReason::Overflow,
                            row_chrome_context.repeat_policy(layout_pt(remaining)),
                            !row_is_repeating_header,
                            !row_is_repeating_footer,
                        );
                        current_fragment_repeat_policy = transition.start.repeat_policy;
                        pending_fragment_start = transition.start;
                        logical_block_cursor = self.apply_table_body_fragment_transition(
                            &mut table_body_fragment,
                            &mut fragment_commit_context,
                            &mut pending_fragmentainer_placement,
                            transition,
                            !table_body_fragment_started,
                            false,
                        );
                        table_body_fragment_started = true;
                        table_x = fragment_commit_context.table_x;
                        continue;
                    }
                    let piece_height = remaining.min(available);
                    let fragment_placement = pending_fragmentainer_placement;
                    self.ensure_committed_table_body_fragment(
                        &mut table_body_fragment,
                        fragmentainer_kind,
                        fragment_placement,
                        &mut pending_fragment_start,
                        current_fragment_repeat_policy,
                        repeating_header_rows,
                    );
                    let decision = self.table_row_fragment_decision(
                        table_body_fragment.as_ref(),
                        table_x,
                        physical_grid_width_points,
                        row_index,
                        self.cursor_y,
                        piece_height,
                        source_offset,
                        row_height,
                        true,
                        TableRowFragmentMode::DecorationOnly,
                    );
                    self.paint_committed_table_row_fragment(
                        &mut table_body_fragment,
                        decision,
                        row,
                        &row_style,
                        rows,
                        grid,
                        style,
                        stylesheets,
                        table_x,
                        physical_grid_width_points,
                        table_cellpadding,
                        column_plan,
                        planned_row_heights,
                        source_row_heights,
                        planned_row_occupancy,
                        table_height_is_definite,
                        table_metrics.clone(),
                        collapsed_geometry,
                        row_baseline_offset,
                        source_grid_placement,
                        root_background_source_grid_placement,
                        wrapper_timeline.clone(),
                    );
                    logical_block_cursor -= piece_height;
                    if !style.writing_mode.has_vertical_lines() {
                        self.cursor_y -= piece_height;
                    }
                    remaining -= piece_height;
                    source_offset += piece_height;
                    if remaining <= 0.01 {
                        break;
                    }
                    let transition = Self::table_body_fragment_transition(
                        fragmentainer_kind,
                        current_fragment_repeat_policy,
                        TableFragmentBreakReason::Overflow,
                        row_chrome_context.repeat_policy(layout_pt(remaining)),
                        !row_is_repeating_header,
                        !row_is_repeating_footer,
                    );
                    current_fragment_repeat_policy = transition.start.repeat_policy;
                    pending_fragment_start = transition.start;
                    logical_block_cursor = self.apply_table_body_fragment_transition(
                        &mut table_body_fragment,
                        &mut fragment_commit_context,
                        &mut pending_fragmentainer_placement,
                        transition,
                        !table_body_fragment_started,
                        false,
                    );
                    table_body_fragment_started = true;
                    table_x = fragment_commit_context.table_x;
                }
            } else {
                let row_fragment_mode = if row_kept_by_avoid_group {
                    TableRowFragmentMode::KeptByAvoidOverflow
                } else {
                    TableRowFragmentMode::Whole
                };
                let decision = self.table_row_fragment_decision(
                    table_body_fragment.as_ref(),
                    table_x,
                    physical_grid_width_points,
                    row_index,
                    row_top,
                    row_height,
                    0.0,
                    row_height,
                    !planned_row_occupancy
                        .get(row_index)
                        .cloned()
                        .unwrap_or(false),
                    row_fragment_mode,
                );
                self.capture_table_row_fragment_decision_assignments(
                    row,
                    &row_style,
                    stylesheets,
                    table_x,
                    decision,
                );
                self.paint_committed_table_row_fragment(
                    &mut table_body_fragment,
                    decision,
                    row,
                    &row_style,
                    rows,
                    grid,
                    style,
                    stylesheets,
                    table_x,
                    physical_grid_width_points,
                    table_cellpadding,
                    column_plan,
                    planned_row_heights,
                    source_row_heights,
                    planned_row_occupancy,
                    table_height_is_definite,
                    table_metrics.clone(),
                    collapsed_geometry,
                    row_baseline_offset,
                    source_grid_placement,
                    root_background_source_grid_placement,
                    wrapper_timeline.clone(),
                );
                logical_block_cursor -= row_height;
                if !style.writing_mode.has_vertical_lines() {
                    self.cursor_y -= row_height;
                }
            }
            if planned_row_occupancy
                .get(row_index)
                .cloned()
                .unwrap_or(false)
                && planned_row_occupancy
                    .get(row_index + 1..)
                    .is_some_and(|following| following.iter().any(|occupied| *occupied))
            {
                let spacing = Self::table_logical_block_spacing(style, &table_metrics);
                logical_block_cursor -= spacing;
                if !style.writing_mode.has_vertical_lines() {
                    self.cursor_y -= spacing;
                }
            }
            let has_next_row = row_index + 1 < rows.len();
            forced_break_carry.finish_box(row_breaks, has_next_row);
            avoid_break_candidates.finish_content_row(row_breaks, row_start_candidate, row_height);
            row_index += 1;
            avoid_row_group_keep_state.finish_row(row_index);
        }

        let final_body_fragment =
            table_body_fragment
                .as_ref()
                .map(|fragment| TableRootFinalBodyFragment {
                    placement: fragment.plan.placement,
                    body_bottom: PageTopBlockPosition::new(fragment.bottom()),
                });
        TableBodyRowsOutcome {
            table_body_fragment,
            final_body_fragment,
            forced_break_after_table_rows: forced_break_carry.outgoing_source_break(),
            current_fragment_repeat_policy,
            continuation_inline_offset,
        }
    }

    /// Commit the current table-body page fragment before starting another one.
    ///
    /// CSS Fragmentation commits one fragmentainer slice before layout
    /// advances to the next page; CSS 2.2 table header/footer repetition is
    /// page-fragment chrome around that same committed body slice. Centralizing
    /// the side effects here keeps footer reservation, footer painting, and
    /// table paint finalization tied to a single fragment boundary decision:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>.
    pub(in crate::layout::table) fn commit_table_body_fragment_boundary(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        context: &TableBodyFragmentCommitContext<'_, '_>,
        boundary: TableFragmentBoundaryDecision,
    ) {
        debug_assert!(
            (context.logical_inline_extent.points() - context.column_plan.total_width().points())
                .abs()
                <= 0.01,
            "fragment commit must retain the column plan's logical inline extent"
        );
        let has_normal_flow_content = fragment.as_ref().is_some_and(|fragment| {
            fragment
                .plan
                .body_rows
                .iter()
                .any(|row| !row.collapsed && row.row_height > 0.0)
        });
        let footer_rows = boundary
            .repeat_policy
            .footer_rows(context.repeating_footer_rows);
        if let Some(fragment) = fragment {
            fragment.mark_outgoing_boundary(boundary);
        }
        if boundary.footer_action.record_repeated_rows() {
            self.mark_table_body_fragment_repeated_footers(
                fragment,
                footer_rows,
                context.planned_row_heights,
                context.planned_row_occupancy,
                context.table_metrics.clone(),
            );
        }
        self.finalize_table_body_paint_fragment(
            fragment,
            context.rows,
            context.grid,
            context.columns,
            context.style,
            context.stylesheets,
            context.table_x,
            context.physical_grid_width.points(),
            context.table_cellpadding,
            context.column_plan,
            context.table_width,
            context.table_metrics.clone(),
            context.collapsed_geometry,
            context.table_is_document_canvas,
        );
        if has_normal_flow_content {
            self.mark_current_page_flow_content();
            self.current_page.mark_fragmentation_content();
        }
        if boundary.footer_action.paint_repeated_chrome() {
            self.layout_repeated_table_footer_rows_at_page_bottom(
                context.rows,
                context.grid,
                context.columns,
                footer_rows,
                context.style,
                context.stylesheets,
                context.table_x,
                context.physical_grid_width.points(),
                context.table_cellpadding,
                context.column_plan,
                context.planned_row_heights,
                context.planned_row_occupancy,
                context.table_width,
                context.table_metrics.clone(),
                context.collapsed_geometry,
            );
        }
    }

    /// Replay table chrome committed at the start of a body fragment.
    ///
    /// CSS Fragmentation fixes the new page fragment before its body rows are
    /// painted. Repeated table headers are page-fragment chrome, so their
    /// replay is driven by the same start decision later recorded on the body
    /// fragment plan:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>.
    fn replay_table_fragment_start_chrome(
        &mut self,
        context: &TableBodyFragmentCommitContext<'_, '_>,
        start: TableFragmentStartDecision,
    ) {
        let header_rows = start.repeated_header_rows(context.repeating_header_rows);
        if header_rows.is_empty() {
            return;
        }
        self.layout_repeated_table_rows(
            context.rows,
            context.grid,
            context.columns,
            header_rows,
            context.style,
            context.stylesheets,
            context.table_x,
            context.physical_grid_width.points(),
            context.table_cellpadding,
            context.column_plan,
            context.planned_row_heights,
            context.planned_row_occupancy,
            context.table_width,
            context.table_metrics.clone(),
            context.collapsed_geometry,
            true,
        );
    }

    /// Apply one committed table-fragment transition.
    ///
    /// The outgoing boundary owns footer replay/finalization and the incoming
    /// start owns repeated header replay. Keeping those decisions paired
    /// prevents target-specific cursor state from becoming an independent
    /// source of table fragmentation behavior:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn apply_table_body_fragment_transition(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        context: &mut TableBodyFragmentCommitContext<'_, '_>,
        pending_fragmentainer_placement: &mut TableFragmentainerPlacement,
        transition: TableFragmentTransitionDecision,
        _starts_table_body: bool,
        source_row_has_occupied_cell: bool,
    ) -> f32 {
        self.commit_table_body_fragment_boundary(fragment, context, transition.boundary);
        let Some(cursor_top) = self.materialize_table_fragmentainer_advance(
            transition.fragmentainer_kind,
            FragmentainerAdvance::Unforced,
        ) else {
            return self.table_block_cursor_after_fragment_start(context.style, 0.0);
        };
        self.start_table_body_fragment(
            context,
            pending_fragmentainer_placement,
            transition.start,
            cursor_top.points(),
            source_row_has_occupied_cell,
        )
    }

    /// Establish one destination table fragment after the page/column advance.
    ///
    /// Both forced and unforced table breaks must derive their row-fit cursor
    /// only after destination placement and optional repeated-header replay.
    /// Otherwise a forced break can retain a pre-header cursor while an
    /// avoid/overflow break observes the post-header cursor, causing the two
    /// equivalent fragment starts to choose different row slices.
    ///
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    /// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>
    fn start_table_body_fragment(
        &mut self,
        context: &mut TableBodyFragmentCommitContext<'_, '_>,
        pending_fragmentainer_placement: &mut TableFragmentainerPlacement,
        start: TableFragmentStartDecision,
        cursor_top: f32,
        source_row_has_occupied_cell: bool,
    ) -> f32 {
        context.rebase_destination_grid_to_fragmentainer(
            self.table_fragmentainer_flow_axes(context.style),
            self.content_left,
            self.content_right,
        );
        let fragmentainer = self.table_fragmentainer_placement(
            context.style,
            context.table_x,
            context.wrapper_table_x,
            if context.style.writing_mode.has_vertical_lines() {
                context.table_inline_origin.points()
            } else {
                cursor_top
            },
        );
        // A table wrapper may already have selected a later outer column for
        // its first grid row (for example after an unbroken vertical caption
        // spans several columns). `push_page` materializes only the next
        // scratch page, so it cannot be used as the semantic successor of
        // that selected ordinal. Advance from the retained placement instead.
        let fragmentainer = pending_fragmentainer_placement
            .outer_fragmentainer()
            .and_then(|previous_outer| {
                self.fragmentainer_override
                    .filter(|override_| override_.kind == FragmentainerKind::Column)
                    .map(|override_| {
                        let successor =
                            override_.placement_for_fragmentainer(previous_outer.ordinal() + 1);
                        fragmentainer.select_outer_fragmentainer(
                            TableOuterFragmentainerPlacement::from_outer(successor),
                        )
                    })
            })
            .unwrap_or(fragmentainer);
        let placement = TableFragmentStartPlacement::for_destination(
            context,
            start,
            fragmentainer,
            source_row_has_occupied_cell,
        );
        if !context.style.writing_mode.has_vertical_lines() {
            self.cursor_y = placement.cursor_for_start();
        }
        *pending_fragmentainer_placement = placement.fragmentainer();
        debug_assert!(
            (placement.fragmentainer().destination_grid_origin().x() - context.table_x).abs()
                <= 0.01
        );
        self.replay_table_fragment_start_chrome(context, start);
        self.table_block_cursor_after_fragment_start(context.style, 0.0)
    }

    /// Ensure a table body paint fragment exists and consume its start decision.
    ///
    /// A committed table fragment start owns the break reason and repeated
    /// header replay for exactly one newly-created table body fragment. After
    /// that start decision is recorded in the fragment plan, pagination resets
    /// the pending start to a neutral table-start value so later rows cannot
    /// accidentally inherit stale break metadata:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn ensure_committed_table_body_fragment(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        fragmentainer_kind: FragmentainerKind,
        placement: TableFragmentainerPlacement,
        pending_start: &mut TableFragmentStartDecision,
        current_repeat_policy: TableFragmentRepeatPolicy,
        repeating_header_rows: &[usize],
    ) {
        if self.ensure_table_body_paint_fragment(
            fragment,
            fragmentainer_kind,
            placement,
            *pending_start,
            repeating_header_rows,
        ) {
            *pending_start = TableFragmentStartDecision::new(
                TableFragmentBreakReason::TableStart,
                current_repeat_policy,
                false,
            );
        }
    }

    /// Build the committed transition between table body fragments.
    ///
    /// CSS Fragmentation treats the outgoing fragment boundary and incoming
    /// fragmentainer start as one break decision. This helper keeps row
    /// pagination branches from independently assembling footer replay,
    /// incoming repeat policy, break reason, and header replay:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn table_body_fragment_transition(
        fragmentainer_kind: FragmentainerKind,
        outgoing_repeat_policy: TableFragmentRepeatPolicy,
        break_reason: TableFragmentBreakReason,
        incoming_repeat_policy: TableFragmentRepeatPolicy,
        paint_repeated_header: bool,
        paint_repeated_footer: bool,
    ) -> TableFragmentTransitionDecision {
        TableFragmentTransitionDecision::from_input(TableFragmentTransitionInput {
            fragmentainer_kind,
            outgoing_repeat_policy,
            footer_action: TableFragmentFooterAction::paint_repeated_if(paint_repeated_footer),
            break_reason,
            incoming_repeat_policy,
            paint_repeated_header,
        })
    }

    /// Build the committed forced break before a table body row.
    ///
    /// Forced row and row-group breaks are resolved before the row is painted.
    /// The decision records the outgoing footer action, the authored break
    /// value passed to paged-media page selection, and the incoming repeated
    /// header/footer policy as one table-local break choice:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
    fn table_body_forced_break(
        outgoing_repeat_policy: TableFragmentRepeatPolicy,
        fragmentainer_kind: FragmentainerKind,
        page_break: PageBreak,
        row_required_height: f32,
        chrome_context: TableFragmentChromeContext,
        paint_repeated_footer: bool,
    ) -> TableForcedBreakDecision {
        TableForcedBreakDecision::choose(TableForcedBreakInput {
            outgoing_repeat_policy,
            fragmentainer_kind,
            page_break,
            row_required_height,
            chrome_context,
            paint_repeated_footer,
        })
    }

    /// Apply a committed forced break before continuing table body pagination.
    ///
    /// The outgoing table fragment is finalized from the committed boundary
    /// before `apply_forced_break` performs the paged-media page transition.
    /// The common destination-start handoff then replays repeated headers and
    /// returns the resulting cursor, matching unforced table transitions:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn apply_table_body_forced_break(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        context: &mut TableBodyFragmentCommitContext<'_, '_>,
        pending_fragmentainer_placement: &mut TableFragmentainerPlacement,
        decision: TableForcedBreakDecision,
        source_row_has_occupied_cell: bool,
    ) -> Option<PageTopBlockPosition> {
        self.commit_table_body_fragment_boundary(fragment, context, decision.boundary);
        let cursor_top = self.materialize_table_fragmentainer_advance(
            decision.fragmentainer_kind,
            FragmentainerAdvance::Forced(decision.page_break),
        )?;
        Some(PageTopBlockPosition::new(self.start_table_body_fragment(
            context,
            pending_fragmentainer_placement,
            decision.start,
            cursor_top.points(),
            source_row_has_occupied_cell,
        )))
    }

    /// Advance a table body to a committed destination fragmentainer.
    ///
    /// Table rows own their page transition directly, so they do not naturally
    /// re-enter the root/body canvas as ordinary block flow does. Replay the
    /// same continuation geometry after the destination page has been chosen
    /// before table chrome or row coordinates are calculated.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout::table) fn materialize_table_fragmentainer_advance(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        advance: FragmentainerAdvance,
    ) -> Option<PageTopBlockPosition> {
        let continuation = (fragmentainer_kind == FragmentainerKind::Page)
            .then(|| self.fragment_continuation_context());
        self.materialize_fragmentainer_advance(fragmentainer_kind, advance)?;
        if let Some(continuation) = continuation {
            self.replay_fragment_continuation_on_page(&continuation, self.current_page_context);
        }
        Some(PageTopBlockPosition::new(self.cursor_y))
    }

    fn effective_table_row_break_before(
        row_index: usize,
        row_style: &ComputedStyle,
        row_group_break_before: &[PageBreak],
    ) -> PageBreak {
        if row_group_break_before[row_index] != PageBreak::Auto {
            row_group_break_before[row_index]
        } else {
            row_style.break_before
        }
    }

    fn effective_table_row_break_after(
        row_index: usize,
        row_style: &ComputedStyle,
        row_group_break_after: &[PageBreak],
    ) -> PageBreak {
        if row_style.break_after != PageBreak::Auto {
            row_style.break_after
        } else {
            row_group_break_after[row_index]
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn table_row_fragment_decision(
        &self,
        fragment: Option<&TableBodyPaintFragment>,
        table_x: f32,
        used_table_width: f32,
        row_index: usize,
        row_top: f32,
        row_height: f32,
        row_offset: f32,
        original_row_height: f32,
        collapsed: bool,
        fragment_mode: TableRowFragmentMode,
    ) -> TableRowFragmentDecision {
        let starts_page_fragment = !self.current_page_has_content();
        let assignment_placement = fragment.map(|fragment| {
            Self::table_row_fragment_assignment_placement(
                fragment,
                table_x,
                used_table_width,
                row_top,
                row_height,
                starts_page_fragment,
            )
        });
        TableRowFragmentDecision {
            row_index,
            row_top,
            row_height,
            row_offset,
            original_row_height,
            collapsed,
            fragment_mode,
            assignment_placement,
            source_fragment: Self::table_row_source_fragment(assignment_placement),
        }
    }

    fn capture_table_row_fragment_decision_assignments(
        &mut self,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_x: f32,
        decision: TableRowFragmentDecision,
    ) {
        let Some(placement) = decision.assignment_placement else {
            return;
        };
        self.capture_table_row_named_string_assignments(
            row,
            row_style,
            placement,
            decision.row_offset,
        );
        self.capture_table_row_running_cell_assignments(
            row,
            row_style,
            stylesheets,
            table_x,
            decision.row_top,
            decision.row_offset,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_committed_table_row_fragment(
        &mut self,
        table_body_fragment: &mut Option<TableBodyPaintFragment>,
        decision: TableRowFragmentDecision,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<TableCellPadding>,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        source_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_height_is_definite: bool,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        row_baseline_offset: Option<TableRowBaselineOffset>,
        source_grid_placement: TableGridPlacement,
        root_background_source_grid_placement: TableGridPlacement,
        wrapper_timeline: TableWrapperFragmentTimeline,
    ) {
        let follows_repeated_header = table_body_fragment.as_ref().is_some_and(|fragment| {
            !fragment.plan.repeated_header_rows.is_empty() && fragment.plan.body_rows.is_empty()
        });
        let grid_placement = table_body_fragment.as_mut().map(|fragment| {
            fragment.initialize_grid_placement(
                table_style,
                column_plan,
                source_grid_placement,
                root_background_source_grid_placement,
                wrapper_timeline.clone(),
                planned_row_heights,
                planned_row_occupancy,
                table_metrics.clone(),
            )
        });
        let destination_row_block_start = table_body_fragment.as_ref().and_then(|fragment| {
            fragment
                .grid_viewport
                .as_ref()
                .and_then(|viewport| viewport.next_destination_block_start(decision))
        });
        if let Some(fragment) = table_body_fragment.as_mut() {
            fragment.record_grid_row_slice_for_paint(decision);
        }
        self.layout_table_row_paint_piece(
            decision.row_index,
            row,
            row_style,
            rows,
            grid,
            table_style,
            stylesheets,
            table_x,
            used_table_width,
            table_cellpadding,
            column_plan,
            planned_row_heights,
            source_row_heights,
            planned_row_occupancy,
            table_height_is_definite,
            table_metrics,
            grid_placement,
            destination_row_block_start,
            decision.row_top,
            decision.original_row_height,
            decision.row_height,
            decision.row_offset,
            decision.fragment_mode,
            follows_repeated_header,
            collapsed_geometry,
            row_baseline_offset,
        );
        if let Some(fragment) = table_body_fragment {
            fragment.push_row_decision(decision);
        }
    }

    /// Returns the first and last CSS `page` values represented by a table row.
    ///
    /// CSS Paged Media applies named pages at class A break opportunities, and
    /// CSS Tables preserves rows and cells as internal table boxes whose
    /// descendants can carry `page` values:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/CSS22/tables.html#model>.
    fn table_row_page_boundary_values(
        &mut self,
        row_index: usize,
        row_group_end: usize,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        table_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
    ) -> ResolvedPageBoundaryValues {
        let inherited_page_name = self.active_page_value_scope(table_style);
        let own_page_name = if row_style.page.is_specified() {
            row_style
                .page
                .specified_name()
                .map(|name| name.as_str().to_string())
                .or_else(|| inherited_page_name.clone())
        } else {
            inherited_page_name.clone()
        };
        let mut values = ResolvedPageBoundaryValues {
            start: own_page_name.clone(),
            end: own_page_name.clone(),
        };
        if row_style.page.is_specified() {
            return values;
        }

        let mut first_cell = None;
        let mut last_cell = None;
        let mut continuing_rowspan = None;
        for cell in &row.cells {
            let cell_style = self.style_for_table_cell(cell, row, row_style, stylesheets);
            if !style_is_in_normal_flow(&cell_style) {
                continue;
            }
            let child_boxes = cell.children.as_deref().unwrap_or_default();
            let cell_sources = page_value_sources_from_style_and_children(&cell_style, child_boxes);
            let cell_values = resolved_page_boundary_values_from_style_and_children(
                &cell_style,
                child_boxes,
                own_page_name.as_deref(),
            );
            if cell_sources.end.overrides_parent_summary()
                && cell.element.is_some_and(|element| {
                    html_table_rowspan(element, row_index, row_group_end) > 1
                })
            {
                continuing_rowspan = Some((cell_values.end.clone(), cell_sources.end.clone()));
            }
            if first_cell.is_none() {
                first_cell = Some((cell_values.start.clone(), cell_sources.start.clone()));
            }
            last_cell = Some((cell_values.end, cell_sources.end));
        }
        if let Some((cell_start, source)) = first_cell
            && source.overrides_parent_summary()
        {
            values.start = cell_start;
        }
        if let Some((cell_end, source)) = last_cell
            && source.overrides_parent_summary()
        {
            values.end = cell_end;
        }
        if values.end == own_page_name
            && let Some((rowspan_end, _source)) = continuing_rowspan
        {
            values.end = rowspan_end;
        }
        if values.start == own_page_name
            && let Some(group) = row.row_groups.last()
        {
            let group_style = self.style_for_table_row_group(group, table_style, stylesheets);
            if group_style.page.is_specified() {
                let group_value = group_style
                    .page
                    .specified_name()
                    .map(|name| name.as_str().to_string())
                    .or_else(|| inherited_page_name.clone());
                values.start = group_value.clone();
                if values.end == own_page_name {
                    values.end = group_value;
                }
            }
        }
        values
    }

    /// Returns the final GCPM assignment placement for a visible table row fragment.
    ///
    /// Table rows are internal table boxes, but CSS Fragmentation still exposes
    /// their page-local fragments as the source positions for generated paged
    /// media such as named strings:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/css-gcpm-3/#setting-named-strings>.
    fn table_row_fragment_assignment_placement(
        fragment: &TableBodyPaintFragment,
        table_x: f32,
        used_table_width: f32,
        row_top: f32,
        row_height: f32,
        starts_page_fragment: bool,
    ) -> AssignmentPlacement {
        AssignmentPlacement {
            page_index: fragment.plan.page_index,
            starts_page_fragment,
            border_box: Some(
                PageTopRect::new(table_x, row_top, used_table_width, row_height).paint_clip(),
            ),
        }
    }

    fn table_row_source_fragment(placement: Option<AssignmentPlacement>) -> TableRowSourceFragment {
        TableRowSourceFragment {
            border_box: placement.and_then(|placement| placement.border_box),
            starts_page_fragment: placement.is_some_and(|placement| placement.starts_page_fragment),
        }
    }

    /// Returns the zero-size source marker for a running table row.
    ///
    /// CSS GCPM removes `position: running()` boxes from normal flow while
    /// keeping their source position for `element(..., start)` resolution:
    /// <https://www.w3.org/TR/css-gcpm-3/#running-elements>.
    fn table_row_running_assignment_placement(
        &self,
        table_x: f32,
        row_top: f32,
    ) -> AssignmentPlacement {
        AssignmentPlacement {
            page_index: self.pages.len(),
            starts_page_fragment: !self.current_page_has_content(),
            border_box: Some(PageTopRect::new(table_x, row_top, 0.0, 0.0).paint_clip()),
        }
    }

    fn capture_table_row_named_string_assignments(
        &mut self,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        placement: AssignmentPlacement,
        row_offset: f32,
    ) {
        if row_offset > 0.01 {
            return;
        }
        let Some(element) = row.element else {
            return;
        };
        self.capture_named_strings_for_fragment_source(element, row_style, placement);
    }

    /// Captures `position: running()` cells removed from a table row.
    ///
    /// CSS GCPM removes running elements from normal flow while retaining their
    /// source position for `element(..., start)` lookup. Running table cells are
    /// filtered out before table grid construction, so the row's first emitted
    /// fragment provides the durable page-local source marker:
    /// <https://www.w3.org/TR/css-gcpm-3/#running-elements> and
    /// <https://drafts.csswg.org/css-tables-3/#cell-assignment>.
    fn capture_table_row_running_cell_assignments(
        &mut self,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_x: f32,
        row_top: f32,
        row_offset: f32,
    ) {
        if row_offset > 0.01 || row.running_cells.is_empty() {
            return;
        }
        let placement = AssignmentPlacement {
            page_index: self.pages.len(),
            starts_page_fragment: !self.current_page_has_content(),
            border_box: Some(PageTopRect::new(table_x, row_top, 0.0, 0.0).paint_clip()),
        };
        for cell in &row.running_cells {
            let Some(element) = cell.element else {
                continue;
            };
            let cell_style = self.style_for_table_cell(cell, row, row_style, stylesheets);
            self.capture_assignments_for_fragment_source(element, &cell_style, placement);
        }
    }
}
