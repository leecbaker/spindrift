use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) enum DeclaredTableWidth {
    Fixed(f32),
    Percent(f32),
    LengthPercentage(css::ComputedLengthPercentage),
}

#[derive(Debug, Clone)]
pub(super) struct TableRow<'a> {
    pub(super) element: Option<&'a Element>,
    pub(super) signature: ElementSignature,
    pub(super) ancestors: Vec<ElementSignature>,
    pub(super) row_groups: Vec<TableRowGroup<'a>>,
    pub(super) style: Option<ComputedStyle>,
    pub(super) cells: Vec<TableCell<'a>>,
}

#[derive(Debug, Clone)]
pub(super) struct TableRowGroup<'a> {
    pub(super) element: &'a Element,
    pub(super) signature: ElementSignature,
    pub(super) style: Option<ComputedStyle>,
}

#[derive(Debug, Clone)]
pub(super) struct TableCell<'a> {
    pub(super) element: Option<&'a Element>,
    pub(super) signature: ElementSignature,
    pub(super) style: Option<ComputedStyle>,
    pub(super) children: Option<Vec<box_tree::FormattingBox<'a>>>,
    pub(super) anonymous: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TableGrid {
    pub(super) rows: Vec<Vec<TableCellPlacement>>,
    pub(super) column_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TableCellPlacement {
    pub(super) cell: usize,
    pub(super) column: usize,
    pub(super) colspan: usize,
    pub(super) rowspan: usize,
}

#[derive(Debug, Clone)]
pub(super) struct TableCaption<'a> {
    pub(super) element: &'a Element,
    pub(super) signature: ElementSignature,
    pub(super) style: Option<ComputedStyle>,
    pub(super) children: Option<Vec<box_tree::FormattingBox<'a>>>,
}

#[derive(Debug, Clone)]
pub(super) struct TableColumn<'a> {
    pub(super) element: &'a Element,
    pub(super) signature: ElementSignature,
    pub(super) style: Option<ComputedStyle>,
    pub(super) group: Option<TableColumnGroup<'a>>,
    pub(super) span: usize,
}

#[derive(Debug, Clone)]
pub(super) struct TableColumnGroup<'a> {
    pub(super) element: &'a Element,
    pub(super) signature: ElementSignature,
    pub(super) style: Option<ComputedStyle>,
}

#[derive(Debug, Clone)]
pub(super) struct TableColumnPlan {
    pub(super) widths: Vec<f32>,
    pub(super) offsets: Vec<f32>,
    pub(super) horizontal_spacing: f32,
    pub(super) collapsed: Vec<bool>,
    pub(super) axes: TableAxes,
}

impl TableColumnPlan {
    /// Build horizontal table grid geometry for the separated/collapsed border model.
    ///
    /// CSS 2.2 separated borders include horizontal `border-spacing` between
    /// adjacent cells and between the table padding edge and edge cells. The
    /// collapsed border model passes zero spacing through `TableMetrics`.
    /// https://www.w3.org/TR/CSS22/tables.html#separated-borders
    pub(super) fn with_collapsed(
        mut widths: Vec<f32>,
        horizontal_spacing: f32,
        mut collapsed: Vec<bool>,
        axes: TableAxes,
    ) -> Self {
        collapsed.resize(widths.len(), false);
        for (index, width) in widths.iter_mut().enumerate() {
            if collapsed[index] {
                *width = 0.0;
            }
        }
        let visible_columns = collapsed.iter().filter(|collapsed| !**collapsed).count();
        let mut offsets = Vec::with_capacity(widths.len());
        let mut offset = if visible_columns > 0 {
            horizontal_spacing
        } else {
            0.0
        };
        for (index, width) in widths.iter().enumerate() {
            offsets.push(offset);
            if !collapsed[index] {
                offset += *width;
                if collapsed
                    .iter()
                    .enumerate()
                    .skip(index + 1)
                    .any(|(_, collapsed)| !collapsed)
                {
                    offset += horizontal_spacing;
                }
            }
        }
        Self {
            widths,
            offsets,
            horizontal_spacing,
            collapsed,
            axes,
        }
    }

    pub(super) fn width_for_span(&self, column: usize, colspan: usize) -> f32 {
        if column >= self.widths.len() {
            return 0.0;
        }
        let end = (column + colspan.max(1)).min(self.widths.len());
        let visible_columns = self.collapsed[column..end]
            .iter()
            .filter(|collapsed| !**collapsed)
            .count();
        let column_widths = self.widths[column..end].iter().sum::<f32>();
        column_widths + self.horizontal_spacing * visible_columns.saturating_sub(1) as f32
    }

    pub(super) fn occupied_inline_bounds(&self) -> Option<TableInlineBounds> {
        let start_column = self.collapsed.iter().position(|collapsed| !*collapsed)?;
        let end_column = self
            .collapsed
            .iter()
            .rposition(|collapsed| !*collapsed)
            .map(|index| index + 1)
            .unwrap_or(start_column + 1);
        Some(self.inline_bounds_for_span(start_column, end_column - start_column))
    }

    pub(super) fn total_width(&self) -> f32 {
        let visible_columns = self
            .collapsed
            .iter()
            .filter(|collapsed| !**collapsed)
            .count();
        if visible_columns == 0 {
            0.0
        } else {
            self.widths.iter().sum::<f32>() + self.horizontal_spacing * (visible_columns + 1) as f32
        }
    }

    pub(super) fn column_count(&self) -> usize {
        self.widths.len()
    }

