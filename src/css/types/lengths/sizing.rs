use super::ComputedLengthPercentage;
use crate::css::types::{
    FontRelativeLengthBasis, ResolveViewportLengths, RootFontMetricLengthBasis, ViewportLengthBasis,
};
use crate::units::{LayoutLength, PercentageBasis, SemanticLengthExt, layout_pt};

/// Computed CSS box size value for box-model properties.
///
/// CSS Cascade defines computed values as the result of resolving specified
/// values before layout computes used values:
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
/// CSS Values defines `<length-percentage>`, and CSS Sizing adds intrinsic
/// size keywords and stretch-fit sizing to width/height/min/max-size
/// properties:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages> and
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComputedLengthPercentageOrAuto {
    Auto,
    MinContent,
    MaxContent,
    FitContent(Option<ComputedLengthPercentage>),
    Stretch,
    LengthPercentage(ComputedLengthPercentage),
    CalcSize(CalcSize),
}

/// A computed `calc-size()` value for a box-size property.
///
/// The calculation remains affine in the special `size` keyword until its
/// formatting context establishes the basis. The additive
/// `<length-percentage>` keeps its ordinary deferred resolution:
/// <https://drafts.csswg.org/css-values-5/#calc-size>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CalcSize {
    pub(crate) basis: CalcSizeBasis,
    pub(crate) size_multiplier: f32,
    pub(crate) additive: ComputedLengthPercentage,
    /// The lower bound from a retained `max()` or `clamp()` calculation.
    pub(crate) lower_bound: Option<CalcSizeAffine>,
    /// The upper bound from a retained `min()` or `clamp()` calculation.
    pub(crate) upper_bound: Option<CalcSizeAffine>,
}

/// An affine branch within a `calc-size()` CSS Math comparison.
///
/// CSS Values evaluates `min()`, `max()`, and `clamp()` only after the
/// `calc-size()` basis is available. Keeping both branches affine preserves
/// that deferral without conflating `size` with an ordinary length:
/// <https://drafts.csswg.org/css-values-5/#calc-size> and
/// <https://drafts.csswg.org/css-values-4/#comp-func>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CalcSizeAffine {
    pub(crate) size_multiplier: f32,
    pub(crate) additive: ComputedLengthPercentage,
}

impl CalcSizeAffine {
    fn used_value<T, Source>(
        &self,
        size: f32,
        percentage_basis: PercentageBasis<T, Source>,
    ) -> LayoutLength
    where
        T: SemanticLengthExt,
    {
        layout_pt(
            self.size_multiplier * size
                + self
                    .additive
                    .used_length_with_percentage_basis_points(percentage_basis.points())
                    .unwrap_or_else(|| self.additive.length_points()),
        )
    }

    fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.additive.resolve_font_metric_lengths(ch_advance);
    }

    #[allow(dead_code)]
    fn resolve_font_relative_lengths(&mut self, basis: FontRelativeLengthBasis) {
        self.additive.resolve_font_relative_lengths(basis);
    }

    fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        self.additive.resolve_em_relative_lengths(font_size);
    }

    fn resolve_ic_relative_lengths(&mut self, ic_advance: LayoutLength) {
        self.additive.resolve_ic_relative_lengths(ic_advance);
    }

    fn resolve_ex_relative_lengths(&mut self, x_height: f32) {
        self.additive.resolve_ex_relative_lengths(x_height);
    }

    fn resolve_cap_relative_lengths(&mut self, cap_height: f32) {
        self.additive.resolve_cap_relative_lengths(cap_height);
    }

    fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        self.additive
            .resolve_line_height_relative_lengths(line_height);
    }

    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.additive.resolve_root_font_metric_lengths(basis);
    }

    fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        self.additive
            .resolve_root_font_relative_lengths(root_font_size);
    }

    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.additive.resolve_viewport_lengths(basis);
    }

    fn requires_ch_advance(&self) -> bool {
        self.additive.requires_ch_advance()
    }

    fn requires_selected_font_metrics(&self) -> bool {
        self.additive.requires_selected_font_metrics()
    }

    fn requires_root_font_metrics(&self) -> bool {
        self.additive.requires_root_font_metrics()
    }
}

