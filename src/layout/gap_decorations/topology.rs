use std::num::NonZeroUsize;

use super::*;

pub(in crate::layout) struct GapDecorationContext<'a> {
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) container: GapDecorationContainer,
    pub(in crate::layout) column_gaps: &'a [GapBand],
    pub(in crate::layout) row_gaps: &'a [GapBand],
    pub(in crate::layout) items: &'a [GapDecorationItem],
    pub(in crate::layout) container_kind: GapContainerKind,
}

/// A stable assignment position in one logical gap-rule sequence.
///
/// A multicolumn rule can have several physical portions after wrapping or a
/// spanner, but every portion still receives its width, style, and color from
/// the same logical rule.  Keeping that identity separate from a temporary
/// row-local vector index prevents replay from silently restarting authored
/// value lists.
/// <https://drafts.csswg.org/css-gaps-1/#assigning>
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::layout) struct GapRuleIndex(usize);

impl GapRuleIndex {
    pub(in crate::layout) fn new(value: usize) -> Self {
        Self(value)
    }

    pub(in crate::layout) fn get(self) -> usize {
        self.0
    }
}

/// A validated assignment in one non-empty CSS gap-rule sequence.
///
/// A paintable gap owns this slot rather than a raw optional index.  The
/// sequence cardinality is committed layout topology, never a speculative
/// sizing estimate, so resolving a width/style/color cannot address an
/// absent rule-list entry.
/// <https://drafts.csswg.org/css-gaps-1/#assigning>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct GapRuleSlot {
    index: GapRuleIndex,
    sequence: NonZeroUsize,
}

impl GapRuleSlot {
    pub(in crate::layout) fn new(index: GapRuleIndex, sequence: NonZeroUsize) -> Option<Self> {
        (index.get() < sequence.get()).then_some(Self { index, sequence })
    }

    pub(super) fn value<T: Clone>(&self, list: &css::GapRuleList<T>) -> T {
        list.value_for_valid_index(self.index.get(), self.sequence)
    }

    pub(super) fn color(&self, rule: &css::GapRuleAxis) -> CssColor {
        let unvisited = self.value(&rule.colors);
        rule.visited_colors
            .as_ref()
            .map(|visited| self.value(visited).with_alpha(unvisited.alpha()))
            .unwrap_or(unvisited)
    }
}

/// Geometry plus the already-validated rule assignment that paints it.
///
/// Crossing-only bands intentionally remain plain [`GapBand`] values because
/// they classify endpoints but never resolve a width, style, or color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct AssignedGapBand {
    pub(in crate::layout) band: GapBand,
    pub(super) slot: GapRuleSlot,
}

impl AssignedGapBand {
    pub(in crate::layout) fn new(band: GapBand, slot: GapRuleSlot) -> Self {
        Self { band, slot }
    }
}

/// A committed wrapped multicolumn row.  This intentionally cannot be mixed
/// with either a rule index or a source-fragment index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::layout) struct MulticolumnRowIndex(usize);

impl MulticolumnRowIndex {
    pub(in crate::layout) fn new(value: usize) -> Self {
        Self(value)
    }

    pub(in crate::layout) fn get(self) -> usize {
        self.0
    }
}

/// One physical portion of a logical multicolumn gap.
///
/// `band` locates the gap on its perpendicular axis, while
/// `segment_range` (stored by [`GapBand`]) is its source-local centerline
/// extent.  A portion may be paintable or may exist solely to classify an
/// endpoint of a neighbouring portion as a junction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct MulticolGapPortion {
    pub(in crate::layout) row: MulticolumnRowIndex,
    pub(in crate::layout) band: AssignedGapBand,
}

impl MulticolGapPortion {
    pub(in crate::layout) fn new(
        row: MulticolumnRowIndex,
        band: GapBand,
        slot: GapRuleSlot,
    ) -> Self {
        Self {
            row,
            band: AssignedGapBand::new(band, slot),
        }
    }
}

