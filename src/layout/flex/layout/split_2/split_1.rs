use super::*;

impl<'a> LayoutBuilder<'a> {
    /// Resolve a flex container's used content width, including intrinsic keywords.
    ///
    /// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` as
    /// width values that resolve from the box's intrinsic contributions. CSS
    /// Flexbox defines those contributions for flex containers separately from
    /// normal block flow, so flex layout must not fall back to CSS 2.2
    /// auto-width filling when the author supplied an intrinsic keyword:
    /// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
    pub(in crate::layout::flex) fn used_flex_container_content_width(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        horizontal_extras: f32,
    ) -> f32 {
        let orthogonal_auto_width = orthogonal_auto_width_flex_container_needs_intrinsic(
            style,
            self.current_child_available_space(),
        );
        let needs_intrinsic = matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        ) || (style.float != Float::None && style.box_values.width.is_auto())
            || orthogonal_auto_width;

        if !needs_intrinsic {
            return used_content_width(style, available_outer_width, horizontal_extras);
        }

        let intrinsic = self.estimate_intrinsic_flex_container_size(
            children,
            style,
            stylesheets,
            FlexAvailableSpace {
                width: available_outer_width.max(0.0),
                width_is_definite: used_content_width_or_auto(
                    style,
                    available_outer_width.max(0.0),
                    horizontal_extras,
                )
                .is_some(),
                height: used_length_percentage_or_auto(
                    style.box_values.height,
                    available_outer_width.max(0.0),
                ),
                height_is_definite: !style.box_values.height.is_auto(),
            },
        );

