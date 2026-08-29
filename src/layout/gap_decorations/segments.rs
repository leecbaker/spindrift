use super::*;

pub(in crate::layout) fn resolved_junction_affects_segments(
    context: AxisRuleContext<'_>,
    junction: &ResolvedGapJunction,
) -> bool {
    context.container_kind == GapContainerKind::Grid
        || junction
            .members
            .iter()
            .copied()
            .any(|crossing_gap| crossing_can_paint_for_gap(context, crossing_gap).unwrap_or(false))
}

pub(in crate::layout) fn segment_junction_endpoint(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    position: f32,
    junction: &ResolvedGapJunction,
) -> GapRuleEndpoint {
    // A geometric crossing is a junction only when its crossing decoration
    // exists. In particular, a hidden/zero-width flex rule leaves a cap;
    // its inset must not acquire `overlap-join` behavior merely because the
    // two resolved gutter rectangles meet.
    // <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
    let crossing_rule_width = junction
        .members
        .iter()
        .copied()
        .filter(|crossing_gap| {
            crossing_can_paint_for_gap(context, *crossing_gap).unwrap_or(false)
                && grid_crossing_segment_present_at_junction(context, gap, *crossing_gap)
                    .is_none_or(|present| present)
        })
        .filter_map(|crossing_gap| crossing_width_for_gap(context, crossing_gap))
        .fold(None, |width: Option<GapRuleWidth>, candidate| {
            Some(width.map_or(candidate, |width| width.max(candidate)))
        });
    if let Some(crossing_rule_width) = crossing_rule_width {
        GapRuleEndpoint::junction(position, junction.width(), crossing_rule_width)
    } else {
        GapRuleEndpoint::cap_at_junction(position, junction.width())
    }
}

pub(in crate::layout) fn crossing_rule_can_paint(
    width: GapRuleWidth,
    style: Option<BorderStyle>,
    color: Option<CssColor>,
) -> bool {
    width.can_paint()
        && !style.unwrap_or(BorderStyle::None).suppresses_used_width()
        && color.unwrap_or(CssColor::TRANSPARENT).is_visible()
}

pub(in crate::layout) fn nearest_crossing_gap(
    crossing_gaps: &[GapBand],
    position: f32,
) -> Option<GapBand> {
    crossing_gaps.iter().cloned().find(|gap| {
        gap.start <= position + GAP_RULE_EPSILON && gap.end >= position - GAP_RULE_EPSILON
    })
}

/// Whether a physical crossing portion reaches this gap's centerline.
///
/// Wrapped flex main-axis gaps are finite portions of otherwise aligned
/// physical bands. Their `segment_range` is therefore part of the junction
/// geometry, rather than paint-only clipping metadata. Grid and ordinary
/// multicol gaps leave it absent and remain full-span crossings.
pub(in crate::layout) fn crossing_portion_reaches_gap(gap: GapBand, crossing_gap: GapBand) -> bool {
    crossing_gap.segment_range.is_none_or(|range| {
        // A flex portion may terminate at a crossing gap's edge. That shared
        // boundary is still a CSS Gaps junction, even though the crossing
        // rule's centerline lies inside the row/column gutter rather than in
        // the portion's line box.
        range.start <= gap.end + GAP_RULE_EPSILON && range.end >= gap.start - GAP_RULE_EPSILON
    })
}

pub(in crate::layout) fn crossing_width_for_gap(
    context: AxisRuleContext<'_>,
    gap: GapBand,
) -> Option<GapRuleWidth> {
    crossing_rule_slot_for_gap(context, gap).and_then(|(index, count)| {
        context
            .crossing_rule
            .widths
            .value_for_index(index, count)
            .map(|width| {
                used_gap_rule_width(width, PercentageBasis::definite(layout_pt(gap.size())))
            })
    })
}

pub(in crate::layout) fn crossing_can_paint_for_gap(
    context: AxisRuleContext<'_>,
    gap: GapBand,
) -> Option<bool> {
    crossing_rule_slot_for_gap(context, gap).map(|(index, count)| {
        crossing_rule_can_paint(
            context
                .crossing_rule
                .widths
                .value_for_index(index, count)
                .map(|width| {
                    used_gap_rule_width(width, PercentageBasis::definite(layout_pt(gap.size())))
                })
                .unwrap_or(GapRuleWidth::ZERO),
            context.crossing_rule.styles.value_for_index(index, count),
            context.crossing_rule.colors.value_for_index(index, count),
        )
    })
}

