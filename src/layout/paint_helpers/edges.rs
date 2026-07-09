use super::*;

pub(crate) fn paint_border_edges(
    rects: &mut Vec<RenderedRect>,
    paths: &mut Vec<RenderedPath>,
    rect: PageTopRect,
    style: &ComputedStyle,
) {
    let borders = used_border(style);
    paint_border_side(rects, paths, BorderEdge::Top, rect, borders.top);
    paint_border_side(rects, paths, BorderEdge::Right, rect, borders.right);
    paint_border_side(rects, paths, BorderEdge::Bottom, rect, borders.bottom);
    paint_border_side(rects, paths, BorderEdge::Left, rect, borders.left);
}
