use super::*;

mod tests {
    use super::*;

    #[test]
    fn measured_baseline_selection_uses_order_modified_flex_line_order() {
        let line = FlexLineLayout {
            item_indices: vec![2, 5],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 2,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(0.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(0.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };

        assert_eq!(
            flex_line_baseline_item_index(
                &line,
                FlexDirection::ColumnReverse,
                FlexBaselineSet::First,
            ),
            Some(2),
        );
        assert_eq!(
            flex_line_baseline_item_index(
                &line,
                FlexDirection::ColumnReverse,
                FlexBaselineSet::Last,
            ),
            Some(5),
        );
    }

    #[test]
    fn wrap_reverse_reverses_the_singleton_baseline_edge() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Row;
        style.flex_wrap = FlexWrap::WrapReverse;

        assert_eq!(
            flex_baseline_alignment_side(&style, FlexBaselineSet::First),
            PhysicalSide::Bottom,
        );
        assert_eq!(
            flex_baseline_alignment_side(&style, FlexBaselineSet::Last),
            PhysicalSide::Top,
        );
        assert_eq!(
            flex_baseline_sharing_group_alignment_side(&style, FlexBaselineSet::First, 1),
            PhysicalSide::Bottom,
        );
        assert_eq!(
            flex_baseline_sharing_group_alignment_side(&style, FlexBaselineSet::Last, 1),
            PhysicalSide::Top,
        );
        assert_eq!(
            flex_baseline_sharing_group_alignment_side(&style, FlexBaselineSet::First, 2),
            PhysicalSide::Top,
        );
        assert_eq!(
            flex_baseline_sharing_group_alignment_side(&style, FlexBaselineSet::Last, 2),
            PhysicalSide::Bottom,
        );
    }

    #[test]
    fn baseline_resolution_keeps_sharing_fallback_and_auto_margin_distinct() {
        let mut measured_style = ComputedStyle::initial();
        measured_style.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Baseline);
        let mut orthogonal_style = measured_style.clone();
        orthogonal_style.writing_mode = WritingMode::VerticalRl;
        let mut auto_margin_style = measured_style.clone();
        auto_margin_style.box_values.margin.top = css::ComputedLengthPercentageOrAuto::Auto;
        let children = vec![
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: measured_style,
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: orthogonal_style,
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: auto_margin_style,
            },
        ];
        let mut measured = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        measured.baselines.vertical.first = Some(flex_vertical_baseline_from_points(5.0));
        let estimates = vec![
            measured,
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            ),
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            ),
        ];
        let container = ComputedStyle::initial();

        let alignments =
            resolve_flex_cross_alignments(&estimates, &children, &container, FlexDirection::Row);

        assert_eq!(
            alignments[0].mode,
            FlexCrossPlacementMode::Baseline {
                set: FlexBaselineSet::First,
                source: FlexBaselineSource::Measured,
                participation: FlexBaselineParticipation::Shares,
            }
        );
        assert_eq!(
            alignments[1].mode,
            FlexCrossPlacementMode::Baseline {
                set: FlexBaselineSet::First,
                source: FlexBaselineSource::Synthesized,
                participation: FlexBaselineParticipation::Fallback,
            }
        );
        assert_eq!(alignments[2].mode, FlexCrossPlacementMode::AutoCrossMargin);
    }

    #[test]
    fn cross_alignment_resolution_preserves_subject_sides_and_safe_centering() {
        let mut self_end_style = ComputedStyle::initial();
        self_end_style.align_self = css::SelfAlignment::safe(SelfAlignmentKeyword::SelfEnd);

        let mut center_style = ComputedStyle::initial();
        center_style.align_self = css::SelfAlignment::safe(SelfAlignmentKeyword::Center);

        let mut auto_margin_style = ComputedStyle::initial();
        auto_margin_style.align_self = css::SelfAlignment::new(SelfAlignmentKeyword::Center);
        auto_margin_style.box_values.margin.top = css::ComputedLengthPercentageOrAuto::Auto;

        let children = vec![
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: self_end_style,
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: center_style,
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: auto_margin_style,
            },
        ];
        let estimates = vec![
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            );
            3
        ];
        let container = ComputedStyle::initial();
        let alignments =
            resolve_flex_cross_alignments(&estimates, &children, &container, FlexDirection::Row);

        assert_eq!(
            alignments[0].mode,
            FlexCrossPlacementMode::Side(PhysicalSide::Bottom)
        );
        assert_eq!(alignments[0].safety, AlignmentSafety::Safe);
        assert_eq!(alignments[0].self_start, PhysicalSide::Top);
        assert_eq!(alignments[0].self_end, PhysicalSide::Bottom);
        assert_eq!(alignments[1].mode, FlexCrossPlacementMode::Center);
        assert_eq!(alignments[1].safety, AlignmentSafety::Safe);
        assert_eq!(alignments[2].mode, FlexCrossPlacementMode::AutoCrossMargin);
    }

    #[test]
    fn centered_cross_placement_uses_margin_box_geometry_and_safe_start() {
        let mut child_style = ComputedStyle::initial();
        child_style.margin.top = -5.0;
        child_style.margin.bottom = 15.0;
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(100.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let mut item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(10.0, 10.0),
        ));
        let centered = ResolvedFlexCrossAlignment {
            mode: FlexCrossPlacementMode::Center,
            safety: AlignmentSafety::Default,
            flex_cross_start: PhysicalSide::Top,
            flex_cross_end: PhysicalSide::Bottom,
            self_start: PhysicalSide::Top,
            self_end: PhysicalSide::Bottom,
        };

        align_item_cross_center(&mut item, &child_style, FlexDirection::Row, &line, centered);
        // The margin box is 20px tall, so its 40px cross start is centered
        // in the 100px line slot; the border box starts 5px before it.
        assert_eq!(item.y(), FlexPhysicalVerticalOffset::new(35.0));

        let mut overflowing_line = line.clone();
        overflowing_line.cross_end = FlexCrossOffset::new(10.0);
        let mut safe_item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(10.0, 10.0),
        ));
        align_item_cross_center(
            &mut safe_item,
            &child_style,
            FlexDirection::Row,
            &overflowing_line,
            ResolvedFlexCrossAlignment {
                safety: AlignmentSafety::Safe,
                ..centered
            },
        );
        assert_eq!(safe_item.y(), FlexPhysicalVerticalOffset::new(-5.0));
    }

    #[test]
    fn final_line_slot_redistributes_auto_cross_margins_for_a_column_item() {
        let mut child_style = ComputedStyle::initial();
        child_style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::Auto;
        child_style.box_values.margin.right = css::ComputedLengthPercentageOrAuto::Auto;
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(10.0),
            cross_end: FlexCrossOffset::new(110.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let mut item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(10.0, 0.0),
            ContainerSize::new(20.0, 10.0),
        ));

        place_item_with_final_auto_cross_margins(
            &mut item,
            &child_style,
            FlexDirection::Column,
            &line,
        );

        assert_eq!(item.x(), FlexPhysicalHorizontalOffset::new(50.0));
    }

    #[test]
    fn baseline_fallback_preserves_negative_cross_start_margins() {
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(10.0),
            cross_end: FlexCrossOffset::new(50.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let row_container = ComputedStyle::initial();

        let mut row_style = ComputedStyle::initial();
        row_style.margin.top = -4.0;
        let mut row_item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 10.0),
            ContainerSize::new(10.0, 10.0),
        ));
        apply_baseline_self_alignment_fallback_offset(
            &mut row_item,
            &row_style,
            &line,
            &row_container,
            FlexBaselineSet::First,
            FlexDirection::Row,
            None,
        );
        assert_eq!(row_item.y(), FlexPhysicalVerticalOffset::new(6.0));

        let mut column_style = ComputedStyle::initial();
        column_style.margin.left = -4.0;
        let mut column_container = ComputedStyle::initial();
        column_container.flex_direction = FlexDirection::Column;
        let mut column_item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(10.0, 0.0),
            ContainerSize::new(10.0, 10.0),
        ));
        apply_baseline_self_alignment_fallback_offset(
            &mut column_item,
            &column_style,
            &line,
            &column_container,
            FlexBaselineSet::First,
            FlexDirection::Column,
            None,
        );
        assert_eq!(column_item.x(), FlexPhysicalHorizontalOffset::new(6.0));
    }

    #[test]
    fn baseline_sharing_resolves_absolute_cross_starts_from_line_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0, 1],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 2,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(20.0),
            cross_start: FlexCrossOffset::new(50.0),
            cross_end: FlexCrossOffset::new(62.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let mut first = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        first.baselines.vertical.first = Some(flex_vertical_baseline_from_points(4.0));
        let mut second = first;
        second.baselines.vertical.first = Some(flex_vertical_baseline_from_points(8.0));
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
        let mut items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 40.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(10.0, 42.0),
                ContainerSize::new(10.0, 10.0),
            )),
        ];
        let container = ComputedStyle::initial();
        let participants = vec![
            ResolvedFlexBaselineParticipant {
                index: 0,
                source: FlexBaselineSource::Measured,
                participation: FlexBaselineParticipation::Shares,
            },
            ResolvedFlexBaselineParticipant {
                index: 1,
                source: FlexBaselineSource::Measured,
                participation: FlexBaselineParticipation::Shares,
            },
        ];

        align_baseline_sharing_group_to_line(
            &mut items,
            &line,
            &participants,
            &[first, second],
            &children,
            &container,
            FlexBaselineSet::First,
            FlexDirection::Row,
        );

        assert_eq!(items[0].y(), FlexPhysicalVerticalOffset::new(54.0));
        assert_eq!(items[1].y(), FlexPhysicalVerticalOffset::new(50.0));
    }

    #[test]
    fn taffy_cross_projection_precedes_line_reconciliation_for_vertical_rtl_columns() {
        for flex_wrap in [FlexWrap::Wrap, FlexWrap::WrapReverse] {
            let mut style = ComputedStyle::initial();
            style.writing_mode = WritingMode::VerticalLr;
            style.direction = Direction::Rtl;
            style.flex_direction = FlexDirection::Column;
            style.flex_wrap = flex_wrap;
            let axes = FlexAxes::for_style(&style);
            assert_eq!(
                axes.taffy_cross_axis_projection(),
                TaffyCrossAxisProjection::Reflect
            );

            let mut items = vec![
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 0.0),
                    ContainerSize::new(20.0, 30.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(20.0, 0.0),
                    ContainerSize::new(20.0, 30.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 30.0),
                    ContainerSize::new(20.0, 30.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(20.0, 30.0),
                    ContainerSize::new(20.0, 30.0),
                )),
            ];
            reproject_taffy_item_cross_axis_coordinates(&mut items, axes, FlexCrossSize::new(60.0));

            // The physical row main axis leaves Y as the cross coordinate.
            // Taffy's first top-origin line is therefore the CSS
            // bottom-origin line, independently of wrap-reverse. This is the
            // four-item wrapped geometry that later Flex line reconciliation
            // receives.
            assert_eq!(items[0].x(), FlexPhysicalHorizontalOffset::new(0.0));
            assert_eq!(items[1].x(), FlexPhysicalHorizontalOffset::new(20.0));
            assert_eq!(
                items.iter().map(|item| item.y()).collect::<Vec<_>>(),
                vec![
                    FlexPhysicalVerticalOffset::new(30.0),
                    FlexPhysicalVerticalOffset::new(30.0),
                    FlexPhysicalVerticalOffset::new(0.0),
                    FlexPhysicalVerticalOffset::new(0.0),
                ],
            );
            assert!(
                items
                    .iter()
                    .all(|item| item.height() == FlexPhysicalVerticalSize::new(30.0))
            );
        }
    }

    #[test]
    fn physical_right_justifies_a_sideways_column_on_its_horizontal_main_axis() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::SidewaysLr;
        style.direction = Direction::Ltr;
        style.flex_direction = FlexDirection::Column;
        let axes = FlexAxes::for_style(&style);
        assert!(axes.is_main_row_axis());
        assert_eq!(axes.main_start_side(), PhysicalSide::Left);

        let right =
            taffy_justify_content(JustifyContent::new(ContentAlignmentKeyword::Right), axes);
        let left = taffy_justify_content(JustifyContent::new(ContentAlignmentKeyword::Left), axes);

        assert_eq!(right.keyword, taffy_layout::AlignContentKeyword::FlexEnd);
        assert_eq!(left.keyword, taffy_layout::AlignContentKeyword::FlexStart);
    }
}
