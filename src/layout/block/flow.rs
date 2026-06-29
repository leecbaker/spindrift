use super::super::*;

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
        let style = &geometry.style;
        let relative_offset = geometry.relative_offset;
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y += relative_offset.y;
        }
        let border_widths = geometry.border_widths;
        let vertical_extras = geometry.vertical_extras;
        let definite_content_height = geometry.definite_content_height;
        let mut outer_inline = geometry.outer_inline();
        let mut content_inline = geometry.content_inline();
        let content_width = content_inline.size();
        let outer_width = outer_inline.size();
        let mut block_align_content_offset_y = 0.0;
        let starts_at_page_top = self.cursor_is_at_page_top() && self.truncate_page_start_margins;
        let applied_start_margin = page_start_margin(style.margin.top, starts_at_page_top);
        self.cursor_y -= applied_start_margin;
        let clearance_count_at_block_entry = self.applied_clearance_count;
        let establishes_independent_bfc = style.display.establishes_block_formatting_context()
            || style.overflow.clips_overflow()
            || block_align_content_establishes_independent_formatting_context(style.align_content);
        if !establishes_independent_bfc {
            let before_clear_page_index = self.pages.len();
            let before_clear_top = self.cursor_y;
            let cleared_top = self.clear_active_floats_top(
                style.clear,
                style.writing_mode,
                style.direction,
                self.cursor_y,
            );
            if self.pages.len() != before_clear_page_index || cleared_top < before_clear_top - 0.01
            {
                self.applied_clearance_count += 1;
            }
            self.cursor_y = cleared_top;
        }
        if establishes_independent_bfc && style.float == Float::None {
            let margin_box_width = style.margin.left + outer_width + style.margin.right;
            let collision_height = definite_content_height.unwrap_or(style.line_height)
                + vertical_extras
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
            let outer_x = margin_box_left + style.margin.left + relative_offset.x;
            outer_inline = BlockInlineBounds::new(outer_x, outer_width);
            content_inline = BlockInlineBounds::new(
                outer_x + border_widths.left + style.padding.left,
                content_width,
            );
        }
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
        let overflow_clip_active = if style.overflow.clips_overflow()
            && !is_document_canvas_element(element)
            && let Some(clip_content_height) =
                used_content_height_or_auto(style, self.page_area_height(), vertical_extras)
                    .map(|height| constrain_height(style, height, content_width))
        {
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

        let list_marker =
            self.marker_for_list_item(element, style, previous_containing_block_direction);
        let pushed_list_context = self.push_list_context(element, style);

        let use_ordered_mixed_flow = child_boxes.is_none()
            && has_ordered_mixed_flow_content(element, style, stylesheets, &self.ancestors);
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
                || style.before_style.is_some()
                || style.after_style.is_some());
        let has_styled_inline_descendant =
            has_styled_inline_descendant(element, style, stylesheets, &self.ancestors);
        let has_collectable_inline_content = !text.is_empty() || has_generated_inline_content;
        let use_inline_items = has_collectable_inline_content
            && (has_styled_inline_descendant
                || has_generated_inline_content
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
            if use_inline_items {
                self.layout_inline_items_block(
                    element,
                    style,
                    stylesheets,
                    (0.0, 0.0),
                    element.attrs.get("href").map(String::as_str),
                    list_marker.as_ref(),
                );
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
                self.layout_text_block(
                    &text,
                    style,
                    0.0,
                    0.0,
                    element.attrs.get("href").map(String::as_str),
                );
            }
        }
        if !has_run_in_inline_content
            && use_box_inline_items
            && !(has_collectable_inline_content && use_inline_items)
            && let Some(child_boxes) = child_boxes
        {
            self.layout_anonymous_block(style, child_boxes, stylesheets, list_marker.as_ref());
        }
        let laid_out_column_children = !use_ordered_mixed_flow
            && text.is_empty()
            && self.layout_definition_list_columns(element, style, stylesheets, child_boxes);
        let has_direct_inline_content = has_run_in_inline_content
            || use_box_inline_items
            || !text.is_empty()
            || laid_out_column_children;
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
                self.layout_list_text_block("", style, 0.0, 0.0, None, Some(marker));
            }
        }
        let can_collapse_start_margin =
            can_collapse_block_start_margin(style, border_widths, has_direct_inline_content);
        let can_collapse_end_margin =
            can_collapse_block_end_margin(style, border_widths, has_direct_inline_content);
        let self_collapsing_block = if let Some(child_boxes) = child_boxes {
            is_self_collapsing_block_box(element, style, child_boxes)
        } else {
            is_self_collapsing_block_dom(element, style, stylesheets, &self.ancestors)
        };
        let mut collapsed_end_margin = false;
        let mut previous_flow_bottom_margin = None;
        let mut seen_flow_child = false;
        let mut trim_block_start_adjoining_margins = style.margin_trim.block_start;
        self.definite_block_size_stack.push(definite_content_height);

        if !laid_out_column_children && !use_box_inline_items {
            if use_ordered_mixed_flow {
                collapsed_end_margin = self.layout_ordered_mixed_flow_children(
                    element,
                    style,
                    stylesheets,
                    can_collapse_start_margin,
                    can_collapse_end_margin,
                );
            } else if let Some(child_boxes) = child_boxes {
                let mut float_run = self.float_run_state();
                let mut previous_child_page_end: Option<Option<String>> = None;
                let mut avoid_run_candidate: Option<AvoidBreakRunCandidate> = None;
                let mut previous_break_after_avoid = false;
                let mut child_box_index = 0usize;
                while child_box_index < child_boxes.len() {
                    let child_box = &child_boxes[child_box_index];
                    let child_start_candidate = AvoidBreakRunCandidate {
                        snapshot: self.snapshot(),
                        index: child_box_index,
                        element_index: 0,
                        previous_flow_bottom_margin,
                        seen_flow_child,
                        trim_block_start_adjoining_margins,
                        collapsed_end_margin,
                        previous_child_page_end: previous_child_page_end.clone(),
                        float_run,
                        height: 0.0,
                    };
                    let zero_height_page_boundary =
                        formatting_box_is_zero_height_page_boundary(child_box);
                    let child_page_value_sources = formatting_box_is_in_normal_flow(child_box)
                        .then(|| formatting_box_page_value_sources(child_box));
                    let effective_child_page_start = if zero_height_page_boundary {
                        Some(coalesced_zero_height_page_start(
                            child_boxes,
                            child_box_index,
                        ))
                    } else {
                        child_page_value_sources
                            .as_ref()
                            .map(|(child_page_start, _)| {
                                page_boundary_name_in_parent_scope(child_page_start.clone(), style)
                            })
                    };
                    if let Some(child_page_start) = &effective_child_page_start
                        && let Some(previous_page_end) = &previous_child_page_end
                        && previous_page_end != child_page_start
                    {
                        self.switch_page_name_at_class_a_boundary(child_page_start.as_deref());
                    }
                    if let box_tree::FormattingBox::AnonymousBlock(box_) = child_box {
                        self.flush_float_run(&mut float_run);
                        self.layout_anonymous_block(&box_.style, &box_.children, stylesheets, None);
                        self.flush_float_run(&mut float_run);
                        trim_block_start_adjoining_margins = false;
                        previous_flow_bottom_margin = None;
                        avoid_run_candidate = None;
                        previous_break_after_avoid = false;
                        if let Some(child_page_start) = effective_child_page_start {
                            previous_child_page_end = Some(child_page_start);
                        } else if let Some((_, child_page_end)) = child_page_value_sources {
                            previous_child_page_end =
                                Some(page_boundary_name_in_parent_scope(child_page_end, style));
                        }
                        child_box_index += 1;
                        continue;
                    }
                    let Some((child_element, child_signature, child_style, child_children)) =
                        child_box.element_parts()
                    else {
                        child_box_index += 1;
                        continue;
                    };
                    let mut child_style = child_style.clone();
                    if self.layout_floating_child(
                        child_element,
                        child_signature.clone(),
                        &child_style,
                        Some(child_children),
                        stylesheets,
                        &mut float_run,
                    ) {
                        seen_flow_child = true;
                        previous_flow_bottom_margin = None;
                        avoid_run_candidate = None;
                        previous_break_after_avoid = false;
                        child_box_index += 1;
                        continue;
                    }
                    self.flush_float_run(&mut float_run);
                    let is_flow_child = is_normal_block_flow_child(child_element, &child_style)
                        || is_document_canvas_element(element)
                        || is_replaced_element(child_element);
                    let descendant_start_margin = (is_flow_child
                        && can_collapse_block_start_margin(
                            &child_style,
                            used_border_widths(&child_style),
                            has_direct_inline_content_box(child_children),
                        ))
                    .then(|| {
                        collapsible_first_child_start_margin_from_boxes(
                            child_children,
                            child_element,
                            &child_style,
                        )
                    })
                    .flatten();
                    let self_collapsing_child = is_flow_child
                        && is_self_collapsing_block_box(
                            child_element,
                            &child_style,
                            child_children,
                        );
                    let self_collapsing_margin_set = self_collapsing_child.then(|| {
                        self_collapsing_block_margin_set_for_box(
                            &child_style,
                            descendant_start_margin,
                        )
                    });
                    let effective_start_margin = self_collapsing_margin_set.unwrap_or_else(|| {
                        descendant_start_margin
                            .map(|descendant| collapse_margins(child_style.margin.top, descendant))
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

                    if is_flow_child {
                        let collapses_with_parent =
                            is_collapsible_block_child(child_element, &child_style);
                        let collapses_with_sibling =
                            is_sibling_margin_collapsible_block_child(child_element, &child_style);
                        if !trimmed_block_start_margin
                            && !seen_flow_child
                            && can_collapse_start_margin
                            && collapses_with_parent
                        {
                            child_style.margin.top = collapsed_start_margin_delta(
                                applied_start_margin,
                                effective_start_margin,
                                starts_at_page_top,
                            ) - descendant_margin_adjustment;
                        } else if !trimmed_block_start_margin
                            && collapses_with_sibling
                            && let Some(previous_margin) = previous_flow_bottom_margin
                        {
                            child_style.margin.top =
                                collapsed_margin_delta(previous_margin, effective_start_margin)
                                    - descendant_margin_adjustment;
                        }

                        if collapses_with_parent
                            && can_collapse_end_margin
                            && !has_later_normal_block_flow_box_child(
                                child_boxes,
                                child_box_index + 1,
                                element,
                            )
                        {
                            child_style.margin.bottom =
                                collapse_margins(child_style.margin.bottom, style.margin.bottom);
                            collapsed_end_margin = true;
                        }
                    }

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
                        Some(child_children),
                    );
                    let avoid_boundary =
                        previous_break_after_avoid || child_style.break_before.avoids_page();
                    let run_start_candidate = if avoid_boundary {
                        avoid_run_candidate
                            .clone()
                            .unwrap_or_else(|| child_start_candidate.clone())
                    } else {
                        child_start_candidate.clone()
                    };
                    if is_flow_child
                        && avoid_boundary
                        && let Some(child_height) = child_estimated_height
                        && should_move_avoid_break_run_to_next_page(
                            run_start_candidate.height,
                            child_height,
                            self.cursor_y - self.page_bottom(),
                            self.page_area_height(),
                            self.cursor_is_at_page_top(),
                        )
                    {
                        self.restore(run_start_candidate.snapshot);
                        previous_flow_bottom_margin =
                            run_start_candidate.previous_flow_bottom_margin;
                        seen_flow_child = run_start_candidate.seen_flow_child;
                        trim_block_start_adjoining_margins =
                            run_start_candidate.trim_block_start_adjoining_margins;
                        collapsed_end_margin = run_start_candidate.collapsed_end_margin;
                        previous_child_page_end = run_start_candidate.previous_child_page_end;
                        float_run = run_start_candidate.float_run;
                        child_box_index = run_start_candidate.index;
                        avoid_run_candidate = None;
                        previous_break_after_avoid = false;
                        self.push_page_if_nonempty();
                        continue;
                    }

                    if is_flow_child {
                        seen_flow_child = true;
                        if trim_block_start_adjoining_margins && !self_collapsing_child {
                            trim_block_start_adjoining_margins = false;
                        }
                        previous_flow_bottom_margin = if self_collapsing_child {
                            Some(if trimmed_block_start_margin {
                                0.0
                            } else {
                                previous_flow_bottom_margin
                                    .map(|previous| {
                                        collapse_margins(previous, effective_start_margin)
                                    })
                                    .unwrap_or(effective_start_margin)
                            })
                        } else {
                            is_sibling_margin_collapsible_block_child(child_element, &child_style)
                                .then_some(child_style.margin.bottom)
                        };
                    } else {
                        previous_flow_bottom_margin = None;
                    }

                    if child_style.display.is_block_level()
                        || is_document_canvas_element(element)
                        || is_replaced_element(child_element)
                    {
                        self.push_ancestor_signature(child_signature.clone());
                        if zero_height_page_boundary {
                            self.push_page_name_element_scope_suppression();
                        }
                        if let box_tree::FormattingBox::Table(table_box) = child_box {
                            self.layout_element_with_child_boxes_and_table_fragment(
                                table_box.element,
                                &child_style,
                                stylesheets,
                                Some(&table_box.children),
                                Some(&table_box.fragment),
                            );
                        } else if let box_tree::FormattingBox::Block(block_box) = child_box {
                            self.layout_element_with_child_boxes_and_run_ins(
                                child_element,
                                &child_style,
                                stylesheets,
                                &block_box.run_in_children,
                                Some(child_children),
                            );
                        } else {
                            self.layout_element_with_child_boxes(
                                child_element,
                                &child_style,
                                stylesheets,
                                Some(child_children),
                            );
                        }
                        if zero_height_page_boundary {
                            self.pop_page_name_element_scope_suppression();
                        }
                        self.ancestors.pop();
                        self.flush_float_run(&mut float_run);
                    }
                    if let Some(child_page_start) = effective_child_page_start {
                        previous_child_page_end = Some(child_page_start);
                    } else if let Some((_, child_page_end)) = child_page_value_sources {
                        previous_child_page_end =
                            Some(page_boundary_name_in_parent_scope(child_page_end, style));
                    }
                    avoid_run_candidate = if is_flow_child {
                        child_estimated_height.map(|child_height| {
                            run_start_candidate
                                .with_height(run_start_candidate.height + child_height)
                        })
                    } else {
                        None
                    };
                    previous_break_after_avoid =
                        is_flow_child && child_style.break_after.avoids_page();
                    child_box_index += 1;
                }
                self.flush_float_run(&mut float_run);
            } else {
                let sibling_tags = element_sibling_tags(element);
                let mut element_index = 0usize;
                let mut float_run = self.float_run_state();
                let mut avoid_run_candidate: Option<AvoidBreakRunCandidate> = None;
                let mut previous_break_after_avoid = false;
                let mut child_node_index = 0usize;
                while child_node_index < element.children.len() {
                    let child = &element.children[child_node_index];
                    let NodeKind::Element(child_element) = &child.kind else {
                        child_node_index += 1;
                        continue;
                    };
                    let child_start_candidate = AvoidBreakRunCandidate {
                        snapshot: self.snapshot(),
                        index: child_node_index,
                        element_index,
                        previous_flow_bottom_margin,
                        seen_flow_child,
                        trim_block_start_adjoining_margins,
                        collapsed_end_margin,
                        previous_child_page_end: None,
                        float_run,
                        height: 0.0,
                    };
                    let child_signature = ElementSignature::with_siblings(
                        child_element.tag.clone(),
                        child_element.attrs.clone(),
                        element_index,
                        sibling_tags.clone(),
                    );
                    element_index += 1;
                    let mut child_style = style_for_layout_element(
                        child_element,
                        child_signature.clone(),
                        stylesheets,
                        Some(style),
                        &self.ancestors,
                    );
                    if self.layout_floating_child(
                        child_element,
                        child_signature.clone(),
                        &child_style,
                        None,
                        stylesheets,
                        &mut float_run,
                    ) {
                        seen_flow_child = true;
                        previous_flow_bottom_margin = None;
                        avoid_run_candidate = None;
                        previous_break_after_avoid = false;
                        child_node_index += 1;
                        continue;
                    }
                    self.flush_float_run(&mut float_run);
                    let mut child_ancestors = self.ancestors.clone();
                    child_ancestors.push(child_signature.clone());
                    let is_flow_child = is_normal_block_flow_child(child_element, &child_style)
                        || is_document_canvas_element(element)
                        || is_replaced_element(child_element);
                    let descendant_start_margin = (is_flow_child
                        && can_collapse_block_start_margin(
                            &child_style,
                            used_border_widths(&child_style),
                            has_direct_inline_content_dom(
                                child_element,
                                &child_style,
                                stylesheets,
                                &child_ancestors,
                            ),
                        ))
                    .then(|| {
                        collapsible_first_child_start_margin_dom(
                            child_element,
                            &child_style,
                            stylesheets,
                            &child_ancestors,
                        )
                    })
                    .flatten();
                    let self_collapsing_child = is_flow_child
                        && is_self_collapsing_block_dom(
                            child_element,
                            &child_style,
                            stylesheets,
                            &child_ancestors,
                        );
                    let self_collapsing_margin_set = self_collapsing_child.then(|| {
                        self_collapsing_block_margin_set_for_box(
                            &child_style,
                            descendant_start_margin,
                        )
                    });
                    let effective_start_margin = self_collapsing_margin_set.unwrap_or_else(|| {
                        descendant_start_margin
                            .map(|descendant| collapse_margins(child_style.margin.top, descendant))
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

                    if is_flow_child {
                        let collapses_with_parent =
                            is_collapsible_block_child(child_element, &child_style);
                        let collapses_with_sibling =
                            is_sibling_margin_collapsible_block_child(child_element, &child_style);
                        if !trimmed_block_start_margin
                            && !seen_flow_child
                            && can_collapse_start_margin
                            && collapses_with_parent
                        {
                            child_style.margin.top = collapsed_start_margin_delta(
                                applied_start_margin,
                                effective_start_margin,
                                starts_at_page_top,
                            ) - descendant_margin_adjustment;
                        } else if !trimmed_block_start_margin
                            && collapses_with_sibling
                            && let Some(previous_margin) = previous_flow_bottom_margin
                        {
                            child_style.margin.top =
                                collapsed_margin_delta(previous_margin, effective_start_margin)
                                    - descendant_margin_adjustment;
                        }

                        if collapses_with_parent
                            && can_collapse_end_margin
                            && !has_later_normal_block_flow_child(
                                element,
                                element_index,
                                &sibling_tags,
                                style,
                                stylesheets,
                                &self.ancestors,
                            )
                        {
                            child_style.margin.bottom =
                                collapse_margins(child_style.margin.bottom, style.margin.bottom);
                            collapsed_end_margin = true;
                        }
                    }

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
                    let avoid_boundary =
                        previous_break_after_avoid || child_style.break_before.avoids_page();
                    let run_start_candidate = if avoid_boundary {
                        avoid_run_candidate
                            .clone()
                            .unwrap_or_else(|| child_start_candidate.clone())
                    } else {
                        child_start_candidate.clone()
                    };
                    if is_flow_child
                        && avoid_boundary
                        && let Some(child_height) = child_estimated_height
                        && should_move_avoid_break_run_to_next_page(
                            run_start_candidate.height,
                            child_height,
                            self.cursor_y - self.page_bottom(),
                            self.page_area_height(),
                            self.cursor_is_at_page_top(),
                        )
                    {
                        self.restore(run_start_candidate.snapshot);
                        element_index = run_start_candidate.element_index;
                        previous_flow_bottom_margin =
                            run_start_candidate.previous_flow_bottom_margin;
                        seen_flow_child = run_start_candidate.seen_flow_child;
                        trim_block_start_adjoining_margins =
                            run_start_candidate.trim_block_start_adjoining_margins;
                        collapsed_end_margin = run_start_candidate.collapsed_end_margin;
                        float_run = run_start_candidate.float_run;
                        child_node_index = run_start_candidate.index;
                        avoid_run_candidate = None;
                        previous_break_after_avoid = false;
                        self.push_page_if_nonempty();
                        continue;
                    }

                    if is_flow_child {
                        seen_flow_child = true;
                        if trim_block_start_adjoining_margins && !self_collapsing_child {
                            trim_block_start_adjoining_margins = false;
                        }
                        previous_flow_bottom_margin = if self_collapsing_child {
                            Some(if trimmed_block_start_margin {
                                0.0
                            } else {
                                previous_flow_bottom_margin
                                    .map(|previous| {
                                        collapse_margins(previous, effective_start_margin)
                                    })
                                    .unwrap_or(effective_start_margin)
                            })
                        } else {
                            is_sibling_margin_collapsible_block_child(child_element, &child_style)
                                .then_some(child_style.margin.bottom)
                        };
                    } else {
                        previous_flow_bottom_margin = None;
                    }

                    if child_style.display.is_block_level()
                        || is_document_canvas_element(element)
                        || is_replaced_element(child_element)
                    {
                        self.push_ancestor_signature(child_signature);
                        self.layout_element(child_element, &child_style, stylesheets);
                        self.ancestors.pop();
                        self.flush_float_run(&mut float_run);
                    }
                    avoid_run_candidate = if is_flow_child {
                        child_estimated_height.map(|child_height| {
                            run_start_candidate
                                .with_height(run_start_candidate.height + child_height)
                        })
                    } else {
                        None
                    };
                    previous_break_after_avoid =
                        is_flow_child && child_style.break_after.avoids_page();
                    child_node_index += 1;
                }
                self.flush_float_run(&mut float_run);
            }
        }
        self.definite_block_size_stack.pop();

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
        let border_box = geometry.border_box_top_rect(outer_x, block_top, block_height);
        let border_paint_rect = border_box.page_top_rect().paint_rect();
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
            if background_page_index == paint_page_index {
                self.current_page.prepend_recorded_primitives_to_fragment(
                    &mut fragment,
                    PaintBand::BackgroundBorder,
                    own_background_primitives.clone(),
                );
                self.current_page.append_recorded_primitives_to_fragment(
                    &mut fragment,
                    PaintBand::Outline,
                    own_outline_primitives.clone(),
                );
            }
            if (has_own_background_primitives || has_own_outline_primitives) && !fragment.is_empty()
            {
                let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
                    .with_source_order(self.next_paint_source_order());
                self.current_page.replace_paint_tree_since_with_context(
                    &paint_checkpoint,
                    PaintBand::InFlowBlock,
                    context,
                );
            }
            if !collapsed_end_margin {
                self.cursor_y -= style.margin.bottom;
            }
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
            if page_index == background_page_index {
                fragment.prepend_primitives_in_band(
                    PaintBand::BackgroundBorder,
                    own_background_primitives.clone(),
                );
                fragment
                    .append_primitives_in_band(PaintBand::Outline, own_outline_primitives.clone());
            }
            if fragment.is_empty() {
                continue;
            }
            let fragment = if has_own_background_primitives || has_own_outline_primitives {
                let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
                    .with_source_order(self.next_paint_source_order());
                PaintFragment::from_stacking_context_in_band(PaintBand::InFlowBlock, context)
            } else {
                fragment
            };
            if page_index < self.pages.len() {
                self.pages[page_index].append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
            } else {
                self.current_page
                    .append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
            }
        }
        if !collapsed_end_margin {
            self.cursor_y -= style.margin.bottom;
        }
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y;
        }
        self.apply_forced_break(style.break_after);
    }

    fn translate_aligned_block_descendant_bookmarks(
        &mut self,
        descendant_bookmark_start: usize,
        page_index: usize,
        x_offset: f32,
        y_offset: f32,
    ) {
        if x_offset.abs() <= 0.01 && y_offset.abs() <= 0.01 {
            return;
        }
        for bookmark in self.bookmarks.iter_mut().skip(descendant_bookmark_start) {
            if bookmark.page_index == page_index {
                bookmark.translate_target(x_offset, y_offset);
            }
        }
    }

    /// Resolve a block box's used content width, including intrinsic keywords.
    ///
    /// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` as
    /// intrinsic sizing keywords for the `width` property. Normal block
    /// width-auto handling still follows CSS 2.2, but intrinsic keywords need
    /// the box contents before they can be converted to a used content width:
    /// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
    pub(super) fn used_block_content_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
        horizontal_extras: f32,
    ) -> f32 {
        let needs_intrinsic = matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        );
        if !needs_intrinsic {
            return used_content_width(style, available_outer_width, horizontal_extras);
        }

        let (min_content, max_content) = self.block_intrinsic_content_widths(
            element,
            style,
            stylesheets,
            child_boxes,
            available_outer_width,
        );
        intrinsic::content_width_from_intrinsic(
            style,
            available_outer_width,
            horizontal_extras,
            min_content,
            max_content,
            intrinsic::IntrinsicAutoWidth::FillAvailable,
        )
    }

    /// Estimate block min-content and max-content content-box inline sizes.
    ///
    /// CSS Sizing computes intrinsic contributions from text soft-wrap
    /// opportunities and descendant intrinsic widths. This helper covers the
    /// normal block text paths used by block layout and falls back to the
    /// existing shrink-to-fit estimator for non-inline descendants until block
    /// intrinsic sizing is fully structured across every formatting context:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic>.
    pub(in crate::layout) fn block_intrinsic_content_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        if style.display.is_flex() {
            return self.estimate_flex_intrinsic_widths(
                element,
                style,
                stylesheets,
                available_outer_width,
                child_boxes,
            );
        }
        let contribution = self.intrinsic_inline_contribution_for_element(
            element,
            style,
            stylesheets,
            child_boxes,
        );
        if contribution.max_content > 0.0 || contribution.min_content > 0.0 {
            return (contribution.min_content, contribution.max_content);
        }
        let shrink_to_fit = self.estimate_shrink_to_fit_width(
            element,
            style,
            stylesheets,
            available_outer_width,
            child_boxes,
            None,
        );
        (shrink_to_fit, shrink_to_fit)
    }

    fn block_layout_geometry(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> BlockLayoutGeometry {
        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, containing_inline_size);
        let relative_offset =
            relative_position_offset(&used_style, self.current_containing_block());
        let available_outer_width = self.content_right
            - self.content_left
            - used_style.margin.left
            - used_style.margin.right;
        let border_widths = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content();
        let vertical_extras = box_metrics.vertical_non_content();
        let requested_content_width = self.used_block_content_width(
            element,
            &used_style,
            stylesheets,
            child_boxes,
            available_outer_width,
            horizontal_extras,
        );
        let content_width =
            constrain_width(&used_style, requested_content_width, available_outer_width);
        let containing_block_content_height =
            self.definite_block_size_stack.last().copied().flatten();
        let definite_content_height = used_content_height_or_auto_with_optional_basis(
            &used_style,
            containing_block_content_height,
            vertical_extras,
        )
        .map(|height| constrain_height(&used_style, height, content_width));
        let outer_width = (content_width + horizontal_extras)
            .min(available_outer_width)
            .max(0.0);
        resolve_normal_flow_block_auto_margins(
            &mut used_style,
            containing_inline_size,
            outer_width,
            self.containing_block_direction,
        );
        let outer_x = normal_flow_block_outer_x(
            self.content_left,
            self.content_right,
            &used_style,
            outer_width,
            self.containing_block_direction,
        ) + relative_offset.x;
        let inner_x = outer_x + border_widths.left + used_style.padding.left;

        BlockLayoutGeometry {
            style: used_style,
            relative_offset,
            border_widths,
            vertical_extras,
            definite_content_height,
            outer_inline: BlockInlineBounds::new(outer_x, outer_width),
            content_inline: BlockInlineBounds::new(inner_x, content_width),
        }
    }
}

