use super::super::super::*;
use super::{HypotheticalClearBorderEdge, exclusions::FLOAT_EPSILON, model::*};

/// Resolve the auto border-box width available to a BFC root beside floats.
///
/// A negative margin on the float-facing side belongs outside the collision
/// rectangle and must not re-expand the border box into the float. A negative
/// margin on the opposite side remains part of the normal block-width
/// equation, so it may extend the border box away from the float.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
pub(in crate::layout) fn float_avoiding_auto_border_box_width(
    band: PageInlineSpan,
    containing: PageInlineSpan,
    margin_left: f32,
    margin_right: f32,
) -> BorderBoxLength {
    let left_margin = if band.left_x() > containing.left_x() + FLOAT_EPSILON {
        0.0
    } else {
        margin_left
    };
    let right_margin = if band.right_x() < containing.right_x() - FLOAT_EPSILON {
        0.0
    } else {
        margin_right
    };
    border_box_pt((band.width() - left_margin - right_margin).max(0.0))
}

impl FloatContext {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn avoiding_position(
        &self,
        page_index: usize,
        top: PageTopBlockPosition,
        margin_box_size: MarginBoxSize,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        containing_inline_span: PageInlineSpan,
    ) -> FloatBandPlacement {
        self.avoiding_position_with_role(
            page_index,
            top,
            margin_box_size,
            clear,
            writing_mode,
            direction,
            containing_inline_span,
            true,
        )
    }

