use super::super::super::*;
use super::{exclusions::FLOAT_EPSILON, model::*};

impl FloatContext {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn avoiding_position(
        &self,
        page_index: usize,
        top: f32,
        margin_box_size: PageTopSize,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        left: f32,
        right: f32,
    ) -> FloatPlacement {
        let width = margin_box_size.width;
        let height = margin_box_size.height;
        let mut top = self.clearance_top(clear, writing_mode, direction, page_index, top);
        // A later float with no clearance may rise beside the preceding
        // float.  `clear` establishes a lower hypothetical position first,
        // however, and must not be undone by that ordinary float stacking
        // optimization.
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
        if clear == Clear::None
            && let Some(last) = self
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
                border_box_left: measured.border_box_left,
                left: band.left(),
                top,
                available_width: band.width(),
                border_box_width: measured.border_box_width,
                border_box_height: measured.border_box_height,
            };
            last_placement = Some(placement);
            // A BFC root avoids float margin boxes with its normal-flow border
            // box. Its border box need not be contained in the residual band:
            // a negative margin may extend it past the opposite side of the
            // containing block while still leaving it disjoint from every
            // float. Test actual rectangle collision rather than comparing
            // the box to the band's two edges.
            // <https://www.w3.org/TR/CSS22/visuren.html#floats>
            let border_box_span =
                PageInlineSpan::new(measured.border_box_left, measured.border_box_width.max(0.0));
            let border_box_block_span =
                PageBlockSpan::new(top, measured.border_box_height.max(FLOAT_EPSILON));
            let avoids_active_floats =
                self.active_shapes(page_index, border_box_block_span)
                    .all(|shape| {
                        border_box_span.right_x() <= shape.left() + FLOAT_EPSILON
                            || border_box_span.left_x() >= shape.right() - FLOAT_EPSILON
                    });
            // A fixed-width BFC root which cannot fit the residual float band
            // is moved below the exclusion rather than overflowing past the
            // containing block's inline end.
            let fits_containing_inline_span = border_box_span.left_x() >= left - FLOAT_EPSILON
                && border_box_span.right_x() <= right + FLOAT_EPSILON;
            if stable && avoids_active_floats && fits_containing_inline_span {
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
                border_box_left: measured.border_box_left,
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
        margin_box_size: PageTopSize,
        clear: Clear,
        clear_writing_mode: WritingMode,
        avoidance_writing_mode: WritingMode,
        direction: Direction,
        left: f32,
        page_bottom: f32,
        side: Option<UsedFloatSide>,
    ) -> FloatPlacement {
        let width = margin_box_size.width;
        let height = margin_box_size.height;
        let top = self.clearance_top(clear, clear_writing_mode, direction, page_index, top);
        let inline_size = (top - page_bottom).max(height).max(1.0);
        // With no exclusions, a BFC root remains at its hypothetical physical
        // top regardless of whether the vertical inline axis starts at the
        // page top or bottom.  Deriving the position from a bottom-to-top
        // empty logical band would instead select that band's far physical
        // edge and displace the box by the complete available inline span.
        // <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        if self.shapes.is_empty() {
            let placed_top = match side {
                // A vertical inline-end float is anchored at the physical
                // inline-end edge even before it has any sibling exclusion.
                // `top` is the block cursor, not the vertical inline-axis
                // start for this float.
                // <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
                Some(UsedFloatSide::Bottom) => page_bottom + height,
                Some(UsedFloatSide::Top)
                | Some(UsedFloatSide::Left | UsedFloatSide::Right)
                | None => top,
            };
            return FloatPlacement::new(left, placed_top, inline_size);
        }
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

impl<'a> LayoutBuilder<'a> {
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
        margin_box_size: PageTopSize,
        clear: Clear,
        writing_mode: WritingMode,
        clear_direction: Direction,
        placement_direction: Direction,
    ) -> (f32, f32, f32) {
        let width = margin_box_size.width;
        if self.containing_block_writing_mode != WritingMode::HorizontalTb {
            let (left, top, available_inline_size) = self.find_vertical_float_avoiding_position(
                top,
                margin_box_size,
                clear,
                writing_mode,
                clear_direction,
                None,
            );
            return (left, top, available_inline_size);
        }
        let (left, top, available_width) = self.find_float_avoiding_position(
            top,
            margin_box_size,
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

    pub(in crate::layout) fn find_float_avoiding_position(
        &self,
        top: f32,
        margin_box_size: PageTopSize,
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
            margin_box_size,
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
        margin_box_size: PageTopSize,
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
            margin_box_size,
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
