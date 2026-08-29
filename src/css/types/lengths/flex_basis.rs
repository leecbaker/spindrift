use super::ComputedLengthPercentage;
use crate::css::types::{ResolveViewportLengths, RootFontMetricLengthBasis, ViewportLengthBasis};
use crate::units::LayoutLength;

/// Computed `flex-basis` value.
///
/// CSS Flexbox defines `flex-basis` as `content | <width>`, where `<width>`
/// includes intrinsic sizing keywords, `<length-percentage>`, and `auto`. The
/// `content` keyword is not a generic box-size value: it forces content-based
/// flex base sizing instead of retrieving the main-size property like `auto`:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property> and
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComputedFlexBasis {
    Auto,
    Content,
    MinContent,
    MaxContent,
    FitContent(Option<ComputedLengthPercentage>),
    LengthPercentage(ComputedFlexBasisLength),
}

/// Computed `<length-percentage>` used by `flex-basis`.
///
/// CSS Flexbox resolves percentages in `flex-basis` against the flex
/// container's inner main size, and falls back to `content` when that size is
/// indefinite. Percentage presence, including authored `0%`, belongs to the
/// unified `<length-percentage>` representation:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedFlexBasisLength {
    pub value: ComputedLengthPercentage,
}

impl ComputedFlexBasisLength {
    pub(crate) fn new(value: ComputedLengthPercentage) -> Self {
        Self { value }
    }

    pub(crate) fn contains_percentage(&self) -> bool {
        self.value.contains_percentage()
    }
}

impl ComputedFlexBasis {
    pub(crate) const AUTO: Self = Self::Auto;

    /// Scale the fixed components of a flex basis at the CSS `zoom`
    /// used-value boundary.
    ///
    /// Percentages intentionally remain unscaled: CSS Flexbox resolves them
    /// against the flex container's already zoomed inner main size.  Intrinsic
    /// keywords and `auto` likewise remain algorithmic values.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://drafts.csswg.org/css-flexbox-1/#flex-basis-property>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        match self {
            Self::FitContent(Some(value)) => value.scale_fixed_length_components(factor),
            Self::LengthPercentage(value) => value.value.scale_fixed_length_components(factor),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        match self {
            Self::FitContent(Some(value)) => {
                value.resolve_font_metric_lengths(ch_advance);
            }
            Self::LengthPercentage(value) => value.value.resolve_font_metric_lengths(ch_advance),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::FitContent(Some(value)) => value.resolve_root_font_metric_lengths(basis),
            Self::LengthPercentage(value) => value.value.resolve_root_font_metric_lengths(basis),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        match self {
            Self::FitContent(Some(value)) => value.requires_ch_advance(),
            Self::LengthPercentage(value) => value.value.requires_ch_advance(),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => false,
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::FitContent(Some(value)) => value.requires_root_font_metrics(),
            Self::LengthPercentage(value) => value.value.requires_root_font_metrics(),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => false,
        }
    }
}

impl ResolveViewportLengths for ComputedFlexBasis {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        match self {
            Self::FitContent(Some(value)) => {
                value.resolve_viewport_lengths(basis);
            }
            Self::LengthPercentage(value) => value.value.resolve_viewport_lengths(basis),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
        }
    }
}
