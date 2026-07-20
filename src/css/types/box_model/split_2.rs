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

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
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
pub(crate) enum TableCellVerticalAlign {
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

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.baseline_shift.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.baseline_shift.requires_ch_advance()
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
    pub(crate) fn length_percentage_shift(self, line_height: LayoutLength) -> LayoutLength {
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

/// Computed CSS Text wrapping mode.
///
/// `white-space` is a legacy shorthand that also sets this component, but
/// `text-wrap-mode` can subsequently override it without changing collapse or
/// segment-break preservation. CSS Text Level 4 defines it as an inherited
/// longhand: <https://drafts.csswg.org/css-text-4/#text-wrap-mode-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextWrapMode {
    /// No CSS `text-wrap-mode` longhand has overridden the legacy shorthand.
    /// This internal state preserves the semantic relationship for styles
    /// assembled directly by layout and UA code.
    Legacy,
    Wrap,
    NoWrap,
}

/// Computed CSS Text wrapping style.
///
/// The style selects among the graph's already-legal soft wrap opportunities;
/// it must never create an opportunity forbidden by `text-wrap-mode` or
/// `white-space`. <https://drafts.csswg.org/css-text-4/#text-wrap-style-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextWrapStyle {
    Auto,
    Balance,
    Stable,
}

/// Controls the preference for soft line breaks within an inline box.
///
/// CSS Text Level 4 makes this a non-inherited property of inline boxes. An
/// `avoid` box retains its ordinary break opportunities, but line selection
/// must prefer an equally fitting opportunity outside that box:
/// <https://drafts.csswg.org/css-text-4/#wrap-inside-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum WrapInside {
    #[default]
    Auto,
    Avoid,
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

    /// Return whether trailing Unicode space separators hang at line end.
    ///
    /// CSS Text white-space processing makes trailing "other space
    /// separators" hang in every legacy white-space mode other than
    /// `break-spaces`. Unlike U+0020, these Unicode separators are not
    /// document white space, so `pre` and `pre-wrap` preservation does not
    /// suppress the Phase II hanging rule:
    /// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
    pub(crate) fn hangs_trailing_space_separators(self) -> bool {
        self != Self::BreakSpaces
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WordBreak {
    Normal,
    BreakAll,
    KeepAll,
    /// CSS Text 4 disables automatic word-boundary detection in complex
    /// (notably Southeast Asian) scripts while retaining manual breaks.
    /// <https://drafts.csswg.org/css-text-4/#word-boundary-detection>
    Manual,
    /// Legacy `word-break: break-word` behaves as `overflow-wrap: anywhere`
    /// for line breaking and intrinsic sizing, without changing the authored
    /// `overflow-wrap` computed value.
    /// <https://drafts.csswg.org/css-text-3/#valdef-word-break-break-word>
    BreakWord,
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

/// Computed CSS Scroll Snap container policy.
///
/// Logical axes stay unresolved until layout maps them through the container's
/// writing mode.
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-snap-type>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScrollSnapType {
    #[default]
    None,
    X(ScrollSnapStrictness),
    Y(ScrollSnapStrictness),
    Block(ScrollSnapStrictness),
    Inline(ScrollSnapStrictness),
    Both(ScrollSnapStrictness),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollSnapStrictness {
    Mandatory,
    Proximity,
}

/// Per-logical-axis alignment contributed by a scroll snap area.
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-snap-align>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ScrollSnapAlign {
    pub(crate) block: ScrollSnapAlignment,
    pub(crate) inline: ScrollSnapAlignment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScrollSnapAlignment {
    #[default]
    None,
    Start,
    End,
    Center,
}

/// Directional scrolling trap policy. Static rendering retains it as a
/// computed value even though no directional operation occurs.
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-snap-stop>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScrollSnapStop {
    #[default]
    Normal,
    Always,
}

/// One computed `scroll-padding-*` edge. `auto` remains distinct until used
/// values are resolved against a concrete scrollport.
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-padding>
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum ScrollPadding {
    #[default]
    Auto,
    LengthPercentage(ComputedLengthPercentage),
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

/// Computed CSS `hyphenate-character` value.
///
/// CSS Text inserts this string only when a selected line ends at a manual or
/// automatic hyphenation opportunity. `auto` intentionally remains distinct
/// from an authored string so a future language/font-specific UA default does
/// not lose that distinction during cascade:
/// <https://drafts.csswg.org/css-text-4/#hyphenate-character>.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum HyphenateCharacter {
    #[default]
    Auto,
    String(String),
}

impl HyphenateCharacter {
    /// Resolve the language-sensitive used string for `hyphenate-character`.
    ///
    /// CSS Text leaves the `auto` glyph to the UA, including its choice for a
    /// particular writing system.  Keep that choice at the line-edge
    /// materialization boundary: only a selected discretionary break inserts
    /// this text, whereas an unselected soft hyphen has no used glyph.
    /// <https://drafts.csswg.org/css-text-4/#hyphenate-character>
    pub(crate) fn used_text_for_language(&self, language: Option<&str>) -> &str {
        match self {
            Self::Auto => match language.map(str::to_ascii_lowercase).as_deref() {
                // Uyghur uses kashida as its conventional discretionary
                // marker. The graph materializer supplies the ZWJ context at
                // a joining-script source boundary.
                Some(language) if language == "ug" || language.starts_with("ug-") => "\u{0640}",
                // Canadian Aboriginal Syllabics uses U+1400 HYPHEN.
                Some(language) if language == "cr" || language.starts_with("cr-") => "\u{1400}",
                _ => "-",
            },
            Self::String(value) => value,
        }
    }
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