/// The retained `calc-size()` sizing basis.
///
/// `auto` here is a basis selected by the current formatting context; it is
/// not the ordinary box-size `auto` value:
/// <https://drafts.csswg.org/css-values-5/#calc-size>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CalcSizeBasis {
    Auto,
    MinContent,
    MaxContent,
    FitContent,
    Stretch,
    LengthPercentage(ComputedLengthPercentage),
}

impl CalcSize {
    /// Evaluates the retained calc-size calculation once layout supplies its
    /// selected basis and the relevant percentage basis.
    /// <https://drafts.csswg.org/css-values-5/#calc-size>
    pub(crate) fn used_value<T, Source>(
        &self,
        auto_size: f32,
        min_content: f32,
        max_content: f32,
        fit_content: f32,
        stretch: f32,
        percentage_basis: PercentageBasis<T, Source>,
    ) -> LayoutLength
    where
        T: SemanticLengthExt,
    {
        let percentage_basis = percentage_basis.points();
        let basis = match &self.basis {
            CalcSizeBasis::Auto => auto_size,
            CalcSizeBasis::MinContent => min_content,
            CalcSizeBasis::MaxContent => max_content,
            CalcSizeBasis::FitContent => fit_content,
            CalcSizeBasis::Stretch => stretch,
            CalcSizeBasis::LengthPercentage(value) => value
                .used_length_with_percentage_basis_points(percentage_basis)
                .unwrap_or_else(|| value.length_points()),
        };
        let primary = CalcSizeAffine {
            size_multiplier: self.size_multiplier,
            additive: self.additive.clone(),
        }
        .used_value(
            basis,
            PercentageBasis::definite(layout_pt(percentage_basis.unwrap_or(0.0))),
        );
        let lower_bounded = self
            .lower_bound
            .as_ref()
            .map(|bound| {
                primary.max(bound.used_value(
                    basis,
                    PercentageBasis::definite(layout_pt(percentage_basis.unwrap_or(0.0))),
                ))
            })
            .unwrap_or(primary);
        self.upper_bound
            .as_ref()
            .map(|bound| {
                lower_bounded.min(bound.used_value(
                    basis,
                    PercentageBasis::definite(layout_pt(percentage_basis.unwrap_or(0.0))),
                ))
            })
            .unwrap_or(lower_bounded)
    }

    pub(crate) fn needs_intrinsic_size(&self) -> bool {
        matches!(
            self.basis,
            CalcSizeBasis::Auto
                | CalcSizeBasis::MinContent
                | CalcSizeBasis::MaxContent
                | CalcSizeBasis::FitContent
        )
    }

    fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.additive.resolve_font_metric_lengths(ch_advance);
        if let Some(bound) = &mut self.lower_bound {
            bound.resolve_font_metric_lengths(ch_advance);
        }
        if let Some(bound) = &mut self.upper_bound {
            bound.resolve_font_metric_lengths(ch_advance);
        }
        if let CalcSizeBasis::LengthPercentage(value) = &mut self.basis {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    #[allow(dead_code)]
    fn resolve_font_relative_lengths(&mut self, basis: FontRelativeLengthBasis) {
        self.additive.resolve_font_relative_lengths(basis);
        if let Some(bound) = &mut self.lower_bound {
            bound.resolve_font_relative_lengths(basis);
        }
        if let Some(bound) = &mut self.upper_bound {
            bound.resolve_font_relative_lengths(basis);
        }
        if let CalcSizeBasis::LengthPercentage(value) = &mut self.basis {
            value.resolve_font_relative_lengths(basis);
        }
    }

    fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        self.additive.resolve_em_relative_lengths(font_size);
        if let Some(bound) = &mut self.lower_bound {
            bound.resolve_em_relative_lengths(font_size);
        }
        if let Some(bound) = &mut self.upper_bound {
            bound.resolve_em_relative_lengths(font_size);
        }
        if let CalcSizeBasis::LengthPercentage(value) = &mut self.basis {
            value.resolve_em_relative_lengths(font_size);
        }
    }

    fn resolve_ic_relative_lengths(&mut self, ic_advance: LayoutLength) {
        self.additive.resolve_ic_relative_lengths(ic_advance);
        if let Some(bound) = &mut self.lower_bound {
            bound.resolve_ic_relative_lengths(ic_advance);
        }
        if let Some(bound) = &mut self.upper_bound {
            bound.resolve_ic_relative_lengths(ic_advance);
        }
        if let CalcSizeBasis::LengthPercentage(value) = &mut self.basis {
            value.resolve_ic_relative_lengths(ic_advance);
        }
    }

