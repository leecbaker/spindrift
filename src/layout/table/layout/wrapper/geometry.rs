use super::super::*;

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
pub(in crate::layout::table) struct TableWrapperBlockOffset(
    pub(in crate::layout::table) TableGridLength,
);

impl TableWrapperBlockOffset {
    pub(in crate::layout::table) fn zero() -> Self {
        Self(TableGridLength::new(0.0))
    }

    pub(super) fn add(self, size: TableGridLength) -> Self {
        Self(self.0 + size)
    }

    pub(in crate::layout::table) fn points(self) -> f32 {
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

    pub(super) fn length(self) -> TableGridLength {
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
pub(in crate::layout::table) struct TableWrapperGridStart {
    pub(in crate::layout::table) wrapper_border_start: TableWrapperBlockOffset,
    pub(in crate::layout::table) grid_content_start: TableWrapperBlockOffset,
}

impl TableWrapperGridStart {
    pub(super) fn new(
        wrapper_border_start: TableWrapperBlockOffset,
        block_start_chrome: TableRootBlockStartChrome,
    ) -> Self {
        Self {
            wrapper_border_start,
            grid_content_start: wrapper_border_start.add(block_start_chrome.length()),
        }
    }

    pub(super) fn grid_body_start(
        self,
        grid_block_start: TableGridBlockOffset,
    ) -> TableWrapperBlockOffset {
        self.grid_content_start.add(grid_block_start.length())
    }

    pub(super) fn root_source_frame(
        self,
        root_rect: TableGridRect,
    ) -> TableWrapperLocalRootSourceFrame {
        TableWrapperLocalRootSourceFrame::new(self.wrapper_border_start, root_rect)
    }
}

/// One source interval in wrapper block-flow order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableWrapperBlockInterval {
    pub(in crate::layout::table) start: TableWrapperBlockOffset,
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

    pub(in crate::layout::table) fn start(self) -> TableWrapperBlockOffset {
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
pub(in crate::layout::table) struct TableWrapperLocalRootSourceFrame {
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

    pub(in crate::layout::table) fn root_rect(self) -> TableGridRect {
        self.root_rect
    }

    pub(in crate::layout::table) fn local_block_start(self) -> TableWrapperBlockOffset {
        self.wrapper_interval.start()
    }

    pub(in crate::layout::table) fn block_span(self) -> TableGridLength {
        self.wrapper_interval.size()
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
