use super::*;

/// Logical coordinates inside a CSS table grid box.
///
/// `x` advances along the table's logical inline axis and `y` along its logical
/// block axis. [`TableGridPlacement`] projects this coordinate space to Quire's
/// physical page-top geometry:
/// <https://drafts.csswg.org/css-tables-3/#table-layout-algorithm>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TableGridSpace {}

/// A scalar distance in resolved table-grid coordinates.
///
/// Column tracks, offsets, and border spacing all share the table grid's
/// coordinate space, but do not necessarily share a CSS box-model space. A
/// track can be constrained by a table-cell border box and adjacent tracks are
/// separated by border spacing, so it must not be labeled as either a content-
/// or border-box length:
/// <https://drafts.csswg.org/css-tables-3/#table-layout-algorithm>.
pub(super) type TableGridLength = euclid::Length<f32, TableGridSpace>;

/// A distance along the table grid's logical inline axis.
///
/// This wrapper is used at projection boundaries, where treating a row offset
/// as a column offset would otherwise silently work for horizontal tables and
/// fail for vertical writing modes:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct TableGridInlineOffset(TableGridLength);

impl TableGridInlineOffset {
    pub(super) fn new(value: TableGridLength) -> Self {
        Self(value)
    }

    pub(super) fn length(self) -> TableGridLength {
        self.0
    }
}

/// A distance along the table grid's logical block axis.
///
/// See [`TableGridInlineOffset`]; block offsets must remain distinct until
/// [`TableGridPlacement`] projects them to page coordinates.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct TableGridBlockOffset(TableGridLength);

impl TableGridBlockOffset {
    pub(super) fn new(value: TableGridLength) -> Self {
        Self(value)
    }

    pub(super) fn length(self) -> TableGridLength {
        self.0
    }
}

/// A point in table-grid logical coordinates.
pub(super) type TableGridPoint = euclid::Point2D<f32, TableGridSpace>;
/// A size in table-grid logical coordinates.
pub(super) type TableGridSize = euclid::Size2D<f32, TableGridSpace>;
/// An axis-aligned rectangle in table-grid logical coordinates.
pub(super) type TableGridRect = euclid::Rect<f32, TableGridSpace>;

/// The table root grid's CSS content-box size in its own logical axes.
///
/// CSS Tables sizes columns along the root's logical inline axis and rows
/// along its logical block axis. Keeping both semantic lengths together
/// prevents a vertical table's inline extent from being passed to a physical
/// page-width consumer before writing-mode projection:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://drafts.csswg.org/css-tables-3/#table-layout-algorithm>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TableGridLogicalSize {
    inline: LogicalInlineContentSize,
    block: LogicalBlockContentSize,
}

impl TableGridLogicalSize {
    pub(super) fn new(inline: LogicalInlineContentSize, block: LogicalBlockContentSize) -> Self {
        Self { inline, block }
    }

    pub(super) fn inline(self) -> LogicalInlineContentSize {
        self.inline
    }

    pub(super) fn block(self) -> LogicalBlockContentSize {
        self.block
    }

    pub(super) fn physical_width(self, axes: TableAxes) -> PhysicalContentWidth {
        let physical = axes.flow.physical_size_from_logical(LogicalSize {
            inline: self.inline.points(),
            block: self.block.points(),
        });
        PhysicalContentWidth::new(content_box_pt(physical.width))
    }
}

/// Maps CSS table logical slots to physical table-grid coordinates.
///
/// CSS Tables keeps cells in a logical slot grid, while CSS Writing Modes says
/// `direction` controls the inline ordering of table columns. This type is the
/// table-specific axis boundary: logical column indices enter here and physical
/// inline offsets leave here:
/// <https://www.w3.org/TR/css-writing-modes-4/#direction> and
/// <https://drafts.csswg.org/css-tables-3/#cell-assignment>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TableAxes {
    pub(super) flow: FlowAxes,
    pub(super) direction: Direction,
}

/// An edge of the table root's logical cell grid.
///
/// A collapsed-border candidate originates from a physical CSS border side,
/// but conflict resolution happens on this root-owned grid.  Keeping this
/// distinct from [`LogicalSide`] prevents a cell with an orthogonal writing
/// mode from selecting a different table boundary than its table root:
/// <https://drafts.csswg.org/css-tables-3/#collapsing-borders> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TableGridEdge {
    InlineStart,
    InlineEnd,
    BlockStart,
    BlockEnd,
}

impl TableAxes {
    pub(super) fn for_style(style: &ComputedStyle) -> Self {
        Self {
            flow: FlowAxes::for_style(style),
            direction: style.used_direction(),
        }
    }

