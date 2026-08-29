use super::*;

mod tests {
    use super::*;

    #[test]
    fn safe_overflow_line_packing_uses_logical_start_after_wrap_reverse() {
        let mut style = ComputedStyle::initial();
        style.flex_wrap = FlexWrap::WrapReverse;

        assert_eq!(
            final_line_packing_start_side(&style, false),
            PhysicalSide::Bottom,
            "ordinary flex-start follows the reversed flex cross axis"
        );
        assert_eq!(
            final_line_packing_start_side(&style, true),
            PhysicalSide::Top,
            "safe overflow falls back to logical start rather than flex-start"
        );
    }

    #[test]
    fn normal_justify_content_uses_main_start_in_a_reverse_column() {
        let justify_content = JustifyContent::new(ContentAlignmentKeyword::Normal);

        assert_eq!(
            justify_content_offsets(
                justify_content,
                FlexDirection::ColumnReverse,
                FlexMainLength::new(14.0),
                2,
            )
            .initial,
            FlexMainLength::new(14.0)
        );
    }

    #[test]
    fn distributed_overflow_fallback_keeps_reverse_flex_start() {
        let justify_content = JustifyContent::new(ContentAlignmentKeyword::SpaceBetween);

        assert_eq!(
            justify_content_fallback_keyword(justify_content, FlexMainLength::new(-250.0), 1),
            ContentAlignmentKeyword::FlexStart,
        );
        assert_eq!(
            justify_content_offsets(
                justify_content,
                FlexDirection::ColumnReverse,
                FlexMainLength::new(-250.0),
                1,
            )
            .initial,
            FlexMainLength::new(-250.0),
        );
    }

    #[test]
    fn line_cross_constraint_keeps_an_indefinite_height_content_based() {
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(80.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(80.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            // A numeric fragmentainer limit is not a CSS definite height.
            height: Some(PhysicalContentHeight::new(content_box_pt(120.0))),
            height_basis: PercentageBasis::indefinite(),
        };

        assert_eq!(
            FlexLineCrossConstraint::from_container(
                &ComputedStyle::initial(),
                available,
                FlexDirection::Row,
                FlexCrossSize::new(120.0),
            ),
            FlexLineCrossConstraint::ContentBased
        );
    }

    #[test]
    fn line_cross_constraint_uses_explicit_single_line_height() {
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(80.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(80.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: Some(PhysicalContentHeight::new(content_box_pt(120.0))),
            height_basis: PercentageBasis::indefinite(),
        };
        let mut style = ComputedStyle::initial();
        style.box_values.height.replace_with_used(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(60.0),
            ),
        );

        assert_eq!(
            FlexLineCrossConstraint::from_container(
                &style,
                available,
                FlexDirection::Row,
                FlexCrossSize::new(60.0),
            ),
            FlexLineCrossConstraint::DefiniteInnerSize(FlexCrossSize::new(60.0))
        );
    }

    #[test]
    fn line_cross_constraint_keeps_a_min_max_clamp_distinct_from_definiteness() {
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(80.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(80.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: Some(PhysicalContentHeight::new(content_box_pt(200.0))),
            height_basis: PercentageBasis::indefinite(),
        };
        let mut style = ComputedStyle::initial();
        style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(200.0),
        );

        assert_eq!(
            FlexLineCrossConstraint::from_container(
                &style,
                available,
                FlexDirection::Row,
                FlexCrossSize::new(200.0),
            ),
            FlexLineCrossConstraint::ClampedInnerSize(FlexCrossSize::new(200.0))
        );
    }

    #[test]
    fn definite_cross_percentage_gap_is_shared_by_line_sizing_and_stretch() {
        let mut style = ComputedStyle::initial();
        style.flex_wrap = FlexWrap::Wrap;
        style.row_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_percent(0.5));
        let definite_available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(80.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(80.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: Some(PhysicalContentHeight::new(content_box_pt(100.0))),
            height_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
        };
        let indefinite_available = FlexAvailableSpace {
            height_basis: PercentageBasis::indefinite(),
            ..definite_available
        };
        let row_gap = style.row_gap.clone();

        assert_eq!(
            flex_line_cross_gap(
                &style,
                FlexDirection::Row,
                definite_available,
                css::ComputedGap::Normal,
                row_gap.clone(),
            ),
            FlexCrossSize::new(50.0)
        );
        assert_eq!(
            flex_line_cross_gap(
                &style,
                FlexDirection::Row,
                indefinite_available,
                css::ComputedGap::Normal,
                row_gap,
            ),
            FlexCrossSize::new(0.0)
        );

        let mut lines = vec![
            test_line(
                Vec::new(),
                FlexCrossOffset::new(0.0),
                FlexCrossOffset::new(0.0),
            ),
            test_line(
                Vec::new(),
                FlexCrossOffset::new(0.0),
                FlexCrossOffset::new(0.0),
            ),
        ];
        stretch_wrapped_flex_lines_to_container_cross_size(
            &mut lines,
            &mut [],
            &style,
            FlexDirection::Row,
            FlexCrossSize::new(100.0),
            FlexCrossSize::new(50.0),
        );

        assert_eq!(lines[0].cross_start, FlexCrossOffset::new(0.0));
        assert_eq!(lines[0].cross_end, FlexCrossOffset::new(25.0));
        assert_eq!(lines[1].cross_start, FlexCrossOffset::new(75.0));
        assert_eq!(lines[1].cross_end, FlexCrossOffset::new(100.0));
    }

    fn test_line(
        item_indices: Vec<usize>,
        cross_start: FlexCrossOffset,
        cross_end: FlexCrossOffset,
    ) -> FlexLineLayout {
        FlexLineLayout {
            logical_cross_start_rank: 0,
            source_start: item_indices.iter().cloned().min().unwrap_or(0),
            source_end: item_indices
                .iter()
                .cloned()
                .max()
                .map(|index| index + 1)
                .unwrap_or(0),
            item_indices,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(0.0),
            cross_start,
            cross_end,
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        }
    }

    fn test_child() -> StyledChild<'static> {
        StyledChild {
            kind: FormattingContextChildKind::AnonymousContent { children: vec![] },
            style: ComputedStyle::initial(),
        }
    }

