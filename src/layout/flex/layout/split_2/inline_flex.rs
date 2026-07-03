use super::*;

impl<'a> LayoutBuilder<'a> {
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

        let explicit_content_height =
            used_content_height_or_auto(style, style.line_height.max(1.0), vertical_extras)
                .map(|height| constrain_height(style, height, available_width));
        let definite_content_height = definite_flex_container_content_height(
            style,
            explicit_content_height,
            content_width,
            available_width,
            horizontal_extras,
            vertical_extras,
        );
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
        let estimated_baseline_offset = flex_layout
            .first_baseline
            .map(|baseline| border_widths.top + style.padding.top + baseline)
            .unwrap_or_else(|| inline_flex_synthesized_baseline_offset(style, border_box_height));

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
            let item_width = item.width().max(0.0);
            let item_height = item.height().max(0.0);

            let placed_style =
                placed_flex_item_style(&child.style, item_width, item_height, style.flex_direction);
            self.with_formatting_context_item_placement(
                FormattingContextItemPlacement {
                    content_left: inner_x + item.x(),
                    content_width: item_width,
                    cursor_y: content_top - item.y(),
                    page_start_margin_policy: PageStartMarginPolicy::Suppress,
                },
                |layout| {
                    layout.layout_flex_item_contents(
                        child,
                        &placed_style,
                        stylesheets,
                        item_height,
                    );
                },
            );
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
        let mut fragment = self.current_page.paint_fragment();
        if style.visibility == Visibility::Visible {
            let gap_gutters =
                flex_gap_decoration_gutters(&flex_layout, style, inner_width, total_content_height);
            fragment.prepend_primitives_in_band(
                PaintBand::BackgroundBorder,
                flex_gap_decoration_primitives_with_gutters(
                    style,
                    inner_x,
                    content_top,
                    inner_width,
                    total_content_height,
                    &flex_gap_decoration_items(&flex_layout),
                    &gap_gutters,
                ),
            );
        }
        let fragment = fragment.translated(PaintVector::new(0.0, -border_bottom));
        let baseline_offset = fragment
            .first_line_y()
            .map(|line_y| (border_box_height - line_y).max(0.0))
            .unwrap_or(estimated_baseline_offset);
        self.restore(snapshot);

        InlineAtom::new(
            InlineAtomContent::InlineFragment(fragment),
            style.clone(),
            None,
            content_width + horizontal_extras + style.margin.left + style.margin.right,
            border_box_height + style.margin.top + style.margin.bottom,
            baseline_offset,
            baseline_shift,
            link_target,
            None,
        )
    }

    /// Lays out an absolutely positioned flex child from its flex static position.
    ///
    /// CSS Flexbox says an absolutely positioned child of a flex container does
    /// not participate in flex layout, but its static-position rectangle is
    /// derived from where it would be positioned as the sole flex item:
    /// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
    pub(in crate::layout::flex) fn layout_positioned_flex_child(
        &mut self,
        child: &StyledChild<'_>,
        context: PositionedFlexStaticContext<'_>,
    ) {
        let mut hypothetical_child = child.clone();
        hypothetical_child.style.position = Position::Static;
        hypothetical_child.style.flex_grow = 0.0;
        hypothetical_child.style.flex_shrink = 0.0;
        hypothetical_child.style.flex_basis = css::ComputedFlexBasis::Auto;
        zero_auto_margins_for_static_flex_probe(&mut hypothetical_child.style);
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

        let static_left = context.inner_x + hypothetical.x();
        self.layout_positioned_formatting_context_child(
            child,
            context.stylesheets,
            PositionedChildStaticRect::new(
                static_left,
                static_left + hypothetical.width(),
                context.content_top - hypothetical.y(),
            ),
            PositionedFormattingChildReplayMode::AbsoluteStaticRect,
        );
    }

    /// Replay a split flex item from its original item layout and clip the
    /// selected page-local slice.
    ///
    /// CSS Fragmentation slices the visual fragment but preserves the source
    /// box's internal layout for continuations:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>.
    pub(in crate::layout::flex) fn paint_split_flex_item_fragment(
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
        self.overflow_clips.clear();
        self.fragment_top_offsets.clear();

        self.with_formatting_context_item_placement(
            FormattingContextItemPlacement {
                content_left: 0.0,
                content_width: item_width,
                cursor_y: offpage_top,
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
            },
            |layout| {
                layout.layout_flex_item_contents(child, placed_style, stylesheets, item_height);
                layout.flush_positioned_layers_since(positioned_layer_start);
            },
        );

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

    pub(in crate::layout::flex) fn resolve_styled_children_viewport_lengths(
        &self,
        children: &mut [StyledChild<'_>],
    ) {
        for child in children {
            self.resolve_style_current_viewport_lengths(&mut child.style);
        }
    }
}

/// Return the synthesized inline-level baseline for an `inline-flex` atom.
///
/// CSS Flexbox leaves an empty row flex container without a main-axis baseline
/// set; CSS Inline then synthesizes the atomic inline's baseline from its
/// margin box in the inline formatting context:
/// <https://drafts.csswg.org/css-flexbox/#flex-baselines> and
/// <https://www.w3.org/TR/css-inline-3/#atomic-inline>.
pub(in crate::layout::flex) fn inline_flex_synthesized_baseline_offset(
    style: &ComputedStyle,
    border_box_height: f32,
) -> f32 {
    match style.writing_mode {
        WritingMode::HorizontalTb => border_box_height + style.margin.bottom,
        WritingMode::VerticalRl | WritingMode::VerticalLr => border_box_height,
    }
}