    /// Map a physical CSS border side onto the table root's logical grid.
    ///
    /// Collapsed-border declarations use physical CSS sides, while rows and
    /// columns are indexed in the table root's logical block and inline axes.
    /// This is deliberately rooted at the table, rather than the originating
    /// cell or table part:
    /// <https://drafts.csswg.org/css-tables-3/#collapsing-borders> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(super) fn grid_edge_for_physical_side(self, side: PhysicalSide) -> TableGridEdge {
        let axes = WritingModeAxes::new(self.flow.writing_mode(), self.direction);
        for (logical_side, grid_edge) in [
            (LogicalSide::InlineStart, TableGridEdge::InlineStart),
            (LogicalSide::InlineEnd, TableGridEdge::InlineEnd),
            (LogicalSide::BlockStart, TableGridEdge::BlockStart),
            (LogicalSide::BlockEnd, TableGridEdge::BlockEnd),
        ] {
            if axes.physical_side(logical_side) == side {
                return grid_edge;
            }
        }
        unreachable!("writing-mode side mapping must be bijective")
    }

    /// Map a table root logical grid edge to the corresponding physical CSS
    /// border side. See [`Self::grid_edge_for_physical_side`].
    pub(super) fn physical_side_for_grid_edge(self, edge: TableGridEdge) -> PhysicalSide {
        let logical_side = match edge {
            TableGridEdge::InlineStart => LogicalSide::InlineStart,
            TableGridEdge::InlineEnd => LogicalSide::InlineEnd,
            TableGridEdge::BlockStart => LogicalSide::BlockStart,
            TableGridEdge::BlockEnd => LogicalSide::BlockEnd,
        };
        WritingModeAxes::new(self.flow.writing_mode(), self.direction).physical_side(logical_side)
    }

    #[cfg(test)]
    pub(super) const fn for_direction(direction: Direction) -> Self {
        Self {
            flow: FlowAxes::new(WritingMode::HorizontalTb, direction),
            direction,
        }
    }

    pub(super) fn boundary_x(
        self,
        total_width: TableGridLength,
        logical_boundary_x: TableGridLength,
    ) -> TableGridLength {
        match self.direction {
            Direction::Ltr => logical_boundary_x,
            Direction::Rtl => total_width - logical_boundary_x,
        }
    }

    pub(super) fn span_start_x(
        self,
        total_width: TableGridLength,
        logical_start_x: TableGridLength,
        logical_end_x: TableGridLength,
    ) -> TableGridLength {
        self.boundary_x(total_width, logical_start_x)
            .min(self.boundary_x(total_width, logical_end_x))
    }

    /// Projects a logical table-grid rectangle into physical table-local
    /// coordinates.
    ///
    /// Table columns occupy the CSS inline axis and table rows occupy the
    /// block axis. Column-plan `direction` reversal has already been applied
    /// to the logical inline coordinate, so this projection deliberately uses
    /// LTR inline progression and does not reverse it a second time.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    /// <https://drafts.csswg.org/css-tables-3/#cell-assignment>
    pub(super) fn physical_rect_from_logical_grid(
        self,
        logical_inline_start: f32,
        logical_block_start: f32,
        logical_inline_size: f32,
        logical_block_size: f32,
        logical_inline_extent: f32,
        logical_block_extent: f32,
    ) -> ContainerRect {
        let axes = FlowAxes::new(self.flow.writing_mode(), Direction::Ltr);
        axes.rect_from_logical(
            ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                axes.physical_size_from_logical(LogicalSize {
                    inline: logical_inline_extent.max(0.0),
                    block: logical_block_extent.max(0.0),
                }),
            ),
            LogicalRect {
                origin: LogicalPoint {
                    // Normal cell slots are non-negative by construction.
                    // Collapsed border rules may deliberately extend beyond
                    // an outer grid line by half of an intersecting rule,
                    // however, so clamping their origins here would cut off
                    // corner joins before the final page projection.
                    inline: logical_inline_start,
                    block: logical_block_start,
                },
                size: LogicalSize {
                    inline: logical_inline_size.max(0.0),
                    block: logical_block_size.max(0.0),
                },
            },
        )
    }
}

/// One of the two logical axes on which a table root owns grid tracks.
///
/// Table columns advance on the root inline axis and rows advance on the root
/// block axis.  A table cell can establish a different flow, so callers must
/// retain the root-track role until they deliberately select the corresponding
/// physical dimension from the cell:
/// <https://drafts.csswg.org/css-tables-3/#table-layout> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TableRootTrackAxis {
    Inline,
    Block,
}

/// Selects the physical CSS sizing dimension for a table root inline track.
///
/// CSS 2.2's fixed table algorithm describes column sizing in terms of
/// `width`, while CSS Writing Modes maps the table root's logical inline axis
/// to physical width in horizontal writing and physical height in vertical or
/// sideways writing. This adapter deliberately has no cell flow or
/// `text-orientation` input: cells do not own the table grid axes, and text
/// orientation only controls glyph orientation.
/// <https://www.w3.org/TR/CSS22/tables.html#width-layout>
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TableInlineTrackSizing {
    physical_axis: PhysicalAxis,
}

impl TableInlineTrackSizing {
    pub(super) fn for_table(table_style: &ComputedStyle) -> Self {
        Self {
            physical_axis: TableCellAxisAdapter::for_table(table_style)
                .root_track_physical_axis(TableRootTrackAxis::Inline),
        }
    }

    pub(super) fn uses_physical_width(self) -> bool {
        self.physical_axis == PhysicalAxis::Horizontal
    }

    /// Select the physical `width` or `height` declaration that supplies a
    /// table root inline-track size.
    pub(super) fn declared_size(
        self,
        style: &ComputedStyle,
    ) -> css::ComputedLengthPercentageOrAuto {
        if self.uses_physical_width() {
            style.box_values.width.clone()
        } else {
            style.box_values.height.value().clone()
        }
    }

    /// Sum the two physical padding or border sides parallel to the table
    /// root inline axis.
    pub(super) fn parallel_insets(self, edges: css::Edges) -> NonContentLength {
        if self.uses_physical_width() {
            non_content_pt(edges.left + edges.right)
        } else {
            non_content_pt(edges.top + edges.bottom)
        }
    }

    /// Apply min/max constraints from the same physical axis as the selected
    /// declared track size.
    pub(super) fn constrain_content_box_size(
        self,
        style: &ComputedStyle,
        value: ContentBoxLength,
        percentage_basis: PercentageBasis<LayoutLength>,
    ) -> ContentBoxLength {
        if self.uses_physical_width() {
            constrain_content_width(style, value, percentage_basis)
        } else {
            constrain_content_height(style, value, percentage_basis)
        }
    }
}

/// Maps the table root's track axes and a cell's own flow to physical axes.
///
/// The adapter prevents table sizing from treating a cell's physical `width`
/// as though it always constrained columns, or its physical `height` as though
/// it always constrained rows.  The table root owns track direction; the cell
/// owns its internal inline/block flow:
/// <https://drafts.csswg.org/css-writing-modes-4/#dimension-mapping> and
/// <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TableCellAxisAdapter {
    table: FlowAxes,
    cell: FlowAxes,
}

