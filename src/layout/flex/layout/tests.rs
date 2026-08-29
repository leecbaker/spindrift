#[cfg(test)]
mod layout_tests {
    use super::super::*;

    #[test]
    fn visible_overflow_fragments_only_after_reaching_the_fragmentainer() {
        let overflow =
            FlexVisibleOverflow::new(FlexFragmentBlockOffset::new(140.0), layout_pt(96.0));

        assert!(
            !overflow.reaches_fragmentainer(FlexFragmentBlockSize::new(200.0)),
            "local overflow must remain inside the current fragmentainer"
        );
        assert!(
            overflow.reaches_fragmentainer(FlexFragmentBlockSize::new(120.0)),
            "source overflow beyond the fragmentainer needs a continuation"
        );
    }

    #[test]
    fn horizontal_only_clip_does_not_suppress_block_axis_continuation() {
        let mut x_only_clip = ComputedStyle::initial();
        x_only_clip.overflow_x = css::Overflow::Clip;
        x_only_clip.overflow_y = css::Overflow::Visible;
        let x_only_axes = UsedOverflowAxes::from_style(&x_only_clip);

        assert!(x_only_axes.clips_x());
        assert!(!x_only_axes.clips_y());
        assert!(!flex_overflow_is_clipped_in_fragmentation_axis(
            x_only_axes,
            false
        ));

        let mut y_clip = ComputedStyle::initial();
        y_clip.overflow_y = css::Overflow::Hidden;
        let y_clip_axes = UsedOverflowAxes::from_style(&y_clip);
        assert!(flex_overflow_is_clipped_in_fragmentation_axis(
            y_clip_axes,
            false
        ));
    }

    #[test]
    fn sole_item_static_probe_resolves_distributed_justify_content_fallbacks() {
        let cases = [
            (
                css::ContentAlignmentKeyword::SpaceBetween,
                css::ContentAlignmentKeyword::FlexStart,
            ),
            (
                css::ContentAlignmentKeyword::Stretch,
                css::ContentAlignmentKeyword::FlexStart,
            ),
            (
                css::ContentAlignmentKeyword::SpaceAround,
                css::ContentAlignmentKeyword::Center,
            ),
            (
                css::ContentAlignmentKeyword::SpaceEvenly,
                css::ContentAlignmentKeyword::Center,
            ),
        ];

        for (authored, expected) in cases {
            let mut style = ComputedStyle::initial();
            style.justify_content.keyword = authored;
            resolve_static_flex_probe_justify_content(&mut style);
            assert_eq!(style.justify_content.keyword, expected);
        }
    }

    #[test]
    fn flex_prebreak_recognizes_a_margin_box_at_page_top() {
        assert!(!should_move_flex_container_to_next_page(
            PageTopBlockPosition::new(980.0),
            layout_pt(20.0),
            layout_pt(990.0),
            PageTopBlockPosition::new(1000.0),
            PageTopBlockPosition::new(0.0),
            layout_pt(1000.0),
        ));
    }

    #[test]
    fn flex_prebreak_still_moves_a_margin_box_that_starts_mid_page() {
        assert!(should_move_flex_container_to_next_page(
            PageTopBlockPosition::new(780.0),
            layout_pt(20.0),
            layout_pt(990.0),
            PageTopBlockPosition::new(1000.0),
            PageTopBlockPosition::new(0.0),
            layout_pt(1000.0),
        ));
    }

    #[test]
    fn isolated_flex_measurement_cannot_whole_box_prebreak() {
        assert!(!flex_container_allows_whole_box_prebreak(
            FragmentainerKind::Page,
            1,
            false,
        ));
        assert!(flex_container_allows_whole_box_prebreak(
            FragmentainerKind::Page,
            0,
            false,
        ));
    }

