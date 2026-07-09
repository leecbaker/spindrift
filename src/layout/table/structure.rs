use super::*;

pub(super) fn table_metrics(element: &Element, style: &ComputedStyle) -> TableMetrics {
    if style.border_collapse == css::BorderCollapse::Collapse {
        return TableMetrics {
            border_collapse: style.border_collapse,
            spacing: css::BorderSpacing::ZERO,
        };
    }

    let spacing = if style.border_spacing_explicit {
        style.border_spacing.clone()
    } else if is_html_table_element(element) {
        element
            .attrs
            .get("cellspacing")
            .and_then(|value| parse_html_length(value))
            .map(|spacing| {
                let spacing = spacing.max(0.0) * style.effective_zoom.factor();
                css::BorderSpacing::from_lengths(spacing, spacing)
            })
            .unwrap_or(style.border_spacing.clone())
    } else {
        // CSS Tables initial `border-spacing` is zero. The embedded HTML UA
        // stylesheet gives real `table` elements 2px spacing for compatibility,
        // but authored `display: table` boxes are CSS table boxes, not HTML
        // table elements, and should not receive that HTML-only default.
        // <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
        css::BorderSpacing::ZERO
    };

    TableMetrics {
        border_collapse: style.border_collapse,
        spacing,
    }
}

pub(super) fn table_rows_from_fragment<'a>(
    fragment: &box_tree::TableFragment<'a>,
) -> Vec<TableRow<'a>> {
    let rows = fragment
        .rows
        .iter()
        .map(|row| {
            let mut cells = Vec::new();
            let mut running_cells = Vec::new();
            for cell in &row.cells {
                let table_cell = TableCell {
                    element: cell.element,
                    signature: cell.signature.clone(),
                    style: cell.style.clone(),
                    children: Some(cell.children.clone()),
                    anonymous: cell.anonymous,
                };
                if table_fragment_cell_is_running(cell) {
                    running_cells.push(table_cell);
                } else {
                    cells.push(table_cell);
                }
            }
            TableRow {
                element: row.element,
                signature: row.signature.clone(),
                ancestors: row.ancestors.clone(),
                row_groups: row
                    .row_groups
                    .iter()
                    .map(|group| TableRowGroup {
                        element: group.element,
                        signature: group.signature.clone(),
                        style: group.style.clone(),
                    })
                    .collect(),
                style: row.style.clone(),
                cells,
                running_cells,
            }
        })
        .collect();
    order_table_rows(rows)
}

fn table_fragment_cell_is_running(cell: &box_tree::TableFragmentCell<'_>) -> bool {
    !cell.anonymous
        && cell
            .style
            .as_ref()
            .is_some_and(|style| style.running_element_name.is_some())
}

/// Apply CSS table row-group visual ordering before grid construction.
///
/// CSS 2.2 defines `table-header-group` as visually preceding all body rows and
/// `table-footer-group` as visually following all body rows; only the first
/// header group and first footer group receive that special treatment.
/// https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group
/// https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group
pub(super) fn order_table_rows<'a>(rows: Vec<TableRow<'a>>) -> Vec<TableRow<'a>> {
    let first_header = first_row_group_signature_matching(&rows, table_row_group_is_header);
    let first_footer = first_row_group_signature_matching(&rows, table_row_group_is_footer);
    if first_header.is_none() && first_footer.is_none() {
        return rows;
    }

    let mut headers = Vec::new();
    let mut body = Vec::new();
    let mut footers = Vec::new();
    for row in rows {
        if row_group_signature_matches(row.row_groups.last(), first_header.as_ref()) {
            headers.push(row);
        } else if row_group_signature_matches(row.row_groups.last(), first_footer.as_ref()) {
            footers.push(row);
        } else {
            body.push(row);
        }
    }

    headers.extend(body);
    headers.extend(footers);
    headers
}

