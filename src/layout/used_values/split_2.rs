#[cfg(test)]
mod tests {
    use super::super::split_1::*;
    use crate::css;
    use crate::layout::{
        BoxSizing, ComputedStyle, Direction, PageInlineSpan, PhysicalContentWidth,
    };
    use crate::units::layout_pt;
    use crate::units::{
        ContentBoxLength, LayoutLength, PercentageBasis, SemanticLengthExt, border_box_pt,
        content_box_pt, layout_points, non_content_pt,
    };

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

    fn fit_content(points: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::FitContent(Some(
            css::ComputedLengthPercentage::from_points(points),
        ))
    }

    fn fit_content_percent(percent: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::FitContent(Some(
            css::ComputedLengthPercentage::from_percent(percent),
        ))
    }

    fn intrinsic_width_constraint_result(style: &ComputedStyle, value: f32) -> f32 {
        constrain_width_with_intrinsic(
            style,
            content_box_pt(value),
            content_box_pt(60.0),
            content_box_pt(120.0),
            PercentageBasis::definite(content_box_pt(300.0)),
            non_content_pt(0.0),
        )
        .points()
    }

    #[test]
    fn percentage_basis_carries_definite_typed_values_without_indefinite_numbers() {
        let definite = PercentageBasis::definite_from(
            content_box_pt(42.0),
            BlockSizeBasisSource::ContainingBlock,
        );
        let indefinite: BlockSizePercentageBasis = PercentageBasis::indefinite();

        assert!(definite.is_definite());
        assert_eq!(definite.points(), Some(42.0));
        assert_eq!(
            definite
                .map_value(|value| content_box_pt(value.points() * 2.0))
                .points(),
            Some(84.0)
        );
        assert!(!indefinite.is_definite());
        assert_eq!(indefinite.points(), None);
    }

    #[test]
    fn normal_flow_auto_width_expands_through_negative_margins() {
        let mut style =
            style_with_horizontal_margins(length_auto(-20.0), length_auto(-50.0), -20.0, -50.0);

        let horizontal_non_content = non_content_pt(20.0);
        let requested = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(100.0),
            horizontal_non_content,
        );
        let width = resolve_normal_flow_block_inline_geometry(
            &mut style,
            PageInlineSpan::from_edges(0.0, 100.0),
            PhysicalContentWidth::new(requested),
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        assert_eq!(requested.points(), 150.0);
        assert_eq!(width.content_width.points(), 150.0);
        assert_eq!(width.border_box_width().points(), 170.0);
        assert_eq!(width.border_box_inline_span.left_x(), -20.0);
    }

    #[test]
    fn normal_flow_percentage_width_uses_containing_block_despite_negative_margins() {
        let mut style =
            style_with_horizontal_margins(length_auto(-20.0), length_auto(-50.0), -20.0, -50.0);
        style.box_values.width = percent_auto(0.5);

        let horizontal_non_content = non_content_pt(0.0);
        let requested = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(100.0),
            horizontal_non_content,
        );
        let width = resolve_normal_flow_block_inline_geometry(
            &mut style,
            PageInlineSpan::from_edges(0.0, 100.0),
            PhysicalContentWidth::new(requested),
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        assert_eq!(requested.points(), 50.0);
        assert_eq!(width.content_width.points(), 50.0);
        assert_eq!(width.border_box_width().points(), 50.0);
        assert_eq!(width.border_box_inline_span.left_x(), -20.0);
    }

    #[test]
    fn normal_flow_rtl_fixed_width_anchors_from_right_side() {
        let mut style =
            style_with_horizontal_margins(length_auto(15.0), length_auto(20.0), 15.0, 20.0);
        style.box_values.width = length_auto(80.0);

        let horizontal_non_content = non_content_pt(0.0);
        let requested = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(100.0),
            horizontal_non_content,
        );
        let width = resolve_normal_flow_block_inline_geometry(
            &mut style,
            PageInlineSpan::from_edges(0.0, 100.0),
            PhysicalContentWidth::new(requested),
            horizontal_non_content,
            Direction::Rtl,
            true,
        );

