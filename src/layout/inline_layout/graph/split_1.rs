use std::borrow::Cow;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::super::mixed::apply_visual_tracking_boundaries;
use super::*;
use crate::css::{BoxDecorationBreak, DiscretionaryHyphenationPolicy, Hyphens};
use crate::layout::inline_collect::{
    InlineBoxEdge, autospace_boundary_character_at_end, autospace_boundary_character_at_start,
    inline_box_edge_components, inline_box_edge_physical_side, inline_box_edge_width,
    text_autospace_boundary_needs_spacing,
};
use crate::text::{
    CursiveProtectedUnitRanges, DiscretionaryOpportunity, LanguageDiscretionaryReplacement,
    TextBreakPolicy, automatic_hyphenation_opportunities, character_is_css_other_space_separator,
    character_is_default_ignorable_code_point, character_is_first_letter_associated_space,
    character_is_first_letter_suffix_punctuation, character_is_unicode_first_letter_base,
    character_is_unicode_mark, character_is_unicode_punctuation,
    collect_measured_break_opportunities, hyphenator_for_language,
    manual_hyphenation_opportunities,
};

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

    fn start_measured_item(&self) -> MeasuredInlineItem {
        let atom = self.edge_atom(InlineLogicalEdge::Start);
        let width = inline_atom_logical_inline_size(&atom, atom.style());
        MeasuredInlineItem::new(InlineLineItem::Atom(atom), width, None)
    }

    fn end_measured_item(&self) -> MeasuredInlineItem {
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
fn mark_clone_continuation_fragment_edges(
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

    pub(in crate::layout) fn between(left: UsedLetterSpacing, right: UsedLetterSpacing) -> Self {
        Self(layout_pt((left.points() + right.points()) * 0.5))
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

    pub(in crate::layout) fn points(self) -> f32 {
        self.0.points()
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
    fn pre_wrap_hanging_width(
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
    wrap_inside_avoid_depths: BTreeMap<InlineGraphPosition, u16>,
}

/// UAX #9 controls that must be virtually restored around a selected
/// soft-wrapped line.
///
/// The controls are UAX #9 input only: they are never measured, painted, or
/// exposed for extraction. They keep a CSS bidi scope intact while UAX #9
/// resolves one formatted line at a time. This applies equally to controls
/// authored in text and controls synthesized for CSS `unicode-bidi`.
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi> and
/// <https://www.unicode.org/reports/tr9/#Explicit_Levels_and_Directions>.
#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct BidiLineScopeContinuations {
    /// A non-painting parent-paragraph directional context restored before a
    /// wrapped isolate. This is separate from the scope control itself: CSS
    /// resolves an isolate as a U+FFFC-like neutral in its parent paragraph,
    /// so its edge neutrals retain the nearest parent strong direction even
    /// when that text was selected onto another line.
    pub(in crate::layout) prefix_parent_context: String,
    pub(in crate::layout) prefix: String,
    pub(in crate::layout) suffix: String,
    /// Virtual bidi-only text placed immediately after selected source text.
    /// This is distinct from `suffix`: CSS scope terminators must remain after
    /// the line-edge context they balance.
    pub(in crate::layout) trailing_line_edge_context: String,
    /// See `prefix_parent_context`.
    pub(in crate::layout) suffix_parent_context: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BidiControlScope {
    start: char,
    end: char,
    is_isolate: bool,
}

impl InlineOpportunityGraph {
    /// Return cloneable inline scopes that are lexically open immediately
    /// before `position`. The stack retains non-clone scopes as empty entries
    /// while scanning so a nested clone scope is paired with its real source
    /// end rather than an unrelated outer edge.
    fn clone_scopes_before(
        &self,
        position: InlineGraphPosition,
    ) -> Vec<InlineFragmentContinuation> {
        let mut scopes = Vec::<Option<InlineFragmentContinuation>>::new();
        for run in self.runs.iter().take(position.run_index) {
            let InlineLineItem::Atom(atom) = &run.item else {
                continue;
            };
            let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content()
            else {
                continue;
            };
            // A positioned bidi isolate carries a separate zero-advance
            // marker *inside* its authored isolate controls. It establishes
            // containing-block identity, but it is not a box-decoration
            // boundary and must not be replayed outside the virtual controls
            // added to a continuation line.
            if edge.is_positioning_marker() {
                continue;
            }
            match edge.logical_edge {
                InlineLogicalEdge::Start => {
                    scopes.push(InlineFragmentContinuation::from_source_start(atom));
                }
                InlineLogicalEdge::End => {
                    scopes.pop();
                }
            }
        }
        scopes.into_iter().flatten().collect()
    }

    /// Add the synthetic edge atoms required by `box-decoration-break: clone`
    /// to one selected graph range. Source-owned edges remain in the range;
    /// only scopes that were already open at the leading/trailing selected
    /// boundary receive continuation chrome.
    fn insert_clone_continuation_edges(
        &self,
        range: InlineGraphRange,
        items: &mut Vec<MeasuredInlineItem>,
    ) {
        let leading = self.clone_scopes_before(range.start);
        let trailing = self.clone_scopes_before(range.end);
        if leading.is_empty() && trailing.is_empty() {
            return;
        }
        let mut continued = Vec::with_capacity(leading.len() + items.len() + trailing.len());
        continued.extend(
            leading
                .iter()
                .map(InlineFragmentContinuation::start_measured_item),
        );
        continued.append(items);
        continued.extend(
            trailing
                .iter()
                .rev()
                .map(InlineFragmentContinuation::end_measured_item),
        );
        mark_clone_continuation_fragment_edges(&mut continued, leading.len(), trailing.len());
        *items = continued;
    }

    /// Return virtual UAX #9 controls needed to balance one selected line.
    ///
    /// An isolate is one U+FFFC-like object to its containing bidi paragraph,
    /// even when line breaking selects only a middle fragment of the isolate.
    /// Reopening the scopes active before the selected range and closing
    /// scopes still active after it gives UAX #9 that same scoped input
    /// without adding glyphs or source text to the line.
    pub(in crate::layout) fn bidi_scope_continuations_for_range(
        &self,
        range: InlineGraphRange,
    ) -> BidiLineScopeContinuations {
        let scopes_before_start = self.bidi_control_scopes_before(range.start);
        let scopes_before_end = self.bidi_control_scopes_before(range.end);
        BidiLineScopeContinuations {
            prefix_parent_context: scopes_before_start
                .iter()
                .any(|scope| scope.is_isolate)
                .then(|| self.parent_direction_before(range.start))
                .flatten()
                .map(bidi_prefix_parent_context_control)
                .unwrap_or_default()
                .to_owned(),
            prefix: scopes_before_start
                .iter()
                .map(|scope| scope.start)
                .collect(),
            suffix: scopes_before_end
                .iter()
                .rev()
                .map(|scope| scope.end)
                .collect(),
            trailing_line_edge_context: String::new(),
            suffix_parent_context: scopes_before_end
                .iter()
                .any(|scope| scope.is_isolate)
                .then(|| {
                    self.parent_direction_after(range.end)
                        .or_else(|| self.parent_direction_before(range.end))
                })
                .flatten()
                .map(bidi_suffix_parent_context_control)
                .unwrap_or_default()
                .to_owned(),
        }
    }

    fn parent_direction_before(&self, position: InlineGraphPosition) -> Option<Direction> {
        let mut scopes = Vec::new();
        let mut direction = None;
        for (run_index, run) in self.runs.iter().enumerate() {
            if run_index > position.run_index {
                break;
            }
            let InlineLineItem::Fragment(fragment) = &run.item else {
                continue;
            };
            let end = if run_index == position.run_index {
                position.byte_offset.min(fragment.text().len())
            } else {
                fragment.text().len()
            };
            let Some(text) = fragment.text().get(..end) else {
                continue;
            };
            if scopes.is_empty()
                && let Some(found) = plaintext_direction_for_text(text)
            {
                direction = Some(found);
            }
            update_bidi_control_scope_stack(&mut scopes, text);
        }
        direction
    }

    fn parent_direction_after(&self, position: InlineGraphPosition) -> Option<Direction> {
        let mut scopes = self.bidi_control_scopes_before(position);
        for (run_index, run) in self.runs.iter().enumerate().skip(position.run_index) {
            let InlineLineItem::Fragment(fragment) = &run.item else {
                continue;
            };
            let start = if run_index == position.run_index {
                position.byte_offset.min(fragment.text().len())
            } else {
                0
            };
            let Some(text) = fragment.text().get(start..) else {
                continue;
            };
            if scopes.is_empty()
                && let Some(direction) = plaintext_direction_for_text(text)
            {
                return Some(direction);
            }
            update_bidi_control_scope_stack(&mut scopes, text);
        }
        None
    }

    fn bidi_control_scopes_before(&self, position: InlineGraphPosition) -> Vec<BidiControlScope> {
        let mut scopes = Vec::new();
        for (run_index, run) in self.runs.iter().enumerate() {
            if run_index > position.run_index {
                break;
            }
            let InlineLineItem::Fragment(fragment) = &run.item else {
                continue;
            };
            let end = if run_index == position.run_index {
                position.byte_offset.min(fragment.text().len())
            } else {
                fragment.text().len()
            };
            if let Some(text) = fragment.text().get(..end) {
                update_bidi_control_scope_stack(&mut scopes, text);
            }
        }
        scopes
    }
}

/// Apply the UAX #9 explicit-formatting controls in `text` to an open scope
/// stack. The stack preserves only the information needed to replay a sliced
/// line's control prefix/suffix; paragraph-level level resolution remains the
/// responsibility of the bidi shaper.
///
/// `PDI` terminates its isolate and any embeddings nested inside it, while
/// `PDF` terminates only an immediately active embedding or override. Invalid
/// unmatched terminators are ignored, as required by UAX #9 X7/X8.
/// <https://www.unicode.org/reports/tr9/#Explicit_Levels_and_Directions>.
fn update_bidi_control_scope_stack(scopes: &mut Vec<BidiControlScope>, text: &str) {
    for character in text.chars() {
        let Some(scope) = bidi_control_scope_start(character) else {
            match character {
                '\u{202c}' => {
                    if scopes.last().is_some_and(|scope| !scope.is_isolate) {
                        scopes.pop();
                    }
                }
                '\u{2069}' => {
                    if let Some(isolate_index) = scopes.iter().rposition(|scope| scope.is_isolate) {
                        scopes.truncate(isolate_index);
                    }
                }
                _ => {}
            }
            continue;
        };
        scopes.push(scope);
    }
}

fn bidi_control_scope_start(character: char) -> Option<BidiControlScope> {
    let (end, is_isolate) = match character {
        '\u{202a}' | '\u{202b}' | '\u{202d}' | '\u{202e}' => ('\u{202c}', false),
        '\u{2066}' | '\u{2067}' | '\u{2068}' => ('\u{2069}', true),
        _ => return None,
    };
    Some(BidiControlScope {
        start: character,
        end,
        is_isolate,
    })
}

fn bidi_prefix_parent_context_control(direction: Direction) -> &'static str {
    match direction {
        Direction::Ltr => "\u{200e}",
        Direction::Rtl => "\u{200f}",
    }
}

fn bidi_suffix_parent_context_control(direction: Direction) -> &'static str {
    match direction {
        Direction::Ltr => "\u{200e}",
        Direction::Rtl => "\u{200f}",
    }
}

fn inline_box_edge_is_wrap_inside_avoid_start(item: &InlineLineItem) -> bool {
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

fn inline_box_edge_is_wrap_inside_avoid_end(item: &InlineLineItem) -> bool {
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

/// The min/max-content contributions of inline content on the consuming
/// formatting context's logical inline axis.
///
/// These are content-box sizes, not physical widths.  A parent with an
/// orthogonal writing mode projects its child contribution before constructing
/// this record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InlineIntrinsicContribution {
    pub(in crate::layout) min_content: LogicalInlineContentSize,
    pub(in crate::layout) max_content: LogicalInlineContentSize,
}

impl InlineIntrinsicContribution {
    pub(in crate::layout) fn new(
        min_content: LogicalInlineContentSize,
        max_content: LogicalInlineContentSize,
    ) -> Self {
        debug_assert!(min_content.points() <= max_content.points());
        Self {
            min_content,
            max_content,
        }
    }

    pub(in crate::layout) fn zero() -> Self {
        Self::new(
            LogicalInlineContentSize::new(content_box_pt(0.0)),
            LogicalInlineContentSize::new(content_box_pt(0.0)),
        )
    }

    /// Include another intrinsic contribution by taking the max-content
    /// contribution independently on each logical inline measure.
    pub(in crate::layout) fn include_max(&mut self, other: Self) {
        self.min_content = self.min_content.max(other.min_content);
        self.max_content = self.max_content.max(other.max_content);
    }
}

impl Default for InlineIntrinsicContribution {
    fn default() -> Self {
        Self::zero()
    }
}

/// Graph-backed intrinsic measurement for one inline paragraph.
///
/// CSS Sizing defines min/max-content contributions from inline break
/// opportunities, while CSS Flexbox also needs the line fragments that a block
/// layout would create for hypothetical cross sizes:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic>,
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>,
/// <https://www.w3.org/TR/css-inline-3/#line-box>, and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineMeasuredParagraph {
    // Kept with intrinsic measurements so future fragmentation can reuse the
    // graph that produced the current line sequence instead of recomputing it.
    #[allow(dead_code)]
    pub(in crate::layout) graph: InlineOpportunityGraph,
    // Kept as paragraph-local intrinsic metadata for future multi-paragraph
    // fragmentation decisions; the aggregate contribution is read today.
    #[allow(dead_code)]
    pub(in crate::layout) contribution: InlineIntrinsicContribution,
}

/// Durable intrinsic measurement for inline content.
///
/// Flex, shrink-to-fit, table, and atomic-inline estimates consume the same
/// graph-backed contribution and selected line fragments instead of
/// independently walking text or descendant trees:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>,
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>,
/// <https://www.w3.org/TR/css-inline-3/#line-layout>, and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct InlineIntrinsicMeasurement {
    pub(in crate::layout) paragraphs: Vec<InlineMeasuredParagraph>,
    pub(in crate::layout) sequence: InlineLineSequence,
    pub(in crate::layout) contribution: InlineIntrinsicContribution,
}

impl InlineIntrinsicMeasurement {
    pub(in crate::layout) fn height(&self) -> f32 {
        self.sequence.total_height()
    }

    pub(in crate::layout) fn physical_height(&self, style: &ComputedStyle) -> f32 {
        match style.writing_mode {
            WritingMode::HorizontalTb => self.height(),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self
                .sequence
                .records
                .iter()
                .filter_map(|record| record.fragment.as_ref())
                .map(|fragment| fragment.metrics.width)
                .fold(0.0, f32::max),
        }
    }

    pub(in crate::layout) fn physical_width(&self, style: &ComputedStyle) -> f32 {
        match style.writing_mode {
            WritingMode::HorizontalTb => self.contribution.max_content.points(),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self.height(),
        }
    }

    /// Return the selected line stack's logical block-axis span.
    ///
    /// A selected line fragment records its physical inline extent for paint,
    /// but an intrinsic block-size walk needs the line's logical block
    /// advance. In vertical writing those are different physical axes, so
    /// derive the advance from the line participants instead of reprojecting
    /// `InlineLineMetrics::width`.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
    /// <https://www.w3.org/TR/css-inline-3/#line-boxes>
    pub(in crate::layout) fn logical_block_span(&self, style: &ComputedStyle) -> f32 {
        match style.writing_mode {
            WritingMode::HorizontalTb => self.height(),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self
                .sequence
                .records
                .iter()
                // A collapsed-space-only record retains source-order data for
                // painting and extraction, but it does not generate a line
                // box. Counting it here would turn each discarded space into
                // an additional vertical column during intrinsic sizing.
                // <https://drafts.csswg.org/css-inline-3/#line-boxes>
                .filter(|record| !record.is_phantom)
                .map(|record| {
                    let line_block_size = record
                        .fragment
                        .as_ref()
                        .map(|fragment| {
                            fragment
                                .items
                                .iter()
                                .map(|item| inline_line_item_logical_block_size(&item.item, style))
                                .fold(style.line_height, f32::max)
                        })
                        .unwrap_or_else(|| record.height());
                    record.block_before + line_block_size
                })
                .sum(),
        }
    }

    pub(in crate::layout) fn line_count(&self) -> usize {
        self.sequence.line_count()
    }

    // Used by tests to lock down preserved forced-break accounting before
    // production fragmentation needs this value directly.
    #[allow(dead_code)]
    pub(in crate::layout) fn forced_empty_line_count(&self) -> usize {
        self.sequence.forced_empty_line_count()
    }
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

/// Float visibility captured with a selected inline line.
///
/// Selection and replay are separate transactions.  Re-querying a mutable
/// float stack is correct only for a normal containing-block replay; an
/// inline-float or initial-letter transaction that already chose its band
/// carries that fact explicitly.  This prevents a later replay from applying
/// the same exclusion once while selecting and again while painting.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
/// <https://drafts.csswg.org/css-inline-3/#line-boxes>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineFloatReplay {
    /// Resolve the current containing block's float band at the replay site.
    RequeryContainingBlock { selected_float_page_index: usize },
    /// Reuse selection geometry on its source fragmentainer; resolve a new
    /// band only after fragmentation moves the line.
    FrozenSelectedBand { selected_float_page_index: usize },
}

impl InlineFloatReplay {
    pub(in crate::layout) fn selected_float_page_index(self) -> usize {
        match self {
            Self::RequeryContainingBlock {
                selected_float_page_index,
            }
            | Self::FrozenSelectedBand {
                selected_float_page_index,
            } => selected_float_page_index,
        }
    }

    pub(in crate::layout) fn reuses_selected_band_on(self, page_index: usize) -> bool {
        matches!(self, Self::FrozenSelectedBand { .. })
            && self.selected_float_page_index() == page_index
    }

    pub(in crate::layout) fn freeze_selected_band(self) -> Self {
        Self::FrozenSelectedBand {
            selected_float_page_index: self.selected_float_page_index(),
        }
    }

    fn requery_containing_block(self) -> Self {
        Self::RequeryContainingBlock {
            selected_float_page_index: self.selected_float_page_index(),
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Build the inline opportunity graph for one mixed inline paragraph.
    ///
    /// Text transform is applied exactly once while normalizing `InlineItem`s
    /// into graph runs. Unicode break opportunities come from the existing
    /// ICU/Parley-backed text helpers; Quire records CSS policy metadata
    /// on the resulting boundaries so later line selection does not repeat
    /// whitespace, hyphenation, and atomic-inline decisions:
    /// <https://www.w3.org/TR/css-text-3/#text-transform-property>,
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>, and
    /// <https://www.w3.org/TR/css-inline-3/#atomic-inline>.
    pub(in crate::layout) fn build_inline_opportunity_graph<I>(
        &mut self,
        items: I,
        block_style: &ComputedStyle,
    ) -> InlineOpportunityGraph
    where
        I: IntoIterator,
        I::Item: AsRef<InlineItem>,
    {
        build_inline_opportunity_graph(&mut self.font_system, items, block_style)
    }

    /// Return a graph whose first typographic letter has its `::first-letter`
    /// style applied before line fitting.
    ///
    /// CSS Inline's `initial-letter` changes the shaped advance and exclusion
    /// geometry of the first letter, so it must be materialized before the
    /// line-break graph selects the first line:
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-property> and
    /// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>.
    pub(in crate::layout) fn graph_with_first_letter_pseudo(
        &mut self,
        graph: &InlineOpportunityGraph,
        block_style: &ComputedStyle,
    ) -> InlineOpportunityGraph {
        let Some(first_letter_style) = block_style.first_letter_style.as_deref() else {
            return graph.clone();
        };
        let selection = first_letter_stream_selection(graph);
        if selection.is_empty() {
            return graph.clone();
        }
        let used_style = used_first_letter_style_for_graph(
            first_letter_style,
            block_style,
            &mut self.font_system,
        );
        let pseudo_group_id = FirstLetterPseudoGroupId::allocate();
        let paint_scope_id = (used_style.opacity.value() < 1.0).then(InlinePaintScopeId::allocate);
        let mut runs = Vec::with_capacity(graph.runs.len() + selection.len() + 2);
        let mut selection_index = 0;
        let mut marked_leading_whitespace = false;
        for (run_index, run) in graph.runs.iter().enumerate() {
            let InlineLineItem::Fragment(fragment) = &run.item else {
                runs.push(run.clone());
                continue;
            };
            let selection_start = selection_index;
            while selection
                .get(selection_index)
                .is_some_and(|slice| slice.run_index == run_index)
            {
                selection_index += 1;
            }
            let selected_slices = &selection[selection_start..selection_index];
            if selected_slices.is_empty() {
                runs.push(run.clone());
                continue;
            }
            if !marked_leading_whitespace {
                mark_leading_preserved_whitespace_as_first_letter_pseudo(
                    &mut runs,
                    first_letter_style,
                    pseudo_group_id,
                );
                marked_leading_whitespace = true;
            }
            for fragment in split_fragment_for_first_letter_stream_selection(
                fragment,
                selected_slices,
                first_letter_style,
                &used_style,
                block_style,
                paint_scope_id,
                pseudo_group_id,
            ) {
                runs.push(measured_fragment_run(
                    fragment,
                    Rc::clone(run_tracking_scope(run, block_style)),
                    &mut self.font_system,
                ));
            }
        }
        if used_style.float != Float::None {
            if matches!(block_style.position, Position::Absolute | Position::Fixed) {
                // The out-of-flow collector traverses the originating
                // positioned source before its final containing block exists.
                // Its graph must not retain the anonymous float marker (or
                // selected text) at that provisional root position. The
                // positioned flow surrogate rebuilds this graph with its
                // final used `position: static` style and owns the one real
                // float transaction.
                remove_positioned_first_letter_float_source(&mut runs, pseudo_group_id);
            } else {
                materialize_first_letter_float(&mut runs, pseudo_group_id, &used_style);
            }
        }
        // `::first-letter` splits an already shaped source fragment. Its
        // pseudo-element boundary is transparent to cursive shaping unless
        // the used style introduces a real shaping boundary, so restore
        // logical source shaping before deriving line-break opportunities.
        // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
        // <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>.
        shape_logical_joining_graph_runs(&mut runs, &mut self.font_system, block_style);
        let opportunities = inline_break_opportunities_for_runs(&runs, block_style);
        InlineOpportunityGraph::new(runs, opportunities)
    }
}

/// Discard a floated first-letter group from an out-of-flow source traversal.
///
/// Absolutely and fixed-positioned elements are collected before their final
/// containing block is known. Their eventual positioned flow surrogate builds
/// the same inline graph at the resolved coordinates, so keeping either the
/// selected text or its synthetic marker here would duplicate the pseudo at
/// the provisional root origin.
/// <https://drafts.csswg.org/css-position-3/#abspos-layout>
fn remove_positioned_first_letter_float_source(
    runs: &mut Vec<InlineParagraphRun>,
    group_id: FirstLetterPseudoGroupId,
) {
    runs.retain(|run| {
        !matches!(
            &run.item,
            InlineLineItem::Fragment(fragment)
                if fragment.first_letter_pseudo_group_id() == Some(group_id)
        )
    });
}

/// Apply `::first-letter` paint ownership to preserved whitespace that the
/// inline collector emitted as runs before the typographic initial.
///
/// Tokenization may split a leading tab from its following letter before the
/// first-letter graph pass runs. The pseudo nevertheless owns that whitespace
/// for paint, while `initial-letter` sizing remains attached to the letter.
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>
fn mark_leading_preserved_whitespace_as_first_letter_pseudo(
    runs: &mut [InlineParagraphRun],
    first_letter_style: &ComputedStyle,
    pseudo_group_id: FirstLetterPseudoGroupId,
) {
    for run in runs.iter_mut().rev() {
        let InlineLineItem::Fragment(fragment) = &mut run.item else {
            break;
        };
        if fragment.text().is_empty()
            || !fragment.text().chars().all(char::is_whitespace)
            || fragment.style().white_space.collapses_spaces()
        {
            break;
        }
        apply_first_letter_style_to_leading_preserved_whitespace(fragment, first_letter_style);
        fragment.set_first_letter_pseudo_group_id(pseudo_group_id);
    }
}

/// Replace one stream-selected first-letter group with a source-order CSS
/// float marker.
///
/// CSS 2 treats a floated first letter like a floated element.  Keep the
/// text fragments as payload rather than fabricating a DOM node: the selected
/// group may contain generated punctuation or cross transparent inline edges.
/// <https://www.w3.org/TR/CSS21/selector.html#first-letter>
fn materialize_first_letter_float(
    runs: &mut Vec<InlineParagraphRun>,
    group_id: FirstLetterPseudoGroupId,
    used_style: &ComputedStyle,
) {
    let mut fragments = Vec::new();
    for run in runs.iter() {
        let InlineLineItem::Fragment(fragment) = &run.item else {
            continue;
        };
        if fragment.first_letter_pseudo_group_id() == Some(group_id) {
            let mut fragment = fragment.clone();
            // The anonymous float wrapper owns the out-of-flow behavior and
            // the pseudo can no longer be an in-flow initial letter.
            fragment.style_mut().float = Float::None;
            fragment.style_mut().initial_letter = css::InitialLetter::Normal;
            fragments.push(fragment);
        }
    }
    if fragments.is_empty() {
        return;
    }
    let mut float_style = used_style.clone();
    float_style.initial_letter = css::InitialLetter::Normal;
    let float = InlineFloat::first_letter(fragments, group_id, float_style);
    let mut materialized = Vec::with_capacity(runs.len() + 1);
    let mut inserted = false;
    for run in std::mem::take(runs) {
        let selected = matches!(
            &run.item,
            InlineLineItem::Fragment(fragment)
                if fragment.first_letter_pseudo_group_id() == Some(group_id)
        );
        if selected {
            if !inserted {
                materialized.push(InlineParagraphRun {
                    item: InlineLineItem::Float(float.clone()),
                    ..run.clone()
                });
                inserted = true;
            }
        } else {
            materialized.push(run);
        }
    }
    *runs = materialized;
}

fn apply_first_letter_style_to_leading_preserved_whitespace(
    fragment: &mut InlineFragment,
    first_letter_style: &ComputedStyle,
) {
    let source_style = fragment.style().clone();
    let mut prefix_style = first_letter_style.clone();
    prefix_style.font_size = source_style.font_size;
    prefix_style.line_height = source_style.line_height;
    prefix_style.white_space = source_style.white_space;
    prefix_style.tab_size = source_style.tab_size;
    prefix_style.initial_letter = css::InitialLetter::Normal;
    *fragment.style_mut() = prefix_style;
    fragment
        .set_first_letter_pseudo_role(FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace);
    fragment.set_mergeable(false);
}

/// One source range selected as part of the originating block's first-letter
/// text, kept separate because the graph preserves lexical inline fragments.
#[derive(Debug, Clone)]
struct FirstLetterStreamSlice {
    run_index: usize,
    range: std::ops::Range<usize>,
    role: FirstLetterPseudoFragmentRole,
}

/// Select first-letter text across source-order graph fragments.
///
/// Marker and CSS-generated bidi-control text do not create first-letter
/// content. Inline box edges are transparent, whereas another in-flow atomic
/// participant prevents a later text run from becoming the first letter.
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>
fn first_letter_stream_selection(graph: &InlineOpportunityGraph) -> Vec<FirstLetterStreamSlice> {
    let mut prefix = Vec::new();
    let mut prefix_origin = None;
    let mut selection = Vec::new();
    let mut pending_suffix_space = Vec::new();
    let mut selected_base = false;

    for (run_index, run) in graph.runs.iter().enumerate() {
        match &run.item {
            InlineLineItem::Fragment(fragment)
                if matches!(
                    fragment.source(),
                    InlineTextSource::BidiControl | InlineTextSource::Marker
                ) =>
            {
                continue;
            }
            InlineLineItem::Fragment(fragment) => {
                // A run split from one lexical source remains continuous for
                // first-letter selection, even when inline box edges occur
                // between its pieces. Generated content and a nested inline's
                // independent text establish their own first-letter scope;
                // an opening quote in either therefore remains the selected
                // pseudo content instead of attaching to later author text.
                if !prefix.is_empty() {
                    let origin = prefix_origin.expect("a prefix has an origin");
                    if !first_letter_prefix_continues_into(origin, fragment) {
                        return prefix;
                    }
                }
                for range in CursiveProtectedUnitRanges::new(fragment.text()) {
                    let Some(base) = first_letter_unit_base(&fragment.text()[range.clone()]) else {
                        continue;
                    };
                    if !selected_base {
                        if character_is_first_letter_associated_space(base) && !prefix.is_empty() {
                            push_first_letter_stream_slice(
                                &mut prefix,
                                run_index,
                                range,
                                FirstLetterPseudoFragmentRole::AssociatedPrefix,
                            );
                        } else if base.is_whitespace() && prefix.is_empty() {
                            // Preserved leading whitespace is styled through
                            // the existing pseudo-prefix path once a base is
                            // found.
                        } else if character_is_unicode_punctuation(base) {
                            if prefix.is_empty() {
                                prefix_origin = Some(fragment);
                            }
                            push_first_letter_stream_slice(
                                &mut prefix,
                                run_index,
                                range,
                                FirstLetterPseudoFragmentRole::AssociatedPrefix,
                            );
                        } else if character_is_unicode_first_letter_base(base) {
                            selection.append(&mut prefix);
                            push_first_letter_stream_slice(
                                &mut selection,
                                run_index,
                                range,
                                FirstLetterPseudoFragmentRole::TypographicInitial,
                            );
                            selected_base = true;
                        } else if !prefix.is_empty() {
                            return Vec::new();
                        }
                    } else if character_is_first_letter_associated_space(base) {
                        push_first_letter_stream_slice(
                            &mut pending_suffix_space,
                            run_index,
                            range,
                            FirstLetterPseudoFragmentRole::AssociatedSuffix,
                        );
                    } else if character_is_first_letter_suffix_punctuation(base) {
                        selection.append(&mut pending_suffix_space);
                        push_first_letter_stream_slice(
                            &mut selection,
                            run_index,
                            range,
                            FirstLetterPseudoFragmentRole::AssociatedSuffix,
                        );
                    } else {
                        return selection;
                    }
                }
            }
            InlineLineItem::Atom(atom) if atom.content().is_inline_edge() => {}
            InlineLineItem::Float(_) => {}
            InlineLineItem::Atom(_) => return Vec::new(),
        }
    }
    selection
}

fn first_letter_prefix_continues_into(origin: &InlineFragment, next: &InlineFragment) -> bool {
    origin.source() == next.source()
        && match (origin.tracking_scope(), next.tracking_scope()) {
            (Some(origin), Some(next)) => Rc::ptr_eq(origin, next),
            (None, None) => true,
            _ => false,
        }
}

fn first_letter_unit_base(unit: &str) -> Option<char> {
    unit.chars().find(|character| {
        !character_is_unicode_mark(*character)
            && !character_is_default_ignorable_code_point(*character)
    })
}

fn push_first_letter_stream_slice(
    slices: &mut Vec<FirstLetterStreamSlice>,
    run_index: usize,
    range: std::ops::Range<usize>,
    role: FirstLetterPseudoFragmentRole,
) {
    if let Some(previous) = slices.last_mut()
        && previous.run_index == run_index
        && previous.role == role
        && previous.range.end == range.start
    {
        previous.range.end = range.end;
        return;
    }
    slices.push(FirstLetterStreamSlice {
        run_index,
        range,
        role,
    });
}

fn split_fragment_for_first_letter_stream_selection(
    fragment: &InlineFragment,
    selected_slices: &[FirstLetterStreamSlice],
    first_letter_style: &ComputedStyle,
    used_style: &ComputedStyle,
    block_style: &ComputedStyle,
    paint_scope_id: Option<InlinePaintScopeId>,
    pseudo_group_id: FirstLetterPseudoGroupId,
) -> Vec<InlineFragment> {
    let mut pieces = Vec::new();
    let mut cursor = 0;
    for slice in selected_slices {
        debug_assert!(cursor <= slice.range.start);
        if cursor < slice.range.start {
            let mut before = fragment.clone();
            before.set_text(Rc::<str>::from(&fragment.text()[cursor..slice.range.start]));
            apply_first_letter_style_to_leading_preserved_whitespace(
                &mut before,
                first_letter_style,
            );
            pieces.push(before);
        }
        let mut selected = fragment.clone();
        selected.set_text(Rc::<str>::from(&fragment.text()[slice.range.clone()]));
        apply_first_letter_style_to_stream_selection(
            &mut selected,
            first_letter_style,
            used_style,
            block_style,
            slice.role,
            paint_scope_id,
            pseudo_group_id,
        );
        pieces.push(selected);
        cursor = slice.range.end;
    }
    if cursor < fragment.text().len() {
        let mut after = fragment.clone();
        after.set_text(Rc::<str>::from(&fragment.text()[cursor..]));
        if let Some(first_line_style) = block_style.first_line_style.as_deref() {
            after.style_mut().color = first_line_style.color;
        }
        pieces.push(after);
    }
    pieces
}

fn apply_first_letter_style_to_stream_selection(
    fragment: &mut InlineFragment,
    first_letter_style: &ComputedStyle,
    used_style: &ComputedStyle,
    block_style: &ComputedStyle,
    role: FirstLetterPseudoFragmentRole,
    paint_scope_id: Option<InlinePaintScopeId>,
    pseudo_group_id: FirstLetterPseudoGroupId,
) {
    let mut used_style = used_style.clone();
    if first_letter_style.color == block_style.color && block_style.first_line_style.is_none() {
        used_style.color = fragment.style().color;
    }
    // Prefixes and suffixes remain within `::first-letter`, but only the
    // L/N/S typographic unit participates in initial-letter geometry.
    if role != FirstLetterPseudoFragmentRole::TypographicInitial {
        used_style.initial_letter = css::InitialLetter::Normal;
    }
    *fragment.style_mut() = used_style;
    fragment.set_first_letter_pseudo_role(role);
    fragment.set_first_letter_pseudo_group_id(pseudo_group_id);
    if fragment.style().opacity.value() < 1.0 {
        let mut marker_style = fragment.style().clone();
        marker_style.opacity = css::Opacity::ONE;
        fragment.push_ancestor_inline_decoration(InlineAncestorDecoration {
            style: marker_style,
            hanging_edges: InlineHangingEdges::default(),
            paints_background_or_border: false,
            positioning_containing_block_id: None,
            paint_effect_scope_id: paint_scope_id,
        });
    }
    fragment.set_mergeable(false);
}

fn used_first_letter_style_for_graph(
    first_letter_style: &ComputedStyle,
    block_style: &ComputedStyle,
    font_system: &mut FontSystem,
) -> ComputedStyle {
    let mut style = first_letter_style.clone();
    // `::first-letter` inherits from the originating `::first-line`. The
    // cascade has already resolved an authored `color: inherit` against the
    // block style, so carry the first-line color into that otherwise
    // indistinguishable inherited value before the initial-letter run is
    // shaped and painted.
    // <https://www.w3.org/TR/css-pseudo-4/#first-line-pseudo>
    if style.color == block_style.color
        && let Some(first_line_style) = block_style.first_line_style.as_deref()
    {
        style.color = first_line_style.color;
    }
    let Some((size, _sink)) = style.initial_letter.specified() else {
        return style;
    };
    let surrounding_cap_height = font_system.used_cap_height_for_style(block_style).points();
    let initial_cap_height = font_system.used_cap_height_for_style(&style).points();
    let cap_ratio = (initial_cap_height / style.font_size.max(0.01)).max(0.01);
    let target_cap_height =
        ((size - 1.0).max(0.0) * block_style.line_height) + surrounding_cap_height.max(0.0);
    // `initial-letter` establishes the used font size from the surrounding
    // line geometry.  The computed `font-size` on `::first-letter` remains
    // observable in the cascade, but does not impose a minimum on the used
    // initial-letter size: authors commonly set an intentionally huge value
    // here to assert that it is ignored.
    // <https://drafts.csswg.org/css-inline-3/#initial-letter-sizing>
    let used_font_size = (target_cap_height / cap_ratio).max(0.01);
    style.font_size = used_font_size;
    style.line_height = used_font_size;
    style.line_height_value = css::ComputedLineHeight::from_points(used_font_size);
    style
}

fn measured_fragment_run(
    fragment: InlineFragment,
    tracking_scope: Rc<InlineTrackingScope>,
    font_system: &mut FontSystem,
) -> InlineParagraphRun {
    let shaped = font_system.shape_untracked_inline_line(
        fragment.text(),
        fragment.style(),
        fragment.style().line_height,
    );
    let width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    InlineParagraphRun {
        item: InlineLineItem::Fragment(fragment.with_tracking_scope(tracking_scope)),
        width,
        shaped: shaped.map(Rc::new),
    }
}

#[derive(Clone)]
struct TextSpacingCharacter {
    item_index: usize,
    range: std::ops::Range<usize>,
    class: Option<crate::text::TextSpacingPunctuationClass>,
    policy: TextSpacingTrim,
}

/// Return the text-spacing characters that participate in selected-line edge
/// selection.
///
/// CSS-generated UAX #9 controls and zero-advance inline-scope boundaries do
/// not create text edges. The remaining visible text is one selected inline
/// line even when it crosses an automatic `::marker` or an authored isolate;
/// a pseudo-element is not a CSS Text edge of its own. Keep the controls in
/// the materialized item stream for bidi, while making them transparent to
/// punctuation spacing:
/// <https://drafts.csswg.org/css-text-4/#text-spacing-trim-property> and
/// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>.
fn selected_line_text_spacing_characters(
    items: &[MeasuredInlineItem],
) -> Vec<TextSpacingCharacter> {
    let mut characters = Vec::new();
    for (item_index, item) in items.iter().enumerate() {
        let InlineLineItem::Fragment(fragment) = &item.item else {
            continue;
        };
        let vertical = matches!(
            fragment.style().text_layout_policy(),
            crate::css::TextLayoutPolicy::Vertical(_)
        );
        for (start, character) in fragment.text().char_indices() {
            if crate::text::character_is_bidi_format_control(character) {
                continue;
            }
            characters.push(TextSpacingCharacter {
                item_index,
                range: start..start + character.len_utf8(),
                class: crate::text::text_spacing_punctuation_class(
                    character,
                    fragment.style().language.as_deref(),
                    vertical,
                ),
                policy: fragment.style().text_spacing_trim.resolved(),
            });
        }
    }
    characters
}

/// Whether selected source can produce a `text-spacing-trim` adjustment.
///
/// This deliberately mirrors the target eligibility in
/// `apply_materialized_text_spacing_trim` without collecting per-character
/// provenance. Ordinary text is by far the common case, and it does not need
/// the allocated character list used to resolve selected-line adjacency.
fn materialized_items_may_use_text_spacing_trim(items: &[MeasuredInlineItem]) -> bool {
    items.iter().any(|item| {
        let InlineLineItem::Fragment(fragment) = &item.item else {
            return false;
        };
        if fragment.style().text_spacing_trim.resolved() == TextSpacingTrim::SpaceAll {
            return false;
        }
        let vertical = matches!(
            fragment.style().text_layout_policy(),
            crate::css::TextLayoutPolicy::Vertical(_)
        );
        fragment.text().chars().any(|character| {
            !crate::text::character_is_bidi_format_control(character)
                && crate::text::text_spacing_punctuation_class(
                    character,
                    fragment.style().language.as_deref(),
                    vertical,
                )
                .is_some()
        })
    })
}

/// Apply `text-spacing-trim` to one candidate's selected source items.
///
/// The graph keeps its source runs full-width. This function runs only after a
/// candidate has chosen its line range, splits the used fragments at affected
/// typographic units, and reshapes those units with the appropriate OpenType
/// alternate. Thus a narrower opening bracket changes the candidate measure
/// before line selection commits, without mutating source text, extraction, or
/// graph break positions:
/// <https://drafts.csswg.org/css-text-4/#text-spacing-trim-property>.
fn apply_materialized_text_spacing_trim(
    items: &mut Vec<MeasuredInlineItem>,
    font_system: &mut FontSystem,
    is_initial_line: bool,
    _available_width: Option<f32>,
) {
    use crate::text::TextSpacingPunctuationClass::{
        Closing, IdeographicSpace, MiddleDot, NarrowClosing, NarrowOpening, Opening,
    };

    if !materialized_items_may_use_text_spacing_trim(items) {
        return;
    }
    let characters = selected_line_text_spacing_characters(items);
    debug_assert!(!characters.is_empty());

    let mut targets = Vec::<(usize, std::ops::Range<usize>)>::new();
    let mut add_target = |character: TextSpacingCharacter| {
        if matches!(character.class, Some(Opening | Closing | MiddleDot))
            && !targets.iter().any(|(item_index, range)| {
                *item_index == character.item_index && *range == character.range
            })
        {
            targets.push((character.item_index, character.range));
        }
    };

    for character in &characters {
        if character.policy == TextSpacingTrim::TrimAll && character.class.is_some() {
            add_target(character.clone());
        }
    }
    // CSS Text's start and end are the physical edges of this selected
    // formatted line. Generated-marker provenance is retained for marker
    // semantics and paint, but it is not a separate text-spacing boundary.
    let first_line_edge_character = characters.first();
    let last_line_edge_character = characters.last();
    if let Some(first) = first_line_edge_character {
        let trims_start = matches!(
            first.policy,
            TextSpacingTrim::TrimStart | TextSpacingTrim::TrimBoth
        ) || (first.policy == TextSpacingTrim::SpaceFirst && !is_initial_line);
        if first.class == Some(Opening) && trims_start {
            add_target(first.clone());
        }
    }
    if let Some(last) = last_line_edge_character
        && last.class == Some(Closing)
        && matches!(
            last.policy,
            TextSpacingTrim::Normal
                | TextSpacingTrim::TrimStart
                | TextSpacingTrim::SpaceFirst
                | TextSpacingTrim::TrimBoth
        )
    {
        add_target(last.clone());
    }
    for pair in characters.windows(2) {
        let [previous, current] = pair else { continue };
        if current.policy == TextSpacingTrim::SpaceAll {
            continue;
        }
        if current.class == Some(Opening)
            && matches!(
                previous.class,
                Some(Opening | MiddleDot | Closing | IdeographicSpace | NarrowOpening)
            )
        {
            add_target(current.clone());
        }
        if previous.policy != TextSpacingTrim::SpaceAll
            && previous.class == Some(Closing)
            && matches!(
                current.class,
                Some(Closing | MiddleDot | IdeographicSpace | NarrowClosing)
            )
        {
            add_target(previous.clone());
        }
    }
    if targets.is_empty() {
        return;
    }

    let mut output = Vec::with_capacity(items.len() + targets.len());
    for (item_index, item) in std::mem::take(items).into_iter().enumerate() {
        let InlineLineItem::Fragment(fragment) = &item.item else {
            output.push(item);
            continue;
        };
        let mut ranges = targets
            .iter()
            .filter(|(target_index, _)| *target_index == item_index)
            .map(|(_, range)| range.clone())
            .collect::<Vec<_>>();
        if ranges.is_empty() {
            output.push(item);
            continue;
        }
        ranges.sort_by_key(|range| range.start);
        let mut cursor = 0;
        for range in ranges {
            if cursor < range.start {
                push_text_spacing_fragment(
                    &mut output,
                    fragment,
                    &fragment.text()[cursor..range.start],
                    false,
                    font_system,
                );
            }
            push_text_spacing_fragment(
                &mut output,
                fragment,
                &fragment.text()[range.clone()],
                true,
                font_system,
            );
            cursor = range.end;
        }
        if cursor < fragment.text().len() {
            push_text_spacing_fragment(
                &mut output,
                fragment,
                &fragment.text()[cursor..],
                false,
                font_system,
            );
        }
    }
    *items = output;
}

fn push_text_spacing_fragment(
    output: &mut Vec<MeasuredInlineItem>,
    source: &InlineFragment,
    text: &str,
    trimmed: bool,
    font_system: &mut FontSystem,
) {
    if text.is_empty() {
        return;
    }
    let mut fragment = source.clone();
    fragment.set_text(Rc::from(text));
    if trimmed {
        let tag = if matches!(
            fragment.style().text_layout_policy(),
            crate::css::TextLayoutPolicy::Vertical(_)
        ) {
            *b"vhal"
        } else {
            *b"halt"
        };
        let style = fragment.style_mut();
        if let Some(setting) = style
            .font_feature_settings
            .0
            .iter_mut()
            .find(|setting| setting.tag == tag)
        {
            setting.value = 1;
        } else {
            style
                .font_feature_settings
                .0
                .push(crate::css::FontFeatureSetting::new(tag, 1));
            style
                .font_feature_settings
                .0
                .sort_by_key(|setting| setting.tag);
        }
    }
    let shaped = font_system.shape_untracked_inline_line(
        fragment.text(),
        fragment.style(),
        fragment.style().line_height,
    );
    let width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    output.push(MeasuredInlineItem::new(
        InlineLineItem::Fragment(fragment),
        width,
        shaped.map(Rc::new),
    ));
}

pub(in crate::layout) fn build_inline_opportunity_graph<I>(
    font_system: &mut FontSystem,
    items: I,
    block_style: &ComputedStyle,
) -> InlineOpportunityGraph
where
    I: IntoIterator,
    I::Item: AsRef<InlineItem>,
{
    let mut runs = Vec::new();
    let mut transform_state = TextTransformState::default();
    let root_tracking_scope = InlineTrackingScope::root(block_style);
    let mut tracking_scopes = vec![root_tracking_scope];
    for item in items {
        match item.as_ref() {
            InlineItem::Word(word) => {
                let text = transform_text_with_state(&word.text, &word.style, &mut transform_state);
                let text = synthesize_missing_font_caps_text(font_system, &text, &word.style);
                push_text_graph_runs(
                    font_system,
                    &mut runs,
                    word,
                    &text,
                    Rc::clone(
                        tracking_scopes
                            .last()
                            .expect("tracking scope stack is rooted"),
                    ),
                );
            }
            InlineItem::Atom(atom) => {
                // Inline box edges are transparent to CSS `capitalize` word
                // boundaries; only replaced/atomic inline content separates
                // adjacent text words.
                if !matches!(atom.content(), InlineAtomContent::InlineEdge(_)) {
                    transform_state.force_word_boundary();
                }
                let scope = Rc::clone(
                    tracking_scopes
                        .last()
                        .expect("tracking scope stack is rooted"),
                );
                let atom = (**atom).clone().with_tracking_scope(Rc::clone(&scope));
                runs.push(InlineParagraphRun {
                    item: InlineLineItem::Atom(atom.clone()),
                    width: inline_atom_logical_inline_size(&atom, block_style),
                    shaped: None,
                });
                if let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content()
                {
                    match edge.logical_edge {
                        InlineLogicalEdge::Start => {
                            tracking_scopes.push(InlineTrackingScope::child(scope, atom.style()))
                        }
                        InlineLogicalEdge::End => {
                            if tracking_scopes.len() > 1 {
                                tracking_scopes.pop();
                            }
                        }
                    }
                }
            }
            InlineItem::Float(float) => {
                // Floats participate in source-order placement but are out
                // of flow for CSS Text. They therefore cannot split the word
                // context used by `text-transform: capitalize`.
                // <https://www.w3.org/TR/css-text-3/#text-transform-property>
                runs.push(InlineParagraphRun {
                    item: InlineLineItem::Float((**float).clone()),
                    width: 0.0,
                    shaped: None,
                });
            }
            InlineItem::Break(_) | InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => {
                transform_state.force_word_boundary();
            }
        }
    }
    resolve_text_autospace_owner_styles(&mut runs, font_system);
    coalesce_trailing_tracking_controls(&mut runs, font_system);
    let automatic_discretionary_breaks =
        apply_auto_hyphenation_across_transparent_inline_edges(&runs);
    let manual_discretionary_effects =
        manual_hyphenation_effects_across_transparent_inline_edges(&runs);
    shape_logical_joining_graph_runs(&mut runs, font_system, block_style);
    let mut opportunities = inline_break_opportunities_for_runs(&runs, block_style);
    merge_automatic_discretionary_breaks(&mut opportunities, automatic_discretionary_breaks);
    merge_manual_discretionary_effects(&mut opportunities, manual_discretionary_effects);
    InlineOpportunityGraph::new(runs, opportunities)
}

/// Resolve each source autospace marker against the innermost lexical inline
/// scope shared by its adjacent typographic units.
///
/// Inline collection must create the marker before the graph has assigned
/// immutable tracking scopes. Once the graph has done so, this pass is the
/// single point where CSS Text's boundary owner selects both the applicable
/// property value and the `1/8ic` used advance.
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>
fn resolve_text_autospace_owner_styles(
    runs: &mut Vec<InlineParagraphRun>,
    font_system: &mut FontSystem,
) {
    let mut index = 0;
    while index < runs.len() {
        if !matches!(
            runs[index].item,
            InlineLineItem::Atom(ref atom)
                if matches!(
                    atom.content(),
                    InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(_))
                )
        ) {
            index += 1;
            continue;
        }
        let Some((left, right)) = autospace_adjacent_fragments(runs, index) else {
            index += 1;
            continue;
        };
        let (Some(left_scope), Some(right_scope)) = (left.tracking_scope(), right.tracking_scope())
        else {
            index += 1;
            continue;
        };
        let owner_style =
            InlineTrackingScope::common_autospace_style(left_scope.as_ref(), right_scope.as_ref())
                .clone();
        let (Some(first), Some(second)) = (
            autospace_boundary_character_at_end(left.text()),
            autospace_boundary_character_at_start(right.text()),
        ) else {
            runs.remove(index);
            continue;
        };
        if !text_autospace_boundary_needs_spacing(
            &owner_style.text_autospace,
            first,
            left.style(),
            second,
            right.style(),
        ) {
            runs.remove(index);
            continue;
        }
        let advance = font_system.ic_advance_for_style(&owner_style) / 8.0;
        let InlineLineItem::Atom(atom) = &mut runs[index].item else {
            unreachable!("autospace marker is an inline atom");
        };
        atom.set_text_autospace_advance(&owner_style, advance);
        runs[index].width = advance.points();
        index += 1;
    }
}

/// Return direct textual neighbors of an already collected autospace marker.
/// Box-edge atoms are lexical scope markers, while any other atom or float
/// would have prevented collection from inserting the autospace marker.
fn autospace_adjacent_fragments(
    runs: &[InlineParagraphRun],
    index: usize,
) -> Option<(&InlineFragment, &InlineFragment)> {
    let mut left = None;
    for run in runs[..index].iter().rev() {
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                left = Some(fragment);
                break;
            }
            InlineLineItem::Atom(atom)
                if matches!(atom.content(), InlineAtomContent::InlineEdge(_)) => {}
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return None,
        }
    }
    let mut right = None;
    for run in &runs[index + 1..] {
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                right = Some(fragment);
                break;
            }
            InlineLineItem::Atom(atom)
                if matches!(atom.content(), InlineAtomContent::InlineEdge(_)) => {}
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return None,
        }
    }
    left.zip(right)
}

