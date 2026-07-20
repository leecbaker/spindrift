use super::*;

#[derive(Debug, Clone, Copy)]
pub(crate) enum BorderEdge {
    Top,
    Right,
    Bottom,
    Left,
}

/// Physical paint geometry for one CSS border side.
///
/// This keeps the side's paint rectangle and its dash-pattern orientation
/// together: <https://www.w3.org/TR/css-backgrounds-3/#border-style>.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BorderSideGeometry {
    pub(crate) rect: PaintRect,
    pub(crate) horizontal: bool,
}

impl BorderSideGeometry {
    pub(crate) fn from_axis_cross(
        axis_start: f32,
        axis_length: f32,
        cross_start: f32,
        cross_width: f32,
        horizontal: bool,
    ) -> Self {
        let rect = if horizontal {
            paint_space_rect(axis_start, cross_start, axis_length, cross_width)
        } else {
            paint_space_rect(cross_start, axis_start, cross_width, axis_length)
        };
        Self { rect, horizontal }
    }

    pub(crate) fn axis_start(self) -> f32 {
        if self.horizontal {
            self.rect.origin.x
        } else {
            self.rect.origin.y
        }
    }

    pub(crate) fn axis_length(self) -> f32 {
        if self.horizontal {
            self.rect.size.width
        } else {
            self.rect.size.height
        }
    }

    pub(crate) fn cross_start(self) -> f32 {
        if self.horizontal {
            self.rect.origin.y
        } else {
            self.rect.origin.x
        }
    }

    pub(crate) fn cross_width(self) -> f32 {
        if self.horizontal {
            self.rect.size.height
        } else {
            self.rect.size.width
        }
    }
}
pub(crate) fn paint_border_side(
    rects: &mut Vec<RenderedRect>,
    paths: &mut Vec<RenderedPath>,
    edge: BorderEdge,
    rect: PageTopRect,
    border: UsedBorderSide,
) {
    if !border.is_visible() {
        return;
    }
    if border.style == BorderStyle::Double
        && DoubleBorderBands::for_used_width(border.used_width).is_some()
    {
        paint_double_border_side(rects, paths, edge, rect, border);
        return;
    }

    let used_width = border.used_width.get();
    let geometry = border_side_geometry(edge, rect, used_width);
    let axis_start = geometry.axis_start();
    let axis_length = geometry.axis_length();
    let cross_start = geometry.cross_start();
    let cross_width = geometry.cross_width();
    let horizontal = geometry.horizontal;
    match border.style {
        BorderStyle::Dashed => paint_dashed_border_side(
            rects,
            axis_start,
            axis_length,
            cross_start,
            PaintStrokeWidth::new(cross_width),
            horizontal,
            PaintStrokeWidth::new(used_width),
            border.color,
        ),
        BorderStyle::Dotted => paint_dotted_border_side(
            paths,
            axis_start,
            axis_length,
            cross_start,
            PaintStrokeWidth::new(cross_width),
            horizontal,
            border.color,
        ),
        BorderStyle::Inset | BorderStyle::Outset => push_border_rect(
            rects,
            axis_start,
            axis_length,
            cross_start,
            cross_width,
            horizontal,
            inset_outset_border_color(border.style, edge, border.color),
        ),
        BorderStyle::Groove | BorderStyle::Ridge => paint_groove_ridge_border_side(
            rects,
            edge,
            axis_start,
            axis_length,
            cross_start,
            cross_width,
            horizontal,
            border.style,
            border.color,
        ),
        _ => push_border_rect(
            rects,
            axis_start,
            axis_length,
            cross_start,
            cross_width,
            horizontal,
            border.color,
        ),
    }
}

/// Return side-adjusted colors for CSS 3D border styles.
///
/// CSS Backgrounds and Borders defines `inset`, `outset`, `groove`, and
/// `ridge` as colors that are darkened or lightened depending on side. The
/// exact color adjustment is not normatively specified; this mirrors
/// WeasyPrint's HSV-based approach for parity:
/// <https://www.w3.org/TR/css-backgrounds-3/#valdef-border-style-groove>.
pub(crate) fn inset_outset_border_color(
    style: BorderStyle,
    edge: BorderEdge,
    color: CssColor,
) -> CssColor {
    let top_or_left = matches!(edge, BorderEdge::Top | BorderEdge::Left);
    let lighten_side = top_or_left ^ (style == BorderStyle::Inset);
    if lighten_side {
        lighten_border_color(color)
    } else {
        darken_border_color(color)
    }
}

