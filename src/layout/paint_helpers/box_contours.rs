//! Resolution of CSS box-content contours at the used-value paint boundary.
//!
//! This is intentionally the single bridge from CSS border/padding/content
//! geometry into retained paint contours. Layout algorithms keep ownership of
//! their overflow axes and fragment bounds; they do not reproduce corner or
//! `border-shape` arithmetic.

use super::*;
use crate::document::paint::contours::{
    BoxContentContour, OverflowClipEffect, ResolvedBoxContentClip,
};
use crate::document::paint::geometry::AxisSelectivePaintClip;

/// The used overflow clip edge for one principal box.
///
/// This keeps the exact contour and conservative bounds coupled to the
/// physical overflow-axis semantics that selected them. Formatters must not
/// reconstruct a padding rectangle after this boundary.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct ResolvedOverflowClipEdge {
    pub(in crate::layout) clip: ResolvedBoxContentClip,
    pub(in crate::layout) clips_x: bool,
    pub(in crate::layout) clips_y: bool,
    pub(in crate::layout) non_scrollable_x: bool,
    pub(in crate::layout) non_scrollable_y: bool,
}

impl ResolvedOverflowClipEdge {
    pub(in crate::layout) fn eager_clip(&self) -> OverflowClip {
        OverflowClip::from_paint_rect_with_axes_and_non_scrollable(
            self.clip.bounds.paint_rect(),
            self.clips_x,
            self.clips_y,
            self.non_scrollable_x,
            self.non_scrollable_y,
        )
    }

    pub(in crate::layout) fn effect(&self) -> OverflowClipEffect {
        if self.clips_x && self.clips_y {
            OverflowClipEffect::Contoured(self.clip.clone())
        } else {
            OverflowClipEffect::AxisSelective(AxisSelectivePaintClip::new(
                self.clip.bounds,
                self.clips_x,
                self.clips_y,
            ))
        }
    }
}

/// Whether a box needs a retained exact contour instead of an eager
/// rectangular primitive clip.  A rectangle is intentionally left on the
/// established formatter path: it can safely trim primitive geometry early,
/// while a curved/path edge must wrap descendants after their paint is
/// captured.
pub(in crate::layout) fn box_content_contour_is_non_rectangular(style: &ComputedStyle) -> bool {
    !style.border_radius.clone().is_zero() || !matches!(style.border_shape, css::BorderShape::None)
}

/// Resolve an exact content contour and its conservative rectangular bounds.
///
/// CSS Borders derives inner radii by subtracting physical border and padding
/// insets from the already-normalized outer radii. Retaining that used result
/// here keeps normal flow, positioned replay, raster images, and SVG on the
/// same geometry.
pub(in crate::layout) fn resolve_replaced_content_contour(
    border_rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
) -> Option<ResolvedBoxContentClip> {
    let reference_box = css::BackgroundBox::Content;
    let outset = 0.0;
    let area = background_rect_area_for_box(border_rect, style, border_insets, reference_box);
    let bounds = PaintClip::new(
        area.origin.x - outset,
        area.origin.y - outset,
        (area.size.width + outset * 2.0).max(0.0),
        (area.size.height + outset * 2.0).max(0.0),
    );
    if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return Some(ResolvedBoxContentClip {
            bounds,
            contour: BoxContentContour::Empty,
        });
    }

    // A paired border shape supplies its inner contour. A single path is
    // centered on its stroke, so this is its inner visible edge.
    if let Some(shape) = border_shape_inner_content_clip(border_rect, style, border_insets) {
        let contour = match shape {
            BorderShapeOverflowClip::Path(path) => BoxContentContour::Path(path),
            BorderShapeOverflowClip::Empty => BoxContentContour::Empty,
        };
        return Some(ResolvedBoxContentClip { bounds, contour });
    }

    let rounded = rounded_clip_rect_for_box_at_edge(
        border_rect,
        style,
        border_insets,
        reference_box,
        bounds.paint_rect(),
    );
    Some(ResolvedBoxContentClip {
        bounds,
        contour: rounded.map_or(BoxContentContour::Rect, BoxContentContour::Rounded),
    })
}