fn first_row_group_signature_matching(
    rows: &[TableRow<'_>],
    predicate: fn(&TableRowGroup<'_>) -> bool,
) -> Option<ElementSignature> {
    rows.iter()
        .filter_map(|row| row.row_groups.last())
        .find(|group| predicate(group))
        .map(|group| group.signature.clone())
}

fn row_group_signature_matches(
    group: Option<&TableRowGroup<'_>>,
    signature: Option<&ElementSignature>,
) -> bool {
    group
        .zip(signature)
        .is_some_and(|(group, signature)| &group.signature == signature)
}

pub(super) fn table_grid(rows: &[TableRow<'_>]) -> TableGrid {
    let mut grid_rows = Vec::with_capacity(rows.len());
    let mut active_rowspans: Vec<usize> = Vec::new();
    let mut column_count = 0usize;
    let row_group_ends = table_row_group_end_indices(rows);

    for (row_index, row) in rows.iter().enumerate() {
        let mut placements = Vec::new();
        let mut column = 0usize;
        for (cell_index, cell) in row.cells.iter().enumerate() {
            while active_rowspans.get(column).cloned().unwrap_or(0) > 0 {
                column += 1;
            }

            let colspan = cell.element.map(html_table_colspan).unwrap_or(1);
            let rowspan = cell
                .element
                .map(|element| html_table_rowspan(element, row_index, row_group_ends[row_index]))
                .unwrap_or(1);
            let end = column + colspan;
            if active_rowspans.len() < end {
                active_rowspans.resize(end, 0);
            }
            for active in &mut active_rowspans[column..end] {
                *active = (*active).max(rowspan);
            }
            placements.push(TableCellPlacement {
                cell: cell_index,
                column,
                colspan,
                rowspan,
            });
            column = end;
        }

        column_count = column_count.max(active_rowspans.len());
        for active in &mut active_rowspans {
            *active = active.saturating_sub(1);
        }
        while active_rowspans.last().cloned() == Some(0) {
            active_rowspans.pop();
        }
        grid_rows.push(placements);
    }

    TableGrid {
        rows: grid_rows,
        column_count: column_count.max(1),
    }
}

pub(super) fn table_captions_from_fragment<'a>(
    fragment: &box_tree::TableFragment<'a>,
) -> Vec<TableCaption<'a>> {
    fragment
        .captions
        .iter()
        .map(|caption| TableCaption {
            element: caption.element,
            signature: caption.signature.clone(),
            style: caption.style.clone(),
            children: Some(caption.children.clone()),
        })
        .collect()
}

pub(super) fn table_columns_from_fragment<'a>(
    fragment: &box_tree::TableFragment<'a>,
) -> Vec<TableColumn<'a>> {
    fragment
        .columns
        .iter()
        .map(|column| TableColumn {
            element: column.element,
            signature: column.signature.clone(),
            style: column.style.clone(),
            group: column.group.as_ref().map(|group| TableColumnGroup {
                element: group.element,
                signature: group.signature.clone(),
                style: group.style.clone(),
            }),
            span: column.span,
        })
        .collect()
}

pub(super) fn table_repeating_header_row_indices(rows: &[TableRow<'_>]) -> Vec<usize> {
    let first_header = first_row_group_signature_matching(rows, table_row_group_is_header);
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| {
            row_group_signature_matches(row.row_groups.last(), first_header.as_ref())
                .then_some(index)
        })
        .collect()
}

pub(super) fn table_repeating_footer_row_indices(rows: &[TableRow<'_>]) -> Vec<usize> {
    let first_footer = first_row_group_signature_matching(rows, table_row_group_is_footer);
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| {
            row_group_signature_matches(row.row_groups.last(), first_footer.as_ref())
                .then_some(index)
        })
        .collect()
}

pub(super) fn table_row_group_is_header(group: &TableRowGroup<'_>) -> bool {
    group
        .style
        .as_ref()
        .map(|style| style.display.is_table_header_group())
        .unwrap_or(is_html_table_header_group_element(group.element))
}

pub(super) fn table_row_group_is_footer(group: &TableRowGroup<'_>) -> bool {
    group
        .style
        .as_ref()
        .map(|style| style.display.is_table_footer_group())
        .unwrap_or(is_html_table_footer_group_element(group.element))
}

pub(super) fn table_row_group_spans<'a>(
    rows: &[TableRow<'a>],
) -> Vec<(usize, usize, TableRowGroup<'a>)> {
    let mut spans = Vec::new();
    let mut start = None;
    let mut current_group: Option<TableRowGroup<'a>> = None;

    for (index, row) in rows.iter().enumerate() {
        let group = row.row_groups.last().cloned();
        if group_signature(&group) != group_signature(&current_group) {
            if let (Some(start), Some(group)) = (start.take(), current_group.take()) {
                spans.push((start, index, group));
            }
            start = group.as_ref().map(|_| index);
            current_group = group;
        }
    }

    if let (Some(start), Some(group)) = (start, current_group) {
        spans.push((start, rows.len(), group));
    }

    spans
}

