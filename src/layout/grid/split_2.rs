#[cfg(test)]
use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_gap_decorations_project_into_page_fragment_bounds() {
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
            GapDecorationItem::with_grid_area(
                0.0,
                0.0,
                50.0,
                120.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 3,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::with_grid_area(
                60.0,
                0.0,
                50.0,
                120.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 3,
                    column_start: 2,
                    column_end: 3,
                },
            ),
        ];

        let primitives = grid_gap_decoration_primitives_for_page(GridGapFragmentProjection {
            style: &style,
            inner_x: 0.0,
            content_top: 200.0,
            inner_width: 110.0,
            total_content_height: 120.0,
            items: &items,
            gutters: &gutters,
            fragment_bounds: PaintClip::new(0.0, 100.0, 110.0, 50.0),
        });
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].x1(), 55.0);
        assert_eq!(strokes[0].y1(), 150.0);
        assert_eq!(strokes[0].y2(), 100.0);
        assert_eq!(strokes[0].width, 4.0);
    }
}