    fn resolve_ex_relative_lengths(&mut self, x_height: f32) {
        self.additive.resolve_ex_relative_lengths(x_height);
        if let Some(bound) = &mut self.lower_bound {
            bound.resolve_ex_relative_lengths(x_height);
        }
        if let Some(bound) = &mut self.upper_bound {
            bound.resolve_ex_relative_lengths(x_height);
        }
        if let CalcSizeBasis::LengthPercentage(value) = &mut self.basis {
            value.resolve_ex_relative_lengths(x_height);
        }
    }

    fn resolve_cap_relative_lengths(&mut self, cap_height: f32) {
        self.additive.resolve_cap_relative_lengths(cap_height);
        if let Some(bound) = &mut self.lower_bound {
            bound.resolve_cap_relative_lengths(cap_height);
        }
        if let Some(bound) = &mut self.upper_bound {
            bound.resolve_cap_relative_lengths(cap_height);
        }
        if let CalcSizeBasis::LengthPercentage(value) = &mut self.basis {
            value.resolve_cap_relative_lengths(cap_height);
        }
    }

    fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        self.additive
            .resolve_line_height_relative_lengths(line_height);
        if let Some(bound) = &mut self.lower_bound {
            bound.resolve_line_height_relative_lengths(line_height);
        }
        if let Some(bound) = &mut self.upper_bound {
            bound.resolve_line_height_relative_lengths(line_height);
        }
        if let CalcSizeBasis::LengthPercentage(value) = &mut self.basis {
            value.resolve_line_height_relative_lengths(line_height);
        }
    }

    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.additive.resolve_root_font_metric_lengths(basis);
        if let Some(bound) = &mut self.lower_bound {
            bound.resolve_root_font_metric_lengths(basis);
        }
        if let Some(bound) = &mut self.upper_bound {
            bound.resolve_root_font_metric_lengths(basis);
        }
        if let CalcSizeBasis::LengthPercentage(value) = &mut self.basis {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        self.additive
            .resolve_root_font_relative_lengths(root_font_size);
        if let Some(bound) = &mut self.lower_bound {
            bound.resolve_root_font_relative_lengths(root_font_size);
        }
        if let Some(bound) = &mut self.upper_bound {
            bound.resolve_root_font_relative_lengths(root_font_size);
        }
        if let CalcSizeBasis::LengthPercentage(value) = &mut self.basis {
            value.resolve_root_font_relative_lengths(root_font_size);
        }
    }

    fn requires_ch_advance(&self) -> bool {
        self.additive.requires_ch_advance()
            || self
                .lower_bound
                .as_ref()
                .is_some_and(CalcSizeAffine::requires_ch_advance)
            || self
                .upper_bound
                .as_ref()
                .is_some_and(CalcSizeAffine::requires_ch_advance)
            || matches!(&self.basis, CalcSizeBasis::LengthPercentage(value) if value.requires_ch_advance())
    }

    fn requires_selected_font_metrics(&self) -> bool {
        self.additive.requires_selected_font_metrics()
            || self
                .lower_bound
                .as_ref()
                .is_some_and(CalcSizeAffine::requires_selected_font_metrics)
            || self
                .upper_bound
                .as_ref()
                .is_some_and(CalcSizeAffine::requires_selected_font_metrics)
            || matches!(&self.basis, CalcSizeBasis::LengthPercentage(value) if value.requires_selected_font_metrics())
    }

    fn requires_root_font_metrics(&self) -> bool {
        self.additive.requires_root_font_metrics()
            || self
                .lower_bound
                .as_ref()
                .is_some_and(CalcSizeAffine::requires_root_font_metrics)
            || self
                .upper_bound
                .as_ref()
                .is_some_and(CalcSizeAffine::requires_root_font_metrics)
            || matches!(&self.basis, CalcSizeBasis::LengthPercentage(value) if value.requires_root_font_metrics())
    }
}

impl ComputedLengthPercentageOrAuto {
    pub(crate) const AUTO: Self = Self::Auto;
    pub(crate) const ZERO: Self = Self::LengthPercentage(ComputedLengthPercentage::ZERO);

