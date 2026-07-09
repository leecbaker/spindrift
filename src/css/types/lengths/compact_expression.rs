use super::*;
use std::cmp::Ordering;
use std::rc::Rc;

/// The affine subset of CSS `<length-percentage>`.
///
/// This is the common representation: an absolute component plus a percentage
/// coefficient. `has_percentage` preserves the specified-value distinction
/// between `0` and authored `0%` where percentage presence affects layout.
/// `percentage_requires_basis` additionally distinguishes authored zero
/// percentages from a percentage component that CSS math has neutralized
/// (for example, `0% * 0.5` during animation interpolation).
///
/// CSS Values and Units Level 4, <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AffineLengthPercentage {
    length: LayoutLength,
    percentage: f32,
    has_percentage: bool,
    percentage_requires_basis: bool,
}

impl AffineLengthPercentage {
    pub(crate) const ZERO: Self = Self {
        length: layout_pt(0.0),
        percentage: 0.0,
        has_percentage: false,
        percentage_requires_basis: false,
    };

    const fn length(length: LayoutLength) -> Self {
        Self {
            length,
            percentage: 0.0,
            has_percentage: false,
            percentage_requires_basis: false,
        }
    }

    const fn percentage(percentage: f32) -> Self {
        Self {
            length: layout_pt(0.0),
            percentage,
            has_percentage: true,
            percentage_requires_basis: true,
        }
    }

    fn sum(self, other: Self) -> Self {
        Self {
            length: self.length + other.length,
            percentage: self.percentage + other.percentage,
            has_percentage: self.has_percentage || other.has_percentage,
            percentage_requires_basis: self.percentage_requires_basis
                || other.percentage_requires_basis,
        }
    }

    fn product(self, factor: f32) -> Self {
        Self {
            length: self.length * factor,
            percentage: self.percentage * factor,
            has_percentage: self.has_percentage,
            percentage_requires_basis: factor != 0.0
                && self.percentage != 0.0
                && self.percentage_requires_basis,
        }
    }

    fn negated(self) -> Self {
        Self {
            length: -self.length,
            percentage: -self.percentage,
            has_percentage: self.has_percentage,
            // Unary negation preserves whether an authored percentage needs
            // a basis. This differs from multiplying by a CSS number, where
            // a zero percentage coefficient is mathematically eliminated.
            percentage_requires_basis: self.percentage_requires_basis,
        }
    }

    fn used(self, percentage_basis: Option<f32>) -> Option<LayoutLength> {
        if self.percentage == 0.0 && !self.percentage_requires_basis {
            Some(self.length)
        } else {
            percentage_basis.map(|basis| self.length + layout_pt(self.percentage * basis))
        }
    }
}

/// A unit whose basis is intentionally deferred to one of CSS's existing
/// computed/used-value resolution phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DeferredLengthUnit {
    Em,
    Rem,
    Ch,
    Ex,
    Cap,
    Ic,
    Lh,
    Rex,
    Rcap,
    Rch,
    Ric,
    Rlh,
    Vw,
    Vh,
    Vmin,
    Vmax,
    Vi,
    Vb,
    Cqw,
    Cqh,
    Cqi,
    Cqb,
    Cqmin,
    Cqmax,
}

/// Immutable semantic CSS Math expression. It contains no parser tokens and
/// has no global lifetime; shared computed styles own its `Rc` directly.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LengthPercentageExpression {
    Affine(AffineLengthPercentage),
    Term {
        unit: DeferredLengthUnit,
        coefficient: f32,
    },
    Sum(Rc<Self>, Rc<Self>),
    Product(Rc<Self>, f32),
    Min(Rc<Self>, Rc<Self>),
    Max(Rc<Self>, Rc<Self>),
    Clamp {
        min: Rc<Self>,
        center: Rc<Self>,
        max: Rc<Self>,
    },
}

/// Compact inline-or-`Rc` computed `<length-percentage>` payload.
///
/// The affine representation covers absolute lengths, percentages, and their
/// sums without allocation. A tree is retained only for unresolved unit terms
/// or comparisons whose branch must remain deferred.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComputedLengthPercentage {
    Affine(AffineLengthPercentage),
    Expression(Rc<LengthPercentageExpression>),
}

impl ComputedLengthPercentage {
    pub(crate) const ZERO: Self = Self::Affine(AffineLengthPercentage::ZERO);

    pub(crate) const fn from_layout_length(length: LayoutLength) -> Self {
        Self::Affine(AffineLengthPercentage::length(length))
    }

    pub(crate) fn from_points(points: f32) -> Self {
        Self::from_layout_length(layout_pt(points))
    }

