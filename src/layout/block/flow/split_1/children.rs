use super::*;
use crate::css::Edges;
use crate::layout::block::float::FLOAT_EPSILON;
use crate::layout::inline_collect::IntrinsicInlineCollectionContext;

impl<'a> LayoutBuilder<'a> {
    #[expect(
        clippy::boxed_local,
        reason = "This large debug-build layout frame stays under the normal worker stack limit."
    )]
    pub(in crate::layout) fn layout_block_flow_children_phase(
        &mut self,
        input: Box<BlockFlowChildrenPhaseInput<'_, '_>>,
    ) -> BlockFlowChildrenPhaseOutcome {
        let BlockFlowChildrenPhaseInput {
            element,
            style,
            stylesheets,
            child_boxes,
            can_collapse_start_margin,
            can_collapse_end_margin,
            applied_start_margin,
            starts_at_page_top,
            laid_out_column_children,
            use_box_inline_items,
            use_ordered_mixed_flow,
            definite_content_height,
        } = *input;
        let mut collapsed_end_margin = false;
        let mut pending_end_margin_collapse = None;
        let mut collapsed_start_margin_offset = 0.0f32;
        let mut previous_flow_bottom_margin = None;
        let mut seen_flow_child = false;
        let mut trim_block_start_adjoining_margins = style.margin_trim.block_start;
        self.definite_block_size_stack.push(definite_content_height);

        if !laid_out_column_children && !use_box_inline_items {
            if use_ordered_mixed_flow {
                pending_end_margin_collapse = self.layout_ordered_mixed_flow_children(
                    element,
                    style,
                    stylesheets,
                    can_collapse_start_margin,
                    can_collapse_end_margin,
                );
            } else if let Some(child_boxes) = child_boxes {
                let mut float_run = self.float_run_state();
                let mut previous_child_page_end: Option<Option<String>> = None;
                let mut avoid_run_candidate: Option<AvoidBreakRunCandidate> = None;
                let mut previous_break_after_avoid = false;
                let mut adjoining_float_replay: Option<AdjoiningFloatReplayCandidate> = None;
                let mut replaying_adjoining_until: Option<usize> = None;
                let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
                let mut first_formatted_line = FirstFormattedLineState::for_style(style);
                let text_box_trim_start_child = text_box_line_trim
                    .trims_block_start
                    .then(|| text_box_trim_formatting_box_child_index(child_boxes, true, false))
                    .flatten();
                let text_box_trim_end_child = text_box_line_trim
                    .trims_block_end
                    .then(|| text_box_trim_formatting_box_child_index(child_boxes, false, true))
                    .flatten();
                let mut child_box_index = 0usize;
                while child_box_index < child_boxes.len() {
                    let replayed_adjoining_origin_y =
                        if replaying_adjoining_until == Some(child_box_index) {
                            replaying_adjoining_until = None;
                            self.adjoining_float_origin_y.take()
                        } else {
                            None
                        };
                    let raw_child_box = &child_boxes[child_box_index];
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
                        trims_block_end: text_box_trim_end_child == Some(child_box_index),
                        block_start: if text_box_trim_start_child == Some(child_box_index) {
                            text_box_line_trim.block_start
                        } else {
                            0.0
                        },
                        block_end: if text_box_trim_end_child == Some(child_box_index) {
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
                            height: 0.0,
                        },
                    };
                    let child_parts = child_box.element_parts();
                    let child_avoid_break_flow =
                        child_parts.is_some_and(|(child_element, _, child_style, _)| {
                            block_avoid_break_flow_child(element, child_element, child_style)
                        });
                    let child_break_before_avoid =
                        child_parts.is_some_and(|(_, _, child_style, _)| {
                            child_style.break_before.avoids_page()
                        });
                    let child_break_after_avoid =
                        child_parts.is_some_and(|(_, _, child_style, _)| {
                            child_style.break_after.avoids_page()
                        });
                    let next_flow_child_break_before_avoid =
                        next_formatting_box_flow_child_break_before_avoid(
                            element,
                            child_boxes,
                            child_box_index,
                        );
                    let child_start_candidate = should_arm_block_avoid_break_child_start(
                        child_avoid_break_flow,
                        child_break_before_avoid,
                        child_break_after_avoid,
                        next_flow_child_break_before_avoid,
                        previous_break_after_avoid,
                        avoid_run_candidate.is_some(),
                    )
                    .then(|| pending_child_start_candidate.arm(self));
                    let zero_height_page_boundary =
                        formatting_box_is_zero_height_page_boundary(child_box);
                    let child_page_value_sources = formatting_box_is_in_normal_flow(child_box)
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
                                page_boundary_name_in_parent_scope(child_page_start.clone(), style)
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
                        self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                            layout.layout_anonymous_block_with_first_line_policy(
                                &box_.style,
                                &box_.children,
                                stylesheets,
                                None,
                                allow_typographic_first_line,
                            );
                        });
                        first_formatted_line.consume_next_formatted_line();
                        self.flush_float_run(&mut float_run);
                        trim_block_start_adjoining_margins = false;
                        previous_flow_bottom_margin = None;
                        avoid_run_candidate = None;
                        previous_break_after_avoid = false;
                        if let Some(child_page_start) = effective_child_page_start {
                            previous_child_page_end = Some(child_page_start);
                        } else if let Some((_, child_page_end)) = child_page_value_sources {
                            previous_child_page_end =
                                Some(page_boundary_name_in_parent_scope(child_page_end, style));
                        }
                        child_box_index += 1;
                        continue;
                    }
                    let Some((child_element, child_signature, child_style, child_children)) =
                        child_parts
                    else {
                        child_box_index += 1;
                        continue;
                    };
                    let mut child_style = Box::new(child_style.clone());
                    let child_table_fragment =
                        if let box_tree::FormattingBox::Table(table_box) = child_box {
                            Some(&table_box.fragment)
                        } else {
                            None
                        };
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
                        avoid_run_candidate = None;
                        previous_break_after_avoid = false;
                        child_box_index += 1;
                        continue;
                    }
                    self.flush_float_run(&mut float_run);
                    let is_flow_child =
                        block_avoid_break_flow_child(element, child_element, &child_style);
                    let descendant_start_margin = (is_flow_child
                        && can_collapse_block_start_margin(
                            &child_style,
                            used_border_widths(&child_style),
                            has_direct_inline_content_box(child_children),
                        ))
                    .then(|| {
                        collapsible_first_child_start_margin_from_boxes(
                            child_children,
                            child_element,
                            &child_style,
                        )
                    })
                    .flatten();
                    let self_collapsing_child = is_flow_child
                        && is_self_collapsing_block_box(
                            child_element,
                            &child_style,
                            child_children,
                        );
                    let self_collapsing_margin_set = self_collapsing_child.then(|| {
                        self_collapsing_block_margin_set_for_box(
                            &child_style,
                            descendant_start_margin,
                        )
                    });
                    let effective_start_margin = self_collapsing_margin_set.unwrap_or_else(|| {
                        descendant_start_margin
                            .map(|descendant| collapse_margins(child_style.margin.top, descendant))
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
                        let collapses_with_parent =
                            is_collapsible_block_child(child_element, &child_style);
                        let collapses_with_sibling =
                            is_sibling_margin_collapsible_block_child(child_element, &child_style);
                        if !trimmed_block_start_margin
                            && !seen_flow_child
                            && can_collapse_start_margin
                            && collapses_with_parent
                        {
                            if previous_flow_bottom_margin.is_some() {
                                let collapsed_start_margin = page_start_margin(
                                    collapse_margins(applied_start_margin, effective_start_margin),
                                    starts_at_page_top,
                                );
                                child_style.margin.top =
                                    collapsed_start_margin - descendant_margin_adjustment;
                                collapsed_start_margin_offset =
                                    collapsed_start_margin_offset.max(collapsed_start_margin);
                            } else {
                                child_style.margin.top = collapsed_start_margin_delta(
                                    applied_start_margin,
                                    effective_start_margin,
                                    starts_at_page_top,
                                ) - descendant_margin_adjustment;
                            }
                        } else if !trimmed_block_start_margin
                            && collapses_with_sibling
                            && let Some(previous_margin) = previous_flow_bottom_margin
                        {
                            child_style.margin.top =
                                collapsed_margin_delta(previous_margin, effective_start_margin)
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
                            collapsed_start_margin_offset.max(start_offset);
                    }

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
                    let avoid_boundary =
                        previous_break_after_avoid || child_style.break_before.avoids_page();
                    let mut run_start_candidate = if is_flow_child {
                        if avoid_boundary {
                            Some(avoid_run_candidate.take().unwrap_or_else(|| {
                                child_start_candidate
                                    .expect("block child start candidate must be armed when avoid boundary has no existing run candidate")
                            }))
                        } else if child_style.break_after.avoids_page()
                            || next_flow_child_break_before_avoid.unwrap_or(false)
                        {
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
                        && avoid_boundary
                        && let Some(child_height) = child_estimated_height
                        && let Some(candidate) = run_start_candidate.as_ref()
                        && should_move_avoid_break_run_to_next_page(
                            candidate.height(),
                            child_height,
                            self.cursor_y - self.page_bottom(),
                            self.page_area_height(),
                            self.cursor_is_at_page_top(),
                        )
                    {
                        let run_start_candidate = run_start_candidate
                            .take()
                            .expect("block child avoid boundary must have an armed run candidate");
                        let AvoidBreakRunCandidateMeta {
                            index,
                            element_index: _,
                            previous_flow_bottom_margin: saved_previous_flow_bottom_margin,
                            seen_flow_child: saved_seen_flow_child,
                            trim_block_start_adjoining_margins:
                                saved_trim_block_start_adjoining_margins,
                            collapsed_end_margin: saved_collapsed_end_margin,
                            previous_child_page_end: saved_previous_child_page_end,
                            float_run: saved_float_run,
                            height: _,
                        } = run_start_candidate.restore(self);
                        previous_flow_bottom_margin = saved_previous_flow_bottom_margin;
                        seen_flow_child = saved_seen_flow_child;
                        trim_block_start_adjoining_margins =
                            saved_trim_block_start_adjoining_margins;
                        collapsed_end_margin = saved_collapsed_end_margin;
                        previous_child_page_end = saved_previous_child_page_end;
                        float_run = saved_float_run;
                        child_box_index = index;
                        avoid_run_candidate = None;
                        previous_break_after_avoid = false;
                        self.push_page_if_nonempty();
                        continue;
                    }
                    let closes_adjoining_float_replay = is_flow_child
                        && !self_collapsing_child
                        && previous_flow_bottom_margin.is_some()
                        && replayed_adjoining_origin_y.is_none();
                    if closes_adjoining_float_replay
                        && let Some(replay) = adjoining_float_replay.take()
                    {
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
                            previous_break_after_avoid = replay_meta.previous_break_after_avoid;
                            avoid_run_candidate = None;
                            child_box_index = replay_meta.index;
                            self.adjoining_float_origin_y = Some(replay_origin_y);
                            replaying_adjoining_until = Some(replay_until);
                            continue;
                        }
                    }

                    let adjoining_candidate = if self_collapsing_child
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
                                    previous_break_after_avoid,
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
                        element_layout_kind(child_element, &child_style),
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
                            if matches!(child_style.position, Position::Absolute | Position::Fixed)
                            {
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
                        if zero_height_page_boundary {
                            self.push_page_name_element_scope_suppression();
                        }
                        if let box_tree::FormattingBox::Table(table_box) = child_box {
                            let split_scope = split_block_context
                                .map(|_| self.begin_inline_split_block_paint_scope());
                            self.with_text_box_line_trim_scope(
                                child_text_box_line_trim,
                                |layout| {
                                    layout.layout_element_with_child_boxes_and_table_fragment(
                                        table_box.element,
                                        &child_style,
                                        stylesheets,
                                        Some(&table_box.children),
                                        Some(&table_box.fragment),
                                    );
                                },
                            );
                            if let (Some(context), Some(scope)) = (split_block_context, split_scope)
                            {
                                self.finish_inline_split_block_paint_scope(context, scope);
                            }
                        } else if let box_tree::FormattingBox::Block(block_box) = child_box {
                            let split_scope = split_block_context
                                .map(|_| self.begin_inline_split_block_paint_scope());
                            self.with_text_box_line_trim_scope(
                                child_text_box_line_trim,
                                |layout| {
                                    layout.layout_element_with_child_boxes_and_run_ins(
                                        child_element,
                                        &child_style,
                                        stylesheets,
                                        &block_box.run_in_children,
                                        Some(child_children),
                                    );
                                },
                            );
                            if let (Some(context), Some(scope)) = (split_block_context, split_scope)
                            {
                                self.finish_inline_split_block_paint_scope(context, scope);
                            }
                        } else {
                            let split_scope = split_block_context
                                .map(|_| self.begin_inline_split_block_paint_scope());
                            self.with_text_box_line_trim_scope(
                                child_text_box_line_trim,
                                |layout| {
                                    layout.layout_element_with_child_boxes(
                                        child_element,
                                        &child_style,
                                        stylesheets,
                                        Some(child_children),
                                    );
                                },
                            );
                            if let (Some(context), Some(scope)) = (split_block_context, split_scope)
                            {
                                self.finish_inline_split_block_paint_scope(context, scope);
                            }
                        }
                        if zero_height_page_boundary {
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
                            self.last_block_layout_outcome.consumed_bottom_margin
                        } else {
                            child_style.margin.bottom
                        };
                        if collapses_with_parent_end {
                            pending_end_margin_collapse = Some(BlockEndMarginCollapse {
                                child_consumed_margin: child_consumed_bottom_margin,
                                collapsed_margin: collapse_margins(
                                    child_consumed_bottom_margin,
                                    style.margin.bottom,
                                ),
                            });
                        }
                        previous_flow_bottom_margin = if self_collapsing_child {
                            Some(if trimmed_block_start_margin {
                                0.0
                            } else {
                                previous_flow_bottom_margin
                                    .map(|previous| {
                                        collapse_margins(previous, effective_start_margin)
                                    })
                                    .unwrap_or(effective_start_margin)
                            })
                        } else {
                            is_sibling_margin_collapsible_block_child(child_element, &child_style)
                                .then_some(child_consumed_bottom_margin)
                        };
                    }
                    if let Some(child_page_start) = effective_child_page_start {
                        previous_child_page_end = Some(child_page_start);
                    } else if let Some((_, child_page_end)) = child_page_value_sources {
                        previous_child_page_end =
                            Some(page_boundary_name_in_parent_scope(child_page_end, style));
                    }
                    avoid_run_candidate = if is_flow_child
                        && (child_style.break_after.avoids_page()
                            || next_flow_child_break_before_avoid.unwrap_or(false))
                    {
                        child_estimated_height.and_then(|child_height| {
                            run_start_candidate
                                .take()
                                .map(|candidate| candidate.add_height(child_height))
                        })
                    } else {
                        None
                    };
                    previous_break_after_avoid =
                        is_flow_child && child_style.break_after.avoids_page();
                    child_box_index += 1;
                }
                self.flush_float_run(&mut float_run);
            } else {
                let sibling_tags = element_sibling_signature_list(element);
                let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
                let text_box_trim_start_child = text_box_line_trim
                    .trims_block_start
                    .then(|| {
                        self.text_box_trim_dom_child_node_index(
                            element,
                            &sibling_tags,
                            style,
                            stylesheets,
                            true,
                            false,
                        )
                    })
                    .flatten();
                let text_box_trim_end_child = text_box_line_trim
                    .trims_block_end
                    .then(|| {
                        self.text_box_trim_dom_child_node_index(
                            element,
                            &sibling_tags,
                            style,
                            stylesheets,
                            false,
                            true,
                        )
                    })
                    .flatten();
                let mut element_index = 0usize;
                let mut float_run = self.float_run_state();
                let mut avoid_run_candidate: Option<AvoidBreakRunCandidate> = None;
                let mut previous_break_after_avoid = false;
                let mut adjoining_float_replay: Option<AdjoiningFloatReplayCandidate> = None;
                let mut replaying_adjoining_until: Option<usize> = None;
                let mut child_node_index = 0usize;
                while child_node_index < element.children.len() {
                    let replayed_adjoining_origin_y =
                        if replaying_adjoining_until == Some(child_node_index) {
                            replaying_adjoining_until = None;
                            self.adjoining_float_origin_y.take()
                        } else {
                            None
                        };
                    let child = &element.children[child_node_index];
                    let NodeKind::Element(child_element) = &child.kind else {
                        child_node_index += 1;
                        continue;
                    };
                    let pending_child_start_candidate = PendingAvoidBreakRunCandidate {
                        meta: AvoidBreakRunCandidateMeta {
                            index: child_node_index,
                            element_index,
                            previous_flow_bottom_margin,
                            seen_flow_child,
                            trim_block_start_adjoining_margins,
                            collapsed_end_margin,
                            previous_child_page_end: None,
                            float_run,
                            height: 0.0,
                        },
                    };
                    let child_signature = ElementSignature::with_sibling_list(
                        child_element.tag.clone(),
                        child_element.attrs.clone(),
                        element_index,
                        sibling_tags.clone(),
                    );
                    element_index += 1;
                    let mut child_style =
                        Box::new(self.style_for_layout_element_with_parent_font_metrics(
                            child_element,
                            child_signature.clone(),
                            stylesheets,
                            Some(style),
                        ));
                    let child_text_box_line_trim = TextBoxLineTrim {
                        trims_block_start: text_box_trim_start_child == Some(child_node_index),
                        trims_block_end: text_box_trim_end_child == Some(child_node_index),
                        block_start: if text_box_trim_start_child == Some(child_node_index) {
                            text_box_line_trim.block_start
                        } else {
                            0.0
                        },
                        block_end: if text_box_trim_end_child == Some(child_node_index) {
                            text_box_line_trim.block_end
                        } else {
                            0.0
                        },
                    };
                    let child_is_flow_candidate =
                        block_avoid_break_flow_child(element, child_element, &child_style);
                    let next_flow_child_break_before_avoid = self
                        .next_dom_flow_child_break_before_avoid(
                            element,
                            child_node_index,
                            element_index,
                            &sibling_tags,
                            style,
                            stylesheets,
                        );
                    let child_start_candidate = should_arm_block_avoid_break_child_start(
                        child_is_flow_candidate,
                        child_style.break_before.avoids_page(),
                        child_style.break_after.avoids_page(),
                        next_flow_child_break_before_avoid,
                        previous_break_after_avoid,
                        avoid_run_candidate.is_some(),
                    )
                    .then(|| pending_child_start_candidate.arm(self));
                    if self.layout_floating_child(
                        child_element,
                        child_signature.clone(),
                        &child_style,
                        None,
                        None,
                        stylesheets,
                        &mut float_run,
                    ) {
                        seen_flow_child = true;
                        previous_flow_bottom_margin = None;
                        avoid_run_candidate = None;
                        previous_break_after_avoid = false;
                        child_node_index += 1;
                        continue;
                    }
                    self.flush_float_run(&mut float_run);
                    let mut child_ancestors = self.ancestors.clone();
                    child_ancestors.push(child_signature.clone());
                    let is_flow_child = child_is_flow_candidate;
                    let descendant_start_margin = (is_flow_child
                        && can_collapse_block_start_margin(
                            &child_style,
                            used_border_widths(&child_style),
                            has_direct_inline_content_dom_with_font_metrics(
                                child_element,
                                &child_style,
                                stylesheets,
                                &child_ancestors,
                                &mut self.font_system,
                            ),
                        ))
                    .then(|| {
                        collapsible_first_child_start_margin_dom_with_font_metrics(
                            child_element,
                            &child_style,
                            stylesheets,
                            &child_ancestors,
                            &mut self.font_system,
                        )
                    })
                    .flatten();
                    let self_collapsing_child = is_flow_child
                        && is_self_collapsing_block_dom_with_font_metrics(
                            child_element,
                            &child_style,
                            stylesheets,
                            &child_ancestors,
                            &mut self.font_system,
                        );
                    let self_collapsing_margin_set = self_collapsing_child.then(|| {
                        self_collapsing_block_margin_set_for_box(
                            &child_style,
                            descendant_start_margin,
                        )
                    });
                    let effective_start_margin = self_collapsing_margin_set.unwrap_or_else(|| {
                        descendant_start_margin
                            .map(|descendant| collapse_margins(child_style.margin.top, descendant))
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
                        let collapses_with_parent =
                            is_collapsible_block_child(child_element, &child_style);
                        let collapses_with_sibling =
                            is_sibling_margin_collapsible_block_child(child_element, &child_style);
                        if !trimmed_block_start_margin
                            && !seen_flow_child
                            && can_collapse_start_margin
                            && collapses_with_parent
                        {
                            if previous_flow_bottom_margin.is_some() {
                                let collapsed_start_margin = page_start_margin(
                                    collapse_margins(applied_start_margin, effective_start_margin),
                                    starts_at_page_top,
                                );
                                child_style.margin.top =
                                    collapsed_start_margin - descendant_margin_adjustment;
                                collapsed_start_margin_offset =
                                    collapsed_start_margin_offset.max(collapsed_start_margin);
                            } else {
                                child_style.margin.top = collapsed_start_margin_delta(
                                    applied_start_margin,
                                    effective_start_margin,
                                    starts_at_page_top,
                                ) - descendant_margin_adjustment;
                            }
                        } else if !trimmed_block_start_margin
                            && collapses_with_sibling
                            && let Some(previous_margin) = previous_flow_bottom_margin
                        {
                            child_style.margin.top =
                                collapsed_margin_delta(previous_margin, effective_start_margin)
                                    - descendant_margin_adjustment;
                        }

                        collapses_with_parent_end = collapses_with_parent
                            && can_collapse_end_margin
                            && !has_later_normal_block_flow_child_with_font_metrics(
                                element,
                                element_index,
                                &sibling_tags,
                                style,
                                stylesheets,
                                &self.ancestors,
                                &mut self.font_system,
                            );
                    }
                    if let Some(origin_y) = replayed_adjoining_origin_y {
                        let start_offset = (self.cursor_y - origin_y).max(0.0);
                        if start_offset > child_style.margin.top {
                            child_style.margin.top = start_offset;
                        }
                        collapsed_start_margin_offset =
                            collapsed_start_margin_offset.max(start_offset);
                    }

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
                        None,
                    );
                    let avoid_boundary =
                        previous_break_after_avoid || child_style.break_before.avoids_page();
                    let mut run_start_candidate = if is_flow_child {
                        if avoid_boundary {
                            Some(avoid_run_candidate.take().unwrap_or_else(|| {
                                child_start_candidate
                                    .expect("DOM child start candidate must be armed when avoid boundary has no existing run candidate")
                            }))
                        } else if child_style.break_after.avoids_page()
                            || next_flow_child_break_before_avoid.unwrap_or(false)
                        {
                            Some(
                                child_start_candidate
                                    .expect("DOM child start candidate must be armed when this child can seed a later avoid boundary"),
                            )
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if is_flow_child
                        && avoid_boundary
                        && let Some(child_height) = child_estimated_height
                        && let Some(candidate) = run_start_candidate.as_ref()
                        && should_move_avoid_break_run_to_next_page(
                            candidate.height(),
                            child_height,
                            self.cursor_y - self.page_bottom(),
                            self.page_area_height(),
                            self.cursor_is_at_page_top(),
                        )
                    {
                        let run_start_candidate = run_start_candidate
                            .take()
                            .expect("DOM child avoid boundary must have an armed run candidate");
                        let AvoidBreakRunCandidateMeta {
                            index,
                            element_index: saved_element_index,
                            previous_flow_bottom_margin: saved_previous_flow_bottom_margin,
                            seen_flow_child: saved_seen_flow_child,
                            trim_block_start_adjoining_margins:
                                saved_trim_block_start_adjoining_margins,
                            collapsed_end_margin: saved_collapsed_end_margin,
                            previous_child_page_end: _,
                            float_run: saved_float_run,
                            height: _,
                        } = run_start_candidate.restore(self);
                        element_index = saved_element_index;
                        previous_flow_bottom_margin = saved_previous_flow_bottom_margin;
                        seen_flow_child = saved_seen_flow_child;
                        trim_block_start_adjoining_margins =
                            saved_trim_block_start_adjoining_margins;
                        collapsed_end_margin = saved_collapsed_end_margin;
                        float_run = saved_float_run;
                        child_node_index = index;
                        avoid_run_candidate = None;
                        previous_break_after_avoid = false;
                        self.push_page_if_nonempty();
                        continue;
                    }

                    let closes_adjoining_float_replay = is_flow_child
                        && !self_collapsing_child
                        && previous_flow_bottom_margin.is_some()
                        && replayed_adjoining_origin_y.is_none();
                    if closes_adjoining_float_replay
                        && let Some(replay) = adjoining_float_replay.take()
                    {
                        let replay_origin_y = self.cursor_y - child_style.margin.top;
                        let replay_separates = self.adjoining_float_replay_separated_by_bfc_root(
                            &replay,
                            child_element,
                            &child_style,
                            stylesheets,
                            None,
                            replay_origin_y,
                        );
                        if !replay_separates
                            && (replay_origin_y - replay.snapshot_cursor_y()).abs() > FLOAT_EPSILON
                        {
                            let replay_until = child_node_index;
                            let replay_meta = replay.restore(self);
                            element_index = replay_meta.element_index;
                            previous_flow_bottom_margin = replay_meta.previous_flow_bottom_margin;
                            seen_flow_child = replay_meta.seen_flow_child;
                            trim_block_start_adjoining_margins =
                                replay_meta.trim_block_start_adjoining_margins;
                            collapsed_end_margin = replay_meta.collapsed_end_margin;
                            float_run = replay_meta.float_run;
                            previous_break_after_avoid = replay_meta.previous_break_after_avoid;
                            avoid_run_candidate = None;
                            child_node_index = replay_meta.index;
                            self.adjoining_float_origin_y = Some(replay_origin_y);
                            replaying_adjoining_until = Some(replay_until);
                            continue;
                        }
                    }

                    let adjoining_candidate = if self_collapsing_child
                        && adjoining_float_replay.is_none()
                        && replaying_adjoining_until.is_none()
                    {
                        Some(
                            PendingAdjoiningFloatReplayCandidate {
                                meta: AdjoiningFloatReplayCandidateMeta {
                                    index: child_node_index,
                                    element_index: element_index.saturating_sub(1),
                                    previous_flow_bottom_margin,
                                    seen_flow_child,
                                    trim_block_start_adjoining_margins,
                                    collapsed_end_margin,
                                    previous_child_page_end: None,
                                    float_run,
                                    previous_break_after_avoid,
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
                        }
                        if trim_block_start_adjoining_margins && !self_collapsing_child {
                            trim_block_start_adjoining_margins = false;
                        }
                    } else {
                        previous_flow_bottom_margin = None;
                    }

                    let child_uses_block_layout = matches!(
                        element_layout_kind(child_element, &child_style),
                        ElementLayoutKind::BlockFlow
                    );
                    self.last_block_layout_outcome = BlockLayoutOutcome::default();
                    if child_style.display.is_block_level()
                        || is_document_canvas_element(element)
                        || is_replaced_element(child_element)
                    {
                        let previous_block_static_position_y_offset =
                            if matches!(child_style.position, Position::Absolute | Position::Fixed)
                            {
                                let previous = self.block_static_position_y_offset;
                                self.block_static_position_y_offset = None;
                                Some(previous)
                            } else {
                                None
                            };
                        self.push_ancestor_signature(child_signature);
                        self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                            layout.layout_element(child_element, &child_style, stylesheets);
                        });
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
                            self.last_block_layout_outcome.consumed_bottom_margin
                        } else {
                            child_style.margin.bottom
                        };
                        if collapses_with_parent_end {
                            pending_end_margin_collapse = Some(BlockEndMarginCollapse {
                                child_consumed_margin: child_consumed_bottom_margin,
                                collapsed_margin: collapse_margins(
                                    child_consumed_bottom_margin,
                                    style.margin.bottom,
                                ),
                            });
                        }
                        previous_flow_bottom_margin = if self_collapsing_child {
                            Some(if trimmed_block_start_margin {
                                0.0
                            } else {
                                previous_flow_bottom_margin
                                    .map(|previous| {
                                        collapse_margins(previous, effective_start_margin)
                                    })
                                    .unwrap_or(effective_start_margin)
                            })
                        } else {
                            is_sibling_margin_collapsible_block_child(child_element, &child_style)
                                .then_some(child_consumed_bottom_margin)
                        };
                    }
                    avoid_run_candidate = if is_flow_child
                        && (child_style.break_after.avoids_page()
                            || next_flow_child_break_before_avoid.unwrap_or(false))
                    {
                        child_estimated_height.and_then(|child_height| {
                            run_start_candidate
                                .take()
                                .map(|candidate| candidate.add_height(child_height))
                        })
                    } else {
                        None
                    };
                    previous_break_after_avoid =
                        is_flow_child && child_style.break_after.avoids_page();
                    child_node_index += 1;
                }
                self.flush_float_run(&mut float_run);
            }
        }
        self.definite_block_size_stack.pop();

        BlockFlowChildrenPhaseOutcome {
            pending_end_margin_collapse,
            collapsed_start_margin_offset,
        }
    }

    fn split_inline_static_position_y_offset_before_child(
        &mut self,
        child_boxes: &[box_tree::FormattingBox<'_>],
        child_box_index: usize,
        block_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> Option<f32> {
        let previous = (0..child_box_index).rev().find_map(|index| {
            let previous = child_boxes.get(index)?;
            (!formatting_box_is_out_of_flow_positioned(previous)).then_some(previous)
        })?;
        if !matches!(previous, box_tree::FormattingBox::Inline(_)) {
            return None;
        }

        let mut items = Vec::new();
        self.collect_intrinsic_inline_box_items(
            std::slice::from_ref(previous),
            stylesheets,
            None,
            IntrinsicInlineCollectionContext {
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                block_style,
                propagated_decoration: block_style.text_decoration,
            },
            &mut items,
        );
        let has_inline_content = items.iter().any(|item| match item {
            InlineItem::Word(_) => !inline_item_is_collapsible_space(item),
            InlineItem::Atom(_) => true,
            InlineItem::Float(_)
            | InlineItem::Break(_)
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => false,
        });
        has_inline_content.then(|| {
            self.block_static_position_y_offset_from_split_inline_items(items, block_style)
        })
    }

    fn block_static_position_y_offset_from_split_inline_items(
        &mut self,
        mut items: Vec<InlineItem>,
        block_style: &ComputedStyle,
    ) -> f32 {
        if !matches!(items.last(), Some(InlineItem::Break(_))) {
            items.push(InlineItem::Break(InlineBreak::default()));
        }
        items.push(InlineItem::Atom(Box::new(
            self.block_static_position_placeholder_atom(block_style),
        )));
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            block_style,
            self.current_content_logical_inline_size().max(1.0),
            0.0,
            0.0,
        );
        let records = sequence.fragment_records_for_paint(0, sequence.records.len());
        let mut offset = 0.0;
        for record in &records {
            if record.fragment.as_ref().is_some_and(|fragment| {
                fragment.items().iter().any(|item| {
                    matches!(
                        &item.item,
                        InlineLineItem::Atom(atom)
                            if matches!(atom.content(), InlineAtomContent::StaticPositionPlaceholder)
                    )
                })
            }) {
                return offset;
            }
            offset += record.height();
        }
        0.0
    }

    fn text_box_trim_dom_child_node_index(
        &mut self,
        element: &Element,
        sibling_tags: &ElementSiblingSignatureList,
        parent_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        block_start: bool,
        find_last: bool,
    ) -> Option<usize> {
        let mut element_index = 0usize;
        let mut child_styles = Vec::new();
        for (child_node_index, child) in element.children.iter().enumerate() {
            let NodeKind::Element(child_element) = &child.kind else {
                continue;
            };
            let child_signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature,
                stylesheets,
                Some(parent_style),
            );
            child_styles.push((child_node_index, child_element, child_style));
        }
        if find_last {
            child_styles.reverse();
        }
        for (child_node_index, child_element, child_style) in child_styles {
            let Some(accepts) =
                dom_element_text_box_trim_reach(child_element, &child_style, block_start)
            else {
                continue;
            };
            return accepts.then_some(child_node_index);
        }
        None
    }
}

