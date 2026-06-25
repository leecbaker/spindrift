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

impl<'a> LayoutBuilder<'a> {
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
        let used_edges = used_box_edges(&used_style, containing_inline_size);
        used_style.margin = used_edges.margin.to_css_edges();
        used_style.padding = used_edges.padding.to_css_edges();
        let style = &used_style;

        let relative_offset = relative_position_offset(style, self.current_containing_block());
        if style.position == Position::Relative {
            self.cursor_y += relative_offset.y;
        }

        let mut outer_x = self.content_left + style.margin.left + relative_offset.x;
        let available_outer_width =
            self.content_right - self.content_left - style.margin.left - style.margin.right;
        let border_widths = used_border_widths(style);
        let horizontal_extras =
            border_widths.left + border_widths.right + style.padding.left + style.padding.right;
        let vertical_extras =
            border_widths.top + border_widths.bottom + style.padding.top + style.padding.bottom;

        let (mut children, mut positioned_children) = child_boxes
            .map(flex_child_lists_from_boxes)
            .unwrap_or_else(|| flex_child_lists(element, style, stylesheets, &self.ancestors));
        self.resolve_styled_children_viewport_lengths(&mut children);
        self.resolve_styled_children_viewport_lengths(&mut positioned_children);

        let requested_content_width =
            if style.float != Float::None && style.box_values.width.is_auto() {
                let intrinsic_size = self.estimate_intrinsic_flex_container_size(
                    &children,
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
                intrinsic::shrink_to_fit_width(
                    intrinsic_size.min_width,
                    intrinsic_size.width,
                    available_outer_width.max(0.0),
                )
            } else {
                used_content_width(style, available_outer_width, horizontal_extras)
            };
        let content_width = constrain_width(style, requested_content_width, available_outer_width);
        let outer_width = if style.float != Float::None {
            (content_width + horizontal_extras).max(0.0)
        } else {
            (content_width + horizontal_extras)
                .min(available_outer_width)
                .max(0.0)
        };
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
                self.containing_block_direction,
            );
            self.cursor_y = avoided_top;
            outer_x = margin_box_left + style.margin.left + relative_offset.x;
            inner_x = outer_x + border_widths.left + style.padding.left;
        } else {
            self.cursor_y = self.clear_active_floats_top(
                style.clear,
                self.containing_block_direction,
                self.cursor_y,
            );
        }
        let block_top = self.cursor_y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        self.cursor_y -= border_widths.top + style.padding.top;

        let Some(flex_layout) = self.compute_flex_layout(
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
            self.layout_block(element, &flow_style, stylesheets, &[], child_boxes);
            return;
        };

        let total_content_height = constrain_height(style, flex_layout.height, content_width);
        let total_height = border_widths.top
            + style.padding.top
            + total_content_height
            + style.padding.bottom
            + border_widths.bottom;
        if should_move_flex_container_to_next_page(
            block_top,
            total_height,
            self.page_top(),
            self.page_bottom(),
            self.page_area_height(),
        ) {
            self.push_page();
            self.layout_flex(element, source_style, stylesheets, child_boxes);
            return;
        }
        let content_top = self.cursor_y;
        let establishes_positioning_containing_block =
            style.position == Position::Relative || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            self.containing_blocks.push(ContainingBlock {
                x: outer_x + border_widths.left,
                top_y: block_top - border_widths.top,
                width: (content_width + style.padding.left + style.padding.right).max(0.0),
                height: (total_content_height + style.padding.top + style.padding.bottom).max(0.0),
            });
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

        self.push_float_context();
        for (index, child) in children.iter().enumerate() {
            if flex_item_is_collapsed(&child.style) {
                continue;
            }
            let item = &flex_layout.items[index];
            let previous_left = self.content_left;
            let previous_right = self.content_right;
            let previous_cursor_y = self.cursor_y;
            let item_width = item.width.max(0.0);

            self.content_left = inner_x + item.x;
            self.content_right = self.content_left + item_width;
            self.cursor_y = content_top - item.y;

            let mut placed_style = child.style.clone();
            placed_style.margin = css::Edges::ZERO;
            suppress_flex_item_fragmentation_breaks(&mut placed_style);
            set_style_used_width(&mut placed_style, item_width);
            set_style_used_height(&mut placed_style, item.height.max(0.0));
            if style.flex_direction.is_row_axis() {
                set_style_used_width_bounds(&mut placed_style, item_width);
            } else {
                set_style_used_height_bounds(&mut placed_style, item.height.max(0.0));
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
            } else if let Some(text) = child.anonymous_text() {
                self.layout_text_block(text, &placed_style, 0.0, 0.0, None);
            }

            self.content_left = previous_left;
            self.content_right = previous_right;
            self.cursor_y = previous_cursor_y;
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

        self.cursor_y = content_top - total_content_height;
        self.cursor_y -= style.padding.bottom + border_widths.bottom;
        let block_bottom = self.cursor_y;
        let block_height = (block_top - block_bottom).max(total_height);
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
            self.apply_forced_break(style.break_after);
            return;
        }
        let fragments = self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        for (page_index, mut fragment) in fragments {
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
                self.pages[page_index].append_paint_fragment(&fragment, 0.0, 0.0);
            } else {
                self.current_page.append_paint_fragment(&fragment, 0.0, 0.0);
            }
        }
        self.cursor_y -= style.margin.bottom;
        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
            self.cursor_y -= relative_offset.y;
        }
        self.apply_forced_break(style.break_after);
    }

    /// Build an atomic inline fragment for an `inline-flex` container.
    ///
    /// CSS Display makes `inline-flex` an inline-level atomic flex container,
    /// while CSS Flexbox defines both its flex item layout and the baseline it
    /// contributes to the parent inline formatting context:
    /// <https://www.w3.org/TR/css-display-3/#the-display-properties> and
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
    pub(in crate::layout) fn inline_flex_atom_for_element(
        &mut self,
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
        let used_edges = used_box_edges(&used_style, available_width);
        used_style.margin = used_edges.margin.to_css_edges();
        used_style.padding = used_edges.padding.to_css_edges();
        let style = &used_style;
        let border_widths = used_border_widths(style);
        let horizontal_extras =
            border_widths.left + border_widths.right + style.padding.left + style.padding.right;
        let vertical_extras =
            border_widths.top + border_widths.bottom + style.padding.top + style.padding.bottom;
        let (mut children, mut positioned_children) = flex_child_lists_from_boxes(child_boxes);
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
        let auto_content_width = intrinsic::shrink_to_fit_width(
            intrinsic.min_width,
            intrinsic.width,
            (available_width - horizontal_extras).max(0.0),
        );
        let requested_content_width =
            used_content_width_or_auto(style, available_width, horizontal_extras)
                .unwrap_or(auto_content_width);
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
            style.position == Position::Relative || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            self.containing_blocks.push(ContainingBlock {
                x: border_widths.left,
                top_y: top - border_widths.top,
                width: (content_width + style.padding.left + style.padding.right).max(0.0),
                height: (total_content_height + style.padding.top + style.padding.bottom).max(0.0),
            });
        }

        self.push_page_name_scope_suppression();
        for (index, child) in children.iter().enumerate() {
            if flex_item_is_collapsed(&child.style) {
                continue;
            }
            let item = &flex_layout.items[index];
            let previous_left = self.content_left;
            let previous_right = self.content_right;
            let previous_cursor_y = self.cursor_y;
            let item_width = item.width.max(0.0);

            self.content_left = inner_x + item.x;
            self.content_right = self.content_left + item_width;
            self.cursor_y = content_top - item.y;

            let mut placed_style = child.style.clone();
            placed_style.margin = css::Edges::ZERO;
            suppress_flex_item_fragmentation_breaks(&mut placed_style);
            set_style_used_width(&mut placed_style, item_width);
            set_style_used_height(&mut placed_style, item.height.max(0.0));
            if style.flex_direction.is_row_axis() {
                set_style_used_width_bounds(&mut placed_style, item_width);
            } else {
                set_style_used_height_bounds(&mut placed_style, item.height.max(0.0));
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
            } else if let Some(text) = child.anonymous_text() {
                self.layout_text_block(text, &placed_style, 0.0, 0.0, None);
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
        self.pop_page_name_scope_suppression();

        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
        }
        let border_bottom = top - border_box_height;
        self.flush_positioned_layers_since(positioned_layer_start);
        let fragment = self
            .current_page
            .paint_fragment()
            .translated(0.0, -border_bottom);
        let baseline_offset = fragment
            .first_line_y()
            .map(|line_y| (border_box_height - line_y).max(0.0))
            .unwrap_or(estimated_baseline_offset);
        self.restore(snapshot);

        InlineAtom {
            content: InlineAtomContent::InlineFragment(fragment),
            style: style.clone(),
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
            .unwrap_or(FlexItemLayout {
                x: 0.0,
                y: 0.0,
                width: context.inner_width,
                height: child.style.line_height,
            });

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;

        self.content_left = context.inner_x + hypothetical.x;
        self.content_right = self.content_left + hypothetical.width.max(1.0);
        self.cursor_y = context.content_top - hypothetical.y;

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
/// CSS Fragmentation should eventually split flex containers across
/// fragmentainers. Until that is fully implemented, prebreaking is only useful
/// when the whole flex border box can fit on an empty page; otherwise moving it
/// just creates an avoidable leading blank page:
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
