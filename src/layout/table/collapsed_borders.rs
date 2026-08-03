use super::*;

pub(super) struct CollapsedBorderGrid {
    horizontal: Vec<Vec<Option<CollapsedBorder>>>,
    vertical: Vec<Vec<Option<CollapsedBorder>>>,
    /// The table root alone determines how physical border sides map to
    /// logical grid edges.  Descendant `direction` values do not reorder the
    /// table's column grid.
    direction: Direction,
    column_count: usize,
}

/// One collapsed-border grid-line segment in page top-edge coordinates.
///
/// CSS Tables centers collapsed borders on grid lines. A horizontal segment
/// therefore has zero block extent at its physical top-edge coordinate, while
/// a vertical segment has zero inline extent and an explicit downward block
/// extent. Keeping this as [`PageTopRect`] prevents a horizontal line's `y`
/// coordinate from being passed to paint APIs as though it were a bottom edge:
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>.
#[derive(Debug, Clone, Copy)]
pub(super) struct CollapsedBorderSegment {
    rect: PageTopRect,
    orientation: CollapsedBorderOrientation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CollapsedBorderOrientation {
    Horizontal,
    Vertical,
}

impl CollapsedBorderSegment {
    #[cfg(test)]
    fn horizontal(left: f32, line_y: f32, width: f32) -> Self {
        Self {
            rect: PageTopRect::new(left, line_y, width, 0.0),
            orientation: CollapsedBorderOrientation::Horizontal,
        }
    }

    #[cfg(test)]
    fn vertical(center_x: f32, top: f32, height: f32) -> Self {
        Self {
            rect: PageTopRect::new(center_x, top, 0.0, height),
            orientation: CollapsedBorderOrientation::Vertical,
        }
    }

    /// Classify a zero-thickness projected grid line by its physical span.
    ///
    /// The table grid stays logical until this final adapter. A logical column
    /// boundary is vertical in horizontal writing but horizontal in vertical
    /// writing, so callers must not select the paint orientation themselves.
    pub(super) fn from_projected_line(rect: PageTopRect) -> Self {
        let orientation = if rect.width() >= rect.height() {
            CollapsedBorderOrientation::Horizontal
        } else {
            CollapsedBorderOrientation::Vertical
        };
        Self { rect, orientation }
    }

    /// Return the physical cross-axis position used to paint adjacent
    /// collapsed-border segments in a stable page order.
    ///
    /// The grid remains in source-logical order through conflict resolution,
    /// but adjacent centered rules overlap at their joins.  Painting them in
    /// source order would reverse that overlap for an RTL table whose columns
    /// are otherwise physically equivalent to an LTR table with reversed DOM
    /// cells.  Select the physical coordinate only after projection so the
    /// same rule covers horizontal and vertical table roots:
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    fn physical_paint_order_position(self) -> f32 {
        match self.orientation {
            CollapsedBorderOrientation::Horizontal => self.rect.top_y(),
            CollapsedBorderOrientation::Vertical => self.rect.x(),
        }
    }
}

/// One resolved collapsed-border rule ready for page paint.
///
/// The grid owns conflict resolution and source-logical indexing; this record
/// is the narrow boundary where the resolved line is put into physical page
/// paint order so coincident joins raster consistently.
#[derive(Debug, Clone, Copy)]
struct CollapsedBorderPaintCommand {
    border: CollapsedBorder,
    segment: CollapsedBorderSegment,
}

impl CollapsedBorderPaintCommand {
    fn physical_paint_order(self) -> f32 {
        self.segment.physical_paint_order_position()
    }

    /// Return the start of this rule's physical painted span.
    ///
    /// All segments generated for one horizontal grid boundary meet at their
    /// ends. Their logical column order reverses for RTL tables, so owning an
    /// antialiased join in that order would make otherwise equivalent LTR and
    /// RTL tables raster differently. Select the physical span only after
    /// writing-mode projection.
    fn physical_span_start(self) -> f32 {
        match self.segment.orientation {
            CollapsedBorderOrientation::Horizontal => self.segment.rect.x(),
            CollapsedBorderOrientation::Vertical => self.segment.rect.top_y(),
        }
    }
}

fn sort_collapsed_border_paint_commands(commands: &mut [CollapsedBorderPaintCommand]) {
    commands.sort_by(|left, right| {
        left.physical_paint_order()
            .total_cmp(&right.physical_paint_order())
    });
}

fn sort_collapsed_boundary_segments(commands: &mut [CollapsedBorderPaintCommand]) {
    commands.sort_by(|left, right| {
        left.physical_span_start()
            .total_cmp(&right.physical_span_start())
    });
}

impl CollapsedBorderGrid {
    pub(super) fn new(row_count: usize, column_count: usize, direction: Direction) -> Self {
        Self {
            horizontal: vec![vec![None; column_count]; row_count + 1],
            vertical: vec![vec![None; column_count + 1]; row_count],
            direction,
            column_count,
        }
    }