    /// Scale only the absolute-length part of this computed value.
    ///
    /// CSS `zoom` leaves percentage coefficients intact: they resolve against
    /// the zoomed containing block at used-value time.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        *self = match self {
            Self::Affine(value) => Self::Affine(AffineLengthPercentage {
                length: value.length * factor,
                percentage: value.percentage,
                has_percentage: value.has_percentage,
                percentage_requires_basis: value.percentage_requires_basis,
            }),
            Self::Expression(expression) => Self::Expression(Rc::new(
                scale_expression_fixed_length_components(expression, factor),
            )),
        };
    }

    pub(crate) const fn from_percent(percentage: f32) -> Self {
        Self::Affine(AffineLengthPercentage::percentage(percentage))
    }

    /// Build the compact affine form while retaining authored `0%`.
    #[cfg(test)]
    pub(crate) const fn from_affine(
        length: LayoutLength,
        percentage: f32,
        has_percentage: bool,
    ) -> Self {
        Self::Affine(AffineLengthPercentage {
            length,
            percentage,
            has_percentage,
            percentage_requires_basis: has_percentage,
        })
    }

    #[cfg(test)]
    pub(crate) const fn from_neutralized_affine(length: LayoutLength, percentage: f32) -> Self {
        Self::Affine(AffineLengthPercentage {
            length,
            percentage,
            has_percentage: true,
            percentage_requires_basis: false,
        })
    }

    pub(crate) fn from_term(unit: DeferredLengthUnit, coefficient: f32) -> Self {
        if coefficient == 0.0 {
            return Self::ZERO;
        }
        Self::Expression(Rc::new(LengthPercentageExpression::Term {
            unit,
            coefficient,
        }))
    }

    pub(crate) fn from_em(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Em, value)
    }

    pub(crate) fn from_rem(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Rem, value)
    }

    pub(crate) fn from_ch(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Ch, value)
    }

    pub(crate) fn from_ex(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Ex, value)
    }

    pub(crate) fn from_cap(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Cap, value)
    }

    pub(crate) fn from_ic(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Ic, value)
    }

    pub(crate) fn from_lh(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Lh, value)
    }

    pub(crate) fn from_rex(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Rex, value)
    }

    pub(crate) fn from_rcap(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Rcap, value)
    }

    pub(crate) fn from_rch(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Rch, value)
    }

    pub(crate) fn from_ric(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Ric, value)
    }

    pub(crate) fn from_rlh(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Rlh, value)
    }

    pub(crate) fn from_vw(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Vw, value)
    }

    pub(crate) fn from_vh(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Vh, value)
    }

    pub(crate) fn from_vmin(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Vmin, value)
    }

    pub(crate) fn from_vmax(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Vmax, value)
    }

    pub(crate) fn from_vi(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Vi, value)
    }

    pub(crate) fn from_vb(value: f32) -> Self {
        Self::from_term(DeferredLengthUnit::Vb, value)
    }

    pub(crate) fn from_container_unit(unit: &str, value: f32) -> Option<Self> {
        let unit = match unit.to_ascii_lowercase().as_str() {
            "cqw" => DeferredLengthUnit::Cqw,
            "cqh" => DeferredLengthUnit::Cqh,
            "cqi" => DeferredLengthUnit::Cqi,
            "cqb" => DeferredLengthUnit::Cqb,
            "cqmin" => DeferredLengthUnit::Cqmin,
            "cqmax" => DeferredLengthUnit::Cqmax,
            _ => return None,
        };
        Some(Self::from_term(unit, value))
    }

    pub(crate) fn sum(left: Self, right: Self) -> Self {
        match (left, right) {
            (Self::Affine(left), Self::Affine(right)) => Self::Affine(left.sum(right)),
            (Self::Affine(left), Self::Expression(right)) => {
                Self::from_expression(LengthPercentageExpression::Sum(
                    right,
                    Rc::new(LengthPercentageExpression::Affine(left)),
                ))
            }
            (left, right) => Self::from_expression(LengthPercentageExpression::Sum(
                left.into_expression(),
                right.into_expression(),
            )),
        }
    }

    pub(crate) fn product(value: Self, factor: f32) -> Self {
        match value {
            Self::Affine(value) => Self::Affine(value.product(factor)),
            _value if factor == 0.0 => Self::ZERO,
            Self::Expression(expression) => match expression.as_ref() {
                LengthPercentageExpression::Term { unit, coefficient } => {
                    Self::from_term(*unit, coefficient * factor)
                }
                LengthPercentageExpression::Product(value, previous_factor) => {
                    Self::from_expression(LengthPercentageExpression::Product(
                        Rc::clone(value),
                        previous_factor * factor,
                    ))
                }
                _ => Self::from_expression(LengthPercentageExpression::Product(expression, factor)),
            },
        }
    }

    pub(crate) fn negated(self) -> Self {
        match self {
            Self::Affine(value) => Self::Affine(value.negated()),
            Self::Expression(expression) => Self::from_expression(expression.negated()),
        }
    }

    pub(crate) fn min(left: Self, right: Self) -> Self {
        if let Some(ordering) = left.computed_ordering(&right) {
            return if ordering.is_gt() { right } else { left };
        }
        Self::from_expression(LengthPercentageExpression::Min(
            left.into_expression(),
            right.into_expression(),
        ))
    }

    pub(crate) fn max(left: Self, right: Self) -> Self {
        if let Some(ordering) = left.computed_ordering(&right) {
            return if ordering.is_lt() { right } else { left };
        }
        Self::from_expression(LengthPercentageExpression::Max(
            left.into_expression(),
            right.into_expression(),
        ))
    }

    pub(crate) fn clamp(min: Self, center: Self, max: Self) -> Self {
        if let (Some(center_vs_max), Some(center_vs_min)) = (
            center.computed_ordering(&max),
            center.computed_ordering(&min),
        ) {
            if center_vs_max.is_gt() {
                return max;
            }
            if center_vs_min.is_lt() {
                return min;
            }
            return center;
        }
        Self::from_expression(LengthPercentageExpression::Clamp {
            min: min.into_expression(),
            center: center.into_expression(),
            max: max.into_expression(),
        })
    }

    fn from_expression(expression: LengthPercentageExpression) -> Self {
        match expression.affine() {
            Some(affine) => Self::Affine(affine),
            None => Self::Expression(Rc::new(expression)),
        }
    }

    fn into_expression(self) -> Rc<LengthPercentageExpression> {
        match self {
            Self::Affine(value) => Rc::new(LengthPercentageExpression::Affine(value)),
            Self::Expression(expression) => expression,
        }
    }

    pub(crate) fn contains_percentage(&self) -> bool {
        match self {
            Self::Affine(value) => value.has_percentage,
            Self::Expression(expression) => expression.contains_percentage(),
        }
    }

    pub(crate) fn needs_percentage_basis(&self) -> bool {
        match self {
            Self::Affine(value) => value.percentage_requires_basis,
            Self::Expression(expression) => expression.needs_percentage_basis(),
        }
    }

    pub(crate) fn is_definitely_absolute(&self) -> bool {
        matches!(self, Self::Affine(value) if !value.percentage_requires_basis && value.percentage == 0.0)
    }

    pub(crate) fn fixed_component(&self) -> LayoutLength {
        // A linear deferred expression still has a useful additive absolute
        // component (for example, `calc(10pt + 1em)` has a 10pt component).
        // Comparisons deliberately do not: selecting a branch of
        // `min(10pt, 50%)` depends on the property's percentage basis.
        self.linear_form()
            .map(|(affine, _)| affine.length)
            .unwrap_or_else(|| layout_pt(0.0))
    }

    pub(crate) fn length_points(&self) -> f32 {
        self.fixed_component().points()
    }

    pub(crate) fn length_max_zero(&self) -> LayoutLength {
        layout_pt(self.length_points().max(0.0))
    }

    pub(crate) fn length_is_zero(&self) -> bool {
        self.length_points() == 0.0
    }

    pub(crate) fn length_if_no_percent(&self) -> Option<f32> {
        self.is_definitely_absolute()
            .then(|| self.fixed_component().points())
    }

    pub(crate) fn difference_if_absolute(&self, other: &Self) -> Option<LayoutLength> {
        Some(layout_pt(
            self.length_if_no_percent()? - other.length_if_no_percent()?,
        ))
    }

    pub(crate) fn percentage_coefficient(&self) -> Option<f32> {
        self.affine().map(|value| value.percentage)
    }

    pub(crate) fn percentage_coefficient_or_zero(&self) -> f32 {
        self.percentage_coefficient().unwrap_or(0.0)
    }

    /// Returns the percentage coefficient only for a value with no absolute
    /// or deferred component.  This is useful for the few grammar ranges
    /// whose percentage basis is known to be non-negative.
    pub(crate) fn pure_percentage_coefficient(&self) -> Option<f32> {
        self.affine()
            .and_then(|value| (value.length == layout_pt(0.0)).then_some(value.percentage))
    }

    /// Compares two expressions only when CSS permits choosing a branch at
    /// computed-value time. Equal percentage coefficients cancel; differing
    /// coefficients require the property-specific used-value basis.
    pub(crate) fn computed_ordering(&self, other: &Self) -> Option<Ordering> {
        let (left_affine, left_terms) = self.linear_form()?;
        let (right_affine, right_terms) = other.linear_form()?;
        let difference = left_affine.sum(right_affine.product(-1.0));
        // Percentage bases are property-specific and can be negative (for
        // example a background-position free-space basis), so unlike metric
        // lengths they cannot establish an ordering before used-value time.
        if difference.percentage != 0.0 {
            return None;
        }
        let terms = merge_linear_terms(left_terms, scale_linear_terms(right_terms, -1.0));
        let mut has_positive = difference.length.points() > 0.0;
        let mut has_negative = difference.length.points() < 0.0;
        for (_, coefficient) in terms {
            has_positive |= coefficient > 0.0;
            has_negative |= coefficient < 0.0;
        }
        match (has_negative, has_positive) {
            (false, false) => Some(Ordering::Equal),
            (false, true) => Some(Ordering::Greater),
            (true, false) => Some(Ordering::Less),
            (true, true) => None,
        }
    }

    /// Conservative range check used by grammars with a non-negative range.
    /// It returns true only when every unresolved component is non-positive
    /// and at least one is negative, so no positive percentage basis or unit
    /// metric can make the value non-negative.
    pub(crate) fn is_definitely_negative(&self) -> bool {
        self.sign_bounds()
            .is_some_and(|(negative, positive)| negative && !positive)
    }

    /// Appends a structural cache key. This deliberately includes every term
    /// and comparison node rather than serializing a lossy used value.
    pub(crate) fn write_cache_key(&self, output: &mut String) {
        match self {
            Self::Affine(value) => {
                output.push_str("a(");
                output.push_str(&value.length.points().to_bits().to_string());
                output.push(',');
                output.push_str(&value.percentage.to_bits().to_string());
                output.push(',');
                output.push_str(if value.has_percentage { "1," } else { "0," });
                output.push_str(if value.percentage_requires_basis {
                    "1)"
                } else {
                    "0)"
                });
            }
            Self::Expression(expression) => expression.write_cache_key(output),
        }
    }

    /// Reduces comparisons whose caller guarantees a non-negative percentage
    /// basis. This is deliberately separate from normal computed-value
    /// canonicalization because other properties (notably background
    /// positions) may use negative bases.
    pub(crate) fn reduce_math_with_nonnegative_percentage_basis(&mut self) {
        let Self::Expression(expression) = self else {
            return;
        };
        *self = Self::from_expression(expression.reduce_nonnegative_percentage_math());
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Expression(expression) if expression.requires_term(DeferredLengthUnit::Ch))
    }

    pub(crate) fn resolve_font_relative_lengths(&mut self, basis: FontRelativeLengthBasis) {
        self.resolve_terms(|unit, coefficient| match unit {
            DeferredLengthUnit::Em => Some(layout_pt(coefficient * basis.font_size().points())),
            _ => None,
        });
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.resolve_terms(|unit, coefficient| {
            (unit == DeferredLengthUnit::Ch).then_some(layout_pt(coefficient * ch_advance.points()))
        });
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        self.resolve_one_metric(DeferredLengthUnit::Em, font_size);
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        self.resolve_terms(|unit, coefficient| {
            (unit == DeferredLengthUnit::Rem).then_some(layout_pt(coefficient * root_font_size))
        });
    }

    pub(crate) fn resolve_ic_relative_lengths(&mut self, metric: LayoutLength) {
        self.resolve_one_metric(DeferredLengthUnit::Ic, metric);
    }

    pub(crate) fn resolve_ex_relative_lengths(&mut self, metric: f32) {
        self.resolve_terms(|unit, coefficient| {
            (unit == DeferredLengthUnit::Ex).then_some(layout_pt(coefficient * metric))
        });
    }

    pub(crate) fn resolve_cap_relative_lengths(&mut self, metric: f32) {
        self.resolve_terms(|unit, coefficient| {
            (unit == DeferredLengthUnit::Cap).then_some(layout_pt(coefficient * metric))
        });
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, metric: LayoutLength) {
        self.resolve_one_metric(DeferredLengthUnit::Lh, metric);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.resolve_terms(|unit, coefficient| match unit {
            DeferredLengthUnit::Rex => Some(layout_pt(coefficient * basis.x_height.points())),
            DeferredLengthUnit::Rcap => Some(layout_pt(coefficient * basis.cap_height.points())),
            DeferredLengthUnit::Rch => Some(layout_pt(coefficient * basis.ch_advance.points())),
            DeferredLengthUnit::Ric => Some(layout_pt(coefficient * basis.ic_advance.points())),
            DeferredLengthUnit::Rlh => Some(layout_pt(coefficient * basis.line_height.points())),
            _ => None,
        });
    }

    pub(crate) fn resolve_container_relative_lengths(&mut self, basis: ContainerLengthBasis) {
        self.resolve_terms(|unit, coefficient| match unit {
            DeferredLengthUnit::Cqw => Some(basis.cqw(coefficient)),
            DeferredLengthUnit::Cqh => Some(basis.cqh(coefficient)),
            DeferredLengthUnit::Cqi => Some(basis.cqi(coefficient)),
            DeferredLengthUnit::Cqb => Some(basis.cqb(coefficient)),
            DeferredLengthUnit::Cqmin => Some(layout_pt(
                coefficient * basis.cqi(100.0).points().min(basis.cqb(100.0).points()) / 100.0,
            )),
            DeferredLengthUnit::Cqmax => Some(layout_pt(
                coefficient * basis.cqi(100.0).points().max(basis.cqb(100.0).points()) / 100.0,
            )),
            _ => None,
        });
    }

    pub(crate) fn used_length_with_percentage_basis<T, Source>(
        &self,
        percentage_basis: PercentageBasis<T, Source>,
    ) -> Option<LayoutLength>
    where
        T: SemanticLengthExt,
    {
        match self {
            Self::Affine(value) => value.used(percentage_basis.points()),
            Self::Expression(expression) => expression.used(percentage_basis.points()),
        }
    }

    pub(crate) fn used_length_with_percentage_basis_points(
        &self,
        percentage_basis: Option<f32>,
    ) -> Option<f32> {
        match self {
            Self::Affine(value) => value.used(percentage_basis),
            Self::Expression(expression) => expression.used(percentage_basis),
        }
        .map(LayoutLength::points)
    }

    fn affine(&self) -> Option<AffineLengthPercentage> {
        match self {
            Self::Affine(value) => Some(*value),
            Self::Expression(expression) => expression.affine(),
        }
    }

    fn linear_form(&self) -> Option<(AffineLengthPercentage, Vec<(DeferredLengthUnit, f32)>)> {
        match self {
            Self::Affine(value) => Some((*value, Vec::new())),
            Self::Expression(expression) => expression.linear_form(),
        }
    }

    fn sign_bounds(&self) -> Option<(bool, bool)> {
        match self {
            Self::Affine(value) => Some((
                value.length.points() < 0.0 || value.percentage < 0.0,
                value.length.points() > 0.0 || value.percentage > 0.0,
            )),
            Self::Expression(expression) => expression.sign_bounds(),
        }
    }

    fn resolve_one_metric(&mut self, unit: DeferredLengthUnit, metric: LayoutLength) {
        self.resolve_terms(|candidate, coefficient| {
            (candidate == unit).then_some(layout_pt(coefficient * metric.points()))
        });
    }

    fn resolve_terms(
        &mut self,
        resolve: impl FnMut(DeferredLengthUnit, f32) -> Option<LayoutLength>,
    ) {
        let Self::Expression(expression) = self else {
            return;
        };
        *self = Self::from_expression(expression.resolve_terms(resolve));
    }
}

