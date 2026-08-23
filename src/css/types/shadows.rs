use super::*;

/// Computed CSS `box-shadow` layer.
///
/// CSS Backgrounds and Borders Level 3 defines each shadow as a box-shaped
/// image outside or inside the border box, with the same geometry as the
/// border box unless offset, blur, or spread modifies it:
/// <https://www.w3.org/TR/css-backgrounds-3/#box-shadow>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoxShadow {
    pub(crate) color: BoxShadowColor,
    pub(crate) offset_x: ComputedLengthPercentage,
    pub(crate) offset_y: ComputedLengthPercentage,
    pub(crate) blur_radius: ComputedLengthPercentage,
    pub(crate) spread: ComputedLengthPercentage,
    pub(crate) inset: bool,
}

impl BoxShadow {
    /// Apply CSS `zoom` to the fixed components of one shadow layer.
    ///
    /// Percentages remain relative to the already zoomed box, while the
    /// length component of each shadow metric is multiplied at the used-value
    /// boundary.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        self.offset_x.scale_fixed_length_components(factor);
        self.offset_y.scale_fixed_length_components(factor);
        self.blur_radius.scale_fixed_length_components(factor);
        self.spread.scale_fixed_length_components(factor);
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.offset_x.resolve_font_metric_lengths(ch_advance);
        self.offset_y.resolve_font_metric_lengths(ch_advance);
        self.blur_radius.resolve_font_metric_lengths(ch_advance);
        self.spread.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.offset_x.resolve_root_font_metric_lengths(basis);
        self.offset_y.resolve_root_font_metric_lengths(basis);
        self.blur_radius.resolve_root_font_metric_lengths(basis);
        self.spread.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.offset_x.requires_ch_advance()
            || self.offset_y.requires_ch_advance()
            || self.blur_radius.requires_ch_advance()
            || self.spread.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.offset_x.requires_root_font_metrics()
            || self.offset_y.requires_root_font_metrics()
            || self.blur_radius.requires_root_font_metrics()
            || self.spread.requires_root_font_metrics()
    }
}

/// CssColor component of a computed CSS `box-shadow`.
///
/// CSS CssColor defines `currentColor` as the element's own computed `color`.
/// `box-shadow` is not inherited, but `currentColor` still resolves against
/// the box that paints the shadow:
/// <https://www.w3.org/TR/css-color-3/#currentcolor>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BoxShadowColor {
    CurrentColor,
    CssColor(CssColor),
}

impl BoxShadowColor {
    pub(crate) fn resolve(self, current_color: CssColor) -> CssColor {
        match self {
            Self::CurrentColor => current_color,
            Self::CssColor(color) => color,
        }
    }
}

/// Computed CSS `text-shadow` layer.
///
/// CSS Text Decoration Level 4 follows the box-shadow grammar but applies
/// each shadow layer to text and decorations:
/// <https://drafts.csswg.org/css-text-decor-4/#text-shadow-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextShadow {
    pub(crate) color: TextShadowColor,
    pub(crate) offset_x: ComputedLengthPercentage,
    pub(crate) offset_y: ComputedLengthPercentage,
    pub(crate) blur_radius: ComputedLengthPercentage,
    pub(crate) spread: ComputedLengthPercentage,
    pub(crate) inset: bool,
}

impl TextShadow {
    /// Apply CSS `zoom` to the fixed components of one shadow layer.
    ///
    /// Percentages remain relative to the already zoomed text metrics, while
    /// the length component of each shadow metric is multiplied at the
    /// used-value boundary.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        self.offset_x.scale_fixed_length_components(factor);
        self.offset_y.scale_fixed_length_components(factor);
        self.blur_radius.scale_fixed_length_components(factor);
        self.spread.scale_fixed_length_components(factor);
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.offset_x.resolve_font_metric_lengths(ch_advance);
        self.offset_y.resolve_font_metric_lengths(ch_advance);
        self.blur_radius.resolve_font_metric_lengths(ch_advance);
        self.spread.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.offset_x.resolve_root_font_metric_lengths(basis);
        self.offset_y.resolve_root_font_metric_lengths(basis);
        self.blur_radius.resolve_root_font_metric_lengths(basis);
        self.spread.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.offset_x.requires_ch_advance()
            || self.offset_y.requires_ch_advance()
            || self.blur_radius.requires_ch_advance()
            || self.spread.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.offset_x.requires_root_font_metrics()
            || self.offset_y.requires_root_font_metrics()
            || self.blur_radius.requires_root_font_metrics()
            || self.spread.requires_root_font_metrics()
    }
}

/// CssColor component of a computed CSS `text-shadow`.
///
/// CSS CssColor defines `currentColor` as the element's own computed `color`.
/// Since `text-shadow` inherits, `currentColor` must remain symbolic until the
/// inheriting element paints the shadow:
/// <https://www.w3.org/TR/css-color-3/#currentcolor>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TextShadowColor {
    CurrentColor,
    CssColor(CssColor),
}

impl TextShadowColor {
    pub(crate) fn resolve(self, current_color: CssColor) -> CssColor {
        match self {
            Self::CurrentColor => current_color,
            Self::CssColor(color) => color,
        }
    }
}

impl ResolveViewportLengths for BoxShadow {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.offset_x.resolve_viewport_lengths(basis);
        self.offset_y.resolve_viewport_lengths(basis);
        self.blur_radius.resolve_viewport_lengths(basis);
        self.spread.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for TextShadow {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.offset_x.resolve_viewport_lengths(basis);
        self.offset_y.resolve_viewport_lengths(basis);
        self.blur_radius.resolve_viewport_lengths(basis);
        self.spread.resolve_viewport_lengths(basis);
    }
}
