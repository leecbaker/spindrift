use super::super::super::*;
use super::model::*;
use std::num::NonZeroUsize;

pub(in crate::layout) const FLOAT_EPSILON: f32 = 0.01;

/// CSS clearance inserted immediately before an in-flow block's top margin.
///
/// This is not a position delta: CSS 2.2 re-resolves adjoining margins after
/// clearance is introduced, so the clearance space itself can be negative or
/// zero while still inhibiting margin collapse.
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct ClearanceSpace(LayoutLength);

impl ClearanceSpace {
    fn new(value: LayoutLength) -> Self {
        Self(value)
    }

    fn flush_to_float(
        margin_edge_before_top_margin: PageTopBlockPosition,
        used_top_margin: LayoutLength,
        cleared_outer_block_end: PageTopBlockPosition,
    ) -> Self {
        // With page-top coordinates, block progression subtracts from the
        // margin edge. The signed clearance is therefore the residual after
        // the non-collapsed top margin has been arranged before the flush
        // border edge. CSS2 expressly allows this residual to be negative.
        Self::new(layout_pt(
            margin_edge_before_top_margin.points()
                - cleared_outer_block_end.points()
                - used_top_margin.points(),
        ))
    }

    fn applied_border_edge(
        self,
        margin_edge_before_top_margin: PageTopBlockPosition,
        used_top_margin: LayoutLength,
    ) -> PageTopBlockPosition {
        PageTopBlockPosition::new(
            margin_edge_before_top_margin.points() - used_top_margin.points() - self.0.points(),
        )
    }
}

/// The used block-start margin of a box whose CSS2 clearance is inserted
/// immediately before its authored top margin.
///
/// This is intentionally distinct from an authored margin. It exists only at
/// the block-flow boundary where clearance prevents adjoining-margin collapse.
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct ClearanceAdjustedTopMargin(LayoutLength);

impl ClearanceAdjustedTopMargin {
    fn from_clearance_and_top_margin(clearance: ClearanceSpace, top_margin: LayoutLength) -> Self {
        Self(layout_pt(clearance.0.points() + top_margin.points()))
    }

    pub(in crate::layout) fn border_edge_from(
        self,
        margin_edge_before_top_margin: PageTopBlockPosition,
    ) -> PageTopBlockPosition {
        PageTopBlockPosition::new(margin_edge_before_top_margin.points() - self.0.points())
    }
}

/// The geometry and flow-relative direction used to resolve CSS2 clearance.
///
/// The three edges intentionally remain distinct: the hypothetical edge
/// selects the float target, while the uncleared and margin edges describe
/// the used normal-flow arrangement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct HypotheticalClearBorderEdge(PageTopBlockPosition);

impl HypotheticalClearBorderEdge {
    pub(in crate::layout) fn new(edge: PageTopBlockPosition) -> Self {
        Self(edge)
    }

    pub(in crate::layout) fn position(self) -> PageTopBlockPosition {
        self.0
    }
}

/// The clear:none position inherited by a child whose start margin would
/// otherwise adjoin its parent's block-start margin.
///
/// CSS 2.2 calculates clearance against this counterfactual position, not
/// against the child's used post-margin position.
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct ParentStartClearanceHypothesis(HypotheticalClearBorderEdge);

impl ParentStartClearanceHypothesis {
    pub(in crate::layout) fn new(parent_border_edge: PageTopBlockPosition) -> Self {
        Self(HypotheticalClearBorderEdge::new(parent_border_edge))
    }