impl TableCellAxisAdapter {
    pub(super) fn for_table(table_style: &ComputedStyle) -> Self {
        Self {
            table: FlowAxes::for_style(table_style),
            cell: FlowAxes::for_style(table_style),
        }
    }

    /// Construct the boundary between a table root and one of its cells.
    ///
    /// Tracks remain in the root flow, while the cell's descendants establish
    /// their containing block in the cell flow.  Keeping both flows here
    /// prevents an orthogonal cell's physical width from being reused as a
    /// root-inline column measure or as the cell's own inline measure:
    /// <https://drafts.csswg.org/css-tables-3/#table-layout> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>.
    pub(super) fn for_cell(table_style: &ComputedStyle, cell_style: &ComputedStyle) -> Self {
        Self {
            table: FlowAxes::for_style(table_style),
            cell: FlowAxes::for_style(cell_style),
        }
    }

    pub(super) fn root_track_physical_axis(self, track: TableRootTrackAxis) -> PhysicalAxis {
        let logical_axis = match track {
            TableRootTrackAxis::Inline => LogicalAxis::Inline,
            TableRootTrackAxis::Block => LogicalAxis::Block,
        };
        WritingModeAxes::new(self.table.writing_mode(), Direction::Ltr).physical_axis(logical_axis)
    }

    pub(super) fn root_track_uses_physical_width(self, track: TableRootTrackAxis) -> bool {
        self.root_track_physical_axis(track) == PhysicalAxis::Horizontal
    }

    pub(super) fn cell_inline_uses_physical_width(self) -> bool {
        WritingModeAxes::new(self.cell.writing_mode(), Direction::Ltr)
            .physical_axis(LogicalAxis::Inline)
            == PhysicalAxis::Horizontal
    }

    /// Project a cell's physical content rectangle into the cell's logical
    /// available sizes.  `PageTopRect` stays the paint/layout bridge, while
    /// this adapter retains the choice of physical axis in one place.
    pub(super) fn content_geometry(
        self,
        content_box: TableCellContentBox,
        block_offset_toward_end: f32,
    ) -> TableCellContentGeometry {
        let rect = content_box.page_top_rect();
        let (x_offset, top_offset) =
            match WritingModeAxes::new(self.cell.writing_mode(), Direction::Ltr)
                .physical_side(LogicalSide::BlockStart)
            {
                // Page-top coordinates decrease as physical CSS y advances down.
                PhysicalSide::Top => (0.0, -block_offset_toward_end),
                PhysicalSide::Bottom => (0.0, block_offset_toward_end),
                PhysicalSide::Left => (block_offset_toward_end, 0.0),
                PhysicalSide::Right => (-block_offset_toward_end, 0.0),
            };
        let rect = PageTopRect::new(
            rect.x() + x_offset,
            rect.top_y() + top_offset,
            rect.width(),
            rect.height(),
        );
        let inline_size = if self.cell_inline_uses_physical_width() {
            rect.width()
        } else {
            rect.height()
        };
        let block_size = if self.cell_inline_uses_physical_width() {
            rect.height()
        } else {
            rect.width()
        };
        TableCellContentGeometry {
            content_box: TableCellContentBox::from_page_top_rect(rect),
            inline_size: LogicalInlineContentSize::new(content_box_pt(inline_size.max(0.0))),
            block_size: LogicalBlockContentSize::new(content_box_pt(block_size.max(0.0))),
        }
    }
}

/// A logical area in the CSS table slot grid.
///
/// `row` and `column` are source-order grid coordinates, and `rowspan` and
/// `colspan` are logical slot counts. This is not a physical rectangle; it must
/// be projected through [`TableAxes`] and the table row-height plan:
/// <https://drafts.csswg.org/css-tables-3/#cell-assignment>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TableGridArea {
    pub(super) row: usize,
    pub(super) column: usize,
    pub(super) rowspan: usize,
    pub(super) colspan: usize,
}

impl TableGridArea {
    pub(super) fn from_placement(row: usize, placement: &TableCellPlacement) -> Self {
        Self {
            row,
            column: placement.column,
            rowspan: placement.rowspan.max(1),
            colspan: placement.colspan.max(1),
        }
    }
}

/// Physical inline bounds for a logical table-column span.
///
/// `start` is an `x` offset in [`TableGridSpace`] after applying the table's
/// `direction`; `size` is the border-box inline size consumed by the cell,
/// column, or collapsed-border segment:
/// <https://www.w3.org/TR/CSS22/tables.html#width-layout>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TableInlineBounds {
    pub(super) start: TableGridLength,
    pub(super) size: TableGridLength,
}

impl TableInlineBounds {
    pub(super) fn new(start: TableGridLength, size: TableGridLength) -> Self {
        Self {
            start,
            size: size.max(TableGridLength::new(0.0)),
        }
    }

    /// Project the inline-start grid offset onto a legacy page x coordinate.
    ///
    /// Table layout keeps this scalar in table-grid space until the final
    /// page/paint API boundary, which currently represents coordinates as raw
    /// `f32` values.
    pub(super) fn page_x(self, table_x: f32) -> f32 {
        table_x + self.start.get()
    }

    /// Return the span width for a legacy page/paint API.
    pub(super) fn page_width(self) -> f32 {
        self.size.get()
    }
}

/// Physical block bounds for a table row, row span, or row fragment.
///
/// `start` is the downward `y` offset from the active table-grid origin and
/// `size` is the row or fragment block size. CSS Tables computes these from
/// row tracks, then CSS Fragmentation may slice them per page:
/// <https://drafts.csswg.org/css-tables-3/#row-layout> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TableRowBounds {
    pub(super) start: f32,
    pub(super) size: f32,
}

impl TableRowBounds {
    pub(super) fn new(start: f32, size: f32) -> Self {
        Self {
            start,
            size: size.max(0.0),
        }
    }
}

/// A table-cell border box in table-grid physical coordinates.
///
/// CSS table cells generate table-cell boxes whose border boxes cover a slot
/// span in the table grid. This wrapper keeps that box typed until final paint
/// or layout-builder APIs require raw floats:
/// <https://www.w3.org/TR/CSS22/tables.html#model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TableCellBorderBox {
    rect: TableGridRect,
}

