//! Fragmentation plans, placements, and body replay state.

use super::*;
use crate::layout::inline_models::FragmentainerPlacement;

/// The table-facing view of an enclosing multicolumn fragmentainer.
///
/// A table grid has its own [`TableGridPlacement`] and may be orthogonal to
/// its parent. It must therefore never use its grid axes to select a parent
/// continuation. This wrapper exposes the parent sequence's already-selected
/// capacity, destination rectangle, and logical block edges without exposing
/// the scratch `PageContext` used to lay out the source column.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flow>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableOuterFragmentainerPlacement {
    placement: FragmentainerPlacement,
}

impl TableOuterFragmentainerPlacement {
    pub(in crate::layout::table) fn from_outer(placement: FragmentainerPlacement) -> Self {
        Self { placement }
    }

    pub(in crate::layout::table) fn axes(self) -> FlowAxes {
        self.placement.flow_axes()
    }

    pub(in crate::layout::table) fn ordinal(self) -> usize {
        self.placement.ordinal()
    }

    pub(in crate::layout::table) fn destination_rect(self) -> PageTopRect {
        self.placement.content_rect()
    }

    /// The destination paint clip selected by the enclosing fragmentainer.
    ///
    /// Wrapper siblings use this rather than rebuilding a physical clip from
    /// table-grid coordinates: an orthogonal table's grid rectangle is not
    /// an outer column rectangle.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout::table) fn destination_clip(self) -> PaintClip {
        self.destination_rect().paint_clip()
    }

    pub(in crate::layout::table) fn logical_block_capacity(self) -> f32 {
        self.placement.logical_block_capacity()
    }

    /// Logical block coordinates used by table row decisions decrease toward
    /// block-end, including in vertical-lr parent flow.
    pub(in crate::layout::table) fn block_start(self) -> TableFragmentainerBlockStart {
        let destination = self.destination_rect();
        debug_assert!(destination.width() >= 0.0 && destination.height() >= 0.0);
        let edge = self.placement.block_start_edge();
        TableFragmentainerBlockStart::new(match self.axes().block_start_side() {
            PhysicalSide::Left => -edge,
            PhysicalSide::Top | PhysicalSide::Bottom | PhysicalSide::Right => edge,
        })
    }

    pub(in crate::layout::table) fn block_end(self) -> TableFragmentainerBlockStart {
        let destination = self.destination_rect();
        debug_assert!(destination.width() >= 0.0 && destination.height() >= 0.0);
        let edge = self.placement.block_end_edge();
        TableFragmentainerBlockStart::new(match self.axes().block_start_side() {
            PhysicalSide::Left => -edge,
            PhysicalSide::Top | PhysicalSide::Bottom | PhysicalSide::Right => edge,
        })
    }
}

/// The logical block-start coordinate of a committed table destination
/// fragmentainer.
///
/// This is deliberately distinct from a page-top Y coordinate: vertical table
/// roots fragment along physical X, so their block start may be a signed
/// physical-X projection rather than a page-top position.
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableFragmentainerBlockStart(f32);

impl TableFragmentainerBlockStart {
    pub(in crate::layout::table) const fn new(value: f32) -> Self {
        Self(value)
    }

    pub(in crate::layout::table) fn points(self) -> f32 {
        self.0
    }
}

/// The durable physical and logical placement of one table destination
/// fragmentainer.
///
/// Table row layout, structural background projection, wrapper decoration,
/// and caption replay must share this value. A source row offset is never a
/// valid replacement for either the physical table X coordinate or the
/// fragmentainer's logical block start.
/// <https://drafts.csswg.org/css-tables-3/#table-fragmentation>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableFragmentainerPlacement {
    /// Fragment-local page origin of the destination grid and row boxes.
    /// This is one typed coordinate rather than independently-maintained X
    /// and top values; it rebases as a table crosses columns/pages.
    pub(in crate::layout::table) destination_grid_origin: PageTopPoint,
    /// Capture origin for wrapper siblings before their committed replay.
    /// It remains typed and is never reconstructed from a source row.
    pub(in crate::layout::table) wrapper_table_x: PageInlinePosition,
    pub(in crate::layout::table) block_start: TableFragmentainerBlockStart,
    pub(in crate::layout::table) block_span: LogicalBlockContentSize,
    pub(in crate::layout::table) writing_mode: WritingMode,
    /// The selected enclosing multicolumn fragmentainer, when this table is
    /// laid out in one. Page-only table fragmentation has no such outer
    /// placement. Retaining the complete value prevents wrapper captions and
    /// grid rows with equal scratch geometry from being treated as one target
    /// and keeps table-local grid axes from replacing outer capacity/edges.
    pub(in crate::layout::table) outer_fragmentainer: Option<TableOuterFragmentainerPlacement>,
}

