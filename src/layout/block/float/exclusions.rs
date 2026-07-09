use super::super::super::*;
use super::model::*;

pub(in crate::layout) const FLOAT_EPSILON: f32 = 0.01;

impl FloatRunState {
    pub(in crate::layout) fn new(left_x: f32, right_x: f32, row_top: f32) -> Self {
        let row_span = PageInlineSpan::from_edges(left_x, right_x);
        Self {
            row_span,
            available_span: row_span,
            occupied_block_span: PageBlockSpan::from_edges(row_top, row_top),
            active: false,
        }
    }

    pub(in crate::layout) fn include_shape(&mut self, shape: FloatShape) {
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

    pub(in crate::layout) fn reset_for_block(&mut self, left_x: f32, right_x: f32, row_top: f32) {
        *self = Self::new(left_x, right_x, row_top);
    }
}

impl FloatContext {
    pub(in crate::layout) fn active_shapes(
        &self,
        page_index: usize,
        block_span: PageBlockSpan,
    ) -> impl Iterator<Item = FloatShape> + '_ {
        self.shapes.iter().cloned().filter(move |shape| {
            shape.page_index == page_index
                && shape.top() > block_span.bottom_y() + FLOAT_EPSILON
                && shape.bottom() < block_span.top_y() - FLOAT_EPSILON
        })
    }

    pub(in crate::layout) fn band(
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
    pub(in crate::layout) fn logical_band(
        &self,
        writing_mode: WritingMode,
        direction: Direction,
        page_index: usize,
        block_start: f32,
        block_size: f32,
        inline_physical_start: f32,
        inline_size: f32,
    ) -> LogicalFloatBand {
        let axes = WritingModeAxes::new(writing_mode, direction);
        if !axes.swaps_physical_axes() {
            let left = block_start;
            let right = block_start + block_size.max(0.0);
            let band = self.band(
                page_index,
                PageBlockSpan::new(inline_physical_start, inline_size),
                PageInlineSpan::from_edges(left, right),
            );
            let (inline_start, inline_end) = if axes.is_reversed(LogicalAxis::Inline) {
                (right - band.right(), right - band.left())
            } else {
                (band.left() - left, band.right() - left)
            };
            let inline_start = inline_start.max(0.0);
            let inline_end = inline_end.max(inline_start).min((right - left).max(0.0));
            LogicalFloatBand::new(
                inline_start,
                inline_end - inline_start,
                inline_physical_start,
                inline_physical_start - inline_size.max(0.0),
            )
        } else {
            let slab_left = block_start;
            let slab_right = block_start + block_size.max(0.0);
            let inline_start_side = axes.physical_side(LogicalSide::InlineStart);
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
            for shape in self.shapes.iter().cloned().filter(|shape| {
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

    pub(in crate::layout) fn clearance_top(
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
    pub(in crate::layout) fn clearance_resolution(
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

    pub(in crate::layout) fn lowest_bottom_on_page(&self, page_index: usize) -> Option<f32> {
        self.shapes
            .iter()
            .filter(|shape| shape.page_index == page_index)
            .map(|shape| shape.bottom())
            .reduce(f32::min)
    }

    /// Return the final page-local block-end occupied by this float context.
    ///
    /// An independent block formatting context encloses its internal floats.
    /// When a float is graphically fragmented, that enclosure reaches the
    /// lowest fragment on the last page rather than stopping at the first
    /// page's local bottom.
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn last_fragment_end(&self) -> Option<(usize, f32)> {
        let last_page_index = self.shapes.iter().map(|shape| shape.page_index).max()?;
        self.lowest_bottom_on_page(last_page_index)
            .map(|bottom| (last_page_index, bottom))
    }
}

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

    /// Return the last page and lowest margin-box edge of a fragmented float
    /// context.
    pub(in crate::layout) fn current_float_context_last_fragment_end(
        &self,
    ) -> Option<(usize, f32)> {
        self.float_contexts
            .last()
            .expect("root float context exists")
            .last_fragment_end()
    }
}