impl TableCellBorderBox {
    pub(super) fn from_bounds(inline: TableInlineBounds, row: TableRowBounds) -> Self {
        Self {
            rect: TableGridRect::new(
                TableGridPoint::from_lengths(inline.start, TableGridLength::new(row.start)),
                TableGridSize::from_lengths(inline.size, TableGridLength::new(row.size)),
            ),
        }
    }

    pub(super) fn rect(self) -> TableGridRect {
        self.rect
    }

    pub(super) fn width(self) -> f32 {
        self.rect.size.width
    }

    pub(super) fn x(self, placement: TableGridPlacement) -> f32 {
        placement.page_top_rect_for(self.rect).x()
    }

    pub(super) fn top_y(self, placement: TableGridPlacement) -> f32 {
        placement.page_top_rect_for(self.rect).top_y()
    }

    /// Project this logical cell border box to physical page geometry.
    pub(super) fn page_top_rect(self, placement: TableGridPlacement) -> PageTopRect {
        placement.page_top_rect_for(self.rect)
    }

    pub(super) fn content_box(
        self,
        placement: TableGridPlacement,
        padding: css::Edges,
        borders: css::Edges,
    ) -> TableCellContentBox {
        let border_box = placement.page_top_rect_for(self.rect);
        let x = border_box.x() + borders.left + padding.left;
        let right = border_box.x() + border_box.width() - borders.right - padding.right;
        let top_y = border_box.top_y() - borders.top - padding.top;
        let height =
            (border_box.height() - borders.top - borders.bottom - padding.top - padding.bottom)
                .max(0.0);
        TableCellContentBox {
            rect: PageTopRect::new(x, top_y, (right - x).max(0.0), height),
        }
    }
}

/// A table-cell content box projected to current page/container coordinates.
///
/// CSS table-cell contents establish a block container inside the cell's
/// padding box. Quire's block layout state still consumes physical `left`,
/// `right`, and `cursor_y` floats, so this type localizes that projection:
/// <https://www.w3.org/TR/CSS22/tables.html#model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TableCellContentBox {
    /// Projected CSS table-cell content box in page top-edge coordinates.
    ///
    /// The source box is the CSS table-cell padding/content area after borders,
    /// padding, vertical alignment offset, and fragmentation offsets have been
    /// applied. It is page-top geometry because child block layout advances
    /// downward from this top edge:
    /// <https://www.w3.org/TR/CSS22/tables.html#model>.
    rect: PageTopRect,
}

/// A cell content rectangle with its available dimensions in the cell's own
/// logical coordinate system.
///
/// A table grid supplies physical geometry after root-flow projection, but
/// descendants resolve widths, heights, percentages, text alignment, and
/// block alignment in the cell flow.  This composite carries both views so a
/// caller cannot accidentally use a root column span as a cell inline size:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TableCellContentGeometry {
    content_box: TableCellContentBox,
    inline_size: LogicalInlineContentSize,
    block_size: LogicalBlockContentSize,
}

impl TableCellContentGeometry {
    pub(super) fn content_box(self) -> TableCellContentBox {
        self.content_box
    }

    /// The final cell-content measure on the cell's own logical inline axis.
    ///
    /// This is intentionally distinct from the table root's column track.
    /// CSS Writing Modes projects an orthogonal cell only after its table
    /// track has been committed: <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>.
    pub(super) fn inline_size(self) -> LogicalInlineContentSize {
        self.inline_size
    }

    /// The final cell-content measure on the cell's own logical block axis.
    ///
    /// Cell-content alignment consumes this extent after final line
    /// construction, as required by CSS Tables:
    /// <https://drafts.csswg.org/css-tables-3/#table-cell-content-layout-second-pass>.
    pub(super) fn block_size(self) -> LogicalBlockContentSize {
        self.block_size
    }
}

impl TableCellContentBox {
    /// Construct a projected content box from an already resolved page-top rect.
    ///
    /// This is used by replay/planning paths that create the same table-cell
    /// block-container coordinate system without going back through grid
    /// placement:
    /// <https://www.w3.org/TR/CSS22/tables.html#model>.
    pub(super) fn from_page_top_rect(rect: PageTopRect) -> Self {
        Self { rect }
    }

    /// The physical inline-start edge used by Quire's block container state.
    ///
    /// The value is page/container-local after projecting the CSS table-cell
    /// padding box out of [`TableGridSpace`]. It is named `left` because table
    /// cell child layout currently consumes physical page coordinates:
    /// <https://www.w3.org/TR/CSS22/tables.html#model>.
    pub(super) fn left(self) -> f32 {
        self.rect.x()
    }

    /// The physical inline-end edge used by Quire's block container state.
    ///
    /// This is the right edge of the projected table-cell content box in the
    /// current page/container coordinate convention, not a CSS logical
    /// inline-end value:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(super) fn right(self) -> f32 {
        self.left() + self.width()
    }

    /// The physical top edge of the table-cell content box.
    ///
    /// Table layout tracks row fragments by top edge while CSS block layout
    /// advances child content downward from that edge:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>.
    pub(super) fn top_y(self) -> f32 {
        self.rect.top_y()
    }

    /// The physical inline size available to table-cell child layout.
    ///
    /// CSS table-cell contents form a block container whose used width is the
    /// cell padding box after borders, padding, and alignment offsets:
    /// <https://www.w3.org/TR/CSS22/tables.html#model>.
    pub(super) fn width(self) -> f32 {
        self.rect.width()
    }

    /// The physical block size available to table-cell child layout.
    ///
    /// This is the content-box height after table-cell borders and padding are
    /// removed from the row-span border box:
    /// <https://drafts.csswg.org/css-tables-3/#row-layout>.
    pub(super) fn height(self) -> f32 {
        self.rect.height()
    }

    /// Return this content box in Quire's page top-edge rectangle convention.
    ///
    /// This is the typed bridge from projected table-cell content geometry into
    /// layout and paint helpers that expect a top-edge page rectangle.
    pub(super) fn page_top_rect(self) -> PageTopRect {
        self.rect
    }
}

