use super::*;
use crate::layout::block::flow::PendingAdjoiningMargin;

/// Replay state for an ordered inline run before a fixed automatic-clamp child.
///
/// The checkpoint survives inline layout so a terminal marker can replay the
/// preceding source range. It is heap-owned because the ordinary ordered
/// mixed-flow path does not use automatic block-size clamping.
/// <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
struct AutomaticInlineReplayCheckpoint {
    snapshot: LayoutSnapshot,
    inline_run_index: usize,
    previous_child_page_end: Option<Option<String>>,
}

/// A short speculative inline layout before a float under line clamping.
///
/// The checkpoint is only needed when the pending inline run might become the
/// terminal clamp line. It stays heap-owned so its LayoutSnapshot never
/// reserves space in the ordered mixed-flow traversal frame.
struct OrderedMixedInlineFloatProbeCheckpoint {
    snapshot: LayoutSnapshot,
    inline_run_index: usize,
    previous_child_page_end: Option<Option<String>>,
}

/// Mutable controller retained while an ordered child enters recursive layout.
///
/// Ordered mixed flow interleaves source indexing, pending inline collection,
/// floats, page boundaries, and margin-collapse state. Keeping that coupled
/// state in one heap-owned controller prevents every recursive ordered child
/// from reserving all of it in its caller's frame.
struct OrderedMixedFlowTraversalState {
    element_index: usize,
    inline_run_index: usize,
    inline_nodes: Vec<(usize, Node)>,
    previous_flow_bottom_margin: Option<PendingAdjoiningMargin>,
    seen_flow_child: bool,
    pending_end_margin_collapse: Option<BlockEndMarginCollapse>,
    float_run: FloatRunState,
    first_formatted_line: FirstFormattedLineState,
    previous_child_page_end: Option<Option<String>>,
    out_of_flow_static_source: Option<StaticPositionRectangle>,
    vertical_child_inline_origin: Option<PageTopBlockPosition>,
}

impl OrderedMixedFlowTraversalState {
    fn new(
        float_run: FloatRunState,
        first_formatted_line: FirstFormattedLineState,
        vertical_child_inline_origin: Option<PageTopBlockPosition>,
    ) -> Self {
        Self {
            element_index: 0,
            inline_run_index: 0,
            inline_nodes: Vec::new(),
            previous_flow_bottom_margin: None,
            seen_flow_child: false,
            pending_end_margin_collapse: None,
            float_run,
            first_formatted_line,
            previous_child_page_end: None,
            out_of_flow_static_source: None,
            vertical_child_inline_origin,
        }
    }

    fn restore_inline_float_probe(
        &mut self,
        checkpoint: Box<OrderedMixedInlineFloatProbeCheckpoint>,
        builder: &mut LayoutBuilder<'_>,
    ) {
        let OrderedMixedInlineFloatProbeCheckpoint {
            snapshot,
            inline_run_index,
            previous_child_page_end,
        } = *checkpoint;
        builder.restore(snapshot);
        self.inline_run_index = inline_run_index;
        self.previous_child_page_end = previous_child_page_end;
    }

