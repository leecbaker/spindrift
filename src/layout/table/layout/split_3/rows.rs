use super::*;

/// Geometry consumed at the start of one destination table fragment.
///
/// A table's page transition bypasses ordinary block-flow re-entry, so the
/// table owns this fragment-local placement explicitly. In particular,
/// separated-border leading edge spacing is consumed once, before either a
/// repeated header or the first source row.
/// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy)]
struct TableFragmentStartPlacement {
    table_x: f32,
    destination_cursor: PageTopBlockPosition,
    wrapper_block_start: NonContentLength,
}

impl TableFragmentStartPlacement {
    fn cursor_for_start(self, has_repeated_header: bool) -> f32 {
        if has_repeated_header {
            self.destination_cursor.points()
        } else {
            self.destination_cursor.points() - self.wrapper_block_start.points()
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::table) fn layout_table_body_rows(
        &mut self,
        input: TableBodyRowsInput<'_, '_>,
    ) -> (
        Option<TableBodyPaintFragment>,
        PageBreak,
        TableFragmentRepeatPolicy,
        TableContinuationInlineOffset,
    ) {
        let TableBodyRowsInput {
            fragmentainer_kind,
            rows,
            grid,
            columns,
            style,
            stylesheets,
            table_x,
            source_grid_placement,
            logical_inline_extent,
            physical_grid_width,
            table_cellpadding,
            column_plan,
            planned_row_heights,
            source_row_heights,
            planned_row_occupancy,
            table_height_is_definite,
            table_width,
            table_metrics,
            collapsed_geometry,
            table_is_document_canvas,
            repeating_header_rows,
            repeating_footer_rows,
            repeating_header_height,
            repeating_footer_height,
            avoid_break_row_groups,
            row_group_break_before,
            row_group_break_after,
            ..
        } = input;
        let physical_grid_width_points = physical_grid_width.points();
        debug_assert!(
            (logical_inline_extent.points() - column_plan.total_width().points()).abs() <= 0.01,
            "body rows must retain the column plan's logical inline extent"
        );
        let mut table_x = table_x;
        let continuation_inline_offset =
            TableContinuationInlineOffset::capture(table_x, self.content_left);
        let mut table_body_fragment_started = false;
        let mut table_body_fragment: Option<TableBodyPaintFragment> = None;
        let mut current_fragment_repeat_policy = table_fragment_repeat_policy(
            layout_pt(0.01),
            layout_pt(self.page_area_height()),
            layout_pt(0.0),
            layout_pt(repeating_footer_height),
            false,
            true,
        );
        let mut pending_fragment_start = TableFragmentStartDecision::new(
            TableFragmentBreakReason::TableStart,
            current_fragment_repeat_policy,
            false,
        );
        let mut forced_break_carry = TableForcedBreakCarryState::new(fragmentainer_kind);
        let mut avoid_break_candidates = TableAvoidBreakCandidateState::new(fragmentainer_kind);
        let mut previous_row_page_end: Option<Option<String>> = None;
        let mut avoid_row_group_keep_state = TableAvoidRowGroupKeepState::default();
        let row_group_end_indices = table_row_group_end_indices(rows);
        // The table grid starts after its top edge spacing and must leave its
        // matching bottom edge spacing, padding, and border before the table
        // can end. Reserve that trailing non-content when deciding whether the
        // final row fits; otherwise a final row may be accepted only to place
        // the table's closing edge outside the fragmentainer.
        // <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
        let trailing_table_non_content = non_content_pt(
            table_vertical_edge_spacing(planned_row_occupancy, table_metrics.clone())
                + table_width.padding.bottom
                + table_width.border_widths.bottom,
        );
        let wrapper_chrome = TableWrapperFragmentChrome::for_table(style, table_width);
        let mut fragment_commit_context = TableBodyFragmentCommitContext {
            rows,
            grid,
            columns,
            style,
            stylesheets,
            table_x,
            continuation_inline_offset,
            logical_inline_extent,
            physical_grid_width,
            table_cellpadding,
            column_plan,
            planned_row_heights,
            planned_row_occupancy,
            table_width,
            table_metrics: table_metrics.clone(),
            collapsed_geometry,
            table_is_document_canvas,
            repeating_header_rows,
            repeating_footer_rows,
        };
        let mut row_index = 0usize;
        while row_index < rows.len() {
            let row = &rows[row_index];
            let row_style = self.style_for_table_row(row, style, stylesheets);
            let row_is_repeating_header = repeating_header_rows.contains(&row_index);
            let row_is_repeating_footer = repeating_footer_rows.contains(&row_index);
            let row_height = planned_row_heights[row_index];
            let row_is_running = row_style.position.is_running();
            let row_collapsed = table_row_is_collapsed(&row_style);
            let row_chrome_context = TableFragmentChromeContext {
                fragmentainer_block_size: layout_pt(self.page_area_height()),
                header_height: layout_pt(repeating_header_height),
                footer_height: layout_pt(repeating_footer_height),
                wrapper_chrome,
                allow_header: !row_is_repeating_header,
                allow_footer: !row_is_repeating_footer,
            };
            let row_fragment_required_height = if row_height > self.page_area_height() + 0.01 {
                0.01
            } else {
                row_height
                    + if row_index + 1 == rows.len() && !row_is_repeating_header {
                        trailing_table_non_content.points()
                    } else {
                        0.0
                    }
            };
            let row_page_values = if row_is_running {
                ResolvedPageBoundaryValues {
                    start: None,
                    end: None,
                }
            } else {
                self.table_row_page_boundary_values(
                    row_index,
                    row_group_end_indices[row_index],
                    row,
                    &row_style,
                    style,
                    stylesheets,
                )
            };
            let row_page_start = row_page_values.start;
            let row_page_end = row_page_values.end;
            // A table's first source row is also a class-A page boundary.
            // Without a preceding row there is no `previous_row_page_end` to
            // trigger the normal transition below, but a row-group's `page`
            // value still selects the page box on which that first row (and
            // any repeated copy) is laid out.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            if previous_row_page_end.is_none()
                && !self.current_page_has_content()
                && row_page_start != self.active_page_value_scope(style)
            {
                self.switch_page_name_at_class_a_boundary(row_page_start.as_deref());
                fragment_commit_context.rebase_to_content_left(self.content_left);
                table_x = fragment_commit_context.table_x;
            }
            if let Some(previous_page_end) = previous_row_page_end.clone()
                && let Some(named_page_break) =
                    TableNamedPageBreakDecision::choose(TableNamedPageBreakInput {
                        previous_page_end,
                        row_page_start: row_page_start.clone(),
                        outgoing_repeat_policy: current_fragment_repeat_policy,
                        row_required_height: row_fragment_required_height,
                        chrome_context: row_chrome_context,
                        paint_repeated_footer: !row_is_repeating_footer
                            && !self.cursor_is_at_page_top(),
                    })
            {
                self.commit_table_body_fragment_boundary(
                    &mut table_body_fragment,
                    &fragment_commit_context,
                    named_page_break.boundary,
                );
                self.switch_page_name_at_class_a_boundary(named_page_break.page_name.as_deref());
                fragment_commit_context.rebase_to_content_left(self.content_left);
                table_x = fragment_commit_context.table_x;
                current_fragment_repeat_policy = named_page_break.start.repeat_policy;
                pending_fragment_start = named_page_break.start;
            }
            if !row_is_running {
                previous_row_page_end = Some(row_page_end);
            }
            let break_before = Self::effective_table_row_break_before(
                row_index,
                &row_style,
                row_group_break_before,
            );
            let break_after =
                Self::effective_table_row_break_after(row_index, &row_style, row_group_break_after);
            let next_break_before = if row_index + 1 < rows.len() {
                let next_row_style =
                    self.style_for_table_row(&rows[row_index + 1], style, stylesheets);
                Self::effective_table_row_break_before(
                    row_index + 1,
                    &next_row_style,
                    row_group_break_before,
                )
            } else {
                PageBreak::Auto
            };
            let row_breaks =
                forced_break_carry.take_box_context(break_before, break_after, next_break_before);
            let forced_break_before = row_breaks.forced_break_before_in(fragmentainer_kind);
            let mut broke_before_row = forced_break_before.is_some();
            let pending_row_start_candidate = PendingTableBreakCandidate {
                meta: TableBreakCandidateMeta {
                    row_index,
                    table_body_fragment: table_body_fragment.clone(),
                    repeat_policy: current_fragment_repeat_policy,
                    height: 0.0,
                },
            };
            let row_start_candidate = avoid_break_candidates
                .row_start_may_be_rollback_target(row_collapsed, row_is_running, row_breaks)
                .then(|| pending_row_start_candidate.arm(self));
            let mut start_chrome_replayed_after_break = false;
            if let Some(page_break) = forced_break_before {
                let forced_break = Self::table_body_forced_break(
                    current_fragment_repeat_policy,
                    fragmentainer_kind,
                    page_break,
                    row_fragment_required_height,
                    row_chrome_context,
                    !row_is_repeating_footer && !self.cursor_is_at_page_top(),
                );
                current_fragment_repeat_policy = forced_break.start.repeat_policy;
                pending_fragment_start = forced_break.start;
                self.apply_table_body_forced_break(
                    &mut table_body_fragment,
                    &mut fragment_commit_context,
                    forced_break,
                );
                table_x = fragment_commit_context.table_x;
            }
            // Rows and row groups are both table fragmentation containers.
            // Represent a row-level `break-inside: avoid` as a one-row range
            // so it shares the measured keep-together decision used for an
            // authored row group rather than relying on the later generic row
            // overflow path.
            // <https://www.w3.org/TR/css-break-3/#break-within>
            let authored_avoid_row_group = avoid_break_row_groups
                .iter()
                .find(|avoid_row_group| avoid_row_group.start == row_index)
                .copied();
            let row_level_avoid = authored_avoid_row_group.is_none()
                && fragmentainer_kind.avoids_break_inside(&row_style);
            let avoid_row_group = authored_avoid_row_group.or_else(|| {
                row_level_avoid.then(|| TableAvoidRowGroup::new(row_index, row_index + 1))
            });
            let mut row_requires_avoid_group_slice = false;
            if let Some(avoid_row_group) = avoid_row_group {
                let authored_group_has_forced_internal_break = authored_avoid_row_group
                    .is_some_and(|group| {
                        (group.start + 1..group.end).any(|candidate_index| {
                            let candidate_style = self.style_for_table_row(
                                &rows[candidate_index],
                                style,
                                stylesheets,
                            );
                            Self::effective_table_row_break_before(
                                candidate_index,
                                &candidate_style,
                                row_group_break_before,
                            ) != PageBreak::Auto
                        })
                    });
                let group_requirement = TableRowGroupFragmentRequirement::from_row_group(
                    avoid_row_group,
                    planned_row_heights,
                    planned_row_occupancy,
                    table_metrics.clone(),
                );
                // A one-row group can still exceed a fresh fragmentainer when
                // separated-border edges are included. Its row track itself
                // is smaller than the page, so the ordinary oversized-row
                // branch would otherwise paint past the trailing edge instead
                // of splitting at a cell-child boundary.
                let no_repeat_fragmentainer = row_chrome_context
                    .without_repeats()
                    .fresh_fragmentainer(TableFragmentRepeatPolicy {
                        repeat_header: false,
                        repeat_footer: false,
                    });
                row_requires_avoid_group_slice = authored_avoid_row_group.is_some()
                    && avoid_row_group.row_span() == 1
                    && group_requirement.block_size().points()
                        > no_repeat_fragmentainer.body_capacity.points()
                            + TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE;
                let current_fragmentainer = row_chrome_context.current_fragmentainer(
                    PageTopBlockPosition::new(self.cursor_y),
                    PageTopBlockPosition::new(self.page_bottom()),
                    current_fragment_repeat_policy,
                    !row_is_repeating_footer,
                );
                // CSS Fragmentation 3 treats `break-inside: avoid` as a
                // preference to keep a fragmentation container together when
                // possible. For table row groups, use the measured group height
                // so rows are moved as a unit when their complete table
                // fragment footprint fits on a fresh page but not in the
                // current remaining fragmentainer.
                // https://www.w3.org/TR/css-break-3/#break-within
                let row_group_avoid_decision =
                    TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
                        group: avoid_row_group,
                        required_block_size: group_requirement.block_size(),
                        current_fragmentainer,
                        chrome_context: row_chrome_context,
                        can_advance: !self.cursor_is_at_page_top(),
                    });
                let oversized_authored_group_needs_fresh_start = authored_avoid_row_group.is_some()
                    && !authored_group_has_forced_internal_break
                    && row_group_avoid_decision.is_none()
                    && group_requirement.block_size().points()
                        > current_fragmentainer.available_block_size().points() + 0.01;
                // When an avoided group cannot remain whole after repeated
                // chrome is accounted for, move its first source row to a
                // fresh table fragment before relaxing the constraint at a
                // legal child boundary. Starting the relaxed split in the
                // previous fragment loses the group-level avoid opportunity
                // and makes its first atomic child share unrelated header or
                // footer chrome.
                // <https://www.w3.org/TR/css-break-3/#break-within>
                if let Some(decision) = row_group_avoid_decision
                    && !(row_level_avoid && decision.keeps_with_overflow())
                {
                    // The decision has already selected the only repeat
                    // policy that preserves this avoided group. Recomputing
                    // it here reintroduces chrome after a bounded-overflow
                    // decision deliberately suppressed it.
                    let incoming_repeat_policy = decision.repeat_policy;
                    debug_assert!(
                        decision.required_block_size.points()
                            > current_fragmentainer.available_block_size().points() + 0.01
                    );
                    let transition = Self::table_body_fragment_transition(
                        fragmentainer_kind,
                        current_fragment_repeat_policy,
                        TableFragmentBreakReason::AvoidedOverflow,
                        incoming_repeat_policy,
                        !row_is_repeating_header,
                        !row_is_repeating_footer,
                    );
                    current_fragment_repeat_policy = transition.start.repeat_policy;
                    // Only `KeptByChromeOverflow` records a range, but the
                    // state owns that distinction. Committing every decision
                    // here keeps the pagination decision and the row-paint
                    // mode coupled without duplicating its predicate at the
                    // call site.
                    avoid_row_group_keep_state.commit(decision);
                    pending_fragment_start = transition.start;
                    self.apply_table_body_fragment_transition(
                        &mut table_body_fragment,
                        &mut fragment_commit_context,
                        transition,
                        !table_body_fragment_started,
                    );
                    table_body_fragment_started = true;
                    table_x = fragment_commit_context.table_x;
                    start_chrome_replayed_after_break = true;
                    broke_before_row = true;
                }
                if !broke_before_row && oversized_authored_group_needs_fresh_start {
                    let incoming_repeat_policy =
                        row_chrome_context.repeat_policy(layout_pt(row_height));
                    let transition = Self::table_body_fragment_transition(
                        fragmentainer_kind,
                        current_fragment_repeat_policy,
                        TableFragmentBreakReason::AvoidedOverflow,
                        incoming_repeat_policy,
                        !row_is_repeating_header,
                        !row_is_repeating_footer,
                    );
                    current_fragment_repeat_policy = transition.start.repeat_policy;
                    pending_fragment_start = transition.start;
                    self.apply_table_body_fragment_transition(
                        &mut table_body_fragment,
                        &mut fragment_commit_context,
                        transition,
                        !table_body_fragment_started,
                    );
                    table_body_fragment_started = true;
                    table_x = fragment_commit_context.table_x;
                    start_chrome_replayed_after_break = true;
                    broke_before_row = true;
                }
            }
            let current_fragmentainer = row_chrome_context.current_fragmentainer(
                PageTopBlockPosition::new(self.cursor_y),
                PageTopBlockPosition::new(self.page_bottom()),
                current_fragment_repeat_policy,
                !row_is_repeating_footer,
            );
            let row_break_opportunity = FragmentBreakOpportunity::before_box_boundary(
                fragmentainer_kind,
                row_index as f32,
                row_breaks,
                avoid_break_candidates.previous_break_after,
                false,
            );
            let avoid_boundary = row_break_opportunity.avoids_break_in(fragmentainer_kind);
            let avoid_candidate = avoid_break_candidates.boundary_candidate(row_breaks);
            if avoid_boundary
                && let Some(candidate) = avoid_candidate
                && let Some(decision) =
                    TableAvoidRunBreakDecision::choose(TableAvoidRunBreakInput {
                        candidate,
                        row_height,
                        current_fragmentainer,
                        chrome_context: row_chrome_context,
                        can_advance: !self.cursor_is_at_page_top(),
                    })
            {
                debug_assert!(
                    decision.avoid_run_height
                        > current_fragmentainer.available_block_size().points() + 0.01
                );
                let candidate_meta = decision.candidate.restore(self);
                table_body_fragment = candidate_meta.table_body_fragment;
                current_fragment_repeat_policy = candidate_meta.repeat_policy;
                let transition = Self::table_body_fragment_transition(
                    fragmentainer_kind,
                    current_fragment_repeat_policy,
                    TableFragmentBreakReason::AvoidedOverflow,
                    decision.incoming_repeat_policy,
                    !row_is_repeating_header,
                    !row_is_repeating_footer && !self.cursor_is_at_page_top(),
                );
                current_fragment_repeat_policy = transition.start.repeat_policy;
                pending_fragment_start = transition.start;
                self.apply_table_body_fragment_transition(
                    &mut table_body_fragment,
                    &mut fragment_commit_context,
                    transition,
                    !table_body_fragment_started,
                );
                table_body_fragment_started = true;
                table_x = fragment_commit_context.table_x;
                row_index = candidate_meta.row_index;
                avoid_break_candidates.reset();
                continue;
            }
            let row_kept_by_avoid_group = avoid_row_group_keep_state.contains_row(row_index);
            if let Some(overflow_break) =
                TableRowOverflowBreakDecision::choose(TableRowOverflowBreakInput {
                    row_height,
                    row_required_height: row_fragment_required_height,
                    current_fragmentainer,
                    row_kept_by_avoid_group,
                    prefer_fresh_fragment: row_level_avoid,
                    can_break: !self.cursor_is_at_page_top()
                        && self.out_of_flow_prebreak_suppression_depth == 0,
                    chrome_context: row_chrome_context,
                })
            {
                debug_assert_eq!(overflow_break.row_height, row_height);
                let transition = Self::table_body_fragment_transition(
                    fragmentainer_kind,
                    current_fragment_repeat_policy,
                    TableFragmentBreakReason::Overflow,
                    overflow_break.incoming_repeat_policy,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                current_fragment_repeat_policy = transition.start.repeat_policy;
                pending_fragment_start = transition.start;
                self.apply_table_body_fragment_transition(
                    &mut table_body_fragment,
                    &mut fragment_commit_context,
                    transition,
                    !table_body_fragment_started,
                );
                table_body_fragment_started = true;
                table_x = fragment_commit_context.table_x;
                start_chrome_replayed_after_break = true;
                broke_before_row = true;
            }
            if broke_before_row {
                if !start_chrome_replayed_after_break {
                    self.replay_table_fragment_start_chrome(
                        &fragment_commit_context,
                        pending_fragment_start,
                    );
                }
                let after_header_fragmentainer = row_chrome_context.current_fragmentainer(
                    PageTopBlockPosition::new(self.cursor_y),
                    PageTopBlockPosition::new(self.page_bottom()),
                    current_fragment_repeat_policy,
                    !row_is_repeating_footer,
                );
                if let Some(overflow_break) =
                    TableRowOverflowBreakDecision::choose(TableRowOverflowBreakInput {
                        row_height,
                        row_required_height: row_fragment_required_height,
                        current_fragmentainer: after_header_fragmentainer,
                        row_kept_by_avoid_group,
                        prefer_fresh_fragment: false,
                        can_break: !self.cursor_is_at_page_top(),
                        chrome_context: TableFragmentChromeContext {
                            allow_header: false,
                            ..row_chrome_context
                        },
                    })
                {
                    debug_assert_eq!(overflow_break.row_height, row_height);
                    let transition = Self::table_body_fragment_transition(
                        fragmentainer_kind,
                        current_fragment_repeat_policy,
                        TableFragmentBreakReason::Overflow,
                        overflow_break.incoming_repeat_policy,
                        false,
                        !row_is_repeating_footer,
                    );
                    current_fragment_repeat_policy = transition.start.repeat_policy;
                    pending_fragment_start = transition.start;
                    self.apply_table_body_fragment_transition(
                        &mut table_body_fragment,
                        &mut fragment_commit_context,
                        transition,
                        !table_body_fragment_started,
                    );
                    table_body_fragment_started = true;
                    table_x = fragment_commit_context.table_x;
                }
            }

            let row_top = self.cursor_y;
            self.ensure_committed_table_body_fragment(
                &mut table_body_fragment,
                fragmentainer_kind,
                row_top,
                &mut pending_fragment_start,
                current_fragment_repeat_policy,
                repeating_header_rows,
            );
            if row_collapsed {
                let decision = self.table_row_fragment_decision(
                    table_body_fragment.as_ref(),
                    table_x,
                    physical_grid_width_points,
                    row_index,
                    row_top,
                    0.0,
                    0.0,
                    0.0,
                    true,
                    TableRowFragmentMode::Whole,
                );
                self.capture_table_row_fragment_decision_assignments(
                    row,
                    &row_style,
                    stylesheets,
                    table_x,
                    decision,
                );
                if let Some(fragment) = &mut table_body_fragment {
                    fragment.push_row_decision(decision);
                }
                avoid_break_candidates.finish_non_content_row(row_breaks, row_start_candidate);
                row_index += 1;
                continue;
            }
            if row_is_running {
                let placement = self.table_row_running_assignment_placement(table_x, row_top);
                if let Some(element) = row.element {
                    self.capture_assignments_for_fragment_source(element, &row_style, placement);
                }
                avoid_break_candidates.finish_non_content_row(row_breaks, row_start_candidate);
                row_index += 1;
                continue;
            }
            let row_baseline_offset = self
                .table_row_baseline_offset(
                    row_index,
                    row,
                    &grid.rows[row_index],
                    &row_style,
                    stylesheets,
                    table_cellpadding,
                    column_plan,
                    table_metrics.clone(),
                    collapsed_geometry,
                )
                .map(|baseline| baseline.offset);
            let row_exceeds_fresh_body = row_height
                > row_chrome_context
                    .fresh_fragmentainer(row_chrome_context.repeat_policy(layout_pt(row_height)))
                    .body_capacity
                    .points()
                    + 0.01;
            if (row_exceeds_fresh_body || row_requires_avoid_group_slice)
                && !row_kept_by_avoid_group
            {
                let mut remaining = row_height;
                let mut piece_offset = 0.0;
                while remaining > 0.01 {
                    let current_fragmentainer = row_chrome_context.current_fragmentainer(
                        PageTopBlockPosition::new(self.cursor_y),
                        PageTopBlockPosition::new(self.page_bottom()),
                        current_fragment_repeat_policy,
                        !row_is_repeating_footer,
                    );
                    let mut slice_decision =
                        TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
                            remaining_height: remaining,
                            row_required_height: row_fragment_required_height,
                            current_fragmentainer,
                            chrome_context: row_chrome_context,
                            can_advance: !self.cursor_is_at_page_top(),
                        });
                    if slice_decision.paints_slice() {
                        let fresh_body_capacity = row_chrome_context
                            .fresh_fragmentainer(slice_decision.incoming_repeat_policy)
                            .body_capacity
                            .points();
                        // Child-boundary measurement reuses final table-cell
                        // relayout, whose nested formatting-context probes can
                        // otherwise append provisional paint. It is only a
                        // break-choice query, so restore its complete page and
                        // side-effect state before committing the selected row
                        // piece.
                        let child_boundary_snapshot = self.snapshot();
                        let child_boundary_piece_height = self
                            .table_row_child_boundary_piece_height(
                                row,
                                &row_style,
                                grid,
                                row_index,
                                stylesheets,
                                table_cellpadding,
                                column_plan,
                                table_metrics.clone(),
                                collapsed_geometry,
                                piece_offset,
                                slice_decision.piece_height,
                                remaining,
                                fresh_body_capacity,
                            );
                        self.restore(child_boundary_snapshot);
                        // A zero result is a real pre-break decision: the
                        // next atomic cell child belongs to the destination
                        // fragmentainer rather than a clipped source slice.
                        if child_boundary_piece_height > 0.01 || !self.cursor_is_at_page_top() {
                            slice_decision =
                                slice_decision.at_child_boundary(child_boundary_piece_height);
                        }
                    }
                    if !slice_decision.paints_slice() {
                        debug_assert!(
                            slice_decision.available_body_size <= 0.01
                                || !self.cursor_is_at_page_top(),
                            "a child-aware table slice may only pre-break away from a nonempty fragmentainer"
                        );
                        let transition = Self::table_body_fragment_transition(
                            fragmentainer_kind,
                            current_fragment_repeat_policy,
                            TableFragmentBreakReason::OversizedRowSlice,
                            slice_decision.incoming_repeat_policy,
                            !row_is_repeating_header,
                            !row_is_repeating_footer,
                        );
                        current_fragment_repeat_policy = transition.start.repeat_policy;
                        pending_fragment_start = transition.start;
                        self.apply_table_body_fragment_transition(
                            &mut table_body_fragment,
                            &mut fragment_commit_context,
                            transition,
                            !table_body_fragment_started,
                        );
                        table_body_fragment_started = true;
                        table_x = fragment_commit_context.table_x;
                        self.ensure_committed_table_body_fragment(
                            &mut table_body_fragment,
                            fragmentainer_kind,
                            self.cursor_y,
                            &mut pending_fragment_start,
                            current_fragment_repeat_policy,
                            repeating_header_rows,
                        );
                        continue;
                    }

                    let piece_height = slice_decision.piece_height;
                    let piece_top = self.cursor_y;
                    self.ensure_committed_table_body_fragment(
                        &mut table_body_fragment,
                        fragmentainer_kind,
                        piece_top,
                        &mut pending_fragment_start,
                        current_fragment_repeat_policy,
                        repeating_header_rows,
                    );
                    let decision = self.table_row_fragment_decision(
                        table_body_fragment.as_ref(),
                        table_x,
                        physical_grid_width_points,
                        row_index,
                        piece_top,
                        piece_height,
                        piece_offset,
                        row_height,
                        !planned_row_occupancy
                            .get(row_index)
                            .cloned()
                            .unwrap_or(false),
                        TableRowFragmentMode::Sliced,
                    );
                    self.capture_table_row_fragment_decision_assignments(
                        row,
                        &row_style,
                        stylesheets,
                        table_x,
                        decision,
                    );
                    self.paint_committed_table_row_fragment(
                        &mut table_body_fragment,
                        decision,
                        row,
                        &row_style,
                        rows,
                        grid,
                        style,
                        stylesheets,
                        table_x,
                        physical_grid_width_points,
                        table_cellpadding,
                        column_plan,
                        planned_row_heights,
                        source_row_heights,
                        planned_row_occupancy,
                        table_height_is_definite,
                        table_metrics.clone(),
                        collapsed_geometry,
                        row_baseline_offset,
                        source_grid_placement,
                    );
                    self.cursor_y -= piece_height;
                    remaining -= piece_height;
                    piece_offset += piece_height;

                    if slice_decision.continues_after_slice() {
                        let transition = Self::table_body_fragment_transition(
                            fragmentainer_kind,
                            current_fragment_repeat_policy,
                            TableFragmentBreakReason::OversizedRowSlice,
                            slice_decision.incoming_repeat_policy,
                            !row_is_repeating_header,
                            !row_is_repeating_footer,
                        );
                        current_fragment_repeat_policy = transition.start.repeat_policy;
                        pending_fragment_start = transition.start;
                        self.apply_table_body_fragment_transition(
                            &mut table_body_fragment,
                            &mut fragment_commit_context,
                            transition,
                            !table_body_fragment_started,
                        );
                        table_body_fragment_started = true;
                        table_x = fragment_commit_context.table_x;
                        self.ensure_committed_table_body_fragment(
                            &mut table_body_fragment,
                            fragmentainer_kind,
                            self.cursor_y,
                            &mut pending_fragment_start,
                            current_fragment_repeat_policy,
                            repeating_header_rows,
                        );
                    }
                }
            } else {
                let row_fragment_mode = if row_kept_by_avoid_group {
                    TableRowFragmentMode::KeptByAvoidOverflow
                } else {
                    TableRowFragmentMode::Whole
                };
                let decision = self.table_row_fragment_decision(
                    table_body_fragment.as_ref(),
                    table_x,
                    physical_grid_width_points,
                    row_index,
                    row_top,
                    row_height,
                    0.0,
                    row_height,
                    !planned_row_occupancy
                        .get(row_index)
                        .cloned()
                        .unwrap_or(false),
                    row_fragment_mode,
                );
                self.capture_table_row_fragment_decision_assignments(
                    row,
                    &row_style,
                    stylesheets,
                    table_x,
                    decision,
                );
                self.paint_committed_table_row_fragment(
                    &mut table_body_fragment,
                    decision,
                    row,
                    &row_style,
                    rows,
                    grid,
                    style,
                    stylesheets,
                    table_x,
                    physical_grid_width_points,
                    table_cellpadding,
                    column_plan,
                    planned_row_heights,
                    source_row_heights,
                    planned_row_occupancy,
                    table_height_is_definite,
                    table_metrics.clone(),
                    collapsed_geometry,
                    row_baseline_offset,
                    source_grid_placement,
                );
                self.cursor_y -= row_height;
            }
            if planned_row_occupancy
                .get(row_index)
                .cloned()
                .unwrap_or(false)
                && planned_row_occupancy
                    .get(row_index + 1..)
                    .is_some_and(|following| following.iter().any(|occupied| *occupied))
            {
                self.cursor_y -= table_metrics.spacing.vertical.length_points();
            }
            let has_next_row = row_index + 1 < rows.len();
            forced_break_carry.finish_box(row_breaks, has_next_row);
            avoid_break_candidates.finish_content_row(row_breaks, row_start_candidate, row_height);
            row_index += 1;
            avoid_row_group_keep_state.finish_row(row_index);
        }

        (
            table_body_fragment,
            forced_break_carry.outgoing_source_break(),
            current_fragment_repeat_policy,
            continuation_inline_offset,
        )
    }

    /// Commit the current table-body page fragment before starting another one.
    ///
    /// CSS Fragmentation commits one fragmentainer slice before layout
    /// advances to the next page; CSS 2.2 table header/footer repetition is
    /// page-fragment chrome around that same committed body slice. Centralizing
    /// the side effects here keeps footer reservation, footer painting, and
    /// table paint finalization tied to a single fragment boundary decision:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-footer-group>.
    pub(in crate::layout::table) fn commit_table_body_fragment_boundary(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        context: &TableBodyFragmentCommitContext<'_, '_>,
        boundary: TableFragmentBoundaryDecision,
    ) {
        debug_assert!(
            (context.logical_inline_extent.points() - context.column_plan.total_width().points())
                .abs()
                <= 0.01,
            "fragment commit must retain the column plan's logical inline extent"
        );
        let has_normal_flow_content = fragment.as_ref().is_some_and(|fragment| {
            fragment
                .plan
                .body_rows
                .iter()
                .any(|row| !row.collapsed && row.row_height > 0.0)
        });
        let footer_rows = boundary
            .repeat_policy
            .footer_rows(context.repeating_footer_rows);
        if let Some(fragment) = fragment {
            fragment.mark_outgoing_boundary(boundary);
        }
        if boundary.footer_action.record_repeated_rows() {
            self.mark_table_body_fragment_repeated_footers(
                fragment,
                footer_rows,
                context.planned_row_heights,
                context.planned_row_occupancy,
                context.table_metrics.clone(),
            );
        }
        self.finalize_table_body_paint_fragment(
            fragment,
            context.rows,
            context.grid,
            context.columns,
            context.style,
            context.stylesheets,
            context.table_x,
            context.physical_grid_width.points(),
            context.table_cellpadding,
            context.column_plan,
            context.table_width,
            context.table_metrics.clone(),
            context.collapsed_geometry,
            context.table_is_document_canvas,
        );
        if has_normal_flow_content {
            self.mark_current_page_flow_content();
            self.current_page.mark_fragmentation_content();
        }
        if boundary.footer_action.paint_repeated_chrome() {
            self.layout_repeated_table_footer_rows_at_page_bottom(
                context.rows,
                context.grid,
                context.columns,
                footer_rows,
                context.style,
                context.stylesheets,
                context.table_x,
                context.physical_grid_width.points(),
                context.table_cellpadding,
                context.column_plan,
                context.planned_row_heights,
                context.planned_row_occupancy,
                context.table_width,
                context.table_metrics.clone(),
                context.collapsed_geometry,
            );
        }
    }

    /// Replay table chrome committed at the start of a body fragment.
    ///
    /// CSS Fragmentation fixes the new page fragment before its body rows are
    /// painted. Repeated table headers are page-fragment chrome, so their
    /// replay is driven by the same start decision later recorded on the body
    /// fragment plan:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/CSS22/tables.html#value-def-table-header-group>.
    fn replay_table_fragment_start_chrome(
        &mut self,
        context: &TableBodyFragmentCommitContext<'_, '_>,
        start: TableFragmentStartDecision,
    ) {
        let header_rows = start.repeated_header_rows(context.repeating_header_rows);
        if header_rows.is_empty() {
            return;
        }
        self.layout_repeated_table_rows(
            context.rows,
            context.grid,
            context.columns,
            header_rows,
            context.style,
            context.stylesheets,
            context.table_x,
            context.physical_grid_width.points(),
            context.table_cellpadding,
            context.column_plan,
            context.planned_row_heights,
            context.planned_row_occupancy,
            context.table_width,
            context.table_metrics.clone(),
            context.collapsed_geometry,
            true,
        );
    }

    /// Apply one committed table-fragment transition.
    ///
    /// The outgoing boundary owns footer replay/finalization and the incoming
    /// start owns repeated header replay. Keeping those decisions paired
    /// prevents target-specific cursor state from becoming an independent
    /// source of table fragmentation behavior:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn apply_table_body_fragment_transition(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        context: &mut TableBodyFragmentCommitContext<'_, '_>,
        transition: TableFragmentTransitionDecision,
        _starts_table_body: bool,
    ) {
        self.commit_table_body_fragment_boundary(fragment, context, transition.boundary);
        let Some(cursor_top) = self.materialize_table_fragmentainer_advance(
            transition.fragmentainer_kind,
            FragmentainerAdvance::Unforced,
        ) else {
            return;
        };
        context.rebase_to_content_left(self.content_left);
        let has_repeated_header = !transition
            .start
            .repeated_header_rows(context.repeating_header_rows)
            .is_empty();
        let placement = TableFragmentStartPlacement {
            table_x: context.table_x,
            destination_cursor: PageTopBlockPosition::new(cursor_top),
            wrapper_block_start: TableWrapperFragmentChrome::for_table(
                context.style,
                context.table_width,
            )
            .continuation_block_start(),
        };
        self.cursor_y = placement.cursor_for_start(has_repeated_header);
        debug_assert!((placement.table_x - context.table_x).abs() <= 0.01);
        self.replay_table_fragment_start_chrome(context, transition.start);
    }

    /// Ensure a table body paint fragment exists and consume its start decision.
    ///
    /// A committed table fragment start owns the break reason and repeated
    /// header replay for exactly one newly-created table body fragment. After
    /// that start decision is recorded in the fragment plan, pagination resets
    /// the pending start to a neutral table-start value so later rows cannot
    /// accidentally inherit stale break metadata:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn ensure_committed_table_body_fragment(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        fragmentainer_kind: FragmentainerKind,
        fragment_top: f32,
        pending_start: &mut TableFragmentStartDecision,
        current_repeat_policy: TableFragmentRepeatPolicy,
        repeating_header_rows: &[usize],
    ) {
        if self.ensure_table_body_paint_fragment(
            fragment,
            fragmentainer_kind,
            fragment_top,
            *pending_start,
            repeating_header_rows,
        ) {
            *pending_start = TableFragmentStartDecision::new(
                TableFragmentBreakReason::TableStart,
                current_repeat_policy,
                false,
            );
        }
    }

    /// Build the committed transition between table body fragments.
    ///
    /// CSS Fragmentation treats the outgoing fragment boundary and incoming
    /// fragmentainer start as one break decision. This helper keeps row
    /// pagination branches from independently assembling footer replay,
    /// incoming repeat policy, break reason, and header replay:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn table_body_fragment_transition(
        fragmentainer_kind: FragmentainerKind,
        outgoing_repeat_policy: TableFragmentRepeatPolicy,
        break_reason: TableFragmentBreakReason,
        incoming_repeat_policy: TableFragmentRepeatPolicy,
        paint_repeated_header: bool,
        paint_repeated_footer: bool,
    ) -> TableFragmentTransitionDecision {
        TableFragmentTransitionDecision::from_input(TableFragmentTransitionInput {
            fragmentainer_kind,
            outgoing_repeat_policy,
            footer_action: TableFragmentFooterAction::paint_repeated_if(paint_repeated_footer),
            break_reason,
            incoming_repeat_policy,
            paint_repeated_header,
        })
    }

    /// Build the committed forced break before a table body row.
    ///
    /// Forced row and row-group breaks are resolved before the row is painted.
    /// The decision records the outgoing footer action, the authored break
    /// value passed to paged-media page selection, and the incoming repeated
    /// header/footer policy as one table-local break choice:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
    fn table_body_forced_break(
        outgoing_repeat_policy: TableFragmentRepeatPolicy,
        fragmentainer_kind: FragmentainerKind,
        page_break: PageBreak,
        row_required_height: f32,
        chrome_context: TableFragmentChromeContext,
        paint_repeated_footer: bool,
    ) -> TableForcedBreakDecision {
        TableForcedBreakDecision::choose(TableForcedBreakInput {
            outgoing_repeat_policy,
            fragmentainer_kind,
            page_break,
            row_required_height,
            chrome_context,
            paint_repeated_footer,
        })
    }

    /// Apply a committed forced break before continuing table body pagination.
    ///
    /// The outgoing table fragment is finalized from the committed boundary
    /// before `apply_forced_break` performs the paged-media page transition.
    /// Repeated header replay remains owned by the returned start decision and
    /// is consumed just before the next row fragment is painted:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn apply_table_body_forced_break(
        &mut self,
        fragment: &mut Option<TableBodyPaintFragment>,
        context: &mut TableBodyFragmentCommitContext<'_, '_>,
        decision: TableForcedBreakDecision,
    ) {
        self.commit_table_body_fragment_boundary(fragment, context, decision.boundary);
        let Some(cursor_top) = self.materialize_table_fragmentainer_advance(
            decision.fragmentainer_kind,
            FragmentainerAdvance::Forced(decision.page_break),
        ) else {
            return;
        };
        context.rebase_to_content_left(self.content_left);
        let has_repeated_header = !decision
            .start
            .repeated_header_rows(context.repeating_header_rows)
            .is_empty();
        let placement = TableFragmentStartPlacement {
            table_x: context.table_x,
            destination_cursor: PageTopBlockPosition::new(cursor_top),
            wrapper_block_start: TableWrapperFragmentChrome::for_table(
                context.style,
                context.table_width,
            )
            .continuation_block_start(),
        };
        self.cursor_y = placement.cursor_for_start(has_repeated_header);
    }

    /// Advance a table body to a committed destination fragmentainer.
    ///
    /// Table rows own their page transition directly, so they do not naturally
    /// re-enter the root/body canvas as ordinary block flow does. Replay the
    /// same continuation geometry after the destination page has been chosen
    /// before table chrome or row coordinates are calculated.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout::table) fn materialize_table_fragmentainer_advance(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        advance: FragmentainerAdvance,
    ) -> Option<f32> {
        let continuation = (fragmentainer_kind == FragmentainerKind::Page)
            .then(|| self.fragment_continuation_context());
        self.materialize_fragmentainer_advance(fragmentainer_kind, advance)?;
        if let Some(continuation) = continuation {
            self.replay_fragment_continuation_on_page(&continuation, self.current_page_context);
        }
        Some(self.cursor_y)
    }

    fn effective_table_row_break_before(
        row_index: usize,
        row_style: &ComputedStyle,
        row_group_break_before: &[PageBreak],
    ) -> PageBreak {
        if row_group_break_before[row_index] != PageBreak::Auto {
            row_group_break_before[row_index]
        } else {
            row_style.break_before
        }
    }

    fn effective_table_row_break_after(
        row_index: usize,
        row_style: &ComputedStyle,
        row_group_break_after: &[PageBreak],
    ) -> PageBreak {
        if row_style.break_after != PageBreak::Auto {
            row_style.break_after
        } else {
            row_group_break_after[row_index]
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn table_row_fragment_decision(
        &self,
        fragment: Option<&TableBodyPaintFragment>,
        table_x: f32,
        used_table_width: f32,
        row_index: usize,
        row_top: f32,
        row_height: f32,
        row_offset: f32,
        original_row_height: f32,
        collapsed: bool,
        fragment_mode: TableRowFragmentMode,
    ) -> TableRowFragmentDecision {
        let starts_page_fragment = !self.current_page_has_content();
        let assignment_placement = fragment.map(|fragment| {
            Self::table_row_fragment_assignment_placement(
                fragment,
                table_x,
                used_table_width,
                row_top,
                row_height,
                starts_page_fragment,
            )
        });
        TableRowFragmentDecision {
            row_index,
            row_top,
            row_height,
            row_offset,
            original_row_height,
            collapsed,
            fragment_mode,
            assignment_placement,
            source_fragment: Self::table_row_source_fragment(assignment_placement),
        }
    }

    fn capture_table_row_fragment_decision_assignments(
        &mut self,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_x: f32,
        decision: TableRowFragmentDecision,
    ) {
        let Some(placement) = decision.assignment_placement else {
            return;
        };
        self.capture_table_row_named_string_assignments(
            row,
            row_style,
            placement,
            decision.row_offset,
        );
        self.capture_table_row_running_cell_assignments(
            row,
            row_style,
            stylesheets,
            table_x,
            decision.row_top,
            decision.row_offset,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_committed_table_row_fragment(
        &mut self,
        table_body_fragment: &mut Option<TableBodyPaintFragment>,
        decision: TableRowFragmentDecision,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        rows: &[TableRow<'_>],
        grid: &TableGrid,
        table_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        source_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_height_is_definite: bool,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        row_baseline_offset: Option<f32>,
        source_grid_placement: TableGridPlacement,
    ) {
        let grid_placement = table_body_fragment.as_mut().map(|fragment| {
            fragment.initialize_grid_placement(
                decision,
                table_style,
                table_x,
                column_plan,
                source_grid_placement,
                planned_row_heights,
                planned_row_occupancy,
                table_metrics.clone(),
            )
        });
        self.layout_table_row_paint_piece(
            decision.row_index,
            row,
            row_style,
            rows,
            grid,
            table_style,
            stylesheets,
            table_x,
            used_table_width,
            table_cellpadding,
            column_plan,
            planned_row_heights,
            source_row_heights,
            planned_row_occupancy,
            table_height_is_definite,
            table_metrics,
            grid_placement,
            decision.row_top,
            decision.original_row_height,
            decision.row_height,
            decision.row_offset,
            decision.fragment_mode,
            collapsed_geometry,
            row_baseline_offset,
        );
        if let Some(fragment) = table_body_fragment {
            fragment.push_row_decision(decision);
        }
    }

    /// Returns the first and last CSS `page` values represented by a table row.
    ///
    /// CSS Paged Media applies named pages at class A break opportunities, and
    /// CSS Tables preserves rows and cells as internal table boxes whose
    /// descendants can carry `page` values:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/CSS22/tables.html#model>.
    fn table_row_page_boundary_values(
        &mut self,
        row_index: usize,
        row_group_end: usize,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        table_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
    ) -> ResolvedPageBoundaryValues {
        let inherited_page_name = self.active_page_value_scope(table_style);
        let own_page_name = if row_style.page.is_specified() {
            row_style
                .page
                .specified_name()
                .map(|name| name.as_str().to_string())
                .or_else(|| inherited_page_name.clone())
        } else {
            inherited_page_name.clone()
        };
        let mut values = ResolvedPageBoundaryValues {
            start: own_page_name.clone(),
            end: own_page_name.clone(),
        };
        if row_style.page.is_specified() {
            return values;
        }

        let mut first_cell = None;
        let mut last_cell = None;
        let mut continuing_rowspan = None;
        for cell in &row.cells {
            let cell_style = self.style_for_table_cell(cell, row, row_style, stylesheets);
            if !style_is_in_normal_flow(&cell_style) {
                continue;
            }
            let child_boxes = cell.children.as_deref().unwrap_or_default();
            let cell_sources = page_value_sources_from_style_and_children(&cell_style, child_boxes);
            let cell_values = resolved_page_boundary_values_from_style_and_children(
                &cell_style,
                child_boxes,
                own_page_name.as_deref(),
            );
            if cell_sources.end.overrides_parent_summary()
                && cell.element.is_some_and(|element| {
                    html_table_rowspan(element, row_index, row_group_end) > 1
                })
            {
                continuing_rowspan = Some((cell_values.end.clone(), cell_sources.end.clone()));
            }
            if first_cell.is_none() {
                first_cell = Some((cell_values.start.clone(), cell_sources.start.clone()));
            }
            last_cell = Some((cell_values.end, cell_sources.end));
        }
        if let Some((cell_start, source)) = first_cell
            && source.overrides_parent_summary()
        {
            values.start = cell_start;
        }
        if let Some((cell_end, source)) = last_cell
            && source.overrides_parent_summary()
        {
            values.end = cell_end;
        }
        if values.end == own_page_name
            && let Some((rowspan_end, _source)) = continuing_rowspan
        {
            values.end = rowspan_end;
        }
        if values.start == own_page_name
            && let Some(group) = row.row_groups.last()
        {
            let group_style = self.style_for_table_row_group(group, table_style, stylesheets);
            if group_style.page.is_specified() {
                let group_value = group_style
                    .page
                    .specified_name()
                    .map(|name| name.as_str().to_string())
                    .or_else(|| inherited_page_name.clone());
                values.start = group_value.clone();
                if values.end == own_page_name {
                    values.end = group_value;
                }
            }
        }
        values
    }

    /// Returns the final GCPM assignment placement for a visible table row fragment.
    ///
    /// Table rows are internal table boxes, but CSS Fragmentation still exposes
    /// their page-local fragments as the source positions for generated paged
    /// media such as named strings:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/css-gcpm-3/#setting-named-strings>.
    fn table_row_fragment_assignment_placement(
        fragment: &TableBodyPaintFragment,
        table_x: f32,
        used_table_width: f32,
        row_top: f32,
        row_height: f32,
        starts_page_fragment: bool,
    ) -> AssignmentPlacement {
        AssignmentPlacement {
            page_index: fragment.plan.page_index,
            starts_page_fragment,
            border_box: Some(
                PageTopRect::new(table_x, row_top, used_table_width, row_height).paint_clip(),
            ),
        }
    }

    fn table_row_source_fragment(placement: Option<AssignmentPlacement>) -> TableRowSourceFragment {
        TableRowSourceFragment {
            border_box: placement.and_then(|placement| placement.border_box),
            starts_page_fragment: placement.is_some_and(|placement| placement.starts_page_fragment),
        }
    }

    /// Returns the zero-size source marker for a running table row.
    ///
    /// CSS GCPM removes `position: running()` boxes from normal flow while
    /// keeping their source position for `element(..., start)` resolution:
    /// <https://www.w3.org/TR/css-gcpm-3/#running-elements>.
    fn table_row_running_assignment_placement(
        &self,
        table_x: f32,
        row_top: f32,
    ) -> AssignmentPlacement {
        AssignmentPlacement {
            page_index: self.pages.len(),
            starts_page_fragment: !self.current_page_has_content(),
            border_box: Some(PageTopRect::new(table_x, row_top, 0.0, 0.0).paint_clip()),
        }
    }

    fn capture_table_row_named_string_assignments(
        &mut self,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        placement: AssignmentPlacement,
        row_offset: f32,
    ) {
        if row_offset > 0.01 {
            return;
        }
        let Some(element) = row.element else {
            return;
        };
        self.capture_named_strings_for_fragment_source(element, row_style, placement);
    }

    /// Captures `position: running()` cells removed from a table row.
    ///
    /// CSS GCPM removes running elements from normal flow while retaining their
    /// source position for `element(..., start)` lookup. Running table cells are
    /// filtered out before table grid construction, so the row's first emitted
    /// fragment provides the durable page-local source marker:
    /// <https://www.w3.org/TR/css-gcpm-3/#running-elements> and
    /// <https://drafts.csswg.org/css-tables-3/#cell-assignment>.
    fn capture_table_row_running_cell_assignments(
        &mut self,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_x: f32,
        row_top: f32,
        row_offset: f32,
    ) {
        if row_offset > 0.01 || row.running_cells.is_empty() {
            return;
        }
        let placement = AssignmentPlacement {
            page_index: self.pages.len(),
            starts_page_fragment: !self.current_page_has_content(),
            border_box: Some(PageTopRect::new(table_x, row_top, 0.0, 0.0).paint_clip()),
        };
        for cell in &row.running_cells {
            let Some(element) = cell.element else {
                continue;
            };
            let cell_style = self.style_for_table_cell(cell, row, row_style, stylesheets);
            self.capture_assignments_for_fragment_source(element, &cell_style, placement);
        }
    }
}