fn scale_expression_fixed_length_components(
    expression: &LengthPercentageExpression,
    factor: f32,
) -> LengthPercentageExpression {
    match expression {
        LengthPercentageExpression::Affine(value) => {
            LengthPercentageExpression::Affine(AffineLengthPercentage {
                length: value.length * factor,
                percentage: value.percentage,
                has_percentage: value.has_percentage,
                percentage_requires_basis: value.percentage_requires_basis,
            })
        }
        // Deferred font-relative components are resolved from the zoomed font
        // metrics after this boundary, so multiplying their coefficient here
        // would scale them twice.
        LengthPercentageExpression::Term { unit, coefficient } => {
            LengthPercentageExpression::Term {
                unit: *unit,
                coefficient: *coefficient,
            }
        }
        LengthPercentageExpression::Sum(left, right) => LengthPercentageExpression::Sum(
            Rc::new(scale_expression_fixed_length_components(left, factor)),
            Rc::new(scale_expression_fixed_length_components(right, factor)),
        ),
        LengthPercentageExpression::Product(value, multiplier) => {
            LengthPercentageExpression::Product(
                Rc::new(scale_expression_fixed_length_components(value, factor)),
                *multiplier,
            )
        }
        LengthPercentageExpression::Min(left, right) => LengthPercentageExpression::Min(
            Rc::new(scale_expression_fixed_length_components(left, factor)),
            Rc::new(scale_expression_fixed_length_components(right, factor)),
        ),
        LengthPercentageExpression::Max(left, right) => LengthPercentageExpression::Max(
            Rc::new(scale_expression_fixed_length_components(left, factor)),
            Rc::new(scale_expression_fixed_length_components(right, factor)),
        ),
        LengthPercentageExpression::Clamp { min, center, max } => {
            LengthPercentageExpression::Clamp {
                min: Rc::new(scale_expression_fixed_length_components(min, factor)),
                center: Rc::new(scale_expression_fixed_length_components(center, factor)),
                max: Rc::new(scale_expression_fixed_length_components(max, factor)),
            }
        }
    }
}

