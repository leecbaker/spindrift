use super::shared::*;
use super::state::{BlockFlowChildTraversalState, ChildFlowTraversalOutcome};
use super::*;
use crate::layout::inline_collect::TextDecorationPropagationContext;
use std::num::NonZeroUsize;

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_dom_flow_children(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        can_collapse_start_margin: bool,
        can_collapse_end_margin: bool,
        applied_start_margin: LayoutLength,
        clearance_consumed_adjoining_start_margin: bool,
        starts_at_page_top: bool,
        has_preceding_inline_flow_content: bool,
        traversal_state: &mut BlockFlowChildTraversalState,
    ) -> ChildFlowTraversalOutcome {
        let mut collapsed_end_margin = false;
        let mut pending_end_margin_collapse = None;
        let mut collapsed_start_margin_offset = layout_pt(0.0);
        let mut previous_flow_bottom_margin = None;
        let mut seen_flow_child = false;
        let mut trim_block_start_adjoining_margins = style.margin_trim.block_start;
        // Direct inline content is laid out before this DOM fallback traversal.
        // A following block child can receive the ancestor pseudo only when
        // that earlier source did not provide its first formatted line.
        // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
        let mut first_formatted_line = FirstFormattedLineState::for_style(style);
        if has_preceding_inline_flow_content {
            first_formatted_line.consume_next_formatted_line();
        }
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
            if traversal_state.has_reached_discard_region_limit(self.pages.len()) {
                let child_count = NonZeroUsize::new(child_node_index);
                debug_assert!(child_count.is_some(), "a local region break retains source");
                if let Some(child_count) = child_count {
                    traversal_state.capture_discard_source_prefix(child_count);
                } else {
                    traversal_state.mark_local_continuation_cutoff();
                }
                break;
            }
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
            // Decorations are not inherited computed values, but their line
            // origins continue through this in-flow formatting-context
            // boundary. Materialize that layout-only state before any child
            // sizing, inline collection, or paint dispatch consumes the
            // style.
            // <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
            let decoration_context = TextDecorationPropagationContext::from_style(style);
            *child_style = decoration_context.used_child_style(&child_style);
            // A block container's first formatted line is supplied by the
            // first line of its first in-flow block child when it has no
            // preceding inline line of its own. Preserve the originating
            // pseudo while the child starts its own block formatting context.
            // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
            if first_formatted_line.applies_to_next_inline_run()
                && style_is_in_normal_flow(&child_style)
                && child_style.display.is_block_level()
                && let Some(style_with_originating_pseudos) =
                    style_with_originating_typographic_pseudos(&child_style, style)
            {
                *child_style = style_with_originating_pseudos;
            }
            // A descendant that consumes the final remaining line must know
            // about later in-flow source before it selects that line. Compute
            // this from the same cascade inputs as the primary traversal so
            // positioned and floated later siblings cannot spuriously create
            // a block ellipsis.
            let mut later_element_index = element_index;
            let has_later_in_flow_child =
                element.children[child_node_index + 1..]
                    .iter()
                    .any(|candidate| {
                        let NodeKind::Element(candidate) = &candidate.kind else {
                            return false;
                        };
                        let candidate_signature = ElementSignature::with_sibling_list(
                            candidate.tag.clone(),
                            candidate.attrs.clone(),
                            later_element_index,
                            sibling_tags.clone(),
                        );
                        later_element_index += 1;
                        let candidate_style = self
                            .style_for_layout_element_with_parent_font_metrics(
                                candidate,
                                candidate_signature,
                                stylesheets,
                                Some(style),
                            );
                        style_is_in_normal_flow(&candidate_style)
                    });
            if traversal_state.is_exhausted()
                && (style_is_in_normal_flow(&child_style) || child_style.float != Float::None)
            {
                traversal_state.capture_forced_discard_before_later_child(child_node_index);
                // A clamp discards later in-flow source and floats generated
                // after that source boundary. Positioned descendants remain
                // eligible because their containing-block placement is
                // independent of normal-flow continuation.
                child_node_index += 1;
                continue;
            }
            let child_shares_clamp_context =
                self.child_shares_line_clamp_formatting_context(child_element, &child_style);
            if child_shares_clamp_context {
                traversal_state.apply_to_with_continuation(
                    &mut child_style,
                    BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                        has_later_in_flow_child,
                    ),
                );
                let border = used_border_widths(&child_style);
                let source_box_envelope = crate::units::content_box_pt(
                    (child_style.margin.top
                        + child_style.margin.bottom
                        + child_style.padding.top
                        + child_style.padding.bottom
                        + border.top
                        + border.bottom)
                        .max(0.0),
                );
                BlockFlowChildTraversalState::reserve_automatic_child_non_content(
                    &mut child_style,
                    source_box_envelope,
                );
                if has_later_in_flow_child && source_box_envelope.points() > 0.01 {
                    BlockFlowChildTraversalState::require_automatic_terminal_marker_when_full(
                        &mut child_style,
                    );
                }
            }
            // A following block sibling establishes a clamp point *after*
            // this child; it is not inline overflow inside the child. Passing
            // that fact into the child's inline selector incorrectly paints
            // an ellipsis after a terminal block-in-inline line.
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
            // Inline-level normal-flow children precede a following class-A
            // block boundary but do not supply page values of their own. Keep
            // the enclosing scope as the preceding end value so the following
            // named block can select a new page group.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            if !child_is_flow_candidate
                && style_is_in_normal_flow(&child_style)
                && previous_child_page_end.is_none()
            {
                previous_child_page_end = Some(self.active_page_value_scope(style));
            }
            let child_page_values = child_is_flow_candidate.then(|| {
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
            let zero_height_page_boundary =
                child_is_flow_candidate && dom_child_is_zero_height_page_boundary(&child_style);
            let effective_child_page_start = if zero_height_page_boundary {
                Some(self.dom_coalesced_zero_height_page_start(
                    element,
                    child_node_index,
                    element_index,
                    &sibling_tags,
                    style,
                    stylesheets,
                    child_page_start.clone().flatten(),
                ))
            } else {
                child_page_start.clone()
            };
            if let Some(start) = effective_child_page_start.as_ref()
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
            if let Some(committed) = self.committed_inline_floats.remove(&child_element.id) {
                // Inline line selection already owns this float's exclusion
                // and captured paint subtree.  Keep the ordinary traversal's
                // out-of-flow sibling state, but never lay out the source
                // element a second time.
                debug_assert!(committed.is_valid());
                seen_flow_child = true;
                previous_flow_bottom_margin = None;
                child_node_index += 1;
                continue;
            }
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
            // Block-flow sibling placement and margin collapsing consume the
            // scalar edge cache before the child's own formatting context is
            // entered.  Resolve all computed margin and padding edges here
            // against the containing block's logical inline basis so mixed
            // `calc(<length> + <percentage>)` values cannot be replaced by
            // their legacy fixed-length cache component.
            // <https://www.w3.org/TR/CSS22/box.html#margin-properties>
            let parent_inline_percentage_basis = self
                .current_child_available_space()
                .logical_inline_percentage_basis_for(style.writing_mode);
            apply_used_box_metrics_for_logical_inline_basis(
                &mut child_style,
                parent_inline_percentage_basis,
            );
            let block_end_margin_trim = BlockEndMarginTrim::for_child(
                style,
                is_flow_child,
                has_later_normal_block_flow_child_with_font_metrics(
                    element,
                    element_index,
                    &sibling_tags,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                ),
            );
            block_end_margin_trim.apply_to_child(&mut child_style);
            let descendant_start_margin = (is_flow_child
                && can_collapse_block_start_margin(
                    child_element,
                    &child_style,
                    UsedEdges::from_css_edges(used_border_widths(&child_style)),
                    has_direct_inline_content_before_first_flow_child_dom_with_font_metrics(
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
                used_style
                    .box_values
                    .height
                    .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
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
            // A self-collapsing child joins its descendant start margin to
            // its block-end edge. If that child itself trims block-end, the
            // descendant portion is discarded at this boundary while the
            // child's own margin remains available to its parent.
            // <https://drafts.csswg.org/css-box-4/#margin-trim-block>
            let collapsed_descendant_start_margin = if child_style.margin_trim.block_end {
                None
            } else {
                descendant_start_margin
            };
            let self_collapsing_margin_set = self_collapsing_child.then(|| {
                self_collapsing_block_margin_set_for_box(
                    &child_style,
                    collapsed_descendant_start_margin,
                )
            });
            let adjoining_start_margin = self_collapsing_margin_set
                .map(|collapsed| AdjoiningBlockStartMargin::from_collapsed(layout_pt(collapsed)))
                .unwrap_or_else(|| {
                    AdjoiningBlockStartMargin::from_child_and_descendant(
                        layout_pt(child_style.margin.top),
                        collapsed_descendant_start_margin.map(layout_pt),
                    )
                });
            if let Some(collapsed_margin) = self_collapsing_margin_set {
                child_style.margin.top = collapsed_margin;
                child_style.margin.bottom = 0.0;
            }
            // A self-collapsing final child joins its start and end margins
            // with the preceding sibling's end margin. Once that joined set
            // adjoins the container's trimmed block-end edge, none of it may
            // remain as an inter-sibling cursor advance.
            // <https://drafts.csswg.org/css-box-4/#margin-trim-block>
            let trims_self_collapsing_end_margin_set =
                block_end_margin_trim == BlockEndMarginTrim::Trim && self_collapsing_child;
            if trims_self_collapsing_end_margin_set {
                child_style.margin.top = 0.0;
                child_style.margin.bottom = 0.0;
                discard_consumed_adjoining_block_end_margin(self, previous_flow_bottom_margin);
                previous_flow_bottom_margin = None;
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
            let mut inherited_adjoining_start_margin = None;
            if is_flow_child {
                let collapses_with_parent = is_collapsible_block_child(child_element, &child_style);
                let collapses_with_sibling =
                    outer_margins_adjoin_block_siblings(child_element, &child_style);
                let adjoins_parent_start = !trimmed_block_start_margin
                    && !seen_flow_child
                    && can_collapse_start_margin
                    && collapses_with_parent;
                if adjoins_parent_start {
                    inherited_adjoining_start_margin = Some(adjoining_start_margin.value());
                    if let Some(previous_margin) = previous_flow_bottom_margin {
                        // Keep the parent's applied start margin in the
                        // collapsed set when a self-collapsing sibling has
                        // zero margin. This mirrors the formatting-box path.
                        child_style.margin.top = adjoining_start_margin
                            .child_delta_after_sibling(collapse_margins(
                                applied_start_margin,
                                layout_pt(previous_margin),
                            ))
                            .points();
                    } else {
                        child_style.margin.top = adjoining_start_margin
                            .child_delta_at_parent_start(applied_start_margin, starts_at_page_top)
                            .points();
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
                    child_style.margin.top = adjoining_start_margin
                        .child_delta_after_sibling(layout_pt(previous_margin))
                        .points();
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
            // Only normal-flow children have margins adjusted by this pass.
            // Preserve the authored `auto` margins of out-of-flow children
            // for the absolute-positioned used-value algorithm.
            // <https://www.w3.org/TR/css-position-3/#abspos-layout>
            if is_flow_child {
                preserve_adjusted_block_margins(&mut child_style);
            }
            if child_shares_clamp_context
                && let Some(Node {
                    kind: NodeKind::Element(next),
                    ..
                }) = element.children.get(child_node_index + 1)
            {
                let next_signature = ElementSignature::with_sibling_list(
                    next.tag.clone(),
                    next.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                let next_style = self.style_for_layout_element_with_parent_font_metrics(
                    next,
                    next_signature,
                    stylesheets,
                    Some(style),
                );
                if style_is_in_normal_flow(&next_style) {
                    let next_border = used_border_widths(&next_style);
                    let next_non_content = crate::units::content_box_pt(
                        (next_style.margin.top
                            + next_style.margin.bottom
                            + next_style.padding.top
                            + next_style.padding.bottom
                            + next_border.top
                            + next_border.bottom)
                            .max(0.0),
                    );
                    if next_non_content.points() > 0.01 {
                        BlockFlowChildTraversalState::reserve_automatic_child_non_content(
                            &mut child_style,
                            next_non_content,
                        );
                        BlockFlowChildTraversalState::require_automatic_terminal_marker_when_full(
                            &mut child_style,
                        );
                    }
                }
            }

            let available_outer_width = (self.content_right
                - self.content_left
                - child_style.margin.left
                - child_style.margin.right)
                .max(child_style.font_size);
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
            let avoid_run_retry_context =
                run_start_candidate
                    .as_ref()
                    .map(|candidate| AvoidRunRetryContext {
                        current_fragmentainer: self.fragmentainer_from_page_cursor(
                            PageTopBlockPosition::new(self.cursor_y),
                        ),
                        empty_destination_fragmentainer: self
                            .next_empty_fragmentainer(fragmentainer_kind),
                        source_occupancy: candidate.source_fragmentainer_occupancy(),
                    });
            // The estimate is only consumed to move or extend a contiguous
            // `break-inside: avoid` run. Measuring every ordinary child here
            // duplicates expensive table row layout before the real
            // fragmenting pass has selected a destination. An empty source
            // probes only when that destination is strictly larger, so equal
            // temporary columns relax the avoid preference instead of
            // replaying forever.
            // <https://www.w3.org/TR/css-break-3/#break-between>
            let child_estimated_height = ((avoid_run_start_decision.is_avoid_boundary
                || avoid_run_start_decision.seeds_later_avoid_boundary)
                && !self.cursor_is_at_page_top()
                && self.fragmentation_suppression_depth == 0
                && avoid_run_retry_context.is_some_and(AvoidRunRetryContext::can_advance))
            .then(|| {
                self.estimate_element_height(
                    child_element,
                    &child_style,
                    stylesheets,
                    available_outer_width,
                    None,
                )
            })
            .flatten();
            if is_flow_child
                && avoid_run_start_decision.is_avoid_boundary
                && let Some(child_height) = child_estimated_height
                && let Some(retry_context) = avoid_run_retry_context
                && let Some(candidate) = run_start_candidate.as_ref()
                && should_move_avoid_break_run_to_next_fragmentainer(AvoidRunPrebreakInput {
                    run_height: candidate.height(),
                    next_height: child_height.min(
                        child_style.margin.top
                            + child_style.line_height * child_style.orphans.get() as f32,
                    ),
                    retry_context,
                })
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
                // A retry from an empty source is permitted only when the
                // destination fragmentainer is larger. Restoring that source
                // also restores its empty temporary fragmentainer, so the ordinary
                // conditional transition would be a no-op and re-run the
                // exact same avoid boundary forever. Materialize the larger
                // destination fragmentainer before replaying the run.
                // <https://www.w3.org/TR/css-break-3/#breaking-rules>
                if retry_context.source_occupancy == AvoidRunSourceFragmentainerOccupancy::Empty {
                    self.push_page();
                } else {
                    self.push_page_if_nonempty();
                }
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
                    // The child has been entered as the first in-flow block
                    // source. Its own line selection owns the propagated
                    // pseudo, so later siblings must not restart it.
                    first_formatted_line.consume_next_formatted_line();
                }
                if trim_block_start_adjoining_margins && !self_collapsing_child {
                    trim_block_start_adjoining_margins = false;
                }
            } else if !matches!(child_style.position, Position::Absolute | Position::Fixed) {
                // An absolutely/fixed-positioned source child is removed from
                // normal flow, so it cannot separate the adjacent in-flow
                // siblings' vertical margins. Preserve the prior in-flow
                // bottom-margin candidate until the next in-flow block has
                // consumed the collapsed set.
                // <https://www.w3.org/TR/CSS22/visuren.html#absolute-positioning>
                // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
                previous_flow_bottom_margin = None;
            }

            let child_uses_block_layout = matches!(
                element_layout_kind(child_element, &child_style),
                ElementLayoutKind::BlockFlow
            );
            // This snapshot is taken after the parent has resolved sibling
            // margins and clearance, immediately before the child enters its
            // own formatting context. The automatic controller consumes the
            // child's actual normal-flow block advance, not paint bounds or
            // an out-of-flow descendant's extent.
            let automatic_child_flow_start =
                (is_flow_child && child_shares_clamp_context).then_some(self.cursor_y);
            let clearance_count_before_child = self.applied_clearance_count;
            self.last_block_layout_outcome = BlockLayoutOutcome::default();
            if child_style.display.is_block_level() {
                let previous_block_static_position_y_offset =
                    if matches!(child_style.position, Position::Absolute | Position::Fixed) {
                        let previous = self.block_static_position_y_offset;
                        self.block_static_position_y_offset = None;
                        Some(previous)
                    } else {
                        None
                    };
                if let Some(margin) = inherited_adjoining_start_margin {
                    self.inherited_adjoining_start_margins.push(margin);
                }
                self.push_ancestor_signature(child_signature);
                self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                    layout.layout_element(child_element, &child_style, stylesheets);
                });
                self.ancestors.pop();
                if inherited_adjoining_start_margin.is_some() {
                    self.inherited_adjoining_start_margins.pop();
                }
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
                previous_flow_bottom_margin = if trims_self_collapsing_end_margin_set {
                    Some(0.0)
                } else if self_collapsing_child {
                    Some(if trimmed_block_start_margin {
                        0.0
                    } else {
                        previous_flow_bottom_margin
                            .map(|previous| {
                                collapse_margins(
                                    layout_pt(previous),
                                    adjoining_start_margin.value(),
                                )
                                .points()
                            })
                            .unwrap_or(adjoining_start_margin.value().points())
                    })
                } else {
                    outer_margins_adjoin_block_siblings(child_element, &child_style)
                        .then_some(child_consumed_bottom_margin)
                };
                if collapses_with_parent_end {
                    let adjoining_end_margin = if self_collapsing_child
                        && !child_style.display.establishes_block_formatting_context()
                    {
                        previous_flow_bottom_margin.unwrap_or(child_consumed_bottom_margin)
                    } else {
                        child_consumed_bottom_margin
                    };
                    pending_end_margin_collapse = Some(BlockEndMarginCollapse {
                        child_consumed_margin: layout_pt(child_consumed_bottom_margin),
                        collapsed_margin: collapse_margins(
                            layout_pt(adjoining_end_margin),
                            layout_pt(style.margin.bottom),
                        ),
                    });
                }
                if zero_height_page_boundary {
                    previous_child_page_end = effective_child_page_start;
                } else if let Some(values) = child_page_values {
                    previous_child_page_end = Some(values.end);
                }
                if child_uses_block_layout && child_shares_clamp_context {
                    if let Some(child_flow_start) = automatic_child_flow_start {
                        traversal_state.debit_automatic_block_contribution(
                            crate::units::content_box_pt(
                                (child_flow_start - self.cursor_y).max(0.0),
                            ),
                        );
                    }
                    traversal_state.record_descendant_clamp_line_slots(
                        self.last_block_layout_outcome.clamp_line_slots,
                    );
                    traversal_state
                        .debit_rendered_slots(self.last_block_layout_outcome.clamp_line_slots);
                    if traversal_state.has_active_clamp()
                        && self.last_block_layout_outcome.has_local_continuation_cutoff
                    {
                        traversal_state.mark_local_continuation_cutoff();
                    }
                }
            }
            avoid_run_candidate = if avoid_run_start_decision.seeds_later_avoid_boundary {
                match (run_start_candidate.take(), child_estimated_height) {
                    (Some(candidate), Some(child_height)) => {
                        Some(candidate.add_height(child_height))
                    }
                    (Some(candidate), None) => Some(candidate),
                    (None, _) => None,
                }
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
            rendered_legend: None,
        }
    }

    /// Coalesce a run of explicit zero-height named-page children into the
    /// next nonzero normal-flow sibling's page group.
    ///
    /// The frozen formatting-tree traversal applies the same CSS Paged Media
    /// rule. DOM fallback layout must preserve it too, otherwise each member
    /// of the zero-height run manufactures an empty page:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    #[allow(clippy::too_many_arguments)]
    fn dom_coalesced_zero_height_page_start(
        &mut self,
        parent: &Element,
        current_node_index: usize,
        next_element_index: usize,
        sibling_tags: &ElementSiblingSignatureList,
        parent_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        fallback: Option<String>,
    ) -> Option<String> {
        let mut element_index = next_element_index;
        for node in parent.children.iter().skip(current_node_index + 1) {
            let NodeKind::Element(child) = &node.kind else {
                continue;
            };
            let signature = ElementSignature::with_sibling_list(
                child.tag.clone(),
                child.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child,
                signature,
                stylesheets,
                Some(parent_style),
            );
            if !style_is_in_normal_flow(&child_style) || !child_style.display.is_block_level() {
                continue;
            }
            if !dom_child_is_zero_height_page_boundary(&child_style) {
                let inherited_page_name = self.active_page_value_scope(parent_style);
                return self
                    .dom_page_boundary_values(
                        child,
                        &child_style,
                        stylesheets,
                        inherited_page_name.as_deref(),
                    )
                    .start;
            }
        }
        fallback
    }

    /// Derives the CSS Paged Media start/end page values for a DOM fallback
    /// subtree. This mirrors the frozen formatting-tree path so dynamically
    /// laid-out block flow cannot bypass class-A named-page transitions.
    pub(in crate::layout) fn dom_page_boundary_values(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_page_name: Option<&str>,
    ) -> ResolvedPageBoundaryValues {
        self.dom_page_boundary_summary(element, style, stylesheets, inherited_page_name)
            .0
    }

    /// Resolve a DOM fallback subtree's page values and the authored sources
    /// that determine whether those values override its parent summary.
    ///
    /// These results are inseparable during the recursive walk: a child's
    /// source decides whether its resolved value replaces the parent.  Keeping
    /// them together avoids re-cascading every descendant once for values and
    /// again for sources at each table row/cell boundary.
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>
    fn dom_page_boundary_summary(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_page_name: Option<&str>,
    ) -> (ResolvedPageBoundaryValues, PageBoundaryValues) {
        let cache_key = (element.id, inherited_page_name.map(str::to_string));
        if let Some(summary) = self.dom_page_boundary_summaries.get(&cache_key) {
            return summary.clone();
        }
        let own_page_name = if style.page.is_specified() {
            style
                .page
                .specified_name()
                .map(|name| name.as_str().to_string())
                .or_else(|| inherited_page_name.map(str::to_string))
        } else {
            inherited_page_name.map(str::to_string)
        };
        let mut values = ResolvedPageBoundaryValues {
            start: own_page_name.clone(),
            end: own_page_name.clone(),
        };
        let mut sources = PageBoundaryValues::from_style(style);
        if style.display.is_flex() {
            return (values, sources);
        }
        if style.display.is_table() {
            // DOM fallback traversal does not retain the table's normalized
            // row/cell structure. Rebuild that durable fragment here so its
            // page boundaries match ordinary frozen-box table layout rather
            // than treating table internals as inapplicable DOM children.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            let child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            let fragment = box_tree::build_frozen_table_fragment(
                element,
                &element_signature(element),
                &child_boxes,
            );
            let table_summary =
                table::table_page_boundary_summary(&fragment, style, inherited_page_name);
            let summary = (table_summary.resolved, table_summary.sources);
            self.dom_page_boundary_summaries
                .insert(cache_key, summary.clone());
            return summary;
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
            // Only block-level boxes establish a class-A page boundary in
            // this block-flow traversal. Inline replaced elements remain in
            // the surrounding inline formatting context, so their own
            // `page` value must not mask a following block sibling's
            // transition.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            let applies =
                child_style.display.is_block_level() && !child_style.display.is_table_internal();
            let (child_values, child_sources) = if applies {
                self.dom_page_boundary_summary(
                    child,
                    &child_style,
                    stylesheets,
                    own_page_name.as_deref(),
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
        if let Some((first_values, first_sources)) = first.as_ref()
            && first_sources.start.overrides_parent_summary()
        {
            values.start = first_values.start.clone();
            sources.start = first_sources.start.clone();
        }
        if let Some((last_values, last_sources)) = last.as_ref()
            && last_sources.end.overrides_parent_summary()
        {
            values.end = last_values.end.clone();
            sources.end = last_sources.end.clone();
        }
        let summary = (values, sources);
        self.dom_page_boundary_summaries
            .insert(cache_key, summary.clone());
        summary
    }
}

fn dom_child_is_zero_height_page_boundary(style: &ComputedStyle) -> bool {
    style.page.is_specified()
        && style
            .box_values
            .height
            .length_if_no_percent()
            .is_some_and(|height| height.abs() < 0.01)
}
