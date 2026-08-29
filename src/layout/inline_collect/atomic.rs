use super::*;
use crate::layout::block::{
    DefinitePhysicalContentHeight, child_available_space_for_formatting_context,
};
use crate::layout::builder::page_for_context;
use crate::units::content_box_to_margin_box_length;

/// Fully resolved physical margin-box geometry for an atomic inline.
///
/// The inline layout graph replays an atom only through [`InlineSize`], but
/// layout must first resolve its CSS margin box: font-relative margins and
/// padding belong to the atomic participant's advance, not to a later paint
/// adjustment. Keeping that conversion here makes the legacy scalar-size
/// boundary explicit.
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>
#[derive(Debug, Clone, Copy)]
struct ResolvedAtomicInlineMarginBox {
    physical_size: MarginBoxSize,
}

impl ResolvedAtomicInlineMarginBox {
    fn from_resolved_boxes(
        content: ContentBoxSize,
        horizontal_non_content: NonContentLength,
        vertical_non_content: NonContentLength,
        horizontal_margins: LayoutLength,
        vertical_margins: LayoutLength,
    ) -> Self {
        let width = content_box_to_margin_box_length(
            content_box_pt(content.width),
            horizontal_non_content,
            horizontal_margins,
        );
        let height = content_box_to_margin_box_length(
            content_box_pt(content.height),
            vertical_non_content,
            vertical_margins,
        );
        Self {
            physical_size: margin_box_size_pt(width.points(), height.points()),
        }
    }

    /// Convert to the legacy graph-size representation at the inline-layout
    /// boundary. No caller may reconstruct this from independent margin and
    /// border scalars after this point.
    fn into_inline_layout_size(self) -> InlineSize {
        InlineSize::new(self.physical_size.width, self.physical_size.height)
    }