/// Resolve a crossing band to the value-list position committed by its layout
/// topology. Flex portions can share a physical coordinate while consuming
/// different sequential slots, so their vector position is not an assignment
/// identity.
fn crossing_rule_slot_for_gap(
    context: AxisRuleContext<'_>,
    gap: GapBand,
) -> Option<(usize, usize)> {
    let sequence_len = context
        .crossing_gaps
        .iter()
        .enumerate()
        .map(|(physical_index, candidate)| candidate.rule_index.unwrap_or(physical_index))
        .max()
        .map(|last_index| last_index + 1)?;
    context
        .crossing_gaps
        .iter()
        .enumerate()
        .find(|(_, candidate)| {
            (candidate.start - gap.start).abs() <= GAP_RULE_EPSILON
                && (candidate.end - gap.end).abs() <= GAP_RULE_EPSILON
                && candidate.segment_range == gap.segment_range
        })
        .map(|(physical_index, candidate)| {
            (candidate.rule_index.unwrap_or(physical_index), sequence_len)
        })
}

pub(in crate::layout) fn offset_gap_rule_segment(
    rule: &css::GapRuleAxis,
    segment: GapDecorationSegment,
) -> GapDecorationSegment {
    let start_inset = used_gap_rule_endpoint_inset(
        match segment.start.kind {
            GapRuleEndpointKind::Cap => rule.inset_cap_start.clone(),
            GapRuleEndpointKind::Junction(_) => rule.inset_junction_start.clone(),
        },
        segment.start,
    );
    let end_inset = used_gap_rule_endpoint_inset(
        match segment.end.kind {
            GapRuleEndpointKind::Cap => rule.inset_cap_end.clone(),
            GapRuleEndpointKind::Junction(_) => rule.inset_junction_end.clone(),
        },
        segment.end,
    );
    GapDecorationSegment {
        start: GapRuleEndpoint {
            position: segment.start.position + start_inset,
            ..segment.start
        },
        end: GapRuleEndpoint {
            position: segment.end.position - end_inset,
            ..segment.end
        },
    }
}

pub(in crate::layout) fn used_gap_rule_endpoint_inset(
    value: css::GapRuleInsetValue,
    endpoint: GapRuleEndpoint,
) -> f32 {
    match value {
        css::GapRuleInsetValue::LengthPercentage(value) => value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                endpoint.junction_width.points(),
            )))
            .map(layout_points)
            .unwrap_or_else(|| value.length_points()),
        css::GapRuleInsetValue::OverlapJoin
            if matches!(endpoint.kind, GapRuleEndpointKind::Junction(_)) =>
        {
            match endpoint.kind {
                GapRuleEndpointKind::Junction(junction) => junction
                    .crossing_rule_width
                    .overlap_join_inset(endpoint.junction_width),
                GapRuleEndpointKind::Cap => unreachable!("junction match retains junction data"),
            }
        }
        css::GapRuleInsetValue::OverlapJoin => 0.0,
    }
}

/// Converts the filled rectangles used for solid gap rules to the centerline
/// geometry asserted by layout tests. Solid rules deliberately paint as areas,
/// rather than strokes, so cap geometry is independent of backend stroke
/// rasterization.
#[cfg(test)]
pub(in crate::layout) fn solid_gap_rule_centerlines(
    primitives: &[PaintPrimitive],
) -> Vec<RenderedStroke> {
    primitives
        .iter()
        .filter_map(|primitive| {
            let PaintPrimitive::Rect(rect) = primitive else {
                return None;
            };
            let color = rect.fill?;
            if rect.width() < rect.height() {
                Some(RenderedStroke::new(
                    rect.x() + rect.width() / 2.0,
                    rect.y() + rect.height(),
                    rect.x() + rect.width() / 2.0,
                    rect.y(),
                    PaintStrokeWidth::new(rect.width()),
                    color,
                    None,
                ))
            } else if rect.height() < rect.width() {
                Some(RenderedStroke::new(
                    rect.x(),
                    rect.y() + rect.height() / 2.0,
                    rect.x() + rect.width(),
                    rect.y() + rect.height() / 2.0,
                    PaintStrokeWidth::new(rect.height()),
                    color,
                    None,
                ))
            } else {
                None
            }
        })
        .collect()
}