    fn restore_automatic_inline_replay(
        &mut self,
        checkpoint: Box<AutomaticInlineReplayCheckpoint>,
        builder: &mut LayoutBuilder<'_>,
    ) {
        let AutomaticInlineReplayCheckpoint {
            snapshot,
            inline_run_index,
            previous_child_page_end,
        } = *checkpoint;
        builder.restore(snapshot);
        self.inline_run_index = inline_run_index;
        self.previous_child_page_end = previous_child_page_end;
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Capture the rollback point for a pending inline run before a float.
    ///
    /// This is deliberately a call boundary: constructing `LayoutSnapshot`
    /// by value here keeps its 11 KiB producer temporary out of the recursive
    /// ordered mixed-flow controller.
    #[inline(never)]
    fn capture_ordered_mixed_inline_float_probe(
        &self,
        inline_run_index: usize,
        previous_child_page_end: Option<Option<String>>,
    ) -> Box<OrderedMixedInlineFloatProbeCheckpoint> {
        Box::new(OrderedMixedInlineFloatProbeCheckpoint {
            snapshot: self.snapshot(),
            inline_run_index,
            previous_child_page_end,
        })
    }

    /// Capture the rollback point for automatic terminal-marker replay.
    ///
    /// The boundary likewise prevents construction of the large checkpoint
    /// payload from contributing to the recursive controller frame.
    #[inline(never)]
    fn capture_automatic_inline_replay_checkpoint(
        &self,
        inline_run_index: usize,
        previous_child_page_end: Option<Option<String>>,
    ) -> Box<AutomaticInlineReplayCheckpoint> {
        Box::new(AutomaticInlineReplayCheckpoint {
            snapshot: self.snapshot(),
            inline_run_index,
            previous_child_page_end,
        })
    }

    #[inline(never)]
    pub(in crate::layout) fn layout_ordered_mixed_flow_children(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        margin_collapse: BlockFlowMarginCollapseContext,
        traversal_state: &mut BlockFlowChildTraversalState,
    ) -> Option<BlockEndMarginCollapse> {
        let BlockFlowMarginCollapseContext {
            can_collapse_start_margin,
            can_collapse_end_margin,
            applied_start_margin,
            starts_at_page_top,
        } = margin_collapse;
        let sibling_tags = element_sibling_signature_list(element);
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
        let text_box_trim_targets = self.ordered_mixed_text_box_trim_targets(
            element,
            style,
            stylesheets,
            &sibling_tags,
            text_box_line_trim,
        );
        // A positioned block is hypothetically placed in this normal-flow
        // source stream. Keep its resolved static rectangle separate from
        // `cursor_y`, which can move while an earlier out-of-flow sibling is
        // laid out.
        // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
        let principal_inline_origin = style.writing_mode.has_vertical_lines().then(|| {
            self.vertical_child_inline_origin(element, style.writing_mode, style.used_direction())
        });
        // Preserve each node's source sibling index. Inline runs are laid out
        // through an isolated formatting context, but selectors such as
        // `:nth-of-type()` still resolve against the original parent.
        let mut state = Box::new(OrderedMixedFlowTraversalState::new(
            self.float_run_state(),
            FirstFormattedLineState::for_style(style),
            principal_inline_origin,
        ));

        for (child_node_index, child) in element.children.iter().enumerate() {
            let NodeKind::Element(child_element) = &child.kind else {
                if !traversal_state.is_exhausted() {
                    state.inline_nodes.push((child_node_index, child.clone()));
                }
                continue;
            };

            let child_signature =
                ElementSignature::from_sibling_snapshot(state.element_index, sibling_tags.clone())
                    .expect("source child must have a cached sibling signature");
            state.element_index += 1;
            let mut child_style = Box::new(self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature.clone(),
                stylesheets,
                Some(style),
            ));
            if child_style.float.layout_role() == FloatLayoutRole::Footnote {
                // A GCPM footnote contributes its generated call to this
                // inline source run. The detached body is installed in the
                // page-level footnote map and must never enter CSS float
                // placement or ordinary block dispatch.
                // <https://www.w3.org/TR/css-gcpm-3/#footnotes>
                state.inline_nodes.push((child_node_index, child.clone()));
                continue;
            }
            if traversal_state.is_exhausted()
                && !traversal_state.admits_zero_height_automatic_child(&child_style)
                && (style_is_in_normal_flow(&child_style) || child_style.float != Float::None)
            {
                // Floats after the clamp boundary are part of discarded
                // source; positioned descendants remain eligible for their
                // independent containing-block layout.
                // <https://drafts.csswg.org/css-overflow-4/#continue>
                continue;
            }
            if child_style.float.layout_role() == FloatLayoutRole::Exclusion {
                let has_later_inline_or_block_source = element.children[child_node_index + 1..]
                    .iter()
                    .any(|node| matches!(&node.kind, NodeKind::Text(text) if !text.trim().is_empty()))
                    || has_later_normal_block_flow_child_with_font_metrics(
                        element,
                        state.element_index,
                        &sibling_tags,
                        style,
                        stylesheets,
                        &self.ancestors,
                        &mut self.font_system,
                    );
                // A float is taken out of normal flow at its source position,
                // but the preceding inline run still forms around that
                // exclusion. Flushing the run first commits a full-width line
                // and wrongly pushes a following right float to the next
                // line. Place the float, then select the pending line against
                // its available band.
                // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                let can_share_pending_inline_line = !state.inline_nodes.is_empty()
                    && !state.inline_nodes.iter().any(|(_, node)| {
                        matches!(&node.kind, NodeKind::Element(element) if is_line_break_element(element))
                    });
                // A line-clamped run can end immediately before this float.
                // Select that terminal source range before committing the
                // float: CSS Overflow's discarded continuation owns neither
                // the float's paint nor its exclusion.  When the run does
                // not exhaust the shared budget, restore the speculative
                // layout and use the normal float-first selection below so
                // the preceding line still sees the float's exclusion.
                // <https://drafts.csswg.org/css-overflow-4/#continue>
                if can_share_pending_inline_line
                    && let Some(remaining_slots) = traversal_state.remaining_line_slots()
                {
                    let checkpoint = self.capture_ordered_mixed_inline_float_probe(
                        state.inline_run_index,
                        state.previous_child_page_end.clone(),
                    );
                    let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                        element,
                        &state.inline_nodes,
                        traversal_state
                            .style_with_remaining_and_continuation(
                                style,
                                BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                                    has_later_inline_or_block_source,
                                ),
                            )
                            .as_ref()
                            .unwrap_or(style),
                        stylesheets,
                        &mut state.inline_run_index,
                        &text_box_trim_targets,
                        text_box_line_trim,
                        state.first_formatted_line.applies_to_next_inline_run(),
                        &mut state.previous_child_page_end,
                    );
                    if inline_outcome.clamp_line_slots >= remaining_slots.visible_line_limit() {
                        if inline_outcome.has_non_phantom_line {
                            state.first_formatted_line.consume_next_formatted_line();
                        }
                        if inline_outcome.has_flow_effects {
                            self.flush_float_run(&mut state.float_run);
                        }
                        traversal_state.debit_inline_outcome(inline_outcome);
                        state.inline_nodes.clear();
                        state.seen_flow_child = true;
                        state.previous_flow_bottom_margin = None;
                        continue;
                    }
                    state.restore_inline_float_probe(checkpoint, self);
                }
                if can_share_pending_inline_line
                    && self.layout_floating_child(
                        child_element,
                        child_signature.clone(),
                        &child_style,
                        None,
                        None,
                        stylesheets,
                        FloatPlacementAxes::for_style(style),
                        &mut state.float_run,
                    )
                {
                    let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                        element,
                        &state.inline_nodes,
                        traversal_state
                            .style_with_remaining_and_continuation(
                                style,
                                BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                                    has_later_inline_or_block_source,
                                ),
                            )
                            .as_ref()
                            .unwrap_or(style),
                        stylesheets,
                        &mut state.inline_run_index,
                        &text_box_trim_targets,
                        text_box_line_trim,
                        state.first_formatted_line.applies_to_next_inline_run(),
                        &mut state.previous_child_page_end,
                    );
                    if inline_outcome.has_non_phantom_line {
                        state.first_formatted_line.consume_next_formatted_line();
                    }
                    if inline_outcome.has_flow_effects {
                        self.flush_float_run(&mut state.float_run);
                    }
                    traversal_state.debit_inline_outcome(inline_outcome);
                    state.inline_nodes.clear();
                    state.seen_flow_child = true;
                    state.previous_flow_bottom_margin = None;
                    continue;
                }
                let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                    element,
                    &state.inline_nodes,
                    traversal_state
                        .style_with_remaining_and_continuation(
                            style,
                            BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                                has_later_inline_or_block_source,
                            ),
                        )
                        .as_ref()
                        .unwrap_or(style),
                    stylesheets,
                    &mut state.inline_run_index,
                    &text_box_trim_targets,
                    text_box_line_trim,
                    state.first_formatted_line.applies_to_next_inline_run(),
                    &mut state.previous_child_page_end,
                );
                if inline_outcome.has_non_phantom_line {
                    state.first_formatted_line.consume_next_formatted_line();
                }
                if inline_outcome.has_flow_effects {
                    state.seen_flow_child = true;
                    state.previous_flow_bottom_margin = None;
                    self.flush_float_run(&mut state.float_run);
                }
                traversal_state.debit_inline_outcome(inline_outcome);
                state.inline_nodes.clear();
                if traversal_state.is_exhausted() {
                    // The pending inline run may have spent the final slot
                    // only when it was flushed at this block boundary. A
                    // float following that source belongs to the discarded
                    // continuation and therefore has no placement.
                    // <https://drafts.csswg.org/css-overflow-4/#continue>
                    continue;
                }
                // No shareable inline line preceded this float (for example,
                // a `<br>` established a forced break), so place it only
                // after flushing that run.  Once placed it is out of normal
                // flow and must not be collected again as inline content.
                // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                if self.layout_floating_child(
                    child_element,
                    child_signature.clone(),
                    &child_style,
                    None,
                    None,
                    stylesheets,
                    FloatPlacementAxes::for_style(style),
                    &mut state.float_run,
                ) {
                    continue;
                }
            }
            // The document canvas is treated as a flow owner for its
            // block-level children, but an HTML `<br>` remains inline content
            // even directly under `body`. Keeping it in the pending inline
            // run preserves its forced-break and `clear` semantics.
            // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
            if is_line_break_element(child_element) {
                state.inline_nodes.push((child_node_index, child.clone()));
                continue;
            }
            if matches!(child_style.position, Position::Absolute | Position::Fixed) {
                // Positioned descendants are out of flow, but their static
                // position is selected at this source-order boundary. Flush
                // the preceding inline run before dispatching the box rather
                // than treating the descendant as inline content (where a
                // blockified source can be dropped by the anonymous-inline
                // collector).
                // <https://www.w3.org/TR/css-position-3/#static-position>
                let has_later_inline_or_block_source = element.children[child_node_index + 1..]
                    .iter()
                    .any(|node| matches!(&node.kind, NodeKind::Text(text) if !text.trim().is_empty()))
                    || has_later_normal_block_flow_child_with_font_metrics(
                        element,
                        state.element_index,
                        &sibling_tags,
                        style,
                        stylesheets,
                        &self.ancestors,
                        &mut self.font_system,
                    );
                let cursor_before_inline = self.cursor_y;
                let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                    element,
                    &state.inline_nodes,
                    traversal_state
                        .style_with_remaining_and_continuation(
                            style,
                            BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                                has_later_inline_or_block_source,
                            ),
                        )
                        .as_ref()
                        .unwrap_or(style),
                    stylesheets,
                    &mut state.inline_run_index,
                    &text_box_trim_targets,
                    text_box_line_trim,
                    state.first_formatted_line.applies_to_next_inline_run(),
                    &mut state.previous_child_page_end,
                );
                if inline_outcome.has_non_phantom_line {
                    state.first_formatted_line.consume_next_formatted_line();
                }
                if inline_outcome.has_flow_effects {
                    state.seen_flow_child = true;
                    state.previous_flow_bottom_margin = None;
                    self.flush_float_run(&mut state.float_run);
                }
                traversal_state.debit_inline_outcome(inline_outcome);
                state.inline_nodes.clear();
                // A block-level abspos at this source boundary gets the
                // static rectangle of its hypothetical in-flow block. The
                // preceding inline run has already consumed its real line
                // boxes; preserve that same measured advance as one
                // non-painting block placeholder instead of anchoring at the
                // end of the preceding line.
                // <https://www.w3.org/TR/css-position-3/#static-position>
                let block_static_y_offset = (child_style.display.is_block_level()
                    && inline_outcome.has_flow_effects)
                    .then(|| (cursor_before_inline - self.cursor_y).max(0.0));
                self.push_ancestor_signature(child_signature);
                self.with_text_box_line_trim_scope(TextBoxLineTrim::default(), |layout| {
                    let previous_static_y_offset = layout.block_static_position_y_offset;
                    layout.block_static_position_y_offset = block_static_y_offset;
                    let rectangle = state.out_of_flow_static_source.unwrap_or_else(|| {
                        layout.block_static_position_rectangle_at(PageTopBlockPosition::new(
                            layout.cursor_y,
                        ))
                    });
                    state.out_of_flow_static_source = Some(rectangle);
                    let previous_absolute_static_position = layout.absolute_static_position;
                    layout.absolute_static_position = Some(
                        layout
                            .absolute_static_position
                            .unwrap_or_else(|| {
                                AbsoluteStaticPosition::from_page_rect(
                                    layout.content_left,
                                    layout.content_right,
                                    rectangle.area.top_y(),
                                )
                            })
                            .with_static_position_rectangle(rectangle),
                    );
                    layout.layout_element(child_element, &child_style, stylesheets);
                    layout.absolute_static_position = previous_absolute_static_position;
                    layout.block_static_position_y_offset = previous_static_y_offset;
                });
                self.ancestors.pop();
                continue;
            }
            // Ordered DOM traversal may be selected for HTML table
            // structure, but the computed outer display alone decides whether
            // this child leaves the pending inline run. In particular,
            // `inline-table` is an atomic inline-level box, not block flow.
            // <https://drafts.csswg.org/css-display-3/#valdef-display-inline-table>
            let is_flow_child = is_normal_block_flow_child(child_element, &child_style);

            if !is_flow_child {
                state.inline_nodes.push((child_node_index, child.clone()));
                continue;
            }

            let block_end_margin_trim = BlockEndMarginTrim::for_child(style, true, || {
                has_later_normal_block_flow_child_with_font_metrics(
                    element,
                    state.element_index,
                    &sibling_tags,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                )
            });
            block_end_margin_trim.apply_to_child(&mut child_style);

            let inherited_page_name = self.active_page_value_scope(style);
            let child_page_values = self.dom_page_boundary_values(
                child_element,
                &child_style,
                stylesheets,
                inherited_page_name.as_deref(),
            );
            // A fixed-height following block can make the preceding inline
            // endpoint the furthest fitting automatic clamp point.  Lay out
            // that inline run speculatively, then restore and replay it with
            // a marker-bearing boundary when the definite following block
            // cannot fit.  The marker is consequently part of line fitting,
            // not a post-layout paint append.
            // <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
            // <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
            let automatic_fixed_child_extent = traversal_state
                .has_automatic_block_size_clamp()
                .then(|| ordered_mixed_definite_child_block_extent(&child_style))
                .flatten();
            let automatic_inline_snapshot = automatic_fixed_child_extent
                .filter(|extent| extent.points() > 0.01)
                .filter(|_| !state.inline_nodes.is_empty())
                .map(|_| {
                    self.capture_automatic_inline_replay_checkpoint(
                        state.inline_run_index,
                        state.previous_child_page_end.clone(),
                    )
                });
            let mut inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                element,
                &state.inline_nodes,
                traversal_state
                    .style_with_remaining_and_continuation(
                        style,
                        css::ClampContinuation::LaterInFlowContent,
                    )
                    .as_ref()
                    .unwrap_or(style),
                stylesheets,
                &mut state.inline_run_index,
                &text_box_trim_targets,
                text_box_line_trim,
                state.first_formatted_line.applies_to_next_inline_run(),
                &mut state.previous_child_page_end,
            );
            let automatic_fixed_child_does_not_fit = automatic_fixed_child_extent
                .zip(traversal_state.automatic_remaining())
                .is_some_and(|(child_extent, remaining)| {
                    inline_outcome.has_non_phantom_line
                        && !inline_outcome.has_local_continuation_cutoff
                        && child_extent.points()
                            > (remaining.points() - inline_outcome.clamp_block_advance.points())
                                .max(0.0)
                                + 0.01
                });
            if automatic_fixed_child_does_not_fit
                && let Some(checkpoint) = automatic_inline_snapshot
                && let Some(boundary_style) =
                    traversal_state.automatic_terminal_boundary_style(style)
            {
                state.restore_automatic_inline_replay(checkpoint, self);
                inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                    element,
                    &state.inline_nodes,
                    &boundary_style,
                    stylesheets,
                    &mut state.inline_run_index,
                    &text_box_trim_targets,
                    text_box_line_trim,
                    state.first_formatted_line.applies_to_next_inline_run(),
                    &mut state.previous_child_page_end,
                );
            }
            if inline_outcome.has_non_phantom_line {
                state.first_formatted_line.consume_next_formatted_line();
            }
            if inline_outcome.has_flow_effects {
                state.seen_flow_child = true;
                state.previous_flow_bottom_margin = None;
                self.flush_float_run(&mut state.float_run);
            }
            traversal_state.debit_inline_outcome(inline_outcome);
            state.inline_nodes.clear();
            let zero_height_automatic_child =
                traversal_state.admits_zero_height_automatic_child(&child_style);
            if traversal_state.is_exhausted() && !zero_height_automatic_child {
                // This child was classified before the preceding inline run
                // was laid out. Once that run exhausts the shared clamp
                // budget, the child and every descendant it would own are
                // outside the continued fragment, including any positioned
                // descendants whose containing block is wholly after the
                // clamp point.
                // <https://drafts.csswg.org/css-overflow-4/#continue>
                continue;
            }
            if state
                .previous_child_page_end
                .as_ref()
                .is_none_or(|previous| previous != &child_page_values.start)
                && (!self.current_page_has_content() || state.previous_child_page_end.is_some())
            {
                self.switch_page_name_at_class_a_boundary(child_page_values.start.as_deref());
            }
            let child_shares_clamp_context =
                self.child_shares_line_clamp_formatting_context(child_element, &child_style);
            if child_shares_clamp_context {
                let has_later_in_flow_child = has_later_normal_block_flow_child_with_font_metrics(
                    element,
                    state.element_index,
                    &sibling_tags,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                ) || element.children[child_node_index + 1..]
                    .iter()
                    .any(|node| matches!(&node.kind, NodeKind::Text(text) if !text.trim().is_empty()));
                if zero_height_automatic_child {
                    let has_later_inline_source = element.children[child_node_index + 1..]
                        .iter()
                        .any(|node| {
                            matches!(&node.kind, NodeKind::Text(text) if !text.trim().is_empty())
                        });
                    traversal_state.apply_zero_height_automatic_boundary(
                        &mut child_style,
                        has_later_in_flow_child || has_later_inline_source,
                    );
                } else {
                    let fixed_child_reaches_boundary = has_later_in_flow_child
                        && ordered_mixed_definite_child_block_extent(&child_style)
                            .zip(traversal_state.automatic_remaining())
                            .is_some_and(|(child_extent, remaining)| {
                                child_extent.points() >= remaining.points() - 0.01
                            });
                    if fixed_child_reaches_boundary {
                        // The block boundary follows this child even when its
                        // final line leaves unused space in a specified-height
                        // box. Select that final same-BFC line as the marker
                        // host before the child is painted.
                        // <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
                        *child_style = traversal_state
                            .automatic_terminal_boundary_style(&child_style)
                            .expect("fixed automatic child requires an active controller");
                    } else {
                        traversal_state.apply_to_with_continuation(
                            &mut child_style,
                            BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                                has_later_in_flow_child,
                            ),
                        );
                        let border = used_border_widths(&child_style);
                        let automatic_child_non_content = crate::units::content_box_pt(
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
                            automatic_child_non_content,
                        );
                        if has_later_in_flow_child {
                            BlockFlowChildTraversalState::require_automatic_terminal_marker_when_full(
                            &mut child_style,
                        );
                        }
                    }
                }
            }

            let collapsible_block_child = is_collapsible_block_child(child_element, &child_style);
            let mut child_ancestors = self.ancestors.clone();
            child_ancestors.push(child_signature.clone());
            let margin_collapse_style = height_behaves_as_auto_for_margin_collapse(
                &child_style,
                self.block_percentage_context_stack
                    .current_percentage_basis(),
            )
            .then(|| {
                let mut style = child_style.clone();
                style
                    .box_values
                    .height
                    .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
                style
            });
            let margin_collapse_style = margin_collapse_style.as_ref().unwrap_or(&child_style);
            let self_collapsing_child = collapsible_block_child
                && !self.has_in_flow_marker_line(child_element, &child_style)
                && is_self_collapsing_block_dom_with_font_metrics(
                    child_element,
                    margin_collapse_style,
                    stylesheets,
                    &child_ancestors,
                    &mut self.font_system,
                    self.document_canvas_overflow,
                );
            let descendant_start_margin = (collapsible_block_child
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
            let self_collapsing_margin_set = self_collapsing_child.then(|| {
                self_collapsing_block_margin_set_for_box(&child_style, descendant_start_margin)
            });
            if let Some(set) = self_collapsing_margin_set {
                child_style.margin.top = set.collapsed().points();
                child_style.margin.bottom = 0.0;
            }
            let mut collapses_with_parent_end = false;
            let mut continued_self_collapsing_margin = None;
            if collapsible_block_child {
                if !state.seen_flow_child && can_collapse_start_margin {
                    if let Some(set) = self_collapsing_margin_set {
                        let (pending, delta) = PendingAdjoiningMargin::from_parent_start_set(
                            applied_start_margin,
                            set,
                            starts_at_page_top,
                        );
                        child_style.margin.top = delta.points();
                        continued_self_collapsing_margin = Some(pending);
                    } else {
                        child_style.margin.top = collapsed_start_margin_delta(
                            applied_start_margin,
                            layout_pt(child_style.margin.top),
                            starts_at_page_top,
                        )
                        .points();
                    }
                } else if let Some(previous_margin) = state.previous_flow_bottom_margin {
                    child_style.margin.top = if let Some(set) = self_collapsing_margin_set {
                        let mut pending = previous_margin;
                        let delta = pending.merge_set(set);
                        continued_self_collapsing_margin = Some(pending);
                        delta.points()
                    } else {
                        let mut pending = previous_margin;
                        pending
                            .merge_margin(layout_pt(child_style.margin.top))
                            .points()
                    };
                }

                collapses_with_parent_end = can_collapse_end_margin
                    && !has_later_normal_block_flow_child_with_font_metrics(
                        element,
                        state.element_index,
                        &sibling_tags,
                        style,
                        stylesheets,
                        &self.ancestors,
                        &mut self.font_system,
                    );
            }

            // The margin-collapse pass above changes used values. Block layout
            // resolves the box values again, so retain those values before
            // delegating to it.
            // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
            preserve_adjusted_block_margins(&mut child_style);

            // A block container whose first in-flow child is block-level
            // takes its first formatted line from that child. Ordered mixed
            // flow can reach this boundary after any number of positioned or
            // floated siblings; those out-of-flow boxes must not consume the
            // originating typographic pseudo before the child descends to its
            // own first line.
            // <https://drafts.csswg.org/css-pseudo-4/#first-text-line>
            if state.first_formatted_line.applies_to_next_inline_run()
                && let Some(style_with_originating_pseudos) =
                    style_with_originating_typographic_pseudos(&child_style, style)
            {
                *child_style = style_with_originating_pseudos;
            }
            state.seen_flow_child = true;
            state.first_formatted_line.consume_next_formatted_line();

            self.flush_float_run(&mut state.float_run);
            self.push_ancestor_signature(child_signature);
            let child_uses_block_layout = matches!(
                element_layout_kind(child_element, &child_style),
                ElementLayoutKind::BlockFlow
            );
            // Ordered mixed-flow traversal is used by the root whenever its
            // source children include both inline runs and block boxes. It
            // therefore needs the same document-canvas vertical projection
            // as the DOM and frozen-box traversals: containment can make the
            // root, rather than the body, the principal-flow source.
            // <https://drafts.csswg.org/css-writing-modes-4/#principal-flow>
            // <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
            // A vertical multicolumn column is itself a finite logical block
            // fragmentainer.  Its direct in-flow children therefore need the
            // same typed inline-origin/block-track handoff as the document
            // principal flow; keeping this page-only made table captions and
            // ordinary vertical blocks advance through unrelated physical-Y
            // state.
            // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
            // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
            let principal_vertical_placement = style
                .writing_mode
                .has_vertical_lines()
                .then(|| {
                    crate::layout::block::flow::children::shared::OrthogonalBlockPlacement::new(
                        style.writing_mode,
                        style.used_direction(),
                        state
                            .vertical_child_inline_origin
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
            if advances_principal_fragmentainers
                && principal_vertical_placement
                    .is_some_and(|placement| placement.block_track_is_exhausted())
                && self.current_page_has_content()
            {
                self.push_page();
            }
            let source_body_canvas = principal_vertical_placement.is_some()
                && self.principal_flow.is_source_body(child_element);
            let bottom_origin_canvas_child = advances_principal_fragmentainers
                && principal_vertical_placement.is_some()
                && inline_start_side(style.writing_mode, style.used_direction())
                    == PhysicalSide::Bottom;
            let canvas_inline_origin = self.cursor_y;
            if bottom_origin_canvas_child {
                self.cursor_y = self.page_top();
            } else if let Some(placement) = principal_vertical_placement {
                self.cursor_y = placement.page_inline_origin().points();
            }
            // An independent formatting context is skipped by `max-lines`,
            // but its normal-flow border-box still consumes block-axis space
            // between automatic-clamp candidates.  Snapshot the parent flow
            // cursor before the child so its own descendant overflow cannot
            // be mistaken for that contribution.
            // <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
            let automatic_child_flow_start = (traversal_state.has_automatic_block_size_clamp()
                && style_is_in_normal_flow(&child_style))
            .then_some(self.cursor_y);
            self.last_block_layout_outcome = BlockLayoutOutcome::default();
            let child_text_box_line_trim = text_box_trim_targets.trim_for(
                OrderedMixedTextBoxTrimTarget::FlowElement(child_node_index),
                text_box_line_trim,
            );
            let previous_direct_block_layout_constraint = self
                .replace_direct_block_layout_constraint(
                    child_element,
                    principal_vertical_placement,
                );
            self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                layout.layout_element(child_element, &child_style, stylesheets);
            });
            self.restore_direct_block_layout_constraint(previous_direct_block_layout_constraint);
            self.ancestors.pop();
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
                } else {
                    PaintTranslation::identity()
                };
                self.current_page
                    .append_paint_fragment_owned(fragment, translation);
                self.cursor_y = if bottom_origin_canvas_child {
                    canvas_inline_origin
                } else {
                    placement.page_inline_origin().points()
                };
                if !source_body_canvas {
                    self.content_left = remaining_block_track.left_x();
                    self.content_right = remaining_block_track.right_x();
                }
            }
            state.out_of_flow_static_source = Some(
                self.block_static_position_rectangle_at(PageTopBlockPosition::new(self.cursor_y)),
            );
            if traversal_state.has_automatic_block_size_clamp()
                && let Some(child_flow_start) = automatic_child_flow_start
            {
                traversal_state.debit_automatic_block_contribution(crate::units::content_box_pt(
                    (child_flow_start - self.cursor_y).max(0.0),
                ));
            }
            if child_uses_block_layout && child_shares_clamp_context {
                traversal_state.record_descendant_clamp_line_slots(
                    self.last_block_layout_outcome.clamp_line_slots,
                );
                traversal_state
                    .debit_rendered_slots(self.last_block_layout_outcome.clamp_line_slots);
                if self.last_block_layout_outcome.has_local_continuation_cutoff {
                    traversal_state.mark_local_continuation_cutoff();
                }
            }
            let child_consumed_bottom_margin = if child_uses_block_layout {
                self.last_block_layout_outcome.consumed_bottom_margin
            } else {
                layout_pt(child_style.margin.bottom)
            };
            if self_collapsing_child && child_uses_block_layout {
                // This traversal has already consumed the child's complete
                // adjoining set at its start edge. A transparent descendant
                // can report the same set as its block-end margin; keep that
                // value for propagation, but remove its duplicate cursor
                // contribution from the zero-height principal box.
                // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
                self.cursor_y += child_consumed_bottom_margin.points();
            }
            let next_pending_margin = if self_collapsing_child {
                Some(continued_self_collapsing_margin.unwrap_or_else(|| {
                    PendingAdjoiningMargin::from_consumed_set(
                        self_collapsing_margin_set
                            .expect("a self-collapsing child has a complete adjoining set"),
                    )
                }))
            } else {
                collapsible_block_child.then(|| {
                    PendingAdjoiningMargin::from_consumed_margin(child_consumed_bottom_margin)
                })
            };
            if collapses_with_parent_end {
                let collapsed_margin = next_pending_margin
                    .map(|pending| pending.collapsed_with_margin(layout_pt(style.margin.bottom)))
                    .unwrap_or_else(|| {
                        collapse_margins(
                            child_consumed_bottom_margin,
                            layout_pt(style.margin.bottom),
                        )
                    });
                state.pending_end_margin_collapse = Some(BlockEndMarginCollapse {
                    child_consumed_margin: child_consumed_bottom_margin,
                    collapsed_margin,
                });
            }
            state.previous_flow_bottom_margin = next_pending_margin;
            state.previous_child_page_end = Some(child_page_values.end);
        }

        let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
            element,
            &state.inline_nodes,
            traversal_state
                .style_with_remaining(style)
                .as_ref()
                .unwrap_or(style),
            stylesheets,
            &mut state.inline_run_index,
            &text_box_trim_targets,
            text_box_line_trim,
            state.first_formatted_line.applies_to_next_inline_run(),
            &mut state.previous_child_page_end,
        );
        if inline_outcome.has_non_phantom_line {
            state.first_formatted_line.consume_next_formatted_line();
        }
        if inline_outcome.has_flow_effects {
            state.previous_flow_bottom_margin = None;
            self.flush_float_run(&mut state.float_run);
        }
        traversal_state.debit_inline_outcome(inline_outcome);
        self.flush_float_run(&mut state.float_run);

        let _ = state.previous_flow_bottom_margin;
        state.pending_end_margin_collapse
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_ordered_mixed_inline_fragment_block(
        &mut self,
        parent: &Element,
        inline_nodes: &[(usize, Node)],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inline_run_index: &mut usize,
        text_box_trim_targets: &OrderedMixedTextBoxTrimTargets,
        text_box_line_trim: TextBoxLineTrim,
        allow_typographic_first_line: bool,
        previous_child_page_end: &mut Option<Option<String>>,
    ) -> InlineLayoutOutcome {
        // Normal white-space-only DOM runs do not create a line box. In
        // particular, an indentation text node before a block child must not
        // consume a fragmentainer row before that child is placed.
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-1>
        if inline_nodes.iter().all(|(_, node)| {
            matches!(&node.kind, NodeKind::Text(text) if normalize_inline_text(text).is_empty())
        }) {
            return InlineLayoutOutcome::default();
        }
        let inline_page_value = self.active_page_value_scope(style);
        if previous_child_page_end
            .as_ref()
            .is_some_and(|previous| previous != &inline_page_value)
        {
            self.switch_page_name_at_class_a_boundary(inline_page_value.as_deref());
        }
        let is_text_box_trim_candidate = ordered_mixed_inline_nodes_accept_text_box_trim(
            &inline_nodes
                .iter()
                .map(|(_, node)| node.clone())
                .collect::<Vec<_>>(),
            style,
        );
        let run_text_box_line_trim = if is_text_box_trim_candidate {
            text_box_trim_targets.trim_for(
                OrderedMixedTextBoxTrimTarget::InlineRun(*inline_run_index),
                text_box_line_trim,
            )
        } else {
            TextBoxLineTrim::default()
        };
        let captured = self.with_text_box_line_trim_scope(run_text_box_line_trim, |layout| {
            layout.begin_clamp_line_slot_capture();
            layout.layout_inline_fragment_block_with_first_line_policy(
                inline_nodes,
                parent,
                style,
                stylesheets,
                allow_typographic_first_line,
            );
            layout.finish_clamp_line_slot_capture()
        });
        if is_text_box_trim_candidate {
            *inline_run_index += 1;
        }
        self.record_clamp_line_slots(captured.line_slots);
        let outcome = InlineLayoutOutcome {
            next_line_index: captured.line_slots,
            clamp_line_slots: captured.line_slots,
            clamp_block_advance: captured.block_advance,
            has_non_phantom_line: captured.line_slots > 0,
            has_flow_effects: captured.line_slots > 0,
            has_local_continuation_cutoff: captured.has_local_continuation_cutoff,
        };
        if outcome.has_flow_effects {
            *previous_child_page_end = Some(inline_page_value);
        }
        outcome
    }

    fn ordered_mixed_text_box_trim_targets(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        sibling_tags: &ElementSiblingSignatureList,
        trim: TextBoxLineTrim,
    ) -> OrderedMixedTextBoxTrimTargets {
        let mut targets = OrderedMixedTextBoxTrimTargets::default();
        if trim.is_empty() {
            return targets;
        }

        let mut element_index = 0usize;
        let mut inline_run_index = 0usize;
        let mut inline_nodes = Vec::new();
        let mut candidates = Vec::new();

        for (child_node_index, child) in element.children.iter().enumerate() {
            let NodeKind::Element(child_element) = &child.kind else {
                inline_nodes.push(child.clone());
                continue;
            };

            let child_signature =
                ElementSignature::from_sibling_snapshot(element_index, sibling_tags.clone())
                    .expect("source child must have a cached sibling signature");
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature,
                stylesheets,
                Some(style),
            );
            if child_style.float != Float::None {
                if ordered_mixed_inline_nodes_accept_text_box_trim(&inline_nodes, style) {
                    candidates.push(OrderedMixedTextBoxTrimCandidate {
                        target: OrderedMixedTextBoxTrimTarget::InlineRun(inline_run_index),
                        accepts_block_start: true,
                        accepts_block_end: true,
                    });
                    inline_run_index += 1;
                }
                inline_nodes.clear();
                continue;
            }

            let is_flow_child = is_normal_block_flow_child(child_element, &child_style);
            if !is_flow_child {
                inline_nodes.push(child.clone());
                continue;
            }

            if ordered_mixed_inline_nodes_accept_text_box_trim(&inline_nodes, style) {
                candidates.push(OrderedMixedTextBoxTrimCandidate {
                    target: OrderedMixedTextBoxTrimTarget::InlineRun(inline_run_index),
                    accepts_block_start: true,
                    accepts_block_end: true,
                });
                inline_run_index += 1;
            }
            inline_nodes.clear();

            candidates.push(OrderedMixedTextBoxTrimCandidate {
                target: OrderedMixedTextBoxTrimTarget::FlowElement(child_node_index),
                accepts_block_start: ordered_mixed_element_accepts_text_box_trim(
                    child_element,
                    &child_style,
                    true,
                ),
                accepts_block_end: ordered_mixed_element_accepts_text_box_trim(
                    child_element,
                    &child_style,
                    false,
                ),
            });
        }

        if ordered_mixed_inline_nodes_accept_text_box_trim(&inline_nodes, style) {
            candidates.push(OrderedMixedTextBoxTrimCandidate {
                target: OrderedMixedTextBoxTrimTarget::InlineRun(inline_run_index),
                accepts_block_start: true,
                accepts_block_end: true,
            });
        }

        if trim.trims_block_start {
            targets.block_start = candidates
                .first()
                .and_then(|candidate| candidate.accepts_block_start.then_some(candidate.target));
        }
        if trim.trims_block_end {
            targets.block_end = candidates
                .iter()
                .next_back()
                .and_then(|candidate| candidate.accepts_block_end.then_some(candidate.target));
        }
        targets
    }
}
