use super::*;

/// Used geometry for painting a CSS table wrapper box.
///
/// CSS 2.2's separated-border model gives the table-root a wrapper border box
/// around the table grid, padding, and table border:
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>.
#[derive(Debug, Clone, Copy)]
struct TableWrapperPaintBox {
    table_x: f32,
    top: f32,
    content_width: f32,
    content_height: f32,
    table_width: UsedTableWidth,
    table_metrics: TableMetrics,
}

/// Page-local table fragment selected before paint replay.
///
/// CSS Fragmentation splits a table wrapper into page fragments, while CSS
/// Tables keeps row, column, and collapsed-border geometry tied to the source
/// table grid. This plan is the durable bridge between those models.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
/// <https://www.w3.org/TR/CSS22/tables.html#model>.
#[derive(Debug, Clone)]
struct TableFragmentPlan {
    page_index: usize,
    fragment_top: f32,
    repeated_header_rows: Vec<usize>,
    body_rows: Vec<TableRowPiecePlan>,
    repeated_footer_rows: Vec<usize>,
    break_reason: TableFragmentBreakReason,
    metadata: FragmentPageMetadata,
}

impl TableFragmentPlan {
    fn new(page_index: usize, fragment_top: f32, break_reason: TableFragmentBreakReason) -> Self {
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

    fn push_body_row(&mut self, row: TableRowPiecePlan) {
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

    fn bottom(&self) -> f32 {
        self.body_rows
            .last()
            .map(TableRowPiecePlan::bottom)
            .unwrap_or(self.fragment_top)
    }
}

/// Why a planned table page fragment starts at this location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableFragmentBreakReason {
    TableStart,
    Forced,
    AvoidedOverflow,
    Overflow,
    OversizedRowSlice,
}

/// One visible source-row slice inside a table page fragment.
#[derive(Debug, Clone)]
struct TableRowPiecePlan {
    row_index: usize,
    row_top: f32,
    row_height: f32,
    row_offset: f32,
    original_row_height: f32,
    collapsed: bool,
    artificial_split: bool,
    metadata: FragmentPageMetadata,
}

impl TableRowPiecePlan {
    fn bottom(&self) -> f32 {
        self.row_top - self.row_height
    }
}

/// Cell-level geometry consumed while painting a planned row piece.
#[derive(Debug, Clone)]
struct TableCellFragmentPlan {
    border_box: TableCellBorderBox,
    placement: TableGridPlacement,
    content_offset: f32,
    content_x_offset: f32,
    content_clip: Option<OverflowClip>,
    area: TableGridArea,
    content: TableCellContentPlan,
}

impl TableCellFragmentPlan {
    fn x(&self) -> f32 {
        self.border_box.x(self.placement)
    }

    fn top_y(&self) -> f32 {
        self.border_box.top_y(self.placement)
    }

    fn width(&self) -> f32 {
        self.border_box.width()
    }

    fn height(&self) -> f32 {
        self.border_box.height()
    }

    fn content_box(
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
struct TableCellContentPlan {
    inline_sequence: Option<inline_layout::InlineLineSequence>,
    child_fragments: Vec<TableCellChildFragmentPlan>,
    children_painted_by_inline_sequence: bool,
}

impl TableCellContentPlan {
    fn empty() -> Self {
        Self {
            inline_sequence: None,
            child_fragments: Vec::new(),
            children_painted_by_inline_sequence: false,
        }
    }
}

/// One planned in-flow table-cell child slice for a split row piece.
#[derive(Debug, Clone)]
struct TableCellChildFragmentPlan {
    source_child_index: usize,
    child_top: f32,
    child_height: f32,
    slice_top: f32,
    slice_bottom: f32,
    kind: TableCellChildFragmentKind,
    inline_sequence: Option<TableCellNestedInlineSequencePlan>,
    nested_fragment: Option<TableCellNestedFragmentPlan>,
    metadata: FragmentPageMetadata,
}

/// Sequence-backed inline content for a nested table-cell slice.
///
/// CSS Text line selection and CSS Fragmentation slicing should consume the
/// same graph-selected line records even when inline content is nested under
/// table-cell split-row replay:
/// <https://www.w3.org/TR/css-text-3/#line-breaking> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone)]
struct TableCellNestedInlineSequencePlan {
    sequence: inline_layout::InlineLineSequence,
    style: ComputedStyle,
}

/// Pre-rendered table-cell nested formatting context for split row replay.
#[derive(Debug, Clone)]
struct TableCellNestedFragmentPlan {
    fragment: PaintFragment,
    width: f32,
    height: f32,
    metadata: FragmentPageMetadata,
}

/// Coarse child kind used to route planned table-cell fragment painting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableCellChildFragmentKind {
    Block,
    AnonymousBlock,
    Inline,
    Text,
    AtomicInline,
    Replaced,
    NestedFormattingContext,
}

#[derive(Clone)]
struct TableBreakCandidate {
    snapshot: LayoutSnapshot,
    row_index: usize,
    table_body_fragment: Option<TableBodyPaintFragment>,
    repeat_policy: TableFragmentRepeatPolicy,
    height: f32,
}

impl TableBreakCandidate {
    fn with_height(&self, height: f32) -> Self {
        let mut candidate = self.clone();
        candidate.height = height;
        candidate
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TableFragmentRepeatPolicy {
    repeat_header: bool,
    repeat_footer: bool,
}

const TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE: f32 = 2.0;

impl TableFragmentRepeatPolicy {
    fn header_rows<'a>(&self, rows: &'a [usize]) -> &'a [usize] {
        if self.repeat_header { rows } else { &[] }
    }

    fn footer_rows<'a>(&self, rows: &'a [usize]) -> &'a [usize] {
        if self.repeat_footer { rows } else { &[] }
    }

    fn reserved_footer_height(&self, footer_height: f32) -> f32 {
        if self.repeat_footer {
            footer_height
        } else {
            0.0
        }
    }

    fn body_capacity(&self, page_area_height: f32, header_height: f32, footer_height: f32) -> f32 {
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
fn table_fragment_repeat_policy(
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
struct TableBodyPaintFragment {
    checkpoint: PaintCheckpoint,
    positioned_layer_start: usize,
    plan: TableFragmentPlan,
}

#[derive(Debug, Clone, Copy)]
struct TableCellLayoutMetrics {
    content_height: f32,
    border_box_height: f32,
    baseline_offset: f32,
}

struct PreparedTableCell {
    style: ComputedStyle,
    area: TableGridArea,
    inline_bounds: TableInlineBounds,
    borders: css::Edges,
    metrics: TableCellLayoutMetrics,
    text: String,
}

impl PreparedTableCell {
    fn width(&self) -> f32 {
        self.inline_bounds.size
    }
}

struct TableGridLayoutContext<'table, 'ctx> {
    rows: &'ctx [TableRow<'table>],
    grid: &'ctx TableGrid,
    table_style: &'ctx ComputedStyle,
    stylesheets: &'ctx [Stylesheet],
    table_cellpadding: Option<f32>,
    column_plan: &'ctx TableColumnPlan,
    table_metrics: TableMetrics,
    collapsed_geometry: Option<&'ctx CollapsedTableGeometry>,
}

struct TableCellBaselineAlignmentContext<'a> {
    row_index: usize,
    row_style: &'a ComputedStyle,
    table_style: &'a ComputedStyle,
    rows: &'a [TableRow<'a>],
    grid: &'a TableGrid,
    stylesheets: &'a [Stylesheet],
    table_cellpadding: Option<f32>,
    column_plan: &'a TableColumnPlan,
    planned_row_heights: &'a [f32],
    table_metrics: TableMetrics,
    collapsed_geometry: Option<&'a CollapsedTableGeometry>,
    row_baseline_offset: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableCellBaselineSet {
    First,
    Last,
}

/// CSS Tables 3 row-height plan for first-pass minimums, reference sizes, and
/// final distributed row sizes.
///
/// Spec: <https://drafts.csswg.org/css-tables-3/#row-layout> and
/// <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>.
#[derive(Debug, Clone)]
struct TableHeightPlan {
    rows: Vec<TableRowHeightPlan>,
}

/// Per-row state used by `TableHeightPlan`.
///
/// `base` is the ROWMIN-style first-pass size, `reference` includes
/// explicit/percentage row, row-group, and cell constraints, and `final_height`
/// is the size after the CSS Tables 3 distribution algorithm.
#[derive(Debug, Clone, Copy)]
struct TableRowHeightPlan {
    base: f32,
    reference: f32,
    final_height: f32,
    auto: bool,
    collapsed: bool,
}

/// Shared CSS 2.2 collapsed-border geometry for one laid-out table.
///
/// The full resolved grid is the source of truth for table wrapper insets,
/// structural background bounds, and fragmented border painting.
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
pub(super) struct CollapsedTableGeometry {
    grid: CollapsedBorderGrid,
    outer_insets: css::Edges,
}

impl CollapsedTableGeometry {
    fn cell_insets(&self, placement: &TableCellPlacement, row_index: usize) -> css::Edges {
        self.grid.cell_insets(
            row_index,
            placement.column,
            placement.colspan,
            placement.rowspan,
        )
    }
}

fn table_cell_border_insets(
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

fn table_cell_border_box_height_with_insets(
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
    fn new(
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

    fn push_row(
        &mut self,
        row_index: usize,
        row_top: f32,
        row_height: f32,
        row_offset: f32,
        original_row_height: f32,
    ) {
        let mut metadata = FragmentPageMetadata::new(
            self.plan.page_index,
            Some(PageTopRect::new(0.0, row_top, 0.0, row_height).paint_clip()),
            self.plan.break_reason != TableFragmentBreakReason::Overflow
                && self.plan.body_rows.is_empty(),
        );
        metadata.continues_from_previous_page = row_offset > 0.0;
        metadata.continues_to_next_page = row_offset + row_height + 0.01 < original_row_height;
        self.plan.push_body_row(TableRowPiecePlan {
            row_index,
            row_top,
            row_height,
            row_offset,
            original_row_height,
            collapsed: row_height <= 0.01 && original_row_height <= 0.01,
            artificial_split: row_offset > 0.0 || row_height + 0.01 < original_row_height,
            metadata,
        });
    }

    fn bottom(&self) -> f32 {
        self.plan.bottom()
    }

    fn mark_repeated_headers(&mut self, rows: &[usize]) {
        self.plan.repeated_header_rows.clear();
        self.plan.repeated_header_rows.extend_from_slice(rows);
    }

    fn mark_repeated_footers(&mut self, rows: &[usize]) {
        self.plan.repeated_footer_rows.clear();
        self.plan.repeated_footer_rows.extend_from_slice(rows);
    }

    fn repeated_rows(&self) -> Vec<usize> {
        self.plan
            .repeated_header_rows
            .iter()
            .chain(self.plan.repeated_footer_rows.iter())
            .copied()
            .collect()
    }

    fn starts_after_break(&self) -> bool {
        self.plan.break_reason != TableFragmentBreakReason::TableStart
    }

    fn has_split_or_collapsed_rows(&self) -> bool {
        self.plan
            .body_rows
            .iter()
            .any(|row| row.collapsed || row.artificial_split)
    }

    fn rows(&self) -> Vec<usize> {
        self.plan
            .body_rows
            .iter()
            .map(|row| row.row_index)
            .collect()
    }

    fn row_tops(&self) -> Vec<f32> {
        self.plan.body_rows.iter().map(|row| row.row_top).collect()
    }

    fn row_heights(&self) -> Vec<f32> {
        self.plan
            .body_rows
            .iter()
            .map(|row| row.row_height)
            .collect()
    }

    fn row_offsets(&self) -> Vec<f32> {
        self.plan
            .body_rows
            .iter()
            .map(|row| row.row_offset)
            .collect()
    }

    fn original_row_heights(&self) -> Vec<f32> {
        self.plan
            .body_rows
            .iter()
            .map(|row| row.original_row_height)
            .collect()
    }
}

fn table_wrapper_border_box_width(content_width: f32, table_width: UsedTableWidth) -> f32 {
    content_width
        + table_width.padding.left
        + table_width.padding.right
        + table_width.border_widths.left
        + table_width.border_widths.right
}

fn table_wrapper_border_box_height(content_height: f32, table_width: UsedTableWidth) -> f32 {
    content_height
        + table_width.padding.top
        + table_width.padding.bottom
        + table_width.border_widths.top
        + table_width.border_widths.bottom
}

fn table_horizontal_non_content_width(style: &ComputedStyle, table_width: UsedTableWidth) -> f32 {
    let border_width = if style.border_collapse == css::BorderCollapse::Collapse {
        0.0
    } else {
        table_width.border_widths.left + table_width.border_widths.right
    };
    border_width + table_width.padding.left + table_width.padding.right
}

fn table_content_width_clamped_to_min_content(
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

fn table_displayed_horizontal_spacing(visible_columns: usize, table_metrics: TableMetrics) -> f32 {
    if visible_columns == 0 {
        0.0
    } else {
        table_metrics.spacing.horizontal * (visible_columns + 1) as f32
    }
}

/// Return separated-border gutters inside a logical column span.
///
/// CSS 2.2 places horizontal `border-spacing` between adjacent column cells.
/// A cell spanning multiple visible columns includes those internal gutters in
/// its border box, so column width constraints derived from that cell must
/// remove them before distributing the remaining width to tracks:
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>.
fn table_internal_horizontal_spacing(
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
    table_metrics.spacing.horizontal * visible_columns.saturating_sub(1) as f32
}

fn table_column_background_rect(
    table_x: f32,
    grid_top: f32,
    grid_height: f32,
    column_plan: &TableColumnPlan,
    start_column: usize,
    end_column: usize,
    fill: Option<Color>,
) -> Option<RenderedRect> {
    let fill = fill?;
    if start_column >= end_column || start_column >= column_plan.column_count() {
        return None;
    }
    let clamped_end = end_column.min(column_plan.column_count());
    let inline_bounds =
        column_plan.inline_bounds_for_span(start_column, clamped_end - start_column);
    let rect = TableGridRect::new(
        TableGridPoint::new(inline_bounds.start, 0.0),
        TableGridSize::new(inline_bounds.size, grid_height),
    );
    let placement = TableGridPlacement::new(PageTopPoint::new(table_x, grid_top));
    let clip = placement.overflow_clip_for(rect);
    Some(RenderedRect::from_paint_rect(clip.paint_rect(), Some(fill)))
}

fn visible_column_span(
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
fn push_table_fragment_row_span_background(
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

fn table_fragment_row_span_bounds(
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

fn table_wrapper_collision_height(
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

/// Return contiguous row-group spans used by table height distribution.
///
/// CSS Tables 3 distributes extra table block size to row groups before rows;
/// anonymous rows without an explicit row-group wrapper still form contiguous
/// distribution groups for the anonymous table objects created by table fixup.
/// <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>
fn table_height_distribution_groups(rows: &[TableRow<'_>]) -> Vec<(usize, usize)> {
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
enum TableHeightTarget {
    Base,
    Reference,
}

fn table_plan_height(row: &TableRowHeightPlan, target: TableHeightTarget) -> f32 {
    match target {
        TableHeightTarget::Base => row.base,
        TableHeightTarget::Reference => row.reference,
    }
}

fn table_plan_height_mut(row: &mut TableRowHeightPlan, target: TableHeightTarget) -> &mut f32 {
    match target {
        TableHeightTarget::Base => &mut row.base,
        TableHeightTarget::Reference => &mut row.reference,
    }
}

fn table_content_height_from_plan(
    rows: &[TableRowHeightPlan],
    target: TableHeightTarget,
    table_metrics: TableMetrics,
) -> f32 {
    let heights = rows
        .iter()
        .map(|row| table_plan_height(row, target))
        .collect::<Vec<_>>();
    table_content_height(&heights, table_metrics)
}

fn table_span_height_from_plan(
    rows: &[TableRowHeightPlan],
    row: usize,
    rowspan: usize,
    target: TableHeightTarget,
    table_metrics: TableMetrics,
) -> f32 {
    let heights = rows
        .iter()
        .map(|row| table_plan_height(row, target))
        .collect::<Vec<_>>();
    table_row_span_height(&heights, row, rowspan, table_metrics)
}

fn distribute_table_span_constraint(
    rows: &mut [TableRowHeightPlan],
    row: usize,
    rowspan: usize,
    required_height: f32,
    table_metrics: TableMetrics,
    target: TableHeightTarget,
) {
    if row >= rows.len() {
        return;
    }
    let current_height = table_span_height_from_plan(rows, row, rowspan, target, table_metrics);
    let extra = required_height - current_height;
    if extra <= 0.01 {
        return;
    }

    let end = (row + rowspan.max(1)).min(rows.len());
    let auto_receivers = (row..end)
        .filter(|index| !rows[*index].collapsed && rows[*index].auto)
        .collect::<Vec<_>>();
    let receivers = if auto_receivers.is_empty() {
        (row..end)
            .filter(|index| !rows[*index].collapsed)
            .collect::<Vec<_>>()
    } else {
        auto_receivers
    };
    if receivers.is_empty() {
        return;
    }

    let share = extra / receivers.len() as f32;
    for index in receivers {
        *table_plan_height_mut(&mut rows[index], target) += share;
    }
}

fn distribute_table_height_extra(
    rows: &mut [TableRowHeightPlan],
    extra: f32,
    predicate: impl Fn(&TableRowHeightPlan) -> bool,
) -> f32 {
    if extra <= 0.01 {
        return 0.0;
    }
    let receivers = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| predicate(row).then_some(index))
        .collect::<Vec<_>>();
    if receivers.is_empty() {
        return 0.0;
    }

    let share = extra / receivers.len() as f32;
    for index in &receivers {
        rows[*index].final_height += share;
    }
    extra
}

fn apply_table_column_style_measures(
    measures: &mut TableColumnMeasures,
    column: usize,
    colspan: usize,
    style: &ComputedStyle,
) {
    let end = (column + colspan).min(measures.min_content_widths.len());
    if column >= end {
        return;
    }
    let declared_width = declared_table_column_width(style);
    let width_floor = declared_width
        .map(declared_table_width_length_floor)
        .unwrap_or(0.0);
    let min_width = constrain_table_intrinsic_width_with_floor(style, 0.0, width_floor);
    let max_width = constrain_table_intrinsic_width_with_floor(style, min_width, width_floor);
    let percentage = intrinsic_percentage_contribution(style).max(
        declared_width
            .map(declared_table_width_percentage)
            .unwrap_or(0.0),
    );
    for index in column..end {
        measures.min_content_widths[index] = measures.min_content_widths[index].max(min_width);
        measures.max_content_widths[index] = measures.max_content_widths[index].max(max_width);
        measures.intrinsic_percentages[index] =
            measures.intrinsic_percentages[index].max(percentage);
        if declared_width.is_some_and(declared_table_width_is_non_percentage) {
            measures.constrained[index] = true;
        }
    }
}

fn distribute_spanned_percentage(
    measures: &mut TableColumnMeasures,
    start: usize,
    end: usize,
    percentage: f32,
) {
    if percentage <= 0.0 || start >= end {
        return;
    }
    let current = measures.intrinsic_percentages[start..end]
        .iter()
        .sum::<f32>();
    if percentage <= current {
        return;
    }
    let extra = percentage - current;
    let receivers = (start..end)
        .filter(|index| measures.intrinsic_percentages[*index] == 0.0)
        .collect::<Vec<_>>();
    let receivers = if receivers.is_empty() {
        (start..end).collect::<Vec<_>>()
    } else {
        receivers
    };
    let max_content_sum = receivers
        .iter()
        .map(|index| measures.max_content_widths[*index].max(0.0))
        .sum::<f32>();
    let receiver_count = receivers.len().max(1) as f32;
    for index in receivers {
        let ratio = if max_content_sum > 0.0 {
            measures.max_content_widths[index].max(0.0) / max_content_sum
        } else {
            1.0 / receiver_count
        };
        measures.intrinsic_percentages[index] += extra * ratio;
    }
}

fn distribute_spanned_measure(
    measures: &mut TableColumnMeasures,
    start: usize,
    end: usize,
    target_width: f32,
    min_content: bool,
) {
    if target_width <= 0.0 || start >= end {
        return;
    }
    let current = if min_content {
        measures.min_content_widths[start..end].iter().sum::<f32>()
    } else {
        measures.max_content_widths[start..end].iter().sum::<f32>()
    };
    if target_width <= current {
        return;
    }
    let snapshot = measures.clone();
    let widths = if min_content {
        &mut measures.min_content_widths
    } else {
        &mut measures.max_content_widths
    };
    distribute_table_excess_width(&snapshot, widths, target_width - current, start..end);
}

fn cap_intrinsic_percentages(percentages: &mut [f32]) {
    let mut used = 0.0_f32;
    for percentage in percentages {
        let remaining = (1.0 - used).max(0.0);
        *percentage = percentage.max(0.0).min(remaining);
        used += *percentage;
    }
}

/// Resolve final auto-layout column widths from precomputed table measures.
///
/// CSS Tables 3 chooses among min-content, percentage, specified, and
/// max-content guesses, interpolates when the assignable width falls between
/// guesses, and distributes remaining width after max-content:
/// <https://drafts.csswg.org/css-tables-3/#width-distribution-algorithm>.
fn auto_table_column_widths(measures: &TableColumnMeasures, assignable_width: f32) -> Vec<f32> {
    let min_content_guess = measures.min_content_widths.clone();
    let mut min_content_percentage_guess = measures.min_content_widths.clone();
    let mut min_content_specified_guess = measures.min_content_widths.clone();
    let mut max_content_guess = measures.max_content_widths.clone();

    for index in 0..measures.min_content_widths.len() {
        if measures.intrinsic_percentages[index] > 0.0 {
            let percentage_width = (measures.intrinsic_percentages[index] * assignable_width)
                .max(measures.min_content_widths[index]);
            min_content_percentage_guess[index] = percentage_width;
            min_content_specified_guess[index] = percentage_width;
            max_content_guess[index] = percentage_width;
        } else if measures.constrained[index] {
            min_content_specified_guess[index] = measures.max_content_widths[index];
        }
    }

    if assignable_width < max_content_guess.iter().sum::<f32>() {
        let guesses = [
            min_content_guess.as_slice(),
            min_content_percentage_guess.as_slice(),
            min_content_specified_guess.as_slice(),
            max_content_guess.as_slice(),
        ];
        let mut lower_guess = guesses[0];
        let mut upper_guess = guesses[guesses.len() - 1];
        for guess in guesses {
            if guess.iter().sum::<f32>() <= assignable_width * (1.0 + 1e-6) {
                lower_guess = guess;
            } else {
                upper_guess = guess;
                break;
            }
        }
        let lower_sum = lower_guess.iter().sum::<f32>();
        let upper_sum = upper_guess.iter().sum::<f32>();
        if (upper_sum - lower_sum).abs() <= 0.01 {
            return upper_guess.to_vec();
        }
        let ratio = ((assignable_width - lower_sum) / (upper_sum - lower_sum)).clamp(0.0, 1.0);
        return lower_guess
            .iter()
            .zip(upper_guess)
            .map(|(lower, upper)| lower + (upper - lower) * ratio)
            .collect();
    }

    let mut widths = max_content_guess;
    let excess_width = assignable_width - widths.iter().sum::<f32>();
    let width_count = widths.len();
    distribute_table_excess_width(measures, &mut widths, excess_width, 0..width_count);
    widths
}

impl<'a> LayoutBuilder<'a> {
    /// Return min-content and max-content grid widths for a durable table fragment.
    ///
    /// CSS Tables computes intrinsic table widths from the row/column grid and
    /// cell min/max-content measures. Reusing the durable fragment keeps
    /// inline-table and positioned sizing aligned with the table object
    /// construction used for normal layout:
    /// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>.
    pub(in crate::layout) fn table_intrinsic_widths_from_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        if rows.is_empty() {
            let width = used_empty_table_grid_width(style, available_table_width, table_width);
            return (width, width);
        }

        let grid = table_grid(rows);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        let measures = self.table_column_measures(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            table_width.content_width,
            table_cellpadding,
            table_metrics,
        );
        let min_content = measures.table_min_content_width().max(0.0);
        let max_content = measures.table_max_content_width().max(min_content);
        (min_content, max_content)
    }

    /// Return parent-facing content-box intrinsic widths for a table fragment.
    ///
    /// CSS Tables computes grid min/max-content widths from column measures,
    /// but CSS Sizing intrinsic contributions also honor a non-auto preferred
    /// size. For auto-layout tables, the used table content box is clamped so
    /// it is not smaller than the grid min-content width:
    /// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width> and
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
    pub(in crate::layout) fn table_parent_intrinsic_content_widths_from_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        let (min_content, max_content) = self.table_intrinsic_widths_from_fragment(
            element,
            style,
            stylesheets,
            fragment,
            available_outer_width,
        );
        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        let horizontal_non_content = table_horizontal_non_content_width(style, table_width);
        let resolved_width =
            used_content_width_or_auto(style, available_table_width, horizontal_non_content)
                .or_else(|| {
                    intrinsic::intrinsic_width_keyword(
                        style.box_values.width,
                        min_content,
                        max_content,
                        available_table_width,
                        horizontal_non_content,
                    )
                })
                .map(|width| {
                    constrain_width(style, width, available_table_width).max(style.font_size)
                });

        if let Some(width) = resolved_width {
            let width = table_content_width_clamped_to_min_content(style, width, min_content);
            (width, width)
        } else {
            (min_content, max_content)
        }
    }

    /// Return parent-facing margin-box intrinsic widths for a table fragment.
    ///
    /// Table parents consume the table wrapper/margin box, while table layout
    /// itself consumes the grid/content width. Keep this conversion separate so
    /// grid sizing remains available for column layout:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
    pub(in crate::layout) fn table_outer_intrinsic_widths_from_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        let (min_content, max_content) = self.table_parent_intrinsic_content_widths_from_fragment(
            element,
            style,
            stylesheets,
            fragment,
            available_outer_width,
        );
        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        let horizontal_extras = table_horizontal_non_content_width(style, table_width)
            + style.margin.left
            + style.margin.right;
        (
            min_content + horizontal_extras,
            max_content + horizontal_extras,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn inline_table_atom_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        fragment: &box_tree::TableFragment<'_>,
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> Option<InlineAtom> {
        // CSS Display 3 maps `inline-table` to an inline-level atomic box whose
        // contents establish a table formatting context.
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        if rows.is_empty() {
            return None;
        }
        let grid = table_grid(rows);
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let mut table_width = used_table_width(style, available_width);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        self.resolve_table_used_content_width(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            available_width,
            table_cellpadding,
            table_metrics,
            &mut table_width,
        );
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            table_width.content_width,
            !style.box_values.width.is_auto(),
            table_cellpadding,
            table_metrics,
        );
        let content_width = column_plan
            .total_width()
            .min(available_width)
            .max(style.font_size);
        let collapsed_geometry = (table_metrics.border_collapse == css::BorderCollapse::Collapse)
            .then(|| {
                self.collapsed_table_geometry(
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    &input.columns,
                    column_plan.column_count(),
                )
            });
        let top = 10_000.0;
        let table_context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics,
            collapsed_geometry: collapsed_geometry.as_ref(),
        };
        let planned_row_heights = self.measure_table_row_heights(&table_context);
        let top_caption_height = self.estimate_table_captions_height(
            &input.captions,
            style,
            stylesheets,
            content_width,
            CaptionSide::Top,
        );
        let first_row_baseline_range = inline_table_first_occupying_row_range(
            top,
            top_caption_height,
            table_width.border_widths,
            table_width.padding,
            &planned_row_heights,
            table_metrics,
        );
        let table_strut_baseline_offset =
            self.font_system.rendered_first_line_baseline_offset(style);

        let snapshot = self.snapshot();
        let mut table_style = style.clone();
        table_style.margin = css::Edges::ZERO;
        set_style_used_width(&mut table_style, content_width);
        table_style.break_before = PageBreak::Auto;
        table_style.break_after = PageBreak::Auto;

        self.current_page = Page::new(content_width, top);
        self.content_left = 0.0;
        self.content_right = content_width;
        self.cursor_y = top;
        self.truncate_page_start_margins = false;
        let _ = children;
        self.layout_table(element, &table_style, stylesheets, fragment);
        let content_height = (top - self.cursor_y).max(style.line_height);
        let fragment_bottom = top - content_height;
        // CSS 2.2 defines an `inline-table` baseline as the baseline of the
        // first row. In the current paint model, align against the first
        // rendered row line's actual shaped-font alignment coordinate.
        // https://www.w3.org/TR/CSS22/tables.html#table-display
        let baseline_offset = first_row_baseline_range
            .and_then(|(row_top, row_bottom)| {
                self.inline_table_baseline_offset_from_fragment(
                    row_top,
                    row_bottom,
                    fragment_bottom,
                    content_height,
                    table_strut_baseline_offset,
                )
            })
            .unwrap_or(content_height);
        let fragment = self
            .current_page
            .paint_fragment()
            .translated(PaintVector::new(0.0, -fragment_bottom));
        self.restore(snapshot);

        let mut atom_style = style.clone();
        atom_style.background_color = None;
        atom_style.border_width = 0.0;
        atom_style.border_widths = css::Edges::ZERO;
        atom_style.border_styles = css::BorderStyles::NONE;
        atom_style.padding = css::Edges::ZERO;

        Some(InlineAtom {
            content: InlineAtomContent::InlineFragment(fragment),
            style: atom_style,
            escaped_positioned_layers: None,
            width: content_width + style.margin.left + style.margin.right,
            height: content_height,
            baseline_offset,
            baseline_shift,
            link_target,
            alt_text: None,
        })
    }

    /// Return the inline-table first-row baseline offset from the fragment top edge.
    ///
    /// CSS 2.2 defines an `inline-table` baseline as the baseline of the first
    /// table row. `reasyprint` stores text using PDF baseline-adjusted glyph
    /// coordinates, so this maps the rendered line inside the first occupying
    /// row to the atom baseline offset from the table wrapper top edge used by
    /// the line builder.
    /// https://www.w3.org/TR/CSS22/tables.html#table-display
    fn inline_table_baseline_offset_from_fragment(
        &self,
        row_top: f32,
        row_bottom: f32,
        fragment_bottom: f32,
        content_height: f32,
        table_strut_baseline_offset: f32,
    ) -> Option<f32> {
        self.current_page
            .lines
            .iter()
            .map(|line| self.font_system.rendered_line_alignment_y(line))
            .find(|alignment_y| *alignment_y <= row_top + 0.01 && *alignment_y >= row_bottom - 0.01)
            .map(|alignment_y| {
                (content_height - (alignment_y - fragment_bottom) + table_strut_baseline_offset)
                    .clamp(0.0, content_height + table_strut_baseline_offset)
            })
    }

    pub(crate) fn estimate_table_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        fragment: &box_tree::TableFragment<'_>,
    ) -> f32 {
        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let captions = input.captions.as_slice();
        let columns = input.columns.as_slice();

        let available_table_width =
            (available_outer_width - style.margin.left - style.margin.right).max(style.font_size);
        let table_width = used_table_width(style, available_table_width);
        if rows.is_empty() {
            return self.estimate_empty_table_height(
                captions,
                style,
                stylesheets,
                available_table_width,
                table_width,
            );
        }
        let grid = table_grid(rows);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            columns,
            table_width.content_width,
            !style.box_values.width.is_auto(),
            table_cellpadding,
            table_metrics,
        );
        let collapsed_geometry = (table_metrics.border_collapse == css::BorderCollapse::Collapse)
            .then(|| {
                self.collapsed_table_geometry(
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    columns,
                    column_plan.column_count(),
                )
            });

        let mut total = style.margin.top;
        total += self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            table_width.content_width,
            CaptionSide::Top,
        );
        let table_context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics,
            collapsed_geometry: collapsed_geometry.as_ref(),
        };
        let row_heights = self.measure_table_row_heights(&table_context);
        total += table_vertical_edge_spacing(&row_heights, table_metrics);
        for (row_index, row_height) in row_heights.iter().copied().enumerate() {
            total += row_height;
            let row_style = self.style_for_table_row(&rows[row_index], style, stylesheets);
            if !self.table_row_is_hidden_empty(
                &rows[row_index],
                &grid.rows[row_index],
                &row_style,
                stylesheets,
                table_cellpadding,
                &column_plan,
                table_metrics,
            ) && row_index + 1 < rows.len()
                && has_following_uncollapsed_row(&rows[row_index + 1..], style, stylesheets, self)
            {
                total += table_metrics.spacing.vertical;
            }
        }
        total += table_vertical_edge_spacing(&row_heights, table_metrics);
        total += self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            table_width.content_width,
            CaptionSide::Bottom,
        );
        total + style.margin.bottom
    }

    pub(crate) fn layout_table(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        fragment: &box_tree::TableFragment<'_>,
    ) {
        self.apply_forced_break(style.break_before);

        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let relative_offset = relative_position_offset(style, self.current_containing_block());
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y += relative_offset.y;
        }
        let captions = input.captions.as_slice();
        let columns = input.columns.as_slice();

        let available_table_width =
            self.content_right - self.content_left - style.margin.left - style.margin.right;
        let mut table_width = used_table_width(style, available_table_width);
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        if rows.is_empty() {
            self.layout_empty_table(
                captions,
                style,
                stylesheets,
                available_table_width,
                table_width,
                table_metrics,
                relative_offset,
            );
            return;
        }
        let grid = table_grid(rows);
        self.resolve_table_used_content_width(
            rows,
            &grid,
            style,
            stylesheets,
            columns,
            available_table_width,
            table_cellpadding,
            table_metrics,
            &mut table_width,
        );
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            columns,
            table_width.content_width,
            !style.box_values.width.is_auto(),
            table_cellpadding,
            table_metrics,
        );
        let collapsed_geometry = (table_metrics.border_collapse == css::BorderCollapse::Collapse)
            .then(|| {
                self.collapsed_table_geometry(
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    columns,
                    column_plan.column_count(),
                )
            });
        if let Some(geometry) = &collapsed_geometry {
            table_width.border_widths = geometry.outer_insets;
        }
        let used_table_width = column_plan.total_width();
        let repeating_header_rows = table_repeating_header_row_indices(rows);
        let repeating_footer_rows = table_repeating_footer_row_indices(rows);