    fn horizontal_span(self) -> MarginBoxLength {
        margin_box_pt(self.physical_size.width)
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn inline_fragment_atom_for_children(
        &mut self,
        element: Option<&Element>,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> InlineAtom {
        // Keep the frozen source for escaped positioned-paint identity; all
        // sizing and paint below use the separate one-way used style.
        let source_style = style.clone();
        let mut used_style = self.style_with_current_used_lengths(style);
        let box_metrics = apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            self.current_content_logical_inline_percentage_basis(),
        );
        let style = &used_style;
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(0.0);
        let containment = element.map(|element| used_property_containment(element, style));
        let borders = box_metrics.border.to_css_edges();
        let horizontal_extras = box_metrics.horizontal_non_content_length().points();
        let vertical_extras = box_metrics.vertical_non_content_length().points();
        let containing_block_height = self
            .block_percentage_context_stack
            .current_percentage_basis();
        let definite_content_height = used_content_box_height_or_auto_with_basis(
            style,
            containing_block_height,
            non_content_pt(vertical_extras),
        )
        .map(|height| {
            constrain_content_height(
                style,
                height,
                PercentageBasis::definite(layout_pt(available_width)),
            )
            .points()
        });
        self.block_percentage_context_stack.push_context(
            DescendantBlockPercentageContext::formatting_context(
                definite_content_height.map(content_box_pt),
                BlockSizeBasisSource::InlineBlock,
            ),
        );
        // An inline-block establishes a new block formatting context. Its
        // intrinsic child layout must therefore not observe float exclusions
        // from the parent line, while floats created by its own descendants
        // remain visible within this temporary context.
        // <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>
        // <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>
        self.push_float_context();
        // Intrinsic sizing may recursively collect this inline formatting
        // context in an off-page scratch coordinate space. Out-of-flow
        // descendants contribute no intrinsic inline size, and materializing
        // them here would retain a positioned layer at that scratch origin
        // instead of during the committed inline-block layout below.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic> and
        // <https://www.w3.org/TR/css-position-3/#absolute-positioning>
        self.positioned_inline_layout_suppression_depth += 1;
        let contribution =
            self.intrinsic_inline_contribution_for_boxes(children, style, stylesheets);
        let block_flow_root_contribution = element
            .filter(|_| has_non_inline_formatting_box(children))
            .map(|element| {
                self.block_intrinsic_content_widths(
                    element,
                    style,
                    stylesheets,
                    Some(children),
                    available_width,
                )
            })
            .unwrap_or((0.0, 0.0));
        let (inline_float_preferred_min, inline_float_preferred) = self
            .inline_float_run_intrinsic_widths_for_boxes(
                children,
                style,
                stylesheets,
                available_width,
            );
        self.positioned_inline_layout_suppression_depth -= 1;
        self.pop_float_context();
        // Size containment makes the principal box's intrinsic content sizes
        // zero, but descendants are still laid out (and may visibly overflow)
        // inside the resulting zero-sized content box.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        let (preferred_min, preferred) = if containment.is_some_and(|effects| effects.size) {
            (0.0, 0.0)
        } else {
            let preferred_min = contribution
                .min_content
                .points()
                .max(block_flow_root_contribution.0)
                .max(inline_float_preferred_min);
            // The collected inline graph is the authoritative source for
            // both min-content and max-content widths. Recursively measuring
            // an inline descendant in isolation loses its selected line-edge
            // effects (for example, a trailing Unicode space separator that
            // hangs through an inline box) and can incorrectly override the
            // graph result.
            // <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes> and
            // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
            let preferred = contribution
                .max_content
                .points()
                .max(block_flow_root_contribution.1)
                .max(inline_float_preferred)
                .max(preferred_min);
            (preferred_min, preferred)
        };
        self.block_percentage_context_stack.pop();
        // This is final inline-block layout, not an intrinsic contribution:
        // a percentage `width` resolves against the definite inline size of
        // the containing block. Only `auto` falls back to shrink-to-fit.
        // <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>
        let requested_content_width = used_content_box_width_or_auto(
            style,
            layout_pt(available_width),
            non_content_pt(horizontal_extras),
        )
        .unwrap_or_else(|| {
            crate::layout::intrinsic::content_box_width_from_intrinsic(
                style,
                layout_pt(available_width),
                non_content_pt(horizontal_extras),
                content_box_pt(preferred_min),
                content_box_pt(preferred),
                crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
            )
        });
        let content_width = constrain_content_width(
            style,
            requested_content_width,
            PercentageBasis::definite(layout_pt(available_width.max(0.0))),
        )
        .points();

        // Retain the real absolute-positioning fallback before replacing the
        // current page with this atomic inline's temporary formatting page.
        // The static-position rectangle itself is constructed below in that
        // temporary coordinate space.
        let escaped_atom_actual_containing_block = self.current_containing_block();
        // Intrinsic inline-block layout runs in a temporary page coordinate
        // space. Its internal lines export the atom baseline, but are not
        // principal list-item lines and therefore cannot consume an outside
        // marker anchor.
        let pending_outside_marker_anchors = self.pending_outside_marker_anchors.suspend();
        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let top = 10_000.0;
        let content_left = borders.left + style.padding.left;
        let content_top = top - borders.top - style.padding.top;
        // The temporary page and its page context form one coordinate space.
        // Keeping the outer page context here would let page-area clips
        // created while laying out an escaped positioned descendant retain
        // outer coordinates while its primitives retain this scratch origin.
        // That detaches the clip when the atom is replayed at its final line
        // position.
        // <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks> and
        // <https://www.w3.org/TR/CSS22/zindex.html>.
        let atom_page_context = PageContext {
            size: PageSize::from_points(content_width + horizontal_extras, top),
            margins: PageMargins::all_points(0.0),
            edges: PageBoxEdges::ZERO,
            rotation: snapshot.current_page_context().rotation,
        };
        self.current_page = page_for_context(atom_page_context);
        self.overflow_clips.clear();
        self.fragment_top_offsets.clear();
        self.content_left = content_left;
        self.content_right = content_left + content_width;
        self.cursor_y = content_top;
        // This independent formatting context starts its own hypothetical
        // line. Retaining the enclosing line's static-position rectangle
        // makes an escaped absolute descendant resolve from that outer line
        // and then receive the atom replay translation a second time.
        // <https://www.w3.org/TR/css-position-3/#static-position>
        self.inline_static_position = None;
        // A query container exposes its content box to descendants.
        // Scrollbar gutters define the scrollport used for clipping and
        // scrolling, but do not change the content-box percentage basis from
        // which container-relative lengths are resolved.
        // <https://drafts.csswg.org/css-conditional-5/#container-lengths>
        let container_unit_scope = self.push_container_unit_context(
            style,
            PhysicalContentWidth::new(content_box_pt(content_width)),
            PhysicalContentHeight::new(content_box_pt(definite_content_height.unwrap_or(0.0))),
        );
        // Atomic inline boxes establish an independent formatting context, so
        // their descendants never pass through the normal block-flow scroll
        // capture boundary. Keep the static scroll scope here instead.
        // CSS Scroll Snap § 4 defines the scroll container over this box's
        // padding-box scrollport.
        let static_scroll_snap_scope = self.begin_static_scroll_snap_scope(style, false);
        self.last_in_flow_line_baseline_y = None;
        self.truncate_page_start_margins = false;
        let positioning_containing_block_mode = element
            .and_then(|element| PositionedContainingBlockMode::for_element(element, style))
            .or_else(|| {
                element
                    .is_none()
                    .then(|| PositionedContainingBlockMode::for_style(style))
                    .flatten()
            });
        let previous_escaped_atom_containing_block = self.escaped_atom_containing_block;
        let positioned_containing_block_scope =
            if let Some(mode) = positioning_containing_block_mode {
                // CSS Positioned Layout uses the padding box of a positioned
                // or transformed ancestor as the containing block for absolute
                // descendants. This inline-block fragment is laid out in a
                // temporary page whose border-box origin is (0, top).
                let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
                    borders.left,
                    top - borders.top,
                    content_width + style.padding.left + style.padding.right,
                    definite_content_height
                        .unwrap_or(style.line_height)
                        .max(0.0)
                        + style.padding.top
                        + style.padding.bottom,
                ));
                let scope = self.push_positioned_containing_block(mode, containing_block);
                self.escaped_atom_containing_block = Some(containing_block);
                Some(scope)
            } else {
                None
            };
        self.push_page_name_scope_suppression();
        self.push_float_context();
        self.content_logical_inline_size_stack.push(content_width);
        let inherited_orthogonal_available_height = self
            .current_child_available_space()
            .orthogonal_available_height;
        self.child_available_space_stack
            .push(child_available_space_for_formatting_context(
                style,
                PhysicalContentWidth::new(content_box_pt(content_width)),
                definite_content_height.map(|height| {
                    DefinitePhysicalContentHeight::new(PhysicalContentHeight::new(content_box_pt(
                        height,
                    )))
                }),
                inherited_orthogonal_available_height,
                self.initial_containing_block_physical_height(),
            ));
        self.block_percentage_context_stack.push_context(
            DescendantBlockPercentageContext::formatting_context(
                definite_content_height.map(content_box_pt),
                BlockSizeBasisSource::InlineBlock,
            ),
        );
        let previous_escaped_atom_positioning_context = self.escaped_atom_positioning_context;
        let previous_block_static_position_y_offset = self.block_static_position_y_offset;
        let previous_absolute_static_position = self.absolute_static_position;
        // The inline-block fragment is laid out on a temporary page, while an
        // absolutely positioned descendant's containing block can still be an
        // ancestor outside that temporary coordinate space. For auto vertical
        // insets, CSS 2.2 uses the hypothetical normal-flow static position;
        // preserve the temporary-flow y instead of clamping it to the outer
        // containing block top.
        // <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height>.
        self.block_static_position_y_offset = Some(0.0);
        // Escaped positioned descendants are stored in atom-local coordinates
        // and translated to the real inline-block line position during paint.
        // Their auto static position must therefore use the temporary
        // formatting context's content-box origin, including cases where the
        // containing block remains an ancestor outside this inline-block.
        let escaped_atom_static_position = AbsoluteStaticPosition::from_page_rect(
            content_left,
            content_left + content_width,
            content_top,
        );
        self.absolute_static_position = Some(escaped_atom_static_position);
        self.escaped_atom_positioning_context = Some(EscapedAtomPositioningContext {
            actual_containing_block: escaped_atom_actual_containing_block,
            static_position: escaped_atom_static_position,
        });
        self.escaped_atom_positioning_depth += 1;
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
        let mut multicol_outcome = None;
        let mut multicol_outcome_includes_control_label_flow = false;
        self.with_text_box_line_trim_scope(text_box_line_trim, |layout| {
            let laid_out_multicol = element.is_some_and(|element| {
                // An atomic inline keeps its inline outer display for the
                // parent line, but its contents establish their own block
                // formatting context.  The multicol planner consumes that
                // *inner* flow; passing the inline-level principal style made
                // direct spanners take a different sizing and positioning
                // path from an equivalent block wrapper.
                // <https://drafts.csswg.org/css-display-3/#blockify>
                let mut multicol_flow_style = style.clone();
                multicol_flow_style.display = multicol_flow_style.display.blockified();
                let mut multicol_flow_source_style = source_style.clone();
                multicol_flow_source_style.display =
                    multicol_flow_source_style.display.blockified();
                let eligible_multicol = (matches!(style.column_count, css::ColumnCount::Count(_))
                    || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
                    || matches!(style.column_height, css::ComputedColumnHeight::Length(_)))
                    && (definite_content_height.is_none()
                        || style.column_fill != css::ColumnFill::Auto
                        || formatting_boxes_contain_column_spanner(children));
                let outcome = eligible_multicol.then(|| {
                    // HTML buttons have an anonymous control-label flow. When
                    // block descendants force that flow to become mixed, the
                    // leading and trailing preserved inline runs remain label
                    // siblings of the internal block flow, rather than being
                    // inserted into its multicolumn content. This is the
                    // same structure as the WPT's explicit inner wrapper.
                    // <https://html.spec.whatwg.org/multipage/rendering.html#the-button-element>
                    let is_button = element.tag.eq_ignore_ascii_case("button");
                    let normalized_children = is_button.then(|| {
                        layout.build_frozen_child_boxes_with_current_ancestors(
                            element,
                            stylesheets,
                            &multicol_flow_source_style,
                        )
                    });
                    let Some(normalized_children) = normalized_children else {
                        return layout.layout_simple_block_child_columns(
                            element,
                            &multicol_flow_style,
                            stylesheets,
                            // The atomic outer display kept this tree from the
                            // block-container normalization pass. Rebuild the
                            // frozen children with the blockified inner style.
                            None,
                            definite_content_height,
                        );
                    };
                    let leading_inline_run =
                        normalized_children.first().and_then(|child| match child {
                            box_tree::FormattingBox::AnonymousBlock(anonymous)
                                if formatting_box_has_inline_content(&anonymous.children) =>
                            {
                                Some((
                                    std::rc::Rc::clone(&anonymous.style),
                                    anonymous.children.clone(),
                                ))
                            }
                            _ => None,
                        });
                    let trailing_inline_run =
                        normalized_children.last().and_then(|child| match child {
                            box_tree::FormattingBox::AnonymousBlock(anonymous)
                                if formatting_box_has_inline_content(&anonymous.children) =>
                            {
                                Some((
                                    std::rc::Rc::clone(&anonymous.style),
                                    anonymous.children.clone(),
                                ))
                            }
                            _ => None,
                        });
                    let first_flow_child = usize::from(leading_inline_run.is_some());
                    let last_flow_child = normalized_children
                        .len()
                        .saturating_sub(usize::from(trailing_inline_run.is_some()));
                    if first_flow_child >= last_flow_child {
                        return layout.layout_simple_block_child_columns(
                            element,
                            &multicol_flow_style,
                            stylesheets,
                            Some(&normalized_children),
                            definite_content_height,
                        );
                    }
                    if let Some((inline_style, inline_children)) = &leading_inline_run {
                        layout.layout_anonymous_block(
                            inline_style,
                            inline_children,
                            stylesheets,
                            None,
                        );
                    }
                    let mut control_flow_children = Vec::with_capacity(
                        last_flow_child - first_flow_child
                            + usize::from(leading_inline_run.is_some())
                            + usize::from(trailing_inline_run.is_some()),
                    );
                    // The control's anonymous internal block owns a fresh
                    // pair of inline-flow edges. Preserve the author-facing
                    // label runs above while also retaining their equivalent
                    // wrapper-edge line in the internal multicol flow.
                    if let Some((inline_style, inline_children)) = &leading_inline_run {
                        control_flow_children.push(box_tree::FormattingBox::AnonymousBlock(
                            box_tree::AnonymousBlockBoxWith {
                                style: std::rc::Rc::clone(inline_style),
                                children: inline_children.clone(),
                            },
                        ));
                    }
                    control_flow_children
                        .extend_from_slice(&normalized_children[first_flow_child..last_flow_child]);
                    if let Some((inline_style, inline_children)) = &trailing_inline_run {
                        control_flow_children.push(box_tree::FormattingBox::AnonymousBlock(
                            box_tree::AnonymousBlockBoxWith {
                                style: std::rc::Rc::clone(inline_style),
                                children: inline_children.clone(),
                            },
                        ));
                    }
                    let outcome = layout.layout_simple_block_child_columns(
                        element,
                        &multicol_flow_style,
                        stylesheets,
                        Some(&control_flow_children),
                        definite_content_height,
                    );
                    if let Some((inline_style, inline_children)) = &trailing_inline_run {
                        layout.layout_anonymous_block(
                            inline_style,
                            inline_children,
                            stylesheets,
                            None,
                        );
                    }
                    multicol_outcome_includes_control_label_flow = true;
                    outcome
                });
                multicol_outcome = outcome.filter(|outcome| outcome.is_multicol_layout());
                multicol_outcome.is_some()
            });
            if laid_out_multicol {
                // The shared planner has consumed the atomic flow root's
                // children, including spanners and anonymous column paint.
            } else if !has_non_inline_formatting_box(children)
                && formatting_box_has_inline_content(children)
            {
                // CSS 2.2 lays out inline-block contents as a separate formatting
                // context. When that context contains inline-level children, they
                // must form inline line boxes rather than being replayed as
                // independent blocks:
                // <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>.
                let _ = layout.layout_anonymous_block(style, children, stylesheets, None);
            } else {
                layout.layout_flow_root_child_boxes(style, children, stylesheets);
            }
        });
        if has_auto_height(style)
            && let Some(float_bottom) = self.current_float_context_lowest_bottom()
        {
            self.cursor_y = self.cursor_y.min(float_bottom.points());
        }
        self.escaped_atom_positioning_depth -= 1;
        self.block_static_position_y_offset = previous_block_static_position_y_offset;
        self.absolute_static_position = previous_absolute_static_position;
        self.escaped_atom_positioning_context = previous_escaped_atom_positioning_context;
        self.block_percentage_context_stack.pop();
        self.child_available_space_stack.pop();
        self.content_logical_inline_size_stack.pop();
        self.pop_float_context();
        self.pop_page_name_scope_suppression();
        // Empty inline-blocks and zero-height block children do not synthesize
        // a line box. Any real inline line or block child has already advanced
        // the temporary formatting-context cursor by its used block size.
        // <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>
        // A later orthogonal block advances its own physical block axis and
        // may leave this temporary formatting context's top-to-bottom cursor
        // unchanged. The atomic inline's physical height must still retain a
        // preceding compatible line box; otherwise normal inline paint is
        // translated above the zero-height atom during replay.
        // <https://drafts.csswg.org/css-align-3/#baseline-export>
        let compatible_line_height = self.last_in_flow_line_baseline_y.map(|baseline| {
            let baseline_from_content_top = (content_top - baseline).max(0.0);
            let baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
            baseline_from_content_top + (style.line_height - baseline_offset).max(0.0)
        });
        let measured_content_height = multicol_outcome
            .map(|outcome| {
                if multicol_outcome_includes_control_label_flow {
                    (content_top - self.cursor_y).max(outcome.committed_block_extent().points())
                } else {
                    outcome.committed_block_extent().points()
                }
            })
            .unwrap_or_else(|| {
                (content_top - self.cursor_y)
                    .max(0.0)
                    .max(compatible_line_height.unwrap_or(0.0))
            });
        // CSS Sizing applies explicit `height` to the content box of the
        // atomic inline-block fragment; internal line/block contents may
        // overflow but do not increase the used height:
        // <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
        let content_height = definite_content_height.unwrap_or_else(|| {
            constrain_content_height(
                style,
                content_box_pt(if containment.is_some_and(|effects| effects.size) {
                    0.0
                } else {
                    measured_content_height
                }),
                PercentageBasis::definite(layout_pt(available_width)),
            )
            .points()
        });
        if let Some(scope) = positioned_containing_block_scope {
            // The atomic root is now auto-sized from the committed multicol
            // flow.  Finalize the padding-box containing block before its
            // captured positioned descendants leave this formatting context.
            let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
                borders.left,
                top - borders.top,
                content_width + style.padding.left + style.padding.right,
                content_height.max(0.0) + style.padding.top + style.padding.bottom,
            ));
            self.finalize_positioned_containing_block(scope, containing_block);
            self.escaped_atom_containing_block = Some(containing_block);
            self.pop_positioned_containing_block(scope);
            self.escaped_atom_containing_block = previous_escaped_atom_containing_block;
        }
        let border_box_height = content_height + vertical_extras;
        let border_box = PageTopRect::new(
            0.0,
            top,
            content_width + horizontal_extras,
            border_box_height,
        )
        .paint_clip();
        let scratch_border_box_origin = PaintPoint::new(border_box.x(), border_box.y());
        let scroll_padding_box = paint_space_rect(
            borders.left,
            border_box.y() + borders.bottom,
            (border_box.width() - borders.left - borders.right).max(0.0),
            (border_box.height() - borders.top - borders.bottom).max(0.0),
        );
        let scroll_content_bounds = self
            .current_page
            .paint_fragment()
            .bounds()
            .map(PaintClip::paint_rect)
            .unwrap_or(scroll_padding_box);
        let static_scroll_offset = self.finish_static_scroll_snap_scope(
            static_scroll_snap_scope,
            scroll_padding_box,
            scroll_content_bounds,
        );
        let mut policy = StackingContextPolicy::for_atomic(style, PaintBand::Inline, border_box);
        // The atom's own background and border are painted by its replaying
        // inline context. Its overflow clip belongs only to the captured
        // descendants below, not to that decoration.
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
        if static_scroll_snap_scope {
            policy.effects.clear_overflow_clip_effects();
        }
        let escaped_positioned_layers =
            if matches!(policy.child_layer_policy, ChildLayerPolicy::EscapeAll)
                && positioned_layer_start < self.positioned_layers.len()
            {
                // CSS 2.2 Appendix E treats inline-blocks as atomically
                // painted inline-level pseudo stacking contexts, but
                // positioned descendants still participate in the parent
                // stacking context rather than being captured by that pseudo
                // context:
                // <https://www.w3.org/TR/CSS22/zindex.html>.
                self.positioned_layers.split_off(positioned_layer_start)
            } else {
                Vec::new()
            };
        self.flush_positioned_layers_since(positioned_layer_start);
        let mut fragment = self.current_page.paint_fragment();
        let escaped_normal_flow_contexts = fragment.take_positioned_stacking_contexts();
        if static_scroll_offset.x != 0.0 || static_scroll_offset.y != 0.0 {
            fragment = fragment.translated(crate::layout::scroll_snap::static_scroll_translation(
                static_scroll_offset,
                style,
            ));
        }
        // Keep raw scratch paint and its border-box-to-border-box bridge
        // together. Outer margins are already resolved by the parent line.
        let replay_coordinates =
            AtomicInlineCaptureFrame::for_scratch_border_box(scratch_border_box_origin)
                .replay_coordinates();
        if static_scroll_snap_scope {
            let overflow_clip = PaintClip::from_paint_rect(scroll_padding_box);
            // The atomic principal's border and background are replayed as
            // its own decoration, not scrolling contents. Scope only the
            // captured descendant bands so an overflow clip cannot trim the
            // scroll container's blue border.
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
            fragment = fragment.with_contents_effect_scoped_to_rect(overflow_clip);
        }
        let escaped_positioned_layers =
            escaped_positioned_layers
                .into_iter()
                .chain(escaped_normal_flow_contexts.into_iter().map(|context| {
                    PositionedPaintLayer {
                        page_index: self.pages.len(),
                        transaction_depth: self.positioned_paint_transaction_depth,
                        source_element: None,
                        source_style_identity: &source_style as *const ComputedStyle as usize,
                        source_style: source_style.clone(),
                        multicol_fragment_index: None,
                        source_is_target: false,
                        stack_level: context.stack_level,
                        context,
                        links: Vec::new(),
                        escaped_atom_translation: EscapedAtomTranslation::normal_flow_fragment(),
                    }
                }))
                .map(|layer| {
                    let escape_offset = layer
                        .escaped_atom_translation
                        .escape_offset(-scratch_border_box_origin.y);
                    layer.translated(escape_offset)
                })
                .collect::<Vec<_>>();
        let escaped_positioned_layers = (!escaped_positioned_layers.is_empty())
            .then(|| escaped_positioned_layers.into_boxed_slice());
        // Layout containment suppresses all descendant baseline sources, not
        // only multicolumn's committed baseline. In particular, the captured
        // atomic flow's last line must not be reintroduced through the
        // `last_in_flow_line_baseline_y` fallback.
        // <https://www.w3.org/TR/css-contain-1/#containment-layout>
        let line_baseline_offset = (!containment.is_some_and(|effects| effects.layout))
            .then(|| {
                multicol_outcome
                    .and_then(|outcome| outcome.final_in_flow_baseline())
                    .map(|baseline| borders.top + style.padding.top + baseline.points())
                    .or_else(|| {
                        self.last_in_flow_line_baseline_y
                            .map(|baseline_y| (top - baseline_y).max(0.0))
                    })
            })
            .flatten();
        let baseline_offset = Self::inline_block_baseline_offset_with_containment(
            style,
            containment.is_some_and(|effects| effects.layout),
            border_box_height,
            line_baseline_offset,
        );
        self.pop_container_unit_context(container_unit_scope);
        self.restore(snapshot);
        self.pending_outside_marker_anchors
            .restore(pending_outside_marker_anchors);

        let resolved_margin_box = ResolvedAtomicInlineMarginBox::from_resolved_boxes(
            content_box_size_pt(content_width, content_height),
            box_metrics.horizontal_non_content_length(),
            box_metrics.vertical_non_content_length(),
            layout_pt(style.margin.left + style.margin.right),
            layout_pt(style.margin.top + style.margin.bottom),
        );
        let atom = InlineAtom::new(
            InlineAtomContent::InlineFragment {
                fragment: Box::new(fragment),
                replay_coordinates,
                table_cell_context: None,
                contents_overflow_clip_applied: static_scroll_snap_scope,
            },
            style.clone(),
            escaped_positioned_layers,
            resolved_margin_box.into_inline_layout_size(),
            baseline_offset,
            baseline_shift,
            link_target,
            None,
        );
        debug_assert!(
            (atom.size.width - resolved_margin_box.horizontal_span().points()).abs() <= 0.01,
            "atomic inline replay advance must equal its resolved horizontal margin-box span"
        );
        atom
    }

    /// Return an inline-block baseline offset from its border-box top.
    ///
    /// CSS 2.2 defines an `inline-block` baseline as the baseline of its last
    /// in-flow line box only when `overflow` computes to `visible`; if there
    /// is no in-flow line box or `overflow` is non-visible, the baseline is
    /// the bottom margin edge:
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    pub(in crate::layout) fn inline_block_baseline_offset(
        style: &ComputedStyle,
        has_layout_containment: bool,
        border_box_height: f32,
        line_baseline_offset: Option<f32>,
    ) -> f32 {
        // Layout containment suppresses baseline export. For vertical
        // alignment the atomic inline therefore uses its synthesized
        // bottom-margin-edge baseline even when descendants produced lines.
        // <https://www.w3.org/TR/css-contain-1/#containment-layout>
        Self::inline_block_baseline_offset_with_containment(
            style,
            has_layout_containment,
            border_box_height,
            line_baseline_offset,
        )
    }

    pub(in crate::layout) fn inline_block_baseline_offset_with_containment(
        style: &ComputedStyle,
        has_layout_containment: bool,
        border_box_height: f32,
        line_baseline_offset: Option<f32>,
    ) -> f32 {
        if has_layout_containment {
            // CSS Containment suppresses a descendant-provided baseline. The
            // containing line then uses the same no-baseline fallback as an
            // empty inline-block: the bottom margin edge.
            // <https://www.w3.org/TR/css-inline-3/#inline-block-baseline>
            // <https://www.w3.org/TR/css-contain-1/#containment-layout>
            return border_box_height + style.margin.bottom;
        }
        if effective_overflow_for_style(style) == css::Overflow::Visible
            && let Some(line_baseline_offset) = line_baseline_offset
        {
            return line_baseline_offset.max(0.0);
        }
        border_box_height + style.margin.bottom
    }

    /// Lays out normalized children inside an atomic flow-root fragment.
    ///
    /// CSS 2.2 defines `inline-block` as an inline-level box whose contents
    /// establish a block formatting context, and floats are positioned in that
    /// current block formatting context instead of being replayed as ordinary
    /// block children:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) fn layout_flow_root_child_boxes(
        &mut self,
        containing_style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
    ) {
        let mut float_run = self.float_run_state();
        for child in children {
            if let Some((child_element, child_signature, child_style, child_children)) =
                child.element_parts()
                && self.layout_floating_child(
                    child_element,
                    child_signature.clone(),
                    child_style,
                    Some(child_children),
                    None,
                    stylesheets,
                    FloatPlacementAxes::for_style(containing_style),
                    &mut float_run,
                )
            {
                continue;
            }
            self.flush_float_run(&mut float_run);
            let prior_line_baseline = self.last_in_flow_line_baseline_y;
            self.layout_formatting_box(child, stylesheets);
            if let Some((child_element, _, child_style, _)) = child.element_parts() {
                let child_cannot_export_compatible_baseline =
                    layout_containment_applies_to_element(child_element, child_style)
                        || crate::layout::block::writing_modes_are_orthogonal(
                            containing_style.writing_mode,
                            child_style.writing_mode,
                        );
                if child_cannot_export_compatible_baseline
                    && !matches!(child_style.position, Position::Absolute | Position::Fixed)
                    && child_style.float == Float::None
                {
                    // A layout-contained or orthogonal block cannot replace
                    // the surrounding atomic flow root's last compatible line
                    // baseline. CSS Align exports baseline sets only across
                    // matching writing-mode axes.
                    // <https://www.w3.org/TR/css-contain-1/#containment-layout>
                    // <https://drafts.csswg.org/css-align-3/#baseline-export>
                    self.last_in_flow_line_baseline_y = prior_line_baseline;
                }
            }
        }
        self.flush_float_run(&mut float_run);
    }

    /// Return an inline-block text fast-path baseline from its last line box.
    ///
    /// CSS 2.2 uses the last in-flow line box only when the inline-block's
    /// `overflow` computes to `visible`; callers pass this line baseline to
    /// [`Self::inline_block_baseline_offset`] so non-visible overflow and
    /// missing-line fallback use the bottom margin edge:
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    pub(in crate::layout) fn inline_box_sequence_baseline_offset(
        &mut self,
        sequence: &inline_layout::InlineLineSequence,
        style: &ComputedStyle,
        borders: css::Edges,
    ) -> Option<f32> {
        if !sequence.has_non_phantom_line() {
            return None;
        }
        let fallback = self.inline_box_text_line_layout_baseline_offset(style);
        Some(borders.top + style.padding.top + sequence.last_line_baseline_offset(fallback))
    }

    /// Return a text line's CSS layout baseline offset from its line-box top.
    ///
    /// Inline layout aligns atomic inline boxes against the same baseline
    /// coordinate used for ordinary text fragments. PDF text emission applies
    /// a backend-specific rendered-baseline projection later, but that
    /// projection must not affect CSS line-box sizing:
    /// <https://www.w3.org/TR/css-inline-3/#line-box>.
    pub(in crate::layout) fn inline_box_text_line_layout_baseline_offset(
        &mut self,
        style: &ComputedStyle,
    ) -> f32 {
        self.inline_text_box_metrics(style, 0.0)
            .line_baseline_offset
    }
}