/// Places table-grid physical coordinates onto the current page/container.
///
/// `origin` is the physical top-left of the CSS table grid box in Quire's
/// page-top coordinate convention. This is the final projection boundary from
/// typed table-grid geometry into page, paint, and positioned-layout geometry:
/// <https://drafts.csswg.org/css-tables-3/#table-layout-algorithm>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TableGridPlacement {
    origin: PageTopPoint,
    axes: TableAxes,
    logical_size: TableGridLogicalSize,
}

/// The two grid coordinate frames owned by a separated-border table wrapper.
///
/// The wrapper grid includes the two outer block-axis `border-spacing` strips
/// that contribute to the table's wrapper size. Cell, row, and column
/// structural painting instead uses the cell grid, whose origin and extent
/// exclude exactly those strips. Keeping both frames together prevents a
/// caller from removing an edge spacing twice (or from the wrong physical
/// side in vertical writing modes).
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
/// <https://drafts.csswg.org/css-writing-modes-4/#abstract-box>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TableGridFrames {
    wrapper_grid: TableGridPlacement,
    cell_grid: TableGridPlacement,
}

/// The immutable complete source grid retained for fragmented table paint.
///
/// Source coordinates resolve CSS table backgrounds and borders before a row
/// piece is assigned to a page or column.  It is deliberately not
/// interchangeable with [`TableDestinationCellGridFrame`]: sharing their
/// underlying page-space representation previously made it possible to use a
/// continuation origin as a repeating-gradient positioning area.
/// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableSourceGridFrame {
    grid: TableGridPlacement,
}

impl TableSourceGridFrame {
    pub(super) fn new(grid: TableGridPlacement) -> Self {
        Self { grid }
    }

    pub(super) fn grid(self) -> TableGridPlacement {
        self.grid
    }
}

/// The cell-paint viewport of one destination table fragmentainer.
///
/// This wrapper has no constructor accepting a raw page origin.  The only
/// route to one is [`TableFragmentainerFrame`], which constructs wrapper and
/// cell frames together and removes separated-border edge spacing exactly
/// once.
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableDestinationCellGridFrame {
    grid: TableGridPlacement,
}

impl TableDestinationCellGridFrame {
    fn new(grid: TableGridPlacement) -> Self {
        Self { grid }
    }

    pub(super) fn grid(self) -> TableGridPlacement {
        self.grid
    }

    /// Test-only fixture constructor. Production destination frames must be
    /// created by [`TableFragmentainerFrame`] so their origin and separated
    /// border-spacing relation cannot be independently reconstructed.
    #[cfg(test)]
    pub(in crate::layout::table) fn fixture(grid: TableGridPlacement) -> Self {
        Self::new(grid)
    }
}

impl TableGridFrames {
    /// Construct the complete wrapper grid and its cell-paint subgrid from
    /// one authoritative placement.
    pub(super) fn new(
        wrapper_grid: TableGridPlacement,
        block_edge_spacing: TableGridLength,
    ) -> Self {
        let edge = block_edge_spacing.get().max(0.0);
        let cell_grid = if edge == 0.0 {
            wrapper_grid
        } else {
            let origin = if wrapper_grid.writing_mode().has_vertical_lines() {
                PageTopPoint::new(wrapper_grid.origin.x() + edge, wrapper_grid.origin.top_y())
            } else {
                PageTopPoint::new(wrapper_grid.origin.x(), wrapper_grid.origin.top_y() - edge)
            };
            let block = (wrapper_grid.logical_block_grid_extent().get() - edge * 2.0).max(0.0);
            TableGridPlacement::with_axes(
                origin,
                wrapper_grid.axes,
                TableGridLogicalSize::new(
                    wrapper_grid.logical_size.inline(),
                    LogicalBlockContentSize::new(content_box_pt(block)),
                ),
            )
        };
        Self {
            wrapper_grid,
            cell_grid,
        }
    }

    /// Reconstruct the complete wrapper grid from a cell-paint grid.
    ///
    /// Fragmented row layout receives a destination cell grid because rows
    /// and structural backgrounds must never include outer separated-border
    /// strips. Wrapper decoration, captions, and collapsed-border paint still
    /// need that complete grid. Keeping the inverse beside [`Self::new`]
    /// prevents continuation code from independently adding spacing on a
    /// physical side.
    pub(super) fn from_cell_grid(
        cell_grid: TableGridPlacement,
        block_edge_spacing: TableGridLength,
    ) -> Self {
        let edge = block_edge_spacing.get().max(0.0);
        if edge == 0.0 {
            return Self {
                wrapper_grid: cell_grid,
                cell_grid,
            };
        }
        let origin = if cell_grid.writing_mode().has_vertical_lines() {
            PageTopPoint::new(cell_grid.origin.x() - edge, cell_grid.origin.top_y())
        } else {
            PageTopPoint::new(cell_grid.origin.x(), cell_grid.origin.top_y() + edge)
        };
        let block = cell_grid.logical_block_grid_extent().get() + edge * 2.0;
        let wrapper_grid = TableGridPlacement::with_axes(
            origin,
            cell_grid.axes,
            TableGridLogicalSize::new(
                cell_grid.logical_size.inline(),
                LogicalBlockContentSize::new(content_box_pt(block)),
            ),
        );
        Self {
            wrapper_grid,
            cell_grid,
        }
    }

    pub(super) fn wrapper_grid(self) -> TableGridPlacement {
        self.wrapper_grid
    }

    pub(super) fn cell_grid(self) -> TableGridPlacement {
        self.cell_grid
    }
}

