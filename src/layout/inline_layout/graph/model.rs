use super::*;

/// One normalized inline formatting participant in an inline paragraph graph.
///
/// CSS Inline builds line boxes from an ordered stream of inline-level text
/// runs and atomic inline boxes. CSS Text then finds soft-wrap opportunities
/// across that stream, treating atomic inline boxes as U+FFFC for line-break
/// policy while preserving the source style and decoration metadata for
/// painting:
/// <https://www.w3.org/TR/css-inline-3/#line-box>,
/// <https://www.w3.org/TR/css-text-3/#line-breaking>, and
/// <https://www.w3.org/TR/css-text-3/#line-break-details>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineParagraphRun {
    pub(in crate::layout) item: InlineLineItem,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) shaped: Option<Rc<ShapedInlineLine>>,
}

/// One cloneable inline-box scope that crosses a selected line boundary.
///
/// The source stream owns the real start/end atoms of an inline box. CSS Box
/// Fragmentation makes `box-decoration-break: clone` create matching used
/// edges for every *intermediate* line fragment as well. Retaining the source
/// start atom gives a continuation all of the otherwise easy-to-lose lexical
/// metadata (baseline, visual offset, link, positioning scope, and paint
/// effect identity) while the edge itself is reconstructed for its used side.
///
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFragmentContinuation {
    source_start: InlineAtom,
    start_edge: InlineBoxEdgeFragment,
    end_edge: InlineBoxEdgeFragment,
}

impl InlineFragmentContinuation {
    /// Capture a cloneable inline scope from its source-owned start edge.
    pub(in crate::layout) fn from_source_start(atom: &InlineAtom) -> Option<Self> {
        let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(start_edge)) = atom.content()
        else {
            return None;
        };
        (start_edge.logical_edge == InlineLogicalEdge::Start
            && atom.style().box_decoration_break == BoxDecorationBreak::Clone)
            .then(|| Self {
                source_start: atom.clone(),
                start_edge: *start_edge,
                end_edge: inline_fragment_continuation_edge(
                    atom.style(),
                    InlineLogicalEdge::End,
                    start_edge.positioning_containing_block_id,
                ),
            })
    }

    /// Materialize one fragment-local edge while preserving the original
    /// scope's non-geometric metadata.
    pub(in crate::layout) fn edge_atom(&self, logical_edge: InlineLogicalEdge) -> InlineAtom {
        let mut atom = self.source_start.clone();
        let style = atom.style().clone();
        let edge_fragment = match logical_edge {
            InlineLogicalEdge::Start => self.start_edge,
            InlineLogicalEdge::End => self.end_edge,
        };
        {
            let data = Rc::make_mut(&mut atom.data);
            data.content = InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge_fragment));
            // The first-line pseudo resolves this deferred value after line
            // materialization. A continuation must not inherit a used color
            // from a prior candidate or line fragment.
            data.current_color_override = None;
        }
        match style.writing_mode {
            WritingMode::HorizontalTb => atom.size.width = edge_fragment.advance,
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => atom.size.height = edge_fragment.advance,
        }
        atom
    }

    pub(in crate::layout) fn start_item(&self) -> InlineItem {
        InlineItem::Atom(Box::new(self.edge_atom(InlineLogicalEdge::Start)))
    }

    pub(in crate::layout) fn end_item(&self) -> InlineItem {
        InlineItem::Atom(Box::new(self.edge_atom(InlineLogicalEdge::End)))
    }

    pub(super) fn start_measured_item(&self) -> MeasuredInlineItem {
        let atom = self.edge_atom(InlineLogicalEdge::Start);
        let width = inline_atom_logical_inline_size(&atom, atom.style());
        MeasuredInlineItem::new(InlineLineItem::Atom(atom), width, None)
    }

    pub(super) fn end_measured_item(&self) -> MeasuredInlineItem {
        let atom = self.edge_atom(InlineLogicalEdge::End);
        let width = inline_atom_logical_inline_size(&atom, atom.style());
        MeasuredInlineItem::new(InlineLineItem::Atom(atom), width, None)
    }
}

fn inline_fragment_continuation_edge(
    style: &ComputedStyle,
    logical_edge: InlineLogicalEdge,
    positioning_containing_block_id: Option<InlinePositioningContainingBlockId>,
) -> InlineBoxEdgeFragment {
    let edge = match logical_edge {
        InlineLogicalEdge::Start => InlineBoxEdge::Start,
        InlineLogicalEdge::End => InlineBoxEdge::End,
    };
    let (_, border, padding) = inline_box_edge_components(style, edge);
    InlineBoxEdgeFragment {
        logical_edge,
        physical_side: inline_box_edge_physical_side(style, edge),
        positioning_containing_block_id,
        advance: inline_box_edge_width(style, edge).points(),
        paint_extent: (border + padding).max(0.0),
    }
}

