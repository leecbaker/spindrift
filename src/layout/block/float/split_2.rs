use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn next_float_id(&mut self) -> FloatId {
        let id = FloatId(self.next_float_id);
        self.next_float_id += 1;
        id
    }

    /// Starts float placement for the current block formatting context.
    ///
    /// This returns a short-lived view used by legacy call sites that need the
    /// current row's immediate exclusions; durable exclusions live in
    /// [`FloatContext`].
    pub(in crate::layout) fn float_run_state(&self) -> FloatRunState {
        FloatRunState::new(self.content_left, self.content_right, self.cursor_y)
    }

    /// Compatibility hook for old row-flush call sites.
    ///
    /// Durable CSS floats do not advance the block cursor when a run ends.
    pub(in crate::layout) fn flush_float_run(&mut self, run: &mut FloatRunState) {
        run.reset_for_block(self.content_left, self.content_right, self.cursor_y);
    }

    pub(in crate::layout) fn push_float_context(&mut self) {
        self.float_contexts
            .push(FloatContext { shapes: Vec::new() });
    }

    pub(in crate::layout) fn pop_float_context(&mut self) {
        if self.float_contexts.len() > 1 {
            self.float_contexts.pop();
        }
    }

    pub(in crate::layout) fn current_float_band(&self, top: f32, height: f32) -> FloatBand {
        self.float_contexts
            .last()
            .expect("root float context exists")
            .band(
                self.current_float_page_index(),
                PageBlockSpan::new(top, height),
                PageInlineSpan::from_edges(self.content_left, self.content_right),
            )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn current_logical_float_band(
        &self,
        writing_mode: WritingMode,
        direction: Direction,
        block_start: f32,
        block_size: f32,
        inline_physical_start: f32,
        inline_size: f32,
    ) -> LogicalFloatBand {
        self.float_contexts
            .last()
            .expect("root float context exists")
            .logical_band(
                writing_mode,
                direction,
                self.current_float_page_index(),
                block_start,
                block_size,
                inline_physical_start,
                inline_size,
            )
    }

    pub(in crate::layout) fn active_float_exclusions_at(&self, top: f32, height: f32) -> bool {
        let band = self.current_float_band(top, height);
        band.left() > self.content_left + FLOAT_EPSILON
            || band.right() < self.content_right - FLOAT_EPSILON
    }

    /// Return the top border edge after applying `clear` in the current BFC.
    ///
    /// CSS 2.2 clearance is page-local for each float fragment, but CSS
    /// Fragmentation can split a prior float across fragmentainers. When a
    /// matching fragment continues, clearance must progress to the next page
    /// and clear the next page-local fragment before normal flow resumes:
    /// <https://www.w3.org/TR/CSS22/visuren.html#flow-control> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn clear_active_floats_top(
        &mut self,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        mut top: f32,
    ) -> f32 {
        if clear == Clear::None {
            return top;
        }
        let mut cleared_continuations = 0usize;
        loop {
            let resolution = self
                .float_contexts
                .last()
                .expect("root float context exists")
                .clearance_resolution(
                    clear,
                    writing_mode,
                    direction,
                    self.current_float_page_index(),
                    top,
                );
            top = resolution.top;
            let Some(continued_float) = resolution.continued_float else {
                return top;
            };
            let next_page_index = self.current_float_page_index() + 1;
            let has_next_fragment = self
                .float_contexts
                .last()
                .expect("root float context exists")
                .shapes
                .iter()
                .any(|shape| {
                    shape.id == continued_float
                        && shape.page_index == next_page_index
                        && shape.starts_on_previous_page
                });
            if !has_next_fragment
                || cleared_continuations
                    > self
                        .float_contexts
                        .last()
                        .expect("root float context exists")
                        .shapes
                        .len()
            {
                return top;
            }
            self.cursor_y = top;
            self.push_page();
            top = self.cursor_y;
            cleared_continuations += 1;
        }
    }

    /// Return the lowest margin-box edge of floats in the current BFC fragment.
    ///
    /// CSS 2.2 makes auto-height block formatting context roots expand to
    /// include floats that belong to that root's formatting context:
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height>.
    pub(in crate::layout) fn current_float_context_lowest_bottom(&self) -> Option<f32> {
        self.float_contexts
            .last()
            .expect("root float context exists")
            .lowest_bottom_on_page(self.current_float_page_index())
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn build_float_paint_fragment(
        &mut self,
        id: FloatId,
        specified_side: Float,
        page_index: usize,
        side: UsedFloatSide,
        left: f32,
        right: f32,
        fallback_bounds: PaintClip,
        style: &ComputedStyle,
        fragment: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Option<FloatPaintFragment> {
        if fragment.is_empty() && child_contexts.is_empty() {
            return None;
        }

        let child_bounds = union_paint_bounds(child_contexts.iter().filter_map(|context| {
            context.bounds.map(|bounds| {
                context
                    .effects
                    .transform
                    .map(|transform| transform.apply_clip_to_aabb(bounds))
                    .unwrap_or(bounds)
            })
        }));
        let bounds = fragment
            .bounds()
            .map(|bounds| union_paint_bounds(child_bounds.into_iter().chain([bounds])).unwrap())
            .or(child_bounds)
            .unwrap_or(fallback_bounds);
        let source_order = self.next_paint_source_order();
        let policy = StackingContextPolicy::for_atomic(style, PaintBand::Float, bounds);
        let context = PaintStackingContext::from_banded_fragment(fragment, child_contexts)
            .with_source_order(source_order)
            .with_effects(policy.effects)
            .with_bounds(bounds);
        Some(FloatPaintFragment {
            id,
            specified_side,
            page_index,
            side,
            rect: PageTopRect::new(
                left,
                bounds.y() + bounds.height(),
                (right - left).max(0.0),
                bounds.height(),
            ),
            source_order,
            fragment_index: 0,
            starts_on_previous_page: false,
            continues_on_next_page: false,
            context,
        })
    }

    pub(in crate::layout) fn push_float_fragment_shape(
        &mut self,
        fragment: &FloatPaintFragment,
        run: &mut FloatRunState,
    ) {
        self.push_float_shape(FloatShape::from_fragment(fragment), run);
    }

    pub(in crate::layout) fn push_float_shape(
        &mut self,
        shape: FloatShape,
        run: &mut FloatRunState,
    ) {
        self.float_contexts
            .last_mut()
            .expect("root float context exists")
            .shapes
            .push(shape);
        run.include_shape(shape);
    }

    pub(in crate::layout) fn apply_pending_float_fragments_for_current_page(&mut self) {
        let page_index = self.pages.len();
        let mut pending_effects = Vec::new();
        let mut ready_effects = Vec::new();
        for effects in std::mem::take(&mut self.pending_float_side_effects) {
            if effects.page_index == page_index {
                ready_effects.push(effects);
            } else {
                pending_effects.push(effects);
            }
        }
        self.pending_float_side_effects = pending_effects;
        for effects in ready_effects {
            self.apply_float_page_side_effects(effects);
        }

        let mut pending = Vec::new();
        let mut ready = Vec::new();
        for fragment in std::mem::take(&mut self.pending_float_fragments) {
            if fragment.page_index == page_index {
                ready.push(fragment.fragment);
            } else {
                pending.push(fragment);
            }
        }
        self.pending_float_fragments = pending;
        for fragment in ready {
            self.current_page
                .append_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
        }
    }

    pub(in crate::layout) fn apply_float_page_side_effects(
        &mut self,
        effects: PendingFloatSideEffects,
    ) {
        merge_named_assignments(&mut self.current_page_named_strings, effects.named_strings);
        merge_named_assignments(
            &mut self.current_page_running_elements,
            effects.running_elements,
        );
        self.current_page.links.extend(effects.links);
    }

    pub(in crate::layout) fn apply_float_layout_side_effects(
        &mut self,
        effects: FloatLayoutSideEffects,
    ) {
        self.bookmarks.extend(effects.bookmarks);
        for (target, page_index) in effects.anchors {
            self.page_anchors.entry(target).or_insert(page_index);
        }
        for (target, text) in effects.anchor_text {
            self.page_anchor_text.entry(target).or_insert(text);
        }
        let current_page_index = self.pages.len();
        for page_effects in effects.page_effects {
            if page_effects.page_index == current_page_index {
                self.apply_float_page_side_effects(page_effects);
            } else {
                self.pending_float_side_effects.push(page_effects);
            }
        }
    }

    pub(in crate::layout) fn float_layout_side_effects_since(
        &self,
        snapshot: &LayoutSnapshot,
    ) -> FloatLayoutSideEffects {
        let mut effects = FloatLayoutSideEffects {
            bookmarks: self
                .bookmarks
                .iter()
                .skip(snapshot.bookmarks.len())
                .cloned()
                .collect(),
            anchors: self
                .page_anchors
                .iter()
                .filter(|(target, _)| !snapshot.page_anchors.contains_key(*target))
                .map(|(target, page_index)| (target.clone(), *page_index))
                .collect(),
            anchor_text: self
                .page_anchor_text
                .iter()
                .filter(|(target, _)| !snapshot.page_anchor_text.contains_key(*target))
                .map(|(target, text)| (target.clone(), text.clone()))
                .collect(),
            page_effects: Vec::new(),
        };

        let first_float_page = snapshot.pages.len();
        let captured_page_count = self
            .page_named_strings
            .len()
            .max(self.page_running_elements.len())
            .max(self.pages.len());
        for page_index in first_float_page..captured_page_count {
            let empty_named = HashMap::new();
            let empty_running = HashMap::new();
            let empty_links = Vec::new();
            let base_named = if page_index == first_float_page {
                &snapshot.current_page_named_strings
            } else {
                &empty_named
            };
            let base_running = if page_index == first_float_page {
                &snapshot.current_page_running_elements
            } else {
                &empty_running
            };
            let base_links = if page_index == first_float_page {
                &snapshot.current_page.links
            } else {
                &empty_links
            };
            let named_strings = self
                .page_named_strings
                .get(page_index)
                .map(|assignments| named_assignment_delta(base_named, assignments))
                .unwrap_or_default();
            let running_elements = self
                .page_running_elements
                .get(page_index)
                .map(|assignments| named_assignment_delta(base_running, assignments))
                .unwrap_or_default();
            let links = self
                .pages
                .get(page_index)
                .map(|page| {
                    if base_links.len() < page.links.len() {
                        page.links[base_links.len()..].to_vec()
                    } else {
                        Vec::new()
                    }
                })
                .unwrap_or_default();
            if !named_strings.is_empty() || !running_elements.is_empty() || !links.is_empty() {
                effects.page_effects.push(PendingFloatSideEffects {
                    page_index,
                    named_strings,
                    running_elements,
                    links,
                });
            }
        }

        let current_base_named;
        let current_base_running;
        let current_base_links;
        let (base_named, base_running, base_links) = if self.pages.len() == snapshot.pages.len() {
            (
                &snapshot.current_page_named_strings,
                &snapshot.current_page_running_elements,
                &snapshot.current_page.links,
            )
        } else {
            current_base_named = HashMap::new();
            current_base_running = HashMap::new();
            current_base_links = Vec::new();
            (
                &current_base_named,
                &current_base_running,
                &current_base_links,
            )
        };
        let current_named = named_assignment_delta(base_named, &self.current_page_named_strings);
        let current_running =
            named_assignment_delta(base_running, &self.current_page_running_elements);
        let current_links = if base_links.len() < self.current_page.links.len() {
            self.current_page.links[base_links.len()..].to_vec()
        } else {
            Vec::new()
        };
        if !current_named.is_empty() || !current_running.is_empty() || !current_links.is_empty() {
            effects.page_effects.push(PendingFloatSideEffects {
                page_index: self.pages.len(),
                named_strings: current_named,
                running_elements: current_running,
                links: current_links,
            });
        }

        effects
    }

    /// Finds the normal-flow margin-box position for a BFC root beside active floats.
    ///
    /// CSS 2.2 requires block formatting context roots, table wrappers, and
    /// replaced block boxes to avoid overlap with earlier floats in the same
    /// block formatting context; `clear` first moves the box below matching
    /// floats, then collision search finds the highest band wide enough for
    /// the margin box:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn place_float_avoiding_margin_box(
        &self,
        top: f32,
        width: f32,
        height: f32,
        clear: Clear,
        writing_mode: WritingMode,
        clear_direction: Direction,
        placement_direction: Direction,
    ) -> (f32, f32, f32) {
        if self.containing_block_writing_mode != WritingMode::HorizontalTb {
            let (left, top, available_inline_size) = self.find_vertical_float_avoiding_position(
                top,
                width,
                height,
                clear,
                writing_mode,
                clear_direction,
                None,
            );
            return (left, top, available_inline_size);
        }
        let (left, top, available_width) = self.find_float_avoiding_position(
            top,
            width,
            height,
            clear,
            writing_mode,
            clear_direction,
        );
        let x = if placement_direction == Direction::Rtl {
            left + (available_width - width).max(0.0)
        } else {
            left
        };
        (x, top, available_width)
    }

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
        placed_style.float = Float::None;

        let inline_size = (self.content_right - self.content_left).max(0.0);
        apply_used_box_metrics(&mut placed_style, inline_size);

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
            inline_size.margin_box_width,
            child_children,
        );
        if self.adjoining_float_origin_y.is_none() {
            self.prebreak_float_if_needed(margin_box_height);
        }
        let placement_top = self.adjoining_float_origin_y.unwrap_or(self.cursor_y);

        let (margin_box_left, top, _) =
            if matches!(float_side, UsedFloatSide::Top | UsedFloatSide::Bottom) {
                self.find_vertical_float_avoiding_position(
                    placement_top,
                    inline_size.margin_box_width,
                    margin_box_height,
                    placed_style.clear,
                    placed_style.writing_mode,
                    placed_style.direction,
                    Some(float_side),
                )
            } else {
                let (band_left, top, available_width) = self.find_float_avoiding_position(
                    placement_top,
                    inline_size.margin_box_width,
                    margin_box_height,
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

        let layout_snapshot = self.snapshot();
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_direction = self.containing_block_direction;
        let previous_writing_mode = self.containing_block_writing_mode;

        self.content_left = margin_box_left;
        self.content_right = margin_box_left + inline_size.margin_box_width.max(1.0);
        self.cursor_y = top;
        self.containing_block_direction = placed_style.direction;
        self.containing_block_writing_mode = placed_style.writing_mode;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        self.push_ancestor_signature(child_signature);
        self.push_page_name_scope_suppression();
        self.push_float_context();
        let previous_preserve_scoped_paint_public_order = self.preserve_scoped_paint_public_order;
        self.preserve_scoped_paint_public_order = true;
        self.layout_element_with_child_boxes_and_table_fragment(
            child_element,
            &placed_style,
            stylesheets,
            child_children,
            table_fragment,
        );
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
        if self.pages.len() == paint_page_index
            && let Some(fragment) = self
                .current_page
                .paint_tree_fragment_since(&paint_checkpoint)
        {
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
                &placed_style,
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
                    &placed_style,
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
            let side_effects = self.float_layout_side_effects_since(&layout_snapshot);
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
            self.apply_float_layout_side_effects(side_effects);
            for float_fragment in float_fragments {
                let paint_fragment = PaintFragment::from_stacking_context_in_band(
                    PaintBand::Float,
                    float_fragment.context.clone(),
                );
                if float_fragment.page_index == self.pages.len() {
                    self.current_page
                        .append_paint_fragment(&paint_fragment, PaintVector::new(0.0, 0.0));
                } else {
                    self.pending_float_fragments
                        .push(PendingFloatPaintFragment {
                            page_index: float_fragment.page_index,
                            fragment: paint_fragment,
                        });
                }
                self.push_float_fragment_shape(&float_fragment, run);
                recorded_float_shape = true;
            }
        }
        if !recorded_float_shape {
            self.push_float_shape(float_shape, run);
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.containing_block_direction = previous_direction;
        self.containing_block_writing_mode = previous_writing_mode;
        true
    }

    pub(in crate::layout) fn float_margin_box_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        containing_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        self.resolved_float_inline_size(
            element,
            style,
            stylesheets,
            containing_width,
            child_boxes,
            table_fragment,
        )
        .margin_box_width
    }

    pub(in crate::layout) fn resolved_float_inline_size(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        containing_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> ResolvedFloatInlineSize {
        let collapsed_table =
            style.display.is_table() && style.border_collapse == css::BorderCollapse::Collapse;
        let border_widths = if collapsed_table {
            css::Edges::ZERO
        } else {
            used_border_widths(style)
        };
        let horizontal_padding = if collapsed_table {
            0.0
        } else {
            style.padding.left + style.padding.right
        };
        let horizontal_extras =
            non_content_pt(border_widths.left + border_widths.right + horizontal_padding);
        let built_child_boxes;
        let built_table_fragment;
        let resolved_table_fragment = if style.display.is_table() {
            if let Some(fragment) = table_fragment {
                Some(fragment)
            } else {
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
                let signature = self
                    .ancestors
                    .last()
                    .cloned()
                    .unwrap_or_else(|| element_signature(element));
                built_table_fragment =
                    box_tree::build_frozen_table_fragment(element, &signature, table_children);
                Some(&built_table_fragment)
            }
        } else {
            table_fragment
        };
        let available_outer_width =
            (containing_width - style.margin.left - style.margin.right).max(0.0);
        let content_width = used_content_box_size(
            style.box_values.width,
            style.box_sizing,
            available_outer_width,
            horizontal_extras,
        )
        .unwrap_or_else(|| {
            let content_available_width =
                (available_outer_width - horizontal_extras.points()).max(0.0);
            let (preferred_min, preferred) = self.formatting_context_intrinsic_widths(
                element,
                style,
                stylesheets,
                content_available_width,
                child_boxes,
                resolved_table_fragment,
            );
            intrinsic::content_box_width_from_intrinsic(
                style,
                available_outer_width,
                horizontal_extras,
                content_box_pt(preferred_min),
                content_box_pt(preferred),
                intrinsic::IntrinsicAutoWidth::ShrinkToFit,
            )
        });
        let content_width = content_box_pt(constrain_width(
            style,
            content_width.points(),
            available_outer_width,
        ));
        let visual_horizontal_extras = if collapsed_table {
            self.collapsed_table_outer_horizontal_insets(
                style,
                stylesheets,
                resolved_table_fragment,
            )
            .unwrap_or(0.0)
        } else {
            0.0
        };
        resolved_float_inline_size_from_content_box(
            style,
            content_width,
            horizontal_extras,
            visual_horizontal_extras,
        )
    }

    /// Return the floated margin-box block size used for placement.
    ///
    /// CSS 2.2 makes auto-height floating non-replaced elements use the same
    /// descendant-based height calculation as BFC roots. Empty floats therefore
    /// have zero content height instead of reserving a synthetic line box:
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height>.
    pub(in crate::layout) fn float_margin_box_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        margin_box_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> f32 {
        self.estimate_element_height(element, style, stylesheets, margin_box_width, child_boxes)
            .unwrap_or(0.0)
    }

    pub(in crate::layout) fn find_float_avoiding_position(
        &self,
        top: f32,
        width: f32,
        height: f32,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
    ) -> (f32, f32, f32) {
        let page_index = self.current_float_page_index();
        let context = self
            .float_contexts
            .last()
            .expect("root float context exists");
        let placement = context.avoiding_position(
            page_index,
            top,
            width,
            height,
            clear,
            writing_mode,
            direction,
            self.content_left,
            self.content_right,
        );
        (
            placement.left(),
            placement.top(),
            placement.available_width(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn find_vertical_float_avoiding_position(
        &self,
        top: f32,
        width: f32,
        height: f32,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        side: Option<UsedFloatSide>,
    ) -> (f32, f32, f32) {
        let page_index = self.current_float_page_index();
        let context = self
            .float_contexts
            .last()
            .expect("root float context exists");
        let avoidance_writing_mode = self.containing_block_writing_mode;
        let placement = context.vertical_avoiding_position(
            page_index,
            top,
            width,
            height,
            clear,
            writing_mode,
            avoidance_writing_mode,
            direction,
            self.content_left,
            self.page_bottom(),
            side,
        );
        (
            placement.left(),
            placement.top(),
            placement.available_width(),
        )
    }

    pub(in crate::layout) fn prebreak_float_if_needed(&mut self, margin_box_height: f32) {
        if margin_box_height <= FLOAT_EPSILON
            || self.cursor_y - margin_box_height >= self.page_bottom() - FLOAT_EPSILON
            || self.cursor_is_at_page_top()
            || !self.current_page_has_content()
        {
            return;
        }
        self.push_page();
    }

    pub(in crate::layout) fn prebreak_bfc_margin_box_if_needed(
        &mut self,
        margin_box_height: f32,
        reapplied_margin_top: f32,
    ) {
        // Out-of-flow roots resolve their static/absolute position first; the
        // normal-flow BFC prebreak heuristic would incorrectly move a
        // bottom-anchored positioned table or block to the next page.
        if margin_box_height <= FLOAT_EPSILON
            || self.out_of_flow_prebreak_suppression_depth > 0
            || margin_box_height > self.page_area_height() + FLOAT_EPSILON
            || self.cursor_y - margin_box_height >= self.page_bottom() - FLOAT_EPSILON
            || self.cursor_is_at_page_top()
            || !self.current_page_has_content()
        {
            return;
        }
        self.push_page();
        self.cursor_y -= reapplied_margin_top;
    }

    pub(in crate::layout) fn current_float_page_index(&self) -> usize {
        self.pages.len()
    }
}

fn resolved_float_inline_size_from_content_box(
    style: &ComputedStyle,
    content_width: ContentBoxLength,
    horizontal_extras: NonContentLength,
    visual_horizontal_extras: f32,
) -> ResolvedFloatInlineSize {
    let border_box_width = content_box_to_border_box_length(content_width, horizontal_extras);
    ResolvedFloatInlineSize {
        content_width,
        border_box_width,
        margin_box_width: style.margin.left
            + border_box_width.points()
            + visual_horizontal_extras
            + style.margin.right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn length_points(value: css::ComputedLengthPercentageOrAuto) -> f32 {
        match value {
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => value.length_points(),
            _ => panic!("expected resolved length"),
        }
    }

    #[test]
    fn float_inline_size_expands_content_box_to_border_and_margin_boxes() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = length(150.0);
        style.margin.left = 5.0;
        style.margin.right = 7.0;

        let inline_size = resolved_float_inline_size_from_content_box(
            &style,
            content_box_pt(150.0),
            non_content_pt(20.0),
            0.0,
        );

        assert_eq!(inline_size.content_width.points(), 150.0);
        assert_eq!(inline_size.border_box_width.points(), 170.0);
        assert_eq!(inline_size.margin_box_width, 182.0);
    }

    #[test]
    fn border_box_float_width_clamps_content_and_keeps_extras_once() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.width = length(100.0);
        let extras = non_content_pt(150.0);
        let content_width =
            used_content_box_size(style.box_values.width, style.box_sizing, 300.0, extras).unwrap();

        let inline_size =
            resolved_float_inline_size_from_content_box(&style, content_width, extras, 0.0);

        assert_eq!(inline_size.content_width.points(), 0.0);
        assert_eq!(inline_size.border_box_width.points(), 150.0);
        assert_eq!(inline_size.margin_box_width, 150.0);
    }

    #[test]
    fn auto_float_shrink_to_fit_returns_content_box_length() {
        let style = ComputedStyle::initial();
        let width = intrinsic::content_box_width_from_intrinsic(
            &style,
            150.0,
            non_content_pt(20.0),
            content_box_pt(80.0),
            content_box_pt(200.0),
            intrinsic::IntrinsicAutoWidth::ShrinkToFit,
        );

        let _typed: ContentBoxLength = width;
        assert_eq!(width.points(), 130.0);
    }

    #[test]
    fn freeze_float_replay_width_writes_box_sizing_specific_used_width() {
        let inline_size = ResolvedFloatInlineSize {
            content_width: content_box_pt(80.0),
            border_box_width: border_box_pt(120.0),
            margin_box_width: 120.0,
        };
        let mut content_box_style = ComputedStyle::initial();
        content_box_style.box_sizing = BoxSizing::ContentBox;
        freeze_float_replay_width(&mut content_box_style, inline_size);

        let mut border_box_style = ComputedStyle::initial();
        border_box_style.box_sizing = BoxSizing::BorderBox;
        freeze_float_replay_width(&mut border_box_style, inline_size);

        assert_eq!(length_points(content_box_style.box_values.width), 80.0);
        assert_eq!(length_points(content_box_style.box_values.min_width), 80.0);
        assert_eq!(length_points(content_box_style.box_values.max_width), 80.0);
        assert_eq!(length_points(border_box_style.box_values.width), 120.0);
        assert_eq!(length_points(border_box_style.box_values.min_width), 80.0);
        assert_eq!(length_points(border_box_style.box_values.max_width), 80.0);
    }
}