/// Returns whether block `align-content` needs descendant paint bounds.
///
/// Horizontal block containers know their alignment-subject block size from
/// normal-flow layout height. In vertical writing modes the block axis is
/// physical horizontal, so same-page alignment uses captured descendant paint
/// bounds as the concrete alignment subject:
/// <https://www.w3.org/TR/css-align-3/#align-content-property> and
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
fn vertical_block_align_content_needs_fragment_bounds(style: &ComputedStyle) -> bool {
    style.writing_mode != WritingMode::HorizontalTb
        && style.align_content.keyword != ContentAlignmentKeyword::Normal
}

fn vertical_block_align_content_x_offset(
    style: &ComputedStyle,
    content_left: f32,
    content_width: f32,
    subject_bounds: Option<PaintClip>,
) -> f32 {
    if !vertical_block_align_content_needs_fragment_bounds(style) {
        return 0.0;
    }
    let Some(subject_bounds) = subject_bounds else {
        return 0.0;
    };
    let subject_width = subject_bounds.width().max(0.0);
    let free_space = content_width.max(0.0) - subject_width;
    let toward_block_end = content_alignment_offset_toward_end(
        style.align_content,
        free_space,
        block_align_content_defaults_to_safe_overflow(style),
    );
    match block_start_side(style.writing_mode) {
        PhysicalSide::Left => content_left + toward_block_end - subject_bounds.x(),
        PhysicalSide::Right => {
            content_left + content_width.max(0.0)
                - toward_block_end
                - (subject_bounds.x() + subject_width)
        }
        PhysicalSide::Top | PhysicalSide::Bottom => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_align_content_offset_uses_single_subject_fallbacks() {
        assert_eq!(block_align_content_y_offset(AlignContent::End, 30.0), -30.0);
        assert_eq!(
            block_align_content_y_offset(AlignContent::SpaceAround, 30.0),
            -15.0
        );
        assert_eq!(
            block_align_content_y_offset(
                AlignContent::safe(ContentAlignmentKeyword::Center),
                -20.0,
            ),
            0.0
        );
        assert_eq!(
            block_align_content_y_offset(
                AlignContent::unsafe_position(ContentAlignmentKeyword::Center),
                -20.0,
            ),
            10.0
        );
        assert_eq!(
            block_align_content_y_offset(AlignContent::LastBaseline, -20.0),
            0.0
        );
        let mut scroll_container_style = ComputedStyle::initial();
        scroll_container_style.align_content = AlignContent::Center;
        scroll_container_style.overflow_y = css::Overflow::Auto;
        assert_eq!(
            block_align_content_y_offset_for_style(&scroll_container_style, -20.0),
            10.0
        );
        assert!(
            block_align_content_establishes_independent_formatting_context(AlignContent::Center)
        );
        assert!(
            !block_align_content_establishes_independent_formatting_context(AlignContent::Normal)
        );
    }

    #[test]
    fn vertical_block_align_content_offsets_use_logical_block_axis() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.align_content = AlignContent::Center;
        let subject = PaintClip::from_paint_rect(paint_space_rect(10.0, 20.0, 20.0, 40.0));
        assert_eq!(
            vertical_block_align_content_x_offset(&style, 10.0, 80.0, Some(subject)),
            30.0
        );

        style.align_content = AlignContent::End;
        assert_eq!(
            vertical_block_align_content_x_offset(&style, 10.0, 80.0, Some(subject)),
            60.0
        );

        style.writing_mode = WritingMode::VerticalRl;
        assert_eq!(
            vertical_block_align_content_x_offset(&style, 10.0, 80.0, Some(subject)),
            0.0
        );
    }

    #[test]
    fn block_border_box_projects_top_edge_to_paint_space() {
        let border_box = BlockBorderBox::new(12.0, 90.0, 40.0, 25.0);
        let page_top_rect = border_box.page_top_rect();
        assert_eq!(page_top_rect.bottom_y(), 65.0);
        assert_eq!(
            page_top_rect.paint_rect(),
            paint_space_rect(12.0, 65.0, 40.0, 25.0)
        );
    }
}

