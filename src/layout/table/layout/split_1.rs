use super::*;
use std::rc::Rc;

/// Used geometry for painting a CSS table wrapper box.
///
/// CSS 2.2's separated-border model gives the table-root a wrapper border box
/// around the table grid, padding, and table border:
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableWrapperPaintBox {
    pub(in crate::layout::table) table_x: f32,
    pub(in crate::layout::table) top: f32,
    pub(in crate::layout::table) content_width: f32,
    pub(in crate::layout::table) content_height: f32,
    pub(in crate::layout::table) table_width: UsedTableWidth,
    pub(in crate::layout::table) table_metrics: TableMetrics,
}

impl TableWrapperPaintBox {
    pub(in crate::layout::table) fn border_box(self) -> PageTopRect {
        PageTopRect::new(
            self.table_x - self.table_width.padding.left - self.table_width.border_widths.left,
            self.top,
            table_wrapper_border_box_width(self.content_width, self.table_width),
            table_wrapper_border_box_height(self.content_height, self.table_width),
        )
    }

    pub(in crate::layout::table) fn padding_box(self) -> PageTopRect {
        PageTopRect::new(
            self.table_x - self.table_width.padding.left,
            self.top - self.table_width.border_widths.top,
            self.content_width + self.table_width.padding.left + self.table_width.padding.right,
            self.content_height + self.table_width.padding.top + self.table_width.padding.bottom,
        )
    }
}

/// Page-local table fragment selected before paint replay.
///
/// CSS Fragmentation splits a table wrapper into page fragments, while CSS
/// Tables keeps row, column, and collapsed-border geometry tied to the source
/// table grid. This plan is the durable bridge between those models.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#model>.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableFragmentPlan {
    pub(in crate::layout::table) page_index: usize,
    pub(in crate::layout::table) fragment_top: f32,
    pub(in crate::layout::table) repeated_header_rows: Vec<usize>,
    pub(in crate::layout::table) body_rows: Vec<TableRowPiecePlan>,
    pub(in crate::layout::table) repeated_footer_rows: Vec<usize>,
    pub(in crate::layout::table) break_reason: TableFragmentBreakReason,
    pub(in crate::layout::table) metadata: FragmentPageMetadata,
}

impl TableFragmentPlan {
    pub(in crate::layout::table) fn new(
        page_index: usize,
        fragment_top: f32,
        break_reason: TableFragmentBreakReason,
    ) -> Self {
        Self {
            page_index,
            fragment_top,
            repeated_header_rows: Vec::new(),
            body_rows: Vec::new(),
            repeated_footer_rows: Vec::new(),
            break_reason,
            metadata: FragmentPageMetadata::new(
                page_index,
                None,
                break_reason == TableFragmentBreakReason::TableStart,
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
            .extend(row.metadata.assignment_ids.iter().copied());
        self.body_rows.push(row);
    }

    pub(in crate::layout::table) fn bottom(&self) -> f32 {
        self.body_rows
            .last()
            .map(TableRowPiecePlan::bottom)
            .unwrap_or(self.fragment_top)
    }
}

/// Why a planned table page fragment starts at this location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) enum TableFragmentBreakReason {
    TableStart,
    Forced,
    AvoidedOverflow,
    Overflow,
    OversizedRowSlice,
}

/// One visible source-row slice inside a table page fragment.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableRowPiecePlan {
    pub(in crate::layout::table) row_index: usize,
    pub(in crate::layout::table) row_top: f32,
    pub(in crate::layout::table) row_height: f32,
    pub(in crate::layout::table) row_offset: f32,
    pub(in crate::layout::table) original_row_height: f32,
    pub(in crate::layout::table) collapsed: bool,
    pub(in crate::layout::table) artificial_split: bool,
    pub(in crate::layout::table) metadata: FragmentPageMetadata,
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
    pub(in crate::layout::table) content_offset: f32,
    pub(in crate::layout::table) content_x_offset: f32,
    pub(in crate::layout::table) content_clip: Option<OverflowClip>,
    pub(in crate::layout::table) area: TableGridArea,
    pub(in crate::layout::table) content: TableCellContentPlan,
}