pub(crate) fn groove_ridge_border_colors(
    style: BorderStyle,
    edge: BorderEdge,
    color: CssColor,
) -> (CssColor, CssColor) {
    let top_or_left = matches!(edge, BorderEdge::Top | BorderEdge::Left);
    let outer_light = top_or_left ^ (style == BorderStyle::Ridge);
    if outer_light {
        (lighten_border_color(color), darken_border_color(color))
    } else {
        (darken_border_color(color), lighten_border_color(color))
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_groove_ridge_border_side(
    rects: &mut Vec<RenderedRect>,
    edge: BorderEdge,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: f32,
    horizontal: bool,
    style: BorderStyle,
    color: CssColor,
) {
    let (outer_color, inner_color) = groove_ridge_border_colors(style, edge, color);
    let half = cross_width / 2.0;
    let (outer_cross, inner_cross) = outer_inner_cross_positions(edge, cross_start, cross_width);
    push_border_rect(
        rects,
        axis_start,
        axis_length,
        outer_cross,
        half,
        horizontal,
        outer_color,
    );
    push_border_rect(
        rects,
        axis_start,
        axis_length,
        inner_cross,
        cross_width - half,
        horizontal,
        inner_color,
    );
}

fn outer_inner_cross_positions(edge: BorderEdge, cross_start: f32, cross_width: f32) -> (f32, f32) {
    let half = cross_width / 2.0;
    match edge {
        BorderEdge::Top | BorderEdge::Right => (cross_start + half, cross_start),
        BorderEdge::Bottom | BorderEdge::Left => (cross_start, cross_start + half),
    }
}

fn lighten_border_color(color: CssColor) -> CssColor {
    // CSS2's inset/outset border shading is an sRGB-era operation. Keep it
    // at the explicit legacy paint boundary until it gains defined CSS CssColor
    // interpolation semantics.
    let color = crate::css::color_to_predefined_rgb(color, crate::css::CssColorSpace::Srgb)
        .expect("sRGB is a predefined CSS RGB space");
    let (hue, mut saturation, mut value) = rgb_to_hsv(
        color.components()[0],
        color.components()[1],
        color.components()[2],
    );
    value = 1.0 - (1.0 - value) / 1.5;
    if saturation > 0.0 {
        saturation = 1.0 - (1.0 - saturation) / 1.25;
    }
    let (r, g, b) = hsv_to_rgb(hue, saturation, value);
    CssColor::srgb(r, g, b, color.alpha())
}

fn darken_border_color(color: CssColor) -> CssColor {
    let color = crate::css::color_to_predefined_rgb(color, crate::css::CssColorSpace::Srgb)
        .expect("sRGB is a predefined CSS RGB space");
    let (hue, saturation, value) = rgb_to_hsv(
        color.components()[0],
        color.components()[1],
        color.components()[2],
    );
    let (r, g, b) = hsv_to_rgb(hue, saturation / 1.25, value / 1.5);
    CssColor::srgb(r, g, b, color.alpha())
}

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = max - min;
    let hue = if chroma == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / chroma).rem_euclid(6.0) / 6.0
    } else if max == g {
        (((b - r) / chroma) + 2.0) / 6.0
    } else {
        (((r - g) / chroma) + 4.0) / 6.0
    };
    let saturation = if max == 0.0 { 0.0 } else { chroma / max };
    (hue, saturation, max)
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> (f32, f32, f32) {
    if saturation == 0.0 {
        return (value, value, value);
    }
    let hue = (hue * 6.0).rem_euclid(6.0);
    let sector = hue.floor();
    let fraction = hue - sector;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * fraction);
    let t = value * (1.0 - saturation * (1.0 - fraction));
    match sector as u8 {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    }
}
pub(crate) fn paint_double_border_side(
    rects: &mut Vec<RenderedRect>,
    paths: &mut Vec<RenderedPath>,
    edge: BorderEdge,
    rect: PageTopRect,
    border: UsedBorderSide,
) {
    let bands = DoubleBorderBands::for_used_width(border.used_width)
        .expect("double border paint requires a double-band width");
    let stripe = bands.stripe.get();
    let used_width = border.used_width.get();
    let stripe_border = UsedBorderSide {
        specified_width: bands.stripe,
        used_width: bands.stripe,
        style: BorderStyle::Solid,
        color: border.color,
    };
    match edge {
        BorderEdge::Top => {
            paint_border_side(rects, paths, edge, rect, stripe_border);
            paint_border_side(
                rects,
                paths,
                edge,
                PageTopRect::new(
                    rect.x(),
                    rect.top_y() - used_width + stripe,
                    rect.width(),
                    rect.height(),
                ),
                stripe_border,
            );
        }
        BorderEdge::Bottom => {
            paint_border_side(rects, paths, edge, rect, stripe_border);
            paint_border_side(
                rects,
                paths,
                edge,
                PageTopRect::new(
                    rect.x(),
                    rect.top_y() + used_width - stripe,
                    rect.width(),
                    rect.height(),
                ),
                stripe_border,
            );
        }
        BorderEdge::Right => {
            paint_border_side(rects, paths, edge, rect, stripe_border);
            paint_border_side(
                rects,
                paths,
                edge,
                PageTopRect::new(
                    rect.x() - used_width + stripe,
                    rect.top_y(),
                    rect.width(),
                    rect.height(),
                ),
                stripe_border,
            );
        }
        BorderEdge::Left => {
            paint_border_side(rects, paths, edge, rect, stripe_border);
            paint_border_side(
                rects,
                paths,
                edge,
                PageTopRect::new(
                    rect.x() + used_width - stripe,
                    rect.top_y(),
                    rect.width(),
                    rect.height(),
                ),
                stripe_border,
            );
        }
    }
}