/// Horizontal and size inputs for one normal-flow block box.
///
/// CSS 2.2 block formatting computes inline-size, margins, padding, and
/// relative-position offsets before child layout determines the final block
/// extent. This struct therefore stores the pre-layout physical inline
/// geometry and exposes typed page-space helpers once a block top and used
/// content height are known:
/// <https://www.w3.org/TR/CSS22/visuren.html#block-formatting> and
/// <https://www.w3.org/TR/CSS22/box.html>.
struct BlockLayoutGeometry {
    style: ComputedStyle,
    relative_offset: RelativeOffset,
    border_widths: css::Edges,
    vertical_extras: f32,
    definite_content_height: Option<f32>,
    outer_inline: BlockInlineBounds,
    content_inline: BlockInlineBounds,
}

impl BlockLayoutGeometry {
    fn outer_inline(&self) -> BlockInlineBounds {
        self.outer_inline
    }

    fn content_inline(&self) -> BlockInlineBounds {
        self.content_inline
    }

    fn outer_width(&self) -> f32 {
        self.outer_inline.size()
    }

    fn content_width(&self) -> f32 {
        self.content_inline.size()
    }

    /// Return the final block border box in block formatting coordinates.
    ///
    /// CSS Box defines the border box as the outer painted box excluding
    /// margins. Block layout knows the top edge before descendants are laid out
    /// and the final block size afterward, so this is the point where Quire can
    /// form a typed block-layout rectangle:
    /// <https://www.w3.org/TR/CSS22/box.html#box-dimensions>.
    fn border_box_top_rect(
        &self,
        outer_x: f32,
        block_top: f32,
        block_height: f32,
    ) -> BlockBorderBox {
        BlockBorderBox::new(outer_x, block_top, self.outer_width(), block_height)
    }

