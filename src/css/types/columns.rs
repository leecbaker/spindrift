use super::{
    ComputedLengthPercentage, ResolveViewportLengths, RootFontMetricLengthBasis,
    ViewportLengthBasis,
};
use crate::units::LayoutLength;
use std::num::NonZeroUsize;

/// Computed CSS `column-count`.
/// <https://www.w3.org/TR/css-multicol-1/#cc>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnCount {
    Auto,
    Count(NonZeroUsize),
}

/// Computed CSS Flexbox Level 2 author-requested minimum number of lines in a
/// balanced flex container.
///
/// The initial and computed value are both `1`; unlike column count, this is
/// not an `auto` value. Keeping the CSS lower bound non-zero makes it
/// impossible for later balance planning to manufacture an empty line.
/// <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FlexLineCount(NonZeroUsize);

impl FlexLineCount {
    pub(crate) const ONE: Self = Self(NonZeroUsize::MIN);

    pub(crate) const fn get(self) -> usize {
        self.0.get()
    }

    pub(crate) const fn new(count: NonZeroUsize) -> Self {
        Self(count)
    }
}

/// Computed CSS value for `column-width`.
///
/// CSS Multi-column Layout defines `column-width` as `auto | <length>`, with
/// the actual column count and used column width derived later by layout:
/// <https://www.w3.org/TR/css-multicol-1/#cw>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComputedColumnWidth {
    Auto,
    Length(ComputedLengthPercentage),
}

impl ComputedColumnWidth {
    pub(crate) const AUTO: Self = Self::Auto;

    /// Scale the fixed component of `column-width` at the CSS `zoom`
    /// used-value boundary. `auto` remains an algorithmic value.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://www.w3.org/TR/css-multicol-1/#cw>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        if let Self::Length(length) = self {
            length.scale_fixed_length_components(factor);
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::Length(length) = self {
            length.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::Length(length) = self {
            length.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::Length(length) if length.requires_root_font_metrics())
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Length(length) if length.requires_ch_advance())
    }
}

/// Computed CSS value for `column-height`.
///
/// The used column block size is resolved by multicol layout; the computed
/// value preserves `auto` or the non-negative absolute length.
/// <https://drafts.csswg.org/css-multicol-2/#column-height>
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComputedColumnHeight {
    Auto,
    Length(ComputedLengthPercentage),
}

impl ComputedColumnHeight {
    pub(crate) const AUTO: Self = Self::Auto;

    /// Scale the fixed component of `column-height` at the CSS `zoom`
    /// used-value boundary. `auto` remains an algorithmic value.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://drafts.csswg.org/css-multicol-2/#column-height>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        if let Self::Length(length) = self {
            length.scale_fixed_length_components(factor);
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::Length(length) = self {
            length.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::Length(length) = self {
            length.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::Length(length) if length.requires_root_font_metrics())
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Length(length) if length.requires_ch_advance())
    }
}

/// Computed value of CSS Multi-column Layout Level 2's `column-wrap`.
/// <https://drafts.csswg.org/css-multicol-2/#column-wrap>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnWrap {
    Auto,
    Nowrap,
    Wrap,
}

/// Computed value of CSS Multi-column Layout's `column-fill` property.
///
/// `balance` balances the last column row in a fragmented context, while
/// `balance-all` balances every row. `auto` fills fragmentainers sequentially.
/// <https://www.w3.org/TR/css-multicol-1/#cf>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnFill {
    Balance,
    BalanceAll,
    Auto,
}

/// Computed value of CSS Multi-column Layout's `column-span` property.
/// <https://www.w3.org/TR/css-multicol-1/#column-span>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnSpan {
    None,
    All,
}

impl ResolveViewportLengths for ComputedColumnWidth {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::Length(length) = self {
            length.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for ComputedColumnHeight {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::Length(length) = self {
            length.resolve_viewport_lengths(basis);
        }
    }
}