impl TableFragmentainerPlacement {
    pub(in crate::layout::table) fn horizontal(
        table_x: PageInlinePosition,
        top: PageTopBlockPosition,
        block_span: LogicalBlockContentSize,
    ) -> Self {
        Self {
            destination_grid_origin: PageTopPoint::from_inline_x_and_block_position(
                table_x.points(),
                top,
            ),
            wrapper_table_x: table_x,
            block_start: TableFragmentainerBlockStart::new(top.points()),
            block_span,
            writing_mode: WritingMode::HorizontalTb,
            outer_fragmentainer: None,
        }
    }

    pub(in crate::layout::table) fn vertical_lr(
        table_x: PageInlinePosition,
        paint_top: PageTopBlockPosition,
        block_start: TableFragmentainerBlockStart,
        block_span: LogicalBlockContentSize,
    ) -> Self {
        Self {
            destination_grid_origin: PageTopPoint::from_inline_x_and_block_position(
                table_x.points(),
                paint_top,
            ),
            wrapper_table_x: table_x,
            block_start,
            block_span,
            writing_mode: WritingMode::VerticalLr,
            outer_fragmentainer: None,
        }
    }

    pub(in crate::layout::table) fn vertical_rl(
        table_x: PageInlinePosition,
        paint_top: PageTopBlockPosition,
        block_start: TableFragmentainerBlockStart,
        block_span: LogicalBlockContentSize,
    ) -> Self {
        Self {
            destination_grid_origin: PageTopPoint::from_inline_x_and_block_position(
                table_x.points(),
                paint_top,
            ),
            wrapper_table_x: table_x,
            block_start,
            block_span,
            writing_mode: WritingMode::VerticalRl,
            outer_fragmentainer: None,
        }
    }

    /// The immutable page origin of this fragment's destination cell grid.
    pub(in crate::layout::table) fn destination_grid_origin(self) -> PageTopPoint {
        self.destination_grid_origin
    }

    /// Rebase the physical grid origin while retaining the wrapper-selected
    /// outer fragmentainer and its logical block interval.  The wrapper owns
    /// the continuation choice; the table grid supplies only its distinct
    /// content-box origin inside that selected destination.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout::table) fn with_destination_grid_origin(
        mut self,
        origin: PageTopPoint,
    ) -> Self {
        self.destination_grid_origin = origin;
        self
    }

    pub(in crate::layout::table) fn with_wrapper_table_x(
        mut self,
        wrapper_table_x: PageInlinePosition,
    ) -> Self {
        self.wrapper_table_x = wrapper_table_x;
        self
    }

    pub(in crate::layout::table) fn with_outer_fragmentainer(
        mut self,
        outer: Option<TableOuterFragmentainerPlacement>,
    ) -> Self {
        self.outer_fragmentainer = outer;
        self
    }

    /// Select an enclosing fragmentainer as this table fragment's authority.
    ///
    /// Unlike [`Self::with_outer_fragmentainer`], this is used when wrapper
    /// progress selects a different outer ordinal. The selected placement
    /// consequently replaces both the capacity and the logical block edges;
    /// retaining those fields from an earlier ordinal would let the first row
    /// immediately transition back into the ambient scratch fragmentainer.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout::table) fn select_outer_fragmentainer(
        mut self,
        outer: TableOuterFragmentainerPlacement,
    ) -> Self {
        self.block_start = outer.block_start();
        self.block_span =
            LogicalBlockContentSize::new(content_box_pt(outer.logical_block_capacity()));
        self.outer_fragmentainer = Some(outer);
        self
    }

    pub(in crate::layout::table) fn outer_fragmentainer_ordinal(self) -> Option<usize> {
        self.outer_fragmentainer
            .map(TableOuterFragmentainerPlacement::ordinal)
    }

    pub(in crate::layout::table) fn outer_fragmentainer(
        self,
    ) -> Option<TableOuterFragmentainerPlacement> {
        self.outer_fragmentainer
    }

    pub(in crate::layout::table) fn wrapper_table_x(self) -> PageInlinePosition {
        self.wrapper_table_x
    }

    /// Advance a wrapper-flow sibling through this placement's continuous
    /// logical block source coordinate.
    ///
    /// This is not a fragmentainer transition: an enclosing multicolumn
    /// formatter will later clip and replay the continuous source interval.
    /// It prevents a following caption from reusing the grid's opening
    /// physical X when an unbroken vertical grid overflows its first column.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    /// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
    pub(in crate::layout::table) fn advance_wrapper_source_block(
        self,
        span: TableGridLength,
    ) -> Self {
        let offset = span.get().max(0.0);
        let x_offset = match self.writing_mode {
            WritingMode::HorizontalTb => 0.0,
            WritingMode::VerticalLr | WritingMode::SidewaysLr => offset,
            WritingMode::VerticalRl | WritingMode::SidewaysRl => -offset,
        };
        Self {
            destination_grid_origin: PageTopPoint::new(
                self.destination_grid_origin.x() + x_offset,
                self.destination_grid_origin.top_y(),
            ),
            wrapper_table_x: PageInlinePosition::new(self.wrapper_table_x.points() + x_offset),
            ..self
        }
    }

    pub(in crate::layout::table) fn paint_top(self) -> PageTopBlockPosition {
        PageTopBlockPosition::new(self.destination_grid_origin.top_y())
    }

    pub(in crate::layout::table) fn block_start(self) -> TableFragmentainerBlockStart {
        self.block_start
    }

    /// Return how far a committed destination grid begins after this
    /// fragmentainer's logical block start.  This is the only bridge used by
    /// wrapper decoration to turn a fragmentainer placement into a logical
    /// progress value; horizontal and vertical roots therefore cannot leak a
    /// physical page-Y cursor into sliced decoration.
    ///
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    pub(in crate::layout::table) fn grid_block_progress(
        self,
        grid: TableGridPlacement,
    ) -> TableGridBlockOffset {
        let rect = grid.full_page_top_rect();
        let progress = match self.writing_mode {
            WritingMode::HorizontalTb => self.block_start.points() - rect.top_y(),
            WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                -rect.x() - self.block_start.points()
            }
            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                self.block_start.points() - (rect.x() + rect.width())
            }
        };
        TableGridBlockOffset::new(TableGridLength::new(progress.max(0.0)))
    }

    /// Return the physical Y coordinate at which wrapper-owned trailing
    /// chrome is replayed after this fragment's final source-row slice.
    ///
    /// Horizontal roots consume physical Y as their logical block axis, so
    /// the row fragment's block end determines the trailing edge. Vertical
    /// roots retain the committed inline-axis Y origin: their logical block
    /// transition is represented by [`Self::block_start`] instead.
    pub(in crate::layout::table) fn trailing_paint_top(
        self,
        body_fragment_bottom: PageTopBlockPosition,
        table_inline_span: LogicalInlineContentSize,
    ) -> PageTopBlockPosition {
        match self.writing_mode {
            WritingMode::HorizontalTb => body_fragment_bottom,
            WritingMode::VerticalLr
            | WritingMode::SidewaysLr
            | WritingMode::VerticalRl
            | WritingMode::SidewaysRl => {
                PageTopBlockPosition::new(self.paint_top().points() - table_inline_span.points())
            }
        }
    }

    /// Construct the grid placement for this destination fragmentainer.
    ///
    /// The source grid remains immutable; this placement supplies only the
    /// destination fragmentainer's physical inline origin and logical block
    /// start/span.
    pub(in crate::layout::table) fn destination_grid_placement(
        self,
        _table_metrics: &TableMetrics,
        _planned_row_occupancy: &[bool],
        axes: TableAxes,
        logical_size: TableGridLogicalSize,
    ) -> TableGridPlacement {
        TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(self.destination_grid_origin),
            axes,
            // The destination placement supplies a fragment-local origin;
            // it does not change the table grid's logical extent.  Retaining
            // the fragmentainer span here makes an unfragmented root table
            // paint its wrapper through the entire page instead of through
            // its measured row grid.
            logical_size,
        )
    }
}

