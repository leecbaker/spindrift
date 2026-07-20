use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_dashed_border_side(
    rects: &mut Vec<RenderedRect>,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: PaintStrokeWidth,
    horizontal: bool,
    border_width: PaintStrokeWidth,
    color: CssColor,
) {
    // WeasyPrint follows CSS Backgrounds and Borders' intentionally flexible
    // dashed-border rendering by distributing same-size dashes and spaces
    // along straight edges, keeping dashes at both corners.
    let cross_width = cross_width.points();
    let border_width = border_width.points();
    let nominal_dash = (border_width * 3.0).max(1.0);
    let spaces = (axis_length / nominal_dash / 2.0).floor();
    let dashes = spaces + 1.0;
    let dash = (axis_length / (spaces + dashes)).max(1.0);
    let mut index = 0.0;
    while index < dashes {
        let offset = index * dash * 2.0;
        if offset >= axis_length - 0.01 {
            break;
        }
        push_border_rect(
            rects,
            axis_start + offset,
            dash.min(axis_length - offset),
            cross_start,
            cross_width,
            horizontal,
            color,
        );
        index += 1.0;
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_patterned_border_side(
    rects: &mut Vec<RenderedRect>,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: f32,
    horizontal: bool,
    segment_length: f32,
    gap_length: f32,
    color: CssColor,
) {
    let mut offset = 0.0;
    while offset < axis_length - 0.01 {
        let length = segment_length.min(axis_length - offset);
        push_border_rect(
            rects,
            axis_start + offset,
            length,
            cross_start,
            cross_width,
            horizontal,
            color,
        );
        offset += segment_length + gap_length;
    }
}

/// Paint a straight CSS dotted border side as a native PDF round-cap dash stroke.
///
/// CSS Backgrounds and Borders Level 3 defines `dotted` as a series of dots
/// and intentionally leaves exact spacing flexible. PDF round caps turn a
/// zero-length dash into a circular dot; the dash pitch is balanced so the
/// outer dots touch the two ends of the side:
/// <https://www.w3.org/TR/css-backgrounds-3/#valdef-border-style-dotted>.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_dotted_border_side(
    paths: &mut Vec<RenderedPath>,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: PaintStrokeWidth,
    horizontal: bool,
    color: CssColor,
) {
    paint_dotted_border_side_with_clip(
        paths,
        axis_start,
        axis_length,
        cross_start,
        cross_width.points(),
        horizontal,
        color,
        None,
    );
}

/// Paint a CSS dotted border side with an optional PDF clipping region.
///
/// CSS Backgrounds and Borders defines dotted borders as a series of dots,
/// while PDF's line-cap and dash graphics-state parameters provide native
/// circular dots. Clipping paths constrain rounded-side dots to their border
/// side transition region:
/// <https://www.w3.org/TR/css-backgrounds-3/#valdef-border-style-dotted> and
/// ISO 32000-1:2008, 8.4.3.3, 8.4.3.6, and 8.5.4.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_dotted_border_side_with_clip(
    paths: &mut Vec<RenderedPath>,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: f32,
    horizontal: bool,
    color: CssColor,
    clip: Option<RenderedPathClip>,
) {
    if axis_length <= 0.0 || cross_width <= 0.0 || !color.is_visible() {
        return;
    }
    let diameter = cross_width;
    let dot_count = (axis_length / (diameter * 2.0)).floor() as usize + 1;
    let center_cross = cross_start + diameter / 2.0;

    if dot_count == 1 {
        let center_axis = axis_start + axis_length / 2.0;
        let (cx, cy) = if horizontal {
            (center_axis, center_cross)
        } else {
            (center_cross, center_axis)
        };
        paths.push(circle_path(cx, cy, diameter / 2.0, color, clip));
        return;
    }

    let first_center = axis_start + diameter / 2.0;
    let last_center = axis_start + axis_length - diameter / 2.0;
    let pitch = (last_center - first_center) / (dot_count - 1) as f32;
    let (start, end) = if horizontal {
        (
            paint_space_point(first_center, center_cross),
            paint_space_point(last_center, center_cross),
        )
    } else {
        (
            paint_space_point(center_cross, first_center),
            paint_space_point(center_cross, last_center),
        )
    };
    paths.push(
        RenderedPath::new(
            vec![
                RenderedPathCommand::move_to(start),
                RenderedPathCommand::line_to(end),
            ],
            None,
            RenderedPathFillRule::NonZero,
            Some(color),
            PaintStrokeWidth::new(diameter),
            clip,
        )
        .with_stroke_style(RenderedPathStrokeStyle {
            line_cap: RenderedPathLineCap::Round,
            dash_array: vec![0.0, pitch],
            ..RenderedPathStrokeStyle::default()
        }),
    );
}

