use std::cell::RefCell;
use std::rc::Rc;

use super::{
    AssignmentPlacement, BlockSizePercentageBasis, BorderBoxLength, CapturedPageAssignment,
    ChildAvailableSpace, CollapsedBorderGrid, ComputedStyle, ContentBoxLength, CssColor, Direction,
    ElementSignature, ForcedBreakCarryState, FragmentAdvanceDecision, FragmentAdvanceInput,
    FragmentAvoidBoundarySide, FragmentAvoidRunStartDecision, FragmentAvoidRunStartInput,
    FragmentBreakContext, FragmentBreakOpportunity, FragmentPageMetadata, FragmentPrebreakDecision,
    FragmentPrebreakInput, FragmentSourceSliceDecision, FragmentSourceSliceInput, Fragmentainer,
    FragmentainerKind, LayoutBuilder, LayoutLength, LayoutSnapshot, LogicalBlockContentSize,
    LogicalInlineContentSize, LogicalSide, LogicalSize, NonContentLength, OverflowClip, PageBreak,
    PageContext, PageInlinePosition, PageInlineSpan, PageTopBlockPosition, PageTopPoint,
    PageTopRect, PaintBackgroundArea, PaintBand, PaintCheckpoint, PaintClip, PaintFragment,
    PaintPrimitive, PaintRect, PaintTranslation, PercentageBasis, PhysicalContentWidth,
    PhysicalSide, RenderedRect, ResourceCache, SemanticLengthExt, StackingContextPolicy,
    Stylesheets, TableAxes, TableCellBaselineOffset, TableCellBorderBox, TableCellContentBox,
    TableCellContentGeometry, TableCellPadding, TableCellPlacement, TableColumnPlan,
    TableDestinationCellGridFrame, TableFragmentainerFrame, TableGrid, TableGridArea,
    TableGridBlockOffset, TableGridContentBoxTopLeft, TableGridFrames, TableGridLength,
    TableGridLogicalSize, TableGridPlacement, TableGridPoint, TableGridRect, TableGridSize,
    TableInlineBounds, TableLayout, TableMetrics, TableRow, TableRowBaselineOffset, TableRowBounds,
    TableSourceGridFrame, TableUsedStyle, UsedOverflowAxes, UsedTableWidth, WritingMode,
    WritingModeAxes, background_rect_clip_area_for_box, border_box_pt, content_box_pt, css,
    effective_overflow_for_style, fragmented_table_root_background_image_primitives, inline_layout,
    intersect_paint_rect_or_empty, layout_pt, non_content_pt, paint_space_rect,
    percentage_basis_from_points, resolve_overflow_clip_edge,
    structural_table_background_image_primitives, table_grid_height, table_root_inline_size,
    table_row_block_start, table_row_span_height, table_vertical_edge_spacing, used_border_widths,
    used_content_box_height_or_auto_with_basis, used_length_percentage_or_auto,
    used_length_percentage_or_auto_with_basis,
};
use crate::layout::block::suppress_fragmented_box_edges;
use crate::layout::paint_ops::FragmentedDecorationSlice;

/// Physical width available to a table caption's outer border box.
///
/// Captions are siblings of the table grid in the table wrapper, so their
/// auto-width resolution uses the wrapper border-box measure rather than the
/// grid content width. Keeping that distinction in the replay API prevents
/// an empty grid from silently dropping its wrapper padding and borders.
/// <https://www.w3.org/TR/CSS22/tables.html#model>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableCaptionOuterWidth(BorderBoxLength);

impl TableCaptionOuterWidth {
    pub(in crate::layout::table) fn from_border_box(width: BorderBoxLength) -> Self {
        Self(width)
    }

    pub(in crate::layout::table) fn points(self) -> f32 {
        self.0.points()
    }
}

/// The table-wrapper frame projected into the legacy caption block-layout
/// boundary.
///
/// Captions are wrapper siblings, not table-grid children.  This composite
/// keeps their physical containing span and the table root's logical axes
/// together so a caller cannot pass an unrelated grid X coordinate to a
/// vertical caption layout entry point.
/// <https://www.w3.org/TR/CSS22/tables.html#model>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableCaptionContainingBlock {
    physical_span: PageInlineSpan,
    outer_width: TableCaptionOuterWidth,
    axes: TableAxes,
    wrapper_table_x: PageInlinePosition,
}

impl TableCaptionContainingBlock {
    pub(in crate::layout::table) fn new(
        physical_span: PageInlineSpan,
        outer_width: TableCaptionOuterWidth,
        axes: TableAxes,
        wrapper_table_x: PageInlinePosition,
    ) -> Self {
        Self {
            physical_span,
            outer_width,
            axes,
            wrapper_table_x,
        }
    }

    pub(in crate::layout::table) fn physical_span(self) -> PageInlineSpan {
        self.physical_span
    }

    pub(in crate::layout::table) fn outer_width(self) -> TableCaptionOuterWidth {
        self.outer_width
    }

    pub(in crate::layout::table) fn axes(self) -> TableAxes {
        self.axes
    }

    pub(in crate::layout::table) fn wrapper_table_x(self) -> PageInlinePosition {
        self.wrapper_table_x
    }

    /// Return the physical span which the legacy generic block entry may use
    /// as its horizontal containing-block coordinate.
    ///
    /// A horizontal table wrapper owns that span directly. A vertical table
    /// instead fragments along physical X, so replacing the active
    /// fragmentainer span with the wrapper's complete block extent would
    /// silently make a split caption fit in one column. The caller must keep
    /// the active fragmentainer bounds in that case; the table wrapper's
    /// logical inline measure is resolved independently by the caption style.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout::table) fn legacy_horizontal_span(self) -> Option<PageInlineSpan> {
        (!self.axes.flow.writing_mode().has_vertical_lines()).then_some(self.physical_span)
    }
}

/// The committed destination state of a table-caption layout pass.
///
/// Generic block layout remains responsible for laying out caption contents,
/// but a table wrapper owns the transition which follows it.
/// `final_destination` is the authoritative post-caption destination,
/// including its remaining logical block capacity. Returning it prevents the
/// wrapper from synthesizing its grid start later from a stale `table_x` and
/// a cursor that belongs to an earlier fragmentainer.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableCaptionLayoutOutcome {
    final_destination: TableFragmentainerPlacement,
    /// Retained source-local slices for caption paint.  They deliberately use
    /// wrapper-local intervals rather than table-grid offsets.
    caption_paint_slices: Vec<TableCaptionPaintSlice>,
    consumed_wrapper_interval: TableWrapperBlockInterval,
    /// The final caption exactly consumed its destination block track.  The
    /// following wrapper part must select a successor rather than inheriting
    /// an exhausted zero-width track.
    next_part_requires_successor: bool,
}

/// One retained caption slice in caption-local source coordinates.
///
/// The parent multicolumn formatter sees only the completed temporary
/// fragment to which this slice was appended.  This record remains table
/// local, preventing parent replay from interpreting caption progress as
/// table-grid source geometry.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableCaptionPaintSlice {
    pub(in crate::layout::table) page_index: usize,
    pub(in crate::layout::table) source_block_start: LayoutLength,
    pub(in crate::layout::table) block_size: LayoutLength,
    /// Table-wrapper destination selected while this source interval was
    /// consumed.  Parent multicolumn replay never reads this table-local
    /// record; it is solely the wrapper ledger's placement contract.
    pub(in crate::layout::table) destination: TableFragmentainerPlacement,
    pub(in crate::layout::table) destination_context: PageContext,
    pub(in crate::layout::table) destination_origin: PageTopPoint,
    pub(in crate::layout::table) destination_extent: LogicalSize,
    pub(in crate::layout::table) destination_block_start: LayoutLength,
}

impl TableCaptionLayoutOutcome {
    pub(in crate::layout::table) fn new(
        final_destination: TableFragmentainerPlacement,
        caption_paint_slices: Vec<TableCaptionPaintSlice>,
        consumed_wrapper_interval: TableWrapperBlockInterval,
        next_part_requires_successor: bool,
    ) -> Self {
        Self {
            final_destination,
            caption_paint_slices,
            consumed_wrapper_interval,
            next_part_requires_successor,
        }
    }

    pub(in crate::layout::table) fn final_destination(&self) -> TableFragmentainerPlacement {
        self.final_destination
    }

    pub(in crate::layout::table) fn caption_paint_slices(&self) -> &[TableCaptionPaintSlice] {
        &self.caption_paint_slices
    }

    pub(in crate::layout::table) fn consumed_wrapper_interval(&self) -> TableWrapperBlockInterval {
        self.consumed_wrapper_interval
    }

    /// Whether the next wrapper-flow part needs a fresh fragmentainer.
    pub(in crate::layout::table) fn next_part_requires_successor(&self) -> bool {
        self.next_part_requires_successor
    }
}

/// The physical top-left corner of the anonymous table wrapper's table-root
/// border box.
///
/// CSS assigns the table's border and padding to the table-root, not the
/// anonymous wrapper. This type is intentionally distinct from
/// [`TableGridContentBoxTopLeft`]: a caller must explicitly project the table
/// root's chrome before constructing a grid placement. The wrapper supplies
/// this origin to its table-root child; it is not a fragmentainer edge or a
/// grid origin.
/// <https://www.w3.org/TR/CSS22/tables.html#model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableWrapperBorderBoxOrigin(PageTopPoint);

impl TableWrapperBorderBoxOrigin {
    pub(in crate::layout::table) fn new(point: PageTopPoint) -> Self {
        Self(point)
    }

    /// Project the table root's border/padding edges to the physical top-left
    /// corner of its grid content box.
    ///
    /// The resulting point is physical because `TableGridPlacement` projects
    /// the grid's logical axes only after this box-model boundary. The
    /// physical top and left edges are not always the root's logical
    /// inline-start and block-start edges (notably in `vertical-rl` and RTL
    /// vertical text). Select those edges through [`TableAxes`] instead of
    /// assuming physical left/top chrome is logically start-side chrome.
    ///
    /// <https://www.w3.org/TR/CSS22/tables.html#model>
    /// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
    pub(in crate::layout::table) fn grid_content_box_top_left(
        self,
        axes: TableAxes,
        table_width: UsedTableWidth,
    ) -> TableGridContentBoxTopLeft {
        let chrome_for_physical_side = |side| {
            let edge = axes.grid_edge_for_physical_side(side);
            let physical_side = axes.physical_side_for_grid_edge(edge);
            match physical_side {
                PhysicalSide::Top => table_width.border_widths.top + table_width.padding.top,
                PhysicalSide::Right => table_width.border_widths.right + table_width.padding.right,
                PhysicalSide::Bottom => {
                    table_width.border_widths.bottom + table_width.padding.bottom
                }
                PhysicalSide::Left => table_width.border_widths.left + table_width.padding.left,
            }
        };
        TableGridContentBoxTopLeft::new(PageTopPoint::new(
            self.0.x() + chrome_for_physical_side(PhysicalSide::Left),
            self.0.top_y() - chrome_for_physical_side(PhysicalSide::Top),
        ))
    }
}

/// Used geometry for painting a CSS table-root grid box.
///
/// The anonymous table wrapper contains captions, while the table-root owns
/// the grid, padding, border, and associated paint areas. This record is
/// intentionally restricted to the latter; parent-flow and transform bounds
/// that include captions are constructed separately at the wrapper boundary.
/// <https://drafts.csswg.org/css-tables/#table-structure>
/// <https://drafts.csswg.org/css-tables/#drawing-backgrounds-and-borders>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableWrapperPaintBox {
    /// The complete grid's physical page origin. This is established once at
    /// the wrapper-to-grid boundary; callers must not rebase it for padding,
    /// borders, or separated-border spacing.
    pub(in crate::layout::table) grid_origin: TableGridContentBoxTopLeft,
    /// The root table flow used to project the logical grid before adding the
    /// wrapper's physical padding and border edges.
    pub(in crate::layout::table) axes: TableAxes,
    pub(in crate::layout::table) grid_size: TableGridLogicalSize,
    pub(in crate::layout::table) table_width: UsedTableWidth,
    pub(in crate::layout::table) table_metrics: TableMetrics,
    pub(in crate::layout::table) block_edge_spacing: TableGridLength,
}

impl TableWrapperPaintBox {
    pub(in crate::layout::table) fn grid_frames(self) -> TableGridFrames {
        TableGridFrames::new(
            TableGridPlacement::with_axes(self.grid_origin, self.axes, self.grid_size),
            self.block_edge_spacing,
        )
    }

    pub(in crate::layout::table) fn grid_placement(self) -> TableGridPlacement {
        self.grid_frames().wrapper_grid()
    }

    /// Return the cell-paint grid after removing the wrapper-owned outer
    /// separated-border block edges exactly once.
    pub(in crate::layout::table) fn cell_grid_placement(self) -> TableGridPlacement {
        self.grid_frames().cell_grid()
    }

    pub(in crate::layout::table) fn grid_content_box(self) -> PageTopRect {
        self.grid_placement().full_page_top_rect()
    }

    /// Return the physical top-edge coordinate used when the first body
    /// fragment is attached to the wrapper's active fragmentainer.
    ///
    /// This is always the projected grid-content edge. The table root's
    /// border and padding have already been consumed when
    /// [`TableWrapperBorderBoxOrigin::grid_content_box_top_left`] constructs
    /// this box. Re-entering a vertical grid at its border edge would apply
    /// the physical top inset a second time, separating cell paint from its
    /// background and from the wrapper's float footprint.
    ///
    /// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
    /// <https://drafts.csswg.org/css-tables-3/#positioning-cells-captions-and-other-internal-table-boxes>
    pub(in crate::layout::table) fn initial_destination_grid_paint_top(
        self,
    ) -> PageTopBlockPosition {
        PageTopBlockPosition::new(self.grid_content_box().top_y())
    }

    pub(in crate::layout::table) fn physical_grid_width(self) -> PhysicalContentWidth {
        self.grid_size.physical_width(self.axes)
    }

