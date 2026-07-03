use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn inline_fragment_atom_for_children(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> InlineAtom {
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
        let style = &used_style;
        let borders = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content();
        let vertical_extras = box_metrics.vertical_non_content();
        let containing_block_height = self.definite_block_size_stack.last().copied().flatten();
        let definite_content_height = used_content_height_or_auto_with_optional_basis(
            style,
            containing_block_height,
            vertical_extras,
        )
        .map(|height| constrain_height(style, height, available_width));
        self.definite_block_size_stack.push(definite_content_height);
        let contribution =
            self.intrinsic_inline_contribution_for_boxes(children, style, stylesheets);
        let (inline_float_preferred_min, inline_float_preferred) = self
            .inline_float_run_intrinsic_widths_for_boxes(
                children,
                style,
                stylesheets,
                available_width,
            );
        let preferred_min = contribution
            .min_content
            .max(inline_float_preferred_min)
            .max(style.font_size);
        let preferred = self
            .inline_boxes_max_content_width(children, stylesheets, available_width)
            .max(contribution.max_content)
            .max(inline_float_preferred)
            .max(preferred_min)
            .max(style.font_size);
        self.definite_block_size_stack.pop();
        let requested_content_width = intrinsic::content_width_from_intrinsic(
            style,
            available_width,
            horizontal_extras,
            preferred_min,
            preferred,
            intrinsic::IntrinsicAutoWidth::ShrinkToFit,
        );
        let content_width =
            constrain_width(style, requested_content_width, available_width).max(style.font_size);

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let top = 10_000.0;
        let content_left = borders.left + style.padding.left;
        let content_top = top - borders.top - style.padding.top;
        self.current_page = Page::new(content_width + horizontal_extras, top);
        self.content_left = content_left;
        self.content_right = content_left + content_width;
        self.cursor_y = content_top;
        self.last_in_flow_line_baseline_y = None;
        self.truncate_page_start_margins = false;
        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        if establishes_positioning_containing_block {
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
            self.containing_blocks.push(containing_block);
            self.escaped_atom_containing_block = Some(containing_block);
        }
        self.push_page_name_scope_suppression();
        self.push_float_context();
        self.content_logical_inline_size_stack.push(content_width);
        self.child_available_space_stack
            .push(ChildAvailableSpace::new(
                style.writing_mode,
                content_width,
                definite_content_height,
                self.page_area_height(),
            ));
        self.definite_block_size_stack.push(definite_content_height);
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
            if !has_non_inline_formatting_box(children)
                && formatting_box_has_inline_content(children)
            {
                // CSS 2.2 lays out inline-block contents as a separate formatting
                // context. When that context contains inline-level children, they
                // must form inline line boxes rather than being replayed as
                // independent blocks:
                // <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>.
                layout.layout_anonymous_block(style, children, stylesheets, None);
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
        if establishes_positioning_containing_block {
            self.escaped_atom_containing_block = previous_escaped_atom_containing_block;
            self.containing_blocks.pop();
        }
        let measured_content_height = (content_top - self.cursor_y).max(style.line_height);
        // CSS Sizing applies explicit `height` to the content box of the
        // atomic inline-block fragment; internal line/block contents may
        // overflow but do not increase the used height:
        // <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
        let content_height = definite_content_height
            .unwrap_or_else(|| constrain_height(style, measured_content_height, available_width));
        let border_box_height = content_height + vertical_extras;
        let border_box = PageTopRect::new(
            0.0,
            top,
            content_width + horizontal_extras,
            border_box_height,
        )
        .paint_clip();
        let border_bottom = border_box.y();
        let policy = StackingContextPolicy::for_atomic(style, PaintBand::Inline, border_box);
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
        let fragment = self
            .current_page
            .paint_fragment()
            .translated(PaintVector::new(0.0, -border_bottom));
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
            .map(|baseline_y| (top - baseline_y).max(0.0))
            .or_else(|| {
                fragment
                    .last_line_y()
                    .map(|line_y| (border_box_height - line_y).max(0.0))
            });
        let baseline_offset =
            Self::inline_block_baseline_offset(style, border_box_height, line_baseline_offset);
        self.restore(snapshot);

        InlineAtom::new(
            InlineAtomContent::InlineFragment(fragment),
            style.clone(),
            escaped_positioned_layers,
            content_width + horizontal_extras + style.margin.left + style.margin.right,
            border_box_height + style.margin.top + style.margin.bottom,
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
        if style.overflow == css::Overflow::Visible
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
            self.layout_formatting_box(child, stylesheets);
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
        if sequence.records.is_empty() {
            return None;
        }
        let preceding_line_height = (0..sequence.records.len().saturating_sub(1))
            .map(|index| sequence.line_height(index))
            .sum::<f32>();
        Some(
            borders.top
                + style.padding.top
                + preceding_line_height
                + self.inline_box_text_line_layout_baseline_offset(style),
        )
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
