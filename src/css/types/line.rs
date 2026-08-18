use super::{
    ComputedLengthPercentage, PercentageBasis, RootFontMetricLengthBasis, layout_points, layout_pt,
};
use crate::units::LayoutLength;

/// Computed CSS value for `line-height`.
///
/// CSS 2.2 defines the computed value of `line-height` as `normal`, a number,
/// or a length for length/percentage inputs; CSS Values defines font-metric
/// units such as `ch` from the used font, so Quire keeps those components
/// unresolved until the selected font face is known:
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-line-height>.
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
#[derive(Debug, Clone, PartialEq)]
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

    /// Scale an explicit length while preserving unitless and `normal`
    /// line-heights, whose used values follow the zoomed font size.
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        if let Self::Length(value) = self {
            value.scale_fixed_length_components(factor);
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::Length(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: super::SelectedFontMetricLengthBasis,
    ) {
        if let Self::Length(value) = self {
            value.resolve_selected_font_metric_lengths(basis);
        }
    }

    /// Resolves `em` components after the element's used font size is known.
    /// <https://www.w3.org/TR/css-values-4/#em>
    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        if let Self::Length(value) = self {
            value.resolve_em_relative_lengths(font_size);
        }
    }

    /// Resolve `lh` in `line-height` against the inherited computed line
    /// height. CSS Values makes this an exception to the normal local `lh`
    /// basis, preventing a declaration from depending on the value it sets.
    /// <https://drafts.csswg.org/css-values-4/#lh>
    pub(crate) fn resolve_inherited_line_height_relative_lengths(
        &mut self,
        inherited_line_height: LayoutLength,
    ) {
        if let Self::Length(value) = self {
            value.resolve_line_height_relative_lengths(inherited_line_height);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Length(value) if value.requires_ch_advance())
    }

    /// Whether this line-height needs a metric from its selected font.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        matches!(self, Self::Length(value) if value.requires_selected_font_metrics())
    }

    /// Whether this line-height needs a metric from the document root's
    /// selected font.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::Length(value) if value.requires_root_font_metrics())
    }

    /// Resolves root-font metric units against the document root's selected
    /// font and computed line height.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::Length(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn projected(self, font_size: f32) -> (f32, Option<f32>, bool) {
        match self {
            Self::Normal => (font_size * 1.2, Some(1.2), true),
            Self::Number(multiplier) => (font_size * multiplier, Some(multiplier), false),
            Self::Length(length) => {
                let mut fallback = length;
                // Until the selected font is available, retain the historic
                // computed-value fallback for `ch` in line-height. Layout
                // replaces it with the selected-font advance later.
                fallback.resolve_font_metric_lengths(layout_pt(font_size));
                (
                    fallback
                        .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                            font_size,
                        )))
                        .map(layout_points)
                        .unwrap_or(fallback.length_points()),
                    None,
                    false,
                )
            }
        }
    }
}

/// Computed CSS `text-indent` value.
///
/// CSS Text defines `text-indent` as an inherited
/// `<length-percentage> && hanging? && each-line?` value whose percentage is
/// resolved against the containing block's inline size during layout:
/// <https://www.w3.org/TR/css-text-3/#text-indent-property>.
#[derive(Debug, Clone, PartialEq)]
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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.amount.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.amount.requires_root_font_metrics()
    }
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