/// Table fragment selected before paint replay.
///
/// CSS Fragmentation splits a table wrapper into fragmentainer-local pieces,
/// while CSS Tables keeps row, column, and collapsed-border geometry tied to
/// the source table grid. This plan is the durable bridge between those models
/// and records the target fragmentainer kind separately from the current
/// page-backed metadata index.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#model>.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableFragmentPlan {
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) page_index: usize,
    pub(in crate::layout::table) placement: TableFragmentainerPlacement,
    pub(in crate::layout::table) start_decision: TableFragmentStartDecision,
    pub(in crate::layout::table) outgoing_boundary: Option<TableFragmentBoundaryDecision>,
    pub(in crate::layout::table) repeated_header_rows: Vec<usize>,
    pub(in crate::layout::table) body_rows: Vec<TableRowPiecePlan>,
    pub(in crate::layout::table) repeated_footer_rows: Vec<usize>,
    pub(in crate::layout::table) metadata: FragmentPageMetadata,
}

impl TableFragmentPlan {
    pub(in crate::layout::table) fn new(
        fragmentainer_kind: FragmentainerKind,
        page_index: usize,
        placement: TableFragmentainerPlacement,
        start_decision: TableFragmentStartDecision,
    ) -> Self {
        Self {
            fragmentainer_kind,
            page_index,
            placement,
            start_decision,
            outgoing_boundary: None,
            repeated_header_rows: Vec::new(),
            body_rows: Vec::new(),
            repeated_footer_rows: Vec::new(),
            metadata: FragmentPageMetadata::new(
                page_index,
                None,
                start_decision.break_reason == TableFragmentBreakReason::TableStart,
            ),
        }
    }

    pub(in crate::layout::table) fn push_body_row(&mut self, row: TableRowPiecePlan) {
        if self.metadata.source_border_box.is_none() {
            self.metadata.source_border_box = row.metadata.source_border_box;
        }
        if self.body_rows.is_empty() {
            self.metadata.starts_page_fragment = row.metadata.starts_page_fragment;
            self.metadata.first_page_value = row.metadata.first_page_value.clone();
        }
        self.metadata.continues_from_previous_page |= row.metadata.continues_from_previous_page;
        self.metadata.continues_to_next_page |= row.metadata.continues_to_next_page;
        self.metadata.last_page_value = row.metadata.last_page_value.clone();
        self.metadata
            .assignment_ids
            .extend(row.metadata.assignment_ids.iter().cloned());
        self.body_rows.push(row);
    }

