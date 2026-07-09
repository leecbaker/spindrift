#[cfg(test)]
use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_gap_decorations_project_from_committed_source_range() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        let gutters = GapDecorationGridGutters {
            columns: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
            rows: vec![GapDecorationGutter::with_grid_line(80.0, 90.0, Some(2))],
        };
        let items = [
            GapDecorationItem::from_rect_with_grid_area(
                GapDecorationRect::new(
                    GapDecorationPoint::new(0.0, 0.0),
                    GapDecorationSize::new(50.0, 120.0),
                ),
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 3,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::from_rect_with_grid_area(
                GapDecorationRect::new(
                    GapDecorationPoint::new(60.0, 0.0),
                    GapDecorationSize::new(50.0, 120.0),
                ),
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 3,
                    column_start: 2,
                    column_end: 3,
                },
            ),
        ];

        let source_segments = grid_gap_rule_paint_segments(
            &style,
            GapDecorationContainer::new(0.0, 200.0, 110.0, 120.0),
            &items,
            &gutters,
        );
        assert_eq!(source_segments.len(), 1);
        assert_eq!(source_segments[0].segment.start.position, 0.0);
        assert_eq!(source_segments[0].segment.end.position, 120.0);

        let primitives = grid_gap_decoration_primitives_for_page(GridGapFragmentProjection {
            style: &style,
            inner_x: 0.0,
            content_top: 200.0,
            inner_width: 110.0,
            total_content_height: 120.0,
            items: &items,
            gutters: &gutters,
            source_block_start: 50.0,
            source_block_end: 100.0,
            ends_at_fragment_break: true,
        });
        let strokes = solid_gap_rule_centerlines(&primitives);

        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].x1(), 55.0);
        assert_eq!(strokes[0].y1(), 200.0);
        assert_eq!(strokes[0].y2(), 146.0);
        assert_eq!(strokes[0].width, 4.0);
    }

    #[test]
    fn fragmented_grid_gap_rule_expands_terminal_cap_after_projection() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(0, 0, 255));
        let gutters = GapDecorationGridGutters {
            columns: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
            rows: Vec::new(),
        };

        let source_segments = grid_gap_rule_paint_segments(
            &style,
            GapDecorationContainer::new(0.0, 120.0, 110.0, 120.0),
            &[],
            &gutters,
        );
        assert_eq!(source_segments.len(), 1);
        assert_eq!(source_segments[0].segment.start.position, 0.0);
        assert_eq!(source_segments[0].segment.end.position, 120.0);

        let first =
            project_grid_gap_rule_segment_to_block_range(source_segments[0], 0.0, 100.0, true, &[])
                .expect("first fragment should contain the source rule");
        let second = project_grid_gap_rule_segment_to_block_range(
            source_segments[0],
            100.0,
            120.0,
            true,
            &[],
        )
        .expect("second fragment should contain the source rule");

        assert_eq!(first.segment.start.position, 0.0);
        assert_eq!(first.segment.end.position, 104.0);
        assert_eq!(second.segment.start.position, 0.0);
        assert_eq!(second.segment.end.position, 20.0);
    }
}