/// Give selected text the fragment-local inline sides created by cloned box
/// edges. Source collection can mark only the DOM scope's first and last
/// visible words; a synthetic continuation becomes a real generated fragment
/// edge only after line selection.
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
pub(super) fn mark_clone_continuation_fragment_edges(
    items: &mut [MeasuredInlineItem],
    leading_edge_count: usize,
    trailing_edge_count: usize,
) {
    if leading_edge_count != 0
        && let Some(InlineLineItem::Fragment(fragment)) = items[leading_edge_count..]
            .iter_mut()
            .map(|item| &mut item.item)
            .find(|item| matches!(item, InlineLineItem::Fragment(fragment) if fragment.source() != InlineTextSource::BidiControl))
    {
        let mut edges = fragment.hanging_edges();
        edges.blocks_start = true;
        *fragment = fragment.clone().with_hanging_edges(edges);
    }
    let trailing_content_end = items.len() - trailing_edge_count;
    if trailing_edge_count != 0
        && let Some(InlineLineItem::Fragment(fragment)) = items[..trailing_content_end]
            .iter_mut()
            .rev()
            .map(|item| &mut item.item)
            .find(|item| matches!(item, InlineLineItem::Fragment(fragment) if fragment.source() != InlineTextSource::BidiControl))
    {
        let mut edges = fragment.hanging_edges();
        edges.blocks_end = true;
        *fragment = fragment.clone().with_hanging_edges(edges);
    }
}

/// Spelling replacement adjacent to a selected discretionary line edge.
///
/// The source count is measured from the edge outward.  It is deliberately
/// independent from `InlineFragment` text so an unselected opportunity never
/// changes the logical source, bidi input, or extracted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct InlineLineEdgeReplacement {
    pub(in crate::layout) source_bytes: usize,
    pub(in crate::layout) text: &'static str,
}

/// Context required to shape a selected source edge without making the
/// context itself paintable content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::layout) enum SelectedLineShapingContext {
    #[default]
    None,
    PreserveJoining,
}

/// The source fragment whose computed style owns a selected discretionary
/// marker. Automatic opportunities use the preceding source text; an
/// authored U+00AD uses the fragment containing that control, which may be a
/// separately styled transparent inline.
///
/// <https://www.w3.org/TR/css-text-3/#hyphenation>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct DiscretionaryMarkerOwner {
    pub(in crate::layout) style_position: InlineGraphPosition,
}

/// CSS Text effects owned by a selected discretionary break.
///
/// Dictionary and authored soft-hyphen opportunities both use this record.
/// It is carried by the graph boundary rather than smuggling a synthetic
/// U+00AD through source text.  The marker is a separate used item and the
/// replacements apply only after line selection.
/// <https://drafts.csswg.org/css-text-3/#hyphenation>
/// <https://drafts.csswg.org/css-text-4/#hyphenate-character>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct DiscretionaryBreakEffect {
    pub(in crate::layout) source_boundary: InlineGraphPosition,
    pub(in crate::layout) marker_owner: DiscretionaryMarkerOwner,
    pub(in crate::layout) left_replacement: Option<InlineLineEdgeReplacement>,
    pub(in crate::layout) right_replacement: Option<InlineLineEdgeReplacement>,
    pub(in crate::layout) leading_shaping_context: SelectedLineShapingContext,
}

impl AsRef<InlineLineItem> for InlineParagraphRun {
    fn as_ref(&self) -> &InlineLineItem {
        &self.item
    }
}

/// Signed logical-inline glyph, inline-edge, or atomic advance without tracking.
///
/// CSS Text inserts tracking between typographic units, so intrinsic item
/// geometry must remain independently replaceable during contextual shaping:
/// <https://drafts.csswg.org/css-text-3/#letter-spacing-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InlineItemBaseAdvance(LayoutLength);

impl InlineItemBaseAdvance {
    pub(in crate::layout) fn from_points(points: f32) -> Self {
        Self(layout_pt(points))
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0.points()
    }
}

/// Paintless spacing inserted before one item in the final visual sequence.
///
/// CSS Text permits negative tracking, so this is a signed logical-inline
/// advance rather than a non-negative extent.
/// <https://drafts.csswg.org/css-text-3/#letter-spacing-property>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InlineBoundaryAdvance(LayoutLength);

impl InlineBoundaryAdvance {
    pub(in crate::layout) fn zero() -> Self {
        Self(layout_pt(0.0))
    }

    pub(in crate::layout) fn between(left: UsedLetterSpacing, _right: UsedLetterSpacing) -> Self {
        // Letter spacing is owned by the preceding typographic unit. A
        // following inline that resets `letter-spacing` must not remove the
        // preceding unit's trailing gap.
        // <https://drafts.csswg.org/css-text-4/#letter-spacing-property>
        Self(left.0)
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0.points()
    }
}

/// A computed `letter-spacing` value attached to one typographic unit.
///
/// <https://drafts.csswg.org/css-text-3/#letter-spacing-property>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct UsedLetterSpacing(LayoutLength);

impl UsedLetterSpacing {
    pub(in crate::layout) fn new(value: LayoutLength) -> Self {
        Self(value)
    }
}

/// The complete signed advance used by line fitting and cursor movement.
///
/// <https://drafts.csswg.org/css-text-3/#letter-spacing-property>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InlineUsedAdvance(LayoutLength);