    /// Scale the fixed-length portions while retaining `auto`, intrinsic
    /// keywords, and percentage coefficients.
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.scale_fixed_length_components(factor);
            }
            Self::CalcSize(value) => {
                value.additive.scale_fixed_length_components(factor);
                if let CalcSizeBasis::LengthPercentage(basis) = &mut value.basis {
                    basis.scale_fixed_length_components(factor);
                }
                for bound in [&mut value.lower_bound, &mut value.upper_bound]
                    .into_iter()
                    .flatten()
                {
                    bound.additive.scale_fixed_length_components(factor);
                }
            }
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    pub(crate) fn length_if_no_percent(&self) -> Option<f32> {
        match self {
            Self::LengthPercentage(value) => value.length_if_no_percent(),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(_)
            | Self::Stretch
            | Self::CalcSize(_) => None,
        }
    }

    pub(crate) fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }

    /// Returns a calc-size value whose retained basis is the automatic size.
    ///
    /// Aspect-ratio transfer must first establish that automatic size, then
    /// apply the calc-size calculation to it:
    /// <https://drafts.csswg.org/css-values-5/#calc-size> and
    /// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
    pub(crate) fn calc_size_with_auto_basis(&self) -> Option<CalcSize> {
        match self {
            Self::CalcSize(value) if matches!(value.basis, CalcSizeBasis::Auto) => {
                Some(value.clone())
            }
            _ => None,
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_font_metric_lengths(ch_advance);
            }
            Self::CalcSize(value) => value.resolve_font_metric_lengths(ch_advance),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    #[allow(dead_code)]
    pub(crate) fn resolve_font_relative_lengths(&mut self, basis: FontRelativeLengthBasis) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_font_relative_lengths(basis);
            }
            Self::CalcSize(value) => value.resolve_font_relative_lengths(basis),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_em_relative_lengths(font_size);
            }
            Self::CalcSize(value) => value.resolve_em_relative_lengths(font_size),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    pub(crate) fn resolve_ic_relative_lengths(&mut self, ic_advance: LayoutLength) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_ic_relative_lengths(ic_advance);
            }
            Self::CalcSize(value) => value.resolve_ic_relative_lengths(ic_advance),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    pub(crate) fn resolve_ex_relative_lengths(&mut self, x_height: f32) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_ex_relative_lengths(x_height);
            }
            Self::CalcSize(value) => value.resolve_ex_relative_lengths(x_height),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    pub(crate) fn resolve_cap_relative_lengths(&mut self, cap_height: f32) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_cap_relative_lengths(cap_height);
            }
            Self::CalcSize(value) => value.resolve_cap_relative_lengths(cap_height),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_line_height_relative_lengths(line_height);
            }
            Self::CalcSize(value) => value.resolve_line_height_relative_lengths(line_height),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_root_font_metric_lengths(basis);
            }
            Self::CalcSize(value) => value.resolve_root_font_metric_lengths(basis),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    /// Resolves deferred root-font-relative components once the root used
    /// font size is available.
    /// <https://www.w3.org/TR/css-values-4/#rem>
    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_root_font_relative_lengths(root_font_size);
            }
            Self::CalcSize(value) => value.resolve_root_font_relative_lengths(root_font_size),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.requires_ch_advance()
            }
            Self::CalcSize(value) => value.requires_ch_advance(),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => false,
        }
    }

    /// Whether this box-size value needs a metric from its selected font.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.requires_selected_font_metrics()
            }
            Self::CalcSize(value) => value.requires_selected_font_metrics(),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => false,
        }
    }

    /// Whether this box-size value needs a metric from the document root's
    /// selected font.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.requires_root_font_metrics()
            }
            Self::CalcSize(value) => value.requires_root_font_metrics(),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => false,
        }
    }
}

impl ResolveViewportLengths for CalcSize {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.additive.resolve_viewport_lengths(basis);
        if let Some(bound) = &mut self.lower_bound {
            bound.resolve_viewport_lengths(basis);
        }
        if let Some(bound) = &mut self.upper_bound {
            bound.resolve_viewport_lengths(basis);
        }
        if let CalcSizeBasis::LengthPercentage(value) = &mut self.basis {
            value.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for ComputedLengthPercentageOrAuto {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_viewport_lengths(basis);
            }
            Self::CalcSize(value) => value.resolve_viewport_lengths(basis),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }
}