impl TableCellFragmentPlan {
    pub(in crate::layout::table) fn x(&self) -> f32 {
        self.border_box.x(self.placement)
    }

    pub(in crate::layout::table) fn top_y(&self) -> f32 {
        self.border_box.top_y(self.placement)
    }

    pub(in crate::layout::table) fn width(&self) -> f32 {
        self.border_box.width()
    }

    pub(in crate::layout::table) fn height(&self) -> f32 {
        self.border_box.height()
    }

    pub(in crate::layout::table) fn content_box(
        &self,
        cell_style: &ComputedStyle,
        cell_borders: css::Edges,
    ) -> TableCellContentBox {
        self.border_box.content_box(
            self.placement,
            cell_style.padding,
            cell_borders,
            self.content_offset,
            self.content_x_offset,
        )
    }
}

/// Planned table-cell content for one page-local row piece.
///
/// CSS table-cell contents are laid out in a block container, but CSS
/// Fragmentation clips and paints only the content visible in each table row
/// piece. This plan records those page-local content decisions before paint:
/// <https://www.w3.org/TR/CSS22/tables.html#model> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableCellContentPlan {
    pub(in crate::layout::table) inline_sequence: Option<inline_layout::InlineLineSequence>,
    pub(in crate::layout::table) child_fragments: Vec<TableCellChildFragmentPlan>,
    pub(in crate::layout::table) children_painted_by_inline_sequence: bool,
}

impl TableCellContentPlan {
    pub(in crate::layout::table) fn empty() -> Self {
        Self {
            inline_sequence: None,
            child_fragments: Vec::new(),
            children_painted_by_inline_sequence: false,
        }
    }
}

/// One planned in-flow table-cell child slice for a split row piece.
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableCellChildFragmentPlan {
    pub(in crate::layout::table) source_child_index: usize,
    pub(in crate::layout::table) child_top: f32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::table) struct TableFragmentRepeatPolicy {
    pub(in crate::layout::table) repeat_header: bool,
    pub(in crate::layout::table) repeat_footer: bool,
}

pub(in crate::layout::table) const TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE: f32 = 2.0;

impl TableFragmentRepeatPolicy {
    pub(in crate::layout::table) fn header_rows<'a>(&self, rows: &'a [usize]) -> &'a [usize] {
        if self.repeat_header { rows } else { &[] }
    }

    pub(in crate::layout::table) fn footer_rows<'a>(&self, rows: &'a [usize]) -> &'a [usize] {
        if self.repeat_footer { rows } else { &[] }
    }

    pub(in crate::layout::table) fn reserved_footer_height(&self, footer_height: f32) -> f32 {
        if self.repeat_footer {
            footer_height
        } else {
            0.0
        }
    }

    pub(in crate::layout::table) fn body_capacity(
        &self,
        page_area_height: f32,
        header_height: f32,
        footer_height: f32,
    ) -> f32 {
        let repeated_height = if self.repeat_header {
            header_height
        } else {
            0.0
        } + if self.repeat_footer {
            footer_height
        } else {
            0.0
        };
        (page_area_height - repeated_height).max(0.0)
    }
}

/// Choose optional repeated table rows for a page fragment with required body space.
///
/// CSS 2.2 permits print user agents to repeat table header and footer groups
/// on each page, but CSS Fragmentation still requires progress and treats
/// `break-inside: avoid` as a constraint to honor when possible. Prefer
/// preserving both repeated groups, then the header, then the footer, and
/// finally suppress optional repeats before creating a fragment with no usable
/// body area.
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>
/// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>
/// <https://www.w3.org/TR/css-break-3/#break-within>
pub(in crate::layout::table) fn table_fragment_repeat_policy(
    required_body_height: f32,
    page_area_height: f32,
    header_height: f32,
    footer_height: f32,
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

    let required_body_height = required_body_height.max(0.0);
    for policy in candidates {
        let body_capacity = policy.body_capacity(page_area_height, header_height, footer_height);
        if body_capacity > 0.01 && required_body_height <= body_capacity + 0.01 {
            return policy;
        }
    }

    candidates
        .into_iter()
        .find(|policy| policy.body_capacity(page_area_height, header_height, footer_height) > 0.01)
        .unwrap_or(TableFragmentRepeatPolicy {
            repeat_header: false,
            repeat_footer: false,
        })
}

