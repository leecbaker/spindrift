use super::super::*;
use crate::layout::assets::paint_effects_for_box;

const FLOAT_EPSILON: f32 = 0.01;

impl FloatRunState {
    fn new(left_x: f32, right_x: f32, row_top: f32) -> Self {
        Self {
            row_left: left_x,
            row_right: right_x,
            left_x,
            right_x,
            row_top,
            row_bottom: row_top,
            active: false,
        }
    }

    fn include_shape(&mut self, shape: FloatShape) {
        if (shape.top - self.row_top).abs() > 0.5 {
            return;
        }
        match shape.side {
            Float::Left => self.left_x = self.left_x.max(shape.right),
            Float::Right => self.right_x = self.right_x.min(shape.left),
            Float::None | Float::InlineStart | Float::InlineEnd => {}
        }
        self.row_bottom = self.row_bottom.min(shape.bottom);
        self.active = true;
    }

    fn reset_for_block(&mut self, left_x: f32, right_x: f32, row_top: f32) {
        *self = Self::new(left_x, right_x, row_top);
    }
}

impl FloatContext {
    fn active_shapes(
        &self,
        page_index: usize,
        top: f32,
        height: f32,
    ) -> impl Iterator<Item = FloatShape> + '_ {
        let bottom = top - height.max(0.0);
        self.shapes.iter().copied().filter(move |shape| {
            shape.page_index == page_index
                && shape.top > bottom + FLOAT_EPSILON
                && shape.bottom < top - FLOAT_EPSILON
        })
    }

    fn band(&self, page_index: usize, top: f32, height: f32, left: f32, right: f32) -> FloatBand {
        let mut band = FloatBand { left, right };
        for shape in self.active_shapes(page_index, top, height) {
            match shape.side {
                Float::Left => band.left = band.left.max(shape.right),
                Float::Right => band.right = band.right.min(shape.left),
                Float::None | Float::InlineStart | Float::InlineEnd => {}
            }
        }
        if band.right < band.left {
            band.right = band.left;
        }
        band
    }

    fn clearance_top(
        &self,
        clear: Clear,
        direction: Direction,
        page_index: usize,
        top: f32,
    ) -> f32 {
        if clear == Clear::None {
            return top;
        }
        self.shapes
            .iter()
            .filter(|shape| {
                shape.page_index == page_index
                    && clear.matches_float(shape.side, direction)
                    && shape.bottom < top + FLOAT_EPSILON
            })
            .map(|shape| shape.bottom)
            .fold(top, f32::min)
    }
}

fn union_paint_bounds(bounds: impl IntoIterator<Item = PaintClip>) -> Option<PaintClip> {
    bounds.into_iter().fold(None, |acc, bounds| {
        Some(match acc {
            Some(existing) => union_paint_clip(existing, bounds),
            None => bounds,
        })
    })
}

fn union_paint_clip(left: PaintClip, right: PaintClip) -> PaintClip {
    let x1 = left.x.min(right.x);
    let x2 = (left.x + left.width).max(right.x + right.width);
    let y1 = left.y.min(right.y);
    let y2 = (left.y + left.height).max(right.y + right.height);
    PaintClip {
        x: x1,
        y: y1,
        width: (x2 - x1).max(0.0),
        height: (y2 - y1).max(0.0),
    }
}

