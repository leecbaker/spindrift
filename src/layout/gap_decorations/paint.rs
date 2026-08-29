use super::*;

/// Expand a gap-rule segment while retaining its logical patterned-rule
/// phase. The phase is only observable for dotted/dashed rules; solid and
/// shaded rules intentionally share the ordinary primitive path.
pub(in crate::layout) fn gap_rule_segment_primitives_with_pattern_phase(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    style: BorderStyle,
    color: CssColor,
    pattern_phase: f32,
) -> Vec<PaintPrimitive> {
    if !width.can_paint() || style.suppresses_used_width() || !color.is_visible() {
        return Vec::new();
    }
    match style {
        BorderStyle::Double if width.double_bands().is_some() => {
            double_gap_rule_primitives(context, gap, segment, width, color)
        }
        BorderStyle::Groove | BorderStyle::Ridge => {
            groove_ridge_gap_rule_primitives(context, gap, segment, width, style, color)
        }
        BorderStyle::Inset | BorderStyle::Outset => groove_ridge_gap_rule_primitives(
            context,
            gap,
            segment,
            width,
            if style == BorderStyle::Inset {
                BorderStyle::Ridge
            } else {
                BorderStyle::Groove
            },
            color,
        ),
        BorderStyle::Dotted | BorderStyle::Dashed => {
            patterned_gap_rule_primitives(context, gap, segment, width, style, color, pattern_phase)
        }
        BorderStyle::Solid | BorderStyle::Double => {
            vec![solid_gap_rule_primitive(
                context, gap, segment, width, color, None,
            )]
        }
        BorderStyle::None | BorderStyle::Hidden => Vec::new(),
    }
}

fn patterned_gap_rule_primitives(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    style: BorderStyle,
    color: CssColor,
    pattern_phase: f32,
) -> Vec<PaintPrimitive> {
    let (axis_start, axis_length, cross_start, horizontal) = match context.kind {
        GapRuleAxisKind::Column => (
            context.container.page_rect.top_y() - segment.end.position,
            segment.end.position - segment.start.position,
            width
                .centered_span(context.container.page_rect.x() + gap.center())
                .start,
            false,
        ),
        GapRuleAxisKind::Row => (
            context.container.page_rect.x() + segment.start.position,
            segment.end.position - segment.start.position,
            width
                .centered_span(context.container.page_rect.top_y() - gap.center())
                .start,
            true,
        ),
    };
    // Preserve the established native border-side painter for an unbroken
    // gap. Besides being cheaper, it is the reference rendering for CSS
    // dotted/dashed caps. The phased path below is only needed when one gap
    // has been split into several portions at an intersection.
    if pattern_phase.abs() <= GAP_RULE_EPSILON {
        if style == BorderStyle::Dotted {
            let mut paths = Vec::new();
            paint_dotted_border_side(
                &mut paths,
                axis_start,
                axis_length,
                cross_start,
                width.into_paint_stroke_width(),
                horizontal,
                color,
            );
            return paths.into_iter().map(PaintPrimitive::Path).collect();
        }
        let mut rects = Vec::new();
        paint_dashed_border_side(
            &mut rects,
            axis_start,
            axis_length,
            cross_start,
            width.into_paint_stroke_width(),
            horizontal,
            width.into_paint_stroke_width(),
            color,
        );
        return rects.into_iter().map(PaintPrimitive::Rect).collect();
    }
    // `axis_start` is the physical paint projection of the portion's logical
    // start.  Keep the sequence phase in that same start-relative direction;
    // the page-coordinate inversion for column rules has already happened
    // while constructing `axis_start` above.
    let phase_at_paint_start = pattern_phase;
    let stroke_width = width.into_paint_stroke_width().points();
    if style == BorderStyle::Dotted {
        let pitch = stroke_width * 2.0;
        let mut center = axis_start + stroke_width * 0.5 - phase_at_paint_start.rem_euclid(pitch);
        let axis_end = axis_start + axis_length;
        let mut paths = Vec::new();
        while center < axis_end - GAP_RULE_EPSILON {
            if center >= axis_start - stroke_width * 0.5 {
                let (x, y) = if horizontal {
                    (center, cross_start + stroke_width * 0.5)
                } else {
                    (cross_start + stroke_width * 0.5, center)
                };
                paths.push(circle_path(x, y, stroke_width * 0.5, color, None));
            }
            center += pitch;
        }
        paths.into_iter().map(PaintPrimitive::Path).collect()
    } else {
        let dash_length = (stroke_width * 3.0).max(1.0);
        let period = dash_length * 2.0;
        let axis_end = axis_start + axis_length;
        let mut dash_start = axis_start - phase_at_paint_start.rem_euclid(period);
        let mut rects = Vec::new();
        while dash_start < axis_end - GAP_RULE_EPSILON {
            let start = dash_start.max(axis_start);
            let end = (dash_start + dash_length).min(axis_end);
            if end > start + GAP_RULE_EPSILON {
                let (x, y, width, height) = if horizontal {
                    (start, cross_start, end - start, stroke_width)
                } else {
                    (cross_start, start, stroke_width, end - start)
                };
                // Patterned gap rules are not border-side boxes; their
                // portions have already been clipped by topology resolution.
                // Emit the remaining dash rectangle directly in paint space.
                rects.push(RenderedRect::new(
                    x,
                    y,
                    width,
                    height,
                    Some(color),
                    None,
                    PaintStrokeWidth::ZERO,
                ));
            }
            dash_start += period;
        }
        rects.into_iter().map(PaintPrimitive::Rect).collect()
    }
}