/// Paint a CSS dashed border side as filled rectangular path segments.
///
/// This mirrors `paint_dashed_border_side`'s straight-edge distribution but
/// stores the dashes as PDF paths so a rounded border ring and side transition
/// clip can be applied:
/// <https://www.w3.org/TR/css-backgrounds-3/#valdef-border-style-dashed>.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_dashed_border_side_with_clip(
    paths: &mut Vec<RenderedPath>,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: f32,
    horizontal: bool,
    border_width: f32,
    color: CssColor,
    clip: Option<RenderedPathClip>,
) {
    if axis_length <= 0.0 || cross_width <= 0.0 || !color.is_visible() {
        return;
    }
    let nominal_dash = (border_width * 3.0).max(1.0);
    let spaces = (axis_length / nominal_dash / 2.0).floor();
    let dashes = spaces + 1.0;
    let dash = (axis_length / (spaces + dashes)).max(1.0);
    let mut index = 0.0;
    while index < dashes {
        let offset = index * dash * 2.0;
        if offset >= axis_length - 0.01 {
            break;
        }
        let length = dash.min(axis_length - offset);
        let rect = if horizontal {
            paint_space_rect(axis_start + offset, cross_start, length, cross_width)
        } else {
            paint_space_rect(cross_start, axis_start + offset, cross_width, length)
        };
        paths.push(rect_path(rect, color, clip.clone()));
        index += 1.0;
    }
}

/// Build a filled circle path for a CSS dotted border dot.
///
/// PDF has no circle primitive, so the single-dot fallback uses four cubic
/// Bezier curves with the standard quarter-circle control-point ratio.
pub(crate) fn circle_path(
    cx: f32,
    cy: f32,
    radius: f32,
    color: CssColor,
    clip: Option<RenderedPathClip>,
) -> RenderedPath {
    let ratio = radius * 0.552_284_8;
    RenderedPath::new(
        vec![
            RenderedPathCommand::move_to(paint_space_point(cx + radius, cy)),
            RenderedPathCommand::curve_to(
                paint_space_point(cx + radius, cy + ratio),
                paint_space_point(cx + ratio, cy + radius),
                paint_space_point(cx, cy + radius),
            ),
            RenderedPathCommand::curve_to(
                paint_space_point(cx - ratio, cy + radius),
                paint_space_point(cx - radius, cy + ratio),
                paint_space_point(cx - radius, cy),
            ),
            RenderedPathCommand::curve_to(
                paint_space_point(cx - radius, cy - ratio),
                paint_space_point(cx - ratio, cy - radius),
                paint_space_point(cx, cy - radius),
            ),
            RenderedPathCommand::curve_to(
                paint_space_point(cx + ratio, cy - radius),
                paint_space_point(cx + radius, cy - ratio),
                paint_space_point(cx + radius, cy),
            ),
            RenderedPathCommand::Close,
        ],
        Some(color),
        RenderedPathFillRule::NonZero,
        None,
        PaintStrokeWidth::ZERO,
        clip,
    )
}

/// Build a filled rectangle path for clipped CSS border dash paint.
///
/// Rounded dashed borders need PDF path clipping, so dashes that are normally
/// optimized as rectangles can also be represented as filled path rectangles:
/// ISO 32000-1:2008, 8.5 "Path Construction and Painting".
pub(crate) fn rect_path(
    rect: PaintRect,
    color: CssColor,
    clip: Option<RenderedPathClip>,
) -> RenderedPath {
    RenderedPath::new(
        paint_rect_path_commands(rect),
        Some(color),
        RenderedPathFillRule::NonZero,
        None,
        PaintStrokeWidth::ZERO,
        clip,
    )
}

