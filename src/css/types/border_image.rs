use super::*;

/// Computed CSS `border-image` longhands.
///
/// CSS Backgrounds and Borders Level 3 defines border images as a source image
/// sliced into a 3x3 grid, then scaled/repeated into the border image area:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-images>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderImage {
    pub source: Option<String>,
    pub source_base_url: Option<PathBuf>,
    pub source_root_url: Option<PathBuf>,
    pub slice: BorderImageSlice,
    pub width: BorderImageWidth,
    pub outset: BorderImageOutset,
    pub repeat: BorderImageRepeat,
}

impl BorderImage {
    pub(crate) fn initial() -> Self {
        Self {
            source: None,
            source_base_url: None,
            source_root_url: None,
            slice: BorderImageSlice::initial(),
            width: BorderImageWidth::initial(),
            outset: BorderImageOutset::initial(),
            repeat: BorderImageRepeat::initial(),
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.width.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.width.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
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
#[derive(Debug, Clone, Copy, PartialEq)]
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

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.top.resolve_font_metric_lengths(ch_advance);
        self.right.resolve_font_metric_lengths(ch_advance);
        self.bottom.resolve_font_metric_lengths(ch_advance);
        self.left.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.top.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.right.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.bottom.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.left.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BorderImageWidthValue {
    Auto,
    Number(f32),
    LengthPercentage(ComputedLengthPercentage),
}

impl BorderImageWidthValue {
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
}

/// Computed `border-image-outset`.
///
/// Numeric values multiply the corresponding `border-width`; lengths are used
/// directly:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-outset>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BorderImageOutset {
    pub top: BorderImageOutsetValue,
    pub right: BorderImageOutsetValue,
    pub bottom: BorderImageOutsetValue,
    pub left: BorderImageOutsetValue,
}

impl BorderImageOutset {
    pub(crate) fn initial() -> Self {
        Self {
            top: BorderImageOutsetValue::Length(0.0),
            right: BorderImageOutsetValue::Length(0.0),
            bottom: BorderImageOutsetValue::Length(0.0),
            left: BorderImageOutsetValue::Length(0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BorderImageOutsetValue {
    Number(f32),
    Length(f32),
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
