use super::*;

pub(crate) const ROOT_FONT_SIZE_PT: f32 = 12.0;

/// Largest coordinate representable in Quire's default PDF user space.
///
/// PDF limits default user-space coordinates to 200 inches. Quire does not
/// currently emit `/UserUnit`, so preserving larger CSS values would create
/// invalid PDF geometry and can turn one box into billions of fragmentainers.
/// Clamp at the CSS used-value boundary instead.
/// <https://www.w3.org/TR/css-values-4/#numeric-ranges>
pub(crate) const MAX_USED_LAYOUT_LENGTH_PT: f32 = 14_400.0;

pub(crate) fn clamp_used_layout_coordinate(value: LayoutLength) -> LayoutLength {
    if value.points().is_nan() {
        layout_pt(0.0)
    } else {
        layout_pt(
            value
                .points()
                .clamp(-MAX_USED_LAYOUT_LENGTH_PT, MAX_USED_LAYOUT_LENGTH_PT),
        )
    }
}

pub(crate) fn clamp_used_layout_length(value: LayoutLength) -> LayoutLength {
    layout_pt(clamp_used_layout_coordinate(value).points().max(0.0))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AbsoluteLengthUnit {
    Px,
    Pt,
    In,
    Cm,
    Mm,
    Q,
    Pc,
    NumberPt,
}

impl AbsoluteLengthUnit {
    pub(crate) fn length_for_value(self, value: f32) -> LayoutLength {
        let points_per_unit = match self {
            Self::Px => CSS_PX_TO_PT,
            Self::Pt | Self::NumberPt => 1.0,
            Self::In => 72.0,
            Self::Cm => 72.0 / 2.54,
            Self::Mm => 72.0 / 25.4,
            Self::Q => 72.0 / 25.4 / 4.0,
            Self::Pc => 12.0,
        };
        layout_pt(value * points_per_unit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SpecifiedLength {
    Absolute {
        value: f32,
        unit: AbsoluteLengthUnit,
    },
    FontRelativeEm(f32),
    FontRelativeCh(f32),
    FontRelativeEx(f32),
    FontRelativeCap(f32),
    FontRelativeIc(f32),
    FontRelativeLh(f32),
    RootFontRelativeRem(f32),
    RootFontRelativeRex(f32),
    RootFontRelativeRcap(f32),
    RootFontRelativeRch(f32),
    RootFontRelativeRic(f32),
    RootFontRelativeRlh(f32),
}

impl SpecifiedLength {
    // CSS Cascade 5 value processing resolves font-relative specified lengths
    // when computed values are produced; layout later turns computed
    // length-percentages into used values against a containing block.
    // https://www.w3.org/TR/css-cascade-5/#computed
    // https://www.w3.org/TR/css-values-4/#font-relative-lengths
    pub(crate) fn to_computed(self, font_size: f32, root_font_size: f32) -> ComputedLength {
        let length = match self {
            Self::Absolute { value, unit } => unit.length_for_value(value),
            Self::FontRelativeEm(value) => layout_pt(value * font_size),
            Self::FontRelativeCh(value) => layout_pt(value * font_size),
            Self::FontRelativeEx(value) => layout_pt(value * font_size * 0.5),
            Self::FontRelativeCap(value) => layout_pt(value * font_size * 0.7),
            Self::FontRelativeIc(value) => layout_pt(value * font_size),
            Self::FontRelativeLh(value) => layout_pt(value * font_size * 1.2),
            Self::RootFontRelativeRem(value) => layout_pt(value * root_font_size),
            Self::RootFontRelativeRex(value) => layout_pt(value * root_font_size * 0.5),
            Self::RootFontRelativeRcap(value) => layout_pt(value * root_font_size * 0.7),
            Self::RootFontRelativeRch(value) | Self::RootFontRelativeRic(value) => {
                layout_pt(value * root_font_size)
            }
            Self::RootFontRelativeRlh(value) => layout_pt(value * root_font_size * 1.2),
        };
        ComputedLength { length }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ComputedLength {
    pub length: LayoutLength,
}

/// A `font-size` whose CSS font-relative components have not yet received
/// their parent font's used metrics.
///
/// Unlike ordinary `em` and `ch` lengths, the font-relative units in
/// `font-size` are relative to the parent element's font. Keeping that basis
/// explicit prevents the structural style phase from selecting a font merely
/// to cascade a descendant:
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
/// <https://www.w3.org/TR/css-fonts-4/#font-size-prop>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DeferredFontSize {
    Absolute(f32),
    /// An inherited computed size. Its numeric value is deliberately not
    /// copied from the pre-font structural phase: it must become the
    /// immediate parent's resolved used size.
    Inherit,
    /// A font size relative to the parent element's computed line height.
    /// CSS Values defines `lh` in `font-size` against the parent, rather than
    /// against the element whose font size is being computed.
    /// <https://www.w3.org/TR/css-values-4/#lh>
    ParentLineHeight(f32),
    RelativeToParent(ComputedLengthPercentage),
}

impl DeferredFontSize {
    pub(crate) const INITIAL: Self = Self::Absolute(ROOT_FONT_SIZE_PT);

    /// Resolves this value against the already-used parent font size and its
    /// selected-font `ch` advance.
    pub(crate) fn resolve(&self, parent: FontRelativeLengthBasis) -> LayoutLength {
        self.resolve_with_viewport(parent, None)
    }

    /// Resolves this value after the initial containing block's viewport is
    /// known. Viewport units in `font-size` are computed against that viewport
    /// before descendants use their inherited font metrics.
    /// <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
    pub(crate) fn resolve_with_viewport(
        &self,
        parent: FontRelativeLengthBasis,
        viewport: Option<ViewportLengthBasis>,
    ) -> LayoutLength {
        self.resolve_with_viewport_and_root_metrics(parent, viewport, None)
    }

    /// Resolve a deferred `font-size`, using the document root's selected
    /// metrics when root-relative metric units occur on a descendant.
    ///
    /// Root-relative font units are not fixed ratios of the root font size:
    /// their basis is the used root font. The root snapshot is only available
    /// once structural font-metric resolution has selected that face.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn resolve_with_viewport_and_root_metrics(
        &self,
        parent: FontRelativeLengthBasis,
        viewport: Option<ViewportLengthBasis>,
        root_metrics: Option<RootFontMetricLengthBasis>,
    ) -> LayoutLength {
        let parent_font_size = parent.font_size().points();
        let parent_ch_advance = parent.ch_advance();
        layout_pt(match self {
            Self::Absolute(value) => *value,
            Self::Inherit => parent_font_size,
            // Cascading converts this form to `Absolute` while the inherited
            // line height is available. Retain a safe fallback for callers
            // that parse a font size outside that cascade path.
            Self::ParentLineHeight(multiplier) => parent_font_size * *multiplier,
            Self::RelativeToParent(value) => {
                let mut value = value.clone();
                if let Some(basis) = viewport {
                    value.resolve_viewport_lengths(basis);
                }
                value.resolve_font_relative_lengths(parent);
                value.resolve_font_metric_lengths(parent_ch_advance);
                value.resolve_ic_relative_lengths(parent.ic_advance());
                value.resolve_ex_relative_lengths(parent.x_height().points());
                value.resolve_cap_relative_lengths(parent.cap_height().points());
                if let Some(root_metrics) = root_metrics {
                    value.resolve_root_font_relative_lengths(root_metrics.font_size.points());
                    value.resolve_root_font_metric_lengths(root_metrics);
                } else {
                    // Resolving the root style itself cannot use a root
                    // snapshot yet. Preserve the CSS initial-metric fallback
                    // for that bootstrap case.
                    value.resolve_root_font_relative_lengths(ROOT_FONT_SIZE_PT);
                }
                // `font-size` resolves its font-relative units against the
                // parent selected font. `FontRelativeLengthBasis` retains the
                // CSS metric fallbacks for cascade-time callers that have not
                // selected a font yet:
                // <https://www.w3.org/TR/css-values-4/#ex>.
                value
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        parent_font_size,
                    )))
                    .map(layout_points)
                    .unwrap_or(parent_font_size)
            }
        })
    }

    /// Returns whether resolving this `font-size` requires the parent's used
    /// `ch` advance.
    ///
    /// CSS Fonts resolves font-relative units in `font-size` against the
    /// parent font. Deferred math is inspected structurally so a `ch` term
    /// hidden by the currently selected `min()`, `max()`, or `clamp()` branch
    /// still receives its required metric:
    /// <https://www.w3.org/TR/css-fonts-4/#font-size-prop> and
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn requires_parent_ch_advance(&self, _parent_font_size: f32) -> bool {
        match self {
            Self::Absolute(_) | Self::Inherit | Self::ParentLineHeight(_) => false,
            Self::RelativeToParent(value) => value.requires_ch_advance(),
        }
    }

    /// Whether this `font-size` must receive the document-root metric
    /// snapshot after cascade has produced its provisional value.
    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::Absolute(_) | Self::Inherit | Self::ParentLineHeight(_) => false,
            Self::RelativeToParent(value) => value.requires_root_font_metrics(),
        }
    }

    /// Whether this `font-size` needs the parent selected font's metrics.
    pub(crate) fn requires_parent_selected_font_metrics(&self) -> bool {
        matches!(self, Self::RelativeToParent(value) if value.requires_parent_selected_font_metrics())
    }
}

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
