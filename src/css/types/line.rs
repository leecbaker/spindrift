use super::ComputedLengthPercentage;

/// Computed CSS value for `line-height`.
///
/// CSS 2.2 defines the computed value of `line-height` as `normal`, a number,
/// or a length for length/percentage inputs; CSS Values defines font-metric
/// units such as `ch` from the used font, so Quire keeps those components
/// unresolved until the selected font face is known:
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-line-height>.
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ComputedLineHeight {
    Normal,
    Number(f32),
    Length(ComputedLengthPercentage),
}

impl ComputedLineHeight {
    pub(crate) const NORMAL: Self = Self::Normal;

    pub(crate) fn from_points(points: f32) -> Self {
        Self::Length(ComputedLengthPercentage::from_points(points))
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Self::Length(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn projected(self, font_size: f32) -> (f32, Option<f32>, bool) {
        match self {
            Self::Normal => (font_size * 1.2, Some(1.2), true),
            Self::Number(multiplier) => (font_size * multiplier, Some(multiplier), false),
            Self::Length(length) => (
                length
                    .used_length_with_percentage_basis(font_size)
                    .unwrap_or(
                        length.length_with_percentage_basis(font_size) + length.ch * font_size,
                    ),
                None,
                false,
            ),
        }
    }
}

/// Computed CSS `text-indent` value.
///
/// CSS Text defines `text-indent` as an inherited
/// `<length-percentage> && hanging? && each-line?` value whose percentage is
/// resolved against the containing block's inline size during layout:
/// <https://www.w3.org/TR/css-text-3/#text-indent-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ComputedTextIndent {
    pub(crate) amount: ComputedLengthPercentage,
    pub(crate) hanging: bool,
    pub(crate) each_line: bool,
}

impl ComputedTextIndent {
    pub(crate) const ZERO: Self = Self {
        amount: ComputedLengthPercentage::ZERO,
        hanging: false,
        each_line: false,
    };
}

/// Computed CSS `hanging-punctuation` keyword set.
///
/// CSS Text defines `hanging-punctuation` as an inherited set of keywords
/// controlling whether hangable glyphs are measured inside or outside line
/// edges:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HangingPunctuation {
    pub(crate) first: bool,
    pub(crate) force_end: bool,
    pub(crate) allow_end: bool,
    pub(crate) last: bool,
}

impl HangingPunctuation {
    pub(crate) const NONE: Self = Self {
        first: false,
        force_end: false,
        allow_end: false,
        last: false,
    };
}
