use super::*;
use std::rc::Rc;

/// Used geometry for painting a CSS table wrapper box.
///
/// CSS 2.2's separated-border model gives the table-root a wrapper border box
/// around the table grid, padding, and table border:
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableWrapperPaintBox {
    pub(in crate::layout::table) table_x: f32,
    pub(in crate::layout::table) top: f32,
    /// The root table flow used to project the logical grid before adding the
    /// wrapper's physical padding and border edges.
    pub(in crate::layout::table) axes: TableAxes,
    pub(in crate::layout::table) grid_size: TableGridLogicalSize,
    pub(in crate::layout::table) table_width: UsedTableWidth,
    pub(in crate::layout::table) table_metrics: TableMetrics,
}

impl TableWrapperPaintBox {
    pub(in crate::layout::table) fn grid_placement(self) -> TableGridPlacement {
        let grid_origin = PageTopPoint::new(
            self.table_x,
            self.top - self.table_width.border_widths.top - self.table_width.padding.top,
        );
        TableGridPlacement::with_axes(grid_origin, self.axes, self.grid_size)
    }

    pub(in crate::layout::table) fn grid_content_box(self) -> PageTopRect {
        self.grid_placement().full_page_top_rect()
    }

    pub(in crate::layout::table) fn physical_grid_width(self) -> PhysicalContentWidth {
        self.grid_size.physical_width(self.axes)
    }

    pub(in crate::layout::table) fn border_box(self) -> PageTopRect {
        let table_width = self.table_width;
        let padding_box = self.padding_box();
        PageTopRect::new(
            padding_box.x() - table_width.border_widths.left,
            padding_box.top_y() + table_width.border_widths.top,
            padding_box.width() + table_width.border_widths.left + table_width.border_widths.right,
            padding_box.height() + table_width.border_widths.top + table_width.border_widths.bottom,
        )
    }

    pub(in crate::layout::table) fn padding_box(self) -> PageTopRect {
        let table_width = self.table_width;
        let content_box = self.grid_content_box();
        PageTopRect::new(
            content_box.x() - table_width.padding.left,
            content_box.top_y() + table_width.padding.top,
            content_box.width() + table_width.padding.left + table_width.padding.right,
            content_box.height() + table_width.padding.top + table_width.padding.bottom,
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
    pub(in crate::layout::table) fragment_top: f32,
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
        fragment_top: f32,
        start_decision: TableFragmentStartDecision,
    ) -> Self {
        Self {
            fragmentainer_kind,
            page_index,
            fragment_top,
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
            .unwrap_or(self.fragment_top)
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
    KeptByAvoidOverflow,
}

impl TableRowFragmentMode {
    pub(in crate::layout::table) fn clips_to_row_piece(self) -> bool {
        self == Self::Sliced
    }

    pub(in crate::layout::table) fn replays_flow_children_from_plan(self) -> bool {
        matches!(self, Self::Sliced | Self::KeptByAvoidOverflow)
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
        Some(OverflowClip::from_paint_rect(paint_space_rect(
            min_x,
            min_y,
            max_x - min_x,
            max_y - min_y,
        )))
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

#[derive(Clone)]
pub(in crate::layout::table) struct TableBreakCandidateMeta {
    pub(in crate::layout::table) row_index: usize,
    pub(in crate::layout::table) table_body_fragment: Option<TableBodyPaintFragment>,
    pub(in crate::layout::table) repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) height: f32,
}

pub(in crate::layout::table) struct PendingTableBreakCandidate {
    pub(in crate::layout::table) meta: TableBreakCandidateMeta,
}

#[derive(Clone)]
pub(in crate::layout::table) struct TableBreakCandidate {
    snapshot: Rc<LayoutSnapshot>,
    pub(in crate::layout::table) meta: TableBreakCandidateMeta,
}

/// Rolling break-candidate state for row-level avoid constraints.
///
/// CSS Fragmentation treats `break-before: avoid` and `break-after: avoid` as
/// constraints on class A break opportunities. Table pagination captures
/// rollback candidates at row starts and updates this state after each source
/// row is consumed so a later overflow can restore the chosen row boundary:
/// <https://www.w3.org/TR/css-break-3/#break-between>.
pub(in crate::layout::table) struct TableAvoidBreakCandidateState {
    fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) avoid_break_candidate: Option<TableBreakCandidate>,
    pub(in crate::layout::table) previous_row_candidate: Option<TableBreakCandidate>,
    pub(in crate::layout::table) previous_break_after: PageBreak,
}

/// Table-local spelling for the shared adjacent-box break context.
pub(in crate::layout::table) type TableRowBreakContext = FragmentBreakContext;

/// Table-local spelling for shared cross-sibling forced break carry state.
pub(in crate::layout::table) type TableForcedBreakCarryState = ForcedBreakCarryState;

/// Committed decision to roll an avoid-constrained run back to an earlier row.
///
/// CSS Fragmentation treats `break-before: avoid` and `break-after: avoid` as
/// constraints between adjacent boxes. Table pagination records row-start
/// rollback candidates before painting, then commits a rollback only when the
/// measured avoid run fits in the next fragmentainer:
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Clone)]
pub(in crate::layout::table) struct TableAvoidRunBreakDecision {
    pub(in crate::layout::table) candidate: TableBreakCandidate,
    pub(in crate::layout::table) avoid_run_height: f32,
    pub(in crate::layout::table) incoming_repeat_policy: TableFragmentRepeatPolicy,
}

pub(in crate::layout::table) struct TableAvoidRunBreakInput {
    pub(in crate::layout::table) candidate: TableBreakCandidate,
    pub(in crate::layout::table) row_height: f32,
    pub(in crate::layout::table) current_fragmentainer: TableFragmentainer,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
    pub(in crate::layout::table) can_advance: bool,
}

/// Committed overflow break before a table body row fragment.
///
/// CSS Fragmentation places content into a finite fragmentainer and chooses a
/// break when the next row would overflow the available block-size. Table
/// pagination records the measured row height, current fragmentainer state, and
/// incoming repeated table chrome policy before advancing to the next fragment:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRowOverflowBreakDecision {
    pub(in crate::layout::table) row_height: f32,
    pub(in crate::layout::table) incoming_repeat_policy: TableFragmentRepeatPolicy,
}

pub(in crate::layout::table) struct TableRowOverflowBreakInput {
    pub(in crate::layout::table) row_height: f32,
    pub(in crate::layout::table) row_required_height: f32,
    pub(in crate::layout::table) current_fragmentainer: TableFragmentainer,
    pub(in crate::layout::table) row_kept_by_avoid_group: bool,
    /// An oversized row with an authored row-level avoid still prefers its
    /// first child fragment to begin at the next class-A boundary.
    pub(in crate::layout::table) prefer_fresh_fragment: bool,
    pub(in crate::layout::table) can_break: bool,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
}

/// Fragment-local decision for the next slice of an oversized table row.
///
/// CSS Fragmentation may split an oversized row across fragmentainers. The
/// table body chooses the current piece height from the remaining source row
/// height and the actual fragmentainer body capacity, including repeated
/// chrome and cloned table-wrapper decoration, before table-cell descendants
/// are replayed for that row slice. A zero-height pre-break is legal only
/// when the destination can consume the deferred cell child.
///
/// <https://drafts.csswg.org/css-tables/#table-fragmentation>
/// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
/// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableOversizedRowSliceDecision {
    pub(in crate::layout::table) kind: TableOversizedRowSliceDecisionKind,
    pub(in crate::layout::table) remaining_height: f32,
    pub(in crate::layout::table) available_body_size: f32,
    pub(in crate::layout::table) piece_height: f32,
    pub(in crate::layout::table) incoming_repeat_policy: TableFragmentRepeatPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableOversizedRowSliceDecisionKind {
    AdvanceBeforeSlice,
    PaintSlice,
}

pub(in crate::layout::table) struct TableOversizedRowSliceInput {
    pub(in crate::layout::table) remaining_height: f32,
    pub(in crate::layout::table) row_required_height: f32,
    pub(in crate::layout::table) current_fragmentainer: TableFragmentainer,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
    pub(in crate::layout::table) can_advance: bool,
}

/// Committed action at the boundary between two table body fragments.
///
/// CSS Fragmentation chooses page-fragment boundaries before the next
/// fragmentainer is laid out. For tables, that same boundary also decides
/// whether optional repeated footer chrome is part of the outgoing fragment:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentBoundaryDecision {
    pub(in crate::layout::table) repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) footer_action: TableFragmentFooterAction,
}

impl TableFragmentBoundaryDecision {
    pub(in crate::layout::table) fn new(
        repeat_policy: TableFragmentRepeatPolicy,
        footer_action: TableFragmentFooterAction,
    ) -> Self {
        Self {
            repeat_policy,
            footer_action,
        }
    }
}

/// Repeated-footer handling committed at a table body fragment boundary.
///
/// Intermediate page boundaries replay repeated footer chrome after the body
/// fragment is finalized. The final table fragment only records repeated
/// footer rows in the fragment plan so structural backgrounds and border
/// painting can account for footer rows already present in source order:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableFragmentFooterAction {
    Omit,
    RecordOnly,
    PaintRepeated,
}

impl TableFragmentFooterAction {
    pub(in crate::layout::table) fn paint_repeated_if(condition: bool) -> Self {
        if condition {
            Self::PaintRepeated
        } else {
            Self::Omit
        }
    }

    pub(in crate::layout::table) fn record_repeated_rows(self) -> bool {
        matches!(self, Self::RecordOnly | Self::PaintRepeated)
    }

    pub(in crate::layout::table) fn paint_repeated_chrome(self) -> bool {
        self == Self::PaintRepeated
    }
}

/// Committed action at the start of a table body fragment.
///
/// CSS Fragmentation creates a new fragmentainer slice with a known break
/// reason before the first body row is painted. For tables, the same start
/// decision owns whether optional repeated header chrome participates in that
/// new fragment:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentStartDecision {
    pub(in crate::layout::table) break_reason: TableFragmentBreakReason,
    pub(in crate::layout::table) repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) paint_repeated_header: bool,
}

impl TableFragmentStartDecision {
    pub(in crate::layout::table) fn new(
        break_reason: TableFragmentBreakReason,
        repeat_policy: TableFragmentRepeatPolicy,
        paint_repeated_header: bool,
    ) -> Self {
        Self {
            break_reason,
            repeat_policy,
            paint_repeated_header,
        }
    }

    pub(in crate::layout::table) fn repeated_header_rows<'a>(
        &self,
        rows: &'a [usize],
    ) -> &'a [usize] {
        if self.paint_repeated_header {
            self.repeat_policy.header_rows(rows)
        } else {
            &[]
        }
    }
}

/// Committed transition between two table body fragments.
///
/// CSS Fragmentation treats a break as an outgoing fragment boundary plus an
/// incoming fragmentainer start. Keeping both halves together lets table
/// pagination reserve footer chrome, carry the active fragmentainer kind, and
/// replay header chrome as one committed table-local transition:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentTransitionDecision {
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) boundary: TableFragmentBoundaryDecision,
    pub(in crate::layout::table) start: TableFragmentStartDecision,
}

/// Inputs used to commit one table body fragment transition.
///
/// CSS Fragmentation makes a fragmentainer transition as an outgoing fragment
/// boundary followed by an incoming fragmentainer start. Table pagination has
/// to bind that model to optional repeated table header/footer chrome, so
/// callers pass both repeat policies and the chrome actions as one value
/// instead of assembling boundary and start decisions independently:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentTransitionInput {
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) outgoing_repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) footer_action: TableFragmentFooterAction,
    pub(in crate::layout::table) break_reason: TableFragmentBreakReason,
    pub(in crate::layout::table) incoming_repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) paint_repeated_header: bool,
}

impl TableFragmentTransitionDecision {
    pub(in crate::layout::table) fn new(
        fragmentainer_kind: FragmentainerKind,
        boundary: TableFragmentBoundaryDecision,
        start: TableFragmentStartDecision,
    ) -> Self {
        Self {
            fragmentainer_kind,
            boundary,
            start,
        }
    }

    pub(in crate::layout::table) fn from_input(input: TableFragmentTransitionInput) -> Self {
        Self::new(
            input.fragmentainer_kind,
            TableFragmentBoundaryDecision::new(input.outgoing_repeat_policy, input.footer_action),
            TableFragmentStartDecision::new(
                input.break_reason,
                input.incoming_repeat_policy,
                input.paint_repeated_header,
            ),
        )
    }
}

/// Committed forced break before a table body row fragment.
///
/// Forced breaks are class A break opportunities in CSS Fragmentation. The
/// table body must commit the outgoing fragment boundary before applying the
/// forced page change, then carry a committed start decision for the incoming
/// fragment's repeated table chrome:
/// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableForcedBreakDecision {
    pub(in crate::layout::table) boundary: TableFragmentBoundaryDecision,
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) page_break: PageBreak,
    pub(in crate::layout::table) start: TableFragmentStartDecision,
}

/// Inputs for choosing a table body forced break decision.
///
/// CSS Fragmentation decides the forced break first, while CSS 2.2 table
/// header/footer repetition determines the usable body capacity on the
/// incoming fragment. Keeping these inputs together prevents forced break
/// branches from recomputing table chrome policy independently:
/// <https://www.w3.org/TR/css-break-3/#break-between> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableForcedBreakInput {
    pub(in crate::layout::table) outgoing_repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout::table) page_break: PageBreak,
    pub(in crate::layout::table) row_required_height: f32,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
    pub(in crate::layout::table) paint_repeated_footer: bool,
}

impl TableForcedBreakDecision {
    pub(in crate::layout::table) fn choose(input: TableForcedBreakInput) -> Self {
        let incoming_repeat_policy = input
            .chrome_context
            .repeat_policy(layout_pt(input.row_required_height));
        Self {
            boundary: TableFragmentBoundaryDecision::new(
                input.outgoing_repeat_policy,
                TableFragmentFooterAction::paint_repeated_if(input.paint_repeated_footer),
            ),
            fragmentainer_kind: input.fragmentainer_kind,
            page_break: input.page_break,
            start: TableFragmentStartDecision::new(
                TableFragmentBreakReason::Forced,
                incoming_repeat_policy,
                input.chrome_context.allow_header,
            ),
        }
    }
}

/// Committed named-page group transition before a table body row fragment.
///
/// CSS Paged Media forms named page groups at class A break opportunities.
/// Table body pagination treats the named-page switch as an outgoing table
/// fragment boundary plus an incoming fragment start so repeated table chrome
/// stays tied to the same committed named-page transition:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
/// <https://www.w3.org/TR/css-break-3/#break-between>.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableNamedPageBreakDecision {
    pub(in crate::layout::table) boundary: TableFragmentBoundaryDecision,
    pub(in crate::layout::table) page_name: Option<String>,
    pub(in crate::layout::table) start: TableFragmentStartDecision,
}

/// Inputs for choosing a table body named-page transition.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableNamedPageBreakInput {
    pub(in crate::layout::table) previous_page_end: Option<String>,
    pub(in crate::layout::table) row_page_start: Option<String>,
    pub(in crate::layout::table) outgoing_repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) row_required_height: f32,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
    pub(in crate::layout::table) paint_repeated_footer: bool,
}

impl TableNamedPageBreakDecision {
    pub(in crate::layout::table) fn choose(input: TableNamedPageBreakInput) -> Option<Self> {
        if input.previous_page_end == input.row_page_start {
            return None;
        }

        let incoming_repeat_policy = input
            .chrome_context
            .repeat_policy(layout_pt(input.row_required_height));
        Some(Self {
            boundary: TableFragmentBoundaryDecision::new(
                input.outgoing_repeat_policy,
                TableFragmentFooterAction::paint_repeated_if(input.paint_repeated_footer),
            ),
            page_name: input.row_page_start,
            start: TableFragmentStartDecision::new(
                TableFragmentBreakReason::Forced,
                incoming_repeat_policy,
                input.chrome_context.allow_header,
            ),
        })
    }
}

