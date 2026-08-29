//! Table row, span, grid, and border-spacing metrics.

use super::*;
pub(in crate::layout::table) fn table_row_span_height(
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
pub(in crate::layout::table) fn table_row_block_start(
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
pub(in crate::layout::table) fn table_grid_height(
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
pub(in crate::layout::table) fn table_vertical_edge_spacing(
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
pub(in crate::layout::table) fn table_content_height(
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
pub(in crate::layout::table) fn repeated_table_rows_height(
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

pub(in crate::layout::table) fn table_row_top(
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
pub(in crate::layout::table) fn inline_table_first_occupying_row_range(
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

pub(in crate::layout::table) fn table_internal_vertical_gap_count_before(
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

pub(in crate::layout::table) fn table_row_is_collapsed(style: &ComputedStyle) -> bool {
    // CSS 2.2 `visibility: collapse` on table rows removes the row's occupied
    // space; outside table row/column objects it behaves like `hidden`.
    style.visibility == Visibility::Collapse
}

/// Return the horizontal border contribution outside a table cell's content box.
pub(in crate::layout::table) fn table_horizontal_borders(
    style: &ComputedStyle,
) -> NonContentLength {
    let borders = table_cell_used_border_edges(style);
    non_content_pt(borders.left + borders.right)
}

/// Return the vertical border contribution outside a table cell's content box.
pub(in crate::layout::table) fn table_vertical_borders(style: &ComputedStyle) -> NonContentLength {
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
pub(in crate::layout::table) fn table_cell_used_border_edges(style: &ComputedStyle) -> css::Edges {
    let mut borders = used_border_widths(style);
    if style.border_collapse == css::BorderCollapse::Collapse {
        borders.top *= 0.5;
        borders.right *= 0.5;
        borders.bottom *= 0.5;
        borders.left *= 0.5;
    }
    borders
}

#[cfg(test)]
mod ordering_tests {
    use std::collections::HashMap;

    use super::*;
    use crate::dom::{Node, NodeKind};

    fn test_signature(name: &str) -> ElementSignature {
        ElementSignature::new(name, HashMap::new())
    }

    fn table_row<'a>(
        row_signature: ElementSignature,
        row_group: &'a Element,
        row_group_signature: ElementSignature,
    ) -> TableRow<'a> {
        TableRow {
            element: None,
            signature: row_signature,
            ancestors: Vec::new(),
            row_groups: vec![TableRowGroup {
                element: row_group,
                signature: row_group_signature,
                style: None,
            }],
            style: None,
            cells: Vec::new(),
            running_cells: Vec::new(),
        }
    }

    #[test]
    fn source_selected_header_and_footer_survive_visual_row_ordering() {
        let group_nodes = [
            Node::element("tbody"),
            Node::element("thead"),
            Node::element("thead"),
            Node::element("tbody"),
            Node::element("thead"),
            Node::element("tfoot"),
            Node::element("tbody"),
            Node::element("tfoot"),
            Node::element("tbody"),
            Node::element("tfoot"),
        ];
        let group_elements = group_nodes
            .iter()
            .map(|node| match &node.kind {
                NodeKind::Element(element) => element,
                NodeKind::Text(_) => unreachable!("table fixture contains only elements"),
            })
            .collect::<Vec<_>>();
        let group_signatures = (0..group_nodes.len())
            .map(|index| test_signature(&format!("group-{index}")))
            .collect::<Vec<_>>();
        let source_row_signatures = [
            "body-1", "head-1", "head-2", "body-2", "head-3", "foot-1", "body-3", "foot-2",
            "body-4", "foot-3",
        ]
        .map(test_signature);
        let rows = source_row_signatures
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, row_signature)| {
                table_row(
                    row_signature,
                    group_elements[index],
                    group_signatures[index].clone(),
                )
            })
            .collect();

        let ordering = order_table_rows(rows);
        let expected_visual_order =
            [1, 0, 2, 3, 4, 6, 7, 8, 9, 5].map(|index| source_row_signatures[index].clone());

        assert_eq!(
            ordering
                .rows
                .iter()
                .map(|row| row.signature.clone())
                .collect::<Vec<_>>(),
            expected_visual_order,
        );
        assert_eq!(ordering.repeating_header_rows, vec![0]);
        assert_eq!(ordering.repeating_footer_rows, vec![9]);
    }
}