        flex_container_content_width_from_intrinsic(
            style,
            available_outer_width,
            horizontal_extras,
            intrinsic,
            style.float != Float::None || orthogonal_auto_width,
        )
    }

    pub(crate) fn layout_flex(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
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
        let source_style = style;
        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, containing_inline_size);

        let relative_offset =
            relative_position_offset(&used_style, self.current_containing_block());
        if matches!(used_style.position, Position::Relative | Position::Sticky) {
            self.cursor_y += relative_offset.y;
        }

        let available_outer_width = self.content_right
            - self.content_left
            - used_style.margin.left
            - used_style.margin.right;
        let border_widths = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content();
        let vertical_extras = box_metrics.vertical_non_content();

        let built_child_boxes;
        let child_boxes = if let Some(child_boxes) = child_boxes {
            child_boxes
        } else {
            built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                element,
                stylesheets,
                &used_style,
            );
            &built_child_boxes
        };
        let container_signature = self.flex_container_signature(element);
        let (mut children, mut positioned_children) =
            flex_child_lists_from_boxes(element, &container_signature, &used_style, child_boxes);
        self.resolve_styled_children_viewport_lengths(&mut children);
        self.resolve_styled_children_viewport_lengths(&mut positioned_children);

        let requested_content_width = self.used_flex_container_content_width(
            &children,
            &used_style,
            stylesheets,
            available_outer_width,
            horizontal_extras,
        );
        let content_width =
            constrain_width(&used_style, requested_content_width, available_outer_width);
        let outer_width = (content_width + horizontal_extras).max(0.0);
        if used_style.float == Float::None {
            resolve_normal_flow_block_auto_margins(
                &mut used_style,
                containing_inline_size,
                outer_width,
                self.containing_block_direction,
            );
        }
        let style = &used_style;
        let mut outer_x = normal_flow_block_outer_x(
            self.content_left,
            self.content_right,
            style,
            outer_width,
            self.containing_block_direction,
        ) + relative_offset.x;
        let mut inner_x = outer_x + border_widths.left + style.padding.left;
        let inner_width = content_width.max(0.0);
        let available_outer_height =
            (self.cursor_y - self.page_bottom() - style.margin.top - style.margin.bottom).max(0.0);
        let explicit_content_height =
            used_content_height_or_auto(style, available_outer_height, vertical_extras)
                .map(|height| constrain_height(style, height, available_outer_height));
        let definite_content_height = definite_flex_container_content_height(
            style,
            explicit_content_height,
            content_width,
            available_outer_height,
            horizontal_extras,
            vertical_extras,
        );
        let has_definite_content_height = definite_content_height.is_some();
        let flex_available_content_height =
            flex_available_content_height(style, definite_content_height, content_width);

        self.cursor_y -= style.margin.top;
        if style.float == Float::None {
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
            outer_x = margin_box_left + style.margin.left + relative_offset.x;
            inner_x = outer_x + border_widths.left + style.padding.left;
        } else {
            self.cursor_y = self.clear_active_floats_top(
                style.clear,
                style.writing_mode,
                style.direction,
                self.cursor_y,
            );
        }
        let block_top = self.cursor_y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        self.cursor_y -= border_widths.top + style.padding.top;

        let Some(mut flex_layout) = self.compute_flex_layout(
            &children,
            style,
            stylesheets,
            FlexAvailableSpace {
                width: inner_width,
                width_is_definite: true,
                height: flex_available_content_height,
                height_is_definite: has_definite_content_height,
            },
        ) else {
            let mut flow_style = style.clone();
            flow_style.display = Display::BLOCK;
            flow_style.margin = css::Edges::ZERO;
            self.layout_block(element, &flow_style, stylesheets, &[], Some(child_boxes));
            return;
        };

        let flex_break_units = flex_break_units(&flex_layout, &children, style);
        let flex_content_height = flex_layout.height;
        let total_content_height = constrain_height(style, flex_content_height, content_width);
        debug_assert!(!flex_layout.lines.is_empty() || children.is_empty());
        debug_assert!(
            flex_layout.fragment_plan.is_empty()
                || flex_layout.fragment_plan.planned_item_fragment_count()
                    <= flex_layout.items.len()
        );
        let total_height = border_widths.top
            + style.padding.top
            + total_content_height
            + style.padding.bottom
            + border_widths.bottom;
        let flex_has_forced_item_breaks = children.iter().any(|child| {
            child.style.break_before.is_forced() || child.style.break_after.is_forced()
        });
        if !flex_has_forced_item_breaks
            && should_move_flex_container_to_next_page(
                block_top,
                total_height,
                self.page_top(),
                self.page_bottom(),
                self.page_area_height(),
            )
        {
            self.push_page();
            self.layout_flex(element, source_style, stylesheets, Some(child_boxes));
            return;
        }
        let defer_own_decoration_promotion = self.defer_next_block_decoration_promotion;
        self.defer_next_block_decoration_promotion = false;
        let content_top = self.cursor_y;
        let flex_overflows_current_page = block_top - total_height < self.page_bottom() - 0.01;
        let flex_fragmentation_enabled = !self.preserve_scoped_paint_public_order
            && !is_document_canvas_element(element)
            && (flex_overflows_current_page || flex_has_forced_item_breaks);
        if flex_fragmentation_enabled {
            self.fragment_top_offsets
                .push(self.current_page_context.top() - content_top);
        }
        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            self.containing_blocks
                .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                    outer_x + border_widths.left,
                    block_top - border_widths.top,
                    content_width + style.padding.left + style.padding.right,
                    total_content_height + style.padding.top + style.padding.bottom,
                )));
        }

        let overflow_clip_active = if used_overflow_clips_element(element, style) {
            self.push_padding_box_overflow_clip(
                style,
                outer_x,
                block_top,
                border_widths,
                content_width,
                total_content_height,
            )
        } else {
            false
        };

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        if flex_fragmentation_enabled {
            self.content_left = inner_x;
            self.content_right = inner_x + inner_width;
        }
        self.push_float_context();
        flex_layout.fragment_plan.fragments.clear();
        let mut fragment_content_top = content_top;
        let mut fragment_block_offset = 0.0f32;
        let mut pending_break_before = PageBreak::Auto;
        let mut previous_break_after_avoid = false;
        let mut forced_break_after_flex_items = PageBreak::Auto;
        for unit in &flex_break_units {
            let break_before = if pending_break_before != PageBreak::Auto {
                pending_break_before
            } else {
                unit.break_before
            };
            let break_is_applicable = flex_fragmentation_enabled;
            if break_is_applicable && break_before.is_forced() {
                self.apply_forced_break(break_before);
                fragment_content_top = self.cursor_y;
                fragment_block_offset = unit.block_start;
            }
            let placed_unit_bottom =
                fragment_content_top - (unit.block_end - fragment_block_offset);
            let unit_overflows = placed_unit_bottom < self.page_bottom() - 0.01;
            let avoid_break =
                unit.break_inside_avoid || previous_break_after_avoid || break_before.avoids_page();
            let unit_is_oversized = unit.block_size() > self.page_area_height() + 0.01;
            if break_is_applicable
                && (unit_overflows || avoid_break)
                && (!self.cursor_is_at_page_top() || self.current_page_has_content())
            {
                self.push_page();
                fragment_content_top = self.cursor_y;
                fragment_block_offset = unit.block_start;
            }

            let mut slice_start = unit.block_start;
            loop {
                let available_block_end = if break_is_applicable {
                    fragment_block_offset + (fragment_content_top - self.page_bottom()).max(0.0)
                } else {
                    unit.block_end
                };
                let slice_end = if break_is_applicable
                    && unit_is_oversized
                    && unit.block_end > available_block_end + 0.01
                {
                    available_block_end.min(unit.block_end).max(slice_start)
                } else {
                    unit.block_end
                };
                if break_is_applicable
                    && slice_end <= slice_start + 0.01
                    && unit.block_end > slice_start + 0.01
                {
                    self.push_page();
                    fragment_content_top = self.cursor_y;
                    fragment_block_offset = slice_start;
                    continue;
                }
                let slice_unit = unit.slice(slice_start, slice_end);
                let fragment_context = FlexFragmentBuildContext {
                    page_index: self.pages.len(),
                    outer_x,
                    outer_width,
                    content_top: fragment_content_top,
                    block_offset: fragment_block_offset,
                    starts_page_fragment: !self.current_page_has_content(),
                };
                let mut planned_fragment = flex_fragment_from_break_unit(
                    &slice_unit,
                    &flex_layout.items,
                    fragment_context,
                );
                if flex_fragmentation_enabled
                    && style.visibility == Visibility::Visible
                    && let Some(fragment_bounds) = planned_fragment.metadata.source_border_box
                    && (style.background_color.is_some()
                        || style.background_image.is_some()
                        || style.border_image.source.is_some()
                        || used_border_width(style) > 0.0)
                {
                    let mut background_fragment =
                        PaintFragment::from_primitives(Vec::new(), Vec::new());
                    background_fragment.prepend_primitives_in_band(
                        PaintBand::BackgroundBorder,
                        self.box_background_primitives(
                            outer_x,
                            fragment_bounds.y(),
                            outer_width,
                            fragment_bounds.height(),
                            style,
                        ),
                    );
                    self.current_page
                        .append_paint_fragment(&background_fragment, PaintVector::new(0.0, 0.0));
                }
                for item_fragment in &mut planned_fragment.items {
                    let index = item_fragment.item_index;
                    let child = &children[index];
                    if flex_item_is_collapsed(&child.style) {
                        continue;
                    }
                    let item = &item_fragment.bounds;
                    let original_item = &item_fragment.original_bounds;
                    let item_width = original_item.width().max(0.0);
                    let item_x = original_item.x();
                    let item_y = original_item.y();
                    let item_height = original_item.height().max(0.0);
                    let visible_item_height = item.height().max(0.0);
                    let item_content_left = inner_x + item_x;
                    let item_cursor_y = fragment_content_top - (item_y - fragment_block_offset);

                    let item_page_index = self.pages.len();
                    let item_starts_page_fragment = !self.current_page_has_content();
                    let visible_item_top =
                        fragment_content_top - (item.y() - fragment_block_offset);
                    let item_border_box = PageTopRect::new(
                        item_content_left,
                        visible_item_top,
                        item_width,
                        visible_item_height,
                    )
                    .paint_clip();
                    let mut item_metadata = FragmentPageMetadata::new(
                        item_page_index,
                        Some(item_border_box),
                        item_starts_page_fragment,
                    );
                    item_metadata.continues_from_previous_page =
                        item_fragment.content_slice.block_start > 0.01;
                    item_metadata.continues_to_next_page =
                        item_fragment.content_slice.block_end < item_height - 0.01;
                    let item_paint_checkpoint = self.current_page.paint_checkpoint();
                    let item_positioned_layer_start = self.positioned_layers.len();
                    let item_is_split = item_metadata.continues_from_previous_page
                        || item_metadata.continues_to_next_page;

                    let placed_style = placed_flex_item_style(
                        &child.style,
                        item_width,
                        item_height,
                        style.flex_direction,
                    );

                    let item_was_split = self.with_formatting_context_item_placement(
                        FormattingContextItemPlacement {
                            content_left: item_content_left,
                            content_width: item_width,
                            cursor_y: item_cursor_y,
                            page_start_margin_policy: PageStartMarginPolicy::Preserve,
                        },
                        |layout| {
                            if item_is_split {
                                let item_top = layout.cursor_y;
                                layout.paint_split_flex_item_fragment(
                                    child,
                                    &placed_style,
                                    stylesheets,
                                    SplitFlexItemPaintContext {
                                        item_width,
                                        item_height,
                                        slice_border_box: item_border_box,
                                        source_item_top: item_top,
                                    },
                                );
                                item_fragment.metadata = item_metadata.clone();
                                flex_layout.items[index].metadata = item_metadata;
                                return true;
                            }

                            layout.begin_assignment_capture_frame();
                            layout.layout_flex_item_contents(
                                child,
                                &placed_style,
                                stylesheets,
                                item_height,
                            );
                            item_metadata.assignment_ids = layout.end_assignment_capture_frame();
                            if !item_metadata.assignment_ids.is_empty() {
                                layout.update_running_assignment_placements(
                                    &item_metadata.assignment_ids,
                                    item_metadata.assignment_placement(),
                                );
                            }
                            item_fragment.metadata = item_metadata.clone();
                            flex_layout.items[index].metadata = item_metadata;
                            let policy = StackingContextPolicy::for_flex_item(
                                &placed_style,
                                item_border_box,
                            );
                            if !matches!(policy.context_kind, StackingContextKind::None) {
                                let child_contexts = layout.positioned_child_contexts_since(
                                    item_positioned_layer_start,
                                    item_page_index,
                                    policy,
                                );
                                layout.scope_current_page_paint_since_with_policy(
                                    &item_paint_checkpoint,
                                    policy,
                                    item_border_box,
                                    child_contexts,
                                );
                            }
                            false
                        },
                    );
                    if item_was_split {
                        continue;
                    }
                }
                flex_layout.fragment_plan.fragments.push(planned_fragment);
                if slice_end >= unit.block_end - 0.01 {
                    break;
                }
                self.push_page();
                fragment_content_top = self.cursor_y;
                fragment_block_offset = slice_end;
                slice_start = slice_end;
            }
            if break_is_applicable && unit.break_after.is_forced() {
                pending_break_before = unit.break_after;
                forced_break_after_flex_items = unit.break_after;
            } else {
                pending_break_before = PageBreak::Auto;
                forced_break_after_flex_items = PageBreak::Auto;
            }
            previous_break_after_avoid = break_is_applicable && unit.break_after.avoids_page();
        }
        self.pop_float_context();

        for child in &positioned_children {
            self.layout_positioned_flex_child(
                child,
                PositionedFlexStaticContext {
                    container_style: style,
                    stylesheets,
                    available: FlexAvailableSpace {
                        width: inner_width,
                        width_is_definite: true,
                        height: flex_available_content_height,
                        height_is_definite: has_definite_content_height,
                    },
                    inner_x,
                    inner_width,
                    content_top,
                },
            );
        }
        self.pop_overflow_clip(overflow_clip_active);
        self.content_left = previous_left;
        self.content_right = previous_right;

        self.cursor_y = fragment_content_top - (total_content_height - fragment_block_offset);
        if flex_fragmentation_enabled {
            self.fragment_top_offsets.pop();
        }
        self.cursor_y -= style.padding.bottom + border_widths.bottom;
        let block_bottom = self.cursor_y;
        let block_height = (block_top - block_bottom).max(total_height);
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
        } else if block_height > 0.0
            && (style.background_color.is_some()
                || style.background_image.is_some()
                || style.border_image.source.is_some()
                || used_border_width(style) > 0.0)
            && style.visibility == Visibility::Visible
        {
            own_background_primitives = self.box_background_primitives(
                outer_x,
                block_bottom,
                outer_width,
                block_height,
                style,
            );
        }
        if block_height > 0.0 && style.visibility == Visibility::Visible {
            let gap_gutters =
                flex_gap_decoration_gutters(&flex_layout, style, inner_width, total_content_height);
            own_background_primitives.extend(flex_gap_decoration_primitives_with_gutters(
                style,
                inner_x,
                content_top,
                inner_width,
                total_content_height,
                &flex_gap_decoration_items(&flex_layout),
                &gap_gutters,
            ));
        }
        if block_height > 0.0 && style.visibility == Visibility::Visible {
            own_outline_primitives = self.box_outline_primitives(
                outer_x,
                block_bottom,
                outer_width,
                block_height,
                style,
            );
        }
        let has_own_background_primitives = !own_background_primitives.is_empty();
        let has_own_outline_primitives = !own_outline_primitives.is_empty();
        if self.preserve_scoped_paint_public_order
            && self.pages.len() == paint_page_index
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
                    own_outline_primitives,
                );
            }
            if !defer_own_decoration_promotion {
                fragment.promote_background_border_to_in_flow_block();
            }
            if (has_own_background_primitives || has_own_outline_primitives) && !fragment.is_empty()
            {
                self.current_page
                    .replace_paint_tree_since_with_fragment(&paint_checkpoint, fragment);
            }
            self.cursor_y -= style.margin.bottom;
            if establishes_positioning_containing_block {
                self.containing_blocks.pop();
                self.cursor_y -= relative_offset.y;
            }
            self.apply_forced_break(if style.break_after.is_forced() {
                style.break_after
            } else {
                forced_break_after_flex_items
            });
            return;
        }
        let fragments = self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        let flex_spanned_pages = self.pages.len() != paint_page_index;
        for (page_index, mut fragment) in fragments {
            if flex_fragmentation_enabled || flex_spanned_pages {
                let fragment_bounds =
                    flex_container_page_fragment_bounds(&flex_layout.fragment_plan, page_index)
                        .or_else(|| {
                            flex_spanned_pages.then(|| {
                                fragment.bounds().map(|bounds| {
                                    PaintClip::from_paint_rect(paint_space_rect(
                                        outer_x,
                                        bounds.y(),
                                        outer_width,
                                        bounds.height(),
                                    ))
                                })
                            })?
                        });
                if let Some(fragment_bounds) = fragment_bounds {
                    if style.visibility == Visibility::Visible
                        && (style.background_color.is_some()
                            || style.background_image.is_some()
                            || style.border_image.source.is_some()
                            || used_border_width(style) > 0.0)
                    {
                        let page_background_primitives = self.box_background_primitives(
                            outer_x,
                            fragment_bounds.y(),
                            outer_width,
                            fragment_bounds.height(),
                            style,
                        );
                        fragment.prepend_primitives_in_band(
                            PaintBand::BackgroundBorder,
                            page_background_primitives,
                        );
                    }
                    if style.visibility == Visibility::Visible {
                        fragment.append_primitives_in_band(
                            PaintBand::BackgroundBorder,
                            flex_gap_decoration_primitives_for_page(
                                &flex_layout,
                                style,
                                page_index,
                                inner_x,
                                inner_width,
                                total_content_height,
                                fragment_bounds,
                            ),
                        );
                    }
                    if style.visibility == Visibility::Visible {
                        let page_outline_primitives = self.box_outline_primitives(
                            outer_x,
                            fragment_bounds.y(),
                            outer_width,
                            fragment_bounds.height(),
                            style,
                        );
                        fragment
                            .append_primitives_in_band(PaintBand::Outline, page_outline_primitives);
                    }
                }
            } else if page_index == background_page_index {
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
        self.cursor_y -= style.margin.bottom;
        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
            self.cursor_y -= relative_offset.y;
        }
        self.apply_forced_break(if style.break_after.is_forced() {
            style.break_after
        } else {
            forced_break_after_flex_items
        });
    }
}