/// Attach a separately collected trailing control run to its preceding text.
///
/// HTML comments and inline DOM boundaries can split a sequence such as
/// `x U+200D` into distinct collector words. CSS Text treats the control as
/// part of the adjacent typographic unit: it must not own a gap itself, while
/// the boundary from that preceding `x` to the following visible character
/// remains eligible for tracking. Coalescing before graph shaping also keeps
/// fitting, intrinsic sizing, and paint on the same source representation.
/// <https://www.w3.org/TR/css-text-3/#letter-spacing>
fn coalesce_trailing_tracking_controls(
    runs: &mut Vec<InlineParagraphRun>,
    font_system: &mut FontSystem,
) {
    let mut index = 1;
    while index < runs.len() {
        let control_text = match &runs[index].item {
            InlineLineItem::Fragment(fragment)
                if crate::text::text_is_inter_character_control_only(fragment.text()) =>
            {
                Some(fragment.text().to_string())
            }
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                None
            }
        };
        let Some(control_text) = control_text else {
            index += 1;
            continue;
        };
        let can_attach = matches!(
            (&runs[index - 1].item, &runs[index].item),
            (InlineLineItem::Fragment(previous), InlineLineItem::Fragment(control))
                if previous.tracking_scope().zip(control.tracking_scope())
                    .is_some_and(|(left, right)| Rc::ptr_eq(left, right))
        );
        if !can_attach {
            index += 1;
            continue;
        }
        let InlineLineItem::Fragment(previous) = &mut runs[index - 1].item else {
            unreachable!("control run can only attach to preceding text")
        };
        let mut text = String::with_capacity(previous.text().len() + control_text.len());
        text.push_str(previous.text());
        text.push_str(&control_text);
        previous.set_text(text);
        let shaped = font_system.shape_untracked_inline_line(
            previous.text(),
            previous.style(),
            previous.style().line_height,
        );
        let width = shaped
            .as_ref()
            .map(ShapedInlineLine::advance_width)
            .unwrap_or(0.0);
        runs[index - 1].width = width;
        runs[index - 1].shaped = shaped.map(Rc::new);
        runs.remove(index);
    }
}