fn formatting_box_is_out_of_flow_positioned(box_: &box_tree::FormattingBox<'_>) -> bool {
    box_.element_parts().is_some_and(|(_, _, style, _)| {
        matches!(style.position, Position::Absolute | Position::Fixed)
    })
}

fn block_avoid_break_flow_child(
    parent_element: &Element,
    child_element: &Element,
    child_style: &ComputedStyle,
) -> bool {
    is_normal_block_flow_child(child_element, child_style)
        || is_document_canvas_element(parent_element)
        || is_replaced_element(child_element)
}

fn should_arm_block_avoid_break_child_start(
    is_flow_child: bool,
    break_before_avoid: bool,
    break_after_avoid: bool,
    next_flow_child_break_before_avoid: Option<bool>,
    previous_break_after_avoid: bool,
    has_avoid_run_candidate: bool,
) -> bool {
    is_flow_child
        && (break_after_avoid
            || next_flow_child_break_before_avoid.unwrap_or(true)
            || (break_before_avoid && !has_avoid_run_candidate)
            || (previous_break_after_avoid && !has_avoid_run_candidate))
}

fn next_formatting_box_flow_child_break_before_avoid(
    parent_element: &Element,
    child_boxes: &[box_tree::FormattingBox<'_>],
    current_index: usize,
) -> Option<bool> {
    for child in child_boxes.iter().skip(current_index + 1) {
        if matches!(child, box_tree::FormattingBox::AnonymousBlock(_)) {
            return Some(false);
        }
        let Some((child_element, _, child_style, _)) = child.element_parts() else {
            continue;
        };
        if block_avoid_break_flow_child(parent_element, child_element, child_style) {
            return Some(child_style.break_before.avoids_page());
        }
        if !style_is_in_normal_flow(child_style) {
            return Some(false);
        }
        return None;
    }
    Some(false)
}

impl<'a> LayoutBuilder<'a> {
    fn next_dom_flow_child_break_before_avoid(
        &mut self,
        parent_element: &Element,
        current_node_index: usize,
        next_element_index: usize,
        sibling_tags: &ElementSiblingSignatureList,
        parent_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> Option<bool> {
        for child in parent_element.children.iter().skip(current_node_index + 1) {
            let NodeKind::Element(child_element) = &child.kind else {
                continue;
            };
            let child_signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                next_element_index,
                sibling_tags.clone(),
            );
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature,
                stylesheets,
                Some(parent_style),
            );
            if block_avoid_break_flow_child(parent_element, child_element, &child_style) {
                return Some(child_style.break_before.avoids_page());
            }
            if !style_is_in_normal_flow(&child_style) {
                return Some(false);
            }
            return None;
        }
        Some(false)
    }

    /// Return whether a following BFC root separates an adjoining float replay.
    ///
    /// CSS 2.2 makes block formatting context roots avoid earlier float margin
    /// boxes. If an adjoining-margin replay would put the next BFC root beside
    /// floats that leave no fitting band, the BFC root is separated in the
    /// same spirit as clearance and the collapsed margin must not drag those
    /// floats downward:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>.
    fn adjoining_float_replay_separated_by_bfc_root(
        &mut self,
        replay: &AdjoiningFloatReplayCandidate,
        child_element: &Element,
        child_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        replay_origin_y: f32,
    ) -> bool {
        let establishes_independent_bfc =
            child_style.display.establishes_block_formatting_context()
                || used_overflow_clips_element(child_element, child_style)
                || block_align_content_establishes_independent_formatting_context(
                    child_style.align_content,
                );
        if !establishes_independent_bfc
            || child_style.float != Float::None
            || matches!(child_style.position, Position::Absolute | Position::Fixed)
        {
            return false;
        }

        let snapshot = replay.snapshot();
        if snapshot.pages.len() != self.pages.len()
            || snapshot.float_contexts.len() != self.float_contexts.len()
        {
            return true;
        }

        let Some(snapshot_context) = snapshot.float_contexts.last() else {
            return true;
        };
        let Some(current_context) = self.float_contexts.last() else {
            return true;
        };
        if current_context.shapes.len() <= snapshot_context.shapes.len() {
            return false;
        }

        if snapshot.containing_block_writing_mode != WritingMode::HorizontalTb
            || child_style.writing_mode != WritingMode::HorizontalTb
        {
            return true;
        }

        let delta_y = replay_origin_y - snapshot.cursor_y;
        let mut replayed_context = snapshot_context.clone();
        replayed_context.shapes.extend(
            current_context.shapes[snapshot_context.shapes.len()..]
                .iter()
                .copied()
                .map(|shape| shape.translated_block(delta_y)),
        );

        let containing_left = snapshot.content_left;
        let containing_right = snapshot.content_right;
        let containing_inline_size = (containing_right - containing_left).max(0.0);
        let page_index = snapshot.pages.len();
        let placement = replayed_context.avoiding_bfc_root_position(
            page_index,
            replay_origin_y,
            child_style.clear,
            child_style.writing_mode,
            child_style.direction,
            containing_left,
            containing_right,
            |band_left, band_width, _candidate_top| {
                let candidate_geometry = self.block_layout_geometry_in_inline_span(
                    child_element,
                    child_style,
                    stylesheets,
                    child_boxes,
                    BlockLayoutInlineConstraint {
                        containing_left: band_left,
                        containing_right: band_left + band_width,
                        percentage_basis: containing_inline_size,
                        auto_border_box_width: (band_width < containing_inline_size - 0.01)
                            .then_some(band_width),
                    },
                );
                let candidate_style = &candidate_geometry.style;
                let estimated_outer_height = self
                    .estimate_element_height(
                        child_element,
                        candidate_style,
                        stylesheets,
                        candidate_geometry.outer_width(),
                        child_boxes,
                    )
                    .unwrap_or(
                        candidate_style.margin.top
                            + candidate_style.line_height
                            + candidate_style.margin.bottom,
                    );
                let border_box_height = (estimated_outer_height
                    - candidate_style.margin.top
                    - candidate_style.margin.bottom)
                    .max(0.0);
                FloatAvoidingBfcMeasurement {
                    border_box_width: candidate_geometry.outer_width(),
                    border_box_height,
                }
            },
        );

        placement.top < replay_origin_y - FLOAT_EPSILON
    }
}