impl InlineUsedAdvance {
    pub(in crate::layout) fn points(self) -> f32 {
        self.0.points()
    }
}

/// Separates authored/shaped geometry from CSS Text boundary spacing.
///
/// Reshaping may replace `base`, while visual-order resolution may replace
/// `boundary_before`. Neither operation can silently subtract one from the
/// other or encode the same tracking advance twice.
/// <https://drafts.csswg.org/css-text-3/#letter-spacing-property>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct MeasuredInlineAdvance {
    base: InlineItemBaseAdvance,
    boundary_before: InlineBoundaryAdvance,
}

impl MeasuredInlineAdvance {
    pub(in crate::layout) fn from_base_points(points: f32) -> Self {
        Self {
            base: InlineItemBaseAdvance::from_points(points),
            boundary_before: InlineBoundaryAdvance::zero(),
        }
    }

    pub(in crate::layout) fn base(self) -> InlineItemBaseAdvance {
        self.base
    }

    pub(in crate::layout) fn replace_base_points(&mut self, points: f32) {
        self.base = InlineItemBaseAdvance::from_points(points);
    }

    pub(in crate::layout) fn boundary_before(self) -> InlineBoundaryAdvance {
        self.boundary_before
    }

    pub(in crate::layout) fn set_boundary_before(&mut self, advance: InlineBoundaryAdvance) {
        self.boundary_before = advance;
    }

    pub(in crate::layout) fn clear_boundary_before(&mut self) {
        self.boundary_before = InlineBoundaryAdvance::zero();
    }

    pub(in crate::layout) fn used(self) -> InlineUsedAdvance {
        InlineUsedAdvance(layout_pt(
            self.base.points() + self.boundary_before.points(),
        ))
    }
}

#[derive(Debug, Clone)]
/// One selected inline item and its explicit base-plus-boundary measurement.
///
/// <https://www.w3.org/TR/css-inline-3/#line-box>
pub(in crate::layout) struct MeasuredInlineItem {
    pub(in crate::layout) item: InlineLineItem,
    pub(in crate::layout) advance: MeasuredInlineAdvance,
    pub(in crate::layout) shaped: Option<Rc<ShapedInlineLine>>,
}

impl MeasuredInlineItem {
    pub(in crate::layout) fn new(
        item: InlineLineItem,
        base_advance_points: f32,
        shaped: Option<Rc<ShapedInlineLine>>,
    ) -> Self {
        Self {
            item,
            advance: MeasuredInlineAdvance::from_base_points(base_advance_points),
            shaped,
        }
    }

    pub(in crate::layout) fn base_advance(&self) -> InlineItemBaseAdvance {
        self.advance.base()
    }

    pub(in crate::layout) fn used_advance(&self) -> InlineUsedAdvance {
        self.advance.used()
    }
}

impl AsRef<InlineLineItem> for MeasuredInlineItem {
    fn as_ref(&self) -> &InlineLineItem {
        &self.item
    }
}

/// A graph-selected line after CSS Text line-edge effects are applied.
///
/// CSS Text defines several effects at the selected break boundary rather than
/// at text collection time: collapsible edge spaces are removed, preserved
/// `pre-wrap` spaces may hang, soft hyphens become visible only at the chosen
/// hyphenation break, and zero-width break controls disappear before painting.
/// Carrying the resulting items and deducted widths from the graph keeps mixed
/// layout, text-only layout, intrinsic sizing, and future fragmentation on the
/// same materialized line model:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
/// CSS Text Phase II effects selected at one line edge.
///
/// Source items are deliberately retained; consumers use these used-advance
/// deductions instead of deleting source text during collection.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
/// The selected source behavior at one line edge.
///
/// CSS Text Phase II changes used geometry without deleting the corresponding
/// source. Keeping the effect keyed to a selected item range lets paint,
/// decorations, and PDF extraction distinguish the source owner from the
/// width deducted for fitting.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineLineEdgeEffectKind {
    CollapsedEndTrim,
    PreWrapHang,
    UnconditionalSeparatorHang,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct InlineLineEdgeEffect {
    pub(in crate::layout) kind: InlineLineEdgeEffectKind,
    pub(in crate::layout) item_index: usize,
    pub(in crate::layout) source_range: std::ops::Range<usize>,
}

#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct InlineLineEdgeEffects {
    /// CSS Text Phase II removes this advance from the selected line measure.
    /// The corresponding source fragments remain in `InlineLineFragment` for
    /// visual painting, text decorations, and extraction.
    /// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
    pub(in crate::layout) collapsed_end_trim_width: f32,
    pub(in crate::layout) pre_wrap_hanging_width: f32,
    pub(in crate::layout) hanging_space_separator_width: f32,
    /// A selected `break-spaces` boundary retains a document-space advance.
    /// In an RTL paragraph its UAX #9 input needs a virtual LTR continuation
    /// so Phase II's logical line-end space remains at the visual inline
    /// start, rather than being reset as trailing whitespace by rule L1.
    /// The control is bidi-only: it never enters source text or extraction.
    /// <https://www.w3.org/TR/css-text-3/#valdef-white-space-break-spaces>
    /// <https://www.unicode.org/reports/tr9/#L1>
    pub(in crate::layout) retained_break_spaces_end: bool,
    /// Source-owned Phase II effects, in selected item coordinates.
    pub(in crate::layout) source_effects: Rc<[InlineLineEdgeEffect]>,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct MaterializedInlineGraphLine {
    pub(in crate::layout) items: Vec<MeasuredInlineItem>,
    /// Advance used while choosing a break candidate. Once a candidate creates
    /// a line edge, CSS Text Phase II trimming and hanging apply to that edge
    /// before the candidate is compared with the available measure.
    pub(in crate::layout) fitting_width: f32,
    pub(in crate::layout) content_width: f32,
    pub(in crate::layout) edge_effects: InlineLineEdgeEffects,
}