fn scale_linear_terms(
    terms: Vec<(DeferredLengthUnit, f32)>,
    factor: f32,
) -> Vec<(DeferredLengthUnit, f32)> {
    terms
        .into_iter()
        .filter_map(|(unit, coefficient)| {
            let coefficient = coefficient * factor;
            (coefficient != 0.0).then_some((unit, coefficient))
        })
        .collect()
}

fn merge_linear_terms(
    mut left: Vec<(DeferredLengthUnit, f32)>,
    right: Vec<(DeferredLengthUnit, f32)>,
) -> Vec<(DeferredLengthUnit, f32)> {
    for (unit, coefficient) in right {
        if let Some((_, existing)) = left.iter_mut().find(|(candidate, _)| *candidate == unit) {
            *existing += coefficient;
        } else {
            left.push((unit, coefficient));
        }
    }
    left.retain(|(_, coefficient)| *coefficient != 0.0);
    left
}

impl LengthPercentageExpression {
    fn negated(&self) -> Self {
        match self {
            Self::Affine(value) => Self::Affine(value.negated()),
            Self::Term { unit, coefficient } => Self::Term {
                unit: *unit,
                coefficient: -*coefficient,
            },
            Self::Sum(left, right) => Self::Sum(Rc::new(left.negated()), Rc::new(right.negated())),
            Self::Product(value, factor) => Self::Product(Rc::new(value.negated()), *factor),
            Self::Min(left, right) => Self::Max(Rc::new(left.negated()), Rc::new(right.negated())),
            Self::Max(left, right) => Self::Min(Rc::new(left.negated()), Rc::new(right.negated())),
            Self::Clamp { min, center, max } => Self::Clamp {
                min: Rc::new(max.negated()),
                center: Rc::new(center.negated()),
                max: Rc::new(min.negated()),
            },
        }
    }