    pub(in crate::layout::table) fn bottom(&self) -> f32 {
        self.body_rows
            .last()
            .map(TableRowPiecePlan::bottom)
            .unwrap_or(self.placement.paint_top().points())
    }

    pub(in crate::layout::table) fn break_reason(&self) -> TableFragmentBreakReason {
        self.start_decision.break_reason
    }
}

/// Why a planned table fragmentainer piece starts at this location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableFragmentBreakReason {
    TableStart,
    Forced,
    AvoidedOverflow,
    Overflow,
    OversizedRowSlice,
}

/// One visible source-row slice inside a table fragmentainer piece.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableRowPiecePlan {
    pub(in crate::layout::table) row_index: usize,
    pub(in crate::layout::table) row_top: f32,
    pub(in crate::layout::table) row_height: f32,
    pub(in crate::layout::table) row_offset: f32,
    pub(in crate::layout::table) original_row_height: f32,
    pub(in crate::layout::table) collapsed: bool,
    pub(in crate::layout::table) fragment_mode: TableRowFragmentMode,
    pub(in crate::layout::table) metadata: FragmentPageMetadata,
}

/// Committed decision for one table-row fragment before row painting.
///
/// CSS Fragmentation picks a row fragment and its source placement before the
/// row's table-cell descendants are painted. Keeping that decision separate
/// from `TableRowPiecePlan` lets pagination choose, paint, and then record the
/// same row fragment without recomputing source placement in each step:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRowFragmentDecision {
    pub(in crate::layout::table) row_index: usize,
    pub(in crate::layout::table) row_top: f32,
    pub(in crate::layout::table) row_height: f32,
    pub(in crate::layout::table) row_offset: f32,
    pub(in crate::layout::table) original_row_height: f32,
    pub(in crate::layout::table) collapsed: bool,
    pub(in crate::layout::table) fragment_mode: TableRowFragmentMode,
    pub(in crate::layout::table) assignment_placement: Option<AssignmentPlacement>,
    pub(in crate::layout::table) source_fragment: TableRowSourceFragment,
}

/// Committed table-row fragmentation mode for a fragmentainer-local row piece.
///
/// CSS Fragmentation chooses breaks for the table row group before descendants
/// are painted. Table-cell content must therefore consume the committed row
/// fragment instead of independently advancing to another fragmentainer:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/css-break-3/#break-within>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableRowFragmentMode {
    Whole,
    Sliced,
    /// A table wrapper can expose consecutive outer fragmentainer slices of
    /// an otherwise monolithic empty row. The row has no descendant break
    /// opportunity, so this mode replays only table-root structural paint;
    /// it never treats the cell contents as independently fragmented.
    ///
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>
    /// <https://drafts.csswg.org/css-tables-3/#table-fragmentation>
    DecorationOnly,
    KeptByAvoidOverflow,
}

impl TableRowFragmentMode {
    pub(in crate::layout::table) fn clips_to_row_piece(self) -> bool {
        matches!(self, Self::Sliced | Self::DecorationOnly)
    }

    pub(in crate::layout::table) fn replays_flow_children_from_plan(self) -> bool {
        matches!(self, Self::Sliced | Self::KeptByAvoidOverflow)
    }

    pub(in crate::layout::table) fn is_decoration_only(self) -> bool {
        self == Self::DecorationOnly
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRowSourceFragment {
    pub(in crate::layout::table) border_box: Option<PaintClip>,
    pub(in crate::layout::table) starts_page_fragment: bool,
}

impl TableRowPiecePlan {
    pub(in crate::layout::table) fn bottom(&self) -> f32 {
        self.row_top - self.row_height
    }
}

/// Cell-level geometry consumed while painting a planned row piece.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableCellFragmentPlan {
    pub(in crate::layout::table) border_box: TableCellBorderBox,
    pub(in crate::layout::table) placement: TableGridPlacement,
    /// The final content rectangle, including alignment in the cell's own
    /// block axis.  Root-table grid geometry is deliberately not reused as a
    /// substitute for this cell-local containing block.
    pub(in crate::layout::table) content_geometry: TableCellContentGeometry,
    pub(in crate::layout::table) content_clip: Option<TableCellClipRegion>,
    pub(in crate::layout::table) area: TableGridArea,
    pub(in crate::layout::table) content: TableCellContentPlan,
}

/// Visible table-cell areas retained across collapsed row/column tracks.
///
/// This stays in table-grid layout until the final retained-paint boundary,
/// where its rectangles become one PDF union clip path.
#[derive(Debug, Clone, Default)]
pub(in crate::layout::table) struct TableCellClipRegion {
    regions: Vec<OverflowClip>,
}

impl TableCellClipRegion {
    pub(in crate::layout::table) fn from_clip(clip: OverflowClip) -> Self {
        Self {
            regions: vec![clip],
        }
    }