/// Page-local body-row paint capture for one fragmented table piece.
///
/// CSS Fragmentation splits the table wrapper into page fragments while CSS
/// 2.2 Appendix E still requires the rows, borders, and positioned descendants
/// in each fragment to paint as one ordered table unit.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
/// <https://www.w3.org/TR/CSS22/zindex.html>
#[derive(Clone)]
pub(in crate::layout::table) struct TableBodyPaintFragment {
    pub(in crate::layout::table) checkpoint: PaintCheckpoint,
    pub(in crate::layout::table) positioned_layer_start: usize,
    pub(in crate::layout::table) plan: TableFragmentPlan,
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
        self.inline_bounds.size
    }
}

pub(in crate::layout::table) struct TableCellContentScope {
    pub(in crate::layout::table) content_left: f32,
    pub(in crate::layout::table) content_right: f32,
    pub(in crate::layout::table) cursor_y: f32,
    pub(in crate::layout::table) ancestors: Vec<ElementSignature>,
    pub(in crate::layout::table) containing_block_direction: Direction,
    pub(in crate::layout::table) containing_block_writing_mode: WritingMode,
    pub(in crate::layout::table) content_logical_inline_size_stack: Vec<f32>,
    pub(in crate::layout::table) child_available_space_stack: Vec<ChildAvailableSpace>,
    pub(in crate::layout::table) definite_block_size_stack: Vec<Option<f32>>,
}

pub(in crate::layout::table) struct TableGridLayoutContext<'table, 'ctx> {
    pub(in crate::layout::table) rows: &'ctx [TableRow<'table>],
    pub(in crate::layout::table) grid: &'ctx TableGrid,
    pub(in crate::layout::table) table_style: &'ctx ComputedStyle,
    pub(in crate::layout::table) stylesheets: &'ctx [Stylesheet],
    pub(in crate::layout::table) table_cellpadding: Option<f32>,
    pub(in crate::layout::table) column_plan: &'ctx TableColumnPlan,
    pub(in crate::layout::table) table_metrics: TableMetrics,
    pub(in crate::layout::table) collapsed_geometry: Option<&'ctx CollapsedTableGeometry>,
}

pub(in crate::layout::table) struct TableCellBaselineAlignmentContext<'a> {
    pub(in crate::layout::table) row_index: usize,
    pub(in crate::layout::table) row_style: &'a ComputedStyle,
    pub(in crate::layout::table) table_style: &'a ComputedStyle,
    pub(in crate::layout::table) rows: &'a [TableRow<'a>],
    pub(in crate::layout::table) grid: &'a TableGrid,
    pub(in crate::layout::table) stylesheets: &'a [Stylesheet],
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
pub(in crate::layout::table) struct TableHeightPlan {
    pub(in crate::layout::table) rows: Vec<TableRowHeightPlan>,
}

/// Per-row state used by `TableHeightPlan`.
///
/// `base` is the ROWMIN-style first-pass size, `reference` includes
/// explicit/percentage row, row-group, and cell constraints, and `final_height`
/// is the size after the CSS Tables 3 distribution algorithm.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRowHeightPlan {
    pub(in crate::layout::table) base: f32,
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
    constrain_height(
        style,
        used_length_percentage_or_auto(style.box_values.height, content_height)
            .unwrap_or(0.0)
            .max(content_height),
        content_height,
    ) + style.padding.top
        + style.padding.bottom
        + border_insets.top
        + border_insets.bottom
}

