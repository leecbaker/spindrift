//! Table-part border painting.

use super::*;
pub(in crate::layout::table) fn paint_table_border_edges(
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
