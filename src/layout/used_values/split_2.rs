#[cfg(test)]
mod tests {
    use super::super::split_1::*;
    use crate::css;
    use crate::layout::{ComputedStyle, Direction};
    use crate::layout_pt;
    use crate::units::{SemanticLengthExt, non_content_pt};

    fn style_with_horizontal_margins(
        left: css::ComputedLengthPercentageOrAuto,
        right: css::ComputedLengthPercentageOrAuto,
        used_left: f32,
        used_right: f32,
    ) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = left;
        style.box_values.margin.right = right;
        style.margin.left = used_left;
        style.margin.right = used_right;
        style
    }

    fn length_auto(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn percent_auto(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(value),
        )
    }

    #[test]
    fn normal_flow_auto_width_expands_through_negative_margins() {
        let mut style =
            style_with_horizontal_margins(length_auto(-20.0), length_auto(-50.0), -20.0, -50.0);

        let horizontal_non_content = non_content_pt(20.0);
        let requested =
            used_normal_flow_block_content_box_width(&style, 100.0, horizontal_non_content);
        let width = resolve_normal_flow_block_width(
            &mut style,
            0.0,
            100.0,
            requested,
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        assert_eq!(requested.points(), 150.0);
        assert_eq!(width.content_width.points(), 150.0);
        assert_eq!(width.border_box_width.points(), 170.0);
        assert_eq!(width.border_box_x, -20.0);
    }

    #[test]
    fn normal_flow_percentage_width_uses_containing_block_despite_negative_margins() {
        let mut style =
            style_with_horizontal_margins(length_auto(-20.0), length_auto(-50.0), -20.0, -50.0);
        style.box_values.width = percent_auto(0.5);

        let horizontal_non_content = non_content_pt(0.0);
        let requested =
            used_normal_flow_block_content_box_width(&style, 100.0, horizontal_non_content);
        let width = resolve_normal_flow_block_width(
            &mut style,
            0.0,
            100.0,
            requested,
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        assert_eq!(requested.points(), 50.0);
        assert_eq!(width.content_width.points(), 50.0);
        assert_eq!(width.border_box_width.points(), 50.0);
        assert_eq!(width.border_box_x, -20.0);
    }

    #[test]
    fn normal_flow_rtl_fixed_width_anchors_from_right_side() {
        let mut style =
            style_with_horizontal_margins(length_auto(15.0), length_auto(20.0), 15.0, 20.0);
        style.box_values.width = length_auto(80.0);

        let horizontal_non_content = non_content_pt(0.0);
        let requested =
            used_normal_flow_block_content_box_width(&style, 100.0, horizontal_non_content);
        let width = resolve_normal_flow_block_width(
            &mut style,
            0.0,
            100.0,
            requested,
            horizontal_non_content,
            Direction::Rtl,
            true,
        );

        assert_eq!(requested.points(), 80.0);
        assert_eq!(width.content_width.points(), 80.0);
        assert_eq!(width.border_box_width.points(), 80.0);
        assert_eq!(width.border_box_x, 0.0);
    }

    #[test]
    fn normal_flow_block_width_keeps_content_and_border_box_types_distinct() {
        let mut style = style_with_horizontal_margins(length_auto(0.0), length_auto(0.0), 0.0, 0.0);
        style.box_values.width = length_auto(150.0);
        let horizontal_non_content = non_content_pt(20.0);
        let requested =
            used_normal_flow_block_content_box_width(&style, 300.0, horizontal_non_content);
        let width = resolve_normal_flow_block_width(
            &mut style,
            0.0,
            300.0,
            requested,
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        let _content: crate::units::ContentBoxLength = width.content_width;
        let _border: crate::units::BorderBoxLength = width.border_box_width;
        assert_eq!(width.content_width.points(), 150.0);
        assert_eq!(width.border_box_width.points(), 170.0);
    }

    #[test]
    fn normal_flow_block_width_uses_border_box_points_for_auto_margins() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::Auto,
            0.0,
            0.0,
        );
        style.box_values.width = length_auto(150.0);
        let horizontal_non_content = non_content_pt(20.0);
        let requested =
            used_normal_flow_block_content_box_width(&style, 200.0, horizontal_non_content);

        let width = resolve_normal_flow_block_width(
            &mut style,
            0.0,
            200.0,
            requested,
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        assert_eq!(width.border_box_width.points(), 170.0);
        assert_eq!(style.margin.left, 15.0);
        assert_eq!(style.margin.right, 15.0);
    }

    #[test]
    fn both_auto_margins_keep_start_side_zero_when_ltr_block_overflows() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::Auto,
            0.0,
            0.0,
        );

        resolve_normal_flow_auto_margins_for_known_width(&mut style, 100.0, 200.0, Direction::Ltr);

        assert_eq!(style.margin.left, 0.0);
        assert_eq!(style.margin.right, -100.0);
    }

    #[test]
    fn right_auto_margin_can_be_negative_when_ltr_block_overflows() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(25.0),
            ),
            css::ComputedLengthPercentageOrAuto::Auto,
            25.0,
            0.0,
        );

        resolve_normal_flow_auto_margins_for_known_width(&mut style, 100.0, 200.0, Direction::Ltr);

        assert_eq!(style.margin.left, 25.0);
        assert_eq!(style.margin.right, -125.0);
    }

    #[test]
    fn left_auto_margin_stays_zero_when_ltr_block_overflows() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(25.0),
            ),
            0.0,
            25.0,
        );

        resolve_normal_flow_auto_margins_for_known_width(&mut style, 100.0, 200.0, Direction::Ltr);

        assert_eq!(style.margin.left, 0.0);
        assert_eq!(style.margin.right, -100.0);
    }

    #[test]
    fn both_auto_margins_keep_end_side_zero_when_rtl_block_overflows() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::Auto,
            0.0,
            0.0,
        );

        resolve_normal_flow_auto_margins_for_known_width(&mut style, 100.0, 200.0, Direction::Rtl);

        assert_eq!(style.margin.left, -100.0);
        assert_eq!(style.margin.right, 0.0);
    }

    #[tokio::test]
    async fn used_lengths_resolve_percentage_against_basis() {
        let value = css::ComputedLengthPercentage {
            length: layout_pt(12.0),
            percent: 0.25,
            ch: 0.0,
            ..css::ComputedLengthPercentage::ZERO
        };
        assert_eq!(used_length_percentage(value, 200.0), 62.0);
    }

    #[tokio::test]
    async fn mutating_used_width_replaces_typed_percentage_with_used_length() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );

        set_style_used_width(&mut style, 42.0);

        assert_eq!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(42.0)
            )
        );
    }

    #[tokio::test]
    async fn multicol_count_can_derive_from_computed_column_width() {
        let mut style = ComputedStyle::initial();
        style.column_width =
            css::ComputedColumnWidth::Length(css::ComputedLengthPercentage::from_points(40.0));

        assert_eq!(used_multicol_column_count(&style, 150.0, 10.0), Some(3));

        style.column_count = Some(2);
        assert_eq!(used_multicol_column_count(&style, 150.0, 10.0), Some(2));
    }
}
