use std::num::NonZeroUsize;

use super::*;

pub(in crate::layout) const GAP_RULE_EPSILON: f32 = 0.01;

/// A non-negative CSS gap-rule thickness in source-local layout geometry.
///
/// This remains distinct from the gap span that contains the rule and from a
/// final [`PaintStrokeWidth`] emitted for dotted rules. CSS Gaps resolves the
/// rule width against its containing gap before the rule is expanded into
/// paint geometry: <https://drafts.csswg.org/css-gaps-1/#gap-decorations>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct GapRuleWidth(f32);

impl GapRuleWidth {
    pub(in crate::layout) const ZERO: Self = Self(0.0);

    pub(in crate::layout) fn new(value: f32) -> Self {
        Self(value.max(0.0))
    }

    pub(in crate::layout) fn can_paint(self) -> bool {
        self.0 > GAP_RULE_EPSILON
    }

    pub(in crate::layout) fn half(self) -> Self {
        Self::new(self.0 / 2.0)
    }

    pub(in crate::layout) fn remainder_after(self, leading: Self) -> Self {
        Self::new(self.0 - leading.0)
    }

    pub(in crate::layout) fn max(self, other: Self) -> Self {
        Self::new(self.0.max(other.0))
    }

    pub(in crate::layout) fn double_bands(self) -> Option<DoubleBorderBands> {
        DoubleBorderBands::for_used_width(layout_pt(self.0))
    }

    pub(in crate::layout) fn centered_span(self, center: f32) -> GapAxisSpan {
        let half = self.0 / 2.0;
        GapAxisSpan::new(center - half, center + half)
    }

    pub(in crate::layout) fn center_offset(self) -> f32 {
        self.0 / 2.0
    }

    pub(in crate::layout) fn extend_axis_position(self, position: f32) -> f32 {
        position + self.0
    }

    pub(in crate::layout) fn overlap_with_gap_half_extent(self, gap: GapBand) -> f32 {
        self.0.max(gap.size()) / 2.0
    }

    pub(in crate::layout) fn overlap_join_inset(self, junction_width: GapJunctionWidth) -> f32 {
        -(junction_width.points() + self.0) / 2.0
    }

    pub(in crate::layout) fn into_paint_stroke_width(self) -> PaintStrokeWidth {
        PaintStrokeWidth::new(self.0)
    }
}

/// The width of one CSS gap junction along the decorated gap's axis.
///
/// A junction can be contributed by several crossing gap portions. Its width
/// is therefore the union of their overlapping or abutting intervals, not the
/// width of any one member:
/// <https://drafts.csswg.org/css-gaps-1/#gap-rule-inset>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct GapJunctionWidth(f32);

impl GapJunctionWidth {
    pub(in crate::layout) const ZERO: Self = Self(0.0);

    pub(in crate::layout) fn new(value: f32) -> Self {
        Self(value.max(0.0))
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

/// Physical container-local coordinates used while resolving CSS gap rules.
/// Local y grows from the content top toward its block end, unlike paint/PDF
/// coordinates; projection is therefore explicit at the page boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GapDecorationSpace {}

pub(in crate::layout) type GapDecorationPoint = euclid::Point2D<f32, GapDecorationSpace>;
pub(in crate::layout) type GapDecorationSize = euclid::Size2D<f32, GapDecorationSpace>;
pub(in crate::layout) type GapDecorationRect = euclid::Rect<f32, GapDecorationSpace>;

/// The local gap-decoration area and its page-local projection.
///
/// Track and item geometry remains in the downward-y local space.  The
/// top-edge page rectangle is carried beside it so paint emission has one
/// explicit, auditable coordinate conversion boundary.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapDecorationContainer {
    pub(in crate::layout) local_size: GapDecorationSize,
    pub(in crate::layout) page_rect: PageTopRect,
}

impl GapDecorationContainer {
    pub(in crate::layout) fn new(x: f32, top_y: f32, width: f32, height: f32) -> Self {
        let local_size = GapDecorationSize::new(width.max(0.0), height.max(0.0));
        Self {
            local_size,
            page_rect: PageTopRect::new(x, top_y, local_size.width, local_size.height),
        }
    }
}

/// A one-dimensional gap/rule range. It is not Cartesian geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct GapAxisSpan {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) end: f32,
}

impl GapAxisSpan {
    pub(in crate::layout) fn new(start: f32, end: f32) -> Self {
        Self {
            start,
            end: end.max(start),
        }
    }

