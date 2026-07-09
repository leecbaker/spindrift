use super::*;

/// A used border side for layout and painting.
///
/// CSS Backgrounds and Borders defines `border-style: none` and `hidden` as
/// producing a used border width of zero, while the computed border width
/// remains available for cascade/defaulting and table conflict resolution:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-style> and
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-width>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UsedBorderSide {
    pub(crate) specified_width: f32,
    pub(crate) used_width: f32,
    pub(crate) style: BorderStyle,
    pub(crate) color: Color,
}

impl UsedBorderSide {
    /// Builds a side from computed border longhands.
    ///
    /// CSS Backgrounds and Borders resolves the used width from the computed
    /// width and style:
    /// <https://www.w3.org/TR/css-backgrounds-3/#border-width>.
    pub(crate) fn new(specified_width: f32, style: BorderStyle, color: Color) -> Self {
        Self {
            specified_width: specified_width.max(0.0),
            used_width: used_border_side_width(specified_width, style),
            style,
            color,
        }
    }

    /// Return whether this side produces visible border paint.
    ///
    /// CSS Color defines `transparent` as transparent black, so a transparent
    /// border still contributes its used width to layout but emits no visible
    /// paint:
    /// <https://www.w3.org/TR/css-color-4/#transparent-color>.
    pub(crate) fn is_visible(self) -> bool {
        self.used_width > 0.0 && !self.style.suppresses_used_width() && self.color.is_visible()
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
            top: self.top.used_width,
            right: self.right.used_width,
            bottom: self.bottom.used_width,
            left: self.left.used_width,
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
            style.border_widths.top,
            style.border_styles.top,
            style.border_colors.top,
        ),
        right: UsedBorderSide::new(
            style.border_widths.right,
            style.border_styles.right,
            style.border_colors.right,
        ),
        bottom: UsedBorderSide::new(
            style.border_widths.bottom,
            style.border_styles.bottom,
            style.border_colors.bottom,
        ),
        left: UsedBorderSide::new(
            style.border_widths.left,
            style.border_styles.left,
            style.border_colors.left,
        ),
    }
}

pub(crate) fn used_border_widths(style: &ComputedStyle) -> css::Edges {
    used_border(style).widths()
}

pub(crate) fn used_border_side_width(width: f32, style: BorderStyle) -> f32 {
    if style.suppresses_used_width() {
        0.0
    } else {
        width.max(0.0)
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
}