pub(crate) fn push_border_rect(
    rects: &mut Vec<RenderedRect>,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: f32,
    horizontal: bool,
    color: CssColor,
) {
    let (x, y, width, height) = if horizontal {
        (axis_start, cross_start, axis_length, cross_width)
    } else {
        (cross_start, axis_start, cross_width, axis_length)
    };
    rects.push(RenderedRect::from_paint_rect(
        paint_space_rect(x, y, width, height),
        Some(color),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::RenderedPathCommandPoints;

    fn rectangular_clip() -> RenderedPathClip {
        RenderedPathClip::new(
            vec![
                RenderedPathCommand::move_to(paint_space_point(0.0, 0.0)),
                RenderedPathCommand::line_to(paint_space_point(10.0, 0.0)),
                RenderedPathCommand::line_to(paint_space_point(10.0, 10.0)),
                RenderedPathCommand::Close,
            ],
            RenderedPathFillRule::NonZero,
            Vec::new(),
        )
    }

    #[test]
    fn dotted_horizontal_side_uses_balanced_round_cap_dash_stroke() {
        let mut paths = Vec::new();
        paint_dotted_border_side_with_clip(
            &mut paths,
            10.0,
            20.0,
            30.0,
            2.0,
            true,
            CssColor::BLACK,
            Some(rectangular_clip()),
        );

        assert_eq!(paths.len(), 1);
        let path = &paths[0];
        assert_eq!(path.fill, None);
        assert_eq!(path.stroke, Some(CssColor::BLACK));
        assert_eq!(path.stroke_width, PaintStrokeWidth::new(2.0));
        assert!(path.clip.is_some());
        assert_eq!(path.stroke_style.line_cap, RenderedPathLineCap::Round);
        assert_eq!(path.stroke_style.dash_array, vec![0.0, 3.6]);
        assert_eq!(path.commands.len(), 2);
        assert_eq!(
            path.commands[0].typed_points(),
            RenderedPathCommandPoints::MoveTo(paint_space_point(11.0, 31.0))
        );
        assert_eq!(
            path.commands[1].typed_points(),
            RenderedPathCommandPoints::LineTo(paint_space_point(29.0, 31.0))
        );
    }

    #[test]
    fn dotted_vertical_side_uses_vertical_centerline() {
        let mut paths = Vec::new();
        paint_dotted_border_side(
            &mut paths,
            10.0,
            20.0,
            30.0,
            PaintStrokeWidth::new(2.0),
            false,
            CssColor::BLACK,
        );

        assert_eq!(paths.len(), 1);
        let path = &paths[0];
        assert_eq!(
            path.commands[0].typed_points(),
            RenderedPathCommandPoints::MoveTo(paint_space_point(31.0, 11.0))
        );
        assert_eq!(
            path.commands[1].typed_points(),
            RenderedPathCommandPoints::LineTo(paint_space_point(31.0, 29.0))
        );
    }

    #[test]
    fn short_dotted_side_uses_one_filled_circle() {
        let mut paths = Vec::new();
        paint_dotted_border_side(
            &mut paths,
            10.0,
            3.0,
            30.0,
            PaintStrokeWidth::new(2.0),
            true,
            CssColor::BLACK,
        );

        assert_eq!(paths.len(), 1);
        let path = &paths[0];
        assert_eq!(path.fill, Some(CssColor::BLACK));
        assert_eq!(path.stroke, None);
        assert_eq!(path.commands.len(), 6);
    }

    #[test]
    fn dotted_side_skips_empty_or_invisible_paint() {
        let mut paths = Vec::new();
        paint_dotted_border_side(
            &mut paths,
            0.0,
            0.0,
            0.0,
            PaintStrokeWidth::new(2.0),
            true,
            CssColor::BLACK,
        );
        paint_dotted_border_side(
            &mut paths,
            0.0,
            10.0,
            0.0,
            PaintStrokeWidth::new(2.0),
            true,
            CssColor::TRANSPARENT,
        );
        assert!(paths.is_empty());
    }

    #[test]
    fn rect_path_preserves_a_nonzero_paint_rect() {
        let path = rect_path(
            paint_space_rect(10.0, 20.0, 30.0, 40.0),
            CssColor::BLACK,
            None,
        );

        assert_eq!(
            path.commands
                .iter()
                .cloned()
                .map(RenderedPathCommand::typed_points)
                .collect::<Vec<_>>(),
            vec![
                RenderedPathCommandPoints::MoveTo(paint_space_point(10.0, 20.0)),
                RenderedPathCommandPoints::LineTo(paint_space_point(40.0, 20.0)),
                RenderedPathCommandPoints::LineTo(paint_space_point(40.0, 60.0)),
                RenderedPathCommandPoints::LineTo(paint_space_point(10.0, 60.0)),
                RenderedPathCommandPoints::Close,
            ]
        );
    }
}