pub(in crate::layout) fn double_gap_rule_primitives(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    color: CssColor,
) -> Vec<PaintPrimitive> {
    // A gap rule has a centerline, unlike a particular side of a border box.
    // Reusing `paint_border_side` here puts the second `double` stripe on the
    // outside of its selected box side, which can move it out of the gap.
    // Keep both stripes symmetric around the rule centerline instead.
    // <https://drafts.csswg.org/css-gaps-1/#gap-rule-painting>
    let stripe = GapRuleWidth::new(
        width
            .double_bands()
            .expect("double gap-rule paint requires a double-band width")
            .stripe
            .get(),
    );
    let offset = width.center_offset() - stripe.center_offset();
    vec![
        solid_gap_rule_primitive_with_cross_offset(context, gap, segment, stripe, color, -offset),
        solid_gap_rule_primitive_with_cross_offset(context, gap, segment, stripe, color, offset),
    ]
}

pub(in crate::layout) fn groove_ridge_gap_rule_primitives(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    style: BorderStyle,
    color: CssColor,
) -> Vec<PaintPrimitive> {
    let (first, second) =
        groove_ridge_border_colors(style, gap_rule_border_edge(context.kind, true), color);
    let half = width.half();
    vec![
        solid_gap_rule_primitive_with_cross_offset(
            context,
            gap,
            segment,
            half,
            first,
            -half.center_offset(),
        ),
        solid_gap_rule_primitive_with_cross_offset(
            context,
            gap,
            segment,
            width.remainder_after(half),
            second,
            width.remainder_after(half).center_offset(),
        ),
    ]
}

pub(in crate::layout) fn solid_gap_rule_primitive(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    color: CssColor,
    dash: Option<(f32, f32)>,
) -> PaintPrimitive {
    solid_gap_rule_primitive_with_cross_offset_and_dash(
        context, gap, segment, width, color, 0.0, dash,
    )
}

pub(in crate::layout) fn solid_gap_rule_primitive_with_cross_offset(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    color: CssColor,
    cross_offset: f32,
) -> PaintPrimitive {
    solid_gap_rule_primitive_with_cross_offset_and_dash(
        context,
        gap,
        segment,
        width,
        color,
        cross_offset,
        None,
    )
}

