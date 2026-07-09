use super::shared::*;
use super::state::{BlockFlowChildTraversalState, ChildFlowTraversalOutcome};
use super::*;

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_formatting_box_flow_children(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: &[box_tree::FormattingBox<'_>],
        can_collapse_start_margin: bool,
        can_collapse_end_margin: bool,
        applied_start_margin: LayoutLength,
        starts_at_page_top: bool,
        traversal_state: &mut BlockFlowChildTraversalState,
    ) -> ChildFlowTraversalOutcome {
        let mut collapsed_end_margin = false;
        let mut pending_end_margin_collapse = None;
        let mut collapsed_start_margin_offset = layout_pt(0.0);
        let mut previous_flow_bottom_margin = None;
        let mut seen_flow_child = false;
        let mut trim_block_start_adjoining_margins = style.margin_trim.block_start;
        let mut float_run = self.float_run_state();
        let mut previous_child_page_end: Option<Option<String>> = None;
        let mut avoid_run_candidate: Option<AvoidBreakRunCandidate> = None;
        let mut previous_break_after = PageBreak::Auto;
        let mut adjoining_float_replay: Option<AdjoiningFloatReplayCandidate> = None;
        let mut replaying_adjoining_until: Option<usize> = None;
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
        let mut first_formatted_line = FirstFormattedLineState::for_style(style);
        // `line-clamp` limits the generated line boxes of the block
        // container, including line boxes formed by its anonymous
        // inline runs and nested in-flow blocks. Carry the remaining
        // allowance in document order so a descendant receives only
        // the capacity left by preceding siblings.
        // <https://drafts.csswg.org/css-overflow-4/#propdef-line-clamp>
        let text_box_trim_start_child = text_box_line_trim
            .trims_block_start
            .then(|| text_box_trim_formatting_box_child_index(child_boxes, true, false))
            .flatten();
        let text_box_trim_end_child = text_box_line_trim
            .trims_block_end
            .then(|| text_box_trim_formatting_box_child_index(child_boxes, false, true))
            .flatten();
        let multicol_text_box_trim_end_children = (fragmentainer_kind == FragmentainerKind::Column)
            // Every anonymous column needs the same source-child
            // endpoint map. Consuming it on the first column leaves
            // continuation columns unable to apply trim-end.
            // https://www.w3.org/TR/css-inline-3/#text-box-trim
            .then(|| self.multicol_text_box_trim_end_child_indices.clone())
            .flatten();
        let mut child_box_index = 0usize;
        while child_box_index < child_boxes.len() {
            let replayed_adjoining_origin_y = if replaying_adjoining_until == Some(child_box_index)
            {
                replaying_adjoining_until = None;
                self.adjoining_float_origin_y.take()
            } else {
                None
            };
            let raw_child_box = &child_boxes[child_box_index];
            if traversal_state.is_exhausted() {
                // Later in-flow content is outside the clamp's
                // fragmentainer and therefore generates neither
                // boxes nor paint in this pass.
                child_box_index += 1;
                continue;
            }
            let (split_block_context, child_box) = match raw_child_box {
                box_tree::FormattingBox::InlineSplitBlockContext(context)
                    if context.children.len() == 1 =>
                {
                    (Some(context), &context.children[0])
                }
                _ => (None, raw_child_box),
            };
            let child_text_box_line_trim = TextBoxLineTrim {
                trims_block_start: text_box_trim_start_child == Some(child_box_index),
                trims_block_end: text_box_trim_end_child == Some(child_box_index)
                    || multicol_text_box_trim_end_children
                        .as_ref()
                        .is_some_and(|indices| indices.contains(&child_box_index)),
                block_start: if text_box_trim_start_child == Some(child_box_index) {
                    text_box_line_trim.block_start
                } else {
                    0.0
                },
                block_end: if text_box_trim_end_child == Some(child_box_index)
                    || multicol_text_box_trim_end_children
                        .as_ref()
                        .is_some_and(|indices| indices.contains(&child_box_index))
                {
                    text_box_line_trim.block_end
                } else {
                    0.0
                },
            };
            let pending_child_start_candidate = PendingAvoidBreakRunCandidate {
                meta: AvoidBreakRunCandidateMeta {
                    index: child_box_index,
                    element_index: 0,
                    previous_flow_bottom_margin,
                    seen_flow_child,
                    trim_block_start_adjoining_margins,
                    collapsed_end_margin,
                    previous_child_page_end: previous_child_page_end.clone(),
                    float_run,
                    remaining_line_clamp: traversal_state.capture_avoid_replay(),
                    height: 0.0,
                },
            };
            let child_parts = child_box.element_parts();
            let child_avoid_break_flow =
                child_parts.is_some_and(|(child_element, _, child_style, _)| {
                    block_avoid_break_flow_child(element, child_element, child_style)
                });
            let next_flow_child_break_before =
                next_formatting_box_flow_child_break_before(element, child_boxes, child_box_index);
            let child_break_context = child_parts
                .map(|(_, _, child_style, _)| {
                    FragmentBreakContext::new(
                        PageBreak::Auto,
                        child_style.break_before,
                        child_style.break_after,
                        next_flow_child_break_before.unwrap_or(PageBreak::Auto),
                    )
                })
                .unwrap_or_else(|| {
                    FragmentBreakContext::new(
                        PageBreak::Auto,
                        PageBreak::Auto,
                        PageBreak::Auto,
                        PageBreak::Auto,
                    )
                });
            let child_break_opportunity = FragmentBreakOpportunity::before_box_boundary(
                fragmentainer_kind,
                child_box_index as f32,
                child_break_context,
                previous_break_after,
                false,
            );
            let avoid_run_start_decision =
                FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
                    participates_in_flow: child_avoid_break_flow,
                    fragmentainer_kind,
                    break_context: child_break_context,
                    break_opportunity: child_break_opportunity,
                    next_break_before: next_flow_child_break_before,
                    has_avoid_run_candidate: avoid_run_candidate.is_some(),
                });
            let child_start_candidate = avoid_run_start_decision
                .should_arm_start_candidate
                .then(|| pending_child_start_candidate.arm(self));
            if std::env::var_os("QUIRE_TRACE_FLOATS").is_some() && child_start_candidate.is_some() {
                eprintln!(
                    "arm avoid parent={} parent_id={:?} left={} right={} cursor={}",
                    element.tag,
                    element.attrs.get("id"),
                    self.content_left,
                    self.content_right,
                    self.cursor_y,
                );
            }
            let zero_height_page_boundary = formatting_box_is_zero_height_page_boundary(child_box);
            // A normal-flow anonymous wrapper containing only an
            // out-of-flow descendant is itself transparent to named
            // page boundaries. Requiring an actual `page`-applying
            // participant keeps that wrapper from restoring its
            // parent page group after a preceding named child.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            let child_page_value_sources = formatting_box_is_page_value_participant(child_box)
                .then(|| formatting_box_page_value_sources(child_box));
            let effective_child_page_start = if zero_height_page_boundary {
                Some(coalesced_zero_height_page_start(
                    child_boxes,
                    child_box_index,
                ))
            } else {
                child_page_value_sources
                    .as_ref()
                    .map(|(child_page_start, _)| {
                        self.page_boundary_name_in_active_scope(child_page_start.clone(), style)
                    })
            };
            if let Some(child_page_start) = &effective_child_page_start
                && let Some(previous_page_end) = &previous_child_page_end
                && previous_page_end != child_page_start
            {
                self.switch_page_name_at_class_a_boundary(child_page_start.as_deref());
            }
            if let box_tree::FormattingBox::AnonymousBlock(box_) = child_box {
                self.flush_float_run(&mut float_run);
                let allow_typographic_first_line =
                    first_formatted_line.applies_to_next_inline_run();
                let originating_pseudo_style = allow_typographic_first_line
                    .then(|| style_with_originating_typographic_pseudos(&box_.style, style))
                    .flatten();
                let anonymous_style = originating_pseudo_style.as_ref().unwrap_or(&box_.style);
                let clamped_anonymous_style = traversal_state.style_with_remaining(anonymous_style);
                let anonymous_style = clamped_anonymous_style.as_ref().unwrap_or(anonymous_style);
                let inline_outcome =
                    self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                        layout.layout_anonymous_block_with_first_line_policy(
                            anonymous_style,
                            &box_.children,
                            stylesheets,
                            None,
                            allow_typographic_first_line,
                        )
                    });
                if inline_outcome.has_flow_effects {
                    seen_flow_child = true;
                }
                if inline_outcome.has_non_phantom_line {
                    first_formatted_line.consume_next_formatted_line();
                }
                traversal_state.debit(inline_outcome.clamp_line_slots);
                self.flush_float_run(&mut float_run);
                if inline_outcome.has_flow_effects {
                    trim_block_start_adjoining_margins = false;
                    previous_flow_bottom_margin = None;
                    avoid_run_candidate = None;
                    previous_break_after = PageBreak::Auto;
                }
                if zero_height_page_boundary {
                    if let Some(child_page_start) = effective_child_page_start {
                        previous_child_page_end = Some(child_page_start);
                    }
                } else if let Some((_, child_page_end)) = child_page_value_sources {
                    // The next class-A sibling compares against this
                    // child's propagated *end* value. Its start can
                    // remain `auto` while a later descendant selects
                    // a named page, in which case using the start here
                    // would lose the required destination transition.
                    // <https://www.w3.org/TR/css-page-3/#using-named-pages>
                    previous_child_page_end =
                        Some(self.page_boundary_name_in_active_scope(child_page_end, style));
                }
                child_box_index += 1;
                continue;
            }
            let Some((child_element, child_signature, child_style, child_children)) = child_parts
            else {
                child_box_index += 1;
                continue;
            };
            let mut child_style = Box::new(child_style.clone());
            traversal_state.apply_to(&mut child_style);
            child_style.apply_effective_zoom();
            let child_table_fragment = if let box_tree::FormattingBox::Table(table_box) = child_box
            {
                Some(&table_box.fragment)
            } else {
                None
            };
            apply_pending_normal_flow_margin_before_float(
                &mut child_style,
                previous_flow_bottom_margin,
            );
            let laid_out_float = if let Some(split_block_context) = split_block_context {
                self.layout_floating_child_in_inline_split_block_context(
                    split_block_context,
                    child_element,
                    child_signature.clone(),
                    &child_style,
                    Some(child_children),
                    child_table_fragment,
                    stylesheets,
                    &mut float_run,
                )
            } else {
                self.layout_floating_child(
                    child_element,
                    child_signature.clone(),
                    &child_style,
                    Some(child_children),
                    child_table_fragment,
                    stylesheets,
                    &mut float_run,
                )
            };
            if laid_out_float {
                seen_flow_child = true;
                previous_flow_bottom_margin = None;
                // A float is taken out of normal flow, so it does not
                // terminate the class A break relationship between the
                // normal-flow block siblings on either side of it. In
                // particular, a later `break-before: avoid` must still
                // be able to roll the preceding in-flow sibling (and
                // this float) into the next fragmentainer.
                // <https://www.w3.org/TR/css-break-3/#possible-breaks>
                child_box_index += 1;
                continue;
            }
            self.flush_float_run(&mut float_run);
            let is_flow_child = block_avoid_break_flow_child(element, child_element, &child_style);
            let descendant_start_margin = (is_flow_child
                && can_collapse_block_start_margin(
                    &child_style,
                    used_border_widths(&child_style),
                    has_direct_inline_content_box(child_children),
                    self.used_overflow_for_element(child_element, &child_style),
                ))
            .then(|| {
                collapsible_first_child_start_margin_from_boxes(
                    child_children,
                    child_element,
                    &child_style,
                    self.document_canvas_overflow,
                )
            })
            .flatten();
            let self_collapsing_child = is_flow_child
                && is_self_collapsing_block_box(
                    child_element,
                    &child_style,
                    child_children,
                    self.document_canvas_overflow,
                );
            let self_collapsing_margin_set = self_collapsing_child.then(|| {
                self_collapsing_block_margin_set_for_box(&child_style, descendant_start_margin)
            });
            let effective_start_margin = self_collapsing_margin_set.unwrap_or_else(|| {
                descendant_start_margin
                    .map(|descendant| {
                        collapse_margins(layout_pt(child_style.margin.top), layout_pt(descendant))
                            .points()
                    })
                    .unwrap_or(child_style.margin.top)
            });
            let descendant_margin_adjustment = if self_collapsing_child {
                0.0
            } else {
                descendant_start_margin.unwrap_or(0.0)
            };
            if let Some(collapsed_margin) = self_collapsing_margin_set {
                child_style.margin.top = collapsed_margin;
                child_style.margin.bottom = 0.0;
            }

            let trimmed_block_start_margin = is_flow_child
                && trim_adjoining_block_start_margin(
                    style,
                    &mut child_style,
                    trim_block_start_adjoining_margins,
                    descendant_start_margin,
                );
            if trimmed_block_start_margin && self_collapsing_child {
                child_style.margin.bottom = 0.0;
            }

            let mut collapses_with_parent_end = false;
            if is_flow_child {
                let collapses_with_parent = is_collapsible_block_child(child_element, &child_style);
                let collapses_with_sibling =
                    is_sibling_margin_collapsible_block_child(child_element, &child_style);
                if !trimmed_block_start_margin
                    && !seen_flow_child
                    && can_collapse_start_margin
                    && collapses_with_parent
                {
                    if let Some(previous_margin) = previous_flow_bottom_margin {
                        // An earlier self-collapsing sibling has
                        // already contributed its adjoining margin at
                        // the parent start. Its following sibling
                        // supplies only the delta of the merged
                        // margin set, not the entire start margin a
                        // second time.
                        child_style.margin.top = collapsed_margin_delta(
                            layout_pt(previous_margin),
                            layout_pt(effective_start_margin),
                        )
                        .points()
                            - descendant_margin_adjustment;
                    } else {
                        child_style.margin.top = collapsed_start_margin_delta(
                            applied_start_margin,
                            layout_pt(effective_start_margin),
                            starts_at_page_top,
                        )
                        .points()
                            - descendant_margin_adjustment;
                    }
                } else if !trimmed_block_start_margin
                    && collapses_with_sibling
                    && FragmentBreakContext::for_standalone_box(&child_style)
                        .forced_break_before_in(fragmentainer_kind)
                        .is_none()
                    && let Some(previous_margin) = previous_flow_bottom_margin
                {
                    child_style.margin.top = collapsed_margin_delta(
                        layout_pt(previous_margin),
                        layout_pt(effective_start_margin),
                    )
                    .points()
                        - descendant_margin_adjustment;
                }

                collapses_with_parent_end = collapses_with_parent
                    && can_collapse_end_margin
                    && !has_later_normal_block_flow_box_child(
                        child_boxes,
                        child_box_index + 1,
                        element,
                    );
            }
            if let Some(origin_y) = replayed_adjoining_origin_y {
                let start_offset = (self.cursor_y - origin_y).max(0.0);
                if start_offset > child_style.margin.top {
                    child_style.margin.top = start_offset;
                }
                collapsed_start_margin_offset =
                    collapsed_start_margin_offset.max(layout_pt(start_offset));
            }
            preserve_adjusted_block_margins(&mut child_style);

            let available_outer_width = (self.content_right
                - self.content_left
                - child_style.margin.left
                - child_style.margin.right)
                .max(child_style.font_size);
            let child_estimated_height = self.estimate_element_height(
                child_element,
                &child_style,
                stylesheets,
                available_outer_width,
                Some(child_children),
            );
            let mut run_start_candidate = if is_flow_child {
                if avoid_run_start_decision.is_avoid_boundary {
                    Some(avoid_run_candidate.take().unwrap_or_else(|| {
                        child_start_candidate
                            .expect("block child start candidate must be armed when avoid boundary has no existing run candidate")
                    }))
                } else if avoid_run_start_decision.seeds_later_avoid_boundary {
                    Some(
                        child_start_candidate
                            .expect("block child start candidate must be armed when this child can seed a later avoid boundary"),
                    )
                } else {
                    None
                }
            } else {
                None
            };
            if is_flow_child
                && avoid_run_start_decision.is_avoid_boundary
                && let Some(child_height) = child_estimated_height
                && let Some(candidate) = run_start_candidate.as_ref()
                && should_move_avoid_break_run_to_next_fragmentainer(
                    candidate.height(),
                    child_height.min(
                        child_style.margin.top
                            + child_style.line_height * child_style.orphans.max(1) as f32,
                    ),
                    Fragmentainer::from_cursor_bounds(
                        self.page_area_height(),
                        self.cursor_y,
                        self.page_bottom(),
                    ),
                    self.cursor_is_at_page_top(),
                )
            {
                if std::env::var_os("QUIRE_TRACE_FLOATS").is_some() {
                    eprintln!(
                        "avoid replay parent={} parent_id={:?} child={} id={:?} previous={:?} before={:?} after={:?} next={:?} cursor={} width={} height={}",
                        element.tag,
                        element.attrs.get("id"),
                        child_element.tag,
                        child_element.attrs.get("id"),
                        previous_break_after,
                        child_style.break_before,
                        child_style.break_after,
                        next_flow_child_break_before,
                        self.cursor_y,
                        available_outer_width,
                        child_height,
                    );
                }
                let run_start_candidate = run_start_candidate
                    .take()
                    .expect("block child avoid boundary must have an armed run candidate");
                let AvoidBreakRunCandidateMeta {
                    index,
                    element_index: _,
                    previous_flow_bottom_margin: saved_previous_flow_bottom_margin,
                    seen_flow_child: saved_seen_flow_child,
                    trim_block_start_adjoining_margins: saved_trim_block_start_adjoining_margins,
                    collapsed_end_margin: saved_collapsed_end_margin,
                    previous_child_page_end: saved_previous_child_page_end,
                    float_run: saved_float_run,
                    remaining_line_clamp: saved_remaining_line_clamp,
                    height: _,
                } = run_start_candidate.restore(self);
                previous_flow_bottom_margin = saved_previous_flow_bottom_margin;
                seen_flow_child = saved_seen_flow_child;
                trim_block_start_adjoining_margins = saved_trim_block_start_adjoining_margins;
                collapsed_end_margin = saved_collapsed_end_margin;
                previous_child_page_end = saved_previous_child_page_end;
                float_run = saved_float_run;
                traversal_state.restore_avoid_replay(saved_remaining_line_clamp);
                child_box_index = index;
                avoid_run_candidate = None;
                previous_break_after = PageBreak::Auto;
                self.push_page_if_nonempty();
                continue;
            }
            let closes_adjoining_float_replay = is_flow_child
                && !self_collapsing_child
                && previous_flow_bottom_margin.is_some()
                && replayed_adjoining_origin_y.is_none();
            if closes_adjoining_float_replay && let Some(replay) = adjoining_float_replay.take() {
                let replay_origin_y = self.cursor_y - child_style.margin.top;
                let replay_separates = self.adjoining_float_replay_separated_by_bfc_root(
                    &replay,
                    child_element,
                    &child_style,
                    stylesheets,
                    Some(child_children),
                    replay_origin_y,
                );
                if !replay_separates
                    && (replay_origin_y - replay.snapshot_cursor_y()).abs() > FLOAT_EPSILON
                {
                    let replay_until = child_box_index;
                    let replay_meta = replay.restore(self);
                    previous_flow_bottom_margin = replay_meta.previous_flow_bottom_margin;
                    seen_flow_child = replay_meta.seen_flow_child;
                    trim_block_start_adjoining_margins =
                        replay_meta.trim_block_start_adjoining_margins;
                    collapsed_end_margin = replay_meta.collapsed_end_margin;
                    previous_child_page_end = replay_meta.previous_child_page_end;
                    float_run = replay_meta.float_run;
                    previous_break_after = replay_meta.previous_break_after;
                    avoid_run_candidate = None;
                    child_box_index = replay_meta.index;
                    self.adjoining_float_origin_y = Some(replay_origin_y);
                    replaying_adjoining_until = Some(replay_until);
                    continue;
                }
            }

            let child_style_with_superseded_break_after = child_break_context
                .next_forced_break_supersedes_after_in(fragmentainer_kind)
                .then(|| {
                    let mut style = child_style.clone();
                    style.break_after = PageBreak::Auto;
                    style
                });
            let child_style = child_style_with_superseded_break_after
                .as_deref()
                .unwrap_or(&child_style);
            let adjoining_candidate = if child_style.float == Float::None
                && self_collapsing_child
                && adjoining_float_replay.is_none()
                && replaying_adjoining_until.is_none()
            {
                Some(
                    PendingAdjoiningFloatReplayCandidate {
                        meta: AdjoiningFloatReplayCandidateMeta {
                            index: child_box_index,
                            element_index: 0,
                            previous_flow_bottom_margin,
                            seen_flow_child,
                            trim_block_start_adjoining_margins,
                            collapsed_end_margin,
                            previous_child_page_end: previous_child_page_end.clone(),
                            float_run,
                            previous_break_after,
                        },
                    }
                    .arm(self),
                )
            } else {
                None
            };
            let float_shape_count_before = self
                .float_contexts
                .last()
                .map(|context| context.shapes.len())
                .unwrap_or(0);

            if is_flow_child {
                if !self_collapsing_child {
                    seen_flow_child = true;
                    first_formatted_line.consume_next_formatted_line();
                }
                if trim_block_start_adjoining_margins && !self_collapsing_child {
                    trim_block_start_adjoining_margins = false;
                }
            } else {
                previous_flow_bottom_margin = None;
            }

            let child_uses_block_layout = matches!(
                element_layout_kind(child_element, child_style),
                ElementLayoutKind::BlockFlow
            );
            let split_inline_static_y_offset =
                matches!(child_style.position, Position::Absolute | Position::Fixed)
                    .then(|| {
                        self.split_inline_static_position_y_offset_before_child(
                            child_boxes,
                            child_box_index,
                            style,
                            stylesheets,
                        )
                    })
                    .flatten();
            self.last_block_layout_outcome = BlockLayoutOutcome::default();
            if child_style.display.is_block_level()
                || is_document_canvas_element(element)
                || is_replaced_element(child_element)
            {
                let previous_block_static_position_y_offset =
                    if matches!(child_style.position, Position::Absolute | Position::Fixed) {
                        let previous = self.block_static_position_y_offset;
                        self.block_static_position_y_offset = split_inline_static_y_offset;
                        Some(previous)
                    } else if let Some(offset) = split_inline_static_y_offset {
                        let previous = self.block_static_position_y_offset;
                        self.block_static_position_y_offset = Some(offset);
                        Some(previous)
                    } else {
                        None
                    };
                self.push_ancestor_signature(child_signature.clone());
                // The parent has already selected this normal-flow
                // child's page group at the class-A sibling boundary
                // above. Re-entering a descendant-derived page value
                // here can manufacture a second boundary even when
                // this is the first in-flow child of its own block.
                // Nested sibling boundaries remain active while this
                // element-entry scope is suppressed.
                // <https://www.w3.org/TR/css-page-3/#using-named-pages>
                let boundary_selected_page_scope = is_flow_child && !zero_height_page_boundary;
                if boundary_selected_page_scope {
                    self.push_page_name_element_scope_suppression();
                }
                if zero_height_page_boundary {
                    self.push_page_name_element_scope_suppression();
                }
                if let box_tree::FormattingBox::Table(table_box) = child_box {
                    let split_scope =
                        split_block_context.map(|_| self.begin_inline_split_block_paint_scope());
                    self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                        layout.layout_element_with_child_boxes_and_table_fragment(
                            table_box.element,
                            child_style,
                            stylesheets,
                            Some(&table_box.children),
                            Some(&table_box.fragment),
                        );
                    });
                    if let (Some(context), Some(scope)) = (split_block_context, split_scope) {
                        self.finish_inline_split_block_paint_scope(context, scope);
                    }
                } else if let box_tree::FormattingBox::Block(block_box) = child_box {
                    let split_scope =
                        split_block_context.map(|_| self.begin_inline_split_block_paint_scope());
                    self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                        if let box_tree::BoxSource::GeneratedPseudo(pseudo) = &block_box.source {
                            layout.layout_generated_pseudo_box(
                                child_element,
                                child_style,
                                match pseudo.kind {
                                    box_tree::GeneratedPseudoKind::Before => {
                                        box_tree::CounterEventSource::Before
                                    }
                                    box_tree::GeneratedPseudoKind::After => {
                                        box_tree::CounterEventSource::After
                                    }
                                },
                                stylesheets,
                                &block_box.run_in_children,
                                Some(child_children),
                                None,
                            );
                        } else {
                            layout.layout_element_with_child_boxes_and_run_ins(
                                child_element,
                                child_style,
                                stylesheets,
                                &block_box.run_in_children,
                                Some(child_children),
                            );
                        }
                    });
                    if let (Some(context), Some(scope)) = (split_block_context, split_scope) {
                        self.finish_inline_split_block_paint_scope(context, scope);
                    }
                } else {
                    let split_scope =
                        split_block_context.map(|_| self.begin_inline_split_block_paint_scope());
                    self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                        layout.layout_element_with_child_boxes(
                            child_element,
                            child_style,
                            stylesheets,
                            Some(child_children),
                        );
                    });
                    if let (Some(context), Some(scope)) = (split_block_context, split_scope) {
                        self.finish_inline_split_block_paint_scope(context, scope);
                    }
                }
                if zero_height_page_boundary {
                    self.pop_page_name_element_scope_suppression();
                }
                if boundary_selected_page_scope {
                    self.pop_page_name_element_scope_suppression();
                }
                self.ancestors.pop();
                if let Some(previous) = previous_block_static_position_y_offset {
                    self.block_static_position_y_offset = previous;
                }
                self.flush_float_run(&mut float_run);
            }
            let emitted_float = self
                .float_contexts
                .last()
                .is_some_and(|context| context.shapes.len() > float_shape_count_before);
            if emitted_float && let Some(candidate) = adjoining_candidate {
                adjoining_float_replay = Some(candidate);
            }
            if is_flow_child {
                let child_consumed_bottom_margin = if child_uses_block_layout {
                    self.last_block_layout_outcome
                        .consumed_bottom_margin
                        .points()
                } else {
                    child_style.margin.bottom
                };
                if collapses_with_parent_end {
                    pending_end_margin_collapse = Some(BlockEndMarginCollapse {
                        child_consumed_margin: layout_pt(child_consumed_bottom_margin),
                        collapsed_margin: collapse_margins(
                            layout_pt(child_consumed_bottom_margin),
                            layout_pt(style.margin.bottom),
                        ),
                    });
                }
                previous_flow_bottom_margin = if self_collapsing_child {
                    Some(if trimmed_block_start_margin {
                        0.0
                    } else {
                        previous_flow_bottom_margin
                            .map(|previous| {
                                collapse_margins(
                                    layout_pt(previous),
                                    layout_pt(effective_start_margin),
                                )
                                .points()
                            })
                            .unwrap_or(effective_start_margin)
                    })
                } else {
                    is_sibling_margin_collapsible_block_child(child_element, child_style)
                        .then_some(child_consumed_bottom_margin)
                };
                if child_uses_block_layout {
                    traversal_state.record_descendant_clamp_line_slots(
                        self.last_block_layout_outcome.clamp_line_slots,
                    );
                }
            }
            if child_uses_block_layout {
                traversal_state.debit(self.last_block_layout_outcome.clamp_line_slots);
            }
            if zero_height_page_boundary {
                if let Some(child_page_start) = effective_child_page_start {
                    previous_child_page_end = Some(child_page_start);
                }
            } else if let Some((_, child_page_end)) = child_page_value_sources {
                previous_child_page_end =
                    Some(self.page_boundary_name_in_active_scope(child_page_end, style));
            }
            avoid_run_candidate = if avoid_run_start_decision.seeds_later_avoid_boundary {
                child_estimated_height.and_then(|child_height| {
                    run_start_candidate
                        .take()
                        .map(|candidate| candidate.add_height(child_height))
                })
            } else {
                None
            };
            previous_break_after = if is_flow_child {
                child_break_context
                    .avoid_after_in(fragmentainer_kind)
                    .unwrap_or(PageBreak::Auto)
            } else {
                PageBreak::Auto
            };
            child_box_index += 1;
        }
        self.flush_float_run(&mut float_run);
        ChildFlowTraversalOutcome {
            pending_end_margin_collapse,
            collapsed_start_margin_offset,
        }
    }
}
