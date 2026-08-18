use super::*;
use std::ops::{Deref, DerefMut};

/// A table-part style after the table used-value boundary.
///
/// Table fragments retain their raw computed styles for durable structure and
/// cascade reconstruction. Table layout, sizing, and painting consume this
/// marker after effective CSS `zoom` has been applied exactly once.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
/// <https://drafts.csswg.org/css-tables-3/#table-layout>
#[derive(Debug, Clone)]
pub(super) struct TableUsedStyle {
    source: ComputedStyle,
    used: css::ZoomedLayoutStyle,
}

pub(super) trait TableStyleSource {
    fn table_source(&self) -> &ComputedStyle;
}

impl TableStyleSource for ComputedStyle {
    fn table_source(&self) -> &ComputedStyle {
        self
    }
}

impl TableStyleSource for TableUsedStyle {
    fn table_source(&self) -> &ComputedStyle {
        self.source()
    }
}

impl TableUsedStyle {
    pub(super) fn from_source_and_normalized(
        source: ComputedStyle,
        used: css::ZoomedLayoutStyle,
    ) -> Self {
        Self { source, used }
    }

    /// The frozen computed style used exclusively as the cascade parent when
    /// reconstructing an anonymous or deferred table part.
    pub(super) fn source(&self) -> &ComputedStyle {
        &self.source
    }

    pub(super) fn used_style(&self) -> &css::ZoomedLayoutStyle {
        &self.used
    }
}

impl Deref for TableUsedStyle {
    type Target = ComputedStyle;

    fn deref(&self) -> &Self::Target {
        &self.used
    }
}

impl DerefMut for TableUsedStyle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.used
    }
}

/// The table-part box-model rule selected for rows and row groups.
///
/// CSS table rows and row groups do not establish ordinary margin, border, or
/// padding boxes. In the collapsed-border model their authored borders remain
/// candidates in the separate conflict-resolution algorithm, but never become
/// layout insets or ordinary paint borders.
/// <https://www.w3.org/TR/CSS22/tables.html#table-layers>
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TablePartBoxModel {
    Separated,
    Collapsed,
}

/// A border candidate supplied by a row or row group to collapsed-border
/// conflict resolution. It cannot be constructed for separated tables.
#[derive(Debug, Clone)]
pub(super) struct CollapsedBorderParticipant {
    style: css::ZoomedLayoutStyle,
}

impl CollapsedBorderParticipant {
    pub(super) fn style(&self) -> &css::ZoomedLayoutStyle {
        &self.style
    }
}

/// Used style for a CSS table row or row group.
///
/// This deliberately does not dereference to [`ComputedStyle`]. Callers may
/// use its decoration-free table-part layout style, or obtain a separately
/// typed collapsed-border participant. The untouched source style remains
/// available only as a cascade parent for descendants, so `border: inherit`
/// on a cell keeps its normal CSS meaning.
#[derive(Debug, Clone)]
pub(super) struct TablePartUsedStyle {
    source: TableUsedStyle,
    layout: css::ZoomedLayoutStyle,
    collapsed_border_participant: Option<CollapsedBorderParticipant>,
}

impl TablePartUsedStyle {
    pub(super) fn from_table_used(source: TableUsedStyle) -> Self {
        let box_model = if source.border_collapse == css::BorderCollapse::Collapse {
            TablePartBoxModel::Collapsed
        } else {
            TablePartBoxModel::Separated
        };
        let collapsed_border_participant =
            matches!(box_model, TablePartBoxModel::Collapsed).then(|| {
                let style = source.used_style().clone().map_used_values(|style| {
                    // The conflict resolver needs an authored border candidate,
                    // not an ordinary row box. Keep border declarations intact
                    // while making the ignored padding and margin unavailable to
                    // that separate layout path as well.
                    strip_table_part_margin_and_padding(style);
                });
                CollapsedBorderParticipant { style }
            });
        let layout = source.used_style().clone().map_used_values(|layout| {
            strip_table_part_margin_and_padding(layout);
            layout.border_width = 0.0;
            layout.border_widths = css::Edges::ZERO;
            layout.border_width_values = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
            layout.border_styles = css::BorderStyles::NONE;
        });
        Self {
            source,
            layout,
            collapsed_border_participant,
        }
    }