/// Apply dictionary hyphenation after joining source fragments that CSS Text
/// treats as one word.
///
/// An ordinary inline element is transparent to word formation: `high<span>
/// way</span>` must be offered to the language dictionary as `highway`, while
/// its resulting soft hyphen remains owned by the source fragment before the
/// selected break. Atomic boxes, used inline-axis decoration, bidi isolation,
/// and differing hyphenation policies terminate the word.
/// <https://www.w3.org/TR/css-text-3/#hyphenation>
fn apply_auto_hyphenation_across_transparent_inline_edges(
    runs: &[InlineParagraphRun],
) -> Vec<InlineBreakOpportunity> {
    let mut automatic_breaks = Vec::new();
    let mut index = 0;
    while index < runs.len() {
        let Some((fragment_indices, next_index)) = hyphenation_fragment_group(runs, index) else {
            index += 1;
            continue;
        };
        index = next_index;
        let InlineLineItem::Fragment(first) = &runs[fragment_indices[0]].item else {
            unreachable!("hyphenation group has a first fragment");
        };
        if first.style().hyphens != Hyphens::Auto
            || matches!(
                first.style().automatic_discretionary_hyphenation_policy(),
                DiscretionaryHyphenationPolicy::Disabled
            )
            || matches!(first.style().line_break, css::LineBreak::Anywhere)
        {
            continue;
        }
        let Some(language) = first.style().language.as_deref() else {
            continue;
        };
        let hyphenator = hyphenator_for_language(language);
        let (source, source_ends) = hyphenation_source_for_fragments(runs, &fragment_indices);
        let opportunities = automatic_hyphenation_opportunities(
            &source,
            hyphenator.as_deref(),
            first.style().hyphenate_limit_chars,
            language,
        );
        automatic_breaks.extend(opportunities.into_iter().filter_map(|opportunity| {
            automatic_opportunity_for_source_offset(
                opportunity,
                &fragment_indices,
                &source_ends,
                runs,
            )
        }));
    }
    automatic_breaks
}

/// Attach language-resource spelling changes to authored U+00AD boundaries.
///
/// The source word is gathered through transparent inline edges before the
/// text-layer resolver removes U+00AD for its lookup. The resolver then maps
/// a matching rule back to the authored source boundary, allowing selected
/// line materialization to use the same discretionary effect as `auto`.
/// <https://www.w3.org/TR/css-text-3/#hyphenation>
fn manual_hyphenation_effects_across_transparent_inline_edges(
    runs: &[InlineParagraphRun],
) -> Vec<(InlineGraphPosition, DiscretionaryBreakEffect)> {
    let mut effects = Vec::new();
    let mut index = 0;
    while index < runs.len() {
        let Some((fragment_indices, next_index)) = hyphenation_fragment_group(runs, index) else {
            index += 1;
            continue;
        };
        index = next_index;
        let InlineLineItem::Fragment(first) = &runs[fragment_indices[0]].item else {
            unreachable!("hyphenation group has a first fragment");
        };
        if matches!(
            first.style().authored_discretionary_hyphenation_policy(),
            DiscretionaryHyphenationPolicy::Disabled
        ) {
            continue;
        }
        let Some(language) = first.style().language.as_deref() else {
            continue;
        };
        let (source, source_ends) = hyphenation_source_for_fragments(runs, &fragment_indices);
        for opportunity in manual_hyphenation_opportunities(&source, language) {
            let position = source_position_for_offset(
                opportunity.byte_offset,
                &fragment_indices,
                &source_ends,
                runs,
            );
            let InlineLineItem::Fragment(fragment) = &runs[position.run_index].item else {
                unreachable!("manual hyphen source position names a text fragment");
            };
            if !fragment.style().allows_soft_wrap() {
                continue;
            }
            effects.push((
                position,
                DiscretionaryBreakEffect {
                    source_boundary: position,
                    marker_owner: DiscretionaryMarkerOwner {
                        style_position: position,
                    },
                    left_replacement: language_replacement_to_line_edge(opportunity.left),
                    right_replacement: language_replacement_to_line_edge(opportunity.right),
                    leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
                },
            ));
        }
    }
    effects
}

fn hyphenation_fragment_group(
    runs: &[InlineParagraphRun],
    start: usize,
) -> Option<(Vec<usize>, usize)> {
    let InlineLineItem::Fragment(first) = &runs.get(start)?.item else {
        return None;
    };
    if first.style().hyphens == Hyphens::None {
        return None;
    }
    let mut fragment_indices = vec![start];
    let mut index = start + 1;
    while let Some(run) = runs.get(index) {
        match &run.item {
            InlineLineItem::Fragment(next)
                if graph_fragments_share_hyphenation_policy(
                    match &runs[*fragment_indices.last().expect("first hyphenation fragment")].item
                    {
                        InlineLineItem::Fragment(fragment) => fragment,
                        InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                            unreachable!("hyphenation fragment index names a fragment")
                        }
                    },
                    next,
                ) =>
            {
                fragment_indices.push(index);
                index += 1;
            }
            InlineLineItem::Atom(atom) if graph_atom_is_transparent_to_shaping(atom) => {
                index += 1;
            }
            InlineLineItem::Float(_) => {
                // An out-of-flow float is transparent to the in-flow word
                // that may continue on either side of its source marker.
                // <https://www.w3.org/TR/css-text-3/#line-break-details>
                index += 1;
            }
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) => {
                break;
            }
        }
    }
    Some((fragment_indices, index))
}

fn hyphenation_source_for_fragments(
    runs: &[InlineParagraphRun],
    fragment_indices: &[usize],
) -> (String, Vec<usize>) {
    let mut source = String::new();
    let mut source_ends = Vec::with_capacity(fragment_indices.len());
    for &fragment_index in fragment_indices {
        let InlineLineItem::Fragment(fragment) = &runs[fragment_index].item else {
            unreachable!("hyphenation source index names a text fragment");
        };
        source.push_str(fragment.text());
        source_ends.push(source.len());
    }
    (source, source_ends)
}

fn automatic_opportunity_for_source_offset(
    opportunity: DiscretionaryOpportunity,
    fragment_indices: &[usize],
    source_ends: &[usize],
    runs: &[InlineParagraphRun],
) -> Option<InlineBreakOpportunity> {
    let position =
        source_position_for_offset(opportunity.byte_offset, fragment_indices, source_ends, runs);
    let availability = match &runs[position.run_index].item {
        InlineLineItem::Fragment(fragment) => discretionary_hyphenation_availability(
            fragment
                .style()
                .automatic_discretionary_hyphenation_policy(),
        )?,
        InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
            unreachable!("hyphenation source position is text")
        }
    };
    Some(InlineBreakOpportunity {
        position,
        kind: BreakEffect::Hyphenation,
        availability,
        whitespace_edge: SelectedWhitespaceEdge::None,
        discretionary: Some(DiscretionaryBreakEffect {
            source_boundary: position,
            marker_owner: DiscretionaryMarkerOwner {
                style_position: position,
            },
            left_replacement: language_replacement_to_line_edge(opportunity.left),
            right_replacement: language_replacement_to_line_edge(opportunity.right),
            leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
        }),
    })
}

fn source_position_for_offset(
    source_byte_offset: usize,
    fragment_indices: &[usize],
    source_ends: &[usize],
    runs: &[InlineParagraphRun],
) -> InlineGraphPosition {
    let fragment_offset = source_ends
        .iter()
        .position(|&end| source_byte_offset <= end)
        .unwrap_or_else(|| source_ends.len().saturating_sub(1));
    let previous_end = fragment_offset
        .checked_sub(1)
        .and_then(|index| source_ends.get(index))
        .copied()
        .unwrap_or(0);
    let run_index = fragment_indices[fragment_offset];
    let byte_offset = source_byte_offset - previous_end;
    let fragment_text_len = match &runs[run_index].item {
        InlineLineItem::Fragment(fragment) => fragment.text().len(),
        InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
            unreachable!("hyphenation source position is text")
        }
    };
    debug_assert!(byte_offset <= fragment_text_len);
    InlineGraphPosition {
        run_index,
        byte_offset,
    }
}

fn language_replacement_to_line_edge(
    replacement: Option<LanguageDiscretionaryReplacement>,
) -> Option<InlineLineEdgeReplacement> {
    replacement.map(|replacement| InlineLineEdgeReplacement {
        source_bytes: replacement.source_bytes,
        text: replacement.replacement,
    })
}

fn merge_automatic_discretionary_breaks(
    opportunities: &mut Vec<InlineBreakOpportunity>,
    automatic_breaks: Vec<InlineBreakOpportunity>,
) {
    for automatic in automatic_breaks {
        // A language resource's explicit discretionary behavior owns this
        // source edge.  Remove any generic UAX candidate at the same edge so
        // selection cannot silently choose a marker-less interpretation.
        opportunities.retain(|opportunity| opportunity.position != automatic.position);
        opportunities.push(automatic);
    }
    opportunities.sort_by_key(|opportunity| {
        (
            opportunity.position,
            opportunity.availability.fitting_stage(),
        )
    });
}

fn merge_manual_discretionary_effects(
    opportunities: &mut [InlineBreakOpportunity],
    effects: Vec<(InlineGraphPosition, DiscretionaryBreakEffect)>,
) {
    for (position, effect) in effects {
        let Some(opportunity) = opportunities
            .iter_mut()
            .find(|opportunity| opportunity.position == position && opportunity.is_discretionary())
        else {
            continue;
        };
        opportunity.discretionary = Some(effect);
    }
}

fn graph_fragments_share_hyphenation_policy(left: &InlineFragment, right: &InlineFragment) -> bool {
    left.style().hyphens == right.style().hyphens
        && left.style().hyphens != Hyphens::None
        && left.style().language == right.style().language
        && (left.style().hyphens != Hyphens::Auto
            || left.style().hyphenate_limit_chars == right.style().hyphenate_limit_chars)
}

/// Shape source runs across transparent inline element edges.
///
/// CSS Text establishes shaping before it selects a line break. Keeping the
/// source-shaped slices on graph runs means a later selected soft-hyphen edge
/// cannot turn an Arabic medial glyph into a final glyph, or lose kerning at a
/// color-only typographic pseudo boundary, merely because an otherwise
/// transparent inline element owns that boundary:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
fn shape_logical_joining_graph_runs(
    runs: &mut [InlineParagraphRun],
    font_system: &mut FontSystem,
    tab_metric_style: &ComputedStyle,
) {
    let mut index = 0;
    while index < runs.len() {
        let InlineLineItem::Fragment(_) = &runs[index].item else {
            index += 1;
            continue;
        };
        let mut fragment_indices = vec![index];
        index += 1;
        while let Some(run) = runs.get(index) {
            match &run.item {
                InlineLineItem::Fragment(right) => {
                    let InlineLineItem::Fragment(left) =
                        &runs[*fragment_indices.last().expect("first graph fragment")].item
                    else {
                        unreachable!("graph fragment indices name fragments");
                    };
                    // A resolved nonzero tracking boundary must not be
                    // reshaped as one run: that could restore an optional
                    // ligature or contextual substitution across the visual
                    // gap. The sole exception is a cursive join, which has no
                    // tracking boundary in the first place and must retain
                    // its shaping context through transparent inline boxes.
                    if graph_fragments_have_nonjoining_tracking_boundary(left, right) {
                        break;
                    }
                    if !can_shape_inline_fragments_together(left, right) {
                        break;
                    }
                    fragment_indices.push(index);
                    index += 1;
                }
                InlineLineItem::Atom(atom) if graph_atom_is_transparent_to_shaping(atom) => {
                    index += 1;
                }
                InlineLineItem::Float(_) => {
                    // Floats alter line-box geometry but not the shaping
                    // context of their neighboring in-flow text.
                    // <https://www.w3.org/TR/css-text-3/#boundary-shaping>
                    index += 1;
                }
                InlineLineItem::Atom(_) => break,
            }
        }
        let needs_source_shape = fragment_indices.len() == 1
            && matches!(
                &runs[fragment_indices[0]].item,
                InlineLineItem::Fragment(fragment) if {
                    let mut opportunities = Vec::new();
                    collect_measured_break_opportunities(
                        fragment.text(),
                        TextBreakPolicy::from(fragment.style()),
                        &mut opportunities,
                    );
                    opportunities.into_iter().any(|position| {
                        position > 0 && position < fragment.text().len()
                    })
                }
            );
        if fragment_indices.len() < 2 && !needs_source_shape {
            continue;
        }
        let mut spans = Vec::with_capacity(fragment_indices.len());
        let mut text = String::new();
        let mut ranges = Vec::with_capacity(fragment_indices.len());
        let mut line_height = None;
        for &fragment_index in &fragment_indices {
            let InlineLineItem::Fragment(fragment) = &runs[fragment_index].item else {
                unreachable!("graph fragment indices name fragments");
            };
            line_height.get_or_insert(fragment.style().line_height);
            let start = text.len();
            text.push_str(fragment.text());
            ranges.push(start..text.len());
            spans.push(StyledTextSpan {
                text: fragment.text(),
                // Keep even a paint-only span boundary in the shaping input.
                // The styled shaper records its paint state independently,
                // and needs the boundary to synthesize cursive context for
                // scripts such as Mongolian that cannot recover it from a
                // flattened source string.
                // <https://drafts.csswg.org/css-text-3/#boundary-shaping>
                style: fragment.style(),
            });
        }
        let Some(shaped) = font_system.shape_untracked_styled_inline_fragments(
            &spans,
            text,
            0.0,
            line_height.expect("graph shaping group has a fragment"),
            0.0,
            tab_metric_style,
        ) else {
            continue;
        };
        let shaped = Rc::new(shaped);
        // `word-space-transform` keeps its replacement as a distinct graph
        // fragment so it can own the original separator's break and text
        // extraction behavior. It nevertheless has no CSS shaping boundary
        // of its own. Retain the complete source shape so visual ordering and
        // paint can directly emit the same glyph geometry as one literal replacement
        // character between the adjacent text fragments.
        // <https://drafts.csswg.org/css-text-4/#word-space-transform>
        let boundary_source = fragment_indices
            .iter()
            .any(|&fragment_index| {
                matches!(
                    &runs[fragment_index].item,
                    InlineLineItem::Fragment(fragment)
                        if matches!(fragment.source(), InlineTextSource::WordSpaceTransform(_))
                )
            })
            .then(|| {
                Rc::new(BoundaryShapedSource {
                    shaped: Rc::clone(&shaped),
                })
            });
        for (&fragment_index, range) in fragment_indices.iter().zip(ranges) {
            let Some(mut selection) =
                SourceShapedSelection::from_source(Rc::clone(&shaped), range.clone())
            else {
                continue;
            };
            let slice = Some(selection.selected().clone());
            let width = slice
                .as_ref()
                .map(ShapedInlineLine::advance_width)
                .unwrap_or(0.0);
            let InlineLineItem::Fragment(fragment) = &mut runs[fragment_index].item else {
                unreachable!("graph fragment indices name fragments");
            };
            if let Some(boundary_source) = &boundary_source {
                fragment.set_boundary_shaped_source(Rc::clone(boundary_source), range);
            }
            if let Some(slice) = slice.as_ref() {
                selection.replace_selected(slice.clone());
            }
            runs[fragment_index].width = width;
            runs[fragment_index].shaped = slice.map(Rc::new);
            fragment.set_source_shaped_selection(Some(selection));
        }
    }
}