impl PendingTableBreakCandidate {
    /// Capture before the first row layout mutation that a later table
    /// avoid-break retry must undo.
    pub(in crate::layout::table) fn arm(self, builder: &LayoutBuilder<'_>) -> TableBreakCandidate {
        TableBreakCandidate {
            snapshot: Rc::new(builder.snapshot()),
            meta: self.meta,
        }
    }
}

impl TableBreakCandidate {
    pub(in crate::layout::table) fn height(&self) -> f32 {
        self.meta.height
    }

    pub(in crate::layout::table) fn with_height(mut self, height: f32) -> Self {
        self.meta.height = height;
        self
    }

    pub(in crate::layout::table) fn restore(
        self,
        builder: &mut LayoutBuilder<'_>,
    ) -> TableBreakCandidateMeta {
        let snapshot = Rc::try_unwrap(self.snapshot).unwrap_or_else(|snapshot| (*snapshot).clone());
        builder.restore(snapshot);
        self.meta
    }
}

impl TableAvoidBreakCandidateState {
    pub(in crate::layout::table) fn new(fragmentainer_kind: FragmentainerKind) -> Self {
        Self {
            fragmentainer_kind,
            avoid_break_candidate: None,
            previous_row_candidate: None,
            previous_break_after: PageBreak::Auto,
        }
    }

    pub(in crate::layout::table) fn row_start_may_be_rollback_target(
        &self,
        row_collapsed: bool,
        row_is_running: bool,
        row_breaks: TableRowBreakContext,
    ) -> bool {
        // A current row's `break-before: avoid` protects the boundary before
        // that row, so overflow should roll back to the previous row candidate
        // rather than arming the current row start as a new target.
        let row_start_breaks = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            if row_collapsed || row_is_running {
                PageBreak::Auto
            } else {
                row_breaks.after
            },
            row_breaks.next_before,
        );
        FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
            participates_in_flow: true,
            fragmentainer_kind: self.fragmentainer_kind,
            break_context: row_start_breaks,
            break_opportunity: FragmentBreakOpportunity::before_box_boundary(
                self.fragmentainer_kind,
                0.0,
                row_start_breaks,
                self.previous_break_after,
                false,
            ),
            next_break_before: Some(row_breaks.next_before),
            has_avoid_run_candidate: self.avoid_break_candidate.is_some(),
        })
        .should_arm_start_candidate
    }

    pub(in crate::layout::table) fn boundary_candidate(
        &self,
        row_breaks: TableRowBreakContext,
    ) -> Option<TableBreakCandidate> {
        match row_breaks
            .avoid_boundary_side_before_box_in(self.fragmentainer_kind, self.previous_break_after)
        {
            FragmentAvoidBoundarySide::Previous => self.avoid_break_candidate.clone(),
            FragmentAvoidBoundarySide::Current => self.previous_row_candidate.clone(),
            FragmentAvoidBoundarySide::None => None,
        }
    }

    pub(in crate::layout::table) fn reset(&mut self) {
        self.avoid_break_candidate = None;
        self.previous_row_candidate = None;
        self.previous_break_after = PageBreak::Auto;
    }

    pub(in crate::layout::table) fn finish_non_content_row(
        &mut self,
        row_breaks: TableRowBreakContext,
        row_start_candidate: Option<TableBreakCandidate>,
    ) {
        self.previous_row_candidate = row_breaks
            .next_avoid_before_in(self.fragmentainer_kind)
            .is_some()
            .then(|| Self::expect_row_start_candidate(row_start_candidate).with_height(0.0));
        self.avoid_break_candidate = None;
        self.previous_break_after = PageBreak::Auto;
    }

    pub(in crate::layout::table) fn finish_content_row(
        &mut self,
        row_breaks: TableRowBreakContext,
        row_start_candidate: Option<TableBreakCandidate>,
        row_height: f32,
    ) {
        let row_candidate = if self.previous_break_after_avoids() {
            let this = self
                .avoid_break_candidate
                .clone()
                .unwrap_or_else(|| Self::expect_row_start_candidate(row_start_candidate.clone()));
            let height = self
                .avoid_break_candidate
                .as_ref()
                .map(TableBreakCandidate::height)
                .unwrap_or(0.0)
                + row_height;
            Some(this.with_height(height))
        } else if row_breaks.seeds_later_avoid_boundary_in_context_for(self.fragmentainer_kind) {
            Some(Self::expect_row_start_candidate(row_start_candidate).with_height(row_height))
        } else {
            None
        };
        self.previous_row_candidate = row_breaks
            .next_avoid_before_in(self.fragmentainer_kind)
            .is_some()
            .then(|| {
                row_candidate
                    .clone()
                    .expect("table break candidate must exist for next row break-before: avoid")
            });
        let avoid_after = row_breaks.avoid_after_in(self.fragmentainer_kind);
        self.avoid_break_candidate = if avoid_after.is_some() {
            Some(row_candidate.expect("table break candidate must exist for break-after: avoid"))
        } else {
            None
        };
        self.previous_break_after = avoid_after.unwrap_or(PageBreak::Auto);
    }

    fn expect_row_start_candidate(candidate: Option<TableBreakCandidate>) -> TableBreakCandidate {
        candidate.expect(
            "row start candidate must be armed when this row can become a table break candidate",
        )
    }

    fn previous_break_after_avoids(&self) -> bool {
        self.fragmentainer_kind
            .is_avoid_break(self.previous_break_after)
    }
}

impl Default for TableAvoidBreakCandidateState {
    fn default() -> Self {
        Self::new(FragmentainerKind::Page)
    }
}

impl TableAvoidRunBreakDecision {
    pub(in crate::layout::table) fn choose(input: TableAvoidRunBreakInput) -> Option<Self> {
        let avoid_run_height = input.candidate.height() + input.row_height;
        let incoming_repeat_policy = input
            .chrome_context
            .repeat_policy(layout_pt(avoid_run_height));
        let next_fragmentainer = input
            .chrome_context
            .fresh_fragmentainer(incoming_repeat_policy);
        FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: input.can_advance,
            current_fragmentainer: input.current_fragmentainer.as_fragmentainer(),
            required_block_size: layout_pt(input.row_height),
            empty_fragmentainer: next_fragmentainer.body_capacity_fragmentainer(),
            empty_fit_block_size: layout_pt(avoid_run_height),
        })
        .should_break
        .then_some(Self {
            candidate: input.candidate,
            avoid_run_height,
            incoming_repeat_policy,
        })
    }
}

impl TableRowOverflowBreakDecision {
    pub(in crate::layout::table) fn choose(input: TableRowOverflowBreakInput) -> Option<Self> {
        // A table body can be fragmented by a column whose usable body area is
        // smaller than the backing page canvas. Compare with the table-local
        // body capacity, not the physical page height, or a row larger than a
        // short column is repeatedly moved to another equally short column
        // without ever becoming eligible for row slicing.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        let row_requires_split =
            input.row_height > input.current_fragmentainer.body_capacity.points() + 0.01;
        // `row_required_height` includes any non-row table edge that must be
        // emitted immediately after this row. The row itself remains the
        // paint/slicing unit, but the fragmentation fit check must reserve the
        // complete trailing contribution.
        let row_overflows_page = if row_requires_split {
            input.prefer_fresh_fragment
                || !input.row_kept_by_avoid_group
                    && input.current_fragmentainer.available_block_size().points() <= 0.01
        } else {
            input.row_required_height > input.current_fragmentainer.available_block_size().points()
        };
        let row_overflows_reserved_footer = if row_requires_split {
            !input.row_kept_by_avoid_group
                && input.current_fragmentainer.available_body_size().points() <= 0.01
        } else {
            input.row_required_height + input.current_fragmentainer.reserved_footer_height.points()
                > input.current_fragmentainer.available_block_size().points()
        };
        let should_advance = FragmentAdvanceDecision::choose(FragmentAdvanceInput {
            break_is_applicable: true,
            overflows: row_overflows_page || row_overflows_reserved_footer,
            can_advance: input.can_break,
        })
        .should_advance;
        if !should_advance {
            return None;
        }

        Some(Self {
            row_height: input.row_height,
            incoming_repeat_policy: input
                .chrome_context
                .repeat_policy(layout_pt(input.row_required_height)),
        })
    }
}

impl TableOversizedRowSliceDecision {
    pub(in crate::layout::table) fn choose(input: TableOversizedRowSliceInput) -> Self {
        let raw_available_body_size = input
            .current_fragmentainer
            .available_body_size()
            .points()
            .min(input.current_fragmentainer.body_capacity.points());
        let available_body_size = raw_available_body_size;
        let incoming_repeat_policy = input
            .chrome_context
            .repeat_policy(layout_pt(input.row_required_height));
        if available_body_size > 0.01 && input.remaining_height > available_body_size + 0.01 {
            return Self {
                kind: TableOversizedRowSliceDecisionKind::PaintSlice,
                remaining_height: input.remaining_height,
                available_body_size,
                piece_height: available_body_size,
                incoming_repeat_policy,
            };
        }
        let source_slice = FragmentSourceSliceDecision::choose(FragmentSourceSliceInput {
            break_is_applicable: input.can_advance,
            source_is_oversized: true,
            source_block_end: input.remaining_height,
            slice_start: 0.0,
            available_block_end: available_body_size,
        });
        if !source_slice.paints_slice() {
            return Self {
                kind: TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice,
                remaining_height: input.remaining_height,
                available_body_size,
                piece_height: 0.0,
                incoming_repeat_policy,
            };
        }

        Self {
            kind: TableOversizedRowSliceDecisionKind::PaintSlice,
            remaining_height: input.remaining_height,
            available_body_size,
            piece_height: source_slice.slice_end,
            incoming_repeat_policy,
        }
    }

    pub(in crate::layout::table) fn paints_slice(self) -> bool {
        self.kind == TableOversizedRowSliceDecisionKind::PaintSlice
    }

    pub(in crate::layout::table) fn continues_after_slice(self) -> bool {
        self.remaining_height - self.piece_height > 0.01
    }

    /// Restrict a height-based candidate to a legal shared table-cell child
    /// boundary. A zero-sized result may advance only after the caller has
    /// verified that the exact destination body capacity can paint the
    /// deferred child; otherwise it must consume a non-zero source slice.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    /// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
    pub(in crate::layout::table) fn at_child_boundary(mut self, piece_height: f32) -> Self {
        debug_assert!(piece_height >= 0.0);
        if !self.paints_slice() {
            return self;
        }
        self.piece_height = piece_height.min(self.piece_height).max(0.0);
        if self.piece_height <= 0.01 {
            self.kind = TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice;
        }
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) struct TableFragmentRepeatPolicy {
    pub(in crate::layout::table) repeat_header: bool,
    pub(in crate::layout::table) repeat_footer: bool,
}

pub(in crate::layout::table) const TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE: f32 = 2.0;

/// Table row-group range with a `break-inside: avoid-*` constraint.
///
/// CSS Fragmentation treats row groups as fragmentation containers. Keeping
/// the constrained source range explicit lets table pagination choose a group
/// fragment before painting rows:
/// <https://www.w3.org/TR/css-break-3/#break-within>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) struct TableAvoidRowGroup {
    pub(in crate::layout::table) start: usize,
    pub(in crate::layout::table) end: usize,
}

/// Complete block-axis space an avoided row group consumes in one table
/// fragment.
///
/// A row group's grid tracks are not the same as its fragmentainer footprint:
/// in the separated-border model the destination fragment also owns the
/// spacing on both sides of the participating range. Keeping that distinction
/// explicit prevents a keep-together decision from accepting a group which
/// the eventual row placement cannot fit.
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
/// <https://www.w3.org/TR/css-break-3/#break-within>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableRowGroupFragmentRequirement {
    row_grid: LayoutLength,
    leading_edge_spacing: LayoutLength,
    trailing_edge_spacing: LayoutLength,
}

impl TableRowGroupFragmentRequirement {
    pub(in crate::layout::table) fn from_row_group(
        group: TableAvoidRowGroup,
        row_heights: &[f32],
        row_occupancy: &[bool],
        table_metrics: TableMetrics,
    ) -> Self {
        let row_grid = layout_pt(table_row_span_height(
            row_heights,
            row_occupancy,
            group.start,
            group.row_span(),
            table_metrics.clone(),
        ));
        let group_end = group.end.min(row_occupancy.len());
        let group_has_occupied_row = row_occupancy
            .get(group.start..group_end)
            .is_some_and(|rows| rows.iter().any(|occupied| *occupied));
        let edge_spacing = if group_has_occupied_row {
            layout_pt(table_vertical_edge_spacing(row_occupancy, table_metrics))
        } else {
            layout_pt(0.0)
        };
        Self {
            row_grid,
            leading_edge_spacing: edge_spacing,
            trailing_edge_spacing: edge_spacing,
        }
    }

    pub(in crate::layout::table) fn block_size(self) -> LayoutLength {
        layout_pt(
            self.row_grid.points()
                + self.leading_edge_spacing.points()
                + self.trailing_edge_spacing.points(),
        )
    }
}

impl TableAvoidRowGroup {
    pub(in crate::layout::table) fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub(in crate::layout::table) fn row_span(self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

impl TableFragmentRepeatPolicy {
    pub(in crate::layout::table) fn header_rows<'a>(&self, rows: &'a [usize]) -> &'a [usize] {
        if self.repeat_header { rows } else { &[] }
    }

    pub(in crate::layout::table) fn footer_rows<'a>(&self, rows: &'a [usize]) -> &'a [usize] {
        if self.repeat_footer { rows } else { &[] }
    }

    pub(in crate::layout::table) fn reserved_footer_height(
        &self,
        footer_height: LayoutLength,
    ) -> LayoutLength {
        if self.repeat_footer {
            footer_height
        } else {
            layout_pt(0.0)
        }
    }

    pub(in crate::layout::table) fn body_capacity(
        &self,
        fragmentainer_block_size: LayoutLength,
        header_height: LayoutLength,
        footer_height: LayoutLength,
    ) -> LayoutLength {
        let repeated_height = if self.repeat_header {
            header_height
        } else {
            layout_pt(0.0)
        } + if self.repeat_footer {
            footer_height
        } else {
            layout_pt(0.0)
        };
        layout_pt((fragmentainer_block_size.points() - repeated_height.points()).max(0.0))
    }
}

/// Decoration owned by one table-wrapper fragment in the block direction.
///
/// The values retain their non-content-box meaning until they cross into the
/// generic fragmentainer adapter. Block-level margins are deliberately absent:
/// CSS Fragmentation truncates cloned margins for block-level boxes.
///
/// <https://drafts.csswg.org/css-tables/#table-fragmentation>
/// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
/// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableWrapperFragmentChrome {
    continuation_block_start: NonContentLength,
    continuation_block_end: NonContentLength,
}

impl TableWrapperFragmentChrome {
    #[cfg(test)]
    pub(in crate::layout::table) const fn none() -> Self {
        Self {
            continuation_block_start: non_content_pt(0.0),
            continuation_block_end: non_content_pt(0.0),
        }
    }

    /// Build the decoration consumed by every continuation fragment.
    ///
    /// `clone` independently wraps every box fragment with border and padding;
    /// `slice` does not insert them at an internal break. Separated-border edge
    /// spacing belongs to the source table grid, rather than to the cloned
    /// wrapper decoration, and is therefore handled by row placement.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    /// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
    pub(in crate::layout::table) fn for_table(
        style: &ComputedStyle,
        table_width: UsedTableWidth,
    ) -> Self {
        let cloned = style.box_decoration_break == css::BoxDecorationBreak::Clone;
        let start = if cloned {
            table_width.border_widths.top + table_width.padding.top
        } else {
            0.0
        };
        let end = if cloned {
            table_width.border_widths.bottom + table_width.padding.bottom
        } else {
            0.0
        };
        Self {
            continuation_block_start: non_content_pt(start),
            continuation_block_end: non_content_pt(end),
        }
    }