/// Layout-owned input for the shared gap-rule resolver.
///
/// Flex and grid already expose resolved gutters directly.  Multicolumn
/// layout additionally has non-painting crossing portions at row boundaries,
/// so its adapter supplies them explicitly instead of asking the renderer to
/// reconstruct rows from replay order.  All coordinates are container-local
/// physical paint coordinates; the caller performs logical-axis projection at
/// the multicol layout boundary.
/// <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
#[derive(Debug, Clone)]
pub(in crate::layout) struct ResolvedGapTopology {
    pub(in crate::layout) container: GapDecorationContainer,
    pub(in crate::layout) container_kind: GapContainerKind,
    pub(in crate::layout) column_gaps: Vec<AssignedGapBand>,
    pub(in crate::layout) row_gaps: Vec<AssignedGapBand>,
    pub(in crate::layout) column_crossings: Vec<GapBand>,
    pub(in crate::layout) row_crossings: Vec<GapBand>,
    pub(in crate::layout) items: Vec<GapDecorationItem>,
}

impl ResolvedGapTopology {
    pub(in crate::layout) fn multicol(container: GapDecorationContainer) -> Self {
        Self {
            container,
            container_kind: GapContainerKind::Multicol,
            column_gaps: Vec::new(),
            row_gaps: Vec::new(),
            column_crossings: Vec::new(),
            row_crossings: Vec::new(),
            items: Vec::new(),
        }
    }
}

/// Attach immutable rule-list slots to gutters supplied by grid and flex.
///
/// Their resolved gutter list is already committed by the corresponding
/// layout algorithm.  A producer may provide an explicit logical index for
/// reversed physical order; otherwise physical order is the rule order.
pub(in crate::layout) fn assign_gap_bands(gaps: &[GapBand]) -> Vec<AssignedGapBand> {
    let sequence_len = gaps
        .iter()
        .enumerate()
        .map(|(physical_index, gap)| gap.rule_index.unwrap_or(physical_index))
        .max()
        .and_then(|last_index| NonZeroUsize::new(last_index + 1));
    let Some(sequence_len) = sequence_len else {
        return Vec::new();
    };
    gaps.iter()
        .copied()
        .enumerate()
        .filter_map(|(physical_index, band)| {
            let index = GapRuleIndex::new(band.rule_index.unwrap_or(physical_index));
            GapRuleSlot::new(index, sequence_len).map(|slot| AssignedGapBand::new(band, slot))
        })
        .collect()
}

/// Resolve both gap-rule axes from one layout-owned topology.
///
/// The two axes intentionally share one paint batch.  In particular, this
/// keeps `rule-overlap` independent from temporary multicolumn replay order.
pub(in crate::layout) fn gap_decoration_primitives_for_topology(
    style: &ComputedStyle,
    topology: &ResolvedGapTopology,
) -> Vec<PaintPrimitive> {
    if style.visibility != Visibility::Visible
        || (topology.column_gaps.is_empty() && topology.row_gaps.is_empty())
    {
        return Vec::new();
    }

    // `GapBand` is deliberately physical: column rules run along local y and
    // row rules along local x.  CSS `start`/`end` insets, however, remain
    // logical.  Project both concerns once here so every shared topology
    // producer (grid, flex, and multicol) uses the same endpoint convention.
    let axis_mapped_style = GapRulePhysicalProjection::new(style).project_style(style);
    let style = &axis_mapped_style;
    let column_rules = axis_rule_primitives(AxisRuleContext {
        kind: GapRuleAxisKind::Column,
        container_kind: topology.container_kind,
        rule: &style.column_rule,
        crossing_rule: &style.row_rule,
        container: topology.container,
        gaps: &topology.column_gaps,
        crossing_gaps: &topology.column_crossings,
        items: &topology.items,
    });
    let row_rules = axis_rule_primitives(AxisRuleContext {
        kind: GapRuleAxisKind::Row,
        container_kind: topology.container_kind,
        rule: &style.row_rule,
        crossing_rule: &style.column_rule,
        container: topology.container,
        gaps: &topology.row_gaps,
        crossing_gaps: &topology.row_crossings,
        items: &topology.items,
    });
    match style.rule_overlap {
        css::GapRuleOverlap::RowOverColumn => column_rules.into_iter().chain(row_rules).collect(),
        css::GapRuleOverlap::ColumnOverRow => row_rules.into_iter().chain(column_rules).collect(),
    }
}

