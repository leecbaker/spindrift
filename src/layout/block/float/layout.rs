use super::super::super::*;
use super::{
    exclusions::FLOAT_EPSILON,
    model::*,
    sizing::{float_replay_style, freeze_float_replay_width},
};

use crate::layout::inline_collect::has_out_of_flow_formatting_box;

impl<'a> LayoutBuilder<'a> {
    /// Lays out one floated child in the current block formatting context.
    ///
    /// CSS 2.2 blockifies floated boxes, gives auto-width floats a
    /// shrink-to-fit width, and records their margin boxes as exclusions for
    /// later line boxes:
    /// <https://www.w3.org/TR/CSS22/visuren.html#dis-pos-flo>,
    /// <https://www.w3.org/TR/CSS22/visudet.html#float-width>, and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_floating_child(
        &mut self,
        child_element: &Element,
        child_signature: ElementSignature,
        child_style: &ComputedStyle,
        child_children: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &[Stylesheet],
        run: &mut FloatRunState,
    ) -> bool {
        if child_style.float == Float::None
            || child_style.display.is_none()
            || matches!(child_style.position, Position::Absolute | Position::Fixed)
        {
            return false;
        }

        let mut placed_style = child_style.clone();
        if placed_style.display.is_inline_level() {
            placed_style.display = placed_style.display.blockified();
        }
        self.resolve_style_current_viewport_lengths(&mut placed_style);
        let specified_side = placed_style.float;
        let float_id = self.next_float_id();
        let Some(float_side) = UsedFloatSide::from_float(
            specified_side,
            placed_style.writing_mode,
            placed_style.direction,
        ) else {
            return false;
        };
        self.push_ancestor_signature(child_signature);
        placed_style.float = Float::None;
        // The replay style participates inside the float's independent BFC,
        // but the floated principal box is out of normal flow and cannot form
        // a named-page group at the parent's class-A sibling boundaries.
        // Clear the root page value before intrinsic measurement as well as
        // final replay so speculative layout cannot create a page transition.
        // <https://www.w3.org/TR/css-break-3/#possible-breaks> and
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        placed_style.page_name_specified = false;
        placed_style.page_name = None;
        if placed_style.display.is_flow() {
            placed_style.display.inner = DisplayInner::FlowRoot;
        }
        let built_child_children =
            if child_children.is_none() && !is_replaced_element(child_element) {
                Some(self.build_frozen_child_boxes_with_current_ancestors(
                    child_element,
                    stylesheets,
                    &placed_style,
                ))
            } else {
                None
            };
        let child_children = child_children.or(built_child_children.as_deref());

        let inline_size = (self.content_right - self.content_left).max(0.0);
        apply_used_box_metrics(
            &mut placed_style,
            PercentageBasis::definite(layout_pt(inline_size)),
        );

        let inline_size = self.resolved_float_inline_size(
            child_element,
            &placed_style,
            stylesheets,
            inline_size,
            child_children,
            table_fragment,
        );
        freeze_float_replay_width(&mut placed_style, inline_size);
        let margin_box_height = self.float_margin_box_height(
            child_element,
            &placed_style,
            stylesheets,
            inline_size,
            child_children,
            table_fragment,
        );
        if self.adjoining_float_origin_y.is_none() {
            self.prebreak_float_if_needed(margin_box_height);
        }
        let placement_top = self.adjoining_float_origin_y.unwrap_or(self.cursor_y);

        let (mut margin_box_left, mut top, _) =
            if matches!(float_side, UsedFloatSide::Top | UsedFloatSide::Bottom) {
                self.find_vertical_float_avoiding_position(
                    placement_top,
                    PageTopSize::new(inline_size.margin_box_width, margin_box_height),
                    placed_style.clear,
                    placed_style.writing_mode,
                    placed_style.direction,
                    Some(float_side),
                )
            } else {
                let (band_left, top, available_width) = self.find_float_avoiding_position(
                    placement_top,
                    PageTopSize::new(inline_size.margin_box_width, margin_box_height),
                    placed_style.clear,
                    placed_style.writing_mode,
                    placed_style.direction,
                );
                let margin_box_left = match float_side {
                    UsedFloatSide::Left => band_left,
                    UsedFloatSide::Right => {
                        band_left + (available_width - inline_size.margin_box_width).max(0.0)
                    }
                    UsedFloatSide::Top | UsedFloatSide::Bottom => unreachable!(),
                };
                (margin_box_left, top, available_width)
            };

        // A float is shifted downward until it fits beside earlier floats. If
        // the first available band crosses the fragmentainer edge, placement
        // continues at the next fragmentainer's block-start rather than
        // leaving the float (and following inline content) outside the page.
        // <https://www.w3.org/TR/css-break-3/#breaking-rules>
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
        if self.fragmentation_suppression_depth == 0
            && margin_box_height <= self.page_area_height() + FLOAT_EPSILON
            && top - margin_box_height < self.page_bottom() - FLOAT_EPSILON
        {
            // A float occupies the old fragmentainer, but as an out-of-flow
            // box it must not make the next in-flow sibling form a named-page
            // boundary there.
            self.current_page_has_flow_content = true;
            self.push_page();
            let next_placement_top = self.cursor_y;
            (margin_box_left, top, _) =
                if matches!(float_side, UsedFloatSide::Top | UsedFloatSide::Bottom) {
                    self.find_vertical_float_avoiding_position(
                        next_placement_top,
                        PageTopSize::new(inline_size.margin_box_width, margin_box_height),
                        placed_style.clear,
                        placed_style.writing_mode,
                        placed_style.direction,
                        Some(float_side),
                    )
                } else {
                    let (band_left, top, available_width) = self.find_float_avoiding_position(
                        next_placement_top,
                        PageTopSize::new(inline_size.margin_box_width, margin_box_height),
                        placed_style.clear,
                        placed_style.writing_mode,
                        placed_style.direction,
                    );
                    let margin_box_left = match float_side {
                        UsedFloatSide::Left => band_left,
                        UsedFloatSide::Right => {
                            band_left + (available_width - inline_size.margin_box_width).max(0.0)
                        }
                        UsedFloatSide::Top | UsedFloatSide::Bottom => unreachable!(),
                    };
                    (margin_box_left, top, available_width)
                };
        }

        let layout_snapshot = self.snapshot();
        let named_page_flow_content_before_float = self.current_page_has_named_page_flow_content;
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        if std::env::var_os("QUIRE_TRACE_FLOATS").is_some() {
            eprintln!(
                "float entry id={float_id:?} containing_left={} containing_right={}",
                previous_left, previous_right,
            );
        }
        let previous_cursor_y = self.cursor_y;
        let previous_direction = self.containing_block_direction;
        let previous_writing_mode = self.containing_block_writing_mode;

        // Float placement is expressed in margin-box coordinates, while the
        // replayed principal box is laid out from its border-box origin. The
        // placement algorithm has already consumed the used margins, so the
        // isolated replay must begin at the border box with those margins
        // suppressed.
        let replay_style = float_replay_style(&placed_style);
        self.content_left = margin_box_left + placed_style.margin.left;
        self.content_right = self.content_left + inline_size.border_box_width.points().max(1.0);
        self.cursor_y = top - placed_style.margin.top;
        self.containing_block_direction = placed_style.direction;
        self.containing_block_writing_mode = placed_style.writing_mode;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        self.push_page_name_scope_suppression();
        self.push_float_context();
        let previous_preserve_scoped_paint_public_order = self.preserve_scoped_paint_public_order;
        self.preserve_scoped_paint_public_order = true;
        self.layout_element_with_child_boxes_and_table_fragment(
            child_element,
            &replay_style,
            stylesheets,
            child_children,
            table_fragment,
        );
        if std::env::var_os("QUIRE_TRACE_FLOATS").is_some() {
            eprintln!(
                "float layout id={float_id:?} start_page={paint_page_index} end_page={} top={top} cursor={}",
                self.pages.len(),
                self.cursor_y,
            );
        }
        self.preserve_scoped_paint_public_order = previous_preserve_scoped_paint_public_order;
        self.pop_float_context();
        self.pop_page_name_scope_suppression();
        self.ancestors.pop();

        let actual_bottom = self.cursor_y.min(top - margin_box_height);
        let float_bounds = PageTopRect::new(
            margin_box_left,
            top,
            inline_size.margin_box_width,
            (top - actual_bottom).max(0.0),
        )
        .paint_clip();
        let float_shape = FloatShape::from_rect(
            float_id,
            specified_side,
            float_side,
            self.next_paint_source_order,
            paint_page_index,
            PageTopRect::new(
                margin_box_left,
                top,
                inline_size.margin_box_width,
                (top - actual_bottom).max(0.0),
            ),
        );
        let mut recorded_float_shape = false;
        let fragmented_float = self.pages.len() != paint_page_index;
        let captures_positioned_descendants = StackingContextPolicy::for_atomic(&placed_style, PaintBand::Float, float_bounds)
                .captures_positioned_descendants
                // Fragmented float escape needs page-fragment-aware positioned
                // containing block mapping. Keep existing per-fragment replay
                // until escaped positioned layers can be assigned to the
                // correct destination page without losing Appendix E ordering.
                || fragmented_float;
        let child_layers = if captures_positioned_descendants
            && positioned_layer_start < self.positioned_layers.len()
        {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        let has_positioned_descendants = !child_layers.is_empty();
        let has_out_of_flow_descendants =
            child_children.is_some_and(has_out_of_flow_formatting_box);
        if self.pages.len() == paint_page_index {
            let fragment = self
                .current_page
                .paint_tree_fragment_since(&paint_checkpoint);
            let child_contexts = child_layers
                .iter()
                .filter(|layer| layer.page_index == paint_page_index)
                .cloned()
                .map(|layer| layer.context.with_links(layer.links))
                .collect::<Vec<_>>();
            if let Some(float_fragment) = self.build_float_paint_fragment(
                float_id,
                specified_side,
                paint_page_index,
                float_side,
                margin_box_left,
                margin_box_left + inline_size.margin_box_width,
                float_bounds,
                &replay_style,
                replaced_element_kind(child_element).is_some(),
                fragment,
                child_contexts,
            ) {
                self.current_page.replace_paint_tree_since_with_context(
                    &paint_checkpoint,
                    PaintBand::Float,
                    float_fragment.context.clone(),
                );
                self.push_float_fragment_shape(&float_fragment, run);
                recorded_float_shape = true;
            }
        } else {
            let fragments =
                self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
            let mut float_fragments = Vec::new();
            for (page_index, fragment) in fragments {
                let child_contexts = child_layers
                    .iter()
                    .filter(|layer| layer.page_index == page_index)
                    .cloned()
                    .map(|layer| layer.context.with_links(layer.links))
                    .collect::<Vec<_>>();
                if let Some(float_fragment) = self.build_float_paint_fragment(
                    float_id,
                    specified_side,
                    page_index,
                    float_side,
                    margin_box_left,
                    margin_box_left + inline_size.margin_box_width,
                    float_bounds,
                    &replay_style,
                    replaced_element_kind(child_element).is_some(),
                    fragment,
                    child_contexts,
                ) {
                    float_fragments.push(float_fragment);
                }
            }
            float_fragments.sort_by_key(|fragment| (fragment.page_index, fragment.source_order));
            let fragment_count = float_fragments.len();
            for (index, fragment) in float_fragments.iter_mut().enumerate() {
                fragment.fragment_index = index;
                fragment.starts_on_previous_page = index > 0;
                fragment.continues_on_next_page = index + 1 < fragment_count;
            }
            let side_effects = self.deferred_layout_side_effects_since(&layout_snapshot);
            let next_paint_source_order = self.next_paint_source_order;
            let escaped_child_layers = if !captures_positioned_descendants
                && positioned_layer_start < self.positioned_layers.len()
            {
                self.positioned_layers.split_off(positioned_layer_start)
            } else {
                Vec::new()
            };
            self.restore(layout_snapshot);
            self.positioned_layers.extend(escaped_child_layers);
            self.next_paint_source_order = next_paint_source_order;
            self.apply_deferred_layout_side_effects(side_effects);
            for float_fragment in float_fragments {
                let paint_fragment = PaintFragment::from_stacking_context_in_band(
                    PaintBand::Float,
                    float_fragment.context.clone(),
                );
                if float_fragment.page_index == self.pages.len() {
                    self.current_page
                        .append_paint_fragment_owned(paint_fragment, PaintTranslation::identity());
                    // The final slice remains in `current_page` until the
                    // document is finalized. It is real page content even
                    // though floats are out of normal flow; otherwise
                    // `finish_boxed` drops a page containing only the final
                    // oversized-float slice.
                    // <https://www.w3.org/TR/css-break-3/#monolithic>
                    self.current_page_has_flow_content = true;
                    self.current_page.mark_fragmentation_content();
                } else {
                    // The float's continuation belongs to a future
                    // fragmentainer, but it must not advance the enclosing
                    // normal-flow cursor. When following flow reaches that
                    // page, `apply_pending_fragments_for_current_page`
                    // attaches the page-local float paint before line layout
                    // queries its registered shape.
                    // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                    self.pending_paint_fragments.push(PendingPaintFragment {
                        page_index: float_fragment.page_index,
                        fragment: paint_fragment,
                    });
                }
                self.push_float_fragment_shape(&float_fragment, run);
                recorded_float_shape = true;
            }
        }
        // A zero-height float participates in its own `clear` placement but
        // has no block-axis area to exclude from following line boxes.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        if !recorded_float_shape
            && (float_shape.rect.height() > FLOAT_EPSILON
                || has_positioned_descendants
                || has_out_of_flow_descendants)
        {
            self.push_float_shape(float_shape, run);
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.containing_block_direction = previous_direction;
        self.containing_block_writing_mode = previous_writing_mode;
        // Floats are out of normal flow. Their descendants can paint and
        // participate in float exclusion, but cannot turn the parent BFC's
        // following class-A sibling into a named-page transition boundary.
        // If float fragmentation advanced to a destination page, that page
        // likewise starts without an in-flow named-page group.
        self.current_page_has_named_page_flow_content = if self.pages.len() == paint_page_index {
            named_page_flow_content_before_float
        } else {
            false
        };
        true
    }
}