    pub(in crate::layout::table) fn continuation_block_start(self) -> NonContentLength {
        self.continuation_block_start
    }

    pub(in crate::layout::table) fn continuation_block_end(self) -> NonContentLength {
        self.continuation_block_end
    }

    /// Return the body area left after this wrapper fragment's decorations.
    ///
    /// CSS Fragmentation permits truncating cloned decoration before allowing a
    /// zero-progress break. This adapter first reserves both sides, then trims
    /// the cloned decoration to leave one paintable layout quantum whenever
    /// the fragmentainer itself has positive capacity.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    /// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
    pub(in crate::layout::table) fn fresh_body_capacity(
        self,
        capacity_before_wrapper_chrome: LayoutLength,
    ) -> LayoutLength {
        let chrome = self.truncated_for_capacity(capacity_before_wrapper_chrome);
        layout_pt(
            (capacity_before_wrapper_chrome.points()
                - chrome.continuation_block_start.points()
                - chrome.continuation_block_end.points())
            .max(0.0),
        )
    }

    /// Truncate cloned decoration only when it would otherwise leave no
    /// content slice in a positive-capacity fragmentainer.
    ///
    /// The retained lengths remain typed non-content-box quantities; scalar
    /// arithmetic is confined to this fragmentation-boundary adapter.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    /// <https://www.w3.org/TR/css-break-3/#box-decoration-break>
    fn truncated_for_capacity(self, capacity: LayoutLength) -> Self {
        const MINIMUM_PAINTABLE_SLICE: f32 = 0.01;

        let decoration =
            self.continuation_block_start.points() + self.continuation_block_end.points();
        let available = capacity.points().max(0.0);
        if available <= MINIMUM_PAINTABLE_SLICE || decoration < available - MINIMUM_PAINTABLE_SLICE
        {
            return self;
        }
        let decoration_budget = (available - MINIMUM_PAINTABLE_SLICE).max(0.0);
        let continuation_block_start = self
            .continuation_block_start
            .points()
            .min(decoration_budget);
        let continuation_block_end = self
            .continuation_block_end
            .points()
            .min((decoration_budget - continuation_block_start).max(0.0));
        Self {
            continuation_block_start: non_content_pt(continuation_block_start),
            continuation_block_end: non_content_pt(continuation_block_end),
        }
    }
}

/// Table-local repeated chrome capacity context for a target fragmentainer.
///
/// CSS Fragmentation defines a finite fragmentainer block-size, while CSS 2.2
/// table header/footer groups may reserve repeated chrome around the table
/// body in paged output. Keeping those values together lets table break
/// decisions share the same capacity calculation without treating every
/// fragmentainer as a page cursor transition:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentChromeContext {
    pub(in crate::layout::table) fragmentainer_block_size: LayoutLength,
    pub(in crate::layout::table) header_height: LayoutLength,
    pub(in crate::layout::table) footer_height: LayoutLength,
    pub(in crate::layout::table) wrapper_chrome: TableWrapperFragmentChrome,
    pub(in crate::layout::table) allow_header: bool,
    pub(in crate::layout::table) allow_footer: bool,
}

impl TableFragmentChromeContext {
    pub(in crate::layout::table) fn repeat_policy(
        self,
        required_body_height: LayoutLength,
    ) -> TableFragmentRepeatPolicy {
        let body_fragmentainer_size = self
            .wrapper_chrome
            .fresh_body_capacity(self.fragmentainer_block_size);
        table_fragment_repeat_policy(
            required_body_height,
            body_fragmentainer_size,
            self.header_height,
            self.footer_height,
            self.allow_header,
            self.allow_footer,
        )
    }

    pub(in crate::layout::table) fn fresh_fragmentainer(
        self,
        repeat_policy: TableFragmentRepeatPolicy,
    ) -> TableFragmentainer {
        TableFragmentainer::fresh_with_wrapper_chrome(
            self.fragmentainer_block_size,
            repeat_policy,
            self.header_height,
            self.footer_height,
            self.wrapper_chrome,
        )
    }

    pub(in crate::layout::table) fn current_fragmentainer(
        self,
        content_block_start: PageTopBlockPosition,
        fragmentainer_block_end: PageTopBlockPosition,
        repeat_policy: TableFragmentRepeatPolicy,
        reserve_footer: bool,
    ) -> TableFragmentainer {
        TableFragmentainer::current_from_page_cursor_bounds(
            self.fragmentainer_block_size,
            content_block_start,
            fragmentainer_block_end,
            repeat_policy,
            self.header_height,
            self.footer_height,
            reserve_footer,
        )
        .with_wrapper_end_reservation(self.wrapper_chrome.continuation_block_end())
    }

    pub(in crate::layout::table) fn without_repeats(self) -> Self {
        Self {
            allow_header: false,
            allow_footer: false,
            ..self
        }
    }
}

/// Table-local view of a page fragmentainer while paginating body rows.
///
/// CSS Fragmentation lays boxes into fragmentainers with a finite block-size,
/// while repeated table header/footer groups reserve page-fragment chrome
/// around the table body. This value keeps the current remaining block-size,
/// optional repeated-footer reservation, and fresh-page body capacity together
/// so table break decisions consume one fragmentainer model instead of
/// repeating cursor arithmetic inline:
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentainer {
    base: Fragmentainer,
    pub(in crate::layout::table) reserved_footer_height: LayoutLength,
    reserved_wrapper_end: LayoutLength,
    pub(in crate::layout::table) body_capacity: LayoutLength,
}

impl TableFragmentainer {
    fn with_base(
        base: Fragmentainer,
        fragmentainer_block_size: LayoutLength,
        repeat_policy: TableFragmentRepeatPolicy,
        header_height: LayoutLength,
        footer_height: LayoutLength,
        reserve_footer: bool,
    ) -> Self {
        let reserved_footer_height = if reserve_footer {
            repeat_policy.reserved_footer_height(footer_height)
        } else {
            layout_pt(0.0)
        };
        Self {
            base,
            reserved_footer_height,
            reserved_wrapper_end: layout_pt(0.0),
            body_capacity: repeat_policy.body_capacity(
                fragmentainer_block_size,
                header_height,
                footer_height,
            ),
        }
    }

    pub(in crate::layout::table) fn current_from_page_cursor_bounds(
        fragmentainer_block_size: LayoutLength,
        content_block_start: PageTopBlockPosition,
        fragmentainer_block_end: PageTopBlockPosition,
        repeat_policy: TableFragmentRepeatPolicy,
        header_height: LayoutLength,
        footer_height: LayoutLength,
        reserve_footer: bool,
    ) -> Self {
        Self::with_base(
            Fragmentainer::from_page_cursor_bounds(
                fragmentainer_block_size,
                content_block_start,
                fragmentainer_block_end,
            ),
            fragmentainer_block_size,
            repeat_policy,
            header_height,
            footer_height,
            reserve_footer,
        )
    }

    fn fresh_with_wrapper_chrome(
        fragmentainer_block_size: LayoutLength,
        repeat_policy: TableFragmentRepeatPolicy,
        header_height: LayoutLength,
        footer_height: LayoutLength,
        wrapper_chrome: TableWrapperFragmentChrome,
    ) -> Self {
        let body_capacity = wrapper_chrome.fresh_body_capacity(repeat_policy.body_capacity(
            fragmentainer_block_size,
            header_height,
            footer_height,
        ));
        Self {
            base: Fragmentainer::new(fragmentainer_block_size, body_capacity),
            reserved_footer_height: layout_pt(0.0),
            reserved_wrapper_end: layout_pt(0.0),
            body_capacity,
        }
    }

    fn with_wrapper_end_reservation(mut self, wrapper_end: NonContentLength) -> Self {
        self.reserved_wrapper_end = layout_pt(wrapper_end.points());
        self
    }

    #[cfg(test)]
    pub(in crate::layout::table) fn fragmentainer_block_size(&self) -> LayoutLength {
        self.base.fragmentainer_block_size()
    }

    pub(in crate::layout::table) fn available_block_size(&self) -> LayoutLength {
        self.base.available_block_size()
    }

    pub(in crate::layout::table) fn required_block_size_overflows(
        &self,
        block_size: LayoutLength,
    ) -> bool {
        self.base.required_block_size_overflows(block_size)
    }

    pub(in crate::layout::table) fn available_body_size(&self) -> LayoutLength {
        self.base.available_block_size_after_reservation(layout_pt(
            self.reserved_footer_height.points() + self.reserved_wrapper_end.points(),
        ))
    }

    pub(in crate::layout::table) fn as_fragmentainer(&self) -> Fragmentainer {
        self.base
    }

    pub(in crate::layout::table) fn body_capacity_fragmentainer(&self) -> Fragmentainer {
        Fragmentainer::new(self.body_capacity, self.body_capacity)
    }
}

/// How an avoided table row group is kept together on the next fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableRowGroupAvoidMode {
    FitsNextFragment,
    KeptByChromeOverflow,
}

/// Committed keep-together choice for one avoided table row group.
///
/// The decision captures the row-group source range, measured block size, the
/// repeated header/footer policy chosen for the destination fragment, and
/// whether optional table chrome had to be suppressed to make progress:
/// <https://www.w3.org/TR/css-break-3/#break-within>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRowGroupAvoidDecision {
    pub(in crate::layout::table) group: TableAvoidRowGroup,
    pub(in crate::layout::table) required_block_size: LayoutLength,
    pub(in crate::layout::table) repeat_policy: TableFragmentRepeatPolicy,
    pub(in crate::layout::table) mode: TableRowGroupAvoidMode,
}

/// Tracks source rows kept together after a row-group avoid decision.
///
/// When a row group is kept together by allowing bounded table-chrome overflow,
/// subsequent source rows in that group must consume the committed
/// `KeptByAvoidOverflow` row mode and must not trigger nested row splitting.
/// This state records the committed source range until pagination advances past
/// the group end:
/// <https://www.w3.org/TR/css-break-3/#break-within>.
#[derive(Debug, Default, Clone, Copy)]
pub(in crate::layout::table) struct TableAvoidRowGroupKeepState {
    end: Option<usize>,
}

pub(in crate::layout::table) struct TableRowGroupAvoidDecisionInput {
    pub(in crate::layout::table) group: TableAvoidRowGroup,
    pub(in crate::layout::table) required_block_size: LayoutLength,
    pub(in crate::layout::table) current_fragmentainer: TableFragmentainer,
    pub(in crate::layout::table) chrome_context: TableFragmentChromeContext,
    pub(in crate::layout::table) can_advance: bool,
}

impl TableRowGroupAvoidDecision {
    pub(in crate::layout::table) fn choose(input: TableRowGroupAvoidDecisionInput) -> Option<Self> {
        if !input.can_advance {
            return None;
        }

        if !input
            .current_fragmentainer
            .required_block_size_overflows(input.required_block_size)
        {
            return None;
        }

        let repeat_policy = input
            .chrome_context
            .repeat_policy(input.required_block_size);
        let repeat_fragmentainer = input.chrome_context.fresh_fragmentainer(repeat_policy);
        if FragmentPrebreakDecision::choose(FragmentPrebreakInput {
            can_advance: input.can_advance,
            current_fragmentainer: input.current_fragmentainer.as_fragmentainer(),
            required_block_size: input.required_block_size,
            empty_fragmentainer: repeat_fragmentainer.body_capacity_fragmentainer(),
            empty_fit_block_size: input.required_block_size,
        })
        .should_break
        {
            return Some(Self {
                group: input.group,
                required_block_size: input.required_block_size,
                repeat_policy,
                mode: TableRowGroupAvoidMode::FitsNextFragment,
            });
        }

        let no_repeat_policy = TableFragmentRepeatPolicy {
            repeat_header: false,
            repeat_footer: false,
        };
        let no_repeat_fragmentainer = input
            .chrome_context
            .without_repeats()
            .fresh_fragmentainer(no_repeat_policy);
        (input.required_block_size.points()
            <= no_repeat_fragmentainer.body_capacity.points()
                + TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE)
            .then_some(Self {
                group: input.group,
                required_block_size: input.required_block_size,
                repeat_policy: no_repeat_policy,
                mode: TableRowGroupAvoidMode::KeptByChromeOverflow,
            })
    }

    pub(in crate::layout::table) fn keeps_with_overflow(self) -> bool {
        self.mode == TableRowGroupAvoidMode::KeptByChromeOverflow
    }
}

impl TableAvoidRowGroupKeepState {
    pub(in crate::layout::table) fn commit(&mut self, decision: TableRowGroupAvoidDecision) {
        if decision.keeps_with_overflow() {
            self.end = Some(decision.group.end);
        }
    }

    pub(in crate::layout::table) fn contains_row(self, row_index: usize) -> bool {
        self.end.is_some_and(|end| row_index < end)
    }

    pub(in crate::layout::table) fn finish_row(&mut self, next_row_index: usize) {
        if self.end.is_some_and(|end| next_row_index >= end) {
            self.end = None;
        }
    }
}

/// Choose optional repeated table rows for a fragment with required body space.
///
/// CSS 2.2 permits print user agents to repeat table header and footer groups
/// on each page, but CSS Fragmentation still requires progress and treats
/// `break-inside: avoid` as a constraint to honor when possible. Prefer
/// preserving both repeated groups, then the header, then the footer, and
/// finally suppress optional repeats before creating a fragmentainer with no
/// usable body area. The repeated chrome is page-oriented today, while the
/// capacity math consumes a generic fragmentainer block size:
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>
/// <https://www.w3.org/TR/css-break-3/#break-within>
pub(in crate::layout::table) fn table_fragment_repeat_policy(
    required_body_height: LayoutLength,
    fragmentainer_block_size: LayoutLength,
    header_height: LayoutLength,
    footer_height: LayoutLength,
    allow_header: bool,
    allow_footer: bool,
) -> TableFragmentRepeatPolicy {
    let candidates = [
        TableFragmentRepeatPolicy {
            repeat_header: allow_header,
            repeat_footer: allow_footer,
        },
        TableFragmentRepeatPolicy {
            repeat_header: allow_header,
            repeat_footer: false,
        },
        TableFragmentRepeatPolicy {
            repeat_header: false,
            repeat_footer: allow_footer,
        },
        TableFragmentRepeatPolicy {
            repeat_header: false,
            repeat_footer: false,
        },
    ];

    let required_body_height = layout_pt(required_body_height.points().max(0.0));
    for policy in candidates {
        let body_capacity =
            policy.body_capacity(fragmentainer_block_size, header_height, footer_height);
        if body_capacity.points() > 0.01
            && required_body_height.points() <= body_capacity.points() + 0.01
        {
            return policy;
        }
    }

    candidates
        .into_iter()
        .find(|policy| {
            policy
                .body_capacity(fragmentainer_block_size, header_height, footer_height)
                .points()
                > 0.01
        })
        .unwrap_or(TableFragmentRepeatPolicy {
            repeat_header: false,
            repeat_footer: false,
        })
}

/// One committed source-row slice exposed by a table fragment.
///
/// The source offset is deliberately retained in table-grid block coordinates;
/// it is not a page coordinate and must never be combined directly with a
/// destination fragmentainer origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableGridSourceRowSlice {
    pub(in crate::layout::table) row_index: usize,
    pub(in crate::layout::table) block_start: TableGridBlockOffset,
    pub(in crate::layout::table) block_size: TableGridLength,
}

/// Separately typed physical viewport receiving a source-grid projection.
///
/// A destination placement has the same logical axes and extent as the source
/// grid, but its physical origin belongs to the committed page/column piece.
/// Keeping the wrapper private prevents callers from mistaking it for the
/// stable source-grid placement.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableGridDestinationPaintViewport {
    placement: TableGridPlacement,
}