impl TableBodyPaintFragment {
    pub(in crate::layout::table) fn new(
        checkpoint: PaintCheckpoint,
        page_index: usize,
        positioned_layer_start: usize,
        fragment_top: f32,
        break_reason: TableFragmentBreakReason,
    ) -> Self {
        Self {
            checkpoint,
            positioned_layer_start,
            plan: TableFragmentPlan::new(page_index, fragment_top, break_reason),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn push_row(
        &mut self,
        row_index: usize,
        row_top: f32,
        row_height: f32,
        row_offset: f32,
        original_row_height: f32,
        collapsed: bool,
        source_fragment: TableRowSourceFragment,
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
            artificial_split: row_offset > 0.0 || row_height + 0.01 < original_row_height,
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

    pub(in crate::layout::table) fn repeated_rows(&self) -> Vec<usize> {
        self.plan
            .repeated_header_rows
            .iter()
            .chain(self.plan.repeated_footer_rows.iter())
            .copied()
            .collect()
    }

    pub(in crate::layout::table) fn starts_after_break(&self) -> bool {
        self.plan.break_reason != TableFragmentBreakReason::TableStart
    }

    pub(in crate::layout::table) fn has_split_or_collapsed_rows(&self) -> bool {
        self.plan
            .body_rows
            .iter()
            .any(|row| row.collapsed || row.artificial_split)
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

pub(in crate::layout::table) fn table_wrapper_border_box_width(
    content_width: f32,
    table_width: UsedTableWidth,
) -> f32 {
    content_width
        + table_width.padding.left
        + table_width.padding.right
        + table_width.border_widths.left
        + table_width.border_widths.right
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

/// Return the CSS overflow clip for a table box, excluding wrapper captions.
///
/// CSS 2.1 errata makes `overflow` apply to the table box instead of the
/// table wrapper box, and defines `scroll`/`auto` as visible on table boxes.
/// The clipping edge therefore uses the table padding box around the grid, not
/// the wrapper area that contains captions:
/// <https://www.w3.org/Style/css2-updates/REC-CSS2-20110607-errata.html#s.11.1.1b>.
pub(in crate::layout::table) fn table_box_overflow_clip(
    style: &ComputedStyle,
    padding_box: PaintClip,
    table_is_document_canvas: bool,
) -> Option<PaintClip> {
    if table_is_document_canvas {
        return None;
    }
    matches!(style.overflow, css::Overflow::Hidden | css::Overflow::Clip).then_some(padding_box)
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
    policy.effects.overflow_clip = overflow_clip;
    policy
}

pub(in crate::layout::table) fn table_horizontal_non_content_width(
    style: &ComputedStyle,
    table_width: UsedTableWidth,
) -> f32 {
    table_width.horizontal_non_content(style).points()
}

pub(in crate::layout::table) fn table_content_width_clamped_to_min_content(
    style: &ComputedStyle,
    content_width: f32,
    min_content: f32,
) -> f32 {
    if style.table_layout == TableLayout::Auto {
        content_width.max(min_content)
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
    table_column_background_primitives_with_clip(
        paint_rect,
        style,
        BackgroundRectArea {
            x: paint_rect.origin.x,
            y: paint_rect.origin.y,
            width: paint_rect.size.width,
            height: paint_rect.size.height,
        },
    )
}

#[allow(clippy::too_many_arguments)]
/// Paint a column or column-group background through row-fragment clips.
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
    start_column: usize,
    end_column: usize,
    style: &ComputedStyle,
    row_tops: &[f32],
    row_heights: &[f32],
) -> Vec<PaintPrimitive> {
    if matches!(
        style.writing_mode,
        WritingMode::VerticalRl | WritingMode::VerticalLr
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
    let mut primitives = Vec::new();
    let full_clip = BackgroundRectArea {
        x: paint_rect.origin.x,
        y: paint_rect.origin.y,
        width: paint_rect.size.width,
        height: paint_rect.size.height,
    };
    for (row_top, row_height) in row_tops.iter().copied().zip(row_heights.iter().copied()) {
        if row_height <= 0.0 {
            continue;
        }
        let row_clip = full_clip.intersection(BackgroundRectArea {
            x: paint_rect.origin.x,
            y: row_top - row_height,
            width: paint_rect.size.width,
            height: row_height,
        });
        primitives.extend(table_column_background_primitives_with_clip(
            paint_rect, style, row_clip,
        ));
    }
    primitives
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
        WritingMode::VerticalRl | WritingMode::VerticalLr
    ) {
        used_length_percentage_or_auto(style.box_values.height, grid_height)
            .unwrap_or(grid_height)
            .max(grid_height)
    } else {
        grid_height
    };
    let rect = TableGridRect::new(
        TableGridPoint::new(inline_bounds.start, 0.0),
        TableGridSize::new(inline_bounds.size, block_size),
    );
    let placement = TableGridPlacement::new(PageTopPoint::new(table_x, grid_top));
    let paint_rect = placement.overflow_clip_for(rect).paint_rect();
    Some((paint_rect, inline_bounds))
}

fn table_column_background_primitives_with_clip(
    paint_rect: PaintRect,
    style: &ComputedStyle,
    clip: BackgroundRectArea,
) -> Vec<PaintPrimitive> {
    let mut rects = Vec::new();
    let rounded_rects: Vec<RenderedRoundedRect> = Vec::new();
    let mut paths = Vec::new();
    let strokes = Vec::new();
    let x = paint_rect.origin.x;
    let y = paint_rect.origin.y;
    let width = paint_rect.size.width;
    let height = paint_rect.size.height;
    if width <= 0.0 || height <= 0.0 || clip.width <= 0.0 || clip.height <= 0.0 {
        return Vec::new();
    }
    if let Some(fill) = style.background_color
        && fill.is_visible()
    {
        let area = background_rect_clip_area_for_box(
            x,
            y,
            width,
            height,
            style,
            css::Edges::ZERO,
            style.background_clip,
            Some(clip),
        );
        if area.width > 0.0 && area.height > 0.0 {
            rects.push(RenderedRect::from_paint_rect(
                paint_space_rect(area.x, area.y, area.width, area.height),
                Some(fill),
            ));
        }
    }
    if style.border_radius.is_zero() {
        rects.extend(linear_gradient_rects_with_clip(
            x,
            y,
            width,
            height,
            style,
            css::Edges::ZERO,
            Some(clip),
        ));
    } else {
        paths.extend(linear_gradient_rect_paths_with_clip(
            x,
            y,
            width,
            height,
            style,
            css::Edges::ZERO,
            Some(clip),
        ));
    }
    paths.extend(linear_gradient_paths_with_clip(
        x,
        y,
        width,
        height,
        style,
        css::Edges::ZERO,
        Some(clip),
    ));
    let mut primitives = Vec::new();
    primitives.extend(rects.into_iter().map(PaintPrimitive::Rect));
    primitives.extend(rounded_rects.into_iter().map(PaintPrimitive::RoundedRect));
    primitives.extend(paths.into_iter().map(PaintPrimitive::Path));
    primitives.extend(strokes.into_iter().map(PaintPrimitive::Stroke));
    primitives
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
    table_x: f32,
    used_table_width: f32,
    row_tops: &[f32],
    row_heights: &[f32],
    start: usize,
    end: usize,
    fill: Color,
) {
    if let Some(bounds) =
        table_fragment_row_span_bounds(table_x, used_table_width, row_tops, row_heights, start, end)
    {
        primitives.push(PaintPrimitive::Rect(RenderedRect::from_paint_rect(
            bounds.paint_rect(),
            Some(fill),
        )));
    }
}

pub(in crate::layout::table) fn table_fragment_row_span_bounds(
    table_x: f32,
    used_table_width: f32,
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
    (height > 0.0).then_some(PageTopRect::new(table_x, top, used_table_width, height).paint_clip())
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
    content_width: f32,
    content_height: f32,
    table_width: UsedTableWidth,
    top_caption_height: f32,
    bottom_caption_height: f32,
) -> PageTopRect {
    PageTopRect::new(
        table_x - table_width.padding.left,
        table_wrapper_top,
        content_width + table_width.padding.left + table_width.padding.right,
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
