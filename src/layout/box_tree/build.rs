use super::*;

pub(crate) fn build_page_box<'a>(
    root: &'a Node,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
) -> PageBox<'a> {
    let children = match &root.kind {
        NodeKind::Element(element) => build_child_boxes(element, stylesheets, parent_style, &[]),
        NodeKind::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![FormattingBox::Text(TextBox {
                    text: text.clone(),
                    style: parent_style.clone(),
                })]
            }
        }
    };
    PageBox { children }
}

pub(crate) fn build_child_boxes<'a>(
    element: &'a Element,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
    ancestors: &[ElementSignature],
) -> Vec<FormattingBox<'a>> {
    build_child_boxes_inner(element, stylesheets, parent_style, ancestors, true)
}

fn build_child_boxes_inner<'a>(
    element: &'a Element,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
    ancestors: &[ElementSignature],
    normalize_for_parent: bool,
) -> Vec<FormattingBox<'a>> {
    let sibling_tags = element_sibling_tags(element);
    let mut element_index = 0usize;
    let mut raw = Vec::new();
    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => {
                if !text.is_empty() {
                    raw.push(FormattingBox::Text(TextBox {
                        text: text.clone(),
                        style: parent_style.clone(),
                    }));
                }
            }
            NodeKind::Element(child_element) => {
                let signature = ElementSignature::with_siblings(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let style = style_for_layout_element(
                    child_element,
                    signature.clone(),
                    stylesheets,
                    Some(parent_style),
                    ancestors,
                );
                let style = if ancestors.is_empty() {
                    root_display_fixed_style(style)
                } else {
                    style
                };
                if style.display.is_contents() {
                    // CSS Display 3 `display: contents` suppresses the
                    // element's principal box but keeps its children in the box
                    // tree, inheriting from the contents element and matching
                    // selectors with that element in their ancestor chain.
                    // https://www.w3.org/TR/css-display-3/#valdef-display-contents
                    let mut child_ancestors = ancestors.to_vec();
                    child_ancestors.push(signature);
                    raw.extend(build_child_boxes_inner(
                        child_element,
                        stylesheets,
                        &style,
                        &child_ancestors,
                        false,
                    ));
                } else if let Some(box_) =
                    build_element_box(child_element, signature, style, stylesheets, ancestors)
                {
                    raw.push(box_);
                }
            }
        }
    }
    if normalize_for_parent {
        normalize_block_container_children(raw, parent_style)
    } else {
        raw
    }
}

pub(crate) fn build_element_box<'a>(
    element: &'a Element,
    signature: ElementSignature,
    mut style: ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
) -> Option<FormattingBox<'a>> {
    if style.display.is_none() {
        return None;
    }
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        style.abspos_static_source_was_inline_level = style.display.is_inline_level();
        style.display = style.display.blockified();
    }

    let content_replacement = matches!(style.content, Content::Replacement { .. });
    let mut child_ancestors = ancestors.to_vec();
    child_ancestors.push(signature.clone());
    let children = if content_replacement || is_horizontal_rule_element(element) {
        Vec::new()
    } else {
        build_child_boxes(element, stylesheets, &style, &child_ancestors)
    };
    let marker = marker_box(&style);

    if content_replacement || is_replaced_element(element) {
        let mut style = style;
        style.display = if style.display.is_block_level() {
            Display::BLOCK_REPLACED.with_list_item(style.display.is_list_item())
        } else if style.display.is_run_in() {
            style.display.with_inner(DisplayInner::Replaced)
        } else {
            Display::INLINE_REPLACED.with_list_item(style.display.is_list_item())
        };
        return if style.display.is_inline_or_run_in_level() {
            Some(FormattingBox::AtomicInline(AtomicInlineBox {
                element,
                signature,
                marker,
                style,
                children,
                table_fragment: None,
            }))
        } else {
            Some(FormattingBox::Replaced(ReplacedBox {
                element,
                signature,
                marker,
                style,
                children,
            }))
        };
    }

    if style.display.is_table() && style.display.is_inline_or_run_in_level() {
        let fragment = build_table_fragment(element, &signature, &children);
        Some(FormattingBox::AtomicInline(AtomicInlineBox {
            element,
            signature,
            marker,
            style,
            children,
            table_fragment: Some(fragment),
        }))
    } else if style.display.is_table()
        || (style.display.is_block_level() && is_html_table_element(element))
    {
        let fragment = build_table_fragment(element, &signature, &children);
        Some(FormattingBox::Table(TableBox {
            element,
            signature,
            marker,
            style,
            children,
            fragment,
        }))
    } else if style.display.is_flex() && style.display.is_block_level() {
        Some(FormattingBox::Flex(FlexBox {
            element,
            signature,
            marker,
            style,
            children,
        }))
    } else if style.display.is_atomic_inline()
        || (style.display.is_run_in() && !style.display.is_flow())
    {
        Some(FormattingBox::AtomicInline(AtomicInlineBox {
            element,
            signature,
            marker,
            style,
            children,
            table_fragment: None,
        }))
    } else if style.display.is_block_level() {
        Some(FormattingBox::Block(BlockBox {
            element,
            signature,
            marker,
            style,
            run_in_children: Vec::new(),
            children,
        }))
    } else {
        Some(FormattingBox::Inline(InlineBox {
            element,
            signature,
            marker,
            style,
            fragment_edges: InlineBoxFragmentEdges::ALL,
            children,
        }))
    }
}