/// Immutable geometry of a table in one destination fragmentainer.
///
/// The table's source grid is deliberately absent: a fragmented table must
/// derive source-to-destination translation from two distinct frames, rather
/// than rebasing a source-local coordinate onto a physical `table_x`.  The
/// wrapper grid owns decoration and caption placement; the cell grid owns
/// rows, columns, and structural backgrounds.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableFragmentainerFrame {
    placement: super::layout::TableFragmentainerPlacement,
    grids: TableGridFrames,
}

impl TableFragmentainerFrame {
    /// Build the complete frame from the destination cell grid.  The inverse
    /// construction is intentionally private to table geometry: continuation
    /// code cannot independently add the outer separated-border spacing.
    pub(super) fn from_cell_grid(
        placement: super::layout::TableFragmentainerPlacement,
        cell_grid: TableGridPlacement,
        block_edge_spacing: TableGridLength,
    ) -> Self {
        Self {
            placement,
            grids: TableGridFrames::from_cell_grid(cell_grid, block_edge_spacing),
        }
    }

    pub(super) fn placement(self) -> super::layout::TableFragmentainerPlacement {
        self.placement
    }

    /// The complete wrapper grid used for captions and table-root decoration.
    pub(super) fn wrapper_grid(self) -> TableGridPlacement {
        self.grids.wrapper_grid()
    }

    /// Typed destination frame for row, cell, and structural paint.
    pub(super) fn cell_grid_frame(self) -> TableDestinationCellGridFrame {
        TableDestinationCellGridFrame::new(self.grids.cell_grid())
    }
}

