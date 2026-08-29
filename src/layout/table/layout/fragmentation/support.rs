//! Tests for shared table-layout support.

use super::*;
#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::Position;
    use crate::layout::{FlowAxes, PageBoxEdges, PageMargins, PageSize};

    #[test]
    fn auto_width_fixed_table_keeps_its_intrinsic_grid_floor() {
        let mut style = ComputedStyle::initial();
        style.table_layout = TableLayout::Fixed;

        assert_eq!(
            table_content_width_clamped_to_min_content(
                &style,
                LogicalInlineContentSize::new(content_box_pt(0.0)),
                LogicalInlineContentSize::new(content_box_pt(75.0)),
            )
            .points(),
            75.0,
        );
    }

    #[test]
    fn block_table_uses_the_in_flow_block_paint_band() {
        let mut style = ComputedStyle::initial();
        style.display = css::Display::TABLE;

        assert_eq!(table_parent_paint_band(&style), PaintBand::InFlowBlock);
    }

    #[test]
    fn inline_table_uses_the_inline_paint_band() {
        let mut style = ComputedStyle::initial();
        style.display = css::Display::INLINE_TABLE;

        assert_eq!(table_parent_paint_band(&style), PaintBand::Inline);
    }

    #[test]
    fn relatively_positioned_table_keeps_the_stacking_context_policy_band() {
        let mut style = ComputedStyle::initial();
        style.display = css::Display::TABLE;
        style.position = Position::Relative;

        let policy = table_atomic_stacking_policy(
            &style,
            table_parent_paint_band(&style),
            PaintClip::from_paint_rect(paint_space_rect(0.0, 0.0, 10.0, 10.0)),
            None,
        );

        assert_eq!(policy.parent_band, PaintBand::AutoZeroZ);
    }

    fn current_fragmentainer(
        block_size: f32,
        content_start: f32,
        block_end: f32,
        repeat_policy: TableFragmentRepeatPolicy,
        header_height: f32,
        footer_height: f32,
        reserve_footer: bool,
    ) -> TableFragmentainer {
        TableFragmentainer::current_from_page_cursor_bounds(
            layout_pt(block_size),
            PageTopBlockPosition::new(content_start),
            PageTopBlockPosition::new(block_end),
            repeat_policy,
            layout_pt(header_height),
            layout_pt(footer_height),
            reserve_footer,
        )
    }

    #[test]
    fn row_span_background_bounds_preserve_the_explicit_physical_inline_span() {
        let bounds = table_fragment_row_span_bounds(
            PageInlineSpan::new(30.0, 90.0),
            &[200.0, 160.0],
            &[40.0, 40.0],
            0,
            2,
        )
        .expect("two visible rows have a paint bound");

        assert_eq!(
            bounds,
            PageTopRect::new(30.0, 200.0, 90.0, 80.0).paint_clip()
        );
    }

    #[test]
    fn table_cell_clip_region_keeps_disjoint_visible_rowspan_areas() {
        let region = TableCellClipRegion::from_clips(vec![
            OverflowClip::from_paint_rect(paint_space_rect(0.0, 0.0, 10.0, 4.0)),
            OverflowClip::from_paint_rect(paint_space_rect(0.0, 6.0, 10.0, 4.0)),
        ])
        .expect("visible areas");
        let viewport = TableCellClipRegion::from_clip(OverflowClip::from_paint_rect(
            paint_space_rect(2.0, 0.0, 4.0, 10.0),
        ));

        let intersection = region.intersect(&viewport).expect("shared area");
        let clips = intersection.paint_clips();
        assert_eq!(clips.len(), 2);
        assert_eq!(
            intersection.bounding_clip(),
            Some(OverflowClip::from_paint_rect(paint_space_rect(
                2.0, 0.0, 4.0, 10.0
            )))
        );
    }

    #[test]
    fn vertical_rl_projection_rebases_source_rows_without_moving_destination_origin() {
        let axes = TableAxes {
            flow: FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
            direction: Direction::Rtl,
        };
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(20.0, 200.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(100.0)),
                LogicalBlockContentSize::new(content_box_pt(300.0)),
            ),
        );
        let destination = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(400.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(100.0)),
                LogicalBlockContentSize::new(content_box_pt(300.0)),
            ),
        );
        let source_slice = TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(10.0), TableGridLength::new(120.0)),
            TableGridSize::from_lengths(TableGridLength::new(50.0), TableGridLength::new(40.0)),
        );
        let destination_slice = TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(10.0), TableGridLength::new(0.0)),
            source_slice.size,
        );
        let fragment_projection = TableGridFragmentProjection::fixture(source, destination);
        let projection = fragment_projection.project_slice(
            source_slice,
            destination_slice,
            TableGridLength::new(0.0),
        );

        assert_eq!(
            projection.destination_clip(),
            destination
                .page_top_rect_for(destination_slice)
                .paint_rect(),
            "source row offsets must not move the destination table origin",
        );
        assert_eq!(
            projection
                .source_to_destination
                .transform_rect(&projection.source_clip()),
            projection.destination_clip(),
            "the logical source row must be rebased exactly once into its destination slice",
        );
    }

    #[test]
    fn vertical_lr_projection_rebases_source_rows_into_the_next_fragmentainer() {
        let axes = TableAxes {
            flow: FlowAxes::new(WritingMode::VerticalLr, Direction::Rtl),
            direction: Direction::Rtl,
        };
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(80.0)),
                LogicalBlockContentSize::new(content_box_pt(240.0)),
            ),
        );
        let destination = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(360.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(80.0)),
                LogicalBlockContentSize::new(content_box_pt(240.0)),
            ),
        );
        let source_slice = TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(0.0), TableGridLength::new(120.0)),
            TableGridSize::from_lengths(TableGridLength::new(80.0), TableGridLength::new(40.0)),
        );
        let destination_slice = TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(0.0), TableGridLength::new(0.0)),
            source_slice.size,
        );
        let fragment_projection = TableGridFragmentProjection::fixture(source, destination);
        let projection = fragment_projection.project_slice(
            source_slice,
            destination_slice,
            TableGridLength::new(0.0),
        );

        assert_eq!(
            projection
                .source_to_destination
                .transform_rect(&projection.source_clip()),
            projection.destination_clip(),
            "a vertical-lr continuation must project the retained source slice once",
        );
        assert_eq!(
            projection.destination_clip(),
            destination
                .page_top_rect_for(destination_slice)
                .paint_rect(),
            "source progress must not move the destination fragmentainer origin",
        );
    }

    #[test]
    fn column_originating_cell_clips_exclude_separated_edge_spacing() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(70.0)),
                LogicalBlockContentSize::new(content_box_pt(20.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0), TableGridLength::new(20.0)],
            TableGridLength::new(10.0),
            vec![false, false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![vec![TableCellPlacement {
                cell: 0,
                column: 0,
                colspan: 1,
                rowspan: 1,
            }]],
            column_count: 2,
        };
        let projection = TableGridFragmentProjection::fixture(source, source);
        let clips = table_column_grid_cell_clips(
            &projection,
            &column_plan,
            &table_grid,
            &[TableRowBounds::new(0.0, 20.0)],
            &[0],
            &[20.0],
            &[0.0],
            0,
            1,
            TableGridLength::new(0.0),
        );

        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].source_clip().origin.x, 110.0);
        assert_eq!(clips[0].destination_clip().origin.x, 110.0);
    }

    #[test]
    fn row_originating_cell_clip_includes_internal_row_span_spacing() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(20.0)),
                LogicalBlockContentSize::new(content_box_pt(50.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0)],
            TableGridLength::new(10.0),
            vec![false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![
                vec![TableCellPlacement {
                    cell: 0,
                    column: 0,
                    colspan: 1,
                    rowspan: 2,
                }],
                Vec::new(),
            ],
            column_count: 1,
        };
        let projection = TableGridFragmentProjection::fixture(source, source);
        let projections = table_structural_originating_cell_projections(
            &projection,
            &[
                TableRowBounds::new(0.0, 20.0),
                TableRowBounds::new(30.0, 20.0),
            ],
            &column_plan,
            &table_grid,
            &[0, 1],
            &[20.0, 20.0],
            &[0.0, 0.0],
            TableStructuralOrigin::Rows { start: 0, end: 1 },
            TableGridLength::new(0.0),
        );

        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].source_clip().height(), 50.0);
        assert_eq!(projections[0].destination_clip().height(), 50.0);
    }

    #[test]
    fn column_background_selects_originating_cells_not_overlapping_cells() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(50.0)),
                LogicalBlockContentSize::new(content_box_pt(40.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0), TableGridLength::new(20.0)],
            TableGridLength::new(10.0),
            vec![false, false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![
                vec![TableCellPlacement {
                    cell: 0,
                    column: 0,
                    colspan: 2,
                    rowspan: 1,
                }],
                vec![TableCellPlacement {
                    cell: 1,
                    column: 1,
                    colspan: 1,
                    rowspan: 1,
                }],
            ],
            column_count: 2,
        };
        let projection = TableGridFragmentProjection::fixture(source, source);
        let projections = table_structural_originating_cell_projections(
            &projection,
            &[
                TableRowBounds::new(0.0, 20.0),
                TableRowBounds::new(20.0, 20.0),
            ],
            &column_plan,
            &table_grid,
            &[0, 1],
            &[20.0, 20.0],
            &[0.0, 0.0],
            TableStructuralOrigin::Columns { start: 1, end: 2 },
            TableGridLength::new(0.0),
        );

        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].source_clip().width(), 20.0);
    }

    #[test]
    fn separate_originating_cells_leave_border_spacing_outside_clips() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(50.0)),
                LogicalBlockContentSize::new(content_box_pt(20.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0), TableGridLength::new(20.0)],
            TableGridLength::new(10.0),
            vec![false, false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![vec![
                TableCellPlacement {
                    cell: 0,
                    column: 0,
                    colspan: 1,
                    rowspan: 1,
                },
                TableCellPlacement {
                    cell: 1,
                    column: 1,
                    colspan: 1,
                    rowspan: 1,
                },
            ]],
            column_count: 2,
        };
        let projection = TableGridFragmentProjection::fixture(source, source);
        let projections = table_structural_originating_cell_projections(
            &projection,
            &[TableRowBounds::new(0.0, 20.0)],
            &column_plan,
            &table_grid,
            &[0],
            &[20.0],
            &[0.0],
            TableStructuralOrigin::Columns { start: 0, end: 2 },
            TableGridLength::new(0.0),
        );

        assert_eq!(projections.len(), 2);
        let first = projections[0].destination_clip();
        let second = projections[1].destination_clip();
        assert_eq!(second.origin.x - (first.origin.x + first.size.width), 10.0);
    }

    #[test]
    fn collapsed_border_geometry_has_no_separated_spacing_between_cells() {
        let axes = TableAxes::for_direction(Direction::Ltr);
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(40.0)),
                LogicalBlockContentSize::new(content_box_pt(20.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0), TableGridLength::new(20.0)],
            TableGridLength::new(0.0),
            vec![false, false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![vec![
                TableCellPlacement {
                    cell: 0,
                    column: 0,
                    colspan: 1,
                    rowspan: 1,
                },
                TableCellPlacement {
                    cell: 1,
                    column: 1,
                    colspan: 1,
                    rowspan: 1,
                },
            ]],
            column_count: 2,
        };
        let projection = TableGridFragmentProjection::fixture(source, source);
        let projections = table_structural_originating_cell_projections(
            &projection,
            &[TableRowBounds::new(0.0, 20.0)],
            &column_plan,
            &table_grid,
            &[0],
            &[20.0],
            &[0.0],
            TableStructuralOrigin::Columns { start: 0, end: 2 },
            TableGridLength::new(0.0),
        );

        assert_eq!(projections.len(), 2);
        let first = projections[0].destination_clip();
        let second = projections[1].destination_clip();
        assert_eq!(second.origin.x, first.origin.x + first.size.width);
    }

    #[test]
    fn vertical_rtl_originating_cell_projection_maps_source_to_destination_once() {
        let axes = TableAxes {
            flow: FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
            direction: Direction::Rtl,
        };
        let source = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(100.0, 500.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(20.0)),
                LogicalBlockContentSize::new(content_box_pt(40.0)),
            ),
        );
        let destination = TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(PageTopPoint::new(300.0, 200.0)),
            axes,
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(20.0)),
                LogicalBlockContentSize::new(content_box_pt(40.0)),
            ),
        );
        let column_plan = TableColumnPlan::with_collapsed(
            vec![TableGridLength::new(20.0)],
            TableGridLength::new(0.0),
            vec![false],
            axes,
        );
        let table_grid = TableGrid {
            rows: vec![vec![TableCellPlacement {
                cell: 0,
                column: 0,
                colspan: 1,
                rowspan: 1,
            }]],
            column_count: 1,
        };
        let fragment_projection = TableGridFragmentProjection::fixture(source, destination);
        let projections = table_structural_originating_cell_projections(
            &fragment_projection,
            &[TableRowBounds::new(0.0, 40.0)],
            &column_plan,
            &table_grid,
            &[0],
            &[40.0],
            &[0.0],
            TableStructuralOrigin::Columns { start: 0, end: 1 },
            TableGridLength::new(0.0),
        );

        assert_eq!(projections.len(), 1);
        let projection = projections[0];
        assert_eq!(
            projection
                .source_to_destination
                .transform_rect(&projection.source_clip()),
            projection.destination_clip()
        );
    }

    #[test]
    fn table_avoid_candidate_does_not_arm_current_row_for_break_before_avoid() {
        let state = TableAvoidBreakCandidateState::default();
        let row_breaks = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::AvoidPage,
            PageBreak::Auto,
            PageBreak::Auto,
        );

        assert!(!state.row_start_may_be_rollback_target(false, false, row_breaks));
    }

    #[test]
    fn table_avoid_candidate_arms_content_row_for_break_after_avoid() {
        let state = TableAvoidBreakCandidateState::default();
        let row_breaks = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::AvoidPage,
            PageBreak::Auto,
        );

        assert!(state.row_start_may_be_rollback_target(false, false, row_breaks));
    }

    #[test]
    fn table_avoid_candidate_scopes_avoid_after_to_fragmentainer_kind() {
        let page_state = TableAvoidBreakCandidateState::new(FragmentainerKind::Page);
        let column_state = TableAvoidBreakCandidateState::new(FragmentainerKind::Column);
        let row_breaks = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::AvoidColumn,
            PageBreak::Auto,
        );

        assert!(!page_state.row_start_may_be_rollback_target(false, false, row_breaks));
        assert!(column_state.row_start_may_be_rollback_target(false, false, row_breaks));
    }

    #[test]
    fn table_repeat_policy_body_capacity_uses_fragmentainer_block_size() {
        let policy = TableFragmentRepeatPolicy {
            repeat_header: true,
            repeat_footer: true,
        };

        assert_eq!(
            policy.body_capacity(layout_pt(100.0), layout_pt(15.0), layout_pt(10.0)),
            layout_pt(75.0)
        );
        assert_eq!(
            policy.body_capacity(layout_pt(20.0), layout_pt(15.0), layout_pt(10.0)),
            layout_pt(0.0)
        );
    }

    #[test]
    fn table_chrome_context_uses_fragmentainer_block_size_for_repeat_policy() {
        let context = TableFragmentChromeContext {
            fragmentainer_block_size: layout_pt(90.0),
            header_height: layout_pt(20.0),
            footer_height: layout_pt(15.0),
            wrapper_chrome: TableWrapperFragmentChrome::none(),
            allow_header: true,
            allow_footer: true,
        };

        let policy = context.repeat_policy(layout_pt(70.0));
        assert!(policy.repeat_header);
        assert!(!policy.repeat_footer);

        let fragmentainer = context.fresh_fragmentainer(policy);
        assert_eq!(fragmentainer.fragmentainer_block_size(), layout_pt(90.0));
        assert_eq!(fragmentainer.body_capacity, layout_pt(70.0));
    }

    #[test]
    fn cloned_wrapper_chrome_reduces_fresh_body_capacity_and_keeps_a_slice_nonzero() {
        let wrapper_chrome = TableWrapperFragmentChrome {
            continuation_block_start: non_content_pt(20.0),
            continuation_block_end: non_content_pt(20.0),
        };
        let context = TableFragmentChromeContext {
            fragmentainer_block_size: layout_pt(100.0),
            header_height: layout_pt(0.0),
            footer_height: layout_pt(0.0),
            wrapper_chrome,
            allow_header: false,
            allow_footer: false,
        };
        let policy = context.repeat_policy(layout_pt(120.0));
        let fresh_fragmentainer = context.fresh_fragmentainer(policy);

        assert_eq!(fresh_fragmentainer.body_capacity, layout_pt(60.0));
        let decision = TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
            remaining_height: 120.0,
            row_required_height: 120.0,
            current_fragmentainer: fresh_fragmentainer,
            chrome_context: context,
            can_advance: false,
        });
        assert_eq!(
            decision.kind,
            TableOversizedRowSliceDecisionKind::PaintSlice
        );
        assert_eq!(decision.piece_height, 60.0);
    }

    #[test]
    fn cloned_wrapper_chrome_truncates_before_returning_zero_body_capacity() {
        let wrapper_chrome = TableWrapperFragmentChrome {
            continuation_block_start: non_content_pt(20.0),
            continuation_block_end: non_content_pt(20.0),
        };

        assert!(
            (wrapper_chrome.fresh_body_capacity(layout_pt(30.0)).points() - 0.01).abs() < 0.001
        );
    }

    #[test]
    fn table_forced_break_decision_preserves_fragmentainer_kind() {
        let decision = TableForcedBreakDecision::choose(TableForcedBreakInput {
            outgoing_repeat_policy: TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: false,
            },
            fragmentainer_kind: FragmentainerKind::Column,
            page_break: PageBreak::Column,
            row_required_height: 40.0,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(10.0),
                footer_height: layout_pt(5.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: true,
                allow_footer: true,
            },
            paint_repeated_footer: false,
        });

        assert_eq!(decision.fragmentainer_kind, FragmentainerKind::Column);
        assert_eq!(decision.page_break, PageBreak::Column);
    }

    #[test]
    fn table_named_page_break_decision_uses_chrome_context() {
        let decision = TableNamedPageBreakDecision::choose(TableNamedPageBreakInput {
            previous_page_end: Some("front".to_string()),
            row_page_start: Some("body".to_string()),
            outgoing_repeat_policy: TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: true,
            },
            row_required_height: 70.0,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(90.0),
                header_height: layout_pt(20.0),
                footer_height: layout_pt(15.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: true,
                allow_footer: true,
            },
            paint_repeated_footer: true,
        })
        .expect("named page change should commit a table fragment transition");

        assert_eq!(decision.page_name.as_deref(), Some("body"));
        assert!(decision.start.repeat_policy.repeat_header);
        assert!(!decision.start.repeat_policy.repeat_footer);
        assert!(decision.start.paint_repeated_header);
        assert_eq!(
            decision.boundary.footer_action,
            TableFragmentFooterAction::PaintRepeated
        );
    }

    #[test]
    fn table_fragment_transition_preserves_fragmentainer_kind() {
        let decision = TableFragmentTransitionDecision::from_input(TableFragmentTransitionInput {
            fragmentainer_kind: FragmentainerKind::Column,
            outgoing_repeat_policy: TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: false,
            },
            footer_action: TableFragmentFooterAction::PaintRepeated,
            break_reason: TableFragmentBreakReason::Overflow,
            incoming_repeat_policy: TableFragmentRepeatPolicy {
                repeat_header: false,
                repeat_footer: true,
            },
            paint_repeated_header: false,
        });

        assert_eq!(decision.fragmentainer_kind, FragmentainerKind::Column);
        assert_eq!(
            decision.boundary.footer_action,
            TableFragmentFooterAction::PaintRepeated
        );
        assert_eq!(
            decision.start.break_reason,
            TableFragmentBreakReason::Overflow
        );
    }

    #[test]
    fn table_fragment_plan_records_fragmentainer_kind() {
        let plan = TableFragmentPlan::new(
            FragmentainerKind::Column,
            3,
            TableFragmentainerPlacement::horizontal(
                PageInlinePosition::new(0.0),
                PageTopBlockPosition::new(120.0),
                LogicalBlockContentSize::new(content_box_pt(100.0)),
            ),
            TableFragmentStartDecision::new(
                TableFragmentBreakReason::Overflow,
                TableFragmentRepeatPolicy {
                    repeat_header: false,
                    repeat_footer: false,
                },
                false,
            ),
        );

        assert_eq!(plan.fragmentainer_kind, FragmentainerKind::Column);
        assert_eq!(plan.page_index, 3);
        assert_eq!(plan.break_reason(), TableFragmentBreakReason::Overflow);
    }

    #[test]
    fn table_fragmentainer_placement_rebases_each_writing_mode_for_next_column() {
        let horizontal = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(72.0),
            PageTopBlockPosition::new(648.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        let horizontal_second_column = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(306.0),
            PageTopBlockPosition::new(648.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        assert_eq!(horizontal.destination_grid_origin().x(), 72.0);
        assert_eq!(horizontal.block_start().points(), 648.0);
        assert_eq!(
            horizontal_second_column.destination_grid_origin().x(),
            306.0
        );

        let vertical_lr = TableFragmentainerPlacement::vertical_lr(
            PageInlinePosition::new(72.0),
            PageTopBlockPosition::new(648.0),
            TableFragmentainerBlockStart::new(-72.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        let vertical_lr_second_column = TableFragmentainerPlacement::vertical_lr(
            PageInlinePosition::new(306.0),
            PageTopBlockPosition::new(648.0),
            TableFragmentainerBlockStart::new(-306.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        )
        .with_wrapper_table_x(PageInlinePosition::new(72.0));
        assert_eq!(vertical_lr.block_start().points(), -72.0);
        assert_eq!(vertical_lr_second_column.block_start().points(), -306.0);
        assert_eq!(
            vertical_lr_second_column.destination_grid_origin().x(),
            306.0
        );
        assert_eq!(vertical_lr_second_column.wrapper_table_x().points(), 72.0);

        let vertical_rl = TableFragmentainerPlacement::vertical_rl(
            PageInlinePosition::new(72.0),
            PageTopBlockPosition::new(648.0),
            TableFragmentainerBlockStart::new(540.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        let vertical_rl_second_column = TableFragmentainerPlacement::vertical_rl(
            PageInlinePosition::new(306.0),
            PageTopBlockPosition::new(648.0),
            TableFragmentainerBlockStart::new(306.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        )
        .with_wrapper_table_x(PageInlinePosition::new(72.0));
        assert_eq!(vertical_rl.block_start().points(), 540.0);
        assert_eq!(vertical_rl_second_column.block_start().points(), 306.0);
        assert_eq!(
            vertical_rl_second_column.destination_grid_origin().x(),
            306.0
        );
        assert_eq!(vertical_rl_second_column.wrapper_table_x().points(), 72.0);
    }

    #[test]
    fn table_root_origin_uses_the_resolved_no_caption_destination() {
        let resolved_destination = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(72.0),
            PageTopBlockPosition::new(648.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        // A normal-flow wrapper cursor may be different after float avoidance
        // or a preceding sibling. No caption means the root still begins at
        // the resolved fragmentainer destination, not at that stale cursor.
        let wrapper_parent_flow_top = PageTopBlockPosition::new(720.0);
        let axes = TableAxes::for_direction(Direction::Ltr);
        let table_width = UsedTableWidth {
            grid_inline: LogicalInlineContentSize::new(content_box_pt(80.0)),
            axes,
            content_width: content_box_pt(80.0),
            border_widths: css::Edges {
                top: 3.0,
                right: 0.0,
                bottom: 0.0,
                left: 5.0,
            },
            padding: css::Edges {
                top: 7.0,
                right: 0.0,
                bottom: 0.0,
                left: 11.0,
            },
        };

        let grid_origin =
            TableWrapperBorderBoxOrigin::new(resolved_destination.destination_grid_origin())
                .grid_content_box_top_left(axes, table_width)
                .page_top_point();

        assert_eq!(grid_origin, PageTopPoint::new(88.0, 638.0));
        assert_ne!(grid_origin.top_y(), wrapper_parent_flow_top.points());
    }

    #[test]
    fn table_fragment_trailing_paint_top_keeps_logical_axes_separate() {
        let inline_span = LogicalInlineContentSize::new(content_box_pt(80.0));
        let horizontal = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(72.0),
            PageTopBlockPosition::new(648.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        assert_eq!(
            horizontal.trailing_paint_top(PageTopBlockPosition::new(512.0), inline_span),
            PageTopBlockPosition::new(512.0),
        );

        for placement in [
            TableFragmentainerPlacement::vertical_lr(
                PageInlinePosition::new(72.0),
                PageTopBlockPosition::new(648.0),
                TableFragmentainerBlockStart::new(-72.0),
                LogicalBlockContentSize::new(content_box_pt(100.0)),
            ),
            TableFragmentainerPlacement::vertical_rl(
                PageInlinePosition::new(72.0),
                PageTopBlockPosition::new(648.0),
                TableFragmentainerBlockStart::new(540.0),
                LogicalBlockContentSize::new(content_box_pt(100.0)),
            ),
            TableFragmentainerPlacement {
                destination_grid_origin: PageTopPoint::new(72.0, 648.0),
                wrapper_table_x: PageInlinePosition::new(72.0),
                block_start: TableFragmentainerBlockStart::new(-72.0),
                block_span: LogicalBlockContentSize::new(content_box_pt(100.0)),
                writing_mode: WritingMode::SidewaysLr,
            },
        ] {
            assert_eq!(
                placement.trailing_paint_top(PageTopBlockPosition::new(512.0), inline_span),
                PageTopBlockPosition::new(568.0),
            );
        }
    }

    #[test]
    fn table_wrapper_margin_footprint_projects_parent_block_end() {
        let footprint = TableWrapperMarginBoxFootprint::from_table_root_border_box(
            PageTopRect::new(30.0, 180.0, 60.0, 80.0),
            PageTopBlockPosition::new(200.0),
            layout_pt(10.0),
            layout_pt(15.0),
            &css::Edges {
                top: 5.0,
                right: 0.0,
                bottom: 7.0,
                left: 0.0,
            },
        );

        assert_eq!(
            footprint.horizontal_parent_block_end(),
            PageTopBlockPosition::new(88.0)
        );
    }

    #[test]
    fn table_avoid_candidate_preserves_next_boundary_across_non_content_row() {
        let state = TableAvoidBreakCandidateState::default();
        let row_breaks = FragmentBreakContext::new(
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::Auto,
            PageBreak::AvoidPage,
        );

        assert!(state.row_start_may_be_rollback_target(true, false, row_breaks));
    }

    #[test]
    fn row_group_avoid_stays_when_group_fits_current_fragmentainer() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            80.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: true,
            },
            10.0,
            10.0,
            true,
        );

        assert_eq!(
            current_fragmentainer.fragmentainer_block_size(),
            layout_pt(100.0)
        );
        assert_eq!(
            current_fragmentainer.available_block_size(),
            layout_pt(80.0)
        );
        assert_eq!(current_fragmentainer.available_body_size(), layout_pt(70.0));
        assert!(
            TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
                group: TableAvoidRowGroup::new(0, 2),
                required_block_size: layout_pt(60.0),
                current_fragmentainer,
                chrome_context: TableFragmentChromeContext {
                    fragmentainer_block_size: layout_pt(100.0),
                    header_height: layout_pt(10.0),
                    footer_height: layout_pt(10.0),
                    wrapper_chrome: TableWrapperFragmentChrome::none(),
                    allow_header: true,
                    allow_footer: true,
                },
                can_advance: true,
            })
            .is_none()
        );
    }

    #[test]
    fn avoided_row_group_requirement_includes_separated_border_edges() {
        let requirement = TableRowGroupFragmentRequirement::from_row_group(
            TableAvoidRowGroup::new(0, 1),
            &[40.0],
            &[true],
            TableMetrics {
                border_collapse: css::BorderCollapse::Separate,
                spacing: css::BorderSpacing::from_lengths(0.0, 3.0),
            },
        );

        assert_eq!(requirement.block_size(), layout_pt(46.0));
    }

    #[test]
    fn avoided_row_group_requirement_excludes_collapsed_or_empty_grid_edges() {
        let collapsed = TableRowGroupFragmentRequirement::from_row_group(
            TableAvoidRowGroup::new(0, 1),
            &[40.0],
            &[true],
            TableMetrics {
                border_collapse: css::BorderCollapse::Collapse,
                spacing: css::BorderSpacing::ZERO,
            },
        );
        let empty = TableRowGroupFragmentRequirement::from_row_group(
            TableAvoidRowGroup::new(0, 1),
            &[40.0],
            &[false],
            TableMetrics {
                border_collapse: css::BorderCollapse::Separate,
                spacing: css::BorderSpacing::from_lengths(0.0, 3.0),
            },
        );

        assert_eq!(collapsed.block_size(), layout_pt(40.0));
        assert_eq!(empty.block_size(), layout_pt(0.0));
    }

    #[test]
    fn row_group_avoid_moves_to_next_fragment_with_repeated_chrome() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            40.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: true,
            },
            10.0,
            10.0,
            true,
        );
        let decision = TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
            group: TableAvoidRowGroup::new(0, 2),
            required_block_size: layout_pt(80.0),
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(10.0),
                footer_height: layout_pt(10.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: true,
                allow_footer: true,
            },
            can_advance: true,
        })
        .expect("row group should fit a fresh fragmentainer with repeats");

        assert_eq!(decision.mode, TableRowGroupAvoidMode::FitsNextFragment);
        assert!(decision.repeat_policy.repeat_header);
        assert!(decision.repeat_policy.repeat_footer);
    }

    #[test]
    fn row_group_avoid_can_suppress_chrome_for_bounded_overflow() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            40.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: true,
            },
            20.0,
            20.0,
            true,
        );
        let decision = TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
            group: TableAvoidRowGroup::new(0, 2),
            required_block_size: layout_pt(101.0),
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(20.0),
                footer_height: layout_pt(20.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: true,
                allow_footer: true,
            },
            can_advance: true,
        })
        .expect("row group should be kept by bounded chrome overflow");

        assert_eq!(decision.mode, TableRowGroupAvoidMode::KeptByChromeOverflow);
        assert!(!decision.repeat_policy.repeat_header);
        assert!(!decision.repeat_policy.repeat_footer);
    }

    #[test]
    fn row_group_avoid_stays_when_fragmentainer_cannot_advance() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            40.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: true,
                repeat_footer: true,
            },
            10.0,
            10.0,
            true,
        );

        assert!(
            TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
                group: TableAvoidRowGroup::new(0, 2),
                required_block_size: layout_pt(80.0),
                current_fragmentainer,
                chrome_context: TableFragmentChromeContext {
                    fragmentainer_block_size: layout_pt(100.0),
                    header_height: layout_pt(10.0),
                    footer_height: layout_pt(10.0),
                    wrapper_chrome: TableWrapperFragmentChrome::none(),
                    allow_header: true,
                    allow_footer: true,
                },
                can_advance: false,
            })
            .is_none()
        );
    }

    #[test]
    fn oversized_row_slice_advances_when_empty_body_can_advance() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            0.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: false,
                repeat_footer: false,
            },
            0.0,
            0.0,
            false,
        );
        let decision = TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
            remaining_height: 120.0,
            row_required_height: 0.01,
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(0.0),
                footer_height: layout_pt(0.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: false,
                allow_footer: false,
            },
            can_advance: true,
        });

        assert_eq!(
            decision.kind,
            TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice
        );
        assert_eq!(decision.piece_height, 0.0);
    }

    #[test]
    fn zero_child_boundary_overflows_when_a_fresh_fragmentainer_is_not_larger() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            100.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: false,
                repeat_footer: false,
            },
            0.0,
            0.0,
            false,
        );
        let decision = TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
            remaining_height: 120.0,
            row_required_height: 120.0,
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(0.0),
                footer_height: layout_pt(0.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: false,
                allow_footer: false,
            },
            can_advance: true,
        })
        .at_child_boundary(0.0);

        assert_eq!(
            decision.kind,
            TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice
        );
        assert!(decision.needs_unfragmented_overflow(100.0));

        let overflow = decision.as_unfragmented_overflow(100.0);
        assert!(overflow.paints_slice());
        assert!(overflow.is_unfragmented_overflow());
        assert_eq!(overflow.piece_height, 120.0);
        assert!(!overflow.continues_after_slice());
    }

    #[test]
    fn repeated_chrome_with_zero_body_capacity_overflows_in_place() {
        let chrome_context = TableFragmentChromeContext {
            fragmentainer_block_size: layout_pt(20.0),
            header_height: layout_pt(15.0),
            footer_height: layout_pt(10.0),
            wrapper_chrome: TableWrapperFragmentChrome::none(),
            allow_header: true,
            allow_footer: true,
        };
        let repeat_policy = TableFragmentRepeatPolicy {
            repeat_header: true,
            repeat_footer: true,
        };
        let next_body_capacity = chrome_context
            .fresh_fragmentainer(repeat_policy)
            .body_capacity
            .points();
        assert_eq!(next_body_capacity, 0.0);

        let decision = TableOversizedRowSliceDecision {
            kind: TableOversizedRowSliceDecisionKind::AdvanceBeforeSlice,
            remaining_height: 40.0,
            available_body_size: 0.0,
            piece_height: 0.0,
            incoming_repeat_policy: repeat_policy,
        };
        assert!(decision.needs_unfragmented_overflow(next_body_capacity));
        assert!(
            decision
                .as_unfragmented_overflow(next_body_capacity)
                .is_unfragmented_overflow()
        );
    }

    #[test]
    fn oversized_row_slice_uses_body_capacity_at_fragment_start() {
        let current_fragmentainer = current_fragmentainer(
            50.0,
            120.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: false,
                repeat_footer: false,
            },
            0.0,
            0.0,
            false,
        );
        let decision = TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
            remaining_height: 120.0,
            row_required_height: 120.0,
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(50.0),
                header_height: layout_pt(0.0),
                footer_height: layout_pt(0.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: false,
                allow_footer: false,
            },
            can_advance: false,
        });

        assert_eq!(
            decision.kind,
            TableOversizedRowSliceDecisionKind::PaintSlice
        );
        assert_eq!(decision.available_body_size, 50.0);
        assert_eq!(decision.piece_height, 50.0);
    }

    #[test]
    fn oversized_row_slice_paints_when_empty_body_cannot_advance() {
        let current_fragmentainer = current_fragmentainer(
            100.0,
            0.0,
            0.0,
            TableFragmentRepeatPolicy {
                repeat_header: false,
                repeat_footer: false,
            },
            0.0,
            0.0,
            false,
        );
        let decision = TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
            remaining_height: 120.0,
            row_required_height: 0.01,
            current_fragmentainer,
            chrome_context: TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(100.0),
                header_height: layout_pt(0.0),
                footer_height: layout_pt(0.0),
                wrapper_chrome: TableWrapperFragmentChrome::none(),
                allow_header: false,
                allow_footer: false,
            },
            can_advance: false,
        });

        assert_eq!(
            decision.kind,
            TableOversizedRowSliceDecisionKind::PaintSlice
        );
        assert_eq!(decision.piece_height, 120.0);
    }

    fn projection_placement(
        writing_mode: WritingMode,
        direction: Direction,
        origin: PageTopPoint,
    ) -> TableGridPlacement {
        TableGridPlacement::with_axes(
            TableGridContentBoxTopLeft::new(origin),
            TableAxes {
                flow: FlowAxes::new(writing_mode, direction),
                direction,
            },
            TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(80.0)),
                LogicalBlockContentSize::new(content_box_pt(120.0)),
            ),
        )
    }

    fn wrapper_viewport_box(
        writing_mode: WritingMode,
        direction: Direction,
        table_x: f32,
        top: f32,
    ) -> TableWrapperPaintBox {
        TableWrapperPaintBox {
            grid_origin: TableGridContentBoxTopLeft::new(PageTopPoint::new(table_x, top)),
            axes: TableAxes {
                flow: FlowAxes::new(writing_mode, direction),
                direction,
            },
            grid_size: TableGridLogicalSize::new(
                LogicalInlineContentSize::new(content_box_pt(80.0)),
                LogicalBlockContentSize::new(content_box_pt(120.0)),
            ),
            table_width: UsedTableWidth {
                grid_inline: LogicalInlineContentSize::new(content_box_pt(80.0)),
                axes: TableAxes {
                    flow: FlowAxes::new(writing_mode, direction),
                    direction,
                },
                content_width: content_box_pt(80.0),
                border_widths: css::Edges::ZERO,
                padding: css::Edges::ZERO,
            },
            table_metrics: TableMetrics {
                border_collapse: css::BorderCollapse::Separate,
                spacing: css::BorderSpacing::ZERO,
            },
            block_edge_spacing: TableGridLength::new(0.0),
        }
    }

    #[test]
    fn table_root_border_origin_consumes_asymmetric_chrome_in_every_writing_mode() {
        let border_box_top_left = TableWrapperBorderBoxOrigin::new(PageTopPoint::new(30.0, 240.0));

        for writing_mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            let axes = TableAxes {
                flow: FlowAxes::new(writing_mode, Direction::Ltr),
                direction: Direction::Ltr,
            };
            let table_width = UsedTableWidth {
                grid_inline: LogicalInlineContentSize::new(content_box_pt(80.0)),
                axes,
                content_width: content_box_pt(80.0),
                border_widths: css::Edges {
                    top: 3.0,
                    right: 5.0,
                    bottom: 7.0,
                    left: 11.0,
                },
                padding: css::Edges {
                    top: 13.0,
                    right: 17.0,
                    bottom: 19.0,
                    left: 23.0,
                },
            };

            assert_eq!(
                border_box_top_left
                    .grid_content_box_top_left(axes, table_width)
                    .page_top_point(),
                PageTopPoint::new(64.0, 224.0),
                "{writing_mode:?} must consume the physical root chrome before grid projection",
            );
        }
    }

    #[test]
    fn vertical_grid_paint_entry_uses_content_edge_after_root_chrome_projection() {
        let border_box_origin = TableWrapperBorderBoxOrigin::new(PageTopPoint::new(30.0, 240.0));
        for writing_mode in [
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
            WritingMode::SidewaysLr,
            WritingMode::SidewaysRl,
        ] {
            let axes = TableAxes {
                flow: FlowAxes::new(writing_mode, Direction::Ltr),
                direction: Direction::Ltr,
            };
            let table_width = UsedTableWidth {
                grid_inline: LogicalInlineContentSize::new(content_box_pt(80.0)),
                axes,
                content_width: content_box_pt(80.0),
                border_widths: css::Edges {
                    top: 3.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                },
                padding: css::Edges {
                    top: 7.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                },
            };
            let paint_box = TableWrapperPaintBox {
                grid_origin: border_box_origin.grid_content_box_top_left(axes, table_width),
                axes,
                grid_size: TableGridLogicalSize::new(
                    LogicalInlineContentSize::new(content_box_pt(80.0)),
                    LogicalBlockContentSize::new(content_box_pt(120.0)),
                ),
                table_width,
                table_metrics: TableMetrics {
                    border_collapse: css::BorderCollapse::Separate,
                    spacing: css::BorderSpacing::ZERO,
                },
                block_edge_spacing: TableGridLength::new(0.0),
            };
            let grid_content_top =
                PageTopBlockPosition::new(paint_box.clone().grid_content_box().top_y());
            let border_box_top = PageTopBlockPosition::new(paint_box.clone().border_box().top_y());
            let grid_paint_top = paint_box.initial_destination_grid_paint_top();

            assert_eq!(
                grid_paint_top, grid_content_top,
                "{writing_mode:?} must not reapply its physical top root chrome",
            );
            assert_ne!(
                grid_paint_top, border_box_top,
                "{writing_mode:?} has non-zero top chrome in this regression fixture",
            );
        }
    }

    #[test]
    fn wrapper_timeline_records_committed_grid_slices() {
        let wrapper = wrapper_viewport_box(WritingMode::HorizontalTb, Direction::Ltr, 120.0, 160.0);
        let viewport = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(120.0),
            PageTopBlockPosition::new(160.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        viewport.record_top_caption_progress(
            TableGridLength::new(0.0),
            destination,
            wrapper.grid_placement(),
            TableRootBlockStartChrome::new(TableGridLength::new(0.0)),
        );

        assert_eq!(
            viewport
                .initial_destination_grid_placement()
                .full_page_top_rect()
                .top_y(),
            160.0
        );
        viewport.record_grid_body_slice(
            destination,
            0,
            TableGridBlockOffset::new(TableGridLength::new(45.0)),
            TableGridLength::new(50.0),
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
        );
        assert_eq!(
            viewport
                .grid_body_slices_for(destination, 0)
                .into_iter()
                .next()
                .unwrap()
                .grid_source_start
                .unwrap(),
            TableGridBlockOffset::new(TableGridLength::new(45.0)),
        );
    }

    #[test]
    fn wrapper_timeline_projects_progress_through_vertical_grid_axes() {
        let wrapper = wrapper_viewport_box(WritingMode::VerticalRl, Direction::Rtl, 120.0, 80.0);
        let viewport = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::vertical_rl(
            PageInlinePosition::new(120.0),
            PageTopBlockPosition::new(80.0),
            TableFragmentainerBlockStart::new(200.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        viewport.record_top_caption_progress(
            TableGridLength::new(0.0),
            destination,
            wrapper.grid_placement(),
            TableRootBlockStartChrome::new(TableGridLength::new(0.0)),
        );

        assert_eq!(
            viewport
                .initial_destination_grid_placement()
                .full_page_top_rect()
                .x(),
            120.0
        );
    }

    #[test]
    fn caption_outcome_preserves_authoritative_vertical_destination_tracks() {
        // A table grid must receive the exact destination selected by the
        // caption consumer. These cover a remaining track, an exhausted
        // track requiring a successor, and a post-break track in both
        // vertical block directions.
        for (destination, requires_successor) in [
            (
                TableFragmentainerPlacement::vertical_rl(
                    PageInlinePosition::new(120.0),
                    PageTopBlockPosition::new(80.0),
                    TableFragmentainerBlockStart::new(170.0),
                    LogicalBlockContentSize::new(content_box_pt(50.0)),
                ),
                false,
            ),
            (
                TableFragmentainerPlacement::vertical_rl(
                    PageInlinePosition::new(120.0),
                    PageTopBlockPosition::new(80.0),
                    TableFragmentainerBlockStart::new(120.0),
                    LogicalBlockContentSize::new(content_box_pt(0.0)),
                ),
                true,
            ),
            (
                TableFragmentainerPlacement::vertical_lr(
                    PageInlinePosition::new(35.0),
                    PageTopBlockPosition::new(80.0),
                    TableFragmentainerBlockStart::new(-35.0),
                    LogicalBlockContentSize::new(content_box_pt(50.0)),
                ),
                false,
            ),
        ] {
            let outcome = TableCaptionLayoutOutcome::new(
                destination,
                Vec::new(),
                TableWrapperBlockInterval::new(
                    TableWrapperBlockOffset::zero(),
                    TableGridLength::new(50.0),
                ),
                requires_successor,
            );

            assert_eq!(outcome.final_destination(), destination);
            assert_eq!(outcome.next_part_requires_successor(), requires_successor);
        }
    }

    #[test]
    fn table_root_decoration_translation_uses_grid_source_progress_only() {
        let progress = TableGridBlockOffset::new(TableGridLength::new(37.5));

        assert_eq!(
            table_grid_source_progress_translation(WritingMode::HorizontalTb, progress),
            PaintTranslation::new(0.0, 37.5),
        );
        assert_eq!(
            table_grid_source_progress_translation(WritingMode::VerticalLr, progress),
            PaintTranslation::new(37.5, 0.0),
        );
        assert_eq!(
            table_grid_source_progress_translation(WritingMode::VerticalRl, progress),
            PaintTranslation::new(-37.5, 0.0),
        );
    }

    #[test]
    fn wrapper_timeline_keeps_caption_and_grid_source_offsets_distinct() {
        let wrapper = wrapper_viewport_box(WritingMode::VerticalLr, Direction::Rtl, 120.0, 80.0);
        let timeline = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::vertical_lr(
            PageInlinePosition::new(120.0),
            PageTopBlockPosition::new(80.0),
            TableFragmentainerBlockStart::new(-220.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        timeline.record_top_caption_progress(
            TableGridLength::new(140.0),
            destination,
            wrapper.grid_placement(),
            TableRootBlockStartChrome::new(TableGridLength::new(10.0)),
        );
        timeline.record_grid_body_slice(
            destination,
            0,
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
            TableGridLength::new(225.0),
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
        );
        timeline.record_grid_end_chrome(
            TableGridLength::new(225.0),
            TableGridLength::new(10.0),
            destination,
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
        );
        timeline.record_bottom_caption_progress(
            TableGridLength::new(225.0),
            TableGridLength::new(10.0),
            TableGridLength::new(15.0),
            destination,
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
        );

        let state = timeline.state.borrow();
        assert_eq!(state.slices.len(), 5);
        assert_eq!(state.slices[0].kind, TableWrapperTimelineKind::TopCaption);
        assert_eq!(
            state.slices[1].kind,
            TableWrapperTimelineKind::GridStartChrome
        );
        assert_eq!(state.slices[2].kind, TableWrapperTimelineKind::GridBody);
        assert_eq!(
            state.slices[3].kind,
            TableWrapperTimelineKind::GridEndChrome
        );
        assert_eq!(
            state.slices[4].kind,
            TableWrapperTimelineKind::BottomCaption
        );
        assert_eq!(state.slices[0].grid_source_start, None);
        assert_eq!(
            state.slices[2]
                .grid_source_start
                .map(|offset| offset.length().get()),
            Some(0.0)
        );
        assert_eq!(state.slices[4].grid_source_start, None);
        assert_eq!(state.slices[4].source.start.0.get(), 385.0);
    }

    #[test]
    fn wrapper_timeline_keeps_each_vertical_caption_destination_before_grid_source() {
        let timeline = TableWrapperFragmentTimeline::new();
        let wrapper = wrapper_viewport_box(WritingMode::VerticalLr, Direction::Rtl, 20.0, 300.0);
        let first_destination = TableFragmentainerPlacement::vertical_lr(
            PageInlinePosition::new(20.0),
            PageTopBlockPosition::new(300.0),
            TableFragmentainerBlockStart::new(-20.0),
            LogicalBlockContentSize::new(content_box_pt(25.0)),
        );
        let second_destination = TableFragmentainerPlacement::vertical_rl(
            PageInlinePosition::new(45.0),
            PageTopBlockPosition::new(300.0),
            TableFragmentainerBlockStart::new(45.0),
            LogicalBlockContentSize::new(content_box_pt(25.0)),
        );
        let context = PageContext {
            size: PageSize::from_points(400.0, 400.0),
            margins: PageMargins::all_points(0.0),
            edges: PageBoxEdges::ZERO,
            rotation: 0,
        };
        let caption_slices = [
            TableCaptionPaintSlice {
                page_index: 0,
                source_block_start: layout_pt(0.0),
                block_size: layout_pt(25.0),
                destination: first_destination,
                destination_context: context,
                destination_origin: PageTopPoint::new(context.left(), context.top()),
                destination_extent: LogicalSize {
                    inline: 100.0,
                    block: 25.0,
                },
                destination_block_start: layout_pt(0.0),
            },
            TableCaptionPaintSlice {
                page_index: 1,
                source_block_start: layout_pt(25.0),
                block_size: layout_pt(25.0),
                destination: second_destination,
                destination_context: context,
                destination_origin: PageTopPoint::new(context.left(), context.top()),
                destination_extent: LogicalSize {
                    inline: 100.0,
                    block: 25.0,
                },
                destination_block_start: layout_pt(0.0),
            },
        ];
        timeline.record_top_caption_slices(
            &caption_slices,
            TableGridLength::new(50.0),
            second_destination,
            wrapper.grid_placement(),
            TableRootBlockStartChrome::new(TableGridLength::new(10.0)),
        );
        timeline.record_grid_body_slice(
            second_destination,
            0,
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
            TableGridLength::new(20.0),
            TableGridBlockOffset::new(TableGridLength::new(0.0)),
        );

        let state = timeline.state.borrow();
        assert_eq!(state.slices.len(), 4);
        assert_eq!(state.slices[0].destination, first_destination);
        assert_eq!(state.slices[1].destination, second_destination);
        assert_eq!(state.slices[0].source.start().points(), 0.0);
        assert_eq!(state.slices[1].source.start().points(), 25.0);
        assert_eq!(
            state.slices[2].kind,
            TableWrapperTimelineKind::GridStartChrome
        );
        assert_eq!(state.slices[3].kind, TableWrapperTimelineKind::GridBody);
        assert_eq!(
            state.slices[3].grid_source_start,
            Some(TableGridBlockOffset::new(TableGridLength::new(0.0)))
        );
    }

    fn wrapper_root_source_rect(block_start: f32, block_span: f32) -> TableGridRect {
        TableGridRect::new(
            TableGridPoint::from_lengths(
                TableGridLength::new(0.0),
                TableGridLength::new(block_start),
            ),
            TableGridSize::from_lengths(
                TableGridLength::new(100.0),
                TableGridLength::new(block_span),
            ),
        )
    }

    #[test]
    fn wrapper_root_source_frame_starts_before_block_start_chrome() {
        let timeline = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(0.0),
            PageTopBlockPosition::new(100.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        timeline.record_top_caption_progress(
            TableGridLength::new(0.0),
            destination,
            projection_placement(
                WritingMode::HorizontalTb,
                Direction::Ltr,
                PageTopPoint::new(0.0, 100.0),
            ),
            TableRootBlockStartChrome::new(TableGridLength::new(10.0)),
        );

        let root = timeline.root_source_frame(wrapper_root_source_rect(-10.0, 400.0));
        let grid_start = timeline
            .state
            .borrow()
            .grid_start
            .expect("grid start is committed");

        assert_eq!(root.local_block_start().points(), 0.0);
        assert_eq!(root.block_span().get(), 400.0);
        assert_eq!(grid_start.grid_content_start.points(), 10.0);
    }

    #[test]
    fn wrapper_root_source_frame_keeps_caption_progress_outside_start_chrome() {
        let timeline = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::horizontal(
            PageInlinePosition::new(0.0),
            PageTopBlockPosition::new(100.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        timeline.record_top_caption_progress(
            TableGridLength::new(140.0),
            destination,
            projection_placement(
                WritingMode::HorizontalTb,
                Direction::Ltr,
                PageTopPoint::new(0.0, 100.0),
            ),
            TableRootBlockStartChrome::new(TableGridLength::new(10.0)),
        );

        let root = timeline.root_source_frame(wrapper_root_source_rect(-10.0, 245.0));
        let grid_start = timeline
            .state
            .borrow()
            .grid_start
            .expect("grid start is committed");

        assert_eq!(root.local_block_start().points(), 140.0);
        assert_eq!(root.block_span().get(), 245.0);
        assert_eq!(grid_start.grid_content_start.points(), 150.0);
    }

    #[test]
    fn wrapper_root_source_frame_uses_logical_progress_for_vertical_tables() {
        let timeline = TableWrapperFragmentTimeline::new();
        let destination = TableFragmentainerPlacement::vertical_rl(
            PageInlinePosition::new(120.0),
            PageTopBlockPosition::new(80.0),
            TableFragmentainerBlockStart::new(-220.0),
            LogicalBlockContentSize::new(content_box_pt(100.0)),
        );
        timeline.record_top_caption_progress(
            TableGridLength::new(140.0),
            destination,
            projection_placement(
                WritingMode::VerticalRl,
                Direction::Rtl,
                PageTopPoint::new(120.0, 80.0),
            ),
            TableRootBlockStartChrome::new(TableGridLength::new(10.0)),
        );

        let root = timeline.root_source_frame(wrapper_root_source_rect(-10.0, 245.0));

        assert_eq!(root.local_block_start().points(), 140.0);
        assert_eq!(root.block_span().get(), 245.0);
    }

    #[test]
    fn source_grid_projection_keeps_logical_slices_separate_from_destinations() {
        let source_rect = TableGridRect::new(
            TableGridPoint::from_lengths(TableGridLength::new(10.0), TableGridLength::new(30.0)),
            TableGridSize::from_lengths(TableGridLength::new(20.0), TableGridLength::new(40.0)),
        );

        for (writing_mode, direction) in [
            (WritingMode::HorizontalTb, Direction::Ltr),
            (WritingMode::VerticalLr, Direction::Rtl),
            (WritingMode::VerticalRl, Direction::Rtl),
        ] {
            let source =
                projection_placement(writing_mode, direction, PageTopPoint::new(20.0, 180.0));
            let destination =
                projection_placement(writing_mode, direction, PageTopPoint::new(220.0, 80.0));
            let source_physical = source.page_top_rect_for(source_rect);
            let destination_physical = destination.page_top_rect_for(source_rect);

            // A logical source slice has exactly one physical projection per
            // destination viewport. Its extent is invariant; only the typed
            // page placement changes.
            assert_eq!(source_physical.width(), destination_physical.width());
            assert_eq!(source_physical.height(), destination_physical.height());
            assert_eq!(destination_physical.x() - source_physical.x(), 200.0);
            assert_eq!(
                destination_physical.top_y() - source_physical.top_y(),
                -100.0
            );
        }
    }

    #[test]
    fn wrapper_margin_footprint_includes_caption_space_and_margins() {
        let table_root_border_box = PageTopRect::new(24.0, 190.0, 80.0, 40.0);
        let wrapper = TableWrapperMarginBoxFootprint::from_table_root_border_box(
            table_root_border_box,
            PageTopBlockPosition::new(200.0),
            layout_pt(10.0),
            layout_pt(15.0),
            &css::Edges {
                top: 4.0,
                right: 5.0,
                bottom: 6.0,
                left: 7.0,
            },
        )
        .page_top_rect();

        assert_eq!(table_root_border_box.x(), 24.0);
        assert_eq!(table_root_border_box.top_y(), 190.0);
        assert_eq!(table_root_border_box.width(), 80.0);
        assert_eq!(table_root_border_box.height(), 40.0);
        assert_eq!(wrapper.x(), 17.0);
        assert_eq!(wrapper.top_y(), 204.0);
        assert_eq!(wrapper.width(), 92.0);
        assert_eq!(wrapper.height(), 75.0);
    }
}