    /// Return the block padding box as a top-edge page rectangle.
    ///
    /// CSS Positioned Layout uses the padding box of positioned ancestors as
    /// the containing block for absolute descendants:
    /// <https://www.w3.org/TR/css-position-3/#def-cb>.
    fn padding_box_top_rect(
        &self,
        outer_x: f32,
        block_top: f32,
        content_height: f32,
    ) -> PageTopRect {
        PageTopRect::new(
            outer_x + self.border_widths.left,
            block_top - self.border_widths.top,
            self.content_width() + self.style.padding.left + self.style.padding.right,
            content_height + self.style.padding.top + self.style.padding.bottom,
        )
    }
}

/// Physical inline-axis bounds for a block formatting box.
///
/// CSS normal-flow block layout resolves the used inline size and physical
/// inline-start offset before child layout determines the block-axis extent.
/// This wrapper keeps those values labelled as block formatting coordinates
/// instead of carrying unqualified `x` and `width` scalars through layout:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BlockInlineBounds {
    span: PageInlineSpan,
}

impl BlockInlineBounds {
    fn new(start: f32, size: f32) -> Self {
        Self {
            span: PageInlineSpan::new(start, size),
        }
    }

    fn start(self) -> f32 {
        self.span.left_x()
    }

    fn size(self) -> f32 {
        self.span.width()
    }
}

