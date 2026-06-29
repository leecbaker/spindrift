use super::super::*;

const FLOAT_EPSILON: f32 = 0.01;

impl FloatRunState {
    fn new(left_x: f32, right_x: f32, row_top: f32) -> Self {
        let row_span = PageInlineSpan::from_edges(left_x, right_x);
        Self {
            row_span,
            available_span: row_span,
            occupied_block_span: PageBlockSpan::from_edges(row_top, row_top),
            active: false,
        }
    }

    fn include_shape(&mut self, shape: FloatShape) {
        if (shape.top() - self.occupied_block_span.top_y()).abs() > 0.5 {
            return;
        }
        let mut left_x = self.available_span.left_x();
        let mut right_x = self.available_span.right_x();
        match shape.side {
            UsedFloatSide::Left => left_x = left_x.max(shape.right()),
            UsedFloatSide::Right => right_x = right_x.min(shape.left()),
            UsedFloatSide::Top | UsedFloatSide::Bottom => {}
        }
        self.available_span = PageInlineSpan::from_edges(left_x, right_x);
        self.occupied_block_span = PageBlockSpan::from_edges(
            self.occupied_block_span.top_y(),
            self.occupied_block_span.bottom_y().min(shape.bottom()),
        );
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
        block_span: PageBlockSpan,
    ) -> impl Iterator<Item = FloatShape> + '_ {
        self.shapes.iter().copied().filter(move |shape| {
            shape.page_index == page_index
                && shape.top() > block_span.bottom_y() + FLOAT_EPSILON
                && shape.bottom() < block_span.top_y() - FLOAT_EPSILON
        })
    }

    fn band(
        &self,
        page_index: usize,
        block_span: PageBlockSpan,
        inline_span: PageInlineSpan,
    ) -> FloatBand {
        let mut band_left = inline_span.left_x();
        let mut band_right = inline_span.right_x();
        for shape in self.active_shapes(page_index, block_span) {
            match shape.side {
                UsedFloatSide::Left => band_left = band_left.max(shape.right()),
                UsedFloatSide::Right => band_right = band_right.min(shape.left()),
                UsedFloatSide::Top | UsedFloatSide::Bottom => {}
            }
        }
        if band_right < band_left {
            band_right = band_left;
        }
        FloatBand::from_edges(band_left, band_right)
    }

    #[allow(clippy::too_many_arguments)]
    fn logical_band(
        &self,
        writing_mode: WritingMode,
        direction: Direction,
        page_index: usize,
        block_start: f32,
        block_size: f32,
        inline_physical_start: f32,
        inline_size: f32,
    ) -> LogicalFloatBand {
        match writing_mode {
            WritingMode::HorizontalTb => {
                let left = block_start;
                let right = block_start + block_size.max(0.0);
                let band = self.band(
                    page_index,
                    PageBlockSpan::new(inline_physical_start, inline_size),
                    PageInlineSpan::from_edges(left, right),
                );
                let (inline_start, inline_end) = match direction {
                    Direction::Ltr => (band.left() - left, band.right() - left),
                    Direction::Rtl => (right - band.right(), right - band.left()),
                };
                let inline_start = inline_start.max(0.0);
                let inline_end = inline_end.max(inline_start).min((right - left).max(0.0));
                LogicalFloatBand::new(
                    inline_start,
                    inline_end - inline_start,
                    inline_physical_start,
                    inline_physical_start - inline_size.max(0.0),
                )
            }
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                let slab_left = block_start;
                let slab_right = block_start + block_size.max(0.0);
                let inline_start_side = inline_start_side(writing_mode, direction);
                let (span_top, span_bottom) = match inline_start_side {
                    PhysicalSide::Top => (
                        inline_physical_start,
                        inline_physical_start - inline_size.max(0.0),
                    ),
                    PhysicalSide::Bottom => (
                        inline_physical_start + inline_size.max(0.0),
                        inline_physical_start,
                    ),
                    PhysicalSide::Left | PhysicalSide::Right => {
                        unreachable!("vertical inline axis must map to top or bottom")
                    }
                };
                let mut band_top = span_top;
                let mut band_bottom = span_bottom;
                for shape in self.shapes.iter().copied().filter(|shape| {
                    shape.page_index == page_index
                        && shape.right() > slab_left + FLOAT_EPSILON
                        && shape.left() < slab_right - FLOAT_EPSILON
                        && shape.top() > span_bottom + FLOAT_EPSILON
                        && shape.bottom() < span_top - FLOAT_EPSILON
                }) {
                    match shape.side {
                        UsedFloatSide::Top => band_top = band_top.min(shape.bottom()),
                        UsedFloatSide::Bottom => band_bottom = band_bottom.max(shape.top()),
                        UsedFloatSide::Left | UsedFloatSide::Right => {}
                    }
                }
                if band_bottom > band_top {
                    band_bottom = band_top;
                }
                let (inline_start, inline_end) = match inline_start_side {
                    PhysicalSide::Top => (span_top - band_top, span_top - band_bottom),
                    PhysicalSide::Bottom => (band_bottom - span_bottom, band_top - span_bottom),
                    PhysicalSide::Left | PhysicalSide::Right => unreachable!(),
                };
                let inline_start = inline_start.max(0.0);
                let inline_end = inline_end.max(inline_start).min(inline_size.max(0.0));
                LogicalFloatBand::new(
                    inline_start,
                    inline_end - inline_start,
                    band_top,
                    band_bottom,
                )
            }
        }
    }

    fn clearance_top(
        &self,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        page_index: usize,
        top: f32,
    ) -> f32 {
        self.clearance_resolution(clear, writing_mode, direction, page_index, top)
            .top
    }

    /// Resolve page-local clearance against matching floats.
    ///
    /// CSS 2.2 defines `clear` by moving the hypothetical border edge below
    /// earlier matching floats in the same block formatting context. In paged
    /// layout a float may have a page-local fragment that continues into a
    /// later fragmentainer, so callers also need to know whether page progress
    /// is required before clearance is complete:
    /// <https://www.w3.org/TR/CSS22/visuren.html#flow-control> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn clearance_resolution(
        &self,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        page_index: usize,
        top: f32,
    ) -> FloatClearanceResolution {
        if clear == Clear::None {
            return FloatClearanceResolution {
                top,
                continued_float: None,
            };
        }
        let mut cleared_top = top;
        let mut continued_float = None;
        for shape in self.shapes.iter().filter(|shape| {
            shape.page_index == page_index
                && shape.side.matches_clear(clear, writing_mode, direction)
                && shape.bottom() < top + FLOAT_EPSILON
        }) {
            cleared_top = cleared_top.min(shape.bottom());
            if shape.continues_on_next_page {
                continued_float = Some(shape.id);
            }
        }
        FloatClearanceResolution {
            top: cleared_top,
            continued_float,
        }
    }

    fn lowest_bottom_on_page(&self, page_index: usize) -> Option<f32> {
        self.shapes
            .iter()
            .filter(|shape| shape.page_index == page_index)
            .map(|shape| shape.bottom())
            .reduce(f32::min)
    }

    #[allow(clippy::too_many_arguments)]
    fn avoiding_position(
        &self,
        page_index: usize,
        top: f32,
        width: f32,
        height: f32,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        left: f32,
        right: f32,
    ) -> FloatPlacement {
        let mut top = self.clearance_top(clear, writing_mode, direction, page_index, top);
        if let Some(last) = self
            .shapes
            .iter()
            .rev()
            .find(|shape| shape.page_index == page_index)
        {
            top = top.min(last.top());
        }

        for _ in 0..self.shapes.len().saturating_add(2) {
            let block_span = PageBlockSpan::new(top, height);
            let inline_span = PageInlineSpan::from_edges(left, right);
            let band = self.band(page_index, block_span, inline_span);
            let available_width = band.width();
            if width <= available_width + FLOAT_EPSILON {
                return FloatPlacement::new(band.left(), top, available_width);
            }

            let next_top = self
                .active_shapes(page_index, block_span)
                .map(|shape| shape.bottom())
                .fold(top, f32::min);
            if next_top >= top - FLOAT_EPSILON {
                return FloatPlacement::new(band.left(), top, available_width);
            }
            top = next_top;
        }

        let band = self.band(
            page_index,
            PageBlockSpan::new(top, height),
            PageInlineSpan::from_edges(left, right),
        );
        FloatPlacement::new(band.left(), top, band.width())
    }

    #[allow(clippy::too_many_arguments)]
    fn vertical_avoiding_position(
        &self,
        page_index: usize,
        top: f32,
        width: f32,
        height: f32,
        clear: Clear,
        clear_writing_mode: WritingMode,
        avoidance_writing_mode: WritingMode,
        direction: Direction,
        left: f32,
        page_bottom: f32,
        side: Option<UsedFloatSide>,
    ) -> FloatPlacement {
        let top = self.clearance_top(clear, clear_writing_mode, direction, page_index, top);
        let inline_size = (top - page_bottom).max(height).max(1.0);
        let block_size = width.max(1.0);
        let mut block_start = left;

        for _ in 0..self.shapes.len().saturating_add(2) {
            let (span_top, span_bottom) =
                vertical_physical_inline_span(avoidance_writing_mode, direction, top, inline_size);
            let band = self.logical_band(
                avoidance_writing_mode,
                direction,
                page_index,
                block_start,
                block_size,
                top,
                inline_size,
            );
            let placed_top = match side {
                Some(UsedFloatSide::Bottom) => band.physical_bottom() + height,
                Some(UsedFloatSide::Top) | None => band.physical_top(),
                Some(UsedFloatSide::Left | UsedFloatSide::Right) => band.physical_top(),
            };
            if height <= band.available_inline_size() + FLOAT_EPSILON {
                return FloatPlacement::new(block_start, placed_top, band.available_inline_size());
            }

            let Some(next_start) = self.next_vertical_float_slab_start(
                page_index,
                block_start,
                block_size,
                span_top,
                span_bottom,
            ) else {
                return FloatPlacement::new(block_start, placed_top, band.available_inline_size());
            };
            if next_start <= block_start + FLOAT_EPSILON {
                return FloatPlacement::new(block_start, placed_top, band.available_inline_size());
            }
            block_start = next_start;
        }

        let band = self.logical_band(
            avoidance_writing_mode,
            direction,
            page_index,
            block_start,
            block_size,
            top,
            inline_size,
        );
        let placed_top = match side {
            Some(UsedFloatSide::Bottom) => band.physical_bottom() + height,
            Some(UsedFloatSide::Top) | None => band.physical_top(),
            Some(UsedFloatSide::Left | UsedFloatSide::Right) => band.physical_top(),
        };
        FloatPlacement::new(block_start, placed_top, band.available_inline_size())
    }

    fn next_vertical_float_slab_start(
        &self,
        page_index: usize,
        block_start: f32,
        block_size: f32,
        physical_top: f32,
        physical_bottom: f32,
    ) -> Option<f32> {
        let block_end = block_start + block_size.max(0.0);
        self.shapes
            .iter()
            .filter(|shape| {
                shape.page_index == page_index
                    && matches!(shape.side, UsedFloatSide::Top | UsedFloatSide::Bottom)
                    && shape.right() > block_start + FLOAT_EPSILON
                    && shape.left() < block_end - FLOAT_EPSILON
                    && shape.top() > physical_bottom + FLOAT_EPSILON
                    && shape.bottom() < physical_top - FLOAT_EPSILON
            })
            .map(|shape| shape.right())
            .filter(|next_start| *next_start > block_start + FLOAT_EPSILON)
            .min_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
    }
}