impl TableGridDestinationPaintViewport {
    fn new(placement: TableGridPlacement) -> Self {
        Self { placement }
    }

    pub(in crate::layout::table) fn placement(self) -> TableGridPlacement {
        self.placement
    }
}

/// Projection of immutable source-grid geometry into one committed table
/// fragment viewport.
///
/// Table tracks retain their unfragmented logical positions while each table
/// body fragment exposes only the row pieces recorded in its
/// [`TableFragmentPlan`]. Keeping those concepts together prevents callers
/// from accidentally treating a fragment-local page origin as a source-grid
/// offset.
/// <https://drafts.csswg.org/css-tables-3/#table-fragmentation>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableGridFragmentViewport {
    source_placement: TableGridPlacement,
    destination_viewport: TableGridDestinationPaintViewport,
    source_row_bounds: Vec<TableRowBounds>,
    source_row_slices: Vec<TableGridSourceRowSlice>,
}

/// The CSS table-root background view of one fragmented table body.
///
/// CSS Tables paints the table root from its grid, separated-border edge
/// spacing, padding, and border, but deliberately excludes captions.  The
/// source area therefore remains the complete root box for
/// `box-decoration-break: slice`, while every committed row piece supplies a
/// distinct destination clip in its fragmentainer.
/// <https://drafts.csswg.org/css-tables-3/#table-root>
/// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableRootBackgroundViewport {
    source_positioning_border_area: PaintBackgroundArea,
    fragments: Vec<TableRootBackgroundFragment>,
}

#[derive(Debug, Clone, Copy)]
struct TableRootBackgroundFragment {
    source_clip_border_area: PaintBackgroundArea,
    source_to_destination: PaintTranslation,
}

#[derive(Debug, Clone, Copy)]
struct TableRootLogicalInsets {
    inline_start: TableGridLength,
    inline_end: TableGridLength,
    block_start: TableGridLength,
    block_end: TableGridLength,
}

impl TableRootBackgroundViewport {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn new(
        source_placement: TableGridPlacement,
        destination_placement: TableGridPlacement,
        row_bounds: &[TableRowBounds],
        fragment_rows: &[usize],
        row_heights: &[f32],
        row_offsets: &[f32],
        style: &ComputedStyle,
        table_width: UsedTableWidth,
        block_edge_spacing: f32,
    ) -> Self {
        let insets = table_root_background_logical_insets(
            source_placement,
            style,
            table_width,
            block_edge_spacing,
        );
        let grid_inline = source_placement.logical_inline_grid_extent();
        let grid_block = source_placement.logical_block_grid_extent();
        let root_rect = TableGridRect::new(
            TableGridPoint::from_lengths(-insets.inline_start, -insets.block_start),
            TableGridSize::from_lengths(
                grid_inline + insets.inline_start + insets.inline_end,
                grid_block + insets.block_start + insets.block_end,
            ),
        );
        // Generic fragmented block painting retains the wrapper's trailing
        // decoration in its continuous source box.  The table grid ends at
        // the trailing content edge, so add that wrapper-owned extent here
        // rather than treating a row fragment as the positioning box.
        let source_positioning_rect = TableGridRect::new(
            root_rect.origin,
            TableGridSize::from_lengths(
                TableGridLength::new(root_rect.size.width),
                TableGridLength::new(root_rect.size.height) + insets.block_end,
            ),
        );
        let source_positioning_border_area = PaintBackgroundArea::from_paint_rect(
            source_placement
                .page_top_rect_for(source_positioning_rect)
                .paint_rect(),
        );
        let mut fragments = Vec::new();
        for (local_row, source_row) in fragment_rows.iter().copied().enumerate() {
            let (Some(row), Some(&row_height), Some(&row_offset)) = (
                row_bounds.get(source_row),
                row_heights.get(local_row),
                row_offsets.get(local_row),
            ) else {
                continue;
            };
            if row_height <= 0.0 {
                continue;
            }
            let block_start = TableGridLength::new(row.start + row_offset.max(0.0));
            let block_end = block_start + TableGridLength::new(row_height);
            let before = if block_start.get() <= 0.0 {
                insets.block_start
            } else {
                TableGridLength::new(0.0)
            };
            let after = if block_end >= grid_block {
                insets.block_end
            } else {
                TableGridLength::new(0.0)
            };
            let rect = TableGridRect::new(
                TableGridPoint::from_lengths(-insets.inline_start, block_start - before),
                TableGridSize::from_lengths(
                    grid_inline + insets.inline_start + insets.inline_end,
                    TableGridLength::new(row_height) + before + after,
                ),
            );
            let source_clip_border_area = PaintBackgroundArea::from_paint_rect(
                source_placement.page_top_rect_for(rect).paint_rect(),
            );
            let destination_clip_border_area = PaintBackgroundArea::from_paint_rect(
                destination_placement.page_top_rect_for(rect).paint_rect(),
            );
            fragments.push(TableRootBackgroundFragment {
                source_to_destination: PaintTranslation::new(
                    destination_clip_border_area.x() - source_clip_border_area.x(),
                    destination_clip_border_area.y() - source_clip_border_area.y(),
                ),
                source_clip_border_area,
            });
        }
        if fragments.is_empty() {
            let source_clip_border_area = source_positioning_border_area;
            let destination_clip_border_area = PaintBackgroundArea::from_paint_rect(
                destination_placement
                    .page_top_rect_for(root_rect)
                    .paint_rect(),
            );
            fragments.push(TableRootBackgroundFragment {
                source_to_destination: PaintTranslation::new(
                    destination_clip_border_area.x() - source_clip_border_area.x(),
                    destination_clip_border_area.y() - source_clip_border_area.y(),
                ),
                source_clip_border_area,
            });
        }
        Self {
            source_positioning_border_area,
            fragments,
        }
    }

    pub(in crate::layout::table) fn image_primitives(
        &self,
        style: &ComputedStyle,
        base_url: Option<&url::Url>,
        root_url: Option<&url::Url>,
        resource_cache: &ResourceCache,
    ) -> Vec<PaintPrimitive> {
        self.fragments
            .iter()
            .flat_map(|fragment| {
                let positioning_border_area =
                    if style.box_decoration_break == css::BoxDecorationBreak::Clone {
                        fragment.source_clip_border_area
                    } else {
                        self.source_positioning_border_area
                    };
                fragmented_table_root_background_image_primitives(
                    positioning_border_area,
                    fragment.source_clip_border_area,
                    style,
                    base_url,
                    root_url,
                    resource_cache,
                )
                .into_iter()
                // Table-root backgrounds resolve in physical CSS background
                // space.  A fragment projection is therefore a translation,
                // which `PaintPrimitive::translated` applies exhaustively to
                // paths, raster images, and every retained pattern kind.
                .map(|primitive| primitive.translated(fragment.source_to_destination))
            })
            .collect()
    }
}

fn table_root_background_logical_insets(
    placement: TableGridPlacement,
    style: &ComputedStyle,
    table_width: UsedTableWidth,
    block_edge_spacing: f32,
) -> TableRootLogicalInsets {
    let axes = WritingModeAxes::new(placement.writing_mode(), style.used_direction());
    let edge = |edges: css::Edges, side| match side {
        PhysicalSide::Top => edges.top,
        PhysicalSide::Right => edges.right,
        PhysicalSide::Bottom => edges.bottom,
        PhysicalSide::Left => edges.left,
    };
    let inset = |side| {
        TableGridLength::new(
            edge(table_width.border_widths, side) + edge(table_width.padding, side),
        )
    };
    TableRootLogicalInsets {
        inline_start: inset(axes.physical_side(LogicalSide::InlineStart)),
        inline_end: inset(axes.physical_side(LogicalSide::InlineEnd)),
        block_start: inset(axes.physical_side(LogicalSide::BlockStart))
            + TableGridLength::new(block_edge_spacing),
        block_end: inset(axes.physical_side(LogicalSide::BlockEnd))
            + TableGridLength::new(block_edge_spacing),
    }
}

impl TableGridFragmentViewport {
    fn new(
        source_placement: TableGridPlacement,
        destination_placement: TableGridPlacement,
        source_row_bounds: Vec<TableRowBounds>,
    ) -> Self {
        Self {
            source_placement,
            destination_viewport: TableGridDestinationPaintViewport::new(destination_placement),
            source_row_bounds,
            source_row_slices: Vec::new(),
        }
    }

    /// The unfragmented logical grid used to resolve structural background
    /// positioning. Its origin is deliberately independent of any destination
    /// page or column, as required by `box-decoration-break: slice`.
    pub(in crate::layout::table) fn destination_placement(&self) -> TableGridPlacement {
        self.destination_viewport.placement()
    }

    /// The retained unfragmented grid used to resolve `slice` backgrounds and
    /// borders before projecting a row piece into this fragmentainer.
    pub(in crate::layout::table) fn source_placement(&self) -> TableGridPlacement {
        self.source_placement
    }

    pub(in crate::layout::table) fn row_bounds(&self) -> &[TableRowBounds] {
        &self.source_row_bounds
    }

    fn record_source_row_slice(&mut self, decision: TableRowFragmentDecision) {
        let Some(row) = self.source_row_bounds.get(decision.row_index).copied() else {
            return;
        };
        let block_start = TableGridBlockOffset::new(TableGridLength::new(
            row.start + decision.row_offset.max(0.0),
        ));
        let block_size = TableGridLength::new(decision.row_height.max(0.0));
        if block_size.get() > 0.0 {
            self.source_row_slices.push(TableGridSourceRowSlice {
                row_index: decision.row_index,
                block_start,
                block_size,
            });
        }
    }

    pub(in crate::layout::table) fn source_row_slices(&self) -> &[TableGridSourceRowSlice] {
        &self.source_row_slices
    }
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

/// Structural table paint owned by a relatively positioned row or row group.
///
/// Table layout creates row and row-group backgrounds after row content has
/// been measured. Retaining those primitives with their originating style
/// lets finalization place them in the positioned auto stack rather than
/// flattening them into the table's in-flow background band.
/// <https://drafts.csswg.org/css-position-3/#relative-positioning>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct RelativeTablePartStructuralPaint {
    pub(in crate::layout::table) style: ComputedStyle,
    pub(in crate::layout::table) bounds: PaintClip,
    pub(in crate::layout::table) primitives: Vec<PaintPrimitive>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableCellLayoutMetrics {
    pub(in crate::layout::table) content_height: f32,
    pub(in crate::layout::table) border_box_height: f32,
    pub(in crate::layout::table) baseline_offset: f32,
}

pub(in crate::layout::table) struct PreparedTableCell {
    pub(in crate::layout::table) style: ComputedStyle,
    pub(in crate::layout::table) row_sizing_style: ComputedStyle,
    pub(in crate::layout::table) area: TableGridArea,
    pub(in crate::layout::table) inline_bounds: TableInlineBounds,
    pub(in crate::layout::table) borders: css::Edges,
    pub(in crate::layout::table) metrics: TableCellLayoutMetrics,
    pub(in crate::layout::table) text: String,
}

impl PreparedTableCell {
    pub(in crate::layout::table) fn width(&self) -> f32 {
        self.inline_bounds.page_width()
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
    pub(in crate::layout::table) definite_block_size_stack: Vec<BlockSizePercentageBasis>,
}

pub(in crate::layout::table) struct TableGridLayoutContext<'table, 'ctx> {
    pub(in crate::layout::table) rows: &'ctx [TableRow<'table>],
    pub(in crate::layout::table) grid: &'ctx TableGrid,
    pub(in crate::layout::table) table_style: &'ctx TableUsedStyle,
    pub(in crate::layout::table) stylesheets: &'ctx Stylesheets<'ctx>,
    pub(in crate::layout::table) table_cellpadding: Option<f32>,
    pub(in crate::layout::table) column_plan: &'ctx TableColumnPlan,
    pub(in crate::layout::table) table_metrics: TableMetrics,
    pub(in crate::layout::table) collapsed_geometry: Option<&'ctx CollapsedTableGeometry>,
    /// A flex/grid-assigned table-wrapper border-box block size. This is
    /// separate from the CSS `height` property, which sizes the table grid.
    /// <https://drafts.csswg.org/css-tables/#computing-the-table-height>
    pub(in crate::layout::table) wrapper_border_box_block_size: Option<BorderBoxLength>,
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
    pub(in crate::layout::table) table_cellpadding: Option<f32>,
    pub(in crate::layout::table) column_plan: &'a TableColumnPlan,
    pub(in crate::layout::table) planned_row_heights: &'a [f32],
    pub(in crate::layout::table) planned_row_occupancy: &'a [bool],
    pub(in crate::layout::table) table_metrics: TableMetrics,
    pub(in crate::layout::table) collapsed_geometry: Option<&'a CollapsedTableGeometry>,
    pub(in crate::layout::table) row_baseline_offset: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableCellBaselineSet {
    First,
    Last,
}

/// CSS Tables 3 row-height plan for first-pass minimums, reference sizes, and
/// final distributed row sizes.
///
/// Spec: <https://drafts.csswg.org/css-tables-3/#row-layout> and
/// <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct TableHeightPlan {
    pub(in crate::layout::table) rows: Vec<TableRowHeightPlan>,
    /// Definite table grid block-size target resolved before row distribution.
    /// This is distinct from the resulting intrinsic grid height: percentage
    /// descendants only become definite through an explicit, resolved target.
    pub(in crate::layout::table) target_content_height: Option<ContentBoxLength>,
}

/// Per-row state used by `TableHeightPlan`.
///
/// `base` is the ROWMIN-style first-pass size, `reference` includes
/// explicit/percentage row, row-group, and cell constraints, and `final_height`
/// is the size after the CSS Tables 3 distribution algorithm.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRowHeightPlan {
    pub(in crate::layout::table) base: f32,
    /// The row's pre-`visibility: collapse` intrinsic block contribution.
    /// Spanning-cell descendants are laid out against these source tracks
    /// before the collapsed tracks are removed from visible painting.
    pub(in crate::layout::table) source_height: f32,
    pub(in crate::layout::table) reference: f32,
    pub(in crate::layout::table) final_height: f32,
    pub(in crate::layout::table) auto: bool,
    pub(in crate::layout::table) collapsed: bool,
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

/// Shared CSS 2.2 collapsed-border geometry for one laid-out table.
///
/// The full resolved grid is the source of truth for table wrapper insets,
/// structural background bounds, and fragmented border painting.
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
pub(in crate::layout::table) struct CollapsedTableGeometry {
    pub(in crate::layout::table) grid: CollapsedBorderGrid,
    pub(in crate::layout::table) outer_insets: css::Edges,
}

impl CollapsedTableGeometry {
    pub(in crate::layout::table) fn cell_insets(
        &self,
        placement: &TableCellPlacement,
        row_index: usize,
    ) -> css::Edges {
        self.grid.cell_insets(
            row_index,
            placement.column,
            placement.colspan,
            placement.rowspan,
        )
    }
}

pub(in crate::layout::table) fn table_cell_border_insets(
    cell_style: &ComputedStyle,
    placement: &TableCellPlacement,
    row_index: usize,
    table_metrics: TableMetrics,
    collapsed_geometry: Option<&CollapsedTableGeometry>,
) -> css::Edges {
    if table_metrics.border_collapse == css::BorderCollapse::Collapse {
        return collapsed_geometry
            .map(|geometry| geometry.cell_insets(placement, row_index))
            .unwrap_or(css::Edges::ZERO);
    }
    used_border_widths(cell_style)
}

pub(in crate::layout::table) fn table_cell_border_box_height_with_insets(
    style: &ComputedStyle,
    content_height: f32,
    border_insets: css::Edges,
) -> f32 {
    table_cell_row_sizing_border_box_height(
        style,
        content_height,
        percentage_basis_from_points(Some(content_height)),
        border_insets,
    )
}

/// Resolve a table-cell minimum border-box height for row height distribution.
///
/// CSS Tables row layout treats a cell's specified `height` as a minimum input
/// to row sizing. The final table-cell box can still grow to fit required
/// in-flow content, so `max-height` must not clamp the row/cell border box:
/// <https://drafts.csswg.org/css-tables-3/#height-distribution> and
/// <https://www.w3.org/TR/CSS22/tables.html#height-layout>.
pub(in crate::layout::table) fn table_cell_row_sizing_border_box_height<Source: Copy>(
    style: &ComputedStyle,
    content_height: f32,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    border_insets: css::Edges,
) -> f32 {
    let vertical_non_content =
        style.padding.top + style.padding.bottom + border_insets.top + border_insets.bottom;
    let height_content = used_content_box_height_or_auto_with_basis(
        style,
        percentage_basis,
        non_content_pt(vertical_non_content),
    )
    .map(SemanticLengthExt::points)
    .unwrap_or(0.0);
    let min_height_content = used_length_percentage_or_auto_with_basis(
        style.box_values.min_height.clone(),
        percentage_basis,
    )
    .map(|height| height.points())
    .unwrap_or(0.0);
    content_height.max(height_content).max(min_height_content) + vertical_non_content
}

impl TableBodyPaintFragment {
    pub(in crate::layout::table) fn new(
        fragmentainer_kind: FragmentainerKind,
        checkpoint: PaintCheckpoint,
        page_index: usize,
        positioned_layer_start: usize,
        fragment_top: f32,
        start_decision: TableFragmentStartDecision,
    ) -> Self {
        Self {
            checkpoint,
            positioned_layer_start,
            plan: TableFragmentPlan::new(
                fragmentainer_kind,
                page_index,
                fragment_top,
                start_decision,
            ),
            grid_viewport: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn initialize_grid_placement(
        &mut self,
        decision: TableRowFragmentDecision,
        table_style: &ComputedStyle,
        table_x: f32,
        column_plan: &TableColumnPlan,
        source_grid_placement: TableGridPlacement,
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
                let logical_block_start = table_row_block_start(
                    planned_row_heights,
                    planned_row_occupancy,
                    decision.row_index,
                    table_metrics.clone(),
                );
                let table_block_extent = table_grid_height(
                    planned_row_heights,
                    planned_row_occupancy,
                    table_metrics.clone(),
                );
                let projected_table_x = if table_style.writing_mode.has_vertical_lines() {
                    table_x
                        + table_vertical_edge_spacing(planned_row_occupancy, table_metrics.clone())
                } else {
                    table_x
                };
                let projected_table_top = decision.row_top
                    + decision.row_offset
                    + logical_block_start
                    + if table_style.writing_mode.has_vertical_lines() {
                        table_metrics.spacing.horizontal.length_points()
                    } else {
                        0.0
                    };
                TableGridFragmentViewport::new(
                    source_grid_placement,
                    TableGridPlacement::with_axes(
                        PageTopPoint::new(projected_table_x, projected_table_top),
                        TableAxes::for_style(table_style),
                        TableGridLogicalSize::new(
                            column_plan.total_width(),
                            LogicalBlockContentSize::new(content_box_pt(table_block_extent)),
                        ),
                    ),
                    source_row_bounds,
                )
            })
            .destination_placement()
    }

