//! Table source structure, ordering, and grid construction.

use super::*;
/// The first and last named-page values represented by a durable table
/// fragment.
///
/// CSS Paged Media selects page contexts at class-A boundaries, including
/// table rows and row groups. The table fragment is the post-fixup source of
/// that sequence, unlike the table root's ordinary formatting children.
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>
/// <https://www.w3.org/TR/css-break-3/#possible-breaks>
#[derive(Clone, Debug)]
pub(in crate::layout) struct TablePageBoundarySummary {
    pub(in crate::layout) sources: PageBoundaryValues,
    pub(in crate::layout) resolved: ResolvedPageBoundaryValues,
}

impl TablePageBoundarySummary {
    fn from_style(style: &ComputedStyle, inherited_page_name: Option<&str>) -> Self {
        Self {
            sources: PageBoundaryValues::from_style(style),
            resolved: ResolvedPageBoundaryValues::uniform(
                style
                    .page
                    .effective_name(inherited_page_name.map(str::to_string)),
            ),
        }
    }
}

/// Returns the named-page boundary summary for a table wrapper.
///
/// Top captions precede the grid, bottom captions follow it, and rows use the
/// header/body/footer visual order that table layout uses. Repeated row
/// fragments are intentionally absent: only source rows establish boundaries.
pub(in crate::layout) fn table_page_boundary_summary(
    fragment: &box_tree::TableFragment<'_>,
    table_style: &ComputedStyle,
    inherited_page_name: Option<&str>,
) -> TablePageBoundarySummary {
    let mut summary = TablePageBoundarySummary::from_style(table_style, inherited_page_name);
    let table_page_name = summary.resolved.start.clone();
    let mut participants = Vec::new();

    for caption in fragment.captions.iter().filter(|caption| {
        caption
            .style
            .as_deref()
            .is_some_and(|style| style.caption_side == CaptionSide::Top)
    }) {
        participants.push(table_caption_page_boundary_summary(
            caption,
            table_style,
            table_page_name.as_deref(),
        ));
    }

    let rows = table_rows_from_fragment(fragment);
    let row_group_ends = table_row_group_end_indices(&rows);
    for (row_index, row) in rows.iter().enumerate() {
        participants.push(table_row_page_boundary_summary(
            row,
            row_index,
            row_group_ends[row_index],
            table_style,
            table_page_name.as_deref(),
        ));
    }

    for caption in fragment.captions.iter().filter(|caption| {
        caption
            .style
            .as_deref()
            .is_some_and(|style| style.caption_side == CaptionSide::Bottom)
    }) {
        participants.push(table_caption_page_boundary_summary(
            caption,
            table_style,
            table_page_name.as_deref(),
        ));
    }

    let mut participants = participants.into_iter().filter(|participant| {
        participant.sources.start != PageBoundaryValue::Inapplicable
            && participant.sources.end != PageBoundaryValue::Inapplicable
    });
    let Some(first) = participants.next() else {
        return summary;
    };
    if first.sources.start.overrides_parent_summary() {
        summary.sources.start = first.sources.start.clone();
        summary.resolved.start = first.resolved.start.clone();
    }
    let last = participants.next_back().unwrap_or(first);
    if last.sources.end.overrides_parent_summary() {
        summary.sources.end = last.sources.end;
        summary.resolved.end = last.resolved.end;
    }
    summary
}

fn table_caption_page_boundary_summary(
    caption: &box_tree::FrozenTableFragmentCaption<'_>,
    table_style: &ComputedStyle,
    inherited_page_name: Option<&str>,
) -> TablePageBoundarySummary {
    let style = caption.style.as_deref().unwrap_or(table_style);
    if !style_is_in_normal_flow(style) || style.display.is_none() {
        return TablePageBoundarySummary {
            sources: PageBoundaryValues::inapplicable(),
            resolved: ResolvedPageBoundaryValues::inapplicable(),
        };
    }
    TablePageBoundarySummary {
        sources: page_value_sources_from_style_and_children(style, &caption.children),
        resolved: resolved_page_boundary_values_from_style_and_children(
            style,
            &caption.children,
            inherited_page_name,
        ),
    }
}