/// A CSS block border box in block formatting coordinates.
///
/// The origin is the physical top-left border edge used by CSS 2.2 normal-flow
/// block layout, and the block extent grows downward. This is intentionally a
/// block-layout type, not a paint-space rectangle; callers must project through
/// [`page_top_rect`](Self::page_top_rect) before creating paint or PDF data:
/// <https://www.w3.org/TR/CSS22/box.html#box-dimensions> and
/// <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BlockBorderBox {
    rect: BlockRect,
}

impl BlockBorderBox {
    fn new(x: f32, top_y: f32, width: f32, height: f32) -> Self {
        Self {
            rect: BlockRect::new(
                BlockPoint::new(x, top_y),
                BlockSize::new(width.max(0.0), height.max(0.0)),
            ),
        }
    }

    fn x(self) -> f32 {
        self.rect.origin.x
    }

    fn top_y(self) -> f32 {
        self.rect.origin.y
    }

    fn width(self) -> f32 {
        self.rect.size.width
    }

    fn height(self) -> f32 {
        self.rect.size.height
    }

    fn page_top_rect(self) -> PageTopRect {
        PageTopRect::new(self.x(), self.top_y(), self.width(), self.height())
    }
}

/// Inputs for deciding whether a definite-height block should prebreak.
///
/// CSS Fragmentation allows class A breaks between sibling block boxes before
/// layout. Keeping those inputs together makes the decision explicit while
/// allowing avoid-retry pagination state to tailor the rule:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
struct DefiniteBlockBreakContext<'a> {
    definite_content_height: Option<f32>,
    vertical_extras: f32,
    style: &'a ComputedStyle,
    remaining_height: f32,
    page_area_height: f32,
    current_page_has_content: bool,
    at_page_top: bool,
    suppress_for_avoid_retry: bool,
}

