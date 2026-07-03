use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) {
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            self.layout_positioned_block_with_static_source(
                element,
                style,
                stylesheets,
                child_boxes,
                None,
            );
            return;
        }

        self.apply_forced_break(style.break_before);
        let mut geometry = self.block_layout_geometry(element, style, stylesheets, child_boxes);
        if should_prebreak_definite_block(DefiniteBlockBreakContext {
            definite_content_height: geometry.definite_content_height,
            vertical_extras: geometry.vertical_extras,
            style: &geometry.style,
            remaining_height: self.cursor_y + geometry.relative_offset.y - self.page_bottom(),
            page_area_height: self.page_area_height(),
            current_page_has_content: self.current_page_has_content(),
            at_page_top: self.cursor_is_at_page_top(),
            suppress_for_avoid_retry: self.avoid_inside_retry_depth > 0,
        }) {
            self.push_page();
            geometry = self.block_layout_geometry(element, style, stylesheets, child_boxes);
        }
        let defer_own_decoration_promotion = self.defer_next_block_decoration_promotion;
        self.defer_next_block_decoration_promotion = false;
        let containing_left = self.content_left;
        let containing_right = self.content_right;
        let containing_inline_size = (containing_right - containing_left).max(0.0);
        if matches!(
            geometry.style.position,
            Position::Relative | Position::Sticky
        ) {
            self.cursor_y += geometry.relative_offset.y;
        }
        let mut block_align_content_offset_y = 0.0;
        let starts_at_page_top = self.cursor_is_at_page_top() && self.truncate_page_start_margins;
        let applied_start_margin = page_start_margin(geometry.style.margin.top, starts_at_page_top);
        self.cursor_y -= applied_start_margin;
        let clearance_count_at_block_entry = self.applied_clearance_count;
        let establishes_independent_bfc = geometry
            .style
            .display
            .establishes_block_formatting_context()
            || used_overflow_clips_element(element, &geometry.style)
            || block_align_content_establishes_independent_formatting_context(
                geometry.style.align_content,
            );
        if !establishes_independent_bfc {
            let before_clear_page_index = self.pages.len();
            let before_clear_top = self.cursor_y;
            let cleared_top = self.clear_active_floats_top(
                geometry.style.clear,
                geometry.style.writing_mode,
                geometry.style.direction,
                self.cursor_y,
            );
            if self.pages.len() != before_clear_page_index || cleared_top < before_clear_top - 0.01
            {
                self.applied_clearance_count += 1;
            }
            self.cursor_y = cleared_top;
        }
        if establishes_independent_bfc && geometry.style.float == Float::None {
            if self.containing_block_writing_mode == WritingMode::HorizontalTb
                && geometry.style.writing_mode == WritingMode::HorizontalTb
            {
                let context = self
                    .float_contexts
                    .last()
                    .expect("root float context exists")
                    .clone();
                let page_index = self.current_float_page_index();
                let clear = geometry.style.clear;
                let writing_mode = geometry.style.writing_mode;
                let direction = geometry.style.direction;
                let placement = context.avoiding_bfc_root_position(
                    page_index,
                    self.cursor_y,
                    clear,
                    writing_mode,
                    direction,
                    containing_left,
                    containing_right,
                    |band_left, band_width, _candidate_top| {
                        let candidate_geometry = self.block_layout_geometry_in_inline_span(
                            element,
                            style,
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
                                element,
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
                self.cursor_y = placement.top;
                geometry = self.block_layout_geometry_in_inline_span(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    BlockLayoutInlineConstraint {
                        containing_left: placement.left,
                        containing_right: placement.left + placement.available_width,
                        percentage_basis: containing_inline_size,
                        auto_border_box_width: (placement.available_width
                            < containing_inline_size - 0.01)
                            .then_some(placement.available_width),
                    },
                );
                if placement.available_width < containing_inline_size - 0.01 {
                    let border_box_left = if self.containing_block_direction == Direction::Rtl {
                        placement.left
                            + (placement.available_width - geometry.outer_width()).max(0.0)
                    } else {
                        placement.left
                    };
                    let outer_x = border_box_left + geometry.relative_offset.x;
                    geometry.outer_inline = BlockInlineBounds::new(outer_x, geometry.outer_width());
                    geometry.content_inline = BlockInlineBounds::new(
                        outer_x + geometry.border_widths.left + geometry.style.padding.left,
                        geometry.content_width(),
                    );
                }
            } else {
                let style = &geometry.style;
                let margin_box_width =
                    style.margin.left + geometry.outer_width() + style.margin.right;
                let collision_height = geometry
                    .definite_content_height
                    .unwrap_or(style.line_height)
                    + geometry.vertical_extras
                    + style.margin.top
                    + style.margin.bottom;
                let (margin_box_left, avoided_top, _) = self.place_float_avoiding_margin_box(
                    self.cursor_y,
                    margin_box_width,
                    collision_height,
                    style.clear,
                    style.writing_mode,
                    style.direction,
                    self.containing_block_direction,
                );
                self.cursor_y = avoided_top;
                let outer_x = margin_box_left + style.margin.left + geometry.relative_offset.x;
                geometry.outer_inline = BlockInlineBounds::new(outer_x, geometry.outer_width());
                geometry.content_inline = BlockInlineBounds::new(
                    outer_x + geometry.border_widths.left + style.padding.left,
                    geometry.content_width(),
                );
            }
        }
        let style = &geometry.style;
        let block_line_trim = self.effective_text_box_line_trim_for_style(style);
        let relative_offset = geometry.relative_offset;
        let border_widths = geometry.border_widths;
        let vertical_extras = geometry.vertical_extras;
        let definite_content_height = geometry.definite_content_height;
        let multicol_content_height =
            definite_content_height.or_else(|| style.box_values.height.length_if_no_percent());
        let outer_inline = geometry.outer_inline();
        let content_inline = geometry.content_inline();
        let content_width = content_inline.size();
        let content_logical_inline_size = geometry.content_logical_inline_size();
        let outer_x = outer_inline.start();
        let inner_x = content_inline.start();
        let inner_width = content_inline.size();
        let block_top = self.cursor_y;
        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        self.cursor_y -= border_widths.top + style.padding.top;
        let content_top = self.cursor_y;
        self.fragment_top_offsets
            .push(self.current_page_context.top() - content_top);
        self.add_bookmark(element, style, inner_x, block_top);
        self.add_page_anchor(element, style);
        let descendant_bookmark_start = self.bookmarks.len();

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_containing_block_direction = self.containing_block_direction;
        let previous_containing_block_writing_mode = self.containing_block_writing_mode;
        self.content_left = inner_x;
        self.content_right = inner_x + inner_width;
        self.containing_block_direction = style.direction;
        self.containing_block_writing_mode = style.writing_mode;
        self.content_logical_inline_size_stack
            .push(content_logical_inline_size);
        self.child_available_space_stack
            .push(child_available_space_for_block(
                style,
                content_width,
                definite_content_height,
                self.page_area_height(),
            ));
        if establishes_independent_bfc {
            self.push_float_context();
        }
        if establishes_positioning_containing_block {
            self.containing_blocks
                .push(ContainingBlock::from_page_top_rect(
                    geometry.padding_box_top_rect(
                        outer_x,
                        block_top,
                        definite_content_height.unwrap_or(style.line_height),
                    ),
                ));
        }
        let overflow_clip_content_height =
            used_content_height_or_auto(style, self.page_area_height(), vertical_extras)
                .map(|height| constrain_height(style, height, content_width));
        let overflow_clip_active = if used_overflow_clips_element(element, style) {
            let clip_content_height = overflow_clip_content_height.unwrap_or_else(|| {
                (block_top
                    - border_widths.top
                    - style.padding.top
                    - style.padding.bottom
                    - self.page_bottom())
                .max(0.0)
            });
            let clip_height = clip_content_height + style.padding.top + style.padding.bottom;
            self.push_overflow_clip(
                PageTopRect::new(
                    outer_x + border_widths.left,
                    block_top - border_widths.top,
                    content_width + style.padding.left + style.padding.right,
                    clip_height,
                )
                .overflow_clip(),
            );
            true
        } else {
            false
        };
        let needs_deferred_overflow_clip =
            overflow_clip_active && overflow_clip_content_height.is_none();

        let list_marker =
            self.marker_for_list_item(element, style, previous_containing_block_direction);
        let pushed_list_context = self.push_list_context(element, style);

        let use_ordered_mixed_flow = child_boxes.is_none()
            && has_ordered_mixed_flow_content_with_font_metrics(
                element,
                style,
                stylesheets,
                &self.ancestors,
                &mut self.font_system,
            );
        let has_generated_content = style.content.is_generated();
        let has_normalized_flow_children = !has_generated_content
            && child_boxes
                .map(has_non_inline_formatting_box)
                .unwrap_or(false);
        let use_box_inline_items = !use_ordered_mixed_flow
            && !has_generated_content
            && !has_normalized_flow_children
            && child_boxes
                .map(formatting_box_has_inline_content)
                .unwrap_or(false);
        let has_run_in_inline_content = !run_in_children.is_empty();

        // If normalization consumed a run-in source's children, do not replay
        // its original DOM text here. Inline pseudo content that survives in
        // normalized inline boxes is handled through `use_box_inline_items`.
        let normalized_children_empty = child_boxes.is_some_and(|boxes| boxes.is_empty());
        let detached_normalized_text = normalized_children_empty
            && !has_generated_content
            && !inline_text_for_style(element, style).is_empty();
        let text = if normalized_children_empty
            || has_generated_content
            || use_ordered_mixed_flow
            || has_normalized_flow_children
            || use_box_inline_items
        {
            String::new()
        } else if is_document_canvas_element(element) {
            own_inline_text_for_style(element, style)
        } else {
            inline_text_for_style(element, style)
        };
        let has_generated_inline_content = !detached_normalized_text
            && (has_generated_content
                || (child_boxes.is_none()
                    && (style.before_style.is_some() || style.after_style.is_some())));
        let has_styled_inline_descendant = has_styled_inline_descendant_with_font_metrics(
            element,
            style,
            stylesheets,
            &self.ancestors,
            &mut self.font_system,
        );
        let has_collectable_inline_content = !text.is_empty() || has_generated_inline_content;
        let use_inline_items = has_collectable_inline_content
            && (has_styled_inline_descendant
                || has_generated_inline_content
                || plain_inline_content_needs_inline_items(&text, style)
                || style.text_align.justifies()
                || self.active_float_exclusions_at(self.cursor_y, style.line_height));
        if has_run_in_inline_content
            && !has_normalized_flow_children
            && let Some(child_boxes) = child_boxes
        {
            self.layout_run_in_inline_items_block(
                element,
                style,
                stylesheets,
                run_in_children,
                child_boxes,
                element.attrs.get("href").map(String::as_str),
                list_marker.as_ref(),
            );
        } else if has_collectable_inline_content {
            let pushed_text_box_trim = self.push_text_box_line_trim_scope(block_line_trim);
            if use_inline_items {
                let laid_out_multicol_inline_items = self.layout_multicol_inline_items_block(
                    element,
                    style,
                    stylesheets,
                    (0.0, 0.0),
                    element.attrs.get("href").map(String::as_str),
                    list_marker.as_ref(),
                    multicol_content_height,
                );
                if !laid_out_multicol_inline_items {
                    self.layout_inline_items_block(
                        element,
                        style,
                        stylesheets,
                        (0.0, 0.0),
                        element.attrs.get("href").map(String::as_str),
                        list_marker.as_ref(),
                    );
                }
            } else if style.display.is_list_item() {
                self.layout_list_text_block(
                    &text,
                    style,
                    0.0,
                    0.0,
                    element.attrs.get("href").map(String::as_str),
                    list_marker.as_ref(),
                );
            } else {
                let laid_out_multicol_text = self.layout_multicol_text_block(
                    &text,
                    style,
                    0.0,
                    0.0,
                    element.attrs.get("href").map(String::as_str),
                    multicol_content_height,
                );
                if !laid_out_multicol_text {
                    self.layout_text_block(
                        &text,
                        style,
                        0.0,
                        0.0,
                        element.attrs.get("href").map(String::as_str),
                    );
                }
            }
            self.pop_text_box_line_trim_scope(pushed_text_box_trim);
        }
        if !has_run_in_inline_content
            && use_box_inline_items
            && !(has_collectable_inline_content && use_inline_items)
            && let Some(child_boxes) = child_boxes
        {
            let pushed_text_box_trim = self.push_text_box_line_trim_scope(block_line_trim);
            self.layout_anonymous_block(style, child_boxes, stylesheets, list_marker.as_ref());
            self.pop_text_box_line_trim_scope(pushed_text_box_trim);
        }
        let laid_out_column_children = !use_ordered_mixed_flow
            && text.is_empty()
            && (self.layout_definition_list_columns(element, style, stylesheets, child_boxes)
                || self.layout_simple_block_child_columns(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                ));
        let has_direct_inline_content = has_run_in_inline_content
            || use_box_inline_items
            || !text.is_empty()
            || laid_out_column_children;
        if style.writing_mode != WritingMode::HorizontalTb && has_direct_inline_content {
            let vertical_inline_height = if use_box_inline_items {
                child_boxes
                    .map(|child_boxes| {
                        self.intrinsic_inline_measurement_for_boxes(
                            child_boxes,
                            style,
                            stylesheets,
                            content_logical_inline_size,
                        )
                        .physical_height(style)
                    })
                    .unwrap_or(0.0)
            } else if !text.is_empty() {
                self.estimate_text_physical_height(
                    &text,
                    style,
                    content_logical_inline_size,
                    0.0,
                    0.0,
                )
            } else {
                0.0
            };
            if vertical_inline_height > 0.0 {
                self.cursor_y = self.cursor_y.min(content_top - vertical_inline_height);
            }
        }
        if let Some(marker) = list_marker.as_ref()
            && text.is_empty()
            && !use_box_inline_items
            && !laid_out_column_children
        {
            if marker.position == ListStylePosition::Outside {
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
            } else {
                let pushed_text_box_trim = self.push_text_box_line_trim_scope(block_line_trim);
                self.layout_list_text_block("", style, 0.0, 0.0, None, Some(marker));
                self.pop_text_box_line_trim_scope(pushed_text_box_trim);
            }
        }
        let can_collapse_start_margin =
            can_collapse_block_start_margin(style, border_widths, has_direct_inline_content);
        let can_collapse_end_margin =
            can_collapse_block_end_margin(style, border_widths, has_direct_inline_content);
        let self_collapsing_block = if let Some(child_boxes) = child_boxes {
            is_self_collapsing_block_box(element, style, child_boxes)
        } else {
            is_self_collapsing_block_dom_with_font_metrics(
                element,
                style,
                stylesheets,
                &self.ancestors,
                &mut self.font_system,
            )
        };
        let children_outcome =
            self.layout_block_flow_children_phase(Box::new(BlockFlowChildrenPhaseInput {
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
            }));
        let pending_end_margin_collapse = children_outcome.pending_end_margin_collapse;
        let collapsed_start_margin_offset = children_outcome.collapsed_start_margin_offset;

        if establishes_independent_bfc
            && has_auto_height(style)
            && let Some(float_bottom) = self.current_float_context_lowest_bottom()
        {
            self.cursor_y = self.cursor_y.min(float_bottom);
        }
        if establishes_independent_bfc {
            self.pop_float_context();
        }
        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
        }
        self.pop_overflow_clip(overflow_clip_active);
        self.child_available_space_stack.pop();
        self.content_logical_inline_size_stack.pop();
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.containing_block_direction = previous_containing_block_direction;
        self.containing_block_writing_mode = previous_containing_block_writing_mode;

        if pushed_list_context {
            self.list_stack.pop();
        }

        if self_collapsing_block
            && self.pages.len() == paint_page_index
            && self.applied_clearance_count == clearance_count_at_block_entry
        {
            self.cursor_y = content_top;
        }

        let mut block_end_margin_to_consume = style.margin.bottom;
        if let Some(pending) = pending_end_margin_collapse {
            let content_height_with_child_margin = content_top - self.cursor_y;
            let content_height_without_child_margin =
                content_height_with_child_margin - pending.child_consumed_margin;
            self.cursor_y += pending.child_consumed_margin;
            if block_end_margin_collapse_survives_height_constraints(
                style,
                content_width,
                vertical_extras,
                content_height_without_child_margin,
            ) {
                block_end_margin_to_consume = pending.collapsed_margin;
            }
        }

        if !has_auto_height(style)
            || used_min_height(style, content_width).is_some()
            || used_max_height(style, content_width).is_some()
        {
            let current_content_height = content_top - self.cursor_y;
            let requested_content_height = definite_content_height.unwrap_or_else(|| {
                used_content_height_or_auto(style, current_content_height, vertical_extras)
                    .unwrap_or(current_content_height)
            });
            let height = constrain_height(style, requested_content_height, content_width);
            if self.pages.len() == paint_page_index
                && style.writing_mode == WritingMode::HorizontalTb
            {
                let free_space = height - current_content_height;
                block_align_content_offset_y = if laid_out_column_children {
                    multicol_align_content_y_offset(style.align_content, free_space)
                } else {
                    block_align_content_y_offset_for_style(style, free_space)
                };
            }
            self.cursor_y = content_top - height;
        }
        self.fragment_top_offsets.pop();
        self.cursor_y -= style.padding.bottom + border_widths.bottom;
        let block_bottom = self.cursor_y;
        let block_height = (block_top - block_bottom).max(0.0);
        let paint_block_top = block_top - collapsed_start_margin_offset;
        let paint_block_height = (block_height - collapsed_start_margin_offset).max(0.0);
        let border_box = geometry.border_box_top_rect(outer_x, paint_block_top, paint_block_height);
        let border_paint_rect = border_box.page_top_rect().paint_rect();
        // Auto-height overflow clips know their inline and block-start edges
        // before child layout, but the block-end edge is only available after
        // resolving the used height. CSS Overflow clips to the used padding box:
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
        let deferred_overflow_clip = needs_deferred_overflow_clip.then(|| {
            let clip_content_height = (block_height - vertical_extras).max(0.0);
            PageTopRect::new(
                outer_x + border_widths.left,
                block_top - border_widths.top,
                content_width + style.padding.left + style.padding.right,
                clip_content_height + style.padding.top + style.padding.bottom,
            )
            .paint_clip()
        });
        if block_height > 0.0 {
            self.mark_current_page_flow_content();
        }
        let background_page_index = self.pages.len();
        let mut own_background_primitives = Vec::new();
        let mut own_outline_primitives = Vec::new();
        if is_document_canvas_element(element) {
            if style.visibility == Visibility::Visible {
                self.capture_document_canvas_background(element, style);
            }
            if block_height > 0.0
                && (used_border_width(style) > 0.0 || style.border_image.source.is_some())
                && style.visibility == Visibility::Visible
            {
                // CSS Backgrounds propagates the root/body background to the
                // canvas, but borders are not canvas backgrounds; they remain
                // ordinary element border painting behind descendants:
                // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>.
                let mut border_style = style.clone();
                border_style.background_color = None;
                border_style.background_image = None;
                border_style.background_layers.clear();
                own_background_primitives = self.box_background_primitives(
                    border_paint_rect.origin.x,
                    border_paint_rect.origin.y,
                    border_paint_rect.size.width,
                    border_paint_rect.size.height,
                    &border_style,
                );
            }
        } else if block_height > 0.0
            && (style.background_color.is_some()
                || style.background_image.is_some()
                || style.border_image.source.is_some()
                || used_border_width(style) > 0.0)
            && style.visibility == Visibility::Visible
        {
            own_background_primitives = self.box_background_primitives(
                border_paint_rect.origin.x,
                border_paint_rect.origin.y,
                border_paint_rect.size.width,
                border_paint_rect.size.height,
                style,
            );
        }
        if block_height > 0.0 && style.visibility == Visibility::Visible {
            own_outline_primitives = self.box_outline_primitives(
                border_paint_rect.origin.x,
                border_paint_rect.origin.y,
                border_paint_rect.size.width,
                border_paint_rect.size.height,
                style,
            );
        }
        let has_own_background_primitives = !own_background_primitives.is_empty();
        let has_own_outline_primitives = !own_outline_primitives.is_empty();
        self.translate_aligned_block_descendant_bookmarks(
            descendant_bookmark_start,
            paint_page_index,
            0.0,
            block_align_content_offset_y,
        );
        if self.preserve_scoped_paint_public_order
            && self.pages.len() == paint_page_index
            && block_align_content_offset_y.abs() <= 0.01
            && !vertical_block_align_content_needs_fragment_bounds(style)
            && let Some(mut fragment) = self
                .current_page
                .paint_tree_fragment_since(&paint_checkpoint)
        {
            if let Some(overflow_clip) = deferred_overflow_clip {
                fragment = fragment.with_contents_effect_scoped_to_rect(overflow_clip);
            }
            if background_page_index == paint_page_index {
                self.current_page.prepend_recorded_primitives_to_fragment(
                    &mut fragment,
                    PaintBand::BackgroundBorder,
                    own_background_primitives,
                );
                self.current_page.append_recorded_primitives_to_fragment(
                    &mut fragment,
                    PaintBand::Outline,
                    own_outline_primitives,
                );
            }
            if !defer_own_decoration_promotion {
                fragment.promote_background_border_to_in_flow_block();
            }
            if ((has_own_background_primitives || has_own_outline_primitives)
                || deferred_overflow_clip.is_some())
                && !fragment.is_empty()
            {
                self.current_page
                    .replace_paint_tree_since_with_fragment(&paint_checkpoint, fragment);
            }
            self.cursor_y -= block_end_margin_to_consume;
            self.last_block_layout_outcome = BlockLayoutOutcome {
                consumed_bottom_margin: block_end_margin_to_consume,
            };
            if matches!(style.position, Position::Relative | Position::Sticky) {
                self.cursor_y -= relative_offset.y;
            }
            self.apply_forced_break(style.break_after);
            return;
        }
        let fragments = self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        let mut translated_vertical_bookmarks = false;
        for (page_index, mut fragment) in fragments {
            let mut block_align_content_offset_x = 0.0;
            if page_index == paint_page_index {
                block_align_content_offset_x = vertical_block_align_content_x_offset(
                    style,
                    inner_x,
                    content_width,
                    fragment.bounds(),
                );
                if block_align_content_offset_x.abs() > 0.01 && !translated_vertical_bookmarks {
                    self.translate_aligned_block_descendant_bookmarks(
                        descendant_bookmark_start,
                        paint_page_index,
                        block_align_content_offset_x,
                        0.0,
                    );
                    translated_vertical_bookmarks = true;
                }
            }
            if page_index == paint_page_index
                && (block_align_content_offset_x.abs() > 0.01
                    || block_align_content_offset_y.abs() > 0.01)
            {
                fragment = fragment.translated(PaintVector::new(
                    block_align_content_offset_x,
                    block_align_content_offset_y,
                ));
            }
            if let Some(overflow_clip) = deferred_overflow_clip {
                fragment = fragment.with_contents_effect_scoped_to_rect(overflow_clip);
            }
            if page_index == background_page_index {
                fragment.prepend_primitives_in_band(
                    PaintBand::BackgroundBorder,
                    own_background_primitives.clone(),
                );
                fragment
                    .append_primitives_in_band(PaintBand::Outline, own_outline_primitives.clone());
            }
            if !defer_own_decoration_promotion {
                fragment.promote_background_border_to_in_flow_block();
            }
            if fragment.is_empty() {
                continue;
            }
            if page_index < self.pages.len() {
                self.pages[page_index].append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
            } else {
                self.current_page
                    .append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
            }
        }
        self.cursor_y -= block_end_margin_to_consume;
        self.last_block_layout_outcome = BlockLayoutOutcome {
            consumed_bottom_margin: block_end_margin_to_consume,
        };
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y;
        }
        self.apply_forced_break(style.break_after);
    }
}