    /// Find a normal-flow BFC root's position without changing its
    /// hypothetical source position to the preceding float's top.
    ///
    /// Unlike a new float, a following BFC root does not participate in float
    /// stacking. It starts at its normal-flow position and only moves toward
    /// the block end when its own margin box cannot fit an active exclusion
    /// band. This is the table/grid/flex/replaced-box form of CSS 2.2 float
    /// avoidance:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    #[allow(clippy::too_many_arguments)]
    fn avoiding_bfc_position(
        &self,
        page_index: usize,
        top: PageTopBlockPosition,
        margin_box_size: MarginBoxSize,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        containing_inline_span: PageInlineSpan,
    ) -> FloatBandPlacement {
        self.avoiding_position_with_role(
            page_index,
            top,
            margin_box_size,
            clear,
            writing_mode,
            direction,
            containing_inline_span,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn avoiding_position_with_role(
        &self,
        page_index: usize,
        top: PageTopBlockPosition,
        margin_box_size: MarginBoxSize,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        containing_inline_span: PageInlineSpan,
        is_float: bool,
    ) -> FloatBandPlacement {
        let width = margin_box_size.width;
        let height = margin_box_size.height;
        let mut top = self.clearance_top(
            clear,
            writing_mode,
            direction,
            page_index,
            HypotheticalClearBorderEdge::new(top),
        );
        // A later float may share a row with an earlier float, but its outer
        // top may not be above that earlier float's outer top. Resolve
        // `clear` first, then apply this independent source-order constraint:
        // a cleared left float can still follow an earlier right float whose
        // top is lower than the cleared-left boundary.
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
        if is_float
            && let Some(last) = self
                .shapes
                .iter()
                .rev()
                .find(|shape| shape.is_css_float() && shape.page_index == page_index)
        {
            top = top.min(PageTopBlockPosition::new(
                last.margin_box_block_span().top_y(),
            ));
        }

        for _ in 0..self.shapes.len().saturating_add(2) {
            let block_span = PageBlockSpan::new(top.points(), height);
            let band = self.placement_band(page_index, block_span, containing_inline_span);
            let available_width = band.width();
            if width <= available_width + FLOAT_EPSILON {
                return FloatBandPlacement::new(band, top);
            }

            let next_top = self
                .active_placement_shapes(page_index, block_span)
                .map(|shape| PageTopBlockPosition::new(shape.margin_box_block_span().bottom_y()))
                .fold(top, PageTopBlockPosition::min);
            if next_top.points() >= top.points() - FLOAT_EPSILON {
                return FloatBandPlacement::new(band, top);
            }
            top = next_top;
        }

        let band = self.placement_band(
            page_index,
            PageBlockSpan::new(top.points(), height),
            containing_inline_span,
        );
        FloatBandPlacement::new(band, top)
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
        top: PageTopBlockPosition,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        left: f32,
        right: f32,
        mut measure: F,
    ) -> FloatAvoidingBfcPlacement
    where
        F: FnMut(FloatBand, PageTopBlockPosition) -> FloatAvoidanceCandidate,
    {
        let mut top = self.clearance_top(
            clear,
            writing_mode,
            direction,
            page_index,
            HypotheticalClearBorderEdge::new(top),
        );
        let inline_span = PageInlineSpan::from_edges(left, right);
        let mut last_placement = None;
        for _ in 0..self.shapes.len().saturating_add(2) {
            let mut band = self.content_band(
                page_index,
                PageBlockSpan::new(top.points(), FLOAT_EPSILON),
                inline_span,
            );
            let mut measured = measure(band, top);
            let mut stable = false;

            for _ in 0..self.shapes.len().saturating_add(3) {
                let height = measured
                    .normal_flow_border_box_block_size
                    .points()
                    .max(FLOAT_EPSILON);
                let next_band = self.content_band(
                    page_index,
                    PageBlockSpan::new(top.points(), height),
                    inline_span,
                );
                let next_measured = measure(next_band, top);
                stable = (next_band.left() - band.left()).abs() <= FLOAT_EPSILON
                    && (next_band.width() - band.width()).abs() <= FLOAT_EPSILON
                    && (next_measured.normal_flow_border_box_inline_span.width()
                        - measured.normal_flow_border_box_inline_span.width())
                    .abs()
                        <= FLOAT_EPSILON
                    && (next_measured.normal_flow_border_box_block_size.points()
                        - measured.normal_flow_border_box_block_size.points())
                    .abs()
                        <= FLOAT_EPSILON;
                band = next_band;
                measured = next_measured;
                if stable {
                    break;
                }
            }

            let placement = FloatAvoidingBfcPlacement {
                placement: FloatBandPlacement::new(band, top),
                candidate: measured,
            };
            last_placement = Some(placement);
            // A BFC root avoids float margin boxes with its normal-flow border
            // box. Its border box need not be contained in the residual band:
            // a negative margin may extend it past the opposite side of the
            // containing block while still leaving it disjoint from every
            // float. Test actual rectangle collision rather than comparing
            // the box to the band's two edges.
            // <https://www.w3.org/TR/CSS22/visuren.html#floats>
            let border_box_span = measured.normal_flow_border_box_inline_span;
            let border_box_block_span = PageBlockSpan::new(
                top.points(),
                measured
                    .normal_flow_border_box_block_size
                    .points()
                    .max(FLOAT_EPSILON),
            );
            let avoids_active_floats =
                self.active_shapes(page_index, border_box_block_span)
                    .all(|shape| {
                        // A zero-inline-size float still participates in
                        // `clear` and establishes its block position, but it
                        // has no horizontal collision area. Treating its two
                        // coincident edges as a rectangle would incorrectly
                        // force an otherwise adjacent BFC root below it.
                        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                        if shape.margin_box_inline_span().width() <= FLOAT_EPSILON {
                            return true;
                        }
                        let Some(shape_span) = shape
                            .area
                            .horizontal_edges(shape.rect, border_box_block_span)
                        else {
                            return true;
                        };
                        border_box_span.right_x() <= shape_span.left_x() + FLOAT_EPSILON
                            || border_box_span.left_x() >= shape_span.right_x() - FLOAT_EPSILON
                    });
            // Avoidance is a collision constraint, but a fixed-width BFC
            // root that merely overflows its containing block must still
            // clear the float. A negative physical margin is the exception:
            // CSS's normal block-width equation may legally put the border
            // box outside the containing block while it remains adjacent to
            // the float.
            // <https://www.w3.org/TR/CSS22/visuren.html#floats>
            let fits_containing_inline_span = (measured.permits_inline_start_overflow()
                || border_box_span.left_x() >= left - FLOAT_EPSILON)
                && (measured.permits_inline_end_overflow()
                    || border_box_span.right_x() <= right + FLOAT_EPSILON);
            if stable && avoids_active_floats && fits_containing_inline_span {
                return placement;
            }

            let block_span = PageBlockSpan::new(
                top.points(),
                measured
                    .normal_flow_border_box_block_size
                    .points()
                    .max(FLOAT_EPSILON),
            );
            let rectangular_next_top = self
                .active_shapes(page_index, block_span)
                .map(|shape| PageTopBlockPosition::new(shape.margin_box_block_span().bottom_y()))
                .fold(top, PageTopBlockPosition::min);
            let shaped_next_top = self.next_content_slab_with_width(
                page_index,
                block_span,
                inline_span,
                measured.normal_flow_border_box_inline_span.width(),
            );
            let next_top = shaped_next_top
                .filter(|next_top| next_top.points() < top.points() - FLOAT_EPSILON)
                .unwrap_or(rectangular_next_top);
            if next_top.points() >= top.points() - FLOAT_EPSILON {
                return placement;
            }
            top = next_top;
        }

        last_placement.unwrap_or_else(|| {
            let band = self.content_band(
                page_index,
                PageBlockSpan::new(top.points(), FLOAT_EPSILON),
                inline_span,
            );
            let measured = measure(band, top);
            FloatAvoidingBfcPlacement {
                placement: FloatBandPlacement::new(band, top),
                candidate: measured,
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn vertical_avoiding_position(
        &self,
        page_index: usize,
        top: PageTopBlockPosition,
        margin_box_size: MarginBoxSize,
        clear: Clear,
        clear_writing_mode: WritingMode,
        avoidance_writing_mode: WritingMode,
        direction: Direction,
        block_slab: PageInlineSpan,
        page_bottom: PageTopBlockPosition,
        side: Option<UsedFloatSide>,
    ) -> FloatBandPlacement {
        let width = margin_box_size.width;
        let height = margin_box_size.height;
        let top = self.clearance_top(
            clear,
            clear_writing_mode,
            direction,
            page_index,
            HypotheticalClearBorderEdge::new(top),
        );
        let inline_size = (top.points() - page_bottom.points()).max(height).max(1.0);
        // With no exclusions, a BFC root remains at its hypothetical physical
        // top regardless of whether the vertical inline axis starts at the
        // page top or bottom.  Deriving the position from a bottom-to-top
        // empty logical band would instead select that band's far physical
        // edge and displace the box by the complete available inline span.
        // <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        if !self.shapes.iter().any(FloatShape::is_css_float) {
            let placed_top = match side {
                // A vertical inline-end float is anchored at the physical
                // inline-end edge even before it has any sibling exclusion.
                // `top` is the block cursor, not the vertical inline-axis
                // start for this float.
                // <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
                Some(UsedFloatSide::Bottom) => page_bottom.points() + height,
                Some(UsedFloatSide::Top)
                | Some(UsedFloatSide::Left | UsedFloatSide::Right)
                | None => top.points(),
            };
            return FloatBandPlacement::new(
                FloatBand::from_span(PageInlineSpan::new(block_slab.left_x(), inline_size)),
                PageTopBlockPosition::new(placed_top),
            );
        }
        let mut block_slab = PageInlineSpan::new(block_slab.left_x(), width.max(1.0));

        for _ in 0..self.shapes.len().saturating_add(2) {
            let vertical_slab = vertical_physical_inline_span(
                avoidance_writing_mode,
                direction,
                top,
                layout_pt(inline_size),
            );
            let band = self.logical_band(
                avoidance_writing_mode,
                direction,
                page_index,
                FloatBandQuery {
                    horizontal_slab: block_slab,
                    vertical_slab,
                },
            );
            let placed_top = match side {
                Some(UsedFloatSide::Bottom) => band.block_span.bottom_y() + height,
                Some(UsedFloatSide::Top) | None => band.block_span.top_y(),
                Some(UsedFloatSide::Left | UsedFloatSide::Right) => band.block_span.top_y(),
            };
            if height <= band.inline_span.size() + FLOAT_EPSILON {
                return FloatBandPlacement::new(
                    FloatBand::from_span(PageInlineSpan::new(
                        block_slab.left_x(),
                        band.inline_span.size(),
                    )),
                    PageTopBlockPosition::new(placed_top),
                );
            }

            let Some(next_start) =
                self.next_vertical_float_slab_start(page_index, block_slab, vertical_slab)
            else {
                return FloatBandPlacement::new(
                    FloatBand::from_span(PageInlineSpan::new(
                        block_slab.left_x(),
                        band.inline_span.size(),
                    )),
                    PageTopBlockPosition::new(placed_top),
                );
            };
            if next_start.left_x() <= block_slab.left_x() + FLOAT_EPSILON {
                return FloatBandPlacement::new(
                    FloatBand::from_span(PageInlineSpan::new(
                        block_slab.left_x(),
                        band.inline_span.size(),
                    )),
                    PageTopBlockPosition::new(placed_top),
                );
            }
            block_slab = next_start;
        }

        let band = self.logical_band(
            avoidance_writing_mode,
            direction,
            page_index,
            FloatBandQuery {
                horizontal_slab: block_slab,
                vertical_slab: vertical_physical_inline_span(
                    avoidance_writing_mode,
                    direction,
                    top,
                    layout_pt(inline_size),
                ),
            },
        );
        let placed_top = match side {
            Some(UsedFloatSide::Bottom) => band.block_span.bottom_y() + height,
            Some(UsedFloatSide::Top) | None => band.block_span.top_y(),
            Some(UsedFloatSide::Left | UsedFloatSide::Right) => band.block_span.top_y(),
        };
        FloatBandPlacement::new(
            FloatBand::from_span(PageInlineSpan::new(
                block_slab.left_x(),
                band.inline_span.size(),
            )),
            PageTopBlockPosition::new(placed_top),
        )
    }

    pub(in crate::layout) fn next_vertical_float_slab_start(
        &self,
        page_index: usize,
        block_slab: PageInlineSpan,
        inline_slab: PageBlockSpan,
    ) -> Option<PageInlineSpan> {
        self.shapes
            .iter()
            .filter(|shape| {
                let shape_block_span = shape.margin_box_block_span();
                let shape_inline_span = shape.margin_box_inline_span();
                shape.is_css_float()
                    && shape.page_index == page_index
                    && matches!(shape.side, UsedFloatSide::Top | UsedFloatSide::Bottom)
                    && shape_inline_span.right_x() > block_slab.left_x() + FLOAT_EPSILON
                    && shape_inline_span.left_x() < block_slab.right_x() - FLOAT_EPSILON
                    && shape_block_span.top_y() > inline_slab.bottom_y() + FLOAT_EPSILON
                    && shape_block_span.bottom_y() < inline_slab.top_y() - FLOAT_EPSILON
            })
            .map(|shape| {
                PageInlineSpan::new(shape.margin_box_inline_span().right_x(), block_slab.width())
            })
            .filter(|next_slab| next_slab.left_x() > block_slab.left_x() + FLOAT_EPSILON)
            .min_by(|left, right| {
                left.left_x()
                    .partial_cmp(&right.left_x())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

pub(in crate::layout) fn vertical_physical_inline_span(
    writing_mode: WritingMode,
    direction: Direction,
    inline_physical_start: PageTopBlockPosition,
    inline_size: LayoutLength,
) -> PageBlockSpan {
    match inline_start_side(writing_mode, direction) {
        PhysicalSide::Top => PageBlockSpan::from_edges(
            inline_physical_start.points(),
            inline_physical_start.toward_block_end(inline_size).points(),
        ),
        PhysicalSide::Bottom => PageBlockSpan::from_edges(
            PageTopBlockPosition::new(inline_physical_start.points() + inline_size.points())
                .points(),
            inline_physical_start.points(),
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
        top: PageTopBlockPosition,
        margin_box_size: MarginBoxSize,
        clear: Clear,
        writing_mode: WritingMode,
        clear_direction: Direction,
        placement_direction: Direction,
    ) -> FloatPlacement {
        let width = margin_box_size.width;
        if self.containing_block_writing_mode != WritingMode::HorizontalTb {
            let placement = self.find_vertical_float_avoiding_position(
                top,
                margin_box_size,
                clear,
                writing_mode,
                clear_direction,
                None,
            );
            return FloatPlacement::new(placement.origin, placement.available_span);
        }
        let placement = self.find_bfc_avoiding_position(
            top,
            margin_box_size,
            clear,
            writing_mode,
            clear_direction,
        );
        let x = if placement_direction == Direction::Rtl {
            placement.origin.x() + (placement.available_span.width() - width).max(0.0)
        } else {
            placement.origin.x()
        };
        FloatPlacement::new(
            PageTopPoint::new(x, placement.origin.top_y()),
            placement.available_span,
        )
    }

    fn find_bfc_avoiding_position(
        &self,
        top: PageTopBlockPosition,
        margin_box_size: MarginBoxSize,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
    ) -> FloatBandPlacement {
        let page_index = self.current_float_page_index();
        let context = self
            .float_contexts
            .last()
            .expect("root float context exists");
        context.avoiding_bfc_position(
            page_index,
            top,
            margin_box_size,
            clear,
            writing_mode,
            direction,
            PageInlineSpan::from_edges(self.content_left, self.content_right),
        )
    }

    /// Find a CSS float position after resolving collision with an earlier
    /// same-side initial letter.
    ///
    /// The initial-letter adjustment is deliberately outside the normal
    /// `clear` resolution so `clear` continues to see CSS floats only.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
    pub(in crate::layout) fn find_inline_float_avoiding_position(
        &self,
        top: PageTopBlockPosition,
        margin_box_size: MarginBoxSize,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        side: UsedFloatSide,
    ) -> FloatBandPlacement {
        let page_index = self.current_float_page_index();
        let context = self
            .float_contexts
            .last()
            .expect("root float context exists");
        let top = context.initial_letter_float_avoidance_top(page_index, top, side);
        context.avoiding_position(
            page_index,
            top,
            margin_box_size,
            clear,
            writing_mode,
            direction,
            PageInlineSpan::from_edges(self.content_left, self.content_right),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn find_vertical_float_avoiding_position(
        &self,
        top: PageTopBlockPosition,
        margin_box_size: MarginBoxSize,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        side: Option<UsedFloatSide>,
    ) -> FloatBandPlacement {
        let page_index = self.current_float_page_index();
        let context = self
            .float_contexts
            .last()
            .expect("root float context exists");
        let avoidance_writing_mode = self.containing_block_writing_mode;
        context.vertical_avoiding_position(
            page_index,
            top,
            margin_box_size,
            clear,
            writing_mode,
            avoidance_writing_mode,
            direction,
            PageInlineSpan::from_edges(self.content_left, self.content_right),
            PageTopBlockPosition::new(self.page_bottom()),
            side,
        )
    }

    pub(in crate::layout) fn prebreak_bfc_margin_box_if_needed(
        &mut self,
        margin_box_height: MarginBoxLength,
        reapplied_margin_top: f32,
    ) {
        // Out-of-flow roots resolve their static/absolute position first; the
        // normal-flow BFC prebreak heuristic would incorrectly move a
        // bottom-anchored positioned table or block to the next page.
        if margin_box_height.points() <= FLOAT_EPSILON
            || self.out_of_flow_prebreak_suppression_depth > 0
            || margin_box_height.points() > self.page_area_height() + FLOAT_EPSILON
            || self.cursor_y - margin_box_height.points() >= self.page_bottom() - FLOAT_EPSILON
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