pub(in crate::layout) fn gap_decoration_primitives_for_gaps(
    context: GapDecorationContext<'_>,
) -> Vec<PaintPrimitive> {
    let topology = ResolvedGapTopology {
        container: context.container,
        container_kind: context.container_kind,
        column_gaps: assign_gap_bands(context.column_gaps),
        row_gaps: assign_gap_bands(context.row_gaps),
        column_crossings: context.row_gaps.to_vec(),
        row_crossings: context.column_gaps.to_vec(),
        items: context.items.to_vec(),
    };
    gap_decoration_primitives_for_topology(context.style, &topology)
}

/// Logical-to-physical projection for the shared gap-rule painter.
///
/// The topology adapter supplies physical column/row bands, whereas CSS Gaps
/// defines rule values and endpoint insets in logical column/row order.  This
/// small projection keeps that semantic boundary explicit instead of making
/// individual painters infer it from their local coordinates.
/// <https://drafts.csswg.org/css-gaps-1/#gap-rule-inset>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapRulePhysicalProjection {
    swaps_axes: bool,
    column_progress_reversed: bool,
    row_progress_reversed: bool,
}

impl GapRulePhysicalProjection {
    pub(in crate::layout) fn new(style: &ComputedStyle) -> Self {
        let axes = WritingModeAxes::new(style.writing_mode, style.direction);
        Self {
            swaps_axes: axes.swaps_physical_axes(),
            // A physical column rule runs along local y; a physical row rule
            // runs along local x. Resolve which logical axis each therefore
            // represents before asking whether its logical start is the high
            // physical coordinate.
            column_progress_reversed: axes
                .is_reversed(axes.logical_axis_for_physical(PhysicalAxis::Vertical)),
            row_progress_reversed: axes
                .is_reversed(axes.logical_axis_for_physical(PhysicalAxis::Horizontal)),
        }
    }

    pub(in crate::layout) fn project_style(self, style: &ComputedStyle) -> ComputedStyle {
        let mut mapped = style.clone();
        if self.swaps_axes {
            std::mem::swap(&mut mapped.column_rule, &mut mapped.row_rule);
            mapped.rule_overlap = match style.rule_overlap {
                css::GapRuleOverlap::RowOverColumn => css::GapRuleOverlap::ColumnOverRow,
                css::GapRuleOverlap::ColumnOverRow => css::GapRuleOverlap::RowOverColumn,
            };
        }
        if self.column_progress_reversed {
            reverse_gap_rule_endpoints(&mut mapped.column_rule);
        }
        if self.row_progress_reversed {
            reverse_gap_rule_endpoints(&mut mapped.row_rule);
        }
        mapped
    }
}

fn reverse_gap_rule_endpoints(rule: &mut css::GapRuleAxis) {
    std::mem::swap(&mut rule.inset_cap_start, &mut rule.inset_cap_end);
    std::mem::swap(&mut rule.inset_junction_start, &mut rule.inset_junction_end);
}