    pub(super) fn layout(&self) -> &css::ZoomedLayoutStyle {
        &self.layout
    }

    pub(super) fn collapsed_border_participant(&self) -> Option<&CollapsedBorderParticipant> {
        self.collapsed_border_participant.as_ref()
    }
}

fn strip_table_part_margin_and_padding(style: &mut ComputedStyle) {
    style.margin = css::Edges::ZERO;
    style.padding = css::Edges::ZERO;
    style.box_values.padding = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
}

impl TableStyleSource for TablePartUsedStyle {
    fn table_source(&self) -> &ComputedStyle {
        self.source.source()
    }
}

/// A declared size contribution to a table root grid track.
///
/// CSS 2.2 names the fixed-table inputs "width", but CSS Writing Modes maps
/// the root table's inline tracks to physical width or height. Keeping this
/// declaration track-neutral prevents a vertical table from accidentally
/// treating a physical cell width as a column constraint.
/// <https://www.w3.org/TR/CSS22/tables.html#width-layout>
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
#[derive(Debug, Clone)]
pub(super) enum DeclaredTableTrackSize {
    Fixed(f32),
    Percent(f32),
}

#[derive(Debug, Clone)]
pub(super) struct TableRow<'a> {
    pub(super) element: Option<&'a Element>,
    pub(super) signature: ElementSignature,
    pub(super) ancestors: Vec<ElementSignature>,
    pub(super) row_groups: Vec<TableRowGroup<'a>>,
    pub(super) style: Option<box_tree::SharedStyle>,
    pub(super) cells: Vec<TableCell<'a>>,
    pub(super) running_cells: Vec<TableCell<'a>>,
}

/// The table's source rows after CSS visual ordering, together with the rows
/// eligible for optional repeated table chrome.
///
/// CSS 2.2 gives special visual and print-repeat treatment only to the first
/// source `table-header-group` and `table-footer-group`. Their source identity
/// must survive moving those groups to their visual positions.
/// https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group
/// https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group
#[derive(Debug, Clone)]
pub(super) struct TableRowOrdering<'a> {
    pub(super) rows: Vec<TableRow<'a>>,
    pub(super) repeating_header_rows: Vec<usize>,
    pub(super) repeating_footer_rows: Vec<usize>,
}

#[derive(Debug, Clone)]
pub(super) struct TableRowGroup<'a> {
    pub(super) element: &'a Element,
    pub(super) signature: ElementSignature,
    pub(super) style: Option<box_tree::SharedStyle>,
}

#[derive(Debug, Clone)]
pub(super) struct TableCell<'a> {
    pub(super) element: Option<&'a Element>,
    pub(super) signature: ElementSignature,
    pub(super) style: Option<box_tree::SharedStyle>,
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
    pub(super) style: Option<box_tree::SharedStyle>,
    pub(super) children: Option<Vec<box_tree::FormattingBox<'a>>>,
}

#[derive(Debug, Clone)]
pub(super) struct TableColumn<'a> {
    pub(super) element: &'a Element,
    pub(super) signature: ElementSignature,
    pub(super) style: Option<box_tree::SharedStyle>,
    pub(super) group: Option<TableColumnGroup<'a>>,
    pub(super) span: usize,
}

#[derive(Debug, Clone)]
pub(super) struct TableColumnGroup<'a> {
    pub(super) element: &'a Element,
    pub(super) signature: ElementSignature,
    pub(super) style: Option<box_tree::SharedStyle>,
}