    pub(super) fn add_table(
        &mut self,
        style: &ComputedStyle,
        row_count: usize,
        column_count: usize,
    ) {
        for column in 0..column_count {
            self.add_horizontal(0, column, style, BorderSide::Top, BorderOrigin::Table, 0);
            self.add_horizontal(
                row_count,
                column,
                style,
                BorderSide::Bottom,
                BorderOrigin::Table,
                0,
            );
        }
        for row in 0..row_count {
            self.add_vertical(
                row,
                self.inline_left_boundary(0, column_count),
                style,
                BorderSide::Left,
                BorderOrigin::Table,
            );
            self.add_vertical(
                row,
                self.inline_right_boundary(0, column_count),
                style,
                BorderSide::Right,
                BorderOrigin::Table,
            );
        }
    }

    pub(super) fn add_row(&mut self, row: usize, column_count: usize, style: &ComputedStyle) {
        for column in 0..column_count {
            self.add_horizontal(row, column, style, BorderSide::Top, BorderOrigin::Row, row);
            self.add_horizontal(
                row + 1,
                column,
                style,
                BorderSide::Bottom,
                BorderOrigin::Row,
                row,
            );
        }
        self.add_vertical(
            row,
            self.inline_left_boundary(0, column_count),
            style,
            BorderSide::Left,
            BorderOrigin::Row,
        );
        self.add_vertical(
            row,
            self.inline_right_boundary(0, column_count),
            style,
            BorderSide::Right,
            BorderOrigin::Row,
        );
    }

    pub(super) fn add_row_group(
        &mut self,
        start_row: usize,
        end_row: usize,
        column_count: usize,
        style: &ComputedStyle,
    ) {
        for column in 0..column_count {
            self.add_horizontal(
                start_row,
                column,
                style,
                BorderSide::Top,
                BorderOrigin::RowGroup,
                start_row,
            );
            self.add_horizontal(
                end_row,
                column,
                style,
                BorderSide::Bottom,
                BorderOrigin::RowGroup,
                start_row,
            );
        }
        for row in start_row..end_row {
            self.add_vertical(
                row,
                self.inline_left_boundary(0, column_count),
                style,
                BorderSide::Left,
                BorderOrigin::RowGroup,
            );
            self.add_vertical(
                row,
                self.inline_right_boundary(0, column_count),
                style,
                BorderSide::Right,
                BorderOrigin::RowGroup,
            );
        }
    }

    pub(super) fn add_column_group(
        &mut self,
        start_column: usize,
        end_column: usize,
        row_count: usize,
        style: &ComputedStyle,
    ) {
        self.add_column_origin(
            start_column,
            end_column,
            row_count,
            style,
            BorderOrigin::ColumnGroup,
        );
    }

    pub(super) fn add_column(
        &mut self,
        start_column: usize,
        end_column: usize,
        row_count: usize,
        style: &ComputedStyle,
    ) {
        self.add_column_origin(
            start_column,
            end_column,
            row_count,
            style,
            BorderOrigin::Column,
        );
    }

    /// Add collapsed-border candidates generated by a column or column group.
    ///
    /// CSS 2.2 includes column and column-group borders in collapsed border
    /// conflict resolution. Their column boxes cover the full table grid in
    /// the block axis, so they contribute both inline-edge vertical candidates
    /// and block-edge horizontal candidates for every covered column segment.
    /// <https://www.w3.org/TR/CSS22/tables.html#border-conflict-resolution>
    pub(super) fn add_column_origin(
        &mut self,
        start_column: usize,
        end_column: usize,
        row_count: usize,
        style: &ComputedStyle,
        origin: BorderOrigin,
    ) {
        for column in start_column..end_column {
            let tie_position = self.physical_column_position(column);
            self.add_horizontal(0, column, style, BorderSide::Top, origin, tie_position);
            self.add_horizontal(
                row_count,
                column,
                style,
                BorderSide::Bottom,
                origin,
                tie_position,
            );
        }
        for row in 0..row_count {
            self.add_vertical(
                row,
                self.inline_left_boundary(start_column, end_column),
                style,
                BorderSide::Left,
                origin,
            );
            self.add_vertical(
                row,
                self.inline_right_boundary(start_column, end_column),
                style,
                BorderSide::Right,
                origin,
            );
        }
    }

    pub(super) fn add_cell(
        &mut self,
        row: usize,
        column: usize,
        colspan: usize,
        rowspan: usize,
        style: &ComputedStyle,
    ) {
        if column >= self.column_count {
            return;
        }
        let column_end = (column + colspan.max(1)).min(self.column_count);
        let row_end = (row + rowspan.max(1)).min(self.horizontal.len().saturating_sub(1));
        for segment_column in column..column_end {
            self.add_horizontal(
                row,
                segment_column,
                style,
                BorderSide::Top,
                BorderOrigin::Cell,
                row,
            );
            self.add_horizontal(
                row_end,
                segment_column,
                style,
                BorderSide::Bottom,
                BorderOrigin::Cell,
                row,
            );
        }
        for boundary in row + 1..row_end {
            for segment_column in column..column_end {
                // CSS collapsed borders have no internal cell edge through a row-spanning cell.
                // A strong null cell-origin candidate suppresses lower-precedence row borders.
                self.add_strong_null_horizontal(boundary, segment_column, row);
            }
        }
        for segment_row in row..row_end {
            for boundary in column + 1..column_end {
                // CSS collapsed borders also have no internal cell edge through a
                // column-spanning cell. Suppress column and column-group borders there.
                self.add_strong_null_vertical(segment_row, boundary, column);
            }
            self.add_vertical(
                segment_row,
                self.inline_left_boundary(column, column_end),
                style,
                BorderSide::Left,
                BorderOrigin::Cell,
            );
            self.add_vertical(
                segment_row,
                self.inline_right_boundary(column, column_end),
                style,
                BorderSide::Right,
                BorderOrigin::Cell,
            );
        }
    }

