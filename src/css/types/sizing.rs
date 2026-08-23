/// Computed value of CSS Sizing `aspect-ratio`.
///
/// CSS Sizing Level 4 defines `aspect-ratio` as `auto || <ratio>`, where the
/// ratio is width divided by height:
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
/// A finite, strictly positive CSS aspect ratio.
///
/// The parser validates both ratio components before division; this wrapper
/// keeps manually constructed computed styles from reintroducing a zero, NaN,
/// or infinite ratio after that boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssRatio(f32);

impl CssRatio {
    pub(crate) fn new(value: f32) -> Option<Self> {
        (value.is_finite() && value > 0.0).then_some(Self(value))
    }

    pub(crate) const fn value(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AspectRatio {
    Auto,
    Ratio(CssRatio),
    AutoWithFallback(CssRatio),
}

impl AspectRatio {
    pub(crate) const AUTO: Self = Self::Auto;

    pub(crate) fn from_ratio(ratio: f32) -> Option<Self> {
        CssRatio::new(ratio).map(Self::Ratio)
    }

    pub(crate) fn auto_with_ratio(ratio: f32) -> Option<Self> {
        CssRatio::new(ratio).map(Self::AutoWithFallback)
    }

    #[cfg(test)]
    pub(crate) const fn specified(self) -> (bool, Option<f32>) {
        match self {
            Self::Auto => (true, None),
            Self::Ratio(ratio) => (false, Some(ratio.value())),
            Self::AutoWithFallback(ratio) => (true, Some(ratio.value())),
        }
    }

    /// Returns the authored preferred ratio for non-replaced boxes.
    ///
    /// CSS Sizing Level 4 gives `auto && <ratio>` special replaced-element
    /// fallback behavior; non-replaced boxes use the authored ratio as their
    /// preferred aspect ratio:
    /// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
    pub(crate) fn preferred_ratio_for_non_replaced(self, is_replaced: bool) -> Option<f32> {
        match self {
            Self::Auto if is_replaced => None,
            Self::Auto => None,
            Self::Ratio(ratio) | Self::AutoWithFallback(ratio) => Some(ratio.value()),
        }
    }

    /// Whether a non-replaced preferred ratio operates on content-box sizes.
    ///
    /// `auto && <ratio>` uses the specified ratio for a non-replaced box, but
    /// CSS Sizing defines its calculations in the content box. A bare ratio,
    /// by contrast, uses the box selected by `box-sizing`.
    /// <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
    pub(crate) const fn uses_content_box_for_non_replaced(self) -> bool {
        matches!(self, Self::AutoWithFallback(_))
    }

    /// Returns the preferred ratio after resolving replaced-element fallback.
    ///
    /// CSS Sizing Level 4 defines `aspect-ratio:auto` on replaced elements as
    /// using the natural aspect ratio, a bare `<ratio>` as overriding that
    /// ratio, and `auto && <ratio>` as falling back to the natural ratio when
    /// one exists:
    /// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
    pub(crate) fn preferred_ratio(
        self,
        is_replaced: bool,
        natural_ratio: Option<f32>,
    ) -> Option<f32> {
        let natural_ratio = natural_ratio.filter(|ratio| ratio.is_finite() && *ratio > 0.0);
        match self {
            Self::Auto => is_replaced.then_some(natural_ratio).flatten(),
            Self::Ratio(ratio) => Some(ratio.value()),
            Self::AutoWithFallback(ratio) if is_replaced => natural_ratio.or(Some(ratio.value())),
            Self::AutoWithFallback(ratio) => Some(ratio.value()),
        }
    }
}