/// Applies CSS Display root-element display fixups during box-tree construction.
///
/// CSS Display 4 blockifies the root element's principal box, and
/// `display: contents` computes to `block` on the root:
/// <https://www.w3.org/TR/css-display-4/#root>.
fn root_display_fixed_style(mut style: ComputedStyle) -> ComputedStyle {
    style.display = if style.display.is_contents() {
        Display::BLOCK
    } else {
        style.display.blockified()
    };
    style
}

/// Build a durable CSS table fragment from normalized child boxes.
///
/// CSS 2.2 anonymous table object construction and table layout require a
/// stable table wrapper, row-group, row, cell, column, caption, and occupancy
/// model before layout:
/// <https://www.w3.org/TR/CSS22/tables.html#anonymous-boxes>.
pub(crate) fn build_table_fragment<'a>(
    element: &'a Element,
    signature: &ElementSignature,
    children: &[FormattingBox<'a>],
) -> TableFragment<'a> {
    let captions = table_fragment_captions(children);
    let columns = table_fragment_columns(children);
    let mut rows = Vec::new();
    collect_table_fragment_rows(children, &mut rows, std::slice::from_ref(signature), &[]);
    if rows.is_empty() && is_html_table_row_element(element) {
        rows.push(TableFragmentRow {
            element: Some(element),
            signature: signature.clone(),
            ancestors: Vec::new(),
            row_groups: Vec::new(),
            style: None,
            cells: Vec::new(),
        });
    }
    let grid = table_fragment_grid(&rows);
    TableFragment {
        rows,
        captions,
        columns,
        grid,
    }
}

fn table_fragment_captions<'a>(children: &[FormattingBox<'a>]) -> Vec<TableFragmentCaption<'a>> {
    let mut captions = Vec::new();
    collect_table_fragment_captions(children, &mut captions);
    captions
}

fn collect_table_fragment_captions<'a>(
    children: &[FormattingBox<'a>],
    captions: &mut Vec<TableFragmentCaption<'a>>,
) {
    for child in children {
        if let Some((element, signature, style, descendants)) = child.element_parts()
            && is_table_caption_box(element, style)
        {
            captions.push(TableFragmentCaption {
                element,
                signature: signature.clone(),
                style: Some(style.clone()),
                children: descendants.to_vec(),
            });
            continue;
        }
        collect_table_fragment_captions(child.children(), captions);
    }
}

fn table_fragment_columns<'a>(children: &[FormattingBox<'a>]) -> Vec<TableFragmentColumn<'a>> {
    let mut columns = Vec::new();
    collect_table_fragment_columns(children, &mut columns);
    columns
}

fn collect_table_fragment_columns<'a>(
    children: &[FormattingBox<'a>],
    columns: &mut Vec<TableFragmentColumn<'a>>,
) {
    for child in children {
        let Some((element, signature, style, descendants)) = child.element_parts() else {
            continue;
        };
        if is_table_column_box(element, style) {
            columns.push(TableFragmentColumn {
                element,
                signature: signature.clone(),
                style: Some(style.clone()),
                group: None,
                span: html_table_column_span(element),
            });
            continue;
        }
        if is_table_column_group_box(element, style) {
            collect_table_fragment_column_group(element, signature, style, descendants, columns);
            continue;
        }
        collect_table_fragment_columns(descendants, columns);
    }
}

fn collect_table_fragment_column_group<'a>(
    group_element: &'a Element,
    group_signature: &ElementSignature,
    group_style: &ComputedStyle,
    children: &[FormattingBox<'a>],
    columns: &mut Vec<TableFragmentColumn<'a>>,
) {
    let group = TableFragmentColumnGroup {
        element: group_element,
        signature: group_signature.clone(),
        style: Some(group_style.clone()),
        span: html_table_column_span(group_element),
    };
    let mut saw_column = false;
    for child in children {
        let Some((element, signature, style, _)) = child.element_parts() else {
            continue;
        };
        if is_table_column_box(element, style) {
            saw_column = true;
            columns.push(TableFragmentColumn {
                element,
                signature: signature.clone(),
                style: Some(style.clone()),
                group: Some(group.clone()),
                span: html_table_column_span(element),
            });
        }
    }
    if !saw_column {
        columns.push(TableFragmentColumn {
            element: group_element,
            signature: group_signature.clone(),
            style: Some(group_style.clone()),
            group: Some(group.clone()),
            span: group.span,
        });
    }
}