/// The source condition that established a selected line's end.
///
/// CSS Text conditionally hangs a preserved `pre-wrap` suffix at a terminal
/// edge only when that suffix would otherwise overflow. A selected soft-wrap
/// boundary always hangs its preserved suffix. Intrinsic min-content segments
/// model the latter behavior without a physical available width.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum SelectedLineEndCondition {
    SoftWrap,
    ForcedBreak,
    ParagraphEnd,
    IntrinsicSegmentEnd,
}

impl SelectedLineEndCondition {
    pub(super) fn pre_wrap_hanging_width(
        self,
        suffix_width: f32,
        untrimmed_width: f32,
        available_width: Option<f32>,
    ) -> f32 {
        match self {
            Self::SoftWrap | Self::IntrinsicSegmentEnd => suffix_width,
            // Conditional hanging retains the portion of the preserved
            // sequence that fits the available measure. Only its overflowing
            // suffix hangs, so `text-align` still sees enough space to fill
            // the line before its source glyphs are positioned.
            Self::ForcedBreak | Self::ParagraphEnd => available_width
                .map(|available_width| (untrimmed_width - available_width).clamp(0.0, suffix_width))
                .unwrap_or(0.0),
        }
    }
}

impl MaterializedInlineGraphLine {
    /// Assemble source-faithful text only when a durable line fragment needs
    /// it for painting or extraction. Speculative line materializations use
    /// only their measured items and widths, so retaining this string there
    /// would copy every selected source range unnecessarily.
    pub(in crate::layout) fn used_text(&self) -> String {
        let mut text = text_for_measured_items(&self.items);
        if self.edge_effects.collapsed_end_trim_width > 0.0 {
            text.truncate(text.trim_end_matches(is_css_collapsible_whitespace).len());
        }
        text
    }