fn graph_fragments_have_nonjoining_tracking_boundary(
    left: &InlineFragment,
    right: &InlineFragment,
) -> bool {
    let (Some(left_scope), Some(right_scope)) = (left.tracking_scope(), right.tracking_scope())
    else {
        return false;
    };
    InlineBoundaryAdvance::between(
        UsedLetterSpacing::new(left_scope.letter_spacing()),
        UsedLetterSpacing::new(right_scope.letter_spacing()),
    )
    .points()
        != 0.0
        && crate::text::inter_character_gap_allowed_between_text(left.text(), right.text())
}

fn graph_atom_is_transparent_to_shaping(atom: &InlineAtom) -> bool {
    matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge))
        if edge.advance == 0.0
            && edge.paint_extent == 0.0
            && !inline_box_edge_breaks_shaping(atom.style())
            && !inline_box_bidi_isolation_breaks_shaping(atom.style()))
}

pub(in crate::layout) fn push_text_graph_runs(
    font_system: &mut FontSystem,
    runs: &mut Vec<InlineParagraphRun>,
    word: &InlineWord,
    text: &str,
    tracking_scope: Rc<InlineTrackingScope>,
) {
    if text.is_empty() {
        return;
    }
    let break_text = if word.style.hyphens == Hyphens::Auto
        && matches!(
            word.style.automatic_discretionary_hyphenation_policy(),
            DiscretionaryHyphenationPolicy::Ordinary
        ) {
        Cow::Borrowed(text)
    } else {
        text_with_hyphenation_controls(text, &word.style)
    };
    let text = break_text.as_ref();
    if text.is_empty() {
        return;
    }
    let source_run = Rc::new(());
    if word.source == InlineTextSource::BidiControl {
        push_text_graph_run_segment(
            font_system,
            runs,
            word,
            text,
            word.hanging_edges,
            tracking_scope,
            source_run,
        );
        return;
    }

    // A nonzero `letter-spacing` value is resolved between CSS Text
    // typographic units, not between source fragments. Keep the fast,
    // source-fragment path for the overwhelmingly common untracked case, but
    // make every tracked unit explicit so a joining word remains one unit
    // while its adjacent space or non-joining content can own a boundary.
    // This also prevents optional ligatures from crossing a nonzero tracking
    // boundary.
    if tracking_scope.letter_spacing().points() == 0.0 {
        push_text_graph_run_segment(
            font_system,
            runs,
            word,
            text,
            word.hanging_edges,
            tracking_scope,
            source_run,
        );
        return;
    }
    for range in CursiveProtectedUnitRanges::new(text) {
        let mut hanging_edges = word.hanging_edges;
        hanging_edges.blocks_start &= range.start == 0;
        hanging_edges.blocks_end &= range.end == text.len();
        push_text_graph_run_segment(
            font_system,
            runs,
            word,
            &text[range],
            hanging_edges,
            Rc::clone(&tracking_scope),
            Rc::clone(&source_run),
        );
    }
}

pub(in crate::layout) fn push_text_graph_run_segment(
    font_system: &mut FontSystem,
    runs: &mut Vec<InlineParagraphRun>,
    word: &InlineWord,
    text: &str,
    hanging_edges: InlineHangingEdges,
    tracking_scope: Rc<InlineTrackingScope>,
    source_run: Rc<()>,
) {
    if text.is_empty() {
        return;
    }
    let mut fragment = InlineFragment::new_shared_style(
        text,
        Rc::clone(&word.style),
        word.baseline_shift,
        word.link_target.clone(),
        word.mergeable,
        word.source,
        false,
        hanging_edges,
        Rc::clone(&word.ancestor_inline_decorations),
    )
    .with_visual_offset(word.visual_offset)
    .with_source_run(source_run)
    .with_tracking_scope(tracking_scope);
    if word.style.content.is_generated() && word.style.display.is_flex() {
        // Generated content can use a block-level principal display (for
        // example, flex) while its fallback text remains in the enclosing
        // inline stream. Its principal decoration belongs to that generated
        // line fragment and must not be rejected as non-inline paint.
        // <https://drafts.csswg.org/css-content-3/#content-property>
        fragment.set_force_inline_background_paint(true);
    }
    // CSS-generated bidi controls are UAX #9 input only. Their fallback
    // glyph records must not contribute an inline advance or line metrics;
    // they remain as source fragments so visual ordering can still consume
    // the controls after line selection.
    // <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>
    let shaped = (word.source != InlineTextSource::BidiControl)
        .then(|| font_system.shape_untracked_inline_line(text, &word.style, word.style.line_height))
        .flatten();
    let width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    let shaped = shaped.map(Rc::new);
    runs.push(InlineParagraphRun {
        item: InlineLineItem::Fragment(fragment),
        width,
        shaped,
    });
}

fn run_tracking_scope<'a>(
    run: &'a InlineParagraphRun,
    fallback: &ComputedStyle,
) -> &'a Rc<InlineTrackingScope> {
    match &run.item {
        InlineLineItem::Fragment(fragment) => fragment
            .tracking_scope()
            .expect("graph fragments retain inline tracking scope"),
        InlineLineItem::Atom(atom) => atom
            .tracking_scope()
            .expect("graph atoms retain inline tracking scope"),
        InlineLineItem::Float(_) => {
            let _ = fallback;
            unreachable!("first-letter graph fragments do not inherit float scope")
        }
    }
}

pub(in crate::layout) fn inline_run_has_nonzero_tracking(run: &InlineParagraphRun) -> bool {
    match &run.item {
        InlineLineItem::Fragment(fragment) => fragment
            .tracking_scope()
            .is_some_and(|scope| scope.letter_spacing().points() != 0.0),
        InlineLineItem::Atom(atom) => atom
            .tracking_scope()
            .is_some_and(|scope| scope.letter_spacing().points() != 0.0),
        InlineLineItem::Float(_) => false,
    }
}

impl InlineOpportunityGraph {
    fn new(runs: Vec<InlineParagraphRun>, opportunities: Vec<InlineBreakOpportunity>) -> Self {
        let mut graph = Self {
            runs,
            opportunities,
            wrap_inside_avoid_depths: BTreeMap::new(),
        };
        for opportunity in &graph.opportunities {
            let depth = graph.lexical_wrap_inside_avoid_depth(opportunity.position);
            graph
                .wrap_inside_avoid_depths
                .insert(opportunity.position, depth);
        }
        graph
    }

    pub(in crate::layout) fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub(in crate::layout) fn start_position(&self) -> InlineGraphPosition {
        InlineGraphPosition::at_run_start(0)
    }

    pub(in crate::layout) fn end_position(&self) -> InlineGraphPosition {
        InlineGraphPosition::at_run_start(self.runs.len())
    }

    /// Return the number of `wrap-inside: avoid` inline boxes split by a
    /// candidate source boundary.
    ///
    /// Collection retains zero-advance inline box edges even when there is no
    /// decoration to paint. Those lexical markers let line selection recover
    /// non-inherited inline-box containment without making `wrap-inside`
    /// behave like an inherited text property. A boundary immediately before
    /// an end edge is outside that box, matching CSS Text's margin-edge rule.
    /// <https://drafts.csswg.org/css-text-4/#wrap-inside-property>
    /// <https://drafts.csswg.org/css-text-4/#line-breaking-details>
    pub(in crate::layout) fn wrap_inside_avoid_depth(&self, position: InlineGraphPosition) -> u16 {
        self.wrap_inside_avoid_depths
            .get(&position)
            .copied()
            .unwrap_or_else(|| self.lexical_wrap_inside_avoid_depth(position))
    }

    fn lexical_wrap_inside_avoid_depth(&self, position: InlineGraphPosition) -> u16 {
        let mut depth = 0u16;
        for run in &self.runs[..position.run_index] {
            if inline_box_edge_is_wrap_inside_avoid_start(&run.item) {
                depth = depth.saturating_add(1);
            } else if inline_box_edge_is_wrap_inside_avoid_end(&run.item) {
                depth = depth.saturating_sub(1);
            }
        }

        let Some(run) = self.runs.get(position.run_index) else {
            return depth;
        };
        let mut trailing_edge_index = match &run.item {
            InlineLineItem::Fragment(fragment) if position.byte_offset == fragment.text().len() => {
                position.run_index + 1
            }
            InlineLineItem::Atom(_) if position.byte_offset == 0 => position.run_index,
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                return depth;
            }
        };

        // A candidate at the end of text is immediately before its following
        // box-end markers. Those markers are on the trailing margin edge and
        // therefore lie outside `wrap-inside: avoid`; source positions encode
        // it as the text's terminal offset rather than a separate run start.
        while let Some(edge_run) = self.runs.get(trailing_edge_index) {
            if inline_box_edge_is_wrap_inside_avoid_end(&edge_run.item) {
                depth = depth.saturating_sub(1);
                trailing_edge_index += 1;
                continue;
            }
            break;
        }
        depth
    }

    pub(in crate::layout) fn float_at_position(
        &self,
        position: InlineGraphPosition,
    ) -> Option<&InlineFloat> {
        if position.byte_offset != 0 {
            return None;
        }
        self.runs
            .get(position.run_index)
            .and_then(|run| match &run.item {
                InlineLineItem::Float(float) => Some(float),
                InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) => None,
            })
    }

    pub(in crate::layout) fn first_float_position_in_range(
        &self,
        range: InlineGraphRange,
    ) -> Option<InlineGraphPosition> {
        let run_range = self.run_indices_for_graph_range(range)?;
        run_range
            .filter(|run_index| {
                *run_index >= range.start.run_index
                    && *run_index < range.end.run_index
                    && matches!(
                        self.runs.get(*run_index).map(|run| &run.item),
                        Some(InlineLineItem::Float(_))
                    )
            })
            .map(InlineGraphPosition::at_run_start)
            .next()
    }

    pub(in crate::layout) fn line_measured_items_for_graph_range(
        &self,
        range: InlineGraphRange,
        font_system: &mut FontSystem,
    ) -> Vec<MeasuredInlineItem> {
        let Some(run_range) = self.run_indices_for_graph_range(range) else {
            return Vec::new();
        };
        let mut items = Vec::with_capacity(run_range.len());
        for run_index in run_range {
            if let Some(item) =
                self.measured_run_slice_for_graph_range(run_index, range, font_system)
            {
                items.push(item);
            }
        }
        items
    }

    pub(in crate::layout) fn materialize_line(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        self.materialize_line_with_text_spacing_width(
            range,
            selected_break,
            self.selected_line_end_condition(range, selected_break),
            None,
            font_system,
            block_style,
        )
    }

    /// Materialize a candidate against a known line measure. This retains the
    /// CSS Text conditional end-trimming decision in the selected line rather
    /// than recomputing a different shaped run for paint.
    pub(in crate::layout) fn materialize_line_for_available_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        available_width: f32,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        self.materialize_line_with_text_spacing_width(
            range,
            selected_break,
            self.selected_line_end_condition(range, selected_break),
            Some(available_width),
            font_system,
            block_style,
        )
    }

    /// Materialize a selected line with its CSS Text line-end condition and
    /// physical available width.
    pub(in crate::layout) fn materialize_line_for_selected_end_for_available_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        line_end: SelectedLineEndCondition,
        available_width: f32,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        self.materialize_line_with_text_spacing_width(
            range,
            selected_break,
            line_end,
            Some(available_width),
            font_system,
            block_style,
        )
    }

    fn materialize_line_with_text_spacing_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        line_end: SelectedLineEndCondition,
        text_spacing_available_width: Option<f32>,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        debug_assert!(selected_break.is_none_or(|opportunity| {
            !opportunity.trims_next_line_start()
                || matches!(opportunity.kind, BreakEffect::PreservedSpace)
        }));
        let mut items = self.line_measured_items_for_graph_range(range, font_system);
        self.insert_clone_continuation_edges(range, &mut items);
        let selected_manual_soft_hyphen = selected_break.is_some_and(|opportunity| {
            opportunity.is_discretionary()
                && self.source_character_before(opportunity.position) == Some('\u{00ad}')
        });
        let trailing_discretionary = selected_break.and_then(|opportunity| {
            opportunity.discretionary.or_else(|| {
                // An authored U+00AD is itself a discretionary break even
                // when no language-specific spelling rule supplies a more
                // detailed effect. Its own source fragment owns the used
                // `hyphenate-character`.
                opportunity
                    .is_discretionary()
                    .then_some(DiscretionaryBreakEffect {
                        source_boundary: opportunity.position,
                        marker_owner: DiscretionaryMarkerOwner {
                            style_position: opportunity.position,
                        },
                        left_replacement: None,
                        right_replacement: None,
                        leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
                    })
            })
        });
        let leading_discretionary = self.discretionary_effect_at(range.start);
        // Do not mutate the selected source sequence for CSS Text Phase II.
        // In particular, a collapsed separator before `br` remains available
        // to bidi, extraction, and decoration ownership even though it has no
        // used advance at the selected line edge.
        let trimmed_width = trailing_collapsible_measured_width(&items);
        let authored_marker_materialized_in_source = selected_manual_soft_hyphen
            && trailing_discretionary.is_some_and(|effect| effect.left_replacement.is_none());
        normalize_materialized_control_characters(
            &mut items,
            authored_marker_materialized_in_source,
            font_system,
        );
        if authored_marker_materialized_in_source {
            if trailing_discretionary.is_some_and(|effect| {
                effect.leading_shaping_context == SelectedLineShapingContext::PreserveJoining
            }) && materialized_items_have_joining_behavior(&items)
            {
                append_materialized_line_joiner(&mut items, font_system);
            }
        } else {
            apply_selected_discretionary_break(
                &mut items,
                trailing_discretionary,
                SelectedLineEdge::Trailing,
                font_system,
                &self.runs,
            );
        }
        apply_selected_discretionary_break(
            &mut items,
            leading_discretionary,
            SelectedLineEdge::Leading,
            font_system,
            &self.runs,
        );
        apply_materialized_text_spacing_trim(
            &mut items,
            font_system,
            range.start == self.start_position(),
            text_spacing_available_width,
        );
        // Candidate fitting and intrinsic sizing use the same ownership-aware
        // tracking advances as final visual paint. In an all-LTR candidate
        // this is already visual order; bidi paint resolves the same typed
        // boundaries again after UBA reordering.
        apply_visual_tracking_boundaries(&mut items);
        resolve_materialized_line_tab_and_ruby_geometry(&mut items, font_system, block_style);
        let widths = inline_content_width_for_line_items(&items, font_system, |item| {
            item.used_advance().points()
        });
        // A `pre-wrap` run hangs at a selected soft boundary.  It also hangs
        // before an unconditionally hanging other-space separator, even when
        // the line itself ends at a forced break: that separator means the
        // preserved run is not immediately followed by the forced break.
        // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
        let pre_wrap_suffix_width =
            trailing_pre_wrap_hanging_width_with_unconditional_separators(&items, font_system);
        let hanging_pre_wrap_width = if widths.trailing_space_width > 0.0 {
            pre_wrap_suffix_width
        } else {
            line_end.pre_wrap_hanging_width(
                pre_wrap_suffix_width,
                widths.fitting_width,
                text_spacing_available_width,
            )
        };
        let edge_effects = InlineLineEdgeEffects {
            collapsed_end_trim_width: trimmed_width,
            pre_wrap_hanging_width: hanging_pre_wrap_width,
            hanging_space_separator_width: widths.trailing_space_width,
            retained_break_spaces_end: selected_break.is_some_and(|opportunity| {
                opportunity.whitespace_edge.retains_break_spaces_advance()
            }),
            source_effects: selected_line_edge_source_effects(
                &items,
                trimmed_width > 0.0,
                hanging_pre_wrap_width > 0.0,
                widths.trailing_space_width > 0.0,
            ),
        };
        // The materialized line retains the selected source text. Edge
        // effects change its used advance; paint materialization applies the
        // corresponding source-range suppression only when emitting the
        // formatted fragment. Keeping this summary source-faithful preserves
        // bidi, extraction, and decoration ownership.
        let fitting_width = (widths.fitting_width
            - edge_effects.collapsed_end_trim_width
            - edge_effects.pre_wrap_hanging_width)
            .max(0.0);
        MaterializedInlineGraphLine {
            items,
            fitting_width,
            content_width: fitting_width,
            edge_effects,
        }
    }

    pub(in crate::layout) fn selected_line_end_condition(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
    ) -> SelectedLineEndCondition {
        match selected_break {
            Some(opportunity) if opportunity_is_soft_wrap(opportunity) => {
                SelectedLineEndCondition::SoftWrap
            }
            Some(_) => SelectedLineEndCondition::ForcedBreak,
            None if range.end == self.end_position() => SelectedLineEndCondition::ParagraphEnd,
            None => SelectedLineEndCondition::ForcedBreak,
        }
    }

    pub(in crate::layout) fn source_character_before(
        &self,
        position: InlineGraphPosition,
    ) -> Option<char> {
        if position.byte_offset > 0 {
            return self
                .runs
                .get(position.run_index)
                .and_then(|run| match &run.item {
                    InlineLineItem::Fragment(fragment) => fragment
                        .text()
                        .get(..position.byte_offset)
                        .and_then(|text| text.chars().next_back()),
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
                });
        }
        previous_text_fragment_before(&self.runs, position.run_index)
            .and_then(|fragment| fragment.text().chars().next_back())
    }

    fn discretionary_effect_at(
        &self,
        position: InlineGraphPosition,
    ) -> Option<DiscretionaryBreakEffect> {
        self.opportunities
            .iter()
            .find(|opportunity| opportunity.position == position)
            .and_then(|opportunity| opportunity.discretionary)
    }

    pub(in crate::layout) fn break_opportunities_after(
        &self,
        start: InlineGraphPosition,
    ) -> impl Iterator<Item = InlineBreakOpportunity> + '_ {
        self.opportunities
            .iter()
            .cloned()
            .filter(move |opportunity| opportunity.position > start)
    }

    pub(in crate::layout) fn break_opportunity_at(
        &self,
        position: InlineGraphPosition,
    ) -> Option<InlineBreakOpportunity> {
        self.opportunities
            .iter()
            .find(|opportunity| opportunity.position == position)
            .copied()
    }

    /// Whether a selected soft hyphen is immediately followed by a source
    /// hard hyphen before another candidate boundary.
    ///
    /// Some language dictionaries treat this as a discretionary replacement:
    /// the selected first line gets `hyphenate-character`, while the literal
    /// hyphen remains at the following line edge. Selecting the later UAX #14
    /// boundary would instead consume that source character and lose the
    /// replacement. The relationship is source-local and therefore belongs to
    /// the opportunity graph rather than to a line-selector string heuristic.
    /// <https://drafts.csswg.org/css-text-4/#hyphenate-character>
    pub(in crate::layout) fn soft_hyphen_precedes_literal_hyphen(
        &self,
        soft_hyphen: InlineBreakOpportunity,
        later: InlineBreakOpportunity,
    ) -> bool {
        if !soft_hyphen.is_discretionary()
            || soft_hyphen.position.run_index != later.position.run_index
            || soft_hyphen.position.byte_offset >= later.position.byte_offset
        {
            return false;
        }
        let Some(InlineLineItem::Fragment(fragment)) = self
            .runs
            .get(soft_hyphen.position.run_index)
            .map(|run| &run.item)
        else {
            return false;
        };
        let Some(between) = fragment
            .text()
            .get(soft_hyphen.position.byte_offset..later.position.byte_offset)
        else {
            return false;
        };
        between == "\u{2010}"
    }

    pub(in crate::layout) fn run_indices_for_graph_range(
        &self,
        range: InlineGraphRange,
    ) -> Option<std::ops::Range<usize>> {
        if range.end <= range.start || range.start.run_index >= self.runs.len() {
            return None;
        }
        let end_run = if range.end.byte_offset == 0 {
            range.end.run_index
        } else {
            range.end.run_index.saturating_add(1)
        }
        .min(self.runs.len());
        (range.start.run_index < end_run).then_some(range.start.run_index..end_run)
    }

    fn range_may_use_text_spacing_trim(&self, range: InlineGraphRange) -> bool {
        let Some(run_range) = self.run_indices_for_graph_range(range) else {
            return false;
        };
        self.runs[run_range].iter().any(|run| {
            let InlineLineItem::Fragment(fragment) = &run.item else {
                return false;
            };
            if fragment.style().text_spacing_trim.resolved() == TextSpacingTrim::SpaceAll {
                return false;
            }
            let vertical = matches!(
                fragment.style().text_layout_policy(),
                crate::css::TextLayoutPolicy::Vertical(_)
            );
            // `text-spacing-trim` changes only classified CJK punctuation.
            // Ordinary text (including preserved white space) retains its
            // source advance, so it can safely reuse the graph's borrowed
            // source measurement. Keeping this eligibility test content-aware
            // avoids unnecessarily materializing every standard text line.
            fragment.text().chars().any(|character| {
                crate::text::text_spacing_punctuation_class(
                    character,
                    fragment.style().language.as_deref(),
                    vertical,
                )
                .is_some()
            })
        })
    }

    pub(in crate::layout) fn borrowed_line_measurement_for_full_run_range(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> Option<BorrowedInlineLineMeasurement> {
        // `text-spacing-trim` selects used punctuation advances only after a
        // candidate establishes its line edges. A source-shaped graph slice
        // therefore cannot be borrowed as a final measurement when any
        // selected text can participate in that policy.
        if self.range_may_use_text_spacing_trim(range) {
            return None;
        }
        // A selected discretionary edge owns a generated marker and may own
        // spelling/shaping changes. Reuse of source-run widths would omit
        // those used-line effects, so only the full materializer may measure
        // it.
        if selected_break.is_some_and(|opportunity| opportunity.discretionary.is_some()) {
            return None;
        }
        if range.start.byte_offset != 0 || range.end.byte_offset != 0 {
            return None;
        }
        let run_range = self.run_indices_for_graph_range(range)?;
        if self.runs[run_range.clone()]
            .iter()
            .any(inline_run_has_nonzero_tracking)
        {
            return None;
        }
        // Ruby overhang depends on selected neighboring source and may reduce
        // the provisional widest-annotation advance. Intrinsic sizing must
        // therefore take the same materialized path as final line layout.
        if self.runs[run_range.clone()].iter().any(|run| {
            matches!(
                run.item,
                InlineLineItem::Atom(ref atom) if matches!(atom.content(), InlineAtomContent::Ruby { .. })
            )
        }) {
            return None;
        }
        // This fast path is used exclusively for intrinsic segments. A final
        // segment ending at the paragraph boundary must therefore use the
        // intrinsic end condition, rather than physical paragraph-end
        // conditional hanging. Otherwise preserved `pre-wrap` spaces
        // incorrectly contribute to min-content width.
        let line_end = selected_break
            .map(|opportunity| self.selected_line_end_condition(range, Some(opportunity)))
            .unwrap_or(SelectedLineEndCondition::IntrinsicSegmentEnd);
        let hanging_pre_wrap_width = if matches!(
            line_end,
            SelectedLineEndCondition::SoftWrap | SelectedLineEndCondition::IntrinsicSegmentEnd
        ) {
            trailing_pre_wrap_hanging_width_with_unconditional_separators(
                &self.runs[run_range.clone()],
                font_system,
            )
        } else {
            0.0
        };
        let runs = &self.runs[run_range];
        if runs.iter().any(|run| match &run.item {
            InlineLineItem::Fragment(fragment) => fragment_text_needs_materialized_normalization(
                fragment.text(),
                selected_break.is_some_and(InlineBreakOpportunity::is_discretionary),
            ),
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => false,
        }) {
            return None;
        }
        let widths = inline_content_width_for_line_items(runs, font_system, |run| run.width);
        let tab_advance_adjustment =
            selected_line_tab_advance_adjustment(runs, font_system, block_style, |run| run.width);
        Some(BorrowedInlineLineMeasurement {
            content_width: (widths.content_width + tab_advance_adjustment
                - trailing_collapsible_run_width(runs)
                - hanging_pre_wrap_width)
                .max(0.0),
        })
    }

    pub(in crate::layout) fn measured_run_slice_for_graph_range(
        &self,
        run_index: usize,
        range: InlineGraphRange,
        font_system: &mut FontSystem,
    ) -> Option<MeasuredInlineItem> {
        let run = self.runs.get(run_index)?;
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                let bidi_continuations = self.bidi_scope_continuations_for_range(range);
                let owns_prefix = run_index == range.start.run_index;
                let last_selected_run = if range.end.byte_offset == 0 {
                    range.end.run_index.checked_sub(1)
                } else {
                    Some(range.end.run_index)
                };
                let owns_suffix = Some(run_index) == last_selected_run;
                let mut bidi_prefix = String::new();
                if owns_prefix {
                    bidi_prefix.push_str(&bidi_continuations.prefix_parent_context);
                    bidi_prefix.push_str(&bidi_continuations.prefix);
                }
                let mut bidi_suffix = String::new();
                if owns_suffix {
                    bidi_suffix.push_str(&bidi_continuations.trailing_line_edge_context);
                    bidi_suffix.push_str(&bidi_continuations.suffix);
                    bidi_suffix.push_str(&bidi_continuations.suffix_parent_context);
                }
                let has_bidi_scope_context = !bidi_prefix.is_empty() || !bidi_suffix.is_empty();
                let text_len = fragment.text().len();
                let start = if run_index == range.start.run_index {
                    range.start.byte_offset.min(text_len)
                } else {
                    0
                };
                let end = if run_index == range.end.run_index {
                    range.end.byte_offset.min(text_len)
                } else {
                    text_len
                };
                if start >= end
                    || !fragment.text().is_char_boundary(start)
                    || !fragment.text().is_char_boundary(end)
                {
                    return None;
                }
                if start == 0 && end == text_len && !has_bidi_scope_context {
                    return Some(MeasuredInlineItem::new(
                        run.item.clone(),
                        run.width,
                        run.shaped.clone(),
                    ));
                }
                let mut fragment = fragment.clone();
                let mut hanging_edges = fragment.hanging_edges();
                let segment_text = Rc::<str>::from(&fragment.text()[start..end]);
                fragment.set_text(segment_text);
                hanging_edges.blocks_start = hanging_edges.blocks_start && start == 0;
                hanging_edges.blocks_end = hanging_edges.blocks_end && end == text_len;
                fragment = fragment.with_hanging_edges(hanging_edges);
                let mut source_selection = (!has_bidi_scope_context)
                    .then(|| {
                        // A transparent inline-boundary group may already
                        // carry a selection from a larger logical source.
                        // Derive this line fragment from that original source
                        // rather than treating the graph's intermediate
                        // slice as a new word-shaped artifact.
                        fragment
                            .source_shaped_selection()
                            .and_then(|selection| selection.subselection(start..end))
                            .or_else(|| {
                                run.shaped.as_ref().and_then(|shaped| {
                                    SourceShapedSelection::from_source(
                                        Rc::clone(shaped),
                                        start..end,
                                    )
                                })
                            })
                    })
                    .flatten();
                let shaped = source_selection
                    .as_ref()
                    .map(|selection| selection.selected().clone())
                    .or_else(|| {
                        font_system.shape_bidi_scoped_logical_line(
                            fragment.text(),
                            fragment.style(),
                            fragment.style().line_height,
                            &bidi_prefix,
                            &bidi_suffix,
                        )
                    });
                let width = shaped
                    .as_ref()
                    .map(ShapedInlineLine::advance_width)
                    .unwrap_or(0.0);
                if let (Some(selection), Some(shaped)) = (&mut source_selection, shaped.as_ref()) {
                    selection.replace_selected(shaped.clone());
                }
                let shaped = shaped.map(Rc::new);
                fragment.set_source_shaped_selection(source_selection);
                Some(MeasuredInlineItem::new(
                    InlineLineItem::Fragment(fragment),
                    width,
                    shaped,
                ))
            }
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                let run_start = InlineGraphPosition::at_run_start(run_index);
                let run_end = InlineGraphPosition::at_run_start(run_index + 1);
                (range.start <= run_start && run_end <= range.end).then(|| {
                    MeasuredInlineItem::new(run.item.clone(), run.width, run.shaped.clone())
                })
            }
        }
    }

    /// Measure a plain source range without materializing selected line items.
    ///
    /// The monotonic `word-break: break-all` selector is the sole caller. Its
    /// eligibility check excludes all CSS Text effects that can make a source
    /// advance differ from the selected line's fitting advance.
    pub(in crate::layout) fn monotonic_source_range_width(
        &self,
        range: InlineGraphRange,
    ) -> Option<f32> {
        let run_range = self.run_indices_for_graph_range(range)?;
        let mut width = 0.0;
        for run_index in run_range {
            let run = self.runs.get(run_index)?;
            let InlineLineItem::Fragment(fragment) = &run.item else {
                return None;
            };
            let text_len = fragment.text().len();
            let start = if run_index == range.start.run_index {
                range.start.byte_offset.min(text_len)
            } else {
                0
            };
            let end = if run_index == range.end.run_index {
                range.end.byte_offset.min(text_len)
            } else {
                text_len
            };
            if start >= end
                || !fragment.text().is_char_boundary(start)
                || !fragment.text().is_char_boundary(end)
            {
                return None;
            }
            width += if start == 0 && end == text_len {
                run.width
            } else {
                run.shaped
                    .as_deref()?
                    .source_range_advance_width(start..end)?
            };
        }
        Some(width)
    }

    pub(in crate::layout) fn intrinsic_contribution(
        &self,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> InlineIntrinsicContribution {
        if self.runs.is_empty() {
            return InlineIntrinsicContribution::default();
        }
        let materialized = self.materialize_line(
            InlineGraphRange {
                start: self.start_position(),
                end: self.end_position(),
            },
            None,
            font_system,
            block_style,
        );
        let hanging_widths = hanging_punctuation_widths_for_line_items(
            font_system,
            &materialized.items,
            block_style,
            true,
            true,
            false,
        );
        let max_content =
            (materialized.content_width - hanging_widths.start - hanging_widths.end).max(0.0);
        let mut min_content = 0.0_f32;
        let mut segment_start = self.start_position();
        for opportunity in self
            .opportunities
            .iter()
            .cloned()
            .filter(|opportunity| opportunity.availability.participates_in_min_content())
        {
            if opportunity.position <= segment_start || opportunity.position >= self.end_position()
            {
                continue;
            }
            let range = InlineGraphRange {
                start: segment_start,
                end: opportunity.position,
            };
            min_content = min_content.max(self.intrinsic_segment_width(
                range,
                Some(opportunity),
                font_system,
                block_style,
            ));
            segment_start = opportunity.position;
        }
        min_content = min_content.max(self.intrinsic_segment_width(
            InlineGraphRange {
                start: segment_start,
                end: self.end_position(),
            },
            None,
            font_system,
            block_style,
        ));
        InlineIntrinsicContribution::new(
            LogicalInlineContentSize::new(content_box_pt(min_content)),
            LogicalInlineContentSize::new(content_box_pt(max_content.max(min_content))),
        )
    }

    pub(in crate::layout) fn intrinsic_segment_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> f32 {
        if let Some(measurement) = self.borrowed_line_measurement_for_full_run_range(
            range,
            selected_break,
            font_system,
            block_style,
        ) {
            return measurement.content_width;
        }
        // The final preserved `pre-wrap` spaces remain in the source line and
        // its max-content geometry, but are non-constraining at the end of a
        // min-content segment. Materialize that terminal candidate with the
        // same Phase II hanging rule used by the borrowed fast path above.
        // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
        let materialized = self.materialize_line_with_text_spacing_width(
            range,
            selected_break,
            if selected_break.is_none() && range.end == self.end_position() {
                SelectedLineEndCondition::IntrinsicSegmentEnd
            } else {
                self.selected_line_end_condition(range, selected_break)
            },
            None,
            font_system,
            block_style,
        );
        if materialized.items.is_empty() {
            return 0.0;
        }
        materialized.content_width
    }
}

