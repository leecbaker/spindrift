use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_line_remeasurement_preserves_flex_resolved_main_size() {
        let mut item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(40.0, 50.0),
        ));
        let estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(100.0)),
            PhysicalContentHeight::new(content_box_pt(50.0)),
        );
        let axes = PhysicalFlexDirection::new(FlexDirection::Row);

        let cross_size_changed = FlexPostLineRemeasurement::new(estimate, FlexCrossSize::new(50.0))
            .apply_to_layout(&mut item, axes);

        assert!(!cross_size_changed);
        assert_eq!(item.main_size(axes), FlexMainSize::new(40.0));
        assert_eq!(item.cross_size(axes), FlexCrossSize::new(50.0));
    }

    #[test]
    fn wrapped_column_fit_content_keeps_a_non_stretched_item_within_container_cross_size() {
        let child_style = ComputedStyle::initial();
        let resolution = FlexItemLineCrossSizeResolution::for_item(
            &child_style,
            FlexDirection::Column,
            // Model the provisional max-content line Taffy can expose before
            // Flex reconciliation. The definite container cross size remains
            // the fit-content constraint.
            FlexCrossSize::new(300.0),
            FlexCrossSizingPhase::Hypothetical,
            Some(FlexCrossSize::new(100.0)),
        );

        let border_size = resolution
            .wrapped_column_fit_content_border_size(content_box_pt(100.0), content_box_pt(300.0));

        assert_eq!(border_size, border_box_pt(100.0));
        assert_eq!(
            resolution.used_content_size(border_size),
            content_box_pt(100.0)
        );
    }

    #[test]
    fn wrapped_column_fit_content_respects_cross_axis_box_model_extras() {
        let mut child_style = ComputedStyle::initial();
        child_style.margin.left = 5.0;
        child_style.margin.right = 5.0;
        child_style.padding.left = 10.0;
        child_style.padding.right = 10.0;
        child_style.border_widths.left = 2.0;
        child_style.border_widths.right = 2.0;
        let resolution = FlexItemLineCrossSizeResolution::for_item(
            &child_style,
            FlexDirection::Column,
            FlexCrossSize::new(200.0),
            FlexCrossSizingPhase::Hypothetical,
            Some(FlexCrossSize::new(100.0)),
        );

        let border_size = resolution
            .wrapped_column_fit_content_border_size(content_box_pt(20.0), content_box_pt(300.0));

        // 100px container slot - 10px margins - 20px padding. The initial
        // style keeps its borders `none`, so its configured border widths do
        // not contribute used box-model space.
        assert_eq!(border_size, border_box_pt(90.0));
        assert_eq!(
            resolution.used_content_size(border_size),
            content_box_pt(70.0)
        );
    }

    #[test]
    fn exported_item_baselines_use_final_border_box_origins_without_reapplying_margins() {
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(13.0, 17.0),
            ContainerSize::new(30.0, 40.0),
        ));
        let mut estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(30.0)),
            PhysicalContentHeight::new(content_box_pt(40.0)),
        );
        estimate.baselines.vertical.first = Some(flex_vertical_baseline_from_points(7.0));
        estimate.baselines.horizontal.first = Some(flex_horizontal_baseline_from_points(11.0));
        let mut child_style = ComputedStyle::initial();
        child_style.margin.top = 19.0;
        child_style.margin.left = 23.0;
        let container_style = ComputedStyle::initial();

        // The final item rect has already incorporated these margins. Both
        // cross-axis sharing and container export must therefore use only its
        // border-box origin plus the measured border-box baseline.
        assert_eq!(
            measured_item_cross_axis_baseline(
                &item,
                &estimate,
                &child_style,
                &container_style,
                FlexBaselineSet::First,
                FlexDirection::Row,
            ),
            FlexCrossOffset::new(24.0),
        );
        assert_eq!(
            measured_item_cross_axis_baseline(
                &item,
                &estimate,
                &child_style,
                &container_style,
                FlexBaselineSet::First,
                FlexDirection::Column,
            ),
            FlexCrossOffset::new(24.0),
        );

        let children = vec![StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: child_style,
        }];
        let items = vec![item];
        let estimates = vec![estimate];
        assert_eq!(
            flex_item_baseline_for_container_axis(
                0,
                &items,
                &estimates,
                &children,
                &container_style,
                FlexBaselineSet::First,
                PhysicalAxis::Horizontal,
            ),
            FlexPhysicalBaselineOffset::Vertical(flex_vertical_baseline_from_points(24.0)),
        );
        assert_eq!(
            flex_item_baseline_for_container_axis(
                0,
                &items,
                &estimates,
                &children,
                &container_style,
                FlexBaselineSet::First,
                PhysicalAxis::Vertical,
            ),
            FlexPhysicalBaselineOffset::Horizontal(flex_horizontal_baseline_from_points(24.0)),
        );
    }

    #[test]
    fn horizontal_column_synthesizes_a_vertical_inline_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(10.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(10.0, 10.0),
        ));
        let estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
        };

        let mut container = ComputedStyle::initial();
        container.flex_direction = FlexDirection::Column;
        assert_eq!(
            flex_container_baselines(
                &[line],
                &[item],
                &[estimate],
                &[child],
                &container,
                FlexDirection::Column,
            )
            .vertical
            .first,
            Some(flex_vertical_baseline_from_points(10.0)),
        );
    }

    #[test]
    fn horizontal_column_exports_its_first_item_main_axis_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(10.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 3.0),
            ContainerSize::new(10.0, 10.0),
        ));
        let mut estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        estimate.baselines.vertical.first = Some(flex_vertical_baseline_from_points(5.0));
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
        };
        let mut container = ComputedStyle::initial();
        container.flex_direction = FlexDirection::Column;
        assert_eq!(
            flex_container_baselines(
                &[line],
                &[item],
                &[estimate],
                &[child],
                &container,
                FlexDirection::Column,
            )
            .vertical
            .first,
            Some(flex_vertical_baseline_from_points(8.0)),
        );
    }

    #[test]
    fn horizontal_column_synthesizes_a_vertical_export_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(10.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 3.0),
            ContainerSize::new(10.0, 10.0),
        ));
        let estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
        };
        let mut container = ComputedStyle::initial();
        container.flex_direction = FlexDirection::Column;
        assert_eq!(
            flex_container_baselines(
                &[line],
                &[item],
                &[estimate],
                &[child],
                &container,
                FlexDirection::Column,
            )
            .vertical
            .first,
            Some(flex_vertical_baseline_from_points(13.0)),
        );
    }

    #[test]
    fn horizontal_column_reverse_uses_the_final_main_start_item() {
        let line = FlexLineLayout {
            item_indices: vec![0, 1],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 2,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(20.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(10.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 3.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 13.0),
                ContainerSize::new(10.0, 10.0),
            )),
        ];
        let mut first = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        first.baselines.vertical.first = Some(flex_vertical_baseline_from_points(2.0));
        let mut second = first;
        second.baselines.vertical.first = Some(flex_vertical_baseline_from_points(5.0));
        let children = vec![
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            },
        ];
        let mut container = ComputedStyle::initial();
        container.flex_direction = FlexDirection::ColumnReverse;

        assert_eq!(
            flex_container_baselines(
                &[line],
                &items,
                &[first, second],
                &children,
                &container,
                FlexDirection::ColumnReverse,
            )
            .vertical
            .first,
            Some(flex_vertical_baseline_from_points(18.0)),
        );
    }

    #[test]
    fn row_export_falls_back_to_the_first_item_when_first_line_has_no_set() {
        let lines = vec![
            FlexLineLayout {
                item_indices: vec![0, 1],
                logical_cross_start_rank: 0,
                source_start: 0,
                source_end: 2,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(20.0),
                cross_start: FlexCrossOffset::new(0.0),
                cross_end: FlexCrossOffset::new(10.0),
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
            FlexLineLayout {
                item_indices: vec![2, 3],
                logical_cross_start_rank: 1,
                source_start: 2,
                source_end: 4,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(20.0),
                cross_start: FlexCrossOffset::new(10.0),
                cross_end: FlexCrossOffset::new(20.0),
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
        ];
        let items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(10.0, 0.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 10.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(10.0, 10.0),
                ContainerSize::new(10.0, 10.0),
            )),
        ];
        let mut estimates = vec![
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            );
            4
        ];
        estimates[0].baselines.vertical.first = Some(flex_vertical_baseline_from_points(4.0));
        estimates[3].baselines.vertical.last = Some(flex_vertical_baseline_from_points(8.0));
        let children = (0..4)
            .map(|_| StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            })
            .collect::<Vec<_>>();

        let exported = flex_container_baselines(
            &lines,
            &items,
            &estimates,
            &children,
            &ComputedStyle::initial(),
            FlexDirection::Row,
        );
        assert_eq!(
            exported.vertical.first,
            Some(flex_vertical_baseline_from_points(4.0)),
        );
        assert_eq!(
            exported.vertical.last,
            Some(flex_vertical_baseline_from_points(18.0)),
        );
    }

    #[test]
    fn wrap_reverse_export_uses_unreversed_writing_mode_edges() {
        let lines = vec![
            FlexLineLayout {
                // This is order-modified first, but `wrap-reverse` packed it
                // at the block-end side of a horizontal writing-mode box.
                item_indices: vec![2, 3],
                logical_cross_start_rank: 0,
                source_start: 2,
                source_end: 4,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(20.0),
                cross_start: FlexCrossOffset::new(10.0),
                cross_end: FlexCrossOffset::new(20.0),
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
            FlexLineLayout {
                item_indices: vec![0, 1],
                logical_cross_start_rank: 1,
                source_start: 0,
                source_end: 2,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(20.0),
                cross_start: FlexCrossOffset::new(0.0),
                cross_end: FlexCrossOffset::new(10.0),
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
        ];
        let mut style = ComputedStyle::initial();
        style.flex_wrap = FlexWrap::WrapReverse;

        let (first, last) = flex_container_baseline_lines(&lines, &style).unwrap();
        assert_eq!(first.logical_cross_start_rank, 1);
        assert_eq!(last.logical_cross_start_rank, 0);
    }

    #[test]
    fn wrap_reverse_exports_shared_baselines_from_final_startmost_and_endmost_lines() {
        let lines = [
            FlexLineLayout {
                // Order-modified first, but wrap-reverse placed this line at
                // the physical block-end edge.
                item_indices: vec![0],
                logical_cross_start_rank: 0,
                source_start: 0,
                source_end: 1,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(10.0),
                cross_start: FlexCrossOffset::new(20.0),
                cross_end: FlexCrossOffset::new(30.0),
                first_baseline: Some(FlexCrossOffset::new(24.0)),
                last_baseline: Some(FlexCrossOffset::new(26.0)),
                collapsed_struts: Vec::new(),
            },
            FlexLineLayout {
                // Order-modified last, but this is the finalized physical
                // block-start line selected for first-baseline export.
                item_indices: vec![1],
                logical_cross_start_rank: 1,
                source_start: 1,
                source_end: 2,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(10.0),
                cross_start: FlexCrossOffset::new(0.0),
                cross_end: FlexCrossOffset::new(10.0),
                first_baseline: Some(FlexCrossOffset::new(4.0)),
                last_baseline: Some(FlexCrossOffset::new(6.0)),
                collapsed_struts: Vec::new(),
            },
        ];
        let items = [
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 20.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                ContainerSize::new(10.0, 10.0),
            )),
        ];
        let estimates = vec![
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            );
            2
        ];
        let children = (0..2)
            .map(|_| StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            })
            .collect::<Vec<_>>();
        let mut style = ComputedStyle::initial();
        style.flex_wrap = FlexWrap::WrapReverse;

        let exported = flex_container_baselines(
            &lines,
            &items,
            &estimates,
            &children,
            &style,
            FlexDirection::Row,
        );
        assert_eq!(
            exported.vertical.first,
            Some(flex_vertical_baseline_from_points(4.0)),
        );
        assert_eq!(
            exported.vertical.last,
            Some(flex_vertical_baseline_from_points(26.0)),
        );
    }

    #[test]
    fn later_flex_line_sharing_group_does_not_replace_first_line_item_fallback() {
        let lines = [
            FlexLineLayout {
                item_indices: vec![0],
                logical_cross_start_rank: 0,
                source_start: 0,
                source_end: 1,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(10.0),
                cross_start: FlexCrossOffset::new(0.0),
                cross_end: FlexCrossOffset::new(10.0),
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
            FlexLineLayout {
                item_indices: vec![1],
                logical_cross_start_rank: 1,
                source_start: 1,
                source_end: 2,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(10.0),
                cross_start: FlexCrossOffset::new(10.0),
                cross_end: FlexCrossOffset::new(20.0),
                first_baseline: Some(FlexCrossOffset::new(17.0)),
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
        ];
        let mut first = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        first.baselines.vertical.first = Some(flex_vertical_baseline_from_points(4.0));
        let estimates = vec![
            first,
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            ),
        ];
        let children = (0..2)
            .map(|_| StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            flex_container_main_axis_baseline_source(
                &lines[0],
                &estimates,
                &children,
                &ComputedStyle::initial(),
                FlexBaselineSet::First,
            ),
            Some(FlexContainerMainAxisBaselineSource::Item {
                index: 0,
                baseline_set: FlexBaselineSet::First,
            }),
        );
    }

    #[test]
    fn container_export_checks_opposite_shared_baseline_before_items() {
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(20.0),
            first_baseline: None,
            last_baseline: Some(FlexCrossOffset::new(13.0)),
            collapsed_struts: Vec::new(),
        };
        let estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
        };

        assert_eq!(
            flex_container_main_axis_baseline_source(
                &line,
                &[estimate],
                &[child],
                &ComputedStyle::initial(),
                FlexBaselineSet::First,
            ),
            Some(FlexContainerMainAxisBaselineSource::Shared {
                baseline_set: FlexBaselineSet::Last,
            }),
        );
    }

    #[test]
    fn container_export_prefers_measured_item_before_synthesis() {
        let line = FlexLineLayout {
            item_indices: vec![0, 1],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 2,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(20.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(20.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let missing = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        let mut measured = missing;
        measured.baselines.vertical.last = Some(flex_vertical_baseline_from_points(7.0));
        let children = (0..2)
            .map(|_| StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            flex_container_main_axis_baseline_source(
                &line,
                &[missing, measured],
                &children,
                &ComputedStyle::initial(),
                FlexBaselineSet::First,
            ),
            Some(FlexContainerMainAxisBaselineSource::Item {
                index: 1,
                baseline_set: FlexBaselineSet::Last,
            }),
        );
        assert_eq!(
            flex_container_main_axis_baseline_source(
                &line,
                &[missing, missing],
                &children,
                &ComputedStyle::initial(),
                FlexBaselineSet::Last,
            ),
            Some(FlexContainerMainAxisBaselineSource::SynthesizedItem {
                index: 1,
                baseline_set: FlexBaselineSet::Last,
            }),
        );
    }
}