    /// Check the used text without assembling an owned line string.
    pub(in crate::layout) fn used_text_contains_whitespace(&self) -> bool {
        if self.edge_effects.collapsed_end_trim_width == 0.0 {
            return self.items.iter().any(|item| {
                matches!(&item.item, InlineLineItem::Fragment(fragment) if fragment.text().chars().any(char::is_whitespace))
            });
        }

        let mut trimming_trailing_space = true;
        for item in self.items.iter().rev() {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                continue;
            };
            for character in fragment.text().chars().rev() {
                if trimming_trailing_space && is_css_collapsible_whitespace(character) {
                    continue;
                }
                trimming_trailing_space = false;
                if character.is_whitespace() {
                    return true;
                }
            }
        }
        false
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct InlineContentWidth {
    /// Advance used to choose a selected line edge. Unconditional hanging
    /// Unicode space separators are painted outside the formatted line and
    /// therefore do not make a candidate overflow. Tracking is already absent
    /// at line edges in `MeasuredInlineAdvance`.
    pub(in crate::layout) fitting_width: f32,
    pub(in crate::layout) content_width: f32,
    pub(in crate::layout) trailing_space_width: f32,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct BorrowedInlineLineMeasurement {
    pub(in crate::layout) content_width: f32,
}

pub(in crate::layout) fn measured_inline_items(
    items: &[MeasuredInlineItem],
) -> Vec<InlineLineItem> {
    items.iter().map(|item| item.item.clone()).collect()
}

pub(in crate::layout) fn inline_content_width_for_line_items<T, F>(
    items: &[T],
    font_system: &mut FontSystem,
    mut item_width: F,
) -> InlineContentWidth
where
    T: AsRef<InlineLineItem>,
    F: FnMut(&T) -> f32,
{
    let raw_width = items.iter().map(&mut item_width).sum::<f32>();
    let trailing_space_width =
        trailing_hanging_space_separator_width_for_line_items(items, font_system);
    InlineContentWidth {
        fitting_width: (raw_width - trailing_space_width).max(0.0),
        content_width: (raw_width - trailing_space_width).max(0.0),
        trailing_space_width,
    }
}

/// Resolve generated `leader()` atoms inside one selected graph line.
///
/// CSS Generated Content leaders fill the remaining inline measure of the
/// selected line, but they still participate in CSS Text collection as
/// generated inline content. Resolving them after graph line selection and
/// before durable line records are built keeps fragmentation, painting, text
/// extraction, and page-margin/generated text on the same line artifact:
/// <https://www.w3.org/TR/css-content-3/#leaders>.
pub(in crate::layout) fn resolve_materialized_line_leaders(
    line: &mut MaterializedInlineGraphLine,
    font_system: &mut FontSystem,
    available_inline_width: f32,
) {
    let leader_count = line
        .items
        .iter()
        .filter(|item| {
            matches!(
                &item.item,
                InlineLineItem::Atom(atom) if matches!(atom.content(), InlineAtomContent::Leader(_))
            )
        })
        .count();
    if leader_count == 0 {
        return;
    }

    let old_trailing_space_width =
        trailing_hanging_space_separator_width_for_line_items(&line.items, font_system);
    let consumed_pre_wrap_width = (line.edge_effects.pre_wrap_hanging_width
        + line.edge_effects.hanging_space_separator_width
        - old_trailing_space_width)
        .max(0.0);
    let mut remaining_inline_width = (available_inline_width - line.content_width).max(0.0);
    let mut remaining_leaders = leader_count;
    let mut resolved_items = Vec::with_capacity(line.items.len());

    for item in std::mem::take(&mut line.items) {
        let InlineLineItem::Atom(atom) = &item.item else {
            resolved_items.push(item);
            continue;
        };
        let InlineAtomContent::Leader(pattern) = atom.content() else {
            resolved_items.push(item);
            continue;
        };

        let pattern_width = font_system.measure_text(pattern, atom.style());
        let leader_share = if remaining_leaders > 0 {
            remaining_inline_width / remaining_leaders as f32
        } else {
            0.0
        };
        remaining_leaders = remaining_leaders.saturating_sub(1);
        // Intrinsic inline measurement deliberately supplies an unbounded
        // available width.  A leader fills *remaining* line space, so it has
        // no finite intrinsic contribution in that pass; expanding it here
        // would convert infinity to `usize::MAX` and attempt an impossible
        // allocation.  The later, used-width materialization receives the
        // finite line width and expands the leader normally.
        if pattern.is_empty()
            || pattern_width <= 0.0
            || leader_share <= 0.0
            || !leader_share.is_finite()
            || available_inline_width == f32::MAX
        {
            continue;
        }

        let repeat_count = (leader_share / pattern_width).floor() as usize;
        if repeat_count == 0 {
            continue;
        }
        let text = pattern.repeat(repeat_count);
        let fragment = InlineFragment::new(
            text,
            atom.style().clone(),
            atom.baseline_shift,
            atom.link_target().map(ToOwned::to_owned),
            false,
            InlineTextSource::Normal,
            true,
            InlineHangingEdges::default(),
            Vec::new(),
        )
        .with_visual_offset(atom.visual_offset);
        let shaped = font_system.shape_unwrapped_line(
            fragment.text(),
            fragment.style(),
            fragment.style().line_height,
        );
        let width = shaped
            .as_ref()
            .map(ShapedInlineLine::advance_width)
            .unwrap_or(0.0);
        let shaped = shaped.map(Rc::new);
        remaining_inline_width = (remaining_inline_width - width).max(0.0);
        resolved_items.push(MeasuredInlineItem::new(
            InlineLineItem::Fragment(fragment),
            width,
            shaped,
        ));
    }

    line.items = resolved_items;
    let widths = inline_content_width_for_line_items(&line.items, font_system, |item| {
        item.used_advance().points()
    });
    line.edge_effects.pre_wrap_hanging_width = consumed_pre_wrap_width;
    line.edge_effects.hanging_space_separator_width = widths.trailing_space_width;
    line.fitting_width = (widths.fitting_width - consumed_pre_wrap_width).max(0.0);
    line.content_width = (widths.content_width - consumed_pre_wrap_width).max(0.0);
}

/// A byte-accurate position in an inline opportunity graph.
///
/// Positions at `byte_offset == 0` are run boundaries. Text runs may also expose
/// interior UTF-8 boundary offsets, allowing CSS Text break opportunities inside
/// one shaped run without splitting the owning inline box:
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::layout) struct InlineGraphPosition {
    pub(in crate::layout) run_index: usize,
    pub(in crate::layout) byte_offset: usize,
}

impl InlineGraphPosition {
    pub(in crate::layout) const fn at_run_start(run_index: usize) -> Self {
        Self {
            run_index,
            byte_offset: 0,
        }
    }
}

/// A selected half-open range in an inline opportunity graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct InlineGraphRange {
    pub(in crate::layout) start: InlineGraphPosition,
    pub(in crate::layout) end: InlineGraphPosition,
}

/// An unbreakable source continuation that contains an inline right-float
/// marker.
///
/// The source range starts after the preceding candidate soft wrap and ends
/// at the continuation's next soft wrap. The marker remains zero-width
/// source-order state; it is not a CSS Text line break.
/// <https://www.w3.org/TR/css-text-3/#white-space-property>
/// <https://www.w3.org/TR/CSS22/visuren.html#float-position>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct UnbreakableInlineFloatContinuation {
    pub(in crate::layout) source_range: InlineGraphRange,
    pub(in crate::layout) marker: InlineGraphPosition,
}

