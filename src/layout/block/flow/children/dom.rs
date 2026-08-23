use std::num::NonZeroUsize;

mod state;

use state::DomFlowTraversalState;

use super::shared::*;
use super::state::{
    AutomaticBlockSizeReplayState, BlockFlowChildTraversalState, ChildFlowTraversalOutcome,
    DomAutomaticBlockSizeReplayCheckpoint,
};
use super::*;
use crate::layout::inline_collect::TextDecorationPropagationContext;

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
        start_margin_arrangement: BlockStartMarginArrangement,
        starts_at_page_top: bool,
        has_preceding_inline_flow_content: bool,
        preceding_inline_clamp_block_advance: ContentBoxLength,
        traversal_state: &mut BlockFlowChildTraversalState,
    ) -> ChildFlowTraversalOutcome {
        #[cfg(all(feature = "stack-profile", target_os = "macos"))]
        let mut stack_profile_scope = stack_profile::enter("layout_dom_flow_children");
        let permits_parent_start_collapse =
            start_margin_arrangement.permits_parent_start_collapse();
        // Direct inline content is laid out before this DOM fallback traversal.
        // A following block child can receive the ancestor pseudo only when
        // that earlier source did not provide its first formatted line.
        // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
        let mut first_formatted_line = FirstFormattedLineState::for_style(style);
        if has_preceding_inline_flow_content {
            first_formatted_line.consume_next_formatted_line();
        }
        // Keep the static-position source separate from the mutable layout
        // cursor. Direct inline content has already advanced that cursor, but
        // an automatic-inset block source is hypothetically placed at the
        // inline run's resolved block position.
        // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
        let out_of_flow_static_source = has_preceding_inline_flow_content.then(|| {
            self.block_static_position_rectangle_at(PageTopBlockPosition::new(
                self.cursor_y - preceding_inline_clamp_block_advance.points(),
            ))
        });
        let vertical_child_inline_origin = style.writing_mode.has_vertical_lines().then(|| {
            self.vertical_child_inline_origin(element, style.writing_mode, style.used_direction())
        });
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
        // Source snapshots retained for automatic block-boundary marker
        // replay. A following child can overflow before it produces a line;
        // in that case the preceding child's final same-BFC line owns the
        // block ellipsis.
        // <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
        let mut dom_state = Box::new(DomFlowTraversalState::new(
            first_formatted_line,
            out_of_flow_static_source,
            self.float_run_state(),
            style.margin_trim.block_start,
        ));
        while dom_state.child_node_index < element.children.len() {
            #[cfg(all(feature = "stack-profile", target_os = "macos"))]
            stack_profile_scope.set_source_index(dom_state.child_node_index);
            if traversal_state.has_reached_discard_region_limit(self.pages.len()) {
                let child_count = NonZeroUsize::new(dom_state.child_node_index);
                debug_assert!(child_count.is_some(), "a local region break retains source");
                if let Some(child_count) = child_count {
                    traversal_state.capture_discard_source_prefix(child_count);
                } else {
                    traversal_state.mark_local_continuation_cutoff();
                }
                break;
            }
            let replaying_adjoining_target =
                if dom_state.replaying_adjoining_until == Some(dom_state.child_node_index) {
                    dom_state.replaying_adjoining_until = None;
                    self.adjoining_float_origin_y.take();
                    true
                } else {
                    false
                };
            let child = &element.children[dom_state.child_node_index];
            let NodeKind::Element(child_element) = &child.kind else {
                dom_state.child_node_index += 1;
                continue;
            };
            let automatic_replay_before_child =
                dom_state.capture_automatic_marker_checkpoint(self, traversal_state);
            let pending_child_start_candidate = PendingAvoidBreakRunCandidate {
                meta: AvoidBreakRunCandidateMeta {
                    index: dom_state.child_node_index,
                    element_index: dom_state.element_index,
                    previous_flow_bottom_margin: dom_state.previous_flow_bottom_margin,
                    seen_flow_child: dom_state.seen_flow_child,
                    trim_block_start_adjoining_margins: dom_state
                        .trim_block_start_adjoining_margins,
                    collapsed_end_margin: dom_state.collapsed_end_margin,
                    previous_child_page_end: None,
                    float_run: dom_state.float_run,
                    remaining_line_clamp: traversal_state.capture_avoid_replay(),
                    block_extent: layout_pt(0.0),
                },
            };
            let child_signature = ElementSignature::from_sibling_snapshot(
                dom_state.element_index,
                sibling_tags.clone(),
            )
            .expect("source child must have a cached sibling signature");
            dom_state.element_index += 1;
            if is_html_select_item_element(child_element)
                && !has_html_select_context(element, &self.ancestors)
            {
                dom_state.child_node_index += 1;
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
            if dom_state.first_formatted_line.applies_to_next_inline_run()
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
            let mut later_element_index = dom_state.element_index;
            let has_later_in_flow_child = element.children[dom_state.child_node_index + 1..]
                .iter()
                .any(|candidate| match &candidate.kind {
                    NodeKind::Text(text) => !text.trim().is_empty(),
                    NodeKind::Element(candidate) => {
                        let candidate_signature = ElementSignature::from_sibling_snapshot(
                            later_element_index,
                            sibling_tags.clone(),
                        )
                        .expect("source child must have a cached sibling signature");
                        later_element_index += 1;
                        let candidate_style = self
                            .style_for_layout_element_with_parent_font_metrics(
                                candidate,
                                candidate_signature,
                                stylesheets,
                                Some(style),
                            );
                        style_is_in_normal_flow(&candidate_style)
                    }
                });
            if traversal_state.is_exhausted()
                && (style_is_in_normal_flow(&child_style) || child_style.float != Float::None)
            {
                traversal_state
                    .capture_forced_discard_before_later_child(dom_state.child_node_index);
                // A clamp discards later in-flow source and floats generated
                // after that source boundary. Positioned descendants remain
                // eligible because their containing-block placement is
                // independent of normal-flow continuation.
                dom_state.child_node_index += 1;
                continue;
            }
            let child_shares_clamp_context =
                self.child_shares_line_clamp_formatting_context(child_element, &child_style);
            if child_shares_clamp_context {
                if dom_state.automatic_marker_replay_target == Some(dom_state.child_node_index) {
                    *child_style = traversal_state
                        .automatic_terminal_boundary_style(&child_style)
                        .expect("automatic marker replay requires an active controller");
                } else {
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
            }
            // A following block sibling establishes a clamp point *after*
            // this child; it is not inline overflow inside the child. Passing
            // that fact into the child's inline selector incorrectly paints
            // an ellipsis after a terminal block-in-inline line.
            let child_text_box_line_trim = TextBoxLineTrim {
                trims_block_start: text_box_trim_start_child == Some(dom_state.child_node_index),
                trims_block_end: text_box_trim_end_child == Some(dom_state.child_node_index),
                block_start: if text_box_trim_start_child == Some(dom_state.child_node_index) {
                    text_box_line_trim.block_start
                } else {
                    0.0
                },
                block_end: if text_box_trim_end_child == Some(dom_state.child_node_index) {
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
                && dom_state.previous_child_page_end.is_none()
            {
                dom_state.previous_child_page_end = Some(self.active_page_value_scope(style));
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
                    dom_state.child_node_index,
                    dom_state.element_index,
                    &sibling_tags,
                    style,
                    stylesheets,
                    child_page_start.clone().flatten(),
                ))
            } else {
                child_page_start.clone()
            };
            if let Some(start) = effective_child_page_start.as_ref()
                && dom_state
                    .previous_child_page_end
                    .as_ref()
                    .is_none_or(|previous| previous != start)
                && (!self.current_page_has_content() || dom_state.previous_child_page_end.is_some())
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
                dom_state.child_node_index,
                dom_state.element_index,
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
                dom_state.child_node_index as f32,
                child_break_context,
                dom_state.previous_break_after,
                false,
            );
            let avoid_run_start_decision =
                FragmentAvoidRunStartDecision::choose(FragmentAvoidRunStartInput {
                    participates_in_flow: child_is_flow_candidate,
                    fragmentainer_kind,
                    break_context: child_break_context,
                    break_opportunity: child_break_opportunity,
                    next_break_before: next_flow_child_break_before,
                    has_avoid_run_candidate: dom_state.avoid_run_candidate.is_some(),
                });
            let child_start_candidate = avoid_run_start_decision
                .should_arm_start_candidate
                .then(|| pending_child_start_candidate.arm(self));
            if let Some(committed) = self
                .committed_inline_floats
                .remove(&InlineFloatId::Element(child_element.id))
            {
                // Inline line selection already owns this float's exclusion
                // and captured paint subtree.  Keep the ordinary traversal's
                // out-of-flow sibling state, but never lay out the source
                // element a second time.
                debug_assert!(committed.is_valid());
                dom_state.previous_flow_bottom_margin = None;
                dom_state.child_node_index += 1;
                continue;
            }
            if self.layout_floating_child(
                child_element,
                child_signature.clone(),
                &child_style,
                None,
                None,
                stylesheets,
                FloatPlacementAxes::for_style(style),
                &mut dom_state.float_run,
            ) {
                dom_state.previous_flow_bottom_margin = None;
                // Preserve an armed normal-flow break candidate across
                // out-of-flow floats. If a following sibling avoids a
                // break before itself, restoring that candidate also
                // replays the intervening float in the new
                // fragmentainer.
                // <https://www.w3.org/TR/css-break-3/#possible-breaks>
                dom_state.child_node_index += 1;
                continue;
            }
            self.flush_float_run(&mut dom_state.float_run);
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
            let block_end_margin_trim = BlockEndMarginTrim::for_child(style, is_flow_child, || {
                has_later_normal_block_flow_child_with_font_metrics(
                    element,
                    dom_state.element_index,
                    &sibling_tags,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                )
            });
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
            let margin_collapse_style = self
                .definite_block_size_stack
                .last()
                .is_some_and(|basis| {
                    height_behaves_as_auto_for_margin_collapse(&child_style, *basis)
                })
                .then(|| Self::dom_auto_height_margin_collapse_style(&child_style));
            let margin_collapse_style = margin_collapse_style.as_deref().unwrap_or(&child_style);
            let self_collapsing_child = is_flow_child
                && !self.has_in_flow_marker_line(child_element, &child_style)
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
                discard_consumed_adjoining_block_end_margin(
                    self,
                    dom_state.previous_flow_bottom_margin,
                );
                dom_state.previous_flow_bottom_margin = None;
            }

            let trimmed_block_start_margin = is_flow_child
                && trim_adjoining_block_start_margin(
                    style,
                    &mut child_style,
                    dom_state.trim_block_start_adjoining_margins,
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
                // `clear` prevents the actual start-margin adjacency, but
                // CSS 2.2 selects its clearance target from the counterfactual
                // `clear:none` arrangement. Retain both states: the former
                // preserves the child's used margin; the latter carries the
                // parent-start hypothesis into block layout.
                let counterfactually_adjoins_parent_start = !trimmed_block_start_margin
                    && !dom_state.seen_flow_child.has_seen()
                    && permits_parent_start_collapse
                    && can_collapse_start_margin
                    && collapses_with_parent;
                let adjoins_parent_start =
                    counterfactually_adjoins_parent_start && child_style.clear == Clear::None;
                if counterfactually_adjoins_parent_start {
                    inherited_adjoining_start_margin = Some(InheritedAdjoiningStartMargin::new(
                        adjoining_start_margin.value(),
                        PageTopBlockPosition::new(self.cursor_y),
                    ));
                }
                if adjoins_parent_start {
                    if let Some(previous_margin) = dom_state.previous_flow_bottom_margin {
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
                } else if !trimmed_block_start_margin
                    && collapses_with_sibling
                    && FragmentBreakContext::for_standalone_box(&child_style)
                        .forced_break_before_in(fragmentainer_kind)
                        .is_none()
                    && let Some(previous_margin) = dom_state.previous_flow_bottom_margin
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
                        dom_state.element_index,
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
            let available_outer_width = (self.content_right
                - self.content_left
                - child_style.margin.left
                - child_style.margin.right)
                .max(child_style.font_size);
            let mut run_start_candidate = if is_flow_child {
                if avoid_run_start_decision.is_avoid_boundary {
                    Some(dom_state.avoid_run_candidate.take().unwrap_or_else(|| {
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
                        current_fragmentainer: self.avoid_break_current_fragmentainer(
                            style.writing_mode,
                            PageTopBlockPosition::new(self.cursor_y),
                        ),
                        empty_destination_fragmentainer: self.next_empty_avoid_break_fragmentainer(
                            fragmentainer_kind,
                            style.writing_mode,
                        ),
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
            let should_measure_avoid_run = (avoid_run_start_decision.is_avoid_boundary
                || avoid_run_start_decision.seeds_later_avoid_boundary)
                && !self.cursor_is_at_page_top()
                && self.fragmentation_suppression_depth == 0
                && avoid_run_retry_context.is_some_and(AvoidRunRetryContext::can_advance);
            let child_avoid_block_extent = should_measure_avoid_run
                .then(|| {
                    let key = AvoidRunPreflightKey::capture(
                        self,
                        dom_state.child_node_index,
                        available_outer_width,
                        style.writing_mode,
                        fragmentainer_kind,
                        avoid_run_retry_context
                            .expect("avoid-run preflight requires a retry context")
                            .current_fragmentainer,
                    );
                    if let Some(extent) = dom_state.avoid_run_preflight_cache.get(&key) {
                        extent
                    } else {
                        let extent = self.avoid_break_fragmentation_extent(
                            child_element,
                            &child_style,
                            stylesheets,
                            available_outer_width,
                            None,
                            style.writing_mode,
                        );
                        dom_state.avoid_run_preflight_cache.insert(key, extent);
                        extent
                    }
                })
                .flatten();
            if is_flow_child
                && avoid_run_start_decision.is_avoid_boundary
                && let Some(child_block_extent) = child_avoid_block_extent
                && let Some(retry_context) = avoid_run_retry_context
                && let Some(candidate) = run_start_candidate.as_ref()
                && should_move_avoid_break_run_to_next_fragmentainer(AvoidRunPrebreakInput {
                    run_block_extent: candidate.block_extent(),
                    next_block_extent: child_block_extent,
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
                    block_extent: _,
                } = run_start_candidate.restore(self);
                dom_state.element_index = saved_element_index;
                dom_state.previous_flow_bottom_margin = saved_previous_flow_bottom_margin;
                dom_state.seen_flow_child = saved_seen_flow_child;
                dom_state.trim_block_start_adjoining_margins =
                    saved_trim_block_start_adjoining_margins;
                dom_state.collapsed_end_margin = saved_collapsed_end_margin;
                dom_state.float_run = saved_float_run;
                traversal_state.restore_avoid_replay(saved_remaining_line_clamp);
                dom_state.child_node_index = index;
                dom_state.avoid_run_candidate = None;
                dom_state.previous_break_after = PageBreak::Auto;
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
                && dom_state.previous_flow_bottom_margin.is_some()
                && !replaying_adjoining_target;
            if closes_adjoining_float_replay
                && let Some(replay) = dom_state.adjoining_float_replay.take()
            {
                let replay_origin_y = self.cursor_y - child_style.margin.top;
                let replay_separation = replay
                    .clearance_boundary()
                    .map(|border_top| AdjoiningFloatReplaySeparation::Clearance { border_top })
                    .unwrap_or_else(|| {
                        self.adjoining_float_replay_separated_by_following_child(
                            &replay,
                            child_element,
                            &child_style,
                            stylesheets,
                            None,
                            replay_origin_y,
                        )
                    });
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
                    let replay_until = dom_state.child_node_index;
                    let replay_meta = replay.restore(self);
                    dom_state.element_index = replay_meta.element_index;
                    dom_state.previous_flow_bottom_margin = replay_meta.previous_flow_bottom_margin;
                    dom_state.seen_flow_child = replay_meta.seen_flow_child;
                    dom_state.trim_block_start_adjoining_margins =
                        replay_meta.trim_block_start_adjoining_margins;
                    dom_state.collapsed_end_margin = replay_meta.collapsed_end_margin;
                    dom_state.float_run = replay_meta.float_run;
                    dom_state.previous_break_after = replay_meta.previous_break_after;
                    dom_state.avoid_run_candidate = None;
                    dom_state.child_node_index = replay_meta.index;
                    self.adjoining_float_origin_y = Some(replay_origin_y);
                    dom_state.replaying_adjoining_until = Some(replay_until);
                    continue;
                }
            }

            let adjoining_candidate = if child_style.float == Float::None
                && self_collapsing_child
                && dom_state.adjoining_float_replay.is_none()
                && dom_state.replaying_adjoining_until.is_none()
            {
                Some(
                    PendingAdjoiningFloatReplayCandidate {
                        meta: AdjoiningFloatReplayCandidateMeta {
                            index: dom_state.child_node_index,
                            element_index: dom_state.element_index.saturating_sub(1),
                            previous_flow_bottom_margin: dom_state.previous_flow_bottom_margin,
                            seen_flow_child: dom_state.seen_flow_child,
                            trim_block_start_adjoining_margins: dom_state
                                .trim_block_start_adjoining_margins,
                            collapsed_end_margin: dom_state.collapsed_end_margin,
                            previous_child_page_end: None,
                            float_run: dom_state.float_run,
                            previous_break_after: dom_state.previous_break_after,
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
                    dom_state.seen_flow_child.record_in_flow_child();
                    // The child has been entered as the first in-flow block
                    // source. Its own line selection owns the propagated
                    // pseudo, so later siblings must not restart it.
                    dom_state.first_formatted_line.consume_next_formatted_line();
                }
                if dom_state.trim_block_start_adjoining_margins && !self_collapsing_child {
                    dom_state.trim_block_start_adjoining_margins = false;
                }
            } else if !matches!(child_style.position, Position::Absolute | Position::Fixed) {
                // An absolutely/fixed-positioned source child is removed from
                // normal flow, so it cannot separate the adjacent in-flow
                // siblings' vertical margins. Preserve the prior in-flow
                // bottom-margin candidate until the next in-flow block has
                // consumed the collapsed set.
                // <https://www.w3.org/TR/CSS22/visuren.html#absolute-positioning>
                // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
                dom_state.previous_flow_bottom_margin = None;
            }

            let child_uses_block_layout = matches!(
                element_layout_kind(child_element, &child_style),
                ElementLayoutKind::BlockFlow
            );
            // The DOM fallback traversal is the normal route for ordinary
            // HTML documents. Its physical top-to-bottom cursor must not
            // become the block cursor for a vertical containing flow: that
            // flow advances through a local horizontal block track.
            //
            // Keep the physical inline origin and the horizontal block track
            // as separate inputs. The child is laid out against the current
            // track, then its used physical border-box span advances the next
            // sibling in the parent's logical block direction.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
            // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
            let principal_vertical_placement = (is_flow_child
                && style.writing_mode.has_vertical_lines())
            .then(|| {
                OrthogonalBlockPlacement::new(
                    style.writing_mode,
                    style.used_direction(),
                    vertical_child_inline_origin
                        .expect("a vertical parent has a fixed physical inline origin"),
                    PageInlineSpan::from_edges(self.content_left, self.content_right),
                    LogicalInlineContentSize::new(content_box_pt(
                        self.current_content_logical_inline_size(),
                    )),
                )
            })
            .flatten();
            let advances_principal_fragmentainers = self.active_fragmentainer_kind()
                == FragmentainerKind::Page
                && self.element_supplies_document_principal_flow(element);
            let principal_flow_page_index = self.pages.len();
            let principal_flow_paint_checkpoint =
                principal_vertical_placement.map(|_| self.current_page.paint_checkpoint());
            if let Some(placement) = principal_vertical_placement {
                debug_assert_eq!(
                    placement.logical_inline_percentage_basis().points(),
                    self.current_content_logical_inline_size(),
                    "vertical principal-flow child constraints use the page-height inline basis"
                );
            }
            if advances_principal_fragmentainers
                && principal_vertical_placement
                    .is_some_and(|placement| placement.block_track_is_exhausted())
                && self.current_page_has_content()
            {
                self.push_page();
            }
            let source_body_canvas = principal_vertical_placement.is_some()
                && self.principal_flow.is_source_body(child_element);
            let bottom_origin_canvas_child = principal_vertical_placement.is_some()
                && self.element_supplies_document_principal_flow(element)
                && inline_start_side(style.writing_mode, style.used_direction())
                    == PhysicalSide::Bottom;
            let bottom_origin_nested_child = principal_vertical_placement.is_some()
                && !self.element_supplies_document_principal_flow(element)
                && inline_start_side(style.writing_mode, style.used_direction())
                    == PhysicalSide::Bottom;
            let canvas_inline_origin = self.cursor_y;
            if bottom_origin_canvas_child {
                // The legacy block formatter measures available physical Y
                // space from the page top.  Lay out a bottom-origin
                // principal-flow child in that scratch coordinate, then
                // project its completed paint back to the canvas inline
                // start. Its logical block advancement remains the separate
                // horizontal track below.
                // <https://drafts.csswg.org/css-writing-modes-4/#principal-flow>
                self.cursor_y = self.page_top();
            } else if let Some(placement) = principal_vertical_placement
                && !bottom_origin_nested_child
            {
                self.cursor_y = placement.page_inline_origin().points();
            }
            // This snapshot is taken after the parent has resolved sibling
            // margins and clearance, immediately before the child enters its
            // own formatting context. The automatic controller consumes the
            // child's actual normal-flow block advance, not paint bounds or
            // an out-of-flow descendant's extent.
            let automatic_child_flow_start = (is_flow_child
                && traversal_state.has_automatic_block_size_clamp())
            .then_some(self.cursor_y);
            self.last_block_layout_outcome = BlockLayoutOutcome::default();
            if child_style.display.is_block_level() {
                let captures_direct_out_of_flow_static_position =
                    matches!(child_style.position, Position::Absolute | Position::Fixed)
                        && !child_style.abspos_static_source.is_inline_level();
                let previous_direct_out_of_flow_static_position = self.absolute_static_position;
                let previous_block_static_position_y_offset =
                    if captures_direct_out_of_flow_static_position {
                        let previous = self.block_static_position_y_offset;
                        self.block_static_position_y_offset = None;
                        Some(previous)
                    } else {
                        None
                    };
                if captures_direct_out_of_flow_static_position {
                    let rectangle = dom_state.out_of_flow_static_source.unwrap_or_else(|| {
                        self.block_static_position_rectangle_at(PageTopBlockPosition::new(
                            self.cursor_y,
                        ))
                    });
                    dom_state.out_of_flow_static_source = Some(rectangle);
                    self.absolute_static_position = Some(
                        self.absolute_static_position
                            .unwrap_or_else(|| {
                                AbsoluteStaticPosition::from_page_rect(
                                    self.content_left,
                                    self.content_right,
                                    rectangle.area.top_y(),
                                )
                            })
                            .with_static_position_rectangle(rectangle),
                    );
                }
                if let Some(margin) = inherited_adjoining_start_margin {
                    self.inherited_adjoining_start_margins.push(margin);
                }
                let previous_direct_block_layout_constraint = self
                    .replace_direct_block_layout_constraint(
                        child_element,
                        principal_vertical_placement,
                    );
                self.push_ancestor_signature(child_signature);
                self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                    layout.layout_element(child_element, &child_style, stylesheets);
                });
                self.ancestors.pop();
                self.restore_direct_block_layout_constraint(
                    previous_direct_block_layout_constraint,
                );
                if inherited_adjoining_start_margin.is_some() {
                    self.inherited_adjoining_start_margins.pop();
                }
                if let Some(previous) = previous_block_static_position_y_offset {
                    self.block_static_position_y_offset = previous;
                }
                if captures_direct_out_of_flow_static_position {
                    self.absolute_static_position = previous_direct_out_of_flow_static_position;
                }
                self.flush_float_run(&mut dom_state.float_run);
            }
            if let (Some(checkpoint), Some(placement)) = (
                principal_flow_paint_checkpoint,
                principal_vertical_placement,
            ) && self.pages.len() == principal_flow_page_index
            {
                let fragment = self.current_page.take_paint_fragment_since(checkpoint);
                let child_block_end_margin = placement.child_block_end_margin(&child_style);
                let child_margin_box_block_extent = placement.child_margin_box_block_extent(
                    self.last_block_layout_outcome
                        .physical_border_box_inline_span,
                    &child_style,
                );
                let remaining_block_track =
                    placement.track_after_committed_child(child_margin_box_block_extent);
                if self.principal_flow.is_source_body(element) {
                    let active_canvas = self
                        .root_principal_flow_context
                        .active_canvas
                        .as_mut()
                        .expect("a propagated body keeps an active document canvas");
                    debug_assert_eq!(active_canvas.body, Some(element.id));
                    active_canvas.trailing_child_block_margin = layout_pt(child_block_end_margin);
                }
                let translation = if bottom_origin_canvas_child {
                    fragment
                        .bounds()
                        .map(|bounds| {
                            PaintTranslation::new(
                                0.0,
                                self.page_bottom()
                                    + if self.principal_flow.is_source_body(element) {
                                        self.principal_inline_end_inset
                                    } else {
                                        0.0
                                    }
                                    - bounds.y(),
                            )
                        })
                        .unwrap_or_else(PaintTranslation::identity)
                } else if bottom_origin_nested_child {
                    self.last_block_layout_outcome
                        .static_border_box
                        .map(|border_box| {
                            // Nested bottom-origin vertical flow is laid out
                            // once at the physical top scratch origin, then
                            // projected so the child's physical bottom margin
                            // edge meets the containing inline-start edge.
                            // Use principal-box geometry rather than ink
                            // bounds, which can include glyph overflow or be
                            // absent for an empty child.
                            // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
                            PaintTranslation::new(
                                0.0,
                                placement.page_inline_origin().points() + child_style.margin.top
                                    - border_box.min_y(),
                            )
                        })
                        .unwrap_or_else(PaintTranslation::identity)
                } else {
                    PaintTranslation::identity()
                };
                self.current_page
                    .append_paint_fragment_owned(fragment, translation);
                self.cursor_y = if bottom_origin_canvas_child || bottom_origin_nested_child {
                    canvas_inline_origin
                } else {
                    placement.page_inline_origin().points()
                };
                if !source_body_canvas {
                    self.content_left = remaining_block_track.left_x();
                    self.content_right = remaining_block_track.right_x();
                }
            }
            if self.last_block_layout_outcome.margin_collapse_boundary
                == BlockMarginCollapseBoundary::Adjoining
                && let Some(offset) = adjoining_start_margin_paint_offset
            {
                dom_state.collapsed_start_margin_offset =
                    dom_state.collapsed_start_margin_offset.max(offset);
            }
            let emitted_float = self
                .float_contexts
                .last()
                .is_some_and(|context| context.shapes.len() > float_shape_count_before);
            if emitted_float && let Some(mut candidate) = adjoining_candidate {
                if self.last_block_layout_outcome.margin_collapse_boundary
                    == BlockMarginCollapseBoundary::SeparatedByClearance
                    && let Some(border_box) = self.last_block_layout_outcome.static_border_box
                {
                    candidate
                        .record_clearance_boundary(PageTopBlockPosition::new(border_box.max_y()));
                }
                dom_state.adjoining_float_replay = Some(candidate);
            }
            if is_flow_child {
                let child_start_separated_by_clearance = child_uses_block_layout
                    && self.last_block_layout_outcome.margin_collapse_boundary
                        == BlockMarginCollapseBoundary::SeparatedByClearance;
                if child_start_separated_by_clearance {
                    dom_state.adjoining_margin_set_boundary =
                        BlockMarginCollapseBoundary::SeparatedByClearance;
                }
                let child_consumed_bottom_margin = if child_uses_block_layout {
                    self.last_block_layout_outcome
                        .consumed_bottom_margin
                        .points()
                } else {
                    child_style.margin.bottom
                };
                dom_state.previous_flow_bottom_margin = if trims_self_collapsing_end_margin_set {
                    Some(0.0)
                } else if self_collapsing_child {
                    Some(if trimmed_block_start_margin {
                        0.0
                    } else if child_start_separated_by_clearance {
                        // CSS2 clearance separates the box's top margin from
                        // its own bottom margin and from its parent. Its
                        // bottom margin may still adjoin a following sibling.
                        child_consumed_bottom_margin
                    } else {
                        dom_state
                            .previous_flow_bottom_margin
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
                if collapses_with_parent_end && !child_start_separated_by_clearance {
                    let adjoining_end_margin = if self_collapsing_child
                        && !child_style.display.establishes_block_formatting_context()
                    {
                        dom_state
                            .previous_flow_bottom_margin
                            .unwrap_or(child_consumed_bottom_margin)
                    } else {
                        child_consumed_bottom_margin
                    };
                    dom_state.pending_end_margin_collapse = Some(BlockEndMarginCollapse {
                        child_consumed_margin: layout_pt(child_consumed_bottom_margin),
                        collapsed_margin: collapse_margins(
                            layout_pt(adjoining_end_margin),
                            layout_pt(style.margin.bottom),
                        ),
                    });
                }
                if zero_height_page_boundary {
                    dom_state.previous_child_page_end = effective_child_page_start;
                } else if let Some(values) = child_page_values {
                    dom_state.previous_child_page_end = Some(values.end);
                }
                if traversal_state.has_automatic_block_size_clamp()
                    && let Some(child_flow_start) = automatic_child_flow_start
                {
                    // The legacy page cursor can include a descendant's
                    // local overflow or omit an authored definite block
                    // extent. Automatic clamp candidates instead consume the
                    // child's resolved normal-flow margin-box contribution.
                    // The static border box is source geometry, before paint
                    // transforms and clipping; use the cursor only for box
                    // kinds that cannot expose one.
                    // <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
                    let static_block_contribution = self
                        .last_block_layout_outcome
                        .static_border_box
                        .map(|border_box| {
                            let border_box_block_size =
                                if WritingModeAxes::new(style.writing_mode, style.direction)
                                    .swaps_physical_axes()
                                {
                                    border_box.size.width
                                } else {
                                    border_box.size.height
                                };
                            crate::units::content_box_pt(
                                (border_box_block_size
                                    + child_style.margin.top
                                    + child_style.margin.bottom)
                                    .max(0.0),
                            )
                        })
                        .unwrap_or_else(|| {
                            crate::units::content_box_pt(
                                (child_flow_start - self.cursor_y).max(0.0),
                            )
                        });
                    traversal_state.debit_automatic_block_contribution(static_block_contribution);
                }
                if child_uses_block_layout && child_shares_clamp_context {
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
            let replay_preceding_child_for_empty_automatic_cutoff = child_shares_clamp_context
                && self.last_block_layout_outcome.clamp_line_slots == 0
                && self.last_block_layout_outcome.has_local_continuation_cutoff
                && dom_state.automatic_marker_replay_target != Some(dom_state.child_node_index);
            if replay_preceding_child_for_empty_automatic_cutoff
                && let Some(checkpoint) = dom_state.automatic_marker_candidate.take()
            {
                dom_state.restore_automatic_marker_checkpoint(checkpoint, self, traversal_state);
                continue;
            }
            let replay_current_child_as_automatic_marker = traversal_state
                .has_automatic_block_size_clamp()
                && traversal_state.is_exhausted()
                && child_shares_clamp_context
                && has_later_in_flow_child
                && self.last_block_layout_outcome.clamp_line_slots > 0
                && !self.last_block_layout_outcome.has_local_continuation_cutoff
                && dom_state.automatic_marker_replay_target != Some(dom_state.child_node_index);
            if replay_current_child_as_automatic_marker
                && let Some(checkpoint) = automatic_replay_before_child
            {
                dom_state.restore_automatic_marker_checkpoint(checkpoint, self, traversal_state);
                continue;
            }
            if traversal_state.has_automatic_block_size_clamp()
                && child_shares_clamp_context
                && self.last_block_layout_outcome.clamp_line_slots > 0
                && !self.last_block_layout_outcome.has_local_continuation_cutoff
                && dom_state.automatic_marker_replay_target != Some(dom_state.child_node_index)
            {
                dom_state.automatic_marker_candidate = automatic_replay_before_child;
            }
            if dom_state.automatic_marker_replay_target == Some(dom_state.child_node_index) {
                dom_state.automatic_marker_replay_target = None;
            }
            dom_state.avoid_run_candidate = if avoid_run_start_decision.seeds_later_avoid_boundary {
                match (run_start_candidate.take(), child_avoid_block_extent) {
                    (Some(candidate), Some(child_block_extent)) => {
                        Some(candidate.add_block_extent(child_block_extent))
                    }
                    (Some(candidate), None) => Some(candidate),
                    (None, _) => None,
                }
            } else {
                None
            };
            dom_state.previous_break_after = if is_flow_child {
                dom_state.out_of_flow_static_source = Some(
                    self.block_static_position_rectangle_at(PageTopBlockPosition::new(
                        self.cursor_y,
                    )),
                );
                child_break_context
                    .avoid_after_in(fragmentainer_kind)
                    .unwrap_or(PageBreak::Auto)
            } else {
                PageBreak::Auto
            };
            dom_state.child_node_index += 1;
        }
        self.flush_float_run(&mut dom_state.float_run);
        ChildFlowTraversalOutcome {
            pending_end_margin_collapse: dom_state.pending_end_margin_collapse,
            collapsed_start_margin_offset: dom_state.collapsed_start_margin_offset,
            adjoining_margin_set_boundary: dom_state.adjoining_margin_set_boundary,
            rendered_legend: None,
        }
    }

    /// Construct the auto-height margin-collapse view outside the recursive
    /// DOM child traversal frame. This override is required only when a
    /// definite descendant percentage basis makes an authored auto height
    /// behave specially for CSS margin collapsing.
    #[inline(never)]
    fn dom_auto_height_margin_collapse_style(style: &ComputedStyle) -> Box<ComputedStyle> {
        let mut style = Box::new(style.clone());
        style
            .box_values
            .height
            .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
        style
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
            let signature =
                ElementSignature::from_sibling_snapshot(element_index, sibling_tags.clone())
                    .expect("source child must have a cached sibling signature");
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
                style,
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
            let signature =
                ElementSignature::from_sibling_snapshot(element_index, siblings.clone())
                    .expect("source child must have a cached sibling signature");
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
