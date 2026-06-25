use super::*;

#[derive(Debug, Clone, Copy)]
pub(crate) enum BorderEdge {
    Top,
    Right,
    Bottom,
    Left,
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_border_side(
    rects: &mut Vec<RenderedRect>,
    paths: &mut Vec<RenderedPath>,
    edge: BorderEdge,
    x: f32,
    top: f32,
    width: f32,
    height: f32,
    border: UsedBorderSide,
) {
    if !border.is_visible() {
        return;
    }
    if border.style == BorderStyle::Double && border.used_width >= 3.0 {
        paint_double_border_side(rects, paths, edge, x, top, width, height, border);
        return;
    }

    let (axis_start, axis_length, cross_start, cross_width, horizontal) =
        border_side_geometry(edge, x, top, width, height, border.used_width);
    match border.style {
        BorderStyle::Dashed => paint_dashed_border_side(
            rects,
            axis_start,
            axis_length,
            cross_start,
            cross_width,
            horizontal,
            border.used_width,
            border.color,
        ),
        BorderStyle::Dotted => paint_dotted_border_side(
            paths,
            axis_start,
            axis_length,
            cross_start,
            cross_width,
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
    color: Color,
) -> Color {
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
    color: Color,
) -> (Color, Color) {
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
    color: Color,
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

fn lighten_border_color(color: Color) -> Color {
    let (hue, mut saturation, mut value) = rgb_to_hsv(color.r, color.g, color.b);
    value = 1.0 - (1.0 - value) / 1.5;
    if saturation > 0.0 {
        saturation = 1.0 - (1.0 - saturation) / 1.25;
    }
    let (r, g, b) = hsv_to_rgb(hue, saturation, value);
    Color {
        r,
        g,
        b,
        a: color.a,
    }
}

fn darken_border_color(color: Color) -> Color {
    let (hue, saturation, value) = rgb_to_hsv(color.r, color.g, color.b);
    let (r, g, b) = hsv_to_rgb(hue, saturation / 1.25, value / 1.5);
    Color {
        r,
        g,
        b,
        a: color.a,
    }
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_double_border_side(
    rects: &mut Vec<RenderedRect>,
    paths: &mut Vec<RenderedPath>,
    edge: BorderEdge,
    x: f32,
    top: f32,
    width: f32,
    height: f32,
    border: UsedBorderSide,
) {
    let stripe = (border.used_width / 3.0).max(1.0);
    let stripe_border = UsedBorderSide {
        specified_width: stripe,
        used_width: stripe,
        style: BorderStyle::Solid,
        color: border.color,
    };
    match edge {
        BorderEdge::Top => {
            paint_border_side(rects, paths, edge, x, top, width, height, stripe_border);
            paint_border_side(
                rects,
                paths,
                edge,
                x,
                top - border.used_width + stripe,
                width,
                height,
                stripe_border,
            );
        }
        BorderEdge::Bottom => {
            paint_border_side(rects, paths, edge, x, top, width, height, stripe_border);
            paint_border_side(
                rects,
                paths,
                edge,
                x,
                top + border.used_width - stripe,
                width,
                height,
                stripe_border,
            );
        }
        BorderEdge::Right => {
            paint_border_side(rects, paths, edge, x, top, width, height, stripe_border);
            paint_border_side(
                rects,
                paths,
                edge,
                x + border.used_width - stripe,
                top,
                width,
                height,
                stripe_border,
            );
        }
        BorderEdge::Left => {
            paint_border_side(rects, paths, edge, x, top, width, height, stripe_border);
            paint_border_side(
                rects,
                paths,
                edge,
                x - border.used_width + stripe,
                top,
                width,
                height,
                stripe_border,
            );
        }
    }
}

pub(crate) fn border_side_geometry(
    edge: BorderEdge,
    x: f32,
    top: f32,
    width: f32,
    height: f32,
    border_width: f32,
) -> (f32, f32, f32, f32, bool) {
    match edge {
        BorderEdge::Top => (x, width, top - border_width, border_width, true),
        BorderEdge::Bottom => (x, width, top - height, border_width, true),
        BorderEdge::Right => (
            top - height,
            height,
            x + width - border_width,
            border_width,
            false,
        ),
        BorderEdge::Left => (top - height, height, x, border_width, false),
    }
}