pub(super) fn group_signature<'a>(
    group: &'a Option<TableRowGroup<'_>>,
) -> Option<&'a ElementSignature> {
    group.as_ref().map(|group| &group.signature)
}

pub(super) fn table_row_group_end_indices(rows: &[TableRow<'_>]) -> Vec<usize> {
    let mut ends = vec![rows.len(); rows.len()];
    let mut start = 0usize;
    let mut current_group = rows.first().and_then(|row| row.row_groups.last().cloned());
    for (index, row) in rows.iter().enumerate() {
        let group = row.row_groups.last().cloned();
        if index > 0 && group_signature(&group) != group_signature(&current_group) {
            for end in &mut ends[start..index] {
                *end = index;
            }
            start = index;
            current_group = group;
        }
    }
    ends
}

pub(super) fn table_column_group_spans<'a>(
    columns: &[TableColumn<'a>],
    column_count: usize,
) -> Vec<(usize, usize, TableColumnGroup<'a>)> {
    let mut spans = Vec::new();
    let mut start = None;
    let mut current_group: Option<TableColumnGroup<'a>> = None;
    let mut column_index = 0usize;

    for column in columns {
        if column_index >= column_count {
            break;
        }
        let span = column.span.min(column_count - column_index).max(1);
        let group = column.group.clone();
        if column_group_signature(&group) != column_group_signature(&current_group) {
            if let (Some(start), Some(group)) = (start.take(), current_group.take()) {
                spans.push((start, column_index, group));
            }
            start = group.as_ref().map(|_| column_index);
            current_group = group;
        }
        column_index += span;
    }

    if let (Some(start), Some(group)) = (start, current_group) {
        spans.push((start, column_index.min(column_count), group));
    }

    spans
}

pub(super) fn column_group_signature<'a>(
    group: &'a Option<TableColumnGroup<'_>>,
) -> Option<&'a ElementSignature> {
    group.as_ref().map(|group| &group.signature)
}

pub(super) fn apply_table_cellpadding(style: &mut ComputedStyle, table_cellpadding: Option<f32>) {
    if let Some(cellpadding) = table_cellpadding
        && style.padding
            == (css::Edges {
                top: css::CSS_PX_TO_PT,
                right: css::CSS_PX_TO_PT,
                bottom: css::CSS_PX_TO_PT,
                left: css::CSS_PX_TO_PT,
            })
    {
        style.padding = css::Edges {
            top: cellpadding,
            right: cellpadding,
            bottom: cellpadding,
            left: cellpadding,
        };
        let cellpadding = css::ComputedLengthPercentage::from_points(cellpadding);
        style.box_values.padding = css::CssEdges {
            top: cellpadding.clone(),
            right: cellpadding.clone(),
            bottom: cellpadding.clone(),
            left: cellpadding,
        };
    }
}

/// Resolve CSS table-cell padding to used physical edge lengths.
///
/// CSS 2.2 resolves padding percentages against the containing block width.
/// Table layout needs those used values before measuring cell content and
/// painting cell boxes; HTML `cellpadding` remains a presentational fallback
/// only when the UA default cell padding is still in effect.
/// https://www.w3.org/TR/CSS22/box.html#padding-properties
pub(super) fn apply_table_cell_used_padding(
    style: &mut ComputedStyle,
    table_cellpadding: Option<f32>,
    inline_basis: PercentageBasis<LayoutLength>,
) {
    apply_table_cellpadding(style, table_cellpadding);
    style.padding = used_padding_edges(style, inline_basis).to_css_edges();
}

/// Collect the inline textual content used by anonymous and real table cells.
///
/// CSS 2.2 anonymous table object construction can wrap raw text nodes in
/// generated rows/cells. Durable table fragments therefore need a content path
/// that does not require a real `td`/`th` element:
/// <https://www.w3.org/TR/CSS22/tables.html#anonymous-boxes>.
pub(super) fn table_cell_inline_text(cell: &TableCell<'_>) -> String {
    if let Some(children) = cell.children.as_deref() {
        let mut text = String::new();
        collect_inline_text_from_formatting_boxes(children, &mut text);
        return text;
    }
    cell.element.map(inline_text).unwrap_or_default()
}