#[derive(Clone)]
struct AvoidBreakRunCandidate {
    snapshot: LayoutSnapshot,
    index: usize,
    element_index: usize,
    previous_flow_bottom_margin: Option<f32>,
    seen_flow_child: bool,
    trim_block_start_adjoining_margins: bool,
    collapsed_end_margin: bool,
    previous_child_page_end: Option<Option<String>>,
    float_run: FloatRunState,
    height: f32,
}

impl AvoidBreakRunCandidate {
    fn with_height(&self, height: f32) -> Self {
        let mut candidate = self.clone();
        candidate.height = height;
        candidate
    }
}

fn should_move_avoid_break_run_to_next_page(
    run_height: f32,
    next_height: f32,
    remaining_height: f32,
    page_area_height: f32,
    at_page_top: bool,
) -> bool {
    !at_page_top
        && next_height > remaining_height + 0.01
        && run_height + next_height <= page_area_height + 0.01
}

/// Returns whether a definite-height normal-flow block should start a new page.
///
/// CSS Fragmentation allows breaks between sibling block boxes. When a block's
/// used border-box height is definite and it fits in an empty page area but not
/// in the remaining fragmentainer space, laying it out after a class A break
/// keeps its own background, border, and descendants in the next page
/// coordinate space:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks> and
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>.
fn should_prebreak_definite_block(context: DefiniteBlockBreakContext<'_>) -> bool {
    if !context.current_page_has_content || context.at_page_top {
        return false;
    }
    let Some(content_height) = context.definite_content_height else {
        return false;
    };
    let block_height = context.style.margin.top
        + context.vertical_extras
        + content_height.max(0.0)
        + context.style.margin.bottom;
    if context.suppress_for_avoid_retry && block_height <= context.page_area_height + 0.01 {
        return false;
    }
    block_height > context.remaining_height + 0.01
        && block_height <= context.page_area_height + 0.01
}
