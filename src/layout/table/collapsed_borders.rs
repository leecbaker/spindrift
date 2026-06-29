use super::*;

pub(super) struct CollapsedBorderGrid {
    horizontal: Vec<Vec<Option<CollapsedBorder>>>,
    vertical: Vec<Vec<Option<CollapsedBorder>>>,
    axes: TableAxes,
    column_count: usize,
}

impl CollapsedBorderGrid {
    pub(super) fn new(row_count: usize, column_count: usize, axes: TableAxes) -> Self {
        Self {
            horizontal: vec![vec![None; column_count]; row_count + 1],
            vertical: vec![vec![None; column_count + 1]; row_count],
            axes,
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

    pub(super) fn add_column_origin(
        &mut self,
        start_column: usize,
        end_column: usize,
        row_count: usize,
        style: &ComputedStyle,
        origin: BorderOrigin,
    ) {
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
        match self.axes.direction {
            Direction::Ltr => start_column,
            Direction::Rtl => end_column,
        }
    }

    fn inline_right_boundary(&self, start_column: usize, end_column: usize) -> usize {
        match self.axes.direction {
            Direction::Ltr => end_column,
            Direction::Rtl => start_column,
        }
    }

    fn physical_boundary_position(&self, boundary: usize) -> usize {
        match self.axes.direction {
            Direction::Ltr => boundary,
            Direction::Rtl => self.column_count.saturating_sub(boundary),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_fragment_rows(
        &self,
        placement: TableGridPlacement,
        column_plan: &TableColumnPlan,
        original_rows: &[usize],
        row_tops: &[f32],
        row_heights: &[f32],
        row_offsets: &[f32],
        original_row_heights: &[f32],
    ) -> (Vec<RenderedRect>, Vec<RenderedPath>) {
        let mut rects = Vec::new();
        let mut paths = Vec::new();
        let mut painted_boundaries = Vec::new();

        for (local_row, original_row) in original_rows.iter().copied().enumerate() {
            let (Some(top), Some(height), Some(offset), Some(original_height)) = (
                row_tops.get(local_row).copied(),
                row_heights.get(local_row).copied(),
                row_offsets.get(local_row).copied(),
                original_row_heights.get(local_row).copied(),
            ) else {
                continue;
            };
            let real_top = offset <= 0.01;
            let real_bottom = offset + height >= original_height - 0.01;
            if real_top {
                self.paint_horizontal_boundary(
                    &mut rects,
                    &mut paths,
                    placement,
                    column_plan,
                    original_row,
                    top,
                    &mut painted_boundaries,
                );
            }
            if real_bottom {
                self.paint_horizontal_boundary(
                    &mut rects,
                    &mut paths,
                    placement,
                    column_plan,
                    original_row + 1,
                    top - height,
                    &mut painted_boundaries,
                );
            }

            let Some(row) = self.vertical.get(original_row) else {
                continue;
            };
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
                border.paint_vertical(
                    &mut rects,
                    &mut paths,
                    placement.x_for(column_plan.boundary_x(boundary)),
                    top + top_extension,
                    height + top_extension + bottom_extension,
                );
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
        y: f32,
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
        for (column, border) in row.iter().enumerate() {
            let Some(border) = border else {
                continue;
            };
            let before_extension =
                self.vertical_intersection_half_width(boundary, column, row_count);
            let after_extension =
                self.vertical_intersection_half_width(boundary, column + 1, row_count);
            border.paint_horizontal(
                rects,
                paths,
                placement.x_for(column_plan.inline_bounds_for_span(column, 1).start)
                    - before_extension,
                y,
                column_plan.width_for_span(column, 1) + before_extension + after_extension,
            );
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
            .map(|border| border.used_side().used_width)
    }

    fn horizontal_border_width(&self, boundary: usize, column: usize) -> Option<f32> {
        self.horizontal
            .get(boundary)?
            .get(column)?
            .map(|border| border.used_side().used_width)
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
    /// CSS 2.2 requires user agents to derive initial table border widths from
    /// collapsed grid-edge borders. WeasyPrint stores half of each winning
    /// outer border as the table's used border width; layout then positions the
    /// grid inside those wrapper insets.
    /// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
    pub(super) fn outer_insets_for_first_displayed_row(
        &self,
        first_displayed_row: usize,
    ) -> css::Edges {
        let top = self
            .horizontal
            .first()
            .into_iter()
            .flat_map(|row| row.iter())
            .filter_map(|border| border.map(|border| border.used_side().used_width))
            .fold(0.0_f32, f32::max)
            / 2.0;
        let bottom = self
            .horizontal
            .last()
            .into_iter()
            .flat_map(|row| row.iter())
            .filter_map(|border| border.map(|border| border.used_side().used_width))
            .fold(0.0_f32, f32::max)
            / 2.0;
        let left = self
            .vertical
            .get(first_displayed_row)
            .and_then(|row| row.first())
            .and_then(|border| *border)
            .map(|border| border.used_side().used_width)
            .into_iter()
            .fold(0.0_f32, f32::max)
            / 2.0;
        let right = self
            .vertical
            .get(first_displayed_row)
            .and_then(|row| row.last())
            .and_then(|border| *border)
            .map(|border| border.used_side().used_width)
            .into_iter()
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
    width: f32,
    style: BorderStyle,
    color: Color,
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
            width: width.max(0.0),
            style: border_style,
            color,
            side,
            origin,
            tie_position,
        }
    }

    fn strong_null(origin: BorderOrigin, tie_position: usize) -> Self {
        Self {
            width: 0.0,
            style: BorderStyle::Hidden,
            color: Color::TRANSPARENT,
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
        x: f32,
        y: f32,
        width: f32,
    ) {
        let border = self.used_side();
        if !border.is_visible() {
            return;
        }
        paint_collapsed_border_side(
            rects,
            paths,
            self.side.border_edge(),
            x,
            width,
            y - border.used_width / 2.0,
            true,
            border,
        );
    }

    pub(super) fn paint_vertical(
        self,
        rects: &mut Vec<RenderedRect>,
        paths: &mut Vec<RenderedPath>,
        x: f32,
        top: f32,
        height: f32,
    ) {
        let border = self.used_side();
        if !border.is_visible() {
            return;
        }
        paint_collapsed_border_side(
            rects,
            paths,
            self.side.border_edge(),
            top - height,
            height,
            x - border.used_width / 2.0,
            false,
            border,
        );
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

    let candidate_none = candidate.style.suppresses_used_width() || candidate.width <= 0.0;
    let current_none = current.style.suppresses_used_width() || current.width <= 0.0;
    if candidate_none != current_none {
        return !candidate_none;
    }

    if (candidate.width - current.width).abs() > 0.01 {
        return candidate.width > current.width;
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
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    horizontal: bool,
    border: UsedBorderSide,
) {
    if !border.is_visible() {
        return;
    }
    let cross_width = border.used_width;
    let style = collapsed_border_paint_style(border.style);
    if style == BorderStyle::Double && cross_width >= 3.0 {
        let stripe = (border.used_width / 3.0).max(1.0);
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
            cross_width,
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
