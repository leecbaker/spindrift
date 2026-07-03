use super::*;

/// Physical coordinates inside a CSS table grid box.
///
/// The origin is the table grid box's physical top-left corner, `x` increases
/// to the physical right, and `y` increases toward the physical bottom. This is
/// the coordinate space where CSS Tables lays out row and column tracks before
/// page painting projects them to Quire's existing `x`/`top_y` fields:
/// <https://drafts.csswg.org/css-tables-3/#table-layout-algorithm>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TableGridSpace {}

/// A point in table-grid physical coordinates.
pub(super) type TableGridPoint = euclid::Point2D<f32, TableGridSpace>;
/// A size in table-grid physical coordinates.
pub(super) type TableGridSize = euclid::Size2D<f32, TableGridSpace>;
/// An axis-aligned rectangle in table-grid physical coordinates.
pub(super) type TableGridRect = euclid::Rect<f32, TableGridSpace>;

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

impl TableAxes {
    pub(super) fn for_style(style: &ComputedStyle) -> Self {
        Self {
            flow: FlowAxes::for_style(style),
            direction: style.direction,
        }
    }

    #[cfg(test)]
    pub(super) const fn for_direction(direction: Direction) -> Self {
        Self {
            flow: FlowAxes::new(WritingMode::HorizontalTb, direction),
            direction,
        }
    }

    pub(super) fn boundary_x(self, total_width: f32, logical_boundary_x: f32) -> f32 {
        match self.direction {
            Direction::Ltr => logical_boundary_x,
            Direction::Rtl => total_width - logical_boundary_x,
        }
    }

    pub(super) fn span_start_x(
        self,
        total_width: f32,
        logical_start_x: f32,
        logical_end_x: f32,
    ) -> f32 {
        self.boundary_x(total_width, logical_start_x)
            .min(self.boundary_x(total_width, logical_end_x))
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
    pub(super) start: f32,
    pub(super) size: f32,
}

impl TableInlineBounds {
    pub(super) fn new(start: f32, size: f32) -> Self {
        Self {
            start,
            size: size.max(0.0),
        }
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
                TableGridPoint::new(inline.start, row.start),
                TableGridSize::new(inline.size, row.size),
            ),
        }
    }

    pub(super) fn rect(self) -> TableGridRect {
        self.rect
    }

    pub(super) fn width(self) -> f32 {
        self.rect.size.width
    }

    pub(super) fn height(self) -> f32 {
        self.rect.size.height
    }

    pub(super) fn x(self, placement: TableGridPlacement) -> f32 {
        placement.x_for(self.rect.origin.x)
    }

    pub(super) fn top_y(self, placement: TableGridPlacement) -> f32 {
        placement.top_y_for(self.rect.origin.y)
    }

    pub(super) fn content_box(
        self,
        placement: TableGridPlacement,
        padding: css::Edges,
        borders: css::Edges,
        content_offset: f32,
        content_x_offset: f32,
    ) -> TableCellContentBox {
        let x = self.x(placement) + borders.left + padding.left + content_x_offset;
        let right =
            self.x(placement) + self.width() - borders.right - padding.right + content_x_offset;
        let top_y = self.top_y(placement) - borders.top - padding.top - content_offset;
        let height =
            (self.height() - borders.top - borders.bottom - padding.top - padding.bottom).max(0.0);
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
}

impl TableGridPlacement {
    pub(super) fn new(origin: PageTopPoint) -> Self {
        Self { origin }
    }

    pub(super) fn x_for(self, grid_x: f32) -> f32 {
        self.origin.x() + grid_x
    }

    pub(super) fn top_y_for(self, grid_y: f32) -> f32 {
        self.origin.top_y() - grid_y
    }

    pub(super) fn overflow_clip_for(self, rect: TableGridRect) -> OverflowClip {
        OverflowClip::from_page_top_rect(PageTopRect::new(
            self.x_for(rect.origin.x),
            self.top_y_for(rect.origin.y),
            rect.size.width,
            rect.size.height,
        ))
    }

    pub(super) fn containing_block_for(
        self,
        border_box: TableCellBorderBox,
        borders: css::Edges,
    ) -> ContainingBlock {
        ContainingBlock::from_page_top_rect(PageTopRect::new(
            border_box.x(self) + borders.left,
            border_box.top_y(self) - borders.top,
            border_box.width() - borders.left - borders.right,
            border_box.height() - borders.top - borders.bottom,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_ltr_column_span_bounds() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        assert_eq!(axes.boundary_x(100.0, 25.0), 25.0);
        assert_eq!(axes.span_start_x(100.0, 10.0, 45.0), 10.0);
    }

    #[test]
    fn maps_rtl_boundaries_and_spans_from_the_right() {
        let axes = TableAxes::for_direction(Direction::Rtl);
        assert_eq!(axes.boundary_x(100.0, 0.0), 100.0);
        assert_eq!(axes.boundary_x(100.0, 25.0), 75.0);
        assert_eq!(axes.span_start_x(100.0, 10.0, 45.0), 55.0);
    }

    #[test]
    fn projects_row_bounds_to_page_coordinates() {
        let placement = TableGridPlacement::new(PageTopPoint::new(20.0, 200.0));
        let border_box = TableCellBorderBox::from_bounds(
            TableInlineBounds::new(15.0, 60.0),
            TableRowBounds::new(25.0, 30.0),
        );
        assert_eq!(border_box.x(placement), 35.0);
        assert_eq!(border_box.top_y(placement), 175.0);
        assert_eq!(border_box.top_y(placement) - border_box.height(), 145.0);
        assert_eq!(
            placement.overflow_clip_for(border_box.rect()),
            OverflowClip::from_paint_rect(paint_space_rect(35.0, 145.0, 60.0, 30.0))
        );
    }
}