    fn write_cache_key(&self, output: &mut String) {
        match self {
            Self::Affine(value) => {
                output.push_str("a(");
                output.push_str(&value.length.points().to_bits().to_string());
                output.push(',');
                output.push_str(&value.percentage.to_bits().to_string());
                output.push(',');
                output.push_str(if value.has_percentage { "1," } else { "0," });
                output.push_str(if value.percentage_requires_basis {
                    "1)"
                } else {
                    "0)"
                });
            }
            Self::Term { unit, coefficient } => {
                output.push_str("t(");
                output.push_str(&format!("{unit:?}"));
                output.push(',');
                output.push_str(&coefficient.to_bits().to_string());
                output.push(')');
            }
            Self::Sum(left, right) => write_pair_key(output, "s", left, right),
            Self::Product(value, factor) => {
                output.push_str("p(");
                value.write_cache_key(output);
                output.push(',');
                output.push_str(&factor.to_bits().to_string());
                output.push(')');
            }
            Self::Min(left, right) => write_pair_key(output, "n", left, right),
            Self::Max(left, right) => write_pair_key(output, "x", left, right),
            Self::Clamp { min, center, max } => {
                output.push_str("c(");
                min.write_cache_key(output);
                output.push(',');
                center.write_cache_key(output);
                output.push(',');
                max.write_cache_key(output);
                output.push(')');
            }
        }
    }
    fn affine(&self) -> Option<AffineLengthPercentage> {
        match self {
            Self::Affine(value) => Some(*value),
            Self::Term { .. } => None,
            Self::Sum(left, right) => Some(left.affine()?.sum(right.affine()?)),
            Self::Product(value, factor) => Some(value.affine()?.product(*factor)),
            Self::Min(left, right) => {
                let left = left.affine()?;
                let right = right.affine()?;
                if left.percentage == right.percentage {
                    Some(if left.length.points() <= right.length.points() {
                        left
                    } else {
                        right
                    })
                } else {
                    None
                }
            }
            Self::Max(left, right) => {
                let left = left.affine()?;
                let right = right.affine()?;
                if left.percentage == right.percentage {
                    Some(if left.length.points() >= right.length.points() {
                        left
                    } else {
                        right
                    })
                } else {
                    None
                }
            }
            Self::Clamp { min, center, max } => {
                let min = min.affine()?;
                let center = center.affine()?;
                let max = max.affine()?;
                (min.percentage == center.percentage && center.percentage == max.percentage).then(
                    || AffineLengthPercentage {
                        length: layout_pt(
                            center
                                .length
                                .points()
                                .clamp(min.length.points(), max.length.points()),
                        ),
                        percentage: center.percentage,
                        has_percentage: min.has_percentage
                            || center.has_percentage
                            || max.has_percentage,
                        percentage_requires_basis: min.percentage_requires_basis
                            || center.percentage_requires_basis
                            || max.percentage_requires_basis,
                    },
                )
            }
        }
    }

    fn reduce_nonnegative_percentage_math(&self) -> Self {
        match self {
            Self::Affine(value) => Self::Affine(*value),
            Self::Term { unit, coefficient } => Self::Term {
                unit: *unit,
                coefficient: *coefficient,
            },
            Self::Sum(left, right) => Self::Sum(
                Rc::new(left.reduce_nonnegative_percentage_math()),
                Rc::new(right.reduce_nonnegative_percentage_math()),
            ),
            Self::Product(value, factor) => {
                Self::Product(Rc::new(value.reduce_nonnegative_percentage_math()), *factor)
            }
            Self::Min(left, right) | Self::Max(left, right) => {
                let left = left.reduce_nonnegative_percentage_math();
                let right = right.reduce_nonnegative_percentage_math();
                if let (Some(left_affine), Some(right_affine)) = (left.affine(), right.affine())
                    && left_affine.length == right_affine.length
                {
                    let take_left = if matches!(self, Self::Min(..)) {
                        left_affine.percentage <= right_affine.percentage
                    } else {
                        left_affine.percentage >= right_affine.percentage
                    };
                    return if take_left { left } else { right };
                }
                if matches!(self, Self::Min(..)) {
                    Self::Min(Rc::new(left), Rc::new(right))
                } else {
                    Self::Max(Rc::new(left), Rc::new(right))
                }
            }
            Self::Clamp { min, center, max } => {
                let min = min.reduce_nonnegative_percentage_math();
                let center = center.reduce_nonnegative_percentage_math();
                let max = max.reduce_nonnegative_percentage_math();
                if let (Some(min_affine), Some(center_affine), Some(max_affine)) =
                    (min.affine(), center.affine(), max.affine())
                    && min_affine.length == center_affine.length
                    && center_affine.length == max_affine.length
                {
                    return Self::Affine(AffineLengthPercentage {
                        length: center_affine.length,
                        percentage: center_affine
                            .percentage
                            .clamp(min_affine.percentage, max_affine.percentage),
                        has_percentage: min_affine.has_percentage
                            || center_affine.has_percentage
                            || max_affine.has_percentage,
                        percentage_requires_basis: min_affine.percentage_requires_basis
                            || center_affine.percentage_requires_basis
                            || max_affine.percentage_requires_basis,
                    });
                }
                Self::Clamp {
                    min: Rc::new(min),
                    center: Rc::new(center),
                    max: Rc::new(max),
                }
            }
        }
    }

