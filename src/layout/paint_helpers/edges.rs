use super::*;

pub(crate) fn paint_border_edges(
    rects: &mut Vec<RenderedRect>,
    paths: &mut Vec<RenderedPath>,
    x: f32,
    top: f32,
    width: f32,
    height: f32,
    style: &ComputedStyle,
) {
    let borders = used_border(style);
    paint_border_side(
        rects,
        paths,
        BorderEdge::Top,
        x,
        top,
        width,
        height,
        borders.top,
    );
    paint_border_side(
        rects,
        paths,
        BorderEdge::Right,
        x,
        top,
        width,
        height,
        borders.right,
    );
    paint_border_side(
        rects,
        paths,
        BorderEdge::Bottom,
        x,
        top,
        width,
        height,
        borders.bottom,
    );
    paint_border_side(
        rects,
        paths,
        BorderEdge::Left,
        x,
        top,
        width,
        height,
        borders.left,
    );
}