    pub(in crate::layout::table) fn push_row_decision(
        &mut self,
        decision: TableRowFragmentDecision,
    ) {
        if let Some(viewport) = &mut self.grid_viewport {
            viewport.record_source_row_slice(decision);
        }
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

pub(in crate::layout::table) fn table_wrapper_border_box_height(
    content_height: f32,
    table_width: UsedTableWidth,
) -> f32 {
    content_height
        + table_width.padding.top
        + table_width.padding.bottom
        + table_width.border_widths.top
        + table_width.border_widths.bottom
}

/// Return the CSS overflow/paint-containment clip for a table box, excluding
/// wrapper captions.
///
/// CSS 2.1 errata makes `overflow` apply to the table box instead of the
/// table wrapper box, and defines `scroll`/`auto` as visible on table boxes.
/// The clipping edge therefore uses the table padding box around the grid, not
/// the wrapper area that contains captions:
/// <https://www.w3.org/Style/css2-updates/REC-CSS2-20110607-errata.html#s.11.1.1b>.
/// Paint containment uses the same table padding edge:
/// <https://www.w3.org/TR/css-contain-1/#containment-paint>.
pub(in crate::layout::table) fn table_box_overflow_clip(
    style: &ComputedStyle,
    padding_box: PaintClip,
    table_is_document_canvas: bool,
) -> Option<PaintClip> {
    if table_is_document_canvas {
        return None;
    }
    (style.contain.paint
        || matches!(
            effective_overflow_for_style(style),
            css::Overflow::Hidden | css::Overflow::Clip
        ))
    .then_some(padding_box)
}

pub(in crate::layout::table) fn table_padding_box_clip_from_border_box(
    border_box: PaintClip,
    table_width: UsedTableWidth,
) -> PaintClip {
    PaintClip::from_paint_rect(paint_space_rect(
        border_box.x() + table_width.border_widths.left,
        border_box.y() + table_width.border_widths.bottom,
        border_box.width() - table_width.border_widths.left - table_width.border_widths.right,
        border_box.height() - table_width.border_widths.top - table_width.border_widths.bottom,
    ))
}

pub(in crate::layout::table) fn table_atomic_stacking_policy(
    style: &ComputedStyle,
    parent_band: PaintBand,
    bounds: PaintClip,
    overflow_clip: Option<PaintClip>,
) -> StackingContextPolicy {
    let mut policy = StackingContextPolicy::for_atomic(style, parent_band, bounds);
    // Table layout records fragment-local paint structure, while the element
    // dispatcher owns the table element's principal effect context. Keeping
    // the transform here as well applies the same CTM once for the table
    // fragment and once for the owning element. Retain table-local overflow
    // clipping but let the enclosing context serialize the principal effect
    // exactly once.
    // <https://drafts.csswg.org/css-transforms-1/#transform-rendering>
    policy.effects.transform = None;
    policy.effects.suppress_paint = false;
    policy.effects.overflow_clip = overflow_clip;
    policy
}

pub(in crate::layout::table) fn table_horizontal_non_content_width(
    table_width: UsedTableWidth,
) -> f32 {
    table_width.horizontal_non_content().points()
}

pub(in crate::layout::table) fn table_content_width_clamped_to_min_content(
    style: &ComputedStyle,
    content_width: LogicalInlineContentSize,
    min_content: LogicalInlineContentSize,
) -> LogicalInlineContentSize {
    if style.table_layout == TableLayout::Auto {
        LogicalInlineContentSize::new(content_box_pt(
            content_width.points().max(min_content.points()),
        ))
    } else {
        content_width
    }
}

pub(in crate::layout::table) fn table_displayed_horizontal_spacing(
    visible_columns: usize,
    table_metrics: TableMetrics,
) -> f32 {
    if visible_columns == 0 {
        0.0
    } else {
        table_metrics.spacing.horizontal.length_points() * (visible_columns + 1) as f32
    }
}

/// Return separated-border gutters inside a logical column span.
///
/// CSS 2.2 places horizontal `border-spacing` between adjacent column cells.
/// A cell spanning multiple visible columns includes those internal gutters in
/// its border box, so column width constraints derived from that cell must
/// remove them before distributing the remaining width to tracks:
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>.
pub(in crate::layout::table) fn table_internal_horizontal_spacing(
    start_column: usize,
    end_column: usize,
    collapsed_columns: &[bool],
    table_metrics: TableMetrics,
) -> f32 {
    let end_column = end_column.min(collapsed_columns.len());
    if start_column >= end_column {
        return 0.0;
    }
    let visible_columns = collapsed_columns[start_column..end_column]
        .iter()
        .filter(|collapsed| !**collapsed)
        .count();
    table_metrics.spacing.horizontal.length_points() * visible_columns.saturating_sub(1) as f32
}

pub(in crate::layout::table) fn table_column_background_primitives(
    table_x: f32,
    grid_top: f32,
    grid_height: f32,
    column_plan: &TableColumnPlan,
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
) -> Vec<PaintPrimitive> {
    let Some((paint_rect, _inline_bounds)) = table_column_background_rect(
        table_x,
        grid_top,
        grid_height,
        column_plan,
        start_column,
        end_column,
        style,
    ) else {
        return Vec::new();
    };
    table_column_background_primitives_with_clip(paint_rect, style, paint_rect)
}

/// Paint a column layer against the root table's projected logical grid.
///
/// A column's background spans the table grid's block extent.  In a vertical
/// table that extent is physical width, not the legacy row fragment's physical
/// height, so structural painting must retain [`TableGridPlacement`] until it
/// reaches the page boundary.
/// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_column_grid_background_primitives(
    source_placement: TableGridPlacement,
    destination_placement: TableGridPlacement,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_bounds: &[TableRowBounds],
    row_heights: &[f32],
    row_offsets: &[f32],
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    if start_column >= end_column || start_column >= column_plan.column_count() {
        return Vec::new();
    }
    // `TableColumnPlan` has already resolved `direction` for its inline
    // bounds. `TableGridPlacement` deliberately projects table-grid slots
    // with LTR inline progression to avoid applying that reversal twice.
    let inline_bounds = column_plan.inline_bounds_for_span(
        start_column,
        end_column.min(column_plan.column_count()) - start_column,
    );
    let positioning_rect = TableGridRect::new(
        TableGridPoint::from_lengths(inline_bounds.start, TableGridLength::new(0.0)),
        TableGridSize::from_lengths(
            inline_bounds.size,
            source_placement.logical_block_grid_extent(),
        ),
    );
    let logical_paint_view =
        source_placement.logical_paint_view_with_inline_edge(column_plan.horizontal_spacing);
    let logical_positioning_area = PaintBackgroundArea::from_paint_rect(
        logical_paint_view
            .overflow_clip_for(positioning_rect)
            .paint_rect(),
    );
    let logical_paint_transform = table_grid_source_to_destination_transform(
        source_placement,
        destination_placement,
        column_plan.horizontal_spacing,
    );
    let image_style = table_column_image_style_for_placement(style, source_placement);
    let mut primitives = Vec::new();
    for (destination_clip, source_clip) in table_column_grid_cell_clips(
        source_placement,
        destination_placement,
        column_plan,
        table_grid,
        row_bounds,
        fragment_rows,
        row_heights,
        row_offsets,
        start_column,
        end_column,
    ) {
        primitives.extend(table_column_background_primitives_with_clip(
            destination_clip,
            style,
            destination_clip,
        ));
        let images = structural_table_background_image_primitives(
            logical_positioning_area,
            PaintBackgroundArea::from_paint_rect(source_clip),
            &image_style,
            base_url,
            root_url,
            resource_cache,
        );
        if source_placement != destination_placement
            || source_placement.writing_mode().has_vertical_lines()
        {
            primitives.extend(images.into_iter().map(|primitive| {
                transform_table_column_image_primitive(primitive, logical_paint_transform)
            }));
        } else {
            primitives.extend(images);
        }
    }
    primitives
}

/// Project the cell-derived paint regions through the retained table grid.
///
/// A structural column layer is positioned against its complete column span,
/// but CSS Tables exposes it only in cells participating in that span.  Keep
/// the source row tracks and the fragment's visible row pieces separate until
/// this final projection so `rowspan`, `colspan`, and vertical writing modes
/// share one clipping rule.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>
#[allow(clippy::too_many_arguments)]
fn table_column_grid_cell_clips(
    source_placement: TableGridPlacement,
    destination_placement: TableGridPlacement,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    row_bounds: &[TableRowBounds],
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    start_column: usize,
    end_column: usize,
) -> Vec<(PaintRect, PaintRect)> {
    let mut clips = Vec::new();
    for (origin_row, cells) in table_grid.rows.iter().enumerate() {
        for cell in cells {
            let cell_end_column = cell.column.saturating_add(cell.colspan);
            if cell.column >= end_column || cell_end_column <= start_column {
                continue;
            }
            let cell_inline = column_plan.inline_bounds_for_span(cell.column, cell.colspan);
            let cell_end_row = origin_row.saturating_add(cell.rowspan.max(1));
            for (local_row, source_row) in fragment_rows.iter().copied().enumerate() {
                if source_row < origin_row || source_row >= cell_end_row {
                    continue;
                }
                let (Some(source), Some(&visible_size), Some(&visible_offset)) = (
                    row_bounds.get(source_row),
                    row_heights.get(local_row),
                    row_offsets.get(local_row),
                ) else {
                    continue;
                };
                if visible_size <= 0.0 {
                    continue;
                }
                let rect = TableGridRect::new(
                    TableGridPoint::from_lengths(
                        cell_inline.start,
                        TableGridLength::new(source.start + visible_offset.max(0.0)),
                    ),
                    TableGridSize::from_lengths(
                        cell_inline.size,
                        TableGridLength::new(visible_size),
                    ),
                );
                clips.push((
                    destination_placement.overflow_clip_for(rect).paint_rect(),
                    source_placement.overflow_clip_for(rect).paint_rect(),
                ));
            }
        }
    }
    clips
}

/// Paint a table-root structural background through the visible source-row
/// pieces of one committed fragment.
///
/// The table grid remains the background positioning area under the default
/// `box-decoration-break: slice`; the fragment viewport only limits paint.
/// Keeping this at the table-grid boundary makes table-root images use the
/// same retained source geometry as row, row-group, and column layers.
/// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
#[allow(clippy::needless_collect)]
pub(in crate::layout::table) fn table_grid_fragment_background_primitives(
    source_placement: TableGridPlacement,
    destination_placement: TableGridPlacement,
    row_bounds: &[TableRowBounds],
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    style: &ComputedStyle,
    collapsed_outer_insets: css::Edges,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let has_collapsed_outer_insets = collapsed_outer_insets != css::Edges::ZERO;
    let logical_paint_view =
        source_placement.logical_paint_view_with_inline_edge(TableGridLength::new(0.0));
    let positioning_rect = logical_paint_view.full_page_top_rect().paint_rect();
    let source_clips: Vec<_> = fragment_rows
        .iter()
        .enumerate()
        .filter_map(|(local_row, source_row)| {
            let source = row_bounds.get(*source_row)?;
            let row_height = *row_heights.get(local_row)?;
            if row_height <= 0.0 {
                return None;
            }
            let row_offset = *row_offsets.get(local_row)?;
            Some(
                source_placement
                    .page_top_rect_for(TableGridRect::new(
                        TableGridPoint::from_lengths(
                            TableGridLength::new(0.0),
                            TableGridLength::new(source.start + row_offset),
                        ),
                        TableGridSize::from_lengths(
                            source_placement.logical_inline_grid_extent(),
                            TableGridLength::new(row_height),
                        ),
                    ))
                    .paint_rect(),
            )
        })
        .collect();
    let destination_clips: Vec<_> = fragment_rows
        .iter()
        .enumerate()
        .filter_map(|(local_row, source_row)| {
            let source = row_bounds.get(*source_row)?;
            let row_height = *row_heights.get(local_row)?;
            if row_height <= 0.0 {
                return None;
            }
            let row_offset = *row_offsets.get(local_row)?;
            Some(
                destination_placement
                    .logical_paint_view_with_inline_edge(TableGridLength::new(0.0))
                    .page_top_rect_for(TableGridRect::new(
                        TableGridPoint::from_lengths(
                            TableGridLength::new(0.0),
                            TableGridLength::new(source.start + row_offset),
                        ),
                        TableGridSize::from_lengths(
                            logical_paint_view.logical_inline_grid_extent(),
                            TableGridLength::new(row_height),
                        ),
                    ))
                    .paint_rect(),
            )
        })
        .collect();
    let mut background_style = style.clone();
    if has_collapsed_outer_insets {
        // The structural helper clips root colors to row pieces. Collapsed
        // outer borders sit outside those pieces, so paint the color clips
        // below with their physical table-wrapper outsets instead.
        background_style.background_color = css::BackgroundColor::TRANSPARENT;
    }
    let mut primitives = table_grid_structural_background_primitives(
        source_placement,
        destination_placement,
        positioning_rect,
        source_clips.into_iter().zip(destination_clips).collect(),
        &background_style,
        base_url,
        root_url,
        resource_cache,
    );
    if let Some(fill) = style.background_color.visible_color(style.color)
        && has_collapsed_outer_insets
    {
        let unfragmented_grid = fragment_rows.len() == row_bounds.len()
            && fragment_rows
                .iter()
                .enumerate()
                .all(|(row, source_row)| row == *source_row);
        if unfragmented_grid {
            let rect = source_placement.full_page_top_rect();
            let expanded = PageTopRect::new(
                rect.x() - collapsed_outer_insets.left,
                rect.top_y() + collapsed_outer_insets.top,
                rect.width() + collapsed_outer_insets.left + collapsed_outer_insets.right,
                rect.height() + collapsed_outer_insets.top + collapsed_outer_insets.bottom,
            )
            .paint_rect();
            primitives.push(PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                expanded,
                Some(fill),
            )));
        } else {
            for (local_row, source_row) in fragment_rows.iter().enumerate() {
                let (Some(source), Some(row_height), Some(row_offset)) = (
                    row_bounds.get(*source_row),
                    row_heights.get(local_row),
                    row_offsets.get(local_row),
                ) else {
                    continue;
                };
                if *row_height <= 0.0 {
                    continue;
                }
                let mut top = 0.0;
                let mut bottom = 0.0;
                if *source_row == 0 {
                    top = collapsed_outer_insets.top;
                }
                if *source_row + 1 == row_bounds.len() {
                    bottom = collapsed_outer_insets.bottom;
                }
                let rect = source_placement.page_top_rect_for(TableGridRect::new(
                    TableGridPoint::from_lengths(
                        TableGridLength::new(0.0),
                        TableGridLength::new(source.start + *row_offset),
                    ),
                    TableGridSize::from_lengths(
                        source_placement.logical_inline_grid_extent(),
                        TableGridLength::new(*row_height),
                    ),
                ));
                let expanded = PageTopRect::new(
                    rect.x() - collapsed_outer_insets.left,
                    rect.top_y() + top,
                    rect.width() + collapsed_outer_insets.left + collapsed_outer_insets.right,
                    rect.height() + top + bottom,
                )
                .paint_rect();
                primitives.push(PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                    expanded,
                    Some(fill),
                )));
            }
        }
    }
    primitives
}