        let table_context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics,
            collapsed_geometry: collapsed_geometry.as_ref(),
        };
        let planned_row_heights = self.measure_table_row_heights(&table_context);
        let repeating_header_height =
            repeated_table_rows_height(&repeating_header_rows, &planned_row_heights, table_metrics);
        let repeating_footer_height =
            repeated_table_rows_height(&repeating_footer_rows, &planned_row_heights, table_metrics);
        let row_group_spans = table_row_group_spans(rows);
        let avoid_break_row_groups = row_group_spans
            .iter()
            .filter_map(|(start, end, row_group)| {
                let row_group_style = self.style_for_table_row_group(row_group, style, stylesheets);
                row_group_style
                    .break_inside_avoid
                    .then_some((*start, *end, row_group_style))
            })
            .collect::<Vec<_>>();
        let mut row_group_break_before = vec![PageBreak::Auto; rows.len()];
        let mut row_group_break_after = vec![PageBreak::Auto; rows.len()];
        for (start, end, row_group) in &row_group_spans {
            let row_group_style = self.style_for_table_row_group(row_group, style, stylesheets);
            if row_group_style.break_before != PageBreak::Auto {
                row_group_break_before[*start] = row_group_style.break_before;
            }
            if row_group_style.break_after != PageBreak::Auto && end > start {
                row_group_break_after[end - 1] = row_group_style.break_after;
            }
        }
        let top_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            used_table_width,
            CaptionSide::Top,
        );
        let bottom_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            used_table_width,
            CaptionSide::Bottom,
        );
        let table_content_height = table_content_height(&planned_row_heights, table_metrics);
        let table_border_box_width = table_wrapper_border_box_width(used_table_width, table_width);
        let table_collision_height = table_wrapper_collision_height(
            style,
            table_width,
            top_caption_height,
            table_content_height,
            bottom_caption_height,
        );
        self.cursor_y -= style.margin.top;
        self.prebreak_bfc_margin_box_if_needed(table_collision_height, style.margin.top);
        let (margin_box_left, avoided_top, _) = self.place_float_avoiding_margin_box(
            self.cursor_y,
            style.margin.left + table_border_box_width + style.margin.right,
            table_collision_height,
            style.clear,
            style.writing_mode,
            style.direction,
            self.containing_block_direction,
        );
        self.cursor_y = avoided_top;
        let table_outer_x = margin_box_left + style.margin.left + relative_offset.x;
        // CSS table wrappers paint borders/padding around the table grid; the
        // grid itself starts at the content-box inline-start edge.
        let table_x = table_width.content_x(table_outer_x);

        self.push_float_context();
        let table_wrapper_top = self.cursor_y;
        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            self.containing_blocks
                .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                    table_x,
                    table_wrapper_top,
                    used_table_width,
                    top_caption_height
                        + table_width.border_widths.top
                        + table_width.padding.top
                        + table_content_height
                        + table_width.padding.bottom
                        + table_width.border_widths.bottom
                        + bottom_caption_height,
                )));
        }
        self.layout_table_captions(
            captions,
            style,
            stylesheets,
            table_x,
            used_table_width,
            CaptionSide::Top,
        );
        let table_box_top = self.cursor_y;
        self.cursor_y -= table_width.border_widths.top + table_width.padding.top;
        let table_edge_spacing = table_vertical_edge_spacing(&planned_row_heights, table_metrics);
        self.cursor_y -= table_edge_spacing;

        let table_structure_paint_checkpoint = self.current_page.paint_checkpoint();
        let table_structure_paint_page_index = self.pages.len();
        self.paint_separated_table_wrapper_border(
            style,
            TableWrapperPaintBox {
                table_x,
                top: table_box_top,
                content_width: used_table_width,
                content_height: table_content_height,
                table_width,
                table_metrics,
            },
        );
        if self.pages.len() == table_structure_paint_page_index {
            let table_border_box_height =
                table_wrapper_border_box_height(table_content_height, table_width);
            self.scope_current_page_atomic_paint_since(
                &table_structure_paint_checkpoint,
                PaintBand::InFlowBlock,
                PageTopRect::new(
                    table_x - table_width.padding.left - table_width.border_widths.left,
                    table_box_top,
                    table_border_box_width,
                    table_border_box_height,
                )
                .paint_clip(),
                style,
                Vec::new(),
            );
        }

        // Table row-grid fragments split independently of the source row's
        // previous block position. Recording the grid's fragment offset lets
        // `push_page` continue table rows at the page-start position of the
        // surrounding formatting context, instead of at the consumed position
        // of the row that triggered the break.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        self.fragment_top_offsets
            .push(self.current_page_context.top() - self.cursor_y);
        let mut table_body_fragment: Option<TableBodyPaintFragment> = None;
        let mut pending_table_fragment_break_reason = TableFragmentBreakReason::TableStart;
        let mut pending_repeated_header_rows = Vec::new();
        let mut current_fragment_repeat_policy = table_fragment_repeat_policy(
            0.01,
            self.page_area_height(),
            0.0,
            repeating_footer_height,
            false,
            true,
        );
        let mut page_break_before_next_row = PageBreak::Auto;
        let mut forced_break_after_table_rows = PageBreak::Auto;
        let mut avoid_break_candidate: Option<TableBreakCandidate> = None;
        let mut previous_row_candidate: Option<TableBreakCandidate> = None;
        let mut previous_break_after_avoid = false;
        let mut unfragmented_avoid_row_group_end: Option<usize> = None;
        let mut row_index = 0usize;
        while row_index < rows.len() {
            let row = &rows[row_index];
            let row_style = self.style_for_table_row(row, style, stylesheets);
            let row_is_repeating_header = repeating_header_rows.contains(&row_index);
            let row_is_repeating_footer = repeating_footer_rows.contains(&row_index);
            let row_height = planned_row_heights[row_index];
            let row_collapsed = table_row_is_collapsed(&row_style);
            let row_fragment_required_height = if row_height > self.page_area_height() + 0.01 {
                0.01
            } else {
                row_height
            };
            let mut broke_before_row = page_break_before_next_row.is_forced();
            let pending_avoid_before_row = page_break_before_next_row.avoids_page();
            let row_start_candidate = TableBreakCandidate {
                snapshot: self.snapshot(),
                row_index,
                table_body_fragment: table_body_fragment.clone(),
                repeat_policy: current_fragment_repeat_policy,
                height: 0.0,
            };
            if page_break_before_next_row.is_forced() {
                if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                    self.mark_table_body_fragment_repeated_footers(
                        &mut table_body_fragment,
                        current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                        &planned_row_heights,
                        table_metrics,
                    );
                }
                self.finalize_table_body_paint_fragment(
                    &mut table_body_fragment,
                    rows,
                    &grid,
                    columns,
                    style,
                    stylesheets,
                    table_x,
                    used_table_width,
                    table_cellpadding,
                    &column_plan,
                    table_width,
                    table_metrics,
                    collapsed_geometry.as_ref(),
                );
                if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                    self.layout_repeated_table_footer_rows_at_page_bottom(
                        rows,
                        &grid,
                        columns,
                        current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        &column_plan,
                        &planned_row_heights,
                        table_width,
                        table_metrics,
                        collapsed_geometry.as_ref(),
                    );
                }
                self.apply_forced_break(page_break_before_next_row);
                current_fragment_repeat_policy = table_fragment_repeat_policy(
                    row_fragment_required_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                pending_table_fragment_break_reason = TableFragmentBreakReason::Forced;
            }
            page_break_before_next_row = PageBreak::Auto;
            // CSS Fragmentation applies forced break-before/break-after values
            // to table row and row-group boxes as ordinary fragmentation boxes.
            // https://www.w3.org/TR/css-break-3/#break-between
            let break_before = if row_group_break_before[row_index] != PageBreak::Auto {
                row_group_break_before[row_index]
            } else {
                row_style.break_before
            };
            if break_before.is_forced() {
                if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                    self.mark_table_body_fragment_repeated_footers(
                        &mut table_body_fragment,
                        current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                        &planned_row_heights,
                        table_metrics,
                    );
                }
                self.finalize_table_body_paint_fragment(
                    &mut table_body_fragment,
                    rows,
                    &grid,
                    columns,
                    style,
                    stylesheets,
                    table_x,
                    used_table_width,
                    table_cellpadding,
                    &column_plan,
                    table_width,
                    table_metrics,
                    collapsed_geometry.as_ref(),
                );
                if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                    self.layout_repeated_table_footer_rows_at_page_bottom(
                        rows,
                        &grid,
                        columns,
                        current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        &column_plan,
                        &planned_row_heights,
                        table_width,
                        table_metrics,
                        collapsed_geometry.as_ref(),
                    );
                }
                self.apply_forced_break(break_before);
                current_fragment_repeat_policy = table_fragment_repeat_policy(
                    row_fragment_required_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                pending_table_fragment_break_reason = TableFragmentBreakReason::Forced;
                broke_before_row = true;
            }
            if let Some((_, end, _)) = avoid_break_row_groups
                .iter()
                .find(|(start, _, _)| *start == row_index)
            {
                let group_height = table_row_span_height(
                    &planned_row_heights,
                    row_index,
                    end - row_index,
                    table_metrics,
                );
                let remaining_height = self.cursor_y - self.page_bottom();
                // CSS Fragmentation 3 treats `break-inside: avoid` as a
                // preference to keep a fragmentation container together when
                // possible. For table row groups, use the measured group height
                // so rows are moved as a unit when the group fits on a fresh
                // page but not in the current remaining fragmentainer.
                // https://www.w3.org/TR/css-break-3/#break-within
                let mut next_fragment_repeat_policy = table_fragment_repeat_policy(
                    group_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                let next_fragment_body_capacity = next_fragment_repeat_policy.body_capacity(
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                );
                let group_fits_next_fragment = group_height <= next_fragment_body_capacity + 0.01;
                let no_repeat_policy = TableFragmentRepeatPolicy {
                    repeat_header: false,
                    repeat_footer: false,
                };
                let no_repeat_body_capacity = no_repeat_policy.body_capacity(
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                );
                let group_can_overflow_next_fragment = !group_fits_next_fragment
                    && group_height
                        <= no_repeat_body_capacity + TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE;
                if group_can_overflow_next_fragment {
                    next_fragment_repeat_policy = no_repeat_policy;
                }
                if (group_fits_next_fragment || group_can_overflow_next_fragment)
                    && group_height > remaining_height + 0.01
                    && !self.cursor_is_at_page_top()
                {
                    if !row_is_repeating_footer {
                        self.mark_table_body_fragment_repeated_footers(
                            &mut table_body_fragment,
                            current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                            &planned_row_heights,
                            table_metrics,
                        );
                    }
                    self.finalize_table_body_paint_fragment(
                        &mut table_body_fragment,
                        rows,
                        &grid,
                        columns,
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        &column_plan,
                        table_width,
                        table_metrics,
                        collapsed_geometry.as_ref(),
                    );
                    if !row_is_repeating_footer {
                        self.layout_repeated_table_footer_rows_at_page_bottom(
                            rows,
                            &grid,
                            columns,
                            current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            &column_plan,
                            &planned_row_heights,
                            table_width,
                            table_metrics,
                            collapsed_geometry.as_ref(),
                        );
                    }
                    self.push_page();
                    self.cursor_y = self.page_top();
                    current_fragment_repeat_policy = next_fragment_repeat_policy;
                    if group_can_overflow_next_fragment {
                        unfragmented_avoid_row_group_end = Some(*end);
                    }
                    pending_table_fragment_break_reason = TableFragmentBreakReason::AvoidedOverflow;
                    broke_before_row = true;
                }
            }
            let reserved_footer_height = if row_is_repeating_footer {
                0.0
            } else {
                current_fragment_repeat_policy.reserved_footer_height(repeating_footer_height)
            };
            let avoid_boundary = pending_avoid_before_row
                || previous_break_after_avoid
                || break_before.avoids_page();
            let avoid_candidate = if pending_avoid_before_row || previous_break_after_avoid {
                avoid_break_candidate.clone()
            } else if break_before.avoids_page() {
                previous_row_candidate.clone()
            } else {
                None
            };
            if avoid_boundary
                && let Some(candidate) = avoid_candidate
                && !self.cursor_is_at_page_top()
                && row_height > self.cursor_y - self.page_bottom() + 0.01
            {
                let avoid_run_height = candidate.height + row_height;
                let next_fragment_repeat_policy = table_fragment_repeat_policy(
                    avoid_run_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                if avoid_run_height
                    > next_fragment_repeat_policy.body_capacity(
                        self.page_area_height(),
                        repeating_header_height,
                        repeating_footer_height,
                    ) + 0.01
                {
                    // The run cannot be kept together on the next fragment;
                    // fall through to the ordinary row break rules.
                } else {
                    self.restore(candidate.snapshot);
                    table_body_fragment = candidate.table_body_fragment;
                    current_fragment_repeat_policy = candidate.repeat_policy;
                    if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                        self.mark_table_body_fragment_repeated_footers(
                            &mut table_body_fragment,
                            current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                            &planned_row_heights,
                            table_metrics,
                        );
                    }
                    self.finalize_table_body_paint_fragment(
                        &mut table_body_fragment,
                        rows,
                        &grid,
                        columns,
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        &column_plan,
                        table_width,
                        table_metrics,
                        collapsed_geometry.as_ref(),
                    );
                    if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                        self.layout_repeated_table_footer_rows_at_page_bottom(
                            rows,
                            &grid,
                            columns,
                            current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            &column_plan,
                            &planned_row_heights,
                            table_width,
                            table_metrics,
                            collapsed_geometry.as_ref(),
                        );
                    }
                    self.push_page();
                    self.cursor_y = self.page_top();
                    current_fragment_repeat_policy = next_fragment_repeat_policy;
                    pending_table_fragment_break_reason = TableFragmentBreakReason::AvoidedOverflow;
                    if !row_is_repeating_header {
                        self.layout_repeated_table_rows(
                            rows,
                            &grid,
                            columns,
                            current_fragment_repeat_policy.header_rows(&repeating_header_rows),
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            &column_plan,
                            &planned_row_heights,
                            table_width,
                            table_metrics,
                            collapsed_geometry.as_ref(),
                        );
                        pending_repeated_header_rows = current_fragment_repeat_policy
                            .header_rows(&repeating_header_rows)
                            .to_vec();
                    }
                    row_index = candidate.row_index;
                    page_break_before_next_row = PageBreak::Auto;
                    avoid_break_candidate = None;
                    previous_row_candidate = None;
                    previous_break_after_avoid = false;
                    continue;
                }
            }
            let row_requires_split = row_height > self.page_area_height() + 0.01;
            let row_kept_by_avoid_group =
                unfragmented_avoid_row_group_end.is_some_and(|end| row_index < end);
            let available_height = self.cursor_y - self.page_bottom();
            let row_overflows_page = if row_requires_split {
                !row_kept_by_avoid_group && available_height <= 0.01
            } else {
                self.cursor_y - row_height < self.page_bottom()
            };
            let row_overflows_reserved_footer = if row_requires_split {
                !row_kept_by_avoid_group && available_height - reserved_footer_height <= 0.01
            } else {
                self.cursor_y - row_height - reserved_footer_height < self.page_bottom()
            };
            if (row_overflows_page || row_overflows_reserved_footer)
                && !self.cursor_is_at_page_top()
                && self.out_of_flow_prebreak_suppression_depth == 0
            {
                if !row_is_repeating_footer {
                    self.mark_table_body_fragment_repeated_footers(
                        &mut table_body_fragment,
                        current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                        &planned_row_heights,
                        table_metrics,
                    );
                }
                self.finalize_table_body_paint_fragment(
                    &mut table_body_fragment,
                    rows,
                    &grid,
                    columns,
                    style,
                    stylesheets,
                    table_x,
                    used_table_width,
                    table_cellpadding,
                    &column_plan,
                    table_width,
                    table_metrics,
                    collapsed_geometry.as_ref(),
                );
                if !row_is_repeating_footer {
                    self.layout_repeated_table_footer_rows_at_page_bottom(
                        rows,
                        &grid,
                        columns,
                        current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        &column_plan,
                        &planned_row_heights,
                        table_width,
                        table_metrics,
                        collapsed_geometry.as_ref(),
                    );
                }
                self.push_page();
                self.cursor_y = self.page_top();
                current_fragment_repeat_policy = table_fragment_repeat_policy(
                    row_fragment_required_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                pending_table_fragment_break_reason = TableFragmentBreakReason::Overflow;
                broke_before_row = true;
            }
            if broke_before_row && !row_is_repeating_header {
                self.layout_repeated_table_rows(
                    rows,
                    &grid,
                    columns,
                    current_fragment_repeat_policy.header_rows(&repeating_header_rows),
                    style,
                    stylesheets,
                    table_x,
                    used_table_width,
                    table_cellpadding,
                    &column_plan,
                    &planned_row_heights,
                    table_width,
                    table_metrics,
                    collapsed_geometry.as_ref(),
                );
                pending_repeated_header_rows = current_fragment_repeat_policy
                    .header_rows(&repeating_header_rows)
                    .to_vec();
                let reserved_footer_height_after_header = if row_is_repeating_footer {
                    0.0
                } else {
                    current_fragment_repeat_policy.reserved_footer_height(repeating_footer_height)
                };
                let available_height_after_header = self.cursor_y - self.page_bottom();
                let row_still_overflows_after_header = if row_requires_split {
                    !row_kept_by_avoid_group
                        && available_height_after_header - reserved_footer_height_after_header
                            <= 0.01
                } else {
                    self.cursor_y - row_height - reserved_footer_height_after_header
                        < self.page_bottom()
                };
                if row_still_overflows_after_header && !self.cursor_is_at_page_top() {
                    if !row_is_repeating_footer {
                        self.mark_table_body_fragment_repeated_footers(
                            &mut table_body_fragment,
                            current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                            &planned_row_heights,
                            table_metrics,
                        );
                    }
                    self.finalize_table_body_paint_fragment(
                        &mut table_body_fragment,
                        rows,
                        &grid,
                        columns,
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        &column_plan,
                        table_width,
                        table_metrics,
                        collapsed_geometry.as_ref(),
                    );
                    if !row_is_repeating_footer {
                        self.layout_repeated_table_footer_rows_at_page_bottom(
                            rows,
                            &grid,
                            columns,
                            current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            &column_plan,
                            &planned_row_heights,
                            table_width,
                            table_metrics,
                            collapsed_geometry.as_ref(),
                        );
                    }
                    self.push_page();
                    self.cursor_y = self.page_top();
                    current_fragment_repeat_policy = table_fragment_repeat_policy(
                        row_fragment_required_height,
                        self.page_area_height(),
                        repeating_header_height,
                        repeating_footer_height,
                        false,
                        !row_is_repeating_footer,
                    );
                    pending_table_fragment_break_reason = TableFragmentBreakReason::Overflow;
                    self.layout_repeated_table_rows(
                        rows,
                        &grid,
                        columns,
                        current_fragment_repeat_policy.header_rows(&repeating_header_rows),
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        &column_plan,
                        &planned_row_heights,
                        table_width,
                        table_metrics,
                        collapsed_geometry.as_ref(),
                    );
                    pending_repeated_header_rows = current_fragment_repeat_policy
                        .header_rows(&repeating_header_rows)
                        .to_vec();
                }
            }

            let row_top = self.cursor_y;
            if self.ensure_table_body_paint_fragment(
                &mut table_body_fragment,
                row_top,
                pending_table_fragment_break_reason,
                &pending_repeated_header_rows,
            ) {
                pending_table_fragment_break_reason = TableFragmentBreakReason::TableStart;
                pending_repeated_header_rows.clear();
            }
            if row_collapsed {
                if let Some(fragment) = &mut table_body_fragment {
                    fragment.push_row(row_index, row_top, 0.0, 0.0, 0.0);
                }
                previous_row_candidate = Some(row_start_candidate.with_height(0.0));
                avoid_break_candidate = None;
                previous_break_after_avoid = false;
                row_index += 1;
                continue;
            }
            let row_baseline_offset = self.table_row_baseline_offset(
                row_index,
                row,
                &grid.rows[row_index],
                &row_style,
                stylesheets,
                table_cellpadding,
                &column_plan,
                table_metrics,
                collapsed_geometry.as_ref(),
            );
            if row_height > self.page_area_height() + 0.01 && !row_kept_by_avoid_group {
                let mut remaining = row_height;
                let mut piece_offset = 0.0;
                while remaining > 0.01 {
                    let reserved_footer_height = if row_is_repeating_footer {
                        0.0
                    } else {
                        current_fragment_repeat_policy
                            .reserved_footer_height(repeating_footer_height)
                    };
                    let available_height =
                        (self.cursor_y - self.page_bottom() - reserved_footer_height).max(0.0);
                    if available_height <= 0.01 {
                        if !row_is_repeating_footer {
                            self.mark_table_body_fragment_repeated_footers(
                                &mut table_body_fragment,
                                current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                                &planned_row_heights,
                                table_metrics,
                            );
                        }
                        self.finalize_table_body_paint_fragment(
                            &mut table_body_fragment,
                            rows,
                            &grid,
                            columns,
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            &column_plan,
                            table_width,
                            table_metrics,
                            collapsed_geometry.as_ref(),
                        );
                        if !row_is_repeating_footer {
                            self.layout_repeated_table_footer_rows_at_page_bottom(
                                rows,
                                &grid,
                                columns,
                                current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                                style,
                                stylesheets,
                                table_x,
                                used_table_width,
                                table_cellpadding,
                                &column_plan,
                                &planned_row_heights,
                                table_width,
                                table_metrics,
                                collapsed_geometry.as_ref(),
                            );
                        }
                        self.push_page();
                        self.cursor_y = self.page_top();
                        current_fragment_repeat_policy = table_fragment_repeat_policy(
                            row_fragment_required_height,
                            self.page_area_height(),
                            repeating_header_height,
                            repeating_footer_height,
                            !row_is_repeating_header,
                            !row_is_repeating_footer,
                        );
                        pending_table_fragment_break_reason =
                            TableFragmentBreakReason::OversizedRowSlice;
                        if !row_is_repeating_header {
                            self.layout_repeated_table_rows(
                                rows,
                                &grid,
                                columns,
                                current_fragment_repeat_policy.header_rows(&repeating_header_rows),
                                style,
                                stylesheets,
                                table_x,
                                used_table_width,
                                table_cellpadding,
                                &column_plan,
                                &planned_row_heights,
                                table_width,
                                table_metrics,
                                collapsed_geometry.as_ref(),
                            );
                            pending_repeated_header_rows = current_fragment_repeat_policy
                                .header_rows(&repeating_header_rows)
                                .to_vec();
                        }
                        if self.ensure_table_body_paint_fragment(
                            &mut table_body_fragment,
                            self.cursor_y,
                            pending_table_fragment_break_reason,
                            &pending_repeated_header_rows,
                        ) {
                            pending_table_fragment_break_reason =
                                TableFragmentBreakReason::TableStart;
                            pending_repeated_header_rows.clear();
                        }
                        continue;
                    }

                    let piece_height = remaining.min(available_height);
                    let piece_top = self.cursor_y;
                    if self.ensure_table_body_paint_fragment(
                        &mut table_body_fragment,
                        piece_top,
                        pending_table_fragment_break_reason,
                        &pending_repeated_header_rows,
                    ) {
                        pending_table_fragment_break_reason = TableFragmentBreakReason::TableStart;
                        pending_repeated_header_rows.clear();
                    }
                    self.layout_table_row_paint_piece(
                        row_index,
                        row,
                        &row_style,
                        rows,
                        &grid,
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        &column_plan,
                        &planned_row_heights,
                        table_metrics,
                        piece_top,
                        row_height,
                        piece_height,
                        piece_offset,
                        collapsed_geometry.as_ref(),
                        row_baseline_offset,
                    );
                    if let Some(fragment) = &mut table_body_fragment {
                        fragment.push_row(
                            row_index,
                            piece_top,
                            piece_height,
                            piece_offset,
                            row_height,
                        );
                    }
                    self.cursor_y -= piece_height;
                    remaining -= piece_height;
                    piece_offset += piece_height;

                    if remaining > 0.01 {
                        if !row_is_repeating_footer {
                            self.mark_table_body_fragment_repeated_footers(
                                &mut table_body_fragment,
                                current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                                &planned_row_heights,
                                table_metrics,
                            );
                        }
                        self.finalize_table_body_paint_fragment(
                            &mut table_body_fragment,
                            rows,
                            &grid,
                            columns,
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            &column_plan,
                            table_width,
                            table_metrics,
                            collapsed_geometry.as_ref(),
                        );
                        if !row_is_repeating_footer {
                            self.layout_repeated_table_footer_rows_at_page_bottom(
                                rows,
                                &grid,
                                columns,
                                current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
                                style,
                                stylesheets,
                                table_x,
                                used_table_width,
                                table_cellpadding,
                                &column_plan,
                                &planned_row_heights,
                                table_width,
                                table_metrics,
                                collapsed_geometry.as_ref(),
                            );
                        }
                        self.push_page();
                        self.cursor_y = self.page_top();
                        current_fragment_repeat_policy = table_fragment_repeat_policy(
                            row_fragment_required_height,
                            self.page_area_height(),
                            repeating_header_height,
                            repeating_footer_height,
                            !row_is_repeating_header,
                            !row_is_repeating_footer,
                        );
                        pending_table_fragment_break_reason =
                            TableFragmentBreakReason::OversizedRowSlice;
                        if !row_is_repeating_header {
                            self.layout_repeated_table_rows(
                                rows,
                                &grid,
                                columns,
                                current_fragment_repeat_policy.header_rows(&repeating_header_rows),
                                style,
                                stylesheets,
                                table_x,
                                used_table_width,
                                table_cellpadding,
                                &column_plan,
                                &planned_row_heights,
                                table_width,
                                table_metrics,
                                collapsed_geometry.as_ref(),
                            );
                            pending_repeated_header_rows = current_fragment_repeat_policy
                                .header_rows(&repeating_header_rows)
                                .to_vec();
                        }
                        if self.ensure_table_body_paint_fragment(
                            &mut table_body_fragment,
                            self.cursor_y,
                            pending_table_fragment_break_reason,
                            &pending_repeated_header_rows,
                        ) {
                            pending_table_fragment_break_reason =
                                TableFragmentBreakReason::TableStart;
                            pending_repeated_header_rows.clear();
                        }
                    }
                }
            } else {
                self.layout_table_row_paint_piece(
                    row_index,
                    row,
                    &row_style,
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    table_x,
                    used_table_width,
                    table_cellpadding,
                    &column_plan,
                    &planned_row_heights,
                    table_metrics,
                    row_top,
                    row_height,
                    row_height,
                    0.0,
                    collapsed_geometry.as_ref(),
                    row_baseline_offset,
                );
                if let Some(fragment) = &mut table_body_fragment {
                    fragment.push_row(row_index, row_top, row_height, 0.0, row_height);
                }
                self.cursor_y -= row_height;
            }
            if !self.table_row_is_hidden_empty(
                row,
                &grid.rows[row_index],
                &row_style,
                stylesheets,
                table_cellpadding,
                &column_plan,
                table_metrics,
            ) && row_index + 1 < rows.len()
                && has_following_uncollapsed_row(&rows[row_index + 1..], style, stylesheets, self)
            {
                self.cursor_y -= table_metrics.spacing.vertical;
            }
            let break_after = if row_style.break_after != PageBreak::Auto {
                row_style.break_after
            } else {
                row_group_break_after[row_index]
            };
            if break_after.is_forced() {
                if row_index + 1 < rows.len() {
                    page_break_before_next_row = break_after;
                } else {
                    forced_break_after_table_rows = break_after;
                }
            }
            let row_candidate = if previous_break_after_avoid {
                avoid_break_candidate
                    .clone()
                    .unwrap_or_else(|| row_start_candidate.clone())
                    .with_height(
                        avoid_break_candidate
                            .as_ref()
                            .map(|candidate| candidate.height)
                            .unwrap_or(0.0)
                            + row_height,
                    )
            } else {
                row_start_candidate.with_height(row_height)
            };
            previous_row_candidate = Some(row_candidate.clone());
            if break_after.avoids_page() {
                avoid_break_candidate = Some(row_candidate);
            } else {
                avoid_break_candidate = None;
            }
            previous_break_after_avoid = break_after.avoids_page();
            row_index += 1;
            if unfragmented_avoid_row_group_end.is_some_and(|end| row_index >= end) {
                unfragmented_avoid_row_group_end = None;
            }
        }

        self.mark_table_body_fragment_repeated_footers(
            &mut table_body_fragment,
            current_fragment_repeat_policy.footer_rows(&repeating_footer_rows),
            &planned_row_heights,
            table_metrics,
        );
        self.finalize_table_body_paint_fragment(
            &mut table_body_fragment,
            rows,
            &grid,
            columns,
            style,
            stylesheets,
            table_x,
            used_table_width,
            table_cellpadding,
            &column_plan,
            table_width,
            table_metrics,
            collapsed_geometry.as_ref(),
        );
        self.fragment_top_offsets.pop();
        self.cursor_y -= table_edge_spacing;

        self.cursor_y -= table_width.padding.bottom + table_width.border_widths.bottom;
        self.layout_table_captions(
            captions,
            style,
            stylesheets,
            table_x,
            used_table_width,
            CaptionSide::Bottom,
        );

        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
        }
        self.pop_float_context();
        self.cursor_y -= style.margin.bottom;
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y;
        }
        self.apply_forced_break(if style.break_after.is_forced() {
            style.break_after
        } else {
            forced_break_after_table_rows
        });
    }

    /// Estimate the block-axis size of a table whose row grid has no rows.
    ///
    /// CSS Tables 3 says that if a table has no slots, its width/height are
    /// computed from the table grid box if definite, otherwise zero; captions,
    /// padding, borders, and margins still contribute to the table wrapper:
    /// <https://drafts.csswg.org/css-tables/#computing-the-table-height>.
    fn estimate_empty_table_height(
        &mut self,
        captions: &[TableCaption<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_table_width: f32,
        table_width: UsedTableWidth,
    ) -> f32 {
        let content_width = used_empty_table_grid_width(style, available_table_width, table_width);
        let content_height =
            used_empty_table_grid_height(style, self.page_area_height(), table_width);
        style.margin.top
            + self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                content_width,
                CaptionSide::Top,
            )
            + table_width.border_widths.top
            + table_width.padding.top
            + content_height
            + table_width.padding.bottom
            + table_width.border_widths.bottom
            + self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                content_width,
                CaptionSide::Bottom,
            )
            + style.margin.bottom
    }

    fn place_empty_table_wrapper(
        &mut self,
        captions: &[TableCaption<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_table_width: f32,
        table_width: UsedTableWidth,
        relative_offset: RelativeOffset,
    ) -> (f32, f32, f32, f32) {
        let content_width = used_empty_table_grid_width(style, available_table_width, table_width);
        let content_height =
            used_empty_table_grid_height(style, self.page_area_height(), table_width);
        let top_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            content_width,
            CaptionSide::Top,
        );
        let bottom_caption_height = self.estimate_table_captions_height(
            captions,
            style,
            stylesheets,
            content_width,
            CaptionSide::Bottom,
        );
        let border_box_width = table_wrapper_border_box_width(content_width, table_width);
        let collision_height = table_wrapper_collision_height(
            style,
            table_width,
            top_caption_height,
            content_height,
            bottom_caption_height,
        );

        self.cursor_y -= style.margin.top;
        self.prebreak_bfc_margin_box_if_needed(collision_height, style.margin.top);
        let (margin_box_left, avoided_top, _) = self.place_float_avoiding_margin_box(
            self.cursor_y,
            style.margin.left + border_box_width + style.margin.right,
            collision_height,
            style.clear,
            style.writing_mode,
            style.direction,
            self.containing_block_direction,
        );
        self.cursor_y = avoided_top;
        (
            margin_box_left + style.margin.left + relative_offset.x,
            content_width,
            content_height,
            border_box_width,
        )
    }

    /// Layout and paint a table whose row grid has no rows.
    ///
    /// CSS Tables 3 keeps an empty table wrapper in layout even when the grid
    /// has no slots. The row grid contributes zero auto width/height, while the
    /// wrapper's padding, borders, captions, margins, and definite grid sizes
    /// still affect painting and block progression:
    /// <https://drafts.csswg.org/css-tables/#computing-the-table-height>.
    #[allow(clippy::too_many_arguments)]
    fn layout_empty_table(
        &mut self,
        captions: &[TableCaption<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_table_width: f32,
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        relative_offset: RelativeOffset,
    ) {
        let (table_outer_x, content_width, content_height, border_box_width) = self
            .place_empty_table_wrapper(
                captions,
                style,
                stylesheets,
                available_table_width,
                table_width,
                relative_offset,
            );
        let border_box_height = table_wrapper_border_box_height(content_height, table_width);
        let table_x = table_width.content_x(table_outer_x);

        self.push_float_context();
        let table_wrapper_top = self.cursor_y;
        let border_box_x = table_x - table_width.padding.left - table_width.border_widths.left;
        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            let top_caption_height = self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                content_width,
                CaptionSide::Top,
            );
            let bottom_caption_height = self.estimate_table_captions_height(
                captions,
                style,
                stylesheets,
                content_width,
                CaptionSide::Bottom,
            );
            self.containing_blocks
                .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                    table_x,
                    table_wrapper_top,
                    content_width,
                    top_caption_height + border_box_height + bottom_caption_height,
                )));
        }
        self.layout_table_captions(
            captions,
            style,
            stylesheets,
            table_x,
            content_width,
            CaptionSide::Top,
        );

        let table_box_top = self.cursor_y;

        let table_structure_paint_checkpoint = self.current_page.paint_checkpoint();
        let table_structure_paint_page_index = self.pages.len();
        if let Some(fill) = style.background_color {
            self.push_rect_in_band(
                PaintBand::InFlowBlock,
                PageTopRect::new(
                    border_box_x,
                    table_box_top,
                    border_box_width,
                    border_box_height,
                )
                .rendered_rect(Some(fill)),
            );
        }
        self.paint_separated_table_wrapper_border(
            style,
            TableWrapperPaintBox {
                table_x,
                top: table_box_top,
                content_width,
                content_height,
                table_width,
                table_metrics,
            },
        );
        if self.pages.len() == table_structure_paint_page_index {
            self.scope_current_page_atomic_paint_since(
                &table_structure_paint_checkpoint,
                PaintBand::InFlowBlock,
                PageTopRect::new(
                    border_box_x,
                    table_box_top,
                    border_box_width,
                    border_box_height,
                )
                .paint_clip(),
                style,
                Vec::new(),
            );
        }

        self.cursor_y -= border_box_height;
        self.layout_table_captions(
            captions,
            style,
            stylesheets,
            table_x,
            content_width,
            CaptionSide::Bottom,
        );
        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
        }
        self.pop_float_context();
        self.cursor_y -= style.margin.bottom;
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y;
        }
        self.apply_forced_break(style.break_after);
    }

    /// Paint the border of a separated-border table wrapper.
    ///
    /// CSS 2.2's separated border model gives the table-root its own ordinary
    /// border box, distinct from row and cell borders. Collapsed borders are
    /// resolved through the collapsed-border grid instead:
    /// <https://www.w3.org/TR/CSS22/tables.html#separated-borders> and
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
    fn paint_separated_table_wrapper_border(
        &mut self,
        style: &ComputedStyle,
        wrapper: TableWrapperPaintBox,
    ) {
        if wrapper.table_metrics.border_collapse == css::BorderCollapse::Collapse {
            return;
        }
        let border_box_x = wrapper.table_x
            - wrapper.table_width.padding.left
            - wrapper.table_width.border_widths.left;
        let border_box_width = wrapper.content_width
            + wrapper.table_width.padding.left
            + wrapper.table_width.padding.right
            + wrapper.table_width.border_widths.left
            + wrapper.table_width.border_widths.right;
        let border_box_height = wrapper.content_height
            + wrapper.table_width.padding.top
            + wrapper.table_width.padding.bottom
            + wrapper.table_width.border_widths.top
            + wrapper.table_width.border_widths.bottom;
        let mut border_rects = Vec::new();
        let mut border_paths = Vec::new();
        paint_table_border_edges(
            &mut border_rects,
            &mut border_paths,
            border_box_x,
            wrapper.top,
            border_box_width,
            border_box_height,
            style,
        );
        for rect in border_rects {
            self.push_rect_in_band(PaintBand::InFlowBlock, rect);
        }
        for path in border_paths {
            self.push_path_in_band(PaintBand::InFlowBlock, path);
        }
    }

    /// Paint a repeated `table-footer-group` at the block-end of a page fragment.
    ///
    /// CSS 2.2 allows print user agents to repeat table footer groups on each
    /// page spanned by a table, visually after the body rows in that page
    /// fragment and before bottom captions.
    /// https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group
    #[allow(clippy::too_many_arguments)]
    pub(super) fn layout_repeated_table_footer_rows_at_page_bottom(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        columns: &[TableColumn<'_>],
        footer_rows: &[usize],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) {
        let footer_height =
            repeated_table_rows_height(footer_rows, planned_row_heights, table_metrics);
        if footer_rows.is_empty() || footer_height > self.page_area_height() + 0.01 {
            return;
        }

        let previous_cursor_y = self.cursor_y;
        self.cursor_y = self.page_bottom() + footer_height;
        self.layout_repeated_table_rows(
            rows,
            grid,
            columns,
            footer_rows,
            table_style,
            stylesheets,
            table_x,
            used_table_width,
            table_cellpadding,
            column_plan,
            planned_row_heights,
            table_width,
            table_metrics,
            collapsed_geometry,
        );
        self.cursor_y = previous_cursor_y;
    }

    /// Replay measured table row boxes for repeated table header/footer groups.
    ///
    /// CSS 2.2 defines `table-header-group` and `table-footer-group` as row
    /// groups that print user agents may repeat on pages spanned by a table.
    /// https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group
    /// https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group
    #[allow(clippy::too_many_arguments)]
    pub(super) fn layout_repeated_table_rows(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        columns: &[TableColumn<'_>],
        repeated_rows: &[usize],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) {
        if repeated_rows.is_empty() {
            return;
        }
        let repeated_height =
            repeated_table_rows_height(repeated_rows, planned_row_heights, table_metrics);
        if repeated_height > self.page_area_height() + 0.01 {
            return;
        }

        let mut repeated_row_tops = Vec::with_capacity(repeated_rows.len());
        let mut repeated_row_heights = Vec::with_capacity(repeated_rows.len());
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        let fragment_top = self.cursor_y;
        let occupied_inline_bounds = column_plan
            .occupied_inline_bounds()
            .unwrap_or_else(|| TableInlineBounds::new(0.0, used_table_width));
        let occupied_x = table_x + occupied_inline_bounds.start;
        let occupied_width = occupied_inline_bounds.size;
        self.paint_repeated_table_fragment_structural_layers(
            rows,
            repeated_rows,
            columns,
            table_style,
            stylesheets,
            table_x,
            used_table_width,
            fragment_top,
            repeated_height,
            table_width,
            column_plan,
            planned_row_heights,
            table_metrics,
        );
        for (position, row_index) in repeated_rows.iter().copied().enumerate() {
            let row = &rows[row_index];
            let row_style = self.style_for_table_row(row, table_style, stylesheets);
            let row_height = planned_row_heights[row_index];
            let row_top = self.cursor_y;
            repeated_row_tops.push(row_top);
            repeated_row_heights.push(if table_row_is_collapsed(&row_style) {
                0.0
            } else {
                row_height
            });
            if table_row_is_collapsed(&row_style) {
                continue;
            }

            let row_baseline_offset = self.table_row_baseline_offset(
                row_index,
                row,
                &grid.rows[row_index],
                &row_style,
                stylesheets,
                table_cellpadding,
                column_plan,
                table_metrics,
                collapsed_geometry,
            );
            // CSS Tables allow repeated table-header-group and
            // table-footer-group boxes on fragmented tables. This replays the
            // row group's visible row content using measured row heights.
            // Collapsed-border conflict resolution for repeated fragments
            // still needs durable per-fragment table border grids.
            // https://www.w3.org/TR/CSS22/tables.html#table-display
            if row_style.background_color.is_some() {
                self.push_rect_in_band(
                    PaintBand::InFlowBlock,
                    PageTopRect::new(occupied_x, row_top, occupied_width, row_height)
                        .rendered_rect(row_style.background_color),
                );
            }
            for placement in &grid.rows[row_index] {
                let cell = &row.cells[placement.cell];
                let Some(prepared) = self.prepare_table_cell(
                    cell,
                    row,
                    &row_style,
                    placement,
                    row_index,
                    table_x,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics,
                    collapsed_geometry,
                ) else {
                    continue;
                };
                let cell_style = &prepared.style;
                let cell_borders = prepared.borders;
                let metrics = prepared.metrics;
                let cell_height = table_row_span_height(
                    planned_row_heights,
                    row_index,
                    placement.rowspan,
                    table_metrics,
                )
                .max(metrics.border_box_height);
                let cell_placement = TableGridPlacement::new(PageTopPoint::new(table_x, row_top));
                let cell_border_box = column_plan
                    .cell_border_box(prepared.area, TableRowBounds::new(0.0, cell_height));
                let cell_x = cell_border_box.x(cell_placement);
                let cell_width = cell_border_box.width();
                let text = prepared.text;
                let cell_is_empty = text.is_empty() && metrics.content_height <= 0.0;
                let baseline_context = TableCellBaselineAlignmentContext {
                    row_index,
                    row_style: &row_style,
                    table_style,
                    rows,
                    grid,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    planned_row_heights,
                    table_metrics,
                    collapsed_geometry,
                    row_baseline_offset,
                };
                let cell_row_baseline_offset = self.table_cell_row_baseline_offset_for_alignment(
                    &baseline_context,
                    placement,
                    cell_style,
                );
                let content_offset = table_cell_content_offset(
                    cell_style,
                    metrics.content_height,
                    cell_height,
                    cell_row_baseline_offset,
                    metrics.baseline_offset,
                );
                let content_x_offset = self.table_cell_content_x_offset(
                    cell,
                    cell_style,
                    stylesheets,
                    cell_width,
                    cell_borders,
                );
                let content_clip = self.collapsed_rowspan_cell_content_clip(
                    row_index,
                    placement.rowspan,
                    rows,
                    table_style,
                    stylesheets,
                    planned_row_heights,
                    table_metrics,
                    cell_border_box,
                    cell_placement,
                );
                let paint_empty_cell = table_metrics.border_collapse
                    == css::BorderCollapse::Collapse
                    || cell_style.empty_cells == EmptyCells::Show
                    || !cell_is_empty;

                if paint_empty_cell && cell_style.background_color.is_some() {
                    self.push_rect_in_band(
                        PaintBand::InFlowBlock,
                        cell_border_box.rendered_rect(cell_placement, cell_style.background_color),
                    );
                }
                if table_metrics.border_collapse != css::BorderCollapse::Collapse
                    && paint_empty_cell
                {
                    let mut border_rects = Vec::new();
                    let mut border_paths = Vec::new();
                    paint_table_border_edges(
                        &mut border_rects,
                        &mut border_paths,
                        cell_x,
                        row_top,
                        cell_width,
                        cell_height,
                        cell_style,
                    );
                    for rect in border_rects {
                        self.push_rect_in_band(PaintBand::InFlowBlock, rect);
                    }
                    for path in border_paths {
                        self.push_path_in_band(PaintBand::InFlowBlock, path);
                    }
                }

                let clip_active = if let Some(clip) = content_clip {
                    self.push_overflow_clip(clip);
                    true
                } else {
                    false
                };

                if !text.is_empty() && cell.children.is_none() {
                    let previous_left = self.content_left;
                    let previous_right = self.content_right;
                    let previous_cursor_y = self.cursor_y;
                    let content_box = cell_border_box.content_box(
                        cell_placement,
                        cell_style.padding,
                        cell_borders,
                        content_offset,
                        content_x_offset,
                    );
                    self.content_left = content_box.left();
                    self.content_right = content_box.right();
                    self.cursor_y = content_box.top_y();
                    self.push_float_context();
                    if let Some(element) = cell.element {
                        self.layout_inline_items_block(
                            element,
                            cell_style,
                            stylesheets,
                            (0.0, 0.0),
                            table_cell_href(cell),
                            None,
                        );
                    } else {
                        self.layout_text_block(&text, cell_style, 0.0, 0.0, table_cell_href(cell));
                    }
                    self.pop_float_context();
                    self.content_left = previous_left;
                    self.content_right = previous_right;
                    self.cursor_y = previous_cursor_y;
                }
                self.layout_table_cell_replaced_children(
                    cell,
                    cell_style,
                    cell_border_box,
                    cell_placement,
                    content_offset,
                    content_x_offset,
                );
                self.layout_table_cell_flow_children(
                    cell,
                    row,
                    cell_style,
                    stylesheets,
                    cell_border_box,
                    cell_placement,
                    content_offset,
                    content_x_offset,
                );
                self.layout_table_cell_positioned_children(
                    cell,
                    row,
                    cell_style,
                    stylesheets,
                    cell_border_box,
                    cell_placement,
                );
                self.pop_overflow_clip(clip_active);
            }

            if table_metrics.border_collapse != css::BorderCollapse::Collapse {
                let mut border_rects = Vec::new();
                let mut border_paths = Vec::new();
                paint_table_border_edges(
                    &mut border_rects,
                    &mut border_paths,
                    occupied_x,
                    row_top,
                    occupied_width,
                    row_height,
                    &row_style,
                );
                for rect in border_rects {
                    self.push_rect_in_band(PaintBand::InFlowBlock, rect);
                }
                for path in border_paths {
                    self.push_path_in_band(PaintBand::InFlowBlock, path);
                }
            }

            self.cursor_y -= row_height;
            if position + 1 < repeated_rows.len() {
                self.cursor_y -= table_metrics.spacing.vertical;
            }
        }
        if let Some(geometry) = collapsed_geometry {
            let repeated_row_offsets = vec![0.0; repeated_rows.len()];
            let repeated_original_heights = repeated_rows
                .iter()
                .map(|row| planned_row_heights[*row])
                .collect::<Vec<_>>();
            let placement = TableGridPlacement::new(PageTopPoint::new(table_x, 0.0));
            let (rects, paths) = geometry.grid.paint_fragment_rows(
                placement,
                column_plan,
                repeated_rows,
                &repeated_row_tops,
                &repeated_row_heights,
                &repeated_row_offsets,
                &repeated_original_heights,
            );
            for rect in rects {
                self.push_rect_in_band(PaintBand::InFlowBlock, rect);
            }
            for path in paths {
                self.push_path_in_band(PaintBand::InFlowBlock, path);
            }
        }
        if self.pages.len() == paint_page_index {
            let bounds = PageTopRect::new(
                table_x - table_width.padding.left - table_width.border_widths.left,
                fragment_top + table_width.padding.top + table_width.border_widths.top,
                used_table_width
                    + table_width.padding.left
                    + table_width.padding.right
                    + table_width.border_widths.left
                    + table_width.border_widths.right,
                fragment_top + table_width.padding.top + table_width.border_widths.top
                    - self.cursor_y
                    + table_width.padding.bottom
                    + table_width.border_widths.bottom,
            )
            .paint_clip();
            let captures_positioned_descendants =
                StackingContextPolicy::for_atomic(table_style, PaintBand::InFlowBlock, bounds)
                    .captures_positioned_descendants;
            let child_contexts = if captures_positioned_descendants
                && positioned_layer_start < self.positioned_layers.len()
            {
                self.positioned_layers
                    .split_off(positioned_layer_start)
                    .into_iter()
                    .filter(|layer| layer.page_index == paint_page_index)
                    .map(|layer| layer.context.with_links(layer.links))
                    .collect()
            } else {
                Vec::new()
            };
            self.scope_current_page_atomic_paint_since(
                &paint_checkpoint,
                PaintBand::InFlowBlock,
                bounds,
                table_style,
                child_contexts,
            );
        }
    }

    /// Paint table and column structural layers for one repeated table fragment.
    ///
    /// CSS 2.2 table painting orders structural backgrounds below row, cell,
    /// and border paint, while outlines paint in the final outline band.
    /// Repeated header/footer fragments therefore need their own page-local
    /// table, column, and row-group layers around row replay:
    /// <https://www.w3.org/TR/CSS22/tables.html#table-layers> and
    /// <https://drafts.csswg.org/css-tables-3/#rendering>.
    #[allow(clippy::too_many_arguments)]
    fn paint_repeated_table_fragment_structural_layers(
        &mut self,
        rows: &[TableRow<'_>],
        repeated_rows: &[usize],
        columns: &[TableColumn<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        fragment_top: f32,
        fragment_height: f32,
        table_width: UsedTableWidth,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        table_metrics: TableMetrics,
    ) {
        if let Some(fill) = table_style.background_color {
            let background_top =
                fragment_top + table_width.padding.top + table_width.border_widths.top;
            let background_bottom = fragment_top
                - fragment_height
                - table_width.padding.bottom
                - table_width.border_widths.bottom;
            self.push_rect_in_band(
                PaintBand::InFlowBlock,
                PageTopRect::new(
                    table_x - table_width.padding.left - table_width.border_widths.left,
                    background_top,
                    used_table_width
                        + table_width.padding.left
                        + table_width.padding.right
                        + table_width.border_widths.left
                        + table_width.border_widths.right,
                    background_top - background_bottom,
                )
                .rendered_rect(Some(fill)),
            );
        }
        for (start_column, end_column, column_group) in
            table_column_group_spans(columns, column_plan.column_count())
        {
            let column_group_style =
                self.style_for_table_column_group(&column_group, table_style, stylesheets);
            self.paint_table_column_background(
                table_x,
                fragment_top,
                fragment_height,
                column_plan,
                start_column,
                end_column,
                column_group_style.background_color,
            );
        }
        let mut column_index = 0;
        for column in columns {
            if column_index >= column_plan.column_count() {
                break;
            }
            let span = column
                .span
                .min(column_plan.column_count() - column_index)
                .max(1);
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            self.paint_table_column_background(
                table_x,
                fragment_top,
                fragment_height,
                column_plan,
                column_index,
                column_index + span,
                column_style.background_color,
            );
            column_index += span;
        }

        let mut local_row_tops = Vec::with_capacity(repeated_rows.len());
        let mut local_row_heights = Vec::with_capacity(repeated_rows.len());
        let mut cursor_y = fragment_top;
        let occupied_inline_bounds = column_plan
            .occupied_inline_bounds()
            .unwrap_or_else(|| TableInlineBounds::new(0.0, used_table_width));
        let occupied_x = table_x + occupied_inline_bounds.start;
        let occupied_width = occupied_inline_bounds.size;
        for (position, row_index) in repeated_rows.iter().copied().enumerate() {
            local_row_tops.push(cursor_y);
            let row_height = planned_row_heights[row_index];
            local_row_heights.push(row_height);
            cursor_y -= row_height;
            if position + 1 < repeated_rows.len() {
                cursor_y -= table_metrics.spacing.vertical;
            }
        }
        for (start_row, end_row, row_group) in table_row_group_spans(rows) {
            let row_group_style =
                self.style_for_table_row_group(&row_group, table_style, stylesheets);
            if let Some(fill) = row_group_style.background_color {
                let mut segment_start = None;
                let mut previous_local = None;
                for (local_row, original_row) in repeated_rows.iter().copied().enumerate() {
                    if original_row >= start_row && original_row < end_row {
                        if segment_start.is_none() {
                            segment_start = Some(local_row);
                        }
                        previous_local = Some(local_row + 1);
                    } else if let (Some(start), Some(end)) =
                        (segment_start.take(), previous_local.take())
                    {
                        self.paint_repeated_table_row_group_background(
                            occupied_x,
                            occupied_width,
                            &local_row_tops,
                            &local_row_heights,
                            start,
                            end,
                            fill,
                        );
                    }
                }
                if let (Some(start), Some(end)) = (segment_start, previous_local) {
                    self.paint_repeated_table_row_group_background(
                        occupied_x,
                        occupied_width,
                        &local_row_tops,
                        &local_row_heights,
                        start,
                        end,
                        fill,
                    );
                }
            }
            if row_group_style.visibility == Visibility::Visible {
                self.paint_repeated_table_row_group_outline(
                    occupied_x,
                    occupied_width,
                    &local_row_tops,
                    &local_row_heights,
                    repeated_rows,
                    start_row,
                    end_row,
                    &row_group_style,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_repeated_table_row_group_background(
        &mut self,
        table_x: f32,
        used_table_width: f32,
        local_row_tops: &[f32],
        local_row_heights: &[f32],
        start: usize,
        end: usize,
        fill: Color,
    ) {
        if let Some(bounds) = table_fragment_row_span_bounds(
            table_x,
            used_table_width,
            local_row_tops,
            local_row_heights,
            start,
            end,
        ) {
            self.push_rect_in_band(
                PaintBand::InFlowBlock,
                RenderedRect::from_paint_rect(bounds.paint_rect(), Some(fill)),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_repeated_table_row_group_outline(
        &mut self,
        table_x: f32,
        used_table_width: f32,
        local_row_tops: &[f32],
        local_row_heights: &[f32],
        repeated_rows: &[usize],
        start_row: usize,
        end_row: usize,
        row_group_style: &ComputedStyle,
    ) {
        let mut segment_start = None;
        let mut previous_local = None;
        for (local_row, original_row) in repeated_rows.iter().copied().enumerate() {
            if original_row >= start_row && original_row < end_row {
                if segment_start.is_none() {
                    segment_start = Some(local_row);
                }
                previous_local = Some(local_row + 1);
            } else if let (Some(start), Some(end)) = (segment_start.take(), previous_local.take()) {
                self.push_repeated_table_row_group_outline(
                    table_x,
                    used_table_width,
                    local_row_tops,
                    local_row_heights,
                    start,
                    end,
                    row_group_style,
                );
            }
        }
        if let (Some(start), Some(end)) = (segment_start, previous_local) {
            self.push_repeated_table_row_group_outline(
                table_x,
                used_table_width,
                local_row_tops,
                local_row_heights,
                start,
                end,
                row_group_style,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_repeated_table_row_group_outline(
        &mut self,
        table_x: f32,
        used_table_width: f32,
        local_row_tops: &[f32],
        local_row_heights: &[f32],
        start: usize,
        end: usize,
        row_group_style: &ComputedStyle,
    ) {
        let Some(bounds) = table_fragment_row_span_bounds(
            table_x,
            used_table_width,
            local_row_tops,
            local_row_heights,
            start,
            end,
        ) else {
            return;
        };
        for primitive in self.box_outline_primitives(
            bounds.x(),
            bounds.y(),
            bounds.width(),
            bounds.height(),
            row_group_style,
        ) {
            self.push_primitive_in_band(PaintBand::Outline, primitive);
        }
    }

    fn ensure_table_body_paint_fragment(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        fragment_top: f32,
        break_reason: TableFragmentBreakReason,
        repeated_header_rows: &[usize],
    ) -> bool {
        if fragment.is_none() {
            let mut new_fragment = TableBodyPaintFragment::new(
                self.current_page.paint_checkpoint(),
                self.pages.len(),
                self.positioned_layers.len(),
                fragment_top,
                break_reason,
            );
            new_fragment.mark_repeated_headers(repeated_header_rows);
            *fragment = Some(new_fragment);
            true
        } else {
            false
        }
    }

    fn mark_table_body_fragment_repeated_footers(
        &self,
        fragment: &mut Option<TableBodyPaintFragment>,
        footer_rows: &[usize],
        planned_row_heights: &[f32],
        table_metrics: TableMetrics,
    ) {
        if footer_rows.is_empty() {
            return;
        }
        let footer_height =
            repeated_table_rows_height(footer_rows, planned_row_heights, table_metrics);
        if footer_height <= self.page_area_height() + 0.01
            && let Some(fragment) = fragment
        {
            fragment.mark_repeated_footers(footer_rows);
        }
    }

    /// Finalize one table-body page piece as a durable scoped paint context.
    ///
    /// CSS 2.2 table painting has internal layer order, and CSS Fragmentation
    /// repeats that painting model for each page fragment. The finalized
    /// context preserves that table-local order until final PDF emission:
    /// <https://www.w3.org/TR/CSS22/tables.html#table-layers> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    #[allow(clippy::too_many_arguments)]
    fn finalize_table_body_paint_fragment(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        rows: &[TableRow<'_>],
        _grid: &TableGrid,
        columns: &[TableColumn<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        _table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) {
        let Some(fragment_state) = fragment.take() else {
            return;
        };
        if fragment_state.plan.body_rows.is_empty()
            || fragment_state.plan.page_index != self.pages.len()
        {
            return;
        }
        let repeated_rows = fragment_state.repeated_rows();
        debug_assert!(repeated_rows.iter().all(|row| *row < rows.len()));
        debug_assert!(
            !fragment_state.starts_after_break()
                || fragment_state.plan.break_reason != TableFragmentBreakReason::TableStart
        );
        debug_assert!(
            !fragment_state.has_split_or_collapsed_rows()
                || fragment_state
                    .plan
                    .body_rows
                    .iter()
                    .any(|row| row.collapsed || row.artificial_split)
        );
        let Some(mut fragment) = self
            .current_page
            .paint_tree_fragment_since(&fragment_state.checkpoint)
        else {
            return;
        };

        let (structural_backgrounds, structural_outlines) = self
            .table_body_fragment_structural_primitives(
                rows,
                columns,
                table_style,
                stylesheets,
                table_x,
                used_table_width,
                table_width,
                table_metrics,
                column_plan,
                &fragment_state,
            );
        self.current_page.prepend_recorded_primitives_to_fragment(
            &mut fragment,
            PaintBand::BackgroundBorder,
            structural_backgrounds,
        );
        self.current_page.append_recorded_primitives_to_fragment(
            &mut fragment,
            PaintBand::Outline,
            structural_outlines,
        );

        if table_metrics.border_collapse == css::BorderCollapse::Collapse {
            let borders = collapsed_geometry
                .map(|geometry| {
                    self.collapsed_table_fragment_border_primitives(
                        geometry,
                        table_x,
                        column_plan,
                        &fragment_state,
                    )
                })
                .unwrap_or_default();
            self.current_page.append_recorded_primitives_to_fragment(
                &mut fragment,
                PaintBand::InFlowBlock,
                borders,
            );
        }

        let bottom = fragment_state.bottom();
        let bounds_x = table_x - table_width.padding.left - table_width.border_widths.left;
        let bounds_top = fragment_state.plan.fragment_top
            + table_width.padding.top
            + table_width.border_widths.top;
        let bounds_bottom = bottom - table_width.padding.bottom - table_width.border_widths.bottom;
        let bounds = PageTopRect::new(
            bounds_x,
            bounds_top,
            used_table_width
                + table_width.padding.left
                + table_width.padding.right
                + table_width.border_widths.left
                + table_width.border_widths.right,
            bounds_top - bounds_bottom,
        )
        .paint_clip();
        let policy = StackingContextPolicy::for_atomic(table_style, PaintBand::InFlowBlock, bounds);
        let child_contexts = if policy.captures_positioned_descendants
            && fragment_state.positioned_layer_start < self.positioned_layers.len()
        {
            self.positioned_layers
                .split_off(fragment_state.positioned_layer_start)
                .into_iter()
                .filter(|layer| layer.page_index == fragment_state.plan.page_index)
                .map(|layer| layer.context.with_links(layer.links))
                .collect()
        } else {
            Vec::new()
        };
        if fragment.is_empty() && child_contexts.is_empty() {
            return;
        }

        let context = PaintStackingContext::from_banded_fragment(fragment, child_contexts)
            .with_source_order(self.next_paint_source_order())
            .with_effects(policy.effects)
            .with_bounds(bounds);
        self.current_page.replace_paint_tree_since_with_context(
            &fragment_state.checkpoint,
            policy.parent_band,
            context,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_table_row_paint_piece(
        &mut self,
        row_index: usize,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        table_metrics: TableMetrics,
        row_top: f32,
        row_height: f32,
        piece_height: f32,
        piece_offset: f32,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        row_baseline_offset: Option<f32>,
    ) {
        let split_piece = piece_offset > 0.0 || piece_height + 0.01 < row_height;
        let occupied_inline_bounds = column_plan
            .occupied_inline_bounds()
            .unwrap_or_else(|| TableInlineBounds::new(0.0, used_table_width));
        let occupied_x = table_x + occupied_inline_bounds.start;
        let occupied_width = occupied_inline_bounds.size;
        let row_piece_clip_active = if split_piece {
            self.push_overflow_clip(
                PageTopRect::new(table_x, row_top, used_table_width, piece_height).overflow_clip(),
            );
            true
        } else {
            false
        };
        let content_row_top = row_top + piece_offset;

        for placement in &grid.rows[row_index] {
            let cell = &row.cells[placement.cell];
            let Some(prepared) = self.prepare_table_cell(
                cell,
                row,
                row_style,
                placement,
                row_index,
                table_x,
                stylesheets,
                table_cellpadding,
                column_plan,
                table_metrics,
                collapsed_geometry,
            ) else {
                continue;
            };
            let cell_style = &prepared.style;
            let cell_borders = prepared.borders;
            let metrics = prepared.metrics;
            let cell_height = table_row_span_height(
                planned_row_heights,
                row_index,
                placement.rowspan,
                table_metrics,
            )
            .max(metrics.border_box_height);
            let cell_placement =
                TableGridPlacement::new(PageTopPoint::new(table_x, content_row_top));
            let cell_border_box =
                column_plan.cell_border_box(prepared.area, TableRowBounds::new(0.0, cell_height));
            let cell_width = cell_border_box.width();
            let text = prepared.text;
            let cell_is_empty = text.is_empty() && metrics.content_height <= 0.0;
            let baseline_context = TableCellBaselineAlignmentContext {
                row_index,
                row_style,
                table_style,
                rows,
                grid,
                stylesheets,
                table_cellpadding,
                column_plan,
                planned_row_heights,
                table_metrics,
                collapsed_geometry,
                row_baseline_offset,
            };
            let cell_row_baseline_offset = self.table_cell_row_baseline_offset_for_alignment(
                &baseline_context,
                placement,
                cell_style,
            );
            let content_offset = table_cell_content_offset(
                cell_style,
                metrics.content_height,
                cell_height,
                cell_row_baseline_offset,
                metrics.baseline_offset,
            );
            let content_x_offset = self.table_cell_content_x_offset(
                cell,
                cell_style,
                stylesheets,
                cell_width,
                cell_borders,
            );
            let content_clip = self.collapsed_rowspan_cell_content_clip(
                row_index,
                placement.rowspan,
                rows,
                table_style,
                stylesheets,
                planned_row_heights,
                table_metrics,
                cell_border_box,
                cell_placement,
            );
            let mut cell_fragment_plan = TableCellFragmentPlan {
                border_box: cell_border_box,
                placement: cell_placement,
                content_offset,
                content_x_offset,
                content_clip,
                area: prepared.area,
                content: TableCellContentPlan::empty(),
            };
            cell_fragment_plan.content = self.plan_table_cell_content(
                cell,
                row,
                cell_style,
                stylesheets,
                cell_width,
                cell_borders,
                &text,
                content_row_top,
                content_offset,
                row_top,
                piece_height,
                split_piece,
            );
            debug_assert_eq!(cell_fragment_plan.area.row, row_index);
            debug_assert_eq!(cell_fragment_plan.area.column, placement.column);
            debug_assert_eq!(cell_fragment_plan.area.colspan, placement.colspan.max(1));
            debug_assert_eq!(cell_fragment_plan.area.rowspan, placement.rowspan.max(1));

            let paint_empty_cell = table_metrics.border_collapse == css::BorderCollapse::Collapse
                || cell_style.empty_cells == EmptyCells::Show
                || !cell_is_empty;

            if paint_empty_cell {
                let (rects, rounded_rects, paths, strokes) = block_paint_ops_with_border_insets(
                    cell_fragment_plan.x(),
                    cell_fragment_plan.top_y() - cell_fragment_plan.height(),
                    cell_fragment_plan.width(),
                    cell_fragment_plan.height(),
                    cell_style,
                    cell_borders,
                    table_metrics.border_collapse != css::BorderCollapse::Collapse,
                );
                for rect in rects {
                    self.push_rect_in_band(PaintBand::InFlowBlock, rect);
                }
                for rounded_rect in rounded_rects {
                    self.push_rounded_rect_in_band(PaintBand::InFlowBlock, rounded_rect);
                }
                for path in paths {
                    self.push_path_in_band(PaintBand::InFlowBlock, path);
                }
                for stroke in strokes {
                    self.push_stroke_in_band(PaintBand::InFlowBlock, stroke);
                }
            }

            let clip_active = if let Some(clip) = cell_fragment_plan.content_clip {
                self.push_overflow_clip(clip);
                true
            } else {
                false
            };

            let inline_sequence_paints_cell_children = cell_fragment_plan
                .content
                .children_painted_by_inline_sequence;
            if !text.is_empty()
                && (cell.children.is_none() || cell_fragment_plan.content.inline_sequence.is_some())
            {
                let previous_left = self.content_left;
                let previous_right = self.content_right;
                let previous_cursor_y = self.cursor_y;
                let content_box = cell_fragment_plan.content_box(cell_style, cell_borders);
                self.content_left = content_box.left();
                self.content_right = content_box.right();
                self.cursor_y = content_box.top_y();
                self.push_float_context();
                if let Some(sequence) = &cell_fragment_plan.content.inline_sequence {
                    if split_piece {
                        self.paint_inline_line_sequence_slice(
                            sequence,
                            cell_style,
                            self.cursor_y,
                            row_top,
                            row_top - piece_height,
                        );
                    } else {
                        self.paint_inline_line_sequence(sequence, cell_style);
                    }
                } else if split_piece {
                    if let Some(element) = cell.element {
                        self.paint_element_inline_block_slice(
                            element,
                            cell_style,
                            stylesheets,
                            0.0,
                            0.0,
                            table_cell_href(cell),
                            self.cursor_y,
                            row_top,
                            row_top - piece_height,
                        );
                    } else {
                        self.paint_text_block_slice(
                            &text,
                            cell_style,
                            0.0,
                            0.0,
                            table_cell_href(cell),
                            self.cursor_y,
                            row_top,
                            row_top - piece_height,
                        );
                    }
                } else if let Some(element) = cell.element {
                    self.layout_inline_items_block(
                        element,
                        cell_style,
                        stylesheets,
                        (0.0, 0.0),
                        table_cell_href(cell),
                        None,
                    );
                } else {
                    self.layout_text_block(&text, cell_style, 0.0, 0.0, table_cell_href(cell));
                }
                self.pop_float_context();
                self.content_left = previous_left;
                self.content_right = previous_right;
                self.cursor_y = previous_cursor_y;
            }
            if !inline_sequence_paints_cell_children {
                if split_piece {
                    self.paint_table_cell_planned_child_fragments(
                        cell,
                        row,
                        cell_style,
                        stylesheets,
                        cell_fragment_plan.border_box,
                        cell_fragment_plan.placement,
                        cell_fragment_plan.content_offset,
                        cell_fragment_plan.content_x_offset,
                        &cell_fragment_plan.content.child_fragments,
                    );
                } else {
                    self.layout_table_cell_flow_children(
                        cell,
                        row,
                        cell_style,
                        stylesheets,
                        cell_fragment_plan.border_box,
                        cell_fragment_plan.placement,
                        cell_fragment_plan.content_offset,
                        cell_fragment_plan.content_x_offset,
                    );
                }
            }
            if !split_piece {
                self.layout_table_cell_replaced_children(
                    cell,
                    cell_style,
                    cell_fragment_plan.border_box,
                    cell_fragment_plan.placement,
                    cell_fragment_plan.content_offset,
                    cell_fragment_plan.content_x_offset,
                );
            }
            self.layout_table_cell_positioned_children(
                cell,
                row,
                cell_style,
                stylesheets,
                cell_fragment_plan.border_box,
                cell_fragment_plan.placement,
            );
            self.pop_overflow_clip(clip_active);
        }
        if table_metrics.border_collapse != css::BorderCollapse::Collapse {
            let mut border_rects = Vec::new();
            let mut border_paths = Vec::new();
            paint_table_border_edges(
                &mut border_rects,
                &mut border_paths,
                occupied_x,
                content_row_top,
                occupied_width,
                row_height,
                row_style,
            );
            for rect in border_rects {
                self.push_rect_in_band(PaintBand::InFlowBlock, rect);
            }
            for path in border_paths {
                self.push_path_in_band(PaintBand::InFlowBlock, path);
            }
        }
        self.pop_overflow_clip(row_piece_clip_active);
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_table_cell_content(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        cell_width: f32,
        cell_borders: css::Edges,
        text: &str,
        content_row_top: f32,
        content_offset: f32,
        row_top: f32,
        piece_height: f32,
        split_piece: bool,
    ) -> TableCellContentPlan {
        let mut plan = TableCellContentPlan::empty();
        let available_width = (cell_width
            - cell_borders.left
            - cell_borders.right
            - cell_style.padding.left
            - cell_style.padding.right)
            .max(1.0);

        if let Some(children) = cell.children.as_deref()
            && table_cell_children_can_use_inline_line_sequence(children)
        {
            let mut items = Vec::new();
            let link_target = table_cell_href(cell).map(str::to_string);
            if let Some(element) = cell.element {
                self.push_generated_pseudo_items(
                    element,
                    cell_style.before_style.as_deref(),
                    link_target.clone(),
                    0.0,
                    GeneratedPseudoCounterMode::Commit,
                    &mut items,
                );
            }
            self.collect_inline_box_items(
                children,
                stylesheets,
                link_target.clone(),
                0.0,
                cell_style,
                cell_style.text_decoration,
                &mut items,
            );
            if let Some(element) = cell.element {
                self.push_generated_pseudo_items(
                    element,
                    cell_style.after_style.as_deref(),
                    link_target,
                    0.0,
                    GeneratedPseudoCounterMode::Commit,
                    &mut items,
                );
            }
            if !items.is_empty() {
                plan.inline_sequence = Some(self.collect_inline_line_sequence(
                    items,
                    cell_style,
                    available_width,
                    0.0,
                    0.0,
                ));
                plan.children_painted_by_inline_sequence = true;
                return plan;
            }
        }

        if cell.children.is_none() {
            if let Some(element) = cell.element {
                let mut items = Vec::new();
                let link_target = table_cell_href(cell).map(str::to_string);
                self.push_generated_pseudo_items(
                    element,
                    cell_style.before_style.as_deref(),
                    link_target.clone(),
                    0.0,
                    GeneratedPseudoCounterMode::Commit,
                    &mut items,
                );
                self.collect_element_content_or_inline_items(
                    element,
                    cell_style,
                    stylesheets,
                    link_target.clone(),
                    0.0,
                    &mut items,
                );
                self.push_generated_pseudo_items(
                    element,
                    cell_style.after_style.as_deref(),
                    link_target,
                    0.0,
                    GeneratedPseudoCounterMode::Commit,
                    &mut items,
                );
                if !items.is_empty() {
                    plan.inline_sequence = Some(self.collect_inline_line_sequence(
                        items,
                        cell_style,
                        available_width,
                        0.0,
                        0.0,
                    ));
                }
            } else if !text.is_empty() {
                plan.inline_sequence = Some(self.inline_line_sequence_for_text(
                    text,
                    cell_style,
                    available_width,
                    0.0,
                    table_cell_href(cell),
                ));
            }
        }

        if split_piece && let Some(children) = cell.children.as_deref() {
            let child_top =
                content_row_top - cell_borders.top - cell_style.padding.top - content_offset;
            plan.child_fragments = self.table_cell_child_fragment_plans(
                children,
                stylesheets,
                available_width,
                child_top,
                row_top,
                row_top - piece_height,
            );
            for child_plan in &mut plan.child_fragments {
                if child_plan.kind == TableCellChildFragmentKind::NestedFormattingContext
                    && let Some(child_box) = children.get(child_plan.source_child_index)
                {
                    child_plan.nested_fragment = self
                        .plan_table_cell_nested_child_fragment(
                            cell,
                            row,
                            child_box,
                            stylesheets,
                            available_width,
                        )
                        .map(|mut nested_fragment| {
                            nested_fragment.metadata = child_plan.metadata.clone();
                            nested_fragment
                        });
                }
            }
        }

        plan
    }

    fn table_cell_nested_inline_sequence_for_child(
        &mut self,
        child_box: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> Option<TableCellNestedInlineSequencePlan> {
        let style = match child_box {
            box_tree::FormattingBox::Text(box_) => &box_.style,
            box_tree::FormattingBox::Line(box_) => box_.children.first().map(|text| &text.style)?,
            box_tree::FormattingBox::Inline(box_) => &box_.style,
            box_tree::FormattingBox::AnonymousBlock(box_) => &box_.style,
            box_tree::FormattingBox::Block(_)
            | box_tree::FormattingBox::AtomicInline(_)
            | box_tree::FormattingBox::Table(_)
            | box_tree::FormattingBox::Flex(_)
            | box_tree::FormattingBox::Replaced(_) => return None,
        };
        self.table_cell_nested_inline_sequence_for_children(
            style,
            std::slice::from_ref(child_box),
            stylesheets,
            None,
            available_width,
        )
    }

    fn table_cell_nested_inline_sequence_for_children(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        available_width: f32,
    ) -> Option<TableCellNestedInlineSequencePlan> {
        let mut items = Vec::new();
        self.collect_inline_box_items(
            children,
            stylesheets,
            inherited_link,
            0.0,
            style,
            style.text_decoration,
            &mut items,
        );
        (!items.is_empty()).then(|| TableCellNestedInlineSequencePlan {
            sequence: self.collect_inline_line_sequence(
                items,
                style,
                available_width.max(1.0),
                0.0,
                0.0,
            ),
            style: style.clone(),
        })
    }

    fn table_cell_child_fragment_plans(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_width: f32,
        mut child_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) -> Vec<TableCellChildFragmentPlan> {
        let mut plans = Vec::new();
        for (source_child_index, child_box) in children.iter().enumerate() {
            let inline_sequence = self.table_cell_nested_inline_sequence_for_child(
                child_box,
                stylesheets,
                available_width,
            );
            if inline_sequence.is_none() && !table_cell_has_in_flow_layout_child(child_box) {
                continue;
            }
            let child_height = inline_sequence
                .as_ref()
                .map(|plan| plan.sequence.total_height())
                .unwrap_or_else(|| table_cell_formatting_child_slice_height(child_box));
            if child_height <= 0.0 {
                continue;
            }
            let child_bottom = child_top - child_height;
            if child_top >= slice_bottom
                && child_bottom <= slice_top
                && let Some(kind) = table_cell_child_fragment_kind(child_box)
            {
                let visible_top = child_top.min(slice_top);
                let visible_bottom = child_bottom.max(slice_bottom);
                let mut metadata = FragmentPageMetadata::new(
                    self.pages.len(),
                    Some(
                        PageTopRect::new(
                            0.0,
                            visible_top,
                            available_width,
                            visible_top - visible_bottom,
                        )
                        .paint_clip(),
                    ),
                    (child_top - slice_top).abs() <= 0.01,
                );
                metadata.continues_from_previous_page = child_top > slice_top + 0.01;
                metadata.continues_to_next_page = child_bottom < slice_bottom - 0.01;
                plans.push(TableCellChildFragmentPlan {
                    source_child_index,
                    child_top,
                    child_height,
                    slice_top,
                    slice_bottom,
                    kind,
                    inline_sequence,
                    nested_fragment: None,
                    metadata,
                });
            }
            child_top = child_bottom;
        }
        plans
    }

    /// Pre-render a nested table/flex formatting context for split table-cell
    /// replay.
    ///
    /// CSS Fragmentation clips the table row piece, but the nested formatting
    /// context itself must keep its internal paint order and effects. Planning
    /// the child into an off-page fragment lets paint replay only translate and
    /// clip the selected page-local slice:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn plan_table_cell_nested_child_fragment(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        child_box: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> Option<TableCellNestedFragmentPlan> {
        if !matches!(
            child_box,
            box_tree::FormattingBox::Table(_) | box_tree::FormattingBox::Flex(_)
        ) {
            return None;
        }

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        self.ancestors = self.table_cell_child_ancestors(cell, row);
        let width = available_width.max(1.0);
        let top = 10_000.0;
        self.current_page = Page::new(width, top);
        self.overflow_clips.clear();
        self.content_left = 0.0;
        self.content_right = width;
        self.cursor_y = top;
        self.truncate_page_start_margins = false;

        self.layout_formatting_box(child_box, stylesheets);
        self.flush_positioned_layers_since(positioned_layer_start);

        let fragment = self
            .current_page
            .paint_fragment()
            .translated(PaintVector::new(0.0, -top));
        let height = (top - self.cursor_y).max(0.0);
        self.restore(snapshot);

        (!fragment.is_empty()).then_some(TableCellNestedFragmentPlan {
            fragment,
            width,
            height,
            metadata: FragmentPageMetadata::empty(self.pages.len()),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_table_cell_planned_child_fragments(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        content_offset: f32,
        content_x_offset: f32,
        child_fragments: &[TableCellChildFragmentPlan],
    ) {
        let Some(children) = cell.children.as_deref() else {
            return;
        };
        if child_fragments.is_empty() {
            return;
        }

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_ancestors = self.ancestors.clone();
        let borders = used_border_widths(cell_style);
        let content_box = border_box.content_box(
            placement,
            cell_style.padding,
            borders,
            content_offset,
            content_x_offset,
        );
        self.content_left = content_box.left();
        self.content_right = content_box.right();
        self.cursor_y = content_box.top_y();
        self.ancestors = self.table_cell_child_ancestors(cell, row);

        for child_plan in child_fragments {
            if let Some(child_box) = children.get(child_plan.source_child_index) {
                self.paint_table_cell_planned_child_slice(child_box, stylesheets, child_plan);
            }
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.ancestors = previous_ancestors;
    }

    fn paint_table_cell_planned_child_slice(
        &mut self,
        child_box: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        child_plan: &TableCellChildFragmentPlan,
    ) {
        let child_top = child_plan.child_top;
        let child_height = child_plan.child_height;
        let slice_top = child_plan.slice_top;
        let slice_bottom = child_plan.slice_bottom;
        if let Some(inline_sequence) = &child_plan.inline_sequence {
            self.paint_table_cell_nested_inline_sequence_slice(
                inline_sequence,
                child_top,
                slice_top,
                slice_bottom,
            );
            return;
        }
        match child_plan.kind {
            TableCellChildFragmentKind::Block => {
                let box_tree::FormattingBox::Block(box_) = child_box else {
                    return;
                };
                self.paint_table_cell_element_child_slice(
                    box_.element,
                    &box_.style,
                    &box_.children,
                    stylesheets,
                    child_top,
                    child_height,
                    slice_top,
                    slice_bottom,
                );
            }
            TableCellChildFragmentKind::AnonymousBlock => {
                let box_tree::FormattingBox::AnonymousBlock(box_) = child_box else {
                    return;
                };
                self.paint_table_cell_anonymous_child_slice(
                    &box_.style,
                    &box_.children,
                    child_top,
                    slice_top,
                    slice_bottom,
                    stylesheets,
                );
            }
            TableCellChildFragmentKind::Inline => {
                let box_tree::FormattingBox::Inline(box_) = child_box else {
                    return;
                };
                self.paint_table_cell_anonymous_child_slice(
                    &box_.style,
                    &box_.children,
                    child_top,
                    slice_top,
                    slice_bottom,
                    stylesheets,
                );
            }
            TableCellChildFragmentKind::Text => {
                let box_tree::FormattingBox::Text(box_) = child_box else {
                    return;
                };
                let text = normalized_text_for_style(&box_.text, &box_.style);
                if !text.is_empty() {
                    self.paint_text_block_slice(
                        &text,
                        &box_.style,
                        0.0,
                        0.0,
                        None,
                        child_top,
                        slice_top,
                        slice_bottom,
                    );
                }
            }
            TableCellChildFragmentKind::AtomicInline => {
                let box_tree::FormattingBox::AtomicInline(box_) = child_box else {
                    return;
                };
                if replaced_element_kind(box_.element) == Some(ReplacedElementKind::Svg) {
                    self.paint_table_cell_replaced_child_slice(
                        box_.element,
                        &box_.style,
                        child_top,
                        child_height,
                    );
                } else {
                    self.paint_table_cell_element_child_slice(
                        box_.element,
                        &box_.style,
                        &box_.children,
                        stylesheets,
                        child_top,
                        child_height,
                        slice_top,
                        slice_bottom,
                    );
                }
            }
            TableCellChildFragmentKind::Replaced => {
                let box_tree::FormattingBox::Replaced(box_) = child_box else {
                    return;
                };
                self.paint_table_cell_replaced_child_slice(
                    box_.element,
                    &box_.style,
                    child_top,
                    child_height,
                );
            }
            TableCellChildFragmentKind::NestedFormattingContext => {
                self.paint_table_cell_nested_child_fragment(child_plan);
            }
        }
    }

    fn paint_table_cell_nested_inline_sequence_slice(
        &mut self,
        inline_sequence: &TableCellNestedInlineSequencePlan,
        child_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) {
        self.paint_inline_line_sequence_slice(
            &inline_sequence.sequence,
            &inline_sequence.style,
            child_top,
            slice_top,
            slice_bottom,
        );
    }

    fn paint_table_cell_nested_child_fragment(&mut self, child_plan: &TableCellChildFragmentPlan) {
        let Some(nested) = &child_plan.nested_fragment else {
            return;
        };
        let slice_height = (child_plan.slice_top - child_plan.slice_bottom).max(0.0);
        if slice_height <= 0.0 {
            return;
        }

        let x = self.content_left;
        let translated = nested
            .fragment
            .clone()
            .translated(PaintVector::new(x, child_plan.child_top));
        let bounds =
            PageTopRect::new(x, child_plan.child_top, nested.width, nested.height).paint_clip();
        let slice_clip = PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(x, child_plan.slice_bottom),
            PaintSize::new(nested.width, slice_height),
        ));
        let context = PaintStackingContext::from_banded_fragment(translated, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(PaintEffects {
                overflow_clip: Some(slice_clip),
                absolute_clip: Some(slice_clip),
                ..PaintEffects::default()
            })
            .with_bounds(bounds);
        let fragment =
            PaintFragment::from_stacking_context_in_band(PaintBand::InFlowBlock, context);
        self.current_page
            .append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_table_cell_element_child_slice(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        child_top: f32,
        child_height: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return;
        }
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let outer_width = (self.content_right - self.content_left).max(0.0);
        let border_box_top = child_top - style.margin.top;
        let border_box_height = (child_height - style.margin.top - style.margin.bottom).max(0.0);
        let bounds = PageTopRect::new(
            self.content_left + style.margin.left,
            border_box_top,
            outer_width - style.margin.left - style.margin.right,
            border_box_height,
        )
        .paint_clip();
        if border_box_height > 0.0 && style.visibility == Visibility::Visible {
            for primitive in self.box_background_primitives(
                bounds.x(),
                bounds.y(),
                bounds.width(),
                bounds.height(),
                style,
            ) {
                self.push_primitive_in_band(PaintBand::BackgroundBorder, primitive);
            }
        }
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let borders = used_border_widths(style);
        self.content_left += style.margin.left + borders.left + style.padding.left;
        self.content_right -= style.margin.right + borders.right + style.padding.right;
        let content_width = (self.content_right - self.content_left).max(1.0);
        if let Some(sequence) = self.table_cell_nested_inline_sequence_for_children(
            style,
            children,
            stylesheets,
            element.attrs.get("href").cloned(),
            content_width,
        ) {
            let text_top = border_box_top - borders.top - style.padding.top;
            self.paint_table_cell_nested_inline_sequence_slice(
                &sequence,
                text_top,
                slice_top,
                slice_bottom,
            );
        }
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            bounds,
            style,
            Vec::new(),
        );
    }

    fn paint_table_cell_replaced_child_slice(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        child_top: f32,
        child_height: f32,
    ) {
        if matches!(style.position, Position::Absolute | Position::Fixed)
            || style.visibility != Visibility::Visible
        {
            return;
        }
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let outer_width = (self.content_right - self.content_left).max(0.0);
        let border_box_top = child_top - style.margin.top;
        let border_box_height = (child_height - style.margin.top - style.margin.bottom).max(0.0);
        let bounds = PageTopRect::new(
            self.content_left + style.margin.left,
            border_box_top,
            outer_width - style.margin.left - style.margin.right,
            border_box_height,
        )
        .paint_clip();
        for primitive in self.box_background_primitives(
            bounds.x(),
            bounds.y(),
            bounds.width(),
            bounds.height(),
            style,
        ) {
            self.push_primitive_in_band(PaintBand::BackgroundBorder, primitive);
        }

        if let Some((width, height, fill)) = svg_rect(element) {
            let borders = used_border_widths(style);
            let x = self.content_left + style.margin.left + borders.left + style.padding.left;
            let y_top = border_box_top - borders.top - style.padding.top;
            self.push_rect_in_band(
                PaintBand::Inline,
                PageTopRect::new(x, y_top, width, height).rendered_rect(Some(fill)),
            );
        }
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            bounds,
            style,
            Vec::new(),
        );
    }

    fn paint_table_cell_anonymous_child_slice(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        child_top: f32,
        slice_top: f32,
        slice_bottom: f32,
        stylesheets: &[Stylesheet],
    ) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        if let Some(sequence) = self.table_cell_nested_inline_sequence_for_children(
            style,
            children,
            stylesheets,
            None,
            available_width,
        ) {
            self.paint_table_cell_nested_inline_sequence_slice(
                &sequence,
                child_top,
                slice_top,
                slice_bottom,
            );
        }
        for child_plan in self.table_cell_child_fragment_plans(
            children,
            stylesheets,
            available_width,
            child_top,
            slice_top,
            slice_bottom,
        ) {
            let child = &children[child_plan.source_child_index];
            if matches!(
                child,
                box_tree::FormattingBox::Replaced(_)
                    | box_tree::FormattingBox::Table(_)
                    | box_tree::FormattingBox::Flex(_)
            ) {
                self.paint_table_cell_planned_child_slice(child, stylesheets, &child_plan);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn table_body_fragment_structural_primitives(
        &mut self,
        rows: &[TableRow<'_>],
        columns: &[TableColumn<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        table_width: UsedTableWidth,
        table_metrics: TableMetrics,
        column_plan: &TableColumnPlan,
        fragment: &TableBodyPaintFragment,
    ) -> (Vec<PaintPrimitive>, Vec<PaintPrimitive>) {
        let top = fragment.plan.fragment_top;
        let bottom = fragment.bottom();
        let height = (top - bottom).max(0.0);
        let mut backgrounds = Vec::new();
        let mut outlines = Vec::new();
        if height <= 0.0 {
            return (backgrounds, outlines);
        }

        if let Some(fill) = table_style.background_color {
            let vertical_edge_spacing = table_metrics.spacing.vertical;
            let background_top = top
                + vertical_edge_spacing
                + table_width.padding.top
                + table_width.border_widths.top;
            let background_bottom = bottom
                - vertical_edge_spacing
                - table_width.padding.bottom
                - table_width.border_widths.bottom;
            backgrounds.push(PaintPrimitive::Rect(
                PageTopRect::new(
                    table_x - table_width.padding.left - table_width.border_widths.left,
                    background_top,
                    used_table_width
                        + table_width.padding.left
                        + table_width.padding.right
                        + table_width.border_widths.left
                        + table_width.border_widths.right,
                    background_top - background_bottom,
                )
                .rendered_rect(Some(fill)),
            ));
        }
        for (start_column, end_column, column_group) in
            table_column_group_spans(columns, column_plan.column_count())
        {
            let column_group_style =
                self.style_for_table_column_group(&column_group, table_style, stylesheets);
            if let Some(rect) = table_column_background_rect(
                table_x,
                top,
                height,
                column_plan,
                start_column,
                end_column,
                column_group_style.background_color,
            ) {
                backgrounds.push(PaintPrimitive::Rect(rect));
            }
        }
        let mut column_index = 0;
        for column in columns {
            if column_index >= column_plan.column_count() {
                break;
            }
            let span = column
                .span
                .min(column_plan.column_count() - column_index)
                .max(1);
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            if let Some(rect) = table_column_background_rect(
                table_x,
                top,
                height,
                column_plan,
                column_index,
                column_index + span,
                column_style.background_color,
            ) {
                backgrounds.push(PaintPrimitive::Rect(rect));
            }
            column_index += span;
        }

        let fragment_rows = fragment.rows();
        let fragment_row_tops = fragment.row_tops();
        let fragment_row_heights = fragment.row_heights();
        let occupied_inline_bounds = column_plan
            .occupied_inline_bounds()
            .unwrap_or_else(|| TableInlineBounds::new(0.0, used_table_width));
        let occupied_x = table_x + occupied_inline_bounds.start;
        let occupied_width = occupied_inline_bounds.size;

        for (start_row, end_row, row_group) in table_row_group_spans(rows) {
            let row_group_style =
                self.style_for_table_row_group(&row_group, table_style, stylesheets);
            if let Some(fill) = row_group_style.background_color {
                let mut segment_start = None;
                let mut previous_local = None;
                for (local_row, original_row) in fragment_rows.iter().copied().enumerate() {
                    if original_row >= start_row && original_row < end_row {
                        if segment_start.is_none() {
                            segment_start = Some(local_row);
                        }
                        previous_local = Some(local_row + 1);
                    } else if let (Some(start), Some(end)) =
                        (segment_start.take(), previous_local.take())
                    {
                        push_table_fragment_row_span_background(
                            &mut backgrounds,
                            occupied_x,
                            occupied_width,
                            &fragment_row_tops,
                            &fragment_row_heights,
                            start,
                            end,
                            fill,
                        );
                    }
                }
                if let (Some(start), Some(end)) = (segment_start, previous_local) {
                    push_table_fragment_row_span_background(
                        &mut backgrounds,
                        occupied_x,
                        occupied_width,
                        &fragment_row_tops,
                        &fragment_row_heights,
                        start,
                        end,
                        fill,
                    );
                }
            }
            if row_group_style.visibility == Visibility::Visible {
                self.push_table_fragment_row_group_outline_segments(
                    &mut outlines,
                    occupied_x,
                    occupied_width,
                    &fragment_row_tops,
                    &fragment_row_heights,
                    &fragment_rows,
                    start_row,
                    end_row,
                    &row_group_style,
                );
            }
        }

        for (local_row, original_row) in fragment_rows.iter().copied().enumerate() {
            let row_style = self.style_for_table_row(&rows[original_row], table_style, stylesheets);
            if let Some(fill) = row_style.background_color {
                push_table_fragment_row_span_background(
                    &mut backgrounds,
                    occupied_x,
                    occupied_width,
                    &fragment_row_tops,
                    &fragment_row_heights,
                    local_row,
                    local_row + 1,
                    fill,
                );
            }
        }
        (backgrounds, outlines)
    }

    #[allow(clippy::too_many_arguments)]
    fn push_table_fragment_row_group_outline_segments(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        table_x: f32,
        used_table_width: f32,
        row_tops: &[f32],
        row_heights: &[f32],
        rows: &[usize],
        start_row: usize,
        end_row: usize,
        row_group_style: &ComputedStyle,
    ) {
        let mut segment_start = None;
        let mut previous_local = None;
        for (local_row, original_row) in rows.iter().copied().enumerate() {
            if original_row >= start_row && original_row < end_row {
                if segment_start.is_none() {
                    segment_start = Some(local_row);
                }
                previous_local = Some(local_row + 1);
            } else if let (Some(start), Some(end)) = (segment_start.take(), previous_local.take()) {
                self.push_table_fragment_row_span_outline(
                    primitives,
                    table_x,
                    used_table_width,
                    row_tops,
                    row_heights,
                    start,
                    end,
                    row_group_style,
                );
            }
        }
        if let (Some(start), Some(end)) = (segment_start, previous_local) {
            self.push_table_fragment_row_span_outline(
                primitives,
                table_x,
                used_table_width,
                row_tops,
                row_heights,
                start,
                end,
                row_group_style,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_table_fragment_row_span_outline(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        table_x: f32,
        used_table_width: f32,
        row_tops: &[f32],
        row_heights: &[f32],
        start: usize,
        end: usize,
        row_group_style: &ComputedStyle,
    ) {
        let Some(bounds) = table_fragment_row_span_bounds(
            table_x,
            used_table_width,
            row_tops,
            row_heights,
            start,
            end,
        ) else {
            return;
        };
        primitives.extend(self.box_outline_primitives(
            bounds.x(),
            bounds.y(),
            bounds.width(),
            bounds.height(),
            row_group_style,
        ));
    }

    /// Build collapsed borders for one generated table row fragment.
    ///
    /// CSS 2.2 centers collapsed borders on grid lines, while CSS
    /// Fragmentation requires each page fragment to paint its own visible table
    /// piece. Resolving a page-local grid keeps body-row collapsed borders on
    /// the same durable paint-tree page fragment as the rows they border:
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    #[allow(clippy::too_many_arguments)]
    fn collapsed_table_fragment_border_primitives(
        &mut self,
        geometry: &CollapsedTableGeometry,
        table_x: f32,
        column_plan: &TableColumnPlan,
        fragment: &TableBodyPaintFragment,
    ) -> Vec<PaintPrimitive> {
        let rows = fragment.rows();
        let row_tops = fragment.row_tops();
        let row_heights = fragment.row_heights();
        let row_offsets = fragment.row_offsets();
        let original_row_heights = fragment.original_row_heights();
        let placement = TableGridPlacement::new(PageTopPoint::new(table_x, 0.0));
        let (rects, paths) = geometry.grid.paint_fragment_rows(
            placement,
            column_plan,
            &rows,
            &row_tops,
            &row_heights,
            &row_offsets,
            &original_row_heights,
        );
        rects
            .into_iter()
            .map(PaintPrimitive::Rect)
            .chain(paths.into_iter().map(PaintPrimitive::Path))
            .collect()
    }

    fn measure_table_row_height(
        &mut self,
        context: &TableGridLayoutContext<'_, '_>,
        row_index: usize,
        row_style: &ComputedStyle,
    ) -> f32 {
        let row = &context.rows[row_index];
        let placements = &context.grid.rows[row_index];
        let mut row_height: f32 =
            used_length_percentage_or_auto_with_optional_basis(row_style.box_values.height, None)
                .unwrap_or(0.0);
        let mut max_baseline: f32 = 0.0;
        let mut max_after_baseline: f32 = 0.0;
        let mut has_baseline_cells = false;
        for placement in placements {
            let cell = &row.cells[placement.cell];
            let Some(prepared) = self.prepare_table_cell(
                cell,
                row,
                row_style,
                placement,
                row_index,
                0.0,
                context.stylesheets,
                context.table_cellpadding,
                context.column_plan,
                context.table_metrics,
                context.collapsed_geometry,
            ) else {
                continue;
            };
            if placement.rowspan == 1 {
                row_height = row_height.max(prepared.metrics.border_box_height);
            }
            if table_cell_participates_in_row_baseline(&prepared.style, row_style, placement) {
                has_baseline_cells = true;
                let baseline = prepared.metrics.baseline_offset;
                max_baseline = max_baseline.max(baseline);
                max_after_baseline = max_after_baseline
                    .max((prepared.metrics.border_box_height - baseline).max(0.0));
            }
        }
        if has_baseline_cells {
            row_height = row_height.max(max_baseline + max_after_baseline);
        }
        row_height
    }

    fn measure_table_row_heights(&mut self, context: &TableGridLayoutContext<'_, '_>) -> Vec<f32> {
        self.table_height_plan(context)
            .rows
            .into_iter()
            .map(|row| row.final_height)
            .collect()
    }

    /// Build the CSS Tables 3 height distribution plan for a table grid.
    ///
    /// The planner keeps row baselines tied to the first pass, grows row spans
    /// before final distribution, resolves explicit and percentage row,
    /// row-group, and cell constraints into reference sizes, then assigns final
    /// row heights before pagination and painting consume the row list.
    ///
    /// Spec: <https://drafts.csswg.org/css-tables-3/#row-layout>,
    /// <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>,
    /// and <https://drafts.csswg.org/css-tables-3/#table-cell-content-layout-second-pass>.
    fn table_height_plan(&mut self, context: &TableGridLayoutContext<'_, '_>) -> TableHeightPlan {
        // CSS Tables 3 row layout first computes minimum row sizes, applies
        // spanning-cell minimum constraints, then distributes any definite
        // table height against reference sizes:
        // <https://drafts.csswg.org/css-tables-3/#row-layout> and
        // <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>.
        let mut plan_rows = Vec::with_capacity(context.rows.len());
        let mut spanning_cells = Vec::new();
        for (row_index, row) in context.rows.iter().enumerate() {
            let row_style = self.style_for_table_row(row, context.table_style, context.stylesheets);
            if table_row_is_collapsed(&row_style) {
                plan_rows.push(TableRowHeightPlan {
                    base: 0.0,
                    reference: 0.0,
                    final_height: 0.0,
                    auto: false,
                    collapsed: true,
                });
                continue;
            }
            if self.table_row_is_hidden_empty(
                row,
                &context.grid.rows[row_index],
                &row_style,
                context.stylesheets,
                context.table_cellpadding,
                context.column_plan,
                context.table_metrics,
            ) {
                plan_rows.push(TableRowHeightPlan {
                    base: 0.0,
                    reference: 0.0,
                    final_height: 0.0,
                    auto: false,
                    collapsed: true,
                });
                continue;
            }
            let base = self.measure_table_row_height(context, row_index, &row_style);
            plan_rows.push(TableRowHeightPlan {
                base,
                reference: base,
                final_height: base,
                auto: row_style.box_values.height.is_auto(),
                collapsed: false,
            });
            for placement in &context.grid.rows[row_index] {
                if placement.rowspan > 1 {
                    let cell = &row.cells[placement.cell];
                    let Some(prepared) = self.prepare_table_cell(
                        cell,
                        row,
                        &row_style,
                        placement,
                        row_index,
                        0.0,
                        context.stylesheets,
                        context.table_cellpadding,
                        context.column_plan,
                        context.table_metrics,
                        context.collapsed_geometry,
                    ) else {
                        continue;
                    };
                    spanning_cells.push((
                        row_index,
                        placement.rowspan,
                        prepared.metrics.border_box_height,
                    ));
                }
            }
        }

        for (row_index, rowspan, required_height) in spanning_cells {
            distribute_table_span_constraint(
                &mut plan_rows,
                row_index,
                rowspan,
                required_height,
                context.table_metrics,
                TableHeightTarget::Base,
            );
        }
        for row in &mut plan_rows {
            row.reference = row.base;
            row.final_height = row.base;
        }

        let target_content_height = self.resolve_table_target_content_height(context.table_style);
        self.compute_table_reference_heights(&mut plan_rows, context, target_content_height);
        self.distribute_table_height_plan(
            &mut plan_rows,
            target_content_height,
            context.table_metrics,
        );

        TableHeightPlan { rows: plan_rows }
    }

    fn resolve_table_target_content_height(&self, table_style: &ComputedStyle) -> Option<f32> {
        let vertical_non_content = if table_style.border_collapse == css::BorderCollapse::Collapse {
            0.0
        } else {
            let border_widths = used_border_widths(table_style);
            table_style.padding.top
                + table_style.padding.bottom
                + border_widths.top
                + border_widths.bottom
        };
        used_table_target_content_height(table_style, self.page_area_height(), vertical_non_content)
    }

    fn compute_table_reference_heights(
        &mut self,
        plan_rows: &mut [TableRowHeightPlan],
        context: &TableGridLayoutContext<'_, '_>,
        target_content_height: Option<f32>,
    ) {
        if plan_rows.is_empty() {
            return;
        }

        for (row_index, row) in context.rows.iter().enumerate() {
            if plan_rows[row_index].collapsed {
                continue;
            }
            let row_style = self.style_for_table_row(row, context.table_style, context.stylesheets);
            if let Some(row_height) = used_length_percentage_or_auto_with_optional_basis(
                row_style.box_values.height,
                target_content_height,
            ) {
                plan_rows[row_index].reference = plan_rows[row_index].reference.max(row_height);
            }
            for placement in &context.grid.rows[row_index] {
                let cell = &row.cells[placement.cell];
                let Some(prepared) = self.prepare_table_cell(
                    cell,
                    row,
                    &row_style,
                    placement,
                    row_index,
                    0.0,
                    context.stylesheets,
                    context.table_cellpadding,
                    context.column_plan,
                    context.table_metrics,
                    context.collapsed_geometry,
                ) else {
                    continue;
                };
                let required_height = self.table_cell_border_box_height_with_basis(
                    &prepared.style,
                    prepared.metrics.content_height,
                    target_content_height,
                    context.column_plan.total_width(),
                    prepared.borders,
                );
                distribute_table_span_constraint(
                    plan_rows,
                    row_index,
                    placement.rowspan,
                    required_height,
                    context.table_metrics,
                    TableHeightTarget::Reference,
                );
            }
        }

        let groups = table_height_distribution_groups(context.rows);
        for (start, end) in groups {
            let Some(row_group) = context.rows[start].row_groups.last() else {
                continue;
            };
            let row_group_style =
                self.style_for_table_row_group(row_group, context.table_style, context.stylesheets);
            let Some(group_height) = used_length_percentage_or_auto_with_optional_basis(
                row_group_style.box_values.height,
                target_content_height,
            ) else {
                continue;
            };
            distribute_table_span_constraint(
                plan_rows,
                start,
                end - start,
                group_height,
                context.table_metrics,
                TableHeightTarget::Reference,
            );
        }
    }

    fn table_cell_border_box_height_with_basis(
        &self,
        style: &ComputedStyle,
        content_height: f32,
        percentage_basis: Option<f32>,
        width_basis: f32,
        border_insets: css::Edges,
    ) -> f32 {
        let vertical_non_content =
            style.padding.top + style.padding.bottom + border_insets.top + border_insets.bottom;
        let requested_content = used_content_height_or_auto_with_optional_basis(
            style,
            percentage_basis,
            vertical_non_content,
        )
        .unwrap_or(0.0)
        .max(content_height);
        constrain_height(style, requested_content, width_basis) + vertical_non_content
    }

    fn distribute_table_height_plan(
        &self,
        rows: &mut [TableRowHeightPlan],
        target_content_height: Option<f32>,
        table_metrics: TableMetrics,
    ) {
        for row in rows.iter_mut() {
            row.final_height = row.base;
        }
        let Some(target_content_height) = target_content_height else {
            for row in rows.iter_mut() {
                row.final_height = row.reference;
            }
            return;
        };

        let base = table_content_height_from_plan(rows, TableHeightTarget::Base, table_metrics);
        let reference =
            table_content_height_from_plan(rows, TableHeightTarget::Reference, table_metrics);
        if target_content_height <= base + 0.01 {
            return;
        }

        if target_content_height <= reference + 0.01 {
            let extra = target_content_height - base;
            let capacity = rows
                .iter()
                .filter(|row| !row.collapsed)
                .map(|row| (row.reference - row.base).max(0.0))
                .sum::<f32>();
            if capacity <= 0.01 {
                distribute_table_height_extra(rows, extra, |row| !row.collapsed);
                return;
            }
            for row in rows.iter_mut().filter(|row| !row.collapsed) {
                let share = (row.reference - row.base).max(0.0) / capacity;
                row.final_height = row.base + extra * share;
            }
            return;
        }

        for row in rows.iter_mut() {
            row.final_height = row.reference;
        }
        let extra = target_content_height - reference;
        let distributed =
            distribute_table_height_extra(rows, extra, |row| !row.collapsed && row.auto);
        if distributed <= 0.0 {
            distribute_table_height_extra(rows, extra, |row| !row.collapsed);
        }
    }

    /// Return whether a separated-border row has only hidden empty cells.
    ///
    /// CSS 2.2 `empty-cells: hide` suppresses empty cell borders/backgrounds
    /// in separated-border tables; if all cells in a row are hidden and empty,
    /// the row has zero height and vertical border spacing on only one side.
    /// https://www.w3.org/TR/CSS22/tables.html#empty-cells
    #[allow(clippy::too_many_arguments)]
    pub(super) fn table_row_is_hidden_empty(
        &mut self,
        row: &TableRow<'_>,
        placements: &[TableCellPlacement],
        row_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
    ) -> bool {
        if table_metrics.border_collapse == css::BorderCollapse::Collapse || placements.is_empty() {
            return false;
        }

        let mut saw_visible_column_cell = false;
        for placement in placements {
            let cell_width = column_plan.width_for_span(placement.column, placement.colspan);
            if cell_width <= 0.0 {
                continue;
            }
            saw_visible_column_cell = true;
            let cell = &row.cells[placement.cell];
            let mut cell_style = self.style_for_table_cell(cell, row, row_style, stylesheets);
            apply_table_cell_used_padding(&mut cell_style, table_cellpadding, cell_width);
            let cell_is_empty = table_cell_inline_text(cell).is_empty()
                && self.table_cell_non_text_content_height(
                    cell,
                    stylesheets,
                    (cell_width
                        - cell_style.padding.left
                        - cell_style.padding.right
                        - table_horizontal_borders(&cell_style))
                    .max(0.0),
                ) <= 0.0;
            if cell_style.empty_cells == EmptyCells::Show || !cell_is_empty {
                return false;
            }
        }
        saw_visible_column_cell
    }

    pub(super) fn estimate_table_captions_height(
        &mut self,
        captions: &[TableCaption<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_width: f32,
        side: CaptionSide,
    ) -> f32 {
        captions
            .iter()
            .filter_map(|caption| {
                let caption_style = self.style_for_table_caption(caption, table_style, stylesheets);
                (caption_style.caption_side == side).then(|| {
                    self.estimate_element_height(
                        caption.element,
                        &caption_style,
                        stylesheets,
                        table_width,
                        caption.children.as_deref(),
                    )
                    .unwrap_or(0.0)
                })
            })
            .sum()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn table_row_baseline_offset(
        &mut self,
        row_index: usize,
        row: &TableRow<'_>,
        placements: &[TableCellPlacement],
        row_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> Option<f32> {
        // CSS 2.2 table height layout aligns cells with `vertical-align:
        // baseline` by the baseline of their first in-flow line box. This
        // lightweight fragment model uses the first text baseline approximation
        // already used for PDF text placement.
        placements
            .iter()
            .filter_map(|placement| {
                let cell = &row.cells[placement.cell];
                let prepared = self.prepare_table_cell(
                    cell,
                    row,
                    row_style,
                    placement,
                    row_index,
                    0.0,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics,
                    collapsed_geometry,
                )?;
                if !table_cell_participates_in_row_baseline(&prepared.style, row_style, placement) {
                    return None;
                }
                Some(prepared.metrics.baseline_offset)
            })
            .reduce(f32::max)
    }

    fn table_cell_row_baseline_offset_for_alignment(
        &mut self,
        context: &TableCellBaselineAlignmentContext<'_>,
        placement: &TableCellPlacement,
        cell_style: &ComputedStyle,
    ) -> Option<f32> {
        if !table_cell_participates_in_baseline(cell_style, context.row_style) {
            return None;
        }
        if table_cell_alignment_baseline_set(cell_style) == TableCellBaselineSet::First {
            return context.row_baseline_offset;
        }
        let target_row_index = (context.row_index + placement.rowspan.saturating_sub(1))
            .min(context.rows.len().saturating_sub(1));
        if target_row_index == context.row_index {
            return context.row_baseline_offset;
        }
        let target_row = context.rows.get(target_row_index)?;
        let target_row_style =
            self.style_for_table_row(target_row, context.table_style, context.stylesheets);
        let target_baseline = self.table_row_baseline_offset(
            target_row_index,
            target_row,
            context.grid.rows.get(target_row_index)?,
            &target_row_style,
            context.stylesheets,
            context.table_cellpadding,
            context.column_plan,
            context.table_metrics,
            context.collapsed_geometry,
        )?;
        let origin_top = table_row_top(
            0.0,
            context.planned_row_heights,
            context.table_metrics,
            context.row_index,
        );
        let target_top = table_row_top(
            0.0,
            context.planned_row_heights,
            context.table_metrics,
            target_row_index,
        );
        Some((origin_top - target_top).max(0.0) + target_baseline)
    }

    #[allow(clippy::too_many_arguments)]
    fn table_row_baseline_only_offset(
        &mut self,
        row_index: usize,
        row: &TableRow<'_>,
        placements: &[TableCellPlacement],
        row_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> Option<f32> {
        placements
            .iter()
            .filter_map(|placement| {
                let cell = &row.cells[placement.cell];
                let prepared = self.prepare_table_cell(
                    cell,
                    row,
                    row_style,
                    placement,
                    row_index,
                    0.0,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics,
                    collapsed_geometry,
                )?;
                if !table_cell_participates_in_row_baseline(&prepared.style, row_style, placement) {
                    return None;
                }
                let available_width = (prepared.width()
                    - prepared.style.padding.left
                    - prepared.style.padding.right
                    - prepared.borders.left
                    - prepared.borders.right)
                    .max(0.0);
                self.table_cell_alignment_baseline_offset(
                    cell,
                    &prepared.style,
                    stylesheets,
                    available_width,
                    prepared.borders,
                )
            })
            .reduce(f32::max)
    }

    /// Measure the content, border box, and table-cell baseline for row layout.
    ///
    /// CSS 2.2 and CSS Tables align table cells by the first in-flow line-box
    /// baseline or first in-flow table-row baseline in the cell; if neither
    /// exists, the baseline is the bottom content edge:
    /// <https://www.w3.org/TR/CSS22/tables.html#height-layout> and
    /// <https://drafts.csswg.org/css-tables-3/#row-layout>.
    #[allow(clippy::too_many_arguments)]
    fn prepare_table_cell(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        placement: &TableCellPlacement,
        row_index: usize,
        _table_x: f32,
        stylesheets: &[Stylesheet],
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
    ) -> Option<PreparedTableCell> {
        let mut style = self.style_for_table_cell(cell, row, row_style, stylesheets);
        let area = TableGridArea::from_placement(row_index, placement);
        let inline_bounds = column_plan.inline_bounds_for_area(area);
        let width = inline_bounds.size;
        if width <= 0.0 {
            return None;
        }
        apply_table_cell_used_padding(&mut style, table_cellpadding, width);
        let borders = table_cell_border_insets(
            &style,
            placement,
            row_index,
            table_metrics,
            collapsed_geometry,
        );
        let metrics = self.table_cell_layout_metrics(cell, &style, stylesheets, width, borders);
        let text = table_cell_inline_text(cell);
        Some(PreparedTableCell {
            style,
            area,
            inline_bounds,
            borders,
            metrics,
            text,
        })
    }

    fn table_cell_layout_metrics(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        cell_width: f32,
        border_insets: css::Edges,
    ) -> TableCellLayoutMetrics {
        let available_width = (cell_width
            - cell_style.padding.left
            - cell_style.padding.right
            - border_insets.left
            - border_insets.right)
            .max(0.0);
        let text_height = self.table_cell_text_content_height(cell, cell_style, available_width);
        let content_height = text_height.max(self.table_cell_non_text_content_height(
            cell,
            stylesheets,
            available_width,
        ));
        let border_box_height =
            table_cell_border_box_height_with_insets(cell_style, content_height, border_insets);
        let baseline_offset = self
            .table_cell_alignment_baseline_offset(
                cell,
                cell_style,
                stylesheets,
                available_width,
                border_insets,
            )
            .unwrap_or_else(|| {
                self.table_cell_content_bottom_baseline(cell_style, content_height, border_insets)
            });

        TableCellLayoutMetrics {
            content_height,
            border_box_height,
            baseline_offset,
        }
    }

    /// Measure non-text content for table row sizing using durable child boxes.
    ///
    /// CSS table row height depends on the minimum height required by cell
    /// content. Nested table boxes already carry a table fragment with row,
    /// caption, spacing, and span information, so row sizing must reuse the
    /// table measurement path instead of approximating from the child list:
    /// <https://www.w3.org/TR/CSS22/tables.html#height-layout> and
    /// <https://drafts.csswg.org/css-tables-3/#row-layout>.
    fn table_cell_non_text_content_height(
        &mut self,
        cell: &TableCell<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        let fallback = table_cell_non_text_content_height(cell);
        let Some(children) = cell.children.as_deref() else {
            return fallback;
        };

        fallback.max(self.table_cell_children_non_text_content_height(
            children,
            stylesheets,
            available_width,
        ))
    }

    fn table_cell_children_non_text_content_height(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        let mut height = 0.0_f32;
        let mut inline_line_height = 0.0_f32;

        for child in children {
            if let Some(inline_height) = table_cell_measured_inline_outer_height(child) {
                inline_line_height = inline_line_height.max(inline_height);
                continue;
            }
            if inline_line_height > 0.0 {
                height += inline_line_height;
                inline_line_height = 0.0;
            }
            height +=
                self.table_cell_measured_block_child_height(child, stylesheets, available_width);
        }

        height + inline_line_height
    }

    fn table_cell_measured_block_child_height(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        match child {
            box_tree::FormattingBox::Table(box_) => {
                if matches!(box_.style.position, Position::Absolute | Position::Fixed) {
                    return 0.0;
                }
                self.estimate_table_height(
                    box_.element,
                    &box_.style,
                    stylesheets,
                    available_width,
                    &box_.fragment,
                )
            }
            box_tree::FormattingBox::Block(box_) => self.table_cell_measured_element_child_height(
                box_.element,
                &box_.style,
                &box_.children,
                stylesheets,
                available_width,
                child,
            ),
            box_tree::FormattingBox::Flex(box_) => self.table_cell_measured_element_child_height(
                box_.element,
                &box_.style,
                &box_.children,
                stylesheets,
                available_width,
                child,
            ),
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .table_cell_children_non_text_content_height(
                    &box_.children,
                    stylesheets,
                    available_width,
                ),
            _ => table_cell_formatting_child_outer_height(child),
        }
    }

    fn table_cell_measured_element_child_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_width: f32,
        fallback_child: &box_tree::FormattingBox<'_>,
    ) -> f32 {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return 0.0;
        }
        self.estimate_element_height(element, style, stylesheets, available_width, Some(children))
            .unwrap_or_else(|| table_cell_formatting_child_outer_height(fallback_child))
    }

    fn table_cell_content_bottom_baseline(
        &self,
        cell_style: &ComputedStyle,
        content_height: f32,
        border_insets: css::Edges,
    ) -> f32 {
        border_insets.top + cell_style.padding.top + content_height
    }

    fn table_cell_baseline_offset(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        border_insets: css::Edges,
    ) -> Option<f32> {
        if let Some(children) = cell.children.as_deref() {
            return self
                .table_cell_children_first_baseline_offset(
                    children,
                    cell_style,
                    stylesheets,
                    available_width,
                )
                .map(|baseline| border_insets.top + cell_style.padding.top + baseline);
        }

        (!table_cell_inline_text(cell).is_empty())
            .then(|| self.table_cell_first_baseline_offset(cell_style))
    }

    fn table_cell_alignment_baseline_offset(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        border_insets: css::Edges,
    ) -> Option<f32> {
        match table_cell_alignment_baseline_set(cell_style) {
            TableCellBaselineSet::First => self.table_cell_baseline_offset(
                cell,
                cell_style,
                stylesheets,
                available_width,
                border_insets,
            ),
            TableCellBaselineSet::Last => self.table_cell_last_baseline_offset(
                cell,
                cell_style,
                stylesheets,
                available_width,
                border_insets,
            ),
        }
    }

    fn table_cell_last_baseline_offset(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        border_insets: css::Edges,
    ) -> Option<f32> {
        if let Some(children) = cell.children.as_deref() {
            return self
                .table_cell_children_baseline_offset(
                    children,
                    cell_style,
                    stylesheets,
                    available_width,
                    TableCellBaselineSet::Last,
                )
                .map(|baseline| border_insets.top + cell_style.padding.top + baseline);
        }

        let text = table_cell_inline_text(cell);
        (!text.is_empty()).then(|| {
            border_insets.top
                + cell_style.padding.top
                + self.table_cell_text_last_baseline_offset(&text, cell_style, available_width)
        })
    }

    fn table_cell_children_first_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        containing_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> Option<f32> {
        self.table_cell_children_baseline_offset(
            children,
            containing_style,
            stylesheets,
            available_width,
            TableCellBaselineSet::First,
        )
    }

    fn table_cell_children_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        containing_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<f32> {
        if formatting_boxes_have_textual_baseline(children)
            && !has_non_inline_formatting_box(children)
        {
            return self.table_cell_inline_content_baseline_offset(
                children,
                containing_style,
                stylesheets,
                available_width,
                baseline_set,
            );
        }

        let mut block_offset = 0.0_f32;
        let mut last_baseline = None;

        for child in children {
            if !table_cell_has_in_flow_layout_child(child) {
                continue;
            }
            let child_baseline = self.table_cell_child_baseline_offset(
                child,
                stylesheets,
                available_width,
                baseline_set,
            );
            if let Some(baseline) = child_baseline {
                let baseline = block_offset + baseline;
                if baseline_set == TableCellBaselineSet::First {
                    return Some(baseline);
                }
                last_baseline = Some(baseline);
            }
            block_offset += table_cell_formatting_child_outer_height(child);
        }

        last_baseline
    }

    fn table_cell_child_baseline_offset(
        &mut self,
        child: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<f32> {
        match child {
            box_tree::FormattingBox::Text(box_) => {
                (!box_tree::formatting_box_is_collapsible_space(child)).then(|| {
                    self.table_cell_text_baseline_offset(
                        &box_.text,
                        &box_.style,
                        available_width,
                        baseline_set,
                    )
                })
            }
            box_tree::FormattingBox::Line(box_) => (!box_.children.is_empty()).then(|| {
                self.table_cell_text_baseline_offset("", &box_.style, available_width, baseline_set)
            }),
            box_tree::FormattingBox::Inline(box_) => self.inline_children_baseline_offset(
                &box_.children,
                &box_.style,
                stylesheets,
                available_width,
                baseline_set,
            ),
            box_tree::FormattingBox::AnonymousBlock(box_) => self
                .table_cell_children_baseline_offset(
                    &box_.children,
                    &box_.style,
                    stylesheets,
                    available_width,
                    baseline_set,
                ),
            box_tree::FormattingBox::Block(box_) => self.block_child_baseline_offset(
                &box_.style,
                &box_.children,
                stylesheets,
                available_width,
                baseline_set,
            ),
            box_tree::FormattingBox::Flex(box_) => self.block_child_baseline_offset(
                &box_.style,
                &box_.children,
                stylesheets,
                available_width,
                baseline_set,
            ),
            box_tree::FormattingBox::Table(box_) => self
                .table_fragment_baseline_offset(
                    box_.element,
                    &box_.style,
                    &box_.fragment,
                    stylesheets,
                    available_width,
                )
                .map(|baseline| box_.style.margin.top + baseline),
            box_tree::FormattingBox::AtomicInline(_) | box_tree::FormattingBox::Replaced(_) => None,
        }
    }

    fn inline_children_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        inline_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<f32> {
        self.table_cell_inline_content_baseline_offset(
            children,
            inline_style,
            stylesheets,
            available_width,
            baseline_set,
        )
    }

    fn block_child_baseline_offset(
        &mut self,
        block_style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<f32> {
        if matches!(block_style.position, Position::Absolute | Position::Fixed) {
            return None;
        }
        let borders = used_border_widths(block_style);
        self.table_cell_children_baseline_offset(
            children,
            block_style,
            stylesheets,
            available_width,
            baseline_set,
        )
        .map(|baseline| block_style.margin.top + borders.top + block_style.padding.top + baseline)
    }

    fn table_cell_inline_content_baseline_offset(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> Option<f32> {
        if !formatting_box_has_inline_content(children) {
            return None;
        }

        let measurement = self.intrinsic_inline_measurement_for_boxes(
            children,
            style,
            stylesheets,
            available_width,
        );
        let line_count = measurement.line_count();
        if line_count == 0 {
            return None;
        }

        let first_baseline = self.font_system.rendered_first_line_baseline_offset(style);
        Some(match baseline_set {
            TableCellBaselineSet::First => first_baseline,
            TableCellBaselineSet::Last => {
                first_baseline
                    + measurement
                        .sequence
                        .records
                        .iter()
                        .take(line_count.saturating_sub(1))
                        .map(|line| line.line_height)
                        .sum::<f32>()
            }
        })
    }

    fn table_fragment_baseline_offset(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        fragment: &box_tree::TableFragment<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> Option<f32> {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return None;
        }

        let input = TableLayoutInput::from_fragment(fragment);
        let rows = input.rows.as_slice();
        let table_width = used_table_width(style, available_width.max(style.font_size));
        let table_cellpadding = element
            .attrs
            .get("cellpadding")
            .and_then(|value| parse_html_length(value));
        let table_metrics = table_metrics(element, style);
        let grid = table_grid(rows);
        let column_plan = self.table_column_plan(
            rows,
            &grid,
            style,
            stylesheets,
            &input.columns,
            table_width.content_width,
            !style.box_values.width.is_auto(),
            table_cellpadding,
            table_metrics,
        );
        let collapsed_geometry = (table_metrics.border_collapse == css::BorderCollapse::Collapse)
            .then(|| {
                self.collapsed_table_geometry(
                    rows,
                    &grid,
                    style,
                    stylesheets,
                    &input.columns,
                    column_plan.column_count(),
                )
            });
        let top_caption_height = self.estimate_table_captions_height(
            &input.captions,
            style,
            stylesheets,
            column_plan.total_width(),
            CaptionSide::Top,
        );
        let table_context = TableGridLayoutContext {
            rows,
            grid: &grid,
            table_style: style,
            stylesheets,
            table_cellpadding,
            column_plan: &column_plan,
            table_metrics,
            collapsed_geometry: collapsed_geometry.as_ref(),
        };

        let Some(row_index) = rows.iter().enumerate().find_map(|(index, row)| {
            let row_style = self.style_for_table_row(row, style, stylesheets);
            if table_row_is_collapsed(&row_style) || grid.rows[index].is_empty() {
                return None;
            }
            if self.table_row_is_hidden_empty(
                row,
                &grid.rows[index],
                &row_style,
                stylesheets,
                table_cellpadding,
                &column_plan,
                table_metrics,
            ) {
                return None;
            }
            Some(index)
        }) else {
            return Some(
                table_width.border_widths.top + table_width.padding.top + top_caption_height,
            );
        };

        let row_style = self.style_for_table_row(&rows[row_index], style, stylesheets);
        let row_baseline = self
            .table_row_baseline_only_offset(
                row_index,
                &rows[row_index],
                &grid.rows[row_index],
                &row_style,
                stylesheets,
                table_cellpadding,
                &column_plan,
                table_metrics,
                collapsed_geometry.as_ref(),
            )
            .unwrap_or_else(|| {
                self.measure_table_row_height(&table_context, row_index, &row_style)
            });

        Some(
            top_caption_height
                + table_width.border_widths.top
                + table_width.padding.top
                + table_metrics.spacing.vertical
                + row_baseline,
        )
    }

    /// Returns the first rendered text baseline offset from a table cell border-box top.
    ///
    /// CSS 2.2 aligns `vertical-align: baseline` table cells by the baseline of
    /// their first in-flow line box. Text painting applies the selected font's
    /// ascender correction, so table layout must use the same metric:
    /// <https://www.w3.org/TR/CSS22/tables.html#height-layout>.
    pub(super) fn table_cell_first_baseline_offset(&mut self, style: &ComputedStyle) -> f32 {
        let borders = used_border_widths(style);
        borders.top
            + style.padding.top
            + self.font_system.rendered_first_line_baseline_offset(style)
    }

    fn table_cell_text_baseline_offset(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        baseline_set: TableCellBaselineSet,
    ) -> f32 {
        match baseline_set {
            TableCellBaselineSet::First => {
                self.font_system.rendered_first_line_baseline_offset(style)
            }
            TableCellBaselineSet::Last => {
                self.table_cell_text_last_baseline_offset(text, style, available_width)
            }
        }
    }

    fn table_cell_text_last_baseline_offset(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> f32 {
        let line_count = if text.is_empty() {
            1
        } else {
            let line_height = style.line_height.max(0.01);
            (self.estimate_text_height(text, style, available_width, 0.0, 0.0) / line_height)
                .ceil()
                .max(1.0) as usize
        };
        self.font_system.rendered_first_line_baseline_offset(style)
            + line_count.saturating_sub(1) as f32 * style.line_height
    }

    /// Measure text content height for a table cell using durable child styles.
    ///
    /// CSS Tables 3 computes row heights from cell content, while
    /// `display: contents` can place inherited raw text boxes inside anonymous
    /// table cells whose generated cell style is not the text style:
    /// <https://drafts.csswg.org/css-tables-3/#row-layout> and
    /// <https://www.w3.org/TR/css-display-3/#valdef-display-contents>.
    pub(super) fn table_cell_text_content_height(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        available_width: f32,
    ) -> f32 {
        if let Some(children) = cell.children.as_deref() {
            return self.table_cell_children_text_content_height(children, available_width);
        }

        let text = table_cell_inline_text(cell);
        if text.is_empty() {
            0.0
        } else {
            self.estimate_text_height(&text, cell_style, available_width, 0.0, 0.0)
        }
    }

    fn table_cell_children_text_content_height(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        available_width: f32,
    ) -> f32 {
        let mut height = 0.0_f32;
        let mut inline_line_height = 0.0_f32;

        for child in children {
            match child {
                box_tree::FormattingBox::Text(box_) => {
                    inline_line_height = inline_line_height.max(self.estimate_text_height(
                        &box_.text,
                        &box_.style,
                        available_width,
                        0.0,
                        0.0,
                    ));
                }
                box_tree::FormattingBox::Inline(box_) => {
                    inline_line_height =
                        inline_line_height.max(self.table_cell_children_text_content_height(
                            &box_.children,
                            available_width,
                        ));
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    if inline_line_height > 0.0 {
                        height += inline_line_height;
                        inline_line_height = 0.0;
                    }
                    height += self
                        .table_cell_children_text_content_height(&box_.children, available_width);
                }
                _ => {
                    if inline_line_height > 0.0 {
                        height += inline_line_height;
                        inline_line_height = 0.0;
                    }
                }
            }
        }

        height + inline_line_height
    }

    /// Return a content clip for cells spanning collapsed row tracks.
    ///
    /// CSS Tables 3 says cells crossing collapsed rows are rendered as though
    /// the content in the collapsed track is clipped away while the cell still
    /// participates in the remaining visible rows:
    /// <https://drafts.csswg.org/css-tables-3/#visibility-collapse-cell-rendering>.
    #[allow(clippy::too_many_arguments)]
    fn collapsed_rowspan_cell_content_clip(
        &self,
        row_index: usize,
        rowspan: usize,
        rows: &[TableRow<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        row_heights: &[f32],
        table_metrics: TableMetrics,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
    ) -> Option<OverflowClip> {
        if rowspan <= 1 {
            return None;
        }
        let end = (row_index + rowspan).min(rows.len());
        let spans_collapsed_row = rows[row_index + 1..end].iter().any(|row| {
            table_row_is_collapsed(&self.style_for_table_row(row, table_style, stylesheets))
        });
        if !spans_collapsed_row {
            return None;
        }

        let visible_height = table_row_span_height(row_heights, row_index, rowspan, table_metrics);
        let rect = TableGridRect::new(
            border_box.rect().origin,
            TableGridSize::new(border_box.width(), visible_height.max(0.0)),
        );
        Some(placement.overflow_clip_for(rect))
    }

    pub(super) fn layout_table_captions(
        &mut self,
        captions: &[TableCaption<'_>],
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_x: f32,
        table_width: f32,
        side: CaptionSide,
    ) {
        for caption in captions {
            let mut caption_style = self.style_for_table_caption(caption, table_style, stylesheets);
            if caption_style.caption_side != side || caption_style.display.is_none() {
                continue;
            }
            let caption_available_width = if has_auto_width(&caption_style) {
                set_style_used_width(&mut caption_style, table_width);
                table_width
            } else {
                let horizontal_non_content = caption_style.padding.left
                    + caption_style.padding.right
                    + horizontal_border_width(&caption_style);
                let caption_content_width =
                    used_content_width_or_auto(&caption_style, table_width, horizontal_non_content)
                        .unwrap_or(table_width);
                table_width.max(
                    caption_style.margin.left
                        + caption_content_width
                        + horizontal_non_content
                        + caption_style.margin.right,
                )
            };
            let previous_left = self.content_left;
            let previous_right = self.content_right;
            self.content_left = table_x;
            self.content_right = table_x + caption_available_width;
            self.push_float_context();
            if let Some(children) = caption.children.as_deref() {
                self.layout_element_box(
                    caption.element,
                    &caption_style,
                    stylesheets,
                    caption.signature.clone(),
                    &[],
                    children,
                );
            } else {
                self.layout_element(caption.element, &caption_style, stylesheets);
            }
            self.pop_float_context();
            self.content_left = previous_left;
            self.content_right = previous_right;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_table_column_background(
        &mut self,
        table_x: f32,
        grid_top: f32,
        grid_height: f32,
        column_plan: &TableColumnPlan,
        start_column: usize,
        end_column: usize,
        fill: Option<Color>,
    ) {
        if let Some(rect) = table_column_background_rect(
            table_x,
            grid_top,
            grid_height,
            column_plan,
            start_column,
            end_column,
            fill,
        ) {
            self.push_rect_in_band(PaintBand::InFlowBlock, rect);
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Resolve the full collapsed-border geometry for one table grid.
    ///
    /// CSS 2.2 collapsed border conflict resolution produces a single grid of
    /// winning borders. The table wrapper consumes the outer half-widths, and
    /// fragmented paint later samples the same full grid.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
    fn collapsed_table_geometry(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        columns: &[TableColumn<'_>],
        column_count: usize,
    ) -> CollapsedTableGeometry {
        let collapsed_columns =
            self.collapsed_table_columns(columns, table_style, stylesheets, column_count);
        let mut collapsed_grid =
            CollapsedBorderGrid::new(rows.len(), column_count, TableAxes::for_style(table_style));
        collapsed_grid.add_table(table_style, rows.len(), column_count);
        let mut first_displayed_row = None;
        for (row_index, row) in rows.iter().enumerate() {
            let row_style = self.style_for_table_row(row, table_style, stylesheets);
            if table_row_is_collapsed(&row_style) {
                continue;
            }
            first_displayed_row.get_or_insert(row_index);
            for placement in &grid.rows[row_index] {
                if placement.column >= column_count {
                    break;
                }
                let cell = &row.cells[placement.cell];
                let cell_style = self.style_for_table_cell(cell, row, &row_style, stylesheets);
                collapsed_grid.add_cell(
                    row_index,
                    placement.column,
                    placement.colspan,
                    placement.rowspan,
                    &cell_style,
                );
            }
            collapsed_grid.add_row(row_index, column_count, &row_style);
        }
        for (start_row, end_row, row_group) in table_row_group_spans(rows) {
            let row_group_style =
                self.style_for_table_row_group(&row_group, table_style, stylesheets);
            collapsed_grid.add_row_group(start_row, end_row, column_count, &row_group_style);
        }
        for (start_column, end_column, column_group) in
            table_column_group_spans(columns, column_count)
        {
            let Some((visible_start, visible_end)) =
                visible_column_span(start_column, end_column, &collapsed_columns)
            else {
                continue;
            };
            let column_group_style =
                self.style_for_table_column_group(&column_group, table_style, stylesheets);
            collapsed_grid.add_column_group(
                visible_start,
                visible_end,
                rows.len(),
                &column_group_style,
            );
        }
        let mut column_index = 0;
        for column in columns {
            if column_index >= column_count {
                break;
            }
            let span = column.span.min(column_count - column_index).max(1);
            let start_column = column_index;
            let end_column = column_index + span;
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            if let Some((visible_start, visible_end)) =
                visible_column_span(start_column, end_column, &collapsed_columns)
            {
                collapsed_grid.add_column(visible_start, visible_end, rows.len(), &column_style);
            }
            column_index += span;
        }

        let outer_insets =
            collapsed_grid.outer_insets_for_first_displayed_row(first_displayed_row.unwrap_or(0));
        CollapsedTableGeometry {
            grid: collapsed_grid,
            outer_insets,
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Resolve the wrapper insets contributed by collapsed outer table borders.
    ///
    /// CSS 2.2 collapsed border conflict resolution produces grid-edge border
    /// winners before table wrapper layout consumes the outer half-widths as
    /// used table border insets.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
    pub(super) fn collapsed_border_outer_insets(
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

    #[allow(clippy::too_many_arguments)]
    /// Collect CSS Tables column measures before final width distribution.
    ///
    /// CSS Tables 3 computes min-content widths, max-content widths,
    /// intrinsic percentage widths, and constrainedness before resolving the
    /// table's used width:
    /// <https://drafts.csswg.org/css-tables-3/#computing-column-measures>.
    fn table_column_measures(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        columns: &[TableColumn<'_>],
        table_width: f32,
        table_cellpadding: Option<f32>,
        table_metrics: TableMetrics,
    ) -> TableColumnMeasures {
        let column_count = grid.column_count;
        let collapsed_columns =
            self.collapsed_table_columns(columns, table_style, stylesheets, column_count);
        let visible_columns = collapsed_columns
            .iter()
            .filter(|collapsed| !**collapsed)
            .count();
        let total_horizontal_spacing =
            table_displayed_horizontal_spacing(visible_columns, table_metrics);
        let padding_basis = (table_width - total_horizontal_spacing).max(table_style.font_size);
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
                apply_table_column_style_measures(&mut measures, column_index, span, &group_style);
            }
            let column_style = self.style_for_table_column(column, table_style, stylesheets);
            apply_table_column_style_measures(&mut measures, column_index, span, &column_style);
            column_index += span;
        }

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
                apply_table_cell_used_padding(&mut cell_style, table_cellpadding, padding_basis);

                let explicit_width = cell
                    .element
                    .and_then(|element| declared_table_cell_width(element, &cell_style));
                let min_content_width =
                    table_cell_content_min_width(self, cell, &cell_style, stylesheets);
                let max_content_width =
                    table_cell_content_max_width(self, cell, &cell_style, stylesheets);
                let width_floor = explicit_width
                    .map(declared_table_width_length_floor)
                    .unwrap_or(0.0);
                let min_target_width = constrain_table_intrinsic_width_with_floor(
                    &cell_style,
                    min_content_width,
                    width_floor,
                );
                let max_target_width = constrain_table_intrinsic_width_with_floor(
                    &cell_style,
                    max_content_width.max(min_target_width),
                    width_floor,
                );
                let percentage = intrinsic_percentage_contribution(&cell_style).max(
                    explicit_width
                        .map(declared_table_width_percentage)
                        .unwrap_or(0.0),
                );

                for index in placement.column..end {
                    measures.occupied[index] = true;
                }
                if colspan == 1 {
                    measures.min_content_widths[placement.column] =
                        measures.min_content_widths[placement.column].max(min_target_width);
                    measures.max_content_widths[placement.column] =
                        measures.max_content_widths[placement.column].max(max_target_width);
                    measures.intrinsic_percentages[placement.column] =
                        measures.intrinsic_percentages[placement.column].max(percentage);
                    if explicit_width.is_some_and(declared_table_width_is_non_percentage) {
                        measures.constrained[placement.column] = true;
                    }
                } else {
                    let internal_spacing = table_internal_horizontal_spacing(
                        placement.column,
                        end,
                        &collapsed_columns,
                        table_metrics,
                    );
                    distribute_spanned_percentage(&mut measures, placement.column, end, percentage);
                    distribute_spanned_measure(
                        &mut measures,
                        placement.column,
                        end,
                        (min_target_width - internal_spacing).max(0.0),
                        true,
                    );
                    distribute_spanned_measure(
                        &mut measures,
                        placement.column,
                        end,
                        (max_target_width - internal_spacing).max(0.0),
                        false,
                    );
                }
            }
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
    fn resolve_table_used_content_width(
        &mut self,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        columns: &[TableColumn<'_>],
        available_outer_width: f32,
        table_cellpadding: Option<f32>,
        table_metrics: TableMetrics,
        table_width: &mut UsedTableWidth,
    ) {
        let measures = self.table_column_measures(
            rows,
            grid,
            table_style,
            stylesheets,
            columns,
            table_width.content_width,
            table_cellpadding,
            table_metrics,
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
        let mut content_width = table_width.content_width;
        if let Some(width) = intrinsic::intrinsic_width_keyword(
            table_style.box_values.width,
            min_content,
            max_content,
            available_outer_width,
            horizontal_non_content,
        ) {
            content_width = constrain_width(table_style, width, available_outer_width)
                .max(table_style.font_size);
        }
        content_width =
            table_content_width_clamped_to_min_content(table_style, content_width, min_content);
        table_width.content_width = content_width;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn table_column_plan(
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
            table_metrics,
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
            table_metrics.spacing.horizontal,
            collapsed_columns,
            TableAxes::for_style(table_style),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn fixed_table_column_plan(
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
            table_displayed_horizontal_spacing(visible_columns, table_metrics);
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
            if let Some(width) = declared_table_column_width(&column_style) {
                let width =
                    constrain_declared_table_width(&column_style, width, content_table_width);
                distribute_fixed_width(&mut widths, &mut declared, column_index, span, width);
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
                    content_table_width,
                );
                if let Some(explicit_width) = cell
                    .element
                    .and_then(|element| declared_table_cell_width(element, &cell_style))
                {
                    let width = constrain_declared_table_width(
                        &cell_style,
                        explicit_width,
                        content_table_width,
                    );
                    let width = if colspan > 1 {
                        let end = (placement.column + colspan).min(collapsed_columns.len());
                        let internal_spacing = table_internal_horizontal_spacing(
                            placement.column,
                            end,
                            &collapsed_columns,
                            table_metrics,
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
            table_metrics.spacing.horizontal,
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
    pub(super) fn collapsed_table_columns(
        &self,
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

    pub(super) fn style_for_table_row(
        &self,
        row: &TableRow<'_>,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> ComputedStyle {
        if let Some(style) = &row.style {
            return style.clone();
        }
        let mut ancestors = self.ancestors.clone();
        ancestors.extend(row.ancestors.iter().cloned());
        let parent_style = row
            .row_groups
            .last()
            .map(|group| self.style_for_table_row_group(group, table_style, stylesheets))
            .unwrap_or_else(|| table_style.clone());
        if let Some(element) = row.element {
            style_for_layout_element(
                element,
                row.signature.clone(),
                stylesheets,
                Some(&parent_style),
                &ancestors,
            )
        } else {
            css::style_for_element_with_signature(
                row.signature.clone(),
                None,
                stylesheets,
                Some(&parent_style),
                &ancestors,
            )
        }
    }

    pub(super) fn style_for_table_row_group(
        &self,
        row_group: &TableRowGroup<'_>,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> ComputedStyle {
        if let Some(style) = &row_group.style {
            return style.clone();
        }
        style_for_layout_element(
            row_group.element,
            row_group.signature.clone(),
            stylesheets,
            Some(table_style),
            &self.ancestors,
        )
    }

    pub(super) fn style_for_table_column(
        &self,
        column: &TableColumn<'_>,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> ComputedStyle {
        if let Some(style) = &column.style {
            return style.clone();
        }
        let mut ancestors = self.ancestors.clone();
        let parent_style = if let Some(group) = &column.group {
            let group_style = self.style_for_table_column_group(group, table_style, stylesheets);
            ancestors.push(group.signature.clone());
            group_style
        } else {
            table_style.clone()
        };
        style_for_layout_element(
            column.element,
            column.signature.clone(),
            stylesheets,
            Some(&parent_style),
            &ancestors,
        )
    }

    pub(super) fn style_for_table_column_group(
        &self,
        group: &TableColumnGroup<'_>,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> ComputedStyle {
        if let Some(style) = &group.style {
            return style.clone();
        }
        style_for_layout_element(
            group.element,
            group.signature.clone(),
            stylesheets,
            Some(table_style),
            &self.ancestors,
        )
    }

    pub(super) fn style_for_table_cell(
        &self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> ComputedStyle {
        if cell.anonymous {
            let mut style = row_style.clone();
            style.display = Display::TABLE_CELL;
            style.margin = css::Edges::ZERO;
            style.padding = css::Edges::ZERO;
            style.border_width = 0.0;
            style.border_widths = css::Edges::ZERO;
            style.border_styles = css::BorderStyles::NONE;
            style.background_color = None;
            set_style_auto_width(&mut style);
            set_style_auto_height(&mut style);
            style.box_values.min_width = css::ComputedLengthPercentageOrAuto::Auto;
            style.box_values.max_width = css::ComputedLengthPercentageOrAuto::Auto;
            style.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
            style.box_values.max_height = css::ComputedLengthPercentageOrAuto::Auto;
            return style;
        }
        let mut ancestors = self.ancestors.clone();
        ancestors.extend(row.ancestors.iter().cloned());
        ancestors.push(row.signature.clone());
        if let Some(element) = cell.element {
            style_for_layout_element(
                element,
                cell.signature.clone(),
                stylesheets,
                Some(row_style),
                &ancestors,
            )
        } else {
            css::style_for_element_with_signature(
                cell.signature.clone(),
                None,
                stylesheets,
                Some(row_style),
                &ancestors,
            )
        }
    }

    pub(super) fn style_for_table_caption(
        &self,
        caption: &TableCaption<'_>,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> ComputedStyle {
        if let Some(style) = &caption.style {
            return style.clone();
        }
        style_for_layout_element(
            caption.element,
            caption.signature.clone(),
            stylesheets,
            Some(table_style),
            &self.ancestors,
        )
    }

    fn table_cell_content_x_offset(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        cell_width: f32,
        border_insets: css::Edges,
    ) -> f32 {
        if cell_style.writing_mode == WritingMode::HorizontalTb
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
        let subject_width = (table_cell_content_max_width(self, cell, cell_style, stylesheets)
            - border_insets.left
            - border_insets.right
            - cell_style.padding.left
            - cell_style.padding.right)
            .max(0.0);
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

    pub(super) fn layout_table_cell_replaced_children(
        &mut self,
        cell: &TableCell<'_>,
        cell_style: &ComputedStyle,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        content_offset: f32,
        content_x_offset: f32,
    ) {
        let borders = used_border_widths(cell_style);
        let content_box = border_box.content_box(
            placement,
            cell_style.padding,
            borders,
            content_offset,
            content_x_offset,
        );
        let content_bounds = content_box.page_top_rect();
        let mut x = content_bounds.x;
        let y_top = content_bounds.top_y;
        if let Some(children) = cell.children.as_deref() {
            for child_box in children {
                let Some((child, _, _, _)) = child_box.element_parts() else {
                    continue;
                };
                if replaced_element_kind(child) == Some(ReplacedElementKind::Svg)
                    && let Some((width, height, fill)) = svg_rect(child)
                {
                    if cell_style.visibility == Visibility::Visible {
                        self.push_rect(
                            PageTopRect::new(x, y_top, width, height).rendered_rect(Some(fill)),
                        );
                    }
                    x += width;
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
                && let Some((width, height, fill)) = svg_rect(child)
            {
                self.push_rect(PageTopRect::new(x, y_top, width, height).rendered_rect(Some(fill)));
                x += width;
            }
        }
    }

    /// Lay out in-flow block descendants inside a table cell content box.
    ///
    /// CSS 2.2 says a table-cell box contains a block container, and its
    /// in-flow descendants therefore participate in normal block formatting
    /// inside the cell after row and column sizing:
    /// <https://www.w3.org/TR/CSS22/tables.html#model>.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn layout_table_cell_flow_children(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
        content_offset: f32,
        content_x_offset: f32,
    ) {
        let Some(children) = cell.children.as_deref() else {
            return;
        };
        if !children.iter().any(table_cell_has_in_flow_layout_child) {
            return;
        }

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_ancestors = self.ancestors.clone();
        let borders = used_border_widths(cell_style);
        let content_box = border_box.content_box(
            placement,
            cell_style.padding,
            borders,
            content_offset,
            content_x_offset,
        );
        self.content_left = content_box.left();
        self.content_right = content_box.right();
        self.cursor_y = content_box.top_y();
        self.ancestors = self.table_cell_child_ancestors(cell, row);
        let cell_content_height = content_box.height();

        self.push_float_context();
        self.definite_block_size_stack
            .push(Some(cell_content_height));
        if formatting_box_has_inline_content(children) && !has_non_inline_formatting_box(children) {
            self.layout_anonymous_block(cell_style, children, stylesheets, None);
        } else {
            for child_box in children {
                if table_cell_has_in_flow_layout_child(child_box) {
                    self.layout_formatting_box(child_box, stylesheets);
                }
            }
        }
        self.definite_block_size_stack.pop();
        self.pop_float_context();

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.ancestors = previous_ancestors;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn layout_table_cell_positioned_children(
        &mut self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
        cell_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
    ) {
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_ancestors = self.ancestors.clone();
        let child_ancestors = self.table_cell_child_ancestors(cell, row);
        let containing_block_pushed =
            self.push_table_cell_containing_block_if_positioned(cell_style, border_box, placement);

        self.ancestors = child_ancestors.clone();
        if let Some(children) = cell.children.as_deref() {
            for child_box in children {
                let Some((child, child_signature, child_style, child_children)) =
                    child_box.element_parts()
                else {
                    continue;
                };
                if !matches!(child_style.position, Position::Absolute | Position::Fixed) {
                    continue;
                }
                self.push_ancestor_signature(child_signature.clone());
                self.layout_element_with_child_boxes(
                    child,
                    child_style,
                    stylesheets,
                    Some(child_children),
                );
                self.ancestors.pop();
            }
        } else if let Some(element) = cell.element {
            let sibling_tags = element_sibling_tags(element);
            let mut element_index = 0usize;
            for child in &element.children {
                let NodeKind::Element(child_element) = &child.kind else {
                    continue;
                };
                let child_signature = ElementSignature::with_siblings(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = style_for_layout_element(
                    child_element,
                    child_signature.clone(),
                    stylesheets,
                    Some(cell_style),
                    &child_ancestors,
                );
                if !matches!(child_style.position, Position::Absolute | Position::Fixed) {
                    continue;
                }
                self.push_ancestor_signature(child_signature);
                self.layout_element(child_element, &child_style, stylesheets);
                self.ancestors.pop();
            }
        }

        if containing_block_pushed {
            self.containing_blocks.pop();
        }
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.ancestors = previous_ancestors;
    }

    pub(super) fn push_table_cell_containing_block_if_positioned(
        &mut self,
        cell_style: &ComputedStyle,
        border_box: TableCellBorderBox,
        placement: TableGridPlacement,
    ) -> bool {
        if !matches!(cell_style.position, Position::Relative | Position::Sticky) {
            return false;
        }
        let borders = used_border_widths(cell_style);
        self.containing_blocks
            .push(placement.containing_block_for(border_box, borders));
        true
    }

    pub(super) fn table_cell_child_ancestors(
        &self,
        cell: &TableCell<'_>,
        row: &TableRow<'_>,
    ) -> Vec<ElementSignature> {
        let mut ancestors = self.ancestors.clone();
        ancestors.extend(row.ancestors.iter().cloned());
        ancestors.push(row.signature.clone());
        ancestors.push(cell.signature.clone());
        ancestors
    }
}

/// Return whether a cell joins its row baseline-sharing group.
///
/// CSS Box Alignment only lets first/last baseline-aligned table cells
/// participate when the cell inline axis is parallel to the table row's inline
/// axis; orthogonal cells use baseline fallback instead:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-content>.
fn table_cell_participates_in_baseline(
    cell_style: &ComputedStyle,
    row_style: &ComputedStyle,
) -> bool {
    if inline_start_side(cell_style.writing_mode, cell_style.direction).axis()
        != inline_start_side(row_style.writing_mode, row_style.direction).axis()
    {
        return false;
    }

    if cell_style.align_content.keyword != ContentAlignmentKeyword::Normal {
        return matches!(
            cell_style.align_content.keyword,
            ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline
        );
    }

    matches!(
        cell_style.vertical_align.table_cell_align,
        TableCellVerticalAlign::Baseline
    )
}

fn table_cell_participates_in_row_baseline(
    cell_style: &ComputedStyle,
    row_style: &ComputedStyle,
    placement: &TableCellPlacement,
) -> bool {
    if !table_cell_participates_in_baseline(cell_style, row_style) {
        return false;
    }
    // CSS Align assigns row-spanning cells to the start-most row for first
    // baseline alignment and to the end-most row for last baseline alignment.
    // TableGrid stores a spanning cell on its start-most row; the end-most row
    // target for last baseline is resolved later when painting the origin row.
    // <https://www.w3.org/TR/css-align-3/#baseline-align-content>.
    placement.rowspan == 1
        || table_cell_alignment_baseline_set(cell_style) == TableCellBaselineSet::First
}

fn table_cell_alignment_baseline_set(style: &ComputedStyle) -> TableCellBaselineSet {
    if style.align_content.keyword == ContentAlignmentKeyword::LastBaseline {
        TableCellBaselineSet::Last
    } else {
        TableCellBaselineSet::First
    }
}

/// Return whether inline table-cell contents expose a textual line baseline.
///
/// CSS table cells fall back to the bottom content edge when no in-flow line
/// box baseline is available; atomic inline-only content must therefore not be
/// mistaken for text-baseline content:
/// <https://www.w3.org/TR/CSS22/tables.html#height-layout>.
fn formatting_boxes_have_textual_baseline(children: &[box_tree::FormattingBox<'_>]) -> bool {
    children.iter().any(|child| match child {
        box_tree::FormattingBox::Text(_) => !box_tree::formatting_box_is_collapsible_space(child),
        box_tree::FormattingBox::Line(box_) => !box_.children.is_empty(),
        box_tree::FormattingBox::Inline(box_) => {
            formatting_boxes_have_textual_baseline(&box_.children)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            formatting_boxes_have_textual_baseline(&box_.children)
        }
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Replaced(_) => false,
    })
}

/// Return whether a table-cell child needs a nested formatting-context pass.
///
/// CSS 2.2 table cells contain a block container. Anonymous blocks that hold
/// text runs and atomic inline boxes, such as empty explicit-size
/// `inline-block` children, still create inline formatting content and must be
/// laid out after table row sizing rather than being treated as empty cells:
/// <https://www.w3.org/TR/CSS22/tables.html#model> and
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>.
fn table_cell_has_in_flow_layout_child(child_box: &box_tree::FormattingBox<'_>) -> bool {
    match child_box {
        box_tree::FormattingBox::Block(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
        }
        box_tree::FormattingBox::Table(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
        }
        box_tree::FormattingBox::Flex(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .any(table_cell_has_in_flow_layout_child),
        box_tree::FormattingBox::Inline(box_) => box_
            .children
            .iter()
            .any(table_cell_has_in_flow_layout_child),
        box_tree::FormattingBox::AtomicInline(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
        }
        box_tree::FormattingBox::Replaced(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
        }
        box_tree::FormattingBox::Line(_) => true,
        box_tree::FormattingBox::Text(_) => {
            !box_tree::formatting_box_is_collapsible_space(child_box)
        }
    }
}

fn table_cell_formatting_child_slice_height(child: &box_tree::FormattingBox<'_>) -> f32 {
    let outer_height = table_cell_formatting_child_outer_height(child);
    let descendant_visual_height = match child {
        box_tree::FormattingBox::AtomicInline(box_)
            if replaced_element_kind(box_.element) == Some(ReplacedElementKind::Svg) =>
        {
            svg_rect(box_.element)
                .map(|(_, height, _)| height + box_.style.margin.top + box_.style.margin.bottom)
                .unwrap_or(0.0)
        }
        box_tree::FormattingBox::Replaced(box_)
            if replaced_element_kind(box_.element) == Some(ReplacedElementKind::Svg) =>
        {
            svg_rect(box_.element)
                .map(|(_, height, _)| height + box_.style.margin.top + box_.style.margin.bottom)
                .unwrap_or(0.0)
        }
        box_tree::FormattingBox::Inline(box_) => box_
            .children
            .iter()
            .map(table_cell_formatting_child_slice_height)
            .fold(0.0_f32, f32::max),
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .map(table_cell_formatting_child_slice_height)
            .fold(0.0_f32, f32::max),
        _ => 0.0,
    };
    outer_height.max(descendant_visual_height)
}

fn table_cell_measured_inline_outer_height(child: &box_tree::FormattingBox<'_>) -> Option<f32> {
    match child {
        box_tree::FormattingBox::Inline(box_) => {
            if matches!(box_.style.position, Position::Absolute | Position::Fixed) {
                Some(0.0)
            } else {
                Some(table_cell_formatting_child_outer_height(child))
            }
        }
        box_tree::FormattingBox::AtomicInline(_) | box_tree::FormattingBox::Replaced(_) => {
            Some(table_cell_formatting_child_outer_height(child))
        }
        box_tree::FormattingBox::AnonymousBlock(_)
        | box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Line(_)
        | box_tree::FormattingBox::Text(_) => None,
    }
}

fn table_cell_child_fragment_kind(
    child_box: &box_tree::FormattingBox<'_>,
) -> Option<TableCellChildFragmentKind> {
    match child_box {
        box_tree::FormattingBox::Block(_) => Some(TableCellChildFragmentKind::Block),
        box_tree::FormattingBox::AnonymousBlock(_) => {
            Some(TableCellChildFragmentKind::AnonymousBlock)
        }
        box_tree::FormattingBox::Inline(_) => Some(TableCellChildFragmentKind::Inline),
        box_tree::FormattingBox::Text(_) => Some(TableCellChildFragmentKind::Text),
        box_tree::FormattingBox::AtomicInline(_) => Some(TableCellChildFragmentKind::AtomicInline),
        box_tree::FormattingBox::Replaced(_) => Some(TableCellChildFragmentKind::Replaced),
        box_tree::FormattingBox::Table(_) | box_tree::FormattingBox::Flex(_) => {
            Some(TableCellChildFragmentKind::NestedFormattingContext)
        }
        box_tree::FormattingBox::Line(_) => None,
    }
}

fn table_cell_children_can_use_inline_line_sequence(
    children: &[box_tree::FormattingBox<'_>],
) -> bool {
    children.iter().all(|child| match child {
        box_tree::FormattingBox::Text(_) | box_tree::FormattingBox::Line(_) => true,
        box_tree::FormattingBox::Inline(box_) => {
            !matches!(box_.style.position, Position::Absolute | Position::Fixed)
                && box_.style.float == Float::None
                && table_cell_children_can_use_inline_line_sequence(&box_.children)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            table_cell_children_can_use_inline_line_sequence(&box_.children)
        }
        box_tree::FormattingBox::AtomicInline(_) => false,
        box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => false,
    })
}