pub(crate) fn border_side_geometry(
    edge: BorderEdge,
    rect: PageTopRect,
    border_width: f32,
) -> BorderSideGeometry {
    match edge {
        BorderEdge::Top => BorderSideGeometry::from_axis_cross(
            rect.x(),
            rect.width(),
            rect.top_y() - border_width,
            border_width,
            true,
        ),
        BorderEdge::Bottom => BorderSideGeometry::from_axis_cross(
            rect.x(),
            rect.width(),
            rect.top_y() - rect.height(),
            border_width,
            true,
        ),
        BorderEdge::Right => BorderSideGeometry::from_axis_cross(
            rect.top_y() - rect.height(),
            rect.height(),
            rect.x() + rect.width() - border_width,
            border_width,
            false,
        ),
        BorderEdge::Left => BorderSideGeometry::from_axis_cross(
            rect.top_y() - rect.height(),
            rect.height(),
            rect.x(),
            border_width,
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_side_geometry_maps_each_physical_edge_to_paint_rect() {
        let rect = PageTopRect::new(10.0, 100.0, 50.0, 30.0);
        let top = border_side_geometry(BorderEdge::Top, rect, 4.0);
        let right = border_side_geometry(BorderEdge::Right, rect, 4.0);
        let bottom = border_side_geometry(BorderEdge::Bottom, rect, 4.0);
        let left = border_side_geometry(BorderEdge::Left, rect, 4.0);

        assert_eq!(top.rect, paint_space_rect(10.0, 96.0, 50.0, 4.0));
        assert_eq!(right.rect, paint_space_rect(56.0, 70.0, 4.0, 30.0));
        assert_eq!(bottom.rect, paint_space_rect(10.0, 70.0, 50.0, 4.0));
        assert_eq!(left.rect, paint_space_rect(10.0, 70.0, 4.0, 30.0));
        assert!(top.horizontal && bottom.horizontal);
        assert!(!right.horizontal && !left.horizontal);
    }

    #[test]
    fn double_top_border_keeps_inner_stripe_on_the_same_page_top_rect_edge() {
        let border =
            UsedBorderSide::new(layout_pt(6.0), BorderStyle::Double, CssColor::new(0, 0, 0));
        let mut rects = Vec::new();
        let mut paths = Vec::new();
        paint_double_border_side(
            &mut rects,
            &mut paths,
            BorderEdge::Top,
            PageTopRect::new(10.0, 100.0, 50.0, 30.0),
            border,
        );

        assert!(paths.is_empty());
        assert_eq!(
            rects
                .into_iter()
                .map(|rect| rect.paint_rect())
                .collect::<Vec<_>>(),
            vec![
                paint_space_rect(10.0, 98.0, 50.0, 2.0),
                paint_space_rect(10.0, 94.0, 50.0, 2.0),
            ]
        );
    }

    #[test]
    fn medium_double_border_paints_two_css_pixel_stripes() {
        let border = UsedBorderSide::new(
            layout_pt(3.0 * css::CSS_PX_TO_PT),
            BorderStyle::Double,
            CssColor::BLACK,
        );
        let mut rects = Vec::new();
        let mut paths = Vec::new();
        paint_border_side(
            &mut rects,
            &mut paths,
            BorderEdge::Top,
            PageTopRect::new(10.0, 100.0, 50.0, 30.0),
            border,
        );

        assert!(paths.is_empty());
        assert_eq!(
            rects
                .into_iter()
                .map(|rect| rect.paint_rect())
                .collect::<Vec<_>>(),
            vec![
                paint_space_rect(10.0, 99.25, 50.0, 0.75),
                paint_space_rect(10.0, 97.75, 50.0, 0.75),
            ]
        );
    }
}
