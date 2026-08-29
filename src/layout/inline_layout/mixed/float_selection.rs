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

/// The two inline measures relevant while selecting one source line.
///
/// A CSS Shapes or initial-letter exclusion narrows the line's usable band,
/// but it never changes the containing block's logical inline measure. Keeping
/// them in one typed record prevents float retry from comparing source fit to
/// an already-reduced band as though that band were the containing block.
/// <https://drafts.csswg.org/css-inline-3/#line-layout>
/// <https://drafts.csswg.org/TR/css-shapes-1/#relation-to-box-model-and-float-behavior>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineSelectionMeasures {
    containing_inline_size: f32,
    float_band: InlineFloatBand,
}

impl InlineSelectionMeasures {
    pub(in crate::layout) fn new(containing_inline_size: f32, float_band: InlineFloatBand) -> Self {
        Self {
            containing_inline_size: containing_inline_size.max(0.0),
            float_band,
        }
    }

    pub(in crate::layout) fn band_after_indent(self, indent: f32) -> f32 {
        (self.float_band.width() - indent).max(0.0)
    }

    pub(in crate::layout) fn containing_after_indent(self, indent: f32) -> f32 {
        (self.containing_inline_size - indent).max(0.0)
    }
}

pub(in crate::layout) const INLINE_FLOAT_EPSILON: f32 = 0.01;

/// Whether the remaining source has an authoritative CSS Text wrapping
/// opportunity.
///
/// A float placement marker is deliberately not represented here: floats
/// change line-box geometry but never manufacture a text break. Keeping this
/// separate from source-order float handling prevents a `nowrap` descendant
/// from being treated as wrappable merely because an earlier word had a
/// hyphenation opportunity:
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineSourceBreakability {
    HasLegalSoftWrap,
    Unbreakable,
}

impl InlineSourceBreakability {
    pub(in crate::layout) const fn has_legal_soft_wrap(self) -> bool {
        matches!(self, Self::HasLegalSoftWrap)
    }
}

/// The source-order relationship between one selected range and its first
/// inline float marker.
///
/// An inherited `pre`/`nowrap` value belongs to the float's source
/// continuation. Its placement may move to a later float row, but that marker
/// cannot be used to split the continuation or to rewind a prior hyphenation
/// choice:
/// <https://www.w3.org/TR/css-text-3/#white-space-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineFloatAffectedRange {
    None,
    Wrappable { marker: InlineGraphPosition },
    UnbreakableContinuation { marker: InlineGraphPosition },
}

impl InlineFloatAffectedRange {
    pub(in crate::layout) const fn marker(self) -> Option<InlineGraphPosition> {
        match self {
            Self::None => None,
            Self::Wrappable { marker } | Self::UnbreakableContinuation { marker } => Some(marker),
        }
    }
}

/// Result of placing a source-order float against one selected line band.
///
/// A zero-width CSS line band is still a valid placement context: an empty
/// float has zero geometry there and an oversized float overflows it. Keeping
/// that state distinct from a rejected placement prevents the selector from
/// turning the float marker into an artificial CSS Text line break.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
/// <https://www.w3.org/TR/css-text-3/#line-break-details>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineFloatBandPlacement {
    Placed,
    PlacedInZeroWidthBand,
    Rejected,
}

impl InlineFloatBandPlacement {
    pub(in crate::layout) const fn is_placed(self) -> bool {
        matches!(self, Self::Placed | Self::PlacedInZeroWidthBand)
    }
}

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
    /// Usable physical line span after committing the float's exclusion.
    ///
    /// This remains a non-negative line-wrap interval. The float's CSS 2.2
    /// outer margin edges are used only to decide whether it fits; a negative
    /// outer extent must not be reconstructed as a line exclusion here.
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) post_float_span: PageInlineSpan,
    pub(in crate::layout) fits_remaining_band: bool,
}

impl InlineFloatPlacement {
    pub(in crate::layout) fn new(
        line_left: f32,
        line_right: f32,
        prefix_width: f32,
        post_float_span: PageInlineSpan,
        fits_remaining_band: bool,
    ) -> Self {
        Self {
            line_span: PageInlineSpan::from_edges(line_left, line_right),
            prefix_width: prefix_width.max(0.0),
            post_float_span,
            fits_remaining_band,
        }
    }

    pub(in crate::layout) fn prefix_right(self) -> f32 {
        self.line_span.left_x() + self.prefix_width
    }

    pub(in crate::layout) fn fits_remaining_band(self) -> bool {
        self.fits_remaining_band
    }

    pub(in crate::layout) fn same_line_suffix_start(self) -> f32 {
        self.prefix_right().max(self.post_float_span.left_x())
    }

    pub(in crate::layout) fn same_line_suffix_available_width(self) -> f32 {
        (self.post_float_span.right_x() - self.same_line_suffix_start()).max(0.0)
    }

    pub(in crate::layout) fn same_line_suffix_gap(self) -> f32 {
        (self.same_line_suffix_start() - self.prefix_right()).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_line_suffix_uses_post_float_exclusion_not_signed_outer_extent() {
        // A preceding prefix and a right float with a negative end margin can
        // leave the whole line available for the suffix. The signed float
        // edges chose its placement, but they must not manufacture a suffix
        // gap or reduce the suffix's non-negative line band.
        let placement = InlineFloatPlacement::new(
            0.0,
            100.0,
            30.0,
            PageInlineSpan::from_edges(0.0, 100.0),
            true,
        );

        assert_eq!(placement.same_line_suffix_gap(), 0.0);
        assert_eq!(placement.same_line_suffix_available_width(), 70.0);
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct CombinedInlineFloatLine {
    pub(in crate::layout) end: InlineGraphPosition,
    pub(in crate::layout) fragment: InlineLineFragment,
}
