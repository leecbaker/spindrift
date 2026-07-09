use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn inline_fragment_atom_for_children(
        &mut self,
        element: Option<&Element>,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> InlineAtom {
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(0.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_width)),
        );
        let style = &used_style;
        let borders = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content_length().points();
        let vertical_extras = box_metrics.vertical_non_content_length().points();
        let containing_block_height = self
            .definite_block_size_stack
            .last()
            .cloned()
            .unwrap_or_else(PercentageBasis::indefinite);
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
        self.definite_block_size_stack
            .push(block_size_percentage_basis_from_points(
                definite_content_height,
                BlockSizeBasisSource::InlineBlock,
            ));
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
        // Size containment makes the principal box's intrinsic content sizes
        // zero, but descendants are still laid out (and may visibly overflow)
        // inside the resulting zero-sized content box.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        let (preferred_min, preferred) = if style.contain.size {
            (0.0, 0.0)
        } else {
            let preferred_min = contribution
                .min_content
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
                .max(block_flow_root_contribution.1)
                .max(inline_float_preferred)
                .max(preferred_min);
            (preferred_min, preferred)
        };
        self.definite_block_size_stack.pop();
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
            intrinsic::content_box_width_from_intrinsic(
                style,
                layout_pt(available_width),
                non_content_pt(horizontal_extras),
                content_box_pt(preferred_min),
                content_box_pt(preferred),
                intrinsic::IntrinsicAutoWidth::ShrinkToFit,
            )
        });
        let content_width = constrain_content_width(
            style,
            requested_content_width,
            PercentageBasis::definite(layout_pt(available_width.max(0.0))),
        )
        .points();

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let top = 10_000.0;
        let content_left = borders.left + style.padding.left;
        let content_top = top - borders.top - style.padding.top;
        self.current_page = Page::new(content_width + horizontal_extras, top);
        self.content_left = content_left;
        self.content_right = content_left + content_width;
        self.cursor_y = content_top;
        // Atomic inline boxes establish an independent formatting context, so
        // their descendants never pass through the normal block-flow scroll
        // capture boundary. Keep the static scroll scope here instead.
        // CSS Scroll Snap § 4 defines the scroll container over this box's
        // padding-box scrollport.
        let static_scroll_snap_scope = self.begin_static_scroll_snap_scope(style, false);
        self.last_in_flow_line_baseline_y = None;
        self.truncate_page_start_margins = false;
        let positioning_containing_block_mode = PositionedContainingBlockMode::for_style(style);
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
        self.child_available_space_stack
            .push(ChildAvailableSpace::new(
                style.writing_mode,
                PhysicalContentWidth::new(content_box_pt(content_width)),
                definite_content_height
                    .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                PhysicalContentHeight::new(content_box_pt(self.page_area_height())),
            ));
        self.definite_block_size_stack
            .push(block_size_percentage_basis_from_points(
                definite_content_height,
                BlockSizeBasisSource::InlineBlock,
            ));
        let previous_block_static_position_y_offset = self.block_static_position_y_offset;
        let previous_absolute_static_position = self.absolute_static_position;
        let previous_escaped_atom_containing_block = self.escaped_atom_containing_block;
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
        self.absolute_static_position = Some(AbsoluteStaticPosition::from_page_rect(
            content_left,
            content_left + content_width,
            content_top,
        ));
        self.escaped_atom_positioning_depth += 1;
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
        self.with_text_box_line_trim_scope(text_box_line_trim, |layout| {
            let laid_out_multicol = element.is_some_and(|element| {
                (style.column_count.is_some()
                    || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
                    || matches!(style.column_height, css::ComputedColumnHeight::Length(_)))
                    && (definite_content_height.is_none()
                        || style.column_fill != css::ColumnFill::Auto
                        || formatting_boxes_contain_column_spanner(children))
                    && layout.layout_simple_block_child_columns(
                        element,
                        style,
                        stylesheets,
                        Some(children),
                        definite_content_height,
                    )
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
                layout.layout_flow_root_child_boxes(children, stylesheets);
            }
        });
        if has_auto_height(style)
            && let Some(float_bottom) = self.current_float_context_lowest_bottom()
        {
            self.cursor_y = self.cursor_y.min(float_bottom);
        }
        self.escaped_atom_positioning_depth -= 1;
        self.block_static_position_y_offset = previous_block_static_position_y_offset;
        self.absolute_static_position = previous_absolute_static_position;
        self.definite_block_size_stack.pop();
        self.child_available_space_stack.pop();
        self.content_logical_inline_size_stack.pop();
        self.pop_float_context();
        self.pop_page_name_scope_suppression();
        if let Some(scope) = positioned_containing_block_scope {
            self.escaped_atom_containing_block = previous_escaped_atom_containing_block;
            self.pop_positioned_containing_block(scope);
        }
        // Empty inline-blocks and zero-height block children do not synthesize
        // a line box. Any real inline line or block child has already advanced
        // the temporary formatting-context cursor by its used block size.
        // <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>
        let measured_content_height = (content_top - self.cursor_y).max(0.0);
        // CSS Sizing applies explicit `height` to the content box of the
        // atomic inline-block fragment; internal line/block contents may
        // overflow but do not increase the used height:
        // <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
        let content_height = definite_content_height.unwrap_or_else(|| {
            constrain_content_height(
                style,
                content_box_pt(if style.contain.size {
                    0.0
                } else {
                    measured_content_height
                }),
                PercentageBasis::definite(layout_pt(available_width)),
            )
            .points()
        });
        let border_box_height = content_height + vertical_extras;
        let border_box = PageTopRect::new(
            0.0,
            top,
            content_width + horizontal_extras,
            border_box_height,
        )
        .paint_clip();
        let border_bottom = border_box.y();
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
            policy.effects.overflow_clip = None;
            policy.effects.rounded_overflow_clip = None;
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
        if static_scroll_offset.x != 0.0 || static_scroll_offset.y != 0.0 {
            fragment = fragment.translated(crate::layout::scroll_snap::static_scroll_translation(
                static_scroll_offset,
                style,
            ));
        }
        // Atomic inline fragments are normalized from their temporary page
        // into atom-local coordinates before inline painting replays them.
        // The overflow effect must be created in that same coordinate space;
        // otherwise its clip remains at the temporary page's 10,000pt origin.
        let local_scroll_padding_box =
            scroll_padding_box.translate(PaintTranslation::new(0.0, -border_bottom).into());
        let mut fragment = fragment.translated(PaintTranslation::new(0.0, -border_bottom));
        if static_scroll_snap_scope {
            // Descendant block backgrounds occupy the captured fragment's
            // background band until atomic-inline paint is replayed. Promote
            // them to their normal-flow slot before applying the contents
            // clip; the atom's own decoration is emitted after restoration.
            fragment.promote_background_border_to_in_flow_block();
            let overflow_clip = PaintClip::from_paint_rect(local_scroll_padding_box);
            fragment = fragment
                .with_primitives_clipped_to_rect_preserving_structure(overflow_clip)
                .with_effect_scoped_to_rect_all_bands(overflow_clip);
        }
        let escaped_positioned_layers = escaped_positioned_layers
            .into_iter()
            .map(|layer| {
                let escape_offset = layer.escaped_atom_translation.escape_offset(-border_bottom);
                layer.translated(escape_offset)
            })
            .collect::<Vec<_>>();
        let escaped_positioned_layers = (!escaped_positioned_layers.is_empty())
            .then(|| escaped_positioned_layers.into_boxed_slice());
        let line_baseline_offset = self
            .last_in_flow_line_baseline_y
            .map(|baseline_y| (top - baseline_y).max(0.0));
        let baseline_offset =
            Self::inline_block_baseline_offset(style, border_box_height, line_baseline_offset);
        self.restore(snapshot);

        InlineAtom::new(
            InlineAtomContent::InlineFragment(Box::new(fragment)),
            style.clone(),
            escaped_positioned_layers,
            InlineSize::new(
                content_width + horizontal_extras + style.margin.left + style.margin.right,
                border_box_height + style.margin.top + style.margin.bottom,
            ),
            baseline_offset,
            baseline_shift,
            link_target,
            None,
        )
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
        border_box_height: f32,
        line_baseline_offset: Option<f32>,
    ) -> f32 {
        // Layout containment suppresses baseline export. For vertical
        // alignment the atomic inline therefore uses its synthesized
        // bottom-margin-edge baseline even when descendants produced lines.
        // <https://www.w3.org/TR/css-contain-1/#containment-layout>
        if style.contain.layout {
            // CSS Containment suppresses the exported baseline entirely. Its
            // inline/flex/grid parent then synthesizes one from the principal
            // border box's block-end edge; margins participate in outer
            // spacing, not in the contained box's exported baseline.
            return border_box_height;
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
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
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
                    &mut float_run,
                )
            {
                continue;
            }
            self.flush_float_run(&mut float_run);
            let prior_line_baseline = self.last_in_flow_line_baseline_y;
            self.layout_formatting_box(child, stylesheets);
            if child.element_parts().is_some_and(|(_, _, style, _)| {
                style.contain.layout
                    && !matches!(style.position, Position::Absolute | Position::Fixed)
                    && style.float == Float::None
            }) {
                // A layout-contained block cannot replace the surrounding
                // atomic flow root's last eligible line baseline with one
                // from its descendants. Preserve the baseline established by
                // the preceding in-flow line instead.
                // <https://www.w3.org/TR/css-contain-1/#containment-layout>
                self.last_in_flow_line_baseline_y = prior_line_baseline;
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
        self.inline_text_box_metrics(style, None, 0.0)
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