/// Map the stable source-grid painting space into one physical destination
/// fragment. Source coordinates stay continuous across columns/pages, while
/// the destination projection applies the table root's writing mode exactly
/// once at the final paint boundary.
fn table_grid_source_to_destination_transform(
    source_placement: TableGridPlacement,
    destination_placement: TableGridPlacement,
    inline_edge: TableGridLength,
) -> PaintTransform {
    let source_view = source_placement.logical_paint_view_with_inline_edge(inline_edge);
    let destination_view = destination_placement.logical_paint_view_with_inline_edge(inline_edge);
    let source_rect = source_view.full_page_top_rect().paint_rect();
    let destination_rect = destination_view.full_page_top_rect().paint_rect();
    destination_placement
        .logical_paint_to_page_transform(inline_edge)
        .multiply(PaintTransform::translate(PaintTranslation::new(
            destination_rect.origin.x - source_rect.origin.x,
            destination_rect.origin.y - source_rect.origin.y,
        )))
}

/// Apply the root table's logical-to-page transform to a structural image
/// primitive. Vector paths and retained CSS gradient patterns both own their
/// source-local geometry, so each can paint through a destination fragment.
fn transform_table_column_image_primitive(
    primitive: PaintPrimitive,
    transform: PaintTransform,
) -> PaintPrimitive {
    match primitive {
        PaintPrimitive::Path(mut path) => {
            path.transform = transform.multiply(path.transform);
            PaintPrimitive::Path(path)
        }
        PaintPrimitive::GradientPattern(pattern) => {
            PaintPrimitive::GradientPattern(pattern.transformed(transform))
        }
        primitive => primitive,
    }
}

/// Project structural column gradients into the physical orientation of the
/// table grid.  The generic painter receives the already-projected physical
/// box, so vertical-rl needs the corresponding quarter-turn of a logical
/// gradient direction.
fn table_column_image_style_for_placement(
    style: &ComputedStyle,
    placement: TableGridPlacement,
) -> ComputedStyle {
    let angle_delta = match placement.writing_mode() {
        WritingMode::VerticalRl | WritingMode::SidewaysRl => -90.0,
        WritingMode::VerticalLr | WritingMode::SidewaysLr => 90.0,
        WritingMode::HorizontalTb => 0.0,
    };
    if angle_delta == 0.0 {
        return style.clone();
    }
    let mut projected = style.clone();
    for layer in &mut projected.background_layers {
        if let Some(css::BackgroundImage::LinearGradient(gradient)) = layer.image.as_image_mut()
            && let LinearGradientDirection::Angle(angle) = &mut gradient.direction
        {
            *angle = (*angle + angle_delta).rem_euclid(360.0);
        }
    }
    if let Some(css::BackgroundImage::LinearGradient(gradient)) =
        projected.background_image.as_image_mut()
        && let LinearGradientDirection::Angle(angle) = &mut gradient.direction
    {
        *angle = (*angle + angle_delta).rem_euclid(360.0);
    }
    projected
}

#[allow(clippy::too_many_arguments)]
/// Paint a column or column-group background through cell-derived clips.
///
/// CSS Tables 3 renders column backgrounds as if each originating cell exposed
/// the column's background, so separated row spacing must remain unpainted
/// while the full column box remains the background positioning area:
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>.
pub(in crate::layout::table) fn table_column_fragment_background_primitives(
    table_x: f32,
    grid_top: f32,
    grid_height: f32,
    column_plan: &TableColumnPlan,
    table_grid: Option<&TableGrid>,
    fragment_rows: &[usize],
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
    row_tops: &[f32],
    row_heights: &[f32],
) -> Vec<PaintPrimitive> {
    if matches!(
        style.writing_mode,
        WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr
    ) {
        return table_column_background_primitives(
            table_x,
            grid_top,
            grid_height,
            column_plan,
            start_column,
            end_column,
            style,
        );
    }
    let Some((paint_rect, _inline_bounds)) = table_column_background_rect(
        table_x,
        grid_top,
        grid_height,
        column_plan,
        start_column,
        end_column,
        style,
    ) else {
        return Vec::new();
    };
    let cell_derived_clips = table_grid.map(|table_grid| {
        table_column_fragment_cell_clips(
            table_x,
            column_plan,
            table_grid,
            fragment_rows,
            row_tops,
            row_heights,
            start_column,
            end_column,
        )
    });
    let clips = cell_derived_clips.unwrap_or_else(|| {
        row_tops
            .iter()
            .cloned()
            .zip(row_heights.iter().cloned())
            .filter(|(_, row_height)| *row_height > 0.0)
            .map(|(row_top, row_height)| {
                intersect_paint_rect_or_empty(
                    paint_rect,
                    paint_space_rect(
                        paint_rect.origin.x,
                        row_top - row_height,
                        paint_rect.size.width,
                        row_height,
                    ),
                )
            })
            .collect()
    });
    let mut primitives = Vec::new();
    if let Some(fill) = style.background_color.visible_color(style.color) {
        primitives.extend(
            clips
                .into_iter()
                .map(|clip| PaintPrimitive::Rect(RenderedRect::from_paint_rect(clip, Some(fill)))),
        );
    }
    primitives
}

/// Paint CSS background-image layers for a column or column group through the
/// cell-derived clips exposed by the current row fragment.
///
/// The structural background's positioning area is the complete column box,
/// while each participating row exposes only its cell-height slice. Reusing
/// the normal background painter keeps gradients, URL images, sizing,
/// positioning, and repetition consistent with ordinary boxes.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_column_fragment_background_image_primitives(
    table_x: f32,
    grid_top: f32,
    grid_height: f32,
    column_plan: &TableColumnPlan,
    table_grid: Option<&TableGrid>,
    fragment_rows: &[usize],
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
    row_tops: &[f32],
    row_heights: &[f32],
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let Some((paint_rect, _inline_bounds)) = table_column_background_rect(
        table_x,
        grid_top,
        grid_height,
        column_plan,
        start_column,
        end_column,
        style,
    ) else {
        return Vec::new();
    };
    let positioning_area = PaintBackgroundArea::from_paint_rect(paint_rect);
    let clips = if matches!(
        style.writing_mode,
        WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr
    ) {
        vec![paint_rect]
    } else if let Some(table_grid) = table_grid {
        table_column_fragment_cell_clips(
            table_x,
            column_plan,
            table_grid,
            fragment_rows,
            row_tops,
            row_heights,
            start_column,
            end_column,
        )
    } else {
        row_tops
            .iter()
            .cloned()
            .zip(row_heights.iter().cloned())
            .filter(|(_, row_height)| *row_height > 0.0)
            .map(|(row_top, row_height)| {
                intersect_paint_rect_or_empty(
                    paint_rect,
                    paint_space_rect(
                        paint_rect.origin.x,
                        row_top - row_height,
                        paint_rect.size.width,
                        row_height,
                    ),
                )
            })
            .collect()
    };
    clips
        .into_iter()
        .filter(|clip| clip.size.width > 0.0 && clip.size.height > 0.0)
        .flat_map(|clip| {
            structural_table_background_image_primitives(
                positioning_area,
                PaintBackgroundArea::from_paint_rect(clip),
                style,
                base_url,
                root_url,
                resource_cache,
            )
        })
        .collect()
}

/// Return the exposed cell slices for a structural column background.
///
/// A column background is positioned against the complete column box, but it
/// is painted only through cells that overlap that column. In particular, a
/// `colspan` must not expose a column image in its other grid columns, and a
/// `rowspan` keeps its cell clip continuous across the rows it occupies.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>
#[allow(clippy::too_many_arguments)]
fn table_column_fragment_cell_clips(
    table_x: f32,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_tops: &[f32],
    row_heights: &[f32],
    start_column: usize,
    end_column: usize,
) -> Vec<PaintRect> {
    let mut clips = Vec::new();
    for source_row in fragment_rows.iter().cloned() {
        let Some(placements) = table_grid.rows.get(source_row) else {
            continue;
        };
        for placement in placements {
            let cell_end = placement.column.saturating_add(placement.colspan);
            if placement.column >= end_column || cell_end <= start_column {
                continue;
            }
            let mut cell_top = None;
            let mut cell_bottom = None;
            for (covered_local_row, covered_source_row) in fragment_rows.iter().cloned().enumerate()
            {
                if covered_source_row < source_row
                    || covered_source_row >= source_row.saturating_add(placement.rowspan)
                {
                    continue;
                }
                let (Some(row_top), Some(row_height)) = (
                    row_tops.get(covered_local_row).cloned(),
                    row_heights.get(covered_local_row).cloned(),
                ) else {
                    continue;
                };
                if row_height <= 0.0 {
                    continue;
                }
                cell_top = Some(cell_top.map_or(row_top, |top: f32| top.max(row_top)));
                let row_bottom = row_top - row_height;
                cell_bottom =
                    Some(cell_bottom.map_or(row_bottom, |bottom: f32| bottom.min(row_bottom)));
            }
            let (Some(cell_top), Some(cell_bottom)) = (cell_top, cell_bottom) else {
                continue;
            };
            let cell_inline =
                column_plan.inline_bounds_for_span(placement.column, placement.colspan);
            let cell_rect = paint_space_rect(
                cell_inline.page_x(table_x),
                cell_bottom,
                cell_inline.page_width(),
                (cell_top - cell_bottom).max(0.0),
            );
            if cell_rect.size.width > 0.0 && cell_rect.size.height > 0.0 {
                clips.push(cell_rect);
            }
        }
    }
    clips
}

/// Paint one row's structural background through the cells it originates.
///
/// CSS Tables draws a row background in its originating cells. A cell that
/// spans later rows therefore continues to expose that row's background, while
/// the image still positions against the originating row box.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_row_fragment_background_primitives(
    table_x: f32,
    positioning_rect: PaintRect,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_tops: &[f32],
    row_heights: &[f32],
    row_offsets: &[f32],
    original_row_heights: &[f32],
    row_index: usize,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let clips = table_row_fragment_cell_clips(
        table_x,
        column_plan,
        table_grid,
        fragment_rows,
        row_tops,
        row_heights,
        row_index,
    );
    // `box-decoration-break` defaults to `slice`, so a row background is
    // positioned against the unfragmented source row even though each table
    // fragment exposes it only through the cells visible in that fragment.
    // In particular, a repeating image must not restart at a column/page
    // boundary.  The row plan retains the amount already consumed from the
    // source row and its original height precisely for this projection:
    // <https://www.w3.org/TR/css-break-3/#break-decoration>.
    let positioning_rect = fragment_rows
        .iter()
        .position(|source_row| *source_row == row_index)
        .and_then(|local_row| {
            let top = *row_tops.get(local_row)? + *row_offsets.get(local_row)?;
            let height = *original_row_heights.get(local_row)?;
            (height > 0.0).then_some(paint_space_rect(
                positioning_rect.origin.x,
                top - height,
                positioning_rect.size.width,
                height,
            ))
        })
        .unwrap_or(positioning_rect);
    let positioning_area = PaintBackgroundArea::from_paint_rect(positioning_rect);
    let mut primitives = Vec::new();
    if let Some(fill) = style.background_color.visible_color(style.color) {
        primitives.extend(
            clips
                .iter()
                .cloned()
                .map(|clip| PaintPrimitive::Rect(RenderedRect::from_paint_rect(clip, Some(fill)))),
        );
    }
    primitives.extend(clips.into_iter().flat_map(|clip| {
        structural_table_background_image_primitives(
            positioning_area,
            PaintBackgroundArea::from_paint_rect(clip),
            style,
            base_url,
            root_url,
            resource_cache,
        )
    }));
    primitives
}

/// Paint one row background from source table-grid geometry.
///
/// Unlike fragment-local `row_top` values, `row_bounds` identifies the whole
/// source row.  The positioning rectangle therefore remains continuous under
/// the default `box-decoration-break: slice`, while the generated primitives
/// are visible only through originating cell pieces in this fragment.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds> and
/// <https://www.w3.org/TR/css-break-3/#break-decoration>.
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_row_grid_background_primitives(
    source_placement: TableGridPlacement,
    destination_placement: TableGridPlacement,
    row_bounds: &[TableRowBounds],
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    row_index: usize,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let Some(source_row) = row_bounds.get(row_index).copied() else {
        return Vec::new();
    };
    let logical_paint_view =
        source_placement.logical_paint_view_with_inline_edge(TableGridLength::new(0.0));
    let positioning_rect = logical_paint_view
        .page_top_rect_for(TableGridRect::new(
            TableGridPoint::from_lengths(
                TableGridLength::new(0.0),
                TableGridLength::new(source_row.start),
            ),
            TableGridSize::from_lengths(
                source_placement.logical_inline_grid_extent(),
                TableGridLength::new(source_row.size),
            ),
        ))
        .paint_rect();
    let clips = table_originating_cell_grid_clips(
        source_placement,
        destination_placement,
        row_bounds,
        column_plan,
        table_grid,
        fragment_rows,
        row_heights,
        row_offsets,
        row_index,
        row_index,
        row_index.saturating_add(1),
    );
    table_grid_structural_background_primitives(
        source_placement,
        destination_placement,
        positioning_rect,
        clips,
        style,
        base_url,
        root_url,
        resource_cache,
    )
}