    pub(in crate::layout) fn border_edge(self) -> HypotheticalClearBorderEdge {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct UnclearedBorderEdge(PageTopBlockPosition);

impl UnclearedBorderEdge {
    pub(in crate::layout) fn new(edge: PageTopBlockPosition) -> Self {
        Self(edge)
    }

    pub(in crate::layout) fn position(self) -> PageTopBlockPosition {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct MarginEdgeBeforeClearance(PageTopBlockPosition);

impl MarginEdgeBeforeClearance {
    pub(in crate::layout) fn new(edge: PageTopBlockPosition) -> Self {
        Self(edge)
    }

    pub(in crate::layout) fn position(self) -> PageTopBlockPosition {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct ClearedFloatOuterBlockEnd(PageTopBlockPosition);

impl ClearedFloatOuterBlockEnd {
    pub(in crate::layout) fn new(edge: PageTopBlockPosition) -> Self {
        Self(edge)
    }

    pub(in crate::layout) fn position(self) -> PageTopBlockPosition {
        self.0
    }

    fn lowest(self, other: Self) -> Self {
        if self.position().points() <= other.position().points() {
            self
        } else {
            other
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::layout) struct BlockClearanceRequest {
    pub(in crate::layout) clear: Clear,
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
    hypothetical_border_edge: HypotheticalClearBorderEdge,
    uncleared_border_edge: UnclearedBorderEdge,
    margin_edge_before_top_margin: MarginEdgeBeforeClearance,
    used_top_margin: LayoutLength,
}

impl BlockClearanceRequest {
    /// Construct a clearance query whose hypothetical, uncleared, and margin
    /// edges coincide. Inline, flex, and grid callers have no adjoining
    /// block-start margin relationship to re-resolve.
    pub(in crate::layout) fn coincident_edges(
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        edge: PageTopBlockPosition,
    ) -> Self {
        Self {
            clear,
            writing_mode,
            direction,
            hypothetical_border_edge: HypotheticalClearBorderEdge::new(edge),
            uncleared_border_edge: UnclearedBorderEdge::new(edge),
            margin_edge_before_top_margin: MarginEdgeBeforeClearance::new(edge),
            used_top_margin: layout_pt(0.0),
        }
    }

    /// Construct the request for an ordinary in-flow block. A first child
    /// that would adjoin its parent's start margin when `clear:none` supplies
    /// the counterfactual parent edge explicitly.
    pub(in crate::layout) fn block_flow(
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        uncleared_border_edge: PageTopBlockPosition,
        margin_edge_before_top_margin: PageTopBlockPosition,
        used_top_margin: LayoutLength,
        parent_start_hypothesis: Option<ParentStartClearanceHypothesis>,
    ) -> Self {
        Self {
            clear,
            writing_mode,
            direction,
            hypothetical_border_edge: parent_start_hypothesis.map_or_else(
                || HypotheticalClearBorderEdge::new(uncleared_border_edge),
                ParentStartClearanceHypothesis::border_edge,
            ),
            uncleared_border_edge: UnclearedBorderEdge::new(uncleared_border_edge),
            margin_edge_before_top_margin: MarginEdgeBeforeClearance::new(
                margin_edge_before_top_margin,
            ),
            used_top_margin,
        }
    }

    /// Construct the request for an independent BFC root. Its hypothetical
    /// edge is its margin edge, while its uncleared border edge is after the
    /// used top margin.
    pub(in crate::layout) fn bfc_root(
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        margin_edge_before_top_margin: PageTopBlockPosition,
        uncleared_border_edge: PageTopBlockPosition,
        used_top_margin: LayoutLength,
    ) -> Self {
        Self {
            clear,
            writing_mode,
            direction,
            hypothetical_border_edge: HypotheticalClearBorderEdge::new(
                margin_edge_before_top_margin,
            ),
            uncleared_border_edge: UnclearedBorderEdge::new(uncleared_border_edge),
            margin_edge_before_top_margin: MarginEdgeBeforeClearance::new(
                margin_edge_before_top_margin,
            ),
            used_top_margin,
        }
    }
}

/// Whether CSS `clear` introduced a margin-collapse boundary for one
/// non-floating block-level box.
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) enum BlockClearance {
    NotIntroduced,
    Introduced {
        space: ClearanceSpace,
        cleared_outer_block_end: ClearedFloatOuterBlockEnd,
    },
}

impl BlockClearance {
    pub(in crate::layout) fn is_introduced(self) -> bool {
        matches!(self, Self::Introduced { .. })
    }
}

/// The block-start margin relationship selected after CSS2 clearance has been
/// resolved. A zero or negative clearance space still selects the separated
/// relationship.
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) enum BlockStartMarginArrangement {
    Adjoining {
        applied_start_margin: LayoutLength,
    },
    SeparatedByClearance {
        adjusted_top_margin: ClearanceAdjustedTopMargin,
    },
}

impl BlockStartMarginArrangement {
    pub(in crate::layout) fn from_clearance(
        clearance: BlockClearance,
        applied_start_margin: LayoutLength,
    ) -> Self {
        match clearance {
            BlockClearance::NotIntroduced => Self::Adjoining {
                applied_start_margin,
            },
            BlockClearance::Introduced { space, .. } => Self::SeparatedByClearance {
                adjusted_top_margin: ClearanceAdjustedTopMargin::from_clearance_and_top_margin(
                    space,
                    applied_start_margin,
                ),
            },
        }
    }

    pub(in crate::layout) fn permits_parent_start_collapse(self) -> bool {
        matches!(self, Self::Adjoining { .. })
    }

    pub(in crate::layout) fn margin_collapse_boundary(self) -> BlockMarginCollapseBoundary {
        if self.permits_parent_start_collapse() {
            BlockMarginCollapseBoundary::Adjoining
        } else {
            BlockMarginCollapseBoundary::SeparatedByClearance
        }
    }

    pub(in crate::layout) fn applied_start_margin(self) -> Option<LayoutLength> {
        match self {
            Self::Adjoining {
                applied_start_margin,
            } => Some(applied_start_margin),
            Self::SeparatedByClearance { .. } => None,
        }
    }
}

#[cfg(test)]
mod clearance_margin_tests {
    use super::*;

    #[test]
    fn adjusted_top_margin_preserves_positive_zero_and_negative_clearance() {
        let margin_edge = PageTopBlockPosition::new(1_000.0);
        let top_margin = layout_pt(100.0);

        let positive = ClearanceAdjustedTopMargin::from_clearance_and_top_margin(
            ClearanceSpace::new(layout_pt(100.0)),
            top_margin,
        );
        let zero = ClearanceAdjustedTopMargin::from_clearance_and_top_margin(
            ClearanceSpace::new(layout_pt(0.0)),
            top_margin,
        );
        let negative = ClearanceAdjustedTopMargin::from_clearance_and_top_margin(
            ClearanceSpace::new(layout_pt(-50.0)),
            top_margin,
        );

        assert_eq!(
            positive.border_edge_from(margin_edge),
            PageTopBlockPosition::new(800.0)
        );
        assert_eq!(
            zero.border_edge_from(margin_edge),
            PageTopBlockPosition::new(900.0)
        );
        assert_eq!(
            negative.border_edge_from(margin_edge),
            PageTopBlockPosition::new(950.0)
        );
    }

    #[test]
    fn zero_clearance_still_separates_parent_start_margin_collapse() {
        let arrangement = BlockStartMarginArrangement::from_clearance(
            BlockClearance::Introduced {
                space: ClearanceSpace::new(layout_pt(0.0)),
                cleared_outer_block_end: ClearedFloatOuterBlockEnd::new(PageTopBlockPosition::new(
                    900.0,
                )),
            },
            layout_pt(100.0),
        );

        assert!(!arrangement.permits_parent_start_collapse());
        assert_eq!(
            arrangement.margin_collapse_boundary(),
            BlockMarginCollapseBoundary::SeparatedByClearance
        );
    }

    #[test]
    fn parent_start_hypothesis_can_require_negative_clearance() {
        let margin_edge = PageTopBlockPosition::new(1_000.0);
        let clearance = ClearanceSpace::flush_to_float(
            margin_edge,
            layout_pt(200.0),
            PageTopBlockPosition::new(950.0),
        );

        assert_eq!(clearance, ClearanceSpace::new(layout_pt(-150.0)));
        assert_eq!(
            clearance.applied_border_edge(margin_edge, layout_pt(200.0)),
            PageTopBlockPosition::new(950.0)
        );
    }
}

/// Fragmentainer progress performed while following a continued cleared
/// float.  A column is a fragmentainer just as a page is, so this count must
/// survive temporary multicolumn-page projection.
/// <https://drafts.csswg.org/css-break-3/#fragmentation-model>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::layout) enum ClearanceFragmentainerProgress {
    Current,
    Advanced { count: NonZeroUsize },
}

impl ClearanceFragmentainerProgress {
    pub(in crate::layout) fn advanced(self) -> bool {
        matches!(self, Self::Advanced { .. })
    }
}

/// The resolved CSS2 clearance state for a non-floating block-level box.
///
/// `used_border_edge` is the float-clearance target; callers use
/// [`BlockClearance`] rather than comparing positions to determine whether
/// margin collapse is inhibited.
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct ResolvedBlockClearance {
    pub(in crate::layout) hypothetical_border_edge: PageTopBlockPosition,
    pub(in crate::layout) used_border_edge: PageTopBlockPosition,
    pub(in crate::layout) clearance: BlockClearance,
    pub(in crate::layout) fragmentainer_progress: ClearanceFragmentainerProgress,
}

impl FloatRunState {
    pub(in crate::layout) fn new(row_span: PageInlineSpan, row_top: PageTopBlockPosition) -> Self {
        Self {
            row_span,
            available_span: row_span,
            occupied_block_span: PageBlockSpan::from_edges(row_top.points(), row_top.points()),
            active: false,
        }
    }

    pub(in crate::layout) fn include_shape(&mut self, shape: FloatShape) {
        if !shape.is_css_float() {
            return;
        }
        let shape_block_span = shape.margin_box_block_span();
        let shape_inline_span = shape.margin_box_inline_span();
        if (shape_block_span.top_y() - self.occupied_block_span.top_y()).abs() > 0.5 {
            return;
        }
        let mut left_x = self.available_span.left_x();
        let mut right_x = self.available_span.right_x();
        match shape.side {
            UsedFloatSide::Left => left_x = left_x.max(shape_inline_span.right_x()),
            UsedFloatSide::Right => right_x = right_x.min(shape_inline_span.left_x()),
            UsedFloatSide::Top | UsedFloatSide::Bottom => {}
        }
        self.available_span = PageInlineSpan::from_edges(left_x, right_x);
        self.occupied_block_span = PageBlockSpan::from_edges(
            self.occupied_block_span.top_y(),
            self.occupied_block_span
                .bottom_y()
                .min(shape_block_span.bottom_y()),
        );
        self.active = true;
    }

    pub(in crate::layout) fn reset_for_block(
        &mut self,
        row_span: PageInlineSpan,
        row_top: PageTopBlockPosition,
    ) {
        *self = Self::new(row_span, row_top);
    }
}

impl FloatContext {
    /// Whether this fragmentainer contains a CSS float in this block
    /// formatting context.
    ///
    /// A normal-flow BFC root needs float-avoidance placement only when an
    /// earlier CSS float can constrain its available inline space. Other flow
    /// exclusions, such as initial letters, do not participate in CSS 2.2
    /// float placement or clearance:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) fn has_css_float_on_page(&self, page_index: usize) -> bool {
        self.shapes
            .iter()
            .any(|shape| shape.is_css_float() && shape.page_index == page_index)
    }

    /// Resolve an initial letter's logical block-start column around already
    /// placed CSS floats.
    ///
    /// A physical `left`/`right` float in vertical writing occupies a
    /// block-axis column, not an inline-axis band. Initial letters are not
    /// CSS floats, but their used margin boxes cannot overlap that preceding
    /// physical float. Keep this query beside float exclusion handling so
    /// both ordinary float replay and initial-letter placement consume the
    /// same durable page-local geometry.
    /// <https://www.w3.org/TR/css-inline-3/#initial-letter-floats>
    /// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
    pub(in crate::layout) fn initial_letter_block_start_avoiding_x(
        &self,
        page_index: usize,
        writing_mode: WritingMode,
        desired_margin_box: PageTopRect,
    ) -> f32 {
        if writing_mode == WritingMode::HorizontalTb {
            return desired_margin_box.x();
        }
        let block_start_side = WritingModeAxes::new(writing_mode, Direction::Ltr)
            .physical_side(LogicalSide::BlockStart);
        debug_assert!(matches!(
            block_start_side,
            PhysicalSide::Left | PhysicalSide::Right
        ));
        self.shapes
            .iter()
            .filter(|shape| {
                let margin_box = shape.physical_margin_box();
                shape.is_css_float()
                    && shape.page_index == page_index
                    && margin_box.top_y()
                        > desired_margin_box.top_y() - desired_margin_box.height() - FLOAT_EPSILON
                    && margin_box.top_y() - margin_box.height()
                        < desired_margin_box.top_y() - FLOAT_EPSILON
                    && margin_box.x() + margin_box.width() > desired_margin_box.x() + FLOAT_EPSILON
                    && margin_box.x()
                        < desired_margin_box.x() + desired_margin_box.width() - FLOAT_EPSILON
            })
            .fold(desired_margin_box.x(), |x, shape| {
                let margin_box = shape.physical_margin_box();
                match block_start_side {
                    PhysicalSide::Right => x.max(margin_box.x() + margin_box.width()),
                    PhysicalSide::Left => x.min(margin_box.x() - desired_margin_box.width()),
                    PhysicalSide::Top | PhysicalSide::Bottom => unreachable!(),
                }
            })
    }

    /// Find the earliest later horizontal line slab with enough inline space.
    ///
    /// CSS Shapes changes the float area used for line wrapping. Unlike a
    /// rectangular float, a circle or rounded corner can make a line fit part
    /// way through its margin box, so retrying only at the margin-box bottom
    /// loses valid placement opportunities.
    /// <https://drafts.csswg.org/css-shapes-1/#relation-to-box-model-and-float-behavior>
    pub(in crate::layout) fn next_content_slab_with_width(
        &self,
        page_index: usize,
        starting_slab: PageBlockSpan,
        inline_span: PageInlineSpan,
        required_width: f32,
    ) -> Option<PageTopBlockPosition> {
        let slab_height = starting_slab.top_y() - starting_slab.bottom_y();
        let start_top = starting_slab.top_y();
        let fits_at = |top_y: f32| {
            self.content_band(
                page_index,
                PageBlockSpan::new(top_y, slab_height),
                inline_span,
            )
            .width()
                + FLOAT_EPSILON
                >= required_width
        };
        if fits_at(start_top) {
            return Some(PageTopBlockPosition::new(start_top));
        }

        let mut transition_tops = Vec::new();
        for shape in self
            .shapes
            .iter()
            .filter(|shape| shape.is_css_float() && shape.page_index == page_index)
        {
            shape
                .area
                .horizontal_transition_tops(shape.rect, slab_height, &mut transition_tops);
        }
        transition_tops.sort_by(|left, right| right.total_cmp(left));
        transition_tops.dedup_by(|left, right| (*left - *right).abs() <= FLOAT_EPSILON);

        let mut previous_top = start_top;
        for candidate_top in transition_tops {
            if candidate_top >= previous_top - FLOAT_EPSILON {
                continue;
            }
            if !fits_at(candidate_top) {
                previous_top = candidate_top;
                continue;
            }

            // A rectangular contour stops excluding exactly at its margin-box
            // block edge. Bisection is only meaningful for a continuous
            // curved boundary; applying it here manufactures an epsilon-sized
            // gap below every ordinary float.
            if self.shapes.iter().any(|shape| {
                shape.is_css_float()
                    && shape.page_index == page_index
                    && shape
                        .area
                        .has_discontinuous_horizontal_boundary_at(shape.rect, candidate_top)
            }) {
                return Some(PageTopBlockPosition::new(candidate_top));
            }

            // Within a contour transition interval the relevant outer edge is
            // monotonic. Refine the first fitting top rather than rounding to
            // an unrelated line-height increment.
            let mut not_fitting_top = previous_top;
            let mut fitting_top = candidate_top;
            for _ in 0..24 {
                let middle = (not_fitting_top + fitting_top) * 0.5;
                if fits_at(middle) {
                    fitting_top = middle;
                } else {
                    not_fitting_top = middle;
                }
            }
            return Some(PageTopBlockPosition::new(fitting_top));
        }
        None
    }

    /// Find the next vertical-writing physical block slab whose logical
    /// inline band can contain `required_width`.
    ///
    /// In vertical writing the line's physical `x` slab advances in the
    /// logical block direction, while the available measure is physical `y`.
    /// A line excluded by a drop initial must therefore advance to the first
    /// fitting *block slab*, not consume arbitrary nominal line slots while
    /// it remains inside the same initial-letter margin box.
    /// <https://drafts.csswg.org/css-writing-modes-4/#abstract-box>
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
    pub(in crate::layout) fn next_vertical_content_slab_with_width(
        &self,
        writing_mode: WritingMode,
        direction: Direction,
        page_index: usize,
        starting_slab: PageInlineSpan,
        vertical_inline_span: PageBlockSpan,
        required_width: f32,
    ) -> Option<PageInlinePosition> {
        debug_assert!(writing_mode != WritingMode::HorizontalTb);
        let block_progresses_right = matches!(
            writing_mode,
            WritingMode::VerticalLr | WritingMode::SidewaysLr
        );
        let slab_width = starting_slab.width();
        let fits_at = |left: f32| {
            let horizontal_slab = PageInlineSpan::new(left, slab_width);
            let occupied_by_block_side_float = self.shapes.iter().any(|shape| {
                let margin_box = shape.physical_margin_box();
                shape.is_css_float()
                    && shape.page_index == page_index
                    && matches!(shape.side, UsedFloatSide::Left | UsedFloatSide::Right)
                    && margin_box.x() + margin_box.width()
                        > horizontal_slab.left_x() + FLOAT_EPSILON
                    && margin_box.x() < horizontal_slab.right_x() - FLOAT_EPSILON
                    && margin_box.top_y() > vertical_inline_span.bottom_y() + FLOAT_EPSILON
                    && margin_box.bottom_y() < vertical_inline_span.top_y() - FLOAT_EPSILON
            });
            if occupied_by_block_side_float {
                return false;
            }
            self.content_logical_band(
                writing_mode,
                direction,
                page_index,
                FloatBandQuery {
                    horizontal_slab,
                    vertical_slab: vertical_inline_span,
                },
            )
            .inline_span
            .size()
                + FLOAT_EPSILON
                >= required_width
        };
        let starting_left = starting_slab.left_x();
        if fits_at(starting_left) {
            return Some(PageInlinePosition::new(starting_left));
        }

        let mut candidate_lefts = self
            .shapes
            .iter()
            .filter(|shape| shape.page_index == page_index)
            .filter_map(|shape| {
                let clip = shape.area.margin_clip.unwrap_or(shape.rect);
                let overlaps_inline_span = clip.top_y()
                    > vertical_inline_span.bottom_y() + FLOAT_EPSILON
                    && clip.bottom_y() < vertical_inline_span.top_y() - FLOAT_EPSILON;
                if !overlaps_inline_span {
                    return None;
                }
                if block_progresses_right {
                    Some(clip.x() + clip.width())
                } else {
                    Some(clip.x() - slab_width)
                }
            })
            .collect::<Vec<_>>();
        if block_progresses_right {
            candidate_lefts.sort_by(|left, right| left.total_cmp(right));
        } else {
            candidate_lefts.sort_by(|left, right| right.total_cmp(left));
        }
        candidate_lefts.dedup_by(|left, right| (*left - *right).abs() <= FLOAT_EPSILON);
        candidate_lefts
            .into_iter()
            .find(|candidate| {
                let is_later = if block_progresses_right {
                    *candidate > starting_left + FLOAT_EPSILON
                } else {
                    *candidate < starting_left - FLOAT_EPSILON
                };
                is_later && fits_at(*candidate)
            })
            .map(PageInlinePosition::new)
    }

    pub(in crate::layout) fn active_shapes(
        &self,
        page_index: usize,
        block_span: PageBlockSpan,
    ) -> impl Iterator<Item = &FloatShape> + '_ {
        self.shapes.iter().filter(move |shape| {
            let shape_block_span = shape.margin_box_block_span();
            shape.is_css_float()
                && shape.page_index == page_index
                && shape_block_span.top_y() > block_span.bottom_y() + FLOAT_EPSILON
                && shape_block_span.bottom_y() < block_span.top_y() - FLOAT_EPSILON
        })
    }

    /// Shapes that a later CSS float must avoid while choosing its physical
    /// margin-box position.
    ///
    /// Initial letters are not CSS floats for `clear` or float containment,
    /// but they are in-flow occupied geometry. A later float may not overlap
    /// that geometry merely because the initial did not create a CSS float.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
    pub(in crate::layout) fn active_placement_shapes(
        &self,
        page_index: usize,
        block_span: PageBlockSpan,
    ) -> impl Iterator<Item = &FloatShape> + '_ {
        self.shapes.iter().filter(move |shape| {
            let shape_block_span = shape.margin_box_block_span();
            shape.page_index == page_index
                && shape_block_span.top_y() > block_span.bottom_y() + FLOAT_EPSILON
                && shape_block_span.bottom_y() < block_span.top_y() - FLOAT_EPSILON
        })
    }

    /// Physical margin-box band used while placing a CSS float.
    ///
    /// This differs from [`Self::band`]: the latter intentionally contains
    /// only CSS floats, whereas float placement must also avoid an earlier
    /// in-flow initial letter without allowing CSS `clear` to observe it.
    pub(in crate::layout) fn placement_band(
        &self,
        page_index: usize,
        block_span: PageBlockSpan,
        inline_span: PageInlineSpan,
    ) -> FloatBand {
        let mut band_left = inline_span.left_x();
        let mut band_right = inline_span.right_x();
        for shape in self.active_placement_shapes(page_index, block_span) {
            match shape.side {
                UsedFloatSide::Left => {
                    band_left = band_left.max(shape.margin_box_inline_span().right_x())
                }
                UsedFloatSide::Right => {
                    band_right = band_right.min(shape.margin_box_inline_span().left_x())
                }
                UsedFloatSide::Top | UsedFloatSide::Bottom => {}
            }
        }
        if band_right < band_left {
            band_right = band_left;
        }
        FloatBand::from_edges(band_left, band_right)
    }

    /// Move a following same-side CSS float past an earlier initial letter's
    /// occupied block span.
    ///
    /// This is placement collision avoidance, not CSS `clear`: an initial
    /// letter does not match a later element's `clear` value, but a physical
    /// float on the same side cannot share the initial's in-flow exclusion
    /// column.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
    pub(in crate::layout) fn initial_letter_float_avoidance_top(
        &self,
        page_index: usize,
        top: PageTopBlockPosition,
        side: UsedFloatSide,
    ) -> PageTopBlockPosition {
        self.shapes
            .iter()
            .filter(|shape| {
                shape.kind == FlowExclusionKind::InitialLetter
                    && shape.page_index == page_index
                    && shape.side == side
            })
            .filter_map(|shape| {
                let span = shape.margin_box_block_span();
                (span.top_y() >= top.points() - FLOAT_EPSILON
                    && span.bottom_y() < top.points() - FLOAT_EPSILON)
                    .then_some(PageTopBlockPosition::new(span.bottom_y()))
            })
            .fold(top, PageTopBlockPosition::min)
    }

    /// Shapes active for content wrapping. CSS 2.2 placement continues to use
    /// the float's margin rectangle, but a replaced image contour may have a
    /// separately resolved used margin clip while that provisional rectangle
    /// is still zero-sized.
    fn active_content_shapes(
        &self,
        page_index: usize,
        block_span: PageBlockSpan,
    ) -> impl Iterator<Item = &FloatShape> + '_ {
        self.shapes.iter().filter(move |shape| {
            let clip = shape.area.margin_clip.unwrap_or(shape.rect);
            let clip_span = PageBlockSpan::new(clip.top_y(), clip.height());
            shape.page_index == page_index
                && clip_span.top_y() > block_span.bottom_y() + FLOAT_EPSILON
                && clip_span.bottom_y() < block_span.top_y() - FLOAT_EPSILON
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
                UsedFloatSide::Left => {
                    band_left = band_left.max(shape.margin_box_inline_span().right_x())
                }
                UsedFloatSide::Right => {
                    band_right = band_right.min(shape.margin_box_inline_span().left_x())
                }
                UsedFloatSide::Top | UsedFloatSide::Bottom => {}
            }
        }
        if band_right < band_left {
            band_right = band_left;
        }
        FloatBand::from_edges(band_left, band_right)
    }

    /// Available horizontal content band after CSS Shapes float areas.
    /// Unlike [`Self::band`], this is not used to place later floats: CSS
    /// Shapes only changes wrapping around an already-positioned float.
    pub(in crate::layout) fn content_band(
        &self,
        page_index: usize,
        block_span: PageBlockSpan,
        inline_span: PageInlineSpan,
    ) -> FloatBand {
        let mut band_left = inline_span.left_x();
        let mut band_right = inline_span.right_x();
        for shape in self.active_content_shapes(page_index, block_span) {
            let Some(shape_span) = shape.area.horizontal_edges(shape.rect, block_span) else {
                continue;
            };
            match shape.side {
                UsedFloatSide::Left => band_left = band_left.max(shape_span.right_x()),
                UsedFloatSide::Right => band_right = band_right.min(shape_span.left_x()),
                UsedFloatSide::Top | UsedFloatSide::Bottom => {}
            }
        }
        if band_right < band_left {
            band_right = band_left;
        }
        FloatBand::from_edges(band_left, band_right)
    }

    pub(in crate::layout) fn logical_band(
        &self,
        writing_mode: WritingMode,
        direction: Direction,
        page_index: usize,
        query: FloatBandQuery,
    ) -> LogicalFloatBand {
        let axes = WritingModeAxes::new(writing_mode, direction);
        let slab_left = query.horizontal_slab.left_x();
        let slab_right = query.horizontal_slab.right_x();
        let span_top = query.vertical_slab.top_y();
        let span_bottom = query.vertical_slab.bottom_y();
        if !axes.swaps_physical_axes() {
            let band = self.band(page_index, query.vertical_slab, query.horizontal_slab);
            let (inline_start, inline_end) = if axes.is_reversed(LogicalAxis::Inline) {
                (slab_right - band.right(), slab_right - band.left())
            } else {
                (band.left() - slab_left, band.right() - slab_left)
            };
            let inline_start = inline_start.max(0.0);
            let inline_end = inline_end
                .max(inline_start)
                .min(query.horizontal_slab.width());
            LogicalFloatBand::new(
                LogicalInlineSpan::new(inline_start, inline_end - inline_start),
                PageBlockSpan::from_edges(span_top, span_bottom),
            )
        } else {
            let inline_start_side = axes.physical_side(LogicalSide::InlineStart);
            let mut band_top = span_top;
            let mut band_bottom = span_bottom;
            for shape in self.shapes.iter().filter(|shape| {
                let shape_inline_span = shape.margin_box_inline_span();
                let shape_block_span = shape.margin_box_block_span();
                shape.is_css_float()
                    && shape.page_index == page_index
                    && shape_inline_span.right_x() > slab_left + FLOAT_EPSILON
                    && shape_inline_span.left_x() < slab_right - FLOAT_EPSILON
                    && shape_block_span.top_y() > span_bottom + FLOAT_EPSILON
                    && shape_block_span.bottom_y() < span_top - FLOAT_EPSILON
            }) {
                let Some(edge) = vertical_writing_shape_band_edge(shape.side, axes) else {
                    continue;
                };
                match edge {
                    VerticalShapeBandEdge::InlineStart => {
                        band_top = band_top.min(shape.margin_box_block_span().bottom_y())
                    }
                    VerticalShapeBandEdge::InlineEnd => {
                        band_bottom = band_bottom.max(shape.margin_box_block_span().top_y())
                    }
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
            let inline_end = inline_end.max(inline_start).min(span_top - span_bottom);
            LogicalFloatBand::new(
                LogicalInlineSpan::new(inline_start, inline_end - inline_start),
                PageBlockSpan::from_edges(band_top, band_bottom),
            )
        }
    }

    pub(in crate::layout) fn content_logical_band(
        &self,
        writing_mode: WritingMode,
        direction: Direction,
        page_index: usize,
        query: FloatBandQuery,
    ) -> LogicalFloatBand {
        let axes = WritingModeAxes::new(writing_mode, direction);
        let slab_left = query.horizontal_slab.left_x();
        let slab_right = query.horizontal_slab.right_x();
        let span_top = query.vertical_slab.top_y();
        let span_bottom = query.vertical_slab.bottom_y();
        if !axes.swaps_physical_axes() {
            let band = self.content_band(page_index, query.vertical_slab, query.horizontal_slab);
            let (inline_start, inline_end) = if axes.is_reversed(LogicalAxis::Inline) {
                (slab_right - band.right(), slab_right - band.left())
            } else {
                (band.left() - slab_left, band.right() - slab_left)
            };
            LogicalFloatBand::new(
                LogicalInlineSpan::new(
                    inline_start.max(0.0),
                    (inline_end - inline_start)
                        .max(0.0)
                        .min(query.horizontal_slab.width()),
                ),
                PageBlockSpan::from_edges(span_top, span_bottom),
            )
        } else {
            let inline_start_side = axes.physical_side(LogicalSide::InlineStart);
            let mut band_top = span_top;
            let mut band_bottom = span_bottom;
            for shape in self
                .active_content_shapes(page_index, PageBlockSpan::from_edges(span_top, span_bottom))
                .filter(|shape| {
                    let clip = shape.area.margin_clip.unwrap_or(shape.rect);
                    clip.x() + clip.width() > slab_left + FLOAT_EPSILON
                        && clip.x() < slab_right - FLOAT_EPSILON
                })
            {
                let Some(shape_span) = shape.area.vertical_edges(
                    shape.rect,
                    PageInlineSpan::from_edges(slab_left, slab_right),
                ) else {
                    continue;
                };
                let Some(edge) = vertical_writing_shape_band_edge(shape.side, axes) else {
                    continue;
                };
                match edge {
                    VerticalShapeBandEdge::InlineStart => {
                        band_top = band_top.min(shape_span.bottom_y())
                    }
                    VerticalShapeBandEdge::InlineEnd => {
                        band_bottom = band_bottom.max(shape_span.top_y())
                    }
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
            LogicalFloatBand::new(
                LogicalInlineSpan::new(
                    inline_start.max(0.0),
                    (inline_end - inline_start)
                        .max(0.0)
                        .min(span_top - span_bottom),
                ),
                PageBlockSpan::from_edges(band_top, band_bottom),
            )
        }
    }

    pub(in crate::layout) fn clearance_top(
        &self,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        page_index: usize,
        hypothetical_border_edge: HypotheticalClearBorderEdge,
    ) -> PageTopBlockPosition {
        self.clearance_target(
            clear,
            writing_mode,
            direction,
            page_index,
            hypothetical_border_edge,
        )
        .lowest_matching_outer_block_end
        .map(ClearedFloatOuterBlockEnd::position)
        .unwrap_or_else(|| hypothetical_border_edge.position())
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
    pub(in crate::layout) fn clearance_target(
        &self,
        clear: Clear,
        writing_mode: WritingMode,
        direction: Direction,
        page_index: usize,
        hypothetical_border_edge: HypotheticalClearBorderEdge,
    ) -> FloatClearanceTarget {
        if clear == Clear::None {
            return FloatClearanceTarget {
                lowest_matching_outer_block_end: None,
                continued_float: None,
            };
        }
        let mut lowest_matching_outer_block_end: Option<ClearedFloatOuterBlockEnd> = None;
        let mut continued_float = None;
        for shape in self.shapes.iter().filter(|shape| {
            shape.is_css_float()
                && shape.page_index == page_index
                && shape.side.matches_clear(clear, writing_mode, direction)
                && shape.margin_box_block_span().bottom_y()
                    < hypothetical_border_edge.position().points() + FLOAT_EPSILON
        }) {
            let block_end = ClearedFloatOuterBlockEnd::new(PageTopBlockPosition::new(
                shape.margin_box_block_span().bottom_y(),
            ));
            lowest_matching_outer_block_end = Some(
                lowest_matching_outer_block_end
                    .map_or(block_end, |current: ClearedFloatOuterBlockEnd| {
                        current.lowest(block_end)
                    }),
            );
            if shape.continues_on_next_page {
                continued_float = Some(shape.id);
            }
        }
        FloatClearanceTarget {
            lowest_matching_outer_block_end,
            continued_float,
        }
    }

    pub(in crate::layout) fn lowest_bottom_on_page(
        &self,
        page_index: usize,
    ) -> Option<PageTopBlockPosition> {
        self.shapes
            .iter()
            .filter(|shape| shape.is_css_float() && shape.page_index == page_index)
            .map(|shape| PageTopBlockPosition::new(shape.margin_box_block_span().bottom_y()))
            .min_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Return the final page-local block-end occupied by this float context.
    ///
    /// An independent block formatting context encloses its internal floats.
    /// When a float is graphically fragmented, that enclosure reaches the
    /// lowest fragment on the last page rather than stopping at the first
    /// page's local bottom.
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn last_fragment_end(&self) -> Option<(usize, PageTopBlockPosition)> {
        let last_page_index = self
            .shapes
            .iter()
            .filter(|shape| shape.is_css_float())
            .map(|shape| shape.page_index)
            .max()?;
        self.lowest_bottom_on_page(last_page_index)
            .map(|bottom| (last_page_index, bottom))
    }
}

/// Map a physical float side to the inline edge it excludes in a vertical
/// writing-mode line slab.
///
/// CSS `float: left` and `right` remain physical sides. When the inline axis
/// is vertical, though, left/right are block-start/block-end floats and their
/// shape contour determines which vertical outer edge later content may use.
/// A block-start float excludes through the contour's physical lower edge;
/// a block-end float excludes through its upper edge.
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
/// <https://drafts.csswg.org/TR/css-shapes-1/#relation-to-box-model-and-float-behavior>
enum VerticalShapeBandEdge {
    InlineStart,
    InlineEnd,
}

fn vertical_writing_shape_band_edge(
    side: UsedFloatSide,
    axes: WritingModeAxes,
) -> Option<VerticalShapeBandEdge> {
    let physical_side = match side {
        UsedFloatSide::Left => PhysicalSide::Left,
        UsedFloatSide::Right => PhysicalSide::Right,
        UsedFloatSide::Top => PhysicalSide::Top,
        UsedFloatSide::Bottom => PhysicalSide::Bottom,
    };
    // A physical float can occupy either an inline or block edge of a
    // vertical line. Once the queried physical block slab intersects that
    // float, both a logical inline-start float (top/bottom) and a logical
    // block-start float (left/right) reserve the contour before the line's
    // inline content; the corresponding end sides reserve its tail. This is
    // the physical-side projection required for CSS `float:left/right` in
    // vertical writing, rather than treating them as absent from the line.
    // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
    // <https://drafts.csswg.org/css-shapes-1/#relation-to-box-model-and-float-behavior>
    if physical_side == axes.physical_side(LogicalSide::InlineStart)
        || physical_side == axes.physical_side(LogicalSide::BlockStart)
    {
        Some(VerticalShapeBandEdge::InlineStart)
    } else if physical_side == axes.physical_side(LogicalSide::InlineEnd)
        || physical_side == axes.physical_side(LogicalSide::BlockEnd)
    {
        Some(VerticalShapeBandEdge::InlineEnd)
    } else {
        unreachable!("physical sides cover both logical axes")
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
        FloatRunState::new(
            PageInlineSpan::from_edges(self.content_left, self.content_right),
            PageTopBlockPosition::new(self.cursor_y),
        )
    }

    /// Remove initial-letter exclusions left by a provisional layout pass at
    /// this block's current physical origin.
    ///
    /// Intrinsic and fragmentation probes shape the same first letter before
    /// the final line sequence is committed. Unlike CSS floats, an initial
    /// letter is an in-flow participant and those probe-local exclusions must
    /// not shift the committed initial into a later line slot. A real earlier
    /// initial has already advanced normal flow by at least one strut, so its
    /// top edge is outside this half-leading proximity window and remains
    /// available to wrap a short following block.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
    pub(in crate::layout) fn discard_provisional_initial_letter_exclusions(
        &mut self,
        block_style: &ComputedStyle,
    ) {
        let page_index = self.current_float_page_index();
        let proximity = (block_style.line_height * 0.5).max(FLOAT_EPSILON);
        let cursor_y = self.cursor_y;
        self.float_contexts
            .last_mut()
            .expect("root float context exists")
            .shapes
            .retain(|shape| {
                shape.kind != FlowExclusionKind::InitialLetter
                    || shape.page_index != page_index
                    || shape.initial_letter.as_ref().is_none_or(|layout| {
                        !layout.provisional || (shape.rect.top_y() - cursor_y).abs() > proximity
                    })
            });
    }

    /// Clear page-local initial-letter exclusions before starting a later
    /// initial letter in the same block formatting context.
    ///
    /// Initial letters participate in ordinary line wrapping, so a short
    /// following block may still wrap around one. They are not CSS floats,
    /// however, and CSS Inline requires a subsequent initial to start below
    /// the prior initial rather than sharing its exclusion band.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
    pub(in crate::layout) fn clear_initial_letter_exclusions_for_new_initial(
        &mut self,
        _block_style: &ComputedStyle,
    ) {
        let page_index = self.current_float_page_index();
        // `FloatShape::rect` is the line-slab wrapping box and already
        // includes the root-strut alignment allowance. Its physical block
        // end is therefore the following initial's clearance edge.
        let cleared_block_end = self
            .float_contexts
            .last()
            .expect("root float context exists")
            .shapes
            .iter()
            .filter(|shape| {
                shape.kind == FlowExclusionKind::InitialLetter
                    && shape.page_index == page_index
                    // A graph-selection probe wraps the current initial's
                    // companion source but is not an earlier in-flow initial.
                    // It must never advance this block's cursor when the
                    // committed first line begins.
                    && shape
                        .initial_letter
                        .as_ref()
                        .is_none_or(|layout| !layout.provisional)
            })
            .map(|shape| shape.rect.bottom_y())
            // Page block coordinates decrease toward the physical page
            // bottom, so clearing advances to the smallest active bottom.
            .fold(self.cursor_y, f32::min);
        self.cursor_y = cleared_block_end;
        self.float_contexts
            .last_mut()
            .expect("root float context exists")
            .shapes
            .retain(|shape| shape.kind != FlowExclusionKind::InitialLetter);
    }

    /// Compatibility hook for old row-flush call sites.
    ///
    /// Durable CSS floats do not advance the block cursor when a run ends.
    pub(in crate::layout) fn flush_float_run(&mut self, run: &mut FloatRunState) {
        run.reset_for_block(
            PageInlineSpan::from_edges(self.content_left, self.content_right),
            PageTopBlockPosition::new(self.cursor_y),
        );
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

    /// Return the float-shortened physical line span for an explicit
    /// containing span.
    ///
    /// Most callers use [`Self::current_float_band`], but outside-marker
    /// fallbacks retain their containing geometry after descendant layout and
    /// therefore must not read the builder's transient content edges.
    pub(in crate::layout) fn float_band_in_span(
        &self,
        block_span: PageBlockSpan,
        containing_inline_span: PageInlineSpan,
    ) -> FloatBand {
        let offset = self.inline_split_float_exclusion_query_offset;
        let translated_block_span = PageBlockSpan::from_edges(
            block_span.top_y() + offset.y(),
            block_span.bottom_y() + offset.y(),
        );
        let translated_inline_span = PageInlineSpan::from_edges(
            containing_inline_span.left_x() + offset.x(),
            containing_inline_span.right_x() + offset.x(),
        );
        let band = self
            .float_contexts
            .last()
            .expect("root float context exists")
            .content_band(
                self.current_float_page_index(),
                translated_block_span,
                translated_inline_span,
            );
        FloatBand::from_edges(band.left() - offset.x(), band.right() - offset.x())
    }

    pub(in crate::layout) fn current_float_band(&self, block_span: PageBlockSpan) -> FloatBand {
        self.float_band_in_span(
            block_span,
            PageInlineSpan::from_edges(self.content_left, self.content_right),
        )
    }

    pub(in crate::layout) fn current_logical_float_band(
        &self,
        writing_mode: WritingMode,
        direction: Direction,
        query: FloatBandQuery,
    ) -> LogicalFloatBand {
        let offset = self.inline_split_float_exclusion_query_offset;
        let translated_query = FloatBandQuery {
            horizontal_slab: PageInlineSpan::from_edges(
                query.horizontal_slab.left_x() + offset.x(),
                query.horizontal_slab.right_x() + offset.x(),
            ),
            vertical_slab: PageBlockSpan::from_edges(
                query.vertical_slab.top_y() + offset.y(),
                query.vertical_slab.bottom_y() + offset.y(),
            ),
        };
        self.float_contexts
            .last()
            .expect("root float context exists")
            .content_logical_band(
                writing_mode,
                direction,
                self.current_float_page_index(),
                translated_query,
            )
    }

    pub(in crate::layout) fn active_float_exclusions_at(&self, block_span: PageBlockSpan) -> bool {
        let band = self.current_float_band(block_span);
        band.left() > self.content_left + FLOAT_EPSILON
            || band.right() < self.content_right - FLOAT_EPSILON
    }

    /// Resolve CSS2 clearance for a non-floating block-level box.
    ///
    /// CSS 2.2 clearance is page-local for each float fragment, but CSS
    /// Fragmentation can split a prior float across fragmentainers. When a
    /// matching fragment continues, clearance must progress to the next page
    /// and clear the next page-local fragment before normal flow resumes:
    /// <https://www.w3.org/TR/CSS22/visuren.html#flow-control> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn resolve_block_clearance(
        &mut self,
        request: BlockClearanceRequest,
    ) -> ResolvedBlockClearance {
        if request.clear == Clear::None {
            return ResolvedBlockClearance {
                hypothetical_border_edge: request.hypothetical_border_edge.position(),
                used_border_edge: request.uncleared_border_edge.position(),
                clearance: BlockClearance::NotIntroduced,
                fragmentainer_progress: ClearanceFragmentainerProgress::Current,
            };
        }
        let mut clearance_query_edge = request.hypothetical_border_edge;
        let mut used_border_edge = request.uncleared_border_edge.position();
        let mut margin_edge_for_clearance = request.margin_edge_before_top_margin.position();
        let mut cleared_continuations = 0usize;
        let mut clearance = BlockClearance::NotIntroduced;
        loop {
            let target = self
                .float_contexts
                .last()
                .expect("root float context exists")
                .clearance_target(
                    request.clear,
                    request.writing_mode,
                    request.direction,
                    self.current_float_page_index(),
                    clearance_query_edge,
                );
            if let Some(cleared_outer_block_end) = target.lowest_matching_outer_block_end {
                // Clearance is inserted before the top margin.  The current
                // page coordinate decreases toward block end; construct and
                // apply the signed margin-space arrangement instead of
                // treating clearance as a cursor adjustment.
                let space = ClearanceSpace::flush_to_float(
                    margin_edge_for_clearance,
                    request.used_top_margin,
                    cleared_outer_block_end.position(),
                );
                used_border_edge =
                    space.applied_border_edge(margin_edge_for_clearance, request.used_top_margin);
                clearance = BlockClearance::Introduced {
                    space,
                    cleared_outer_block_end,
                };
            }
            let Some(continued_float) = target.continued_float else {
                let fragmentainer_progress = NonZeroUsize::new(cleared_continuations)
                    .map_or(ClearanceFragmentainerProgress::Current, |count| {
                        ClearanceFragmentainerProgress::Advanced { count }
                    });
                return ResolvedBlockClearance {
                    hypothetical_border_edge: request.hypothetical_border_edge.position(),
                    used_border_edge,
                    clearance,
                    fragmentainer_progress,
                };
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
                let fragmentainer_progress = NonZeroUsize::new(cleared_continuations)
                    .map_or(ClearanceFragmentainerProgress::Current, |count| {
                        ClearanceFragmentainerProgress::Advanced { count }
                    });
                return ResolvedBlockClearance {
                    hypothetical_border_edge: request.hypothetical_border_edge.position(),
                    used_border_edge,
                    clearance,
                    fragmentainer_progress,
                };
            }
            self.cursor_y = used_border_edge.points();
            self.push_page();
            clearance_query_edge =
                HypotheticalClearBorderEdge::new(PageTopBlockPosition::new(self.cursor_y));
            used_border_edge = clearance_query_edge.position();
            margin_edge_for_clearance = clearance_query_edge.position();
            cleared_continuations += 1;
        }
    }

    /// Return the lowest margin-box edge of floats in the current BFC fragment.
    ///
    /// CSS 2.2 makes auto-height block formatting context roots expand to
    /// include floats that belong to that root's formatting context:
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height>.
    pub(in crate::layout) fn current_float_context_lowest_bottom(
        &self,
    ) -> Option<PageTopBlockPosition> {
        self.float_contexts
            .last()
            .expect("root float context exists")
            .lowest_bottom_on_page(self.current_float_page_index())
    }

    /// Return the last page and lowest margin-box edge of a fragmented float
    /// context.
    pub(in crate::layout) fn current_float_context_last_fragment_end(
        &self,
    ) -> Option<(usize, PageTopBlockPosition)> {
        self.float_contexts
            .last()
            .expect("root float context exists")
            .last_fragment_end()
    }
}
