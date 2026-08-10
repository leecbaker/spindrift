//! Resolution of CSS box-content contours at the used-value paint boundary.
//!
//! This is intentionally the single bridge from CSS border/padding/content
//! geometry into retained paint contours. Layout algorithms keep ownership of
//! their overflow axes and fragment bounds; they do not reproduce corner or
//! `border-shape` arithmetic.

use super::*;
use crate::document::paint::contours::{BoxContentContour, ResolvedBoxContentClip};

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum BoxContentContourRequest {
    /// Resolve the selected CSS Overflow clip edge. `outset` is the used
    /// `overflow-clip-margin`, and is never applied to replaced content.
    Overflow {
        reference_box: css::BackgroundBox,
        outset: f32,
    },
    /// Replaced content is clipped to the shaped content edge even when
    /// overflow is visible.
    /// <https://drafts.csswg.org/css-borders-4/#corner-clipping>
    ReplacedContent,
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
pub(in crate::layout) fn resolve_box_content_contour(
    border_rect: PaintRect,
    style: &ComputedStyle,
    border_insets: css::Edges,
    request: BoxContentContourRequest,
) -> Option<ResolvedBoxContentClip> {
    let (reference_box, outset) = match request {
        BoxContentContourRequest::Overflow {
            reference_box,
            outset,
        } => (reference_box, outset.max(0.0)),
        BoxContentContourRequest::ReplacedContent => (css::BackgroundBox::Content, 0.0),
    };
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

    let rounded = rounded_clip_rect_for_box_with_outset(
        border_rect,
        style,
        border_insets,
        reference_box,
        outset,
    );
    Some(ResolvedBoxContentClip {
        bounds,
        contour: rounded.map_or(BoxContentContour::Rect, BoxContentContour::Rounded),
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
        let style = style("border: 10pt solid black; padding: 5pt; border-radius: 20pt");
        let border = paint_space_rect(0.0, 0.0, 100.0, 80.0);
        let insets = used_border_widths(&style);

        let padding = resolve_box_content_contour(
            border,
            &style,
            insets,
            BoxContentContourRequest::Overflow {
                reference_box: css::BackgroundBox::Padding,
                outset: 0.0,
            },
        )
        .expect("padding contour");
        let content = resolve_box_content_contour(
            border,
            &style,
            insets,
            BoxContentContourRequest::ReplacedContent,
        )
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
        let contour = resolve_box_content_contour(
            paint_space_rect(0.0, 0.0, 100.0, 100.0),
            &style,
            used_border_widths(&style),
            BoxContentContourRequest::Overflow {
                reference_box: css::BackgroundBox::Padding,
                outset: 0.0,
            },
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
}
