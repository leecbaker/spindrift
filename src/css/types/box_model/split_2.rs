use super::*;

/// Computed CSS `box-decoration-break`.
///
/// CSS Backgrounds and Borders defines how borders, padding, backgrounds, and
/// related box decorations behave when a box is fragmented. CSS Inline reuses
/// the same policy for block-container `text-box-trim` in fragmented flows:
/// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break> and
/// <https://drafts.csswg.org/css-inline-3/#text-box-trim>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxDecorationBreak {
    Slice,
    Clone,
}

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

#[derive(Debug, Clone, Copy, PartialEq)]
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

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
    }

    /// Resolve `<length-percentage>` against the element's own line-height.
    ///
    /// CSS Inline Layout Level 3 defines percentages on `baseline-shift` as
    /// percentages of the element's own line-height:
    /// <https://drafts.csswg.org/css-inline-3/#baseline-shift-property>.
    pub(crate) fn length_percentage_shift(self, line_height: f32) -> f32 {
        match self {
            Self::LengthPercentage(value) => value
                .used_length_with_percentage_basis(line_height)
                .unwrap_or(value.length_with_percentage_basis(line_height)),
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableCellVerticalAlign {
    Baseline,
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerticalAlign {
    pub(crate) dominant_baseline: DominantBaseline,
    pub(crate) alignment_baseline: AlignmentBaseline,
    pub(crate) baseline_source: BaselineSource,
    pub(crate) baseline_shift: BaselineShift,
    pub(crate) table_cell_align: TableCellVerticalAlign,
}

impl VerticalAlign {
    pub(crate) const BASELINE: Self = Self {
        dominant_baseline: DominantBaseline::Auto,
        alignment_baseline: AlignmentBaseline::Baseline,
        baseline_source: BaselineSource::Auto,
        baseline_shift: BaselineShift::ZERO,
        table_cell_align: TableCellVerticalAlign::Baseline,
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
        table_cell_align: TableCellVerticalAlign,
    ) -> Self {
        self.table_cell_align = table_cell_align;
        self
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.baseline_shift.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.baseline_shift.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }

    /// Return whether `baseline-shift` positions the aligned subtree relative
    /// to the resolved line box instead of the parent baseline.
    ///
    /// CSS Inline Layout Level 3 defines `top`, `center`, and `bottom` in
    /// terms of aligning the shifted box with the line box:
    /// <https://drafts.csswg.org/css-inline-3/#baseline-shift-property>.
    pub(crate) fn has_line_relative_baseline_shift(self) -> bool {
        matches!(
            self.baseline_shift,
            BaselineShift::Top | BaselineShift::Center | BaselineShift::Bottom
        )
    }

    /// Resolve the `baseline-shift` longhand against the element's own line-height.
    ///
    /// Positive values raise the box and negative values lower it.
    pub(crate) fn length_percentage_shift(self, line_height: f32) -> f32 {
        self.baseline_shift.length_percentage_shift(line_height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WhiteSpace {
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
}

impl WhiteSpace {
    pub(crate) fn collapses_spaces(self) -> bool {
        matches!(self, Self::Normal | Self::NoWrap | Self::PreLine)
    }

    pub(crate) fn preserves_newlines(self) -> bool {
        matches!(
            self,
            Self::Pre | Self::PreWrap | Self::PreLine | Self::BreakSpaces
        )
    }

    pub(crate) fn allows_soft_wrap(self) -> bool {
        !matches!(self, Self::NoWrap | Self::Pre)
    }

    /// Return whether trailing Unicode space separators hang at line end.
    ///
    /// CSS Text white-space processing makes trailing "other space
    /// separators" hang for `normal`, `nowrap`, and `pre-line`; preserved
    /// modes keep their own edge-space behavior, and `break-spaces` explicitly
    /// prevents this hanging:
    /// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
    pub(crate) fn hangs_trailing_space_separators(self) -> bool {
        matches!(self, Self::Normal | Self::NoWrap | Self::PreLine)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WordBreak {
    Normal,
    BreakAll,
    KeepAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverflowWrap {
    Normal,
    Anywhere,
    BreakWord,
}

/// Computed CSS `overflow` value.
///
/// CSS Overflow defines `overflow` as a shorthand controlling whether content
/// that extends past the padding box is visible, clipped, or scrollable:
/// <https://www.w3.org/TR/css-overflow-3/#propdef-overflow>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Overflow {
    Visible,
    Hidden,
    Clip,
    Scroll,
    Auto,
}

impl Overflow {
    pub(crate) fn clips_overflow(self) -> bool {
        !matches!(self, Self::Visible)
    }

    /// Returns whether this computed overflow value is scrollable.
    ///
    /// CSS Overflow classifies `hidden`, `scroll`, and `auto` as scrollable
    /// overflow values, while `visible` and `clip` are non-scrollable. CSS
    /// Flexbox uses that distinction when resolving automatic minimum sizes
    /// for flex items:
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-properties> and
    /// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
    pub(crate) fn is_scrollable(self) -> bool {
        matches!(self, Self::Hidden | Self::Scroll | Self::Auto)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineBreak {
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hyphens {
    None,
    Manual,
    Auto,
}

/// Computed value of CSS `hyphenate-limit-chars`.
///
/// CSS Text defines this as total word characters, characters before the
/// hyphenation break, and characters after the break. `auto` values are
/// user-agent defined; this renderer uses the CSS Text examples' conventional
/// defaults of 5/2/2:
/// <https://www.w3.org/TR/css-text-4/#hyphenate-limit-chars>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HyphenateLimitChars {
    pub(crate) total: u16,
    pub(crate) before: u16,
    pub(crate) after: u16,
}

impl HyphenateLimitChars {
    pub(crate) const AUTO_TOTAL: u16 = 5;
    pub(crate) const AUTO_BEFORE: u16 = 2;
    pub(crate) const AUTO_AFTER: u16 = 2;

    pub(crate) const AUTO: Self = Self {
        total: Self::AUTO_TOTAL,
        before: Self::AUTO_BEFORE,
        after: Self::AUTO_AFTER,
    };
}