#[derive(Debug, Clone)]
pub(super) struct TableColumnPlan {
    pub(super) widths: Vec<TableGridLength>,
    pub(super) offsets: Vec<TableGridLength>,
    pub(super) horizontal_spacing: TableGridLength,
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
        mut widths: Vec<TableGridLength>,
        horizontal_spacing: TableGridLength,
        mut collapsed: Vec<bool>,
        axes: TableAxes,
    ) -> Self {
        collapsed.resize(widths.len(), false);
        for (index, width) in widths.iter_mut().enumerate() {
            if collapsed[index] {
                *width = TableGridLength::new(0.0);
            }
        }
        let visible_columns = collapsed.iter().filter(|collapsed| !**collapsed).count();
        let mut offsets = Vec::with_capacity(widths.len());
        let mut offset = if visible_columns > 0 {
            horizontal_spacing
        } else {
            TableGridLength::new(0.0)
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

    pub(super) fn width_for_span(&self, column: usize, colspan: usize) -> TableGridLength {
        if column >= self.widths.len() {
            return TableGridLength::new(0.0);
        }
        let end = (column + colspan.max(1)).min(self.widths.len());
        let visible_columns = self.collapsed[column..end]
            .iter()
            .filter(|collapsed| !**collapsed)
            .count();
        let column_widths = self.widths[column..end].iter().sum::<TableGridLength>();
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

    pub(super) fn total_width(&self) -> LogicalInlineContentSize {
        let visible_columns = self
            .collapsed
            .iter()
            .filter(|collapsed| !**collapsed)
            .count();
        if visible_columns == 0 {
            LogicalInlineContentSize::new(content_box_pt(0.0))
        } else {
            let total = self.widths.iter().sum::<TableGridLength>()
                + self.horizontal_spacing * (visible_columns + 1) as f32;
            LogicalInlineContentSize::new(total.cast_unit())
        }
    }

    pub(super) fn column_count(&self) -> usize {
        self.widths.len()
    }

    /// Whether a logical cell span crosses a collapsed column track.
    ///
    /// CSS Tables clips a spanning cell's content to its displayed cell area
    /// when a covered column is removed by `visibility: collapse`:
    /// <https://drafts.csswg.org/css-tables-3/#visibility-collapse-cell-rendering>.
    pub(super) fn span_contains_collapsed_column(&self, column: usize, colspan: usize) -> bool {
        let end = (column + colspan.max(1)).min(self.collapsed.len());
        self.collapsed[column.min(end)..end]
            .iter()
            .any(|collapsed| *collapsed)
    }

    /// Return physical inline bounds for a logical column span.
    ///
    /// CSS Tables assigns cells to logical grid slots, with separated
    /// `border-spacing` outside the edge cells and between adjacent cells. CSS
    /// Writing Modes then maps those slots to physical inline positions
    /// according to `direction`. Use the resolved span width to find the
    /// logical end edge so RTL spans mirror the actual cell area, not the
    /// table grid's trailing outer spacing:
    /// <https://drafts.csswg.org/css-tables-3/#cell-assignment>,
    /// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>, and
    /// <https://www.w3.org/TR/css-writing-modes-4/#direction>.
    pub(super) fn inline_bounds_for_span(
        &self,
        column: usize,
        colspan: usize,
    ) -> TableInlineBounds {
        let end = (column + colspan.max(1)).min(self.widths.len());
        let width = self.width_for_span(column, colspan);
        let logical_start = self.logical_boundary_offset(column);
        let logical_end = logical_start + width;
        let start = self.axes.span_start_x(
            self.grid_inline_extent(),
            logical_start,
            logical_end.min(self.logical_boundary_offset(end)),
        );
        TableInlineBounds::new(start, width)
    }

    pub(super) fn inline_bounds_for_area(&self, area: TableGridArea) -> TableInlineBounds {
        self.inline_bounds_for_span(area.column, area.colspan)
    }

    /// Return a logical table-grid rectangle for a column span.
    ///
    /// Unlike [`Self::inline_bounds_for_span`], this keeps the span in the
    /// root's logical inline axis. Structural background positioning areas
    /// are resolved in that source space before writing-mode projection:
    /// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(super) fn logical_span_rect(
        &self,
        start_column: usize,
        end_column: usize,
        block_start: TableGridLength,
        block_size: TableGridLength,
    ) -> TableGridRect {
        let start_column = start_column.min(self.widths.len());
        let end_column = end_column
            .min(self.widths.len())
            .max(start_column.saturating_add(1).min(self.widths.len()));
        let inline_start = self.logical_boundary_offset(start_column);
        let inline_size =
            self.width_for_span(start_column, end_column.saturating_sub(start_column));
        TableGridRect::new(
            TableGridPoint::from_lengths(inline_start, block_start),
            TableGridSize::from_lengths(inline_size, block_size),
        )
    }

    /// Return the logical span occupied by all visible columns, excluding the
    /// table's leading and trailing separated-border gaps.
    pub(super) fn logical_occupied_inline_rect(&self) -> Option<TableGridRect> {
        let start_column = self.collapsed.iter().position(|collapsed| !*collapsed)?;
        let end_column = self
            .collapsed
            .iter()
            .rposition(|collapsed| !*collapsed)
            .map(|index| index + 1)
            .unwrap_or(start_column + 1);
        Some(self.logical_span_rect(
            start_column,
            end_column,
            TableGridLength::new(0.0),
            TableGridLength::new(0.0),
        ))
    }

    /// Return the physical x offset for a logical grid boundary.
    ///
    /// The returned value is table-grid local; use `TableGridPlacement` before
    /// writing it into paint or layout-builder fields.
    pub(super) fn boundary_x(&self, boundary: usize) -> TableGridLength {
        let boundary = boundary.min(self.widths.len());
        self.axes.boundary_x(
            self.grid_inline_extent(),
            self.logical_boundary_offset(boundary),
        )
    }

    pub(super) fn cell_border_box(
        &self,
        area: TableGridArea,
        row_bounds: TableRowBounds,
    ) -> TableCellBorderBox {
        TableCellBorderBox::from_bounds(self.inline_bounds_for_area(area), row_bounds)
    }

    fn logical_offset_for_column(&self, column: usize) -> TableGridLength {
        self.offsets
            .get(column)
            .cloned()
            .unwrap_or_else(|| TableGridLength::new(0.0))
    }

    fn logical_boundary_offset(&self, boundary: usize) -> TableGridLength {
        if boundary == self.widths.len() {
            return self.grid_inline_extent();
        }
        self.logical_offset_for_column(boundary)
    }

    fn grid_inline_extent(&self) -> TableGridLength {
        let visible_columns = self
            .collapsed
            .iter()
            .filter(|collapsed| !**collapsed)
            .count();
        if visible_columns == 0 {
            TableGridLength::new(0.0)
        } else {
            self.widths.iter().sum::<TableGridLength>()
                + self.horizontal_spacing * (visible_columns + 1) as f32
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct TableMetrics {
    pub(super) border_collapse: css::BorderCollapse,
    pub(super) spacing: css::BorderSpacing,
}

pub(super) struct TableLayoutInput<'a> {
    pub(super) row_ordering: TableRowOrdering<'a>,
    pub(super) captions: Vec<TableCaption<'a>>,
    pub(super) columns: Vec<TableColumn<'a>>,
}

impl<'a> TableLayoutInput<'a> {
    pub(super) fn from_fragment(fragment: &box_tree::TableFragment<'a>) -> Self {
        Self {
            row_ordering: table_row_ordering_from_fragment(fragment),
            captions: table_captions_from_fragment(fragment),
            columns: table_columns_from_fragment(fragment),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_length(value: f32) -> TableGridLength {
        TableGridLength::new(value)
    }

    fn column_plan(direction: Direction) -> TableColumnPlan {
        TableColumnPlan::with_collapsed(
            vec![grid_length(10.0), grid_length(20.0), grid_length(30.0)],
            grid_length(5.0),
            vec![false, false, false],
            TableAxes::for_direction(direction),
        )
    }

    #[test]
    fn ltr_column_span_bounds_stay_in_logical_order() {
        let plan = column_plan(Direction::Ltr);
        assert_eq!(plan.total_width().points(), 80.0);
        assert_eq!(
            plan.inline_bounds_for_span(1, 2),
            TableInlineBounds::new(grid_length(20.0), grid_length(55.0))
        );
        assert_eq!(plan.boundary_x(0), grid_length(5.0));
        assert_eq!(plan.boundary_x(3), grid_length(80.0));
    }

    #[test]
    fn rtl_first_logical_column_is_on_the_physical_right() {
        let plan = column_plan(Direction::Rtl);
        assert_eq!(
            plan.inline_bounds_for_span(0, 1),
            TableInlineBounds::new(grid_length(65.0), grid_length(10.0))
        );
        assert_eq!(plan.boundary_x(0), grid_length(75.0));
        assert_eq!(plan.boundary_x(3), grid_length(0.0));
    }

    #[test]
    fn rtl_colspan_leaves_outer_spacing_on_both_sides() {
        let plan = column_plan(Direction::Rtl);
        assert_eq!(
            plan.inline_bounds_for_span(1, 2),
            TableInlineBounds::new(grid_length(5.0), grid_length(55.0))
        );
    }

    #[test]
    fn collapsed_columns_keep_logical_indices_but_do_not_add_span_width() {
        let plan = TableColumnPlan::with_collapsed(
            vec![grid_length(10.0), grid_length(20.0), grid_length(30.0)],
            grid_length(5.0),
            vec![false, true, false],
            TableAxes::for_direction(Direction::Ltr),
        );
        assert_eq!(plan.total_width().points(), 55.0);
        assert_eq!(
            plan.inline_bounds_for_span(1, 1),
            TableInlineBounds::new(grid_length(20.0), grid_length(0.0))
        );
        assert_eq!(
            plan.inline_bounds_for_span(0, 3),
            TableInlineBounds::new(grid_length(5.0), grid_length(45.0))
        );
        assert_eq!(
            plan.occupied_inline_bounds(),
            Some(TableInlineBounds::new(grid_length(5.0), grid_length(45.0)))
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
        assert_eq!(
            border_box.top_y(placement) - border_box.rect().size.height,
            235.0
        );
        assert_eq!(border_box.width(), 55.0);
        assert_eq!(border_box.rect().size.height, 25.0);
    }

    fn table_part_style(border_collapse: css::BorderCollapse) -> TableUsedStyle {
        let edges = |value| css::Edges {
            top: value,
            right: value,
            bottom: value,
            left: value,
        };
        let mut style = ComputedStyle::initial();
        style.border_collapse = border_collapse;
        style.margin = edges(7.0);
        style.padding = edges(5.0);
        style.box_values.padding =
            css::CssEdges::all(css::ComputedLengthPercentage::from_points(5.0));
        style.border_width = 3.0;
        style.border_widths = edges(3.0);
        style.border_width_values =
            css::CssEdges::all(css::ComputedLengthPercentage::from_points(3.0));
        style.border_styles = css::BorderStyles {
            top: css::BorderStyle::Solid,
            right: css::BorderStyle::Solid,
            bottom: css::BorderStyle::Solid,
            left: css::BorderStyle::Solid,
        };
        let source = style.clone();
        TableUsedStyle::from_source_and_normalized(
            source,
            css::LayoutStyle::from_computed(&style).into_zoomed(),
        )
    }

    #[test]
    fn separated_table_parts_have_no_ordinary_box_model_or_border_participant() {
        let part =
            TablePartUsedStyle::from_table_used(table_part_style(css::BorderCollapse::Separate));

        assert_eq!(part.layout().margin, css::Edges::ZERO);
        assert_eq!(part.layout().padding, css::Edges::ZERO);
        assert_eq!(part.layout().border_widths, css::Edges::ZERO);
        assert_eq!(part.layout().border_styles, css::BorderStyles::NONE);
        assert!(part.collapsed_border_participant().is_none());
    }

    #[test]
    fn collapsed_table_parts_only_expose_borders_to_conflict_resolution() {
        let part =
            TablePartUsedStyle::from_table_used(table_part_style(css::BorderCollapse::Collapse));

        assert_eq!(part.layout().border_widths, css::Edges::ZERO);
        assert_eq!(
            part.collapsed_border_participant()
                .expect("collapsed table part needs a border participant")
                .style()
                .border_widths,
            css::Edges {
                top: 3.0,
                right: 3.0,
                bottom: 3.0,
                left: 3.0,
            }
        );
    }

    #[test]
    fn table_part_keeps_its_cascade_style_for_cell_inheritance() {
        let part =
            TablePartUsedStyle::from_table_used(table_part_style(css::BorderCollapse::Separate));

        // The decoration-free layout view must not become the cascade parent:
        // a cell with `padding: inherit` or `border: inherit` inherits the
        // row's computed declaration, even though that declaration cannot
        // create row box geometry.
        let source = part.table_source();
        assert_eq!(source.margin.left, 7.0);
        assert_eq!(source.padding.left, 5.0);
        assert_eq!(source.border_widths.left, 3.0);
        assert_eq!(source.border_styles.left, css::BorderStyle::Solid);
    }
}
