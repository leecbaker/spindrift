use super::*;
use crate::units::layout_px;

/// A used border side for layout and painting.
///
/// CSS Backgrounds and Borders defines `border-style: none` and `hidden` as
/// producing a used border width of zero, while the computed border width
/// remains available for cascade/defaulting and table conflict resolution:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-style> and
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-width>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UsedBorderSide {
    pub(crate) specified_width: LayoutLength,
    pub(crate) used_width: LayoutLength,
    pub(crate) style: BorderStyle,
    pub(crate) color: CssColor,
}

impl UsedBorderSide {
    /// Builds a side from computed border longhands.
    ///
    /// CSS Backgrounds and Borders resolves the used width from the computed
    /// width and style:
    /// <https://www.w3.org/TR/css-backgrounds-3/#border-width>.
    pub(crate) fn new(specified_width: LayoutLength, style: BorderStyle, color: CssColor) -> Self {
        Self {
            specified_width: layout_pt(specified_width.get().max(0.0)),
            used_width: used_border_side_width(specified_width, style),
            style,
            color,
        }
    }

    /// Return whether this side produces visible border paint.
    ///
    /// CSS CssColor defines `transparent` as transparent black, so a transparent
    /// border still contributes its used width to layout but emits no visible
    /// paint:
    /// <https://www.w3.org/TR/css-color-4/#transparent-color>.
    pub(crate) fn is_visible(self) -> bool {
        self.used_width > layout_pt(0.0)
            && !self.style.suppresses_used_width()
            && self.color.is_visible()
    }
}

/// Used physical border sides for a border box.
///
/// CSS Box Model and CSS Backgrounds and Borders define physical border edges
/// as part of the box model and resolve their used widths before layout and
/// painting consume them:
/// <https://www.w3.org/TR/css-box-3/#box-model> and
/// <https://www.w3.org/TR/css-backgrounds-3/#borders>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UsedBorder {
    pub(crate) top: UsedBorderSide,
    pub(crate) right: UsedBorderSide,
    pub(crate) bottom: UsedBorderSide,
    pub(crate) left: UsedBorderSide,
}

impl UsedBorder {
    pub(crate) fn widths(self) -> css::Edges {
        css::Edges {
            top: self.top.used_width.get(),
            right: self.right.used_width.get(),
            bottom: self.bottom.used_width.get(),
            left: self.left.used_width.get(),
        }
    }
}

/// Resolves computed border longhands to used border sides.
///
/// CSS Backgrounds and Borders makes the style determine whether the used
/// border width is zero; keeping this as a single model avoids different
/// layout, painting, and table-collapse interpretations:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-style>.
pub(crate) fn used_border(style: &ComputedStyle) -> UsedBorder {
    UsedBorder {
        top: UsedBorderSide::new(
            layout_pt(style.border_widths.top),
            style.border_styles.top,
            style.border_colors.top,
        ),
        right: UsedBorderSide::new(
            layout_pt(style.border_widths.right),
            style.border_styles.right,
            style.border_colors.right,
        ),
        bottom: UsedBorderSide::new(
            layout_pt(style.border_widths.bottom),
            style.border_styles.bottom,
            style.border_colors.bottom,
        ),
        left: UsedBorderSide::new(
            layout_pt(style.border_widths.left),
            style.border_styles.left,
            style.border_colors.left,
        ),
    }
}

pub(crate) fn used_border_widths(style: &ComputedStyle) -> css::Edges {
    used_border(style).widths()
}

pub(crate) fn used_border_side_width(width: LayoutLength, style: BorderStyle) -> LayoutLength {
    if style.suppresses_used_width() {
        layout_pt(0.0)
    } else {
        layout_pt(width.get().max(0.0))
    }
}

/// Typed paint metrics for a `double` border.
///
/// A double border is painted as two solid lines separated by an equal-width
/// gap. The rendering fallback below three CSS pixels is retained for
/// compatibility, but the cutoff is expressed as a CSS length before the
/// layout-to-paint boundary.
/// <https://www.w3.org/TR/css-backgrounds-3/#border-style>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DoubleBorderBands {
    pub(crate) stripe: LayoutLength,
}

impl DoubleBorderBands {
    pub(crate) fn for_used_width(used_width: LayoutLength) -> Option<Self> {
        (used_width >= layout_px(3.0)).then(|| Self {
            stripe: layout_pt(used_width.get() / 3.0),
        })
    }
}

/// Return the maximum used border width as a semantic layout length.
///
/// CSS Backgrounds and Borders suppresses the used width of `none` and
/// `hidden` border styles. Callers use this to decide whether a box has a
/// visible border, but the value remains a CSS length until a paint or
/// geometry boundary explicitly needs points:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-style>.
pub(crate) fn used_border_width(style: &ComputedStyle) -> LayoutLength {
    layout_pt(max_edge(used_border_widths(style)))
}

pub(crate) fn max_edge(edges: css::Edges) -> f32 {
    edges.top.max(edges.right).max(edges.bottom).max(edges.left)
}

pub(crate) fn horizontal_border_width(style: &ComputedStyle) -> f32 {
    let borders = used_border_widths(style);
    borders.left + borders.right
}

pub(crate) fn vertical_border_width(style: &ComputedStyle) -> f32 {
    let borders = used_border_widths(style);
    borders.top + borders.bottom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_border_width_preserves_layout_length_type() {
        let mut style = ComputedStyle::initial();
        style.border_widths.left = 3.0;
        style.border_styles.left = BorderStyle::Solid;

        let width: LayoutLength = used_border_width(&style);

        assert_eq!(width, layout_pt(3.0));
    }

    #[test]
    fn double_border_bands_use_css_pixel_cutoff_and_equal_layout_bands() {
        assert_eq!(DoubleBorderBands::for_used_width(layout_px(2.0)), None);

        let bands = DoubleBorderBands::for_used_width(layout_px(3.0)).unwrap();
        assert_eq!(bands.stripe, layout_px(1.0));
        assert_eq!(bands.stripe.get() * 3.0, layout_px(3.0).get());
    }
}