pub(super) fn physical_column_rule_sequence_is_reversed(style: &ComputedStyle) -> bool {
    let axes = WritingModeAxes::new(style.writing_mode, style.direction);
    if axes.swaps_physical_axes() {
        axes.physical_side(LogicalSide::BlockStart) == PhysicalSide::Right
    } else {
        axes.physical_side(LogicalSide::InlineStart) == PhysicalSide::Right
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::grid::{GridAxisTopology, grid_axis_gutters_from_topology};

    fn solid_gap_rule_style() -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style
    }

    fn gap_rule_slot(index: usize, sequence_len: usize) -> GapRuleSlot {
        GapRuleSlot::new(
            GapRuleIndex::new(index),
            NonZeroUsize::new(sequence_len).expect("test rule sequence is non-empty"),
        )
        .expect("test rule index is in range")
    }

    fn auto_fit_topology(
        track_sizes: Vec<f32>,
        interior_gutters: Vec<f32>,
        collapsed_tracks: Vec<bool>,
    ) -> GridAxisTopology {
        GridAxisTopology::from_auto_fit_track_layout(
            track_sizes,
            interior_gutters,
            collapsed_tracks,
        )
        .expect("test topology has matching track geometry")
    }

    fn topology_gap_spans(topology: &GridAxisTopology) -> Vec<GapAxisSpan> {
        grid_axis_gutters_from_topology(
            topology,
            260.0,
            css::ContentAlignment::new(css::ContentAlignmentKeyword::Start),
            false,
        )
        .into_iter()
        .map(|gutter| gutter.span)
        .collect()
    }

    #[test]
    fn auto_fit_collapse_reindexes_gap_rules_after_leading_middle_and_trailing_runs() {
        let cases = [
            (
                auto_fit_topology(
                    vec![20.0; 9],
                    vec![10.0; 8],
                    vec![true, true, true, true, false, false, false, false, false],
                ),
                vec![
                    GapAxisSpan::new(20.0, 30.0),
                    GapAxisSpan::new(50.0, 60.0),
                    GapAxisSpan::new(80.0, 90.0),
                    GapAxisSpan::new(110.0, 120.0),
                ],
            ),
            (
                auto_fit_topology(
                    vec![20.0; 9],
                    vec![10.0; 8],
                    vec![false, false, false, true, true, true, true, false, false],
                ),
                vec![
                    GapAxisSpan::new(20.0, 30.0),
                    GapAxisSpan::new(50.0, 60.0),
                    GapAxisSpan::new(80.0, 90.0),
                    GapAxisSpan::new(110.0, 120.0),
                ],
            ),
            (
                auto_fit_topology(
                    vec![20.0; 9],
                    vec![10.0; 8],
                    vec![false, false, false, false, true, true, true, true, true],
                ),
                vec![
                    GapAxisSpan::new(20.0, 30.0),
                    GapAxisSpan::new(50.0, 60.0),
                    GapAxisSpan::new(80.0, 90.0),
                ],
            ),
        ];

        for (topology, expected_spans) in cases {
            let gaps = topology_gap_spans(&topology);
            assert_eq!(gaps, expected_spans);
            let slots = assign_gap_bands(
                &gaps
                    .iter()
                    .copied()
                    .map(|span| GapBand {
                        start: span.start,
                        end: span.end,
                        grid_line: None,
                        segment_range: None,
                        rule_index: None,
                    })
                    .collect::<Vec<_>>(),
            );
            assert_eq!(
                slots
                    .iter()
                    .map(|gap| gap.slot.index.get())
                    .collect::<Vec<_>>(),
                (0..expected_spans.len()).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn occupied_zero_sized_track_remains_a_gap_participant() {
        let topology = auto_fit_topology(vec![0.0, 20.0], vec![10.0], vec![false, false]);
        assert_eq!(
            topology_gap_spans(&topology),
            vec![GapAxisSpan::new(0.0, 10.0)]
        );
    }

    #[test]
    fn physical_crossings_form_union_junctions_without_losing_rule_order() {
        let mut style = solid_gap_rule_style();
        style.row_rule.rule_break = css::GapRuleBreak::Intersection;
        style.column_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(5.0));
        style.column_rule.colors = css::GapRuleList::single(CssColor::new(0, 0, 255));
        style.row_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(5.0));
        style.row_rule.colors = css::GapRuleList::single(CssColor::new(255, 0, 0));

        let row_gap = GapBand {
            start: 100.0,
            end: 190.0,
            grid_line: None,
            segment_range: Some(GapAxisSpan::new(0.0, 600.0)),
            rule_index: Some(0),
        };
        // This is CSS flex order: all main-axis gaps from the first line,
        // followed by all main-axis gaps from the second line. It is not
        // physical x-axis order because the wrapped lines have different
        // item widths.
        let crossings = [
            GapBand {
                start: 100.0,
                end: 190.0,
                grid_line: None,
                segment_range: Some(GapAxisSpan::new(0.0, 100.0)),
                rule_index: Some(0),
            },
            GapBand {
                start: 390.0,
                end: 480.0,
                grid_line: None,
                segment_range: Some(GapAxisSpan::new(0.0, 100.0)),
                rule_index: Some(1),
            },
            GapBand {
                start: 160.0,
                end: 250.0,
                grid_line: None,
                segment_range: Some(GapAxisSpan::new(190.0, 290.0)),
                rule_index: Some(2),
            },
            GapBand {
                start: 360.0,
                end: 450.0,
                grid_line: None,
                segment_range: Some(GapAxisSpan::new(190.0, 290.0)),
                rule_index: Some(3),
            },
        ];
        let junctions = PhysicalGapJunctions::for_gap(row_gap, &crossings);
        assert_eq!(
            junctions
                .iter()
                .map(|junction| (
                    junction.span,
                    junction
                        .members
                        .iter()
                        .map(|member| member.rule_index)
                        .collect::<Vec<_>>()
                ))
                .collect::<Vec<_>>(),
            vec![
                (GapAxisSpan::new(100.0, 250.0), vec![Some(0), Some(2)]),
                (GapAxisSpan::new(360.0, 480.0), vec![Some(3), Some(1)]),
            ]
        );
        assert_eq!(
            crossings
                .iter()
                .map(|crossing| (crossing.start, crossing.end, crossing.rule_index))
                .collect::<Vec<_>>(),
            vec![
                (100.0, 190.0, Some(0)),
                (390.0, 480.0, Some(1)),
                (160.0, 250.0, Some(2)),
                (360.0, 450.0, Some(3)),
            ]
        );

        let row_gaps = [AssignedGapBand::new(row_gap, gap_rule_slot(0, 1))];
        let segments = gap_rule_segments(
            AxisRuleContext {
                kind: GapRuleAxisKind::Row,
                container_kind: GapContainerKind::Flex,
                rule: &style.row_rule,
                crossing_rule: &style.column_rule,
                container: GapDecorationContainer::new(0.0, 290.0, 600.0, 290.0),
                gaps: &row_gaps,
                crossing_gaps: &crossings,
                items: &[],
            },
            row_gap,
            GapRuleWidth::new(5.0),
        );
        assert_eq!(
            segments
                .iter()
                .map(|segment| (segment.start.position, segment.end.position))
                .collect::<Vec<_>>(),
            vec![(0.0, 100.0), (250.0, 360.0), (480.0, 600.0)]
        );
    }

    fn wrapped_flex_inset_segments(percent: f32) -> Vec<(f32, f32)> {
        let mut style = solid_gap_rule_style();
        style.row_rule.rule_break = css::GapRuleBreak::Intersection;
        style.column_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(5.0));
        style.column_rule.colors = css::GapRuleList::single(CssColor::new(255, 215, 0));
        style.row_rule.widths =
            css::GapRuleList::single(css::ComputedLengthPercentage::from_points(5.0));
        style.row_rule.colors = css::GapRuleList::single(CssColor::new(255, 0, 0));
        let inset = css::GapRuleInsetValue::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(percent),
        );
        style.row_rule.inset_junction_start = inset.clone();
        style.row_rule.inset_junction_end = inset;

        let row_gap = GapBand {
            start: 50.0,
            end: 60.0,
            grid_line: None,
            segment_range: Some(GapAxisSpan::new(0.0, 400.0)),
            rule_index: Some(0),
        };
        let top_range = Some(GapAxisSpan::new(0.0, 50.0));
        let bottom_range = Some(GapAxisSpan::new(60.0, 110.0));
        let crossings = [
            (70.0, 82.5, top_range, 0),
            (152.5, 165.0, top_range, 1),
            (235.0, 247.5, top_range, 2),
            (317.5, 330.0, top_range, 3),
            (70.0, 110.0, bottom_range, 4),
            (180.0, 220.0, bottom_range, 5),
            (290.0, 330.0, bottom_range, 6),
        ]
        .map(|(start, end, segment_range, rule_index)| GapBand {
            start,
            end,
            grid_line: None,
            segment_range,
            rule_index: Some(rule_index),
        });
        let row_gaps = [AssignedGapBand::new(row_gap, gap_rule_slot(0, 1))];
        axis_rule_paint_segments(AxisRuleContext {
            kind: GapRuleAxisKind::Row,
            container_kind: GapContainerKind::Flex,
            rule: &style.row_rule,
            crossing_rule: &style.column_rule,
            container: GapDecorationContainer::new(0.0, 110.0, 400.0, 110.0),
            gaps: &row_gaps,
            crossing_gaps: &crossings,
            items: &[],
        })
        .into_iter()
        .map(|segment| (segment.segment.start.position, segment.segment.end.position))
        .collect()
    }

    #[test]
    fn wrapped_flex_union_junctions_resolve_negative_quarter_insets() {
        assert_eq!(
            wrapped_flex_inset_segments(-0.25),
            vec![
                (0.0, 80.0),
                (100.0, 155.625),
                (161.875, 190.0),
                (210.0, 238.125),
                (244.375, 300.0),
                (320.0, 400.0),
            ]
        );
    }

    #[test]
    fn wrapped_flex_union_junctions_resolve_negative_half_insets() {
        assert_eq!(wrapped_flex_inset_segments(-0.5), vec![(0.0, 400.0)]);
    }

    #[test]
    fn wrapped_flex_union_junctions_resolve_positive_half_insets() {
        assert_eq!(
            wrapped_flex_inset_segments(0.5),
            vec![
                (0.0, 50.0),
                (130.0, 146.25),
                (253.75, 270.0),
                (350.0, 400.0),
            ]
        );
    }

    #[test]
    fn union_junction_overlap_join_uses_widest_visible_crossing_rule() {
        let mut style = solid_gap_rule_style();
        style.column_rule.widths = css::GapRuleList::from_parts(
            vec![
                css::GapRuleListComponent::Value(css::ComputedLengthPercentage::from_points(2.0)),
                css::GapRuleListComponent::Value(css::ComputedLengthPercentage::from_points(8.0)),
            ],
            None,
            Vec::new(),
        );
        style.column_rule.colors = css::GapRuleList::single(CssColor::new(0, 0, 255));
        let row_gap = GapBand {
            start: 50.0,
            end: 60.0,
            grid_line: None,
            segment_range: Some(GapAxisSpan::new(0.0, 100.0)),
            rule_index: Some(0),
        };
        let crossings = [
            GapBand {
                start: 10.0,
                end: 30.0,
                grid_line: None,
                segment_range: Some(GapAxisSpan::new(0.0, 50.0)),
                rule_index: Some(0),
            },
            GapBand {
                start: 10.0,
                end: 50.0,
                grid_line: None,
                segment_range: Some(GapAxisSpan::new(60.0, 110.0)),
                rule_index: Some(1),
            },
        ];
        let junctions = PhysicalGapJunctions::for_gap(row_gap, &crossings);
        let context = AxisRuleContext {
            kind: GapRuleAxisKind::Row,
            container_kind: GapContainerKind::Flex,
            rule: &style.row_rule,
            crossing_rule: &style.column_rule,
            container: GapDecorationContainer::new(0.0, 110.0, 100.0, 110.0),
            gaps: &[],
            crossing_gaps: &crossings,
            items: &[],
        };
        let endpoint = segment_junction_endpoint(
            context,
            row_gap,
            10.0,
            junctions.iter().next().expect("union junction exists"),
        );

        assert_eq!(
            used_gap_rule_endpoint_inset(
                css::GapRuleInsetValue::LengthPercentage(
                    css::ComputedLengthPercentage::from_percent(0.5),
                ),
                endpoint,
            ),
            20.0
        );
        assert_eq!(
            used_gap_rule_endpoint_inset(css::GapRuleInsetValue::OverlapJoin, endpoint),
            -24.0
        );
    }

    #[test]
    fn multicol_topology_keeps_row_gap_as_column_crossing() {
        let style = solid_gap_rule_style();
        let topology = multicol_gap_topology_for_row(MulticolGapTopologyRowInput {
            style: &style,
            content_left: 0.0,
            content_top: 0.0,
            column_height: 60.0,
            inline_size: 200.0,
            column_width: 60.0,
            column_gap: 10.0,
            column_count: 3,
            row: MulticolumnRowIndex::new(0),
            previous_row_gap: None,
            boundary: MulticolRowBoundary::FollowedBy {
                gap: 10.0,
                slot: gap_rule_slot(0, 1),
            },
        });

        assert_eq!(topology.column_gaps.len(), 2);
        assert_eq!(topology.row_gaps.len(), 1);
        assert_eq!(topology.column_crossings.len(), 1);
        assert_eq!(
            topology.column_gaps[0].band.segment_range,
            Some(GapAxisSpan::new(0.0, 60.0))
        );
        assert_eq!(topology.column_crossings[0].start, 60.0);
        assert_eq!(topology.column_crossings[0].end, 70.0);
        assert_eq!(topology.column_gaps[0].slot.index.get(), 0);
        assert_eq!(topology.column_gaps[1].slot.index.get(), 1);
        assert_eq!(topology.row_gaps[0].slot.index.get(), 0);
        assert_eq!(topology.row_gaps[0].slot.sequence.get(), 1);
    }

    #[test]
    fn multicol_row_boundary_is_a_junction_not_a_cap() {
        let style = solid_gap_rule_style();
        let topology = multicol_gap_topology_for_row(MulticolGapTopologyRowInput {
            style: &style,
            content_left: 0.0,
            content_top: 0.0,
            column_height: 60.0,
            inline_size: 200.0,
            column_width: 60.0,
            column_gap: 10.0,
            column_count: 3,
            row: MulticolumnRowIndex::new(0),
            previous_row_gap: None,
            boundary: MulticolRowBoundary::FollowedBy {
                gap: 10.0,
                slot: gap_rule_slot(0, 1),
            },
        });
        let segments = gap_rule_segments(
            AxisRuleContext {
                kind: GapRuleAxisKind::Column,
                container_kind: GapContainerKind::Multicol,
                rule: &style.column_rule,
                crossing_rule: &style.row_rule,
                container: topology.container,
                gaps: &topology.column_gaps,
                crossing_gaps: &topology.column_crossings,
                items: &[],
            },
            topology.column_gaps[0].band,
            GapRuleWidth::new(10.0),
        );

        assert_eq!(segments.len(), 1);
        assert!(matches!(
            segments[0].end.kind,
            GapRuleEndpointKind::Junction(_)
        ));
    }

    #[test]
    fn multicol_final_row_uses_the_preceding_row_gap_for_its_start_endpoint() {
        let style = solid_gap_rule_style();
        let topology = multicol_gap_topology_for_row(MulticolGapTopologyRowInput {
            style: &style,
            content_left: 0.0,
            content_top: 70.0,
            column_height: 60.0,
            inline_size: 200.0,
            column_width: 60.0,
            column_gap: 10.0,
            column_count: 3,
            row: MulticolumnRowIndex::new(1),
            previous_row_gap: Some(10.0),
            boundary: MulticolRowBoundary::Final,
        });
        assert!(topology.row_gaps.is_empty());
        let segments = gap_rule_segments(
            AxisRuleContext {
                kind: GapRuleAxisKind::Column,
                container_kind: GapContainerKind::Multicol,
                rule: &style.column_rule,
                crossing_rule: &style.row_rule,
                container: topology.container,
                gaps: &topology.column_gaps,
                crossing_gaps: &topology.column_crossings,
                items: &[],
            },
            topology.column_gaps[0].band,
            GapRuleWidth::new(10.0),
        );

        assert_eq!(segments.len(), 1);
        assert!(matches!(
            segments[0].start.kind,
            GapRuleEndpointKind::Junction(_)
        ));
    }

    #[test]
    fn column_rule_order_uses_sideways_block_progression() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::SidewaysRl;
        assert!(physical_column_rule_sequence_is_reversed(&style));

        style.writing_mode = WritingMode::SidewaysLr;
        assert!(!physical_column_rule_sequence_is_reversed(&style));
    }

    #[test]
    fn physical_projection_reverses_logical_endpoints_without_changing_their_axis() {
        let mut style = ComputedStyle::initial();
        style.direction = Direction::Rtl;
        let start = css::GapRuleInsetValue::LengthPercentage(
            css::ComputedLengthPercentage::from_points(3.0),
        );
        let end = css::GapRuleInsetValue::LengthPercentage(
            css::ComputedLengthPercentage::from_points(7.0),
        );
        style.row_rule.inset_cap_start = start.clone();
        style.row_rule.inset_cap_end = end.clone();

        let mapped = GapRulePhysicalProjection::new(&style).project_style(&style);

        // In horizontal RTL, a physical row rule still runs along the inline
        // axis, whose logical start is the right/high-x endpoint.
        assert_eq!(mapped.row_rule.inset_cap_start, end);
        assert_eq!(mapped.row_rule.inset_cap_end, start);
        assert_eq!(
            mapped.column_rule.inset_cap_start,
            style.column_rule.inset_cap_start
        );
    }
}