/// Paint one row-group background from source table-grid geometry.
///
/// Row groups and rows deliberately share originating-cell clipping so cells
/// spanning later source rows expose the correct structural background in a
/// fragmented table.
#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_row_group_grid_background_primitives(
    source_placement: TableGridPlacement,
    destination_placement: TableGridPlacement,
    row_bounds: &[TableRowBounds],
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    start_row: usize,
    end_row: usize,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let Some(start) = row_bounds.get(start_row).copied() else {
        return Vec::new();
    };
    let Some(end) = end_row
        .checked_sub(1)
        .and_then(|index| row_bounds.get(index))
        .copied()
    else {
        return Vec::new();
    };
    let logical_paint_view =
        source_placement.logical_paint_view_with_inline_edge(TableGridLength::new(0.0));
    let positioning_rect = logical_paint_view
        .page_top_rect_for(TableGridRect::new(
            TableGridPoint::from_lengths(
                TableGridLength::new(0.0),
                TableGridLength::new(start.start),
            ),
            TableGridSize::from_lengths(
                source_placement.logical_inline_grid_extent(),
                TableGridLength::new((end.start + end.size - start.start).max(0.0)),
            ),
        ))
        .paint_rect();
    let clips: Vec<(PaintRect, PaintRect)> = (start_row..end_row)
        .flat_map(|originating_row| {
            table_originating_cell_grid_clips(
                source_placement,
                destination_placement,
                row_bounds,
                column_plan,
                table_grid,
                fragment_rows,
                row_heights,
                row_offsets,
                originating_row,
                start_row,
                end_row,
            )
        })
        .collect();
    table_grid_structural_background_primitives(
        source_placement,
        destination_placement,
        positioning_rect,
        clips,
        style,
        base_url,
        root_url,
        resource_cache,
    )
}

#[allow(clippy::too_many_arguments)]
fn table_originating_cell_grid_clips(
    source_placement: TableGridPlacement,
    destination_placement: TableGridPlacement,
    row_bounds: &[TableRowBounds],
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    originating_row: usize,
    structural_start_row: usize,
    structural_end_row: usize,
) -> Vec<(PaintRect, PaintRect)> {
    let Some(placements) = table_grid.rows.get(originating_row) else {
        return Vec::new();
    };
    let mut clips = Vec::new();
    for cell in placements {
        let cell_start_row = originating_row;
        let cell_end_row = originating_row
            .saturating_add(cell.rowspan.max(1))
            .min(row_bounds.len());
        let Some(cell_start) = row_bounds.get(cell_start_row).copied() else {
            continue;
        };
        let Some(cell_end) = cell_end_row
            .checked_sub(1)
            .and_then(|index| row_bounds.get(index))
            .copied()
        else {
            continue;
        };
        let cell_block_start = cell_start.start;
        let cell_block_end = cell_end.start + cell_end.size;
        let inline = column_plan.inline_bounds_for_span(cell.column, cell.colspan);
        for (local_row, source_row) in fragment_rows.iter().copied().enumerate() {
            if source_row < structural_start_row
                || source_row >= structural_end_row
                || source_row < cell_start_row
                || source_row >= cell_end_row
            {
                continue;
            }
            let (Some(source_bounds), Some(visible_size), Some(visible_offset)) = (
                row_bounds.get(source_row),
                row_heights.get(local_row),
                row_offsets.get(local_row),
            ) else {
                continue;
            };
            let visible_start = source_bounds.start + visible_offset.max(0.0);
            let visible_end = visible_start + visible_size.max(0.0);
            let start = visible_start.max(cell_block_start);
            let end = visible_end.min(cell_block_end);
            if end <= start {
                continue;
            }
            let rect = TableGridRect::new(
                TableGridPoint::from_lengths(inline.start, TableGridLength::new(start)),
                TableGridSize::from_lengths(inline.size, TableGridLength::new(end - start)),
            );
            clips.push((
                source_placement.page_top_rect_for(rect).paint_rect(),
                destination_placement.page_top_rect_for(rect).paint_rect(),
            ));
        }
    }
    clips
}

/// Paint table structural layers from source-grid geometry into the cell
/// regions exposed by a single destination fragment. CSS background colors
/// use the physical destination clips; images resolve in the unfragmented
/// source positioning area and are then transformed once into the root table's
/// writing mode.
#[allow(clippy::too_many_arguments)]
fn table_grid_structural_background_primitives(
    source_placement: TableGridPlacement,
    destination_placement: TableGridPlacement,
    source_positioning_rect: PaintRect,
    clips: Vec<(PaintRect, PaintRect)>,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let positioning_area = PaintBackgroundArea::from_paint_rect(source_positioning_rect);
    let source_to_destination = table_grid_source_to_destination_transform(
        source_placement,
        destination_placement,
        TableGridLength::new(0.0),
    );
    let mut primitives = Vec::new();
    if let Some(fill) = style.background_color.visible_color(style.color) {
        primitives.extend(clips.iter().map(|(_, destination_clip)| {
            PaintPrimitive::Rect(RenderedRect::from_paint_rect(*destination_clip, Some(fill)))
        }));
    }
    for (source_clip, _) in clips {
        let images = structural_table_background_image_primitives(
            positioning_area,
            PaintBackgroundArea::from_paint_rect(source_clip),
            style,
            base_url,
            root_url,
            resource_cache,
        );
        primitives.extend(images.into_iter().map(|primitive| {
            transform_table_column_image_primitive(primitive, source_to_destination)
        }));
    }
    primitives
}

#[allow(clippy::too_many_arguments)]
fn table_row_fragment_cell_clips(
    table_x: f32,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_tops: &[f32],
    row_heights: &[f32],
    row_index: usize,
) -> Vec<PaintRect> {
    let Some(placements) = table_grid.rows.get(row_index) else {
        return Vec::new();
    };
    let mut clips = Vec::new();
    for placement in placements {
        let mut cell_top = None;
        let mut cell_bottom = None;
        for (local_row, source_row) in fragment_rows.iter().cloned().enumerate() {
            if source_row < row_index || source_row >= row_index.saturating_add(placement.rowspan) {
                continue;
            }
            let (Some(row_top), Some(row_height)) = (
                row_tops.get(local_row).cloned(),
                row_heights.get(local_row).cloned(),
            ) else {
                continue;
            };
            if row_height <= 0.0 {
                continue;
            }
            cell_top = Some(cell_top.map_or(row_top, |top: f32| top.max(row_top)));
            let row_bottom = row_top - row_height;
            cell_bottom =
                Some(cell_bottom.map_or(row_bottom, |bottom: f32| bottom.min(row_bottom)));
        }
        let (Some(cell_top), Some(cell_bottom)) = (cell_top, cell_bottom) else {
            continue;
        };
        let cell_inline = column_plan.inline_bounds_for_span(placement.column, placement.colspan);
        clips.push(paint_space_rect(
            cell_inline.page_x(table_x),
            cell_bottom,
            cell_inline.page_width(),
            (cell_top - cell_bottom).max(0.0),
        ));
    }
    clips
}

fn table_column_background_rect(
    table_x: f32,
    grid_top: f32,
    grid_height: f32,
    column_plan: &TableColumnPlan,
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
) -> Option<(PaintRect, TableInlineBounds)> {
    if start_column >= end_column || start_column >= column_plan.column_count() {
        return None;
    }
    let clamped_end = end_column.min(column_plan.column_count());
    let inline_bounds =
        column_plan.inline_bounds_for_span(start_column, clamped_end - start_column);
    let block_size = if matches!(
        style.writing_mode,
        WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr
    ) {
        used_length_percentage_or_auto(
            style.box_values.height.value().clone(),
            PercentageBasis::definite(layout_pt(grid_height)),
        )
        .map(|height| height.points())
        .unwrap_or(grid_height)
        .max(grid_height)
    } else {
        grid_height
    };
    let rect = TableGridRect::new(
        TableGridPoint::from_lengths(inline_bounds.start, TableGridLength::new(0.0)),
        TableGridSize::from_lengths(inline_bounds.size, TableGridLength::new(block_size)),
    );
    let placement = TableGridPlacement::with_axes(
        PageTopPoint::new(table_x, grid_top),
        column_plan.axes,
        TableGridLogicalSize::new(
            column_plan.total_width(),
            LogicalBlockContentSize::new(content_box_pt(block_size)),
        ),
    );
    let paint_rect = placement.overflow_clip_for(rect).paint_rect();
    Some((paint_rect, inline_bounds))
}

fn table_column_background_primitives_with_clip(
    paint_rect: PaintRect,
    style: &ComputedStyle,
    clip: PaintRect,
) -> Vec<PaintPrimitive> {
    let mut rects = Vec::new();
    if paint_rect.size.width <= 0.0
        || paint_rect.size.height <= 0.0
        || clip.size.width <= 0.0
        || clip.size.height <= 0.0
    {
        return Vec::new();
    }
    if let Some(fill) = style.background_color.visible_color(style.color) {
        let area = background_rect_clip_area_for_box(
            paint_rect,
            style,
            css::Edges::ZERO,
            style.background_clip,
            Some(clip),
        );
        if area.size.width > 0.0 && area.size.height > 0.0 {
            rects.push(RenderedRect::from_paint_rect(area, Some(fill)));
        }
    }
    rects.into_iter().map(PaintPrimitive::Rect).collect()
}