fn table_row_page_boundary_summary(
    row: &TableRow<'_>,
    row_index: usize,
    row_group_end: usize,
    table_style: &ComputedStyle,
    inherited_page_name: Option<&str>,
) -> TablePageBoundarySummary {
    let row_style = row.style.as_deref().unwrap_or(table_style);
    if !style_is_in_normal_flow(row_style) || row_style.display.is_none() {
        return TablePageBoundarySummary {
            sources: PageBoundaryValues::inapplicable(),
            resolved: ResolvedPageBoundaryValues::inapplicable(),
        };
    }
    let mut summary = TablePageBoundarySummary::from_style(row_style, inherited_page_name);
    let own_page_name = summary.resolved.start.clone();
    if row_style.page.is_specified() {
        return summary;
    }

    let mut first_cell = None;
    let mut last_cell = None;
    let mut continuing_rowspan = None;
    for cell in &row.cells {
        let cell_style = cell.style.as_deref().unwrap_or(row_style);
        if !style_is_in_normal_flow(cell_style) || cell_style.display.is_none() {
            continue;
        }
        let children = cell.children.as_deref().unwrap_or_default();
        let sources = page_value_sources_from_style_and_children(cell_style, children);
        let resolved = resolved_page_boundary_values_from_style_and_children(
            cell_style,
            children,
            own_page_name.as_deref(),
        );
        if sources.end.overrides_parent_summary()
            && cell
                .element
                .is_some_and(|element| html_table_rowspan(element, row_index, row_group_end) > 1)
        {
            continuing_rowspan = Some((resolved.end.clone(), sources.end.clone()));
        }
        if first_cell.is_none() {
            first_cell = Some((resolved.start.clone(), sources.start.clone()));
        }
        last_cell = Some((resolved.end, sources.end));
    }
    if let Some((start, source)) = first_cell
        && source.overrides_parent_summary()
    {
        summary.sources.start = source;
        summary.resolved.start = start;
    }
    if let Some((end, source)) = last_cell
        && source.overrides_parent_summary()
    {
        summary.sources.end = source;
        summary.resolved.end = end;
    }
    if summary.resolved.end == own_page_name
        && let Some((end, source)) = continuing_rowspan
    {
        summary.sources.end = source;
        summary.resolved.end = end;
    }
    if summary.resolved.start == own_page_name
        && let Some(group) = row.row_groups.last()
    {
        let group_style = group.style.as_deref().unwrap_or(table_style);
        if group_style.page.is_specified() {
            let group_summary =
                TablePageBoundarySummary::from_style(group_style, inherited_page_name);
            summary.sources.start = group_summary.sources.start.clone();
            summary.resolved.start = group_summary.resolved.start.clone();
            if summary.resolved.end == own_page_name {
                summary.sources.end = group_summary.sources.end;
                summary.resolved.end = group_summary.resolved.end;
            }
        }
    }
    summary
}

pub(in crate::layout::table) fn table_metrics(
    element: &Element,
    style: &ComputedStyle,
) -> TableMetrics {
    if style.border_collapse == css::BorderCollapse::Collapse {
        return TableMetrics {
            border_collapse: style.border_collapse,
            spacing: css::BorderSpacing::ZERO,
        };
    }

    let spacing = if style.border_spacing.is_author_declared() {
        style.border_spacing.value().clone()
    } else if is_html_table_element(element) {
        element
            .attrs
            .get("cellspacing")
            .and_then(|value| parse_html_length(value))
            .map(|spacing| {
                let spacing = spacing.max(0.0) * style.effective_zoom.factor();
                css::BorderSpacing::from_lengths(spacing, spacing)
            })
            .unwrap_or_else(|| style.border_spacing.value().clone())
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

pub(in crate::layout::table) fn table_rows_from_fragment<'a>(
    fragment: &box_tree::TableFragment<'a>,
) -> Vec<TableRow<'a>> {
    table_row_ordering_from_fragment(fragment).rows
}

