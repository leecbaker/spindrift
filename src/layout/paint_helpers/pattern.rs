use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_dashed_border_side(
    rects: &mut Vec<RenderedRect>,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: f32,
    horizontal: bool,
    border_width: f32,
    color: Color,
) {
    // WeasyPrint follows CSS Backgrounds and Borders' intentionally flexible
    // dashed-border rendering by distributing same-size dashes and spaces
    // along straight edges, keeping dashes at both corners.
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
    color: Color,
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

/// Paint a straight CSS dotted border side as round dot paths.
///
/// CSS Backgrounds and Borders Level 3 defines `dotted` as a series of round
/// dots and intentionally leaves exact spacing flexible. We mirror
/// WeasyPrint's straight-edge placement: dot diameter equals border width,
/// spaces are distributed between dot centers when possible, and each dot is a
/// filled Bezier circle:
/// <https://www.w3.org/TR/css-backgrounds-3/#valdef-border-style-dotted>.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_dotted_border_side(
    paths: &mut Vec<RenderedPath>,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: f32,
    horizontal: bool,
    color: Color,
) {
    paint_dotted_border_side_with_clip(
        paths,
        axis_start,
        axis_length,
        cross_start,
        cross_width,
        horizontal,
        color,
        None,
    );
}

/// Paint a CSS dotted border side with an optional PDF clipping region.
///
/// CSS Backgrounds and Borders defines dotted borders as round dots, while PDF
/// clipping paths define the intersection needed for rounded border side
/// transition areas:
/// <https://www.w3.org/TR/css-backgrounds-3/#valdef-border-style-dotted> and
/// ISO 32000-1:2008, 8.5.4 "Clipping Path Operators".
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_dotted_border_side_with_clip(
    paths: &mut Vec<RenderedPath>,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: f32,
    horizontal: bool,
    color: Color,
    clip: Option<RenderedPathClip>,
) {
    if axis_length <= 0.0 || cross_width <= 0.0 || !color.is_visible() {
        return;
    }
    let diameter = cross_width.max(1.0);
    let spaces = (axis_length / diameter / 2.0).floor();
    let dots = spaces + 1.0;
    let space = if spaces > 0.0 {
        ((axis_length - dots * diameter) / spaces).max(0.0)
    } else {
        0.0
    };

    let mut index = 0.0;
    while index < dots {
        let advance = index * (space + diameter);
        let center_axis = axis_start + advance + diameter / 2.0;
        let center_cross = cross_start + cross_width / 2.0;
        let (cx, cy) = if horizontal {
            (center_axis, center_cross)
        } else {
            (center_cross, center_axis)
        };
        paths.push(circle_path(cx, cy, diameter / 2.0, color, clip.clone()));
        index += 1.0;
    }
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
    color: Color,
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
        let (x, y, width, height) = if horizontal {
            (axis_start + offset, cross_start, length, cross_width)
        } else {
            (cross_start, axis_start + offset, cross_width, length)
        };
        paths.push(rect_path(x, y, width, height, color, clip.clone()));
        index += 1.0;
    }
}

/// Build a filled circle path for a CSS dotted border dot.
///
/// PDF has no circle primitive, so paths approximate dots with four cubic
/// Bezier curves. The control-point ratio mirrors WeasyPrint's dotted-border
/// drawing for raster parity.
pub(crate) fn circle_path(
    cx: f32,
    cy: f32,
    radius: f32,
    color: Color,
    clip: Option<RenderedPathClip>,
) -> RenderedPath {
    let ratio = radius / std::f32::consts::PI.sqrt();
    RenderedPath {
        clip,
        commands: vec![
            RenderedPathCommand::MoveTo(cx + radius, cy),
            RenderedPathCommand::CurveTo {
                x1: cx + radius,
                y1: cy + ratio,
                x2: cx + ratio,
                y2: cy + radius,
                x3: cx,
                y3: cy + radius,
            },
            RenderedPathCommand::CurveTo {
                x1: cx - ratio,
                y1: cy + radius,
                x2: cx - radius,
                y2: cy + ratio,
                x3: cx - radius,
                y3: cy,
            },
            RenderedPathCommand::CurveTo {
                x1: cx - radius,
                y1: cy - ratio,
                x2: cx - ratio,
                y2: cy - radius,
                x3: cx,
                y3: cy - radius,
            },
            RenderedPathCommand::CurveTo {
                x1: cx + ratio,
                y1: cy - radius,
                x2: cx + radius,
                y2: cy - ratio,
                x3: cx + radius,
                y3: cy,
            },
            RenderedPathCommand::Close,
        ],
        fill: Some(color),
        fill_rule: RenderedPathFillRule::NonZero,
        stroke: None,
        stroke_width: 0.0,
    }
}

/// Build a filled rectangle path for clipped CSS border dash paint.
///
/// Rounded dashed borders need PDF path clipping, so dashes that are normally
/// optimized as rectangles can also be represented as filled path rectangles:
/// ISO 32000-1:2008, 8.5 "Path Construction and Painting".
pub(crate) fn rect_path(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
    clip: Option<RenderedPathClip>,
) -> RenderedPath {
    RenderedPath {
        clip,
        commands: vec![
            RenderedPathCommand::MoveTo(x, y),
            RenderedPathCommand::LineTo(x + width, y),
            RenderedPathCommand::LineTo(x + width, y + height),
            RenderedPathCommand::LineTo(x, y + height),
            RenderedPathCommand::Close,
        ],
        fill: Some(color),
        fill_rule: RenderedPathFillRule::NonZero,
        stroke: None,
        stroke_width: 0.0,
    }
}

pub(crate) fn push_border_rect(
    rects: &mut Vec<RenderedRect>,
    axis_start: f32,
    axis_length: f32,
    cross_start: f32,
    cross_width: f32,
    horizontal: bool,
    color: Color,
) {
    let (x, y, width, height) = if horizontal {
        (axis_start, cross_start, axis_length, cross_width)
    } else {
        (cross_start, axis_start, cross_width, axis_length)
    };
    rects.push(RenderedRect {
        x,
        y,
        width,
        height,
        fill: Some(color),
        stroke: None,
        stroke_width: 0.0,
    });
}