fn collect_table_fragment_rows<'a>(
    children: &[FormattingBox<'a>],
    rows: &mut Vec<TableFragmentRow<'a>>,
    ancestors: &[ElementSignature],
    row_groups: &[TableFragmentRowGroup<'a>],
) {
    let mut anonymous_cells = Vec::new();
    let mut anonymous_cell_children = Vec::new();
    for (index, child) in children.iter().enumerate() {
        let Some((element, signature, style, descendants)) = child.element_parts() else {
            if matches!(child, FormattingBox::Text(_))
                && !table_fragment_whitespace_is_ignorable(children, index)
            {
                anonymous_cell_children.push(child.clone());
            }
            continue;
        };
        if is_table_row_box(element, style) {
            flush_anonymous_table_fragment_cell(&mut anonymous_cells, &mut anonymous_cell_children);
            flush_anonymous_table_fragment_row(rows, &mut anonymous_cells, ancestors, row_groups);
            let cells = table_fragment_row_child_cells(descendants);
            rows.push(TableFragmentRow {
                element: Some(element),
                signature: signature.clone(),
                ancestors: ancestors.to_vec(),
                row_groups: row_groups.to_vec(),
                style: Some(style.clone()),
                cells,
            });
            continue;
        }
        if is_table_cell_box(element, style) {
            flush_anonymous_table_fragment_cell(&mut anonymous_cells, &mut anonymous_cell_children);
            anonymous_cells.push(TableFragmentCell {
                element: Some(element),
                signature: signature.clone(),
                children: descendants.to_vec(),
                anonymous: false,
            });
            continue;
        }
        if is_table_column_box(element, style)
            || is_table_column_group_box(element, style)
            || is_table_caption_box(element, style)
        {
            continue;
        }
        if is_table_row_group_box(element, style) {
            flush_anonymous_table_fragment_cell(&mut anonymous_cells, &mut anonymous_cell_children);
            flush_anonymous_table_fragment_row(rows, &mut anonymous_cells, ancestors, row_groups);
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(signature.clone());
            let mut child_row_groups = row_groups.to_vec();
            child_row_groups.push(TableFragmentRowGroup {
                element,
                signature: signature.clone(),
                style: Some(style.clone()),
            });
            collect_table_fragment_rows(descendants, rows, &child_ancestors, &child_row_groups);
            continue;
        }
        anonymous_cell_children.push(child.clone());
    }
    flush_anonymous_table_fragment_cell(&mut anonymous_cells, &mut anonymous_cell_children);
    flush_anonymous_table_fragment_row(rows, &mut anonymous_cells, ancestors, row_groups);
}

fn table_fragment_row_child_cells<'a>(
    children: &[FormattingBox<'a>],
) -> Vec<TableFragmentCell<'a>> {
    let mut cells = Vec::new();
    let mut anonymous_cell_children = Vec::new();
    for (index, child) in children.iter().enumerate() {
        let Some((element, signature, style, descendants)) = child.element_parts() else {
            if matches!(child, FormattingBox::Text(_))
                && !table_fragment_whitespace_is_ignorable(children, index)
            {
                anonymous_cell_children.push(child.clone());
            }
            continue;
        };
        if is_table_cell_box(element, style) {
            flush_anonymous_table_fragment_cell(&mut cells, &mut anonymous_cell_children);
            cells.push(TableFragmentCell {
                element: Some(element),
                signature: signature.clone(),
                children: descendants.to_vec(),
                anonymous: false,
            });
            continue;
        }
        if is_table_column_box(element, style)
            || is_table_column_group_box(element, style)
            || is_table_caption_box(element, style)
            || is_table_row_group_box(element, style)
            || is_table_row_box(element, style)
        {
            continue;
        }
        anonymous_cell_children.push(child.clone());
    }
    flush_anonymous_table_fragment_cell(&mut cells, &mut anonymous_cell_children);
    cells
}