fn text_box_trim_formatting_box_child_index(
    child_boxes: &[box_tree::FormattingBox<'_>],
    block_start: bool,
    find_last: bool,
) -> Option<usize> {
    let candidate = if find_last {
        child_boxes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, child)| {
                text_box_trim_formatting_box_reach(child, block_start)
                    .map(|accepts| (index, accepts))
            })
    } else {
        child_boxes.iter().enumerate().find_map(|(index, child)| {
            text_box_trim_formatting_box_reach(child, block_start).map(|accepts| (index, accepts))
        })
    };
    candidate.and_then(|(index, accepts)| accepts.then_some(index))
}

fn text_box_trim_formatting_box_reach(
    box_: &box_tree::FormattingBox<'_>,
    block_start: bool,
) -> Option<bool> {
    if !formatting_box_is_in_normal_flow(box_) || formatting_box_is_zero_height_page_boundary(box_)
    {
        return None;
    }
    match box_ {
        box_tree::FormattingBox::AnonymousBlock(_) => Some(true),
        box_tree::FormattingBox::InlineSplitBlockContext(context)
            if context.children.len() == 1 =>
        {
            text_box_trim_formatting_box_reach(&context.children[0], block_start)
        }
        box_tree::FormattingBox::Block(box_) => Some(
            matches!(
                element_layout_kind(box_.element, &box_.style),
                ElementLayoutKind::BlockFlow
            ) && style_allows_text_box_trim_propagation(&box_.style, block_start),
        ),
        box_tree::FormattingBox::Inline(_)
        | box_tree::FormattingBox::InlineSplitBlockContext(_)
        | box_tree::FormattingBox::AtomicInline(_)
        | box_tree::FormattingBox::Text(_) => None,
        box_tree::FormattingBox::Table(_)
        | box_tree::FormattingBox::Flex(_)
        | box_tree::FormattingBox::Replaced(_) => Some(false),
    }
}

fn dom_element_text_box_trim_reach(
    element: &Element,
    style: &ComputedStyle,
    block_start: bool,
) -> Option<bool> {
    if !style_is_in_normal_flow(style) {
        return None;
    }
    let layout_kind = element_layout_kind(element, style);
    if matches!(layout_kind, ElementLayoutKind::BlockFlow) {
        return Some(style_allows_text_box_trim_propagation(style, block_start));
    }
    (is_replaced_element(element) || style.display.is_block_level()).then_some(false)
}

fn style_allows_text_box_trim_propagation(style: &ComputedStyle, block_start: bool) -> bool {
    let side = if block_start {
        block_start_side(style.writing_mode)
    } else {
        block_end_side(style.writing_mode)
    };
    physical_edge_value(style.padding, side) <= 0.0
        && physical_edge_value(used_border_widths(style), side) <= 0.0
}

fn physical_edge_value(edges: Edges, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => edges.top,
        PhysicalSide::Right => edges.right,
        PhysicalSide::Bottom => edges.bottom,
        PhysicalSide::Left => edges.left,
    }
}