/// The complete shaping and used-text request for one selected discretionary
/// edge.  Source fragments remain unchanged in the graph; this request is
/// applied only to the short-lived line materialization used for measuring and
/// painting the selected line.
#[derive(Debug, Clone, Copy)]
struct SelectedLineShapingRequest {
    effect: DiscretionaryBreakEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectedLineEdge {
    Leading,
    Trailing,
}

/// Apply CSS Text's selected discretionary behavior without inspecting the
/// language or spelling in the materializer.  Language resources create the
/// effect; this generic boundary applies its two replacements, non-painting
/// shaping context, and separate used marker.
fn apply_selected_discretionary_break(
    items: &mut Vec<MeasuredInlineItem>,
    effect: Option<DiscretionaryBreakEffect>,
    edge: SelectedLineEdge,
    font_system: &mut FontSystem,
    graph_runs: &[InlineParagraphRun],
) {
    let Some(effect) = effect else {
        return;
    };
    let request = SelectedLineShapingRequest { effect };
    match edge {
        SelectedLineEdge::Trailing => {
            if let Some(replacement) = request.effect.left_replacement {
                apply_trailing_line_edge_replacement(items, replacement, font_system);
            }
            if request.effect.leading_shaping_context == SelectedLineShapingContext::PreserveJoining
                && materialized_items_have_joining_behavior(items)
            {
                append_materialized_line_joiner(items, font_system);
            }
            // The marker owns the trailing shaping context. Keeping its ZWJ
            // in the generated item places it immediately before
            // `hyphenate-character` in logical text, including RTL markers
            // whose leading NBSP must not be shaped as a line-start document
            // space.
            append_discretionary_marker(items, request.effect, graph_runs, font_system);
        }
        SelectedLineEdge::Leading => {
            if let Some(replacement) = request.effect.right_replacement {
                apply_leading_line_edge_replacement(items, replacement, font_system);
            }
            if request.effect.leading_shaping_context == SelectedLineShapingContext::PreserveJoining
                && materialized_items_have_joining_behavior(items)
            {
                // This helper presently uses a ZWJ-backed shaper request. The
                // ZWJ never becomes a separate paint item; the typed request
                // above is the sole authority for its use at a selected edge.
                prepend_materialized_line_joiner(items, font_system);
            }
        }
    }
}

fn apply_trailing_line_edge_replacement(
    items: &mut [MeasuredInlineItem],
    replacement: InlineLineEdgeReplacement,
    font_system: &mut FontSystem,
) {
    let Some(item) = items.iter_mut().rev().find(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
    }) else {
        return;
    };
    let InlineLineItem::Fragment(fragment) = &mut item.item else {
        return;
    };
    let text = fragment.text();
    let Some(prefix_end) = text.len().checked_sub(replacement.source_bytes) else {
        return;
    };
    if !text.is_char_boundary(prefix_end) {
        return;
    }
    let mut used = String::with_capacity(prefix_end + replacement.text.len());
    used.push_str(&text[..prefix_end]);
    used.push_str(replacement.text);
    fragment.set_text(used);
    remeasure_materialized_item(item, font_system);
}

fn apply_leading_line_edge_replacement(
    items: &mut [MeasuredInlineItem],
    replacement: InlineLineEdgeReplacement,
    font_system: &mut FontSystem,
) {
    let Some(item) = items.iter_mut().find(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
    }) else {
        return;
    };
    let InlineLineItem::Fragment(fragment) = &mut item.item else {
        return;
    };
    let text = fragment.text();
    if replacement.source_bytes > text.len() || !text.is_char_boundary(replacement.source_bytes) {
        return;
    }
    let mut used = String::with_capacity(replacement.text.len() + text.len());
    used.push_str(replacement.text);
    used.push_str(&text[replacement.source_bytes..]);
    fragment.set_text(used);
    remeasure_materialized_item(item, font_system);
}

/// Append the selected `hyphenate-character` as a paint item distinct from
/// the source fragment that owns the break.  This keeps its style, bidi
/// behavior, and advance visible to normal line materialization rather than
/// disguising it as an edit to a source word.
fn append_discretionary_marker(
    items: &mut Vec<MeasuredInlineItem>,
    effect: DiscretionaryBreakEffect,
    graph_runs: &[InlineParagraphRun],
    font_system: &mut FontSystem,
) {
    let Some(source_index) = items.iter().rposition(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
    }) else {
        return;
    };
    let InlineLineItem::Fragment(source_fragment) = &items[source_index].item else {
        return;
    };
    let Some(InlineLineItem::Fragment(marker_owner)) = graph_runs
        .get(effect.marker_owner.style_position.run_index)
        .map(|run| &run.item)
    else {
        return;
    };
    let marker_text = used_discretionary_marker_text(marker_owner);
    let mut marker = marker_owner.clone();
    marker.set_text(marker_text);
    marker.mark_selected_discretionary_marker();
    let source_text = source_fragment.text().to_owned();
    let marker_range = source_text.len()..source_text.len() + marker.text().len();
    let mut logical_text = source_text;
    logical_text.push_str(marker.text());
    let spans = [
        StyledTextSpan {
            text: source_fragment.text(),
            style: source_fragment.style(),
        },
        StyledTextSpan {
            text: marker.text(),
            style: marker.style(),
        },
    ];
    // Shape the generated marker with the selected source edge before it is
    // separated into paint items. Script fallback and Arabic joining are
    // chosen for the complete logical request; `source_slice` then gives each
    // item only its source-owned glyph cluster range.
    // <https://www.w3.org/TR/css-text-3/#boundary-shaping>
    if let Some(shaped) = font_system.shape_untracked_styled_inline_fragments(
        &spans,
        logical_text,
        0.0,
        source_fragment.style().line_height,
        0.0,
        source_fragment.style(),
    ) {
        let source_range = 0..marker_range.start;
        if let Some(source_slice) = shaped.source_slice(source_range) {
            let source = &mut items[source_index];
            source
                .advance
                .replace_base_points(source_slice.advance_width());
            source.shaped = Some(Rc::new(source_slice));
        }
        if let Some(marker_slice) = shaped.source_slice(marker_range) {
            let width = marker_slice.advance_width();
            items.push(MeasuredInlineItem::new(
                InlineLineItem::Fragment(marker),
                width,
                Some(Rc::new(marker_slice)),
            ));
            return;
        }
    }
    let mut materialized_marker =
        MeasuredInlineItem::new(InlineLineItem::Fragment(marker), 0.0, None);
    remeasure_materialized_item(&mut materialized_marker, font_system);
    items.push(materialized_marker);
}

/// Resolve the UA-selected `hyphenate-character` at the selected line edge.
/// Default horizontal text delegates its language-specific choice to the
/// computed `hyphenate-character`; vertical layout uses U+2010, whose vertical
/// form is the interoperable conditional-hyphen presentation.
/// Explicit author strings remain unchanged in every writing mode.
/// <https://drafts.csswg.org/css-text-4/#hyphenate-character>
fn used_discretionary_marker_text(fragment: &InlineFragment) -> &str {
    if matches!(
        fragment.style().hyphenate_character,
        crate::css::HyphenateCharacter::Auto
    ) && matches!(
        fragment.style().text_layout_policy(),
        crate::css::TextLayoutPolicy::Vertical(_)
    ) {
        "\u{2010}"
    } else {
        fragment
            .style()
            .hyphenate_character
            .used_text_for_language(fragment.style().language.as_deref())
    }
}

/// Reconcile tabs after graph materialization has rejoined adjacent text runs.
///
/// The opportunity graph intentionally splits text at legal boundaries, but a
/// preserved tab's advance depends on all preceding text in its selected line.
/// Re-shaping one all-text materialized line keeps its fitting and alignment
/// measure consistent with the boundary-shaped paint group. Atomic inline
/// participants contribute to the running logical inline cursor even though
/// they split a shaping group, so a following tab resolves from the same block
/// content edge as it does during paint:
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>.
fn selected_line_tab_advance_adjustment<T>(
    items: &[T],
    font_system: &mut FontSystem,
    tab_metric_style: &ComputedStyle,
    item_width: impl Fn(&T) -> f32,
) -> f32
where
    T: AsRef<InlineLineItem>,
{
    let mut cursor = 0.0;
    let mut adjustment = 0.0;
    let mut index = 0;
    while index < items.len() {
        let InlineLineItem::Fragment(first_fragment) = items[index].as_ref() else {
            cursor += item_width(&items[index]);
            index += 1;
            continue;
        };

        let start = index;
        let mut spans = Vec::new();
        let mut text = String::new();
        let mut unadjusted_width = 0.0;
        let mut has_tab = false;
        while let Some(item) = items.get(index) {
            let InlineLineItem::Fragment(fragment) = item.as_ref() else {
                break;
            };
            has_tab |= fragment.text().contains('\t');
            spans.push(StyledTextSpan {
                text: fragment.text(),
                style: fragment.style(),
            });
            text.push_str(fragment.text());
            unadjusted_width += item_width(item);
            index += 1;
        }
        debug_assert!(index > start);
        let used_width = if has_tab {
            font_system
                .shape_untracked_styled_inline_fragments(
                    &spans,
                    text,
                    0.0,
                    first_fragment.style().line_height,
                    cursor,
                    tab_metric_style,
                )
                .map(|shaped| shaped.advance_width())
                .unwrap_or(unadjusted_width)
        } else {
            unadjusted_width
        };
        adjustment += used_width - unadjusted_width;
        cursor += used_width;
    }
    adjustment
}

/// Tabs depend on the used inline cursor, while ruby overhang can reduce the
/// preceding atom's used advance. Iterate the two selected-line operations to
/// a small fixed point so a tab after ruby never retains a stop calculated
/// from the source atom's conservative annotation width.
fn resolve_materialized_line_tab_and_ruby_geometry(
    items: &mut [MeasuredInlineItem],
    font_system: &mut FontSystem,
    block_style: &ComputedStyle,
) {
    if !items.iter().any(|item| {
        matches!(
            &item.item,
            InlineLineItem::Fragment(fragment) if fragment.text().contains('\t')
        ) || matches!(
            &item.item,
            InlineLineItem::Atom(atom) if matches!(atom.content(), InlineAtomContent::Ruby { .. })
        )
    }) {
        return;
    }
    const MAX_GEOMETRY_PASSES: usize = 4;
    for _ in 0..MAX_GEOMETRY_PASSES {
        let tabs_changed = resolve_materialized_line_tab_advances(items, font_system, block_style);
        let ruby_changed = resolve_materialized_ruby_overhang(items, font_system, block_style);
        if !tabs_changed && !ruby_changed {
            break;
        }
    }
}

/// Whether a used inline advance changed enough to require another geometry
/// pass. `NaN` remains non-convergent, matching the former direct comparison.
fn materialized_inline_geometry_changed(previous: f32, current: f32) -> bool {
    match (previous - current).abs().partial_cmp(&0.01) {
        Some(std::cmp::Ordering::Less) => false,
        Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater) | None => true,
    }
}

/// Resolve ruby annotation overlap against the selected parent inline line.
///
/// Ruby source atoms are conservatively sized to their widest annotation for
/// graph fitting. Once CSS Text has selected, trimmed, and tab-resolved a
/// line, the annotation can borrow the permitted adjacent inline space and
/// expose its smaller normal-flow base-column span. This pass owns no source
/// text and clones only the selected atom, so another candidate line cannot
/// inherit a placement from this one.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-overhang>
fn resolve_materialized_ruby_overhang(
    items: &mut [MeasuredInlineItem],
    font_system: &mut FontSystem,
    block_style: &ComputedStyle,
) -> bool {
    let mut geometry_changed = false;
    for item_index in 0..items.len() {
        let Some(placement) =
            resolved_ruby_placement_for_line_item(items, item_index, font_system, block_style)
        else {
            continue;
        };
        let flow_span = placement.flow_inline_span.points();
        let InlineLineItem::Atom(atom) = &items[item_index].item else {
            continue;
        };
        let mut atom = atom.clone().with_ruby_placement(placement);
        match block_style.writing_mode {
            WritingMode::HorizontalTb => atom.size.width = flow_span,
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => atom.size.height = flow_span,
        }
        geometry_changed |= materialized_inline_geometry_changed(
            items[item_index].base_advance().points(),
            flow_span,
        );
        items[item_index].advance.replace_base_points(flow_span);
        items[item_index].item = InlineLineItem::Atom(atom);
    }
    geometry_changed
}