/// The kind of CSS Text break represented at one graph boundary.
///
/// CSS Text assigns different effects to soft wraps, preserved spaces,
/// hyphenation, emergency breaks, atomic inline boundaries, and hanging
/// punctuation candidates. Keeping this as structured data prevents line
/// fitting, intrinsic sizing, and fragmentation from re-discovering the same
/// facts through ad hoc string inspection:
/// <https://www.w3.org/TR/css-text-3/#line-breaking>,
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>,
/// <https://www.w3.org/TR/css-text-3/#hyphenation>, and
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum BreakEffect {
    Forced,
    SoftWrap,
    /// An author-supplied virtual word separator (U+200B or HTML `<wbr>`).
    ///
    /// This remains an ordinary soft-wrap opportunity, but keeping its
    /// source distinct prevents the zero-advance control and its neighboring
    /// text-run edge from accidentally producing two intrinsic segments.
    /// <https://drafts.csswg.org/css-text-4/#word-space-transform>
    ExplicitVirtual,
    PreservedSpace,
    BreakSpaces,
    Hyphenation,
    Emergency,
    AtomicBoundary,
    /// A zero-advance source-order checkpoint used to place an inline float.
    ///
    /// This is deliberately distinct from a CSS Text soft-wrap opportunity:
    /// floats affect line-box geometry, but out-of-flow elements do not split
    /// adjacent in-flow text.
    /// <https://drafts.csswg.org/css-text-3/#line-break-details>
    FloatPlacement,
}

/// CSS Text Phase II whitespace behavior selected at a line boundary.
///
/// This is independent from [`BreakEffect`]: the latter records why a break
/// is available, while this records whether selected source is collapsed,
/// hangs, or remains part of the line's used advance.
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum SelectedWhitespaceEdge {
    None,
    CollapseAtNextLineStart,
    PreWrapHang,
    BreakSpacesRetained,
}

impl SelectedWhitespaceEdge {
    pub(in crate::layout) const fn trims_next_line_start(self) -> bool {
        matches!(self, Self::CollapseAtNextLineStart)
    }

    pub(in crate::layout) const fn hangs_from_fitting_measure(self) -> bool {
        matches!(self, Self::PreWrapHang)
    }

    pub(in crate::layout) const fn retains_break_spaces_advance(self) -> bool {
        matches!(self, Self::BreakSpacesRetained)
    }
}

/// Compatibility name for the graph's break effect.
///
/// `BreakEffect` is the preferred name: it describes what happens when a
/// boundary wins selection, while [`BreakAvailability`] independently models
/// when that boundary may be selected.
pub(in crate::layout) type InlineBreakKind = BreakEffect;

/// The CSS policy that makes a break candidate available for line fitting.
///
/// This is deliberately independent of [`BreakEffect`]. For example, an
/// authored soft hyphen remains a discretionary effect whether it is an
/// ordinary break or is temporarily withheld by `word-break: auto-phrase`.
/// Encoding the distinction prevents intrinsic sizing from accidentally
/// treating a last-resort candidate as an ordinary wrap.
/// <https://drafts.csswg.org/css-text-4/#word-break-property>
/// <https://drafts.csswg.org/css-text-3/#overflow-wrap-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::layout) enum BreakAvailability {
    Ordinary,
    RelaxedWordBreak(WordBreakRelaxation),
    OverflowWrap(OverflowWrapFallback),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::layout) enum WordBreakRelaxation {
    KeepAll,
    AutoPhraseWrap,
    AutoPhraseHyphenation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::layout) enum OverflowWrapFallback {
    Anywhere,
    BreakWord,
}

impl BreakAvailability {
    /// Rank candidates by the CSS fallback stage in which they become legal.
    /// Lower ranks are always preferred over higher ranks by line fitting.
    pub(in crate::layout) const fn fitting_stage(self) -> u8 {
        match self {
            Self::Ordinary => 0,
            Self::RelaxedWordBreak(WordBreakRelaxation::KeepAll) => 1,
            Self::RelaxedWordBreak(WordBreakRelaxation::AutoPhraseWrap) => 1,
            Self::RelaxedWordBreak(WordBreakRelaxation::AutoPhraseHyphenation) => 2,
            Self::OverflowWrap(_) => 3,
        }
    }

    pub(in crate::layout) const fn is_fallback(self) -> bool {
        !matches!(self, Self::Ordinary)
    }

    /// CSS Sizing includes ordinary wraps and `overflow-wrap:anywhere`, but
    /// not relaxed `word-break` or `break-word` fallbacks.
    pub(in crate::layout) const fn participates_in_min_content(self) -> bool {
        matches!(
            self,
            Self::Ordinary | Self::OverflowWrap(OverflowWrapFallback::Anywhere)
        )
    }
}

/// Translate a source-policy decision into the availability consumed by line
/// fitting. A disabled policy deliberately emits no graph opportunity.
///
/// <https://drafts.csswg.org/css-text-4/#word-break-property>
pub(in crate::layout) const fn discretionary_hyphenation_availability(
    policy: DiscretionaryHyphenationPolicy,
) -> Option<BreakAvailability> {
    match policy {
        DiscretionaryHyphenationPolicy::Disabled => None,
        DiscretionaryHyphenationPolicy::Ordinary => Some(BreakAvailability::Ordinary),
        DiscretionaryHyphenationPolicy::DeferredForAutoPhrase => Some(
            BreakAvailability::RelaxedWordBreak(WordBreakRelaxation::AutoPhraseHyphenation),
        ),
    }
}