    /// Return the physical wrapper measure used by table captions.
    ///
    /// This is deliberately distinct from [`Self::physical_grid_width`]: the
    /// latter is a grid content-box width, while captions participate beside
    /// the grid at the wrapper border-box boundary.
    pub(in crate::layout::table) fn caption_outer_width(self) -> TableCaptionOuterWidth {
        TableCaptionOuterWidth::from_border_box(border_box_pt(self.border_box().width()))
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

/// The anonymous table-wrapper's physical flow bounds.
///
/// The wrapper has no table-root padding or border of its own. Instead, it
/// contains the table-root border box and the margin boxes of its captions.
/// Keeping this box distinct from [`TableWrapperPaintBox`] prevents transforms
/// and parent-flow consumers from mistaking the table-root paint area for the
/// wrapper's complete principal box.
/// <https://drafts.csswg.org/css-tables/#table-structure>
/// <https://drafts.csswg.org/css-tables/#positioning-cells-captions-and-other-internal-table-boxes>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableWrapperMarginBoxFootprint(PageTopRect);

impl TableWrapperMarginBoxFootprint {
    /// Construct the wrapper bounds after caption layout has established the
    /// root border box and the wrapper-owned caption extents.
    pub(in crate::layout::table) fn from_table_root_border_box(
        table_root_border_box: PageTopRect,
        wrapper_top: PageTopBlockPosition,
        top_caption_height: LayoutLength,
        bottom_caption_height: LayoutLength,
        margins: &css::Edges,
    ) -> Self {
        let border_box_height = top_caption_height.points()
            + table_root_border_box.height()
            + bottom_caption_height.points();
        Self(PageTopRect::new(
            table_root_border_box.x() - margins.left,
            wrapper_top.points() + margins.top,
            table_root_border_box.width() + margins.left + margins.right,
            border_box_height + margins.top + margins.bottom,
        ))
    }

    pub(in crate::layout::table) fn page_top_rect(self) -> PageTopRect {
        self.0
    }

    /// Return the following parent block cursor for a horizontal containing
    /// formatting context. This is intentionally a wrapper-margin-box result;
    /// table-grid logical progress is not parent-flow progress.
    pub(in crate::layout::table) fn horizontal_parent_block_end(self) -> PageTopBlockPosition {
        PageTopBlockPosition::new(self.0.top_y() - self.0.height())
    }
}

/// Logical block offset in the table wrapper's fragmentation timeline.
///
/// This is intentionally distinct from a table-grid offset. Captions consume
/// wrapper progress, but do not belong to the table-root background area.
/// The conversion at the grid boundary is named below so callers cannot
/// accidentally use caption progress as a grid background offset.
/// <https://www.w3.org/TR/css-tables-3/#table-structure>
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout::table) struct TableWrapperBlockOffset(TableGridLength);

impl TableWrapperBlockOffset {
    pub(in crate::layout::table) fn zero() -> Self {
        Self(TableGridLength::new(0.0))
    }

    fn add(self, size: TableGridLength) -> Self {
        Self(self.0 + size)
    }

    fn points(self) -> f32 {
        self.0.get()
    }
}

/// Block-start border, padding, and separated-border spacing between a table
/// wrapper's border edge and its grid content.
///
/// This must remain distinct from a grid offset: it is consumed once when the
/// wrapper timeline enters the grid, while a root decoration source frame
/// begins before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableRootBlockStartChrome(TableGridLength);

impl TableRootBlockStartChrome {
    pub(in crate::layout::table) fn new(length: TableGridLength) -> Self {
        Self(length)
    }

    fn length(self) -> TableGridLength {
        self.0
    }
}

/// The two wrapper-flow origins at the table-grid boundary.
///
/// The wrapper border box begins after any top caption. Grid content begins
/// after the wrapper's block-start chrome. Keeping both origins together
/// prevents root decoration replay from using the latter with the former's
/// border-box span.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TableWrapperGridStart {
    wrapper_border_start: TableWrapperBlockOffset,
    grid_content_start: TableWrapperBlockOffset,
}

impl TableWrapperGridStart {
    fn new(
        wrapper_border_start: TableWrapperBlockOffset,
        block_start_chrome: TableRootBlockStartChrome,
    ) -> Self {
        Self {
            wrapper_border_start,
            grid_content_start: wrapper_border_start.add(block_start_chrome.length()),
        }
    }

    fn grid_body_start(self, grid_block_start: TableGridBlockOffset) -> TableWrapperBlockOffset {
        self.grid_content_start.add(grid_block_start.length())
    }

    fn root_source_frame(self, root_rect: TableGridRect) -> TableWrapperLocalRootSourceFrame {
        TableWrapperLocalRootSourceFrame::new(self.wrapper_border_start, root_rect)
    }
}

/// One source interval in wrapper block-flow order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableWrapperBlockInterval {
    start: TableWrapperBlockOffset,
    size: TableGridLength,
}

impl TableWrapperBlockInterval {
    pub(in crate::layout::table) fn new(
        start: TableWrapperBlockOffset,
        size: TableGridLength,
    ) -> Self {
        Self { start, size }
    }

    pub(in crate::layout::table) fn size(self) -> TableGridLength {
        self.size
    }

    fn start(self) -> TableWrapperBlockOffset {
        self.start
    }
}

/// The complete table-root source rectangle paired with its wrapper-flow
/// source interval.
///
/// A root `TableGridRect` is grid-local, but fragmentation traverses wrapper
/// flow, including captions and block-start chrome. Keeping them in one
/// record prevents grid content from becoming the root border-box origin.
/// This local frame deliberately has no conversion to a multicolumn
/// continuous-source coordinate; enclosing projection owns that boundary.
/// <https://drafts.csswg.org/css-tables-3/#table-root>
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
#[derive(Debug, Clone, Copy, PartialEq)]
struct TableWrapperLocalRootSourceFrame {
    root_rect: TableGridRect,
    wrapper_interval: TableWrapperBlockInterval,
}

impl TableWrapperLocalRootSourceFrame {
    fn new(wrapper_border_start: TableWrapperBlockOffset, root_rect: TableGridRect) -> Self {
        Self {
            root_rect,
            wrapper_interval: TableWrapperBlockInterval::new(
                wrapper_border_start,
                TableGridLength::new(root_rect.size.height),
            ),
        }
    }

    fn root_rect(self) -> TableGridRect {
        self.root_rect
    }

    fn local_block_start(self) -> TableWrapperBlockOffset {
        self.wrapper_interval.start()
    }

    fn block_span(self) -> TableGridLength {
        self.wrapper_interval.size()
    }
}

/// Table-wrapper part that consumed a fragmentainer slice.
#[allow(dead_code)] // Caption/chrome entries are added by their layout recorders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableWrapperTimelineKind {
    TopCaption,
    GridStartChrome,
    GridBody,
    GridEndChrome,
    BottomCaption,
}

/// One committed wrapper source/destination slice.
///
/// Every entry is retained in logical source order and carries the actual
/// destination fragmentainer selected by layout. This makes it impossible to
/// reconstruct a table-root continuation from a physical Y cursor.
#[derive(Debug, Clone, Copy)]
struct TableWrapperFragmentSlice {
    kind: TableWrapperTimelineKind,
    source: TableWrapperBlockInterval,
    /// The table-grid source interval, when this wrapper entry exposes grid
    /// content.  Wrapper offsets include captions; grid offsets deliberately
    /// do not.
    grid_source_start: Option<TableGridBlockOffset>,
    destination: TableFragmentainerPlacement,
    /// The concrete destination page/column instance. Horizontal page
    /// fragments can share identical geometry, so placement alone cannot
    /// distinguish their separate table-root decoration clips.
    destination_page_index: Option<usize>,
    destination_grid_start: TableGridBlockOffset,
}

#[derive(Debug, Default)]
struct TableWrapperFragmentTimelineState {
    slices: Vec<TableWrapperFragmentSlice>,
    grid_start: Option<TableWrapperGridStart>,
    initial_destination_grid_placement: Option<TableGridPlacement>,
}

#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableWrapperFragmentTimeline {
    state: Rc<RefCell<TableWrapperFragmentTimelineState>>,
}

/// A rollback boundary in the table wrapper's committed paint timeline.
///
/// Break-avoid selection may restore the layout builder to an earlier row
/// boundary. The wrapper timeline is reference-counted across the candidate
/// fragment and the active fragment, so it needs an explicit transactional
/// boundary rather than relying on a clone to undo later row records.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableWrapperTimelineCheckpoint(usize);

impl TableWrapperFragmentTimeline {
    /// Start a wrapper-local recorder before caption layout.  Its first grid
    /// placement cannot be known yet: a split top caption can move the grid
    /// into a successor fragmentainer.
    pub(in crate::layout::table) fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(TableWrapperFragmentTimelineState::default())),
        }
    }

    pub(in crate::layout::table) fn checkpoint(&self) -> TableWrapperTimelineCheckpoint {
        TableWrapperTimelineCheckpoint(self.state.borrow().slices.len())
    }

    pub(in crate::layout::table) fn rewind(&self, checkpoint: TableWrapperTimelineCheckpoint) {
        self.state.borrow_mut().slices.truncate(checkpoint.0);
    }

    /// The grid's actual starting placement in the fragmentainer which
    /// contains the tail of a split top caption.
    #[cfg(test)]
    pub(in crate::layout::table) fn initial_destination_grid_placement(
        &self,
    ) -> TableGridPlacement {
        self.state
            .borrow()
            .initial_destination_grid_placement
            .expect("table wrapper recorder must commit its grid start before row layout")
    }

    fn root_source_frame(&self, root_rect: TableGridRect) -> TableWrapperLocalRootSourceFrame {
        self.state
            .borrow()
            .grid_start
            .expect("table wrapper root source requires a committed grid start")
            .root_source_frame(root_rect)
    }

    /// Commit the wrapper progress consumed by top captions and the actual
    /// placement at which the grid starts.  The progress is measured through
    /// the fragmentainer/table placement adapter, rather than reconstructed
    /// from a page-Y cursor in table-root paint.
    #[allow(dead_code)] // Test shorthand for one unsplit caption interval.
    pub(in crate::layout::table) fn record_top_caption_progress(
        &self,
        source_size: TableGridLength,
        destination: TableFragmentainerPlacement,
        destination_grid_placement: TableGridPlacement,
        root_block_start_chrome: TableRootBlockStartChrome,
    ) {
        self.record_top_caption_slices(
            &[],
            source_size,
            destination,
            destination_grid_placement,
            root_block_start_chrome,
        );
    }

    /// Record the table wrapper's top-caption source intervals before the
    /// grid starts.  A vertically fragmented caption can have multiple
    /// table-local destinations; retaining every interval prevents the final
    /// grid placement from retroactively becoming the caption's destination.
    pub(in crate::layout::table) fn record_top_caption_slices(
        &self,
        caption_slices: &[TableCaptionPaintSlice],
        source_size: TableGridLength,
        destination: TableFragmentainerPlacement,
        destination_grid_placement: TableGridPlacement,
        root_block_start_chrome: TableRootBlockStartChrome,
    ) {
        // The destination grid placement starts after table-root block-start
        // border/padding/edge spacing. The wrapper composite starts at the
        // root border edge, so strip only that named grid-start chrome before
        // using the progress as a sliced-decoration phase.
        let destination_grid_start = TableGridBlockOffset::new(TableGridLength::new(
            (destination
                .grid_block_progress(destination_grid_placement)
                .length()
                .get()
                - root_block_start_chrome.length().get())
            .max(0.0),
        ));
        let mut state = self.state.borrow_mut();
        if source_size.get() > 0.0 {
            if caption_slices.is_empty() {
                Self::push_slice(
                    &mut state,
                    TableWrapperFragmentSlice {
                        kind: TableWrapperTimelineKind::TopCaption,
                        source: TableWrapperBlockInterval::new(
                            TableWrapperBlockOffset::zero(),
                            source_size,
                        ),
                        grid_source_start: None,
                        destination,
                        destination_page_index: None,
                        destination_grid_start,
                    },
                );
            } else {
                for caption in caption_slices {
                    let source_start = TableWrapperBlockOffset::zero()
                        .add(TableGridLength::new(caption.source_block_start.points()));
                    Self::push_slice(
                        &mut state,
                        TableWrapperFragmentSlice {
                            kind: TableWrapperTimelineKind::TopCaption,
                            source: TableWrapperBlockInterval::new(
                                source_start,
                                TableGridLength::new(caption.block_size.points()),
                            ),
                            grid_source_start: None,
                            destination: caption.destination,
                            destination_page_index: None,
                            destination_grid_start: TableGridBlockOffset::new(
                                TableGridLength::new(0.0),
                            ),
                        },
                    );
                }
            }
        }
        let caption_end = TableWrapperBlockOffset::zero().add(source_size);
        if root_block_start_chrome.length().get() > 0.0 {
            Self::push_slice(
                &mut state,
                TableWrapperFragmentSlice {
                    kind: TableWrapperTimelineKind::GridStartChrome,
                    source: TableWrapperBlockInterval::new(
                        caption_end,
                        root_block_start_chrome.length(),
                    ),
                    grid_source_start: None,
                    destination,
                    destination_page_index: None,
                    destination_grid_start,
                },
            );
        }
        state.grid_start = Some(TableWrapperGridStart::new(
            caption_end,
            root_block_start_chrome,
        ));
        state.initial_destination_grid_placement = Some(destination_grid_placement);
    }

    /// Record an already-committed body slice. The body owns its source-grid
    /// interval; the wrapper timeline owns the order and destination.
    fn record_grid_body_slice(
        &self,
        destination: TableFragmentainerPlacement,
        destination_page_index: usize,
        source_start: TableGridBlockOffset,
        source_size: TableGridLength,
        destination_grid_start: TableGridBlockOffset,
    ) {
        if source_size.get() <= 0.0 {
            return;
        }
        let slice = TableWrapperFragmentSlice {
            kind: TableWrapperTimelineKind::GridBody,
            source: TableWrapperBlockInterval::new(
                self.state
                    .borrow()
                    .grid_start
                    .expect("table wrapper grid start must be committed before body slices")
                    .grid_body_start(source_start),
                source_size,
            ),
            grid_source_start: Some(source_start),
            destination,
            destination_page_index: Some(destination_page_index),
            destination_grid_start,
        };
        Self::push_slice(&mut self.state.borrow_mut(), slice);
    }

    /// Record table-root block-end chrome after all grid source content.
    ///
    /// Captions remain outside the grid source, but their wrapper interval
    /// follows this chrome in the same destination sequence.
    pub(in crate::layout::table) fn record_grid_end_chrome(
        &self,
        grid_source_extent: TableGridLength,
        source_size: TableGridLength,
        destination: TableFragmentainerPlacement,
        destination_grid_start: TableGridBlockOffset,
    ) {
        if source_size.get() <= 0.0 {
            return;
        }
        let state = &mut *self.state.borrow_mut();
        let grid_start = state
            .grid_start
            .expect("table wrapper grid start must be committed before trailing chrome");
        let source = TableWrapperBlockInterval::new(
            grid_start.grid_content_start.add(grid_source_extent),
            source_size,
        );
        Self::push_slice(
            state,
            TableWrapperFragmentSlice {
                kind: TableWrapperTimelineKind::GridEndChrome,
                source,
                grid_source_start: None,
                destination,
                destination_page_index: None,
                destination_grid_start,
            },
        );
    }

    /// Record a bottom-caption wrapper interval after table grid and trailing
    /// chrome. Captions deliberately have no table-grid source offset.
    #[allow(dead_code)] // Test shorthand for one unsplit caption interval.
    pub(in crate::layout::table) fn record_bottom_caption_progress(
        &self,
        grid_source_extent: TableGridLength,
        trailing_chrome: TableGridLength,
        source_size: TableGridLength,
        destination: TableFragmentainerPlacement,
        destination_grid_start: TableGridBlockOffset,
    ) {
        self.record_bottom_caption_slices(
            &[],
            grid_source_extent,
            trailing_chrome,
            source_size,
            destination,
            destination_grid_start,
        );
    }

    /// Record bottom-caption slices after the grid's immutable source range
    /// and trailing chrome.  Their source intervals are wrapper-local, so no
    /// caption entry can acquire a grid-source offset.
    pub(in crate::layout::table) fn record_bottom_caption_slices(
        &self,
        caption_slices: &[TableCaptionPaintSlice],
        grid_source_extent: TableGridLength,
        trailing_chrome: TableGridLength,
        source_size: TableGridLength,
        destination: TableFragmentainerPlacement,
        destination_grid_start: TableGridBlockOffset,
    ) {
        if source_size.get() <= 0.0 {
            return;
        }
        let state = &mut *self.state.borrow_mut();
        let grid_start = state
            .grid_start
            .expect("table wrapper grid start must be committed before bottom captions");
        let caption_start = grid_start
            .grid_content_start
            .add(grid_source_extent)
            .add(trailing_chrome);
        if caption_slices.is_empty() {
            Self::push_slice(
                state,
                TableWrapperFragmentSlice {
                    kind: TableWrapperTimelineKind::BottomCaption,
                    source: TableWrapperBlockInterval::new(caption_start, source_size),
                    grid_source_start: None,
                    destination,
                    destination_page_index: None,
                    destination_grid_start,
                },
            );
        } else {
            for caption in caption_slices {
                Self::push_slice(
                    state,
                    TableWrapperFragmentSlice {
                        kind: TableWrapperTimelineKind::BottomCaption,
                        source: TableWrapperBlockInterval::new(
                            caption_start
                                .add(TableGridLength::new(caption.source_block_start.points())),
                            TableGridLength::new(caption.block_size.points()),
                        ),
                        grid_source_start: None,
                        destination: caption.destination,
                        destination_page_index: None,
                        destination_grid_start: TableGridBlockOffset::new(TableGridLength::new(
                            0.0,
                        )),
                    },
                );
            }
        }
    }

    fn push_slice(state: &mut TableWrapperFragmentTimelineState, slice: TableWrapperFragmentSlice) {
        if let Some(previous) = state.slices.last() {
            // A table layout may revisit the current row while resolving a
            // deferred fragment boundary. It has not advanced either source
            // or destination in that case, so retain one committed slice
            // rather than treating the idempotent replay as an overlap.
            if previous.kind == slice.kind
                && previous.source == slice.source
                && previous.grid_source_start == slice.grid_source_start
                && previous.destination == slice.destination
                && previous.destination_page_index == slice.destination_page_index
                && previous.destination_grid_start == slice.destination_grid_start
            {
                return;
            }
            debug_assert!(
                slice.source.start.0.get()
                    >= previous.source.start.0.get() + previous.source.size().get() - 0.01,
                "table-wrapper source slices must remain ordered and non-overlapping"
            );
        }
        state.slices.push(slice);
    }

    /// Return every grid-body intersection committed in one destination
    /// fragmentainer. Root decoration deliberately ignores caption entries:
    /// captions affect placement, not the table-root positioning area.
    ///
    /// A final page can contain both the tail of a sliced row and a following
    /// row. Table-root decoration must cover both source intervals rather
    /// than only the last one recorded before the fragment is finalized.
    ///
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    /// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
    fn grid_body_slices_for(
        &self,
        destination: TableFragmentainerPlacement,
        destination_page_index: usize,
    ) -> Vec<TableWrapperFragmentSlice> {
        self.state
            .borrow()
            .slices
            .iter()
            .filter(|slice| {
                slice.kind == TableWrapperTimelineKind::GridBody
                    && slice.destination == destination
                    && slice.destination_page_index == Some(destination_page_index)
            })
            .copied()
            .collect()
    }

    /// Whether this wrapper has committed any grid-body source interval.
    /// A later table fragment with no matching local ledger entry must not
    /// fall back to painting the complete root rectangle: that was only valid
    /// before table-local structural slices were committed.
    fn has_grid_body_slices(&self) -> bool {
        self.state
            .borrow()
            .slices
            .iter()
            .any(|slice| slice.kind == TableWrapperTimelineKind::GridBody)
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
    destination_grid_origin: PageTopPoint,
    /// Capture origin for wrapper siblings before their committed replay.
    /// It remains typed and is never reconstructed from a source row.
    wrapper_table_x: PageInlinePosition,
    block_start: TableFragmentainerBlockStart,
    block_span: LogicalBlockContentSize,
    writing_mode: WritingMode,
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
        }
    }

    /// The immutable page origin of this fragment's destination cell grid.
    pub(in crate::layout::table) fn destination_grid_origin(self) -> PageTopPoint {
        self.destination_grid_origin
    }

    pub(in crate::layout::table) fn with_wrapper_table_x(
        mut self,
        wrapper_table_x: PageInlinePosition,
    ) -> Self {
        self.wrapper_table_x = wrapper_table_x;
        self
    }

    pub(in crate::layout::table) fn wrapper_table_x(self) -> PageInlinePosition {
        self.wrapper_table_x
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
    fn grid_block_progress(self, grid: TableGridPlacement) -> TableGridBlockOffset {
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

#[derive(Clone)]
pub(in crate::layout::table) struct TableBreakCandidateMeta {
    pub(in crate::layout::table) row_index: usize,
    pub(in crate::layout::table) table_body_fragment: Option<TableBodyPaintFragment>,
    pub(in crate::layout::table) wrapper_timeline_checkpoint:
        Option<TableWrapperTimelineCheckpoint>,
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
    /// The row cannot be divided at a legal cell-child boundary and the next
    /// fragmentainer has no more body capacity.  Commit it at this
    /// fragmentainer rather than repeatedly advancing through equivalent
    /// fragmentainers.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    PaintUnfragmentedOverflow,
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
        matches!(
            self.kind,
            TableOversizedRowSliceDecisionKind::PaintSlice
                | TableOversizedRowSliceDecisionKind::PaintUnfragmentedOverflow
        )
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

    /// Return whether an empty child-boundary pre-break would make no
    /// fragmentation progress.
    ///
    /// A table row may be taller than every available fragmentainer while its
    /// first cell child is atomic.  Retrying that child in another
    /// fragmentainer is only useful when the destination has strictly more
    /// usable table-body space; otherwise CSS fragmentation must accept the
    /// row's unfragmented overflow at its current start.
    ///
    /// <https://drafts.csswg.org/css-tables/#table-fragmentation>
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    pub(in crate::layout::table) fn needs_unfragmented_overflow(
        self,
        next_body_capacity: f32,
    ) -> bool {
        self.kind == TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice
            && next_body_capacity <= self.available_body_size + 0.01
    }

    /// Convert a zero-progress pre-break into one unfragmented row fragment.
    ///
    /// The caller restricts this to the first source piece of the row, so
    /// consuming all remaining height keeps the row's source and destination
    /// fragments identical rather than synthesizing an unanchored partial
    /// slice.
    pub(in crate::layout::table) fn as_unfragmented_overflow(
        mut self,
        next_body_capacity: f32,
    ) -> Self {
        debug_assert!(
            self.needs_unfragmented_overflow(next_body_capacity),
            "only a no-progress table pre-break may become unfragmented overflow"
        );
        self.kind = TableOversizedRowSliceDecisionKind::PaintUnfragmentedOverflow;
        self.piece_height = self.remaining_height;
        self
    }

    pub(in crate::layout::table) fn is_unfragmented_overflow(self) -> bool {
        self.kind == TableOversizedRowSliceDecisionKind::PaintUnfragmentedOverflow
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
    /// The matching destination fragmentainer block offset. This is recorded
    /// at row-commit time rather than reconstructed during structural paint.
    pub(in crate::layout::table) destination_block_start: TableGridBlockOffset,
}

/// The complete mapping from one retained table grid to one committed
/// destination fragmentainer.
///
/// Source row slices belong to the unfragmented table grid, while destination
/// slices are packed at the fragmentainer's logical block start. Keeping both
/// placements and the durable source slices in one value prevents a caller
/// from accidentally using a source-table offset as a physical destination
/// origin.
/// <https://drafts.csswg.org/css-tables-3/#table-fragmentation>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableGridFragmentProjection {
    source_frame: TableSourceGridFrame,
    destination_frame: TableDestinationCellGridFrame,
    source_row_slices: Vec<TableGridSourceRowSlice>,
}

impl TableGridFragmentProjection {
    fn new(
        source_placement: TableGridPlacement,
        destination_frame: TableDestinationCellGridFrame,
    ) -> Self {
        Self {
            source_frame: TableSourceGridFrame::new(source_placement),
            destination_frame,
            source_row_slices: Vec::new(),
        }
    }

    #[cfg(test)]
    fn fixture(source_placement: TableGridPlacement, destination: TableGridPlacement) -> Self {
        Self::new(
            source_placement,
            TableDestinationCellGridFrame::fixture(destination),
        )
    }

    pub(in crate::layout::table) fn source_placement(&self) -> TableGridPlacement {
        self.source_frame.grid()
    }

    pub(in crate::layout::table) fn destination_placement(&self) -> TableGridPlacement {
        self.destination_frame.grid()
    }

    fn record_source_row_slice(&mut self, row: TableRowBounds, decision: TableRowFragmentDecision) {
        let block_start = TableGridBlockOffset::new(TableGridLength::new(
            row.start + decision.row_offset.max(0.0),
        ));
        let block_size = TableGridLength::new(decision.row_height.max(0.0));
        if block_size.get() > 0.0 {
            let destination_block_start = self
                .source_row_slices
                .last()
                .map(|previous| {
                    let source_gap = (block_start.length().get()
                        - (previous.block_start.length().get() + previous.block_size.get()))
                    .max(0.0);
                    TableGridBlockOffset::new(TableGridLength::new(
                        previous.destination_block_start.length().get()
                            + previous.block_size.get()
                            + source_gap,
                    ))
                })
                .unwrap_or_else(|| TableGridBlockOffset::new(TableGridLength::new(0.0)));
            self.source_row_slices.push(TableGridSourceRowSlice {
                row_index: decision.row_index,
                block_start,
                block_size,
                destination_block_start,
            });
        }
    }

    pub(in crate::layout::table) fn source_row_slices(&self) -> &[TableGridSourceRowSlice] {
        &self.source_row_slices
    }

    /// Look up a committed source slice by its source row identity.
    ///
    /// Collapsed rows intentionally have no visible source slice, so the
    /// slice vector is not index-aligned with a fragment plan's row list.
    /// Structural paint must therefore use the durable source-row identity
    /// rather than its local plan position.
    fn source_row_slice(&self, row_index: usize) -> Option<&TableGridSourceRowSlice> {
        self.source_row_slices
            .iter()
            .find(|slice| slice.row_index == row_index)
    }

    /// Project one source-grid slice into this fragmentainer exactly once.
    fn project_slice(
        &self,
        source_slice: TableGridRect,
        destination_slice: TableGridRect,
        source_inline_edge: TableGridLength,
    ) -> TableStructuralPaintProjection {
        TableStructuralPaintProjection::from_grid_slices(
            self.source_placement(),
            self.destination_placement(),
            source_slice,
            destination_slice,
            source_inline_edge,
        )
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
    projection: TableGridFragmentProjection,
    destination_frame: TableFragmentainerFrame,
    root_background_source_placement: TableGridPlacement,
    wrapper_timeline: TableWrapperFragmentTimeline,
    source_row_bounds: Vec<TableRowBounds>,
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
pub(in crate::layout::table) struct TableWrapperDecorationViewport {
    fragments: Vec<TableWrapperDecorationSlice>,
}

#[derive(Debug, Clone, Copy)]
struct TableWrapperDecorationSlice {
    destination_clip_border_area: PaintBackgroundArea,
    decoration: FragmentedDecorationSlice,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRootLogicalInsets {
    inline_start: TableGridLength,
    inline_end: TableGridLength,
    block_start: TableGridLength,
    block_end: TableGridLength,
}

impl TableRootLogicalInsets {
    pub(in crate::layout::table) fn block_start(self) -> TableGridLength {
        self.block_start
    }
}

/// One structural table-paint slice projected from the unfragmented logical
/// grid into a committed fragmentainer.
///
/// Source and destination row rectangles intentionally share a table-grid
/// type but never a placement. The source retains the row offset used for
/// `box-decoration-break: slice`; the destination is packed at the physical
/// fragmentainer grid origin. Keeping them together prevents a source offset
/// from shifting the destination table origin.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy)]
struct TableStructuralPaintProjection {
    source_clip: PaintRect,
    destination_clip: PaintRect,
    source_to_destination: PaintTranslation,
}

impl TableStructuralPaintProjection {
    fn from_grid_slices(
        source_placement: TableGridPlacement,
        destination_placement: TableGridPlacement,
        source_slice: TableGridRect,
        destination_slice: TableGridRect,
        _source_inline_edge: TableGridLength,
    ) -> Self {
        // `TableGridPlacement::page_top_rect_for` is the writing-mode
        // boundary. Both rectangles are consequently already physical page
        // geometry; applying a logical-to-page transform here would rotate
        // vertical backgrounds a second time.
        let source_clip = source_placement
            .page_top_rect_for(source_slice)
            .paint_rect();
        let destination_clip = destination_placement
            .page_top_rect_for(destination_slice)
            .paint_rect();
        Self {
            source_clip,
            destination_clip,
            source_to_destination: PaintTranslation::new(
                destination_clip.origin.x - source_clip.origin.x,
                destination_clip.origin.y - source_clip.origin.y,
            ),
        }
    }

    fn source_clip(self) -> PaintRect {
        self.source_clip
    }

    fn destination_clip(self) -> PaintRect {
        self.destination_clip
    }
}

/// Select the table structural layer whose originating cells should expose a
/// background. The selected layer's positioning area remains separate from
/// the cell projections produced below.
///
/// CSS 2.2 paints row, column, row-group, and column-group backgrounds through
/// the complete areas of cells originating in those structures. A cell that
/// merely overlaps a column does not expose that column's background. The
/// same origin rule is used by CSS Tables 3's cell-background algorithm:
/// <https://www.w3.org/TR/CSS2/tables.html#table-layers>;
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>.
#[derive(Debug, Clone, Copy)]
enum TableStructuralOrigin {
    Rows { start: usize, end: usize },
    Columns { start: usize, end: usize },
}

impl TableStructuralOrigin {
    fn contains(self, row: usize, column: usize) -> bool {
        match self {
            Self::Rows { start, end } => (start..end).contains(&row),
            Self::Columns { start, end } => (start..end).contains(&column),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct TableStructuralVisibleCellRun {
    last_local_row: usize,
    source_start: f32,
    source_end: f32,
    destination_start: f32,
    destination_end: f32,
    last_source_row: usize,
}

impl TableStructuralVisibleCellRun {
    fn new(
        local_row: usize,
        source_row: usize,
        source_start: f32,
        source_end: f32,
        destination_start: f32,
        destination_end: f32,
    ) -> Self {
        Self {
            last_local_row: local_row,
            source_start,
            source_end,
            destination_start,
            destination_end,
            last_source_row: source_row,
        }
    }

    fn extend(
        &mut self,
        local_row: usize,
        source_row: usize,
        source_end: f32,
        destination_end: f32,
    ) {
        self.last_local_row = local_row;
        self.last_source_row = source_row;
        self.source_end = source_end;
        self.destination_end = destination_end;
    }
}

#[allow(clippy::too_many_arguments)]
fn table_structural_originating_cell_projections(
    projection: &TableGridFragmentProjection,
    row_bounds: &[TableRowBounds],
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    origin: TableStructuralOrigin,
    source_inline_edge: TableGridLength,
) -> Vec<TableStructuralPaintProjection> {
    let mut projections = Vec::new();
    for (origin_row, cells) in table_grid.rows.iter().enumerate() {
        for cell in cells {
            if !origin.contains(origin_row, cell.column) {
                continue;
            }
            let cell_end_row = origin_row
                .saturating_add(cell.rowspan.max(1))
                .min(row_bounds.len());
            let Some(cell_start) = row_bounds.get(origin_row).copied() else {
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
            let cell_inline = column_plan.inline_bounds_for_span(cell.column, cell.colspan);
            let mut visible_run = None;

            let mut commit_run = |run: Option<TableStructuralVisibleCellRun>| {
                let Some(run) = run else {
                    return;
                };
                if run.source_end <= run.source_start
                    || run.destination_end <= run.destination_start
                {
                    return;
                }
                let source_rect = TableGridRect::new(
                    TableGridPoint::from_lengths(
                        cell_inline.start,
                        TableGridLength::new(run.source_start),
                    ),
                    TableGridSize::from_lengths(
                        cell_inline.size,
                        TableGridLength::new(run.source_end - run.source_start),
                    ),
                );
                let destination_rect = TableGridRect::new(
                    TableGridPoint::from_lengths(
                        cell_inline.start,
                        TableGridLength::new(run.destination_start),
                    ),
                    TableGridSize::from_lengths(
                        cell_inline.size,
                        TableGridLength::new(run.destination_end - run.destination_start),
                    ),
                );
                projections.push(projection.project_slice(
                    source_rect,
                    destination_rect,
                    source_inline_edge,
                ));
            };

            for (local_row, source_row) in fragment_rows.iter().copied().enumerate() {
                if source_row < origin_row || source_row >= cell_end_row {
                    commit_run(visible_run.take());
                    continue;
                }
                let Some(&visible_size) = row_heights.get(local_row) else {
                    commit_run(visible_run.take());
                    continue;
                };
                if visible_size <= 0.0 {
                    commit_run(visible_run.take());
                    continue;
                }
                // A committed fragment records source-to-destination row
                // slices as it is laid out.  Structural painting is also
                // useful before that commitment (for example for an
                // unfragmented table and the geometry-level callers), where
                // the row bounds plus the visible row offset are the
                // authoritative grid coordinates.
                let (source_start, destination_start) =
                    if let Some(slice) = projection.source_row_slice(source_row) {
                        (
                            slice.block_start.length().get(),
                            slice.destination_block_start.length().get(),
                        )
                    } else {
                        let Some(row) = row_bounds.get(source_row) else {
                            commit_run(visible_run.take());
                            continue;
                        };
                        let visible_offset = row_offsets.get(local_row).copied().unwrap_or(0.0);
                        let start = row.start + visible_offset.max(0.0);
                        (start, start)
                    };
                let source_start = source_start.max(cell_block_start);
                let source_end = (source_start + visible_size).min(cell_block_end);
                if source_end <= source_start {
                    commit_run(visible_run.take());
                    continue;
                }
                let destination_end = destination_start + visible_size;
                if let Some(run) = &mut visible_run
                    && run.last_local_row + 1 == local_row
                    && run.last_source_row + 1 == source_row
                {
                    run.extend(local_row, source_row, source_end, destination_end);
                } else {
                    commit_run(visible_run.take());
                    visible_run = Some(TableStructuralVisibleCellRun::new(
                        local_row,
                        source_row,
                        source_start,
                        source_end,
                        destination_start,
                        destination_end,
                    ));
                }
            }
            commit_run(visible_run.take());
        }
    }
    projections
}

impl TableWrapperDecorationViewport {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn new(
        projection: &TableGridFragmentProjection,
        fragmentainer_placement: TableFragmentainerPlacement,
        destination_page_index: usize,
        root_source_placement: TableGridPlacement,
        wrapper_timeline: TableWrapperFragmentTimeline,
        style: &ComputedStyle,
        table_width: UsedTableWidth,
        block_edge_spacing: f32,
    ) -> Self {
        let source_placement = root_source_placement;
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
        // Root decoration replay consumes the wrapper-root source frame,
        // never a grid-content source offset with the full border-box span.
        let root_source_frame = wrapper_timeline.root_source_frame(root_rect);
        debug_assert!(root_source_frame.local_block_start().points() >= 0.0);
        debug_assert!(
            (root_source_frame.block_span().get() - root_rect.size.height).abs() <= f32::EPSILON
        );
        let root_rect = root_source_frame.root_rect();
        // `root_rect` already includes both wrapper block insets. Its source
        // geometry is the complete unfragmented border box used for
        // `box-decoration-break: slice`; adding the trailing inset again
        // shifts the positioning area and changes a repeating gradient's
        // phase in every continuation fragment.
        let source_positioning_rect = TableGridRect::new(
            root_rect.origin,
            TableGridSize::from_lengths(
                TableGridLength::new(root_rect.size.width),
                TableGridLength::new(root_rect.size.height),
            ),
        );
        let source_positioning_border_area = PaintBackgroundArea::from_paint_rect(
            source_placement
                .page_top_rect_for(source_positioning_rect)
                .paint_rect(),
        );
        let mut fragments = Vec::new();
        // Structural paint is emitted while each row piece is committed. The
        // current timeline entry is therefore exactly this paint call's
        // visible grid intersection; replaying all earlier entries would
        // paint their root backgrounds again for every subsequent row.
        for slice in
            wrapper_timeline.grid_body_slices_for(fragmentainer_placement, destination_page_index)
        {
            let row_height = slice.source.size().get();
            debug_assert!(row_height > 0.0);
            let block_start = slice
                .grid_source_start
                .expect("grid-body timeline entries retain their grid source interval")
                .length();
            let destination_placement = projection.destination_placement();
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
            let destination_block_start = slice.destination_grid_start.length().get();
            let destination_rect = TableGridRect::new(
                TableGridPoint::from_lengths(
                    -insets.inline_start,
                    TableGridLength::new(destination_block_start) - before,
                ),
                TableGridSize::from_lengths(
                    grid_inline + insets.inline_start + insets.inline_end,
                    TableGridLength::new(row_height) + before + after,
                ),
            );
            let projection = TableStructuralPaintProjection::from_grid_slices(
                source_placement,
                destination_placement,
                rect,
                destination_rect,
                TableGridLength::new(0.0),
            );
            let owns_block_start = block_start.get() <= 0.01;
            let owns_block_end = block_end.get() >= grid_block.get() - 0.01;
            let destination_clip_border_area =
                PaintBackgroundArea::from_paint_rect(projection.destination_clip());
            fragments.push(TableWrapperDecorationSlice {
                destination_clip_border_area: PaintBackgroundArea::from_paint_rect(
                    projection.destination_clip(),
                ),
                decoration: FragmentedDecorationSlice::new(
                    source_positioning_border_area.paint_rect(),
                    destination_clip_border_area.paint_rect(),
                    table_grid_source_progress_translation(
                        source_placement.writing_mode(),
                        TableGridBlockOffset::new(block_start),
                    ),
                    owns_block_start,
                    owns_block_end,
                ),
            });
        }
        if fragments.is_empty() && !wrapper_timeline.has_grid_body_slices() {
            let projection = TableStructuralPaintProjection::from_grid_slices(
                source_placement,
                projection.destination_placement(),
                root_rect,
                root_rect,
                TableGridLength::new(0.0),
            );
            let destination_clip_border_area =
                PaintBackgroundArea::from_paint_rect(projection.destination_clip());
            fragments.push(TableWrapperDecorationSlice {
                destination_clip_border_area: PaintBackgroundArea::from_paint_rect(
                    projection.destination_clip(),
                ),
                decoration: FragmentedDecorationSlice::new(
                    source_positioning_border_area.paint_rect(),
                    destination_clip_border_area.paint_rect(),
                    projection.source_to_destination,
                    true,
                    true,
                ),
            });
        }
        Self { fragments }
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
                let mut fragment_style = style.clone();
                suppress_fragmented_box_edges(
                    &mut fragment_style,
                    fragment.decoration.owns_block_start(),
                    fragment.decoration.owns_block_end(),
                );
                // Resolve the CSS background in the destination fragment's
                // physical coordinate system.  For `slice`, the shared
                // decoration contract translates the one unbroken source
                // positioning area; the destination area remains solely the
                // paint/clip geometry.  Resolving in source coordinates and
                // translating the primitive afterwards double-counts the
                // table fragmentainer translation for gradients and patterns.
                // <https://www.w3.org/TR/css-break-3/#break-decoration>
                // <https://www.w3.org/TR/css-backgrounds-3/#background-position>
                let positioning_border_area = PaintBackgroundArea::from_paint_rect(
                    fragment
                        .decoration
                        .positioning_border_rect(style.box_decoration_break),
                );
                fragmented_table_root_background_image_primitives(
                    positioning_border_area,
                    fragment.destination_clip_border_area,
                    &fragment_style,
                    base_url,
                    root_url,
                    resource_cache,
                )
            })
            .collect()
    }

    /// Paint the table-root color through the same projected clips as its
    /// background images.
    ///
    /// A fragmented table root has one source border area and one destination
    /// clip per visible row piece.  Resolving the color against the old
    /// fragment-local wrapper rectangle made the color layer disagree with the
    /// image layer, especially after a vertical writing-mode projection.
    pub(in crate::layout::table) fn color_primitives(
        &self,
        style: &ComputedStyle,
        table_width: UsedTableWidth,
    ) -> Vec<PaintPrimitive> {
        let Some(fill) = style.background.background_color.visible_color(style.color) else {
            return Vec::new();
        };
        self.fragments
            .iter()
            .filter_map(|fragment| {
                let destination = fragment.destination_clip_border_area.paint_rect();
                let clip = background_rect_clip_area_for_box(
                    destination,
                    style,
                    table_width.border_widths,
                    style.background.background_clip,
                    None,
                );
                (clip.size.width > 0.0 && clip.size.height > 0.0)
                    .then(|| PaintPrimitive::Rect(RenderedRect::from_paint_rect(clip, Some(fill))))
            })
            .collect()
    }
}

/// Map a table-grid source interval into the table-local replay canvas.
///
/// The enclosing fragmentation context later maps completed temporary parent
/// fragments to columns/pages.  A table-root decoration must therefore carry
/// only the immutable grid-source progress here: using the difference between
/// temporary-page origins leaks the parent replay translation into the table
/// background phase, and makes captions affect `box-decoration-break: slice`.
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
fn table_grid_source_progress_translation(
    writing_mode: WritingMode,
    source_block_start: TableGridBlockOffset,
) -> PaintTranslation {
    let progress = source_block_start.length().get();
    match writing_mode {
        WritingMode::HorizontalTb => PaintTranslation::new(0.0, progress),
        WritingMode::VerticalLr | WritingMode::SidewaysLr => PaintTranslation::new(progress, 0.0),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => PaintTranslation::new(-progress, 0.0),
    }
}

pub(in crate::layout::table) fn table_root_background_logical_insets(
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
        destination_frame: TableFragmentainerFrame,
        root_background_source_placement: TableGridPlacement,
        wrapper_timeline: TableWrapperFragmentTimeline,
        source_row_bounds: Vec<TableRowBounds>,
    ) -> Self {
        Self {
            projection: TableGridFragmentProjection::new(
                source_placement,
                destination_frame.cell_grid_frame(),
            ),
            destination_frame,
            root_background_source_placement,
            wrapper_timeline,
            source_row_bounds,
        }
    }

    /// The unfragmented logical grid used to resolve structural background
    /// positioning. Its origin is deliberately independent of any destination
    /// page or column, as required by `box-decoration-break: slice`.
    pub(in crate::layout::table) fn destination_placement(&self) -> TableGridPlacement {
        self.projection.destination_placement()
    }

    pub(in crate::layout::table) fn fragmentainer_placement(&self) -> TableFragmentainerPlacement {
        self.destination_frame.placement()
    }

    pub(in crate::layout::table) fn destination_frame(&self) -> TableFragmentainerFrame {
        self.destination_frame
    }

    /// The retained unfragmented grid used to resolve `slice` backgrounds and
    /// borders before projecting a row piece into this fragmentainer.
    pub(in crate::layout::table) fn source_placement(&self) -> TableGridPlacement {
        self.projection.source_placement()
    }

    /// The stable grid-local source placement used only by table-root
    /// backgrounds. Captions are wrapper siblings and cannot influence this
    /// CSS background positioning area.
    pub(in crate::layout::table) fn root_background_source_placement(&self) -> TableGridPlacement {
        self.root_background_source_placement
    }

    pub(in crate::layout::table) fn wrapper_timeline(&self) -> TableWrapperFragmentTimeline {
        self.wrapper_timeline.clone()
    }

    pub(in crate::layout::table) fn projection(&self) -> &TableGridFragmentProjection {
        &self.projection
    }

    pub(in crate::layout::table) fn row_bounds(&self) -> &[TableRowBounds] {
        &self.source_row_bounds
    }

    fn record_source_row_slice(
        &mut self,
        decision: TableRowFragmentDecision,
        destination_page_index: usize,
    ) {
        let Some(row) = self.source_row_bounds.get(decision.row_index).copied() else {
            return;
        };
        self.projection.record_source_row_slice(row, decision);
        if let Some(slice) = self.projection.source_row_slices().last().copied() {
            self.wrapper_timeline.record_grid_body_slice(
                self.destination_frame.placement(),
                destination_page_index,
                slice.block_start,
                slice.block_size,
                slice.destination_block_start,
            );
        }
    }

    /// Return the next packed destination block offset without projecting a
    /// physical page-Y coordinate back into the table grid. This is required
    /// for vertical tables, whose block axis is physical X.
    pub(in crate::layout::table) fn next_destination_block_start(
        &self,
        decision: TableRowFragmentDecision,
    ) -> Option<TableGridBlockOffset> {
        let row = self.source_row_bounds.get(decision.row_index)?;
        let block_start = TableGridBlockOffset::new(TableGridLength::new(
            row.start + decision.row_offset.max(0.0),
        ));
        Some(
            self.projection
                .source_row_slices
                .last()
                .map(|previous| {
                    let source_gap = (block_start.length().get()
                        - (previous.block_start.length().get() + previous.block_size.get()))
                    .max(0.0);
                    TableGridBlockOffset::new(TableGridLength::new(
                        previous.destination_block_start.length().get()
                            + previous.block_size.get()
                            + source_gap,
                    ))
                })
                .unwrap_or_else(|| TableGridBlockOffset::new(TableGridLength::new(0.0))),
        )
    }

    pub(in crate::layout::table) fn source_row_slices(&self) -> &[TableGridSourceRowSlice] {
        self.projection.source_row_slices()
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
    pub(in crate::layout::table) definite_block_size_stack: Vec<BlockSizePercentageBasis>,
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

/// CSS Tables 3 row-height plan for first-pass minimums, reference sizes, and
/// final distributed row sizes.
///
/// Spec: <https://drafts.csswg.org/css-tables-3/#row-layout> and
/// <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum TableHeightDistributionTarget {
    /// No definite table content-box height constrains row distribution.
    Intrinsic,
    /// A resolved table content-box height constrains row distribution.
    Definite(ContentBoxLength),
}

impl TableHeightDistributionTarget {
    /// The definite content-box height needed by legacy sizing interfaces.
    pub(in crate::layout::table) fn definite_content_height(self) -> Option<ContentBoxLength> {
        match self {
            Self::Intrinsic => None,
            Self::Definite(height) => Some(height),
        }
    }
}

/// Cache representation of [`TableHeightDistributionTarget`].
///
/// This is the only boundary that reduces a semantic content-box length to
/// its scalar bit representation for hashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) enum TableHeightDistributionTargetKey {
    Intrinsic,
    Definite(u32),
}

impl From<TableHeightDistributionTarget> for TableHeightDistributionTargetKey {
    fn from(target: TableHeightDistributionTarget) -> Self {
        match target {
            TableHeightDistributionTarget::Intrinsic => Self::Intrinsic,
            TableHeightDistributionTarget::Definite(height) => {
                Self::Definite(height.points().to_bits())
            }
        }
    }
}

#[cfg(test)]
mod table_height_distribution_target_tests {
    use super::*;

    #[test]
    fn cache_key_distinguishes_intrinsic_and_definite_distribution_targets() {
        assert_ne!(
            TableHeightDistributionTargetKey::from(TableHeightDistributionTarget::Intrinsic),
            TableHeightDistributionTargetKey::from(TableHeightDistributionTarget::Definite(
                content_box_pt(75.0),
            )),
        );
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct TableHeightPlan {
    pub(in crate::layout::table) rows: Vec<TableRowHeightPlan>,
    /// Resolved constraint for distributing the table grid's rows.
    ///
    /// This is distinct from the resulting intrinsic grid height: percentage
    /// descendants become definite only for the `Definite` variant.
    pub(in crate::layout::table) target: TableHeightDistributionTarget,
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
            viewport.record_source_row_slice(decision, self.plan.page_index);
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
    let clips = style.contain.paint
        || matches!(
            effective_overflow_for_style(style),
            css::Overflow::Hidden | css::Overflow::Clip
        );
    if !clips {
        return None;
    }
    let borders = used_border_widths(style);
    let border_box = paint_space_rect(
        padding_box.x() - borders.left,
        padding_box.y() - borders.bottom,
        padding_box.width() + borders.left + borders.right,
        padding_box.height() + borders.top + borders.bottom,
    );
    resolve_overflow_clip_edge(
        border_box,
        style,
        borders,
        UsedOverflowAxes::from_style(style),
        style.contain.paint,
        None,
    )
    .map(|edge| edge.clip.bounds)
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

/// Select the paint band for a table root from its computed outer display role.
///
/// CSS Tables defines `table` as block-level and `inline-table` as inline-level.
/// The table root's outer role therefore determines whether the atomic table
/// participates in the in-flow block or inline painting band; DOM ancestry,
/// including an enclosing table cell, does not change that role. Positioned and
/// relative tables are subsequently promoted by [`StackingContextPolicy`].
/// <https://drafts.csswg.org/css-display/#outer-role>;
/// <https://drafts.csswg.org/css-tables/#table-model>;
/// <https://www.w3.org/TR/CSS22/zindex.html>.
pub(in crate::layout::table) fn table_parent_paint_band(style: &ComputedStyle) -> PaintBand {
    debug_assert!(
        style.display.is_table(),
        "table paint-band classification requires a table root display"
    );
    if style.display.is_inline_level() {
        PaintBand::Inline
    } else {
        PaintBand::InFlowBlock
    }
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
    policy.effects.set_rectangular_overflow_clip(overflow_clip);
    policy
}

/// Whether this table fragment's structural outlines join the enclosing
/// normal-flow outline phase.
///
/// A static table is an atomic table-paint unit, but not an atomic *stacking*
/// context. Its row-group outlines therefore follow Quire's normal-flow
/// compatibility phase. Positioned and effect-owning tables retain a final
/// local outline phase instead.
pub(in crate::layout::table) fn table_outlines_use_in_flow_phase(
    style: &ComputedStyle,
    table_is_document_canvas: bool,
    policy: &StackingContextPolicy,
) -> bool {
    !table_is_document_canvas
        && !style.position.is_in_flow_positioned()
        && !policy.is_real_stacking_context
}

pub(in crate::layout::table) fn table_horizontal_non_content_width(
    table_width: UsedTableWidth,
) -> f32 {
    table_width.inline_non_content().points()
}

pub(in crate::layout::table) fn table_content_width_clamped_to_min_content(
    style: &ComputedStyle,
    content_width: LogicalInlineContentSize,
    min_content: LogicalInlineContentSize,
) -> LogicalInlineContentSize {
    // CSS 2.2 permits the fixed layout algorithm only when the table has a
    // non-auto width. An auto-width `table-layout: fixed` table therefore
    // still needs the automatic table's intrinsic floor before the fixed
    // planner consumes its used grid width.
    // <https://www.w3.org/TR/CSS22/tables.html#fixed-table-layout>
    if style.table_layout == TableLayout::Auto || table_root_inline_size(style).is_auto() {
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
    projection: &TableGridFragmentProjection,
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
    // `inline_bounds_for_span` is the table's direction-projected physical
    // span. The source paint view deliberately uses horizontal LTR page
    // coordinates, so this keeps a horizontal RTL column's background image
    // in its physical column without mirroring the CSS gradient itself.
    // <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
    let source_inline_edge = TableGridLength::new(0.0);
    let source_placement = projection.source_placement();
    let destination_placement = projection.destination_placement();
    let (Some(first_row), Some(last_row)) = (row_bounds.first(), row_bounds.last()) else {
        return Vec::new();
    };
    let block_start = TableGridLength::new(first_row.start);
    let block_size = TableGridLength::new(last_row.start + last_row.size - first_row.start);
    let inline_bounds = column_plan.inline_bounds_for_span(
        start_column,
        end_column.min(column_plan.column_count()) - start_column,
    );
    let positioning_rect = TableGridRect::new(
        TableGridPoint::from_lengths(inline_bounds.start, block_start),
        TableGridSize::from_lengths(inline_bounds.size, block_size),
    );
    let logical_positioning_area = PaintBackgroundArea::from_paint_rect(
        source_placement
            .overflow_clip_for(positioning_rect)
            .paint_rect(),
    );
    let mut primitives = Vec::new();
    let cell_clips = table_column_grid_cell_clips(
        projection,
        column_plan,
        table_grid,
        row_bounds,
        fragment_rows,
        row_heights,
        row_offsets,
        start_column,
        end_column,
        source_inline_edge,
    );
    for projection in cell_clips {
        let destination_clip = projection.destination_clip();
        let source_clip = projection.source_clip();
        primitives.extend(table_column_background_primitives_with_clip(
            destination_clip,
            style,
            destination_clip,
        ));
        let images = structural_table_background_image_primitives(
            logical_positioning_area,
            PaintBackgroundArea::from_paint_rect(source_clip),
            style,
            base_url,
            root_url,
            resource_cache,
        );
        if source_placement != destination_placement
            || source_placement.writing_mode().has_vertical_lines()
        {
            primitives.extend(images.into_iter().map(|primitive| {
                transform_table_column_image_primitive(primitive, projection.source_to_destination)
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
    projection: &TableGridFragmentProjection,
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    row_bounds: &[TableRowBounds],
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    start_column: usize,
    end_column: usize,
    source_inline_edge: TableGridLength,
) -> Vec<TableStructuralPaintProjection> {
    table_structural_originating_cell_projections(
        projection,
        row_bounds,
        column_plan,
        table_grid,
        fragment_rows,
        row_heights,
        row_offsets,
        TableStructuralOrigin::Columns {
            start: start_column,
            end: end_column,
        },
        source_inline_edge,
    )
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
    projection: &TableGridFragmentProjection,
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
    let source_placement = projection.source_placement();
    let has_collapsed_outer_insets = collapsed_outer_insets != css::Edges::ZERO;
    let positioning_rect = source_placement.full_page_top_rect().paint_rect();
    let clips: Vec<_> = fragment_rows
        .iter()
        .enumerate()
        .filter_map(|(local_row, source_row)| {
            let _source = row_bounds.get(*source_row)?;
            let row_height = *row_heights.get(local_row)?;
            let slice = projection.source_row_slice(*source_row)?;
            if row_height <= 0.0 {
                return None;
            }
            let source_rect = TableGridRect::new(
                TableGridPoint::from_lengths(TableGridLength::new(0.0), slice.block_start.length()),
                TableGridSize::from_lengths(
                    source_placement.logical_inline_grid_extent(),
                    TableGridLength::new(row_height),
                ),
            );
            let destination_rect = TableGridRect::new(
                TableGridPoint::from_lengths(
                    TableGridLength::new(0.0),
                    slice.destination_block_start.length(),
                ),
                source_rect.size,
            );
            Some(projection.project_slice(source_rect, destination_rect, TableGridLength::new(0.0)))
        })
        .collect();
    let mut background_style = style.clone();
    if has_collapsed_outer_insets {
        // The structural helper clips root colors to row pieces. Collapsed
        // outer borders sit outside those pieces, so paint the color clips
        // below with their physical table-wrapper outsets instead.
        background_style.background.background_color = css::BackgroundColor::TRANSPARENT;
    }
    let mut primitives = table_grid_structural_background_primitives(
        positioning_rect,
        clips,
        &background_style,
        base_url,
        root_url,
        resource_cache,
    );
    if let Some(fill) = style.background.background_color.visible_color(style.color)
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

/// Project a cell-derived structural primitive into its destination table
/// fragment. These backgrounds are resolved for their originating structural
/// box before cell clipping, so their existing projection also moves the
/// pattern placement.
fn transform_table_column_image_primitive(
    primitive: PaintPrimitive,
    translation: PaintTranslation,
) -> PaintPrimitive {
    primitive.translated(translation)
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
    if let Some(fill) = style.background.background_color.visible_color(style.color) {
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
                table_x + cell_inline.logical_start().get(),
                cell_bottom,
                cell_inline.logical_size().get(),
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
    if let Some(fill) = style.background.background_color.visible_color(style.color) {
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
    projection: &TableGridFragmentProjection,
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
    let source_placement = projection.source_placement();
    let Some(source_row) = row_bounds.get(row_index).copied() else {
        return Vec::new();
    };
    let inline_rect = column_plan
        .logical_occupied_inline_rect()
        .unwrap_or_else(|| {
            TableGridRect::new(
                TableGridPoint::from_lengths(TableGridLength::new(0.0), TableGridLength::new(0.0)),
                TableGridSize::from_lengths(
                    source_placement.logical_inline_grid_extent(),
                    TableGridLength::new(0.0),
                ),
            )
        });
    let positioning_rect = source_placement
        .page_top_rect_for(TableGridRect::new(
            TableGridPoint::from_lengths(
                TableGridLength::new(inline_rect.origin.x),
                TableGridLength::new(source_row.start),
            ),
            TableGridSize::from_lengths(
                TableGridLength::new(inline_rect.size.width),
                TableGridLength::new(source_row.size),
            ),
        ))
        .paint_rect();
    let clips = table_originating_cell_grid_clips(
        projection,
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
    projection: &TableGridFragmentProjection,
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
    let source_placement = projection.source_placement();
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
    let inline_rect = column_plan
        .logical_occupied_inline_rect()
        .unwrap_or_else(|| {
            TableGridRect::new(
                TableGridPoint::from_lengths(TableGridLength::new(0.0), TableGridLength::new(0.0)),
                TableGridSize::from_lengths(
                    source_placement.logical_inline_grid_extent(),
                    TableGridLength::new(0.0),
                ),
            )
        });
    let positioning_rect = source_placement
        .page_top_rect_for(TableGridRect::new(
            TableGridPoint::from_lengths(
                TableGridLength::new(inline_rect.origin.x),
                TableGridLength::new(start.start),
            ),
            TableGridSize::from_lengths(
                TableGridLength::new(inline_rect.size.width),
                TableGridLength::new((end.start + end.size - start.start).max(0.0)),
            ),
        ))
        .paint_rect();
    let clips = table_structural_originating_cell_projections(
        projection,
        row_bounds,
        column_plan,
        table_grid,
        fragment_rows,
        row_heights,
        row_offsets,
        TableStructuralOrigin::Rows {
            start: start_row,
            end: end_row,
        },
        TableGridLength::new(0.0),
    );
    table_grid_structural_background_primitives(
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
    projection: &TableGridFragmentProjection,
    row_bounds: &[TableRowBounds],
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    _originating_row: usize,
    structural_start_row: usize,
    structural_end_row: usize,
) -> Vec<TableStructuralPaintProjection> {
    table_structural_originating_cell_projections(
        projection,
        row_bounds,
        column_plan,
        table_grid,
        fragment_rows,
        row_heights,
        row_offsets,
        TableStructuralOrigin::Rows {
            start: structural_start_row,
            end: structural_end_row,
        },
        TableGridLength::new(0.0),
    )
}

/// Paint table structural layers from source-grid geometry into the cell
/// regions exposed by a single destination fragment. CSS background colors
/// use the physical destination clips; images resolve in the unfragmented
/// source positioning area and are then transformed once into the root table's
/// writing mode.
/// <https://www.w3.org/TR/css-backgrounds-3/#background-position>
/// <https://www.w3.org/TR/CSS22/tables.html#table-layers>
#[allow(clippy::too_many_arguments)]
fn table_grid_structural_background_primitives(
    source_positioning_rect: PaintRect,
    clips: Vec<TableStructuralPaintProjection>,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Vec<PaintPrimitive> {
    let positioning_area = PaintBackgroundArea::from_paint_rect(source_positioning_rect);
    let mut primitives = Vec::new();
    if let Some(fill) = style.background.background_color.visible_color(style.color) {
        primitives.extend(clips.iter().map(|projection| {
            PaintPrimitive::Rect(RenderedRect::from_paint_rect(
                projection.destination_clip(),
                Some(fill),
            ))
        }));
    }
    for projection in clips {
        let images = structural_table_background_image_primitives(
            positioning_area,
            PaintBackgroundArea::from_paint_rect(projection.source_clip()),
            style,
            base_url,
            root_url,
            resource_cache,
        );
        primitives.extend(images.into_iter().map(|primitive| {
            transform_table_column_image_primitive(primitive, projection.source_to_destination)
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
            table_x + cell_inline.logical_start().get(),
            cell_bottom,
            cell_inline.logical_size().get(),
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
        TableGridContentBoxTopLeft::new(PageTopPoint::new(table_x, grid_top)),
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
    if let Some(fill) = style.background.background_color.visible_color(style.color) {
        let area = background_rect_clip_area_for_box(
            paint_rect,
            style,
            css::Edges::ZERO,
            style.background.background_clip,
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
    use crate::css::Position;
    use crate::layout::{FlowAxes, PageBoxEdges, PageMargins, PageSize};

    #[test]
    fn auto_width_fixed_table_keeps_its_intrinsic_grid_floor() {
        let mut style = ComputedStyle::initial();
        style.table_layout = TableLayout::Fixed;

        assert_eq!(
            table_content_width_clamped_to_min_content(
                &style,
                LogicalInlineContentSize::new(content_box_pt(0.0)),
                LogicalInlineContentSize::new(content_box_pt(75.0)),
            )
            .points(),
            75.0,
        );
    }

    #[test]
    fn block_table_uses_the_in_flow_block_paint_band() {
        let mut style = ComputedStyle::initial();
        style.display = css::Display::TABLE;

        assert_eq!(table_parent_paint_band(&style), PaintBand::InFlowBlock);
    }

    #[test]
    fn inline_table_uses_the_inline_paint_band() {
        let mut style = ComputedStyle::initial();
        style.display = css::Display::INLINE_TABLE;

        assert_eq!(table_parent_paint_band(&style), PaintBand::Inline);
    }

    #[test]
    fn relatively_positioned_table_keeps_the_stacking_context_policy_band() {
        let mut style = ComputedStyle::initial();
        style.display = css::Display::TABLE;
        style.position = Position::Relative;

        let policy = table_atomic_stacking_policy(
            &style,
            table_parent_paint_band(&style),
            PaintClip::from_paint_rect(paint_space_rect(0.0, 0.0, 10.0, 10.0)),
            None,
        );

        assert_eq!(policy.parent_band, PaintBand::AutoZeroZ);
    }

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
    fn vertical_rl_projection_rebases_source_rows_without_moving_destination_origin() {
        let axes = TableAxes {
            flow: FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
            direction: Direction::Rtl,
        };
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(20.0, 200.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(100.0)),
                LogicalBlockContentSize::new(content_box_pt(300.0)),
            ),
        );
        let destination = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(400.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(100.0)),
                LogicalBlockContentSize::new(content_box_pt(300.0)),
            ),
        );
        let source_slice = TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(10.0), TableGridLength::new(120.0)),
            TableGridSize::from_lengths(TableGridLength::new(50.0), TableGridLength::new(40.0)),
        );
        let destination_slice = TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(10.0), TableGridLength::new(0.0)),
            source_slice.size,
        );
        let fragment_projection = TableGridFragmentProjection::fixture(source, destination);
        let projection = fragment_projection.project_slice(
            source_slice,
            destination_slice,
            TableGridLength::new(0.0),
        );

        assert_eq!(
            projection.destination_clip(),
            destination
                .page_top_rect_for(destination_slice)
                .paint_rect(),
            "source row offsets must not move the destination table origin",
        );
        assert_eq!(
            projection
                .source_to_destination
                .transform_rect(&projection.source_clip()),
            projection.destination_clip(),
            "the logical source row must be rebased exactly once into its destination slice",
        );
    }

    #[test]
    fn vertical_lr_projection_rebases_source_rows_into_the_next_fragmentainer() {
        let axes = TableAxes {
            flow: FlowAxes::new(WritingMode::VerticalLr, Direction::Rtl),
            direction: Direction::Rtl,
        };
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(80.0)),
                LogicalBlockContentSize::new(content_box_pt(240.0)),
            ),
        );
        let destination = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(360.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(80.0)),
                LogicalBlockContentSize::new(content_box_pt(240.0)),
            ),
        );
        let source_slice = TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(0.0), TableGridLength::new(120.0)),
            TableGridSize::from_lengths(TableGridLength::new(80.0), TableGridLength::new(40.0)),
        );
        let destination_slice = TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(0.0), TableGridLength::new(0.0)),
            source_slice.size,
        );
        let fragment_projection = TableGridFragmentProjection::fixture(source, destination);
        let projection = fragment_projection.project_slice(
            source_slice,
            destination_slice,
            TableGridLength::new(0.0),
        );

        assert_eq!(
            projection
                .source_to_destination
                .transform_rect(&projection.source_clip()),
            projection.destination_clip(),
            "a vertical-lr continuation must project the retained source slice once",
        );
        assert_eq!(
            projection.destination_clip(),
            destination
                .page_top_rect_for(destination_slice)
                .paint_rect(),
            "source progress must not move the destination fragmentainer origin",
        );
    }

    #[test]
    fn column_originating_cell_clips_exclude_separated_edge_spacing() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(70.0)),
                LogicalBlockContentSize::new(content_box_pt(20.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0), TableGridLength::new(20.0)],
            TableGridLength::new(10.0),
            vec![false, false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![vec![TableCellPlacement {
                cell: 0,
                column: 0,
                colspan: 1,
                rowspan: 1,
            }]],
            column_count: 2,
        };
        let projection = TableGridFragmentProjection::fixture(source, source);
        let clips = table_column_grid_cell_clips(
            &projection,
            &column_plan,
            &table_grid,
            &[TableRowBounds::new(0.0, 20.0)],
            &[0],
            &[20.0],
            &[0.0],
            0,
            1,
            TableGridLength::new(0.0),
        );

        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].source_clip().origin.x, 110.0);
        assert_eq!(clips[0].destination_clip().origin.x, 110.0);
    }

    #[test]
    fn row_originating_cell_clip_includes_internal_row_span_spacing() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(20.0)),
                LogicalBlockContentSize::new(content_box_pt(50.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0)],
            TableGridLength::new(10.0),
            vec![false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![
                vec![TableCellPlacement {
                    cell: 0,
                    column: 0,
                    colspan: 1,
                    rowspan: 2,
                }],
                Vec::new(),
            ],
            column_count: 1,
        };
        let projection = TableGridFragmentProjection::fixture(source, source);
        let projections = table_structural_originating_cell_projections(
            &projection,
            &[
                TableRowBounds::new(0.0, 20.0),
                TableRowBounds::new(30.0, 20.0),
            ],
            &column_plan,
            &table_grid,
            &[0, 1],
            &[20.0, 20.0],
            &[0.0, 0.0],
            TableStructuralOrigin::Rows { start: 0, end: 1 },
            TableGridLength::new(0.0),
        );

        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].source_clip().height(), 50.0);
        assert_eq!(projections[0].destination_clip().height(), 50.0);
    }

    #[test]
    fn column_background_selects_originating_cells_not_overlapping_cells() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(50.0)),
                LogicalBlockContentSize::new(content_box_pt(40.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0), TableGridLength::new(20.0)],
            TableGridLength::new(10.0),
            vec![false, false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![
                vec![TableCellPlacement {
                    cell: 0,
                    column: 0,
                    colspan: 2,
                    rowspan: 1,
                }],
                vec![TableCellPlacement {
                    cell: 1,
                    column: 1,
                    colspan: 1,
                    rowspan: 1,
                }],
            ],
            column_count: 2,
        };
        let projection = TableGridFragmentProjection::fixture(source, source);
        let projections = table_structural_originating_cell_projections(
            &projection,
            &[
                TableRowBounds::new(0.0, 20.0),
                TableRowBounds::new(20.0, 20.0),
            ],
            &column_plan,
            &table_grid,
            &[0, 1],
            &[20.0, 20.0],
            &[0.0, 0.0],
            TableStructuralOrigin::Columns { start: 1, end: 2 },
            TableGridLength::new(0.0),
        );

        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].source_clip().width(), 20.0);
    }

    #[test]
    fn separate_originating_cells_leave_border_spacing_outside_clips() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(50.0)),
                LogicalBlockContentSize::new(content_box_pt(20.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0), TableGridLength::new(20.0)],
            TableGridLength::new(10.0),
            vec![false, false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![vec![
                TableCellPlacement {
                    cell: 0,
                    column: 0,
                    colspan: 1,
                    rowspan: 1,
                },
                TableCellPlacement {
                    cell: 1,
                    column: 1,
                    colspan: 1,
                    rowspan: 1,
                },
            ]],
            column_count: 2,
        };
        let projection = TableGridFragmentProjection::fixture(source, source);
        let projections = table_structural_originating_cell_projections(
            &projection,
            &[TableRowBounds::new(0.0, 20.0)],
            &column_plan,
            &table_grid,
            &[0],
            &[20.0],
            &[0.0],
            TableStructuralOrigin::Columns { start: 0, end: 2 },
            TableGridLength::new(0.0),
        );

        assert_eq!(projections.len(), 2);
        let first = projections[0].destination_clip();
        let second = projections[1].destination_clip();
        assert_eq!(second.origin.x - (first.origin.x + first.size.width), 10.0);
    }

    #[test]
    fn collapsed_border_geometry_has_no_separated_spacing_between_cells() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(40.0)),
                LogicalBlockContentSize::new(content_box_pt(20.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0), TableGridLength::new(20.0)],
            TableGridLength::new(0.0),
            vec![false, false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![vec![
                TableCellPlacement {
                    cell: 0,
                    column: 0,
                    colspan: 1,
                    rowspan: 1,
                },
                TableCellPlacement {
                    cell: 1,
                    column: 1,
                    colspan: 1,
                    rowspan: 1,
                },
            ]],
            column_count: 2,
        };
        let projection = TableGridFragmentProjection::fixture(source, source);
        let projections = table_structural_originating_cell_projections(
            &projection,
            &[TableRowBounds::new(0.0, 20.0)],
            &column_plan,
            &table_grid,
            &[0],
            &[20.0],
            &[0.0],
            TableStructuralOrigin::Columns { start: 0, end: 2 },
            TableGridLength::new(0.0),
        );

        assert_eq!(projections.len(), 2);
        let first = projections[0].destination_clip();
        let second = projections[1].destination_clip();
        assert_eq!(second.origin.x, first.origin.x + first.size.width);
    }

    #[test]
    fn vertical_rtl_originating_cell_projection_maps_source_to_destination_once() {
        let axes = TableAxes {
            flow: FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
            direction: Direction::Rtl,
        };
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(20.0)),
                LogicalBlockContentSize::new(content_box_pt(40.0)),
            ),
        );
        let destination = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(300.0, 200.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(20.0)),
                LogicalBlockContentSize::new(content_box_pt(40.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0)],
            TableGridLength::new(0.0),
            vec![false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![vec![TableCellPlacement {
                cell: 0,
                column: 0,
                colspan: 1,
                rowspan: 1,
            }]],
            column_count: 1,
        };
        let fragment_projection = TableGridFragmentProjection::fixture(source, destination);
        let projections = table_structural_originating_cell_projections(
            &fragment_projection,
            &[TableRowBounds::new(0.0, 40.0)],
            &column_plan,
            &table_grid,
            &[0],
            &[40.0],
            &[0.0],
            TableStructuralOrigin::Columns { start: 0, end: 1 },
            TableGridLength::new(0.0),
        );

        assert_eq!(projections.len(), 1);
        let projection = projections[0];
        assert_eq!(
            projection
                .source_to_destination
                .transform_rect(&projection.source_clip()),
            projection.destination_clip()
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
            TableFragmentainerPlacement::horizontal(
                PageInlinePosition::new(0.0),
                PageTopBlockPosition::new(120.0),
                LogicalBlockContentSize::new(content_box_pt(100.0)),
            ),
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
    fn table_fragmentainer_placement_rebases_each_writing_mode_for_next_column() {
        let horizontal = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(72.0),
            PageTopBlockPosition::new(648.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        let horizontal_second_column = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(306.0),
            PageTopBlockPosition::new(648.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        assert_eq!(horizontal.destination_grid_origin().x(), 72.0);
        assert_eq!(horizontal.block_start().points(), 648.0);
        assert_eq!(
            horizontal_second_column.destination_grid_origin().x(),
            306.0
        );

        let vertical_lr = TableFragmentainerPlacement::vertical_lr(
            PageInlinePosition::new(72.0),
            PageTopBlockPosition::new(648.0),
            TableFragmentainerBlockStart::new(-72.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        let vertical_lr_second_column = TableFragmentainerPlacement::vertical_lr(
            PageInlinePosition::new(306.0),
            PageTopBlockPosition::new(648.0),
            TableFragmentainerBlockStart::new(-306.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        )
        .with_wrapper_table_x(PageInlinePosition::new(72.0));
        assert_eq!(vertical_lr.block_start().points(), -72.0);
        assert_eq!(vertical_lr_second_column.block_start().points(), -306.0);
        assert_eq!(
            vertical_lr_second_column.destination_grid_origin().x(),
            306.0
        );
        assert_eq!(vertical_lr_second_column.wrapper_table_x().points(), 72.0);

        let vertical_rl = TableFragmentainerPlacement::vertical_rl(
            PageInlinePosition::new(72.0),
            PageTopBlockPosition::new(648.0),
            TableFragmentainerBlockStart::new(540.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        let vertical_rl_second_column = TableFragmentainerPlacement::vertical_rl(
            PageInlinePosition::new(306.0),
            PageTopBlockPosition::new(648.0),
            TableFragmentainerBlockStart::new(306.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        )
        .with_wrapper_table_x(PageInlinePosition::new(72.0));
        assert_eq!(vertical_rl.block_start().points(), 540.0);
        assert_eq!(vertical_rl_second_column.block_start().points(), 306.0);
        assert_eq!(
            vertical_rl_second_column.destination_grid_origin().x(),
            306.0
        );
        assert_eq!(vertical_rl_second_column.wrapper_table_x().points(), 72.0);
    }

    #[test]
    fn table_root_origin_uses_the_resolved_no_caption_destination() {
        let resolved_destination = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(72.0),
            PageTopBlockPosition::new(648.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        // A normal-flow wrapper cursor may be different after float avoidance
        // or a preceding sibling. No caption means the root still begins at
        // the resolved fragmentainer destination, not at that stale cursor.
        let wrapper_parent_flow_top = PageTopBlockPosition::new(720.0);
        let axes = TableAxes::for_direction(Direction::Ltr);
        let table_width = UsedTableWidth {
            grid_inline: LogicalInlineContentSize::new(content_box_pt(80.0)),
            axes,
            content_width: content_box_pt(80.0),
            border_widths: css::Edges {
                top: 3.0,
                right: 0.0,
                bottom: 0.0,
                left: 5.0,
            },
            padding: css::Edges {
                top: 7.0,
                right: 0.0,
                bottom: 0.0,
                left: 11.0,
            },
        };

        let grid_origin =
            TableWrapperBorderBoxOrigin::new(resolved_destination.destination_grid_origin())
                .grid_content_box_top_left(axes, table_width)
                .page_top_point();

        assert_eq!(grid_origin, PageTopPoint::new(88.0, 638.0));
        assert_ne!(grid_origin.top_y(), wrapper_parent_flow_top.points());
    }

    #[test]
    fn table_fragment_trailing_paint_top_keeps_logical_axes_separate() {
        let inline_span = LogicalInlineContentSize::new(content_box_pt(80.0));
        let horizontal = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(72.0),
            PageTopBlockPosition::new(648.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        assert_eq!(
            horizontal.trailing_paint_top(PageTopBlockPosition::new(512.0), inline_span),
            PageTopBlockPosition::new(512.0),
        );

        for placement in [
            TableFragmentainerPlacement::vertical_lr(
                PageInlinePosition::new(72.0),
                PageTopBlockPosition::new(648.0),
                TableFragmentainerBlockStart::new(-72.0),
                LogicalBlockContentSize::new(content_box_pt(100.0)),
            ),
            TableFragmentainerPlacement::vertical_rl(
                PageInlinePosition::new(72.0),
                PageTopBlockPosition::new(648.0),
                TableFragmentainerBlockStart::new(540.0),
                LogicalBlockContentSize::new(content_box_pt(100.0)),
            ),
            TableFragmentainerPlacement {
                destination_grid_origin: PageTopPoint::new(72.0, 648.0),
                wrapper_table_x: PageInlinePosition::new(72.0),
                block_start: TableFragmentainerBlockStart::new(-72.0),
                block_span: LogicalBlockContentSize::new(content_box_pt(100.0)),
                writing_mode: WritingMode::SidewaysLr,
            },
        ] {
            assert_eq!(
                placement.trailing_paint_top(PageTopBlockPosition::new(512.0), inline_span),
                PageTopBlockPosition::new(568.0),
            );
        }
    }

    #[test]
    fn table_wrapper_margin_footprint_projects_parent_block_end() {
        let footprint = TableWrapperMarginBoxFootprint::from_table_root_border_box(
            PageTopRect::new(30.0, 180.0, 60.0, 80.0),
            PageTopBlockPosition::new(200.0),
            layout_pt(10.0),
            layout_pt(15.0),
            &css::Edges {
                top: 5.0,
                right: 0.0,
                bottom: 7.0,
                left: 0.0,
            },
        );

        assert_eq!(
            footprint.horizontal_parent_block_end(),
            PageTopBlockPosition::new(88.0)
        );
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
    fn zero_child_boundary_overflows_when_a_fresh_fragmentainer_is_not_larger() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            100.0,
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
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(0.0),
                footer_height: layout_pt(0.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: false,
                allow_footer: false,
            },
            can_advance: true,
        })
        .at_child_boundary(0.0);

        assert_eq!(
            decision.kind,
            TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice
        );
        assert!(decision.needs_unfragmented_overflow(100.0));

        let overflow = decision.as_unfragmented_overflow(100.0);
        assert!(overflow.paints_slice());
        assert!(overflow.is_unfragmented_overflow());
        assert_eq!(overflow.piece_height, 120.0);
        assert!(!overflow.continues_after_slice());
    }

    #[test]
    fn repeated_chrome_with_zero_body_capacity_overflows_in_place() {
        let chrome_context = TableFragmentChromeContext {
            fragmentainer_block_size: layout_pt(20.0),
            header_height: layout_pt(15.0),
            footer_height: layout_pt(10.0),
            wrapper_chrome: TableWrapperFragmentChrome::none(),
            allow_header: true,
            allow_footer: true,
        };
        let repeat_policy = TableFragmentRepeatPolicy {
            repeat_header: true,
            repeat_footer: true,
        };
        let next_body_capacity = chrome_context
            .fresh_fragmentainer(repeat_policy)
            .body_capacity
            .points();
        assert_eq!(next_body_capacity, 0.0);

        let decision = TableOversizedRowSliceDecision {
            kind: TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice,
            remaining_height: 40.0,
            available_body_size: 0.0,
            piece_height: 0.0,
            incoming_repeat_policy: repeat_policy,
        };
        assert!(decision.needs_unfragmented_overflow(next_body_capacity));
        assert!(
            decision
                .as_unfragmented_overflow(next_body_capacity)
                .is_unfragmented_overflow()
        );
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
            TableGridContentBoxTopLeft::new(origin),
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

    fn wrapper_viewport_box(
        writing_mode: WritingMode,
        direction: Direction,
        table_x: f32,
        top: f32,
    ) -> TableWrapperPaintBox {
        TableWrapperPaintBox {
            grid_origin: TableGridContentBoxTopLeft::new(PageTopPoint::new(table_x, top)),
            axes: TableAxes {
                flow: FlowAxes::new(writing_mode, direction),
                direction,
            },
            grid_size: TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(80.0)),
                LogicalBlockContentSize::new(content_box_pt(120.0)),
            ),
            table_width: UsedTableWidth {
                grid_inline: LogicalInlineContentSize::new(content_box_pt(80.0)),
                axes: TableAxes {
                    flow: FlowAxes::new(writing_mode, direction),
                    direction,
                },
                content_width: content_box_pt(80.0),
                border_widths: css::Edges::ZERO,
                padding: css::Edges::ZERO,
            },
            table_metrics: TableMetrics {
                border_collapse: css::BorderCollapse::Separate,
                spacing: css::BorderSpacing::ZERO,
            },
            block_edge_spacing: TableGridLength::new(0.0),
        }
    }

    #[test]
    fn table_root_border_origin_consumes_asymmetric_chrome_in_every_writing_mode() {
        let border_box_top_left = TableWrapperBorderBoxOrigin::new(PageTopPoint::new(30.0, 240.0));

        for writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            let axes = TableAxes {
                flow: FlowAxes::new(writing_mode, Direction::Ltr),
                direction: Direction::Ltr,
            };
            let table_width = UsedTableWidth {
                grid_inline: LogicalInlineContentSize::new(content_box_pt(80.0)),
                axes,
                content_width: content_box_pt(80.0),
                border_widths: css::Edges {
                    top: 3.0,
                    right: 5.0,
                    bottom: 7.0,
                    left: 11.0,
                },
                padding: css::Edges {
                    top: 13.0,
                    right: 17.0,
                    bottom: 19.0,
                    left: 23.0,
                },
            };

            assert_eq!(
                border_box_top_left
                    .grid_content_box_top_left(axes, table_width)
                    .page_top_point(),
                PageTopPoint::new(64.0, 224.0),
                "{writing_mode:?} must consume the physical root chrome before grid projection",
            );
        }
    }

    #[test]
    fn vertical_grid_paint_entry_uses_content_edge_after_root_chrome_projection() {
        let border_box_origin = TableWrapperBorderBoxOrigin::new(PageTopPoint::new(30.0, 240.0));
        for writing_mode in [
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
            WritingMode::SidewaysLr,
            WritingMode::SidewaysRl,
        ] {
            let axes = TableAxes {
                flow: FlowAxes::new(writing_mode, Direction::Ltr),
                direction: Direction::Ltr,
            };
            let table_width = UsedTableWidth {
                grid_inline: LogicalInlineContentSize::new(content_box_pt(80.0)),
                axes,
                content_width: content_box_pt(80.0),
                border_widths: css::Edges {
                    top: 3.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                },
                padding: css::Edges {
                    top: 7.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                },
            };
            let paint_box = TableWrapperPaintBox {
                grid_origin: border_box_origin.grid_content_box_top_left(axes, table_width),
                axes,
                grid_size: TableGridLogicalSize::new(
                    LogicalInlineContentSize::new(content_box_pt(80.0)),
                    LogicalBlockContentSize::new(content_box_pt(120.0)),
                ),
                table_width,
                table_metrics: TableMetrics {
                    border_collapse: css::BorderCollapse::Separate,
                    spacing: css::BorderSpacing::ZERO,
                },
                block_edge_spacing: TableGridLength::new(0.0),
            };
            let grid_content_top =
                PageTopBlockPosition::new(paint_box.clone().grid_content_box().top_y());
            let border_box_top = PageTopBlockPosition::new(paint_box.clone().border_box().top_y());
            let grid_paint_top = paint_box.initial_destination_grid_paint_top();

            assert_eq!(
                grid_paint_top, grid_content_top,
                "{writing_mode:?} must not reapply its physical top root chrome",
            );
            assert_ne!(
                grid_paint_top, border_box_top,
                "{writing_mode:?} has non-zero top chrome in this regression fixture",
            );
        }
    }

    #[test]
    fn wrapper_timeline_records_committed_grid_slices() {
        let wrapper = wrapper_viewport_box(WritingMode::HorizontalTb, Direction::Ltr, 120.0, 160.0);
        let viewport = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(120.0),
            PageTopBlockPosition::new(160.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        viewport.record_top_caption_progress(
            TableGridLength::new(0.0),
            destination,
            wrapper.grid_placement(),
            TableRootBlockStartChrome::new(TableGridLength::new(0.0)),
        );

        assert_eq!(
            viewport
                .initial_destination_grid_placement()
                .full_page_top_rect()
                .top_y(),
            160.0
        );
        viewport.record_grid_body_slice(
            destination,
            0,
            TableGridBlockOffset::new(TableGridLength::new(45.0)),
            TableGridLength::new(50.0),
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
        );
        assert_eq!(
            viewport
                .grid_body_slices_for(destination, 0)
                .into_iter()
                .next()
                .unwrap()
                .grid_source_start
                .unwrap(),
            TableGridBlockOffset::new(TableGridLength::new(45.0)),
        );
    }

    #[test]
    fn wrapper_timeline_projects_progress_through_vertical_grid_axes() {
        let wrapper = wrapper_viewport_box(WritingMode::VerticalRl, Direction::Rtl, 120.0, 80.0);
        let viewport = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::vertical_rl(
            PageInlinePosition::new(120.0),
            PageTopBlockPosition::new(80.0),
            TableFragmentainerBlockStart::new(200.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        viewport.record_top_caption_progress(
            TableGridLength::new(0.0),
            destination,
            wrapper.grid_placement(),
            TableRootBlockStartChrome::new(TableGridLength::new(0.0)),
        );

        assert_eq!(
            viewport
                .initial_destination_grid_placement()
                .full_page_top_rect()
                .x(),
            120.0
        );
    }

    #[test]
    fn caption_outcome_preserves_authoritative_vertical_destination_tracks() {
        // A table grid must receive the exact destination selected by the
        // caption consumer. These cover a remaining track, an exhausted
        // track requiring a successor, and a post-break track in both
        // vertical block directions.
        for (destination, requires_successor) in [
            (
                TableFragmentainerPlacement::vertical_rl(
                    PageInlinePosition::new(120.0),
                    PageTopBlockPosition::new(80.0),
                    TableFragmentainerBlockStart::new(170.0),
                    LogicalBlockContentSize::new(content_box_pt(50.0)),
                ),
                false,
            ),
            (
                TableFragmentainerPlacement::vertical_rl(
                    PageInlinePosition::new(120.0),
                    PageTopBlockPosition::new(80.0),
                    TableFragmentainerBlockStart::new(120.0),
                    LogicalBlockContentSize::new(content_box_pt(0.0)),
                ),
                true,
            ),
            (
                TableFragmentainerPlacement::vertical_lr(
                    PageInlinePosition::new(35.0),
                    PageTopBlockPosition::new(80.0),
                    TableFragmentainerBlockStart::new(-35.0),
                    LogicalBlockContentSize::new(content_box_pt(50.0)),
                ),
                false,
            ),
        ] {
            let outcome = TableCaptionLayoutOutcome::new(
                destination,
                Vec::new(),
                TableWrapperBlockInterval::new(
                    TableWrapperBlockOffset::zero(),
                    TableGridLength::new(50.0),
                ),
                requires_successor,
            );

            assert_eq!(outcome.final_destination(), destination);
            assert_eq!(outcome.next_part_requires_successor(), requires_successor);
        }
    }

    #[test]
    fn table_root_decoration_translation_uses_grid_source_progress_only() {
        let progress = TableGridBlockOffset::new(TableGridLength::new(37.5));

        assert_eq!(
            table_grid_source_progress_translation(WritingMode::HorizontalTb, progress),
            PaintTranslation::new(0.0, 37.5),
        );
        assert_eq!(
            table_grid_source_progress_translation(WritingMode::VerticalLr, progress),
            PaintTranslation::new(37.5, 0.0),
        );
        assert_eq!(
            table_grid_source_progress_translation(WritingMode::VerticalRl, progress),
            PaintTranslation::new(-37.5, 0.0),
        );
    }

    #[test]
    fn wrapper_timeline_keeps_caption_and_grid_source_offsets_distinct() {
        let wrapper = wrapper_viewport_box(WritingMode::VerticalLr, Direction::Rtl, 120.0, 80.0);
        let timeline = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::vertical_lr(
            PageInlinePosition::new(120.0),
            PageTopBlockPosition::new(80.0),
            TableFragmentainerBlockStart::new(-220.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        timeline.record_top_caption_progress(
            TableGridLength::new(140.0),
            destination,
            wrapper.grid_placement(),
            TableRootBlockStartChrome::new(TableGridLength::new(10.0)),
        );
        timeline.record_grid_body_slice(
            destination,
            0,
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
            TableGridLength::new(225.0),
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
        );
        timeline.record_grid_end_chrome(
            TableGridLength::new(225.0),
            TableGridLength::new(10.0),
            destination,
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
        );
        timeline.record_bottom_caption_progress(
            TableGridLength::new(225.0),
            TableGridLength::new(10.0),
            TableGridLength::new(15.0),
            destination,
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
        );

        let state = timeline.state.borrow();
        assert_eq!(state.slices.len(), 5);
        assert_eq!(state.slices[0].kind, TableWrapperTimelineKind::TopCaption);
        assert_eq!(
            state.slices[1].kind,
            TableWrapperTimelineKind::GridStartChrome
        );
        assert_eq!(state.slices[2].kind, TableWrapperTimelineKind::GridBody);
        assert_eq!(
            state.slices[3].kind,
            TableWrapperTimelineKind::GridEndChrome
        );
        assert_eq!(
            state.slices[4].kind,
            TableWrapperTimelineKind::BottomCaption
        );
        assert_eq!(state.slices[0].grid_source_start, None);
        assert_eq!(
            state.slices[2]
                .grid_source_start
                .map(|offset| offset.length().get()),
            Some(0.0)
        );
        assert_eq!(state.slices[4].grid_source_start, None);
        assert_eq!(state.slices[4].source.start.0.get(), 385.0);
    }

    #[test]
    fn wrapper_timeline_keeps_each_vertical_caption_destination_before_grid_source() {
        let timeline = TableWrapperFragmentTimeline::new();
        let wrapper = wrapper_viewport_box(WritingMode::VerticalLr, Direction::Rtl, 20.0, 300.0);
        let first_destination = TableFragmentainerPlacement::vertical_lr(
            PageInlinePosition::new(20.0),
            PageTopBlockPosition::new(300.0),
            TableFragmentainerBlockStart::new(-20.0),
            LogicalBlockContentSize::new(content_box_pt(25.0)),
        );
        let second_destination = TableFragmentainerPlacement::vertical_rl(
            PageInlinePosition::new(45.0),
            PageTopBlockPosition::new(300.0),
            TableFragmentainerBlockStart::new(45.0),
            LogicalBlockContentSize::new(content_box_pt(25.0)),
        );
        let context = PageContext {
            size: PageSize::from_points(400.0, 400.0),
            margins: PageMargins::all_points(0.0),
            edges: PageBoxEdges::ZERO,
            rotation: 0,
        };
        let caption_slices = [
            TableCaptionPaintSlice {
                page_index: 0,
                source_block_start: layout_pt(0.0),
                block_size: layout_pt(25.0),
                destination: first_destination,
                destination_context: context,
                destination_origin: PageTopPoint::new(context.left(), context.top()),
                destination_extent: LogicalSize {
                    inline: 100.0,
                    block: 25.0,
                },
                destination_block_start: layout_pt(0.0),
            },
            TableCaptionPaintSlice {
                page_index: 1,
                source_block_start: layout_pt(25.0),
                block_size: layout_pt(25.0),
                destination: second_destination,
                destination_context: context,
                destination_origin: PageTopPoint::new(context.left(), context.top()),
                destination_extent: LogicalSize {
                    inline: 100.0,
                    block: 25.0,
                },
                destination_block_start: layout_pt(0.0),
            },
        ];
        timeline.record_top_caption_slices(
            &caption_slices,
            TableGridLength::new(50.0),
            second_destination,
            wrapper.grid_placement(),
            TableRootBlockStartChrome::new(TableGridLength::new(10.0)),
        );
        timeline.record_grid_body_slice(
            second_destination,
            0,
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
            TableGridLength::new(20.0),
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
        );

        let state = timeline.state.borrow();
        assert_eq!(state.slices.len(), 4);
        assert_eq!(state.slices[0].destination, first_destination);
        assert_eq!(state.slices[1].destination, second_destination);
        assert_eq!(state.slices[0].source.start().points(), 0.0);
        assert_eq!(state.slices[1].source.start().points(), 25.0);
        assert_eq!(
            state.slices[2].kind,
            TableWrapperTimelineKind::GridStartChrome
        );
        assert_eq!(state.slices[3].kind, TableWrapperTimelineKind::GridBody);
        assert_eq!(
            state.slices[3].grid_source_start,
            Some(TableGridBlockOffset::new(TableGridLength::new(0.0)))
        );
    }

    fn wrapper_root_source_rect(block_start: f32, block_span: f32) -> TableGridRect {
        TableGridRect::new(
            TableGridPoint::from_lengths(
                TableGridLength::new(0.0),
                TableGridLength::new(block_start),
            ),
            TableGridSize::from_lengths(
                TableGridLength::new(100.0),
                TableGridLength::new(block_span),
            ),
        )
    }

    #[test]
    fn wrapper_root_source_frame_starts_before_block_start_chrome() {
        let timeline = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(0.0),
            PageTopBlockPosition::new(100.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        timeline.record_top_caption_progress(
            TableGridLength::new(0.0),
            destination,
            projection_placement(
                WritingMode::HorizontalTb,
                Direction::Ltr,
                PageTopPoint::new(0.0, 100.0),
            ),
            TableRootBlockStartChrome::new(TableGridLength::new(10.0)),
        );

        let root = timeline.root_source_frame(wrapper_root_source_rect(-10.0, 400.0));
        let grid_start = timeline
            .state
            .borrow()
            .grid_start
            .expect("grid start is committed");

        assert_eq!(root.local_block_start().points(), 0.0);
        assert_eq!(root.block_span().get(), 400.0);
        assert_eq!(grid_start.grid_content_start.points(), 10.0);
    }

    #[test]
    fn wrapper_root_source_frame_keeps_caption_progress_outside_start_chrome() {
        let timeline = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(0.0),
            PageTopBlockPosition::new(100.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        timeline.record_top_caption_progress(
            TableGridLength::new(140.0),
            destination,
            projection_placement(
                WritingMode::HorizontalTb,
                Direction::Ltr,
                PageTopPoint::new(0.0, 100.0),
            ),
            TableRootBlockStartChrome::new(TableGridLength::new(10.0)),
        );

        let root = timeline.root_source_frame(wrapper_root_source_rect(-10.0, 245.0));
        let grid_start = timeline
            .state
            .borrow()
            .grid_start
            .expect("grid start is committed");

        assert_eq!(root.local_block_start().points(), 140.0);
        assert_eq!(root.block_span().get(), 245.0);
        assert_eq!(grid_start.grid_content_start.points(), 150.0);
    }

    #[test]
    fn wrapper_root_source_frame_uses_logical_progress_for_vertical_tables() {
        let timeline = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::vertical_rl(
            PageInlinePosition::new(120.0),
            PageTopBlockPosition::new(80.0),
            TableFragmentainerBlockStart::new(-220.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        timeline.record_top_caption_progress(
            TableGridLength::new(140.0),
            destination,
            projection_placement(
                WritingMode::VerticalRl,
                Direction::Rtl,
                PageTopPoint::new(120.0, 80.0),
            ),
            TableRootBlockStartChrome::new(TableGridLength::new(10.0)),
        );

        let root = timeline.root_source_frame(wrapper_root_source_rect(-10.0, 245.0));

        assert_eq!(root.local_block_start().points(), 140.0);
        assert_eq!(root.block_span().get(), 245.0);
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

    #[test]
    fn wrapper_margin_footprint_includes_caption_space_and_margins() {
        let table_root_border_box = PageTopRect::new(24.0, 190.0, 80.0, 40.0);
        let wrapper = TableWrapperMarginBoxFootprint::from_table_root_border_box(
            table_root_border_box,
            PageTopBlockPosition::new(200.0),
            layout_pt(10.0),
            layout_pt(15.0),
            &css::Edges {
                top: 4.0,
                right: 5.0,
                bottom: 6.0,
                left: 7.0,
            },
        )
        .page_top_rect();

        assert_eq!(table_root_border_box.x(), 24.0);
        assert_eq!(table_root_border_box.top_y(), 190.0);
        assert_eq!(table_root_border_box.width(), 80.0);
        assert_eq!(table_root_border_box.height(), 40.0);
        assert_eq!(wrapper.x(), 17.0);
        assert_eq!(wrapper.top_y(), 204.0);
        assert_eq!(wrapper.width(), 92.0);
        assert_eq!(wrapper.height(), 75.0);
    }
}