        assert_eq!(requested.points(), 80.0);
        assert_eq!(width.content_width.points(), 80.0);
        assert_eq!(width.border_box_width().points(), 80.0);
        assert_eq!(width.border_box_inline_span.left_x(), 0.0);
    }

    #[test]
    fn normal_flow_block_width_keeps_content_and_border_box_types_distinct() {
        let mut style = style_with_horizontal_margins(length_auto(0.0), length_auto(0.0), 0.0, 0.0);
        style.box_values.width = length_auto(150.0);
        let horizontal_non_content = non_content_pt(20.0);
        let requested = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(300.0),
            horizontal_non_content,
        );
        let width = resolve_normal_flow_block_inline_geometry(
            &mut style,
            PageInlineSpan::from_edges(0.0, 300.0),
            PhysicalContentWidth::new(requested),
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        let _content: crate::units::ContentBoxLength = width.content_width;
        let _border: crate::units::BorderBoxLength = width.border_box_width();
        assert_eq!(width.content_width.points(), 150.0);
        assert_eq!(width.border_box_width().points(), 170.0);
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
        let requested = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(200.0),
            horizontal_non_content,
        );

        let width = resolve_normal_flow_block_inline_geometry(
            &mut style,
            PageInlineSpan::from_edges(0.0, 200.0),
            PhysicalContentWidth::new(requested),
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        assert_eq!(width.border_box_width().points(), 170.0);
        assert_eq!(style.margin.left, 15.0);
        assert_eq!(style.margin.right, 15.0);
    }

    #[test]
    fn intrinsic_fit_content_min_width_clamps_tentative_content_width() {
        let mut style = ComputedStyle::initial();
        style.box_values.min_width = fit_content(100.0);

        assert_eq!(intrinsic_width_constraint_result(&style, 10.0), 100.0);
    }

    #[test]
    fn intrinsic_fit_content_max_width_clamps_tentative_content_width() {
        let mut style = ComputedStyle::initial();
        style.box_values.max_width = fit_content(100.0);

        assert_eq!(intrinsic_width_constraint_result(&style, 150.0), 100.0);
    }

    #[test]
    fn intrinsic_min_and_max_content_constraints_clamp_content_width() {
        let mut min_style = ComputedStyle::initial();
        min_style.box_values.min_width = css::ComputedLengthPercentageOrAuto::MinContent;
        let mut max_style = ComputedStyle::initial();
        max_style.box_values.max_width = css::ComputedLengthPercentageOrAuto::MaxContent;

        assert_eq!(intrinsic_width_constraint_result(&min_style, 10.0), 60.0);
        assert_eq!(intrinsic_width_constraint_result(&max_style, 150.0), 120.0);
    }

    #[test]
    fn intrinsic_width_constraints_convert_border_box_limits_to_content_box() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.min_width = fit_content(100.0);

        let constrained = constrain_width_with_intrinsic(
            &style,
            content_box_pt(10.0),
            content_box_pt(60.0),
            content_box_pt(120.0),
            PercentageBasis::definite(content_box_pt(300.0)),
            non_content_pt(20.0),
        );

        assert_eq!(constrained.points(), 80.0);
    }

    #[test]
    fn non_replaced_intrinsic_width_uses_fit_content_length_preferred_size() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = fit_content(100.0);

        let contributions = non_replaced_intrinsic_width_contributions(
            &style,
            content_box_pt(60.0),
            content_box_pt(120.0),
            non_content_pt(0.0),
        );

        assert_eq!(contributions.0.points(), 100.0);
        assert_eq!(contributions.1.points(), 100.0);
    }

    #[test]
    fn non_replaced_intrinsic_width_treats_fit_content_percentage_as_auto() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = fit_content_percent(0.5);

        let contributions = non_replaced_intrinsic_width_contributions(
            &style,
            content_box_pt(100.0),
            content_box_pt(200.0),
            non_content_pt(0.0),
        );

        assert_eq!(contributions.0.points(), 100.0);
        assert_eq!(contributions.1.points(), 200.0);
    }

    #[test]
    fn non_replaced_intrinsic_width_converts_border_box_preferred_size() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.width = length_auto(100.0);

        let contributions = non_replaced_intrinsic_width_contributions(
            &style,
            content_box_pt(60.0),
            content_box_pt(120.0),
            non_content_pt(20.0),
        );

        assert_eq!(contributions.0.points(), 80.0);
        assert_eq!(contributions.1.points(), 80.0);
    }

    #[test]
    fn non_replaced_intrinsic_width_preserves_min_and_cyclic_max_constraints() {
        let mut style = ComputedStyle::initial();
        style.box_values.min_width = percent_auto(0.5);
        style.box_values.max_width = fit_content_percent(0.5);

        let contributions = non_replaced_intrinsic_width_contributions(
            &style,
            content_box_pt(20.0),
            content_box_pt(120.0),
            non_content_pt(0.0),
        );

        assert_eq!(contributions.0.points(), 20.0);
        assert_eq!(contributions.1.points(), 120.0);
    }

    #[test]
    fn both_auto_margins_keep_start_side_zero_when_ltr_block_overflows() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::Auto,
            0.0,
            0.0,
        );

        resolve_normal_flow_auto_margins_for_known_width(
            &mut style,
            PageInlineSpan::new(0.0, 100.0),
            border_box_pt(200.0),
            Direction::Ltr,
        );

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

        resolve_normal_flow_auto_margins_for_known_width(
            &mut style,
            PageInlineSpan::new(0.0, 100.0),
            border_box_pt(200.0),
            Direction::Ltr,
        );

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

        resolve_normal_flow_auto_margins_for_known_width(
            &mut style,
            PageInlineSpan::new(0.0, 100.0),
            border_box_pt(200.0),
            Direction::Ltr,
        );

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

        resolve_normal_flow_auto_margins_for_known_width(
            &mut style,
            PageInlineSpan::new(0.0, 100.0),
            border_box_pt(200.0),
            Direction::Rtl,
        );

        assert_eq!(style.margin.left, -100.0);
        assert_eq!(style.margin.right, 0.0);
    }

    #[test]
    fn used_lengths_resolve_percentage_against_basis() {
        let value = css::ComputedLengthPercentage::from_affine(layout_pt(12.0), 0.25, true);
        let used: LayoutLength =
            used_length_percentage(value.clone(), PercentageBasis::definite(layout_pt(200.0)));
        assert_eq!(used.points(), 62.0);
        assert_eq!(
            used_length_percentage_with_basis(
                value.clone(),
                PercentageBasis::definite(content_box_pt(200.0)),
            )
            .map(layout_points),
            Some(62.0)
        );
        assert_eq!(
            used_length_percentage_with_basis(
                value,
                PercentageBasis::<ContentBoxLength>::indefinite(),
            ),
            None
        );
    }

    #[test]
    fn used_length_or_auto_keeps_fixed_lengths_under_an_indefinite_basis() {
        let fixed: LayoutLength = used_length_percentage_or_auto(
            length_auto(12.0),
            PercentageBasis::<ContentBoxLength>::indefinite(),
        )
        .expect("fixed lengths resolve without a percentage basis");
        assert_eq!(fixed.points(), 12.0);

        assert_eq!(
            used_length_percentage_or_auto(
                percent_auto(0.5),
                PercentageBasis::<ContentBoxLength>::indefinite(),
            ),
            None,
        );

        let percentage: LayoutLength = used_length_percentage_or_auto(
            percent_auto(0.5),
            PercentageBasis::definite(content_box_pt(200.0)),
        )
        .expect("a definite basis resolves percentages");
        assert_eq!(percentage.points(), 100.0);
    }

    #[test]
    fn unresolved_metric_expression_is_not_silently_treated_as_zero() {
        let unresolved = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::sum(
                css::ComputedLengthPercentage::from_points(12.0),
                css::ComputedLengthPercentage::from_em(1.0),
            ),
        );

        assert_eq!(
            used_length_percentage_or_auto(
                unresolved.clone(),
                PercentageBasis::<ContentBoxLength>::indefinite(),
            ),
            None,
        );
        assert_eq!(
            used_content_box_size_with_basis(
                unresolved,
                BoxSizing::ContentBox,
                PercentageBasis::<ContentBoxLength>::indefinite(),
                non_content_pt(0.0),
            ),
            None,
        );
    }

    #[test]
    fn typed_constraint_entry_points_preserve_content_box_lengths() {
        let style = ComputedStyle::initial();
        let width: ContentBoxLength = constrain_content_width(
            &style,
            content_box_pt(42.0),
            PercentageBasis::definite(layout_pt(100.0)),
        );
        let height: ContentBoxLength = constrain_content_height(
            &style,
            content_box_pt(24.0),
            PercentageBasis::definite(layout_pt(100.0)),
        );

        assert_eq!(width.points(), 42.0);
        assert_eq!(height.points(), 24.0);
    }

    #[test]
    fn gap_resolvers_preserve_typed_fixed_components_when_basis_is_indefinite() {
        let mixed = css::ComputedLengthPercentage::from_affine(layout_pt(4.0), 0.5, true);
        let gap = css::ComputedGap::LengthPercentage(mixed);

        let flex_gap: LayoutLength = used_flex_gap(
            gap.clone(),
            PercentageBasis::<ContentBoxLength>::indefinite(),
        );
        let multicol_gap: LayoutLength =
            used_multicol_column_gap(gap, PercentageBasis::<ContentBoxLength>::indefinite(), 16.0);

        assert_eq!(flex_gap.points(), 4.0);
        assert_eq!(multicol_gap.points(), 4.0);
    }

    #[test]
    fn used_padding_edge_resolves_zero_percent_calc_against_zero_basis() {
        let calc_zero_percent =
            css::ComputedLengthPercentage::from_affine(layout_pt(50.0), 0.0, true);

        assert_eq!(
            used_padding_edge(
                calc_zero_percent,
                0.0,
                PercentageBasis::definite(layout_pt(0.0))
            ),
            layout_pt(50.0)
        );
        assert_eq!(
            used_padding_edge(
                css::ComputedLengthPercentage::from_points(7.0),
                7.0,
                PercentageBasis::definite(layout_pt(0.0))
            ),
            layout_pt(7.0)
        );
        assert_eq!(
            used_padding_edge(
                css::ComputedLengthPercentage::from_percent(0.25),
                0.0,
                PercentageBasis::definite(layout_pt(80.0))
            ),
            layout_pt(20.0)
        );
    }

    #[test]
    fn used_margin_edge_resolves_zero_percent_calc_against_zero_basis() {
        let calc_zero_percent = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_affine(layout_pt(30.0), 0.0, true),
        );

        assert_eq!(
            used_margin_edge(
                calc_zero_percent,
                0.0,
                PercentageBasis::definite(layout_pt(0.0))
            ),
            layout_pt(30.0)
        );
        assert_eq!(
            used_margin_edge(
                length_auto(9.0),
                9.0,
                PercentageBasis::definite(layout_pt(0.0))
            ),
            layout_pt(9.0)
        );
        assert_eq!(
            used_margin_edge(
                percent_auto(0.25),
                0.0,
                PercentageBasis::definite(layout_pt(80.0))
            ),
            layout_pt(20.0)
        );
        assert_eq!(
            used_margin_edge(
                css::ComputedLengthPercentageOrAuto::Auto,
                42.0,
                PercentageBasis::definite(layout_pt(80.0))
            ),
            layout_pt(0.0)
        );
    }

    #[test]
    fn intrinsic_margin_edges_resolve_cyclic_percentages_against_zero() {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_affine(layout_pt(100.0), 0.10, true),
        );
        style.box_values.margin.right = percent_auto(0.25);
        style.box_values.margin.top = css::ComputedLengthPercentageOrAuto::Auto;
        style.margin.left = 999.0;
        style.margin.right = 999.0;

        let margin = intrinsic_margin_edges(&style).to_css_edges();

        assert_eq!(margin.left, 100.0);
        assert_eq!(margin.right, 0.0);
        assert_eq!(margin.top, 0.0);
    }

    #[test]
    fn intrinsic_padding_edges_resolve_cyclic_percentages_against_zero() {
        let mut style = ComputedStyle::initial();
        style.box_values.padding.left =
            css::ComputedLengthPercentage::from_affine(layout_pt(50.0), 0.20, true);
        style.box_values.padding.right = css::ComputedLengthPercentage::from_percent(0.25);
        style.box_values.padding.top = css::ComputedLengthPercentage::from_points(-5.0);
        style.padding.left = 999.0;

        let padding = intrinsic_padding_edges(&style).to_css_edges();

        assert_eq!(padding.left, 50.0);
        assert_eq!(padding.right, 0.0);
        assert_eq!(padding.top, 0.0);
    }

    #[test]
    fn intrinsic_box_metrics_include_zero_basis_edges_and_borders() {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_affine(layout_pt(10.0), 0.25, true),
        );
        style.box_values.padding.left =
            css::ComputedLengthPercentage::from_affine(layout_pt(20.0), 0.25, true);
        style.border_width_values.left = css::ComputedLengthPercentage::from_points(3.0);
        style.border_width_values.right = css::ComputedLengthPercentage::from_points(4.0);
        style.border_widths.left = 3.0;
        style.border_widths.right = 4.0;
        style.border_styles.left = css::BorderStyle::Solid;
        style.border_styles.right = css::BorderStyle::Solid;

        let metrics = intrinsic_box_metrics(&style);

        assert_eq!(metrics.margin.left, layout_pt(10.0));
        assert_eq!(metrics.padding.left, layout_pt(20.0));
        assert_eq!(metrics.border.left, layout_pt(3.0));
        assert_eq!(metrics.border.right, layout_pt(4.0));
        assert_eq!(
            metrics.horizontal_non_content_length(),
            non_content_pt(27.0)
        );
    }

    #[test]
    fn applying_used_box_metrics_updates_style_only_at_the_css_edge_boundary() {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = percent_auto(0.25);
        style.box_values.padding.right = css::ComputedLengthPercentage::from_percent(0.5);
        style.border_width_values.top = css::ComputedLengthPercentage::from_points(3.0);
        style.border_widths.top = 3.0;
        style.border_styles.top = css::BorderStyle::Solid;

        let metrics =
            apply_used_box_metrics(&mut style, PercentageBasis::definite(layout_pt(80.0)));

        assert_eq!(metrics.margin.left, layout_pt(20.0));
        assert_eq!(metrics.padding.right, layout_pt(40.0));
        assert_eq!(metrics.border.top, layout_pt(3.0));
        assert_eq!(style.margin.left, 20.0);
        assert_eq!(style.padding.right, 40.0);
    }

    #[test]
    fn mutating_used_width_replaces_typed_percentage_with_used_length() {
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

    #[test]
    fn multicol_count_can_derive_from_computed_column_width() {
        let mut style = ComputedStyle::initial();
        style.column_width =
            css::ComputedColumnWidth::Length(css::ComputedLengthPercentage::from_points(40.0));

        assert_eq!(used_multicol_column_count(&style, 150.0, 10.0), Some(3));

        style.column_count = Some(2);
        assert_eq!(used_multicol_column_count(&style, 150.0, 10.0), Some(2));
        assert_eq!(used_multicol_column_count(&style, 40.0, 10.0), Some(1));

        style.column_width = css::ComputedColumnWidth::Auto;
        assert_eq!(used_multicol_column_count(&style, 1.0, 10.0), Some(2));
    }

    #[test]
    fn size_containment_preserves_authored_multicol_intrinsic_width() {
        let mut style = ComputedStyle::initial();
        style.contain.size = true;
        style.column_count = Some(3);
        style.column_width =
            css::ComputedColumnWidth::Length(css::ComputedLengthPercentage::from_points(20.0));
        style.column_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0));

        assert_eq!(
            size_contained_multicol_intrinsic_inline_sizes(&style),
            Some((70.0, 70.0))
        );
    }

    #[test]
    fn size_contained_auto_width_multicol_preserves_authored_gaps() {
        let mut style = ComputedStyle::initial();
        style.contain.size = true;
        style.column_count = Some(3);
        style.column_width = css::ComputedColumnWidth::Auto;
        style.font_size = 12.0;
        style.column_gap = css::ComputedGap::Normal;

        assert_eq!(
            size_contained_multicol_intrinsic_inline_sizes(&style),
            Some((24.0, 24.0))
        );
    }
}
