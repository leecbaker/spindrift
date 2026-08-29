use crate::css::types::{
    ComputedLengthPercentage, ResolveViewportLengths, RootFontMetricLengthBasis,
    ViewportLengthBasis,
};
use crate::units::{LayoutLength, PercentageBasis, layout_pt};

/// Computed CSS `text-box-trim`.
///
/// CSS Inline Layout Level 3 lets block containers and inline boxes trim
/// leading from the start and/or end side of their formatted line boxes:
/// <https://drafts.csswg.org/css-inline-3/#text-box-trim>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextBoxTrim {
    None,
    TrimStart,
    TrimEnd,
    TrimBoth,
}

impl TextBoxTrim {
    pub(crate) fn trims_start(self) -> bool {
        matches!(self, Self::TrimStart | Self::TrimBoth)
    }

    pub(crate) fn trims_end(self) -> bool {
        matches!(self, Self::TrimEnd | Self::TrimBoth)
    }
}

/// A CSS Inline `<text-edge>` metric keyword.
///
/// CSS Inline Layout Level 3 uses text-edge metrics for `line-fit-edge` and
/// `text-box-edge`:
/// <https://drafts.csswg.org/css-inline-3/#text-edges>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextEdgeMetric {
    Text,
    Cap,
    Ex,
    Ideographic,
    IdeographicInk,
    Alphabetic,
}

impl TextEdgeMetric {
    pub(crate) fn can_resolve_over_edge(self) -> bool {
        matches!(
            self,
            Self::Text | Self::Cap | Self::Ex | Self::Ideographic | Self::IdeographicInk
        )
    }

    pub(crate) fn can_resolve_under_edge(self) -> bool {
        matches!(
            self,
            Self::Text | Self::Ideographic | Self::IdeographicInk | Self::Alphabetic
        )
    }
}

/// Resolved over/under pair for CSS Inline `<text-edge>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextEdgePair {
    pub(crate) over: TextEdgeMetric,
    pub(crate) under: TextEdgeMetric,
}

impl TextEdgePair {
    pub(crate) const TEXT: Self = Self {
        over: TextEdgeMetric::Text,
        under: TextEdgeMetric::Text,
    };

    pub(crate) const fn new(over: TextEdgeMetric, under: TextEdgeMetric) -> Self {
        Self { over, under }
    }
}

/// Computed CSS `line-fit-edge`.
///
/// The initial `leading` value is kept distinct because `text-box-edge: auto`
/// resolves through `line-fit-edge`, with `leading` interpreted as `text` for
/// text-box trimming:
/// <https://drafts.csswg.org/css-inline-3/#line-fit-edge-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineFitEdge {
    Leading,
    Text(TextEdgePair),
}

impl LineFitEdge {
    pub(crate) fn text_box_pair(self) -> TextEdgePair {
        match self {
            Self::Leading => TextEdgePair::TEXT,
            Self::Text(pair) => pair,
        }
    }
}

/// Computed CSS `text-box-edge`.
///
/// CSS Inline Layout Level 3 defines `auto` plus the full `<text-edge>` grammar:
/// <https://drafts.csswg.org/css-inline-3/#text-box-edge>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextBoxEdge {
    Auto,
    Text(TextEdgePair),
}

impl TextBoxEdge {
    pub(crate) fn resolved_pair(self, line_fit_edge: LineFitEdge) -> TextEdgePair {
        match self {
            Self::Auto => line_fit_edge.text_box_pair(),
            Self::Text(pair) => pair,
        }
    }
}

/// Computed CSS `initial-letter`.
///
/// CSS Inline Layout Level 3 defines initial letters as inline-level boxes
/// with special line-spanning layout. The computed value is either `normal` or
/// a requested size paired with an integer sink:
/// <https://drafts.csswg.org/css-inline-3/#initial-letter-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum InitialLetter {
    Normal,
    Specified { size: f32, sink: u32 },
}

impl InitialLetter {
    pub(crate) fn is_normal(self) -> bool {
        matches!(self, Self::Normal)
    }

    pub(crate) fn specified(self) -> Option<(f32, u32)> {
        match self {
            Self::Normal => None,
            Self::Specified { size, sink } => Some((size, sink)),
        }
    }
}

/// Baseline alignment keyword for `initial-letter-align`.
///
/// CSS Inline uses these keywords to choose the over/under alignment points
/// used when sizing and positioning an initial letter:
/// <https://drafts.csswg.org/css-inline-3/#propdef-initial-letter-align>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InitialLetterAlignKeyword {
    Alphabetic,
    Ideographic,
    Hanging,
    Leading,
}