impl TableGridPlacement {
    pub(super) fn new(origin: PageTopPoint) -> Self {
        Self::with_axes(
            origin,
            TableAxes {
                flow: FlowAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
                direction: Direction::Ltr,
            },
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(0.0)),
                LogicalBlockContentSize::new(content_box_pt(0.0)),
            ),
        )
    }

    /// Creates a table-grid placement whose slot geometry is projected through
    /// the table root's logical axes.
    ///
    /// `origin` is the physical top-left corner of the table grid's containing
    /// rectangle. The logical extents let right-to-left block progression in
    /// `vertical-rl` locate rows from the opposite physical edge.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    pub(super) fn with_axes(
        origin: PageTopPoint,
        axes: TableAxes,
        logical_size: TableGridLogicalSize,
    ) -> Self {
        Self {
            origin,
            axes,
            logical_size,
        }
    }

    /// Return this immutable frame's physical page origin.
    ///
    /// Fragmentation may rebase a destination frame, but row and structural
    /// paint must take that origin from the frame rather than retain a second
    /// mutable `table_x` coordinate beside it.
    pub(super) fn origin(self) -> PageTopPoint {
        self.origin
    }

    pub(super) fn overflow_clip_for(self, rect: TableGridRect) -> OverflowClip {
        OverflowClip::from_page_top_rect(self.page_top_rect_for(rect))
    }

    /// Projects one logical table-grid rectangle into page top-edge geometry.
    pub(super) fn page_top_rect_for(self, rect: TableGridRect) -> PageTopRect {
        let logical_inline_extent = self.logical_size.inline().points().max(rect.max_x());
        let logical_block_extent = self.logical_size.block().points().max(rect.max_y());
        let physical = self.axes.physical_rect_from_logical_grid(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
            logical_inline_extent,
            logical_block_extent,
        );
        PageTopRect::new(
            self.origin.x() + physical.origin.x,
            self.origin.top_y() - physical.origin.y,
            physical.size.width,
            physical.size.height,
        )
    }

    /// Project a logical inline grid line through a logical block-axis span.
    ///
    /// A column boundary is a line of zero inline extent and non-zero block
    /// extent.  The returned segment is physical only at the final collapsed
    /// border paint boundary, so vertical writing modes rotate the segment
    /// instead of treating it as an X/Y special case.
    pub(super) fn project_inline_line(
        self,
        inline: TableGridInlineOffset,
        block_start: TableGridBlockOffset,
        block_size: TableGridBlockOffset,
    ) -> CollapsedBorderSegment {
        CollapsedBorderSegment::from_projected_line(self.page_top_rect_for(TableGridRect::new(
            TableGridPoint::from_lengths(inline.length(), block_start.length()),
            TableGridSize::from_lengths(TableGridLength::new(0.0), block_size.length()),
        )))
    }

    /// Project a logical block grid line through a logical inline-axis span.
    ///
    /// A row boundary is a line of zero block extent and non-zero inline
    /// extent. See [`Self::project_inline_line`] for why this conversion is
    /// kept at the placement boundary.
    pub(super) fn project_block_line(
        self,
        inline_start: TableGridInlineOffset,
        inline_size: TableGridInlineOffset,
        block: TableGridBlockOffset,
    ) -> CollapsedBorderSegment {
        CollapsedBorderSegment::from_projected_line(self.page_top_rect_for(TableGridRect::new(
            TableGridPoint::from_lengths(inline_start.length(), block.length()),
            TableGridSize::from_lengths(inline_size.length(), TableGridLength::new(0.0)),
        )))
    }

    /// Re-label a committed page top-edge coordinate as a logical block
    /// offset relative to this placement. This is the horizontal-table bridge
    /// for fragmented collapsed borders, whose row pieces are committed in
    /// page coordinates before their grid-line segments are emitted.
    pub(super) fn block_offset_from_page_top(self, page_top: f32) -> TableGridBlockOffset {
        TableGridBlockOffset::new(TableGridLength::new(self.origin.top_y() - page_top))
    }

    /// Returns the complete logical table-grid extent in page coordinates.
    pub(super) fn full_page_top_rect(self) -> PageTopRect {
        self.page_top_rect_for(TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(0.0), TableGridLength::new(0.0)),
            TableGridSize::from_lengths(
                self.logical_size.inline().content_box_length().cast_unit(),
                self.logical_size.block().content_box_length().cast_unit(),
            ),
        ))
    }

    /// Return the logical inline extent re-labeled for table-grid geometry.
    pub(super) fn logical_inline_grid_extent(self) -> TableGridLength {
        self.logical_size.inline().content_box_length().cast_unit()
    }

    /// Return the logical block extent re-labeled for table-grid geometry.
    pub(super) fn logical_block_grid_extent(self) -> TableGridLength {
        self.logical_size.block().content_box_length().cast_unit()
    }

    pub(super) fn writing_mode(self) -> WritingMode {
        self.axes.flow.writing_mode()
    }

    pub(super) fn containing_block_for(
        self,
        border_box: TableCellBorderBox,
        borders: css::Edges,
    ) -> ContainingBlock {
        let border_box = self.page_top_rect_for(border_box.rect());
        ContainingBlock::from_page_top_rect(PageTopRect::new(
            border_box.x() + borders.left,
            border_box.top_y() - borders.top,
            border_box.width() - borders.left - borders.right,
            border_box.height() - borders.top - borders.bottom,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_length(value: f32) -> TableGridLength {
        TableGridLength::new(value)
    }

    #[test]
    fn maps_ltr_column_span_bounds() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        assert_eq!(
            axes.boundary_x(grid_length(100.0), grid_length(25.0)),
            grid_length(25.0)
        );
        assert_eq!(
            axes.span_start_x(grid_length(100.0), grid_length(10.0), grid_length(45.0)),
            grid_length(10.0)
        );
    }

    #[test]
    fn maps_rtl_boundaries_and_spans_from_the_right() {
        let axes = TableAxes::for_direction(Direction::Rtl);
        assert_eq!(
            axes.boundary_x(grid_length(100.0), grid_length(0.0)),
            grid_length(100.0)
        );
        assert_eq!(
            axes.boundary_x(grid_length(100.0), grid_length(25.0)),
            grid_length(75.0)
        );
        assert_eq!(
            axes.span_start_x(grid_length(100.0), grid_length(10.0), grid_length(45.0)),
            grid_length(55.0)
        );
    }

    #[test]
    fn table_grid_edges_round_trip_through_every_root_writing_mode() {
        for writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                let axes = TableAxes {
                    flow: FlowAxes::new(writing_mode, direction),
                    direction,
                };
                for edge in [
                    TableGridEdge::InlineStart,
                    TableGridEdge::InlineEnd,
                    TableGridEdge::BlockStart,
                    TableGridEdge::BlockEnd,
                ] {
                    let physical = axes.physical_side_for_grid_edge(edge);
                    assert_eq!(axes.grid_edge_for_physical_side(physical), edge);
                }
            }
        }
    }

    #[test]
    fn projects_vertical_rl_table_slots_from_logical_axes() {
        let axes = TableAxes {
            flow: FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr),
            direction: Direction::Ltr,
        };
        let rect = axes.physical_rect_from_logical_grid(10.0, 20.0, 30.0, 40.0, 100.0, 200.0);

        assert_eq!(rect.origin, ContainerPoint::new(140.0, 10.0));
        assert_eq!(rect.size, ContainerSize::new(40.0, 30.0));
    }

    #[test]
    fn projects_vertical_lr_table_slots_from_logical_axes() {
        let axes = TableAxes {
            flow: FlowAxes::new(WritingMode::VerticalLr, Direction::Ltr),
            direction: Direction::Ltr,
        };
        let rect = axes.physical_rect_from_logical_grid(10.0, 20.0, 30.0, 40.0, 100.0, 200.0);

        assert_eq!(rect.origin, ContainerPoint::new(20.0, 10.0));
        assert_eq!(rect.size, ContainerSize::new(40.0, 30.0));
    }

    #[test]
    fn preserves_negative_logical_inline_offsets_for_collapsed_rule_extensions() {
        let horizontal = TableAxes::for_direction(Direction::Ltr);
        let vertical = TableAxes {
            flow: FlowAxes::new(WritingMode::VerticalLr, Direction::Rtl),
            direction: Direction::Rtl,
        };

        // A collapsed rule can extend half its winning width beyond the
        // outer grid line. It is a line-segment projection, not a cell slot,
        // so clamping this offset to zero would cut off the corner join.
        assert_eq!(
            horizontal
                .physical_rect_from_logical_grid(-1.5, 0.0, 3.0, 2.0, 20.0, 10.0)
                .origin,
            ContainerPoint::new(-1.5, 0.0)
        );
        assert_eq!(
            vertical
                .physical_rect_from_logical_grid(-1.5, 0.0, 3.0, 2.0, 20.0, 10.0)
                .origin,
            ContainerPoint::new(0.0, -1.5)
        );
    }

    #[test]
    fn table_grid_logical_size_projects_physical_width_by_writing_mode() {
        let size = TableGridLogicalSize::new(
            LogicalInlineContentSize::new(content_box_pt(100.0)),
            LogicalBlockContentSize::new(content_box_pt(240.0)),
        );
        let horizontal = TableAxes {
            flow: FlowAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
            direction: Direction::Ltr,
        };
        let vertical = TableAxes {
            flow: FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr),
            direction: Direction::Ltr,
        };

        assert_eq!(size.physical_width(horizontal).points(), 100.0);
        assert_eq!(size.physical_width(vertical).points(), 240.0);
    }

    #[test]
    fn cell_axis_adapter_keeps_root_track_axes_direction_independent() {
        for (writing_mode, root_block_axis) in [
            (WritingMode::HorizontalTb, PhysicalAxis::Vertical),
            (WritingMode::VerticalRl, PhysicalAxis::Horizontal),
            (WritingMode::VerticalLr, PhysicalAxis::Horizontal),
            (WritingMode::SidewaysRl, PhysicalAxis::Horizontal),
            (WritingMode::SidewaysLr, PhysicalAxis::Horizontal),
        ] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                let adapter = TableCellAxisAdapter {
                    table: FlowAxes::new(writing_mode, direction),
                    cell: FlowAxes::new(WritingMode::HorizontalTb, direction),
                };
                assert_eq!(
                    adapter.root_track_physical_axis(TableRootTrackAxis::Block),
                    root_block_axis
                );
            }
        }
    }

    #[test]
    fn cell_content_geometry_keeps_root_tracks_and_cell_flow_separate() {
        let content_box =
            TableCellContentBox::from_page_top_rect(PageTopRect::new(10.0, 200.0, 30.0, 40.0));
        for root_writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                let mut table = ComputedStyle::initial();
                table.writing_mode = root_writing_mode;
                table.direction = direction;

                let mut horizontal_cell = ComputedStyle::initial();
                horizontal_cell.writing_mode = WritingMode::HorizontalTb;
                horizontal_cell.direction = direction;
                let horizontal = TableCellAxisAdapter::for_cell(&table, &horizontal_cell)
                    .content_geometry(content_box, 5.0);
                assert_eq!(horizontal.inline_size().points(), 30.0);
                assert_eq!(horizontal.block_size().points(), 40.0);
                assert_eq!(horizontal.content_box().top_y(), 195.0);

                let mut vertical_cell = ComputedStyle::initial();
                vertical_cell.writing_mode = WritingMode::VerticalRl;
                vertical_cell.direction = direction;
                let vertical = TableCellAxisAdapter::for_cell(&table, &vertical_cell)
                    .content_geometry(content_box, 5.0);
                assert_eq!(vertical.inline_size().points(), 40.0);
                assert_eq!(vertical.block_size().points(), 30.0);
                assert_eq!(vertical.content_box().left(), 5.0);
            }
        }
    }

    #[test]
    fn full_grid_projection_is_root_flow_owned_for_every_mode_and_direction() {
        for writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                let axes = TableAxes {
                    flow: FlowAxes::new(writing_mode, direction),
                    direction,
                };
                let rect = axes.physical_rect_from_logical_grid(0.0, 0.0, 30.0, 40.0, 30.0, 40.0);

                // Direction reversal belongs to TableColumnPlan.  Projecting
                // the complete logical grid must therefore be identical for
                // both directions and begin at the grid placement origin.
                assert_eq!(rect.origin, ContainerPoint::new(0.0, 0.0));
                assert_eq!(
                    rect.size,
                    axes.flow.physical_size_from_logical(LogicalSize {
                        inline: 30.0,
                        block: 40.0,
                    })
                );
            }
        }
    }

    #[test]
    fn projects_row_bounds_to_page_coordinates() {
        let placement = TableGridPlacement::new(PageTopPoint::new(20.0, 200.0));
        let border_box = TableCellBorderBox::from_bounds(
            TableInlineBounds::new(grid_length(15.0), grid_length(60.0)),
            TableRowBounds::new(25.0, 30.0),
        );
        assert_eq!(border_box.x(placement), 35.0);
        assert_eq!(border_box.top_y(placement), 175.0);
        assert_eq!(
            border_box.top_y(placement) - border_box.rect().size.height,
            145.0
        );
        assert_eq!(
            placement.overflow_clip_for(border_box.rect()),
            OverflowClip::from_paint_rect(paint_space_rect(35.0, 145.0, 60.0, 30.0))
        );
    }

    #[test]
    fn vertical_row_source_rect_keeps_the_unfragmented_logical_block_range() {
        let placement = TableGridPlacement::with_axes(
            PageTopPoint::new(20.0, 200.0),
            TableAxes {
                flow: FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
                direction: Direction::Rtl,
            },
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(100.0)),
                LogicalBlockContentSize::new(content_box_pt(300.0)),
            ),
        );
        let source_row = placement.page_top_rect_for(TableGridRect::new(
            TableGridPoint::from_lengths(grid_length(0.0), grid_length(120.0)),
            TableGridSize::from_lengths(grid_length(100.0), grid_length(60.0)),
        ));

        // The table grid deliberately uses its source block extent, not a
        // fragment-local physical width. A later fragment clips this same
        // rectangle without resetting its background positioning area.
        assert_eq!(source_row, PageTopRect::new(140.0, 200.0, 60.0, 100.0));
    }

    #[test]
    fn grid_frames_remove_block_edge_spacing_once_in_every_root_flow() {
        for writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                let placement = TableGridPlacement::with_axes(
                    PageTopPoint::new(20.0, 200.0),
                    TableAxes {
                        flow: FlowAxes::new(writing_mode, direction),
                        direction,
                    },
                    TableGridLogicalSize::new(
                        LogicalInlineContentSize::new(content_box_pt(100.0)),
                        LogicalBlockContentSize::new(content_box_pt(300.0)),
                    ),
                );
                let frames = TableGridFrames::new(placement, grid_length(10.0));

                assert_eq!(frames.wrapper_grid(), placement);
                assert_eq!(
                    frames.cell_grid().logical_size.inline(),
                    placement.logical_size.inline()
                );
                assert_eq!(frames.cell_grid().logical_size.block().points(), 280.0);
                let expected_origin = if writing_mode.has_vertical_lines() {
                    PageTopPoint::new(30.0, 200.0)
                } else {
                    PageTopPoint::new(20.0, 190.0)
                };
                assert_eq!(frames.cell_grid().origin(), expected_origin);
            }
        }
    }

    #[test]
    fn cell_grid_reconstructs_its_wrapper_frame_without_a_second_rebase() {
        for writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                let wrapper = TableGridPlacement::with_axes(
                    PageTopPoint::new(20.0, 200.0),
                    TableAxes {
                        flow: FlowAxes::new(writing_mode, direction),
                        direction,
                    },
                    TableGridLogicalSize::new(
                        LogicalInlineContentSize::new(content_box_pt(100.0)),
                        LogicalBlockContentSize::new(content_box_pt(300.0)),
                    ),
                );
                let initial = TableGridFrames::new(wrapper, grid_length(10.0));
                let reconstructed =
                    TableGridFrames::from_cell_grid(initial.cell_grid(), grid_length(10.0));

                assert_eq!(reconstructed.wrapper_grid(), wrapper);
                assert_eq!(reconstructed.cell_grid(), initial.cell_grid());
            }
        }
    }
}
