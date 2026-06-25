use super::ComputedLengthPercentage;

/// Computed CSS value for `line-height`.
///
/// CSS 2.2 defines the computed value of `line-height` as `normal`, a number,
/// or an absolute length for length/percentage inputs; layout later turns that
/// computed value into a used line box height:
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-line-height>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ComputedLineHeight {
    Normal,
    Number(f32),
    Length(f32),
}

impl ComputedLineHeight {
    pub(crate) const NORMAL: Self = Self::Normal;
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