    fn linear_form(&self) -> Option<(AffineLengthPercentage, Vec<(DeferredLengthUnit, f32)>)> {
        match self {
            Self::Affine(value) => Some((*value, Vec::new())),
            Self::Term { unit, coefficient } => {
                Some((AffineLengthPercentage::ZERO, vec![(*unit, *coefficient)]))
            }
            Self::Sum(left, right) => {
                let (left_affine, left_terms) = left.linear_form()?;
                let (right_affine, right_terms) = right.linear_form()?;
                Some((
                    left_affine.sum(right_affine),
                    merge_linear_terms(left_terms, right_terms),
                ))
            }
            Self::Product(value, factor) => {
                let (affine, terms) = value.linear_form()?;
                Some((affine.product(*factor), scale_linear_terms(terms, *factor)))
            }
            Self::Min(..) | Self::Max(..) | Self::Clamp { .. } => None,
        }
    }

    fn sign_bounds(&self) -> Option<(bool, bool)> {
        match self {
            Self::Affine(value) => Some((
                value.length.points() < 0.0 || value.percentage < 0.0,
                value.length.points() > 0.0 || value.percentage > 0.0,
            )),
            Self::Term { coefficient, .. } => Some((*coefficient < 0.0, *coefficient > 0.0)),
            Self::Sum(left, right) => {
                let (left_negative, left_positive) = left.sign_bounds()?;
                let (right_negative, right_positive) = right.sign_bounds()?;
                Some((
                    left_negative || right_negative,
                    left_positive || right_positive,
                ))
            }
            Self::Product(value, factor) => {
                let (negative, positive) = value.sign_bounds()?;
                Some(if *factor < 0.0 {
                    (positive, negative)
                } else if *factor == 0.0 {
                    (false, false)
                } else {
                    (negative, positive)
                })
            }
            // Comparisons can select a branch only at used-value time.
            Self::Min(..) | Self::Max(..) | Self::Clamp { .. } => None,
        }
    }

    fn contains_percentage(&self) -> bool {
        match self {
            Self::Affine(value) => value.has_percentage,
            Self::Term { .. } => false,
            Self::Sum(left, right) | Self::Min(left, right) | Self::Max(left, right) => {
                left.contains_percentage() || right.contains_percentage()
            }
            Self::Product(value, _) => value.contains_percentage(),
            Self::Clamp { min, center, max } => {
                min.contains_percentage()
                    || center.contains_percentage()
                    || max.contains_percentage()
            }
        }
    }

    fn needs_percentage_basis(&self) -> bool {
        match self {
            Self::Affine(value) => value.percentage_requires_basis,
            Self::Term { .. } => false,
            Self::Sum(left, right) => {
                left.needs_percentage_basis() || right.needs_percentage_basis()
            }
            Self::Product(value, factor) => {
                *factor != 0.0
                    && value.affine().map_or_else(
                        || value.needs_percentage_basis(),
                        |value| value.percentage != 0.0 && value.percentage_requires_basis,
                    )
            }
            // A comparison can select a percentage-bearing branch only once
            // the property's basis is known.
            Self::Min(..) | Self::Max(..) | Self::Clamp { .. } => self.contains_percentage(),
        }
    }

    fn used(&self, percentage_basis: Option<f32>) -> Option<LayoutLength> {
        match self {
            Self::Affine(value) => value.used(percentage_basis),
            Self::Term { .. } => None,
            Self::Sum(left, right) => {
                Some(left.used(percentage_basis)? + right.used(percentage_basis)?)
            }
            Self::Product(value, factor) => Some(value.used(percentage_basis)? * *factor),
            Self::Min(left, right) => Some(
                left.used(percentage_basis)?
                    .min(right.used(percentage_basis)?),
            ),
            Self::Max(left, right) => Some(
                left.used(percentage_basis)?
                    .max(right.used(percentage_basis)?),
            ),
            Self::Clamp { min, center, max } => Some(
                center
                    .used(percentage_basis)?
                    .min(max.used(percentage_basis)?)
                    .max(min.used(percentage_basis)?),
            ),
        }
    }

    fn requires_term(&self, wanted: DeferredLengthUnit) -> bool {
        match self {
            Self::Affine(_) => false,
            Self::Term { unit, coefficient } => *unit == wanted && *coefficient != 0.0,
            Self::Sum(left, right) | Self::Min(left, right) | Self::Max(left, right) => {
                left.requires_term(wanted) || right.requires_term(wanted)
            }
            Self::Product(value, _) => value.requires_term(wanted),
            Self::Clamp { min, center, max } => {
                min.requires_term(wanted)
                    || center.requires_term(wanted)
                    || max.requires_term(wanted)
            }
        }
    }

    fn resolve_terms(
        &self,
        mut resolve: impl FnMut(DeferredLengthUnit, f32) -> Option<LayoutLength>,
    ) -> Self {
        self.resolve_terms_with(&mut resolve)
    }