    #[test]
    fn balance_context_uses_cross_gap_when_adding_requested_lines() {
        let mut lines = vec![test_line(
            vec![0, 1],
            FlexCrossOffset::new(0.0),
            FlexCrossOffset::new(10.0),
        )];
        let mut items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                ContainerSize::new(20.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(20.0, 0.0),
                ContainerSize::new(20.0, 10.0),
            )),
        ];
        let children = vec![test_child(), test_child()];

        assert!(rebalance_flex_line_membership(
            &mut lines,
            &mut items,
            &children,
            FlexBalanceContext {
                physical_direction: FlexDirection::Row,
                minimum_line_count: 2,
                hypothetical_main_sizes: None,
                main_gap: FlexMainSize::new(0.0),
                cross_gap: FlexCrossSize::new(15.0),
                reserved_line_cross_size: None,
                available_main_size: FlexMainSize::new(100.0),
            },
        ));

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].cross_start, FlexCrossOffset::new(25.0));
        assert_eq!(lines[1].cross_end, FlexCrossOffset::new(35.0));
    }

    #[test]
    fn balance_line_count_is_a_minimum_and_keeps_normal_wrapping() {
        let lines = (0..3)
            .map(|line| {
                test_line(
                    (line * 3..line * 3 + 3).collect(),
                    FlexCrossOffset::new(line as f32 * 10.0),
                    FlexCrossOffset::new((line + 1) as f32 * 10.0),
                )
            })
            .collect::<Vec<_>>();
        let items = (0..9)
            .map(|index| {
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(index as f32 * 20.0, 0.0),
                    ContainerSize::new(20.0, 10.0),
                ))
            })
            .collect::<Vec<_>>();
        let children = (0..9).map(|_| test_child()).collect::<Vec<_>>();
        let context = |minimum_line_count| FlexBalanceContext {
            physical_direction: FlexDirection::Row,
            minimum_line_count,
            hypothetical_main_sizes: None,
            main_gap: FlexMainSize::new(20.0),
            cross_gap: FlexCrossSize::new(0.0),
            reserved_line_cross_size: None,
            available_main_size: FlexMainSize::new(100.0),
        };

        let normal = balanced_flex_line_plan(&lines, &items, &children, context(1))
            .expect("normal wrapping produces a balance plan");
        assert_eq!(
            normal.partitions,
            vec![vec![0, 1, 2], vec![3, 4, 5], vec![6, 7, 8]]
        );

        let minimum = balanced_flex_line_plan(&lines, &items, &children, context(4))
            .expect("minimum line count produces a balance plan");
        assert_eq!(
            minimum.partitions,
            vec![vec![0, 1, 2], vec![3, 4], vec![5, 6], vec![7, 8]],
        );

        let clamped = balanced_flex_line_plan(&lines, &items, &children, context(16))
            .expect("item count bounds the requested minimum");
        assert_eq!(clamped.partitions.len(), 9);
        assert!(clamped.partitions.iter().all(|line| line.len() == 1));
    }

    #[test]
    fn balance_start_bias_is_stable_at_css_pixel_to_point_scale() {
        let item_indices = (0..9).collect::<Vec<_>>();
        let outer_main_sizes = vec![FlexMainSize::new(15.0); 9];

        assert_eq!(
            balanced_flex_line_partitions(
                &item_indices,
                &outer_main_sizes,
                4,
                FlexMainSize::new(15.0),
                FlexMainSize::new(75.0),
            ),
            Some(vec![vec![0, 1, 2], vec![3, 4], vec![5, 6], vec![7, 8]]),
        );
    }

    #[test]
    fn balance_start_bias_survives_an_unrelated_available_main_size() {
        let item_indices = (0..9).collect::<Vec<_>>();
        let outer_main_sizes = vec![FlexMainSize::new(15.0); 9];

        assert_eq!(
            balanced_flex_line_partitions(
                &item_indices,
                &outer_main_sizes,
                4,
                FlexMainSize::new(15.0),
                FlexMainSize::new(470.77557),
            ),
            Some(vec![vec![0, 1, 2], vec![3, 4], vec![5, 6], vec![7, 8]]),
        );
    }

    #[test]
    fn canonical_wrap_topology_uses_hypothetical_outer_main_sizes() {
        let lines = collect_flex_line_topology(
            4,
            FlexWrap::Wrap,
            &[
                FlexMainSize::new(60.0),
                FlexMainSize::new(45.0),
                FlexMainSize::new(20.0),
                FlexMainSize::new(25.0),
            ],
            Some(FlexMainSize::new(100.0)),
            FlexMainSize::new(5.0),
        );

        assert_eq!(
            lines,
            vec![
                FlexLineTopology {
                    item_indices: vec![0],
                    source_start: 0,
                    source_end: 1,
                },
                FlexLineTopology {
                    item_indices: vec![1, 2, 3],
                    source_start: 1,
                    source_end: 4,
                },
            ]
        );
    }

    #[test]
    fn canonical_wrap_topology_keeps_an_oversized_first_item_on_its_own_line() {
        let lines = collect_flex_line_topology(
            3,
            FlexWrap::WrapReverse,
            &[
                FlexMainSize::new(140.0),
                FlexMainSize::new(0.0),
                FlexMainSize::new(30.0),
            ],
            Some(FlexMainSize::new(100.0)),
            FlexMainSize::new(0.0),
        );

        assert_eq!(lines[0].item_indices, vec![0]);
        assert_eq!(lines[1].item_indices, vec![1, 2]);
    }

    #[test]
    fn balance_outer_main_sizes_keep_negative_margin_clamp() {
        let outer_main_sizes = [FlexMainSize::new(20.0 - 30.0), FlexMainSize::new(30.0)];

        assert_eq!(outer_main_sizes[0], FlexMainSize::new(0.0));
        assert_eq!(
            balanced_flex_line_count(
                &outer_main_sizes,
                FlexMainSize::new(0.0),
                FlexMainSize::new(30.0),
            ),
            1
        );
    }

    #[test]
    fn balance_partitions_overflowing_main_sizes_at_available_boundary() {
        let item_indices = [0, 1, 2];
        let outer_main_sizes = [
            FlexMainSize::new(60.0),
            FlexMainSize::new(60.0),
            FlexMainSize::new(40.0),
        ];

        assert_eq!(
            balanced_flex_line_partitions(
                &item_indices,
                &outer_main_sizes,
                2,
                FlexMainSize::new(0.0),
                FlexMainSize::new(100.0),
            ),
            Some(vec![vec![0], vec![1, 2]])
        );
    }
}
