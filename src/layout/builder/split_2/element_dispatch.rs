use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Exits an inline page-name scope, breaking before following inline content.
    ///
    /// When inline content has already been painted on the named page, returning
    /// to the surrounding page group must create a new page box before
    /// restoring that group. Otherwise following inline content would use the
    /// previous page box's margins and page selectors:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn exit_inline_page_name_scope(&mut self, scope: Option<PageNameScope>) {
        if scope.is_some() && self.current_page_has_content() {
            self.push_page_if_nonempty();
        }
        self.exit_page_name_scope(scope);
    }

    /// Suppresses CSS named-page group creation for out-of-flow and atomic layout.
    ///
    /// CSS Paged Media defines named page groups through normal-flow class A
    /// page-break boundaries. Absolutely positioned and fixed-position boxes
    /// are out of flow, while inline-block contents are laid out in an
    /// independent atomic inline formatting context; in both cases descendant
    /// `page` values do not directly select document page groups:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>,
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>, and
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning>.
    pub(in crate::layout) fn push_page_name_scope_suppression(&mut self) {
        self.page_name_scope_suppression += 1;
    }

    /// Re-enables CSS named-page group creation after suppressed layout.
    ///
    /// This closes the temporary suppression scope opened for out-of-flow or
    /// atomic inline formatting-context layout:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn pop_page_name_scope_suppression(&mut self) {
        self.page_name_scope_suppression = self.page_name_scope_suppression.saturating_sub(1);
    }

    /// Suppresses element-entry named-page scopes while preserving sibling switches.
    ///
    /// Flex items do not expose their own `page` value, or descendant-derived
    /// first/last page values, to the flex container boundary. Class A break
    /// opportunities between ordinary block descendants inside the flex item
    /// still select named page groups:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
    pub(in crate::layout) fn push_page_name_element_scope_suppression(&mut self) {
        self.page_name_element_scope_suppression += 1;
    }

    /// Re-enables element-entry named-page scopes after isolated item layout.
    ///
    /// This closes the flex-item page-scope isolation described by CSS Paged
    /// Media named pages and CSS Flexbox pagination:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn pop_page_name_element_scope_suppression(&mut self) {
        self.page_name_element_scope_suppression =
            self.page_name_element_scope_suppression.saturating_sub(1);
    }

    pub(in crate::layout) fn enter_page_name_scope_for_value(
        &mut self,
        page_name: Option<&str>,
    ) -> Option<Option<String>> {
        if self.current_page_name.as_deref() == page_name {
            return None;
        }
        let previous = self.current_page_name.clone();
        // CSS Paged Media assigns a named page type to boxes using the `page`
        // property. The initial `auto` value is still a real page type when
        // explicitly specified, because it can end an ancestor's named page
        // group. In this cursor-based layout engine, pages occupied by the
        // scoped element inherit that page value until the element finishes.
        // https://www.w3.org/TR/css-page-3/#using-named-pages
        self.push_page_if_nonempty();
        self.current_page_name = page_name.map(str::to_string);
        self.rebuild_empty_current_page_context();
        Some(previous)
    }

    pub(in crate::layout) fn exit_page_name_scope(&mut self, scope: Option<PageNameScope>) {
        let Some(scope) = scope else {
            return;
        };
        if self.current_page_name == scope.end_page_name {
            return;
        }
        self.current_page_name = scope.end_page_name;
        self.rebuild_empty_current_page_context();
    }

    pub(in crate::layout) fn layout_element_inner(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let layout_kind = element_layout_kind(element, style);
        if self.should_capture_non_positioned_effect_context(layout_kind, element, style) {
            self.layout_non_positioned_effect_context(
                layout_kind,
                element,
                style,
                stylesheets,
                run_in_children,
                child_boxes,
                table_fragment,
            );
            return;
        }
        self.layout_element_inner_kind(
            layout_kind,
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_element_inner_kind(
        &mut self,
        layout_kind: ElementLayoutKind,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        match layout_kind {
            ElementLayoutKind::None => (),
            ElementLayoutKind::Positioned => {
                self.layout_positioned_block_with_static_source(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    table_fragment,
                );
            }
            ElementLayoutKind::Canvas => self.layout_canvas(element, style),
            ElementLayoutKind::Image => self.layout_image(element, style),
            ElementLayoutKind::GeneratedImage => self.layout_generated_image(element, style),
            ElementLayoutKind::Svg => self.layout_svg(element, style),
            ElementLayoutKind::Flex => self.layout_flex(element, style, stylesheets, child_boxes),
            ElementLayoutKind::Grid => self.layout_grid(element, style, stylesheets, child_boxes),
            ElementLayoutKind::Table => {
                let built_child_boxes;
                let table_children = if let Some(children) = child_boxes {
                    children
                } else {
                    built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                        element,
                        stylesheets,
                        style,
                    );
                    &built_child_boxes
                };
                let built_fragment;
                let fragment = if let Some(fragment) = table_fragment {
                    fragment
                } else {
                    let signature = self
                        .ancestors
                        .last()
                        .cloned()
                        .unwrap_or_else(|| element_signature(element));
                    built_fragment =
                        box_tree::build_frozen_table_fragment(element, &signature, table_children);
                    &built_fragment
                };
                self.layout_table(element, style, stylesheets, fragment)
            }
            ElementLayoutKind::InlineFlow => {
                let text = inline_text_for_style(element, style);
                if !text.is_empty() {
                    if style.display.is_list_item() {
                        let marker = self.marker_for_list_item(
                            element,
                            style,
                            self.containing_block_direction,
                        );
                        self.layout_list_text_block(
                            &text,
                            style,
                            0.0,
                            0.0,
                            element.attrs.get("href").map(String::as_str),
                            marker.as_ref(),
                        );
                    } else {
                        self.layout_text_block(
                            &text,
                            style,
                            0.0,
                            0.0,
                            element.attrs.get("href").map(String::as_str),
                        );
                    }
                }
            }
            ElementLayoutKind::BlockFlow => {
                self.layout_block(element, style, stylesheets, run_in_children, child_boxes);
            }
        }
    }

    pub(in crate::layout) fn should_capture_non_positioned_effect_context(
        &self,
        layout_kind: ElementLayoutKind,
        element: &Element,
        style: &ComputedStyle,
    ) -> bool {
        !matches!(
            layout_kind,
            ElementLayoutKind::None | ElementLayoutKind::Positioned
        ) && StackingContextPolicy::style_needs_non_positioned_scope(element, style)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_non_positioned_effect_context(
        &mut self,
        layout_kind: ElementLayoutKind,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        let initial_policy = StackingContextPolicy::for_non_positioned_effect(
            element,
            style,
            PaintClip::from_paint_rect(paint_space_rect(0.0, 0.0, 0.0, 0.0)),
        );
        let previous_defer_block_decoration_promotion = self.defer_next_block_decoration_promotion;
        self.defer_next_block_decoration_promotion = true;
        self.layout_element_inner_kind(
            layout_kind,
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
        );
        self.defer_next_block_decoration_promotion = previous_defer_block_decoration_promotion;
        let child_layers = if positioned_layer_start < self.positioned_layers.len()
            && !matches!(
                initial_policy.child_layer_policy,
                ChildLayerPolicy::EscapeAll
            ) {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        let (child_layers, escaped_layers): (Vec<_>, Vec<_>) =
            match initial_policy.child_layer_policy {
                ChildLayerPolicy::CaptureAll => (child_layers, Vec::new()),
                ChildLayerPolicy::CaptureAutoLevel => child_layers
                    .into_iter()
                    .partition(|layer| matches!(layer.stack_level, StackLevel::Auto)),
                ChildLayerPolicy::EscapeAll => (Vec::new(), child_layers),
            };
        self.positioned_layers.extend(escaped_layers);
        let mut fragments =
            self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        for layer in &child_layers {
            if !fragments
                .iter()
                .any(|(page_index, _)| *page_index == layer.page_index)
            {
                fragments.push((
                    layer.page_index,
                    PaintFragment::from_primitives(Vec::new(), Vec::new()),
                ));
            }
        }
        for (page_index, mut fragment) in fragments {
            let mut child_contexts = child_layers
                .iter()
                .filter(|layer| layer.page_index == page_index)
                .cloned()
                .map(|layer| layer.context.with_links(layer.links))
                .collect::<Vec<_>>();
            if fragment.is_empty() && child_contexts.is_empty() {
                continue;
            }
            let source_order = self.next_paint_source_order();
            let (page_width, page_height) = if page_index < self.pages.len() {
                (
                    self.pages[page_index].width(),
                    self.pages[page_index].height(),
                )
            } else {
                (self.current_page.width(), self.current_page.height())
            };
            let target_page = if page_index < self.pages.len() {
                &mut self.pages[page_index]
            } else {
                &mut self.current_page
            };
            let bounds = fragment
                .bounds()
                .unwrap_or(PaintClip::from_paint_rect(paint_space_rect(
                    0.0,
                    0.0,
                    page_width,
                    page_height,
                )));
            let mut policy =
                StackingContextPolicy::for_non_positioned_effect(element, style, bounds);
            if let Some(overflow_clip) = policy.effects.overflow_clip.take() {
                if matches!(policy.context_kind, StackingContextKind::None)
                    && child_contexts.is_empty()
                {
                    fragment = fragment.with_contents_effect_scoped_to_rect(overflow_clip);
                    target_page.append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
                    continue;
                } else {
                    fragment = fragment.with_contents_clipped_to_rect(
                        overflow_clip,
                        std::mem::take(&mut child_contexts),
                    );
                }
            }
            let context = PaintStackingContext::from_banded_fragment_with_stack_level(
                policy.stack_level,
                fragment,
                child_contexts,
            )
            .with_source_order(source_order)
            .with_effects(policy.effects)
            .with_bounds(bounds);
            let context_fragment =
                PaintFragment::from_stacking_context_in_band(policy.parent_band, context);
            target_page.append_paint_fragment(&context_fragment, PaintVector::new(0.0, 0.0));
        }
    }

    pub(in crate::layout) fn layout_positioned_block_with_static_source(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        if style.abspos_static_source_was_inline_level
            && let Some(static_baseline_y) = self.current_page.lines.last().map(|line| line.y())
        {
            self.layout_positioned_block_with_inline_static_position(
                element,
                style,
                stylesheets,
                child_boxes,
                table_fragment,
                InlineStaticPosition {
                    start_x: self.content_left,
                    end_x: self.content_right,
                    top_y: self.cursor_y,
                    baseline_y: static_baseline_y,
                    use_margin_box_top: false,
                },
            );
            return;
        }
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
    }

    pub(in crate::layout) fn layout_anonymous_block(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        marker: Option<&ListMarker>,
    ) {
        self.layout_anonymous_block_with_first_line_policy(
            style,
            children,
            stylesheets,
            marker,
            true,
        );
    }

    pub(in crate::layout) fn layout_anonymous_block_with_first_line_policy(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        marker: Option<&ListMarker>,
        allow_typographic_first_line: bool,
    ) {
        let suppressed_style = (!allow_typographic_first_line)
            .then(|| style_without_typographic_first_line_pseudos(style))
            .flatten();
        let style = suppressed_style.as_ref().unwrap_or(style);
        let available_width = self.current_content_logical_inline_size().max(1.0);
        if marker.is_none()
            && anonymous_block_is_plain_text_with_style(children, style)
            && !self.active_float_exclusions_at(self.cursor_y, style.line_height)
        {
            let text = inline_text_from_formatting_boxes(children);
            if !text.is_empty() {
                self.layout_text_block(&text, style, 0.0, 0.0, None);
            }
            return;
        }
        let mut items = Vec::new();
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Outside
        {
            if self.cursor_y - style.font_size < self.page_bottom() {
                self.push_page();
            }
            self.paint_outside_marker(
                marker,
                style,
                self.content_left,
                self.content_right,
                self.cursor_y,
            );
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
        }
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Inside
            && (marker.image.is_some() || !trim_css_collapsible_whitespace(&marker.text).is_empty())
        {
            self.push_inside_marker_items(marker, style, None, &mut items);
        }
        self.collect_inline_box_items(
            children,
            stylesheets,
            None,
            0.0,
            InlineVisualOffset::zero(),
            style,
            style.text_decoration,
            &mut items,
        );
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
        }
        if !items.is_empty() {
            let multicol_content_height = style
                .box_values
                .height
                .length_if_no_percent()
                .or_else(|| self.definite_block_size_stack.last().copied().flatten());
            match self.try_layout_multicol_inline_items(
                items,
                style,
                available_width,
                (0.0, 0.0),
                multicol_content_height,
            ) {
                Ok(()) => return,
                Err(returned_items) => items = returned_items,
            }
            self.layout_inline_items(items, style, available_width, 0.0, 0.0, stylesheets);
        }
    }

    pub(in crate::layout) fn layout_inline_split_block_context(
        &mut self,
        context: &box_tree::InlineSplitBlockContextBox<'_>,
        stylesheets: &[Stylesheet],
    ) {
        let scope = self.begin_inline_split_block_paint_scope();
        for child in &context.children {
            self.layout_formatting_box(child, stylesheets);
        }
        self.finish_inline_split_block_paint_scope(context, scope);
    }

    pub(in crate::layout) fn begin_inline_split_block_paint_scope(
        &mut self,
    ) -> InlineSplitBlockPaintScope {
        InlineSplitBlockPaintScope {
            page_index: self.pages.len(),
            checkpoint: self.current_page.paint_checkpoint(),
            positioned_layer_start: self.positioned_layers.len(),
            source_order: self.next_paint_source_order(),
        }
    }

    /// Lays out a float generated by a block-in-inline split while preserving
    /// the split inline ancestor as the absolute containing block.
    ///
    /// CSS 2.2 defines the containing block for an absolutely positioned box
    /// whose nearest positioned ancestor is inline as the bounding box around
    /// that inline's padding boxes. Block-in-inline normalization unwraps the
    /// block child for normal flow, so floated descendants need this temporary
    /// scope to keep absolute descendants from resolving against the outer
    /// block or page instead:
    /// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_floating_child_in_inline_split_block_context(
        &mut self,
        context: &box_tree::InlineSplitBlockContextBox<'_>,
        child_element: &Element,
        child_signature: ElementSignature,
        child_style: &ComputedStyle,
        child_children: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &[Stylesheet],
        run: &mut FloatRunState,
    ) -> bool {
        let pushed_containing_block = self.push_inline_split_positioning_containing_block(context);
        let laid_out = self.layout_floating_child(
            child_element,
            child_signature,
            child_style,
            child_children,
            table_fragment,
            stylesheets,
            run,
        );
        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        laid_out
    }

    /// Push the CSS absolute containing block established by a positioned
    /// inline split fragment.
    ///
    /// CSS 2.2 makes an inline positioned ancestor establish the absolute
    /// containing block from its padding boxes. For a split segment containing
    /// only a block-level child, Quire has no inline line fragment to measure,
    /// so the single-line fragment is represented by the inline padding box at
    /// the current block-flow cursor:
    /// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
    pub(in crate::layout) fn push_inline_split_positioning_containing_block(
        &mut self,
        context: &box_tree::InlineSplitBlockContextBox<'_>,
    ) -> bool {
        let style = &context.style;
        if !inline_split_style_establishes_positioning_containing_block(style) {
            return false;
        }
        let border_widths = used_border_widths(style);
        let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
            self.content_left + style.margin.left + border_widths.left,
            self.cursor_y - border_widths.top,
            style.padding.left + style.padding.right,
            style.line_height + style.padding.top + style.padding.bottom,
        ));
        self.containing_blocks.push(containing_block);
        true
    }

    /// Replays a block-in-inline split segment under its inline ancestor's
    /// visual positioning and stacking policy.
    ///
    /// CSS 2.2 splits an inline around in-flow block-level descendants, but
    /// relative positioning applies to all generated boxes for that inline and
    /// Appendix E paints a positioned inline's generated content at the inline's
    /// stack level. This scopes only paint; normal-flow layout has already used
    /// the split block child directly:
    /// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>,
    /// <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>, and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(in crate::layout) fn finish_inline_split_block_paint_scope(
        &mut self,
        context: &box_tree::InlineSplitBlockContextBox<'_>,
        scope: InlineSplitBlockPaintScope,
    ) {
        let initial_policy = StackingContextPolicy::for_non_positioned_style_effect(
            &context.style,
            PaintClip::from_paint_rect(paint_space_rect(0.0, 0.0, 0.0, 0.0)),
        );
        let child_layers = if scope.positioned_layer_start < self.positioned_layers.len()
            && !matches!(
                initial_policy.child_layer_policy,
                ChildLayerPolicy::EscapeAll
            ) {
            self.positioned_layers
                .split_off(scope.positioned_layer_start)
        } else {
            Vec::new()
        };
        let (child_layers, escaped_layers): (Vec<_>, Vec<_>) =
            match initial_policy.child_layer_policy {
                ChildLayerPolicy::CaptureAll => (child_layers, Vec::new()),
                ChildLayerPolicy::CaptureAutoLevel => child_layers
                    .into_iter()
                    .partition(|layer| matches!(layer.stack_level, StackLevel::Auto)),
                ChildLayerPolicy::EscapeAll => (Vec::new(), child_layers),
            };
        self.positioned_layers.extend(escaped_layers);

        let mut fragments =
            self.take_positioned_fragments_since(scope.page_index, scope.checkpoint);
        for layer in &child_layers {
            if !fragments
                .iter()
                .any(|(page_index, _)| *page_index == layer.page_index)
            {
                fragments.push((
                    layer.page_index,
                    PaintFragment::from_primitives(Vec::new(), Vec::new()),
                ));
            }
        }

        let offset = relative_position_offset(&context.style, self.current_containing_block());
        let paint_offset = PaintVector::new(offset.x, offset.y);
        for (page_index, fragment) in fragments {
            let child_contexts = child_layers
                .iter()
                .filter(|layer| layer.page_index == page_index)
                .cloned()
                .map(|layer| {
                    layer
                        .context
                        .translated(paint_offset)
                        .with_links(layer.links)
                })
                .collect::<Vec<_>>();
            let fragment = fragment.translated(paint_offset);
            if fragment.is_empty() && child_contexts.is_empty() {
                continue;
            }
            let (page_width, page_height) = if page_index < self.pages.len() {
                (
                    self.pages[page_index].width(),
                    self.pages[page_index].height(),
                )
            } else {
                (self.current_page.width(), self.current_page.height())
            };
            let bounds = fragment
                .bounds()
                .unwrap_or(PaintClip::from_paint_rect(paint_space_rect(
                    0.0,
                    0.0,
                    page_width,
                    page_height,
                )));
            let policy =
                StackingContextPolicy::for_non_positioned_style_effect(&context.style, bounds);
            let context = PaintStackingContext::from_banded_fragment_with_stack_level(
                policy.stack_level,
                fragment,
                child_contexts,
            )
            .with_source_order(scope.source_order)
            .with_effects(policy.effects)
            .with_bounds(bounds);
            let fragment =
                PaintFragment::from_stacking_context_in_band(policy.parent_band, context);
            let target_page = if page_index < self.pages.len() {
                &mut self.pages[page_index]
            } else {
                &mut self.current_page
            };
            target_page.append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
            target_page.sort_paint_tree_stacking_contexts();
        }
    }

    pub(in crate::layout) fn push_page(&mut self) {
        if !self.current_page_has_content() {
            // CSS Fragmentation allows a box fragment to be split across
            // fragmentainers, but a carried fragment offset must not make a
            // fresh empty page permanently unfillable. If a break is requested
            // before anything painted on the current page, keep the same page
            // number and retry the fragment at the top of this page area:
            // <https://www.w3.org/TR/css-break-3/#breaking-rules>.
            let offsets = FragmentOffsets {
                top: 0.0,
                ..self.current_fragment_offsets()
            };
            let context = self.resolved_page_context(self.pages.len() + 1, false);
            self.current_page = page_for_context(context);
            self.current_page_has_flow_content = false;
            self.apply_page_context(context, offsets);
            self.truncate_page_start_margins = true;
            self.apply_pending_float_fragments_for_current_page();
            return;
        }
        self.flush_positioned_layers();
        let offsets = self.current_fragment_offsets_for_page_break();
        let next_context = self.resolved_page_context(self.pages.len() + 2, false);
        let next_page = page_for_context(next_context);
        let page = std::mem::replace(&mut self.current_page, next_page);
        self.current_page_has_flow_content = false;
        self.pages.push(page);
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(false);
        self.page_named_strings
            .push(std::mem::take(&mut self.current_page_named_strings));
        self.page_running_elements
            .push(std::mem::take(&mut self.current_page_running_elements));
        self.apply_page_context(next_context, offsets);
        self.truncate_page_start_margins = true;
        self.apply_pending_float_fragments_for_current_page();
    }

    pub(in crate::layout) fn push_blank_page(&mut self) {
        // CSS Fragmentation forced left/right/recto/verso breaks can generate
        // blank pages. Those pages are real page boxes and match `@page :blank`.
        // https://www.w3.org/TR/css-break-3/#break-between
        let page_number = self.pages.len() + 1;
        let context = self.resolved_page_context(page_number, true);
        self.pages.push(page_for_context(context));
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(true);
        self.page_named_strings.push(HashMap::new());
        self.page_running_elements.push(HashMap::new());
    }

    pub(in crate::layout) fn push_page_if_nonempty(&mut self) {
        if self.current_page_has_content() {
            self.push_page();
        }
    }

    /// Captures the active formatting-context insets from the current page area.
    ///
    /// A page break fragments boxes without leaving their containing block, while
    /// a named-page transition can select a different page area. Keeping these
    /// offsets preserves ancestor margins and padding on the new page fragment:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn current_fragment_offsets(&self) -> FragmentOffsets {
        FragmentOffsets {
            left: self.content_left - self.current_page_context.left(),
            right: self.current_page_context.right() - self.content_right,
            top: self
                .fragment_top_offsets
                .last()
                .copied()
                .unwrap_or_else(|| self.current_page_context.top() - self.cursor_y),
        }
    }

    /// Captures fragment insets for an actual page break.
    ///
    /// The next fragment keeps horizontal containing-block insets, but starts
    /// at the block-start edge of the new fragmentainer. CSS Fragmentation's
    /// initial `box-decoration-break: slice` behavior does not clone ancestor
    /// block-start margin, border, or padding into continuation fragments:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>.
    pub(in crate::layout) fn current_fragment_offsets_for_page_break(&self) -> FragmentOffsets {
        let mut offsets = self.current_fragment_offsets();
        offsets.top = 0.0;
        offsets
    }

    /// Applies a new page context while preserving active fragment insets.
    ///
    /// CSS Paged Media changes the page area's size and margins per page, but
    /// CSS Fragmentation keeps content in the same containing block across page
    /// fragments:
    /// <https://www.w3.org/TR/css-page-3/#page-model> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn apply_page_context(
        &mut self,
        context: PageContext,
        offsets: FragmentOffsets,
    ) {
        self.current_page_context = context;
        self.current_page.rotation = context.rotation;
        self.cursor_y = context.top() - offsets.top;
        self.content_left = context.left() + offsets.left;
        self.content_right = (context.right() - offsets.right).max(self.content_left);
    }

    pub(in crate::layout) fn apply_forced_break(&mut self, forced_break: PageBreak) {
        if !forced_break.is_forced() {
            return;
        }
        if self.current_page_has_content() {
            self.push_page();
        }
        while !forced_break_satisfied(
            forced_break,
            self.pages.len() + 1,
            self.page_progression_direction,
        ) {
            self.push_blank_page();
        }
        if !self.current_page_has_content() {
            let offsets = self.current_fragment_offsets_for_page_break();
            let page_number = self.pages.len() + 1;
            let context = self.resolved_page_context(page_number, false);
            self.current_page = page_for_context(context);
            self.apply_page_context(context, offsets);
        }
        self.truncate_page_start_margins = true;
    }

    pub(in crate::layout) fn current_page_has_content(&self) -> bool {
        self.current_page.has_paint_content() || self.current_page_has_flow_content
    }

    /// Marks the current page as containing a non-empty normal-flow box.
    ///
    /// CSS Fragmentation fragments boxes into page fragmentainers even when a
    /// particular fragment has no visible paint. A used border box with
    /// positive area must therefore keep its page for forced breaks and final
    /// pagination, independently from PDF paint primitives:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/css-box-3/#box-model>.
    pub(in crate::layout) fn mark_current_page_flow_content(&mut self) {
        self.current_page_has_flow_content = true;
    }

    pub(in crate::layout) fn page_left(&self) -> f32 {
        self.current_page_context.left()
    }

    pub(in crate::layout) fn page_top(&self) -> f32 {
        self.current_page_context.top()
    }

    pub(in crate::layout) fn page_bottom(&self) -> f32 {
        self.current_page_context.bottom()
    }

    pub(in crate::layout) fn page_area_width(&self) -> f32 {
        self.current_page_context.area_width()
    }

    pub(in crate::layout) fn page_area_height(&self) -> f32 {
        self.current_page_context.area_height()
    }

    pub(in crate::layout) fn current_content_logical_inline_size(&self) -> f32 {
        self.content_logical_inline_size_stack
            .last()
            .copied()
            .unwrap_or_else(|| (self.content_right - self.content_left).max(0.0))
    }

    pub(in crate::layout) fn page_child_available_space(&self) -> ChildAvailableSpace {
        ChildAvailableSpace::new(
            WritingMode::HorizontalTb,
            self.page_area_width(),
            Some(self.page_area_height()),
            self.page_area_height(),
        )
    }

    pub(in crate::layout) fn current_child_available_space(&self) -> ChildAvailableSpace {
        self.child_available_space_stack
            .last()
            .copied()
            .unwrap_or_else(|| self.page_child_available_space())
    }

    pub(in crate::layout) fn resolved_page_context(
        &mut self,
        page_number: usize,
        is_blank: bool,
    ) -> PageContext {
        let declarations = self.page_declarations_for_page(
            page_number,
            self.current_page_name.as_deref(),
            is_blank,
        );
        let base = PageContext::from_options(self.options);
        let ch_advance = self.page_ch_advance_for_declarations(&declarations);
        // CSS Paged Media defines page size and page margins in the page
        // context; these declarations select the page box before its content
        // area is used for layout.
        // https://www.w3.org/TR/css-page-3/#page-model
        let size = css::page_size_from_with_ch_advance(&declarations, base.size, ch_advance);
        let page_edges =
            page_box_edges_from_declarations_with_ch_advance(&declarations, size, ch_advance);
        PageContext {
            size,
            margins: css::page_margins_from_for_size_and_edges_with_ch_advance(
                &declarations,
                base.margins,
                size,
                page_edges.total(),
                ch_advance,
            ),
            edges: page_edges,
            rotation: css::page_rotation_from(&declarations, base.rotation),
        }
    }

    pub(in crate::layout) fn finished_page_context(
        &mut self,
        page_number: usize,
        page_size: PageSize,
    ) -> PageContext {
        let page_name = self.page_name_for_number(page_number);
        let is_blank = self.page_is_blank_for_number(page_number);
        let declarations = self.page_declarations_for_page(page_number, page_name, is_blank);
        let base = PageContext::from_options(self.options);
        let ch_advance = self.page_ch_advance_for_declarations(&declarations);
        let page_edges =
            page_box_edges_from_declarations_with_ch_advance(&declarations, page_size, ch_advance);
        PageContext {
            size: page_size,
            margins: css::page_margins_from_for_size_and_edges_with_ch_advance(
                &declarations,
                base.margins,
                page_size,
                page_edges.total(),
                ch_advance,
            ),
            edges: page_edges,
            rotation: css::page_rotation_from(&declarations, base.rotation),
        }
    }

    pub(in crate::layout) fn page_ch_advance_for_declarations(
        &mut self,
        declarations: &Declarations,
    ) -> f32 {
        let style = css::page_style_for_declarations(declarations);
        self.font_system.ch_advance(&style)
    }

    pub(in crate::layout) fn rebuild_empty_current_page_context(&mut self) {
        if self.current_page_has_content() {
            return;
        }
        let offsets = self.current_fragment_offsets();
        let page_number = self.pages.len() + 1;
        let context = self.resolved_page_context(page_number, false);
        self.current_page = page_for_context(context);
        self.apply_page_context(context, offsets);
    }

    pub(in crate::layout) fn has_renderable_content(&self) -> bool {
        !self.pages.is_empty()
            || self.current_page_has_content()
            || !self.positioned_layers.is_empty()
            || !self.fixed_layers.is_empty()
            || !self.page_margin_boxes.is_empty()
            || self
                .page_rules
                .iter()
                .any(|rule| !rule.margin_boxes.is_empty())
    }

    pub(in crate::layout) fn cursor_is_at_page_top(&self) -> bool {
        (self.cursor_y - self.page_top()).abs() < 0.01
    }

    /// Resolves a captured assignment to the first source fragment's final page.
    ///
    /// CSS GCPM `start` lookups are based on the source fragment at the page
    /// boundary, not on the earlier style/counter capture point. If layout
    /// pushes a page after capture, the original page checkpoint tells whether
    /// the source painted there or moved wholly to the new current page:
    /// <https://www.w3.org/TR/css-gcpm-3/#named-strings>.
    pub(in crate::layout) fn final_source_assignment_placement(
        &self,
        style: &ComputedStyle,
        captured_page_index: usize,
        captured_paint_checkpoint: PaintCheckpoint,
        captured_starts_page_fragment: bool,
        captured_content_left: f32,
        captured_cursor_y: f32,
    ) -> AssignmentPlacement {
        let height = style.line_height.max(0.0);
        let width = (self.content_right - self.content_left).max(0.0);
        if captured_page_index < self.pages.len() {
            let original_page_changed =
                self.pages[captured_page_index].paint_checkpoint() != captured_paint_checkpoint;
            if original_page_changed {
                return AssignmentPlacement {
                    page_index: captured_page_index,
                    starts_page_fragment: captured_starts_page_fragment,
                    border_box: Some(
                        PageTopRect::new(captured_content_left, captured_cursor_y, width, height)
                            .paint_clip(),
                    ),
                };
            }
            return AssignmentPlacement {
                page_index: self.pages.len(),
                starts_page_fragment: true,
                border_box: Some(
                    PageTopRect::new(self.content_left, self.page_top(), width, height)
                        .paint_clip(),
                ),
            };
        }
        AssignmentPlacement {
            page_index: captured_page_index,
            starts_page_fragment: captured_starts_page_fragment,
            border_box: Some(
                PageTopRect::new(captured_content_left, captured_cursor_y, width, height)
                    .paint_clip(),
            ),
        }
    }
}

fn inline_split_style_establishes_positioning_containing_block(style: &ComputedStyle) -> bool {
    matches!(
        style.position,
        Position::Absolute | Position::Fixed | Position::Relative | Position::Sticky
    ) || !style.transform.is_empty()
}
