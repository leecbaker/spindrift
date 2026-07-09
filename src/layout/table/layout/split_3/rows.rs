use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::table) fn layout_table_body_rows(
        &mut self,
        input: TableBodyRowsInput<'_, '_>,
    ) -> (
        Option<TableBodyPaintFragment>,
        PageBreak,
        TableFragmentRepeatPolicy,
    ) {
        let TableBodyRowsInput {
            fragmentainer_kind,
            rows,
            grid,
            columns,
            style,
            stylesheets,
            table_x,
            used_table_width,
            table_cellpadding,
            column_plan,
            planned_row_heights,
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
        } = input;
        let mut table_x = table_x;
        let mut table_page_content_left = self.content_left;
        let mut table_body_fragment: Option<TableBodyPaintFragment> = None;
        let mut current_fragment_repeat_policy = table_fragment_repeat_policy(
            0.01,
            self.page_area_height(),
            0.0,
            repeating_footer_height,
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
        let mut fragment_commit_context = TableBodyFragmentCommitContext {
            rows,
            grid,
            columns,
            style,
            stylesheets,
            table_x,
            used_table_width,
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
            let row_is_running = row_style.running_element_name.is_some();
            let row_collapsed = table_row_is_collapsed(&row_style);
            let row_chrome_context = TableFragmentChromeContext {
                fragmentainer_block_size: self.page_area_height(),
                header_height: repeating_header_height,
                footer_height: repeating_footer_height,
                allow_header: !row_is_repeating_header,
                allow_footer: !row_is_repeating_footer,
            };
            let row_fragment_required_height = if row_height > self.page_area_height() + 0.01 {
                0.01
            } else {
                row_height
            };
            let (row_page_start_source, row_page_end_source) = if row_is_running {
                ((None, false), (None, false))
            } else {
                self.table_row_page_value_sources(
                    row_index,
                    row_group_end_indices[row_index],
                    row,
                    &row_style,
                    style,
                    stylesheets,
                )
            };
            let row_page_start = page_boundary_name_in_parent_scope(row_page_start_source, style);
            let row_page_end = page_boundary_name_in_parent_scope(row_page_end_source, style);
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
                table_x += self.content_left - table_page_content_left;
                fragment_commit_context.table_x = table_x;
                table_page_content_left = self.content_left;
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
                    &fragment_commit_context,
                    forced_break,
                );
            }
            if let Some(avoid_row_group) = avoid_break_row_groups
                .iter()
                .find(|avoid_row_group| avoid_row_group.start == row_index)
            {
                let group_height = table_row_span_height(
                    planned_row_heights,
                    planned_row_occupancy,
                    row_index,
                    avoid_row_group.row_span(),
                    table_metrics.clone(),
                );
                let current_fragmentainer = TableFragmentainer::current_from_cursor_bounds(
                    self.page_area_height(),
                    self.cursor_y,
                    self.page_bottom(),
                    current_fragment_repeat_policy,
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_footer,
                );
                // CSS Fragmentation 3 treats `break-inside: avoid` as a
                // preference to keep a fragmentation container together when
                // possible. For table row groups, use the measured group height
                // so rows are moved as a unit when the group fits on a fresh
                // page but not in the current remaining fragmentainer.
                // https://www.w3.org/TR/css-break-3/#break-within
                let row_group_avoid_decision =
                    TableRowGroupAvoidDecision::choose(TableRowGroupAvoidDecisionInput {
                        group: *avoid_row_group,
                        group_height,
                        current_fragmentainer,
                        chrome_context: row_chrome_context,
                        can_advance: !self.cursor_is_at_page_top(),
                    });
                if let Some(decision) = row_group_avoid_decision {
                    debug_assert!(
                        decision.group_height > current_fragmentainer.available_block_size() + 0.01
                    );
                    let transition = Self::table_body_fragment_transition(
                        fragmentainer_kind,
                        current_fragment_repeat_policy,
                        TableFragmentBreakReason::AvoidedOverflow,
                        decision.repeat_policy,
                        !row_is_repeating_header,
                        !row_is_repeating_footer,
                    );
                    current_fragment_repeat_policy = transition.start.repeat_policy;
                    avoid_row_group_keep_state.commit(decision);
                    pending_fragment_start = transition.start;
                    self.apply_table_body_fragment_transition(
                        &mut table_body_fragment,
                        &fragment_commit_context,
                        transition,
                    );
                    start_chrome_replayed_after_break = true;
                    broke_before_row = true;
                }
            }
            let current_fragmentainer = TableFragmentainer::current_from_cursor_bounds(
                self.page_area_height(),
                self.cursor_y,
                self.page_bottom(),
                current_fragment_repeat_policy,
                repeating_header_height,
                repeating_footer_height,
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
                    decision.avoid_run_height > current_fragmentainer.available_block_size() + 0.01
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
                    &fragment_commit_context,
                    transition,
                );
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
                    &fragment_commit_context,
                    transition,
                );
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
                let after_header_fragmentainer = TableFragmentainer::current_from_cursor_bounds(
                    self.page_area_height(),
                    self.cursor_y,
                    self.page_bottom(),
                    current_fragment_repeat_policy,
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_footer,
                );
                if let Some(overflow_break) =
                    TableRowOverflowBreakDecision::choose(TableRowOverflowBreakInput {
                        row_height,
                        row_required_height: row_fragment_required_height,
                        current_fragmentainer: after_header_fragmentainer,
                        row_kept_by_avoid_group,
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
                        &fragment_commit_context,
                        transition,
                    );
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
                    used_table_width,
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
            let row_baseline_offset = self.table_row_baseline_offset(
                row_index,
                row,
                &grid.rows[row_index],
                &row_style,
                stylesheets,
                table_cellpadding,
                column_plan,
                table_metrics.clone(),
                collapsed_geometry,
            );
            if row_height > self.page_area_height() + 0.01 && !row_kept_by_avoid_group {
                let mut remaining = row_height;
                let mut piece_offset = 0.0;
                while remaining > 0.01 {
                    let current_fragmentainer = TableFragmentainer::current_from_cursor_bounds(
                        self.page_area_height(),
                        self.cursor_y,
                        self.page_bottom(),
                        current_fragment_repeat_policy,
                        repeating_header_height,
                        repeating_footer_height,
                        !row_is_repeating_footer,
                    );
                    let slice_decision =
                        TableOversizedRowSliceDecision::choose(TableOversizedRowSliceInput {
                            remaining_height: remaining,
                            row_required_height: row_fragment_required_height,
                            current_fragmentainer,
                            chrome_context: row_chrome_context,
                            can_advance: !self.cursor_is_at_page_top(),
                        });
                    if !slice_decision.paints_slice() {
                        debug_assert!(slice_decision.available_body_size <= 0.01);
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
                            &fragment_commit_context,
                            transition,
                        );
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
                        used_table_width,
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
                        used_table_width,
                        table_cellpadding,
                        column_plan,
                        planned_row_heights,
                        planned_row_occupancy,
                        table_height_is_definite,
                        table_metrics.clone(),
                        collapsed_geometry,
                        row_baseline_offset,
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
                            &fragment_commit_context,
                            transition,
                        );
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
                    used_table_width,
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
                    used_table_width,
                    table_cellpadding,
                    column_plan,
                    planned_row_heights,
                    planned_row_occupancy,
                    table_height_is_definite,
                    table_metrics.clone(),
                    collapsed_geometry,
                    row_baseline_offset,
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
            context.used_table_width,
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
                context.used_table_width,
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
            context.used_table_width,
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
        context: &TableBodyFragmentCommitContext<'_, '_>,
        transition: TableFragmentTransitionDecision,
    ) {
        self.commit_table_body_fragment_boundary(fragment, context, transition.boundary);
        let Some(cursor_top) = self.materialize_fragmentainer_advance(
            transition.fragmentainer_kind,
            FragmentainerAdvance::Unforced,
        ) else {
            return;
        };
        self.cursor_y = cursor_top;
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
        context: &TableBodyFragmentCommitContext<'_, '_>,
        decision: TableForcedBreakDecision,
    ) {
        self.commit_table_body_fragment_boundary(fragment, context, decision.boundary);
        let _ = self.materialize_fragmentainer_advance(
            decision.fragmentainer_kind,
            FragmentainerAdvance::Forced(decision.page_break),
        );
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
        stylesheets: &[Stylesheet],
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
        stylesheets: &[Stylesheet],
        table_x: f32,
        used_table_width: f32,
        table_cellpadding: Option<f32>,
        column_plan: &TableColumnPlan,
        planned_row_heights: &[f32],
        planned_row_occupancy: &[bool],
        table_height_is_definite: bool,
        table_metrics: TableMetrics,
        collapsed_geometry: Option<&CollapsedTableGeometry>,
        row_baseline_offset: Option<f32>,
    ) {
        if let Some(fragment) = table_body_fragment {
            fragment.initialize_grid_placement(
                decision,
                table_style,
                table_x,
                column_plan,
                planned_row_heights,
                planned_row_occupancy,
                table_metrics.clone(),
            );
        }
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
            planned_row_occupancy,
            table_height_is_definite,
            table_metrics,
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
    fn table_row_page_value_sources(
        &mut self,
        row_index: usize,
        row_group_end: usize,
        row: &TableRow<'_>,
        row_style: &ComputedStyle,
        table_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> ((Option<String>, bool), (Option<String>, bool)) {
        let own = row_style
            .page_name_specified
            .then(|| row_style.page_name.clone())
            .map(|name| (name, true))
            .unwrap_or((None, false));
        let mut start = own.clone();
        let mut end = own;
        if row_style.page_name_specified {
            return (start, end);
        }

        let mut first_cell_source = None;
        let mut last_cell_source = None;
        let mut continuing_rowspan_source = None;
        for cell in &row.cells {
            let cell_style = self.style_for_table_cell(cell, row, row_style, stylesheets);
            if !style_is_in_normal_flow(&cell_style) {
                continue;
            }
            let child_boxes = cell.children.as_deref().unwrap_or_default();
            let cell_sources = page_value_sources_from_style_and_children(&cell_style, child_boxes);
            if cell_sources.1.1
                && cell.element.is_some_and(|element| {
                    html_table_rowspan(element, row_index, row_group_end) > 1
                })
            {
                continuing_rowspan_source = Some(cell_sources.1.clone());
            }
            if first_cell_source.is_none() {
                first_cell_source = Some(cell_sources.0.clone());
            }
            last_cell_source = Some(cell_sources.1);
        }
        if let Some(cell_start) = first_cell_source
            && cell_start.1
        {
            start = cell_start;
        }
        if let Some(cell_end) = last_cell_source
            && cell_end.1
        {
            end = cell_end;
        }
        if !end.1
            && let Some(rowspan_end) = continuing_rowspan_source
        {
            end = rowspan_end;
        }
        if !start.1
            && let Some(group) = row.row_groups.last()
        {
            let group_style = self.style_for_table_row_group(group, table_style, stylesheets);
            if group_style.page_name_specified {
                let group_value = (group_style.page_name.clone(), true);
                start = group_value.clone();
                if !end.1 {
                    end = group_value;
                }
            }
        }
        (start, end)
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
        stylesheets: &[Stylesheet],
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