pub(super) fn table_cell_href<'a>(cell: &'a TableCell<'a>) -> Option<&'a str> {
    cell.element
        .and_then(|element| element.attrs.get("href").map(String::as_str))
}

/// Measure replaced descendants that contribute to table-cell intrinsic height.
///
/// CSS table row height depends on the maximum minimum height of cells in the
/// row, including replaced content:
/// <https://www.w3.org/TR/CSS22/tables.html#height-layout>.
pub(super) fn table_cell_replaced_content_height(cell: &TableCell<'_>) -> LayoutLength {
    if let Some(children) = cell.children.as_deref() {
        return layout_pt(
            children
                .iter()
                .filter_map(|child| match child {
                    box_tree::FormattingBox::Replaced(box_)
                        if replaced_element_kind(box_.element)
                            == Some(ReplacedElementKind::Svg) =>
                    {
                        svg_rect(box_.element).map(|(_width, height, _fill)| height)
                    }
                    box_tree::FormattingBox::AtomicInline(box_)
                        if replaced_element_kind(box_.element)
                            == Some(ReplacedElementKind::Svg) =>
                    {
                        svg_rect(box_.element).map(|(_width, height, _fill)| height)
                    }
                    _ => None,
                })
                .sum(),
        );
    }

    layout_pt(
        cell.element
            .into_iter()
            .flat_map(|element| element.children.iter())
            .filter_map(|child| match &child.kind {
                NodeKind::Element(child)
                    if replaced_element_kind(child) == Some(ReplacedElementKind::Svg) =>
                {
                    svg_rect(child).map(|(_width, height, _fill)| height)
                }
                _ => None,
            })
            .sum(),
    )
}

/// Measure non-text in-flow cell content for table row height and alignment.
///
/// CSS 2.2 row height layout uses the minimum height required by each cell's
/// content, and cells establish block containers for normal-flow descendants:
/// <https://www.w3.org/TR/CSS22/tables.html#height-layout> and
/// <https://www.w3.org/TR/CSS22/tables.html#model>.
pub(super) fn table_cell_non_text_content_height(cell: &TableCell<'_>) -> LayoutLength {
    layout_pt(
        table_cell_replaced_content_height(cell)
            .points()
            .max(table_cell_block_content_height(cell).points()),
    )
}

fn table_cell_block_content_height(cell: &TableCell<'_>) -> LayoutLength {
    let Some(children) = cell.children.as_deref() else {
        return layout_pt(0.0);
    };

    table_cell_formatting_children_block_height(children)
}

/// Return normal-flow block-size contribution from table-cell formatting boxes.
///
/// CSS 2.2 table cells establish block containers, and inline-level children
/// form line boxes inside those block containers. Atomic inline boxes such as
/// `inline-block` contribute their used outer height to the line box rather
/// than being ignored or stacked independently:
/// <https://www.w3.org/TR/CSS22/tables.html#model> and
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>.
fn table_cell_formatting_children_block_height(
    children: &[box_tree::FormattingBox<'_>],
) -> LayoutLength {
    let mut height = layout_pt(0.0);
    let mut inline_line_height = layout_pt(0.0);

    for child in children {
        if let Some(inline_height) = table_cell_inline_level_outer_height(child) {
            inline_line_height = layout_pt(inline_line_height.points().max(inline_height.points()));
            continue;
        }
        if inline_line_height.points() > 0.0 {
            height += inline_line_height;
            inline_line_height = layout_pt(0.0);
        }
        height += table_cell_block_child_height(child);
    }

    height + inline_line_height
}

/// Return a normal-flow table-cell child's outer block-size contribution.
///
/// CSS 2.2 table row height layout depends on cell content height, including
/// block descendants and their signed margins:
/// <https://www.w3.org/TR/CSS22/tables.html#height-layout> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(super) fn table_cell_formatting_child_outer_height(
    child: &box_tree::FormattingBox<'_>,
) -> LayoutLength {
    table_cell_inline_level_outer_height(child)
        .unwrap_or_else(|| table_cell_block_child_height(child))
}