    #[test]
    fn flex_fragment_materializes_overlapping_wrapped_column_line_slices() {
        let first = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(20.0, 100.0),
        ));
        let second = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(20.0, 0.0),
            ContainerSize::new(20.0, 50.0),
        ));
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(100.0)),
            main_gap: FlexMainSize::new(0.0),
            baselines: FlexContainerBaselineSets::default(),
            items: vec![first, second],
            lines: vec![
                test_flex_line(
                    vec![0],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(100.0),
                    FlexCrossOffset::new(0.0),
                    FlexCrossOffset::new(20.0),
                ),
                test_flex_line(
                    vec![1],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(50.0),
                    FlexCrossOffset::new(20.0),
                    FlexCrossOffset::new(40.0),
                ),
            ],
            fragment_plan: FlexFragmentPlan::default(),
        };
        let fragment = flex_fragment_from_break_unit(
            &FlexBreakUnit {
                topology: FlexReplayTopology::Fragmented,
                item_indices: vec![0, 1],
                line_start: 0,
                line_end: 2,
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(75.0),
                break_before: PageBreak::Auto,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
            &flex_layout,
            FlexFragmentBuildContext {
                page_index: 0,
                outer_inline_span: PageInlineSpan::new(0.0, 40.0),
                content_top: PageTopBlockPosition::new(100.0),
                block_offset: FlexFragmentBlockOffset::new(0.0),
                first_fragmentainer_capacity: layout_pt(75.0),
                continuation_fragmentainer_capacity: layout_pt(75.0),
                starts_page_fragment: true,
            },
            false,
        );

        assert_eq!(fragment.line_fragments.len(), 2);
        assert_eq!(fragment.line_fragments[0].item_indices, vec![0]);
        assert_eq!(fragment.items[0].line_index, 0);
        assert_eq!(
            fragment.line_fragments[0].source_bounds,
            FlexFragmentBlockBounds::new(
                FlexFragmentBlockOffset::new(0.0),
                FlexFragmentBlockOffset::new(75.0),
            )
        );
        assert_eq!(fragment.line_fragments[1].item_indices, vec![1]);
        assert_eq!(fragment.items[1].line_index, 1);
        assert_eq!(
            fragment.line_fragments[1].source_bounds,
            FlexFragmentBlockBounds::new(
                FlexFragmentBlockOffset::new(0.0),
                FlexFragmentBlockOffset::new(50.0),
            )
        );

        let unfragmented = flex_fragment_from_break_unit(
            &FlexBreakUnit {
                topology: FlexReplayTopology::Unfragmented,
                // The physical-Y order of these wrapped column lines is not
                // the replay order for `column-reverse`.
                item_indices: vec![1, 0],
                line_start: 0,
                line_end: 2,
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(100.0),
                break_before: PageBreak::Auto,
                break_after: PageBreak::Auto,
                break_inside_avoid: false,
            },
            &flex_layout,
            FlexFragmentBuildContext {
                page_index: 0,
                outer_inline_span: PageInlineSpan::new(0.0, 40.0),
                content_top: PageTopBlockPosition::new(100.0),
                block_offset: FlexFragmentBlockOffset::new(0.0),
                first_fragmentainer_capacity: layout_pt(100.0),
                continuation_fragmentainer_capacity: layout_pt(100.0),
                starts_page_fragment: true,
            },
            false,
        );
        assert_eq!(
            unfragmented
                .items
                .iter()
                .map(|item| item.item_index)
                .collect::<Vec<_>>(),
            vec![1, 0]
        );
        assert_eq!(
            unfragmented.items[0].bounds.height(),
            FlexPhysicalVerticalSize::new(50.0)
        );
        assert_eq!(
            unfragmented.items[1].bounds.height(),
            FlexPhysicalVerticalSize::new(100.0)
        );
        assert!(unfragmented.items.iter().all(|item| {
            !item.continuation.continues_from_previous_fragment()
                && (item.continuation.source_content_slice.block_end.points()
                    - item.source_bounds.height().points())
                .abs()
                    <= 0.01
        }));
    }

    #[test]
    fn vertical_row_break_units_partition_overlapping_wrapped_lines_by_item_interval() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::SidewaysRl;
        style.flex_direction = FlexDirection::Row;
        let children = (0..4)
            .map(|_| StyledChild {
                kind: crate::layout::itemization::FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            })
            .collect::<Vec<_>>();
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(110.0)),
            main_gap: FlexMainSize::new(10.0),
            baselines: FlexContainerBaselineSets::default(),
            items: vec![
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 0.0),
                    ContainerSize::new(50.0, 50.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 60.0),
                    ContainerSize::new(50.0, 50.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(60.0, 0.0),
                    ContainerSize::new(50.0, 50.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(60.0, 60.0),
                    ContainerSize::new(50.0, 50.0),
                )),
            ],
            lines: vec![
                test_flex_line(
                    vec![0, 1],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(110.0),
                    FlexCrossOffset::new(0.0),
                    FlexCrossOffset::new(50.0),
                ),
                test_flex_line(
                    vec![2, 3],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(110.0),
                    FlexCrossOffset::new(60.0),
                    FlexCrossOffset::new(110.0),
                ),
            ],
            fragment_plan: FlexFragmentPlan::default(),
        };

        let units = flex_container_break_units(
            FragmentainerKind::Page,
            &flex_layout,
            &children,
            &style,
            false,
            layout_pt(110.0),
        );

        assert_eq!(units.len(), 2);
        assert_eq!(units[0].item_indices, vec![0, 2]);
        assert_eq!(
            (units[0].block_start, units[0].block_end),
            (
                FlexFragmentBlockOffset::new(0.0),
                FlexFragmentBlockOffset::new(50.0),
            )
        );
        assert_eq!(units[1].item_indices, vec![1, 3]);
        assert_eq!(
            (units[1].block_start, units[1].block_end),
            (
                FlexFragmentBlockOffset::new(60.0),
                FlexFragmentBlockOffset::new(110.0),
            )
        );
    }

    #[test]
    fn wrapped_column_item_growth_stays_within_its_own_line() {
        let mut items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                ContainerSize::new(20.0, 120.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 120.0),
                ContainerSize::new(20.0, 20.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(20.0, 0.0),
                ContainerSize::new(20.0, 120.0),
            )),
        ];
        let lines = vec![
            test_flex_line(
                vec![0, 1],
                FlexMainOffset::new(0.0),
                FlexMainOffset::new(140.0),
                FlexCrossOffset::new(0.0),
                FlexCrossOffset::new(20.0),
            ),
            test_flex_line(
                vec![2],
                FlexMainOffset::new(0.0),
                FlexMainOffset::new(120.0),
                FlexCrossOffset::new(20.0),
                FlexCrossOffset::new(40.0),
            ),
        ];

        assert!(expand_wrapped_column_items_through_fragmentainers(
            &mut items,
            &lines,
            FlexFragmentBlockSize::new(100.0),
            FlexFragmentBlockSize::new(100.0),
        ));
        assert_eq!(items[0].height().points(), 200.0);
        assert_eq!(items[1].y().points(), 200.0);
        assert_eq!(items[2].height().points(), 200.0);
        assert_eq!(items[0].y().points(), 0.0);
        assert_eq!(items[2].y().points(), 0.0);
    }

    #[test]
    fn orthogonal_block_flex_auto_inline_size_projects_to_physical_height() {
        let mut vertical = ComputedStyle::initial();
        vertical.writing_mode = WritingMode::VerticalRl;
        assert_eq!(
            orthogonal_block_flex_auto_inline_content_height(
                &vertical,
                true,
                PercentageBasis::definite(PhysicalContentHeight::new(content_box_pt(100.0))),
                non_content_pt(12.0),
            ),
            Some(content_box_pt(88.0))
        );

        let horizontal = ComputedStyle::initial();
        assert_eq!(
            orthogonal_block_flex_auto_inline_content_height(
                &horizontal,
                true,
                PercentageBasis::definite(PhysicalContentHeight::new(content_box_pt(100.0))),
                non_content_pt(12.0),
            ),
            None
        );

        assert_eq!(
            orthogonal_block_flex_auto_inline_content_height(
                &vertical,
                false,
                PercentageBasis::definite(PhysicalContentHeight::new(content_box_pt(100.0))),
                non_content_pt(12.0),
            ),
            None,
            "floats use shrink-to-fit sizing rather than normal-flow block fill"
        );

        assert_eq!(
            orthogonal_block_flex_auto_inline_content_height(
                &vertical,
                true,
                PercentageBasis::indefinite(),
                non_content_pt(12.0),
            ),
            None,
            "an orthogonal fallback chooses fit-content measurement but is not a used height"
        );
    }

    fn containing_block_height_basis(height: PhysicalContentHeight) -> BlockSizePercentageBasis {
        PercentageBasis::definite_from(
            height.content_box_length(),
            BlockSizeBasisSource::ContainingBlock,
        )
    }

    #[test]
    fn definite_flex_container_height_transfers_content_box_aspect_ratio() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::ContentBox;
        style.aspect_ratio = css::AspectRatio::from_ratio(2.0).unwrap();

        let height = definite_flex_container_content_height(
            &style,
            None,
            content_box_pt(120.0),
            containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(100.0))),
            non_content_pt(20.0),
            non_content_pt(10.0),
        );

        assert_eq!(height, Some(content_box_pt(60.0)));
    }

    #[test]
    fn definite_flex_container_height_transfers_border_box_aspect_ratio() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.aspect_ratio = css::AspectRatio::from_ratio(2.0).unwrap();

        let height = definite_flex_container_content_height(
            &style,
            None,
            content_box_pt(100.0),
            containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(100.0))),
            non_content_pt(20.0),
            non_content_pt(10.0),
        );

        assert_eq!(height, Some(content_box_pt(50.0)));
    }

    #[test]
    fn definite_flex_container_height_keeps_explicit_height_and_rejects_invalid_ratio() {
        let explicit_height = content_box_pt(45.0);
        let style = ComputedStyle::initial();
        assert_eq!(
            definite_flex_container_content_height(
                &style,
                Some(explicit_height),
                content_box_pt(120.0),
                containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(100.0))),
                non_content_pt(0.0),
                non_content_pt(0.0),
            ),
            Some(explicit_height)
        );

        let invalid_ratio_style = ComputedStyle::initial();
        assert!(css::AspectRatio::from_ratio(f32::NAN).is_none());
        assert_eq!(
            definite_flex_container_content_height(
                &invalid_ratio_style,
                None,
                content_box_pt(120.0),
                containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(100.0))),
                non_content_pt(0.0),
                non_content_pt(0.0),
            ),
            None
        );
    }

    #[test]
    fn ratio_derived_flex_container_height_is_floored_by_automatic_minimum() {
        let style = ComputedStyle::initial();

        let height = select_ratio_derived_flex_container_height(
            &style,
            content_box_pt(50.0),
            content_box_pt(100.0),
            containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(500.0))),
        );

        assert_eq!(height, content_box_pt(100.0));
    }

    #[test]
    fn ratio_derived_flex_container_height_applies_max_after_automatic_minimum() {
        let mut style = ComputedStyle::initial();
        style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(80.0),
        );

        let height = select_ratio_derived_flex_container_height(
            &style,
            content_box_pt(50.0),
            content_box_pt(100.0),
            containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(500.0))),
        );

        assert_eq!(height, content_box_pt(80.0));
    }

    #[test]
    fn wrapped_column_flex_uses_max_height_as_available_height() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Column;
        style.flex_wrap = FlexWrap::Wrap;
        style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(60.0),
        );

        assert_eq!(
            flex_available_content_height(
                &style,
                None,
                containing_block_height_basis(PhysicalContentHeight::new(content_box_pt(100.0))),
            ),
            Some(content_box_pt(60.0))
        );
    }

    #[test]
    fn flex_source_block_end_projects_typed_capacity_into_local_offset() {
        assert_eq!(
            flex_source_block_end_after_available_capacity(
                FlexFragmentBlockOffset::new(70.0),
                layout_pt(30.0),
            ),
            FlexFragmentBlockOffset::new(100.0)
        );
    }

    #[test]
    fn flex_gap_gutters_use_line_local_main_axis_gaps() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Row;
        style.column_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        style.row_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(50.0)),
            main_gap: FlexMainSize::new(10.0),
            baselines: FlexContainerBaselineSets {
                vertical: FlexItemBaselinePair {
                    first: Some(flex_vertical_baseline_from_points(0.0)),
                    last: None,
                },
                horizontal: FlexItemBaselinePair::default(),
                ..FlexContainerBaselineSets::default()
            },
            items: vec![
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 0.0),
                    ContainerSize::new(30.0, 20.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(40.0, 0.0),
                    ContainerSize::new(30.0, 20.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 30.0),
                    ContainerSize::new(40.0, 20.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(50.0, 30.0),
                    ContainerSize::new(30.0, 20.0),
                )),
            ],
            lines: vec![
                test_flex_line(
                    vec![0, 1],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(70.0),
                    FlexCrossOffset::new(0.0),
                    FlexCrossOffset::new(20.0),
                ),
                test_flex_line(
                    vec![2, 3],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(80.0),
                    FlexCrossOffset::new(30.0),
                    FlexCrossOffset::new(50.0),
                ),
            ],
            fragment_plan: FlexFragmentPlan::default(),
        };

        let gutters = flex_gap_decoration_gutters(
            &flex_layout,
            &style,
            PhysicalContentWidth::new(content_box_pt(100.0)),
            PhysicalContentHeight::new(content_box_pt(50.0)),
        );

        assert_eq!(gutters.columns.len(), 2);
        assert_eq!(gutters.columns[0].span.start, 30.0);
        assert_eq!(gutters.columns[0].span.end, 40.0);
        assert_eq!(gutters.columns[1].span.start, 40.0);
        assert_eq!(gutters.columns[1].span.end, 50.0);
        assert_eq!(gutters.rows.len(), 1);
        assert_eq!(gutters.rows[0].span.start, 20.0);
        assert_eq!(gutters.rows[0].span.end, 30.0);

        let mut no_gap_style = style;
        no_gap_style.column_gap = css::ComputedGap::Normal;
        no_gap_style.row_gap = css::ComputedGap::Normal;
        let no_gap_gutters = flex_gap_decoration_gutters(
            &flex_layout,
            &no_gap_style,
            PhysicalContentWidth::new(content_box_pt(100.0)),
            PhysicalContentHeight::new(content_box_pt(50.0)),
        );
        // A zero-width, non-fragmented flex gap remains a member of the CSS
        // Gaps assignment sequence. Its decoration may intentionally paint
        // outside that collapsed gutter.
        assert_eq!(no_gap_gutters.columns.len(), 2);
        assert!(
            no_gap_gutters
                .columns
                .iter()
                .all(|gutter| (gutter.span.end - gutter.span.start).abs() <= 0.01)
        );
        assert_eq!(
            no_gap_gutters
                .columns
                .iter()
                .map(|gutter| gutter.rule_index)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1)]
        );
        assert_eq!(no_gap_gutters.rows.len(), 1);
        assert!(
            (no_gap_gutters.rows[0].span.end - no_gap_gutters.rows[0].span.start).abs() <= 0.01
        );
    }

    #[test]
    fn flex_gap_gutters_preserve_stretched_line_bands() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Column;
        style.justify_content.keyword = ContentAlignmentKeyword::SpaceBetween;
        style.column_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(5.0));
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(200.0)),
            main_gap: FlexMainSize::new(0.0),
            baselines: FlexContainerBaselineSets::default(),
            items: vec![
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 0.0),
                    ContainerSize::new(50.0, 50.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 150.0),
                    ContainerSize::new(50.0, 50.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(62.5, 0.0),
                    ContainerSize::new(50.0, 50.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(62.5, 150.0),
                    ContainerSize::new(50.0, 50.0),
                )),
            ],
            lines: vec![
                test_flex_line(
                    vec![0, 1],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(200.0),
                    FlexCrossOffset::new(0.0),
                    FlexCrossOffset::new(57.5),
                ),
                test_flex_line(
                    vec![2, 3],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(200.0),
                    FlexCrossOffset::new(62.5),
                    FlexCrossOffset::new(120.0),
                ),
            ],
            fragment_plan: FlexFragmentPlan::default(),
        };

        let gutters = flex_gap_decoration_gutters(
            &flex_layout,
            &style,
            PhysicalContentWidth::new(content_box_pt(120.0)),
            PhysicalContentHeight::new(content_box_pt(200.0)),
        );

        assert_eq!(gutters.columns.len(), 1);
        assert_eq!(gutters.columns[0].span, GapAxisSpan::new(57.5, 62.5));
        assert_eq!(gutters.rows.len(), 2);
        assert_eq!(gutters.rows[0].span, GapAxisSpan::new(50.0, 150.0));
        assert_eq!(
            gutters.rows[0].segment_range,
            Some(GapAxisSpan::new(0.0, 57.5))
        );
        assert_eq!(gutters.rows[1].span, GapAxisSpan::new(50.0, 150.0));
        assert_eq!(
            gutters.rows[1].segment_range,
            Some(GapAxisSpan::new(62.5, 120.0))
        );
    }

    #[test]
    fn wrapped_flex_row_gap_paints_between_two_full_width_lines() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Row;
        style.row_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        style.row_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(10.0));
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.row_rule.colors = css::GapRuleList::single(CssColor::new(255, 215, 0));
        let first = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(100.0, 50.0),
        ));
        let second = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 60.0),
            ContainerSize::new(100.0, 50.0),
        ));
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(110.0)),
            main_gap: FlexMainSize::new(0.0),
            baselines: FlexContainerBaselineSets::default(),
            items: vec![first, second],
            lines: vec![
                test_flex_line(
                    vec![0],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(100.0),
                    FlexCrossOffset::new(0.0),
                    FlexCrossOffset::new(50.0),
                ),
                test_flex_line(
                    vec![1],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(100.0),
                    FlexCrossOffset::new(60.0),
                    FlexCrossOffset::new(110.0),
                ),
            ],
            fragment_plan: FlexFragmentPlan::default(),
        };

        let primitives = flex_gap_decoration_primitives_with_gutters(
            &style,
            GapDecorationContainer::new(0.0, 110.0, 100.0, 110.0),
            &flex_gap_decoration_items(&flex_layout),
            &flex_gap_decoration_gutters(
                &flex_layout,
                &style,
                PhysicalContentWidth::new(content_box_pt(100.0)),
                PhysicalContentHeight::new(content_box_pt(110.0)),
            ),
        );
        let strokes = solid_gap_rule_centerlines(&primitives);

        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].x1(), 0.0);
        assert_eq!(strokes[0].x2(), 100.0);
        assert_eq!(strokes[0].y1(), 55.0);
        assert_eq!(strokes[0].stroke_width, PaintStrokeWidth::new(10.0));
    }

    #[test]
    fn finalized_geometry_splits_stale_line_membership_for_gap_topology() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Row;
        style.row_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(110.0)),
            main_gap: FlexMainSize::new(0.0),
            baselines: FlexContainerBaselineSets::default(),
            items: vec![
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 0.0),
                    ContainerSize::new(100.0, 50.0),
                )),
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 60.0),
                    ContainerSize::new(100.0, 50.0),
                )),
            ],
            // This is the Taffy replay shape observed for an auto-height,
            // wrapped container: its line record is stale, but the final
            // rectangles unambiguously occupy two cross-axis bands.
            lines: vec![test_flex_line(
                vec![0, 1],
                FlexMainOffset::new(0.0),
                FlexMainOffset::new(100.0),
                FlexCrossOffset::new(0.0),
                FlexCrossOffset::new(110.0),
            )],
            fragment_plan: FlexFragmentPlan::default(),
        };

        let gutters = flex_gap_decoration_gutters(
            &flex_layout,
            &style,
            PhysicalContentWidth::new(content_box_pt(100.0)),
            PhysicalContentHeight::new(content_box_pt(110.0)),
        );

        assert!(gutters.columns.is_empty());
        assert_eq!(gutters.rows.len(), 1);
        assert_eq!(gutters.rows[0].span, GapAxisSpan::new(50.0, 60.0));
    }

    #[test]
    fn flex_gap_decorations_are_projected_into_page_fragments() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.flex_direction = FlexDirection::Row;
        style.column_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        style.column_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(CssColor::new(255, 0, 0));
        let left = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 20.0),
            ContainerSize::new(30.0, 50.0),
        ));
        let right = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(40.0, 20.0),
            ContainerSize::new(30.0, 50.0),
        ));
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(70.0)),
            main_gap: FlexMainSize::new(10.0),
            baselines: FlexContainerBaselineSets {
                vertical: FlexItemBaselinePair {
                    first: Some(flex_vertical_baseline_from_points(0.0)),
                    last: None,
                },
                horizontal: FlexItemBaselinePair::default(),
                ..FlexContainerBaselineSets::default()
            },
            items: vec![left.clone(), right.clone()],
            lines: vec![test_flex_line(
                vec![0, 1],
                FlexMainOffset::new(0.0),
                FlexMainOffset::new(70.0),
                FlexCrossOffset::new(20.0),
                FlexCrossOffset::new(70.0),
            )],
            fragment_plan: FlexFragmentPlan {
                fragments: vec![FlexFragmentLayout {
                    page_index: 0,
                    line_start: 0,
                    line_end: 1,
                    block_start: FlexFragmentBlockOffset::new(20.0),
                    block_end: FlexFragmentBlockOffset::new(70.0),
                    line_fragments: Vec::new(),
                    items: vec![
                        test_flex_item_fragment(0, left),
                        test_flex_item_fragment(1, right),
                    ],
                    metadata: FragmentPageMetadata::empty(0),
                }],
                materialized_fragments: Vec::new(),
            },
        };

        let primitives = flex_gap_decoration_primitives_for_page(
            &flex_layout,
            &style,
            FlexGapDecorationFragmentContext {
                page_index: 0,
                content_inline_span: PageInlineSpan::new(0.0, 70.0),
                content_height: PhysicalContentHeight::new(content_box_pt(70.0)),
                fragment_bounds: PaintClip::new(0.0, 100.0, 70.0, 50.0),
                has_forced_item_breaks: false,
            },
        );
        let strokes = solid_gap_rule_centerlines(&primitives);

        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].x1(), 35.0);
        assert_eq!(strokes[0].y1(), 150.0);
        assert_eq!(strokes[0].y2(), 100.0);
        assert_eq!(strokes[0].stroke_width, PaintStrokeWidth::new(4.0));
    }

    #[test]
    fn vertical_main_axis_break_suppresses_cross_gutter_on_outgoing_fragment() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.writing_mode = WritingMode::VerticalRl;
        style.flex_direction = FlexDirection::Row;
        style.column_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        style.row_gap =
            css::ComputedGap::LengthPercentage(css::ComputedLengthPercentage::from_points(10.0));
        style.column_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(10.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(CssColor::new(255, 0, 0));
        style.row_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(10.0));
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.row_rule.colors = css::GapRuleList::single(CssColor::new(0, 128, 0));
        let items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                ContainerSize::new(50.0, 50.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 60.0),
                ContainerSize::new(50.0, 50.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(60.0, 0.0),
                ContainerSize::new(50.0, 50.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(60.0, 60.0),
                ContainerSize::new(50.0, 50.0),
            )),
        ];
        let flex_layout = FlexLayout {
            height: PhysicalContentHeight::new(content_box_pt(110.0)),
            main_gap: FlexMainSize::new(10.0),
            baselines: FlexContainerBaselineSets::default(),
            items,
            lines: vec![
                test_flex_line(
                    vec![0, 1],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(110.0),
                    FlexCrossOffset::new(0.0),
                    FlexCrossOffset::new(50.0),
                ),
                test_flex_line(
                    vec![2, 3],
                    FlexMainOffset::new(0.0),
                    FlexMainOffset::new(110.0),
                    FlexCrossOffset::new(60.0),
                    FlexCrossOffset::new(110.0),
                ),
            ],
            fragment_plan: FlexFragmentPlan {
                fragments: vec![
                    FlexFragmentLayout {
                        page_index: 0,
                        line_start: 0,
                        line_end: 2,
                        block_start: FlexFragmentBlockOffset::new(0.0),
                        block_end: FlexFragmentBlockOffset::new(60.0),
                        line_fragments: Vec::new(),
                        items: Vec::new(),
                        metadata: FragmentPageMetadata::empty(0),
                    },
                    FlexFragmentLayout {
                        page_index: 1,
                        line_start: 0,
                        line_end: 2,
                        block_start: FlexFragmentBlockOffset::new(60.0),
                        block_end: FlexFragmentBlockOffset::new(110.0),
                        line_fragments: Vec::new(),
                        items: Vec::new(),
                        metadata: FragmentPageMetadata::empty(1),
                    },
                ],
                materialized_fragments: Vec::new(),
            },
        };

        let primitives = flex_gap_decoration_primitives_for_page(
            &flex_layout,
            &style,
            FlexGapDecorationFragmentContext {
                page_index: 0,
                content_inline_span: PageInlineSpan::new(0.0, 110.0),
                content_height: PhysicalContentHeight::new(content_box_pt(110.0)),
                fragment_bounds: PaintClip::new(0.0, 100.0, 110.0, 60.0),
                has_forced_item_breaks: false,
            },
        );
        let strokes = solid_gap_rule_centerlines(&primitives);

        // The fragment break suppresses the vertical cross-axis gutter. Its
        // two neighboring horizontal main-gap portions meet at the former
        // junction instead of leaving a decoration-sized hole.
        assert_eq!(strokes.len(), 2);
        assert!(strokes.iter().all(|stroke| stroke.y1() == stroke.y2()));
        assert_eq!(
            strokes
                .iter()
                .map(|stroke| stroke.x1().min(stroke.x2()))
                .min_by(|left, right| left.partial_cmp(right).unwrap())
                .unwrap(),
            0.0
        );
        assert_eq!(
            strokes
                .iter()
                .map(|stroke| stroke.x1().max(stroke.x2()))
                .max_by(|left, right| left.partial_cmp(right).unwrap())
                .unwrap(),
            110.0
        );
        assert!(
            strokes
                .iter()
                .all(|stroke| stroke.stroke_width == PaintStrokeWidth::new(10.0))
        );
    }

    #[test]
    fn flex_break_combiner_ignores_other_fragmentainer_values() {
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Auto, PageBreak::Column),
            PageBreak::Auto
        );
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Auto, PageBreak::AvoidColumn),
            PageBreak::Auto
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Auto, PageBreak::Column),
            PageBreak::Column
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Auto, PageBreak::AvoidColumn),
            PageBreak::AvoidColumn
        );
    }

    #[test]
    fn flex_break_combiner_keeps_existing_target_forced_break() {
        assert_eq!(
            FragmentainerKind::Page.combine_break(PageBreak::Left, PageBreak::Page),
            PageBreak::Left
        );
        assert_eq!(
            FragmentainerKind::Column.combine_break(PageBreak::Column, PageBreak::Avoid),
            PageBreak::Column
        );
    }

    #[test]
    fn flex_unit_break_aggregation_scopes_forced_values_to_fragmentainer_kind() {
        let mut first = ComputedStyle::initial();
        first.break_before = PageBreak::Column;
        first.break_after = PageBreak::AvoidColumn;
        let mut second = ComputedStyle::initial();
        second.break_before = PageBreak::Auto;
        second.break_after = PageBreak::Page;
        let styles = [&first, &second];

        assert_eq!(
            flex_unit_break_before_for_styles(FragmentainerKind::Page, styles),
            PageBreak::Auto
        );
        assert_eq!(
            flex_unit_break_before_for_styles(FragmentainerKind::Column, styles),
            PageBreak::Column
        );
        assert_eq!(
            flex_unit_break_after_for_styles(FragmentainerKind::Page, styles),
            PageBreak::Page
        );
        assert_eq!(
            flex_unit_break_after_for_styles(FragmentainerKind::Column, styles),
            PageBreak::AvoidColumn
        );
    }

    #[test]
    fn flex_unit_prebreak_scopes_avoid_to_fragmentainer_kind() {
        let opportunity = FragmentBreakOpportunity {
            source_block_offset: 20.0,
            break_before: PageBreak::Auto,
            break_after: PageBreak::AvoidColumn,
            break_inside_avoid: false,
        };
        let available_content_block_size = layout_pt(10.0);

        let page_decision = FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
            fragmentainer_kind: FragmentainerKind::Page,
            break_is_applicable: true,
            unit_is_oversized: false,
            has_prior_unit: false,
            has_later_unit: false,
            cursor: FlexFragmentCursor::new(
                PageTopBlockPosition::new(0.0),
                FlexFragmentBlockOffset::new(0.0),
            ),
            unit_block_start: FlexFragmentBlockOffset::new(20.0),
            unit_block_end: FlexFragmentBlockOffset::new(40.0),
            available_content_block_size,
            break_opportunity: opportunity,
            can_advance: true,
        });
        let column_decision = FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
            fragmentainer_kind: FragmentainerKind::Column,
            break_is_applicable: true,
            unit_is_oversized: false,
            has_prior_unit: false,
            has_later_unit: false,
            cursor: FlexFragmentCursor::new(
                PageTopBlockPosition::new(0.0),
                FlexFragmentBlockOffset::new(0.0),
            ),
            unit_block_start: FlexFragmentBlockOffset::new(20.0),
            unit_block_end: FlexFragmentBlockOffset::new(40.0),
            available_content_block_size,
            break_opportunity: opportunity,
            can_advance: true,
        });

        assert!(page_decision.transition_before_unit.is_none());
        let column_transition = column_decision
            .transition_before_unit
            .expect("column avoid should advance before the flex unit");
        assert_eq!(
            column_transition.fragmentainer_kind,
            FragmentainerKind::Column
        );
    }

    #[test]
    fn flex_unit_prebreak_advances_sole_unit_from_exhausted_fragmentainer() {
        let decision = FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
            fragmentainer_kind: FragmentainerKind::Column,
            break_is_applicable: true,
            unit_is_oversized: false,
            has_prior_unit: false,
            has_later_unit: false,
            cursor: FlexFragmentCursor::new(
                PageTopBlockPosition::new(0.0),
                FlexFragmentBlockOffset::new(0.0),
            ),
            unit_block_start: FlexFragmentBlockOffset::new(0.0),
            unit_block_end: FlexFragmentBlockOffset::new(75.0),
            available_content_block_size: layout_pt(0.0),
            break_opportunity: FragmentBreakOpportunity::before_box_boundary(
                FragmentainerKind::Column,
                0.0,
                FragmentBreakContext::new(
                    PageBreak::Auto,
                    PageBreak::Auto,
                    PageBreak::Auto,
                    PageBreak::Auto,
                ),
                PageBreak::Auto,
                false,
            ),
            can_advance: true,
        });

        assert_eq!(
            decision
                .transition_before_unit
                .expect("a sole flex unit advances out of an exhausted column")
                .reason,
            FlexFragmentBreakReason::OverflowOrAvoid
        );
    }

    #[test]
    fn flex_unit_prebreak_preserves_remaining_source_gap() {
        let decision = FlexUnitPrebreakDecision::choose(FlexUnitPrebreakDecisionInput {
            fragmentainer_kind: FragmentainerKind::Page,
            break_is_applicable: true,
            // The next item itself fits in an empty fragmentainer. Its source
            // start is separated from the preceding item by a gap, of which
            // 18pt remain after this fragmentainer's 18pt capacity.
            unit_is_oversized: false,
            has_prior_unit: true,
            has_later_unit: true,
            cursor: FlexFragmentCursor::new(
                PageTopBlockPosition::new(0.0),
                FlexFragmentBlockOffset::new(90.0),
            ),
            unit_block_start: FlexFragmentBlockOffset::new(126.0),
            unit_block_end: FlexFragmentBlockOffset::new(162.0),
            available_content_block_size: layout_pt(18.0),
            break_opportunity: FragmentBreakOpportunity::before_box_boundary(
                FragmentainerKind::Page,
                126.0,
                FragmentBreakContext::new(
                    PageBreak::Auto,
                    PageBreak::Auto,
                    PageBreak::Auto,
                    PageBreak::Auto,
                ),
                PageBreak::Auto,
                false,
            ),
            can_advance: true,
        });

        assert_eq!(
            decision
                .transition_before_unit
                .expect("the gap-separated item moves to the next fragmentainer")
                .next_block_offset,
            FlexFragmentBlockOffset::new(108.0),
        );
    }

    #[test]
    fn flex_fragment_transition_page_cursor_gate_is_target_specific() {
        let page_transition = FlexFragmentTransitionDecision::forced(
            FragmentainerKind::Page,
            FlexFragmentBlockOffset::new(40.0),
        );
        let column_transition = FlexFragmentTransitionDecision::forced(
            FragmentainerKind::Column,
            FlexFragmentBlockOffset::new(40.0),
        );

        assert!(page_transition.materializes_page_cursor());
        assert!(!column_transition.materializes_page_cursor());
        assert_eq!(
            column_transition.cursor_after_fragmentainer_advance(PageTopBlockPosition::new(200.0)),
            FlexFragmentCursor::new(
                PageTopBlockPosition::new(200.0),
                FlexFragmentBlockOffset::new(40.0)
            )
        );
    }

    #[test]
    fn single_line_row_continuation_fills_its_final_fragment() {
        assert_eq!(
            single_line_row_fragmented_cross_size(
                FlexCrossSize::new(112.5),
                FlexFragmentBlockSize::new(100.0),
                FlexFragmentBlockSize::new(100.0),
            ),
            Some(FlexCrossSize::new(200.0))
        );
        assert_eq!(
            single_line_row_fragmented_cross_size(
                FlexCrossSize::new(100.0),
                FlexFragmentBlockSize::new(100.0),
                FlexFragmentBlockSize::new(100.0),
            ),
            None
        );
    }

    #[test]
    fn cloned_single_line_row_uses_content_capacity_in_every_column() {
        // `clone-014`: the first cursor is already below the 7.5pt
        // block-start decoration, while each fresh 75pt column needs both
        // cloned edges reserved. Four 60pt content slices must therefore
        // account for the 240pt item exactly, without a fifth empty box
        // fragment.
        assert_eq!(
            single_line_row_fragmented_cross_size(
                FlexCrossSize::new(240.0),
                FlexFragmentBlockSize::new(60.0),
                FlexFragmentBlockSize::new(60.0),
            ),
            Some(FlexCrossSize::new(240.0))
        );
    }

    fn test_flex_item_fragment(item_index: usize, item: FlexItemLayout) -> FlexItemFragmentLayout {
        FlexItemFragmentLayout {
            item_index,
            source_item_index: item_index,
            line_index: 0,
            source_bounds: item.clone(),
            used_bounds: item.clone(),
            bounds: item.clone(),
            content_slice: FlexFragmentSlice {
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(item.height().points()),
            },
            decoration_slice: FlexFragmentSlice {
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(item.height().points()),
            },
            continuation: FlexItemContinuation::default(),
            metadata: FragmentPageMetadata::empty(0),
        }
    }

    fn test_flex_line(
        item_indices: Vec<usize>,
        main_start: FlexMainOffset,
        main_end: FlexMainOffset,
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
            main_start,
            main_end,
            cross_start,
            cross_end,
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        }
    }
}
