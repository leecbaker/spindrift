//! Used cell padding and unlaid cell-content measurement.

use super::*;
pub(in crate::layout::table) fn apply_table_cellpadding(
    style: &mut ComputedStyle,
    table_cellpadding: Option<TableCellPadding>,
) {
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
            top: cellpadding.points(),
            right: cellpadding.points(),
            bottom: cellpadding.points(),
            left: cellpadding.points(),
        };
        let cellpadding = css::ComputedLengthPercentage::from_points(cellpadding.points());
        style.box_values.padding = css::PhysicalEdges {
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
pub(in crate::layout::table) fn apply_table_cell_used_padding(
    style: &mut ComputedStyle,
    table_cellpadding: Option<TableCellPadding>,
    inline_basis: LogicalInlinePercentageBasis,
) {
    apply_table_cellpadding(style, table_cellpadding);
    // Keep the containing block's logical-inline marker through used-value
    // resolution; table-cell padding is physical only after this boundary.
    style.padding = used_padding_edges_for_logical_inline_basis(style, inline_basis).to_css_edges();
}

/// Collect the inline textual content used by anonymous and real table cells.
///
/// CSS 2.2 anonymous table object construction can wrap raw text nodes in
/// generated rows/cells. Durable table fragments therefore need a content path
/// that does not require a real `td`/`th` element:
/// <https://www.w3.org/TR/CSS22/tables.html#anonymous-boxes>.
pub(in crate::layout::table) fn table_cell_inline_text(cell: &TableCell<'_>) -> String {
    if let Some(children) = cell.children.as_deref() {
        let mut text = String::new();
        collect_inline_text_from_formatting_boxes(children, &mut text);
        return text;
    }
    cell.element.map(inline_text).unwrap_or_default()
}

pub(in crate::layout::table) fn table_cell_href<'a>(cell: &'a TableCell<'a>) -> Option<&'a str> {
    cell.element
        .and_then(|element| element.attrs.get("href").map(String::as_str))
}

/// Measure replaced descendants that contribute to table-cell intrinsic height.
///
/// CSS table row height depends on the maximum minimum height of cells in the
/// row, including replaced content:
/// <https://www.w3.org/TR/CSS22/tables.html#height-layout>.
pub(in crate::layout::table) fn table_cell_replaced_content_height(
    cell: &TableCell<'_>,
) -> LayoutLength {
    if let Some(children) = cell.children.as_deref() {
        return layout_pt(
            children
                .iter()
                .filter_map(|child| match child {
                    box_tree::FormattingBox::Replaced(box_)
                        if replaced_element_kind(box_.core.element)
                            == Some(ReplacedElementKind::Svg) =>
                    {
                        svg_rect(box_.core.element).map(|(_width, height, _fill)| height)
                    }
                    box_tree::FormattingBox::AtomicInline(box_)
                        if replaced_element_kind(box_.core.element)
                            == Some(ReplacedElementKind::Svg) =>
                    {
                        svg_rect(box_.core.element).map(|(_width, height, _fill)| height)
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
pub(in crate::layout::table) fn table_cell_non_text_content_height(
    cell: &TableCell<'_>,
) -> LayoutLength {
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
pub(in crate::layout::table) fn table_cell_formatting_child_outer_height(
    child: &box_tree::FormattingBox<'_>,
) -> LayoutLength {
    table_cell_inline_level_outer_height(child)
        .unwrap_or_else(|| table_cell_block_child_height(child))
}

fn table_cell_block_child_height(child: &box_tree::FormattingBox<'_>) -> LayoutLength {
    match child {
        box_tree::FormattingBox::Block(box_) => table_cell_element_outer_height(
            box_.core.element,
            &box_.core.style,
            &box_.core.children,
        ),
        box_tree::FormattingBox::Table(box_) => table_cell_element_outer_height(
            box_.core.element,
            &box_.core.style,
            &box_.core.children,
        ),
        box_tree::FormattingBox::Flex(box_) => table_cell_element_outer_height(
            box_.core.element,
            &box_.core.style,
            &box_.core.children,
        ),
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            table_cell_formatting_children_block_height(&box_.children)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            table_cell_formatting_children_block_height(&box_.core.children)
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
            let height = if box_.core.style.float != Float::None {
                used_content_box_height_or_auto(
                    &box_.core.style,
                    layout_pt(0.0),
                    non_content_pt(
                        box_.core.style.padding.top
                            + box_.core.style.padding.bottom
                            + vertical_border_width(&box_.core.style),
                    ),
                )
                .map(SemanticLengthExt::points)
                .unwrap_or_else(|| {
                    table_cell_formatting_children_block_height(&box_.core.children).points()
                }) + box_.core.style.margin.top
                    + box_.core.style.margin.bottom
            } else {
                table_cell_formatting_children_block_height(&box_.core.children).points()
            };
            (height > 0.0).then_some(layout_pt(height))
        }
        box_tree::FormattingBox::AtomicInline(box_) => Some(table_cell_atomic_inline_outer_height(
            box_.core.element,
            &box_.core.style,
            &box_.core.children,
        )),
        box_tree::FormattingBox::Replaced(box_) => Some(table_cell_atomic_inline_outer_height(
            box_.core.element,
            &box_.core.style,
            &box_.core.children,
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
    element: &Element,
    style: &ComputedStyle,
    children: &[box_tree::FormattingBox<'_>],
) -> LayoutLength {
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        return layout_pt(0.0);
    }
    let containment = used_property_containment(element, style);
    let nested_height = if containment.size {
        0.0
    } else {
        table_cell_formatting_children_block_height(children).points()
    };
    let vertical_non_content =
        non_content_pt(style.padding.top + style.padding.bottom) + table_vertical_borders(style);
    let preferred_content_height = if containment.size {
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
    if !containment.size {
        content_height = content_height.max(nested_height);
    }
    layout_pt(
        (content_height + vertical_non_content.points() + style.margin.top + style.margin.bottom)
            .max(0.0),
    )
}

fn table_cell_element_outer_height(
    element: &Element,
    style: &ComputedStyle,
    children: &[box_tree::FormattingBox<'_>],
) -> LayoutLength {
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        return layout_pt(0.0);
    }
    let containment = used_property_containment(element, style);
    let nested_height = if containment.size {
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
    if !containment.size {
        content_height = content_height.max(nested_height);
    }
    layout_pt(
        (content_height + vertical_non_content.points() + style.margin.top + style.margin.bottom)
            .max(0.0),
    )
}