fn resolved_ruby_placement_for_line_item(
    items: &[MeasuredInlineItem],
    item_index: usize,
    font_system: &mut FontSystem,
    block_style: &ComputedStyle,
) -> Option<ruby::ResolvedRubyPlacement> {
    let InlineLineItem::Atom(atom) = &items.get(item_index)?.item else {
        return None;
    };
    let InlineAtomContent::Ruby {
        base, annotations, ..
    } = atom.content()
    else {
        return None;
    };
    let column_span = base.containing_inline_size.points();
    let spaces = ruby_adjacent_space_allowance(items, item_index, block_style);
    let mut levels = Vec::with_capacity(annotations.len());
    let mut maximum_start = 0.0_f32;
    let mut maximum_end = 0.0_f32;
    for annotation in annotations {
        let available_span = annotation.containing_inline_size.points();
        let paint_span = annotation.paint_inline_size.points();
        let (alignment_offset, overhang) =
            ruby_alignment_geometry(annotation.style.ruby_align, available_span, paint_span);
        let policy_allowance = match annotation.overhang_policy {
            css::RubyOverhang::Spaces => spaces,
            css::RubyOverhang::Auto => ruby_auto_overhang_allowance(
                items,
                item_index,
                annotation.style.as_ref(),
                font_system,
            ),
        };
        let resolved_overhang = resolve_ruby_overhang(overhang, policy_allowance);
        maximum_start = maximum_start.max(resolved_overhang.unborrowed.inline_start.points());
        maximum_end = maximum_end.max(resolved_overhang.unborrowed.inline_end.points());
        levels.push((alignment_offset, overhang, resolved_overhang));
    }
    Some(ruby::ResolvedRubyPlacement {
        flow_inline_span: ruby::RubyColumnInlineSpan::new(
            column_span + maximum_start + maximum_end,
        ),
        base_inline_offset: ruby::RubyInlineDisplacement::new(maximum_start),
        annotation_inline_offsets: levels
            .iter()
            .map(|(alignment_offset, overhang, _)| {
                debug_assert!(
                    (*alignment_offset + overhang.inline_start.points()).abs() < 0.01
                        || overhang.inline_start.points() == 0.0
                );
                // `alignment_offset` includes the negative start overhang;
                // the common flow start retains only unborrowed excess.
                ruby::RubyInlineDisplacement::new(maximum_start + *alignment_offset)
            })
            .collect(),
        overhang: levels
            .into_iter()
            .map(|(_, _, resolved)| resolved)
            .collect(),
    })
}

/// Consume the selected line's independent start and end offers. Any excess
/// that cannot be borrowed stays in the ruby atom's normal-flow span.
fn resolve_ruby_overhang(
    overhang: ruby::RubyAlignedOverhang,
    allowance: ruby::RubyOverhangAllowance,
) -> ruby::ResolvedRubyOverhang {
    let borrowed_start = overhang
        .inline_start
        .points()
        .min(allowance.inline_start.points());
    let borrowed_end = overhang
        .inline_end
        .points()
        .min(allowance.inline_end.points());
    ruby::ResolvedRubyOverhang {
        borrowed: ruby::RubyOverhangAllowance {
            inline_start: ruby::RubyInlineSpan::new(borrowed_start),
            inline_end: ruby::RubyInlineSpan::new(borrowed_end),
        },
        unborrowed: ruby::RubyAlignedOverhang {
            inline_start: ruby::RubyInlineSpan::new(
                (overhang.inline_start.points() - borrowed_start).max(0.0),
            ),
            inline_end: ruby::RubyInlineSpan::new(
                (overhang.inline_end.points() - borrowed_end).max(0.0),
            ),
        },
    }
}

fn ruby_alignment_geometry(
    align: css::RubyAlign,
    available_span: f32,
    paint_span: f32,
) -> (f32, ruby::RubyAlignedOverhang) {
    if paint_span <= available_span {
        let offset = match align {
            css::RubyAlign::Start => 0.0,
            css::RubyAlign::Center | css::RubyAlign::SpaceBetween | css::RubyAlign::SpaceAround => {
                (available_span - paint_span) / 2.0
            }
        };
        return (offset, ruby::RubyAlignedOverhang::default());
    }
    let excess = paint_span - available_span;
    let start = match align {
        css::RubyAlign::Start => 0.0,
        css::RubyAlign::Center | css::RubyAlign::SpaceBetween | css::RubyAlign::SpaceAround => {
            excess / 2.0
        }
    };
    (
        -start,
        ruby::RubyAlignedOverhang {
            inline_start: ruby::RubyInlineSpan::new(start),
            inline_end: ruby::RubyInlineSpan::new(excess - start),
        },
    )
}

fn ruby_adjacent_space_allowance(
    items: &[MeasuredInlineItem],
    item_index: usize,
    block_style: &ComputedStyle,
) -> ruby::RubyOverhangAllowance {
    ruby::RubyOverhangAllowance {
        inline_start: ruby::RubyInlineSpan::new(
            item_index
                .checked_sub(1)
                .and_then(|index| ruby_inline_end_space_offer(&items[index], block_style))
                .unwrap_or(0.0),
        ),
        inline_end: ruby::RubyInlineSpan::new(
            items
                .get(item_index + 1)
                .and_then(|item| ruby_inline_start_space_offer(item, block_style))
                .unwrap_or(0.0),
        ),
    }
}

fn ruby_auto_overhang_allowance(
    items: &[MeasuredInlineItem],
    item_index: usize,
    style: &ComputedStyle,
    font_system: &mut FontSystem,
) -> ruby::RubyOverhangAllowance {
    let maximum = font_system.ic_advance_for_style(style).points() / 2.0;
    let neighbor_width = |index: Option<usize>| {
        index
            .and_then(|index| items.get(index))
            .filter(|item| !matches!(item.item, InlineLineItem::Atom(_)))
            .map_or(0.0, |item| {
                ruby_auto_overhang_offer(item.base_advance().points(), maximum)
            })
    };
    ruby::RubyOverhangAllowance {
        inline_start: ruby::RubyInlineSpan::new(neighbor_width(item_index.checked_sub(1))),
        inline_end: ruby::RubyInlineSpan::new(neighbor_width(
            (item_index + 1 < items.len()).then_some(item_index + 1),
        )),
    }
}

/// Quire's deterministic `auto` policy: never borrow more than half an `ic`
/// from either immediate visual neighbor.
fn ruby_auto_overhang_offer(neighbor_inline_span: f32, half_ic: f32) -> f32 {
    neighbor_inline_span.max(0.0).min(half_ic.max(0.0))
}

fn ruby_inline_end_space_offer(
    item: &MeasuredInlineItem,
    block_style: &ComputedStyle,
) -> Option<f32> {
    ruby_inline_adjacent_space_offer(item, block_style, true)
}

fn ruby_inline_start_space_offer(
    item: &MeasuredInlineItem,
    block_style: &ComputedStyle,
) -> Option<f32> {
    ruby_inline_adjacent_space_offer(item, block_style, false)
}

fn ruby_inline_adjacent_space_offer(
    item: &MeasuredInlineItem,
    block_style: &ComputedStyle,
    at_end: bool,
) -> Option<f32> {
    let InlineLineItem::Fragment(fragment) = &item.item else {
        return None;
    };
    let text = fragment.text();
    let vertical = matches!(
        block_style.text_layout_policy(),
        css::TextLayoutPolicy::Vertical(_)
    );
    let boundary = if at_end {
        text.char_indices().next_back()
    } else {
        text.char_indices().next()
    }?;
    let (offset, character) = boundary;
    let character_end = offset + character.len_utf8();
    let punctuation = crate::text::text_spacing_punctuation_class(
        character,
        fragment.style().language.as_deref(),
        vertical,
    );
    let punctuation_share = ruby_punctuation_overhang_share(
        at_end,
        punctuation,
        fragment.style().text_spacing_trim.resolved(),
    );
    if let Some(share) = punctuation_share {
        return ruby_fragment_source_range_width(item, offset..character_end)
            .map(|width| width * share);
    }
    let is_eligible_space =
        |character: char| ruby_overhang_space_is_eligible(character, fragment.style());
    if !is_eligible_space(character) {
        return None;
    }
    let range = if at_end {
        let start = text
            .char_indices()
            .rev()
            .take_while(|(_, character)| is_eligible_space(*character))
            .last()
            .map_or(text.len(), |(offset, _)| offset);
        start..text.len()
    } else {
        let end = text
            .char_indices()
            .take_while(|(_, character)| is_eligible_space(*character))
            .last()
            .map_or(0, |(offset, character)| offset + character.len_utf8());
        0..end
    };
    ruby_fragment_source_range_width(item, range)
}

fn ruby_punctuation_overhang_share(
    at_end: bool,
    punctuation: Option<crate::text::TextSpacingPunctuationClass>,
    text_spacing_trim: TextSpacingTrim,
) -> Option<f32> {
    if text_spacing_trim != TextSpacingTrim::SpaceAll {
        return None;
    }
    match (at_end, punctuation) {
        (true, Some(crate::text::TextSpacingPunctuationClass::Closing))
        | (false, Some(crate::text::TextSpacingPunctuationClass::Opening)) => Some(0.5),
        (_, Some(crate::text::TextSpacingPunctuationClass::MiddleDot)) => Some(0.25),
        _ => None,
    }
}

/// The `spaces` policy considers preserved document spaces/tabs, U+00A0, and
/// Unicode General_Category `Zs` characters. It does not treat arbitrary
/// control whitespace as borrowable inline space.
fn ruby_overhang_space_is_eligible(character: char, style: &ComputedStyle) -> bool {
    (matches!(character, ' ' | '\t') && !style.white_space.collapses_spaces())
        || matches!(character, '\u{00a0}')
        || crate::text::character_is_css_other_space_separator(character)
}

fn ruby_fragment_source_range_width(
    item: &MeasuredInlineItem,
    range: std::ops::Range<usize>,
) -> Option<f32> {
    let InlineLineItem::Fragment(fragment) = &item.item else {
        return None;
    };
    let shaped = item.shaped.as_deref()?;
    if range.start == 0 && range.end == fragment.text().len() {
        return Some(item.base_advance().points());
    }
    let range_width = shaped.source_range_advance_width(range.clone())?;
    // Tab expansion has already updated the complete fragment's used width.
    // Derive a leading/trailing tab run from the complementary shaped range so
    // it receives the actual selected tab-stop advance.
    if range.start == 0 {
        let remainder = shaped
            .source_range_advance_width(range.end..fragment.text().len())
            .unwrap_or(0.0);
        Some((item.base_advance().points() - remainder).max(0.0))
    } else if range.end == fragment.text().len() {
        let prefix = shaped
            .source_range_advance_width(0..range.start)
            .unwrap_or(0.0);
        Some((item.base_advance().points() - prefix).max(0.0))
    } else {
        Some(range_width)
    }
}

/// Resolve preserved tabs into the selected line's fragment measurements.
///
/// A tab stop depends on the preceding *used* inline cursor, so keeping its
/// advance only as an aggregate line-width correction leaves the tab's own
/// background, decoration, and initial-letter pseudo geometry at zero width.
/// Re-slice the complete selected text group instead: every fragment then
/// owns the same advance that line fitting and paint use.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
fn resolve_materialized_line_tab_advances(
    items: &mut [MeasuredInlineItem],
    font_system: &mut FontSystem,
    tab_metric_style: &ComputedStyle,
) -> bool {
    let mut cursor = 0.0;
    let mut index = 0;
    let mut geometry_changed = false;
    while index < items.len() {
        let InlineLineItem::Fragment(_) = &items[index].item else {
            cursor += items[index].used_advance().points();
            index += 1;
            continue;
        };
        let start = index;
        let mut has_tab = false;
        while let Some(item) = items.get(index) {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                break;
            };
            has_tab |= fragment.text().contains('\t');
            index += 1;
        }
        debug_assert!(index > start);
        if !has_tab {
            cursor += items[start..index]
                .iter()
                .map(|item| item.used_advance().points())
                .sum::<f32>();
            continue;
        }
        // Tracking is represented as a boundary advance on the following
        // typographic unit. Feed that advance into the cursor *before*
        // resolving the fragment's tab, rather than reshaping the complete
        // untracked group and appending all spacing afterwards. This keeps
        // tab selection, graph fitting, and paint on the same used cursor.
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
        for item in &mut items[start..index] {
            cursor += item.advance.boundary_before().points();
            let InlineLineItem::Fragment(fragment) = &item.item else {
                unreachable!("a contiguous text group only contains fragments");
            };
            let width = if fragment.text().contains('\t') {
                let span = [StyledTextSpan {
                    text: fragment.text(),
                    style: fragment.style(),
                }];
                if let Some(shaped) = font_system.shape_untracked_styled_inline_fragments(
                    &span,
                    fragment.text().to_owned(),
                    0.0,
                    fragment.style().line_height,
                    cursor,
                    tab_metric_style,
                ) {
                    let width = shaped.advance_width();
                    geometry_changed |=
                        materialized_inline_geometry_changed(item.base_advance().points(), width);
                    item.advance.replace_base_points(width);
                    item.shaped = Some(Rc::new(shaped));
                    width
                } else {
                    item.base_advance().points()
                }
            } else {
                item.base_advance().points()
            };
            cursor += width;
        }
    }
    geometry_changed
}

pub(in crate::layout) fn opportunity_is_soft_wrap(opportunity: InlineBreakOpportunity) -> bool {
    !matches!(
        opportunity.kind,
        InlineBreakKind::Forced | InlineBreakKind::FloatPlacement
    )
}

/// Return the selected Phase II trim advance without deleting source items.
///
/// Regular inline box edges are transparent to CSS Text line-edge processing,
/// but remain in the item sequence for their painting ownership.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
pub(in crate::layout) fn trailing_collapsible_measured_width(items: &[MeasuredInlineItem]) -> f32 {
    let mut trimmed_width = 0.0;
    for item in items.iter().rev() {
        match &item.item {
            // Inline box edges decorate the same inline stream but do not
            // give an otherwise-empty tail textual content. Phase II trims
            // collapsed spaces through nested inline boxes, retaining their
            // borders/padding while removing the space advances on either
            // side of those edges.
            // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
            InlineLineItem::Atom(atom)
                if matches!(
                    atom.content(),
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                ) => {}
            InlineLineItem::Fragment(fragment)
                if fragment.style().white_space.collapses_spaces()
                    && fragment.text().chars().all(is_css_collapsible_whitespace) =>
            {
                trimmed_width += item.used_advance().points();
            }
            _ => break,
        }
    }
    trimmed_width
}

pub(in crate::layout) fn trailing_collapsible_run_width(runs: &[InlineParagraphRun]) -> f32 {
    let mut trimmed_width = 0.0;
    for run in runs.iter().rev() {
        match &run.item {
            InlineLineItem::Atom(atom)
                if matches!(
                    atom.content(),
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                ) => {}
            InlineLineItem::Fragment(fragment)
                if fragment.style().white_space.collapses_spaces()
                    && fragment.text().chars().all(is_css_collapsible_whitespace) =>
            {
                trimmed_width += run.width;
            }
            _ => break,
        }
    }
    trimmed_width
}

/// Return conditional `pre-wrap` hanging advance at a selected line edge,
/// including a preserved-space run immediately before a trailing sequence of
/// unconditionally hanging Unicode space separators.
///
/// Phase II first identifies the complete visual line-end whitespace sequence:
/// a `pre-wrap` run before U+3000 (or another other-space separator) is still
/// at line end and therefore hangs with it.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
pub(in crate::layout) fn trailing_pre_wrap_hanging_width_with_unconditional_separators<T>(
    items: &[T],
    font_system: &mut FontSystem,
) -> f32
where
    T: AsRef<InlineLineItem>,
{
    let mut width = 0.0;
    for item in items.iter().rev() {
        let InlineLineItem::Fragment(fragment) = item.as_ref() else {
            if matches!(
                item.as_ref(),
                InlineLineItem::Atom(atom)
                    if matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_)))
            ) {
                continue;
            }
            break;
        };
        for character in fragment.text().chars().rev() {
            // Collapsed terminal document whitespace is removed before the
            // unconditional other-space-separator rule. Continue through it
            // so a preceding U+3000 (or another other separator) is still
            // recognized as the visual line edge.
            // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
            if fragment.style().white_space.collapses_spaces()
                && is_css_collapsible_whitespace(character)
            {
                continue;
            }
            if fragment.style().white_space == WhiteSpace::PreWrap
                && is_css_preserved_document_space(character)
            {
                width += font_system.measure_text(&character.to_string(), fragment.style());
                continue;
            }
            if character_is_css_other_space_separator(character)
                && fragment
                    .style()
                    .white_space
                    .hangs_trailing_space_separators()
            {
                continue;
            }
            return width;
        }
    }
    width
}

/// Collect the selected source ranges that own Phase II end-edge behavior.
///
/// The width helpers intentionally remain geometry-only, but painting cannot
/// infer source ownership from a scalar advance when spaces cross inline
/// boxes. Record the selected fragment ranges in the same reverse visual-edge
/// traversal used by CSS Text's Phase II whitespace rules.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
fn selected_line_edge_source_effects(
    items: &[MeasuredInlineItem],
    has_collapsed_trim: bool,
    has_pre_wrap_hang: bool,
    has_unconditional_separator_hang: bool,
) -> Rc<[InlineLineEdgeEffect]> {
    let mut effects = Vec::new();

    if has_collapsed_trim {
        for (item_index, item) in items.iter().enumerate().rev() {
            match &item.item {
                InlineLineItem::Atom(atom)
                    if matches!(
                        atom.content(),
                        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                    ) => {}
                InlineLineItem::Fragment(fragment)
                    if fragment.style().white_space.collapses_spaces()
                        && fragment.text().chars().all(is_css_collapsible_whitespace) =>
                {
                    effects.push(InlineLineEdgeEffect {
                        kind: InlineLineEdgeEffectKind::CollapsedEndTrim,
                        item_index,
                        source_range: 0..fragment.text().len(),
                    });
                }
                _ => break,
            }
        }
    }

    if has_pre_wrap_hang {
        for (item_index, item) in items.iter().enumerate().rev() {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                if matches!(
                    &item.item,
                    InlineLineItem::Atom(atom)
                        if matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_)))
                ) {
                    continue;
                }
                break;
            };
            let mut start = fragment.text().len();
            let mut saw_pre_wrap_space = false;
            for (offset, character) in fragment.text().char_indices().rev() {
                if fragment.style().white_space.collapses_spaces()
                    && is_css_collapsible_whitespace(character)
                {
                    continue;
                }
                if fragment.style().white_space == WhiteSpace::PreWrap
                    && is_css_preserved_document_space(character)
                {
                    start = offset;
                    saw_pre_wrap_space = true;
                    continue;
                }
                if character_is_css_other_space_separator(character)
                    && fragment
                        .style()
                        .white_space
                        .hangs_trailing_space_separators()
                {
                    continue;
                }
                break;
            }
            if saw_pre_wrap_space {
                effects.push(InlineLineEdgeEffect {
                    kind: InlineLineEdgeEffectKind::PreWrapHang,
                    item_index,
                    source_range: start..fragment.text().len(),
                });
                continue;
            }
            if fragment
                .text()
                .chars()
                .all(character_is_css_other_space_separator)
                && fragment
                    .style()
                    .white_space
                    .hangs_trailing_space_separators()
            {
                continue;
            }
            break;
        }
    }

    if has_unconditional_separator_hang {
        let mut follows_hanging_separator = false;
        for (item_index, item) in items.iter().enumerate().rev() {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                if matches!(
                    &item.item,
                    InlineLineItem::Atom(atom)
                        if matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_)))
                ) {
                    continue;
                }
                break;
            };
            let mut start = fragment.text().len();
            let mut saw_separator = false;
            for (offset, character) in fragment.text().char_indices().rev() {
                if fragment.style().white_space.collapses_spaces()
                    && is_css_collapsible_whitespace(character)
                    && !follows_hanging_separator
                {
                    continue;
                }
                if character_is_css_other_space_separator(character)
                    && fragment
                        .style()
                        .white_space
                        .hangs_trailing_space_separators()
                {
                    start = offset;
                    saw_separator = true;
                    follows_hanging_separator = true;
                    continue;
                }
                if fragment.style().white_space != WhiteSpace::PreWrap
                    && fragment
                        .style()
                        .white_space
                        .hangs_trailing_space_separators()
                    && follows_hanging_separator
                    && is_css_preserved_document_space(character)
                {
                    start = offset;
                    continue;
                }
                break;
            }
            if saw_separator {
                effects.push(InlineLineEdgeEffect {
                    kind: InlineLineEdgeEffectKind::UnconditionalSeparatorHang,
                    item_index,
                    source_range: start..fragment.text().len(),
                });
                continue;
            }
            if fragment.style().white_space != WhiteSpace::PreWrap
                && fragment
                    .style()
                    .white_space
                    .hangs_trailing_space_separators()
                && follows_hanging_separator
                && fragment.text().chars().all(is_css_preserved_document_space)
            {
                effects.push(InlineLineEdgeEffect {
                    kind: InlineLineEdgeEffectKind::UnconditionalSeparatorHang,
                    item_index,
                    source_range: 0..fragment.text().len(),
                });
                continue;
            }
            break;
        }
    }

    effects.sort_by_key(|effect| effect.item_index);
    Rc::from(effects.into_boxed_slice())
}

pub(in crate::layout) fn normalize_materialized_control_characters(
    items: &mut Vec<MeasuredInlineItem>,
    visible_trailing_soft_hyphen: bool,
    font_system: &mut FontSystem,
) {
    let trailing_soft_hyphen_index = visible_trailing_soft_hyphen
        .then(|| {
            items.iter().rposition(|item| {
            matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
        })
        })
        .flatten();
    let mut index = 0;
    while index < items.len() {
        let mut remove = false;
        if let InlineLineItem::Fragment(fragment) = &mut items[index].item
            && fragment_text_needs_materialized_normalization(
                fragment.text(),
                Some(index) == trailing_soft_hyphen_index,
            )
            && let Some(text) = normalize_materialized_fragment_text(
                fragment.text(),
                Some(index) == trailing_soft_hyphen_index,
                false,
                used_discretionary_marker_text(fragment),
            )
        {
            fragment.set_text(text);
            remove = fragment.text().is_empty();
            if !remove {
                remeasure_materialized_item(&mut items[index], font_system);
            }
        }
        if remove {
            items.remove(index);
        } else {
            index += 1;
        }
    }
}

fn materialized_items_have_joining_behavior(items: &[MeasuredInlineItem]) -> bool {
    items.iter().any(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment)
        if fragment.text().chars().any(|character| {
            character_has_cursive_shaping_behavior(character)
                && !character_is_join_control(character)
        }))
    })
}

