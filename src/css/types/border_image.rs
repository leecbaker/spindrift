use super::*;

/// Computed CSS `border-image` longhands.
///
/// CSS Backgrounds and Borders Level 3 defines border images as a source image
/// sliced into a 3x3 grid, then scaled/repeated into the border image area:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-images>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderImage {
    pub source: ComputedImage,
    pub source_base_url: Option<url::Url>,
    pub source_root_url: Option<url::Url>,
    pub slice: BorderImageSlice,
    pub width: BorderImageWidth,
    pub outset: BorderImageOutset,
    pub repeat: BorderImageRepeat,
}

impl BorderImage {
    pub(crate) fn initial() -> Self {
        Self {
            source: ComputedImage::None,
            source_base_url: None,
            source_root_url: None,
            slice: BorderImageSlice::initial(),
            width: BorderImageWidth::initial(),
            outset: BorderImageOutset::initial(),
            repeat: BorderImageRepeat::initial(),
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.width.resolve_font_metric_lengths(ch_advance);
        self.outset.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.width.requires_ch_advance() || self.outset.requires_ch_advance()
    }
}

/// Computed `border-image-slice`.
///
/// Unitless numbers are image pixels, percentages resolve against image
/// dimensions at used-value time, and `fill` preserves the middle slice:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BorderImageSlice {
    pub offsets: BorderImageSliceOffsets,
    pub fill: bool,
}

impl BorderImageSlice {
    pub(crate) fn initial() -> Self {
        Self {
            offsets: BorderImageSliceOffsets {
                top: BorderImageSliceValue::Percent(1.0),
                right: BorderImageSliceValue::Percent(1.0),
                bottom: BorderImageSliceValue::Percent(1.0),
                left: BorderImageSliceValue::Percent(1.0),
            },
            fill: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BorderImageSliceOffsets {
    pub top: BorderImageSliceValue,
    pub right: BorderImageSliceValue,
    pub bottom: BorderImageSliceValue,
    pub left: BorderImageSliceValue,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BorderImageSliceValue {
    Number(f32),
    Percent(f32),
}

/// Computed `border-image-width`.
///
/// Numeric values multiply the corresponding `border-width`; explicit
/// length/percentage values and `auto` are kept for later used-value
/// resolution:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-width>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderImageWidth {
    pub top: BorderImageWidthValue,
    pub right: BorderImageWidthValue,
    pub bottom: BorderImageWidthValue,
    pub left: BorderImageWidthValue,
}

impl BorderImageWidth {
    pub(crate) fn initial() -> Self {
        Self {
            top: BorderImageWidthValue::Number(1.0),
            right: BorderImageWidthValue::Number(1.0),
            bottom: BorderImageWidthValue::Number(1.0),
            left: BorderImageWidthValue::Number(1.0),
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.top.resolve_font_metric_lengths(ch_advance);
        self.right.resolve_font_metric_lengths(ch_advance);
        self.bottom.resolve_font_metric_lengths(ch_advance);
        self.left.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.top.requires_ch_advance()
            || self.right.requires_ch_advance()
            || self.bottom.requires_ch_advance()
            || self.left.requires_ch_advance()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BorderImageWidthValue {
    Auto,
    Number(f32),
    LengthPercentage(ComputedLengthPercentage),
}

impl BorderImageWidthValue {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
    }
}

/// Computed `border-image-outset`.
///
/// Numeric values multiply the corresponding `border-width`; lengths are used
/// directly:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-outset>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderImageOutset {
    pub top: BorderImageOutsetValue,
    pub right: BorderImageOutsetValue,
    pub bottom: BorderImageOutsetValue,
    pub left: BorderImageOutsetValue,
}

impl BorderImageOutset {
    pub(crate) fn initial() -> Self {
        Self {
            top: BorderImageOutsetValue::Length(ComputedLengthPercentage::ZERO),
            right: BorderImageOutsetValue::Length(ComputedLengthPercentage::ZERO),
            bottom: BorderImageOutsetValue::Length(ComputedLengthPercentage::ZERO),
            left: BorderImageOutsetValue::Length(ComputedLengthPercentage::ZERO),
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.top.resolve_font_metric_lengths(ch_advance);
        self.right.resolve_font_metric_lengths(ch_advance);
        self.bottom.resolve_font_metric_lengths(ch_advance);
        self.left.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.top.requires_ch_advance()
            || self.right.requires_ch_advance()
            || self.bottom.requires_ch_advance()
            || self.left.requires_ch_advance()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BorderImageOutsetValue {
    Number(f32),
    Length(ComputedLengthPercentage),
}

impl BorderImageOutsetValue {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::Length(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Length(value) if value.requires_ch_advance())
    }
}

/// Computed `border-image-repeat` value for horizontal and vertical axes.
///
/// CSS accepts one keyword for both axes or two keywords for horizontal then
/// vertical repetition:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-repeat>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BorderImageRepeat {
    pub horizontal: BorderImageRepeatKeyword,
    pub vertical: BorderImageRepeatKeyword,
}

impl BorderImageRepeat {
    pub(crate) fn initial() -> Self {
        Self {
            horizontal: BorderImageRepeatKeyword::Stretch,
            vertical: BorderImageRepeatKeyword::Stretch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BorderImageRepeatKeyword {
    Stretch,
    Repeat,
    Round,
    Space,
}

impl ResolveViewportLengths for BorderImage {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.width.resolve_viewport_lengths(basis);
        self.outset.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for BorderImageWidth {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.top.resolve_viewport_lengths(basis);
        self.right.resolve_viewport_lengths(basis);
        self.bottom.resolve_viewport_lengths(basis);
        self.left.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for BorderImageWidthValue {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for BorderImageOutset {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.top.resolve_viewport_lengths(basis);
        self.right.resolve_viewport_lengths(basis);
        self.bottom.resolve_viewport_lengths(basis);
        self.left.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for BorderImageOutsetValue {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::Length(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}
