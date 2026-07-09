use super::*;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineFloatBand {
    pub(in crate::layout) span: LogicalInlineSpan,
}

impl InlineFloatBand {
    pub(in crate::layout) fn new(left_offset: f32, width: f32) -> Self {
        Self {
            span: LogicalInlineSpan::new(left_offset, width.max(0.0)),
        }
    }

    pub(in crate::layout) fn left_offset(self) -> f32 {
        self.span.start()
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.span.size()
    }

    pub(in crate::layout) fn end(self) -> f32 {
        self.span.end()
    }
}

pub(in crate::layout) const INLINE_FLOAT_EPSILON: f32 = 0.01;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct SelectedInlineLineEnd {
    pub(in crate::layout) position: InlineGraphPosition,
    pub(in crate::layout) break_opportunity: Option<InlineBreakOpportunity>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineFloatPlacement {
    /// Physical horizontal line-box band that accepted the inline float.
    ///
    /// CSS 2.2 floats shorten line boxes in the current block formatting
    /// context. This span is page-local physical `x` after writing-mode and
    /// `direction` have already been resolved for the horizontal line:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) line_span: PageInlineSpan,
    /// Inline advance consumed by text before the same-line float.
    ///
    /// The same-line optimization only runs for horizontal LTR suffix layout,
    /// so this is a physical advance from the line span's left edge in the
    /// already-resolved line-box coordinate system:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>.
    pub(in crate::layout) prefix_width: f32,
    /// Physical margin-box span of the placed inline float.
    ///
    /// The span comes from the durable float exclusion shape and is used to
    /// split the remaining line into the gap before/after the float.
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) float_span: PageInlineSpan,
    pub(in crate::layout) fits_remaining_band: bool,
    pub(in crate::layout) side: UsedFloatSide,
}

impl InlineFloatPlacement {
    pub(in crate::layout) fn new(
        line_left: f32,
        line_right: f32,
        prefix_width: f32,
        float_left: f32,
        float_right: f32,
        fits_remaining_band: bool,
        side: UsedFloatSide,
    ) -> Self {
        Self {
            line_span: PageInlineSpan::from_edges(line_left, line_right),
            prefix_width: prefix_width.max(0.0),
            float_span: PageInlineSpan::from_edges(float_left, float_right),
            fits_remaining_band,
            side,
        }
    }

    pub(in crate::layout) fn line_right(self) -> f32 {
        self.line_span.right_x()
    }

    pub(in crate::layout) fn prefix_right(self) -> f32 {
        self.line_span.left_x() + self.prefix_width
    }

    pub(in crate::layout) fn float_left(self) -> f32 {
        self.float_span.left_x()
    }

    pub(in crate::layout) fn float_right(self) -> f32 {
        self.float_span.right_x()
    }

    pub(in crate::layout) fn fits_remaining_band(self) -> bool {
        self.fits_remaining_band
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct CombinedInlineFloatLine {
    pub(in crate::layout) end: InlineGraphPosition,
    pub(in crate::layout) fragment: InlineLineFragment,
}