/// Add the trailing half of a selected joining boundary before the generated
/// marker. The marker remains a separate paint item, but the source-side ZWJ
/// keeps the combined paint shaping request faithful to the selected logical
/// edge sequence.
fn append_materialized_line_joiner(items: &mut [MeasuredInlineItem], font_system: &mut FontSystem) {
    let Some(item) = items.iter_mut().rev().find(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
    }) else {
        return;
    };
    let InlineLineItem::Fragment(fragment) = &mut item.item else {
        return;
    };
    let mut text = String::with_capacity(fragment.text().len() + '\u{200d}'.len_utf8());
    text.push_str(fragment.text());
    text.push('\u{200d}');
    fragment.set_text(text);
    remeasure_materialized_item(item, font_system);
}

/// Add the leading half of a selected soft-hyphen shaping boundary.
///
/// The source soft hyphen is removed during CSS Text's line-edge processing,
/// so the following physical line otherwise begins without the joining context
/// that the source word had. The control has no advance and is not added at
/// arbitrary visual-order boundaries.
fn prepend_materialized_line_joiner(
    items: &mut [MeasuredInlineItem],
    font_system: &mut FontSystem,
) {
    let Some(item) = items.iter_mut().find(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
    }) else {
        return;
    };
    let InlineLineItem::Fragment(fragment) = &mut item.item else {
        return;
    };
    let mut text = String::with_capacity(fragment.text().len() + '\u{200d}'.len_utf8());
    text.push('\u{200d}');
    text.push_str(fragment.text());
    fragment.set_text(text);
    remeasure_materialized_item(item, font_system);
}

