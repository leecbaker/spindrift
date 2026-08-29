use crate::css::types::{
    ComputedLengthPercentage, CssColor, CssColorOrCurrentColor, ResolveViewportLengths,
    RootFontMetricLengthBasis, ViewportLengthBasis,
};
use crate::units::{LayoutLength, PercentageBasis, layout_pt};

/// Computed physical border colors.
///
/// CSS Backgrounds and Borders defines physical border-color longhands and the
/// `border-color` shorthand:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-color>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BorderColors {
    pub top: CssColorOrCurrentColor,
    pub right: CssColorOrCurrentColor,
    pub bottom: CssColorOrCurrentColor,
    pub left: CssColorOrCurrentColor,
}

impl BorderColors {
    pub const CURRENT_COLOR: Self = Self {
        top: CssColorOrCurrentColor::CurrentColor,
        right: CssColorOrCurrentColor::CurrentColor,
        bottom: CssColorOrCurrentColor::CurrentColor,
        left: CssColorOrCurrentColor::CurrentColor,
    };

    pub(crate) const fn resolve(self, current_color: CssColor) -> ResolvedBorderColors {
        ResolvedBorderColors {
            top: self.top.resolve(current_color),
            right: self.right.resolve(current_color),
            bottom: self.bottom.resolve(current_color),
            left: self.left.resolve(current_color),
        }
    }
}

/// Border colors ready for used-value layout and paint.
///
/// Keeping this separate from [`BorderColors`] prevents a symbolic
/// `currentcolor` from reaching a painter without the foreground color of the
/// concrete fragment that owns the border.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResolvedBorderColors {
    pub top: CssColor,
    pub right: CssColor,
    pub bottom: CssColor,
    pub left: CssColor,
}

/// Computed physical border styles.
///
/// CSS Backgrounds and Borders defines physical border-style longhands and the
/// `border-style` shorthand:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-style>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BorderStyles {
    pub top: BorderStyle,
    pub right: BorderStyle,
    pub bottom: BorderStyle,
    pub left: BorderStyle,
}

impl BorderStyles {
    pub const NONE: Self = Self {
        top: BorderStyle::None,
        right: BorderStyle::None,
        bottom: BorderStyle::None,
        left: BorderStyle::None,
    };
}

/// CSS border line style.
///
/// CSS Backgrounds and Borders defines the standardized line styles and makes
/// `none` and `hidden` force the used border width to zero:
/// <https://www.w3.org/TR/css-backgrounds-3/#line-style> and
/// <https://www.w3.org/TR/css-backgrounds-3/#border-width>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BorderStyle {
    None,
    Hidden,
    Dotted,
    Dashed,
    Solid,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl BorderStyle {
    /// Returns whether this line style forces a zero used border width.
    ///
    /// CSS Backgrounds and Borders defines `none` and `hidden` as styles whose
    /// used border width is zero:
    /// <https://www.w3.org/TR/css-backgrounds-3/#border-width>.
    pub(crate) fn suppresses_used_width(self) -> bool {
        matches!(self, Self::None | Self::Hidden)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderRadius {
    pub top_left: CornerRadius,
    pub top_right: CornerRadius,
    pub bottom_right: CornerRadius,
    pub bottom_left: CornerRadius,
}

impl BorderRadius {
    pub const ZERO: Self = Self {
        top_left: CornerRadius::ZERO,
        top_right: CornerRadius::ZERO,
        bottom_right: CornerRadius::ZERO,
        bottom_left: CornerRadius::ZERO,
    };

    pub(crate) fn is_zero(&self) -> bool {
        self.top_left.is_zero()
            && self.top_right.is_zero()
            && self.bottom_right.is_zero()
            && self.bottom_left.is_zero()
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.top_left.resolve_font_metric_lengths(ch_advance);
        self.top_right.resolve_font_metric_lengths(ch_advance);
        self.bottom_right.resolve_font_metric_lengths(ch_advance);
        self.bottom_left.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.top_left.resolve_root_font_metric_lengths(basis);
        self.top_right.resolve_root_font_metric_lengths(basis);
        self.bottom_right.resolve_root_font_metric_lengths(basis);
        self.bottom_left.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.top_left.requires_ch_advance()
            || self.top_right.requires_ch_advance()
            || self.bottom_right.requires_ch_advance()
            || self.bottom_left.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.top_left.requires_root_font_metrics()
            || self.top_right.requires_root_font_metrics()
            || self.bottom_right.requires_root_font_metrics()
            || self.bottom_left.requires_root_font_metrics()
    }
}

/// Superellipse corner-shape parameter from CSS Borders and Box Decorations
/// Level 4.
///
/// CSS Borders and Box Decorations Level 4 defines `corner-*-shape` in terms
/// of `superellipse()` parameters, with keyword aliases for common values:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SuperellipseParameter {
    NegativeInfinity,
    Number(f32),
    Infinity,
}

impl SuperellipseParameter {
    pub(crate) const ROUND: Self = Self::Number(1.0);
    pub(crate) const SQUIRCLE: Self = Self::Number(2.0);
    pub(crate) const BEVEL: Self = Self::Number(0.0);
    pub(crate) const SCOOP: Self = Self::Number(-1.0);
}

/// Per-corner shape from CSS Borders and Box Decorations Level 4.
///
/// The `corner-*-shape` properties define how the border contour connects the
/// two radius tangent points for a corner:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CornerShape {
    pub(crate) superellipse: SuperellipseParameter,
}