    pub(in crate::layout::table) fn from_clips(regions: Vec<OverflowClip>) -> Option<Self> {
        (!regions.is_empty()).then_some(Self { regions })
    }

    pub(in crate::layout::table) fn intersect(&self, other: &Self) -> Option<Self> {
        Self::from_clips(
            self.regions
                .iter()
                .flat_map(|left| {
                    other
                        .regions
                        .iter()
                        .filter_map(move |right| left.intersect(*right))
                })
                .collect(),
        )
    }

    pub(in crate::layout::table) fn bounding_clip(&self) -> Option<OverflowClip> {
        let first = *self.regions.first()?;
        let mut min_x = first.paint_rect().min_x();
        let mut min_y = first.paint_rect().min_y();
        let mut max_x = first.paint_rect().max_x();
        let mut max_y = first.paint_rect().max_y();
        for clip in &self.regions[1..] {
            let rect = clip.paint_rect();
            min_x = min_x.min(rect.min_x());
            min_y = min_y.min(rect.min_y());
            max_x = max_x.max(rect.max_x());
            max_y = max_y.max(rect.max_y());
        }
        Some(
            OverflowClip::from_paint_rect(paint_space_rect(
                min_x,
                min_y,
                max_x - min_x,
                max_y - min_y,
            ))
            .with_axes_and_non_scrollable(
                self.regions.iter().any(|clip| clip.clips_x),
                self.regions.iter().any(|clip| clip.clips_y),
                self.regions.iter().any(|clip| clip.non_scrollable_x),
                self.regions.iter().any(|clip| clip.non_scrollable_y),
            ),
        )
    }

    pub(in crate::layout::table) fn paint_clips(&self) -> Vec<PaintClip> {
        self.regions
            .iter()
            .map(|clip| PaintClip::from_paint_rect(clip.paint_rect()))
            .collect()
    }
}

impl TableCellFragmentPlan {
    pub(in crate::layout::table) fn x(&self) -> f32 {
        self.border_box.x(self.placement)
    }

    pub(in crate::layout::table) fn top_y(&self) -> f32 {
        self.border_box.top_y(self.placement)
    }

    pub(in crate::layout::table) fn width(&self) -> f32 {
        self.border_box.page_top_rect(self.placement).width()
    }

    pub(in crate::layout::table) fn height(&self) -> f32 {
        self.border_box.page_top_rect(self.placement).height()
    }

    pub(in crate::layout::table) fn content_box(&self) -> TableCellContentBox {
        self.content_geometry.content_box()
    }
}

/// Planned table-cell content for one fragmentainer-local row piece.
///
/// CSS table-cell contents are laid out in a block container, but CSS
/// Fragmentation clips and paints only the content visible in each table row
/// piece. This plan records those fragment-local content decisions before paint:
/// <https://www.w3.org/TR/CSS22/tables.html#model> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableCellContentPlan {
    pub(in crate::layout::table) inline_sequence: Option<inline_layout::InlineLineSequence>,
    pub(in crate::layout::table) child_fragments: Vec<TableCellChildFragmentPlan>,
    /// The source and painted block spans represented by this cell piece.
    ///
    /// A row fragment is not merely a height clipped from the original row:
    /// its cell contents retain their source-child range and continuation
    /// state so subsequent fragmentainers can resume at a legal child
    /// boundary.  CSS Fragmentation breaks table-cell block contents at the
    /// same class-C opportunities as an ordinary block container:
    /// <https://www.w3.org/TR/css-break-3/#break-within>.
    pub(in crate::layout::table) fragment_range: Option<TableCellFragmentRange>,
    pub(in crate::layout::table) children_painted_by_inline_sequence: bool,
}

impl TableCellContentPlan {
    /// Return the final inline fragment span on the cell's logical block axis.
    ///
    /// CSS Tables aligns a cell's actual content fragment, after its inline
    /// constraint has formed lines, rather than an unconstrained intrinsic
    /// probe.  Orthogonal cells map this span to physical width; keeping the
    /// projection on the planned line sequence prevents a root-table track
    /// metric from leaking back into cell alignment.
    /// <https://drafts.csswg.org/css-tables-3/#table-cell-content-layout-second-pass>
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    pub(in crate::layout::table) fn logical_block_subject_size(
        &self,
        cell_style: &ComputedStyle,
    ) -> f32 {
        let Some(sequence) = &self.inline_sequence else {
            return 0.0;
        };
        match cell_style.writing_mode {
            WritingMode::HorizontalTb => sequence.total_height(),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => {
                // The vertical line stack reserves a full `line-height` for
                // its final line, but its last typographic unit occupies only
                // the font-size block extent. Align the actual inline-text
                // subject rather than treating that trailing leading as
                // content. For one upright line this is its glyph's em box;
                // for several lines the inter-line distances remain intact.
                // <https://www.w3.org/TR/css-inline-3/#line-height-property>
                (sequence.total_height() - (cell_style.line_height - cell_style.font_size).max(0.0))
                    .max(0.0)
            }
        }
    }
}

