use super::super::mixed::apply_visual_tracking_boundaries;
use super::*;
use crate::css::Hyphens;
use crate::text::character_is_css_other_space_separator;
use crate::text::typographic_unit_ranges;
use crate::text::{
    DiscretionaryOpportunity, LanguageDiscretionaryReplacement,
    automatic_hyphenation_opportunities, hyphenator_for_language, manual_hyphenation_opportunities,
};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::rc::Rc;

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
    pub(in crate::layout) trailing_marker: bool,
    pub(in crate::layout) left_replacement: Option<InlineLineEdgeReplacement>,
    pub(in crate::layout) right_replacement: Option<InlineLineEdgeReplacement>,
    pub(in crate::layout) leading_shaping_context: SelectedLineShapingContext,
}

impl AsRef<InlineLineItem> for InlineParagraphRun {
    fn as_ref(&self) -> &InlineLineItem {
        &self.item
    }
}

/// One selected inline item with the measurement artifact used for line layout.
///
/// CSS Inline lays out a stream of text fragments and atomic inline boxes. Text
/// fragments are measured from shaped glyph advances, and carrying that
/// measurement beside the item prevents later paint preparation from reshaping
/// the same fragment only to recover its width:
/// <https://www.w3.org/TR/css-inline-3/#line-box> and
/// <https://www.w3.org/TR/css-text-3/#text-processing-order>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct MeasuredInlineItem {
    pub(in crate::layout) item: InlineLineItem,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) shaped: Option<Rc<ShapedInlineLine>>,
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
    pub(in crate::layout) trailing_tracking_width: f32,
    /// Source-owned Phase II effects, in selected item coordinates.
    pub(in crate::layout) source_effects: Rc<[InlineLineEdgeEffect]>,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct MaterializedInlineGraphLine {
    pub(in crate::layout) items: Vec<MeasuredInlineItem>,
    pub(in crate::layout) text: String,
    /// Advance used while choosing a break candidate. Once a candidate creates
    /// a line edge, CSS Text Phase II trimming and hanging apply to that edge
    /// before the candidate is compared with the available measure.
    pub(in crate::layout) fitting_width: f32,
    pub(in crate::layout) content_width: f32,
    pub(in crate::layout) edge_effects: InlineLineEdgeEffects,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct InlineContentWidth {
    /// Advance used to choose a selected line edge. Terminal tracking and
    /// unconditional hanging Unicode space separators are painted outside the
    /// formatted line and therefore do not make a candidate overflow.
    pub(in crate::layout) fitting_width: f32,
    pub(in crate::layout) content_width: f32,
    pub(in crate::layout) trailing_space_width: f32,
    pub(in crate::layout) trailing_tracking_width: f32,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct BorrowedInlineLineMeasurement {
    pub(in crate::layout) run_range: std::ops::Range<usize>,
    pub(in crate::layout) fitting_width: f32,
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
    let trailing_tracking_width = trailing_letter_spacing_width_for_line_items(items);
    InlineContentWidth {
        fitting_width: (raw_width - trailing_space_width - trailing_tracking_width).max(0.0),
        content_width: (raw_width - trailing_space_width - trailing_tracking_width).max(0.0),
        trailing_space_width,
        trailing_tracking_width,
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
        resolved_items.push(MeasuredInlineItem {
            item: InlineLineItem::Fragment(fragment),
            width,
            shaped,
        });
    }

    line.items = resolved_items;
    let widths = inline_content_width_for_line_items(&line.items, font_system, |item| item.width);
    line.edge_effects.pre_wrap_hanging_width = consumed_pre_wrap_width;
    line.edge_effects.hanging_space_separator_width = widths.trailing_space_width;
    line.edge_effects.trailing_tracking_width = widths.trailing_tracking_width;
    line.fitting_width = (widths.fitting_width - consumed_pre_wrap_width).max(0.0);
    line.content_width = (widths.content_width - consumed_pre_wrap_width).max(0.0);
    line.text = text_for_measured_items(&line.items);
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
pub(in crate::layout) enum InlineBreakKind {
    Forced,
    SoftWrap,
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
    pub(in crate::layout) kind: InlineBreakKind,
    pub(in crate::layout) priority: u8,
    pub(in crate::layout) trims: bool,
    pub(in crate::layout) hangs: bool,
    pub(in crate::layout) soft_hyphen: bool,
    /// Used behavior at this boundary.  `soft_hyphen` remains a compact
    /// classification for legacy priority/min-content policy; consumers that
    /// materialize the selected line must use this record instead.
    pub(in crate::layout) discretionary: Option<DiscretionaryBreakEffect>,
    pub(in crate::layout) emergency: bool,
    pub(in crate::layout) min_content: bool,
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

/// CSS `unicode-bidi` controls that must be virtually restored around a
/// selected soft-wrapped line.
///
/// The controls are UAX #9 input only: they are never measured, painted, or
/// exposed for extraction. They keep a CSS bidi scope intact while UAX #9
/// resolves one formatted line at a time.
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>
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
    /// See `prefix_parent_context`.
    pub(in crate::layout) suffix_parent_context: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CssBidiScope {
    start: &'static str,
    end: &'static str,
    is_isolate: bool,
}

impl InlineOpportunityGraph {
    /// Return virtual CSS bidi controls needed to balance one selected line.
    ///
    /// A CSS isolate is one U+FFFC-like object to its containing bidi
    /// paragraph, even when line breaking selects only a middle fragment of
    /// the isolate. Reopening the scopes active before the selected range and
    /// closing scopes still active after it gives UAX #9 that same scoped
    /// input without adding glyphs or source text to the line.
    pub(in crate::layout) fn bidi_scope_continuations_for_range(
        &self,
        range: InlineGraphRange,
    ) -> BidiLineScopeContinuations {
        let scopes_before_start = self.css_bidi_scopes_before(range.start);
        let scopes_before_end = self.css_bidi_scopes_before(range.end);
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
            if InlineGraphPosition::at_run_start(run_index) >= position {
                break;
            }
            self.update_css_bidi_scope_stack(&mut scopes, run);
            if scopes.is_empty()
                && let InlineLineItem::Fragment(fragment) = &run.item
                && fragment.source() != InlineTextSource::BidiControl
                && let Some(found) = plaintext_direction_for_text(fragment.text())
            {
                direction = Some(found);
            }
        }
        direction
    }

    fn parent_direction_after(&self, position: InlineGraphPosition) -> Option<Direction> {
        let mut scopes = self.css_bidi_scopes_before(position);
        for run in self.runs.iter().skip(position.run_index) {
            if scopes.is_empty()
                && let InlineLineItem::Fragment(fragment) = &run.item
                && fragment.source() != InlineTextSource::BidiControl
                && let Some(direction) = plaintext_direction_for_text(fragment.text())
            {
                return Some(direction);
            }
            self.update_css_bidi_scope_stack(&mut scopes, run);
        }
        None
    }

    fn css_bidi_scopes_before(&self, position: InlineGraphPosition) -> Vec<CssBidiScope> {
        let mut scopes = Vec::new();
        for (run_index, run) in self.runs.iter().enumerate() {
            if InlineGraphPosition::at_run_start(run_index) >= position {
                break;
            }
            let InlineLineItem::Fragment(fragment) = &run.item else {
                continue;
            };
            if fragment.source() != InlineTextSource::BidiControl {
                continue;
            }
            let Some((start, end)) = bidi_control_scope_for_style(fragment.style()) else {
                continue;
            };
            if fragment.text() == start {
                scopes.push(CssBidiScope {
                    start,
                    end,
                    is_isolate: matches!(
                        fragment.style().unicode_bidi,
                        UnicodeBidi::Isolate
                            | UnicodeBidi::IsolateOverride
                            | UnicodeBidi::Plaintext
                    ),
                });
            } else if fragment.text() == end {
                let scope = scopes.pop();
                debug_assert!(scope.as_ref().is_some_and(|scope| scope.end == end));
            }
        }
        scopes
    }

    fn update_css_bidi_scope_stack(
        &self,
        scopes: &mut Vec<CssBidiScope>,
        run: &InlineParagraphRun,
    ) {
        let InlineLineItem::Fragment(fragment) = &run.item else {
            return;
        };
        if fragment.source() != InlineTextSource::BidiControl {
            return;
        }
        let Some((start, end)) = bidi_control_scope_for_style(fragment.style()) else {
            return;
        };
        if fragment.text() == start {
            scopes.push(CssBidiScope {
                start,
                end,
                is_isolate: matches!(
                    fragment.style().unicode_bidi,
                    UnicodeBidi::Isolate | UnicodeBidi::IsolateOverride | UnicodeBidi::Plaintext
                ),
            });
        } else if fragment.text() == end {
            let scope = scopes.pop();
            debug_assert!(scope.as_ref().is_some_and(|scope| scope.end == end));
        }
    }
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
    /// This is distinct from the physical extent used by legacy intrinsic
    /// adapters: table-cell `align-content` aligns the line box on the cell's
    /// logical block axis, which is physical width in vertical writing.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
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
                .filter_map(|record| record.fragment.as_ref())
                .map(|fragment| fragment.metrics.width)
                .fold(0.0, f32::max),
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
    /// Fragmentainer identity of the exclusion band used while selecting this
    /// line. A collected line reuses that band on this page instead of
    /// querying a context that later selection has already mutated.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-layout>
    pub(in crate::layout) selected_float_page_index: usize,
    pub(in crate::layout) suppress_float_adjust: bool,
    pub(in crate::layout) edge_effects: InlineLineEdgeEffects,
    pub(in crate::layout) bidi_scope_continuations: BidiLineScopeContinuations,
    pub(in crate::layout) text: Rc<str>,
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
        suppress_float_adjust: bool,
        text: impl Into<String>,
    ) -> Self {
        Self {
            items: Rc::from(items.into_boxed_slice()),
            metrics,
            hanging_widths,
            indent,
            source_paint_indent: None,
            available_width,
            selected_float_page_index,
            suppress_float_adjust,
            edge_effects: InlineLineEdgeEffects::default(),
            bidi_scope_continuations: BidiLineScopeContinuations::default(),
            text: Rc::from(text.into()),
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
        let mut runs = Vec::with_capacity(graph.runs.len() + 2);
        let mut applied = false;
        for run in &graph.runs {
            let InlineLineItem::Fragment(fragment) = &run.item else {
                runs.push(run.clone());
                continue;
            };
            if applied {
                runs.push(run.clone());
                continue;
            }
            let Some(range) = first_letter_byte_range(fragment.text()) else {
                runs.push(run.clone());
                continue;
            };
            mark_leading_preserved_whitespace_as_first_letter_pseudo(&mut runs, first_letter_style);
            applied = true;
            for fragment in split_fragment_for_first_letter_in_graph(
                fragment,
                range,
                first_letter_style,
                block_style,
                &mut self.font_system,
            ) {
                runs.push(measured_fragment_run(
                    fragment,
                    Rc::clone(run_tracking_scope(run, block_style)),
                    &mut self.font_system,
                ));
            }
        }
        if !applied {
            return graph.clone();
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
    }
}

fn apply_first_letter_style_to_leading_preserved_whitespace(
    fragment: &mut InlineFragment,
    first_letter_style: &ComputedStyle,
) {
    let source_style = fragment.style().clone();
    let mut prefix_style = first_letter_style.clone();
    prefix_style.font_size = source_style.font_size;
    prefix_style.line_height = source_style.line_height;
    prefix_style.line_height_multiplier = source_style.line_height_multiplier;
    prefix_style.line_height_is_normal = source_style.line_height_is_normal;
    prefix_style.white_space = source_style.white_space;
    prefix_style.tab_size = source_style.tab_size;
    prefix_style.initial_letter = css::InitialLetter::Normal;
    *fragment.style_mut() = prefix_style;
    fragment
        .set_first_letter_pseudo_role(FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace);
    fragment.set_mergeable(false);
}

fn split_fragment_for_first_letter_in_graph(
    fragment: &InlineFragment,
    range: std::ops::Range<usize>,
    first_letter_style: &ComputedStyle,
    block_style: &ComputedStyle,
    font_system: &mut FontSystem,
) -> Vec<InlineFragment> {
    let mut pieces = Vec::new();
    if range.start > 0 {
        let mut before = fragment.clone();
        before.set_text(Rc::<str>::from(&fragment.text()[..range.start]));
        // Leading preserved whitespace belongs to `::first-letter` styling
        // even though it is not the typographic initial used by
        // `initial-letter`. Keep its originating metrics and advance (notably
        // tab-stop expansion), while copying the pseudo's paint style such as
        // a background.
        // <https://drafts.csswg.org/css-pseudo-4/#first-letter-pseudo>
        apply_first_letter_style_to_leading_preserved_whitespace(&mut before, first_letter_style);
        pieces.push(before);
    }
    let mut letter = fragment.clone();
    letter.set_text(Rc::<str>::from(&fragment.text()[range.clone()]));
    let mut used_style =
        used_first_letter_style_for_graph(first_letter_style, block_style, font_system);
    // A first-letter pseudo that merely inherits its originating block's
    // color wraps descendant text. When that text comes from a flattened
    // `display: contents` inline, the descendant's inherited color remains
    // the used color for the typographic fragment; the pseudo's background
    // and other applicable properties still apply.
    // <https://drafts.csswg.org/css-pseudo-4/#first-letter-pseudo>
    if first_letter_style.color == block_style.color && block_style.first_line_style.is_none() {
        used_style.color = fragment.style().color;
    }
    *letter.style_mut() = used_style;
    letter.set_mergeable(false);
    pieces.push(letter);
    if range.end < fragment.text().len() {
        let mut after = fragment.clone();
        after.set_text(Rc::<str>::from(&fragment.text()[range.end..]));
        if let Some(first_line_style) = block_style.first_line_style.as_deref() {
            // This source remains on the same originating formatted line as
            // the graph-owned first letter. Apply the first-line inherited
            // color without reopening the pseudo split at paint time.
            after.style_mut().color = first_line_style.color;
        }
        pieces.push(after);
    }
    pieces
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
    style.line_height_multiplier = None;
    style.line_height_is_normal = false;
    style
}

fn measured_fragment_run(
    mut fragment: InlineFragment,
    tracking_scope: Rc<InlineTrackingScope>,
    font_system: &mut FontSystem,
) -> InlineParagraphRun {
    let mut shaped = font_system.shape_untracked_inline_line(
        fragment.text(),
        fragment.style(),
        fragment.style().line_height,
    );
    let mut width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    normalize_graph_fragment_terminal_tracking(&mut fragment, &mut shaped, &mut width);
    InlineParagraphRun {
        item: InlineLineItem::Fragment(fragment.with_tracking_scope(tracking_scope)),
        width,
        shaped: shaped.map(Rc::new),
    }
}

/// Mark graph shaping as free of backend terminal tracking.
///
/// CSS Text graph layout shapes with `letter-spacing: 0` and owns tracking at
/// final visual boundaries, after line selection and bidi reordering. The
/// marker prevents a later mixed-layout fallback from assuming a backend
/// terminal advance exists and subtracting it from the already-untracked
/// glyph stream.
/// <https://drafts.csswg.org/css-text-3/#letter-spacing-property>
fn normalize_graph_fragment_terminal_tracking(
    fragment: &mut InlineFragment,
    _shaped: &mut Option<ShapedInlineLine>,
    _width: &mut f32,
) {
    if fragment.terminal_tracking_normalized() {
        return;
    }
    fragment.mark_terminal_tracking_normalized();
}

#[derive(Clone)]
struct TextSpacingCharacter {
    item_index: usize,
    range: std::ops::Range<usize>,
    class: Option<crate::text::TextSpacingPunctuationClass>,
    policy: TextSpacingTrim,
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

    let mut characters = Vec::<TextSpacingCharacter>::new();
    for (item_index, item) in items.iter().enumerate() {
        let InlineLineItem::Fragment(fragment) = &item.item else {
            continue;
        };
        let vertical = matches!(
            fragment.style().text_layout_policy(),
            crate::css::TextLayoutPolicy::Vertical(_)
        );
        for (start, character) in fragment.text().char_indices() {
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
    if characters.is_empty() {
        return;
    }

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
    if let Some(first) = characters.first() {
        let trims_start = matches!(
            first.policy,
            TextSpacingTrim::TrimStart | TextSpacingTrim::TrimBoth
        ) || (first.policy == TextSpacingTrim::SpaceFirst && !is_initial_line);
        if first.class == Some(Opening) && trims_start {
            add_target(first.clone());
        }
    }
    if let Some(last) = characters.last()
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
    fragment.set_preserves_source_shaping(false);
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
    let mut shaped = font_system.shape_untracked_inline_line(
        fragment.text(),
        fragment.style(),
        fragment.style().line_height,
    );
    let mut width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    normalize_graph_fragment_terminal_tracking(&mut fragment, &mut shaped, &mut width);
    output.push(MeasuredInlineItem {
        item: InlineLineItem::Fragment(fragment),
        width,
        shaped: shaped.map(Rc::new),
    });
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
    let mut root_tracking_scope = InlineTrackingScope::root(block_style);
    let mut tracking_scopes = vec![Rc::clone(&root_tracking_scope)];
    for item in items {
        match item.as_ref() {
            InlineItem::Word(word) => {
                let text = transform_text_with_state(&word.text, &word.style, &mut transform_state);
                // Some anonymous inline formatting contexts retain the
                // block's inherited text style only on their first text
                // item. Establish that implicit lexical scope before the
                // paragraph receives any explicit inline-edge marker. Once
                // an edge exists, its parent chain remains authoritative for
                // nested-inline LCA ownership.
                if runs.is_empty()
                    && tracking_scopes.len() == 1
                    && root_tracking_scope.letter_spacing().points() == 0.0
                    && word.style.used_letter_spacing().points() != 0.0
                {
                    root_tracking_scope = InlineTrackingScope::root_with_boundary_policy(
                        &word.style,
                        root_tracking_scope.boundary_policy(),
                    );
                    tracking_scopes[0] = Rc::clone(&root_tracking_scope);
                }
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
                transform_state.force_word_boundary();
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
        let mut shaped = font_system.shape_untracked_inline_line(
            previous.text(),
            previous.style(),
            previous.style().line_height,
        );
        let mut width = shaped
            .as_ref()
            .map(ShapedInlineLine::advance_width)
            .unwrap_or(0.0);
        normalize_graph_fragment_terminal_tracking(previous, &mut shaped, &mut width);
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
        if first.style().hyphens != Hyphens::Auto {
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
        for opportunity in opportunities {
            automatic_breaks.push(automatic_opportunity_for_source_offset(
                opportunity,
                &fragment_indices,
                &source_ends,
                runs,
            ));
        }
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
                    trailing_marker: true,
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
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
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
) -> InlineBreakOpportunity {
    let position =
        source_position_for_offset(opportunity.byte_offset, fragment_indices, source_ends, runs);
    InlineBreakOpportunity {
        position,
        kind: InlineBreakKind::Hyphenation,
        priority: 200,
        trims: false,
        hangs: false,
        soft_hyphen: true,
        discretionary: Some(DiscretionaryBreakEffect {
            source_boundary: position,
            trailing_marker: true,
            left_replacement: language_replacement_to_line_edge(opportunity.left),
            right_replacement: language_replacement_to_line_edge(opportunity.right),
            leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
        }),
        emergency: false,
        min_content: true,
    }
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
    opportunities.sort_by_key(|opportunity| (opportunity.position, opportunity.priority));
}

fn merge_manual_discretionary_effects(
    opportunities: &mut [InlineBreakOpportunity],
    effects: Vec<(InlineGraphPosition, DiscretionaryBreakEffect)>,
) {
    for (position, effect) in effects {
        let Some(opportunity) = opportunities
            .iter_mut()
            .find(|opportunity| opportunity.position == position && opportunity.soft_hyphen)
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
                InlineLineItem::Atom(_) | InlineLineItem::Float(_) => break,
            }
        }
        if fragment_indices.len() < 2 {
            continue;
        }
        let mut spans = Vec::with_capacity(fragment_indices.len());
        let mut text = String::new();
        let mut ranges = Vec::with_capacity(fragment_indices.len());
        let mut line_height = None;
        let InlineLineItem::Fragment(first) = &runs[fragment_indices[0]].item else {
            unreachable!("graph shaping group has a first fragment");
        };
        let source_style = first.style();
        let one_text_style = fragment_indices.iter().all(|&fragment_index| {
            let InlineLineItem::Fragment(fragment) = &runs[fragment_index].item else {
                return false;
            };
            styles_have_equivalent_text_shaping_inputs(source_style, fragment.style())
        });
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
                style: if one_text_style {
                    source_style
                } else {
                    fragment.style()
                },
            });
        }
        let Some(shaped) = font_system.shape_styled_inline_fragments(
            &spans,
            text,
            0.0,
            line_height.expect("graph shaping group has a fragment"),
            0.0,
            tab_metric_style,
        ) else {
            continue;
        };
        for (&fragment_index, range) in fragment_indices.iter().zip(ranges) {
            let Some(slice) = shaped.source_slice(range) else {
                continue;
            };
            let mut slice = Some(slice);
            let mut width = slice
                .as_ref()
                .map(ShapedInlineLine::advance_width)
                .unwrap_or(0.0);
            let InlineLineItem::Fragment(fragment) = &mut runs[fragment_index].item else {
                unreachable!("graph fragment indices name fragments");
            };
            normalize_graph_fragment_terminal_tracking(fragment, &mut slice, &mut width);
            runs[fragment_index].width = width;
            runs[fragment_index].shaped = slice.map(Rc::new);
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
    let owner = InlineTrackingScope::lowest_common(left_scope, right_scope);
    owner.letter_spacing().points() != 0.0
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
    let break_text = if word.style.hyphens == Hyphens::Auto {
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
    for range in typographic_unit_ranges(text) {
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
    let mut shaped =
        font_system.shape_untracked_inline_line(text, &word.style, word.style.line_height);
    let mut width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    normalize_graph_fragment_terminal_tracking(&mut fragment, &mut shaped, &mut width);
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

fn inline_run_has_nonzero_tracking(run: &InlineParagraphRun) -> bool {
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
        run_range
            .filter_map(|run_index| {
                self.measured_run_slice_for_graph_range(run_index, range, font_system)
            })
            .collect()
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
            false,
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
            false,
            Some(available_width),
            font_system,
            block_style,
        )
    }

    /// Materialize a selected line with both the terminal preserved-space
    /// behavior and its physical available width.
    pub(in crate::layout) fn materialize_line_with_terminal_pre_wrap_hang_for_available_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        terminal_pre_wrap_hang: bool,
        available_width: f32,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        self.materialize_line_with_text_spacing_width(
            range,
            selected_break,
            terminal_pre_wrap_hang,
            Some(available_width),
            font_system,
            block_style,
        )
    }

    fn materialize_line_with_text_spacing_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        terminal_pre_wrap_hang: bool,
        text_spacing_available_width: Option<f32>,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        let mut items = self.line_measured_items_for_graph_range(range, font_system);
        let selected_manual_soft_hyphen = selected_break.is_some_and(|opportunity| {
            opportunity.soft_hyphen
                && self.source_character_before(opportunity.position) == Some('\u{00ad}')
        });
        // A joining-script source edge needs a generated marker-shaped
        // context group rather than an in-place U+00AD replacement. The
        // latter would split an otherwise transparent inline boundary before
        // the final line-level shaping pass can preserve the cursive forms.
        let authored_marker_owns_style =
            selected_manual_soft_hyphen && !materialized_items_have_joining_behavior(&items);
        let authored_spelling_replacement = selected_manual_soft_hyphen
            && selected_break
                .and_then(|opportunity| opportunity.discretionary)
                .is_some_and(|effect| effect.left_replacement.is_some());
        let trailing_discretionary = selected_break
            .and_then(|opportunity| opportunity.discretionary)
            .map(|mut effect| {
                if selected_manual_soft_hyphen {
                    // An authored U+00AD owns the used marker's style. Its
                    // fragment can be transparent or otherwise styled
                    // differently from the preceding source text, so the
                    // normalizer must materialize that marker before the
                    // fragment disappears. Automatic opportunities use the
                    // separate selected-edge marker path below.
                    effect.trailing_marker = authored_spelling_replacement;
                    if authored_marker_owns_style {
                        effect.leading_shaping_context = SelectedLineShapingContext::None;
                    }
                }
                effect
            });
        let leading_discretionary = self.discretionary_effect_at(range.start);
        // Do not mutate the selected source sequence for CSS Text Phase II.
        // In particular, a collapsed separator before `br` remains available
        // to bidi, extraction, and decoration ownership even though it has no
        // used advance at the selected line edge.
        let trimmed_width = trailing_collapsible_measured_width(&items);
        normalize_materialized_control_characters(
            &mut items,
            authored_marker_owns_style && !authored_spelling_replacement,
            font_system,
        );
        apply_selected_discretionary_break(
            &mut items,
            trailing_discretionary,
            SelectedLineEdge::Trailing,
            font_system,
        );
        apply_selected_discretionary_break(
            &mut items,
            leading_discretionary,
            SelectedLineEdge::Leading,
            font_system,
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
        resolve_materialized_line_tab_advances(&mut items, font_system, block_style);
        let widths = inline_content_width_for_line_items(&items, font_system, |item| item.width);
        // A `pre-wrap` run hangs at a selected soft boundary.  It also hangs
        // before an unconditionally hanging other-space separator, even when
        // the line itself ends at a forced break: that separator means the
        // preserved run is not immediately followed by the forced break.
        // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
        let hanging_pre_wrap_width = if selected_break
            .is_some_and(|opportunity| opportunity.hangs || opportunity_is_soft_wrap(opportunity))
            || terminal_pre_wrap_hang
            || widths.trailing_space_width > 0.0
        {
            trailing_pre_wrap_hanging_width_with_unconditional_separators(&items, font_system)
        } else {
            0.0
        };
        let edge_effects = InlineLineEdgeEffects {
            collapsed_end_trim_width: trimmed_width,
            pre_wrap_hanging_width: hanging_pre_wrap_width,
            hanging_space_separator_width: widths.trailing_space_width,
            trailing_tracking_width: widths.trailing_tracking_width,
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
        let mut text = text_for_measured_items(&items);
        if trimmed_width > 0.0 {
            text.truncate(text.trim_end_matches(is_css_collapsible_whitespace).len());
        }
        let fitting_width = (widths.fitting_width
            - edge_effects.collapsed_end_trim_width
            - edge_effects.pre_wrap_hanging_width)
            .max(0.0);
        MaterializedInlineGraphLine {
            items,
            text,
            fitting_width,
            content_width: fitting_width,
            edge_effects,
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
        if !soft_hyphen.soft_hyphen
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
        let ends_line = selected_break
            .is_some_and(|opportunity| opportunity.hangs || opportunity_is_soft_wrap(opportunity))
            || (selected_break.is_none() && range.end == self.end_position());
        let hanging_pre_wrap_width = if ends_line {
            trailing_pre_wrap_hanging_width_with_unconditional_separators(
                &self.runs[run_range.clone()],
                font_system,
            )
        } else {
            0.0
        };
        let runs = &self.runs[run_range.clone()];
        if runs.iter().any(|run| match &run.item {
            InlineLineItem::Fragment(fragment) => fragment_text_needs_materialized_normalization(
                fragment.text(),
                selected_break.is_some_and(|opportunity| opportunity.soft_hyphen),
            ),
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => false,
        }) {
            return None;
        }
        let widths = inline_content_width_for_line_items(runs, font_system, |run| run.width);
        let tab_advance_adjustment =
            selected_line_tab_advance_adjustment(runs, font_system, block_style, |run| run.width);
        Some(BorrowedInlineLineMeasurement {
            run_range,
            fitting_width: (widths.fitting_width + tab_advance_adjustment
                - trailing_collapsible_run_width(runs)
                - hanging_pre_wrap_width)
                .max(0.0),
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
                if start == 0 && end == text_len {
                    return Some(MeasuredInlineItem {
                        item: run.item.clone(),
                        width: run.width,
                        shaped: run.shaped.clone(),
                    });
                }
                let source_text = fragment.text().to_owned();
                let mut fragment = fragment.clone();
                let mut hanging_edges = fragment.hanging_edges();
                let segment_text = Rc::<str>::from(&source_text[start..end]);
                fragment.set_text(segment_text);
                hanging_edges.blocks_start = hanging_edges.blocks_start && start == 0;
                hanging_edges.blocks_end = hanging_edges.blocks_end && end == text_len;
                fragment = fragment.with_hanging_edges(hanging_edges);
                let selected_shaped = run
                    .shaped
                    .as_deref()
                    .and_then(|shaped| shaped.source_slice(start..end));
                if selected_shaped.is_some() {
                    // Source graph shaping has already removed backend
                    // terminal tracking. Keep that invariant when a selected
                    // line reuses its contextual shaped slice.
                    fragment.mark_terminal_tracking_normalized();
                }
                fragment.set_preserves_source_shaping(selected_shaped.is_some());
                let mut shaped = selected_shaped.or_else(|| {
                    font_system.shape_untracked_inline_line(
                        fragment.text(),
                        fragment.style(),
                        fragment.style().line_height,
                    )
                });
                let mut width = shaped
                    .as_ref()
                    .map(ShapedInlineLine::advance_width)
                    .unwrap_or(0.0);
                normalize_graph_fragment_terminal_tracking(&mut fragment, &mut shaped, &mut width);
                let shaped = shaped.map(Rc::new);
                Some(MeasuredInlineItem {
                    item: InlineLineItem::Fragment(fragment),
                    width,
                    shaped,
                })
            }
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                let run_start = InlineGraphPosition::at_run_start(run_index);
                let run_end = InlineGraphPosition::at_run_start(run_index + 1);
                (range.start <= run_start && run_end <= range.end).then(|| MeasuredInlineItem {
                    item: run.item.clone(),
                    width: run.width,
                    shaped: run.shaped.clone(),
                })
            }
        }
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
            .filter(|opportunity| opportunity.min_content)
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
            selected_break.is_none() && range.end == self.end_position(),
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
            if request.effect.trailing_marker {
                // The marker owns the trailing shaping context. Keeping its
                // ZWJ in the generated item places it immediately before
                // `hyphenate-character` in logical text, including RTL
                // markers whose leading NBSP must not be shaped as a
                // line-start document space.
                append_discretionary_marker(
                    items,
                    request.effect.leading_shaping_context,
                    font_system,
                );
            }
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
    fragment.set_preserves_source_shaping(false);
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
    fragment.set_preserves_source_shaping(false);
    remeasure_materialized_item(item, font_system);
}

/// Append the selected `hyphenate-character` as a paint item distinct from
/// the source fragment that owns the break.  This keeps its style, bidi
/// behavior, and advance visible to normal line materialization rather than
/// disguising it as an edit to a source word.
fn append_discretionary_marker(
    items: &mut Vec<MeasuredInlineItem>,
    _shaping_context: SelectedLineShapingContext,
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
    let marker_text = source_fragment
        .style()
        .hyphenate_character
        .used_text_for_language(source_fragment.style().language.as_deref());
    let mut marker = source_fragment.clone();
    marker.set_text(marker_text);
    marker.set_preserves_source_shaping(false);
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
    if let Some(shaped) = font_system.shape_styled_inline_fragments(
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
            source.width = source_slice.advance_width();
            source.shaped = Some(Rc::new(source_slice));
            if let InlineLineItem::Fragment(fragment) = &mut source.item {
                fragment.set_preserves_source_shaping(true);
            }
        }
        if let Some(marker_slice) = shaped.source_slice(marker_range) {
            let width = marker_slice.advance_width();
            marker.set_preserves_source_shaping(true);
            items.push(MeasuredInlineItem {
                item: InlineLineItem::Fragment(marker),
                width,
                shaped: Some(Rc::new(marker_slice)),
            });
            return;
        }
    }
    let mut materialized_marker = MeasuredInlineItem {
        item: InlineLineItem::Fragment(marker),
        width: 0.0,
        shaped: None,
    };
    remeasure_materialized_item(&mut materialized_marker, font_system);
    items.push(materialized_marker);
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
                .shape_styled_inline_fragments(
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
) {
    let mut cursor = 0.0;
    let mut index = 0;
    while index < items.len() {
        let InlineLineItem::Fragment(first_fragment) = &items[index].item else {
            cursor += items[index].width;
            index += 1;
            continue;
        };
        let start = index;
        let mut spans = Vec::new();
        let mut text = String::new();
        let mut ranges = Vec::new();
        let mut unadjusted_width = 0.0;
        let mut has_tab = false;
        let mut line_height = first_fragment.style().line_height;
        while let Some(item) = items.get(index) {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                break;
            };
            has_tab |= fragment.text().contains('\t');
            line_height = fragment.style().line_height;
            let range_start = text.len();
            text.push_str(fragment.text());
            ranges.push(range_start..text.len());
            spans.push(StyledTextSpan {
                text: fragment.text(),
                style: fragment.style(),
            });
            unadjusted_width += item.width;
            index += 1;
        }
        debug_assert!(index > start);
        if !has_tab {
            cursor += unadjusted_width;
            continue;
        }
        let Some(shaped) = font_system.shape_styled_inline_fragments(
            &spans,
            text,
            0.0,
            line_height,
            cursor,
            tab_metric_style,
        ) else {
            cursor += unadjusted_width;
            continue;
        };
        let used_width = shaped.advance_width();
        for (item_index, range) in (start..index).zip(ranges) {
            let Some(slice) = shaped.source_slice(range) else {
                continue;
            };
            let mut slice = Some(slice);
            let mut width = slice
                .as_ref()
                .map(ShapedInlineLine::advance_width)
                .unwrap_or(0.0);
            let InlineLineItem::Fragment(fragment) = &mut items[item_index].item else {
                unreachable!("a contiguous text group only contains fragments");
            };
            normalize_graph_fragment_terminal_tracking(fragment, &mut slice, &mut width);
            fragment.set_preserves_source_shaping(true);
            items[item_index].width = width;
            items[item_index].shaped = slice.map(Rc::new);
        }
        cursor += used_width;
    }
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
                trimmed_width += item.width;
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
    let preserve_soft_hyphen_joining =
        visible_trailing_soft_hyphen && materialized_items_have_joining_behavior(items);
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
            && let Some(text) = normalize_materialized_fragment_text(
                fragment.text(),
                Some(index) == trailing_soft_hyphen_index,
                preserve_soft_hyphen_joining,
                fragment
                    .style()
                    .hyphenate_character
                    .used_text_for_language(fragment.style().language.as_deref()),
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
            character_has_joining_behavior(character)
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
    fragment.set_preserves_source_shaping(false);
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
    fragment.set_preserves_source_shaping(false);
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
                // An authored FSI must not be classified as a CSS scope.
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
            &[MeasuredInlineItem {
                item: InlineLineItem::Fragment(fragment),
                width: 40.0 + separator_width,
                shaped: None,
            }],
            &mut font_system,
            |item| item.width,
        );

        assert_eq!(widths.trailing_space_width, separator_width);
        assert_eq!(widths.fitting_width, 40.0);
        assert_eq!(widths.content_width, 40.0);
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
            &[MeasuredInlineItem {
                item: InlineLineItem::Fragment(fragment),
                width: 40.0 + separator_width + document_space_width,
                shaped: None,
            }],
            &mut font_system,
            |item| item.width,
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
            &[MeasuredInlineItem {
                item: InlineLineItem::Fragment(fragment),
                width: 40.0 + hanging_width,
                shaped: None,
            }],
            &mut font_system,
            |item| item.width,
        );

        assert_eq!(widths.trailing_space_width, hanging_width);
        assert_eq!(widths.fitting_width, 40.0);
    }

    #[test]
    fn automatic_marker_is_a_separate_selected_item_with_source_context() {
        let mut style = ComputedStyle::initial();
        style.language = Some("ug".into());
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
        let mut items = vec![MeasuredInlineItem {
            item: InlineLineItem::Fragment(source),
            width: 0.0,
            shaped: None,
        }];
        let mut font_system = FontSystem::new();
        apply_selected_discretionary_break(
            &mut items,
            Some(DiscretionaryBreakEffect {
                source_boundary: InlineGraphPosition::at_run_start(0),
                trailing_marker: true,
                left_replacement: None,
                right_replacement: None,
                leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
            }),
            SelectedLineEdge::Trailing,
            &mut font_system,
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
}
