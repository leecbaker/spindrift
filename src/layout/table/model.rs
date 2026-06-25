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
        }
    }

    pub(super) fn offset_for_column(&self, column: usize) -> f32 {
        self.offsets.get(column).copied().unwrap_or(0.0)
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

    pub(super) fn boundary_offset(&self, boundary: usize) -> f32 {
        if boundary == self.widths.len() {
            return self.total_width();
        }
        self.offset_for_column(boundary)
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