pub(in crate::layout) fn solid_gap_rule_primitive_with_cross_offset_and_dash(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    width: GapRuleWidth,
    color: CssColor,
    cross_offset: f32,
    dash: Option<(f32, f32)>,
) -> PaintPrimitive {
    // A solid gap rule is an opaque rectangular rule area. Representing it as
    // a stroked centerline makes cap rasterization and coincident endpoints
    // renderer-dependent, especially after negative insets join segments.
    // Patterned rules remain strokes because dash phase is part of their
    // painting model.
    // <https://drafts.csswg.org/css-gaps-1/#gap-rule-painting>
    if dash.is_none() {
        return match context.kind {
            GapRuleAxisKind::Column => {
                let span = width
                    .centered_span(context.container.page_rect.x() + gap.center() + cross_offset);
                let rect = RenderedRect::new(
                    span.start,
                    context.container.page_rect.top_y() - segment.end.position,
                    span.size(),
                    (segment.end.position - segment.start.position).max(0.0),
                    Some(color),
                    None,
                    PaintStrokeWidth::ZERO,
                );
                // A multicolumn rule paints over its container background.
                // Mark this opaque cover so PDF serialization can remove the
                // fully hidden underpaint before rasterization exposes it at
                // the rule's terminal edge.
                // <https://drafts.csswg.org/css-gaps-1/#gap-rule-painting>
                PaintPrimitive::Rect(if context.container_kind == GapContainerKind::Multicol {
                    rect.with_opaque_underpaint_culling()
                } else {
                    rect
                })
            }
            GapRuleAxisKind::Row => {
                let span = width.centered_span(
                    context.container.page_rect.top_y() - gap.center() - cross_offset,
                );
                PaintPrimitive::Rect(RenderedRect::new(
                    context.container.page_rect.x() + segment.start.position,
                    span.start,
                    (segment.end.position - segment.start.position).max(0.0),
                    span.size(),
                    Some(color),
                    None,
                    PaintStrokeWidth::ZERO,
                ))
            }
        };
    }
    match context.kind {
        GapRuleAxisKind::Column => {
            let x = context.container.page_rect.x() + gap.center() + cross_offset;
            PaintPrimitive::Stroke(RenderedStroke::new(
                x,
                context.container.page_rect.top_y() - segment.start.position,
                x,
                context.container.page_rect.top_y() - segment.end.position,
                width.into_paint_stroke_width(),
                color,
                dash,
            ))
        }
        GapRuleAxisKind::Row => {
            let y = context.container.page_rect.top_y() - gap.center() - cross_offset;
            PaintPrimitive::Stroke(RenderedStroke::new(
                context.container.page_rect.x() + segment.start.position,
                y,
                context.container.page_rect.x() + segment.end.position,
                y,
                width.into_paint_stroke_width(),
                color,
                dash,
            ))
        }
    }
}

pub(in crate::layout) fn gap_rule_border_edge(
    kind: GapRuleAxisKind,
    first_half: bool,
) -> BorderEdge {
    match (kind, first_half) {
        (GapRuleAxisKind::Column, true) => BorderEdge::Left,
        (GapRuleAxisKind::Column, false) => BorderEdge::Right,
        (GapRuleAxisKind::Row, true) => BorderEdge::Top,
        (GapRuleAxisKind::Row, false) => BorderEdge::Bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junction_insets_retain_crossing_gap_geometry() {
        let junction =
            GapRuleEndpoint::junction(20.0, GapJunctionWidth::new(20.0), GapRuleWidth::new(4.0));
        let percentage = css::GapRuleInsetValue::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );

        assert_eq!(
            used_gap_rule_endpoint_inset(percentage.clone(), junction),
            10.0
        );
        assert_eq!(
            used_gap_rule_endpoint_inset(css::GapRuleInsetValue::OverlapJoin, junction),
            -12.0
        );

        let junction_cap = GapRuleEndpoint::cap_at_junction(20.0, GapJunctionWidth::new(20.0));
        assert_eq!(
            used_gap_rule_endpoint_inset(percentage.clone(), junction_cap),
            10.0
        );
        assert_eq!(
            used_gap_rule_endpoint_inset(css::GapRuleInsetValue::OverlapJoin, junction_cap),
            0.0
        );

        let boundary_cap = GapRuleEndpoint::cap(20.0);
        assert_eq!(used_gap_rule_endpoint_inset(percentage, boundary_cap), 0.0);
        assert_eq!(
            used_gap_rule_endpoint_inset(css::GapRuleInsetValue::OverlapJoin, boundary_cap),
            0.0
        );
    }
}
