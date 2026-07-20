use super::*;

pub(crate) fn paint_border_edges(
    rects: &mut Vec<RenderedRect>,
    paths: &mut Vec<RenderedPath>,
    rect: PageTopRect,
    style: &ComputedStyle,
) {
    let borders = used_border(style);
    paint_border_side(
        rects,
        paths,
        BorderEdge::Top,
        patterned_border_side_rect(rect, BorderEdge::Top, borders),
        borders.top,
    );
    paint_border_side(
        rects,
        paths,
        BorderEdge::Right,
        patterned_border_side_rect(rect, BorderEdge::Right, borders),
        borders.right,
    );
    paint_border_side(
        rects,
        paths,
        BorderEdge::Bottom,
        patterned_border_side_rect(rect, BorderEdge::Bottom, borders),
        borders.bottom,
    );
    paint_border_side(
        rects,
        paths,
        BorderEdge::Left,
        patterned_border_side_rect(rect, BorderEdge::Left, borders),
        borders.left,
    );
}

/// Collapse a sub-half-CSS-pixel content extent for one patterned border's
/// paint geometry.
///
/// Patterned borders intentionally have implementation-defined dash placement.
/// Their visible far edge must nevertheless coincide with the zero-sized-box
/// case when the box's content extent is below half a CSS pixel.  Keep the
/// fractional layout box intact and adjust only the dashed or dotted side's
/// paint geometry, preserving the long-axis dash distribution.
/// <https://www.w3.org/TR/css-backgrounds-3/#border-style>
fn patterned_border_side_rect(
    rect: PageTopRect,
    edge: BorderEdge,
    borders: UsedBorder,
) -> PageTopRect {
    let side = match edge {
        BorderEdge::Top => borders.top,
        BorderEdge::Right => borders.right,
        BorderEdge::Bottom => borders.bottom,
        BorderEdge::Left => borders.left,
    };
    if !matches!(side.style, BorderStyle::Dashed | BorderStyle::Dotted) {
        return rect;
    }

    let (content_extent, collapsed_rect) = match edge {
        BorderEdge::Top | BorderEdge::Bottom => {
            let content_height =
                rect.height() - borders.top.used_width.get() - borders.bottom.used_width.get();
            (
                content_height,
                PageTopRect::new(
                    rect.x(),
                    rect.top_y(),
                    rect.width(),
                    borders.top.used_width.get() + borders.bottom.used_width.get(),
                ),
            )
        }
        BorderEdge::Right | BorderEdge::Left => {
            let content_width =
                rect.width() - borders.left.used_width.get() - borders.right.used_width.get();
            (
                content_width,
                PageTopRect::new(
                    rect.x(),
                    rect.top_y(),
                    borders.left.used_width.get() + borders.right.used_width.get(),
                    rect.height(),
                ),
            )
        }
    };
    if (0.0..css::CSS_PX_TO_PT / 2.0).contains(&content_extent) {
        collapsed_rect
    } else {
        rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn borders() -> UsedBorder {
        let none = UsedBorderSide::new(layout_pt(0.0), BorderStyle::None, CssColor::BLACK);
        let dashed = UsedBorderSide::new(
            layout_pt(css::CSS_PX_TO_PT),
            BorderStyle::Dashed,
            CssColor::BLACK,
        );
        UsedBorder {
            top: none,
            right: dashed,
            bottom: none,
            left: none,
        }
    }

    #[test]
    fn patterned_far_edge_ignores_sub_half_css_pixel_content_width() {
        let rect = PageTopRect::new(10.0, 100.0, css::CSS_PX_TO_PT * 1.25, 50.0);

        let adjusted = patterned_border_side_rect(rect, BorderEdge::Right, borders());

        assert_eq!(adjusted.width(), css::CSS_PX_TO_PT);
        assert_eq!(adjusted.x(), rect.x());
        assert_eq!(adjusted.top_y(), rect.top_y());
    }
}