    /// Add a hidden cell-origin candidate for an internal spanning-cell edge.
    ///
    /// CSS 2.2 border conflict resolution lets `hidden` suppress all
    /// conflicting borders. The table model has no real border inside a
    /// spanning cell, so this durable null candidate prevents row, column, and
    /// group borders from painting through the cell interior.
    /// <https://www.w3.org/TR/CSS22/tables.html#border-conflict-resolution>
    fn add_strong_null_horizontal(&mut self, boundary: usize, column: usize, tie_position: usize) {
        if let Some(row) = self.horizontal.get_mut(boundary)
            && let Some(edge) = row.get_mut(column)
        {
            resolve_collapsed_border(
                edge,
                CollapsedBorder::strong_null(BorderOrigin::Cell, tie_position),
            );
        }
    }

    /// Add a hidden cell-origin candidate for an internal spanning-cell edge.
    ///
    /// This is the inline-axis counterpart of `add_strong_null_horizontal` and
    /// is required for colspans so column borders do not leak through a cell.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
    fn add_strong_null_vertical(&mut self, row: usize, boundary: usize, tie_position: usize) {
        let tie_position = self.physical_boundary_position(tie_position);
        if let Some(row_edges) = self.vertical.get_mut(row)
            && let Some(edge) = row_edges.get_mut(boundary)
        {
            resolve_collapsed_border(
                edge,
                CollapsedBorder::strong_null(BorderOrigin::Cell, tie_position),
            );
        }
    }

    pub(super) fn add_horizontal(
        &mut self,
        boundary: usize,
        column: usize,
        style: &ComputedStyle,
        side: BorderSide,
        origin: BorderOrigin,
        tie_position: usize,
    ) {
        if let Some(row) = self.horizontal.get_mut(boundary)
            && let Some(edge) = row.get_mut(column)
        {
            resolve_collapsed_border(
                edge,
                CollapsedBorder::from_style(style, side, origin, tie_position),
            );
        }
    }

    pub(super) fn add_vertical(
        &mut self,
        row: usize,
        boundary: usize,
        style: &ComputedStyle,
        side: BorderSide,
        origin: BorderOrigin,
    ) {
        let tie_position = self.physical_boundary_position(boundary);
        if let Some(row_edges) = self.vertical.get_mut(row)
            && let Some(edge) = row_edges.get_mut(boundary)
        {
            resolve_collapsed_border(
                edge,
                CollapsedBorder::from_style(style, side, origin, tie_position),
            );
        }
    }

    fn inline_left_boundary(&self, start_column: usize, end_column: usize) -> usize {
        match self.direction {
            Direction::Ltr => start_column,
            Direction::Rtl => end_column,
        }
    }

    fn inline_right_boundary(&self, start_column: usize, end_column: usize) -> usize {
        match self.direction {
            Direction::Ltr => end_column,
            Direction::Rtl => start_column,
        }
    }

    fn physical_boundary_position(&self, boundary: usize) -> usize {
        match self.direction {
            Direction::Ltr => boundary,
            Direction::Rtl => self.column_count.saturating_sub(boundary),
        }
    }

