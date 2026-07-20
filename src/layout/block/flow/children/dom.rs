use super::shared::*;
use super::state::{BlockFlowChildTraversalState, ChildFlowTraversalOutcome};
use super::*;

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_dom_flow_children(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        can_collapse_start_margin: bool,
        can_collapse_end_margin: bool,
        applied_start_margin: LayoutLength,
        clearance_consumed_adjoining_start_margin: bool,
        starts_at_page_top: bool,
        traversal_state: &mut BlockFlowChildTraversalState,
    ) -> ChildFlowTraversalOutcome {
        let mut collapsed_end_margin = false;
        let mut pending_end_margin_collapse = None;
        let mut collapsed_start_margin_offset = layout_pt(0.0);
        let mut previous_flow_bottom_margin = None;
        let mut seen_flow_child = false;
        let mut trim_block_start_adjoining_margins = style.margin_trim.block_start;
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
        let mut previous_break_after = PageBreak::Auto;
        let mut previous_child_page_end: Option<Option<String>> = None;
        let mut adjoining_float_replay: Option<AdjoiningFloatReplayCandidate> = None;
        let mut replaying_adjoining_until: Option<usize> = None;
        let mut child_node_index = 0usize;
        while child_node_index < element.children.len() {
            let replaying_adjoining_target = if replaying_adjoining_until == Some(child_node_index)
            {
                replaying_adjoining_until = None;
                self.adjoining_float_origin_y.take();
                true
            } else {
                false
            };
            let child = &element.children[child_node_index];
            let NodeKind::Element(child_element) = &child.kind else {
                child_node_index += 1;
                continue;
            };
            if traversal_state.is_exhausted() {
                child_node_index += 1;
                continue;
            }
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
                    remaining_line_clamp: traversal_state.capture_avoid_replay(),
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
            if is_html_select_item_element(child_element)
                && !has_html_select_context(element, &self.ancestors)
            {
                child_node_index += 1;
                continue;
            }
            let mut child_style = Box::new(self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature.clone(),
                stylesheets,
                Some(style),
            ));
            child_style.apply_effective_zoom();
            traversal_state.apply_to(&mut child_style);
            if block_avoid_break_flow_child(element, child_element, &child_style)
                && let Some(line_clamp) = &mut child_style.line_clamp
            {
                line_clamp.continues_after_clamp_point =
                    has_later_normal_block_flow_child_with_font_metrics(
                        element,
                        element_index.saturating_sub(1),
                        &sibling_tags,
                        style,
                        stylesheets,
                        &self.ancestors,
                        &mut self.font_system,
                    );
            }
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
            let child_page_values = style_is_in_normal_flow(&child_style).then(|| {
                let inherited_page_name = self.active_page_value_scope(style);
                self.dom_page_boundary_values(
                    child_element,
                    &child_style,
                    stylesheets,
                    inherited_page_name.as_deref(),
                )
            });
            let child_page_start = child_page_values
                .as_ref()
                .map(|values| values.start.clone());
            if let Some(start) = child_page_start.as_ref()
                && previous_child_page_end
                    .as_ref()
                    .is_none_or(|previous| previous != start)
                && (!self.current_page_has_content() || previous_child_page_end.is_some())
            {
                // The initial in-flow child has no preceding sibling to
                // supply a class-A end value, but an explicit `page`
                // value still selects its first fragmentainer. Without
                // that selection a first `page: named` box is laid out
                // against the default page size until a later sibling
                // happens to create a named-page boundary.
                // <https://www.w3.org/TR/css-page-3/#using-named-pages>
                self.switch_page_name_at_class_a_boundary(start.as_deref());
            }
            let next_flow_child_break_before = self.next_dom_flow_child_break_before(
                element,
                child_node_index,
                element_index,
                &sibling_tags,
                style,
                stylesheets,
            );
            let child_break_context = FragmentBreakContext::new(
                PageBreak::Auto,
                child_style.break_before,
                child_style.break_after,
                next_flow_child_break_before.unwrap_or(PageBreak::Auto),
            );
            if child_break_context.next_forced_break_supersedes_after_in(fragmentainer_kind) {
                child_style.break_after = PageBreak::Auto;
            }
            let child_break_opportunity = FragmentBreakOpportunity::before_box_boundary(
                fragmentainer_kind,
                child_node_index as f32,
                child_break_context,
                previous_break_after,
                false,
            );
            let avoid_run_start_decision =
                FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
                    participates_in_flow: child_is_flow_candidate,
                    fragmentainer_kind,
                    break_context: child_break_context,
                    break_opportunity: child_break_opportunity,
                    next_break_before: next_flow_child_break_before,
                    has_avoid_run_candidate: avoid_run_candidate.is_some(),
                });
            let child_start_candidate = avoid_run_start_decision
                .should_arm_start_candidate
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
                // Preserve an armed normal-flow break candidate across
                // out-of-flow floats. If a following sibling avoids a
                // break before itself, restoring that candidate also
                // replays the intervening float in the new
                // fragmentainer.
                // <https://www.w3.org/TR/css-break-3/#possible-breaks>
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
                    UsedEdges::from_css_edges(used_border_widths(&child_style)),
                    has_direct_inline_content_dom_with_font_metrics(
                        child_element,
                        &child_style,
                        stylesheets,
                        &child_ancestors,
                        &mut self.font_system,
                    ),
                    self.used_overflow_for_element(child_element, &child_style),
                ))
            .then(|| {
                collapsible_first_child_start_margin_dom_with_font_metrics(
                    child_element,
                    &child_style,
                    stylesheets,
                    &child_ancestors,
                    &mut self.font_system,
                    self.document_canvas_overflow,
                )
            })
            .flatten();
            let mut margin_collapse_style = None;
            if self.definite_block_size_stack.last().is_some_and(|basis| {
                percentage_height_is_auto_for_margin_collapse(&child_style, *basis)
            }) {
                let mut used_style = (*child_style).clone();
                used_style.box_values.height = css::ComputedLengthPercentageOrAuto::Auto;
                margin_collapse_style = Some(used_style);
            }
            let margin_collapse_style = margin_collapse_style.as_ref().unwrap_or(&child_style);
            let self_collapsing_child = is_flow_child
                && is_self_collapsing_block_dom_with_font_metrics(
                    child_element,
                    margin_collapse_style,
                    stylesheets,
                    &child_ancestors,
                    &mut self.font_system,
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
            let mut adjoining_start_margin_paint_offset = None;
            if is_flow_child {
                let collapses_with_parent = is_collapsible_block_child(child_element, &child_style);
                let collapses_with_sibling =
                    is_sibling_margin_collapsible_block_child(child_element, &child_style);
                let adjoins_parent_start = !trimmed_block_start_margin
                    && !seen_flow_child
                    && can_collapse_start_margin
                    && collapses_with_parent;
                if adjoins_parent_start {
                    if let Some(previous_margin) = previous_flow_bottom_margin {
                        // Keep the parent's applied start margin in the
                        // collapsed set when a self-collapsing sibling has
                        // zero margin. This mirrors the formatting-box path.
                        child_style.margin.top = collapsed_margin_delta(
                            collapse_margins(applied_start_margin, layout_pt(previous_margin)),
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
                    if clearance_consumed_adjoining_start_margin {
                        child_style.margin.top = 0.0;
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
                // A first child's adjoining positive margin lies outside the
                // parent's border box, unless actual clearance separates the
                // margins while laying out that child. Keep the candidate at
                // this collapse boundary and commit it after child layout.
                // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
                if adjoins_parent_start {
                    adjoining_start_margin_paint_offset =
                        Some(layout_pt(child_style.margin.top.max(0.0)));
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
                None,
            );
            let mut run_start_candidate = if is_flow_child {
                if avoid_run_start_decision.is_avoid_boundary {
                    Some(avoid_run_candidate.take().unwrap_or_else(|| {
                        child_start_candidate
                            .expect("DOM child start candidate must be armed when avoid boundary has no existing run candidate")
                    }))
                } else if avoid_run_start_decision.seeds_later_avoid_boundary {
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
                && avoid_run_start_decision.is_avoid_boundary
                && let Some(child_height) = child_estimated_height
                && let Some(candidate) = run_start_candidate.as_ref()
                && should_move_avoid_break_run_to_next_fragmentainer(
                    candidate.height(),
                    child_height.min(
                        child_style.margin.top
                            + child_style.line_height * child_style.orphans.max(1) as f32,
                    ),
                    self.fragmentainer_from_page_cursor(PageTopBlockPosition::new(self.cursor_y)),
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
                    trim_block_start_adjoining_margins: saved_trim_block_start_adjoining_margins,
                    collapsed_end_margin: saved_collapsed_end_margin,
                    previous_child_page_end: _,
                    float_run: saved_float_run,
                    remaining_line_clamp: saved_remaining_line_clamp,
                    height: _,
                } = run_start_candidate.restore(self);
                element_index = saved_element_index;
                previous_flow_bottom_margin = saved_previous_flow_bottom_margin;
                seen_flow_child = saved_seen_flow_child;
                trim_block_start_adjoining_margins = saved_trim_block_start_adjoining_margins;
                collapsed_end_margin = saved_collapsed_end_margin;
                float_run = saved_float_run;
                traversal_state.restore_avoid_replay(saved_remaining_line_clamp);
                child_node_index = index;
                avoid_run_candidate = None;
                previous_break_after = PageBreak::Auto;
                self.push_page_if_nonempty();
                continue;
            }

            let closes_adjoining_float_replay = is_flow_child
                && !self_collapsing_child
                && previous_flow_bottom_margin.is_some()
                && !replaying_adjoining_target;
            if closes_adjoining_float_replay && let Some(replay) = adjoining_float_replay.take() {
                let replay_origin_y = self.cursor_y - child_style.margin.top;
                let replay_separation = self.adjoining_float_replay_separated_by_following_child(
                    &replay,
                    child_element,
                    &child_style,
                    stylesheets,
                    None,
                    replay_origin_y,
                );
                if let AdjoiningFloatReplaySeparation::Clearance { border_top }
                | AdjoiningFloatReplaySeparation::MarginSeparation { border_top } =
                    replay_separation
                {
                    child_style.margin.top =
                        layout_pt((self.cursor_y - border_top.points()).max(0.0)).points();
                    preserve_adjusted_block_margins(&mut child_style);
                }
                if replay_separation == AdjoiningFloatReplaySeparation::None
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
                    previous_break_after = replay_meta.previous_break_after;
                    avoid_run_candidate = None;
                    child_node_index = replay_meta.index;
                    self.adjoining_float_origin_y = Some(replay_origin_y);
                    replaying_adjoining_until = Some(replay_until);
                    continue;
                }
            }

            let adjoining_candidate = if child_style.float == Float::None
                && self_collapsing_child
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
            let clearance_count_before_child = self.applied_clearance_count;
            self.last_block_layout_outcome = BlockLayoutOutcome::default();
            if child_style.display.is_block_level()
                || is_document_canvas_element(element)
                || is_replaced_element(child_element)
            {
                let previous_block_static_position_y_offset =
                    if matches!(child_style.position, Position::Absolute | Position::Fixed) {
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
            if self.applied_clearance_count == clearance_count_before_child
                && let Some(offset) = adjoining_start_margin_paint_offset
            {
                collapsed_start_margin_offset = collapsed_start_margin_offset.max(offset);
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
                    is_sibling_margin_collapsible_block_child(child_element, &child_style)
                        .then_some(child_consumed_bottom_margin)
                };
                if let Some(values) = child_page_values {
                    previous_child_page_end = Some(values.end);
                }
                if child_uses_block_layout {
                    traversal_state.record_descendant_clamp_line_slots(
                        self.last_block_layout_outcome.clamp_line_slots,
                    );
                    traversal_state.debit(self.last_block_layout_outcome.clamp_line_slots);
                }
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
            child_node_index += 1;
        }
        self.flush_float_run(&mut float_run);
        ChildFlowTraversalOutcome {
            pending_end_margin_collapse,
            collapsed_start_margin_offset,
        }
    }

    /// Derives the CSS Paged Media start/end page values for a DOM fallback
    /// subtree. This mirrors the frozen formatting-tree path so dynamically
    /// laid-out block flow cannot bypass class-A named-page transitions.
    fn dom_page_boundary_values(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_page_name: Option<&str>,
    ) -> ResolvedPageBoundaryValues {
        let own_page_name = if style.page_name_specified {
            style
                .page_name
                .clone()
                .or_else(|| inherited_page_name.map(str::to_string))
        } else {
            inherited_page_name.map(str::to_string)
        };
        let mut values = ResolvedPageBoundaryValues {
            start: own_page_name.clone(),
            end: own_page_name.clone(),
        };
        if style.display.is_flex() {
            return values;
        }
        let siblings = element_sibling_signature_list(element);
        let mut element_index = 0;
        let mut first = None;
        let mut last = None;
        for child in &element.children {
            let NodeKind::Element(child) = &child.kind else {
                continue;
            };
            let signature = ElementSignature::with_sibling_list(
                child.tag.clone(),
                child.attrs.clone(),
                element_index,
                siblings.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child,
                signature,
                stylesheets,
                Some(style),
            );
            if !style_is_in_normal_flow(&child_style) {
                continue;
            }
            let applies = child_style.display.is_block_level() || is_replaced_element(child);
            let (child_values, child_sources) = if applies {
                (
                    self.dom_page_boundary_values(
                        child,
                        &child_style,
                        stylesheets,
                        own_page_name.as_deref(),
                    ),
                    self.dom_page_boundary_value_sources(child, &child_style, stylesheets),
                )
            } else {
                (
                    ResolvedPageBoundaryValues {
                        start: None,
                        end: None,
                    },
                    PageBoundaryValues::inapplicable(),
                )
            };
            first.get_or_insert_with(|| (child_values.clone(), child_sources.clone()));
            last = Some((child_values, child_sources));
        }
        if let Some((first, sources)) = first
            && sources.start.overrides_parent_summary()
        {
            values.start = first.start;
        }
        if let Some((last, sources)) = last
            && sources.end.overrides_parent_summary()
        {
            values.end = last.end;
        }
        values
    }

    /// Structural companion to [`Self::dom_page_boundary_values`]. The DOM
    /// fallback needs this only to determine which recursively resolved child
    /// boundary replaces its parent; it never uses the source value itself as
    /// an output page selection.
    fn dom_page_boundary_value_sources(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> PageBoundaryValues {
        let mut values = PageBoundaryValues::from_style(style);
        if style.display.is_flex() {
            return values;
        }
        let siblings = element_sibling_signature_list(element);
        let mut element_index = 0;
        let mut first = None;
        let mut last = None;
        for child in &element.children {
            let NodeKind::Element(child) = &child.kind else {
                continue;
            };
            let signature = ElementSignature::with_sibling_list(
                child.tag.clone(),
                child.attrs.clone(),
                element_index,
                siblings.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child,
                signature,
                stylesheets,
                Some(style),
            );
            if !style_is_in_normal_flow(&child_style) {
                continue;
            }
            let applies = child_style.display.is_block_level() || is_replaced_element(child);
            let child_values = if applies {
                self.dom_page_boundary_value_sources(child, &child_style, stylesheets)
            } else {
                PageBoundaryValues::inapplicable()
            };
            first.get_or_insert_with(|| child_values.clone());
            last = Some(child_values);
        }
        if let Some(first) = first
            && first.start.overrides_parent_summary()
        {
            values.start = first.start;
        }
        if let Some(last) = last
            && last.end.overrides_parent_summary()
        {
            values.end = last.end;
        }
        values
    }
}