/// A legal line-break opportunity in an inline paragraph graph.
///
/// The `index` is a run boundary: `0` is before the first run and `n` is after
/// `runs[n - 1]`. CSS Text line fitting chooses from these boundaries, then
/// applies the recorded trimming/hanging/soft-hyphen effects to materialize the
/// line fragment:
/// <https://www.w3.org/TR/css-text-3/#line-breaking> and
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct InlineBreakOpportunity {
    pub(in crate::layout) position: InlineGraphPosition,
    pub(in crate::layout) kind: BreakEffect,
    pub(in crate::layout) availability: BreakAvailability,
    pub(in crate::layout) whitespace_edge: SelectedWhitespaceEdge,
    /// Used behavior at this boundary.  `soft_hyphen` remains a compact
    /// classification for legacy priority/min-content policy; consumers that
    /// materialize the selected line must use this record instead.
    pub(in crate::layout) discretionary: Option<DiscretionaryBreakEffect>,
}

impl InlineBreakOpportunity {
    pub(in crate::layout) const fn trims_next_line_start(self) -> bool {
        self.whitespace_edge.trims_next_line_start()
    }

    pub(in crate::layout) const fn hangs_from_fitting_measure(self) -> bool {
        self.whitespace_edge.hangs_from_fitting_measure()
    }

    pub(in crate::layout) const fn is_discretionary(self) -> bool {
        self.discretionary.is_some()
    }
}

/// A CSS Text break-opportunity graph for one inline paragraph.
///
/// CSS Sizing defines intrinsic inline contributions in terms of the same
/// soft-wrap opportunities used by normal line layout. This graph is therefore
/// shared by line selection, intrinsic measurement, and future fragmentation
/// decisions instead of making each subsystem measure text independently:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic> and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineOpportunityGraph {
    pub(in crate::layout) runs: Vec<InlineParagraphRun>,
    pub(in crate::layout) opportunities: Vec<InlineBreakOpportunity>,
    /// Lexically derived `wrap-inside: avoid` containment for each legal
    /// candidate. This is graph metadata rather than an inherited text style:
    /// <https://www.w3.org/TR/css-text-4/#wrap-inside-property>.
    pub(super) wrap_inside_avoid_depths: BTreeMap<InlineGraphPosition, u16>,
    /// Immutable, provenance-safe source advances for all graph boundaries.
    /// `None` caches the fact that this graph needs conventional shaped-line
    /// materialization instead.
    pub(super) monotonic_source_measurement: RefCell<Option<Option<Rc<InlineLineMeasureIndex>>>>,
}

/// One graph-wide source-advance table shared by every greedy line cursor.
///
/// Its values are measured from the paragraph start, so a later line only
/// subtracts its selected start. Constructing a separate prefix table for
/// every selected line would rescan the remaining `break-all` boundaries and
/// reintroduce quadratic fitting work.
#[derive(Debug, Clone)]
pub(super) struct InlineLineMeasureIndex {
    pub(super) run_starts: Vec<f32>,
    pub(super) opportunity_advances: Vec<f32>,
}

/// A borrowed suffix of one graph-wide source-advance table.
pub(in crate::layout) struct LineMeasureCursor<'a> {
    pub(super) opportunities: &'a [InlineBreakOpportunity],
    pub(super) index: Rc<InlineLineMeasureIndex>,
    pub(super) first_opportunity: usize,
    pub(super) start_advance: f32,
}

impl LineMeasureCursor<'_> {
    /// Return the last legal boundary whose provenance-safe source advance
    /// fits the available inline size, with the normal glyph-rounding
    /// tolerance used by shaped-line fitting.
    pub(in crate::layout) fn last_fitting(
        &self,
        available_width: f32,
    ) -> Option<InlineBreakOpportunity> {
        let first_too_wide = self.index.opportunity_advances[self.first_opportunity..]
            .partition_point(|advance| *advance - self.start_advance <= available_width + 0.5);
        self.opportunities
            .get(first_too_wide.saturating_sub(1))
            .copied()
    }

    /// Return the earliest legal source boundary for the required-progress
    /// fallback when a zero-width or narrower-than-one-unit line has no
    /// fitting candidate.
    pub(in crate::layout) fn first(&self) -> Option<InlineBreakOpportunity> {
        self.opportunities.first().copied()
    }
}

pub(super) fn inline_box_edge_is_wrap_inside_avoid_start(item: &InlineLineItem) -> bool {
    matches!(
        item,
        InlineLineItem::Atom(atom)
            if matches!(
                atom.content(),
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge))
                    if edge.logical_edge == InlineLogicalEdge::Start
            ) && matches!(atom.style().wrap_inside, css::WrapInside::Avoid)
    )
}