    pub(in crate::layout) fn size(self) -> f32 {
        self.end - self.start
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapDecorationGutter {
    pub(in crate::layout) span: GapAxisSpan,
    pub(in crate::layout) grid_line: Option<u16>,
    pub(in crate::layout) segment_range: Option<GapAxisSpan>,
    pub(in crate::layout) rule_index: Option<usize>,
}

impl GapDecorationGutter {
    pub(in crate::layout) fn new(start: f32, end: f32) -> Self {
        Self::with_grid_line(start, end, None)
    }

    pub(in crate::layout) fn with_grid_line(start: f32, end: f32, grid_line: Option<u16>) -> Self {
        Self {
            span: GapAxisSpan::new(start, end),
            grid_line,
            segment_range: None,
            rule_index: None,
        }
    }

    pub(in crate::layout) fn with_segment_range(
        start: f32,
        end: f32,
        segment_start: f32,
        segment_end: f32,
    ) -> Self {
        Self {
            segment_range: Some(GapAxisSpan::new(segment_start, segment_end)),
            ..Self::new(start, end)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct GapDecorationGutters {
    pub(in crate::layout) columns: Vec<GapDecorationGutter>,
    pub(in crate::layout) rows: Vec<GapDecorationGutter>,
}

pub(in crate::layout) type GapDecorationGridGutters = GapDecorationGutters;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapDecorationItem {
    pub(in crate::layout) rect: GapDecorationRect,
    pub(in crate::layout) grid_area: Option<GapDecorationGridArea>,
}

impl GapDecorationItem {
    pub(in crate::layout) fn from_rect(rect: GapDecorationRect) -> Self {
        Self {
            rect: GapDecorationRect::new(
                rect.origin,
                GapDecorationSize::new(rect.size.width.max(0.0), rect.size.height.max(0.0)),
            ),
            grid_area: None,
        }
    }

    pub(in crate::layout) fn from_rect_with_grid_area(
        rect: GapDecorationRect,
        grid_area: GapDecorationGridArea,
    ) -> Self {
        Self {
            grid_area: Some(grid_area),
            ..Self::from_rect(rect)
        }
    }

    #[cfg(test)]
    pub(in crate::layout) fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self::from_rect(GapDecorationRect::new(
            GapDecorationPoint::new(x, y),
            GapDecorationSize::new(width.max(0.0), height.max(0.0)),
        ))
    }

    pub(in crate::layout) fn x_end(self) -> f32 {
        self.rect.max_x()
    }

    pub(in crate::layout) fn y_end(self) -> f32 {
        self.rect.max_y()
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapDecorationGridArea {
    pub(in crate::layout) row_start: u16,
    pub(in crate::layout) row_end: u16,
    pub(in crate::layout) column_start: u16,
    pub(in crate::layout) column_end: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GapContainerKind {
    Flex,
    Grid,
    Multicol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GapRuleAxisKind {
    Column,
    Row,
}

/// Paints CSS Gap Decoration rules for resolved flex gutters.
///
/// Flex layout resolves gaps from line and item placement after wrapping and
/// alignment. Supplying that metadata avoids treating unrelated item bands as
/// synthetic gutters:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-lines> and
/// <https://drafts.csswg.org/css-gaps-1/#segments>.
pub(in crate::layout) fn flex_gap_decoration_primitives_with_gutters(
    style: &ComputedStyle,
    container: GapDecorationContainer,
    items: &[GapDecorationItem],
    gutters: &GapDecorationGutters,
) -> Vec<PaintPrimitive> {
    let column_gaps = gutters
        .columns
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    let row_gaps = gutters
        .rows
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    gap_decoration_primitives_for_gaps(GapDecorationContext {
        style,
        container,
        column_gaps: &column_gaps,
        row_gaps: &row_gaps,
        items,
        container_kind: GapContainerKind::Flex,
    })
}

/// Paints CSS Gap Decoration rules for resolved gutters in a grid container.
///
/// CSS Grid resolves explicit/implicit tracks and gutters before CSS Gaps
/// assigns decoration rules to the resulting gap sequence:
/// <https://www.w3.org/TR/css-grid-1/#track-sizing> and
/// <https://drafts.csswg.org/css-gaps-1/#assigning>.
pub(in crate::layout) fn grid_gap_decoration_primitives(
    style: &ComputedStyle,
    container: GapDecorationContainer,
    items: &[GapDecorationItem],
    gutters: &GapDecorationGridGutters,
) -> Vec<PaintPrimitive> {
    let column_gaps = gutters
        .columns
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    let row_gaps = gutters
        .rows
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    gap_decoration_primitives_for_gaps(GapDecorationContext {
        style,
        container,
        column_gaps: &column_gaps,
        row_gaps: &row_gaps,
        items,
        container_kind: GapContainerKind::Grid,
    })
}

/// A resolved gap-rule centerline segment before it is expanded into PDF paint
/// primitives.
///
/// Fragmentation projects this semantic geometry before expanding a rule's
/// width.  Projecting an already-filled rectangle changes an endpoint at a
/// page break into an artificial clipped cap.
/// <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct GapRulePaintSegment {
    pub(in crate::layout) kind: GapRuleAxisKind,
    pub(in crate::layout) gap: GapBand,
    pub(in crate::layout) segment: GapDecorationSegment,
    pub(in crate::layout) width: GapRuleWidth,
    pub(in crate::layout) style: BorderStyle,
    pub(in crate::layout) color: CssColor,
    /// Distance from the start of this axis's logical flex gap sequence.
    /// Patterned rules consume it as their phase origin so wrapping does not
    /// restart dots or dashes at each flex line.
    pub(in crate::layout) pattern_phase: f32,
}

/// Resolves grid gap rules into source-coordinate centerline segments.
///
/// This is deliberately separate from primitive generation because a grid may
/// fragment after its complete track and item geometry has been resolved.
/// <https://drafts.csswg.org/css-gaps-1/#fragmentation>
pub(in crate::layout) fn grid_gap_rule_paint_segments(
    style: &ComputedStyle,
    container: GapDecorationContainer,
    items: &[GapDecorationItem],
    gutters: &GapDecorationGridGutters,
) -> Vec<GapRulePaintSegment> {
    let axis_mapped_style = GapRulePhysicalProjection::new(style).project_style(style);
    let style = &axis_mapped_style;
    let column_gaps = gutters
        .columns
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    let row_gaps = gutters
        .rows
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    let assigned_column_gaps = assign_gap_bands(&column_gaps);
    let assigned_row_gaps = assign_gap_bands(&row_gaps);
    let column_rules = axis_rule_paint_segments(AxisRuleContext {
        kind: GapRuleAxisKind::Column,
        container_kind: GapContainerKind::Grid,
        rule: &style.column_rule,
        crossing_rule: &style.row_rule,
        container,
        gaps: &assigned_column_gaps,
        crossing_gaps: &row_gaps,
        items,
    });
    let row_rules = axis_rule_paint_segments(AxisRuleContext {
        kind: GapRuleAxisKind::Row,
        container_kind: GapContainerKind::Grid,
        rule: &style.row_rule,
        crossing_rule: &style.column_rule,
        container,
        gaps: &assigned_row_gaps,
        crossing_gaps: &column_gaps,
        items,
    });
    match style.rule_overlap {
        css::GapRuleOverlap::RowOverColumn => column_rules.into_iter().chain(row_rules).collect(),
        css::GapRuleOverlap::ColumnOverRow => row_rules.into_iter().chain(column_rules).collect(),
    }
}

/// Expands a previously resolved grid segment into paint primitives.
pub(in crate::layout) fn grid_gap_rule_segment_primitives(
    style: &ComputedStyle,
    container: GapDecorationContainer,
    rule_segment: GapRulePaintSegment,
) -> Vec<PaintPrimitive> {
    let (rule, crossing_rule) = match rule_segment.kind {
        GapRuleAxisKind::Column => (&style.column_rule, &style.row_rule),
        GapRuleAxisKind::Row => (&style.row_rule, &style.column_rule),
    };
    gap_rule_segment_primitives_with_pattern_phase(
        AxisRuleContext {
            kind: rule_segment.kind,
            container_kind: GapContainerKind::Grid,
            rule,
            crossing_rule,
            container,
            gaps: &[],
            crossing_gaps: &[],
            items: &[],
        },
        rule_segment.gap,
        rule_segment.segment,
        rule_segment.width,
        rule_segment.style,
        rule_segment.color,
        rule_segment.pattern_phase,
    )
}

/// Builds grid gutter bands from Taffy's detailed track sizes.
///
/// Taffy models gutters as zero or positive tracks between grid tracks. CSS gap
/// decorations paint those gutter tracks, while any distributed free space from
/// `align-content`/`justify-content` remains outside the decorated gutter.
pub(in crate::layout) fn grid_gap_decoration_gutters_from_tracks(
    column_sizes: &[f32],
    column_gutters: &[f32],
    row_sizes: &[f32],
    row_gutters: &[f32],
    style: &ComputedStyle,
    content_width: f32,
    content_height: f32,
) -> GapDecorationGridGutters {
    GapDecorationGutters {
        columns: grid_axis_gutters_from_tracks(
            column_sizes,
            column_gutters,
            content_width,
            style.justify_content,
            style.direction == Direction::Rtl,
        ),
        rows: grid_axis_gutters_from_tracks(
            row_sizes,
            row_gutters,
            content_height,
            style.align_content,
            false,
        ),
    }
}

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

    fn value<T: Clone>(&self, list: &css::GapRuleList<T>) -> T {
        list.value_for_valid_index(self.index.get(), self.sequence)
    }

    fn color(&self, rule: &css::GapRuleAxis) -> CssColor {
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
    slot: GapRuleSlot,
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
fn assign_gap_bands(gaps: &[GapBand]) -> Vec<AssignedGapBand> {
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

/// Paints CSS Gap Decoration column rules for multi-column gutters.
///
/// CSS Multi-column layout uses `column-gap` to separate adjacent column boxes,
/// while CSS Gaps applies column-rule decorations to those gaps:
/// <https://www.w3.org/TR/css-multicol-1/#column-gaps-and-rules> and
/// <https://drafts.csswg.org/css-gaps-1/#gap-rule-painting>.
pub(in crate::layout) fn multicol_gap_decoration_primitives(
    style: &ComputedStyle,
    content_left: f32,
    content_top: f32,
    content_bottom: f32,
    column_width: f32,
    gap: f32,
    column_count: usize,
) -> Vec<PaintPrimitive> {
    let topology = multicol_gap_topology_for_row(MulticolGapTopologyRowInput {
        style,
        content_left,
        content_top,
        column_height: (content_top - content_bottom).max(0.0),
        inline_size: (column_width * column_count as f32
            + gap * column_count.saturating_sub(1) as f32)
            .max(0.0),
        column_width,
        column_gap: gap,
        column_count,
        row: MulticolumnRowIndex::new(0),
        previous_row_gap: None,
        boundary: MulticolRowBoundary::Final,
    });
    gap_decoration_primitives_for_topology(style, &topology)
}

/// One committed multicolumn row and its adjacent row-gap topology.
///
/// The previous row gap is crossing-only: it classifies the current column
/// portions' start endpoints without repainting an already committed rule.
/// The following row gap is paintable and is emitted together with the row's
/// column portions, so `rule-overlap` is independent of replay timing.
#[derive(Clone, Copy)]
pub(in crate::layout) struct MulticolGapTopologyRowInput<'a> {
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_top: f32,
    pub(in crate::layout) column_height: f32,
    pub(in crate::layout) inline_size: f32,
    pub(in crate::layout) column_width: f32,
    pub(in crate::layout) column_gap: f32,
    pub(in crate::layout) column_count: usize,
    pub(in crate::layout) row: MulticolumnRowIndex,
    pub(in crate::layout) previous_row_gap: Option<f32>,
    pub(in crate::layout) boundary: MulticolRowBoundary,
}

/// The committed boundary after one wrapped multicolumn row.
///
/// A following row is represented by a prevalidated slot, so constructing a
/// paintable row gap without a corresponding entry in its CSS rule sequence
/// is impossible.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum MulticolRowBoundary {
    Final,
    FollowedBy { gap: f32, slot: GapRuleSlot },
}

/// Build the physical topology for one committed multicolumn row.
///
/// Column portions end at a row gap rather than overlapping it.  The shared
/// resolver still receives the abutting gap as crossing geometry, which gives
/// the endpoint its required junction classification.
/// <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
pub(in crate::layout) fn multicol_gap_topology_for_row(
    input: MulticolGapTopologyRowInput<'_>,
) -> ResolvedGapTopology {
    let MulticolGapTopologyRowInput {
        style,
        content_left,
        content_top,
        column_height,
        inline_size,
        column_width,
        column_gap,
        column_count,
        row,
        previous_row_gap,
        boundary,
    } = input;
    let column_height = column_height.max(0.0);
    let following_row_gap = match boundary {
        MulticolRowBoundary::Final => None,
        MulticolRowBoundary::FollowedBy { gap, .. } => Some(gap.max(0.0)),
    };
    let mut topology = ResolvedGapTopology::multicol(GapDecorationContainer::new(
        content_left,
        content_top,
        inline_size.max(0.0),
        column_height + following_row_gap.unwrap_or(0.0),
    ));
    let column_segment = GapAxisSpan::new(0.0, column_height);
    let column_rule_sequence = NonZeroUsize::new(column_count.saturating_sub(1));
    for physical_index in 0..column_count.saturating_sub(1) {
        let start = (column_width + column_gap) * physical_index as f32 + column_width;
        let rule_index = if physical_column_rule_sequence_is_reversed(style) {
            column_count
                .saturating_sub(2)
                .saturating_sub(physical_index)
        } else {
            physical_index
        };
        let portion = MulticolGapPortion::new(
            row,
            GapBand {
                start,
                end: start + column_gap,
                grid_line: None,
                segment_range: Some(column_segment),
                rule_index: None,
            },
            GapRuleSlot::new(
                GapRuleIndex::new(rule_index),
                column_rule_sequence.expect("a multicolumn gap has a rule sequence"),
            )
            .expect("physical multicol gap index is inside its rule sequence"),
        );
        topology.column_gaps.push(portion.band);
        topology.row_crossings.push(portion.band.band);
    }

    if let Some(previous_gap) = previous_row_gap.filter(|gap| *gap >= 0.0) {
        topology.column_crossings.push(GapBand {
            start: -previous_gap,
            end: 0.0,
            grid_line: None,
            segment_range: Some(GapAxisSpan::new(0.0, inline_size)),
            rule_index: None,
        });
    }
    if let MulticolRowBoundary::FollowedBy { slot, .. } = boundary {
        let next_gap = following_row_gap.expect("following row boundary carries its gap");
        let row_gap = GapBand {
            start: column_height,
            end: column_height + next_gap,
            grid_line: None,
            segment_range: Some(GapAxisSpan::new(0.0, inline_size)),
            rule_index: None,
        };
        topology.column_crossings.push(row_gap);
        topology.row_gaps.push(AssignedGapBand::new(row_gap, slot));
    }
    topology
}

/// Resolves how many anonymous columns contribute gutters for
/// `column-rule-visibility-items`.
/// <https://drafts.csswg.org/css-gaps-1/#rule-visibility>
pub(in crate::layout) fn multicol_decorated_column_count(
    style: &ComputedStyle,
    occupied: usize,
    available: usize,
) -> usize {
    match style.column_rule.visibility_items {
        // With `column-count:auto`, `all` bypasses item-adjacency suppression
        // but does not instantiate otherwise nonexistent anonymous columns;
        // the available inline size only caps how many columns may be made.
        // An explicit count establishes that complete track sequence, so all
        // of its gaps remain eligible.
        // <https://drafts.csswg.org/css-gaps-1/#rule-visibility>
        css::GapRuleVisibilityItems::All
            if matches!(style.column_count, css::ColumnCount::Auto) =>
        {
            occupied.min(available)
        }
        css::GapRuleVisibilityItems::All => available,
        css::GapRuleVisibilityItems::Around if occupied > 0 => (occupied + 1).min(available),
        css::GapRuleVisibilityItems::Normal
        | css::GapRuleVisibilityItems::Between
        | css::GapRuleVisibilityItems::Around => occupied.min(available),
    }
}

/// Paints the row rule between two adjacent wrapped multicol rows.
///
/// Multicol Level 2 creates a row gap after each complete row when overflow
/// columns wrap in the block direction. Column gaps crossing that row gap are
/// supplied so `rule-break` and junction inset behavior use the same segment
/// model as flex and grid decorations.
/// <https://drafts.csswg.org/css-multicol-2/#row-gaps>
#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn multicol_row_gap_decoration_primitives(
    style: &ComputedStyle,
    content_left: f32,
    content_top: f32,
    inline_size: f32,
    column_height: f32,
    row_gap: f32,
    row_index: usize,
    row_gap_count: usize,
    column_width: f32,
    column_gap: f32,
    column_count: usize,
) -> Vec<PaintPrimitive> {
    if style.visibility != Visibility::Visible || row_gap < 0.0 {
        return Vec::new();
    }
    let Some(rule_sequence) = NonZeroUsize::new(row_gap_count) else {
        return Vec::new();
    };
    let row_start = column_height + row_index as f32 * (column_height + row_gap);
    let row_rule_index = if style.writing_mode.ltr_inline_progresses_upward() {
        row_gap_count.saturating_sub(1).saturating_sub(row_index)
    } else {
        row_index
    };
    let row_gap_band = GapBand {
        start: row_start,
        end: row_start + row_gap,
        grid_line: None,
        segment_range: Some(GapAxisSpan::new(0.0, inline_size)),
        rule_index: None,
    };
    let row_crossings = (0..column_count.saturating_sub(1))
        .map(|index| {
            let start = (column_width + column_gap) * index as f32 + column_width;
            GapBand {
                start,
                end: start + column_gap,
                grid_line: None,
                segment_range: None,
                rule_index: None,
            }
        })
        .collect::<Vec<_>>();
    let mut topology = ResolvedGapTopology::multicol(GapDecorationContainer::new(
        content_left,
        content_top,
        inline_size,
        row_start + row_gap,
    ));
    let Some(row_slot) = GapRuleSlot::new(GapRuleIndex::new(row_rule_index), rule_sequence) else {
        return Vec::new();
    };
    topology
        .row_gaps
        .push(AssignedGapBand::new(row_gap_band, row_slot));
    topology.row_crossings = row_crossings;
    gap_decoration_primitives_for_topology(style, &topology)
}

/// Logical-to-physical projection for the shared gap-rule painter.
///
/// The topology adapter supplies physical column/row bands, whereas CSS Gaps
/// defines rule values and endpoint insets in logical column/row order.  This
/// small projection keeps that semantic boundary explicit instead of making
/// individual painters infer it from their local coordinates.
/// <https://drafts.csswg.org/css-gaps-1/#gap-rule-inset>
#[derive(Debug, Clone, Copy)]
struct GapRulePhysicalProjection {
    swaps_axes: bool,
    column_progress_reversed: bool,
    row_progress_reversed: bool,
}

impl GapRulePhysicalProjection {
    fn new(style: &ComputedStyle) -> Self {
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

    fn project_style(self, style: &ComputedStyle) -> ComputedStyle {
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

fn physical_column_rule_sequence_is_reversed(style: &ComputedStyle) -> bool {
    let axes = WritingModeAxes::new(style.writing_mode, style.direction);
    if axes.swaps_physical_axes() {
        axes.physical_side(LogicalSide::BlockStart) == PhysicalSide::Right
    } else {
        axes.physical_side(LogicalSide::InlineStart) == PhysicalSide::Right
    }
}

#[derive(Clone, Copy)]
pub(in crate::layout) struct AxisRuleContext<'a> {
    pub(in crate::layout) kind: GapRuleAxisKind,
    pub(in crate::layout) container_kind: GapContainerKind,
    pub(in crate::layout) rule: &'a css::GapRuleAxis,
    pub(in crate::layout) crossing_rule: &'a css::GapRuleAxis,
    pub(in crate::layout) container: GapDecorationContainer,
    pub(in crate::layout) gaps: &'a [AssignedGapBand],
    pub(in crate::layout) crossing_gaps: &'a [GapBand],
    pub(in crate::layout) items: &'a [GapDecorationItem],
}

/// One physical gap span.
///
/// This geometry carries no traversal-order invariant; callers that scan
/// crossings along a rule centerline must construct [`PhysicalGapJunctions`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct GapBand {
    pub(in crate::layout) start: f32,
    pub(in crate::layout) end: f32,
    pub(in crate::layout) grid_line: Option<u16>,
    pub(in crate::layout) segment_range: Option<GapAxisSpan>,
    pub(in crate::layout) rule_index: Option<usize>,
}

impl GapBand {
    pub(in crate::layout) fn size(self) -> f32 {
        (self.end - self.start).max(0.0)
    }

    pub(in crate::layout) fn center(self) -> f32 {
        (self.start + self.end) * 0.5
    }
}

/// One gap junction formed by the union of crossing gap portions.
///
/// `members` retain their individual rule-list identities and finite flex
/// line ranges. Only `span` is unioned for endpoint construction and inset
/// percentage resolution.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct ResolvedGapJunction {
    pub(in crate::layout) span: GapAxisSpan,
    pub(in crate::layout) members: Vec<GapBand>,
}

impl ResolvedGapJunction {
    pub(in crate::layout) fn width(&self) -> GapJunctionWidth {
        GapJunctionWidth::new(self.span.size())
    }
}

/// Gap junctions in increasing physical centerline order.
///
/// CSS Gap Decorations builds the endpoints of one rule by walking its
/// centerline from start to end. Flex layout, by contrast, supplies its
/// gutters in CSS flex order so rule-list values can be assigned correctly.
/// Those orders need not agree when wrapped lines have non-uniform main-axis
/// gaps. Overlapping or abutting bands form one junction, while every member
/// retains its rule-list identity:
/// <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
#[derive(Debug, Clone)]
pub(in crate::layout) struct PhysicalGapJunctions(Vec<ResolvedGapJunction>);

impl PhysicalGapJunctions {
    fn for_gap(gap: GapBand, crossings: &[GapBand]) -> Self {
        let mut crossings = crossings
            .iter()
            .copied()
            .filter(|crossing| crossing_portion_reaches_gap(gap, *crossing))
            .collect::<Vec<_>>();
        // `sort_by` is stable, so physically coincident portions retain their
        // committed CSS sequence order as a deterministic tie-breaker.
        crossings.sort_by(|left, right| {
            left.start
                .total_cmp(&right.start)
                .then_with(|| left.end.total_cmp(&right.end))
        });
        let mut junctions = Vec::<ResolvedGapJunction>::new();
        for crossing in crossings {
            if let Some(junction) = junctions.last_mut()
                && crossing.start <= junction.span.end + GAP_RULE_EPSILON
            {
                junction.span.end = junction.span.end.max(crossing.end);
                junction.members.push(crossing);
            } else {
                junctions.push(ResolvedGapJunction {
                    span: GapAxisSpan::new(crossing.start, crossing.end),
                    members: vec![crossing],
                });
            }
        }
        Self(junctions)
    }

    fn iter(&self) -> impl Iterator<Item = &ResolvedGapJunction> {
        self.0.iter()
    }

    fn boundary(&self, position: f32, is_start: bool) -> Option<&ResolvedGapJunction> {
        self.iter().find(|junction| {
            let boundary = if is_start {
                junction.span.end
            } else {
                junction.span.start
            };
            (boundary - position).abs() <= GAP_RULE_EPSILON
        })
    }
}

impl From<GapDecorationGutter> for GapBand {
    fn from(gutter: GapDecorationGutter) -> Self {
        Self {
            start: gutter.span.start,
            end: gutter.span.end,
            grid_line: gutter.grid_line,
            segment_range: gutter.segment_range,
            rule_index: gutter.rule_index,
        }
    }
}

pub(in crate::layout) fn grid_axis_gutters_from_tracks(
    sizes: &[f32],
    gutters: &[f32],
    axis_size: f32,
    alignment: AlignContent,
    axis_is_reversed: bool,
) -> Vec<GapDecorationGutter> {
    if sizes.is_empty() || gutters.len() != sizes.len() + 1 {
        return Vec::new();
    }
    let used_size = sizes.iter().chain(gutters.iter()).sum::<f32>();
    let free_space = axis_size - used_size;
    let track_count = sizes
        .iter()
        .filter(|size| **size > GAP_RULE_EPSILON)
        .count();
    let alignment = grid_alignment_fallback(free_space, track_count, alignment);
    let alignment = if axis_is_reversed {
        reverse_grid_alignment(alignment)
    } else {
        alignment
    };

    let mut cursor = 0.0;
    let mut seen_track = false;
    let mut bands = Vec::new();
    for (index, size) in sizes.iter().cloned().enumerate() {
        let gutter_size = gutters[index].max(0.0);
        if index > 0 && gutter_size > GAP_RULE_EPSILON {
            bands.push(GapDecorationGutter::with_grid_line(
                cursor,
                cursor + gutter_size,
                Some((index + 1) as u16),
            ));
        }
        cursor += gutter_size;

        let is_track = size > GAP_RULE_EPSILON;
        if is_track {
            cursor += grid_alignment_offset(free_space, track_count, alignment, !seen_track);
            seen_track = true;
        }
        cursor += size.max(0.0);
    }
    bands
}

pub(in crate::layout) fn grid_alignment_fallback(
    free_space: f32,
    track_count: usize,
    alignment: AlignContent,
) -> ContentAlignmentKeyword {
    let mut keyword = grid_alignment_keyword(alignment.keyword);
    let mut safe = alignment.safety == AlignmentSafety::Safe;
    if track_count <= 1 || free_space <= 0.0 {
        (keyword, safe) = match keyword {
            ContentAlignmentKeyword::Stretch | ContentAlignmentKeyword::SpaceBetween => {
                (ContentAlignmentKeyword::FlexStart, true)
            }
            ContentAlignmentKeyword::SpaceAround | ContentAlignmentKeyword::SpaceEvenly => {
                (ContentAlignmentKeyword::Center, true)
            }
            other => (other, safe),
        };
    }
    if free_space <= 0.0 && safe {
        ContentAlignmentKeyword::Start
    } else {
        keyword
    }
}

pub(in crate::layout) fn grid_alignment_keyword(
    keyword: ContentAlignmentKeyword,
) -> ContentAlignmentKeyword {
    match keyword {
        ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch => {
            ContentAlignmentKeyword::Stretch
        }
        ContentAlignmentKeyword::Left | ContentAlignmentKeyword::FlexStart => {
            ContentAlignmentKeyword::FlexStart
        }
        ContentAlignmentKeyword::Right | ContentAlignmentKeyword::FlexEnd => {
            ContentAlignmentKeyword::FlexEnd
        }
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            ContentAlignmentKeyword::FlexStart
        }
        other => other,
    }
}

pub(in crate::layout) fn reverse_grid_alignment(
    keyword: ContentAlignmentKeyword,
) -> ContentAlignmentKeyword {
    match keyword {
        ContentAlignmentKeyword::Start => ContentAlignmentKeyword::End,
        ContentAlignmentKeyword::End => ContentAlignmentKeyword::Start,
        ContentAlignmentKeyword::FlexStart => ContentAlignmentKeyword::FlexEnd,
        ContentAlignmentKeyword::FlexEnd => ContentAlignmentKeyword::FlexStart,
        other => other,
    }
}

pub(in crate::layout) fn grid_alignment_offset(
    free_space: f32,
    track_count: usize,
    alignment: ContentAlignmentKeyword,
    is_first: bool,
) -> f32 {
    if track_count == 0 {
        return 0.0;
    }
    if is_first {
        match alignment {
            ContentAlignmentKeyword::Start
            | ContentAlignmentKeyword::FlexStart
            | ContentAlignmentKeyword::Stretch
            | ContentAlignmentKeyword::SpaceBetween => 0.0,
            ContentAlignmentKeyword::End | ContentAlignmentKeyword::FlexEnd => free_space,
            ContentAlignmentKeyword::Center => free_space / 2.0,
            ContentAlignmentKeyword::SpaceAround => {
                if free_space >= 0.0 {
                    (free_space / track_count as f32) / 2.0
                } else {
                    free_space / 2.0
                }
            }
            ContentAlignmentKeyword::SpaceEvenly => {
                if free_space >= 0.0 {
                    free_space / (track_count + 1) as f32
                } else {
                    free_space / 2.0
                }
            }
            ContentAlignmentKeyword::Normal
            | ContentAlignmentKeyword::Left
            | ContentAlignmentKeyword::Right
            | ContentAlignmentKeyword::Baseline
            | ContentAlignmentKeyword::LastBaseline => 0.0,
        }
    } else {
        let free_space = free_space.max(0.0);
        match alignment {
            ContentAlignmentKeyword::SpaceBetween if track_count > 1 => {
                free_space / (track_count - 1) as f32
            }
            ContentAlignmentKeyword::SpaceAround => free_space / track_count as f32,
            ContentAlignmentKeyword::SpaceEvenly => free_space / (track_count + 1) as f32,
            _ => 0.0,
        }
    }
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

pub(in crate::layout) fn segment_crosses_spanning_item(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    crossing_gap: GapBand,
    own_width: GapRuleWidth,
) -> bool {
    if context.items.is_empty() {
        return false;
    }
    let half = own_width.overlap_with_gap_half_extent(gap);
    context.items.iter().any(|item| match context.kind {
        GapRuleAxisKind::Column => grid_item_spans_intersection(
            context.container_kind,
            *item,
            gap.grid_line,
            crossing_gap.grid_line,
            GapRuleAxisKind::Column,
        )
        .unwrap_or_else(|| {
            item.rect.origin.x < gap.center() + half
                && item.x_end() > gap.center() - half
                && item.rect.origin.y <= crossing_gap.start + GAP_RULE_EPSILON
                && item.y_end() >= crossing_gap.end - GAP_RULE_EPSILON
        }),
        GapRuleAxisKind::Row => grid_item_spans_intersection(
            context.container_kind,
            *item,
            gap.grid_line,
            crossing_gap.grid_line,
            GapRuleAxisKind::Row,
        )
        .unwrap_or_else(|| {
            item.rect.origin.y < gap.center() + half
                && item.y_end() > gap.center() - half
                && item.rect.origin.x <= crossing_gap.start + GAP_RULE_EPSILON
                && item.x_end() >= crossing_gap.end - GAP_RULE_EPSILON
        }),
    })
}

pub(in crate::layout) fn grid_item_spans_intersection(
    container_kind: GapContainerKind,
    item: GapDecorationItem,
    gap_line: Option<u16>,
    crossing_gap_line: Option<u16>,
    axis: GapRuleAxisKind,
) -> Option<bool> {
    if container_kind != GapContainerKind::Grid {
        return None;
    }
    let area = item.grid_area?;
    let gap_line = gap_line?;
    let crossing_gap_line = crossing_gap_line?;
    let crosses_own_axis = match axis {
        GapRuleAxisKind::Column => area.column_start < gap_line && area.column_end > gap_line,
        GapRuleAxisKind::Row => area.row_start < gap_line && area.row_end > gap_line,
    };
    let crosses_cross_axis = match axis {
        GapRuleAxisKind::Column => {
            area.row_start < crossing_gap_line && area.row_end > crossing_gap_line
        }
        GapRuleAxisKind::Row => {
            area.column_start < crossing_gap_line && area.column_end > crossing_gap_line
        }
    };
    Some(crosses_own_axis && crosses_cross_axis)
}

pub(in crate::layout) fn grid_segment_is_flanked_by_spanning_items(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    crossing_gap: GapBand,
) -> Option<bool> {
    if context.container_kind != GapContainerKind::Grid {
        return None;
    }
    let own_line = gap.grid_line?;
    let crossing_line = crossing_gap.grid_line?;
    let (column_line, row_line) = match context.kind {
        GapRuleAxisKind::Column => (own_line, crossing_line),
        GapRuleAxisKind::Row => (crossing_line, own_line),
    };
    let mut before_side = false;
    let mut after_side = false;
    let mut saw_grid_area = false;
    for area in context.items.iter().filter_map(|item| item.grid_area) {
        saw_grid_area = true;
        match context.kind {
            GapRuleAxisKind::Column if grid_area_spans_row_line(area, row_line) => {
                before_side |= area.column_start < column_line && area.column_end <= column_line;
                after_side |= area.column_start >= column_line && area.column_end > column_line;
            }
            GapRuleAxisKind::Row if grid_area_spans_column_line(area, column_line) => {
                before_side |= area.row_start < row_line && area.row_end <= row_line;
                after_side |= area.row_start >= row_line && area.row_end > row_line;
            }
            _ => {}
        }
    }
    saw_grid_area.then_some(before_side && after_side)
}

pub(in crate::layout) fn grid_area_spans_row_line(
    area: GapDecorationGridArea,
    row_line: u16,
) -> bool {
    area.row_start < row_line && area.row_end > row_line
}

pub(in crate::layout) fn grid_area_spans_column_line(
    area: GapDecorationGridArea,
    column_line: u16,
) -> bool {
    area.column_start < column_line && area.column_end > column_line
}

/// Whether joining the two segments adjacent to a grid gap junction would
/// paint through an item.
///
/// The `normal` break behavior joins two adjacent segments unless their union
/// is discontiguous. A union is discontiguous when its gap-width line segment
/// intersects an item. At a grid junction that means an item spanning the
/// decorated grid line in either track immediately adjacent to the crossing
/// line. <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
pub(in crate::layout) fn grid_junction_candidate_is_discontiguous(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    crossing_gap: GapBand,
) -> Option<bool> {
    if context.container_kind != GapContainerKind::Grid {
        return None;
    }
    let own_line = gap.grid_line?;
    let crossing_line = crossing_gap.grid_line?;
    let (column_line, row_line) = match context.kind {
        GapRuleAxisKind::Column => (own_line, crossing_line),
        GapRuleAxisKind::Row => (crossing_line, own_line),
    };
    let mut saw_grid_area = false;
    let mut intersects_candidate = false;
    for area in context.items.iter().filter_map(|item| item.grid_area) {
        saw_grid_area = true;
        let intersects = match context.kind {
            GapRuleAxisKind::Column => {
                area.column_start < column_line
                    && area.column_end > column_line
                    && area.row_start <= row_line
                    && area.row_end >= row_line
            }
            GapRuleAxisKind::Row => {
                area.row_start < row_line
                    && area.row_end > row_line
                    && area.column_start <= column_line
                    && area.column_end >= column_line
            }
        };
        intersects_candidate |= intersects;
    }
    saw_grid_area.then_some(intersects_candidate)
}

/// Whether a grid gap-rule segment intersects a grid item.
///
/// CSS Gaps discards a segment whose gap-width line segment intersects a
/// child item. Grid-area line numbers make that test independent of physical
/// writing direction and of track-size rounding.
/// <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
pub(in crate::layout) fn grid_gap_rule_segment_is_discontiguous(
    context: AxisRuleContext<'_>,
    gap: GapBand,
    segment: GapDecorationSegment,
) -> Option<bool> {
    if context.container_kind != GapContainerKind::Grid {
        return None;
    }
    let own_line = gap.grid_line?;
    let (cross_start, cross_end) = grid_segment_cross_axis_line_range(context, segment)?;
    let mut saw_grid_area = false;
    let mut intersects_item = false;
    for area in context.items.iter().filter_map(|item| item.grid_area) {
        saw_grid_area = true;
        intersects_item |= match context.kind {
            GapRuleAxisKind::Column => {
                area.column_start < own_line
                    && area.column_end > own_line
                    && area.row_start < cross_end
                    && area.row_end > cross_start
            }
            GapRuleAxisKind::Row => {
                area.row_start < own_line
                    && area.row_end > own_line
                    && area.column_start < cross_end
                    && area.column_end > cross_start
            }
        };
    }
    saw_grid_area.then_some(intersects_item)
}

#[cfg(test)]
mod tests {
    use super::*;

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
