use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_flex_sizing_view_resolves_percentage_box_edges_once() {
        let mut child_style = ComputedStyle::initial();
        child_style.box_values.padding.top = css::ComputedLengthPercentage::from_percent(1.0);
        child_style.box_values.padding.right = css::ComputedLengthPercentage::from_percent(1.0);
        child_style.box_values.padding.bottom = css::ComputedLengthPercentage::from_percent(1.0);
        child_style.box_values.padding.left = css::ComputedLengthPercentage::from_percent(1.0);
        let children = [StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: child_style,
        }];
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(12.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(12.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: None,
            height_basis: PercentageBasis::indefinite(),
        };

        let sizing_children = flex_sizing_children_with_used_box_edges(
            &children,
            &ComputedStyle::initial(),
            available,
        );

        assert_eq!(
            sizing_children[0].style.padding,
            css::Edges {
                top: 12.0,
                right: 12.0,
                bottom: 12.0,
                left: 12.0,
            }
        );
        assert!(
            sizing_children[0]
                .style
                .box_values
                .padding
                .top
                .contains_percentage()
        );
        assert_eq!(children[0].style.padding, css::Edges::ZERO);
    }

    #[test]
    fn indefinite_column_main_size_forces_a_single_taffy_line() {
        let indefinite = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(100.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            // A fragmentainer can impose a numeric layout limit without
            // making this auto-height flex container's own main size
            // definite.
            height: Some(PhysicalContentHeight::new(content_box_pt(300.0))),
            height_basis: PercentageBasis::indefinite(),
        };
        let mut auto_height = ComputedStyle::initial();
        auto_height.flex_wrap = FlexWrap::Wrap;

        assert_eq!(
            taffy_flex_wrap(&auto_height, FlexDirection::Column, indefinite),
            taffy_layout::FlexWrap::NoWrap,
        );
        auto_height.flex_wrap = FlexWrap::WrapReverse;
        assert_eq!(
            taffy_flex_wrap(&auto_height, FlexDirection::Column, indefinite),
            taffy_layout::FlexWrap::NoWrap,
        );

        let definite = FlexAvailableSpace {
            height_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            ..indefinite
        };
        let mut definite_height = ComputedStyle::initial();
        definite_height.flex_wrap = FlexWrap::Wrap;
        *definite_height.box_values.height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(100.0),
        );
        assert_eq!(
            taffy_flex_wrap(&definite_height, FlexDirection::Column, definite),
            taffy_layout::FlexWrap::Wrap,
        );

        let ratio_derived = FlexAvailableSpace {
            height: Some(PhysicalContentHeight::new(content_box_pt(100.0))),
            height_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::AspectRatioDerived,
            ),
            ..indefinite
        };
        let mut automatic_ratio_height = ComputedStyle::initial();
        automatic_ratio_height.flex_wrap = FlexWrap::Wrap;
        assert_eq!(
            taffy_flex_wrap(
                &automatic_ratio_height,
                FlexDirection::Column,
                ratio_derived,
            ),
            taffy_layout::FlexWrap::Wrap,
        );
    }

    #[test]
    fn wrap_reverse_flips_the_physical_flex_cross_start_side() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Row;
        style.flex_wrap = FlexWrap::Wrap;
        assert_eq!(flex_cross_start_side(&style), PhysicalSide::Top);
        assert_eq!(flex_cross_end_side(&style), PhysicalSide::Bottom);

        style.flex_wrap = FlexWrap::WrapReverse;
        assert_eq!(flex_cross_start_side(&style), PhysicalSide::Bottom);
        assert_eq!(flex_cross_end_side(&style), PhysicalSide::Top);
        assert_eq!(flex_unreversed_cross_start_side(&style), PhysicalSide::Top);
    }

    #[test]
    fn auto_cross_margin_does_not_make_an_auto_row_item_percentage_definite() {
        let mut child_style = ComputedStyle::initial();
        child_style.box_values.margin.top = css::ComputedLengthPercentageOrAuto::Auto;
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: child_style,
        };
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(20.0, 10.0),
        ));
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(100.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: None,
            height_basis: PercentageBasis::indefinite(),
        };

        assert_eq!(
            flex_item_final_percentage_height_basis(
                &item,
                FlexItemEstimate::fixed(
                    PhysicalContentWidth::new(content_box_pt(20.0)),
                    PhysicalContentHeight::new(content_box_pt(10.0)),
                ),
                &child,
                &ComputedStyle::initial(),
                FlexDirection::Row,
                available,
            ),
            PercentageBasis::indefinite(),
        );
    }

    #[test]
    fn auto_row_line_cross_span_does_not_make_percentage_height_definite() {
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
        };
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(20.0, 10.0),
        ));
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(100.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: None,
            height_basis: PercentageBasis::indefinite(),
        };

        assert_eq!(
            flex_item_final_percentage_height_basis(
                &item,
                FlexItemEstimate::fixed(
                    PhysicalContentWidth::new(content_box_pt(20.0)),
                    PhysicalContentHeight::new(content_box_pt(10.0)),
                ),
                &child,
                &ComputedStyle::initial(),
                FlexDirection::Row,
                available,
            ),
            PercentageBasis::indefinite(),
        );
    }

    #[test]
    fn aspect_ratio_transferred_column_basis_makes_descendant_height_definite() {
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
        };
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(100.0, 100.0),
        ));
        let mut estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(100.0)),
            PhysicalContentHeight::new(content_box_pt(100.0)),
        );
        estimate.set_main_size_provenance(FlexMainSizeProvenance::AspectRatioTransfer);
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(100.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: None,
            height_basis: PercentageBasis::indefinite(),
        };

        let basis = flex_item_final_percentage_height_basis(
            &item,
            estimate,
            &child,
            &ComputedStyle::initial(),
            FlexDirection::Column,
            available,
        );

        assert!(basis.is_definite());
        assert_eq!(basis.points(), Some(100.0));
    }

    fn definite_height_style() -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.box_values.height.replace_with_used(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(40.0),
            ),
        );
        style
    }

    #[test]
    fn column_final_block_span_never_replaces_the_flexed_main_size() {
        let mut style = definite_height_style();
        style.flex_basis = css::ComputedFlexBasis::Content;

        assert!(
            !final_normal_flow_block_span_replaces_provisional_height_with_cross_definiteness(
                &style,
                FlexDirection::Column,
                FlexMainSizeProvenance::NormalFlowContent,
                false,
            )
        );
    }

    #[test]
    fn column_final_block_span_does_not_replace_any_main_size_provenance() {
        let style = definite_height_style();

        for provenance in [
            FlexMainSizeProvenance::NormalFlowContent,
            FlexMainSizeProvenance::AspectRatioTransfer,
            FlexMainSizeProvenance::MainSizeProperty,
            FlexMainSizeProvenance::DefiniteFlexBasis,
        ] {
            assert!(
                !final_normal_flow_block_span_replaces_provisional_height_with_cross_definiteness(
                    &style,
                    FlexDirection::Column,
                    provenance,
                    false,
                )
            );
        }
    }

    #[test]
    fn row_content_basis_does_not_replace_definite_cross_height() {
        let mut style = definite_height_style();
        style.flex_basis = css::ComputedFlexBasis::Content;

        assert!(
            !final_normal_flow_block_span_replaces_provisional_height_with_cross_definiteness(
                &style,
                FlexDirection::Row,
                FlexMainSizeProvenance::NormalFlowContent,
                false,
            )
        );
    }

    #[test]
    fn row_content_basis_uses_final_block_span_for_automatic_cross_size() {
        let mut style = ComputedStyle::initial();
        style.flex_basis = css::ComputedFlexBasis::Content;

        assert!(
            final_normal_flow_block_span_replaces_provisional_height_with_cross_definiteness(
                &style,
                FlexDirection::Row,
                FlexMainSizeProvenance::NormalFlowContent,
                false,
            )
        );
    }

    #[test]
    fn definite_line_cross_size_keeps_percentage_cross_constraints_for_final_replay() {
        let mut style = ComputedStyle::initial();
        style.flex_basis = css::ComputedFlexBasis::Content;
        style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(1.0),
        );

        assert!(
            final_normal_flow_block_span_replaces_provisional_height_with_cross_definiteness(
                &style,
                FlexDirection::Row,
                FlexMainSizeProvenance::NormalFlowContent,
                false,
            )
        );
        assert!(
            !final_normal_flow_block_span_replaces_provisional_height_with_cross_definiteness(
                &style,
                FlexDirection::Row,
                FlexMainSizeProvenance::NormalFlowContent,
                true,
            )
        );
    }

    #[test]
    fn row_final_block_span_does_not_replace_aspect_ratio_transfer() {
        let style = ComputedStyle::initial();

        assert!(
            !final_normal_flow_block_span_replaces_provisional_height_with_cross_definiteness(
                &style,
                FlexDirection::Row,
                FlexMainSizeProvenance::AspectRatioTransfer,
                false,
            )
        );
    }

    #[test]
    fn column_content_basis_measurement_restores_auto_height_and_source_bounds() {
        let mut source_style = definite_height_style();
        source_style.flex_basis = css::ComputedFlexBasis::Content;
        source_style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(12.0),
        );
        source_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(48.0),
        );
        let mut placed_style = definite_height_style();
        placed_style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(24.0),
        );
        placed_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(24.0),
        );

        let mode = FinalNormalFlowMeasurementMode::for_item(
            &source_style,
            FlexDirection::Column,
            FlexMainSizeProvenance::NormalFlowContent,
            false,
            false,
        );
        mode.prepare_placed_style(&mut placed_style, &source_style);

        assert_eq!(mode, FinalNormalFlowMeasurementMode::ColumnContentMainSize);
        assert!(placed_style.box_values.height.is_auto());
        assert_eq!(
            placed_style.box_values.min_height,
            source_style.box_values.min_height
        );
        assert_eq!(
            placed_style.box_values.max_height,
            source_style.box_values.max_height
        );
    }

    #[test]
    fn row_automatic_cross_measurement_keeps_frozen_height_bounds() {
        let mut source_style = ComputedStyle::initial();
        source_style.flex_basis = css::ComputedFlexBasis::Content;
        let mut placed_style = definite_height_style();
        let frozen_min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(24.0),
        );
        placed_style.box_values.min_height = frozen_min_height.clone();
        placed_style.box_values.max_height = frozen_min_height.clone();

        let mode = FinalNormalFlowMeasurementMode::for_item(
            &source_style,
            FlexDirection::Row,
            FlexMainSizeProvenance::NormalFlowContent,
            false,
            false,
        );
        mode.prepare_placed_style(&mut placed_style, &source_style);

        assert_eq!(mode, FinalNormalFlowMeasurementMode::RowAutomaticCrossSize);
        assert!(placed_style.box_values.height.is_auto());
        assert_eq!(placed_style.box_values.min_height, frozen_min_height);
        assert_eq!(placed_style.box_values.max_height, frozen_min_height);
    }

    #[test]
    fn fixed_main_size_provenance_uses_replayed_geometry_measurement() {
        let style = definite_height_style();

        for provenance in [
            FlexMainSizeProvenance::AspectRatioTransfer,
            FlexMainSizeProvenance::MainSizeProperty,
            FlexMainSizeProvenance::DefiniteFlexBasis,
        ] {
            assert_eq!(
                FinalNormalFlowMeasurementMode::for_item(
                    &style,
                    FlexDirection::Column,
                    provenance,
                    false,
                    false,
                ),
                FinalNormalFlowMeasurementMode::ReplayedUsedGeometry,
            );
        }
    }

    #[test]
    fn replaced_content_basis_uses_replayed_geometry_measurement() {
        let mut style = ComputedStyle::initial();
        style.flex_basis = css::ComputedFlexBasis::Content;

        assert_eq!(
            FinalNormalFlowMeasurementMode::for_item(
                &style,
                FlexDirection::Column,
                FlexMainSizeProvenance::NormalFlowContent,
                true,
                false,
            ),
            FinalNormalFlowMeasurementMode::ReplayedUsedGeometry,
        );
    }

    #[test]
    fn final_normal_flow_span_converts_replay_border_box_to_content_box() {
        let mut style = ComputedStyle::initial();
        style.padding.top = 2.0;
        style.padding.bottom = 3.0;
        style.border_widths.top = 1.0;
        style.border_widths.bottom = 4.0;
        style.border_styles.top = css::BorderStyle::Solid;
        style.border_styles.bottom = css::BorderStyle::Solid;

        let span = final_normal_flow_content_block_span(border_box_pt(24.0), &style);

        assert_eq!(span.points(), 14.0);
    }
}
