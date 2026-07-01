use super::*;

/// Input geometry for an abspos flex child's static-position calculation.
///
/// CSS Flexbox derives the static position of an absolutely positioned flex
/// child from the flex container's content box and hypothetical sole-item flex
/// placement:
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
struct PositionedFlexStaticContext<'a> {
    container_style: &'a ComputedStyle,
    stylesheets: &'a [Stylesheet],
    available: FlexAvailableSpace,
    inner_x: f32,
    inner_width: f32,
    content_top: f32,
}

/// One flex fragmentation boundary in the physical block direction.
///
/// CSS Flexbox fragments row containers by flex line and column containers by
/// item progression in paged media:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
#[derive(Debug, Clone)]
struct FlexBreakUnit {
    item_indices: Vec<usize>,
    line_start: usize,
    line_end: usize,
    block_start: f32,
    block_end: f32,
    break_before: PageBreak,
    break_after: PageBreak,
    break_inside_avoid: bool,
}

impl FlexBreakUnit {
    fn block_size(&self) -> f32 {
        (self.block_end - self.block_start).max(0.0)
    }

    fn slice(&self, block_start: f32, block_end: f32) -> Self {
        Self {
            item_indices: self.item_indices.clone(),
            line_start: self.line_start,
            line_end: self.line_end,
            block_start,
            block_end,
            break_before: self.break_before,
            break_after: self.break_after,
            break_inside_avoid: self.break_inside_avoid,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FlexFragmentBuildContext {
    page_index: usize,
    outer_x: f32,
    outer_width: f32,
    content_top: f32,
    block_offset: f32,
    starts_page_fragment: bool,
}

#[derive(Debug, Clone, Copy)]
struct SplitFlexItemPaintContext {
    item_width: f32,
    item_height: f32,
    slice_border_box: PaintClip,
    source_item_top: f32,
}

/// Resolve a flex container width keyword from known intrinsic contributions.
///
/// CSS Sizing defines `fit-content` as
/// `min(max-content, max(min-content, stretch-or-argument))`. Auto widths keep
/// normal block fill behavior, except float and inline-flex atom callers pass
/// `shrink_auto_width` to request CSS 2.2 shrink-to-fit sizing:
/// <https://www.w3.org/TR/css-sizing-3/#fit-content-size> and
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width>.
fn flex_container_content_width_from_intrinsic(
    style: &ComputedStyle,
    available_outer_width: f32,
    horizontal_extras: f32,
    intrinsic: FlexItemEstimate,
    shrink_auto_width: bool,
) -> f32 {
    let min_content = intrinsic.min_width.max(0.0);
    let max_content = intrinsic.width.max(min_content).max(0.0);
    let auto_width = if shrink_auto_width {
        intrinsic::IntrinsicAutoWidth::ShrinkToFit
    } else {
        intrinsic::IntrinsicAutoWidth::FillAvailable
    };
    intrinsic::content_width_from_intrinsic(
        style,
        available_outer_width,
        horizontal_extras,
        min_content,
        max_content,
        auto_width,
    )
}

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
    fn used_flex_container_content_width(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        horizontal_extras: f32,
    ) -> f32 {
        let needs_intrinsic = matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        ) || (style.float != Float::None && style.box_values.width.is_auto());

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
            style.float != Float::None,
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
            built_child_boxes = box_tree::build_child_boxes_with_font_metrics(
                element,
                stylesheets,
                &used_style,
                &self.ancestors,
                &mut self.font_system,
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
        let definite_content_height =
            used_content_height_or_auto(style, available_outer_height, vertical_extras)
                .map(|height| constrain_height(style, height, available_outer_height));
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
        let flex_break_content_extent = flex_break_units
            .iter()
            .map(|unit| unit.block_end)
            .fold(0.0f32, f32::max);
        let flex_content_height = if style.box_values.height.is_auto() {
            flex_layout.height.max(flex_break_content_extent)
        } else {
            flex_layout.height
        };
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

        let overflow_clip_active = if !is_document_canvas_element(element) {
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
                    let previous_item_left = self.content_left;
                    let previous_item_right = self.content_right;
                    let previous_cursor_y = self.cursor_y;
                    let item_width = original_item.width().max(0.0);
                    let item_x = original_item.x();
                    let item_y = original_item.y();
                    let item_height = original_item.height().max(0.0);
                    let visible_item_height = item.height().max(0.0);

                    self.content_left = inner_x + item_x;
                    self.content_right = self.content_left + item_width;
                    self.cursor_y = fragment_content_top - (item_y - fragment_block_offset);
                    let item_page_index = self.pages.len();
                    let item_starts_page_fragment = !self.current_page_has_content();
                    let visible_item_top =
                        fragment_content_top - (item.y() - fragment_block_offset);
                    let item_border_box = PageTopRect::new(
                        self.content_left,
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

                    let mut placed_style = child.style.clone();
                    placed_style.margin = css::Edges::ZERO;
                    placed_style.page_name_specified = false;
                    placed_style.page_name = None;
                    suppress_flex_item_fragmentation_breaks(&mut placed_style);
                    set_style_used_width(&mut placed_style, item_width);
                    set_style_used_height(&mut placed_style, item_height);
                    if style.flex_direction.is_row_axis() {
                        set_style_used_width_bounds(&mut placed_style, item_width);
                    } else {
                        set_style_used_height_bounds(&mut placed_style, item_height);
                    }
                    placed_style.box_sizing = BoxSizing::BorderBox;
                    if placed_style.display.is_inline_level() {
                        placed_style.display = placed_style.display.blockified();
                    }

                    if item_is_split {
                        let item_top = self.cursor_y;
                        self.paint_split_flex_item_fragment(
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
                        self.content_left = previous_item_left;
                        self.content_right = previous_item_right;
                        self.cursor_y = previous_cursor_y;
                        continue;
                    }

                    self.begin_assignment_capture_frame();
                    if let Some((child_element, signature, child_boxes)) = child.element_parts() {
                        self.push_ancestor_signature(signature.clone());
                        self.push_page_name_element_scope_suppression();
                        self.layout_element_with_child_boxes(
                            child_element,
                            &placed_style,
                            stylesheets,
                            child_boxes,
                        );
                        self.pop_page_name_element_scope_suppression();
                        self.ancestors.pop();
                    } else if let Some(children) = child.anonymous_content() {
                        self.layout_anonymous_block(&placed_style, children, stylesheets, None);
                    }
                    item_metadata.assignment_ids = self.end_assignment_capture_frame();
                    if !item_metadata.assignment_ids.is_empty() {
                        self.update_running_assignment_placements(
                            &item_metadata.assignment_ids,
                            item_metadata.assignment_placement(),
                        );
                    }
                    item_fragment.metadata = item_metadata.clone();
                    flex_layout.items[index].metadata = item_metadata;
                    let policy =
                        StackingContextPolicy::for_flex_item(&placed_style, item_border_box);
                    if !matches!(policy.context_kind, StackingContextKind::None) {
                        let child_contexts = if policy.captures_positioned_descendants
                            && item_positioned_layer_start < self.positioned_layers.len()
                        {
                            self.positioned_layers
                                .split_off(item_positioned_layer_start)
                                .into_iter()
                                .filter(|layer| layer.page_index == item_page_index)
                                .map(|layer| layer.context.with_links(layer.links))
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let fragment = self
                            .current_page
                            .paint_tree_fragment_since(&item_paint_checkpoint);
                        if let Some(fragment) = fragment
                            && (!fragment.is_empty() || !child_contexts.is_empty())
                        {
                            let context =
                                PaintStackingContext::from_banded_fragment_with_stack_level(
                                    policy.stack_level,
                                    fragment,
                                    child_contexts,
                                )
                                .with_source_order(self.next_paint_source_order())
                                .with_effects(policy.effects)
                                .with_bounds(item_border_box);
                            self.current_page.replace_paint_tree_since_with_context(
                                &item_paint_checkpoint,
                                policy.parent_band,
                                context,
                            );
                        }
                    }

                    self.content_left = previous_item_left;
                    self.content_right = previous_item_right;
                    self.cursor_y = previous_cursor_y;
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

    /// Build an atomic inline fragment for an `inline-flex` container.
    ///
    /// CSS Display makes `inline-flex` an inline-level atomic flex container,
    /// while CSS Flexbox defines both its flex item layout and the baseline it
    /// contributes to the parent inline formatting context:
    /// <https://www.w3.org/TR/css-display-3/#the-display-properties> and
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn inline_flex_atom_for_element(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        style: &ComputedStyle,
        child_boxes: &[box_tree::FormattingBox<'_>],
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
        let border_widths = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content();
        let vertical_extras = box_metrics.vertical_non_content();
        let (mut children, mut positioned_children) =
            flex_child_lists_from_boxes(element, signature, style, child_boxes);
        self.resolve_styled_children_viewport_lengths(&mut children);
        self.resolve_styled_children_viewport_lengths(&mut positioned_children);

        let intrinsic = self.estimate_intrinsic_flex_container_size(
            &children,
            style,
            stylesheets,
            FlexAvailableSpace {
                width: available_width.max(0.0),
                width_is_definite: used_content_width_or_auto(
                    style,
                    available_width.max(0.0),
                    horizontal_extras,
                )
                .is_some(),
                height: used_length_percentage_or_auto(style.box_values.height, available_width),
                height_is_definite: !style.box_values.height.is_auto(),
            },
        );
        let requested_content_width = flex_container_content_width_from_intrinsic(
            style,
            available_width,
            horizontal_extras,
            intrinsic,
            true,
        );
        let content_width =
            constrain_width(style, requested_content_width, available_width).max(0.0);

        let definite_content_height =
            used_content_height_or_auto(style, style.line_height.max(1.0), vertical_extras)
                .map(|height| constrain_height(style, height, available_width));
        let has_definite_content_height = definite_content_height.is_some();
        let flex_available_content_height =
            flex_available_content_height(style, definite_content_height, content_width);

        let Some(flex_layout) = self.compute_flex_layout(
            &children,
            style,
            stylesheets,
            FlexAvailableSpace {
                width: content_width,
                width_is_definite: true,
                height: flex_available_content_height,
                height_is_definite: has_definite_content_height,
            },
        ) else {
            return self.inline_fragment_atom_for_children(
                style,
                child_boxes,
                stylesheets,
                baseline_shift,
                link_target,
            );
        };

        let total_content_height = constrain_height(style, flex_layout.height, content_width);
        debug_assert!(!flex_layout.lines.is_empty() || children.is_empty());
        debug_assert!(
            flex_layout.fragment_plan.is_empty()
                || flex_layout.fragment_plan.planned_item_fragment_count()
                    <= flex_layout.items.len()
        );
        let border_box_height = total_content_height + vertical_extras;
        let estimated_baseline_offset =
            border_widths.top + style.padding.top + flex_layout.first_baseline;

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let top = 10_000.0;
        let content_top = top - border_widths.top - style.padding.top;
        let inner_x = border_widths.left + style.padding.left;
        let inner_width = content_width.max(0.0);
        self.current_page = Page::new(content_width + horizontal_extras, top);
        self.content_left = inner_x;
        self.content_right = inner_x + inner_width;
        self.cursor_y = content_top;
        self.truncate_page_start_margins = false;

        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            self.containing_blocks
                .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                    border_widths.left,
                    top - border_widths.top,
                    content_width + style.padding.left + style.padding.right,
                    total_content_height + style.padding.top + style.padding.bottom,
                )));
        }

        for (index, child) in children.iter().enumerate() {
            if flex_item_is_collapsed(&child.style) {
                continue;
            }
            let item = &flex_layout.items[index];
            let previous_left = self.content_left;
            let previous_right = self.content_right;
            let previous_cursor_y = self.cursor_y;
            let item_width = item.width().max(0.0);

            self.content_left = inner_x + item.x();
            self.content_right = self.content_left + item_width;
            self.cursor_y = content_top - item.y();

            let mut placed_style = child.style.clone();
            placed_style.margin = css::Edges::ZERO;
            placed_style.page_name_specified = false;
            placed_style.page_name = None;
            suppress_flex_item_fragmentation_breaks(&mut placed_style);
            set_style_used_width(&mut placed_style, item_width);
            set_style_used_height(&mut placed_style, item.height().max(0.0));
            if style.flex_direction.is_row_axis() {
                set_style_used_width_bounds(&mut placed_style, item_width);
            } else {
                set_style_used_height_bounds(&mut placed_style, item.height().max(0.0));
            }
            placed_style.box_sizing = BoxSizing::BorderBox;
            if placed_style.display.is_inline_level() {
                placed_style.display = placed_style.display.blockified();
            }

            if let Some((child_element, signature, child_boxes)) = child.element_parts() {
                self.push_ancestor_signature(signature.clone());
                self.push_page_name_element_scope_suppression();
                self.layout_element_with_child_boxes(
                    child_element,
                    &placed_style,
                    stylesheets,
                    child_boxes,
                );
                self.pop_page_name_element_scope_suppression();
                self.ancestors.pop();
            } else if let Some(children) = child.anonymous_content() {
                self.layout_anonymous_block(&placed_style, children, stylesheets, None);
            }

            self.content_left = previous_left;
            self.content_right = previous_right;
            self.cursor_y = previous_cursor_y;
        }

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

        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
        }
        let border_bottom = top - border_box_height;
        self.flush_positioned_layers_since(positioned_layer_start);
        let fragment = self
            .current_page
            .paint_fragment()
            .translated(PaintVector::new(0.0, -border_bottom));
        let baseline_offset = fragment
            .first_line_y()
            .map(|line_y| (border_box_height - line_y).max(0.0))
            .unwrap_or(estimated_baseline_offset);
        self.restore(snapshot);

        InlineAtom {
            content: InlineAtomContent::InlineFragment(fragment),
            style: style.clone(),
            escaped_positioned_layers: None,
            width: content_width + horizontal_extras + style.margin.left + style.margin.right,
            height: border_box_height + style.margin.top + style.margin.bottom,
            baseline_offset,
            baseline_shift,
            link_target,
            alt_text: None,
        }
    }

    /// Lays out an absolutely positioned flex child from its flex static position.
    ///
    /// CSS Flexbox says an absolutely positioned child of a flex container does
    /// not participate in flex layout, but its static-position rectangle is
    /// derived from where it would be positioned as the sole flex item:
    /// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
    fn layout_positioned_flex_child(
        &mut self,
        child: &StyledChild<'_>,
        context: PositionedFlexStaticContext<'_>,
    ) {
        let mut hypothetical_child = child.clone();
        hypothetical_child.style.position = Position::Static;
        hypothetical_child.style.flex_grow = 0.0;
        hypothetical_child.style.flex_shrink = 0.0;
        hypothetical_child.style.flex_basis = css::ComputedFlexBasis::Auto;
        if hypothetical_child.style.display.is_inline_level() {
            hypothetical_child.style.display = hypothetical_child.style.display.blockified();
        }
        let hypothetical = self
            .compute_flex_layout(
                std::slice::from_ref(&hypothetical_child),
                context.container_style,
                context.stylesheets,
                context.available,
            )
            .and_then(|layout| layout.items.into_iter().next())
            .unwrap_or_else(|| {
                FlexItemLayout::new(0.0, 0.0, context.inner_width, child.style.line_height)
            });

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;

        self.content_left = context.inner_x + hypothetical.x();
        self.content_right = self.content_left + hypothetical.width().max(1.0);
        self.cursor_y = context.content_top - hypothetical.y();

        let mut positioned_style = child.style.clone();
        if positioned_style.display.is_inline_level() {
            positioned_style.display = positioned_style.display.blockified();
        }
        if let Some((child_element, signature, child_boxes)) = child.element_parts() {
            self.push_ancestor_signature(signature.clone());
            self.layout_element_with_child_boxes(
                child_element,
                &positioned_style,
                context.stylesheets,
                child_boxes,
            );
            self.ancestors.pop();
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
    }

    /// Replay a split flex item from its original item layout and clip the
    /// selected page-local slice.
    ///
    /// CSS Fragmentation slices the visual fragment but preserves the source
    /// box's internal layout for continuations:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>.
    fn paint_split_flex_item_fragment(
        &mut self,
        child: &StyledChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        context: SplitFlexItemPaintContext,
    ) {
        let item_width = context.item_width;
        let item_height = context.item_height;
        let slice_border_box = context.slice_border_box;
        let source_item_top = context.source_item_top;
        if slice_border_box.width() <= 0.0 || slice_border_box.height() <= 0.0 {
            return;
        }

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let offpage_top = 10_000.0;
        self.current_page = Page::new(item_width.max(1.0), offpage_top);
        self.content_left = 0.0;
        self.content_right = item_width.max(0.0);
        self.cursor_y = offpage_top;
        self.truncate_page_start_margins = false;
        self.overflow_clips.clear();
        self.fragment_top_offsets.clear();

        if let Some((child_element, signature, child_boxes)) = child.element_parts() {
            self.push_ancestor_signature(signature.clone());
            self.push_page_name_element_scope_suppression();
            self.layout_element_with_child_boxes(
                child_element,
                placed_style,
                stylesheets,
                child_boxes,
            );
            self.pop_page_name_element_scope_suppression();
            self.ancestors.pop();
        } else if let Some(children) = child.anonymous_content() {
            self.layout_anonymous_block(placed_style, children, stylesheets, None);
        }
        self.flush_positioned_layers_since(positioned_layer_start);

        let fragment = self
            .current_page
            .paint_fragment()
            .translated(PaintVector::new(
                slice_border_box.x(),
                source_item_top - offpage_top,
            ))
            .clipped_to_rect(slice_border_box);
        self.restore(snapshot);

        if fragment.is_empty() {
            return;
        }

        let policy = StackingContextPolicy::for_flex_item(placed_style, slice_border_box);
        let mut effects = policy.effects;
        effects.overflow_clip = Some(slice_border_box);
        effects.absolute_clip = Some(slice_border_box);
        let source_bounds = PageTopRect::new(
            slice_border_box.x(),
            source_item_top,
            slice_border_box.width(),
            item_height,
        )
        .paint_clip();
        let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(effects)
            .with_bounds(source_bounds);
        let fragment = PaintFragment::from_stacking_context_in_band(policy.parent_band, context);
        self.current_page
            .append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
    }

    fn resolve_styled_children_viewport_lengths(&self, children: &mut [StyledChild<'_>]) {
        for child in children {
            self.resolve_style_current_viewport_lengths(&mut child.style);
        }
    }
}

/// Resolves the definite main-axis size made available to flex line wrapping.
///
/// CSS Flexbox wraps lines against the flex container's used main size. When a
/// column flex container has `height:auto`, `max-height` still constrains that
/// used main size and must be visible to the flex algorithm, while `min-height`
/// only clamps the final auto height and should not force otherwise overflowing
/// content to wrap:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-line-break>.
fn flex_available_content_height(
    style: &ComputedStyle,
    definite_content_height: Option<f32>,
    percentage_basis: f32,
) -> Option<f32> {
    if definite_content_height.is_some() || style.flex_wrap == FlexWrap::NoWrap {
        return definite_content_height;
    }
    if !style.flex_direction.is_column_axis() {
        return definite_content_height;
    }
    used_max_height(style, percentage_basis)
}

/// Returns whether an unfragmented flex container should prebreak to the next page.
///
/// CSS Fragmentation can move an unfragmented box to the next fragmentainer
/// when it fits there but not in the current remaining space. Flex containers
/// with item-level forced breaks skip this whole-box move so the break is
/// consumed at a flex boundary instead:
/// <https://www.w3.org/TR/css-break-3/#breaking-rules> and
/// <https://drafts.csswg.org/css-flexbox-1/#pagination>.
fn should_move_flex_container_to_next_page(
    block_top: f32,
    total_height: f32,
    page_top: f32,
    page_bottom: f32,
    page_area_height: f32,
) -> bool {
    let overflows_current_page = block_top - total_height < page_bottom;
    let starts_at_page_top = (block_top - page_top).abs() < 0.01;
    overflows_current_page && !starts_at_page_top && total_height <= page_area_height + 0.01
}

fn flex_break_units(
    flex_layout: &FlexLayout,
    children: &[StyledChild<'_>],
    style: &ComputedStyle,
) -> Vec<FlexBreakUnit> {
    let physical_direction = physical_flex_direction(style);
    if physical_direction.is_row_axis() {
        let mut units = flex_layout
            .lines
            .iter()
            .enumerate()
            .filter_map(|(line_index, line)| {
                let item_indices = line
                    .item_indices
                    .iter()
                    .copied()
                    .filter(|&index| {
                        children
                            .get(index)
                            .is_some_and(|child| !flex_item_is_collapsed(&child.style))
                    })
                    .collect::<Vec<_>>();
                (!item_indices.is_empty()).then(|| FlexBreakUnit {
                    line_start: line_index,
                    line_end: line_index + 1,
                    block_start: line.cross_start,
                    block_end: line.cross_end,
                    break_before: flex_unit_break_before(&item_indices, children),
                    break_after: flex_unit_break_after(&item_indices, children),
                    break_inside_avoid: item_indices
                        .iter()
                        .any(|&index| children[index].style.break_inside_avoid),
                    item_indices,
                })
            })
            .collect::<Vec<_>>();
        units.sort_by(|a, b| {
            a.block_start
                .partial_cmp(&b.block_start)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        return units;
    }

    let mut units = flex_layout
        .items
        .iter()
        .enumerate()
        .filter(|(index, _)| !flex_item_is_collapsed(&children[*index].style))
        .map(|(index, item)| {
            let (block_start, block_end) = flex_item_block_bounds(item);
            let (line_start, line_end) = flex_item_line_range(flex_layout, index);
            FlexBreakUnit {
                item_indices: vec![index],
                line_start,
                line_end,
                block_start,
                block_end,
                break_before: children[index].style.break_before,
                break_after: children[index].style.break_after,
                break_inside_avoid: children[index].style.break_inside_avoid,
            }
        })
        .collect::<Vec<_>>();
    units.sort_by(|a, b| {
        a.block_start
            .partial_cmp(&b.block_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    units
}

fn flex_fragment_from_break_unit(
    unit: &FlexBreakUnit,
    items: &[FlexItemLayout],
    context: FlexFragmentBuildContext,
) -> FlexFragmentLayout {
    let fragment_height = unit.block_size();
    let fragment_bottom = context.content_top - (unit.block_end - context.block_offset);
    FlexFragmentLayout {
        page_index: context.page_index,
        line_start: unit.line_start,
        line_end: unit.line_end,
        block_start: unit.block_start,
        block_end: unit.block_end,
        items: unit
            .item_indices
            .iter()
            .filter_map(|&item_index| {
                let item = items.get(item_index)?;
                let (item_block_start, item_block_end) = flex_item_block_bounds(item);
                let slice_start = item_block_start.max(unit.block_start);
                let slice_end = item_block_end.min(unit.block_end);
                if slice_end <= slice_start + 0.01 {
                    return None;
                }
                let mut bounds = item.clone();
                bounds.set_y(slice_start);
                bounds.set_height((slice_end - slice_start).max(0.0));
                let content_slice = FlexFragmentSlice {
                    block_start: (slice_start - item_block_start).max(0.0),
                    block_end: (slice_end - item_block_start).min(item.height().max(0.0)),
                };
                Some(FlexItemFragmentLayout {
                    item_index,
                    source_item_index: item_index,
                    original_bounds: item.clone(),
                    bounds,
                    content_slice,
                    decoration_slice: content_slice,
                    metadata: FragmentPageMetadata::empty(context.page_index),
                })
            })
            .collect(),
        metadata: FragmentPageMetadata::new(
            context.page_index,
            Some(PaintClip::from_paint_rect(paint_space_rect(
                context.outer_x,
                fragment_bottom,
                context.outer_width,
                fragment_height,
            ))),
            context.starts_page_fragment,
        ),
    }
}

fn flex_container_page_fragment_bounds(
    plan: &FlexFragmentPlan,
    page_index: usize,
) -> Option<PaintClip> {
    plan.fragments
        .iter()
        .filter(|fragment| fragment.page_index == page_index)
        .filter_map(|fragment| fragment.metadata.source_border_box)
        .fold(None, |bounds, fragment_box| {
            Some(match bounds {
                Some(bounds) => {
                    let bottom = bounds.y().min(fragment_box.y());
                    let top = (bounds.y() + bounds.height())
                        .max(fragment_box.y() + fragment_box.height());
                    let left = bounds.x().min(fragment_box.x());
                    PaintClip::from_paint_rect(paint_space_rect(
                        left,
                        bottom,
                        (bounds.x() + bounds.width()).max(fragment_box.x() + fragment_box.width())
                            - left,
                        top - bottom,
                    ))
                }
                None => fragment_box,
            })
        })
}

fn flex_item_line_range(flex_layout: &FlexLayout, item_index: usize) -> (usize, usize) {
    flex_layout
        .lines
        .iter()
        .enumerate()
        .find(|(_, line)| line.item_indices.contains(&item_index))
        .map(|(line_index, _)| (line_index, line_index + 1))
        .unwrap_or((0, 0))
}

fn flex_item_block_bounds(item: &FlexItemLayout) -> (f32, f32) {
    (item.y(), item.y() + item.height())
}

fn flex_unit_break_before(item_indices: &[usize], children: &[StyledChild<'_>]) -> PageBreak {
    item_indices
        .iter()
        .map(|&index| children[index].style.break_before)
        .fold(PageBreak::Auto, combine_flex_break)
}

fn flex_unit_break_after(item_indices: &[usize], children: &[StyledChild<'_>]) -> PageBreak {
    item_indices
        .iter()
        .map(|&index| children[index].style.break_after)
        .fold(PageBreak::Auto, combine_flex_break)
}

fn combine_flex_break(current: PageBreak, candidate: PageBreak) -> PageBreak {
    if current.is_forced() {
        current
    } else if candidate.is_forced() || candidate.avoids_page() {
        candidate
    } else {
        current
    }
}

/// Consume flex item break requests at the flex-container layer.
///
/// CSS Flexbox fragmentation handles forced breaks from flex items as breaks
/// between flex lines/container fragments. They must not be re-applied when
/// each item is laid out through the block-layout entrypoint, or a
/// `break-before`/`page-break-before` on an item can incorrectly push that item
/// to a standalone PDF page:
/// <https://drafts.csswg.org/css-flexbox-1/#pagination>.
fn suppress_flex_item_fragmentation_breaks(style: &mut ComputedStyle) {
    style.break_before = PageBreak::Auto;
    style.break_after = PageBreak::Auto;
}