/// Return whether whitespace is ignored while fixing up table-internal boxes.
///
/// CSS Tables ignores whitespace-only anonymous inline boxes that touch
/// table-internal boxes, but the consecutive-box rules keep whitespace between
/// non-internal siblings so it can participate in the generated anonymous cell:
/// <https://drafts.csswg.org/css-tables/#consecutive-boxes>.
fn table_fragment_whitespace_is_ignorable(children: &[FormattingBox<'_>], index: usize) -> bool {
    children
        .get(index)
        .is_some_and(formatting_box_is_collapsible_space)
        && (index == 0
            || index + 1 == children.len()
            || table_fragment_box_is_internal_or_caption(&children[index - 1])
            || table_fragment_box_is_internal_or_caption(&children[index + 1]))
}

fn table_fragment_box_is_internal_or_caption(box_: &FormattingBox<'_>) -> bool {
    let Some((element, _, style, _)) = box_.element_parts() else {
        return false;
    };
    is_table_caption_box(element, style)
        || is_table_column_group_box(element, style)
        || is_table_column_box(element, style)
        || is_table_row_group_box(element, style)
        || is_table_row_box(element, style)
        || is_table_cell_box(element, style)
}

/// Flush consecutive improper table children into one anonymous table cell.
///
/// CSS Tables treats consecutive non-table-cell boxes as one run when
/// generating missing cells, and only ignores whitespace for table-internal
/// adjacency. Whitespace between improper children therefore remains inline
/// content inside the generated cell:
/// <https://drafts.csswg.org/css-tables/#consecutive-boxes> and
/// <https://www.w3.org/TR/CSS22/tables.html#anonymous-boxes>.
fn flush_anonymous_table_fragment_cell<'a>(
    cells: &mut Vec<TableFragmentCell<'a>>,
    children: &mut Vec<FormattingBox<'a>>,
) {
    if children.is_empty() {
        return;
    }
    cells.push(TableFragmentCell {
        element: None,
        signature: ElementSignature::new("td", HashMap::new()),
        children: std::mem::take(children),
        anonymous: true,
    });
}

fn flush_anonymous_table_fragment_row<'a>(
    rows: &mut Vec<TableFragmentRow<'a>>,
    cells: &mut Vec<TableFragmentCell<'a>>,
    ancestors: &[ElementSignature],
    row_groups: &[TableFragmentRowGroup<'a>],
) {
    if cells.is_empty() {
        return;
    }
    rows.push(TableFragmentRow {
        element: cells[0].element,
        signature: ElementSignature::new("tr", HashMap::new()),
        ancestors: ancestors.to_vec(),
        row_groups: row_groups.to_vec(),
        style: Some(css::default_style_for_tag("tr")),
        cells: std::mem::take(cells),
    });
}

fn table_fragment_grid(rows: &[TableFragmentRow<'_>]) -> TableFragmentGrid {
    let mut grid_rows = Vec::with_capacity(rows.len());
    let mut active_rowspans: Vec<usize> = Vec::new();
    let mut column_count = 0usize;
    let row_group_ends = table_fragment_row_group_end_indices(rows);

    for (row_index, row) in rows.iter().enumerate() {
        let mut placements = Vec::new();
        let mut column = 0usize;
        for (cell_index, cell) in row.cells.iter().enumerate() {
            while active_rowspans.get(column).copied().unwrap_or(0) > 0 {
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
            placements.push(TableFragmentCellPlacement {
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
        while active_rowspans.last().copied() == Some(0) {
            active_rowspans.pop();
        }
        grid_rows.push(placements);
    }

    TableFragmentGrid {
        rows: grid_rows,
        column_count: column_count.max(1),
    }
}

fn table_fragment_row_group_end_indices(rows: &[TableFragmentRow<'_>]) -> Vec<usize> {
    let mut ends = vec![rows.len(); rows.len()];
    let mut start = 0usize;
    let mut current_group = rows.first().and_then(|row| row.row_groups.last().cloned());
    for (index, row) in rows.iter().enumerate() {
        let group = row.row_groups.last().cloned();
        if index > 0
            && table_fragment_group_signature(&group)
                != table_fragment_group_signature(&current_group)
        {
            for end in &mut ends[start..index] {
                *end = index;
            }
            start = index;
            current_group = group;
        }
    }
    ends
}

fn table_fragment_group_signature<'a>(
    group: &'a Option<TableFragmentRowGroup<'_>>,
) -> Option<&'a ElementSignature> {
    group.as_ref().map(|group| &group.signature)
}

fn is_table_caption_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_caption_element(element) || style.display.is_table_caption()
}

fn is_table_column_group_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_column_group_element(element) || style.display.is_table_column_group()
}

fn is_table_column_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_column_element(element) || style.display.is_table_column()
}

fn is_table_row_group_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_row_group_element(element) || style.display.is_table_row_group()
}

fn is_table_row_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_row_element(element) || style.display.is_table_row()
}

fn is_table_cell_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_cell_element(element) || style.display.is_table_cell()
}

pub(crate) fn marker_box(style: &ComputedStyle) -> Option<MarkerBox> {
    // CSS Lists 3: a list item generates a marker box associated with the
    // principal box. Full `::marker` styling will replace this style clone.
    // https://www.w3.org/TR/css-lists-3/#markers
    style.display.is_list_item().then(|| MarkerBox {
        style: style
            .marker_style
            .as_deref()
            .cloned()
            .unwrap_or_else(|| style.clone()),
    })
}
