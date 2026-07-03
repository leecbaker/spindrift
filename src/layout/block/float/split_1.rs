use super::*;

pub(in crate::layout) const FLOAT_EPSILON: f32 = 0.01;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct ResolvedFloatInlineSize {
    pub(in crate::layout) content_width: ContentBoxLength,
    pub(in crate::layout) border_box_width: BorderBoxLength,
    pub(in crate::layout) margin_box_width: f32,
}

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
        self.shapes.iter().copied().filter(move |shape| {
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

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn avoiding_position(
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

    /// Find a float-avoiding placement for a normal-flow BFC root whose size
    /// depends on the available band.
    ///
    /// CSS 2.2 requires a BFC root's border box to avoid earlier float margin
    /// boxes in the same block formatting context, and allows the BFC root to
    /// become narrower than the normal block-width equation would otherwise
    /// make it. Because that narrower width can change the root's block size,
    /// this solves placement as a fixed point between the active float band and
    /// the measured border box:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn avoiding_bfc_root_position<F>(
        &self,
        page_index: usize,
        top: f32,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        left: f32,
        right: f32,
        mut measure: F,
    ) -> FloatAvoidingBfcPlacement
    where
        F: FnMut(f32, f32, f32) -> FloatAvoidingBfcMeasurement,
    {
        let mut top = self.clearance_top(clear, writing_mode, direction, page_index, top);
        if let Some(last) = self
            .shapes
            .iter()
            .rev()
            .find(|shape| shape.page_index == page_index)
        {
            top = top.min(last.top());
        }

        let inline_span = PageInlineSpan::from_edges(left, right);
        let mut last_placement = None;
        for _ in 0..self.shapes.len().saturating_add(2) {
            let mut band = self.band(
                page_index,
                PageBlockSpan::new(top, FLOAT_EPSILON),
                inline_span,
            );
            let mut measured = measure(band.left(), band.width(), top);
            let mut stable = false;

            for _ in 0..self.shapes.len().saturating_add(3) {
                let height = measured.border_box_height.max(FLOAT_EPSILON);
                let next_band = self.band(page_index, PageBlockSpan::new(top, height), inline_span);
                let next_measured = measure(next_band.left(), next_band.width(), top);
                stable = (next_band.left() - band.left()).abs() <= FLOAT_EPSILON
                    && (next_band.width() - band.width()).abs() <= FLOAT_EPSILON
                    && (next_measured.border_box_width - measured.border_box_width).abs()
                        <= FLOAT_EPSILON
                    && (next_measured.border_box_height - measured.border_box_height).abs()
                        <= FLOAT_EPSILON;
                band = next_band;
                measured = next_measured;
                if stable {
                    break;
                }
            }

            let placement = FloatAvoidingBfcPlacement {
                left: band.left(),
                top,
                available_width: band.width(),
                border_box_width: measured.border_box_width,
                border_box_height: measured.border_box_height,
            };
            last_placement = Some(placement);
            if stable && measured.border_box_width <= band.width() + FLOAT_EPSILON {
                return placement;
            }

            let block_span = PageBlockSpan::new(top, measured.border_box_height.max(FLOAT_EPSILON));
            let next_top = self
                .active_shapes(page_index, block_span)
                .map(|shape| shape.bottom())
                .fold(top, f32::min);
            if next_top >= top - FLOAT_EPSILON {
                return placement;
            }
            top = next_top;
        }

        last_placement.unwrap_or_else(|| {
            let band = self.band(
                page_index,
                PageBlockSpan::new(top, FLOAT_EPSILON),
                inline_span,
            );
            let measured = measure(band.left(), band.width(), top);
            FloatAvoidingBfcPlacement {
                left: band.left(),
                top,
                available_width: band.width(),
                border_box_width: measured.border_box_width,
                border_box_height: measured.border_box_height,
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn vertical_avoiding_position(
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

    pub(in crate::layout) fn next_vertical_float_slab_start(
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

pub(in crate::layout) fn vertical_physical_inline_span(
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

/// Freeze a float's temporary replay style to the used inline size.
///
/// CSS 2.2 resolves a float's used width from its original containing block,
/// then lays out the float's contents in that used box. Quire replays the
/// floated element in an isolated temporary containing block, so percentage
/// widths and constraints must not resolve a second time against the replay
/// block:
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width> and
/// <https://www.w3.org/TR/css-cascade-5/#used>.
pub(in crate::layout) fn freeze_float_replay_width(
    style: &mut ComputedStyle,
    inline_size: ResolvedFloatInlineSize,
) {
    let replay_width = match style.box_sizing {
        BoxSizing::ContentBox => inline_size.content_width.points(),
        BoxSizing::BorderBox => inline_size.border_box_width.points(),
    };
    style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(replay_width.max(0.0)),
    );
    let content_width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(inline_size.content_width.points().max(0.0)),
    );
    style.box_values.min_width = content_width;
    style.box_values.max_width = content_width;
}

pub(in crate::layout) fn named_assignment_delta(
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

pub(in crate::layout) fn merge_named_assignments(
    target: &mut HashMap<String, Vec<NamedStringAssignment>>,
    source: HashMap<String, Vec<NamedStringAssignment>>,
) {
    for (name, mut assignments) in source {
        target.entry(name).or_default().append(&mut assignments);
    }
}

pub(in crate::layout) fn union_paint_bounds(
    bounds: impl IntoIterator<Item = PaintClip>,
) -> Option<PaintClip> {
    bounds.into_iter().fold(None, |acc, bounds| {
        Some(match acc {
            Some(existing) => union_paint_clip(existing, bounds),
            None => bounds,
        })
    })
}

pub(in crate::layout) fn union_paint_clip(left: PaintClip, right: PaintClip) -> PaintClip {
    let x1 = left.x().min(right.x());
    let x2 = (left.x() + left.width()).max(right.x() + right.width());
    let y1 = left.y().min(right.y());
    let y2 = (left.y() + left.height()).max(right.y() + right.height());
    PaintClip::from_paint_rect(paint_space_rect(x1, y1, x2 - x1, y2 - y1))
}