pub(in crate::layout::table) fn visible_column_span(
    start_column: usize,
    end_column: usize,
    collapsed_columns: &[bool],
) -> Option<(usize, usize)> {
    let clamped_end = end_column.min(collapsed_columns.len());
    let visible_start = (start_column..clamped_end).find(|index| !collapsed_columns[*index])?;
    let visible_end = (visible_start + 1..clamped_end)
        .rfind(|index| !collapsed_columns[*index])
        .map(|index| index + 1)
        .unwrap_or(visible_start + 1);
    Some((visible_start, visible_end))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn push_table_fragment_row_span_background(
    primitives: &mut Vec<PaintPrimitive>,
    inline_span: PageInlineSpan,
    row_tops: &[f32],
    row_heights: &[f32],
    start: usize,
    end: usize,
    fill: CssColor,
) {
    if let Some(bounds) =
        table_fragment_row_span_bounds(inline_span, row_tops, row_heights, start, end)
    {
        primitives.push(PaintPrimitive::Rect(RenderedRect::from_paint_rect(
            bounds.paint_rect(),
            Some(fill),
        )));
    }
}

pub(in crate::layout::table) fn table_fragment_row_span_bounds(
    inline_span: PageInlineSpan,
    row_tops: &[f32],
    row_heights: &[f32],
    start: usize,
    end: usize,
) -> Option<PaintClip> {
    if start >= end || end > row_tops.len() || end > row_heights.len() {
        return None;
    }
    let top = row_tops[start];
    let last = end - 1;
    let bottom = row_tops[last] - row_heights[last];
    let height = (top - bottom).max(0.0);
    (height > 0.0).then_some(
        PageTopRect::new(inline_span.left_x(), top, inline_span.width(), height).paint_clip(),
    )
}

pub(in crate::layout::table) fn table_wrapper_collision_height(
    style: &ComputedStyle,
    table_width: UsedTableWidth,
    top_caption_height: f32,
    content_height: f32,
    bottom_caption_height: f32,
) -> f32 {
    style.margin.top
        + top_caption_height
        + table_wrapper_border_box_height(content_height, table_width)
        + bottom_caption_height
        + style.margin.bottom
}

/// Return a table wrapper's physical margin-box height for float collision.
///
/// Table track sizes are logical inline/block quantities.  Floats, however,
/// are placed in the containing block's physical coordinate system, so the
/// caller must first project the grid and pass its physical wrapper border-box
/// height here.  Captions remain wrapper-level block children at this boundary:
/// <https://drafts.csswg.org/css-tables-3/#table-layout> and
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>.
pub(in crate::layout::table) fn table_wrapper_collision_height_for_border_box(
    style: &ComputedStyle,
    border_box_height: f32,
    top_caption_height: f32,
    bottom_caption_height: f32,
) -> f32 {
    style.margin.top
        + top_caption_height
        + border_box_height
        + bottom_caption_height
        + style.margin.bottom
}

/// Return the positioned containing block for a CSS table wrapper.
///
/// CSS Positioned Layout resolves absolutely positioned descendants against
/// the padding box of the nearest positioned ancestor, while CSS Tables places
/// captions in the table wrapper around the table grid. Keep the table wrapper
/// containing block as wrapper-level geometry so positioned table descendants
/// encountered while laying out captions do not fall back to a grid-only box:
/// <https://www.w3.org/TR/css-position-3/#def-cb> and
/// <https://www.w3.org/TR/CSS22/tables.html#model>.
pub(in crate::layout::table) fn table_wrapper_positioning_containing_block(
    table_x: f32,
    table_wrapper_top: f32,
    content_width: PhysicalContentWidth,
    content_height: f32,
    table_width: UsedTableWidth,
    top_caption_height: f32,
    bottom_caption_height: f32,
) -> PageTopRect {
    PageTopRect::new(
        table_x - table_width.padding.left,
        table_wrapper_top,
        content_width.points() + table_width.padding.left + table_width.padding.right,
        top_caption_height
            + table_width.border_widths.top
            + table_width.padding.top
            + content_height
            + table_width.padding.bottom
            + table_width.border_widths.bottom
            + bottom_caption_height,
    )
}

/// Return contiguous row-group spans used by table height distribution.
///
/// CSS Tables 3 distributes extra table block size to row groups before rows;
/// anonymous rows without an explicit row-group wrapper still form contiguous
/// distribution groups for the anonymous table objects created by table fixup.
/// <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>
pub(in crate::layout::table) fn table_height_distribution_groups(
    rows: &[TableRow<'_>],
) -> Vec<(usize, usize)> {
    let Some(first_row) = rows.first() else {
        return Vec::new();
    };

    let mut groups = Vec::new();
    let mut start = 0;
    let mut current_group = first_row.row_groups.last().map(|group| &group.signature);
    for (index, row) in rows.iter().enumerate().skip(1) {
        let group = row.row_groups.last().map(|group| &group.signature);
        if group != current_group {
            groups.push((start, index));
            start = index;
            current_group = group;
        }
    }
    groups.push((start, rows.len()));
    groups
}

#[derive(Clone, Copy)]
pub(in crate::layout::table) enum TableHeightTarget {
    Base,
    Reference,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current_fragmentainer(
        block_size: f32,
        content_start: f32,
        block_end: f32,
        repeat_policy: TableFragmentRepeatPolicy,
        header_height: f32,
        footer_height: f32,
        reserve_footer: bool,
    ) -> TableFragmentainer {
        TableFragmentainer::current_from_page_cursor_bounds(
            layout_pt(block_size),
            PageTopBlockPosition::new(content_start),
            PageTopBlockPosition::new(block_end),
            repeat_policy,
            layout_pt(header_height),
            layout_pt(footer_height),
            reserve_footer,
        )
    }

    #[test]
    fn row_span_background_bounds_preserve_the_explicit_physical_inline_span() {
        let bounds = table_fragment_row_span_bounds(
            PageInlineSpan::new(30.0, 90.0),
            &[200.0, 160.0],
            &[40.0, 40.0],
            0,
            2,
        )
        .expect("two visible rows have a paint bound");

        assert_eq!(
            bounds,
            PageTopRect::new(30.0, 200.0, 90.0, 80.0).paint_clip()
        );
    }

    #[test]
    fn table_cell_clip_region_keeps_disjoint_visible_rowspan_areas() {
        let region = TableCellClipRegion::from_clips(vec![
            OverflowClip::from_paint_rect(paint_space_rect(0.0, 0.0, 10.0, 4.0)),
            OverflowClip::from_paint_rect(paint_space_rect(0.0, 6.0, 10.0, 4.0)),
        ])
        .expect("visible areas");
        let viewport = TableCellClipRegion::from_clip(OverflowClip::from_paint_rect(
            paint_space_rect(2.0, 0.0, 4.0, 10.0),
        ));

        let intersection = region.intersect(&viewport).expect("shared area");
        let clips = intersection.paint_clips();
        assert_eq!(clips.len(), 2);
        assert_eq!(
            intersection.bounding_clip(),
            Some(OverflowClip::from_paint_rect(paint_space_rect(
                2.0, 0.0, 4.0, 10.0
            )))
        );
    }

    #[test]
    fn table_avoid_candidate_does_not_arm_current_row_for_break_before_avoid() {
        let state = TableAvoidBreakCandidateState::default();
        let row_breaks = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::AvoidPage,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert!(!state.row_start_may_be_rollback_target(false, false, row_breaks));
    }

    #[test]
    fn table_avoid_candidate_arms_content_row_for_break_after_avoid() {
        let state = TableAvoidBreakCandidateState::default();
        let row_breaks = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::AvoidPage,
            PageBreak::Auto,
        );

        assert!(state.row_start_may_be_rollback_target(false, false, row_breaks));
    }

    #[test]
    fn table_avoid_candidate_scopes_avoid_after_to_fragmentainer_kind() {
        let page_state = TableAvoidBreakCandidateState::new(FragmentainerKind::Page);
        let column_state = TableAvoidBreakCandidateState::new(FragmentainerKind::Column);
        let row_breaks = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::AvoidColumn,
            PageBreak::Auto,
        );

        assert!(!page_state.row_start_may_be_rollback_target(false, false, row_breaks));
        assert!(column_state.row_start_may_be_rollback_target(false, false, row_breaks));
    }

    #[test]
    fn table_repeat_policy_body_capacity_uses_fragmentainer_block_size() {
        let policy = TableFragmentRepeatPolicy {
            repeat_header: true,
            repeat_footer: true,
        };

        assert_eq!(
            policy.body_capacity(layout_pt(100.0), layout_pt(15.0), layout_pt(10.0)),
            layout_pt(75.0)
        );
        assert_eq!(
            policy.body_capacity(layout_pt(20.0), layout_pt(15.0), layout_pt(10.0)),
            layout_pt(0.0)
        );
    }

    #[test]
    fn table_chrome_context_uses_fragmentainer_block_size_for_repeat_policy() {
        let context = TableFragmentChromeContext {
            fragmentainer_block_size: layout_pt(90.0),
            header_height: layout_pt(20.0),
            footer_height: layout_pt(15.0),
            wrapper_chrome: TableWrapperFragmentChrome::none(),
            allow_header: true,
            allow_footer: true,
        };

        let policy = context.repeat_policy(layout_pt(70.0));
        assert!(policy.repeat_header);
        assert!(!policy.repeat_footer);

        let fragmentainer = context.fresh_fragmentainer(policy);
        assert_eq!(fragmentainer.fragmentainer_block_size(), layout_pt(90.0));
        assert_eq!(fragmentainer.body_capacity, layout_pt(70.0));
    }

    #[test]
    fn cloned_wrapper_chrome_reduces_fresh_body_capacity_and_keeps_a_slice_nonzero() {
        let wrapper_chrome = TableWrapperFragmentChrome {
            continuation_block_start: non_content_pt(20.0),
            continuation_block_end: non_content_pt(20.0),
        };
        let context = TableFragmentChromeContext {
            fragmentainer_block_size: layout_pt(100.0),
            header_height: layout_pt(0.0),
            footer_height: layout_pt(0.0),
            wrapper_chrome,
            allow_header: false,
            allow_footer: false,
        };
        let policy = context.repeat_policy(layout_pt(120.0));
        let fresh_fragmentainer = context.fresh_fragmentainer(policy);

        assert_eq!(fresh_fragmentainer.body_capacity, layout_pt(60.0));
        let decision = TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
            remaining_height: 120.0,
            row_required_height: 120.0,
            current_fragmentainer: fresh_fragmentainer,
            chrome_context: context,
            can_advance: false,
        });
        assert_eq!(
            decision.kind,
            TableOversizedRowSliceDecisionKind::PaintSlice
        );
        assert_eq!(decision.piece_height, 60.0);
    }

    #[test]
    fn cloned_wrapper_chrome_truncates_before_returning_zero_body_capacity() {
        let wrapper_chrome = TableWrapperFragmentChrome {
            continuation_block_start: non_content_pt(20.0),
            continuation_block_end: non_content_pt(20.0),
        };

        assert!(
            (wrapper_chrome.fresh_body_capacity(layout_pt(30.0)).points() - 0.01).abs() < 0.001
        );
    }

    #[test]
    fn table_forced_break_decision_preserves_fragmentainer_kind() {
        let decision = TableForcedBreakDecision::choose(TableForcedBreakInput {
            outgoing_repeat_policy: TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: false,
            },
            fragmentainer_kind: FragmentainerKind::Column,
            page_break: PageBreak::Column,
            row_required_height: 40.0,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(10.0),
                footer_height: layout_pt(5.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: true,
                allow_footer: true,
            },
            paint_repeated_footer: false,
        });

        assert_eq!(decision.fragmentainer_kind, FragmentainerKind::Column);
        assert_eq!(decision.page_break, PageBreak::Column);
    }

    #[test]
    fn table_named_page_break_decision_uses_chrome_context() {
        let decision = TableNamedPageBreakDecision::choose(TableNamedPageBreakInput {
            previous_page_end: Some("front".to_string()),
            row_page_start: Some("body".to_string()),
            outgoing_repeat_policy: TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: true,
            },
            row_required_height: 70.0,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(90.0),
                header_height: layout_pt(20.0),
                footer_height: layout_pt(15.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: true,
                allow_footer: true,
            },
            paint_repeated_footer: true,
        })
        .expect("named page change should commit a table fragment transition");

        assert_eq!(decision.page_name.as_deref(), Some("body"));
        assert!(decision.start.repeat_policy.repeat_header);
        assert!(!decision.start.repeat_policy.repeat_footer);
        assert!(decision.start.paint_repeated_header);
        assert_eq!(
            decision.boundary.footer_action,
            TableFragmentFooterAction::PaintRepeated
        );
    }

    #[test]
    fn table_fragment_transition_preserves_fragmentainer_kind() {
        let decision = TableFragmentTransitionDecision::from_input(TableFragmentTransitionInput {
            fragmentainer_kind: FragmentainerKind::Column,
            outgoing_repeat_policy: TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: false,
            },
            footer_action: TableFragmentFooterAction::PaintRepeated,
            break_reason: TableFragmentBreakReason::Overflow,
            incoming_repeat_policy: TableFragmentRepeatPolicy {
                repeat_header: false,
                repeat_footer: true,
            },
            paint_repeated_header: false,
        });

        assert_eq!(decision.fragmentainer_kind, FragmentainerKind::Column);
        assert_eq!(
            decision.boundary.footer_action,
            TableFragmentFooterAction::PaintRepeated
        );
        assert_eq!(
            decision.start.break_reason,
            TableFragmentBreakReason::Overflow
        );
    }

    #[test]
    fn table_fragment_plan_records_fragmentainer_kind() {
        let plan = TableFragmentPlan::new(
            FragmentainerKind::Column,
            3,
            120.0,
            TableFragmentStartDecision::new(
                TableFragmentBreakReason::Overflow,
                TableFragmentRepeatPolicy {
                    repeat_header: false,
                    repeat_footer: false,
                },
                false,
            ),
        );

        assert_eq!(plan.fragmentainer_kind, FragmentainerKind::Column);
        assert_eq!(plan.page_index, 3);
        assert_eq!(plan.break_reason(), TableFragmentBreakReason::Overflow);
    }

    #[test]
    fn table_avoid_candidate_preserves_next_boundary_across_non_content_row() {
        let state = TableAvoidBreakCandidateState::default();
        let row_breaks = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::AvoidPage,
        );

        assert!(state.row_start_may_be_rollback_target(true, false, row_breaks));
    }

    #[test]
    fn row_group_avoid_stays_when_group_fits_current_fragmentainer() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            80.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: true,
            },
            10.0,
            10.0,
            true,
        );

        assert_eq!(
            current_fragmentainer.fragmentainer_block_size(),
            layout_pt(100.0)
        );
        assert_eq!(
            current_fragmentainer.available_block_size(),
            layout_pt(80.0)
        );
        assert_eq!(current_fragmentainer.available_body_size(), layout_pt(70.0));
        assert!(
            TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
                group: TableAvoidRowGroup::new(0, 2),
                required_block_size: layout_pt(60.0),
                current_fragmentainer,
                chrome_context: TableFragmentChromeContext {
                    fragmentainer_block_size: layout_pt(100.0),
                    header_height: layout_pt(10.0),
                    footer_height: layout_pt(10.0),
                    wrapper_chrome: TableWrapperFragmentChrome::none(),
                    allow_header: true,
                    allow_footer: true,
                },
                can_advance: true,
            })
            .is_none()
        );
    }

    #[test]
    fn avoided_row_group_requirement_includes_separated_border_edges() {
        let requirement = TableRowGroupFragmentRequirement::from_row_group(
            TableAvoidRowGroup::new(0, 1),
            &[40.0],
            &[true],
            TableMetrics {
                border_collapse: css::BorderCollapse::Separate,
                spacing: css::BorderSpacing::from_lengths(0.0, 3.0),
            },
        );

        assert_eq!(requirement.block_size(), layout_pt(46.0));
    }

    #[test]
    fn avoided_row_group_requirement_excludes_collapsed_or_empty_grid_edges() {
        let collapsed = TableRowGroupFragmentRequirement::from_row_group(
            TableAvoidRowGroup::new(0, 1),
            &[40.0],
            &[true],
            TableMetrics {
                border_collapse: css::BorderCollapse::Collapse,
                spacing: css::BorderSpacing::ZERO,
            },
        );
        let empty = TableRowGroupFragmentRequirement::from_row_group(
            TableAvoidRowGroup::new(0, 1),
            &[40.0],
            &[false],
            TableMetrics {
                border_collapse: css::BorderCollapse::Separate,
                spacing: css::BorderSpacing::from_lengths(0.0, 3.0),
            },
        );

        assert_eq!(collapsed.block_size(), layout_pt(40.0));
        assert_eq!(empty.block_size(), layout_pt(0.0));
    }

    #[test]
    fn row_group_avoid_moves_to_next_fragment_with_repeated_chrome() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            40.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: true,
            },
            10.0,
            10.0,
            true,
        );
        let decision = TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
            group: TableAvoidRowGroup::new(0, 2),
            required_block_size: layout_pt(80.0),
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(10.0),
                footer_height: layout_pt(10.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: true,
                allow_footer: true,
            },
            can_advance: true,
        })
        .expect("row group should fit a fresh fragmentainer with repeats");

        assert_eq!(decision.mode, TableRowGroupAvoidMode::FitsNextFragment);
        assert!(decision.repeat_policy.repeat_header);
        assert!(decision.repeat_policy.repeat_footer);
    }

    #[test]
    fn row_group_avoid_can_suppress_chrome_for_bounded_overflow() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            40.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: true,
            },
            20.0,
            20.0,
            true,
        );
        let decision = TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
            group: TableAvoidRowGroup::new(0, 2),
            required_block_size: layout_pt(101.0),
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(20.0),
                footer_height: layout_pt(20.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: true,
                allow_footer: true,
            },
            can_advance: true,
        })
        .expect("row group should be kept by bounded chrome overflow");

        assert_eq!(decision.mode, TableRowGroupAvoidMode::KeptByChromeOverflow);
        assert!(!decision.repeat_policy.repeat_header);
        assert!(!decision.repeat_policy.repeat_footer);
    }

    #[test]
    fn row_group_avoid_stays_when_fragmentainer_cannot_advance() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            40.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: true,
            },
            10.0,
            10.0,
            true,
        );

        assert!(
            TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
                group: TableAvoidRowGroup::new(0, 2),
                required_block_size: layout_pt(80.0),
                current_fragmentainer,
                chrome_context: TableFragmentChromeContext {
                    fragmentainer_block_size: layout_pt(100.0),
                    header_height: layout_pt(10.0),
                    footer_height: layout_pt(10.0),
                    wrapper_chrome: TableWrapperFragmentChrome::none(),
                    allow_header: true,
                    allow_footer: true,
                },
                can_advance: false,
            })
            .is_none()
        );
    }

    #[test]
    fn oversized_row_slice_advances_when_empty_body_can_advance() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            0.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: false,
                repeat_footer: false,
            },
            0.0,
            0.0,
            false,
        );
        let decision = TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
            remaining_height: 120.0,
            row_required_height: 0.01,
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(0.0),
                footer_height: layout_pt(0.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: false,
                allow_footer: false,
            },
            can_advance: true,
        });

        assert_eq!(
            decision.kind,
            TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice
        );
        assert_eq!(decision.piece_height, 0.0);
    }

    #[test]
    fn oversized_row_slice_uses_body_capacity_at_fragment_start() {
        let current_fragmentainer = current_fragmentainer(
            50.0,
            120.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: false,
                repeat_footer: false,
            },
            0.0,
            0.0,
            false,
        );
        let decision = TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
            remaining_height: 120.0,
            row_required_height: 120.0,
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(50.0),
                header_height: layout_pt(0.0),
                footer_height: layout_pt(0.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: false,
                allow_footer: false,
            },
            can_advance: false,
        });

        assert_eq!(
            decision.kind,
            TableOversizedRowSliceDecisionKind::PaintSlice
        );
        assert_eq!(decision.available_body_size, 50.0);
        assert_eq!(decision.piece_height, 50.0);
    }

    #[test]
    fn oversized_row_slice_paints_when_empty_body_cannot_advance() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            0.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: false,
                repeat_footer: false,
            },
            0.0,
            0.0,
            false,
        );
        let decision = TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
            remaining_height: 120.0,
            row_required_height: 0.01,
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(0.0),
                footer_height: layout_pt(0.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: false,
                allow_footer: false,
            },
            can_advance: false,
        });

        assert_eq!(
            decision.kind,
            TableOversizedRowSliceDecisionKind::PaintSlice
        );
        assert_eq!(decision.piece_height, 120.0);
    }

    fn projection_placement(
        writing_mode: WritingMode,
        direction: Direction,
        origin: PageTopPoint,
    ) -> TableGridPlacement {
        TableGridPlacement::with_axes(
            origin,
            TableAxes {
                flow: FlowAxes::new(writing_mode, direction),
                direction,
            },
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(80.0)),
                LogicalBlockContentSize::new(content_box_pt(120.0)),
            ),
        )
    }

    #[test]
    fn source_grid_projection_keeps_logical_slices_separate_from_destinations() {
        let source_rect = TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(10.0), TableGridLength::new(30.0)),
            TableGridSize::from_lengths(TableGridLength::new(20.0), TableGridLength::new(40.0)),
        );

        for (writing_mode, direction) in [
            (WritingMode::HorizontalTb, Direction::Ltr),
            (WritingMode::VerticalLr, Direction::Rtl),
            (WritingMode::VerticalRl, Direction::Rtl),
        ] {
            let source =
                projection_placement(writing_mode, direction, PageTopPoint::new(20.0, 180.0));
            let destination =
                projection_placement(writing_mode, direction, PageTopPoint::new(220.0, 80.0));
            let source_physical = source.page_top_rect_for(source_rect);
            let destination_physical = destination.page_top_rect_for(source_rect);

            // A logical source slice has exactly one physical projection per
            // destination viewport. Its extent is invariant; only the typed
            // page placement changes.
            assert_eq!(source_physical.width(), destination_physical.width());
            assert_eq!(source_physical.height(), destination_physical.height());
            assert_eq!(destination_physical.x() - source_physical.x(), 200.0);
            assert_eq!(
                destination_physical.top_y() - source_physical.top_y(),
                -100.0
            );
        }
    }
}