fn table_cell_block_child_height(child: &box_tree::FormattingBox<'_>) -> LayoutLength {
    match child {
        box_tree::FormattingBox::Block(box_) => {
            table_cell_element_outer_height(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::Table(box_) => {
            table_cell_element_outer_height(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::Flex(box_) => {
            table_cell_element_outer_height(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            table_cell_formatting_children_block_height(&box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            table_cell_formatting_children_block_height(&box_.children)
        }
        box_tree::FormattingBox::Inline(_)
        | box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Text(_)
        | box_tree::FormattingBox::Replaced(_) => layout_pt(0.0),
    }
}

fn table_cell_inline_level_outer_height(
    child: &box_tree::FormattingBox<'_>,
) -> Option<LayoutLength> {
    match child {
        box_tree::FormattingBox::Inline(box_) => {
            let height = if box_.style.float != Float::None {
                used_content_box_height_or_auto(
                    &box_.style,
                    layout_pt(0.0),
                    non_content_pt(
                        box_.style.padding.top
                            + box_.style.padding.bottom
                            + vertical_border_width(&box_.style),
                    ),
                )
                .map(SemanticLengthExt::points)
                .unwrap_or_else(|| {
                    table_cell_formatting_children_block_height(&box_.children).points()
                }) + box_.style.margin.top
                    + box_.style.margin.bottom
            } else {
                table_cell_formatting_children_block_height(&box_.children).points()
            };
            (height > 0.0).then_some(layout_pt(height))
        }
        box_tree::FormattingBox::AtomicInline(box_) => Some(table_cell_atomic_inline_outer_height(
            &box_.style,
            &box_.children,
        )),
        box_tree::FormattingBox::Replaced(box_) => Some(table_cell_atomic_inline_outer_height(
            &box_.style,
            &box_.children,
        )),
        box_tree::FormattingBox::AnonymousBlock(_)
        | box_tree::FormattingBox::InlineSplitBlockContext(_)
        | box_tree::FormattingBox::Block(_)
        | box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Text(_) => None,
    }
}

/// Resolve an atomic inline's outer block-size contribution for table rows.
///
/// CSS Inline lays atomic inlines as single line-box participants, while CSS
/// Sizing applies `height` to their content box and then adds padding, border,
/// and margins:
/// <https://www.w3.org/TR/css-inline-3/#atomic-inline> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
fn table_cell_atomic_inline_outer_height(
    style: &ComputedStyle,
    children: &[box_tree::FormattingBox<'_>],
) -> LayoutLength {
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        return layout_pt(0.0);
    }
    let nested_height = if style.contain.size {
        0.0
    } else {
        table_cell_formatting_children_block_height(children).points()
    };
    let vertical_non_content =
        non_content_pt(style.padding.top + style.padding.bottom) + table_vertical_borders(style);
    let preferred_content_height = if style.contain.size {
        0.0
    } else {
        nested_height.max(style.line_height)
    };
    let mut content_height = used_content_box_height_or_auto(
        style,
        layout_pt(preferred_content_height),
        vertical_non_content,
    )
    .map(SemanticLengthExt::points)
    .unwrap_or(preferred_content_height);
    if !style.contain.size {
        content_height = content_height.max(nested_height);
    }
    layout_pt(
        (content_height + vertical_non_content.points() + style.margin.top + style.margin.bottom)
            .max(0.0),
    )
}

fn table_cell_element_outer_height(
    style: &ComputedStyle,
    children: &[box_tree::FormattingBox<'_>],
) -> LayoutLength {
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        return layout_pt(0.0);
    }
    let nested_height = if style.contain.size {
        0.0
    } else {
        table_cell_formatting_children_block_height(children).points()
    };
    let vertical_non_content =
        non_content_pt(style.padding.top + style.padding.bottom) + table_vertical_borders(style);
    let mut content_height =
        used_content_box_height_or_auto(style, layout_pt(nested_height), vertical_non_content)
            .map(SemanticLengthExt::points)
            .unwrap_or(nested_height);
    if !style.contain.size {
        content_height = content_height.max(nested_height);
    }
    layout_pt(
        (content_height + vertical_non_content.points() + style.margin.top + style.margin.bottom)
            .max(0.0),
    )
}

pub(super) fn table_cell_content_offset(
    style: &ComputedStyle,
    border_insets: css::Edges,
    content_height: f32,
    border_box_height: f32,
    row_baseline_offset: Option<f32>,
    cell_baseline_offset: f32,
) -> f32 {
    // CSS 2.2 table height algorithms align cell boxes within row boxes using
    // `vertical-align`; CSS Box Alignment keeps that legacy behavior for
    // `align-content: normal`, while non-normal values align the table-cell
    // contents in the block axis:
    // <https://www.w3.org/TR/css-align-3/#align-content-property>.
    let content_box_height = (border_box_height
        - border_insets.top
        - border_insets.bottom
        - style.padding.top
        - style.padding.bottom)
        .max(0.0);
    let free_space = content_box_height - content_height;
    if style.align_content.keyword != ContentAlignmentKeyword::Normal {
        if matches!(
            style.align_content.keyword,
            ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline
        ) {
            if let Some(baseline) = row_baseline_offset {
                return (baseline - cell_baseline_offset).max(0.0);
            }
            if block_start_side(style.writing_mode).axis() == PhysicalAxis::Vertical {
                return content_alignment_offset_toward_end(
                    style.align_content,
                    free_space,
                    block_align_content_defaults_to_safe_overflow(style),
                );
            }
            return 0.0;
        }
        return -block_align_content_y_offset_for_style(style, free_space);
    }

    let extra = free_space.max(0.0);
    match style.vertical_align.table_cell_align {
        TableCellVerticalAlign::Top => 0.0,
        TableCellVerticalAlign::Middle => extra / 2.0,
        TableCellVerticalAlign::Bottom => extra,
        TableCellVerticalAlign::Baseline => row_baseline_offset
            .map(|baseline| (baseline - cell_baseline_offset).max(0.0))
            .unwrap_or(0.0)
            .min(extra),
    }
}

pub(super) fn table_row_span_height(
    row_heights: &[f32],
    row_occupancy: &[bool],
    row: usize,
    rowspan: usize,
    table_metrics: TableMetrics,
) -> f32 {
    let end = (row + rowspan.max(1)).min(row_heights.len());
    if row >= end {
        return 0.0;
    }
    table_grid_height(
        &row_heights[row..end],
        &row_occupancy[row..end.min(row_occupancy.len())],
        table_metrics,
    )
}

/// Return the logical block-start offset of a table row track.
///
/// Table row tracks are ordered in the table root's block direction.  The
/// offset includes only the separated-border gaps between participating prior
/// rows and the target row; outer edge spacing belongs to the table grid box,
/// not to a row track.
/// <https://drafts.csswg.org/css-tables-3/#row-layout>
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
pub(super) fn table_row_block_start(
    row_heights: &[f32],
    row_occupancy: &[bool],
    row: usize,
    table_metrics: TableMetrics,
) -> f32 {
    let target_occupies_grid = row_occupancy
        .get(row)
        .copied()
        .unwrap_or_else(|| row_heights.get(row).copied().unwrap_or(0.0) > 0.0);
    let mut offset = 0.0;
    let mut previous_occupies_grid = false;
    for (index, height) in row_heights.iter().take(row).enumerate() {
        let occupies_grid = row_occupancy.get(index).copied().unwrap_or(*height > 0.0);
        if !occupies_grid {
            continue;
        }
        if previous_occupies_grid {
            offset += table_metrics.spacing.vertical.length_points();
        }
        offset += height.max(0.0);
        previous_occupies_grid = true;
    }
    if target_occupies_grid && previous_occupies_grid {
        offset += table_metrics.spacing.vertical.length_points();
    }
    offset
}

/// Return the block-axis size of the table row grid.
///
/// CSS 2.2 separated borders add vertical `border-spacing` only between
/// participating adjacent row tracks. Rows collapsed by `visibility: collapse`
/// and rows suppressed by `empty-cells: hide` are non-participating; visible
/// zero-height rows still participate in edge and inter-row spacing.
/// https://www.w3.org/TR/CSS22/tables.html#dynamic-effects
/// https://www.w3.org/TR/CSS22/tables.html#separated-borders
pub(super) fn table_grid_height(
    row_heights: &[f32],
    row_occupancy: &[bool],
    table_metrics: TableMetrics,
) -> f32 {
    row_heights
        .iter()
        .enumerate()
        .filter_map(|(index, height)| {
            row_occupancy
                .get(index)
                .cloned()
                .unwrap_or(*height > 0.0)
                .then_some(*height)
        })
        .sum::<f32>()
        + table_metrics.spacing.vertical.length_points()
            * table_internal_vertical_gap_count(row_occupancy) as f32
}

/// Return separated-border spacing between the table padding edge and edge rows.
///
/// CSS 2.2 separated borders put the distance between a table padding edge and
/// an edge cell border at table padding plus the relevant `border-spacing`
/// value. Collapsed rows and hidden-empty rows do not create edge cells for this
/// spacing calculation, but visible zero-height rows do.
/// https://www.w3.org/TR/CSS22/tables.html#separated-borders
pub(super) fn table_vertical_edge_spacing(
    row_occupancy: &[bool],
    table_metrics: TableMetrics,
) -> f32 {
    if table_metrics.border_collapse == css::BorderCollapse::Separate
        && row_occupancy.iter().any(|occupied| *occupied)
    {
        table_metrics.spacing.vertical.length_points()
    } else {
        0.0
    }
}

/// Return the table content-box block size for separated/collapsed table layout.
///
/// CSS 2.2 defines separated table width/height as including spacing between
/// cells and between edge cells and the table padding edge; row group, row, and
/// column backgrounds still use the row grid, not this outer spacing area.
/// https://www.w3.org/TR/CSS22/tables.html#separated-borders
pub(super) fn table_content_height(
    row_heights: &[f32],
    row_occupancy: &[bool],
    table_metrics: TableMetrics,
) -> f32 {
    table_grid_height(row_heights, row_occupancy, table_metrics.clone())
        + table_vertical_edge_spacing(row_occupancy, table_metrics) * 2.0
}

/// Return the block-axis height occupied by repeated table row fragments.
///
/// CSS 2.2 allows print user agents to repeat `table-header-group` and
/// `table-footer-group` rows on pages spanned by a table. Repeated groups keep
/// the same row heights and inter-row border spacing as the source group.
/// https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group
/// https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group
pub(super) fn repeated_table_rows_height(
    row_indices: &[usize],
    row_heights: &[f32],
    row_occupancy: &[bool],
    table_metrics: TableMetrics,
) -> f32 {
    let occupied_heights = row_indices
        .iter()
        .filter(|row_index| {
            row_occupancy
                .get(**row_index)
                .cloned()
                .unwrap_or_else(|| row_heights.get(**row_index).cloned().unwrap_or(0.0) > 0.0)
        })
        .map(|row_index| row_heights.get(*row_index).cloned().unwrap_or(0.0))
        .collect::<Vec<_>>();
    table_grid_height(
        &occupied_heights,
        &vec![true; occupied_heights.len()],
        table_metrics,
    )
}

pub(super) fn table_row_top(
    grid_top: f32,
    row_heights: &[f32],
    row_occupancy: &[bool],
    table_metrics: TableMetrics,
    row: usize,
) -> f32 {
    let row = row.min(row_heights.len());
    let offset = row_heights[..row]
        .iter()
        .enumerate()
        .filter_map(|(index, height)| {
            row_occupancy
                .get(index)
                .cloned()
                .unwrap_or(*height > 0.0)
                .then_some(*height)
        })
        .sum::<f32>()
        + table_metrics.spacing.vertical.length_points()
            * table_internal_vertical_gap_count_before(row_occupancy, row) as f32;
    grid_top - offset
}

/// Return the first occupying row's block-axis range for inline-table baseline alignment.
///
/// CSS 2.2 uses the first row baseline as the baseline of an `inline-table`.
/// This helper maps the first non-zero row into the temporary table fragment's
/// page coordinates so rendered text lines can be matched to that row.
/// https://www.w3.org/TR/CSS22/tables.html#table-display
pub(super) fn inline_table_first_occupying_row_range(
    table_top: f32,
    top_caption_height: f32,
    table_border_widths: css::Edges,
    table_padding: css::Edges,
    row_heights: &[f32],
    row_occupancy: &[bool],
    table_metrics: TableMetrics,
) -> Option<(f32, f32)> {
    let (row_index, row_height) = row_heights
        .iter()
        .cloned()
        .enumerate()
        .find(|(index, _)| row_occupancy.get(*index).cloned().unwrap_or(false))?;
    let grid_top = table_top
        - top_caption_height
        - table_border_widths.top
        - table_padding.top
        - table_vertical_edge_spacing(row_occupancy, table_metrics.clone());
    let row_top = table_row_top(
        grid_top,
        row_heights,
        row_occupancy,
        table_metrics,
        row_index,
    );
    Some((row_top, row_top - row_height))
}

fn table_internal_vertical_gap_count(row_occupancy: &[bool]) -> usize {
    row_occupancy
        .iter()
        .filter(|occupied| **occupied)
        .count()
        .saturating_sub(1)
}

pub(super) fn table_internal_vertical_gap_count_before(
    row_occupancy: &[bool],
    row: usize,
) -> usize {
    let row = row.min(row_occupancy.len());
    let occupied_before = row_occupancy[..row]
        .iter()
        .filter(|occupied| **occupied)
        .count();
    if occupied_before == 0 {
        return 0;
    }
    if row_occupancy[row..].iter().any(|occupied| *occupied) {
        occupied_before
    } else {
        occupied_before.saturating_sub(1)
    }
}

pub(super) fn table_row_is_collapsed(style: &ComputedStyle) -> bool {
    // CSS 2.2 `visibility: collapse` on table rows removes the row's occupied
    // space; outside table row/column objects it behaves like `hidden`.
    style.visibility == Visibility::Collapse
}

/// Return the horizontal border contribution outside a table cell's content box.
pub(super) fn table_horizontal_borders(style: &ComputedStyle) -> NonContentLength {
    let borders = table_cell_used_border_edges(style);
    non_content_pt(borders.left + borders.right)
}

/// Return the vertical border contribution outside a table cell's content box.
pub(super) fn table_vertical_borders(style: &ComputedStyle) -> NonContentLength {
    let borders = table_cell_used_border_edges(style);
    non_content_pt(borders.top + borders.bottom)
}

/// Return the cell border contribution used by table sizing.
///
/// CSS 2.2 collapsed borders are centered on grid lines, so cell intrinsic
/// width/height and content offsets consume half of each resolved edge rather
/// than the full authored cell border width. Separated-border cells use their
/// full ordinary border box edges.
/// <https://www.w3.org/TR/CSS22/tables.html#collapsing-borders>
/// <https://www.w3.org/TR/CSS22/tables.html#auto-table-layout>
pub(super) fn table_cell_used_border_edges(style: &ComputedStyle) -> css::Edges {
    let mut borders = used_border_widths(style);
    if style.border_collapse == css::BorderCollapse::Collapse {
        borders.top *= 0.5;
        borders.right *= 0.5;
        borders.bottom *= 0.5;
        borders.left *= 0.5;
    }
    borders
}

pub(super) fn paint_table_border_edges(
    rects: &mut Vec<RenderedRect>,
    paths: &mut Vec<RenderedPath>,
    rect: PageTopRect,
    style: &ComputedStyle,
) {
    super::paint_border_edges(rects, paths, rect, style);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(vertical_spacing: f32) -> TableMetrics {
        TableMetrics {
            border_collapse: css::BorderCollapse::Separate,
            spacing: css::BorderSpacing::from_lengths(0.0, vertical_spacing),
        }
    }

    #[test]
    fn row_track_offsets_include_only_inter_row_spacing() {
        let rows = [10.0, 20.0, 30.0];
        let occupancy = [true, true, true];

        assert_eq!(
            table_row_block_start(&rows, &occupancy, 0, metrics(4.0)),
            0.0
        );
        assert_eq!(
            table_row_block_start(&rows, &occupancy, 1, metrics(4.0)),
            14.0
        );
        assert_eq!(
            table_row_block_start(&rows, &occupancy, 2, metrics(4.0)),
            38.0
        );
    }

    #[test]
    fn collapsed_rows_do_not_create_logical_track_gaps() {
        let rows = [10.0, 20.0, 30.0];
        let occupancy = [true, false, true];

        assert_eq!(
            table_row_block_start(&rows, &occupancy, 2, metrics(4.0)),
            14.0
        );
    }
}