    /// Return physical inline bounds for a logical column span.
    ///
    /// CSS Tables assigns cells to logical grid slots, then CSS Writing Modes
    /// maps those slots to physical inline positions according to `direction`.
    /// This helper is the table boundary for that projection:
    /// <https://drafts.csswg.org/css-tables-3/#cell-assignment> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#direction>.
    pub(super) fn inline_bounds_for_span(
        &self,
        column: usize,
        colspan: usize,
    ) -> TableInlineBounds {
        let end = (column + colspan.max(1)).min(self.widths.len());
        let start = self.axes.span_start_x(
            self.total_width(),
            self.logical_boundary_offset(column),
            self.logical_boundary_offset(end),
        );
        TableInlineBounds::new(start, self.width_for_span(column, colspan))
    }

    pub(super) fn inline_bounds_for_area(&self, area: TableGridArea) -> TableInlineBounds {
        self.inline_bounds_for_span(area.column, area.colspan)
    }

    /// Return the physical x offset for a logical grid boundary.
    ///
    /// The returned value is table-grid local; use `TableGridPlacement` before
    /// writing it into paint or layout-builder fields.
    pub(super) fn boundary_x(&self, boundary: usize) -> f32 {
        let boundary = boundary.min(self.widths.len());
        self.axes
            .boundary_x(self.total_width(), self.logical_boundary_offset(boundary))
    }

    pub(super) fn cell_border_box(
        &self,
        area: TableGridArea,
        row_bounds: TableRowBounds,
    ) -> TableCellBorderBox {
        TableCellBorderBox::from_bounds(self.inline_bounds_for_area(area), row_bounds)
    }

    fn logical_offset_for_column(&self, column: usize) -> f32 {
        self.offsets.get(column).copied().unwrap_or(0.0)
    }

    fn logical_boundary_offset(&self, boundary: usize) -> f32 {
        if boundary == self.widths.len() {
            return self.total_width();
        }
        self.logical_offset_for_column(boundary)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TableMetrics {
    pub(super) border_collapse: css::BorderCollapse,
    pub(super) spacing: css::BorderSpacing,
}

pub(super) struct TableLayoutInput<'a> {
    pub(super) rows: Vec<TableRow<'a>>,
    pub(super) captions: Vec<TableCaption<'a>>,
    pub(super) columns: Vec<TableColumn<'a>>,
}

impl<'a> TableLayoutInput<'a> {
    pub(super) fn from_fragment(fragment: &box_tree::TableFragment<'a>) -> Self {
        Self {
            rows: table_rows_from_fragment(fragment),
            captions: table_captions_from_fragment(fragment),
            columns: table_columns_from_fragment(fragment),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column_plan(direction: Direction) -> TableColumnPlan {
        TableColumnPlan::with_collapsed(
            vec![10.0, 20.0, 30.0],
            5.0,
            vec![false, false, false],
            TableAxes::for_direction(direction),
        )
    }

    #[test]
    fn ltr_column_span_bounds_stay_in_logical_order() {
        let plan = column_plan(Direction::Ltr);
        assert_eq!(plan.total_width(), 80.0);
        assert_eq!(
            plan.inline_bounds_for_span(1, 2),
            TableInlineBounds::new(20.0, 55.0)
        );
        assert_eq!(plan.boundary_x(0), 5.0);
        assert_eq!(plan.boundary_x(3), 80.0);
    }

    #[test]
    fn rtl_first_logical_column_is_on_the_physical_right() {
        let plan = column_plan(Direction::Rtl);
        assert_eq!(
            plan.inline_bounds_for_span(0, 1),
            TableInlineBounds::new(60.0, 10.0)
        );
        assert_eq!(plan.boundary_x(0), 75.0);
        assert_eq!(plan.boundary_x(3), 0.0);
    }

    #[test]
    fn rtl_colspan_uses_the_physical_minimum_boundary_as_origin() {
        let plan = column_plan(Direction::Rtl);
        assert_eq!(
            plan.inline_bounds_for_span(1, 2),
            TableInlineBounds::new(0.0, 55.0)
        );
    }

    #[test]
    fn collapsed_columns_keep_logical_indices_but_do_not_add_span_width() {
        let plan = TableColumnPlan::with_collapsed(
            vec![10.0, 20.0, 30.0],
            5.0,
            vec![false, true, false],
            TableAxes::for_direction(Direction::Ltr),
        );
        assert_eq!(plan.total_width(), 55.0);
        assert_eq!(
            plan.inline_bounds_for_span(1, 1),
            TableInlineBounds::new(20.0, 0.0)
        );
        assert_eq!(
            plan.inline_bounds_for_span(0, 3),
            TableInlineBounds::new(5.0, 45.0)
        );
        assert_eq!(
            plan.occupied_inline_bounds(),
            Some(TableInlineBounds::new(5.0, 45.0))
        );
    }

    #[test]
    fn cell_border_box_uses_typed_row_bounds() {
        let plan = column_plan(Direction::Ltr);
        let area = TableGridArea {
            row: 2,
            column: 1,
            rowspan: 1,
            colspan: 2,
        };
        let placement = TableGridPlacement::new(PageTopPoint::new(100.0, 300.0));
        let border_box = plan.cell_border_box(area, TableRowBounds::new(40.0, 25.0));
        assert_eq!(border_box.x(placement), 120.0);
        assert_eq!(border_box.top_y(placement), 260.0);
        assert_eq!(border_box.bottom_y(placement), 235.0);
        assert_eq!(border_box.width(), 55.0);
        assert_eq!(border_box.height(), 25.0);
    }
}