    fn resolve_terms_with(
        &self,
        resolve: &mut impl FnMut(DeferredLengthUnit, f32) -> Option<LayoutLength>,
    ) -> Self {
        match self {
            Self::Affine(value) => Self::Affine(*value),
            Self::Term { unit, coefficient } => resolve(*unit, *coefficient)
                .map(AffineLengthPercentage::length)
                .map(Self::Affine)
                .unwrap_or(Self::Term {
                    unit: *unit,
                    coefficient: *coefficient,
                }),
            Self::Sum(left, right) => Self::Sum(
                Rc::new(left.resolve_terms_with(resolve)),
                Rc::new(right.resolve_terms_with(resolve)),
            ),
            Self::Product(value, factor) => {
                Self::Product(Rc::new(value.resolve_terms_with(resolve)), *factor)
            }
            Self::Min(left, right) => Self::Min(
                Rc::new(left.resolve_terms_with(resolve)),
                Rc::new(right.resolve_terms_with(resolve)),
            ),
            Self::Max(left, right) => Self::Max(
                Rc::new(left.resolve_terms_with(resolve)),
                Rc::new(right.resolve_terms_with(resolve)),
            ),
            Self::Clamp { min, center, max } => Self::Clamp {
                min: Rc::new(min.resolve_terms_with(resolve)),
                center: Rc::new(center.resolve_terms_with(resolve)),
                max: Rc::new(max.resolve_terms_with(resolve)),
            },
        }
    }
}

fn write_pair_key(
    output: &mut String,
    tag: &str,
    left: &LengthPercentageExpression,
    right: &LengthPercentageExpression,
) {
    output.push_str(tag);
    output.push('(');
    left.write_cache_key(output);
    output.push(',');
    right.write_cache_key(output);
    output.push(')');
}

impl ResolveViewportLengths for ComputedLengthPercentage {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.resolve_terms(|unit, coefficient| match unit {
            DeferredLengthUnit::Vw => Some(basis.vw(coefficient)),
            DeferredLengthUnit::Vh => Some(basis.vh(coefficient)),
            DeferredLengthUnit::Vmin => Some(basis.vmin(coefficient)),
            DeferredLengthUnit::Vmax => Some(basis.vmax(coefficient)),
            DeferredLengthUnit::Vi => Some(basis.vi(coefficient)),
            DeferredLengthUnit::Vb => Some(basis.vb(coefficient)),
            _ => None,
        });
        self.resolve_container_relative_lengths(basis.container_fallback());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LayoutSize;
    use std::mem::size_of;