impl<'a> LayoutBuilder<'a> {
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
                top,
                height,
                self.content_left,
                self.content_right,
            )
    }

    pub(in crate::layout) fn active_float_exclusions_at(&self, top: f32, height: f32) -> bool {
        let band = self.current_float_band(top, height);
        band.left > self.content_left + FLOAT_EPSILON
            || band.right < self.content_right - FLOAT_EPSILON
    }

    pub(in crate::layout) fn clear_active_floats_top(
        &self,
        clear: Clear,
        direction: Direction,
        top: f32,
    ) -> f32 {
        self.float_contexts
            .last()
            .expect("root float context exists")
            .clearance_top(clear, direction, self.current_float_page_index(), top)
    }

    #[allow(clippy::too_many_arguments)]
    fn build_float_paint_fragment(
        &mut self,
        page_index: usize,
        side: Float,
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
        let context = PaintStackingContext::from_banded_fragment(fragment, child_contexts)
            .with_source_order(source_order)
            .with_effects(paint_effects_for_box(style, bounds))
            .with_bounds(bounds);
        Some(FloatPaintFragment {
            page_index,
            side,
            left,
            right,
            top: bounds.y + bounds.height,
            bottom: bounds.y,
            source_order,
            context,
        })
    }

    fn push_float_fragment_shape(
        &mut self,
        fragment: &FloatPaintFragment,
        run: &mut FloatRunState,
    ) {
        let shape = FloatShape {
            side: fragment.side,
            page_index: fragment.page_index,
            left: fragment.left,
            right: fragment.right,
            top: fragment.top,
            bottom: fragment.bottom,
        };
        self.float_contexts
            .last_mut()
            .expect("root float context exists")
            .shapes
            .push(shape);
        run.include_shape(shape);
    }

    pub(in crate::layout) fn apply_pending_float_fragments_for_current_page(&mut self) {
        let page_index = self.pages.len();
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
            self.current_page.append_paint_fragment(&fragment, 0.0, 0.0);
        }
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
    pub(in crate::layout) fn place_float_avoiding_margin_box(
        &self,
        top: f32,
        width: f32,
        height: f32,
        clear: Clear,
        direction: Direction,
    ) -> (f32, f32, f32) {
        let (left, top, available_width) =
            self.find_float_avoiding_position(top, width, height, clear, direction);
        let x = if direction == Direction::Rtl {
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
    pub(in crate::layout) fn layout_floating_child(
        &mut self,
        child_element: &Element,
        child_signature: ElementSignature,
        child_style: &ComputedStyle,
        child_children: Option<&[box_tree::FormattingBox<'_>]>,
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
        let float_side = placed_style.float.physical(self.containing_block_direction);
        placed_style.float = Float::None;

        let inline_size = (self.content_right - self.content_left).max(0.0);
        let used_edges = used_box_edges(&placed_style, inline_size);
        placed_style.margin = used_edges.margin.to_css_edges();
        placed_style.padding = used_edges.padding.to_css_edges();

        let margin_box_width = self.float_margin_box_width(
            child_element,
            &placed_style,
            stylesheets,
            inline_size,
            child_children,
        );
        let margin_box_height = self.float_margin_box_height(
            child_element,
            &placed_style,
            stylesheets,
            margin_box_width,
            child_children,
        );
        self.prebreak_float_if_needed(margin_box_height);

        let (band_left, top, available_width) = self.find_float_avoiding_position(
            self.cursor_y,
            margin_box_width,
            margin_box_height,
            placed_style.clear,
            self.containing_block_direction,
        );
        let margin_box_left = match float_side {
            Float::Left => band_left,
            Float::Right => band_left + (available_width - margin_box_width).max(0.0),
            Float::None | Float::InlineStart | Float::InlineEnd => {
                unreachable!("float side was normalized above")
            }
        };

        let layout_snapshot = self.snapshot();
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_direction = self.containing_block_direction;

        self.content_left = margin_box_left;
        self.content_right = margin_box_left + margin_box_width.max(1.0);
        self.cursor_y = top;
        self.containing_block_direction = placed_style.direction;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        self.push_ancestor_signature(child_signature);
        self.push_page_name_scope_suppression();
        self.push_float_context();
        let previous_preserve_scoped_paint_public_order = self.preserve_scoped_paint_public_order;
        self.preserve_scoped_paint_public_order = true;
        self.layout_element_with_child_boxes(
            child_element,
            &placed_style,
            stylesheets,
            child_children,
        );
        self.preserve_scoped_paint_public_order = previous_preserve_scoped_paint_public_order;
        self.pop_float_context();
        self.pop_page_name_scope_suppression();
        self.ancestors.pop();

        let actual_bottom = self.cursor_y.min(top - margin_box_height);
        let child_layers = if positioned_layer_start < self.positioned_layers.len() {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        let float_bounds = PaintClip {
            x: margin_box_left,
            y: actual_bottom,
            width: margin_box_width,
            height: (top - actual_bottom).max(0.0),
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
                paint_page_index,
                float_side,
                margin_box_left,
                margin_box_left + margin_box_width,
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
                    page_index,
                    float_side,
                    margin_box_left,
                    margin_box_left + margin_box_width,
                    float_bounds,
                    &placed_style,
                    fragment,
                    child_contexts,
                ) {
                    float_fragments.push(float_fragment);
                }
            }
            float_fragments.sort_by_key(|fragment| (fragment.page_index, fragment.source_order));
            let next_paint_source_order = self.next_paint_source_order;
            self.restore(layout_snapshot);
            self.next_paint_source_order = next_paint_source_order;
            for float_fragment in float_fragments {
                let paint_fragment = PaintFragment::from_stacking_context_in_band(
                    PaintBand::Float,
                    float_fragment.context.clone(),
                );
                if float_fragment.page_index == self.pages.len() {
                    self.current_page
                        .append_paint_fragment(&paint_fragment, 0.0, 0.0);
                } else {
                    self.pending_float_fragments
                        .push(PendingFloatPaintFragment {
                            page_index: float_fragment.page_index,
                            fragment: paint_fragment,
                        });
                }
                self.push_float_fragment_shape(&float_fragment, run);
            }
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.containing_block_direction = previous_direction;
        true
    }

    pub(in crate::layout) fn float_margin_box_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        containing_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> f32 {
        let border_widths = used_border_widths(style);
        let horizontal_extras =
            border_widths.left + border_widths.right + style.padding.left + style.padding.right;
        let available_outer_width =
            (containing_width - style.margin.left - style.margin.right).max(0.0);
        let content_width =
            used_content_width_or_auto(style, available_outer_width, horizontal_extras)
                .unwrap_or_else(|| {
                    self.estimate_shrink_to_fit_width(
                        element,
                        style,
                        stylesheets,
                        available_outer_width,
                        child_boxes,
                        None,
                    )
                });
        let content_width = constrain_width(style, content_width, available_outer_width);
        style.margin.left + content_width + horizontal_extras + style.margin.right
    }

    fn float_margin_box_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        margin_box_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> f32 {
        self.estimate_element_height(element, style, stylesheets, margin_box_width, child_boxes)
            .unwrap_or(style.line_height)
            .max(style.line_height)
    }

    fn find_float_avoiding_position(
        &self,
        top: f32,
        width: f32,
        height: f32,
        clear: Clear,
        direction: Direction,
    ) -> (f32, f32, f32) {
        let page_index = self.current_float_page_index();
        let context = self
            .float_contexts
            .last()
            .expect("root float context exists");
        let mut top = context.clearance_top(clear, direction, page_index, top);
        if let Some(last) = context
            .shapes
            .iter()
            .rev()
            .find(|shape| shape.page_index == page_index)
        {
            top = top.min(last.top);
        }

        for _ in 0..context.shapes.len().saturating_add(2) {
            let band = context.band(
                page_index,
                top,
                height,
                self.content_left,
                self.content_right,
            );
            let available_width = (band.right - band.left).max(0.0);
            if width <= available_width + FLOAT_EPSILON {
                return (band.left, top, available_width);
            }

            let next_top = context
                .active_shapes(page_index, top, height)
                .map(|shape| shape.bottom)
                .fold(top, f32::min);
            if next_top >= top - FLOAT_EPSILON {
                return (band.left, top, available_width);
            }
            top = next_top;
        }

        let band = context.band(
            page_index,
            top,
            height,
            self.content_left,
            self.content_right,
        );
        (band.left, top, (band.right - band.left).max(0.0))
    }

    fn prebreak_float_if_needed(&mut self, margin_box_height: f32) {
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
        if margin_box_height <= FLOAT_EPSILON
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

    fn current_float_page_index(&self) -> usize {
        self.pages.len()
    }
}