pub(in crate::layout) fn fragment_text_needs_materialized_normalization(
    text: &str,
    visible_trailing_soft_hyphen: bool,
) -> bool {
    const SOFT_HYPHEN: char = '\u{00ad}';
    const ZERO_WIDTH_SPACE: char = '\u{200b}';
    text.contains(ZERO_WIDTH_SPACE)
        || text.contains(SOFT_HYPHEN)
        || (visible_trailing_soft_hyphen && text.ends_with(SOFT_HYPHEN))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{ContentLanguage, HyphenateCharacter, RubyAlign, WritingMode};

    fn bidi_scope_run(
        text: &str,
        style: ComputedStyle,
        source: InlineTextSource,
    ) -> InlineParagraphRun {
        InlineParagraphRun {
            item: InlineLineItem::Fragment(InlineFragment::new(
                text,
                style,
                0.0,
                None,
                true,
                source,
                false,
                InlineHangingEdges::default(),
                Vec::new(),
            )),
            width: 0.0,
            shaped: None,
        }
    }

    fn cloneable_box_edge_run(
        style: ComputedStyle,
        logical_edge: InlineLogicalEdge,
        positioning_containing_block_id: usize,
    ) -> InlineParagraphRun {
        let physical_side = match logical_edge {
            InlineLogicalEdge::Start => PhysicalSide::Left,
            InlineLogicalEdge::End => PhysicalSide::Right,
        };
        InlineParagraphRun {
            item: InlineLineItem::Atom(InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                    logical_edge,
                    physical_side,
                    positioning_containing_block_id: Some(InlinePositioningContainingBlockId(
                        positioning_containing_block_id,
                    )),
                    advance: 7.0,
                    paint_extent: 5.0,
                })),
                style.clone(),
                None,
                InlineSize::new(7.0, style.line_height),
                11.0,
                3.0,
                Some(format!("scope-{positioning_containing_block_id}")),
                None,
            )),
            width: 7.0,
            shaped: None,
        }
    }

    #[test]
    fn clone_continuations_nest_in_source_order_and_preserve_positioning_scope() {
        let mut outer = ComputedStyle::initial();
        outer.box_decoration_break = BoxDecorationBreak::Clone;
        outer.padding.left = 7.0;
        outer.padding.right = 7.0;
        let mut inner = outer.clone();
        inner.color = CssColor::new(10, 20, 30);
        let text = bidi_scope_run("x", outer.clone(), InlineTextSource::Normal);
        let graph = InlineOpportunityGraph::new(
            vec![
                cloneable_box_edge_run(outer.clone(), InlineLogicalEdge::Start, 1),
                cloneable_box_edge_run(inner.clone(), InlineLogicalEdge::Start, 2),
                text.clone(),
                cloneable_box_edge_run(inner, InlineLogicalEdge::End, 2),
                cloneable_box_edge_run(outer, InlineLogicalEdge::End, 1),
            ],
            Vec::new(),
        );
        let range = InlineGraphRange {
            start: InlineGraphPosition::at_run_start(2),
            end: InlineGraphPosition::at_run_start(3),
        };
        let mut items = vec![MeasuredInlineItem::new(text.item, 0.0, None)];

        graph.insert_clone_continuation_edges(range, &mut items);

        let edges = items
            .iter()
            .filter_map(|item| match &item.item {
                InlineLineItem::Atom(atom) => match atom.content() {
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) => Some((
                        edge.logical_edge,
                        edge.positioning_containing_block_id,
                        atom.link_target(),
                        atom.baseline_offset_from_alignment_source_block_start(
                            atom.size.height,
                            atom.style(),
                        )
                        .points(),
                        atom.baseline_shift,
                    )),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            edges,
            vec![
                (
                    InlineLogicalEdge::Start,
                    Some(InlinePositioningContainingBlockId(1)),
                    Some("scope-1"),
                    11.0,
                    3.0,
                ),
                (
                    InlineLogicalEdge::Start,
                    Some(InlinePositioningContainingBlockId(2)),
                    Some("scope-2"),
                    11.0,
                    3.0,
                ),
                (
                    InlineLogicalEdge::End,
                    Some(InlinePositioningContainingBlockId(2)),
                    Some("scope-2"),
                    11.0,
                    3.0,
                ),
                (
                    InlineLogicalEdge::End,
                    Some(InlinePositioningContainingBlockId(1)),
                    Some("scope-1"),
                    11.0,
                    3.0,
                ),
            ]
        );
        assert_eq!(items[0].base_advance().points(), 7.0);
        assert_eq!(items[1].base_advance().points(), 7.0);
        assert_eq!(items[3].base_advance().points(), 7.0);
        assert_eq!(items[4].base_advance().points(), 7.0);
    }

    #[test]
    fn clone_continuations_do_not_replay_positioning_markers_outside_bidi_controls() {
        let mut style = ComputedStyle::initial();
        style.box_decoration_break = BoxDecorationBreak::Clone;
        style.padding.left = 7.0;
        style.padding.right = 7.0;
        let mut positioning_marker =
            cloneable_box_edge_run(style.clone(), InlineLogicalEdge::Start, 9);
        let InlineLineItem::Atom(marker) = &mut positioning_marker.item else {
            unreachable!("test helper constructs an inline atom");
        };
        let marker_data = Rc::make_mut(&mut marker.data);
        let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = &mut marker_data.content
        else {
            unreachable!("test helper constructs an inline box edge");
        };
        edge.advance = 0.0;
        edge.paint_extent = 0.0;

        let text = bidi_scope_run("x", style.clone(), InlineTextSource::Normal);
        let graph = InlineOpportunityGraph::new(
            vec![
                cloneable_box_edge_run(style.clone(), InlineLogicalEdge::Start, 1),
                bidi_scope_run("\u{2066}", style.clone(), InlineTextSource::BidiControl),
                positioning_marker,
                text.clone(),
                bidi_scope_run("\u{2069}", style.clone(), InlineTextSource::BidiControl),
                cloneable_box_edge_run(style, InlineLogicalEdge::End, 1),
            ],
            Vec::new(),
        );
        let range = InlineGraphRange {
            start: InlineGraphPosition::at_run_start(3),
            end: InlineGraphPosition::at_run_start(4),
        };
        let mut items = vec![MeasuredInlineItem::new(text.item, 0.0, None)];

        graph.insert_clone_continuation_edges(range, &mut items);

        let continuation_ids = items
            .iter()
            .filter_map(|item| match &item.item {
                InlineLineItem::Atom(atom) => match atom.content() {
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) => {
                        Some(edge.positioning_containing_block_id)
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            continuation_ids,
            vec![
                Some(InlinePositioningContainingBlockId(1)),
                Some(InlinePositioningContainingBlockId(1))
            ]
        );
        assert_eq!(items.len(), 3, "the virtual bidi prefix owns its marker");
    }

    fn measured_text_spacing_item(
        text: &str,
        style: ComputedStyle,
        source: InlineTextSource,
        font_system: &mut FontSystem,
    ) -> MeasuredInlineItem {
        let fragment = InlineFragment::new(
            text,
            style,
            0.0,
            None,
            true,
            source,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut items = Vec::new();
        push_text_spacing_fragment(&mut items, &fragment, text, false, font_system);
        items.pop().expect("non-empty text produces one fragment")
    }

    fn has_font_feature(item: &MeasuredInlineItem, tag: [u8; 4]) -> bool {
        matches!(&item.item, InlineLineItem::Fragment(fragment)
        if fragment.style().font_feature_settings.0.iter().any(|setting| {
            setting.tag == tag && setting.value == 1
        }))
    }

    #[test]
    fn tab_resolution_leaves_adjacent_non_tab_fragments_unchanged() {
        let style = ComputedStyle::initial();
        let mut font_system = FontSystem::new();
        let mut items = vec![
            measured_text_spacing_item(
                "prefix",
                style.clone(),
                InlineTextSource::Normal,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "suffix",
                style.clone(),
                InlineTextSource::Normal,
                &mut font_system,
            ),
        ];
        let widths = items
            .iter()
            .map(|item| item.base_advance().points())
            .collect::<Vec<_>>();
        let shaped = items
            .iter()
            .map(|item| item.shaped.as_ref().map(Rc::clone))
            .collect::<Vec<_>>();

        assert!(!resolve_materialized_line_tab_advances(
            &mut items,
            &mut font_system,
            &style,
        ));
        assert_eq!(
            items
                .iter()
                .map(|item| item.base_advance().points())
                .collect::<Vec<_>>(),
            widths
        );
        for (item, original) in items.iter().zip(shaped) {
            assert_eq!(
                item.shaped.as_ref().map(Rc::as_ptr),
                original.as_ref().map(Rc::as_ptr),
                "a non-tab fragment must retain its graph shaping artifact"
            );
        }
    }

    #[test]
    fn stable_tab_geometry_does_not_force_extra_convergence_passes() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::Pre;
        let mut font_system = FontSystem::new();
        let mut items = vec![
            measured_text_spacing_item(
                "prefix",
                style.clone(),
                InlineTextSource::Normal,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "\tsuffix",
                style.clone(),
                InlineTextSource::Normal,
                &mut font_system,
            ),
        ];
        let widths = items
            .iter()
            .map(|item| item.base_advance().points())
            .collect::<Vec<_>>();

        assert!(resolve_materialized_line_tab_advances(
            &mut items,
            &mut font_system,
            &style,
        ));
        assert_ne!(
            items
                .iter()
                .map(|item| item.base_advance().points())
                .collect::<Vec<_>>(),
            widths,
            "the selected-line cursor changes the leading tab's advance"
        );
        assert!(!resolve_materialized_line_tab_advances(
            &mut items,
            &mut font_system,
            &style,
        ));
    }

    #[test]
    fn text_spacing_trim_eligibility_skips_ordinary_text() {
        let style = ComputedStyle::initial();
        let mut font_system = FontSystem::new();
        let ordinary = vec![measured_text_spacing_item(
            "ordinary ASCII text",
            style.clone(),
            InlineTextSource::Normal,
            &mut font_system,
        )];
        let punctuation = vec![measured_text_spacing_item(
            "、",
            style,
            InlineTextSource::Normal,
            &mut font_system,
        )];

        assert!(!materialized_items_may_use_text_spacing_trim(&ordinary));
        assert!(materialized_items_may_use_text_spacing_trim(&punctuation));
    }

    #[test]
    fn ruby_overhang_resolver_keeps_start_and_end_excess_independent() {
        let (offset, overhang) = ruby_alignment_geometry(RubyAlign::Center, 20.0, 60.0);
        assert_eq!(offset, -20.0);
        assert_eq!(overhang.inline_start.points(), 20.0);
        assert_eq!(overhang.inline_end.points(), 20.0);

        let resolved = resolve_ruby_overhang(
            overhang,
            ruby::RubyOverhangAllowance {
                inline_start: ruby::RubyInlineSpan::new(8.0),
                inline_end: ruby::RubyInlineSpan::new(30.0),
            },
        );
        assert_eq!(resolved.borrowed.inline_start.points(), 8.0);
        assert_eq!(resolved.borrowed.inline_end.points(), 20.0);
        assert_eq!(resolved.unborrowed.inline_start.points(), 12.0);
        assert_eq!(resolved.unborrowed.inline_end.points(), 0.0);
    }

    #[test]
    fn ruby_start_alignment_retains_all_excess_at_logical_end() {
        let (offset, overhang) = ruby_alignment_geometry(RubyAlign::Start, 20.0, 60.0);
        assert_eq!(offset, 0.0);
        assert_eq!(overhang.inline_start.points(), 0.0);
        assert_eq!(overhang.inline_end.points(), 40.0);
    }

    #[test]
    fn ruby_spaces_accepts_only_preserved_and_unicode_space_separators() {
        let collapsed = ComputedStyle::initial();
        assert!(!ruby_overhang_space_is_eligible(' ', &collapsed));
        assert!(!ruby_overhang_space_is_eligible('\t', &collapsed));
        assert!(ruby_overhang_space_is_eligible('\u{00a0}', &collapsed));
        assert!(ruby_overhang_space_is_eligible('\u{3000}', &collapsed));
        assert!(!ruby_overhang_space_is_eligible('\n', &collapsed));

        let mut preserved = ComputedStyle::initial();
        preserved.white_space = WhiteSpace::Pre;
        assert!(ruby_overhang_space_is_eligible(' ', &preserved));
        assert!(ruby_overhang_space_is_eligible('\t', &preserved));
    }

    #[test]
    fn ruby_spaces_punctuation_requires_an_untrimmed_boundary_side() {
        use crate::text::TextSpacingPunctuationClass;

        assert_eq!(
            ruby_punctuation_overhang_share(
                true,
                Some(TextSpacingPunctuationClass::Closing),
                TextSpacingTrim::SpaceAll,
            ),
            Some(0.5),
        );
        assert_eq!(
            ruby_punctuation_overhang_share(
                false,
                Some(TextSpacingPunctuationClass::Opening),
                TextSpacingTrim::SpaceAll,
            ),
            Some(0.5),
        );
        assert_eq!(
            ruby_punctuation_overhang_share(
                true,
                Some(TextSpacingPunctuationClass::MiddleDot),
                TextSpacingTrim::SpaceAll,
            ),
            Some(0.25),
        );
        assert_eq!(
            ruby_punctuation_overhang_share(
                true,
                Some(TextSpacingPunctuationClass::Closing),
                TextSpacingTrim::Normal,
            ),
            None,
        );
    }

    #[test]
    fn ruby_line_edges_and_auto_collision_cap_do_not_offer_extra_space() {
        let style = ComputedStyle::initial();
        assert_eq!(
            ruby_adjacent_space_allowance(&[], 0, &style),
            ruby::RubyOverhangAllowance::default(),
        );
        assert_eq!(ruby_auto_overhang_offer(40.0, 10.0), 10.0);
        assert_eq!(ruby_auto_overhang_offer(4.0, 10.0), 4.0);
        assert_eq!(ruby_auto_overhang_offer(-4.0, 10.0), 0.0);
    }

    #[test]
    fn ruby_overhang_geometry_is_logical_for_vertical_lines() {
        let horizontal = ruby_alignment_geometry(RubyAlign::Center, 20.0, 60.0);
        let mut vertical_style = ComputedStyle::initial();
        vertical_style.writing_mode = WritingMode::VerticalRl;
        // Ruby resolution operates in logical inline coordinates; the paint
        // adapter alone projects this same geometry to physical height.
        let vertical = ruby_alignment_geometry(RubyAlign::Center, 20.0, 60.0);
        assert_eq!(horizontal, vertical);
        assert_eq!(vertical_style.writing_mode, WritingMode::VerticalRl);
    }

    #[test]
    fn inside_marker_suffix_keeps_its_inline_advance_across_bidi_isolate_controls() {
        let style = ComputedStyle::initial();
        let mut font_system = FontSystem::new();
        let expected_marker_advance = font_system.measure_text("壱、", &style);
        let mut items = vec![
            measured_text_spacing_item(
                "\u{2068}",
                style.clone(),
                InlineTextSource::BidiControl,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "壱、",
                style.clone(),
                InlineTextSource::Marker,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "\u{2069}",
                style.clone(),
                InlineTextSource::BidiControl,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "壱、",
                style.clone(),
                InlineTextSource::Normal,
                &mut font_system,
            ),
        ];

        apply_materialized_text_spacing_trim(&mut items, &mut font_system, true, None);

        let marker = items
            .iter()
            .find(|item| {
                matches!(&item.item, InlineLineItem::Fragment(fragment)
                if fragment.source() == InlineTextSource::Marker)
            })
            .expect("inside automatic marker remains a selected text item");
        assert_eq!(marker.base_advance().points(), expected_marker_advance);
        assert!(
            !has_font_feature(marker, *b"halt"),
            "a marker suffix preceding ordinary inline content is not a line edge"
        );
    }

    #[test]
    fn marker_suffix_at_the_selected_line_end_uses_halt() {
        let style = ComputedStyle::initial();
        let mut font_system = FontSystem::new();
        let mut items = vec![measured_text_spacing_item(
            "壱、",
            style,
            InlineTextSource::Marker,
            &mut font_system,
        )];
        apply_materialized_text_spacing_trim(&mut items, &mut font_system, true, None);
        assert!(
            items.iter().any(|item| has_font_feature(item, *b"halt")),
            "visible marker punctuation at the actual selected-line end is trimmed"
        );
    }

    #[test]
    fn text_spacing_adjacency_crosses_marker_isolate_controls() {
        let style = ComputedStyle::initial();
        let mut font_system = FontSystem::new();
        let mut items = vec![
            measured_text_spacing_item(
                "、",
                style.clone(),
                InlineTextSource::Marker,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "\u{2069}",
                style.clone(),
                InlineTextSource::BidiControl,
                &mut font_system,
            ),
            measured_text_spacing_item("、", style, InlineTextSource::Normal, &mut font_system),
        ];

        apply_materialized_text_spacing_trim(&mut items, &mut font_system, true, None);

        let marker = items
            .iter()
            .find(|item| {
                matches!(&item.item, InlineLineItem::Fragment(fragment)
                if fragment.source() == InlineTextSource::Marker)
            })
            .expect("marker punctuation remains selected");
        assert!(
            has_font_feature(marker, *b"halt"),
            "the marker comma participates only because the following comma is adjacent text"
        );
    }

    #[test]
    fn vertical_marker_suffix_at_the_selected_line_end_uses_vhal() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        let mut font_system = FontSystem::new();
        let mut items = vec![measured_text_spacing_item(
            "壱、",
            style,
            InlineTextSource::Marker,
            &mut font_system,
        )];

        apply_materialized_text_spacing_trim(&mut items, &mut font_system, true, None);

        assert!(
            items.iter().any(|item| has_font_feature(item, *b"vhal")),
            "vertical selected-line marker punctuation uses the vertical alternate"
        );
    }

    #[test]
    fn break_availability_orders_fallbacks_and_min_content() {
        let ordinary = BreakAvailability::Ordinary;
        let keep_all = BreakAvailability::RelaxedWordBreak(WordBreakRelaxation::KeepAll);
        let phrase_wrap = BreakAvailability::RelaxedWordBreak(WordBreakRelaxation::AutoPhraseWrap);
        let phrase_hyphen =
            BreakAvailability::RelaxedWordBreak(WordBreakRelaxation::AutoPhraseHyphenation);
        let anywhere = BreakAvailability::OverflowWrap(OverflowWrapFallback::Anywhere);
        let break_word = BreakAvailability::OverflowWrap(OverflowWrapFallback::BreakWord);

        assert_eq!(ordinary.fitting_stage(), 0);
        assert_eq!(keep_all.fitting_stage(), 1);
        assert_eq!(phrase_wrap.fitting_stage(), 1);
        assert_eq!(phrase_hyphen.fitting_stage(), 2);
        assert_eq!(anywhere.fitting_stage(), 3);
        assert!(ordinary.participates_in_min_content());
        assert!(anywhere.participates_in_min_content());
        assert!(!keep_all.participates_in_min_content());
        assert!(!phrase_wrap.participates_in_min_content());
        assert!(!phrase_hyphen.participates_in_min_content());
        assert!(!break_word.participates_in_min_content());
    }

    #[test]
    fn automatic_hyphenation_is_not_offered_for_line_break_anywhere() {
        let mut ordinary = ComputedStyle::initial();
        ordinary.hyphens = Hyphens::Auto;
        ordinary.language = ContentLanguage::from_html_attribute("en");
        let ordinary_runs = vec![bidi_scope_run(
            "hyphenation",
            ordinary.clone(),
            InlineTextSource::Normal,
        )];
        assert!(
            !apply_auto_hyphenation_across_transparent_inline_edges(&ordinary_runs).is_empty(),
            "the fixture must have ordinary dictionary opportunities"
        );

        ordinary.line_break = css::LineBreak::Anywhere;
        let anywhere_runs = vec![bidi_scope_run(
            "hyphenation",
            ordinary,
            InlineTextSource::Normal,
        )];
        assert!(
            apply_auto_hyphenation_across_transparent_inline_edges(&anywhere_runs).is_empty(),
            "line-break:anywhere supplies its own soft opportunities without a used hyphen"
        );
    }

    #[test]
    fn auto_phrase_defers_automatic_hyphenation_opportunities() {
        let mut style = ComputedStyle::initial();
        style.hyphens = Hyphens::Auto;
        style.language = ContentLanguage::from_html_attribute("en");
        style.word_break = css::WordBreak::AutoPhrase;

        let opportunities =
            apply_auto_hyphenation_across_transparent_inline_edges(&[bidi_scope_run(
                "hyphenation",
                style,
                InlineTextSource::Normal,
            )]);

        assert!(opportunities.iter().any(|opportunity| {
            opportunity.kind == BreakEffect::Hyphenation
                && opportunity.availability
                    == BreakAvailability::RelaxedWordBreak(
                        WordBreakRelaxation::AutoPhraseHyphenation,
                    )
        }));
    }

    #[test]
    fn bidi_scope_continuations_balance_nested_css_scopes_without_author_controls() {
        let mut outer = ComputedStyle::initial();
        outer.unicode_bidi = UnicodeBidi::Isolate;
        outer.direction = Direction::Ltr;
        let mut inner = outer.clone();
        inner.direction = Direction::Rtl;
        let graph = InlineOpportunityGraph::new(
            vec![
                bidi_scope_run("\u{2066}", outer.clone(), InlineTextSource::BidiControl),
                bidi_scope_run("outer", outer.clone(), InlineTextSource::Normal),
                bidi_scope_run("\u{2067}", inner.clone(), InlineTextSource::BidiControl),
                bidi_scope_run("inner", inner.clone(), InlineTextSource::Normal),
                bidi_scope_run("\u{2069}", inner, InlineTextSource::BidiControl),
                bidi_scope_run("tail", outer.clone(), InlineTextSource::Normal),
                bidi_scope_run("\u{2069}", outer, InlineTextSource::BidiControl),
                // An authored FSI participates in the generic UAX #9 scope
                // stack, rather than using CSS `unicode-bidi` provenance.
                bidi_scope_run(
                    "\u{2068}",
                    ComputedStyle::initial(),
                    InlineTextSource::Normal,
                ),
            ],
            Vec::new(),
        );

        let middle = graph.bidi_scope_continuations_for_range(InlineGraphRange {
            start: InlineGraphPosition::at_run_start(3),
            end: InlineGraphPosition::at_run_start(4),
        });
        assert_eq!(middle.prefix, "\u{2066}\u{2067}");
        assert_eq!(middle.suffix, "\u{2069}\u{2069}");

        let after_inner = graph.bidi_scope_continuations_for_range(InlineGraphRange {
            start: InlineGraphPosition::at_run_start(5),
            end: InlineGraphPosition::at_run_start(6),
        });
        assert_eq!(after_inner.prefix, "\u{2066}");
        assert_eq!(after_inner.suffix, "\u{2069}");
    }

    #[test]
    fn bidi_scope_continuations_balance_authored_isolate_inside_one_graph_run() {
        let text = "a\u{2068}BC\u{2069}d";
        let graph = InlineOpportunityGraph::new(
            vec![bidi_scope_run(
                text,
                ComputedStyle::initial(),
                InlineTextSource::Normal,
            )],
            Vec::new(),
        );
        let isolate_content_start = text.find('B').expect("test isolate content");
        let isolate_content_end = isolate_content_start + 'B'.len_utf8();
        let continuations = graph.bidi_scope_continuations_for_range(InlineGraphRange {
            start: InlineGraphPosition {
                run_index: 0,
                byte_offset: isolate_content_start,
            },
            end: InlineGraphPosition {
                run_index: 0,
                byte_offset: isolate_content_end,
            },
        });

        assert_eq!(continuations.prefix_parent_context, "\u{200e}");
        assert_eq!(continuations.prefix, "\u{2068}");
        assert_eq!(continuations.suffix, "\u{2069}");
        assert_eq!(continuations.suffix_parent_context, "\u{200e}");
    }

    #[test]
    fn bidi_scope_continuations_replay_nested_authored_controls() {
        let text = "a\u{2068}b\u{202e}c\u{202c}d\u{2069}e";
        let graph = InlineOpportunityGraph::new(
            vec![bidi_scope_run(
                text,
                ComputedStyle::initial(),
                InlineTextSource::Normal,
            )],
            Vec::new(),
        );
        let selected_start = text.find('c').expect("test override content");
        let selected_end = selected_start + 'c'.len_utf8();
        let continuations = graph.bidi_scope_continuations_for_range(InlineGraphRange {
            start: InlineGraphPosition {
                run_index: 0,
                byte_offset: selected_start,
            },
            end: InlineGraphPosition {
                run_index: 0,
                byte_offset: selected_end,
            },
        });

        assert_eq!(continuations.prefix, "\u{2068}\u{202e}");
        assert_eq!(continuations.suffix, "\u{202c}\u{2069}");
    }

    fn wrap_inside_avoid_edge(logical_edge: InlineLogicalEdge) -> InlineParagraphRun {
        let mut style = ComputedStyle::initial();
        style.wrap_inside = css::WrapInside::Avoid;
        InlineParagraphRun {
            item: InlineLineItem::Atom(InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                    logical_edge,
                    physical_side: match logical_edge {
                        InlineLogicalEdge::Start => PhysicalSide::Left,
                        InlineLogicalEdge::End => PhysicalSide::Right,
                    },
                    positioning_containing_block_id: None,
                    advance: 0.0,
                    paint_extent: 0.0,
                })),
                style.clone(),
                None,
                InlineSize::new(0.0, style.line_height),
                style.font_size,
                0.0,
                None,
                None,
            )),
            width: 0.0,
            shaped: None,
        }
    }

    #[test]
    fn wrap_inside_avoid_depth_uses_lexical_inline_edges() {
        let style = ComputedStyle::initial();
        let text = |contents| InlineParagraphRun {
            item: InlineLineItem::Fragment(InlineFragment::new(
                contents,
                style.clone(),
                0.0,
                None,
                true,
                InlineTextSource::Normal,
                false,
                InlineHangingEdges::default(),
                Vec::new(),
            )),
            width: 0.0,
            shaped: None,
        };
        let graph = InlineOpportunityGraph::new(
            vec![
                wrap_inside_avoid_edge(InlineLogicalEdge::Start),
                wrap_inside_avoid_edge(InlineLogicalEdge::Start),
                text("x"),
                wrap_inside_avoid_edge(InlineLogicalEdge::End),
                wrap_inside_avoid_edge(InlineLogicalEdge::End),
            ],
            Vec::new(),
        );

        assert_eq!(
            graph.wrap_inside_avoid_depth(InlineGraphPosition::at_run_start(0)),
            0
        );
        assert_eq!(
            graph.wrap_inside_avoid_depth(InlineGraphPosition::at_run_start(2)),
            2
        );
        assert_eq!(
            graph.wrap_inside_avoid_depth(InlineGraphPosition {
                run_index: 2,
                byte_offset: 1,
            }),
            0,
            "the trailing margin edge is outside both nested boxes"
        );
    }

    #[test]
    fn graph_text_run_preserves_inline_word_style_handle() {
        let mut style = ComputedStyle::initial();
        style.font_size = 12.0;
        style.line_height = 14.0;
        let shared_style = inline_style(&style);
        let word = InlineWord {
            text: "Hello".to_string(),
            style: Rc::clone(&shared_style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        };
        let mut font_system = FontSystem::new();
        let mut runs = Vec::new();

        push_text_graph_run_segment(
            &mut font_system,
            &mut runs,
            &word,
            &word.text,
            InlineHangingEdges::default(),
            InlineTrackingScope::root(&style),
            Rc::new(()),
        );

        let InlineLineItem::Fragment(fragment) = &runs[0].item else {
            panic!("expected graph run fragment");
        };
        assert!(Rc::ptr_eq(&shared_style, &fragment.data.style));
    }

    #[test]
    fn transformed_separator_retains_shared_boundary_shape() {
        let style = ComputedStyle::initial();
        let run = |text, source| InlineParagraphRun {
            item: InlineLineItem::Fragment(InlineFragment::new(
                text,
                style.clone(),
                0.0,
                None,
                true,
                source,
                false,
                InlineHangingEdges::default(),
                Vec::new(),
            )),
            width: 0.0,
            shaped: None,
        };
        let mut runs = vec![
            run("あ", InlineTextSource::Normal),
            run(
                "\u{3000}",
                InlineTextSource::WordSpaceTransform(
                    ExplicitWordSeparatorSource::AuthoredZeroWidthSpace,
                ),
            ),
            run("い", InlineTextSource::Normal),
        ];
        let mut font_system = FontSystem::new();

        shape_logical_joining_graph_runs(&mut runs, &mut font_system, &style);

        let fragments = runs
            .iter()
            .map(|run| match &run.item {
                InlineLineItem::Fragment(fragment) => fragment,
                InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                    panic!("word-space-transform fixture contains only text fragments")
                }
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            fragments[1].source(),
            InlineTextSource::WordSpaceTransform(
                ExplicitWordSeparatorSource::AuthoredZeroWidthSpace
            )
        ));
        let source = fragments[0]
            .boundary_shaped_source()
            .expect("left text retains complete source shape");
        assert_eq!(source.shaped.text.as_ref(), "あ　い");
        assert!(fragments.iter().all(|fragment| {
            fragment
                .boundary_shaped_source()
                .is_some_and(|candidate| std::ptr::eq(source, candidate))
        }));
        assert_eq!(fragments[0].boundary_shaped_range(), Some(&(0.."あ".len())));
        assert_eq!(
            fragments[1].boundary_shaped_range(),
            Some(&("あ".len().."あ　".len()))
        );
        assert_eq!(
            fragments[2].boundary_shaped_range(),
            Some(&("あ　".len().."あ　い".len()))
        );
    }

    #[test]
    fn css_bidi_control_graph_run_has_no_shaped_advance() {
        let style = ComputedStyle::initial();
        let word = InlineWord {
            text: "\u{202a}".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::BidiControl,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        };
        let mut font_system = FontSystem::new();
        let mut runs = Vec::new();

        push_text_graph_run_segment(
            &mut font_system,
            &mut runs,
            &word,
            &word.text,
            InlineHangingEdges::default(),
            InlineTrackingScope::root(&style),
            Rc::new(()),
        );

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].width, 0.0);
        assert!(runs[0].shaped.is_none());
    }

    #[test]
    fn unconditional_hanging_separator_does_not_constrain_fitting() {
        let style = ComputedStyle::initial();
        let fragment = InlineFragment::new(
            "A\u{3000}",
            style.clone(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut font_system = FontSystem::new();
        let separator_width = font_system.measure_text("\u{3000}", &style);
        let widths = inline_content_width_for_line_items(
            &[MeasuredInlineItem::new(
                InlineLineItem::Fragment(fragment),
                40.0 + separator_width,
                None,
            )],
            &mut font_system,
            |item| item.used_advance().points(),
        );

        assert_eq!(widths.trailing_space_width, separator_width);
        assert_eq!(widths.fitting_width, 40.0);
        assert_eq!(widths.content_width, 40.0);
    }

    #[test]
    fn break_spaces_keeps_narrow_no_break_space_in_the_fitting_measure() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::BreakSpaces;
        let fragment = InlineFragment::new(
            "A\u{202f}",
            style.clone(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut font_system = FontSystem::new();
        let separator_width = font_system.measure_text("\u{202f}", &style);
        let total_width = 40.0 + separator_width;
        let widths = inline_content_width_for_line_items(
            &[MeasuredInlineItem::new(
                InlineLineItem::Fragment(fragment),
                total_width,
                None,
            )],
            &mut font_system,
            |item| item.used_advance().points(),
        );

        assert_eq!(widths.trailing_space_width, 0.0);
        assert_eq!(widths.fitting_width, total_width);
        assert_eq!(widths.content_width, total_width);
    }

    #[test]
    fn collapsed_terminal_space_exposes_hanging_separator_for_fitting() {
        let style = ComputedStyle::initial();
        let fragment = InlineFragment::new(
            "A\u{3000} ",
            style.clone(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut font_system = FontSystem::new();
        let separator_width = font_system.measure_text("\u{3000}", &style);
        let document_space_width = font_system.measure_text(" ", &style);
        let widths = inline_content_width_for_line_items(
            &[MeasuredInlineItem::new(
                InlineLineItem::Fragment(fragment),
                40.0 + separator_width + document_space_width,
                None,
            )],
            &mut font_system,
            |item| item.used_advance().points(),
        );

        assert_eq!(widths.trailing_space_width, separator_width);
        assert_eq!(widths.fitting_width, 40.0 + document_space_width,);
    }

    #[test]
    fn pre_hanging_sequence_includes_interleaved_document_spaces() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::Pre;
        let text = "A\u{3000} \u{2000}";
        let fragment = InlineFragment::new(
            text,
            style.clone(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut font_system = FontSystem::new();
        let hanging_width = font_system.measure_text("\u{3000} \u{2000}", &style);
        let widths = inline_content_width_for_line_items(
            &[MeasuredInlineItem::new(
                InlineLineItem::Fragment(fragment),
                40.0 + hanging_width,
                None,
            )],
            &mut font_system,
            |item| item.used_advance().points(),
        );

        assert_eq!(widths.trailing_space_width, hanging_width);
        assert_eq!(widths.fitting_width, 40.0);
    }

    #[test]
    fn automatic_marker_is_a_separate_selected_item_with_source_context() {
        let mut style = ComputedStyle::initial();
        style.language = ContentLanguage::from_html_attribute("ug");
        let source = InlineFragment::new(
            "دامي",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut items = vec![MeasuredInlineItem::new(
            InlineLineItem::Fragment(source),
            0.0,
            None,
        )];
        let graph_runs = vec![InlineParagraphRun {
            item: items[0].item.clone(),
            width: 0.0,
            shaped: None,
        }];
        let mut font_system = FontSystem::new();
        apply_selected_discretionary_break(
            &mut items,
            Some(DiscretionaryBreakEffect {
                source_boundary: InlineGraphPosition::at_run_start(0),
                marker_owner: DiscretionaryMarkerOwner {
                    style_position: InlineGraphPosition::at_run_start(0),
                },
                left_replacement: None,
                right_replacement: None,
                leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
            }),
            SelectedLineEdge::Trailing,
            &mut font_system,
            &graph_runs,
        );

        assert_eq!(items.len(), 2);
        let InlineLineItem::Fragment(source) = &items[0].item else {
            panic!("selected source remains a fragment");
        };
        let InlineLineItem::Fragment(marker) = &items[1].item else {
            panic!("selected marker is a fragment");
        };
        assert_eq!(source.text(), "دامي\u{200d}");
        assert_eq!(marker.text(), "\u{0640}");
        assert!(marker.is_selected_discretionary_marker());
    }

    #[test]
    fn authored_marker_uses_the_soft_hyphen_fragment_style() {
        let source_style = ComputedStyle::initial();
        let mut marker_style = ComputedStyle::initial();
        marker_style.hyphenate_character = HyphenateCharacter::String("=".into());
        let source = InlineFragment::new(
            "word",
            source_style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let soft_hyphen = InlineFragment::new(
            "\u{00ad}",
            marker_style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut items = vec![MeasuredInlineItem::new(
            InlineLineItem::Fragment(source.clone()),
            0.0,
            None,
        )];
        let graph_runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(source),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(soft_hyphen),
                width: 0.0,
                shaped: None,
            },
        ];
        let mut font_system = FontSystem::new();
        apply_selected_discretionary_break(
            &mut items,
            Some(DiscretionaryBreakEffect {
                source_boundary: InlineGraphPosition::at_run_start(1),
                marker_owner: DiscretionaryMarkerOwner {
                    style_position: InlineGraphPosition::at_run_start(1),
                },
                left_replacement: None,
                right_replacement: None,
                leading_shaping_context: SelectedLineShapingContext::None,
            }),
            SelectedLineEdge::Trailing,
            &mut font_system,
            &graph_runs,
        );

        let InlineLineItem::Fragment(marker) = &items[1].item else {
            panic!("selected marker is a fragment");
        };
        assert_eq!(marker.text(), "=");
        assert!(items[1].base_advance().points() > 0.0);
    }

    #[test]
    fn vertical_auto_marker_uses_the_vertical_hyphen() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        let fragment = InlineFragment::new(
            "word",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );

        assert_eq!(used_discretionary_marker_text(&fragment), "\u{2010}");
    }

    #[test]
    fn selected_vertical_soft_hyphen_normalization_uses_the_vertical_auto_marker() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        let fragment = InlineFragment::new(
            "word\u{00ad}",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut items = vec![MeasuredInlineItem::new(
            InlineLineItem::Fragment(fragment),
            0.0,
            None,
        )];

        normalize_materialized_control_characters(&mut items, true, &mut FontSystem::new());

        let InlineLineItem::Fragment(fragment) = &items[0].item else {
            panic!("selected source remains a text fragment");
        };
        assert_eq!(fragment.text(), "word\u{2010}");
    }

    #[test]
    fn selected_vertical_soft_hyphen_normalization_preserves_explicit_marker() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        style.hyphenate_character = HyphenateCharacter::String("+=".into());
        let fragment = InlineFragment::new(
            "word\u{00ad}",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut items = vec![MeasuredInlineItem::new(
            InlineLineItem::Fragment(fragment),
            0.0,
            None,
        )];

        normalize_materialized_control_characters(&mut items, true, &mut FontSystem::new());

        let InlineLineItem::Fragment(fragment) = &items[0].item else {
            panic!("selected source remains a text fragment");
        };
        assert_eq!(fragment.text(), "word+=");
    }

    #[test]
    fn frozen_float_replay_never_reuses_a_source_band_after_relocation() {
        let selected = InlineFloatReplay::RequeryContainingBlock {
            selected_float_page_index: 3,
        };
        assert!(!selected.reuses_selected_band_on(3));

        let frozen = selected.freeze_selected_band();
        assert!(frozen.reuses_selected_band_on(3));
        assert!(!frozen.reuses_selected_band_on(4));
        assert_eq!(frozen.selected_float_page_index(), 3);
    }

    #[test]
    fn pre_wrap_terminal_hanging_depends_on_the_selected_line_end() {
        assert_eq!(
            SelectedLineEndCondition::SoftWrap.pre_wrap_hanging_width(10.0, 20.0, Some(10.0)),
            10.0
        );
        assert_eq!(
            SelectedLineEndCondition::IntrinsicSegmentEnd.pre_wrap_hanging_width(10.0, 20.0, None),
            10.0
        );
        assert_eq!(
            SelectedLineEndCondition::ForcedBreak.pre_wrap_hanging_width(10.0, 20.0, Some(10.0)),
            10.0
        );
        assert_eq!(
            SelectedLineEndCondition::ParagraphEnd.pre_wrap_hanging_width(10.0, 15.0, Some(10.0)),
            5.0
        );
        assert_eq!(
            SelectedLineEndCondition::ForcedBreak.pre_wrap_hanging_width(10.0, 10.0, Some(10.0)),
            0.0
        );
        assert_eq!(
            SelectedLineEndCondition::ParagraphEnd.pre_wrap_hanging_width(10.0, 20.0, None),
            0.0
        );
    }

    #[test]
    fn first_letter_stream_keeps_prefix_punctuation_across_split_fragments() {
        let style = ComputedStyle::initial();
        let quote = bidi_scope_run("\u{201c}", style.clone(), InlineTextSource::Normal);
        let mut initial = quote.clone();
        let InlineLineItem::Fragment(fragment) = &mut initial.item else {
            panic!("test text run must be a fragment");
        };
        fragment.set_text(Rc::from("abc"));
        let graph = InlineOpportunityGraph::new(
            vec![
                quote,
                cloneable_box_edge_run(style.clone(), InlineLogicalEdge::Start, 1),
                initial,
                cloneable_box_edge_run(style, InlineLogicalEdge::End, 1),
            ],
            Vec::new(),
        );

        let selection = first_letter_stream_selection(&graph);
        assert_eq!(selection.len(), 2);
        assert_eq!(selection[0].run_index, 0);
        assert_eq!(selection[0].range, 0.."\u{201c}".len());
        assert_eq!(
            selection[0].role,
            FirstLetterPseudoFragmentRole::AssociatedPrefix
        );
        assert_eq!(selection[1].run_index, 2);
        assert_eq!(selection[1].range, 0..1);
        assert_eq!(
            selection[1].role,
            FirstLetterPseudoFragmentRole::TypographicInitial
        );
    }

    #[test]
    fn first_letter_stream_selects_a_generated_quote_before_author_text() {
        let style = ComputedStyle::initial();
        let graph = InlineOpportunityGraph::new(
            vec![
                bidi_scope_run("\u{201c}", style.clone(), InlineTextSource::Generated),
                bidi_scope_run("abc", style, InlineTextSource::Normal),
            ],
            Vec::new(),
        );

        let selection = first_letter_stream_selection(&graph);
        assert_eq!(selection.len(), 1);
        assert_eq!(selection[0].run_index, 0);
        assert_eq!(selection[0].range, 0.."\u{201c}".len());
        assert_eq!(
            selection[0].role,
            FirstLetterPseudoFragmentRole::AssociatedPrefix
        );
    }

    #[test]
    fn first_letter_stream_rejects_text_after_an_atomic_inline() {
        let style = ComputedStyle::initial();
        let atom = InlineAtom::new(
            InlineAtomContent::StaticPositionPlaceholder,
            style.clone(),
            None,
            InlineSize::new(0.0, 0.0),
            0.0,
            0.0,
            None,
            None,
        );
        let graph = InlineOpportunityGraph::new(
            vec![
                bidi_scope_run("\u{201c}", style.clone(), InlineTextSource::Normal),
                InlineParagraphRun {
                    item: InlineLineItem::Atom(atom),
                    width: 0.0,
                    shaped: None,
                },
                bidi_scope_run("abc", style, InlineTextSource::Normal),
            ],
            Vec::new(),
        );

        assert!(first_letter_stream_selection(&graph).is_empty());
    }

    #[test]
    fn floated_first_letter_group_becomes_one_marker_without_source_text() {
        let mut style = ComputedStyle::initial();
        style.float = Float::Left;
        let group_id = FirstLetterPseudoGroupId::allocate();
        let mut prefix = bidi_scope_run("\u{201c}", style.clone(), InlineTextSource::Generated);
        let mut initial = bidi_scope_run("A", style.clone(), InlineTextSource::Normal);
        for run in [&mut prefix, &mut initial] {
            let InlineLineItem::Fragment(fragment) = &mut run.item else {
                unreachable!("test run is text");
            };
            fragment.set_first_letter_pseudo_group_id(group_id);
        }
        let mut runs = vec![prefix, initial];

        materialize_first_letter_float(&mut runs, group_id, &style);

        assert_eq!(runs.len(), 1);
        let InlineLineItem::Float(float) = &runs[0].item else {
            panic!("first selected text becomes an inline float marker");
        };
        let fragments = float
            .first_letter_fragments()
            .expect("first-letter float keeps text payload");
        assert_eq!(fragments.len(), 2);
        assert_eq!(fragments[0].text(), "\u{201c}");
        assert_eq!(fragments[1].text(), "A");
        assert!(
            fragments
                .iter()
                .all(|fragment| fragment.style().float == Float::None)
        );
        assert!(
            fragments
                .iter()
                .all(|fragment| fragment.style().initial_letter.is_normal())
        );
        assert!(float.style().initial_letter.is_normal());
    }
}