/// Return whether an atomic multicol flow root contains a potential spanner.
///
/// Block-in-inline normalization can wrap the atomic root's direct children in
/// a transparent split context carrying the root's own multicol style. The
/// committed planner still decides actual eligibility; this walk only decides
/// whether the atomic flow root must enter that planner at all.
fn formatting_boxes_contain_column_spanner(boxes: &[box_tree::FormattingBox<'_>]) -> bool {
    boxes.iter().any(|box_| {
        box_.element_parts().is_some_and(|(_, _, style, _)| {
            style.column_span == css::ColumnSpan::All
                && style_is_in_normal_flow(style)
                && style.float == Float::None
        }) || formatting_boxes_contain_column_spanner(box_.children())
    })
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn layout_containment_uses_the_bottom_margin_edge_when_descendant_baseline_exists() {
        let mut style = ComputedStyle::initial();
        style.margin.bottom = 2.0;

        assert_eq!(
            LayoutBuilder::inline_block_baseline_offset(&style, true, 20.0, Some(8.0)),
            22.0,
        );
        assert_eq!(
            LayoutBuilder::inline_block_baseline_offset(&style, false, 20.0, None),
            22.0,
        );
    }
}

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn inline_atom_for_element(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &Stylesheets<'_>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        link_target: Option<String>,
    ) -> Option<InlineAtom> {
        if let Content::Replacement {
            image: GeneratedContentPart::Image { image },
            ..
        } = &style.content
        {
            let alt_text = self.generated_alt_text(element, style);
            return image.as_image().and_then(|image| {
                self.generated_image_atom_for_image(
                    image,
                    style,
                    false,
                    baseline_shift,
                    visual_offset,
                    link_target,
                    alt_text,
                )
            });
        }
        match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => {
                let available_width = (self.content_right - self.content_left).max(1.0);
                let mut style = self.style_with_current_viewport_lengths(style);
                let metrics = apply_used_box_metrics_for_logical_inline_basis(
                    &mut style,
                    self.current_content_logical_inline_percentage_basis(),
                );
                let containing_block_height = self
                    .block_percentage_context_stack
                    .current_percentage_basis();
                let canvas = resolve_replaced_element(
                    element,
                    &style,
                    ReplacedBoxSizingContext {
                        available_width: content_box_pt(available_width),
                        inline_percentage_basis: PercentageBasis::definite_from(
                            content_box_pt(available_width),
                            IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
                        ),
                        block_basis: IntrinsicBlockBasis::from_layout_percentage_basis(
                            containing_block_height,
                        ),
                    },
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                )?
                .geometry();
                let content = if element.tag.eq_ignore_ascii_case("iframe") {
                    self.resource_cache.record_iframe_viewport(
                        element.id,
                        IframeEmbeddingContext {
                            viewport: PageSize::from_points(
                                canvas.content_size.width,
                                canvas.content_size.height,
                            ),
                            effective_zoom: style.effective_zoom,
                        },
                    );
                    InlineAtomContent::Iframe(element.id)
                } else {
                    InlineAtomContent::Canvas
                };
                let border_box_width = canvas.border_box_size.width;
                let border_box_height = canvas.border_box_size.height;
                let atom_width =
                    border_box_width + metrics.margin.left.points() + metrics.margin.right.points();
                Some(
                    InlineAtom::new(
                        content,
                        style.clone(),
                        None,
                        InlineSize::new(
                            atom_width,
                            border_box_height
                                + metrics.margin.top.points()
                                + metrics.margin.bottom.points(),
                        ),
                        border_box_height,
                        baseline_shift,
                        link_target,
                        None,
                    )
                    .with_visual_offset(visual_offset),
                )
            }
            Some(ReplacedElementKind::Image) => {
                // Table intrinsic probes collect direct replaced boxes through
                // this atom constructor. A no-source image with a cyclic
                // percentage width has no intrinsic inline contribution;
                // final layout will revisit it with the committed cell width.
                // <https://html.spec.whatwg.org/multipage/images.html#the-img-element>
                // <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
                if element.tag == "img"
                    && crate::dom::selected_img_source(element).is_none()
                    && self
                        .intrinsic_inline_percentage_basis_stack
                        .last()
                        .is_some_and(|basis| !basis.is_definite())
                {
                    return None;
                }
                let available_width = (self.content_right - self.content_left).max(1.0);
                let mut used_style = self.style_with_current_viewport_lengths(style);
                apply_used_box_metrics_for_logical_inline_basis(
                    &mut used_style,
                    self.current_content_logical_inline_percentage_basis(),
                );
                let style = &used_style;
                let image = resolve_replaced_element(
                    element,
                    style,
                    ReplacedBoxSizingContext {
                        available_width: content_box_pt(available_width),
                        inline_percentage_basis: PercentageBasis::definite_from(
                            content_box_pt(available_width),
                            IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
                        ),
                        block_basis: IntrinsicBlockBasis::from_layout_percentage_basis(
                            self.block_percentage_context_stack
                                .current_percentage_basis(),
                        ),
                    },
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                )?
                .into_image()?;
                let border_box_width = image.border_box_size.width;
                let border_box_height = image.border_box_size.height;
                let content = image
                    .svg
                    .map(|asset| InlineAtomContent::Svg { asset: Some(asset) })
                    .unwrap_or(InlineAtomContent::Image(image.decoded));
                Some(
                    InlineAtom::new(
                        content,
                        style.clone(),
                        None,
                        InlineSize::new(
                            border_box_width + style.margin.left + style.margin.right,
                            border_box_height + style.margin.top + style.margin.bottom,
                        ),
                        border_box_height,
                        baseline_shift,
                        link_target,
                        element.attrs.get("alt").cloned(),
                    )
                    .with_visual_offset(visual_offset),
                )
            }
            Some(ReplacedElementKind::Svg) => {
                let asset = self.resource_cache.inline_svg_asset(element)?;
                let available_width = (self.content_right - self.content_left).max(1.0);
                let mut style = self.style_with_current_viewport_lengths(style);
                let metrics = apply_used_box_metrics_for_logical_inline_basis(
                    &mut style,
                    self.current_content_logical_inline_percentage_basis(),
                );
                let svg = resolve_replaced_element(
                    element,
                    &style,
                    ReplacedBoxSizingContext {
                        available_width: content_box_pt(available_width),
                        inline_percentage_basis: PercentageBasis::definite_from(
                            content_box_pt(available_width),
                            IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
                        ),
                        block_basis: IntrinsicBlockBasis::from_layout_percentage_basis(
                            self.block_percentage_context_stack
                                .current_percentage_basis(),
                        ),
                    },
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                )?
                .geometry();
                let width = svg.border_box_size.width;
                let height = svg.border_box_size.height;
                Some(
                    InlineAtom::new(
                        InlineAtomContent::Svg { asset: Some(asset) },
                        style.clone(),
                        None,
                        InlineSize::new(
                            width + metrics.margin.left.points() + metrics.margin.right.points(),
                            height + metrics.margin.top.points() + metrics.margin.bottom.points(),
                        ),
                        height,
                        baseline_shift,
                        link_target,
                        None,
                    )
                    .with_visual_offset(visual_offset),
                )
            }
            None if style.display.is_table() => self
                .inline_table_atom_for_element(
                    element,
                    style,
                    children,
                    table_fragment?,
                    stylesheets,
                    baseline_shift,
                    link_target,
                )
                .map(|atom| atom.with_visual_offset(visual_offset)),
            None if style.display.is_flex() && style.display.is_inline_level() => Some(
                self.inline_flex_atom_for_element(
                    element,
                    signature,
                    style,
                    children,
                    stylesheets,
                    baseline_shift,
                    link_target,
                )
                .with_visual_offset(visual_offset),
            ),
            None if style.display.is_grid() && style.display.is_inline_level() => Some(
                self.inline_grid_atom_for_element(
                    element,
                    style,
                    children,
                    stylesheets,
                    baseline_shift,
                    link_target,
                )
                .with_visual_offset(visual_offset),
            ),
            None if style.display.is_atomic_inline() => {
                if has_non_inline_formatting_box(children)
                    || has_atomic_inline_formatting_box(children)
                    || has_inline_container_formatting_box(children)
                    || has_out_of_flow_formatting_box(children)
                {
                    return Some(
                        self.inline_fragment_atom_for_children(
                            Some(element),
                            style,
                            children,
                            stylesheets,
                            baseline_shift,
                            link_target,
                        )
                        .with_visual_offset(visual_offset),
                    );
                }
                let available_width = (self.content_right
                    - self.content_left
                    - style.margin.left
                    - style.margin.right)
                    .max(0.0);
                let mut used_style = self.style_with_current_used_lengths(style);
                let box_metrics = apply_used_box_metrics_for_logical_inline_basis(
                    &mut used_style,
                    self.current_content_logical_inline_percentage_basis(),
                );
                let style = &used_style;
                let border_widths = box_metrics.border.to_css_edges();
                let horizontal_extras = box_metrics.horizontal_non_content_length().points();
                let vertical_extras = box_metrics.vertical_non_content_length().points();
                let containing_block_height = self
                    .block_percentage_context_stack
                    .current_percentage_basis();
                let definite_content_height = used_content_box_height_or_auto_with_basis(
                    style,
                    containing_block_height,
                    non_content_pt(vertical_extras),
                )
                .map(|height| {
                    constrain_content_height(
                        style,
                        height,
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .points()
                });
                self.block_percentage_context_stack.push_context(
                    DescendantBlockPercentageContext::formatting_context(
                        definite_content_height.map(content_box_pt),
                        BlockSizeBasisSource::InlineBlock,
                    ),
                );
                // An inline-block establishes a new block formatting context.
                // Its intrinsic child layout and final inline line sequence
                // cannot observe float exclusions from the parent line.
                // <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>
                // <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>
                self.push_float_context();
                let intrinsic = self.intrinsic_inline_measurement_for_element(
                    element,
                    style,
                    stylesheets,
                    Some(children),
                    available_width,
                );
                // This is the used size of an atomic inline box. Resolve a
                // specified percentage against the definite line containing
                // block; reserve intrinsic shrink-to-fit for `auto` only.
                // <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>
                let requested_content_width = used_content_box_width_or_auto(
                    style,
                    layout_pt(available_width),
                    non_content_pt(horizontal_extras),
                )
                .unwrap_or_else(|| {
                    crate::layout::intrinsic::content_box_width_from_intrinsic(
                        style,
                        layout_pt(available_width),
                        non_content_pt(horizontal_extras),
                        intrinsic.contribution.min_content.content_box_length(),
                        intrinsic.contribution.max_content.content_box_length(),
                        crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                    )
                });
                let mut content_width = constrain_content_width(
                    style,
                    requested_content_width,
                    PercentageBasis::definite(layout_pt(available_width.max(0.0))),
                )
                .points();
                let mut sequence_items = Vec::new();
                let mut outside_marker = None;
                if style.display.is_list_item()
                    && let Some(marker) =
                        self.marker_for_list_item(element, style, self.containing_block_direction)
                {
                    if marker.participates_in_first_line() {
                        self.push_inside_marker_items(
                            &marker,
                            style,
                            link_target.clone(),
                            &mut sequence_items,
                        );
                    } else {
                        outside_marker = Some(marker);
                    }
                }
                self.push_generated_pseudo_items(
                    element,
                    style,
                    style.before_style.as_deref(),
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    GeneratedPseudoCounterMode::Commit,
                    &mut sequence_items,
                );
                if style.content.is_generated() {
                    self.push_element_content_items_from_boxes(
                        element,
                        style,
                        box_tree::CounterEventSource::Principal,
                        children,
                        stylesheets,
                        link_target.clone(),
                        0.0,
                        InlineVisualOffset::zero(),
                        style,
                        style.text_decoration_origins.effective_layers_vec(),
                        &mut sequence_items,
                    );
                } else {
                    self.collect_inline_box_items(
                        children,
                        stylesheets,
                        link_target.clone(),
                        0.0,
                        InlineVisualOffset::zero(),
                        style,
                        style.text_decoration_origins.effective_layers_vec(),
                        &mut sequence_items,
                    );
                }
                self.push_generated_pseudo_items(
                    element,
                    style,
                    style.after_style.as_deref(),
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    GeneratedPseudoCounterMode::Commit,
                    &mut sequence_items,
                );
                self.block_percentage_context_stack.pop();
                let vertical_writing_mode = style.writing_mode.has_vertical_lines();
                // A physical `width` is the logical block size in vertical
                // writing. Select the line's logical inline measure from
                // `height` (or its shrink-to-fit intrinsic contribution),
                // then derive the physical width from the resulting wrapped
                // logical block contribution.
                // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
                let logical_inline_measure = if vertical_writing_mode {
                    definite_content_height.unwrap_or_else(|| {
                        crate::layout::intrinsic::content_box_width_from_intrinsic(
                            style,
                            layout_pt(available_width),
                            non_content_pt(horizontal_extras),
                            intrinsic.contribution.min_content.content_box_length(),
                            intrinsic.contribution.max_content.content_box_length(),
                            crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                        )
                        .points()
                    })
                } else {
                    content_width
                };
                let sequence = self.collect_inline_line_sequence_with_text_box_trim(
                    sequence_items,
                    style,
                    logical_inline_measure,
                    0.0,
                    0.0,
                );
                self.pop_float_context();
                let measured_logical_block_size = if vertical_writing_mode {
                    sequence
                        .records
                        .iter()
                        .map(|record| record.block_before + record.height().max(style.line_height))
                        .sum::<f32>()
                        .max(0.0)
                } else {
                    sequence.total_height().max(0.0)
                };
                if vertical_writing_mode && style.box_values.width.is_auto() {
                    content_width = constrain_content_width(
                        style,
                        content_box_pt(measured_logical_block_size),
                        PercentageBasis::definite(layout_pt(available_width.max(0.0))),
                    )
                    .points();
                }
                // CSS Sizing applies `height` to the content box; line-height
                // can overflow explicit-height inline-blocks but must not
                // increase their used height:
                // <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
                let content_height = if vertical_writing_mode {
                    definite_content_height.unwrap_or(logical_inline_measure)
                } else {
                    definite_content_height.unwrap_or_else(|| {
                        constrain_content_height(
                            style,
                            content_box_pt(if used_property_containment(element, style).size {
                                0.0
                            } else {
                                measured_logical_block_size
                            }),
                            PercentageBasis::definite(layout_pt(available_width)),
                        )
                        .points()
                    })
                };
                let border_box_height = content_height + vertical_extras;
                let line_baseline_offset =
                    self.inline_box_sequence_baseline_offset(&sequence, style, border_widths);
                let baseline_offset = Self::inline_block_baseline_offset(
                    style,
                    used_property_containment(element, style).layout,
                    border_box_height,
                    line_baseline_offset,
                );
                let mut atom = InlineAtom::new(
                    InlineAtomContent::InlineBox { sequence },
                    style.clone(),
                    None,
                    InlineSize::new(
                        content_width + horizontal_extras + style.margin.left + style.margin.right,
                        border_box_height + style.margin.top + style.margin.bottom,
                    ),
                    baseline_offset,
                    baseline_shift,
                    link_target,
                    None,
                );
                // An empty inline-block has no content-derived baseline.
                // Its synthesized alphabetic baseline is therefore the
                // line-under margin edge, projected by the containing inline
                // context's logical block axis.
                // <https://drafts.csswg.org/css-inline-3/#synthesize-baselines>
                if line_baseline_offset.is_none() {
                    atom = atom.with_synthesized_margin_box_block_end_baseline();
                }
                Some(
                    atom.with_outside_marker(outside_marker)
                        .with_visual_offset(visual_offset),
                )
            }
            None => None,
        }
    }
}