/// Resolve CSS Overflow's used clip edge, including scroll-container and
/// paint-containment adjustments.
///
/// A scroll container's edge may never extend outside its padding box, and a
/// `border-box` offset is ignored there. Paint containment converts otherwise
/// visible axes into non-scrollable clipping axes without changing the chosen
/// edge.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-margin>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn resolve_overflow_clip_edge(
    border_rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    used_overflow: UsedOverflowAxes,
    paint_containment: bool,
    reserved_scrollport: Option<PaintClip>,
) -> Option<ResolvedOverflowClipEdge> {
    let clips_x = used_overflow.clips_x() || paint_containment;
    let clips_y = used_overflow.clips_y() || paint_containment;
    if !clips_x && !clips_y {
        return None;
    }

    let reference_box = match style.overflow_clip_margin.reference_box {
        css::OverflowClipMarginBox::Border => css::BackgroundBox::Border,
        css::OverflowClipMarginBox::Padding => css::BackgroundBox::Padding,
        css::OverflowClipMarginBox::Content => css::BackgroundBox::Content,
    };
    let is_scroll_container = used_overflow.horizontal.is_scroll_container()
        || used_overflow.vertical.is_scroll_container();
    let padding_box = PaintClip::from_paint_rect(background_rect_area_for_box(
        border_rect,
        style,
        border_insets,
        css::BackgroundBox::Padding,
    ));
    let (effective_box, offset) = if is_scroll_container {
        (css::BackgroundBox::Padding, 0.0)
    } else {
        (reference_box, style.overflow_clip_margin.offset.get())
    };
    let area = background_rect_area_for_box(border_rect, style, border_insets, effective_box);
    let requested = PaintClip::new(
        area.origin.x - offset,
        area.origin.y - offset,
        (area.size.width + 2.0 * offset).max(0.0),
        (area.size.height + 2.0 * offset).max(0.0),
    );
    let mut bounds = if is_scroll_container {
        requested
            .intersect(padding_box)
            .unwrap_or_else(|| PaintClip::new(requested.x(), requested.y(), 0.0, 0.0))
    } else {
        requested
    };
    if let Some(scrollport) = reserved_scrollport {
        bounds = bounds
            .intersect(scrollport)
            .unwrap_or_else(|| PaintClip::new(bounds.x(), bounds.y(), 0.0, 0.0));
    }

    let contour = if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        BoxContentContour::Empty
    } else if clips_x != clips_y {
        // A curved contour would also bound the companion `visible` axis.
        BoxContentContour::Rect
    } else if let Some(shape) = border_shape_inner_content_clip(border_rect, style, border_insets) {
        match shape {
            BorderShapeOverflowClip::Path(path) => BoxContentContour::Path(path),
            BorderShapeOverflowClip::Empty => BoxContentContour::Empty,
        }
    } else {
        rounded_clip_rect_for_box_at_edge(
            border_rect,
            style,
            border_insets,
            effective_box,
            bounds.paint_rect(),
        )
        .map_or(BoxContentContour::Rect, BoxContentContour::Rounded)
    };

    Some(ResolvedOverflowClipEdge {
        clip: ResolvedBoxContentClip { bounds, contour },
        clips_x,
        clips_y,
        non_scrollable_x: clips_x
            && (used_overflow.non_scrollable_clip_x()
                || (paint_containment && !used_overflow.horizontal.is_scroll_container())),
        non_scrollable_y: clips_y
            && (used_overflow.non_scrollable_clip_y()
                || (paint_containment && !used_overflow.vertical.is_scroll_container())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(declarations: &str) -> ComputedStyle {
        let mut style = crate::css::default_style_for_tag("div");
        crate::css::apply_declarations(&mut style, &crate::css::parse_declarations(declarations));
        style
    }

    #[test]
    fn padding_and_content_requests_keep_bounds_distinct_from_the_exact_edge() {
        let content_style = style("border: 10pt solid black; padding: 5pt; border-radius: 20pt");
        let border = paint_space_rect(0.0, 0.0, 100.0, 80.0);
        let insets = used_border_widths(&content_style);

        let padding_style =
            style("border: 10pt solid black; padding: 5pt; border-radius: 20pt; overflow:clip");
        let padding = resolve_overflow_clip_edge(
            border,
            &padding_style,
            used_border_widths(&padding_style),
            UsedOverflowAxes::from_style(&padding_style),
            false,
            None,
        )
        .expect("padding contour")
        .clip;
        let content = resolve_replaced_content_contour(border, &content_style, insets)
            .expect("content contour");

        assert_eq!(
            padding.bounds.paint_rect(),
            paint_space_rect(10.0, 10.0, 80.0, 60.0)
        );
        assert_eq!(
            content.bounds.paint_rect(),
            paint_space_rect(15.0, 15.0, 70.0, 50.0)
        );
        assert!(matches!(padding.contour, BoxContentContour::Rounded(_)));
        assert!(matches!(content.contour, BoxContentContour::Rounded(_)));
    }

    #[test]
    fn single_border_shape_resolves_its_inner_stroke_edge() {
        let style = style("border: 10pt solid black; border-shape: circle(50%)");
        let contour = resolve_replaced_content_contour(
            paint_space_rect(0.0, 0.0, 100.0, 100.0),
            &style,
            used_border_widths(&style),
        )
        .expect("shape contour");

        let Some(path) = contour.path_clip() else {
            panic!("a single border shape has an exact inner path");
        };
        // The default half-border geometry is a 45pt circle; a 10pt centered
        // border therefore resolves its inner stroke edge at 40pt.
        let Some(RenderedPathCommand::MoveTo(point)) = path.commands.first() else {
            panic!("circle path must start with a move: {:?}", path.commands);
        };
        assert!((point.x - 90.0).abs() < 0.01, "point={point:?}");
        assert!((point.y - 50.0).abs() < 0.01, "point={point:?}");
    }

    #[test]
    fn signed_offsets_resolve_each_reference_box_and_can_collapse() {
        let border = paint_space_rect(0.0, 0.0, 100.0, 80.0);
        for (declarations, expected) in [
            (
                "border:10pt solid; padding:5pt; overflow:clip; overflow-clip-margin:border-box 10pt",
                paint_space_rect(-10.0, -10.0, 120.0, 100.0),
            ),
            (
                "border:10pt solid; padding:5pt; overflow:clip; overflow-clip-margin:padding-box 10pt",
                paint_space_rect(0.0, 0.0, 100.0, 80.0),
            ),
            (
                "border:10pt solid; padding:5pt; overflow:clip; overflow-clip-margin:content-box 10pt",
                paint_space_rect(5.0, 5.0, 90.0, 70.0),
            ),
        ] {
            let style = style(declarations);
            let edge = resolve_overflow_clip_edge(
                border,
                &style,
                used_border_widths(&style),
                UsedOverflowAxes::from_style(&style),
                false,
                None,
            )
            .expect("clip edge");
            assert_eq!(edge.clip.bounds.paint_rect(), expected);
        }

        let style = style(
            "border:10pt solid; padding:5pt; overflow:clip; overflow-clip-margin:padding-box -50pt",
        );
        let edge = resolve_overflow_clip_edge(
            border,
            &style,
            used_border_widths(&style),
            UsedOverflowAxes::from_style(&style),
            false,
            None,
        )
        .expect("collapsed clip edge");
        assert!(matches!(edge.clip.contour, BoxContentContour::Empty));
    }

    #[test]
    fn scroll_containers_use_the_padding_scrollport_regardless_of_clip_margin() {
        let border = paint_space_rect(0.0, 0.0, 100.0, 80.0);
        for declarations in [
            "border:10pt solid; padding:5pt; overflow:hidden; overflow-clip-margin:border-box 100pt",
            "border:10pt solid; padding:5pt; overflow:hidden; overflow-clip-margin:padding-box 100pt",
            "border:10pt solid; padding:5pt; overflow:hidden; overflow-clip-margin:content-box 100pt",
        ] {
            let style = style(declarations);
            let edge = resolve_overflow_clip_edge(
                border,
                &style,
                used_border_widths(&style),
                UsedOverflowAxes::from_style(&style),
                false,
                None,
            )
            .expect("scroll clip edge");
            assert_eq!(
                edge.clip.bounds.paint_rect(),
                paint_space_rect(10.0, 10.0, 80.0, 60.0)
            );
        }
    }

    #[test]
    fn paint_containment_uses_expanded_edge_and_axis_selective_clips_are_rectangular() {
        let border = paint_space_rect(0.0, 0.0, 100.0, 80.0);
        let contained = style("overflow:visible; overflow-clip-margin:padding-box 5pt");
        let edge = resolve_overflow_clip_edge(
            border,
            &contained,
            css::Edges::ZERO,
            UsedOverflowAxes::from_style(&contained),
            true,
            None,
        )
        .expect("paint containment edge");
        assert!(edge.clips_x && edge.clips_y);
        assert!(edge.non_scrollable_x && edge.non_scrollable_y);
        assert_eq!(
            edge.clip.bounds.paint_rect(),
            paint_space_rect(-5.0, -5.0, 110.0, 90.0)
        );

        let selective = style(
            "overflow-x:clip; overflow-y:visible; border-radius:20pt; overflow-clip-margin:5pt",
        );
        let edge = resolve_overflow_clip_edge(
            border,
            &selective,
            css::Edges::ZERO,
            UsedOverflowAxes::from_style(&selective),
            false,
            None,
        )
        .expect("axis-selective edge");
        assert!(edge.clips_x && !edge.clips_y);
        assert!(matches!(edge.clip.contour, BoxContentContour::Rect));
    }

    #[test]
    fn overflow_radius_uses_the_outset_adjustment_cubic() {
        let style = style(
            "border:5pt solid; overflow:clip; overflow-clip-margin:20pt; border-radius:0 15pt 25pt 35pt",
        );
        let edge = resolve_overflow_clip_edge(
            paint_space_rect(0.0, 0.0, 110.0, 110.0),
            &style,
            used_border_widths(&style),
            UsedOverflowAxes::from_style(&style),
            false,
            None,
        )
        .expect("rounded edge");
        let BoxContentContour::Rounded(rounded) = edge.clip.contour else {
            panic!("expected rounded contour");
        };
        assert_eq!(rounded.radii.top_left.x(), 0.0);
        assert!((rounded.radii.top_right.x() - 27.52).abs() < 0.01);
        assert!((rounded.radii.bottom_right.x() - 40.0).abs() < 0.01);
        assert!((rounded.radii.bottom_left.x() - 50.0).abs() < 0.01);
    }
}
