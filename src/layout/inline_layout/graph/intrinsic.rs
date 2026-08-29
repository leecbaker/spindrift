use super::*;

/// The min/max-content contributions of inline content on the consuming
/// formatting context's logical inline axis.
///
/// These are content-box sizes, not physical widths.  A parent with an
/// orthogonal writing mode projects its child contribution before constructing
/// this record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InlineIntrinsicContribution {
    pub(in crate::layout) min_content: LogicalInlineContentSize,
    pub(in crate::layout) max_content: LogicalInlineContentSize,
}

impl InlineIntrinsicContribution {
    pub(in crate::layout) fn new(
        min_content: LogicalInlineContentSize,
        max_content: LogicalInlineContentSize,
    ) -> Self {
        debug_assert!(min_content.points() <= max_content.points());
        Self {
            min_content,
            max_content,
        }
    }

    pub(in crate::layout) fn zero() -> Self {
        Self::new(
            LogicalInlineContentSize::new(content_box_pt(0.0)),
            LogicalInlineContentSize::new(content_box_pt(0.0)),
        )
    }

    /// Include another intrinsic contribution by taking the max-content
    /// contribution independently on each logical inline measure.
    pub(in crate::layout) fn include_max(&mut self, other: Self) {
        self.min_content = self.min_content.max(other.min_content);
        self.max_content = self.max_content.max(other.max_content);
    }
}

impl Default for InlineIntrinsicContribution {
    fn default() -> Self {
        Self::zero()
    }
}

/// Graph-backed intrinsic measurement for one inline paragraph.
///
/// CSS Sizing defines min/max-content contributions from inline break
/// opportunities, while CSS Flexbox also needs the line fragments that a block
/// layout would create for hypothetical cross sizes:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic>,
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>,
/// <https://www.w3.org/TR/css-inline-3/#line-box>, and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineMeasuredParagraph {
    // Kept with intrinsic measurements so future fragmentation can reuse the
    // graph that produced the current line sequence instead of recomputing it.
    #[allow(dead_code)]
    pub(in crate::layout) graph: InlineOpportunityGraph,
    // Kept as paragraph-local intrinsic metadata for future multi-paragraph
    // fragmentation decisions; the aggregate contribution is read today.
    #[allow(dead_code)]
    pub(in crate::layout) contribution: InlineIntrinsicContribution,
}

/// Durable intrinsic measurement for inline content.
///
/// Flex, shrink-to-fit, table, and atomic-inline estimates consume the same
/// graph-backed contribution and selected line fragments instead of
/// independently walking text or descendant trees:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>,
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>,
/// <https://www.w3.org/TR/css-inline-3/#line-layout>, and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct InlineIntrinsicMeasurement {
    pub(in crate::layout) paragraphs: Vec<InlineMeasuredParagraph>,
    pub(in crate::layout) sequence: InlineLineSequence,
    pub(in crate::layout) contribution: InlineIntrinsicContribution,
}

/// The containing-block inputs that may change an intrinsic inline result.
///
/// Consumers such as Grid use this proof to avoid repeating a measurement
/// only when the already-built opportunity graphs show that a different
/// available inline size cannot select a different line stack.  It deliberately
/// says nothing about non-inline formatting contexts, which retain their own
/// conservative dependency handling at their measurement boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct IntrinsicMeasurementSensitivity {
    pub(in crate::layout) block_extent_depends_on_available_inline_size: bool,
}

impl InlineIntrinsicMeasurement {
    /// Return the basis sensitivity proved by the retained opportunity graphs.
    ///
    /// A forced break preserves the same line stack at every available inline
    /// size. Every other graph opportunity can select a different stack, or
    /// represents an inline float boundary whose line geometry is likewise
    /// width-sensitive.
    pub(in crate::layout) fn sensitivity(&self) -> IntrinsicMeasurementSensitivity {
        IntrinsicMeasurementSensitivity {
            block_extent_depends_on_available_inline_size: self.paragraphs.iter().any(
                |paragraph| {
                    paragraph
                        .graph
                        .opportunities
                        .iter()
                        .any(|opportunity| !matches!(opportunity.kind, BreakEffect::Forced))
                },
            ),
        }
    }

    pub(in crate::layout) fn height(&self) -> f32 {
        self.sequence.total_height()
    }

    pub(in crate::layout) fn physical_height(&self, style: &ComputedStyle) -> f32 {
        match style.writing_mode {
            WritingMode::HorizontalTb => self.height(),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self
                .sequence
                .records
                .iter()
                .filter_map(|record| record.fragment.as_ref())
                .map(|fragment| fragment.metrics.width)
                .fold(0.0, f32::max),
        }
    }

    pub(in crate::layout) fn physical_width(&self, style: &ComputedStyle) -> f32 {
        match style.writing_mode {
            WritingMode::HorizontalTb => self.contribution.max_content.points(),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self.height(),
        }
    }

    /// Return the selected line stack's logical block-axis span.
    ///
    /// A selected line fragment records its physical inline extent for paint,
    /// but an intrinsic block-size walk needs the line's logical block
    /// advance. In vertical writing those are different physical axes, so
    /// derive the advance from the line participants instead of reprojecting
    /// `InlineLineMetrics::width`.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    /// <https://www.w3.org/TR/css-inline-3/#line-boxes>
    pub(in crate::layout) fn logical_block_span(&self, style: &ComputedStyle) -> f32 {
        match style.writing_mode {
            WritingMode::HorizontalTb => self.height(),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self
                .sequence
                .records
                .iter()
                // A collapsed-space-only record retains source-order data for
                // painting and extraction, but it does not generate a line
                // box. Counting it here would turn each discarded space into
                // an additional vertical column during intrinsic sizing.
                // <https://drafts.csswg.org/css-inline-3/#line-boxes>
                .filter(|record| !record.is_phantom)
                .map(|record| {
                    let line_block_size = record
                        .fragment
                        .as_ref()
                        .map(|fragment| {
                            fragment
                                .items
                                .iter()
                                .map(|item| inline_line_item_logical_block_size(&item.item, style))
                                .fold(style.line_height, f32::max)
                        })
                        .unwrap_or_else(|| record.height());
                    record.block_before + line_block_size
                })
                .sum(),
        }
    }

    pub(in crate::layout) fn line_count(&self) -> usize {
        self.sequence.line_count()
    }

    // Used by tests to lock down preserved forced-break accounting before
    // production fragmentation needs this value directly.
    #[allow(dead_code)]
    pub(in crate::layout) fn forced_empty_line_count(&self) -> usize {
        self.sequence.forced_empty_line_count()
    }
}