pub(in crate::layout) fn segment_is_visible(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
) -> bool {
    match effective_visibility_items(context) {
        css::GapRuleVisibilityItems::All => true,
        css::GapRuleVisibilityItems::Around => {
            segment_has_adjacent_item(context, gap, segment, false)
                || segment_has_adjacent_item(context, gap, segment, true)
        }
        css::GapRuleVisibilityItems::Between => {
            segment_has_adjacent_item(context, gap, segment, false)
                && segment_has_adjacent_item(context, gap, segment, true)
        }
        css::GapRuleVisibilityItems::Normal => true,
    }
}

pub(in crate::layout) fn effective_visibility_items(
    context: AxisRuleContext<'_>,
) -> css::GapRuleVisibilityItems {
    match (
        context.container_kind,
        context.kind,
        context.rule.visibility_items,
    ) {
        // `*-rule-visibility-items` does not apply to flex containers, so
        // its initial `normal` value must not make flex decoration painting
        // depend on incidental item-rectangle metadata. Flex layout has
        // already materialized only resolved gutters between flex items or
        // lines at this boundary.
        // <https://drafts.csswg.org/css-gaps-1/#gap-rule-visibility>
        (GapContainerKind::Flex, _, css::GapRuleVisibilityItems::Normal) => {
            css::GapRuleVisibilityItems::All
        }
        (_, _, css::GapRuleVisibilityItems::Normal)
            if context.container_kind == GapContainerKind::Grid =>
        {
            css::GapRuleVisibilityItems::All
        }
        (
            GapContainerKind::Multicol,
            GapRuleAxisKind::Column,
            css::GapRuleVisibilityItems::Normal,
        ) => {
            // Multicol callers materialize only gutters whose adjacent
            // anonymous columns both received content. Unlike grid/flex they
            // do not have item rectangles to re-derive that fact here.
            // <https://www.w3.org/TR/css-multicol-1/#column-gaps-and-rules>
            css::GapRuleVisibilityItems::All
        }
        (GapContainerKind::Multicol, GapRuleAxisKind::Row, css::GapRuleVisibilityItems::Normal) => {
            css::GapRuleVisibilityItems::All
        }
        (_, _, css::GapRuleVisibilityItems::Normal) => css::GapRuleVisibilityItems::Between,
        (_, _, visibility) => visibility,
    }
}

