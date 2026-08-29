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

impl AxisRuleContext<'_> {
    pub(in crate::layout) fn axis_size(&self) -> f32 {
        match self.kind {
            GapRuleAxisKind::Column => self.container.local_size.height,
            GapRuleAxisKind::Row => self.container.local_size.width,
        }
    }
}

/// A resolved gap-rule centerline segment before it is expanded into PDF paint
/// primitives.
///
/// Fragmentation projects this semantic geometry before expanding a rule's
/// width. Projecting an already-filled rectangle changes an endpoint at a page
/// break into an artificial clipped cap.
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
    pub(in crate::layout) pattern_phase: f32,
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
    pub(super) fn for_gap(gap: GapBand, crossings: &[GapBand]) -> Self {
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

    pub(super) fn iter(&self) -> impl Iterator<Item = &ResolvedGapJunction> {
        self.0.iter()
    }

    pub(super) fn boundary(&self, position: f32, is_start: bool) -> Option<&ResolvedGapJunction> {
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

pub(in crate::layout) fn used_gap_rule_width<T, Source>(
    value: css::ComputedLengthPercentage,
    percentage_basis: PercentageBasis<T, Source>,
) -> GapRuleWidth
where
    T: SemanticLengthExt,
{
    GapRuleWidth::new(
        value
            .used_length_with_percentage_basis(percentage_basis)
            .map(layout_points)
            .unwrap_or(value.length_points()),
    )
}