/// Computed CSS `initial-letter-align`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InitialLetterAlign {
    pub(crate) border_box: bool,
    pub(crate) keyword: InitialLetterAlignKeyword,
}

impl InitialLetterAlign {
    pub(crate) const ALPHABETIC: Self = Self {
        border_box: false,
        keyword: InitialLetterAlignKeyword::Alphabetic,
    };
}

/// Computed CSS `initial-letter-wrap`.
///
/// The `first`, `all`, and `grid` values are at-risk in CSS Inline 3 but are
/// modeled so layout can distinguish rectangular wrapping, glyph-contour
/// wrapping, grid expansion, and explicit author offsets:
/// <https://drafts.csswg.org/css-inline-3/#propdef-initial-letter-wrap>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InitialLetterWrap {
    None,
    First,
    All,
    Grid,
    Offset(ComputedLengthPercentage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlignmentBaseline {
    Baseline,
    Metric(BaselineMetric),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BaselineSource {
    Auto,
    First,
    Last,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BaselineShift {
    LengthPercentage(ComputedLengthPercentage),
    Sub,
    Super,
    Top,
    Center,
    Bottom,
}

impl BaselineShift {
    pub(crate) const ZERO: Self = Self::LengthPercentage(ComputedLengthPercentage::ZERO);

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_root_font_metrics())
    }

    /// Resolve `<length-percentage>` against the element's own line-height.
    ///
    /// CSS Inline Layout Level 3 defines percentages on `baseline-shift` as
    /// percentages of the element's own line-height:
    /// <https://drafts.csswg.org/css-inline-3/#baseline-shift-property>.
    pub(crate) fn length_percentage_shift(&self, line_height: LayoutLength) -> LayoutLength {
        match self {
            Self::LengthPercentage(value) => value
                .used_length_with_percentage_basis(PercentageBasis::definite(line_height))
                .unwrap_or(value.fixed_component()),
            _ => layout_pt(0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableCellVerticalAlignKeyword {
    Baseline,
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VerticalAlign {
    pub(crate) dominant_baseline: DominantBaseline,
    pub(crate) alignment_baseline: AlignmentBaseline,
    pub(crate) baseline_source: BaselineSource,
    pub(crate) baseline_shift: BaselineShift,
    pub(crate) table_cell_align: TableCellVerticalAlignKeyword,
}

impl VerticalAlign {
    pub(crate) const BASELINE: Self = Self {
        dominant_baseline: DominantBaseline::Auto,
        alignment_baseline: AlignmentBaseline::Baseline,
        baseline_source: BaselineSource::Auto,
        baseline_shift: BaselineShift::ZERO,
        table_cell_align: TableCellVerticalAlignKeyword::Baseline,
    };

    pub(crate) fn with_alignment_baseline(mut self, alignment_baseline: AlignmentBaseline) -> Self {
        self.alignment_baseline = alignment_baseline;
        self
    }

    pub(crate) fn with_baseline_source(mut self, baseline_source: BaselineSource) -> Self {
        self.baseline_source = baseline_source;
        self
    }

    pub(crate) fn with_baseline_shift(mut self, baseline_shift: BaselineShift) -> Self {
        self.baseline_shift = baseline_shift;
        self
    }

    pub(crate) fn with_table_cell_align(
        mut self,
        table_cell_align: TableCellVerticalAlignKeyword,
    ) -> Self {
        self.table_cell_align = table_cell_align;
        self
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.baseline_shift.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.baseline_shift.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.baseline_shift.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.baseline_shift.requires_root_font_metrics()
    }

    /// Resolve the `baseline-shift` longhand against the element's own line-height.
    ///
    /// Positive values raise the box and negative values lower it.
    pub(crate) fn length_percentage_shift(self, line_height: LayoutLength) -> LayoutLength {
        self.baseline_shift.length_percentage_shift(line_height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BaselineMetric {
    TextBottom,
    Alphabetic,
    Ideographic,
    Middle,
    Central,
    Mathematical,
    Hanging,
    TextTop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DominantBaseline {
    Auto,
    Metric(BaselineMetric),
}

impl ResolveViewportLengths for BaselineShift {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for VerticalAlign {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.baseline_shift.resolve_viewport_lengths(basis);
    }
}