pub(in crate::layout) fn segment_has_adjacent_item(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
    after: bool,
) -> bool {
    if context.items.is_empty() {
        return true;
    }
    context.items.iter().any(|item| match context.kind {
        GapRuleAxisKind::Column => grid_item_has_adjacent_area(context, *item, gap, segment, after)
            .unwrap_or_else(|| {
                let adjacent = if after {
                    item.rect.origin.x >= gap.end - GAP_RULE_EPSILON
                } else {
                    item.x_end() <= gap.start + GAP_RULE_EPSILON
                };
                adjacent
                    && item.rect.origin.y < segment.end.position - GAP_RULE_EPSILON
                    && item.y_end() > segment.start.position + GAP_RULE_EPSILON
            }),
        GapRuleAxisKind::Row => grid_item_has_adjacent_area(context, *item, gap, segment, after)
            .unwrap_or_else(|| {
                let adjacent = if after {
                    item.rect.origin.y >= gap.end - GAP_RULE_EPSILON
                } else {
                    item.y_end() <= gap.start + GAP_RULE_EPSILON
                };
                adjacent
                    && item.rect.origin.x < segment.end.position - GAP_RULE_EPSILON
                    && item.x_end() > segment.start.position + GAP_RULE_EPSILON
            }),
    })
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapDecorationSegment {
    pub(in crate::layout) start: GapRuleEndpoint,
    pub(in crate::layout) end: GapRuleEndpoint,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapRuleEndpoint {
    pub(in crate::layout) position: f32,
    pub(in crate::layout) kind: GapRuleEndpointKind,
    /// Percentage basis determined by this endpoint's position. A cap at an
    /// interior gap junction retains the junction width even though no
    /// crossing decoration is present there.
    pub(in crate::layout) junction_width: GapJunctionWidth,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum GapRuleEndpointKind {
    Cap,
    Junction(GapRuleJunction),
}

/// The crossing geometry that exists only at a gap-rule junction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct GapRuleJunction {
    pub(in crate::layout) crossing_rule_width: GapRuleWidth,
}

impl GapRuleEndpoint {
    pub(in crate::layout) fn cap(position: f32) -> Self {
        Self {
            position,
            kind: GapRuleEndpointKind::Cap,
            junction_width: GapJunctionWidth::ZERO,
        }
    }

    pub(in crate::layout) fn cap_at_junction(
        position: f32,
        junction_width: GapJunctionWidth,
    ) -> Self {
        Self {
            position,
            kind: GapRuleEndpointKind::Cap,
            junction_width,
        }
    }

    pub(in crate::layout) fn junction(
        position: f32,
        junction_width: GapJunctionWidth,
        crossing_rule_width: GapRuleWidth,
    ) -> Self {
        Self {
            position,
            kind: GapRuleEndpointKind::Junction(GapRuleJunction {
                crossing_rule_width,
            }),
            junction_width,
        }
    }
}

pub(in crate::layout) fn axis_rule_primitives(context: AxisRuleContext<'_>) -> Vec<PaintPrimitive> {
    axis_rule_paint_segments(context)
        .into_iter()
        .flat_map(|rule_segment| {
            gap_rule_segment_primitives_with_pattern_phase(
                context,
                rule_segment.gap,
                rule_segment.segment,
                rule_segment.width,
                rule_segment.style,
                rule_segment.color,
                rule_segment.pattern_phase,
            )
        })
        .collect()
}

/// Resolves a rule axis to centerline segments while retaining endpoint
/// metadata for a later fragment projection.
pub(in crate::layout) fn axis_rule_paint_segments(
    context: AxisRuleContext<'_>,
) -> Vec<GapRulePaintSegment> {
    let mut paint_segments = Vec::new();
    for assigned_gap in context.gaps.iter().copied() {
        let gap = assigned_gap.band;
        let width = used_gap_rule_width(
            assigned_gap.slot.value(&context.rule.widths),
            PercentageBasis::definite(layout_pt(gap.size())),
        );
        let rule_style = assigned_gap.slot.value(&context.rule.styles);
        let rule_color = assigned_gap.slot.color(context.rule);
        let mut segments = gap_rule_segments(context, gap, width)
            .into_iter()
            .map(|segment| offset_gap_rule_segment(context.rule, segment))
            .filter(|segment| {
                segment.end.position > segment.start.position + GAP_RULE_EPSILON
                    && segment_is_visible(context, gap, *segment)
            })
            .collect::<Vec<_>>();
        if rule_style == BorderStyle::Solid {
            segments = coalesce_overlapping_solid_gap_rule_segments(segments);
        }
        let source_range = gap
            .segment_range
            .unwrap_or_else(|| GapAxisSpan::new(0.0, context.axis_size()));
        paint_segments.extend(segments.into_iter().map(|segment| GapRulePaintSegment {
            kind: context.kind,
            gap,
            // One CSS gap owns one pattern origin.  A rule can split into
            // several clipped portions at intersections, which must retain
            // their offset within that same gap; the next actual flex gap is
            // a distinct decoration segment and starts its own pattern.
            // <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
            pattern_phase: segment.start.position - source_range.start,
            segment,
            width,
            style: rule_style,
            color: rule_color,
        }));
    }
    paint_segments
}

/// Returns the geometric union of overlapping collinear solid-rule segments.
///
/// Negative cap and junction insets may deliberately extend adjacent segments
/// through one another. Painting those opaque pieces independently changes the
/// antialiasing at their coincident edges; a solid rule instead represents the
/// union of its segment areas. Patterned rules remain separate so their dash
/// phase and junction behavior are preserved.
/// <https://drafts.csswg.org/css-gaps-1/#gap-rule-inset>
fn coalesce_overlapping_solid_gap_rule_segments(
    mut segments: Vec<GapDecorationSegment>,
) -> Vec<GapDecorationSegment> {
    segments.sort_by(|a, b| {
        a.start
            .position
            .partial_cmp(&b.start.position)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut merged: Vec<GapDecorationSegment> = Vec::with_capacity(segments.len());
    for segment in segments {
        if let Some(previous) = merged.last_mut()
            && segment.start.position <= previous.end.position + GAP_RULE_EPSILON
        {
            if segment.end.position > previous.end.position {
                previous.end = segment.end;
            }
        } else {
            merged.push(segment);
        }
    }
    merged
}

pub(in crate::layout) fn gap_rule_segments(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    own_width: GapRuleWidth,
) -> Vec<GapDecorationSegment> {
    let axis_size = context.axis_size();
    let axis_span = gap
        .segment_range
        .unwrap_or_else(|| GapAxisSpan::new(0.0, axis_size));
    let (axis_start, axis_end) = (axis_span.start, axis_span.end);
    let axis_start = axis_start.clamp(0.0, axis_size);
    let axis_end = axis_end.clamp(axis_start, axis_size);
    if axis_end <= axis_start + GAP_RULE_EPSILON {
        return Vec::new();
    }
    let junctions = PhysicalGapJunctions::for_gap(gap, context.crossing_gaps);
    let boundary_start = gap_rule_boundary_endpoint(context, gap, axis_start, true, &junctions);
    let boundary_end = gap_rule_boundary_endpoint(context, gap, axis_end, false, &junctions);
    let break_behavior = effective_rule_break(context);
    if break_behavior == css::GapRuleBreak::None {
        return vec![GapDecorationSegment {
            start: boundary_start,
            end: boundary_end,
        }];
    }

    let mut segments = Vec::new();
    let mut cursor = axis_start;
    let mut cursor_endpoint = boundary_start;
    for junction in junctions.iter() {
        // Flex has no track-area model that could make a suppressed crossing
        // discontinuous, so it can ignore the crossing altogether. Grid
        // still needs to split at its track junction: a spanning item can
        // make the two portions distinct even when the crossing rule itself
        // is suppressed. Those resulting endpoints are caps below.
        if !resolved_junction_affects_segments(context, junction) {
            continue;
        }
        let junction_start = junction.span.start.clamp(axis_start, axis_end);
        let junction_end = junction.span.end.clamp(axis_start, axis_end);
        if junction_end <= axis_start + GAP_RULE_EPSILON
            || junction_start >= axis_end - GAP_RULE_EPSILON
        {
            continue;
        }
        if junction_start > cursor + GAP_RULE_EPSILON {
            segments.push(GapDecorationSegment {
                start: cursor_endpoint,
                end: segment_junction_endpoint(context, gap, junction_start, junction),
            });
        }
        if resolved_junction_joins_segments(context, gap, junction, own_width) {
            cursor = junction_start;
        } else {
            cursor = junction_end.max(cursor);
        }
        cursor_endpoint = segment_junction_endpoint(context, gap, cursor, junction);
    }
    if axis_end > cursor + GAP_RULE_EPSILON {
        segments.push(GapDecorationSegment {
            start: cursor_endpoint,
            end: boundary_end,
        });
    }
    if break_behavior == css::GapRuleBreak::Normal
        && context.container_kind == GapContainerKind::Grid
    {
        // A segment which crosses an item is discontiguous and is discarded
        // before it can be joined to its neighbour.
        // <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
        segments.retain(|segment| {
            !grid_gap_rule_segment_is_discontiguous(context, gap, *segment).unwrap_or(false)
        });
        if !matches!(
            effective_visibility_items(context),
            css::GapRuleVisibilityItems::All | css::GapRuleVisibilityItems::Normal
        ) {
            // Visibility applies to individual gap portions. Once invisible
            // portions have been removed, the remaining contiguous portions
            // can again form one normal-break segment.
            // <https://drafts.csswg.org/css-gaps-1/#visibility>
            segments.retain(|segment| segment_is_visible(context, gap, *segment));
            segments = join_visible_grid_gap_rule_segments(context, gap, segments);
        }
    }
    segments
}

/// Classify an endpoint where a multicolumn gap merely abuts a crossing gap.
///
/// Row and column gaps in wrapped multicolumn layout do not overlap, but the
/// shared CSS Gaps endpoint algorithm still needs their common edge to be a
/// junction.  Grid normally reaches this path through an overlapping gutter;
/// keeping the adjacency handling here lets every topology adapter use the
/// same segment resolver.
fn gap_rule_boundary_endpoint(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    position: f32,
    is_start: bool,
    junctions: &PhysicalGapJunctions,
) -> GapRuleEndpoint {
    let Some(junction) = junctions.boundary(position, is_start) else {
        return GapRuleEndpoint::cap(position);
    };
    segment_junction_endpoint(context, gap, position, junction)
}

/// Joins adjacent visible atomic portions of a normal grid gap rule.
///
/// `around` and `between` visibility can remove an otherwise ordinary grid
/// portion. Joining only after that filter prevents a visible portion from
/// expanding through an empty one, while preserving the uninterrupted rule
/// that remains across occupied portions.
/// <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments> and
/// <https://drafts.csswg.org/css-gaps-1/#visibility>
fn join_visible_grid_gap_rule_segments(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segments: Vec<GapDecorationSegment>,
) -> Vec<GapDecorationSegment> {
    let mut joined = Vec::<GapDecorationSegment>::with_capacity(segments.len());
    for segment in segments {
        let Some(previous) = joined.last_mut() else {
            joined.push(segment);
            continue;
        };
        let crossing_gap = context.crossing_gaps.iter().cloned().find(|crossing_gap| {
            crossing_gap.start <= previous.end.position + GAP_RULE_EPSILON
                && crossing_gap.end >= segment.start.position - GAP_RULE_EPSILON
        });
        let joins = crossing_gap.is_some_and(|crossing_gap| {
            grid_crossing_segment_present_at_junction(context, gap, crossing_gap)
                .is_none_or(|present| present)
                && !grid_junction_candidate_is_discontiguous(context, gap, crossing_gap)
                    .unwrap_or(false)
        });
        if joins {
            previous.end = segment.end;
        } else {
            joined.push(segment);
        }
    }
    joined
}

pub(in crate::layout) fn effective_rule_break(context: AxisRuleContext<'_>) -> css::GapRuleBreak {
    match (
        context.container_kind,
        context.kind,
        context.rule.rule_break,
    ) {
        (GapContainerKind::Multicol, GapRuleAxisKind::Column, css::GapRuleBreak::Normal) => {
            css::GapRuleBreak::Intersection
        }
        (GapContainerKind::Multicol, GapRuleAxisKind::Row, css::GapRuleBreak::Normal) => {
            css::GapRuleBreak::None
        }
        (GapContainerKind::Flex, _, css::GapRuleBreak::Normal) => css::GapRuleBreak::None,
        (_, _, rule_break) => rule_break,
    }
}

pub(in crate::layout) fn should_join_across_junction(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    crossing_gap: GapBand,
    own_width: GapRuleWidth,
) -> bool {
    match effective_rule_break(context) {
        css::GapRuleBreak::Intersection => {
            grid_segment_is_flanked_by_spanning_items(context, gap, crossing_gap).unwrap_or_else(
                || segment_crosses_spanning_item(context, gap, crossing_gap, own_width),
            )
        }
        css::GapRuleBreak::Normal if context.container_kind == GapContainerKind::Grid => {
            // Visibility is evaluated for each segment portion. Keeping the
            // portions separate lets `around` and `between` remove empty
            // portions without making an adjacent occupied portion expand
            // through the rest of the grid gap.
            if !matches!(
                effective_visibility_items(context),
                css::GapRuleVisibilityItems::All | css::GapRuleVisibilityItems::Normal
            ) {
                return false;
            }
            if grid_crossing_segment_present_at_junction(context, gap, crossing_gap)
                .is_some_and(|present| !present)
            {
                return false;
            }
            !grid_junction_candidate_is_discontiguous(context, gap, crossing_gap).unwrap_or_else(
                || segment_crosses_spanning_item(context, gap, crossing_gap, own_width),
            )
        }
        _ => false,
    }
}

/// Whether every crossing portion contributing to one union junction permits
/// the adjacent rule portions to join.
///
/// CSS defines the flanking condition over all perpendicular gaps that form
/// the junction. A single discontiguous or unflanked member therefore keeps
/// the rule split.
pub(in crate::layout) fn resolved_junction_joins_segments(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    junction: &ResolvedGapJunction,
    own_width: GapRuleWidth,
) -> bool {
    !junction.members.is_empty()
        && junction
            .members
            .iter()
            .copied()
            .all(|crossing_gap| should_join_across_junction(context, gap, crossing_gap, own_width))
}
