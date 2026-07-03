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
        let mut pending_table_fragment_break_reason = TableFragmentBreakReason::TableStart;
        let mut pending_repeated_header_rows = Vec::new();
        let mut current_fragment_repeat_policy = table_fragment_repeat_policy(
            0.01,
            self.page_area_height(),
            0.0,
            repeating_footer_height,
            false,
            true,
        );
        let mut page_break_before_next_row = PageBreak::Auto;
        let mut forced_break_after_table_rows = PageBreak::Auto;
        let mut avoid_break_candidate: Option<TableBreakCandidate> = None;
        let mut previous_row_candidate: Option<TableBreakCandidate> = None;
        let mut previous_break_after_avoid = false;
        let mut previous_row_page_end: Option<Option<String>> = None;
        let mut unfragmented_avoid_row_group_end: Option<usize> = None;
        let row_group_end_indices = table_row_group_end_indices(rows);
        let mut row_index = 0usize;
        while row_index < rows.len() {
            let row = &rows[row_index];
            let row_style = self.style_for_table_row(row, style, stylesheets);
            let row_is_repeating_header = repeating_header_rows.contains(&row_index);
            let row_is_repeating_footer = repeating_footer_rows.contains(&row_index);
            let row_height = planned_row_heights[row_index];
            let row_is_running = row_style.running_element_name.is_some();
            let row_collapsed = table_row_is_collapsed(&row_style);
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
            if let Some(previous_page_end) = &previous_row_page_end
                && previous_page_end != &row_page_start
            {
                if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                    self.mark_table_body_fragment_repeated_footers(
                        &mut table_body_fragment,
                        current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                        planned_row_heights,
                        planned_row_occupancy,
                        table_metrics,
                    );
                }
                self.finalize_table_body_paint_fragment(
                    &mut table_body_fragment,
                    rows,
                    grid,
                    columns,
                    style,
                    stylesheets,
                    table_x,
                    used_table_width,
                    table_cellpadding,
                    column_plan,
                    table_width,
                    table_metrics,
                    collapsed_geometry,
                    table_is_document_canvas,
                );
                if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                    self.layout_repeated_table_footer_rows_at_page_bottom(
                        rows,
                        grid,
                        columns,
                        current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        column_plan,
                        planned_row_heights,
                        planned_row_occupancy,
                        table_width,
                        table_metrics,
                        collapsed_geometry,
                    );
                }
                self.switch_page_name_at_class_a_boundary(row_page_start.as_deref());
                table_x += self.content_left - table_page_content_left;
                table_page_content_left = self.content_left;
                current_fragment_repeat_policy = table_fragment_repeat_policy(
                    row_fragment_required_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                pending_table_fragment_break_reason = TableFragmentBreakReason::Forced;
                if !row_is_repeating_header {
                    pending_repeated_header_rows = current_fragment_repeat_policy
                        .header_rows(repeating_header_rows)
                        .to_vec();
                }
            }
            if !row_is_running {
                previous_row_page_end = Some(row_page_end);
            }
            let mut broke_before_row = page_break_before_next_row.is_forced();
            let pending_avoid_before_row = page_break_before_next_row.avoids_page();
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
            let pending_row_start_candidate = PendingTableBreakCandidate {
                meta: TableBreakCandidateMeta {
                    row_index,
                    table_body_fragment: table_body_fragment.clone(),
                    repeat_policy: current_fragment_repeat_policy,
                    height: 0.0,
                },
            };
            let row_start_may_be_rollback_target =
                (!row_collapsed && !row_is_running && break_after.avoids_page())
                    || next_break_before.avoids_page()
                    || (previous_break_after_avoid && avoid_break_candidate.is_none());
            let row_start_candidate =
                row_start_may_be_rollback_target.then(|| pending_row_start_candidate.arm(self));
            if page_break_before_next_row.is_forced() {
                if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                    self.mark_table_body_fragment_repeated_footers(
                        &mut table_body_fragment,
                        current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                        planned_row_heights,
                        planned_row_occupancy,
                        table_metrics,
                    );
                }
                self.finalize_table_body_paint_fragment(
                    &mut table_body_fragment,
                    rows,
                    grid,
                    columns,
                    style,
                    stylesheets,
                    table_x,
                    used_table_width,
                    table_cellpadding,
                    column_plan,
                    table_width,
                    table_metrics,
                    collapsed_geometry,
                    table_is_document_canvas,
                );
                if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                    self.layout_repeated_table_footer_rows_at_page_bottom(
                        rows,
                        grid,
                        columns,
                        current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        column_plan,
                        planned_row_heights,
                        planned_row_occupancy,
                        table_width,
                        table_metrics,
                        collapsed_geometry,
                    );
                }
                self.apply_forced_break(page_break_before_next_row);
                current_fragment_repeat_policy = table_fragment_repeat_policy(
                    row_fragment_required_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                pending_table_fragment_break_reason = TableFragmentBreakReason::Forced;
            }
            page_break_before_next_row = PageBreak::Auto;
            // CSS Fragmentation applies forced break-before/break-after values
            // to table row and row-group boxes as ordinary fragmentation boxes.
            // https://www.w3.org/TR/css-break-3/#break-between
            if break_before.is_forced() {
                if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                    self.mark_table_body_fragment_repeated_footers(
                        &mut table_body_fragment,
                        current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                        planned_row_heights,
                        planned_row_occupancy,
                        table_metrics,
                    );
                }
                self.finalize_table_body_paint_fragment(
                    &mut table_body_fragment,
                    rows,
                    grid,
                    columns,
                    style,
                    stylesheets,
                    table_x,
                    used_table_width,
                    table_cellpadding,
                    column_plan,
                    table_width,
                    table_metrics,
                    collapsed_geometry,
                    table_is_document_canvas,
                );
                if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                    self.layout_repeated_table_footer_rows_at_page_bottom(
                        rows,
                        grid,
                        columns,
                        current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        column_plan,
                        planned_row_heights,
                        planned_row_occupancy,
                        table_width,
                        table_metrics,
                        collapsed_geometry,
                    );
                }
                self.apply_forced_break(break_before);
                current_fragment_repeat_policy = table_fragment_repeat_policy(
                    row_fragment_required_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                pending_table_fragment_break_reason = TableFragmentBreakReason::Forced;
                broke_before_row = true;
            }
            if let Some((_, end, _)) = avoid_break_row_groups
                .iter()
                .find(|(start, _, _)| *start == row_index)
            {
                let group_height = table_row_span_height(
                    planned_row_heights,
                    planned_row_occupancy,
                    row_index,
                    end - row_index,
                    table_metrics,
                );
                let remaining_height = self.cursor_y - self.page_bottom();
                // CSS Fragmentation 3 treats `break-inside: avoid` as a
                // preference to keep a fragmentation container together when
                // possible. For table row groups, use the measured group height
                // so rows are moved as a unit when the group fits on a fresh
                // page but not in the current remaining fragmentainer.
                // https://www.w3.org/TR/css-break-3/#break-within
                let mut next_fragment_repeat_policy = table_fragment_repeat_policy(
                    group_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                let next_fragment_body_capacity = next_fragment_repeat_policy.body_capacity(
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                );
                let group_fits_next_fragment = group_height <= next_fragment_body_capacity + 0.01;
                let no_repeat_policy = TableFragmentRepeatPolicy {
                    repeat_header: false,
                    repeat_footer: false,
                };
                let no_repeat_body_capacity = no_repeat_policy.body_capacity(
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                );
                let group_can_overflow_next_fragment = !group_fits_next_fragment
                    && group_height
                        <= no_repeat_body_capacity + TABLE_AVOID_UNFRAGMENTED_OVERFLOW_TOLERANCE;
                if group_can_overflow_next_fragment {
                    next_fragment_repeat_policy = no_repeat_policy;
                }
                if (group_fits_next_fragment || group_can_overflow_next_fragment)
                    && group_height > remaining_height + 0.01
                    && !self.cursor_is_at_page_top()
                {
                    if !row_is_repeating_footer {
                        self.mark_table_body_fragment_repeated_footers(
                            &mut table_body_fragment,
                            current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                            planned_row_heights,
                            planned_row_occupancy,
                            table_metrics,
                        );
                    }
                    self.finalize_table_body_paint_fragment(
                        &mut table_body_fragment,
                        rows,
                        grid,
                        columns,
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        column_plan,
                        table_width,
                        table_metrics,
                        collapsed_geometry,
                        table_is_document_canvas,
                    );
                    if !row_is_repeating_footer {
                        self.layout_repeated_table_footer_rows_at_page_bottom(
                            rows,
                            grid,
                            columns,
                            current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            column_plan,
                            planned_row_heights,
                            planned_row_occupancy,
                            table_width,
                            table_metrics,
                            collapsed_geometry,
                        );
                    }
                    self.push_page();
                    self.cursor_y = self.page_top();
                    current_fragment_repeat_policy = next_fragment_repeat_policy;
                    if group_can_overflow_next_fragment {
                        unfragmented_avoid_row_group_end = Some(*end);
                    }
                    pending_table_fragment_break_reason = TableFragmentBreakReason::AvoidedOverflow;
                    broke_before_row = true;
                }
            }
            let reserved_footer_height = if row_is_repeating_footer {
                0.0
            } else {
                current_fragment_repeat_policy.reserved_footer_height(repeating_footer_height)
            };
            let avoid_boundary = pending_avoid_before_row
                || previous_break_after_avoid
                || break_before.avoids_page();
            let avoid_candidate = if pending_avoid_before_row || previous_break_after_avoid {
                avoid_break_candidate.clone()
            } else if break_before.avoids_page() {
                previous_row_candidate.clone()
            } else {
                None
            };
            if avoid_boundary
                && let Some(candidate) = avoid_candidate
                && !self.cursor_is_at_page_top()
                && row_height > self.cursor_y - self.page_bottom() + 0.01
            {
                let avoid_run_height = candidate.height() + row_height;
                let next_fragment_repeat_policy = table_fragment_repeat_policy(
                    avoid_run_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                if avoid_run_height
                    > next_fragment_repeat_policy.body_capacity(
                        self.page_area_height(),
                        repeating_header_height,
                        repeating_footer_height,
                    ) + 0.01
                {
                    // The run cannot be kept together on the next fragment;
                    // fall through to the ordinary row break rules.
                } else {
                    let candidate_meta = candidate.restore(self);
                    table_body_fragment = candidate_meta.table_body_fragment;
                    current_fragment_repeat_policy = candidate_meta.repeat_policy;
                    if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                        self.mark_table_body_fragment_repeated_footers(
                            &mut table_body_fragment,
                            current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                            planned_row_heights,
                            planned_row_occupancy,
                            table_metrics,
                        );
                    }
                    self.finalize_table_body_paint_fragment(
                        &mut table_body_fragment,
                        rows,
                        grid,
                        columns,
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        column_plan,
                        table_width,
                        table_metrics,
                        collapsed_geometry,
                        table_is_document_canvas,
                    );
                    if !row_is_repeating_footer && !self.cursor_is_at_page_top() {
                        self.layout_repeated_table_footer_rows_at_page_bottom(
                            rows,
                            grid,
                            columns,
                            current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            column_plan,
                            planned_row_heights,
                            planned_row_occupancy,
                            table_width,
                            table_metrics,
                            collapsed_geometry,
                        );
                    }
                    self.push_page();
                    self.cursor_y = self.page_top();
                    current_fragment_repeat_policy = next_fragment_repeat_policy;
                    pending_table_fragment_break_reason = TableFragmentBreakReason::AvoidedOverflow;
                    if !row_is_repeating_header {
                        self.layout_repeated_table_rows(
                            rows,
                            grid,
                            columns,
                            current_fragment_repeat_policy.header_rows(repeating_header_rows),
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            column_plan,
                            planned_row_heights,
                            planned_row_occupancy,
                            table_width,
                            table_metrics,
                            collapsed_geometry,
                        );
                        pending_repeated_header_rows = current_fragment_repeat_policy
                            .header_rows(repeating_header_rows)
                            .to_vec();
                    }
                    row_index = candidate_meta.row_index;
                    page_break_before_next_row = PageBreak::Auto;
                    avoid_break_candidate = None;
                    previous_row_candidate = None;
                    previous_break_after_avoid = false;
                    continue;
                }
            }
            let row_requires_split = row_height > self.page_area_height() + 0.01;
            let row_kept_by_avoid_group =
                unfragmented_avoid_row_group_end.is_some_and(|end| row_index < end);
            let available_height = self.cursor_y - self.page_bottom();
            let row_overflows_page = if row_requires_split {
                !row_kept_by_avoid_group && available_height <= 0.01
            } else {
                self.cursor_y - row_height < self.page_bottom()
            };
            let row_overflows_reserved_footer = if row_requires_split {
                !row_kept_by_avoid_group && available_height - reserved_footer_height <= 0.01
            } else {
                self.cursor_y - row_height - reserved_footer_height < self.page_bottom()
            };
            if (row_overflows_page || row_overflows_reserved_footer)
                && !self.cursor_is_at_page_top()
                && self.out_of_flow_prebreak_suppression_depth == 0
            {
                if !row_is_repeating_footer {
                    self.mark_table_body_fragment_repeated_footers(
                        &mut table_body_fragment,
                        current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                        planned_row_heights,
                        planned_row_occupancy,
                        table_metrics,
                    );
                }
                self.finalize_table_body_paint_fragment(
                    &mut table_body_fragment,
                    rows,
                    grid,
                    columns,
                    style,
                    stylesheets,
                    table_x,
                    used_table_width,
                    table_cellpadding,
                    column_plan,
                    table_width,
                    table_metrics,
                    collapsed_geometry,
                    table_is_document_canvas,
                );
                if !row_is_repeating_footer {
                    self.layout_repeated_table_footer_rows_at_page_bottom(
                        rows,
                        grid,
                        columns,
                        current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        column_plan,
                        planned_row_heights,
                        planned_row_occupancy,
                        table_width,
                        table_metrics,
                        collapsed_geometry,
                    );
                }
                self.push_page();
                self.cursor_y = self.page_top();
                current_fragment_repeat_policy = table_fragment_repeat_policy(
                    row_fragment_required_height,
                    self.page_area_height(),
                    repeating_header_height,
                    repeating_footer_height,
                    !row_is_repeating_header,
                    !row_is_repeating_footer,
                );
                pending_table_fragment_break_reason = TableFragmentBreakReason::Overflow;
                broke_before_row = true;
            }
            if broke_before_row && !row_is_repeating_header {
                self.layout_repeated_table_rows(
                    rows,
                    grid,
                    columns,
                    current_fragment_repeat_policy.header_rows(repeating_header_rows),
                    style,
                    stylesheets,
                    table_x,
                    used_table_width,
                    table_cellpadding,
                    column_plan,
                    planned_row_heights,
                    planned_row_occupancy,
                    table_width,
                    table_metrics,
                    collapsed_geometry,
                );
                pending_repeated_header_rows = current_fragment_repeat_policy
                    .header_rows(repeating_header_rows)
                    .to_vec();
                let reserved_footer_height_after_header = if row_is_repeating_footer {
                    0.0
                } else {
                    current_fragment_repeat_policy.reserved_footer_height(repeating_footer_height)
                };
                let available_height_after_header = self.cursor_y - self.page_bottom();
                let row_still_overflows_after_header = if row_requires_split {
                    !row_kept_by_avoid_group
                        && available_height_after_header - reserved_footer_height_after_header
                            <= 0.01
                } else {
                    self.cursor_y - row_height - reserved_footer_height_after_header
                        < self.page_bottom()
                };
                if row_still_overflows_after_header && !self.cursor_is_at_page_top() {
                    if !row_is_repeating_footer {
                        self.mark_table_body_fragment_repeated_footers(
                            &mut table_body_fragment,
                            current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                            planned_row_heights,
                            planned_row_occupancy,
                            table_metrics,
                        );
                    }
                    self.finalize_table_body_paint_fragment(
                        &mut table_body_fragment,
                        rows,
                        grid,
                        columns,
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        column_plan,
                        table_width,
                        table_metrics,
                        collapsed_geometry,
                        table_is_document_canvas,
                    );
                    if !row_is_repeating_footer {
                        self.layout_repeated_table_footer_rows_at_page_bottom(
                            rows,
                            grid,
                            columns,
                            current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            column_plan,
                            planned_row_heights,
                            planned_row_occupancy,
                            table_width,
                            table_metrics,
                            collapsed_geometry,
                        );
                    }
                    self.push_page();
                    self.cursor_y = self.page_top();
                    current_fragment_repeat_policy = table_fragment_repeat_policy(
                        row_fragment_required_height,
                        self.page_area_height(),
                        repeating_header_height,
                        repeating_footer_height,
                        false,
                        !row_is_repeating_footer,
                    );
                    pending_table_fragment_break_reason = TableFragmentBreakReason::Overflow;
                    self.layout_repeated_table_rows(
                        rows,
                        grid,
                        columns,
                        current_fragment_repeat_policy.header_rows(repeating_header_rows),
                        style,
                        stylesheets,
                        table_x,
                        used_table_width,
                        table_cellpadding,
                        column_plan,
                        planned_row_heights,
                        planned_row_occupancy,
                        table_width,
                        table_metrics,
                        collapsed_geometry,
                    );
                    pending_repeated_header_rows = current_fragment_repeat_policy
                        .header_rows(repeating_header_rows)
                        .to_vec();
                }
            }

            let row_top = self.cursor_y;
            if self.ensure_table_body_paint_fragment(
                &mut table_body_fragment,
                row_top,
                pending_table_fragment_break_reason,
                &pending_repeated_header_rows,
            ) {
                pending_table_fragment_break_reason = TableFragmentBreakReason::TableStart;
                pending_repeated_header_rows.clear();
            }
            if row_collapsed {
                let starts_page_fragment = !self.current_page_has_content();
                let placement = table_body_fragment.as_ref().map(|fragment| {
                    Self::table_row_fragment_assignment_placement(
                        fragment,
                        table_x,
                        used_table_width,
                        row_top,
                        0.0,
                        starts_page_fragment,
                    )
                });
                if let Some(placement) = placement {
                    self.capture_table_row_named_string_assignments(
                        row, &row_style, placement, 0.0,
                    );
                    self.capture_table_row_running_cell_assignments(
                        row,
                        &row_style,
                        stylesheets,
                        table_x,
                        row_top,
                        0.0,
                    );
                }
                if let Some(fragment) = &mut table_body_fragment {
                    fragment.push_row(
                        row_index,
                        row_top,
                        0.0,
                        0.0,
                        0.0,
                        true,
                        Self::table_row_source_fragment(placement),
                    );
                }
                previous_row_candidate = next_break_before.avoids_page().then(|| {
                    row_start_candidate
                        .clone()
                        .expect(
                            "row start candidate must be armed when next row has break-before: avoid",
                        )
                        .with_height(0.0)
                });
                avoid_break_candidate = None;
                previous_break_after_avoid = false;
                row_index += 1;
                continue;
            }
            if row_is_running {
                let placement = self.table_row_running_assignment_placement(table_x, row_top);
                if let Some(element) = row.element {
                    self.capture_assignments_for_fragment_source(element, &row_style, placement);
                }
                previous_row_candidate = next_break_before.avoids_page().then(|| {
                    row_start_candidate
                        .clone()
                        .expect(
                            "row start candidate must be armed when next row has break-before: avoid",
                        )
                        .with_height(0.0)
                });
                avoid_break_candidate = None;
                previous_break_after_avoid = false;
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
                table_metrics,
                collapsed_geometry,
            );
            if row_height > self.page_area_height() + 0.01 && !row_kept_by_avoid_group {
                let mut remaining = row_height;
                let mut piece_offset = 0.0;
                while remaining > 0.01 {
                    let reserved_footer_height = if row_is_repeating_footer {
                        0.0
                    } else {
                        current_fragment_repeat_policy
                            .reserved_footer_height(repeating_footer_height)
                    };
                    let available_height =
                        (self.cursor_y - self.page_bottom() - reserved_footer_height).max(0.0);
                    if available_height <= 0.01 {
                        if !row_is_repeating_footer {
                            self.mark_table_body_fragment_repeated_footers(
                                &mut table_body_fragment,
                                current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                                planned_row_heights,
                                planned_row_occupancy,
                                table_metrics,
                            );
                        }
                        self.finalize_table_body_paint_fragment(
                            &mut table_body_fragment,
                            rows,
                            grid,
                            columns,
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            column_plan,
                            table_width,
                            table_metrics,
                            collapsed_geometry,
                            table_is_document_canvas,
                        );
                        if !row_is_repeating_footer {
                            self.layout_repeated_table_footer_rows_at_page_bottom(
                                rows,
                                grid,
                                columns,
                                current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                                style,
                                stylesheets,
                                table_x,
                                used_table_width,
                                table_cellpadding,
                                column_plan,
                                planned_row_heights,
                                planned_row_occupancy,
                                table_width,
                                table_metrics,
                                collapsed_geometry,
                            );
                        }
                        self.push_page();
                        self.cursor_y = self.page_top();
                        current_fragment_repeat_policy = table_fragment_repeat_policy(
                            row_fragment_required_height,
                            self.page_area_height(),
                            repeating_header_height,
                            repeating_footer_height,
                            !row_is_repeating_header,
                            !row_is_repeating_footer,
                        );
                        pending_table_fragment_break_reason =
                            TableFragmentBreakReason::OversizedRowSlice;
                        if !row_is_repeating_header {
                            self.layout_repeated_table_rows(
                                rows,
                                grid,
                                columns,
                                current_fragment_repeat_policy.header_rows(repeating_header_rows),
                                style,
                                stylesheets,
                                table_x,
                                used_table_width,
                                table_cellpadding,
                                column_plan,
                                planned_row_heights,
                                planned_row_occupancy,
                                table_width,
                                table_metrics,
                                collapsed_geometry,
                            );
                            pending_repeated_header_rows = current_fragment_repeat_policy
                                .header_rows(repeating_header_rows)
                                .to_vec();
                        }
                        if self.ensure_table_body_paint_fragment(
                            &mut table_body_fragment,
                            self.cursor_y,
                            pending_table_fragment_break_reason,
                            &pending_repeated_header_rows,
                        ) {
                            pending_table_fragment_break_reason =
                                TableFragmentBreakReason::TableStart;
                            pending_repeated_header_rows.clear();
                        }
                        continue;
                    }

                    let piece_height = remaining.min(available_height);
                    let piece_top = self.cursor_y;
                    if self.ensure_table_body_paint_fragment(
                        &mut table_body_fragment,
                        piece_top,
                        pending_table_fragment_break_reason,
                        &pending_repeated_header_rows,
                    ) {
                        pending_table_fragment_break_reason = TableFragmentBreakReason::TableStart;
                        pending_repeated_header_rows.clear();
                    }
                    let starts_page_fragment = !self.current_page_has_content();
                    let placement = table_body_fragment.as_ref().map(|fragment| {
                        Self::table_row_fragment_assignment_placement(
                            fragment,
                            table_x,
                            used_table_width,
                            piece_top,
                            piece_height,
                            starts_page_fragment,
                        )
                    });
                    if let Some(placement) = placement {
                        self.capture_table_row_named_string_assignments(
                            row,
                            &row_style,
                            placement,
                            piece_offset,
                        );
                        self.capture_table_row_running_cell_assignments(
                            row,
                            &row_style,
                            stylesheets,
                            table_x,
                            piece_top,
                            piece_offset,
                        );
                    }
                    self.layout_table_row_paint_piece(
                        row_index,
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
                        table_metrics,
                        piece_top,
                        row_height,
                        piece_height,
                        piece_offset,
                        collapsed_geometry,
                        row_baseline_offset,
                    );
                    if let Some(fragment) = &mut table_body_fragment {
                        fragment.push_row(
                            row_index,
                            piece_top,
                            piece_height,
                            piece_offset,
                            row_height,
                            !planned_row_occupancy
                                .get(row_index)
                                .copied()
                                .unwrap_or(false),
                            Self::table_row_source_fragment(placement),
                        );
                    }
                    self.cursor_y -= piece_height;
                    remaining -= piece_height;
                    piece_offset += piece_height;

                    if remaining > 0.01 {
                        if !row_is_repeating_footer {
                            self.mark_table_body_fragment_repeated_footers(
                                &mut table_body_fragment,
                                current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                                planned_row_heights,
                                planned_row_occupancy,
                                table_metrics,
                            );
                        }
                        self.finalize_table_body_paint_fragment(
                            &mut table_body_fragment,
                            rows,
                            grid,
                            columns,
                            style,
                            stylesheets,
                            table_x,
                            used_table_width,
                            table_cellpadding,
                            column_plan,
                            table_width,
                            table_metrics,
                            collapsed_geometry,
                            table_is_document_canvas,
                        );
                        if !row_is_repeating_footer {
                            self.layout_repeated_table_footer_rows_at_page_bottom(
                                rows,
                                grid,
                                columns,
                                current_fragment_repeat_policy.footer_rows(repeating_footer_rows),
                                style,
                                stylesheets,
                                table_x,
                                used_table_width,
                                table_cellpadding,
                                column_plan,
                                planned_row_heights,
                                planned_row_occupancy,
                                table_width,
                                table_metrics,
                                collapsed_geometry,
                            );
                        }
                        self.push_page();
                        self.cursor_y = self.page_top();
                        current_fragment_repeat_policy = table_fragment_repeat_policy(
                            row_fragment_required_height,
                            self.page_area_height(),
                            repeating_header_height,
                            repeating_footer_height,
                            !row_is_repeating_header,
                            !row_is_repeating_footer,
                        );
                        pending_table_fragment_break_reason =
                            TableFragmentBreakReason::OversizedRowSlice;
                        if !row_is_repeating_header {
                            self.layout_repeated_table_rows(
                                rows,
                                grid,
                                columns,
                                current_fragment_repeat_policy.header_rows(repeating_header_rows),
                                style,
                                stylesheets,
                                table_x,
                                used_table_width,
                                table_cellpadding,
                                column_plan,
                                planned_row_heights,
                                planned_row_occupancy,
                                table_width,
                                table_metrics,
                                collapsed_geometry,
                            );
                            pending_repeated_header_rows = current_fragment_repeat_policy
                                .header_rows(repeating_header_rows)
                                .to_vec();
                        }
                        if self.ensure_table_body_paint_fragment(
                            &mut table_body_fragment,
                            self.cursor_y,
                            pending_table_fragment_break_reason,
                            &pending_repeated_header_rows,
                        ) {
                            pending_table_fragment_break_reason =
                                TableFragmentBreakReason::TableStart;
                            pending_repeated_header_rows.clear();
                        }
                    }
                }
            } else {
                let starts_page_fragment = !self.current_page_has_content();
                let placement = table_body_fragment.as_ref().map(|fragment| {
                    Self::table_row_fragment_assignment_placement(
                        fragment,
                        table_x,
                        used_table_width,
                        row_top,
                        row_height,
                        starts_page_fragment,
                    )
                });
                if let Some(placement) = placement {
                    self.capture_table_row_named_string_assignments(
                        row, &row_style, placement, 0.0,
                    );
                    self.capture_table_row_running_cell_assignments(
                        row,
                        &row_style,
                        stylesheets,
                        table_x,
                        row_top,
                        0.0,
                    );
                }
                self.layout_table_row_paint_piece(
                    row_index,
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
                    table_metrics,
                    row_top,
                    row_height,
                    row_height,
                    0.0,
                    collapsed_geometry,
                    row_baseline_offset,
                );
                if let Some(fragment) = &mut table_body_fragment {
                    fragment.push_row(
                        row_index,
                        row_top,
                        row_height,
                        0.0,
                        row_height,
                        !planned_row_occupancy
                            .get(row_index)
                            .copied()
                            .unwrap_or(false),
                        Self::table_row_source_fragment(placement),
                    );
                }
                self.cursor_y -= row_height;
            }
            if planned_row_occupancy
                .get(row_index)
                .copied()
                .unwrap_or(false)
                && planned_row_occupancy
                    .get(row_index + 1..)
                    .is_some_and(|following| following.iter().any(|occupied| *occupied))
            {
                self.cursor_y -= table_metrics.spacing.vertical.length_points();
            }
            if break_after.is_forced() {
                if row_index + 1 < rows.len() {
                    page_break_before_next_row = break_after;
                } else {
                    forced_break_after_table_rows = break_after;
                }
            }
            let row_candidate = if previous_break_after_avoid {
                let this = avoid_break_candidate.clone().unwrap_or_else(|| {
                    row_start_candidate.clone().expect(
                        "row start candidate must be armed when an avoid run fallback is needed",
                    )
                });
                let height = avoid_break_candidate
                    .as_ref()
                    .map(|candidate| candidate.height())
                    .unwrap_or(0.0)
                    + row_height;
                Some(this.with_height(height))
            } else if break_after.avoids_page() || next_break_before.avoids_page() {
                Some(
                    row_start_candidate
                        .clone()
                        .expect(
                            "row start candidate must be armed when this row can become a table break candidate",
                        )
                        .with_height(row_height),
                )
            } else {
                None
            };
            previous_row_candidate = next_break_before.avoids_page().then(|| {
                row_candidate
                    .clone()
                    .expect("table break candidate must exist for next row break-before: avoid")
            });
            if break_after.avoids_page() {
                avoid_break_candidate = Some(
                    row_candidate.expect("table break candidate must exist for break-after: avoid"),
                );
            } else {
                avoid_break_candidate = None;
            }
            previous_break_after_avoid = break_after.avoids_page();
            row_index += 1;
            if unfragmented_avoid_row_group_end.is_some_and(|end| row_index >= end) {
                unfragmented_avoid_row_group_end = None;
            }
        }

        (
            table_body_fragment,
            forced_break_after_table_rows,
            current_fragment_repeat_policy,
        )
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
                let group_value = (group_style.page_name, true);
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
