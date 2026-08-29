use super::*;
use crate::layout::inline_collect::TextDecorationPropagationContext;
use crate::layout::inline_layout::InlineLayoutOutcome;
use crate::text::trim_css_collapsible_whitespace;

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineSplitBlockPaintScope {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) checkpoint: PaintCheckpoint,
    pub(in crate::layout) positioned_layer_start: usize,
    pub(in crate::layout) source_order: usize,
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_anonymous_block(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        marker: Option<&ListMarker>,
    ) -> bool {
        self.layout_anonymous_block_with_first_line_policy(
            style,
            children,
            stylesheets,
            marker,
            true,
            true,
        )
        .has_flow_effects
    }

    pub(in crate::layout) fn layout_anonymous_block_with_first_line_policy(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        marker: Option<&ListMarker>,
        allow_typographic_first_line: bool,
        initial_first_formatted_line: bool,
    ) -> InlineLayoutOutcome {
        let suppressed_style = (!allow_typographic_first_line)
            .then(|| style_without_typographic_first_line_pseudos(style))
            .flatten();
        let style = suppressed_style.as_ref().unwrap_or(style);
        let available_width = self.current_content_logical_inline_size().max(1.0);
        if marker.is_none()
            && anonymous_block_is_plain_text_with_style(children, style)
            // `layout_text_block` starts a fresh inline formatting context,
            // which is correct only for the originating block's first
            // anonymous run. A later run after an in-flow block must carry
            // the already-consumed first-formatted-line state through the
            // shared item formatter so it cannot restart `text-indent`.
            // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
            && initial_first_formatted_line
            && !self
                .active_float_exclusions_at(PageBlockSpan::new(self.cursor_y, style.line_height))
        {
            let text = inline_text_from_formatting_boxes(children);
            // A whitespace-only anonymous run at a line edge is discarded by
            // CSS Text whitespace processing.  It must not manufacture a
            // line box before a following float: that would move a source-
            // early float down and erase the CSS 2.2 distinction between
            // floats that occur before and after prior inline content.
            // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
            if !style.white_space.collapses_spaces()
                || !trim_css_collapsible_whitespace(&text).is_empty()
            {
                let outcome = self.layout_text_block(&text, style, 0.0, 0.0, None);
                return outcome;
            }
            return InlineLayoutOutcome::default();
        }
        let mut items = Vec::new();
        if let Some(marker) = marker
            && marker.paints_outside()
            && !self.outside_marker_anchor_is_pending(marker)
        {
            if self.cursor_y - style.font_size < self.page_bottom() {
                self.push_page();
            }
            let anchor = self.outside_marker_fallback_anchor(
                style,
                PageInlineSpan::from_edges(self.content_left, self.content_right),
            );
            self.paint_outside_marker(marker, style, anchor);
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
        }
        if let Some(marker) = marker
            && marker.participates_in_first_line()
            && !marker.follows_content_in_first_line()
            && (marker.image.is_some() || !trim_css_collapsible_whitespace(&marker.text).is_empty())
        {
            self.push_inside_marker_items(marker, style, None, &mut items);
        }
        let multicol_column_width = {
            let multicol_style = self.multicol_used_style(style);
            let style = &multicol_style;
            let gap = used_multicol_column_gap(
                style.column_gap.clone(),
                PercentageBasis::definite(content_box_pt(available_width)),
                style.font_size,
            )
            .points();
            used_multicol_column_count(style, available_width, gap)
                .filter(|count| *count > 1)
                .map(|column_count| {
                    let total_gap = gap * column_count.saturating_sub(1) as f32;
                    ((available_width - total_gap) / column_count as f32).max(1.0)
                })
        };
        let saved_content_right = self.content_right;
        if let Some(column_width) = multicol_column_width {
            // Inline atoms resolve percentage sizes during collection. A
            // multicol anonymous block therefore supplies its column box as
            // the containing-block basis before line construction.
            // <https://www.w3.org/TR/css-multicol-1/#column-box>.
            self.content_right = self.content_left + column_width;
            self.content_logical_inline_size_stack.push(column_width);
        }
        self.collect_inline_box_items(
            children,
            stylesheets,
            None,
            0.0,
            InlineVisualOffset::zero(),
            style,
            style.text_decoration_origins.effective_layers_vec(),
            &mut items,
        );
        if let Some(marker) = marker
            && marker.follows_content_in_first_line()
        {
            self.push_inside_marker_items(marker, style, None, &mut items);
        }
        if multicol_column_width.is_some() {
            self.content_logical_inline_size_stack.pop();
            self.content_right = saved_content_right;
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
        }
        if !items.is_empty() {
            let multicol_content_height =
                style.box_values.height.length_if_no_percent().or_else(|| {
                    self.block_percentage_context_stack
                        .current_percentage_basis()
                        .points()
                });
            match self.try_layout_multicol_inline_items(
                items,
                style,
                available_width,
                (0.0, 0.0),
                multicol_content_height,
            ) {
                Ok(()) => {
                    return InlineLayoutOutcome {
                        next_line_index: 0,
                        clamp_line_slots: 0,
                        clamp_block_advance: Default::default(),
                        has_non_phantom_line: true,
                        has_flow_effects: true,
                        has_local_continuation_cutoff: false,
                    };
                }
                Err(returned_items) => items = returned_items,
            }
            return self.layout_inline_items_with_first_formatted_line_policy(
                items,
                style,
                available_width,
                0.0,
                0.0,
                stylesheets,
                initial_first_formatted_line,
            );
        }
        InlineLayoutOutcome::default()
    }

    pub(in crate::layout) fn layout_inline_split_block_context_with_parent_decoration(
        &mut self,
        context: &box_tree::InlineSplitBlockContextBox<'_>,
        stylesheets: &Stylesheets<'_>,
        parent_style: Option<&ComputedStyle>,
    ) {
        let used_context_style = parent_style
            .map(|style| {
                TextDecorationPropagationContext::from_style(style)
                    .used_child_style(&context.core.style)
            })
            .unwrap_or_else(|| (*context.core.style).clone());
        let scope = self.begin_inline_split_block_paint_scope();
        self.with_inline_split_block_relative_layout_scope(Some(context), |layout| {
            for child in &context.core.children {
                let prior_line_baseline = layout.last_in_flow_line_baseline_y;
                layout.layout_formatting_box_with_parent_decoration(
                    child,
                    stylesheets,
                    Some(&used_context_style),
                );
                if child.element_parts().is_some_and(|(element, _, style, _)| {
                    layout_containment_applies_to_element(element, style)
                        && !matches!(style.position, Position::Absolute | Position::Fixed)
                        && style.float == Float::None
                }) {
                    // A layout-contained block cannot replace the preceding line
                    // baseline through the anonymous block generated by
                    // block-in-inline splitting.
                    // <https://www.w3.org/TR/css-contain-1/#containment-layout>
                    layout.last_in_flow_line_baseline_y = prior_line_baseline;
                }
            }
        });
        self.finish_inline_split_block_paint_scope(context, scope);
    }

    /// Query floats for an in-flow block fragment of a split inline in the
    /// inline's relative-positioned coordinate space.
    ///
    /// CSS 2.2 splits an inline around an in-flow block child, but the
    /// relative translation of the original inline still affects that block.
    /// In particular, float exclusions must be queried against the translated
    /// line-box span. The block itself remains in its parent flow coordinate
    /// space, so the normal-flow cursor, sibling geometry, and eventual paint
    /// translation each remain applied exactly once.
    ///
    /// The scope is deliberately entered only for the split block children;
    /// an intervening float keeps its own static placement in the parent flow.
    /// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
    /// <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>
    pub(in crate::layout) fn with_inline_split_block_relative_layout_scope<R>(
        &mut self,
        context: Option<&box_tree::InlineSplitBlockContextBox<'_>>,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let Some(context) = context else {
            return layout(self);
        };
        let offset = self.normal_flow_relative_position_offset(&context.core.style);
        if offset.is_zero() {
            return layout(self);
        }

        let previous_offset = self.inline_split_float_exclusion_query_offset;
        self.inline_split_float_exclusion_query_offset = RelativeOffset {
            vector: ContainerVector::new(
                previous_offset.x() + offset.x(),
                previous_offset.y() + offset.y(),
            ),
        };

        let result = layout(self);

        self.inline_split_float_exclusion_query_offset = previous_offset;
        result
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
        stylesheets: &Stylesheets<'_>,
        placement_axes: FloatPlacementAxes,
        run: &mut FloatRunState,
        split_inline_block_offset: Option<f32>,
        pseudo_source: Option<box_tree::CounterEventSource>,
    ) -> bool {
        let pushed_containing_block = self.push_inline_split_positioning_containing_block(context);
        let saved_cursor_y = self.cursor_y;
        if let Some(offset) = split_inline_block_offset {
            self.cursor_y -= offset;
        }
        let laid_out = if let Some(pseudo_source) = pseudo_source {
            self.layout_generated_floating_child(
                child_element,
                child_signature,
                child_style,
                child_children,
                table_fragment,
                stylesheets,
                placement_axes,
                run,
                pseudo_source,
            )
        } else {
            self.layout_floating_child(
                child_element,
                child_signature,
                child_style,
                child_children,
                table_fragment,
                stylesheets,
                placement_axes,
                run,
            )
        };
        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        self.cursor_y = saved_cursor_y;
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
        let style = &context.core.style;
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

    /// Captures a block-in-inline split segment under its inline ancestor's
    /// stacking policy.
    ///
    /// CSS 2.2 splits an inline around in-flow block-level descendants, but
    /// relative positioning applies to all generated boxes for that inline and
    /// Appendix E paints a positioned inline's generated content at the inline's
    /// stack level. The layout scope makes float exclusion queries use the
    /// final visual coordinates; this method applies the corresponding paint
    /// translation once:
    /// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>,
    /// <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>, and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(in crate::layout) fn finish_inline_split_block_paint_scope(
        &mut self,
        context: &box_tree::InlineSplitBlockContextBox<'_>,
        scope: InlineSplitBlockPaintScope,
    ) {
        let initial_policy = StackingContextPolicy::for_non_positioned_style_effect(
            &context.core.style,
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

        let relative_offset = self.normal_flow_relative_position_offset(&context.core.style);
        let paint_offset = PaintTranslation::new(relative_offset.x(), relative_offset.y());
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
                StackingContextPolicy::for_non_positioned_style_effect(&context.core.style, bounds);
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
            self.append_or_defer_scoped_paint_fragment(page_index, fragment);
        }
    }
}
fn inline_split_style_establishes_positioning_containing_block(style: &ComputedStyle) -> bool {
    matches!(
        style.position,
        Position::Absolute | Position::Fixed | Position::Relative | Position::Sticky
    ) || style.has_transform()
}

pub(in crate::layout) fn anonymous_block_is_plain_text_with_style(
    children: &[box_tree::FormattingBox<'_>],
    style: &ComputedStyle,
) -> bool {
    children
        .iter()
        .all(|child| matches!(child, box_tree::FormattingBox::Text(box_) if *box_.style == *style))
}