impl TableCellContentPlan {
    pub(in crate::layout::table) fn empty() -> Self {
        Self {
            inline_sequence: None,
            child_fragments: Vec::new(),
            fragment_range: None,
            children_painted_by_inline_sequence: false,
        }
    }
}

/// Child-aware source range represented by one table-cell fragment.
///
/// The coordinates remain in the source cell's block coordinate system while
/// `painted_*` records the portion selected for the destination fragmentainer.
/// Keeping both spans explicit prevents continuation paint from inferring its
/// state solely from a row-height slice.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableCellFragmentRange {
    pub(in crate::layout::table) source_child_start: usize,
    pub(in crate::layout::table) source_child_end: usize,
    pub(in crate::layout::table) source_block_top: f32,
    pub(in crate::layout::table) source_block_bottom: f32,
    pub(in crate::layout::table) painted_block_top: f32,
    pub(in crate::layout::table) painted_block_bottom: f32,
    pub(in crate::layout::table) continues_from_previous: bool,
    pub(in crate::layout::table) continues_to_next: bool,
}

/// One planned in-flow table-cell child slice for a split row piece.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableCellChildFragmentPlan {
    pub(in crate::layout::table) source_child_index: usize,
    /// Block-start edge in the source cell's coordinate system. This is used
    /// only to decide which source child interval intersects a row piece.
    pub(in crate::layout::table) source_child_top: f32,
    /// Block-start edge in the destination fragmentainer's coordinate system.
    ///
    /// A continuation piece restarts at the new row's block-start edge, so it
    /// cannot paint using `source_child_top` directly.
    pub(in crate::layout::table) painted_child_top: f32,
    pub(in crate::layout::table) child_height: f32,
    pub(in crate::layout::table) slice_top: f32,
    pub(in crate::layout::table) slice_bottom: f32,
    pub(in crate::layout::table) kind: TableCellChildFragmentKind,
    pub(in crate::layout::table) inline_sequence: Option<TableCellNestedInlineSequencePlan>,
    pub(in crate::layout::table) nested_fragment: Option<TableCellNestedFragmentPlan>,
    pub(in crate::layout::table) metadata: FragmentPageMetadata,
}

/// Sequence-backed inline content for a nested table-cell slice.
///
/// CSS Text line selection and CSS Fragmentation slicing should consume the
/// same graph-selected line records even when inline content is nested under
/// table-cell split-row replay:
/// <https://www.w3.org/TR/css-text-3/#line-breaking> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableCellNestedInlineSequencePlan {
    pub(in crate::layout::table) sequence: inline_layout::InlineLineSequence,
    pub(in crate::layout::table) style: ComputedStyle,
}

/// Pre-rendered table-cell nested formatting context for split row replay.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableCellNestedFragmentPlan {
    pub(in crate::layout::table) fragment: PaintFragment,
    pub(in crate::layout::table) width: f32,
    pub(in crate::layout::table) height: f32,
    pub(in crate::layout::table) metadata: FragmentPageMetadata,
    pub(in crate::layout::table) assignments: Vec<CapturedPageAssignment>,
}

/// Coarse child kind used to route planned table-cell fragment painting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableCellChildFragmentKind {
    Block,
    AnonymousBlock,
    Inline,
    Text,
    AtomicInline,
    Replaced,
    NestedFormattingContext,
}

/// Fragment-local body-row paint capture for one fragmented table piece.
///
/// CSS Fragmentation splits the table wrapper into fragmentainer pieces while
/// CSS 2.2 Appendix E still requires the rows, borders, and positioned
/// descendants in each fragment to paint as one ordered table unit.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
/// <https://www.w3.org/TR/CSS22/zindex.html>
#[derive(Clone)]
pub(in crate::layout::table) struct TableBodyPaintFragment {
    pub(in crate::layout::table) checkpoint: PaintCheckpoint,
    pub(in crate::layout::table) positioned_layer_start: usize,
    pub(in crate::layout::table) plan: TableFragmentPlan,
    /// The single source-grid projection used by cells and structural paint in
    /// this destination fragment. Its [`TableFragmentPlan`] owns the visible
    /// row-piece viewport.
    pub(in crate::layout::table) grid_viewport: Option<TableGridFragmentViewport>,
}

impl TableBodyPaintFragment {
    pub(in crate::layout::table) fn wrapper_timeline_checkpoint(
        &self,
    ) -> Option<TableWrapperTimelineCheckpoint> {
        self.grid_viewport
            .as_ref()
            .map(|viewport| viewport.wrapper_timeline().checkpoint())
    }