pub(super) fn inline_box_edge_is_wrap_inside_avoid_end(item: &InlineLineItem) -> bool {
    matches!(
        item,
        InlineLineItem::Atom(atom)
            if matches!(
                atom.content(),
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge))
                    if edge.logical_edge == InlineLogicalEdge::End
            ) && matches!(atom.style().wrap_inside, css::WrapInside::Avoid)
    )
}

/// A selected reusable line fragment from an inline opportunity graph.
///
/// CSS Fragmentation, CSS Inline painting, and PDF emission all consume the
/// same selected line geometry: line metrics, float band, indentation, visual
/// text summary, and the ordered line items that will be materialized into
/// durable shaped paint groups:
/// <https://www.w3.org/TR/css-inline-3/#line-box>,
/// <https://www.w3.org/TR/css-break-3/#widows-orphans>, and
/// ISO 32000-2:2020, 9.4 "Text".
#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineLineFragment {
    pub(in crate::layout) items: Rc<[MeasuredInlineItem]>,
    pub(in crate::layout) metrics: InlineLineMetrics,
    pub(in crate::layout) hanging_widths: HangingPunctuationWidths,
    pub(in crate::layout) indent: f32,
    /// The logical inline origin used to paint source content. When absent,
    /// the selected exclusion-band indent remains the paint origin.
    pub(in crate::layout) source_paint_indent: Option<f32>,
    pub(in crate::layout) available_width: f32,
    /// Immutable policy for resolving float exclusions when this selected
    /// source line is replayed.  This is deliberately a single value rather
    /// than a page index plus a boolean: a replay site must state whether it
    /// may consult ambient float state or must use the selection transaction.
    pub(in crate::layout) float_replay: InlineFloatReplay,
    pub(in crate::layout) edge_effects: InlineLineEdgeEffects,
    pub(in crate::layout) bidi_scope_continuations: BidiLineScopeContinuations,
    pub(in crate::layout) text: Rc<str>,
    /// Block-axis trim selected for this line's inline box. It is carried
    /// through the durable line fragment so paint-time links and decorations
    /// share the background's trimmed content rectangle.
    pub(in crate::layout) text_box_trim: TextBoxLineTrim,
    /// The selected graph boundary after this line's source. It is retained
    /// so automatic clamping can stay attached to source across a balanced
    /// reflow instead of reusing a raw line ordinal.
    pub(in crate::layout) source_end: Option<InlineGraphPosition>,
    /// The `::first-line` used style has already been materialized into the
    /// selected items, so paint must not overlay it a second time.
    pub(in crate::layout) first_line_style_materialized: bool,
}

impl InlineLineFragment {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn new(
        items: Vec<MeasuredInlineItem>,
        metrics: InlineLineMetrics,
        hanging_widths: HangingPunctuationWidths,
        indent: f32,
        available_width: f32,
        selected_float_page_index: usize,
        text: impl Into<String>,
    ) -> Self {
        Self {
            items: Rc::from(items.into_boxed_slice()),
            metrics,
            hanging_widths,
            indent,
            source_paint_indent: None,
            available_width,
            float_replay: InlineFloatReplay::RequeryContainingBlock {
                selected_float_page_index,
            },
            edge_effects: InlineLineEdgeEffects::default(),
            bidi_scope_continuations: BidiLineScopeContinuations::default(),
            text: Rc::from(text.into()),
            text_box_trim: TextBoxLineTrim::default(),
            source_end: None,
            first_line_style_materialized: false,
        }
    }

    pub(in crate::layout) fn items(&self) -> &[MeasuredInlineItem] {
        &self.items
    }

    pub(in crate::layout) fn text(&self) -> &str {
        &self.text
    }

    pub(in crate::layout) fn with_source_paint_indent(mut self, indent: f32) -> Self {
        self.source_paint_indent = Some(indent);
        self
    }

    pub(in crate::layout) fn with_edge_effects(
        mut self,
        edge_effects: InlineLineEdgeEffects,
    ) -> Self {
        self.edge_effects = edge_effects;
        self
    }

    pub(in crate::layout) fn with_bidi_scope_continuations(
        mut self,
        continuations: BidiLineScopeContinuations,
    ) -> Self {
        self.bidi_scope_continuations = continuations;
        self
    }

    pub(in crate::layout) fn with_source_end(mut self, source_end: InlineGraphPosition) -> Self {
        self.source_end = Some(source_end);
        self
    }

    pub(in crate::layout) fn mark_first_line_style_materialized(&mut self) {
        self.first_line_style_materialized = true;
    }

    /// Preserve the float band selected by this source line when it is
    /// replayed on the same fragmentainer.  A relocated line still resolves
    /// exclusions at its destination.
    pub(in crate::layout) fn freeze_float_band(&mut self) {
        self.float_replay = self.float_replay.freeze_selected_band();
    }

    /// Restore ordinary containing-block float resolution after an abandoned
    /// speculative placement.
    pub(in crate::layout) fn requery_float_band(&mut self) {
        self.float_replay = self.float_replay.requery_containing_block();
    }

    pub(in crate::layout) fn with_float_replay(mut self, replay: InlineFloatReplay) -> Self {
        self.float_replay = replay;
        self
    }
}
