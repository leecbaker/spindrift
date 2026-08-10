use super::shared::*;
use super::state::{
    BlockFlowChildTraversalState, ChildFlowTraversalOutcome, RenderedLegendGeometry,
};
use super::*;
use crate::layout::inline_collect::TextDecorationPropagationContext;
use std::num::NonZeroUsize;

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_formatting_box_flow_children(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: &[box_tree::FormattingBox<'_>],
        can_collapse_start_margin: bool,
        can_collapse_end_margin: bool,
        applied_start_margin: LayoutLength,
        clearance_consumed_adjoining_start_margin: bool,
        starts_at_page_top: bool,
        has_preceding_inline_flow_content: bool,
        run_in_inline_items_laid_out: bool,
        traversal_state: &mut BlockFlowChildTraversalState,
    ) -> ChildFlowTraversalOutcome {
        let mut collapsed_end_margin = false;
        let mut pending_end_margin_collapse = None;
        let mut collapsed_start_margin_offset = layout_pt(0.0);
        let mut rendered_legend = None;
        let mut previous_flow_bottom_margin = None;
        let mut seen_flow_child = false;
        let mut trim_block_start_adjoining_margins = style.margin_trim.block_start;
        let mut float_run = self.float_run_state();
        // Direct inline content is laid out before this child traversal and
        // is not represented by a formatting-box sibling. It still occupies
        // the source side of the first class-A break: a following named block
        // must start its own page group rather than retroactively selecting
        // the page that contains that text.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        let mut previous_child_page_end = has_preceding_inline_flow_content
            .then(|| self.page_boundary_name_in_active_scope(PageBoundaryValue::Inherited, style));
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
            if traversal_state.has_reached_discard_region_limit(self.pages.len()) {
                let child_count = NonZeroUsize::new(child_box_index);
                debug_assert!(child_count.is_some(), "a local region break retains source");
                if let Some(child_count) = child_count {
                    traversal_state.capture_discard_source_prefix(child_count);
                } else {
                    traversal_state.mark_local_continuation_cutoff();
                }
                break;
            }
            let replaying_adjoining_target = if replaying_adjoining_until == Some(child_box_index) {
                replaying_adjoining_until = None;
                self.adjoining_float_origin_y.take();
                true
            } else {
                false
            };
            let raw_child_box = &child_boxes[child_box_index];
            // The run-in target has already collected these anonymous inline
            // wrappers together with its run-in prelude. Replaying them as
            // block-flow children would create a second line after the
            // merged sequence. Preserve actual block-flow descendants for
            // the normal traversal below.
            if run_in_inline_items_laid_out
                && matches!(raw_child_box, box_tree::FormattingBox::AnonymousBlock(box_)
                    if formatting_box_has_inline_content(&box_.children)
                        && !has_non_inline_formatting_box(&box_.children))
            {
                child_box_index += 1;
                continue;
            }
            let raw_child_is_in_normal_flow = raw_child_box
                .element_parts()
                .is_none_or(|(_, _, child_style, _)| style_is_in_normal_flow(child_style));
            let raw_child_is_float = raw_child_box
                .element_parts()
                .is_some_and(|(_, _, child_style, _)| child_style.float != Float::None);
            if traversal_state.is_exhausted() && (raw_child_is_in_normal_flow || raw_child_is_float)
            {
                traversal_state.capture_forced_discard_before_later_child(child_box_index);
                // A later float belongs to the discarded source just like a
                // later in-flow box. Positioned descendants retain their
                // independent containing-block layout pass.
                child_box_index += 1;
                continue;
            }
            let (split_block_context, child_box) = match raw_child_box {
                box_tree::FormattingBox::InlineSplitBlockContext(context)
                    if context.core.children.len() == 1 =>
                {
                    (Some(context), &context.core.children[0])
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
            let (child_break_before, child_break_after) =
                formatting_box_fragment_boundary_breaks(child_box, fragmentainer_kind);
            let child_avoid_break_flow =
                child_parts.is_some_and(|(child_element, _, child_style, _)| {
                    block_avoid_break_flow_child(element, child_element, child_style)
                });
            let next_flow_child_break_before = next_formatting_box_flow_child_break_before(
                element,
                child_boxes,
                child_box_index,
                fragmentainer_kind,
            );
            let child_break_context = child_parts
                .map(|_| {
                    FragmentBreakContext::new(
                        PageBreak::Auto,
                        child_break_before,
                        child_break_after,
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
            let inherited_page_name = self.active_page_value_scope(style);
            let child_page_value_sources = formatting_box_is_page_value_participant(child_box)
                .then(|| {
                    resolved_formatting_box_page_boundary_values(
                        child_box,
                        inherited_page_name.as_deref(),
                    )
                });
            let effective_child_page_start = if zero_height_page_boundary {
                Some(coalesced_zero_height_page_start(
                    child_boxes,
                    child_box_index,
                    inherited_page_name.as_deref(),
                ))
            } else {
                child_page_value_sources
                    .as_ref()
                    .map(|sources| sources.start.clone())
            };
            let child_page_boundary_selected = if let Some(child_page_start) =
                &effective_child_page_start
                && previous_child_page_end
                    .as_ref()
                    .is_none_or(|previous_page_end| previous_page_end != child_page_start)
                && (!self.current_page_has_content() || previous_child_page_end.is_some())
            {
                // Match DOM traversal: the first normal-flow child selects an
                // explicit named page even without a preceding class-A
                // sibling boundary.
                // <https://www.w3.org/TR/css-page-3/#using-named-pages>
                self.switch_page_name_at_class_a_boundary(child_page_start.as_deref());
                true
            } else {
                false
            };
            if let box_tree::FormattingBox::AnonymousBlock(box_) = child_box {
                self.flush_float_run(&mut float_run);
                let root_principal_inline_pseudo = element.tag.eq_ignore_ascii_case("html")
                    && self.principal_flow.has_propagated_body()
                    && WritingModeAxes::new(style.writing_mode, style.used_direction())
                        .swaps_physical_axes()
                    && anonymous_block_wraps_root_principal_pseudo(&box_.children);
                let consuming_root_canvas = root_principal_inline_pseudo
                    && self.begin_root_inline_canvas_continuation(element);
                // An anonymous inline wrapper following a propagated
                // sideways-lr body (notably a root ::after) shares the
                // bottom-origin inline axis. The legacy pagination cursor is
                // top-origin, so use a scratch line and project its paint
                // back to the canvas edge just as for block children.
                // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
                let bottom_inline_start = !consuming_root_canvas
                    && WritingModeAxes::new(style.writing_mode, style.used_direction())
                        .swaps_physical_axes()
                    && self.active_fragmentainer_kind() == FragmentainerKind::Page
                    && inline_start_side(style.writing_mode, style.used_direction())
                        == PhysicalSide::Bottom;
                let inline_cursor = self.cursor_y;
                let page_index = self.pages.len();
                let paint_checkpoint =
                    bottom_inline_start.then(|| self.current_page.paint_checkpoint());
                if bottom_inline_start {
                    self.cursor_y = self.page_top();
                }
                let allow_typographic_first_line =
                    first_formatted_line.applies_to_next_inline_run();
                let initial_first_formatted_line = first_formatted_line.is_pending();
                let decoration_context = TextDecorationPropagationContext::from_style(style);
                let propagated_anonymous_style = decoration_context.used_child_style(&box_.style);
                let originating_pseudo_style = allow_typographic_first_line
                    .then(|| {
                        style_with_originating_typographic_pseudos(
                            &propagated_anonymous_style,
                            style,
                        )
                    })
                    .flatten();
                let anonymous_style = originating_pseudo_style
                    .as_ref()
                    .unwrap_or(&propagated_anonymous_style);
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
                            initial_first_formatted_line,
                        )
                    });
                if consuming_root_canvas {
                    self.finish_root_inline_canvas_continuation();
                }
                if let Some(checkpoint) = paint_checkpoint
                    && self.pages.len() == page_index
                {
                    let fragment = self.current_page.take_paint_fragment_since(checkpoint);
                    let translation = fragment
                        .bounds()
                        .map(|bounds| {
                            self.page_bottom() + self.principal_inline_end_inset - bounds.y()
                        })
                        .unwrap_or(0.0);
                    self.current_page.append_paint_fragment_owned(
                        fragment,
                        PaintTranslation::new(0.0, translation),
                    );
                    self.cursor_y = inline_cursor;
                }
                if inline_outcome.has_flow_effects {
                    seen_flow_child = true;
                }
                if inline_outcome.has_non_phantom_line {
                    first_formatted_line.consume_next_formatted_line();
                }
                traversal_state.debit_inline_outcome(inline_outcome);
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
                } else if let Some(sources) = child_page_value_sources {
                    // The next class-A sibling compares against this
                    // child's propagated *end* value. Its start can
                    // remain `auto` while a later descendant selects
                    // a named page, in which case using the start here
                    // would lose the required destination transition.
                    // <https://www.w3.org/TR/css-page-3/#using-named-pages>
                    previous_child_page_end = Some(sources.end);
                }
                child_box_index += 1;
                continue;
            }
            let Some((child_element, child_signature, child_style, child_children)) = child_parts
            else {
                child_box_index += 1;
                continue;
            };
            let decoration_context = TextDecorationPropagationContext::from_style(style);
            let mut child_style = Box::new(decoration_context.used_child_style(child_style));
            // A block container whose first in-flow child is block-level takes
            // its first formatted line from that child. Carry the originating
            // typographic pseudo into the child's layout style so a deeper
            // block descent continues to find the same first line instead of
            // treating the ancestor pseudo as consumed before any line exists.
            // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
            let child_receives_originating_first_line = first_formatted_line
                .applies_to_next_inline_run()
                && style_is_in_normal_flow(&child_style)
                && child_style.display.is_block_level();
            if child_receives_originating_first_line
                && let Some(style_with_originating_pseudos) =
                    style_with_originating_typographic_pseudos(&child_style, style)
            {
                *child_style = style_with_originating_pseudos;
            }
            let child_shares_clamp_context =
                self.child_shares_line_clamp_formatting_context(child_element, &child_style);
            if child_shares_clamp_context {
                let has_later_in_flow_child =
                    child_boxes[child_box_index + 1..].iter().any(|candidate| {
                        candidate
                            .element_parts()
                            .is_none_or(|(_, _, style, _)| style_is_in_normal_flow(style))
                    });
                traversal_state.apply_to_with_continuation(
                    &mut child_style,
                    BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                        has_later_in_flow_child,
                    ),
                );
            }
            // A following block sibling establishes a clamp point *after*
            // this child; it is not inline overflow inside the child. Passing
            // that fact into the child's inline selector incorrectly paints
            // an ellipsis after a terminal block-in-inline line.
            // Margin-collapse and sibling placement consume the physical
            // margin cache before the child's own block-layout geometry runs.
            // Resolve percentage edges here against the parent's logical
            // inline basis, rather than leaving (for example) a vertical
            // child's `margin-top: 10%` as the stale zero-length cache value.
            // CSS percentages on every margin edge use the containing block's
            // inline size, independent of the child's writing mode.
            // <https://www.w3.org/TR/CSS22/box.html#margin-properties>
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
            let parent_inline_percentage_basis = self
                .current_child_available_space()
                .logical_inline_percentage_basis_for(style.writing_mode);
            apply_used_box_metrics_for_logical_inline_basis(
                &mut child_style,
                parent_inline_percentage_basis,
            );
            let child_table_fragment = if let box_tree::FormattingBox::Table(table_box) = child_box
            {
                Some(&table_box.fragment)
            } else {
                None
            };
            let split_inline_float_block_offset = split_block_context.and_then(|_| {
                self.split_inline_static_position_y_offset_before_child(
                    child_boxes,
                    child_box_index,
                    style,
                    stylesheets,
                )
            });
            if let Some(committed) = self.committed_inline_floats.remove(&child_element.id) {
                // The selected inline source row owns this float's exclusion
                // and paint capture.  Do not replay the same formatting box
                // as an independent block-child float.
                debug_assert!(committed.is_valid());
                seen_flow_child = true;
                previous_flow_bottom_margin = None;
                child_box_index += 1;
                continue;
            }
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
                    split_inline_float_block_offset,
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
            let block_end_margin_trim = BlockEndMarginTrim::for_child(
                style,
                is_flow_child,
                has_later_normal_block_flow_box_child(
                    child_boxes,
                    child_box_index + 1,
                    element,
                    self.document_canvas_overflow,
                ),
            );
            block_end_margin_trim.apply_to_child(&mut child_style);
            let descendant_start_margin = (is_flow_child
                && can_collapse_block_start_margin(
                    child_element,
                    &child_style,
                    UsedEdges::from_css_edges(used_border_widths(&child_style)),
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
                && is_self_collapsing_block_box(
                    child_element,
                    margin_collapse_style,
                    child_children,
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
                // This traversal's start-margin machinery is expressed in
                // physical top/bottom coordinates. In a vertical principal
                // flow, those coordinates are the inline axis, not the
                // logical block axis whose margins may adjoin. In particular,
                // a root body's UA `margin-top` must not disappear merely
                // because its first in-flow descendant happens to be
                // collapsible.
                // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
                // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
                let physical_top_is_inline_axis =
                    WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes();
                let adjoins_parent_start = !trimmed_block_start_margin
                    && !seen_flow_child
                    && can_collapse_start_margin
                    && collapses_with_parent
                    && !physical_top_is_inline_axis;
                if adjoins_parent_start {
                    inherited_adjoining_start_margin = Some(adjoining_start_margin.value());
                    if let Some(previous_margin) = previous_flow_bottom_margin {
                        // An earlier self-collapsing sibling has
                        // already contributed its adjoining margin at
                        // the parent start. Merge that contribution with the
                        // parent's already-applied start margin before
                        // calculating the next delta: a zero-margin float
                        // wrapper must not discard the ancestor margin in
                        // this same collapsed set.
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
                    && !has_later_normal_block_flow_box_child(
                        child_boxes,
                        child_box_index + 1,
                        element,
                        self.document_canvas_overflow,
                    );
            }
            // Margin-collapsing and fragmentainer replay only adjust
            // in-flow children. Freezing an out-of-flow child's scalar cache
            // here would turn its authored `auto` block margins into zero
            // before absolute-positioned layout can solve the inset equation.
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
                    Some(child_children),
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
                    Some(child_children),
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
            if let Some(margin) = inherited_adjoining_start_margin {
                self.inherited_adjoining_start_margins.push(margin);
            }
            // `layout_block` normalizes a fieldset into a frozen child list
            // with its selected rendered legend at index zero.  Keep the
            // source-fragment identity with that selection so continuation
            // fragments never manufacture a second legend exclusion.
            // <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
            let rendered_fieldset_legend = element.tag.eq_ignore_ascii_case("fieldset")
                && child_box_index == 0
                && box_tree::FieldsetFormattingBox::from_children(child_boxes)
                    .rendered_legend_index
                    == Some(0);
            let fieldset_legend_paint_checkpoint =
                rendered_fieldset_legend.then(|| self.current_page.paint_checkpoint());
            let fieldset_legend_page_index = self.pages.len();

            if is_flow_child {
                if !self_collapsing_child {
                    seen_flow_child = true;
                    first_formatted_line.consume_next_formatted_line();
                }
                if trim_block_start_adjoining_margins && !self_collapsing_child {
                    trim_block_start_adjoining_margins = false;
                }
            } else if !matches!(child_style.position, Position::Absolute | Position::Fixed) {
                // Positioned source children do not participate in normal
                // flow, so their presence must not end an adjoining in-flow
                // sibling margin set.
                // <https://www.w3.org/TR/CSS22/visuren.html#absolute-positioning>
                // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
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
            let clearance_count_before_child = self.applied_clearance_count;
            // Block traversal's legacy cursor is physical top-to-bottom.
            // The principal vertical flow instead advances through the page's
            // horizontal span; preserve the inline cursor while consuming the
            // completed child fragment from logical block-start.
            let principal_vertical_flow = is_flow_child
                && WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes()
                && self.active_fragmentainer_kind() == FragmentainerKind::Page;
            let principal_flow_page_index = self.pages.len();
            let principal_flow_paint_checkpoint =
                principal_vertical_flow.then(|| self.current_page.paint_checkpoint());
            let principal_flow_inline_cursor = self.cursor_y;
            // The physical `content_left`/`content_right` span is the root
            // principal flow's logical block cursor in vertical writing.  A
            // finished fragment consumes that span before the next sibling is
            // laid out.  Page transitions must therefore happen at the next
            // logical block fragmentainer, rather than waiting for the legacy
            // top-to-bottom cursor to overflow.
            //
            // This is intentionally before the child is entered: once the
            // preceding child filled the horizontal block span its paint
            // belongs to the preceding page, while the following child starts
            // at the block-start edge of a fresh page area.
            // <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            if principal_vertical_flow
                && is_flow_child
                && self.content_left >= self.content_right - 0.01
                && self.current_page_has_content()
            {
                self.push_page();
            }
            // CSS Writing Modes makes the selected HTML body the source of
            // the document canvas and initial containing block. Its automatic
            // canvas-sized block span is not an ordinary sibling advance in
            // the root principal flow: otherwise a following anonymous root
            // inline box (such as `html::after`) starts after the whole
            // viewport instead of after the body's actual document flow.
            // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
            let source_body_canvas =
                principal_vertical_flow && self.principal_flow.is_source_body(child_element);
            self.last_block_layout_outcome = BlockLayoutOutcome::default();
            let mut rendered_legend_style;
            let child_layout_style =
                if rendered_fieldset_legend && child_style.box_values.width.is_auto() {
                    rendered_legend_style = child_style.clone();
                    rendered_legend_style.box_values.width =
                        css::ComputedLengthPercentageOrAuto::MaxContent;
                    &rendered_legend_style
                } else {
                    child_style
                };
            if child_layout_style.display.is_block_level() {
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
                // Suppress entry only when the parent has actually selected
                // this normal-flow child's page group at the class-A sibling
                // boundary above. A first named child after direct inline
                // content has no preceding formatting-box sibling, so it
                // must retain its own entry scope to form that boundary.
                // Re-entering a page value already selected by the parent
                // would manufacture a second break instead.
                // Nested sibling boundaries remain active while this
                // element-entry scope is suppressed.
                // <https://www.w3.org/TR/css-page-3/#using-named-pages>
                let boundary_selected_page_scope =
                    is_flow_child && !zero_height_page_boundary && child_page_boundary_selected;
                if boundary_selected_page_scope {
                    self.push_page_name_element_scope_suppression();
                }
                if zero_height_page_boundary {
                    self.push_page_name_element_scope_suppression();
                }
                if let box_tree::FormattingBox::Table(table_box) = child_box {
                    let split_scope =
                        split_block_context.map(|_| self.begin_inline_split_block_paint_scope());
                    self.with_inline_split_block_relative_layout_scope(
                        split_block_context,
                        |layout| {
                            layout.with_text_box_line_trim_scope(
                                child_text_box_line_trim,
                                |layout| {
                                    layout.layout_element_with_child_boxes_and_table_fragment(
                                        table_box.core.element,
                                        child_layout_style,
                                        stylesheets,
                                        Some(&table_box.core.children),
                                        Some(&table_box.fragment),
                                    );
                                },
                            );
                        },
                    );
                    if let (Some(context), Some(scope)) = (split_block_context, split_scope) {
                        self.finish_inline_split_block_paint_scope(context, scope);
                    }
                } else if let box_tree::FormattingBox::Block(block_box) = child_box {
                    let split_scope =
                        split_block_context.map(|_| self.begin_inline_split_block_paint_scope());
                    self.with_inline_split_block_relative_layout_scope(
                        split_block_context,
                        |layout| {
                            layout.with_text_box_line_trim_scope(
                                child_text_box_line_trim,
                                |layout| {
                                    if let box_tree::BoxSource::GeneratedPseudo(pseudo) =
                                        &block_box.core.source
                                    {
                                        layout.layout_generated_pseudo_box(
                                            child_element,
                                            child_layout_style,
                                            pseudo.kind.counter_event_source(),
                                            stylesheets,
                                            &block_box.run_in_children,
                                            Some(child_children),
                                            None,
                                        );
                                    } else {
                                        let principal_root_layout_style = child_element
                                            .tag
                                            .eq_ignore_ascii_case("html")
                                            .then(|| {
                                                layout
                                                    .principal_flow
                                                    .root_layout_style(child_layout_style)
                                            });
                                        layout.layout_element_with_child_boxes_and_run_ins(
                                            child_element,
                                            principal_root_layout_style
                                                .as_ref()
                                                .unwrap_or(child_layout_style),
                                            stylesheets,
                                            &block_box.run_in_children,
                                            Some(child_children),
                                        );
                                    }
                                },
                            );
                        },
                    );
                    if let (Some(context), Some(scope)) = (split_block_context, split_scope) {
                        self.finish_inline_split_block_paint_scope(context, scope);
                    }
                } else {
                    let split_scope =
                        split_block_context.map(|_| self.begin_inline_split_block_paint_scope());
                    self.with_inline_split_block_relative_layout_scope(
                        split_block_context,
                        |layout| {
                            layout.with_text_box_line_trim_scope(
                                child_text_box_line_trim,
                                |layout| {
                                    if let Some(core) = child_box.element_core()
                                        && let box_tree::BoxSource::GeneratedPseudo(pseudo) =
                                            &core.source
                                    {
                                        layout.layout_generated_pseudo_box(
                                            child_element,
                                            child_layout_style,
                                            pseudo.kind.counter_event_source(),
                                            stylesheets,
                                            &[],
                                            Some(child_children),
                                            None,
                                        );
                                    } else {
                                        let principal_root_layout_style = child_element
                                            .tag
                                            .eq_ignore_ascii_case("html")
                                            .then(|| {
                                                layout
                                                    .principal_flow
                                                    .root_layout_style(child_layout_style)
                                            });
                                        layout.layout_element_with_child_boxes(
                                            child_element,
                                            principal_root_layout_style
                                                .as_ref()
                                                .unwrap_or(child_layout_style),
                                            stylesheets,
                                            Some(child_children),
                                        );
                                    }
                                },
                            );
                        },
                    );
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
                if inherited_adjoining_start_margin.is_some() {
                    self.inherited_adjoining_start_margins.pop();
                }
                if rendered_fieldset_legend
                    && self.pages.len() == fieldset_legend_page_index
                    && let Some(border_box) = self.last_block_layout_outcome.static_border_box
                {
                    // The fieldset has already placed its regular content
                    // cursor at the original padding edge.  HTML instead
                    // centers the rendered legend's border box on the
                    // fieldset block-start border.  Derive that translation
                    // from resolved box geometry, never the legend's ink.
                    // <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
                    // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
                    let parent_block_start = block_start_side(style.writing_mode);
                    let parent_border_start =
                        physical_edge_value(used_border_widths(style), parent_block_start);
                    let parent_padding_start =
                        physical_edge_value(style.padding, parent_block_start);
                    let child_margin_start =
                        physical_edge_value(child_style.margin, parent_block_start);
                    let (current_block_start, current_border_center) = match parent_block_start {
                        PhysicalSide::Top => (
                            border_box.max_y() + child_margin_start,
                            border_box.origin.y + border_box.size.height / 2.0,
                        ),
                        PhysicalSide::Right => (
                            border_box.max_x() + child_margin_start,
                            border_box.origin.x + border_box.size.width / 2.0,
                        ),
                        PhysicalSide::Bottom => (
                            border_box.min_y() - child_margin_start,
                            border_box.origin.y + border_box.size.height / 2.0,
                        ),
                        PhysicalSide::Left => (
                            border_box.min_x() - child_margin_start,
                            border_box.origin.x + border_box.size.width / 2.0,
                        ),
                    };
                    let target_border_center = match parent_block_start {
                        PhysicalSide::Top | PhysicalSide::Right => {
                            current_block_start + parent_padding_start + parent_border_start / 2.0
                        }
                        PhysicalSide::Bottom | PhysicalSide::Left => {
                            current_block_start - parent_padding_start - parent_border_start / 2.0
                        }
                    };
                    let static_offset = match parent_block_start {
                        PhysicalSide::Top | PhysicalSide::Bottom => {
                            PaintTranslation::new(0.0, target_border_center - current_border_center)
                        }
                        PhysicalSide::Left | PhysicalSide::Right => {
                            PaintTranslation::new(target_border_center - current_border_center, 0.0)
                        }
                    };
                    let static_border_box = static_offset.transform_rect(&border_box);
                    rendered_legend = Some(RenderedLegendGeometry::from_static_border_box(
                        static_border_box,
                        child_style.margin,
                        fieldset_legend_page_index,
                    ));
                    if let Some(checkpoint) = fieldset_legend_paint_checkpoint {
                        let fragment = self.current_page.take_paint_fragment_since(checkpoint);
                        self.current_page
                            .append_paint_fragment_owned(fragment, static_offset);
                    }
                    // A normal first child starts after both the fieldset
                    // block-start border and its own margin box.  The HTML
                    // rendered-legend model reserves their maximum instead.
                    // The horizontal block cursor is the physical vertical
                    // cursor; vertical writing modes use their dedicated
                    // logical sibling advance below.
                    if style.writing_mode == WritingMode::HorizontalTb {
                        let legend_margin_block_span = border_box.size.height
                            + child_style.margin.top
                            + child_style.margin.bottom;
                        self.cursor_y += parent_border_start.min(legend_margin_block_span);
                    }
                }
                self.flush_float_run(&mut float_run);
            }
            if let Some(checkpoint) = principal_flow_paint_checkpoint
                && self.pages.len() == principal_flow_page_index
            {
                let fragment = self.current_page.take_paint_fragment_since(checkpoint);
                // A logical block sibling consumes the preceding child's used
                // border-box span, regardless of whether that child is the
                // document canvas. Paint bounds include glyph overhang and
                // omit empty boxes, so using them here changes normal-flow
                // placement according to incidental ink.
                // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
                let block_advance = self
                    .last_block_layout_outcome
                    .physical_border_box_inline_span
                    .points()
                    + match block_start_side(style.writing_mode) {
                        PhysicalSide::Left => child_style.margin.right,
                        PhysicalSide::Right => child_style.margin.left,
                        PhysicalSide::Top | PhysicalSide::Bottom => {
                            unreachable!(
                                "a vertical writing mode must have a horizontal block axis"
                            )
                        }
                    };
                if is_flow_child && self.principal_flow.is_source_body(element) {
                    // The body's ordinary child cursor is local to the
                    // document canvas. Retain only its trailing logical
                    // margin for a later root-inline continuation; its box
                    // span is never a root sibling advance.
                    // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
                    let trailing_child_margin = match block_start_side(style.writing_mode) {
                        PhysicalSide::Left => child_style.margin.right,
                        PhysicalSide::Right => child_style.margin.left,
                        PhysicalSide::Top | PhysicalSide::Bottom => unreachable!(
                            "a vertical writing mode must have a horizontal block axis"
                        ),
                    };
                    let active_canvas = self
                        .root_principal_flow_context
                        .active_canvas
                        .as_mut()
                        .expect("a propagated body keeps an active document canvas");
                    debug_assert_eq!(active_canvas.body, Some(element.id));
                    active_canvas.trailing_child_block_margin = layout_pt(trailing_child_margin);
                }
                let canvas_child_block_start_translation =
                    if self.principal_flow.is_source_body(element)
                        && style.writing_mode == WritingMode::VerticalRl
                        && child_style.margin.right > 0.0
                    {
                        // The body's physical block-start canvas inset and
                        // the child's own block-start margin both offset a
                        // non-initial canvas child. They are layout
                        // coordinates, not ink bounds, so preserve them when
                        // projecting the vertical-rl canvas child.
                        // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
                        style.margin.right + child_style.margin.right
                    } else {
                        0.0
                    };
                self.current_page.append_paint_fragment_owned(
                    fragment,
                    PaintTranslation::new(canvas_child_block_start_translation, 0.0),
                );
                self.cursor_y = principal_flow_inline_cursor;
                if !source_body_canvas {
                    match block_start_side(style.writing_mode) {
                        PhysicalSide::Left => {
                            self.content_left =
                                (self.content_left + block_advance).min(self.content_right)
                        }
                        PhysicalSide::Right => {
                            self.content_right =
                                (self.content_right - block_advance).max(self.content_left)
                        }
                        PhysicalSide::Top | PhysicalSide::Bottom => {
                            unreachable!(
                                "a vertical writing mode must have a horizontal block axis"
                            )
                        }
                    }
                }
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
                previous_flow_bottom_margin = if principal_vertical_flow {
                    // This traversal cursor is physical top-to-bottom, but
                    // a principal vertical flow advances along the physical
                    // horizontal block axis. The child's physical bottom
                    // margin is consequently an inline-axis margin and must
                    // not collapse into the next sibling's physical top
                    // margin.
                    // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
                    // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
                    None
                } else if trims_self_collapsing_end_margin_set {
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
                    outer_margins_adjoin_block_siblings(child_element, child_style)
                        .then_some(child_consumed_bottom_margin)
                };
                if collapses_with_parent_end {
                    // A self-collapsing normal-flow child can have already
                    // folded both margins into the adjoining sibling set,
                    // leaving a zero consumed bottom edge. A child BFC (such
                    // as a generated `display: flow-root` pseudo) is a
                    // boundary instead: importing that set would collapse
                    // margins through the BFC.
                    // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
                    // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
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
                if child_uses_block_layout && child_shares_clamp_context {
                    traversal_state.record_descendant_clamp_line_slots(
                        self.last_block_layout_outcome.clamp_line_slots,
                    );
                }
            }
            if child_uses_block_layout && child_shares_clamp_context {
                traversal_state
                    .debit_rendered_slots(self.last_block_layout_outcome.clamp_line_slots);
                if traversal_state.has_active_clamp()
                    && self.last_block_layout_outcome.has_local_continuation_cutoff
                {
                    traversal_state.mark_local_continuation_cutoff();
                }
            }
            if zero_height_page_boundary {
                if let Some(child_page_start) = effective_child_page_start {
                    previous_child_page_end = Some(child_page_start);
                }
            } else if let Some(sources) = child_page_value_sources {
                previous_child_page_end = Some(sources.end);
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
            child_box_index += 1;
        }
        self.flush_float_run(&mut float_run);
        ChildFlowTraversalOutcome {
            pending_end_margin_collapse,
            collapsed_start_margin_offset,
            rendered_legend,
        }
    }
}

/// Whether an anonymous block exists solely because an inline generated
/// pseudo-element belongs to the HTML root. Such a wrapper shares the
/// document principal flow; block-level root pseudos remain independent
/// formatting contexts.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
fn anonymous_block_wraps_root_principal_pseudo(children: &[box_tree::FormattingBox<'_>]) -> bool {
    children.iter().any(|child| {
        child.element_core().is_some_and(|core| {
            core.element.document_syntax == crate::dom::DocumentSyntax::Html
                && core.element.tag.eq_ignore_ascii_case("html")
                && !core.style.display.is_block_level()
                && matches!(
                    &core.source,
                    box_tree::BoxSource::GeneratedPseudo(pseudo)
                        if matches!(
                            pseudo.kind,
                            box_tree::GeneratedPseudoKind::Before
                                | box_tree::GeneratedPseudoKind::After
                        )
                )
        })
    })
}