fn vertical_physical_inline_span(
    writing_mode: WritingMode,
    direction: Direction,
    inline_physical_start: f32,
    inline_size: f32,
) -> (f32, f32) {
    match inline_start_side(writing_mode, direction) {
        PhysicalSide::Top => (
            inline_physical_start,
            inline_physical_start - inline_size.max(0.0),
        ),
        PhysicalSide::Bottom => (
            inline_physical_start + inline_size.max(0.0),
            inline_physical_start,
        ),
        PhysicalSide::Left | PhysicalSide::Right => {
            unreachable!("vertical inline axis must map to top or bottom")
        }
    }
}

fn named_assignment_delta(
    before: &HashMap<String, Vec<NamedStringAssignment>>,
    after: &HashMap<String, Vec<NamedStringAssignment>>,
) -> HashMap<String, Vec<NamedStringAssignment>> {
    let mut delta = HashMap::new();
    for (name, assignments) in after {
        let before_len = before.get(name).map(Vec::len).unwrap_or(0);
        if before_len < assignments.len() {
            delta.insert(name.clone(), assignments[before_len..].to_vec());
        }
    }
    delta
}

fn merge_named_assignments(
    target: &mut HashMap<String, Vec<NamedStringAssignment>>,
    source: HashMap<String, Vec<NamedStringAssignment>>,
) {
    for (name, mut assignments) in source {
        target.entry(name).or_default().append(&mut assignments);
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
    let x1 = left.x().min(right.x());
    let x2 = (left.x() + left.width()).max(right.x() + right.width());
    let y1 = left.y().min(right.y());
    let y2 = (left.y() + left.height()).max(right.y() + right.height());
    PaintClip::from_paint_rect(paint_space_rect(x1, y1, x2 - x1, y2 - y1))
}

impl<'a> LayoutBuilder<'a> {
    fn next_float_id(&mut self) -> FloatId {
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
    fn build_float_paint_fragment(
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

    fn push_float_fragment_shape(
        &mut self,
        fragment: &FloatPaintFragment,
        run: &mut FloatRunState,
    ) {
        let shape = FloatShape::from_fragment(fragment);
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

    fn apply_float_page_side_effects(&mut self, effects: PendingFloatSideEffects) {
        merge_named_assignments(&mut self.current_page_named_strings, effects.named_strings);
        merge_named_assignments(
            &mut self.current_page_running_elements,
            effects.running_elements,
        );
        self.current_page.links.extend(effects.links);
    }

    fn apply_float_layout_side_effects(&mut self, effects: FloatLayoutSideEffects) {
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

    fn float_layout_side_effects_since(&self, snapshot: &LayoutSnapshot) -> FloatLayoutSideEffects {
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

        let (margin_box_left, top, _) =
            if matches!(float_side, UsedFloatSide::Top | UsedFloatSide::Bottom) {
                self.find_vertical_float_avoiding_position(
                    self.cursor_y,
                    margin_box_width,
                    margin_box_height,
                    placed_style.clear,
                    placed_style.writing_mode,
                    placed_style.direction,
                    Some(float_side),
                )
            } else {
                let (band_left, top, available_width) = self.find_float_avoiding_position(
                    self.cursor_y,
                    margin_box_width,
                    margin_box_height,
                    placed_style.clear,
                    placed_style.writing_mode,
                    placed_style.direction,
                );
                let margin_box_left = match float_side {
                    UsedFloatSide::Left => band_left,
                    UsedFloatSide::Right => {
                        band_left + (available_width - margin_box_width).max(0.0)
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
        self.content_right = margin_box_left + margin_box_width.max(1.0);
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
        let float_bounds = PageTopRect::new(
            margin_box_left,
            top,
            margin_box_width,
            (top - actual_bottom).max(0.0),
        )
        .paint_clip();
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
                    float_id,
                    specified_side,
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
            }
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
    ) -> f32 {
        let border_widths = used_border_widths(style);
        let horizontal_extras =
            border_widths.left + border_widths.right + style.padding.left + style.padding.right;
        let available_outer_width =
            (containing_width - style.margin.left - style.margin.right).max(0.0);
        let content_width =
            used_content_width_or_auto(style, available_outer_width, horizontal_extras)
                .unwrap_or_else(|| {
                    self.used_intrinsic_or_shrink_to_fit_width(
                        element,
                        style,
                        stylesheets,
                        available_outer_width,
                        horizontal_extras,
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
    fn find_vertical_float_avoiding_position(
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

    fn current_float_page_index(&self) -> usize {
        self.pages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(
        side: Float,
        page_index: usize,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> FloatShape {
        shape_with_used_side(
            side,
            UsedFloatSide::from_float(side, WritingMode::HorizontalTb, Direction::Ltr).unwrap(),
            page_index,
            left,
            right,
            top,
            bottom,
        )
    }

    fn shape_with_used_side(
        specified_side: Float,
        side: UsedFloatSide,
        page_index: usize,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> FloatShape {
        FloatShape::from_edges(
            FloatId(1),
            specified_side,
            side,
            1,
            0,
            false,
            false,
            page_index,
            left,
            right,
            top,
            bottom,
        )
    }

    fn continued_shape(
        side: Float,
        page_index: usize,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> FloatShape {
        let mut shape = shape(side, page_index, left, right, top, bottom);
        shape.starts_on_previous_page = true;
        shape.fragment_index = 1;
        shape
    }

    #[test]
    fn float_band_combines_active_left_and_right_shapes() {
        let context = FloatContext {
            shapes: vec![
                shape(Float::Left, 0, 10.0, 40.0, 100.0, 60.0),
                shape(Float::Right, 0, 80.0, 110.0, 95.0, 55.0),
                shape(Float::Left, 0, 10.0, 70.0, 40.0, 10.0),
            ],
        };

        let band = context.band(
            0,
            PageBlockSpan::new(90.0, 20.0),
            PageInlineSpan::from_edges(10.0, 110.0),
        );

        assert_eq!(band, FloatBand::from_edges(40.0, 80.0));
    }

    #[test]
    fn float_band_ignores_inactive_and_other_page_shapes() {
        let context = FloatContext {
            shapes: vec![
                shape(Float::Left, 1, 10.0, 80.0, 100.0, 60.0),
                shape(Float::Left, 0, 10.0, 80.0, 40.0, 10.0),
            ],
        };

        let band = context.band(
            0,
            PageBlockSpan::new(90.0, 20.0),
            PageInlineSpan::from_edges(10.0, 110.0),
        );

        assert_eq!(band, FloatBand::from_edges(10.0, 110.0));
    }

    #[test]
    fn clearance_uses_logical_direction_mapping() {
        let context = FloatContext {
            shapes: vec![shape(Float::Right, 0, 80.0, 110.0, 100.0, 60.0)],
        };

        assert_eq!(
            context.clearance_top(
                Clear::InlineStart,
                WritingMode::HorizontalTb,
                Direction::Rtl,
                0,
                95.0
            ),
            60.0
        );
        assert_eq!(
            context.clearance_top(
                Clear::InlineStart,
                WritingMode::HorizontalTb,
                Direction::Ltr,
                0,
                95.0
            ),
            95.0
        );
    }

    #[test]
    fn clearance_uses_vertical_logical_used_sides() {
        let context = FloatContext {
            shapes: vec![shape_with_used_side(
                Float::InlineStart,
                UsedFloatSide::Top,
                0,
                10.0,
                40.0,
                100.0,
                60.0,
            )],
        };

        assert_eq!(
            context.clearance_top(
                Clear::InlineStart,
                WritingMode::VerticalRl,
                Direction::Ltr,
                0,
                95.0
            ),
            60.0
        );
        assert_eq!(
            context.clearance_top(
                Clear::Left,
                WritingMode::VerticalRl,
                Direction::Ltr,
                0,
                95.0
            ),
            95.0
        );
    }

    #[test]
    fn vertical_top_float_reduces_inline_start_band() {
        let context = FloatContext {
            shapes: vec![shape_with_used_side(
                Float::InlineStart,
                UsedFloatSide::Top,
                0,
                10.0,
                40.0,
                100.0,
                70.0,
            )],
        };

        let band = context.logical_band(
            WritingMode::VerticalRl,
            Direction::Ltr,
            0,
            10.0,
            20.0,
            100.0,
            90.0,
        );

        assert_eq!(band, LogicalFloatBand::new(30.0, 60.0, 70.0, 10.0));
    }

    #[test]
    fn vertical_bottom_float_reduces_inline_end_band() {
        let context = FloatContext {
            shapes: vec![shape_with_used_side(
                Float::InlineEnd,
                UsedFloatSide::Bottom,
                0,
                10.0,
                40.0,
                40.0,
                10.0,
            )],
        };

        let band = context.logical_band(
            WritingMode::VerticalRl,
            Direction::Ltr,
            0,
            10.0,
            20.0,
            100.0,
            90.0,
        );

        assert_eq!(band, LogicalFloatBand::new(0.0, 60.0, 100.0, 40.0));
    }

    #[test]
    fn vertical_avoiding_position_moves_past_over_tall_top_exclusion() {
        let context = FloatContext {
            shapes: vec![shape_with_used_side(
                Float::InlineStart,
                UsedFloatSide::Top,
                0,
                10.0,
                40.0,
                100.0,
                70.0,
            )],
        };

        let placement = context.vertical_avoiding_position(
            0,
            100.0,
            20.0,
            80.0,
            Clear::None,
            WritingMode::VerticalRl,
            WritingMode::VerticalRl,
            Direction::Ltr,
            10.0,
            10.0,
            None,
        );

        assert_eq!(placement, FloatPlacement::new(40.0, 100.0, 90.0));
    }

    #[test]
    fn vertical_avoiding_position_moves_past_over_tall_bottom_exclusion() {
        let context = FloatContext {
            shapes: vec![shape_with_used_side(
                Float::InlineEnd,
                UsedFloatSide::Bottom,
                0,
                10.0,
                40.0,
                40.0,
                10.0,
            )],
        };

        let placement = context.vertical_avoiding_position(
            0,
            100.0,
            20.0,
            80.0,
            Clear::None,
            WritingMode::VerticalLr,
            WritingMode::VerticalLr,
            Direction::Ltr,
            10.0,
            10.0,
            None,
        );

        assert_eq!(placement, FloatPlacement::new(40.0, 100.0, 90.0));
    }

    #[test]
    fn lowest_bottom_is_page_local() {
        let context = FloatContext {
            shapes: vec![
                shape(Float::Left, 0, 10.0, 40.0, 100.0, 70.0),
                shape(Float::Left, 0, 10.0, 40.0, 80.0, 30.0),
                shape(Float::Left, 1, 10.0, 40.0, 100.0, 10.0),
            ],
        };

        assert_eq!(context.lowest_bottom_on_page(0), Some(30.0));
        assert_eq!(context.lowest_bottom_on_page(1), Some(10.0));
        assert_eq!(context.lowest_bottom_on_page(2), None);
    }

    #[test]
    fn avoiding_position_uses_highest_band_that_fits() {
        let context = FloatContext {
            shapes: vec![
                shape(Float::Left, 0, 10.0, 50.0, 100.0, 70.0),
                shape(Float::Right, 0, 70.0, 110.0, 100.0, 70.0),
            ],
        };

        let placement = context.avoiding_position(
            0,
            100.0,
            50.0,
            10.0,
            Clear::None,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            10.0,
            110.0,
        );

        assert_eq!(placement, FloatPlacement::new(10.0, 70.0, 100.0));
    }

    #[test]
    fn avoiding_position_applies_clearance_before_collision_search() {
        let context = FloatContext {
            shapes: vec![shape(Float::Left, 0, 10.0, 40.0, 100.0, 60.0)],
        };

        let placement = context.avoiding_position(
            0,
            95.0,
            20.0,
            10.0,
            Clear::Left,
            WritingMode::HorizontalTb,
            Direction::Ltr,
            10.0,
            110.0,
        );

        assert_eq!(placement.top(), 60.0);
        assert_eq!(placement.left(), 10.0);
        assert_eq!(placement.available_width(), 100.0);
    }

    #[test]
    fn clearance_sees_continued_float_fragment_on_current_page() {
        let context = FloatContext {
            shapes: vec![continued_shape(Float::Left, 1, 10.0, 40.0, 100.0, 60.0)],
        };

        assert_eq!(
            context.clearance_top(
                Clear::Both,
                WritingMode::HorizontalTb,
                Direction::Ltr,
                1,
                95.0
            ),
            60.0
        );
        assert_eq!(
            context.clearance_top(
                Clear::Right,
                WritingMode::HorizontalTb,
                Direction::Ltr,
                1,
                95.0
            ),
            95.0
        );
    }

    #[test]
    fn clearance_resolution_reports_future_continuation() {
        let mut first = shape(Float::Left, 0, 10.0, 40.0, 100.0, 10.0);
        first.id = FloatId(9);
        first.continues_on_next_page = true;
        let mut second = continued_shape(Float::Left, 1, 10.0, 40.0, 100.0, 50.0);
        second.id = FloatId(9);
        let context = FloatContext {
            shapes: vec![first, second],
        };

        assert_eq!(
            context.clearance_resolution(
                Clear::Both,
                WritingMode::HorizontalTb,
                Direction::Ltr,
                0,
                95.0
            ),
            FloatClearanceResolution {
                top: 10.0,
                continued_float: Some(FloatId(9))
            }
        );
        assert_eq!(
            context.clearance_resolution(
                Clear::Both,
                WritingMode::HorizontalTb,
                Direction::Ltr,
                1,
                95.0
            ),
            FloatClearanceResolution {
                top: 50.0,
                continued_float: None
            }
        );
    }

    #[test]
    fn float_shape_keeps_fragment_identity_and_source_order() {
        let mut second = continued_shape(Float::Right, 2, 70.0, 110.0, 100.0, 60.0);
        second.id = FloatId(7);
        second.source_order = 42;
        second.continues_on_next_page = true;

        assert_eq!(second.id, FloatId(7));
        assert_eq!(second.fragment_index, 1);
        assert_eq!(second.source_order, 42);
        assert!(second.starts_on_previous_page);
        assert!(second.continues_on_next_page);
    }
}