pub(in crate::layout::table) fn table_row_ordering_from_fragment<'a>(
    fragment: &box_tree::TableFragment<'a>,
) -> TableRowOrdering<'a> {
    let rows = table_source_rows_from_fragment(fragment);
    order_table_rows(rows)
}

fn table_source_rows_from_fragment<'a>(
    fragment: &box_tree::TableFragment<'a>,
) -> Vec<TableRow<'a>> {
    fragment
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
        .collect()
}

fn table_fragment_cell_is_running(cell: &box_tree::TableFragmentCell<'_>) -> bool {
    !cell.anonymous
        && cell
            .style
            .as_ref()
            .is_some_and(|style| style.position.is_running())
}

/// Apply CSS table row-group visual ordering before grid construction.
///
/// CSS 2.2 defines `table-header-group` as visually preceding all body rows and
/// `table-footer-group` as visually following all body rows; only the first
/// header group and first footer group receive that special treatment.
/// https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group
/// https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group
pub(in crate::layout::table) fn order_table_rows<'a>(
    rows: Vec<TableRow<'a>>,
) -> TableRowOrdering<'a> {
    let first_header = first_row_group_signature_matching(&rows, table_row_group_is_header);
    let first_footer = first_row_group_signature_matching(&rows, table_row_group_is_footer);
    if first_header.is_none() && first_footer.is_none() {
        return TableRowOrdering {
            rows,
            repeating_header_rows: Vec::new(),
            repeating_footer_rows: Vec::new(),
        };
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
    let repeating_header_rows = table_rows_matching_row_group_signature(&headers, first_header);
    let repeating_footer_rows = table_rows_matching_row_group_signature(&headers, first_footer);
    TableRowOrdering {
        rows: headers,
        repeating_header_rows,
        repeating_footer_rows,
    }
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

fn table_rows_matching_row_group_signature(
    rows: &[TableRow<'_>],
    signature: Option<ElementSignature>,
) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| {
            row_group_signature_matches(row.row_groups.last(), signature.as_ref()).then_some(index)
        })
        .collect()
}

pub(in crate::layout::table) fn table_grid(rows: &[TableRow<'_>]) -> TableGrid {
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

pub(in crate::layout::table) fn table_captions_from_fragment<'a>(
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

pub(in crate::layout::table) fn table_columns_from_fragment<'a>(
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

pub(in crate::layout::table) fn table_row_group_is_header(group: &TableRowGroup<'_>) -> bool {
    group
        .style
        .as_ref()
        .map(|style| style.display.is_table_header_group())
        .unwrap_or(is_html_table_header_group_element(group.element))
}

pub(in crate::layout::table) fn table_row_group_is_footer(group: &TableRowGroup<'_>) -> bool {
    group
        .style
        .as_ref()
        .map(|style| style.display.is_table_footer_group())
        .unwrap_or(is_html_table_footer_group_element(group.element))
}

pub(in crate::layout::table) fn table_row_group_spans<'a>(
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

pub(in crate::layout::table) fn group_signature<'a>(
    group: &'a Option<TableRowGroup<'_>>,
) -> Option<&'a ElementSignature> {
    group.as_ref().map(|group| &group.signature)
}

pub(in crate::layout::table) fn table_row_group_end_indices(rows: &[TableRow<'_>]) -> Vec<usize> {
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

pub(in crate::layout::table) fn table_column_group_spans<'a>(
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

pub(in crate::layout::table) fn column_group_signature<'a>(
    group: &'a Option<TableColumnGroup<'_>>,
) -> Option<&'a ElementSignature> {
    group.as_ref().map(|group| &group.signature)
}