    pub(in crate::layout::table) fn rewind_wrapper_timeline(
        &self,
        checkpoint: TableWrapperTimelineCheckpoint,
    ) {
        if let Some(viewport) = &self.grid_viewport {
            viewport.wrapper_timeline().rewind(checkpoint);
        }
    }
}
impl TableBodyPaintFragment {
    pub(in crate::layout::table) fn new(
        fragmentainer_kind: FragmentainerKind,
        checkpoint: PaintCheckpoint,
        page_index: usize,
        positioned_layer_start: usize,
        placement: TableFragmentainerPlacement,
        start_decision: TableFragmentStartDecision,
    ) -> Self {
        Self {
            checkpoint,
            positioned_layer_start,
            plan: TableFragmentPlan::new(fragmentainer_kind, page_index, placement, start_decision),
            grid_viewport: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn initialize_grid_placement(
        &mut self,
        table_style: &ComputedStyle,
        column_plan: &TableColumnPlan,
        source_grid_placement: TableGridPlacement,
        root_background_source_grid_placement: TableGridPlacement,
        wrapper_timeline: TableWrapperFragmentTimeline,
        planned_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_metrics: TableMetrics,
    ) -> TableGridPlacement {
        self.grid_viewport
            .get_or_insert_with(|| {
                let source_row_bounds = planned_row_heights
                    .iter()
                    .enumerate()
                    .map(|(row_index, row_height)| {
                        TableRowBounds::new(
                            table_row_block_start(
                                planned_row_heights,
                                planned_row_occupancy,
                                row_index,
                                table_metrics.clone(),
                            ),
                            *row_height,
                        )
                    })
                    .collect();
                let table_block_extent = table_grid_height(
                    planned_row_heights,
                    planned_row_occupancy,
                    table_metrics.clone(),
                );
                let destination_cell_grid = self.plan.placement.destination_grid_placement(
                    &table_metrics,
                    planned_row_occupancy,
                    TableAxes::for_style(table_style),
                    TableGridLogicalSize::new(
                        column_plan.total_width(),
                        LogicalBlockContentSize::new(content_box_pt(table_block_extent)),
                    ),
                );
                let destination_frame = TableFragmentainerFrame::from_cell_grid(
                    self.plan.placement,
                    destination_cell_grid,
                    TableGridLength::new(table_vertical_edge_spacing(
                        planned_row_occupancy,
                        table_metrics.clone(),
                    )),
                );
                TableGridFragmentViewport::new(
                    source_grid_placement,
                    destination_frame,
                    root_background_source_grid_placement,
                    wrapper_timeline,
                    source_row_bounds,
                )
            })
            .destination_placement()
    }

    pub(in crate::layout::table) fn push_row_decision(
        &mut self,
        decision: TableRowFragmentDecision,
    ) {
        self.push_row_with_fragment_mode(
            decision.row_index,
            decision.row_top,
            decision.row_height,
            decision.row_offset,
            decision.original_row_height,
            decision.collapsed,
            decision.source_fragment,
            decision.fragment_mode,
        );
    }

    /// Commit source/destination grid geometry before row structural paint is
    /// produced. Table-root decoration is painted in that same call, so
    /// delaying this until the row plan is appended would make it replay the
    /// preceding row slice (or a synthetic whole-root fallback).
    pub(in crate::layout::table) fn record_grid_row_slice_for_paint(
        &mut self,
        decision: TableRowFragmentDecision,
    ) {
        if let Some(viewport) = &mut self.grid_viewport {
            // The outer multicolumn sequence, not the temporary page vector,
            // identifies this destination. Non-multicol page fragmentation
            // intentionally retains the page-plan index as its backend key.
            let destination_ordinal = self
                .plan
                .placement
                .outer_fragmentainer_ordinal()
                .unwrap_or(self.plan.page_index);
            viewport.record_source_row_slice(decision, destination_ordinal);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn push_row_with_fragment_mode(
        &mut self,
        row_index: usize,
        row_top: f32,
        row_height: f32,
        row_offset: f32,
        original_row_height: f32,
        collapsed: bool,
        source_fragment: TableRowSourceFragment,
        fragment_mode: TableRowFragmentMode,
    ) {
        let mut metadata = FragmentPageMetadata::new(
            self.plan.page_index,
            source_fragment
                .border_box
                .or_else(|| Some(PageTopRect::new(0.0, row_top, 0.0, row_height).paint_clip())),
            source_fragment.starts_page_fragment,
        );
        metadata.continues_from_previous_page = row_offset > 0.0;
        metadata.continues_to_next_page = row_offset + row_height + 0.01 < original_row_height;
        self.plan.push_body_row(TableRowPiecePlan {
            row_index,
            row_top,
            row_height,
            row_offset,
            original_row_height,
            collapsed,
            fragment_mode,
            metadata,
        });
    }

    pub(in crate::layout::table) fn bottom(&self) -> f32 {
        self.plan.bottom()
    }

    pub(in crate::layout::table) fn mark_repeated_headers(&mut self, rows: &[usize]) {
        self.plan.repeated_header_rows.clear();
        self.plan.repeated_header_rows.extend_from_slice(rows);
    }

    pub(in crate::layout::table) fn mark_repeated_footers(&mut self, rows: &[usize]) {
        self.plan.repeated_footer_rows.clear();
        self.plan.repeated_footer_rows.extend_from_slice(rows);
    }

    pub(in crate::layout::table) fn mark_outgoing_boundary(
        &mut self,
        boundary: TableFragmentBoundaryDecision,
    ) {
        self.plan.outgoing_boundary = Some(boundary);
    }

    pub(in crate::layout::table) fn repeated_rows(&self) -> Vec<usize> {
        self.plan
            .repeated_header_rows
            .iter()
            .chain(self.plan.repeated_footer_rows.iter())
            .cloned()
            .collect()
    }

    pub(in crate::layout::table) fn starts_after_break(&self) -> bool {
        self.plan.break_reason() != TableFragmentBreakReason::TableStart
    }

    pub(in crate::layout::table) fn has_split_or_collapsed_rows(&self) -> bool {
        self.plan
            .body_rows
            .iter()
            .any(|row| row.collapsed || row.fragment_mode != TableRowFragmentMode::Whole)
    }

    pub(in crate::layout::table) fn rows(&self) -> Vec<usize> {
        self.plan
            .body_rows
            .iter()
            .map(|row| row.row_index)
            .collect()
    }

    pub(in crate::layout::table) fn row_tops(&self) -> Vec<f32> {
        self.plan.body_rows.iter().map(|row| row.row_top).collect()
    }

    pub(in crate::layout::table) fn row_heights(&self) -> Vec<f32> {
        self.plan
            .body_rows
            .iter()
            .map(|row| row.row_height)
            .collect()
    }

    pub(in crate::layout::table) fn row_offsets(&self) -> Vec<f32> {
        self.plan
            .body_rows
            .iter()
            .map(|row| row.row_offset)
            .collect()
    }

    pub(in crate::layout::table) fn original_row_heights(&self) -> Vec<f32> {
        self.plan
            .body_rows
            .iter()
            .map(|row| row.original_row_height)
            .collect()
    }
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
    /// Wrapper-selected fragmentainer for the grid's first row. This is kept
    /// distinct from the grid placement so table-local geometry cannot
    /// replace the enclosing multicolumn continuation ordinal.
    pub(in crate::layout::table) initial_fragmentainer_placement: TableFragmentainerPlacement,
    /// Physical top of the destination grid content box after the wrapper's
    /// physical top border and padding. Vertical table rows must start here,
    /// rather than at the wrapper border edge retained by the root-decoration
    /// source frame.
    ///
    /// <https://drafts.csswg.org/css-tables-3/#positioning>
    /// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
    pub(in crate::layout::table) initial_grid_content_top: PageTopBlockPosition,
    /// Retained wrapper source/destination progress shared by every body
    /// fragment. This carries caption progress without making captions part
    /// of table-root background geometry.
    pub(in crate::layout::table) wrapper_timeline: TableWrapperFragmentTimeline,
    pub(in crate::layout::table) logical_inline_extent: LogicalInlineContentSize,
    pub(in crate::layout::table) physical_grid_width: PhysicalContentWidth,
    pub(in crate::layout::table) table_cellpadding: Option<TableCellPadding>,
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

/// The final visible body slice committed for one table wrapper.
///
/// This is deliberately neither the table's complete source grid nor a
/// synthetic root border box. CSS Fragmentation assigns the final row slice to
/// one destination fragmentainer, and wrapper-owned trailing chrome and a
/// bottom caption must continue from that fragment-local edge.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRootFinalBodyFragment {
    pub(in crate::layout::table) placement: TableFragmentainerPlacement,
    pub(in crate::layout::table) body_bottom: PageTopBlockPosition,
}

/// The committed result of table-body row layout.
///
/// The body fragment remains table-local fragmentation state. Its final
/// fragmentainer-local edge is retained explicitly so wrapper siblings never
/// reconstruct it from the complete unfragmented table-root box.
pub(in crate::layout::table) struct TableBodyRowsOutcome {
    pub(in crate::layout::table) table_body_fragment: Option<TableBodyPaintFragment>,
    pub(in crate::layout::table) final_body_fragment: Option<TableRootFinalBodyFragment>,
    pub(in crate::layout::table) forced_break_after_table_rows: PageBreak,
    pub(in crate::layout::table) current_fragment_repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) continuation_inline_offset:
        HorizontalTableContinuationInlineOffset,
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
    fn for_continuation(
        fragmentainer_axes: FlowAxes,
        content_left: f32,
        content_right: f32,
        horizontal_inline_offset: HorizontalTableContinuationInlineOffset,
        cell_grid_block_extent: TableGridLength,
        inline_top: PageTopBlockPosition,
    ) -> Self {
        let x = match fragmentainer_axes.writing_mode() {
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
    pub(in crate::layout::table) table_cellpadding: Option<TableCellPadding>,
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
        fragmentainer_axes: FlowAxes,
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
            fragmentainer_axes,
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
        assert_eq!(
            TableFragmentainerGridOrigin::for_continuation(
                FlowAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
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
                FlowAxes::new(WritingMode::VerticalLr, Direction::Ltr),
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
                FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr),
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