impl CornerShape {
    pub(crate) const ROUND: Self = Self {
        superellipse: SuperellipseParameter::ROUND,
    };
    pub(crate) const SQUIRCLE: Self = Self {
        superellipse: SuperellipseParameter::SQUIRCLE,
    };
    pub(crate) const SQUARE: Self = Self {
        superellipse: SuperellipseParameter::Infinity,
    };
    pub(crate) const BEVEL: Self = Self {
        superellipse: SuperellipseParameter::BEVEL,
    };
    pub(crate) const SCOOP: Self = Self {
        superellipse: SuperellipseParameter::SCOOP,
    };
    pub(crate) const NOTCH: Self = Self {
        superellipse: SuperellipseParameter::NegativeInfinity,
    };

    pub(crate) const fn superellipse(parameter: SuperellipseParameter) -> Self {
        Self {
            superellipse: parameter,
        }
    }

    pub(crate) fn is_round(self) -> bool {
        self.superellipse == SuperellipseParameter::ROUND
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CornerShapes {
    pub(crate) top_left: CornerShape,
    pub(crate) top_right: CornerShape,
    pub(crate) bottom_right: CornerShape,
    pub(crate) bottom_left: CornerShape,
}

impl CornerShapes {
    pub(crate) const ROUND: Self = Self {
        top_left: CornerShape::ROUND,
        top_right: CornerShape::ROUND,
        bottom_right: CornerShape::ROUND,
        bottom_left: CornerShape::ROUND,
    };

    pub(crate) fn all_round(self) -> bool {
        self.top_left.is_round()
            && self.top_right.is_round()
            && self.bottom_right.is_round()
            && self.bottom_left.is_round()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CornerRadius {
    pub horizontal: CornerRadiusComponent,
    pub vertical: CornerRadiusComponent,
}

impl CornerRadius {
    pub const ZERO: Self = Self {
        horizontal: CornerRadiusComponent::ZERO,
        vertical: CornerRadiusComponent::ZERO,
    };

    pub(crate) fn is_zero(&self) -> bool {
        self.horizontal.is_zero() && self.vertical.is_zero()
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.horizontal.resolve_font_metric_lengths(ch_advance);
        self.vertical.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.horizontal.resolve_root_font_metric_lengths(basis);
        self.vertical.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.horizontal.requires_ch_advance() || self.vertical.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.horizontal.requires_root_font_metrics() || self.vertical.requires_root_font_metrics()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CornerRadiusComponent {
    pub value: ComputedLengthPercentage,
}

impl CornerRadiusComponent {
    pub const ZERO: Self = Self {
        value: ComputedLengthPercentage::ZERO,
    };

    pub(crate) fn is_zero(&self) -> bool {
        self.value == ComputedLengthPercentage::ZERO
    }

    pub(crate) fn resolve(self, basis: PercentageBasis<LayoutLength>) -> LayoutLength {
        self.value
            .used_length_with_percentage_basis(basis)
            .unwrap_or_else(|| layout_pt(self.value.length_points()))
            .max(layout_pt(0.0))
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.value.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.value.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.value.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.value.requires_root_font_metrics()
    }
}

/// Computed CSS `box-decoration-break`.
///
/// CSS Backgrounds and Borders defines how borders, padding, backgrounds, and
/// related box decorations behave when a box is fragmented. CSS Inline reuses
/// the same policy for block-container `text-box-trim` in fragmented flows:
/// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break> and
/// <https://drafts.csswg.org/css-inline-3/#text-box-trim>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxDecorationBreak {
    Slice,
    Clone,
}

impl ResolveViewportLengths for BorderRadius {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.top_left.resolve_viewport_lengths(basis);
        self.top_right.resolve_viewport_lengths(basis);
        self.bottom_right.resolve_viewport_lengths(basis);
        self.bottom_left.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for CornerRadius {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.horizontal.resolve_viewport_lengths(basis);
        self.vertical.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for CornerRadiusComponent {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.value.resolve_viewport_lengths(basis);
    }
}