    #[test]
    fn common_cases_are_compact_and_tree_free() {
        assert!(size_of::<ComputedLengthPercentage>() <= 16);
        for value in [
            ComputedLengthPercentage::from_points(10.0),
            ComputedLengthPercentage::from_percent(0.5),
            ComputedLengthPercentage::sum(
                ComputedLengthPercentage::from_points(10.0),
                ComputedLengthPercentage::from_percent(0.5),
            ),
        ] {
            assert!(matches!(value, ComputedLengthPercentage::Affine(_)));
        }

        let authored_zero = ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_points(10.0),
            ComputedLengthPercentage::from_percent(0.0),
        );
        assert!(matches!(authored_zero, ComputedLengthPercentage::Affine(_)));
        assert!(authored_zero.contains_percentage());
    }

    #[test]
    fn authored_zero_percentage_is_not_an_absolute_zero() {
        let zero = ComputedLengthPercentage::ZERO;
        let percentage = ComputedLengthPercentage::from_percent(0.0);
        assert_ne!(zero, percentage);
        assert!(percentage.contains_percentage());
        assert!(!percentage.is_definitely_absolute());
    }

    #[test]
    fn phase_resolution_replaces_terms_without_mutating_shared_trees() {
        let original = ComputedLengthPercentage::sum(
            ComputedLengthPercentage::sum(
                ComputedLengthPercentage::from_em(2.0),
                ComputedLengthPercentage::from_vw(10.0),
            ),
            ComputedLengthPercentage::sum(
                ComputedLengthPercentage::from_container_unit("cqw", 20.0).unwrap(),
                ComputedLengthPercentage::from_percent(0.5),
            ),
        );
        let mut resolved = original.clone();
        resolved.resolve_font_relative_lengths(FontRelativeLengthBasis::new(
            layout_pt(12.0),
            layout_pt(6.0),
        ));
        resolved.resolve_container_relative_lengths(ContainerLengthBasis::for_writing_mode(
            LayoutSize::new(150.0, 80.0),
            WritingMode::HorizontalTb,
        ));
        resolved.resolve_viewport_lengths(ViewportLengthBasis::for_writing_mode(
            LayoutSize::new(200.0, 100.0),
            WritingMode::HorizontalTb,
        ));
        assert_eq!(
            resolved
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(40.0)))
                .unwrap(),
            layout_pt(94.0),
        );
        assert!(matches!(resolved, ComputedLengthPercentage::Affine(_)));
        assert!(!matches!(original, ComputedLengthPercentage::Affine(_)));
    }

    #[test]
    fn expression_tree_drops_after_last_style_value() {
        let value = ComputedLengthPercentage::from_em(1.0);
        let ComputedLengthPercentage::Expression(tree) = &value else {
            panic!("deferred term must allocate a tree");
        };
        let weak = Rc::downgrade(tree);
        let clone = value.clone();
        drop(value);
        assert!(weak.upgrade().is_some());
        drop(clone);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn deferred_comparisons_wait_for_the_property_percentage_basis() {
        let minimum = ComputedLengthPercentage::min(
            ComputedLengthPercentage::from_points(10.0),
            ComputedLengthPercentage::from_percent(0.5),
        );
        let clamped = ComputedLengthPercentage::clamp(
            ComputedLengthPercentage::from_percent(0.05),
            ComputedLengthPercentage::from_percent(0.10),
            ComputedLengthPercentage::from_percent(0.15),
        );

        assert!(matches!(minimum, ComputedLengthPercentage::Expression(_)));
        assert!(matches!(clamped, ComputedLengthPercentage::Expression(_)));
        assert_eq!(
            minimum
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(-100.0)))
                .unwrap(),
            layout_pt(-50.0),
        );
        assert_eq!(
            clamped
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(-100.0)))
                .unwrap(),
            layout_pt(-5.0),
        );
    }

    #[test]
    fn linear_expression_retains_its_absolute_component() {
        let value = ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_points(10.0),
            ComputedLengthPercentage::from_em(1.0),
        );

        assert_eq!(value.fixed_component(), layout_pt(10.0));
        assert_eq!(value.length_points(), 10.0);
        assert!(!value.is_definitely_absolute());

        let comparison = ComputedLengthPercentage::min(
            ComputedLengthPercentage::from_points(10.0),
            ComputedLengthPercentage::from_percent(0.5),
        );
        assert_eq!(comparison.fixed_component(), layout_pt(0.0));
    }

    #[test]
    fn cloned_trees_resolve_each_font_phase_independently() {
        let original = ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_rem(1.0),
            ComputedLengthPercentage::sum(
                ComputedLengthPercentage::from_em(1.0),
                ComputedLengthPercentage::sum(
                    ComputedLengthPercentage::from_ch(1.0),
                    ComputedLengthPercentage::sum(
                        ComputedLengthPercentage::from_ex(1.0),
                        ComputedLengthPercentage::sum(
                            ComputedLengthPercentage::from_cap(1.0),
                            ComputedLengthPercentage::sum(
                                ComputedLengthPercentage::from_ic(1.0),
                                ComputedLengthPercentage::from_lh(1.0),
                            ),
                        ),
                    ),
                ),
            ),
        );
        let mut first = original.clone();
        let mut second = original.clone();

        for (value, root, font, ch, ex, cap, ic, line_height) in [
            (&mut first, 10.0, 20.0, 3.0, 4.0, 5.0, 6.0, 7.0),
            (&mut second, 11.0, 21.0, 4.0, 5.0, 6.0, 7.0, 8.0),
        ] {
            value.resolve_root_font_relative_lengths(root);
            value.resolve_font_relative_lengths(FontRelativeLengthBasis::new(
                layout_pt(font),
                layout_pt(ch),
            ));
            value.resolve_font_metric_lengths(layout_pt(ch));
            value.resolve_ex_relative_lengths(ex);
            value.resolve_cap_relative_lengths(cap);
            value.resolve_ic_relative_lengths(layout_pt(ic));
            value.resolve_line_height_relative_lengths(layout_pt(line_height));
        }

        assert_eq!(
            first.used_length_with_percentage_basis(PercentageBasis::<LayoutLength>::indefinite()),
            Some(layout_pt(55.0))
        );
        assert_eq!(
            second.used_length_with_percentage_basis(PercentageBasis::<LayoutLength>::indefinite()),
            Some(layout_pt(62.0))
        );
        assert!(!original.is_definitely_absolute());
        assert_ne!(
            ComputedLengthPercentage::from_rem(1.0),
            ComputedLengthPercentage::from_em(1.0)
        );
    }

    #[test]
    fn affine_calc_with_authored_zero_percentage_requires_a_basis() {
        let value = ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_percent(0.0),
            ComputedLengthPercentage::from_points(10.0),
        );

        assert!(value.contains_percentage());
        assert!(value.needs_percentage_basis());
        assert_eq!(
            value.used_length_with_percentage_basis(PercentageBasis::<LayoutLength>::indefinite()),
            None,
        );
        assert_eq!(
            value.used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(20.0))),
            Some(layout_pt(10.0)),
        );
    }

    #[test]
    fn root_font_metric_terms_resolve_to_an_affine_value() {
        let mut value = ComputedLengthPercentage::sum(
            ComputedLengthPercentage::from_rex(1.0),
            ComputedLengthPercentage::sum(
                ComputedLengthPercentage::from_rcap(1.0),
                ComputedLengthPercentage::sum(
                    ComputedLengthPercentage::from_rch(1.0),
                    ComputedLengthPercentage::sum(
                        ComputedLengthPercentage::from_ric(1.0),
                        ComputedLengthPercentage::from_rlh(1.0),
                    ),
                ),
            ),
        );
        value.resolve_root_font_metric_lengths(RootFontMetricLengthBasis {
            font_size: layout_pt(10.0),
            ch_advance: layout_pt(2.0),
            x_height: layout_pt(3.0),
            cap_height: layout_pt(4.0),
            ic_advance: layout_pt(5.0),
            line_height: layout_pt(6.0),
        });

        assert!(matches!(value, ComputedLengthPercentage::Affine(_)));
        assert_eq!(
            value.used_length_with_percentage_basis(PercentageBasis::<LayoutLength>::indefinite()),
            Some(layout_pt(20.0)),
        );
    }

    #[test]
    fn cache_keys_and_absolute_eligibility_preserve_expression_structure() {
        let absolute = ComputedLengthPercentage::from_points(10.0);
        let authored_zero_percentage = ComputedLengthPercentage::sum(
            absolute.clone(),
            ComputedLengthPercentage::from_percent(0.0),
        );
        let rem = ComputedLengthPercentage::from_rem(1.0);
        let em = ComputedLengthPercentage::from_em(1.0);
        let mut absolute_key = String::new();
        let mut percentage_key = String::new();
        let mut rem_key = String::new();
        let mut em_key = String::new();

        absolute.write_cache_key(&mut absolute_key);
        authored_zero_percentage.write_cache_key(&mut percentage_key);
        rem.write_cache_key(&mut rem_key);
        em.write_cache_key(&mut em_key);

        assert_ne!(absolute_key, percentage_key);
        assert_ne!(rem_key, em_key);
        assert!(absolute.is_definitely_absolute());
        assert!(!authored_zero_percentage.is_definitely_absolute());
        assert!(!rem.is_definitely_absolute());
    }
}