    fn physical_column_position(&self, column: usize) -> usize {
        match self.direction {
            Direction::Ltr => column,
            Direction::Rtl => self.column_count.saturating_sub(column + 1),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_fragment_rows(
        &self,
        placement: TableGridPlacement,
        horizontal_page_placement: TableGridPlacement,
        column_plan: &TableColumnPlan,
        original_rows: &[usize],
        row_tops: &[f32],
        row_heights: &[f32],
        row_offsets: &[f32],
        original_row_heights: &[f32],
        row_bounds: Option<&[TableRowBounds]>,
    ) -> (Vec<RenderedRect>, Vec<RenderedPath>) {
        let mut rects = Vec::new();
        let mut paths = Vec::new();
        let mut painted_boundaries = Vec::new();
        let vertical_root = placement.writing_mode().has_vertical_lines();

        for (local_row, original_row) in original_rows.iter().cloned().enumerate() {
            let (Some(top), Some(height), Some(offset), Some(original_height)) = (
                row_tops.get(local_row).cloned(),
                row_heights.get(local_row).cloned(),
                row_offsets.get(local_row).cloned(),
                original_row_heights.get(local_row).cloned(),
            ) else {
                continue;
            };
            let real_top = offset <= 0.01;
            let real_bottom = offset + height >= original_height - 0.01;
            let segment_placement = if vertical_root {
                placement
            } else {
                horizontal_page_placement
            };
            let source_block_start = if vertical_root {
                let Some(source_row) = row_bounds
                    .and_then(|bounds| bounds.get(original_row))
                    .copied()
                else {
                    continue;
                };
                source_row.start + offset
            } else {
                // Fragment row tops are already committed page geometry for
                // horizontal tables. Re-label their page-top coordinate as a
                // logical block offset only at the line projection boundary.
                horizontal_page_placement
                    .block_offset_from_page_top(top)
                    .length()
                    .get()
            };
            if real_top {
                self.paint_horizontal_boundary(
                    &mut rects,
                    &mut paths,
                    segment_placement,
                    column_plan,
                    original_row,
                    TableGridBlockOffset::new(TableGridLength::new(source_block_start)),
                    &mut painted_boundaries,
                );
            }
            if real_bottom {
                self.paint_horizontal_boundary(
                    &mut rects,
                    &mut paths,
                    segment_placement,
                    column_plan,
                    original_row + 1,
                    TableGridBlockOffset::new(TableGridLength::new(source_block_start + height)),
                    &mut painted_boundaries,
                );
            }

            let Some(row) = self.vertical.get(original_row) else {
                continue;
            };
            let mut vertical_paint_commands = Vec::new();
            for (boundary, border) in row.iter().enumerate() {
                let Some(border) = border else {
                    continue;
                };
                let top_extension = if real_top {
                    self.horizontal_intersection_half_width(original_row, boundary)
                } else {
                    0.0
                };
                let bottom_extension = if real_bottom {
                    self.horizontal_intersection_half_width(original_row + 1, boundary)
                } else {
                    0.0
                };
                let block_start = source_block_start - top_extension;
                vertical_paint_commands.push(CollapsedBorderPaintCommand {
                    border: *border,
                    segment: segment_placement.project_inline_line(
                        TableGridInlineOffset::new(column_plan.boundary_x(boundary)),
                        TableGridBlockOffset::new(TableGridLength::new(block_start)),
                        TableGridBlockOffset::new(TableGridLength::new(
                            height + top_extension + bottom_extension,
                        )),
                    ),
                });
            }
            sort_collapsed_border_paint_commands(&mut vertical_paint_commands);
            for command in vertical_paint_commands {
                command
                    .border
                    .paint_vertical(&mut rects, &mut paths, command.segment);
            }
        }

        (rects, paths)
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_horizontal_boundary(
        &self,
        rects: &mut Vec<RenderedRect>,
        paths: &mut Vec<RenderedPath>,
        placement: TableGridPlacement,
        column_plan: &TableColumnPlan,
        boundary: usize,
        block: TableGridBlockOffset,
        painted_boundaries: &mut Vec<usize>,
    ) {
        if painted_boundaries.contains(&boundary) {
            return;
        }
        painted_boundaries.push(boundary);
        let Some(row) = self.horizontal.get(boundary) else {
            return;
        };
        let row_count = self.vertical.len();
        let mut commands = Vec::new();
        for (column, border) in row.iter().enumerate() {
            let Some(border) = border else {
                continue;
            };
            let before_extension =
                self.vertical_intersection_half_width(boundary, column, row_count);
            let after_extension =
                self.vertical_intersection_half_width(boundary, column + 1, row_count);
            // `TableInlineBounds` describes the cell border box after its
            // resolved collapsed insets. A collapsed horizontal rule instead
            // runs between the underlying grid lines, just like its vertical
            // counterpart. Starting from the boundary positions keeps outer
            // corner joins aligned with the centered vertical rules.
            // <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
            let first_boundary = column_plan.boundary_x(column);
            let second_boundary = column_plan.boundary_x(column + 1);
            let (inline_start, inline_end) = if first_boundary <= second_boundary {
                (
                    first_boundary - TableGridLength::new(before_extension),
                    second_boundary + TableGridLength::new(after_extension),
                )
            } else {
                // Column-plan boundaries are physical and decrease for RTL.
                // The logical start edge is therefore the physical right edge;
                // preserve the border-joint extensions while emitting the
                // non-negative physical line span required by page paint.
                (
                    second_boundary - TableGridLength::new(after_extension),
                    first_boundary + TableGridLength::new(before_extension),
                )
            };
            commands.push(CollapsedBorderPaintCommand {
                border: *border,
                segment: placement.project_block_line(
                    TableGridInlineOffset::new(inline_start),
                    TableGridInlineOffset::new(inline_end - inline_start),
                    block,
                ),
            });
        }
        sort_collapsed_boundary_segments(&mut commands);
        for command in commands {
            command
                .border
                .paint_horizontal(rects, paths, command.segment);
        }
    }

    /// Return half of the widest vertical border touching a horizontal segment end.
    ///
    /// CSS 2.2 paints collapsed borders centered on table grid lines. At grid
    /// intersections, a horizontal segment extends by half the adjacent
    /// vertical border width so collapsed border joins do not leave gaps.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
    fn vertical_intersection_half_width(
        &self,
        horizontal_boundary: usize,
        vertical_boundary: usize,
        row_count: usize,
    ) -> f32 {
        let before = horizontal_boundary
            .checked_sub(1)
            .and_then(|row| self.vertical_border_width(row, vertical_boundary));
        let after = (horizontal_boundary < row_count)
            .then(|| self.vertical_border_width(horizontal_boundary, vertical_boundary))
            .flatten();
        before.into_iter().chain(after).fold(0.0_f32, f32::max) / 2.0
    }

    /// Return half of the widest horizontal border touching a vertical segment end.
    ///
    /// CSS 2.2 collapsed borders are centered on grid lines. Vertical segments
    /// therefore extend by half of the adjacent horizontal border at each
    /// intersection, matching the geometry used by WeasyPrint.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
    fn horizontal_intersection_half_width(
        &self,
        horizontal_boundary: usize,
        vertical_boundary: usize,
    ) -> f32 {
        let before = vertical_boundary
            .checked_sub(1)
            .and_then(|column| self.horizontal_border_width(horizontal_boundary, column));
        let after = self.horizontal_border_width(horizontal_boundary, vertical_boundary);
        before.into_iter().chain(after).fold(0.0_f32, f32::max) / 2.0
    }

    fn vertical_border_width(&self, row: usize, boundary: usize) -> Option<f32> {
        self.vertical
            .get(row)?
            .get(boundary)?
            .map(|border| border.used_side().used_width.get())
    }

    fn horizontal_border_width(&self, boundary: usize, column: usize) -> Option<f32> {
        self.horizontal
            .get(boundary)?
            .get(column)?
            .map(|border| border.used_side().used_width.get())
    }

    /// Return the cell border insets contributed by the resolved collapsed grid.
    ///
    /// CSS 2.2 collapsed borders are centered on grid lines; table-cell content
    /// layout therefore consumes half of the winning border on each outside
    /// edge of the cell span, rather than the cell's authored border widths.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
    pub(super) fn cell_insets(
        &self,
        row: usize,
        column: usize,
        colspan: usize,
        rowspan: usize,
    ) -> css::Edges {
        let row_end = (row + rowspan.max(1)).min(self.horizontal.len().saturating_sub(1));
        let column_end = (column + colspan.max(1)).min(self.column_count);
        let top = (column..column_end)
            .filter_map(|segment_column| self.horizontal_border_width(row, segment_column))
            .fold(0.0_f32, f32::max)
            / 2.0;
        let bottom = (column..column_end)
            .filter_map(|segment_column| self.horizontal_border_width(row_end, segment_column))
            .fold(0.0_f32, f32::max)
            / 2.0;
        let left_boundary = self.inline_left_boundary(column, column_end);
        let right_boundary = self.inline_right_boundary(column, column_end);
        let left = (row..row_end)
            .filter_map(|segment_row| self.vertical_border_width(segment_row, left_boundary))
            .fold(0.0_f32, f32::max)
            / 2.0;
        let right = (row..row_end)
            .filter_map(|segment_row| self.vertical_border_width(segment_row, right_boundary))
            .fold(0.0_f32, f32::max)
            / 2.0;

        css::Edges {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Return the table wrapper insets created by collapsed outer grid borders.
    ///
    /// CSS 2.2 centers collapsed borders on grid lines, and CSS Tables 3
    /// paints table-root backgrounds and borders around the grid plus the
    /// collapsed border widths that occupy that visual grid area. Use the
    /// largest half-width crossing each outer grid edge so later rows with
    /// wider outer borders still expand the collapsed table's painted area.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders> and
    /// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds-and-borders>
    pub(super) fn outer_insets(&self) -> css::Edges {
        let top = self
            .horizontal
            .first()
            .into_iter()
            .flat_map(|row| row.iter())
            .filter_map(|border| border.map(|border| border.used_side().used_width.get()))
            .fold(0.0_f32, f32::max)
            / 2.0;
        let bottom = self
            .horizontal
            .last()
            .into_iter()
            .flat_map(|row| row.iter())
            .filter_map(|border| border.map(|border| border.used_side().used_width.get()))
            .fold(0.0_f32, f32::max)
            / 2.0;
        // The grid is indexed in logical column order, while these insets are
        // physical box-model edges.  `direction: rtl` reverses the physical
        // placement of column zero, so using `first`/`last` directly here
        // would apply the outer borders to the wrong physical sides and shift
        // every collapsed border join.
        // <https://www.w3.org/TR/css-writing-modes-4/#direction>
        let left_boundary = self.inline_left_boundary(0, self.column_count);
        let right_boundary = self.inline_right_boundary(0, self.column_count);
        let left = self
            .vertical
            .iter()
            .filter_map(|row| row.get(left_boundary))
            .filter_map(|border| border.map(|border| border.used_side().used_width.get()))
            .fold(0.0_f32, f32::max)
            / 2.0;
        let right = self
            .vertical
            .iter()
            .filter_map(|row| row.get(right_boundary))
            .filter_map(|border| border.map(|border| border.used_side().used_width.get()))
            .fold(0.0_f32, f32::max)
            / 2.0;

        css::Edges {
            top,
            right,
            bottom,
            left,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BorderSide {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum BorderOrigin {
    Table,
    ColumnGroup,
    Column,
    RowGroup,
    Row,
    Cell,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CollapsedBorder {
    width: LayoutLength,
    style: BorderStyle,
    color: CssColor,
    side: BorderSide,
    origin: BorderOrigin,
    tie_position: usize,
}

impl CollapsedBorder {
    pub(super) fn from_style(
        style: &ComputedStyle,
        side: BorderSide,
        origin: BorderOrigin,
        tie_position: usize,
    ) -> Self {
        let (width, border_style, color) = match side {
            BorderSide::Top => (
                style.border_widths.top,
                style.border_styles.top,
                style.border_colors.top,
            ),
            BorderSide::Right => (
                style.border_widths.right,
                style.border_styles.right,
                style.border_colors.right,
            ),
            BorderSide::Bottom => (
                style.border_widths.bottom,
                style.border_styles.bottom,
                style.border_colors.bottom,
            ),
            BorderSide::Left => (
                style.border_widths.left,
                style.border_styles.left,
                style.border_colors.left,
            ),
        };
        Self {
            width: layout_pt(width.max(0.0)),
            style: border_style,
            color,
            side,
            origin,
            tie_position,
        }
    }

    fn strong_null(origin: BorderOrigin, tie_position: usize) -> Self {
        Self {
            width: layout_pt(0.0),
            style: BorderStyle::Hidden,
            color: CssColor::TRANSPARENT,
            side: BorderSide::Top,
            origin,
            tie_position,
        }
    }

    fn used_side(self) -> UsedBorderSide {
        UsedBorderSide::new(self.width, self.style, self.color)
    }

    pub(super) fn paint_horizontal(
        self,
        rects: &mut Vec<RenderedRect>,
        paths: &mut Vec<RenderedPath>,
        segment: CollapsedBorderSegment,
    ) {
        let border = self.used_side();
        if !border.is_visible() {
            return;
        }
        paint_collapsed_border_side(rects, paths, self.side.border_edge(), segment, border);
    }

    pub(super) fn paint_vertical(
        self,
        rects: &mut Vec<RenderedRect>,
        paths: &mut Vec<RenderedPath>,
        segment: CollapsedBorderSegment,
    ) {
        let border = self.used_side();
        if !border.is_visible() {
            return;
        }
        paint_collapsed_border_side(rects, paths, self.side.border_edge(), segment, border);
    }
}

impl BorderSide {
    fn border_edge(self) -> BorderEdge {
        match self {
            BorderSide::Top => BorderEdge::Top,
            BorderSide::Right => BorderEdge::Right,
            BorderSide::Bottom => BorderEdge::Bottom,
            BorderSide::Left => BorderEdge::Left,
        }
    }
}

pub(super) fn resolve_collapsed_border(
    edge: &mut Option<CollapsedBorder>,
    candidate: CollapsedBorder,
) {
    match edge {
        Some(current) if collapsed_border_wins(candidate, *current) => *current = candidate,
        None => *edge = Some(candidate),
        _ => {}
    }
}

pub(super) fn collapsed_border_wins(candidate: CollapsedBorder, current: CollapsedBorder) -> bool {
    if candidate.style == BorderStyle::Hidden {
        return current.style != BorderStyle::Hidden
            || candidate.origin > current.origin
            || (candidate.origin == current.origin
                && candidate.tie_position < current.tie_position);
    }
    if current.style == BorderStyle::Hidden {
        return false;
    }

    let candidate_none =
        candidate.style.suppresses_used_width() || candidate.width <= layout_pt(0.0);
    let current_none = current.style.suppresses_used_width() || current.width <= layout_pt(0.0);
    if candidate_none != current_none {
        return !candidate_none;
    }

    let candidate_width_priority = collapsed_border_width_priority(candidate.width);
    let current_width_priority = collapsed_border_width_priority(current.width);
    if candidate_width_priority != current_width_priority {
        return candidate_width_priority > current_width_priority;
    }

    let candidate_style = collapsed_border_style_priority(candidate.style);
    let current_style = collapsed_border_style_priority(current.style);
    if candidate_style != current_style {
        return candidate_style > current_style;
    }

    if candidate.origin != current.origin {
        return candidate.origin > current.origin;
    }

    candidate.tie_position < current.tie_position
}

/// Return the width priority key for collapsed-border conflict resolution.
///
/// CSS 2.2 resolves collapsed-border conflicts by preferring wider borders
/// before style and origin specificity. CSS Tables 3 defines that comparison
/// after converting widths into CSS pixels, and interoperable subpixel WPTs
/// floor those CSS-pixel widths before comparing specificity.
/// <https://www.w3.org/TR/CSS22/tables.html#border-conflict-resolution> and
/// <https://drafts.csswg.org/css-tables-3/#border-conflict-resolution-algorithm>
fn collapsed_border_width_priority(width: LayoutLength) -> i32 {
    (width.get().max(0.0) / css::CSS_PX_TO_PT).floor() as i32
}

pub(super) fn collapsed_border_style_priority(style: BorderStyle) -> u8 {
    match style {
        BorderStyle::Hidden => 10,
        BorderStyle::Double => 9,
        BorderStyle::Solid => 8,
        BorderStyle::Dashed => 7,
        BorderStyle::Dotted => 6,
        BorderStyle::Ridge => 5,
        BorderStyle::Outset => 4,
        BorderStyle::Groove => 3,
        BorderStyle::Inset => 2,
        BorderStyle::None => 1,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_collapsed_border_side(
    rects: &mut Vec<RenderedRect>,
    paths: &mut Vec<RenderedPath>,
    edge: BorderEdge,
    segment: CollapsedBorderSegment,
    border: UsedBorderSide,
) {
    if !border.is_visible() {
        return;
    }
    let cross_width = border.used_width.get();
    let (axis_start, axis_length, cross_start, horizontal) = match segment.orientation {
        CollapsedBorderOrientation::Horizontal => (
            segment.rect.x(),
            segment.rect.width(),
            segment.rect.top_y() - cross_width / 2.0,
            true,
        ),
        CollapsedBorderOrientation::Vertical => (
            segment.rect.bottom_y(),
            segment.rect.height(),
            segment.rect.x() - cross_width / 2.0,
            false,
        ),
    };
    let style = collapsed_border_paint_style(border.style);
    if style == BorderStyle::Double
        && let Some(bands) = DoubleBorderBands::for_used_width(border.used_width)
    {
        let stripe = bands.stripe.get();
        super::push_border_rect(
            rects,
            axis_start,
            axis_length,
            cross_start,
            stripe,
            horizontal,
            border.color,
        );
        super::push_border_rect(
            rects,
            axis_start,
            axis_length,
            cross_start + cross_width - stripe,
            stripe,
            horizontal,
            border.color,
        );
        return;
    }

    match style {
        BorderStyle::Dashed => super::paint_patterned_border_side(
            rects,
            axis_start,
            axis_length,
            cross_start,
            cross_width,
            horizontal,
            (cross_width * 3.0).max(1.0),
            (cross_width * 3.0).max(1.0),
            border.color,
        ),
        BorderStyle::Dotted => super::paint_dotted_border_side(
            paths,
            axis_start,
            axis_length,
            cross_start,
            PaintStrokeWidth::new(cross_width),
            horizontal,
            border.color,
        ),
        BorderStyle::Groove | BorderStyle::Ridge => super::paint_groove_ridge_border_side(
            rects,
            edge,
            axis_start,
            axis_length,
            cross_start,
            cross_width,
            horizontal,
            style,
            border.color,
        ),
        _ => super::push_border_rect(
            rects,
            axis_start,
            axis_length,
            cross_start,
            cross_width,
            horizontal,
            border.color,
        ),
    }
}

/// Return the paint style for a resolved collapsed border.
///
/// CSS 2.2 keeps `inset` and `outset` in conflict resolution but says that in
/// the collapsing border model they are rendered as `ridge` and `groove`.
/// <https://www.w3.org/TR/CSS22/tables.html#border-conflict-resolution>
fn collapsed_border_paint_style(style: BorderStyle) -> BorderStyle {
    match style {
        BorderStyle::Inset => BorderStyle::Ridge,
        BorderStyle::Outset => BorderStyle::Groove,
        _ => style,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style_with_solid_border(width_css_px: f32, color: CssColor) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        let width = width_css_px * css::CSS_PX_TO_PT;
        style.border_width = width;
        style.border_widths = css::Edges {
            top: width,
            right: width,
            bottom: width,
            left: width,
        };
        style.border_styles = css::BorderStyles {
            top: BorderStyle::Solid,
            right: BorderStyle::Solid,
            bottom: BorderStyle::Solid,
            left: BorderStyle::Solid,
        };
        style.border_colors = css::BorderColors {
            top: color,
            right: color,
            bottom: color,
            left: color,
        };
        style
    }

    fn solid_border(width_css_px: f32, origin: BorderOrigin) -> CollapsedBorder {
        CollapsedBorder {
            width: layout_pt(width_css_px * css::CSS_PX_TO_PT),
            style: BorderStyle::Solid,
            color: CssColor::BLACK,
            side: BorderSide::Top,
            origin,
            tie_position: 0,
        }
    }

    fn vertical_paint_command(x: f32, color: CssColor) -> CollapsedBorderPaintCommand {
        CollapsedBorderPaintCommand {
            border: CollapsedBorder {
                width: layout_pt(2.0 * css::CSS_PX_TO_PT),
                style: BorderStyle::Solid,
                color,
                side: BorderSide::Left,
                origin: BorderOrigin::Cell,
                tie_position: 0,
            },
            segment: CollapsedBorderSegment::vertical(x, 20.0, 40.0),
        }
    }

    #[test]
    fn collapsed_vertical_rules_paint_in_the_same_physical_order_for_ltr_and_rtl() {
        let green = CssColor::new(0, 128, 0);
        let blue = CssColor::new(0, 0, 255);
        let black = CssColor::BLACK;
        let mut ltr = vec![
            vertical_paint_command(10.0, green),
            vertical_paint_command(20.0, blue),
            vertical_paint_command(30.0, black),
        ];
        // An RTL grid visits those same physical boundaries in reverse logical
        // column order.  Its paint result must nevertheless be identical.
        let mut rtl = vec![
            vertical_paint_command(30.0, black),
            vertical_paint_command(20.0, blue),
            vertical_paint_command(10.0, green),
        ];

        sort_collapsed_border_paint_commands(&mut ltr);
        sort_collapsed_border_paint_commands(&mut rtl);

        let colors = |commands: &[CollapsedBorderPaintCommand]| {
            commands
                .iter()
                .map(|command| command.border.color)
                .collect::<Vec<_>>()
        };
        assert_eq!(colors(&rtl), colors(&ltr));
        assert_eq!(colors(&rtl), vec![green, blue, black]);
    }

    #[test]
    fn collapsed_border_width_priority_floors_css_pixels_before_origin_ties() {
        let table = solid_border(5.95, BorderOrigin::Table);
        let cell = solid_border(5.0, BorderOrigin::Cell);

        assert!(collapsed_border_wins(cell, table));
        assert!(!collapsed_border_wins(table, cell));
    }

    #[test]
    fn collapsed_border_width_priority_keeps_next_whole_css_pixel_wider() {
        let table = solid_border(6.0, BorderOrigin::Table);
        let cell = solid_border(5.0, BorderOrigin::Cell);

        assert!(collapsed_border_wins(table, cell));
    }

    #[test]
    fn equal_width_group_loser_does_not_expand_resolved_cell_insets() {
        let mut grid = CollapsedBorderGrid::new(1, 1, Direction::Ltr);
        let loser = style_with_solid_border(25.0, CssColor::new(255, 0, 0));
        let winner = style_with_solid_border(25.0, CssColor::new(0, 128, 0));

        grid.add_cell(0, 0, 1, 1, &winner);
        grid.add_row_group(0, 1, 1, &loser);

        let insets = grid.cell_insets(0, 0, 1, 1);
        let expected_side = 25.0 * css::CSS_PX_TO_PT / 2.0;
        assert!((insets.left - expected_side).abs() < 0.01);
        assert!((insets.right - expected_side).abs() < 0.01);
        assert!((insets.top - expected_side).abs() < 0.01);
        assert!((insets.bottom - expected_side).abs() < 0.01);
    }

    #[test]
    fn collapsed_border_segments_project_horizontal_and_vertical_grid_lines() {
        let border = UsedBorderSide::new(layout_pt(6.0), BorderStyle::Solid, CssColor::BLACK);
        let mut rects = Vec::new();
        let mut paths = Vec::new();

        paint_collapsed_border_side(
            &mut rects,
            &mut paths,
            BorderEdge::Top,
            CollapsedBorderSegment::horizontal(10.0, 80.0, 30.0),
            border,
        );
        paint_collapsed_border_side(
            &mut rects,
            &mut paths,
            BorderEdge::Left,
            CollapsedBorderSegment::vertical(50.0, 90.0, 40.0),
            border,
        );

        assert!(paths.is_empty());
        assert_eq!(rects.len(), 2);
        assert_eq!(
            (
                rects[0].x(),
                rects[0].y(),
                rects[0].width(),
                rects[0].height()
            ),
            (10.0, 77.0, 30.0, 6.0)
        );
        assert_eq!(
            (
                rects[1].x(),
                rects[1].y(),
                rects[1].width(),
                rects[1].height()
            ),
            (47.0, 50.0, 6.0, 40.0)
        );
    }

    #[test]
    fn medium_double_collapsed_border_paints_equal_stripes_on_both_grid_axes() {
        let border = UsedBorderSide::new(
            layout_pt(3.0 * css::CSS_PX_TO_PT),
            BorderStyle::Double,
            CssColor::BLACK,
        );
        let mut rects = Vec::new();
        let mut paths = Vec::new();

        paint_collapsed_border_side(
            &mut rects,
            &mut paths,
            BorderEdge::Top,
            CollapsedBorderSegment::horizontal(10.0, 80.0, 30.0),
            border,
        );
        paint_collapsed_border_side(
            &mut rects,
            &mut paths,
            BorderEdge::Left,
            CollapsedBorderSegment::vertical(50.0, 90.0, 40.0),
            border,
        );

        assert!(paths.is_empty());
        assert_eq!(rects.len(), 4);
        assert_eq!(
            rects[0].paint_rect(),
            paint_space_rect(10.0, 78.875, 30.0, 0.75)
        );
        assert_eq!(
            rects[1].paint_rect(),
            paint_space_rect(10.0, 80.375, 30.0, 0.75)
        );
        assert_eq!(
            rects[2].paint_rect(),
            paint_space_rect(48.875, 50.0, 0.75, 40.0)
        );
        assert_eq!(
            rects[3].paint_rect(),
            paint_space_rect(50.375, 50.0, 0.75, 40.0)
        );
    }
}
