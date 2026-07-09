use super::*;
use crate::css::Hyphens;
use crate::text::character_is_css_other_space_separator;
use crate::text::{hyphenator_for_language, text_with_auto_hyphenation, text_with_css_line_breaks};
use std::borrow::Cow;
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
    /// The corresponding source fragments remain in `InlineLineFragment` and
    /// are omitted only from the visual paint sequence after bidi ordering.
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout) struct InlineIntrinsicContribution {
    pub(in crate::layout) min_content: f32,
    pub(in crate::layout) max_content: f32,
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
            WritingMode::HorizontalTb => self.contribution.max_content,
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self.height(),
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
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) suppress_float_adjust: bool,
    pub(in crate::layout) edge_effects: InlineLineEdgeEffects,
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
        suppress_float_adjust: bool,
        text: impl Into<String>,
    ) -> Self {
        Self {
            items: Rc::from(items.into_boxed_slice()),
            metrics,
            hanging_widths,
            indent,
            available_width,
            suppress_float_adjust,
            edge_effects: InlineLineEdgeEffects::default(),
            text: Rc::from(text.into()),
        }
    }

    pub(in crate::layout) fn items(&self) -> &[MeasuredInlineItem] {
        &self.items
    }

    pub(in crate::layout) fn text(&self) -> &str {
        &self.text
    }

    pub(in crate::layout) fn with_edge_effects(
        mut self,
        edge_effects: InlineLineEdgeEffects,
    ) -> Self {
        self.edge_effects = edge_effects;
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
            applied = true;
            for fragment in split_fragment_for_first_letter_in_graph(
                fragment,
                range,
                first_letter_style,
                block_style,
                &mut self.font_system,
            ) {
                runs.push(measured_fragment_run(fragment, &mut self.font_system));
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
        shape_logical_joining_graph_runs(&mut runs, &mut self.font_system);
        let opportunities = inline_break_opportunities_for_runs(&runs, block_style);
        InlineOpportunityGraph {
            runs,
            opportunities,
        }
    }
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
        pieces.push(before);
    }
    let mut letter = fragment.clone();
    letter.set_text(Rc::<str>::from(&fragment.text()[range.clone()]));
    let used_style =
        used_first_letter_style_for_graph(first_letter_style, block_style, font_system);
    if let Some((_size, sink)) = used_style.initial_letter.specified() {
        letter.baseline_shift -= sink.saturating_sub(1) as f32 * block_style.line_height;
    }
    *letter.style_mut() = used_style;
    letter.set_mergeable(false);
    pieces.push(letter);
    if range.end < fragment.text().len() {
        let mut after = fragment.clone();
        after.set_text(Rc::<str>::from(&fragment.text()[range.end..]));
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
    let Some((size, _sink)) = style.initial_letter.specified() else {
        return style;
    };
    let surrounding_cap_height = font_system.used_cap_height_for_style(block_style).points();
    let initial_cap_height = font_system.used_cap_height_for_style(&style).points();
    let cap_ratio = (initial_cap_height / style.font_size.max(0.01)).max(0.01);
    let target_cap_height =
        ((size - 1.0).max(0.0) * block_style.line_height) + surrounding_cap_height.max(0.0);
    let used_font_size = (target_cap_height / cap_ratio).max(style.font_size);
    style.font_size = used_font_size;
    style.line_height = used_font_size;
    style.line_height_multiplier = None;
    style.line_height_is_normal = false;
    style
}

fn measured_fragment_run(
    fragment: InlineFragment,
    font_system: &mut FontSystem,
) -> InlineParagraphRun {
    let shaped = font_system.shape_unwrapped_line(
        fragment.text(),
        fragment.style(),
        fragment.style().line_height,
    );
    let width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    InlineParagraphRun {
        item: InlineLineItem::Fragment(fragment),
        width,
        shaped: shaped.map(Rc::new),
    }
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
    for item in items {
        match item.as_ref() {
            InlineItem::Word(word) => {
                let text = transform_text_with_state(&word.text, &word.style, &mut transform_state);
                push_text_graph_runs(font_system, &mut runs, word, &text);
            }
            InlineItem::Atom(atom) => {
                transform_state.force_word_boundary();
                runs.push(InlineParagraphRun {
                    item: InlineLineItem::Atom((**atom).clone()),
                    width: inline_atom_logical_inline_size(atom, block_style),
                    shaped: None,
                });
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
    apply_auto_hyphenation_across_transparent_inline_edges(&mut runs, font_system);
    shape_logical_joining_graph_runs(&mut runs, font_system);
    let opportunities = inline_break_opportunities_for_runs(&runs, block_style);
    InlineOpportunityGraph {
        runs,
        opportunities,
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
    runs: &mut [InlineParagraphRun],
    font_system: &mut FontSystem,
) {
    let mut index = 0;
    while index < runs.len() {
        let InlineLineItem::Fragment(first) = &runs[index].item else {
            index += 1;
            continue;
        };
        if first.style().hyphens != Hyphens::Auto {
            index += 1;
            continue;
        }
        let mut fragment_indices = vec![index];
        index += 1;
        while let Some(run) = runs.get(index) {
            match &run.item {
                InlineLineItem::Fragment(next)
                    if graph_fragments_share_auto_hyphenation_policy(
                        match &runs[*fragment_indices.last().expect("first auto fragment")].item {
                            InlineLineItem::Fragment(fragment) => fragment,
                            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                                unreachable!("auto fragment index names a fragment")
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
                InlineLineItem::Fragment(_)
                | InlineLineItem::Atom(_)
                | InlineLineItem::Float(_) => break,
            }
        }
        let InlineLineItem::Fragment(first) = &runs[fragment_indices[0]].item else {
            unreachable!("first auto fragment index names a fragment");
        };
        let Some(language) = first.style().language.as_deref() else {
            continue;
        };
        let Some(hyphenator) = hyphenator_for_language(language) else {
            continue;
        };
        let mut source = String::new();
        let mut source_ends = Vec::with_capacity(fragment_indices.len());
        for &fragment_index in &fragment_indices {
            let InlineLineItem::Fragment(fragment) = &runs[fragment_index].item else {
                unreachable!("auto fragment index names a fragment");
            };
            source.push_str(fragment.text());
            source_ends.push(source.len());
        }
        let hyphenated =
            text_with_auto_hyphenation(&source, &hyphenator, first.style().hyphenate_limit_chars);
        let mut output = vec![String::new(); fragment_indices.len()];
        let mut source_offset = 0;
        let mut fragment_offset = 0;
        for character in hyphenated.chars() {
            let source_character = source[source_offset..].chars().next();
            if character == '\u{00ad}' && source_character != Some(character) {
                output[fragment_offset].push(character);
                continue;
            }
            while fragment_offset + 1 < source_ends.len()
                && source_offset >= source_ends[fragment_offset]
            {
                fragment_offset += 1;
            }
            output[fragment_offset].push(character);
            source_offset += character.len_utf8();
        }
        debug_assert_eq!(source_offset, source.len());
        for (&fragment_index, text) in fragment_indices.iter().zip(output) {
            let InlineLineItem::Fragment(fragment) = &mut runs[fragment_index].item else {
                unreachable!("auto fragment index names a fragment");
            };
            let text = text_with_css_line_breaks(&text, fragment.style());
            if fragment.text() != text {
                fragment.set_text(text);
                let shaped = font_system.shape_unwrapped_line(
                    fragment.text(),
                    fragment.style(),
                    fragment.style().line_height,
                );
                runs[fragment_index].width = shaped
                    .as_ref()
                    .map(ShapedInlineLine::advance_width)
                    .unwrap_or(0.0);
                runs[fragment_index].shaped = shaped.map(Rc::new);
            }
        }
    }
}

fn graph_fragments_share_auto_hyphenation_policy(
    left: &InlineFragment,
    right: &InlineFragment,
) -> bool {
    left.style().hyphens == Hyphens::Auto
        && right.style().hyphens == Hyphens::Auto
        && left.style().language == right.style().language
        && left.style().hyphenate_limit_chars == right.style().hyphenate_limit_chars
}

/// Shape joining-script source runs across transparent inline element edges.
///
/// CSS Text establishes shaping before it selects a line break. Keeping the
/// source-shaped slices on graph runs means a later selected soft-hyphen edge
/// cannot turn an Arabic medial glyph into a final glyph merely because an
/// otherwise transparent `span` owns the soft hyphen:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
fn shape_logical_joining_graph_runs(runs: &mut [InlineParagraphRun], font_system: &mut FontSystem) {
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
        if fragment_indices.len() < 2
            || !fragment_indices.iter().any(|&fragment_index| {
                matches!(&runs[fragment_index].item, InlineLineItem::Fragment(fragment)
                    if fragment.text().chars().any(character_has_joining_behavior))
            })
        {
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
        ) else {
            continue;
        };
        for (&fragment_index, range) in fragment_indices.iter().zip(ranges) {
            let Some(slice) = shaped.source_slice(range) else {
                continue;
            };
            runs[fragment_index].width = slice.advance_width();
            runs[fragment_index].shaped = Some(Rc::new(slice));
        }
    }
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

    push_text_graph_run_segment(font_system, runs, word, text, word.hanging_edges);
}

pub(in crate::layout) fn push_text_graph_run_segment(
    font_system: &mut FontSystem,
    runs: &mut Vec<InlineParagraphRun>,
    word: &InlineWord,
    text: &str,
    hanging_edges: InlineHangingEdges,
) {
    if text.is_empty() {
        return;
    }
    let fragment = InlineFragment::new_shared_style(
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
    .with_visual_offset(word.visual_offset);
    let shaped = font_system.shape_unwrapped_line(text, &word.style, word.style.line_height);
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

impl InlineOpportunityGraph {
    pub(in crate::layout) fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub(in crate::layout) fn start_position(&self) -> InlineGraphPosition {
        InlineGraphPosition::at_run_start(0)
    }

    pub(in crate::layout) fn end_position(&self) -> InlineGraphPosition {
        InlineGraphPosition::at_run_start(self.runs.len())
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
        _block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        self.materialize_line_with_terminal_pre_wrap_hang(
            range,
            selected_break,
            false,
            font_system,
            _block_style,
        )
    }

    /// Materialize a selected line whose paragraph began at a preserved forced
    /// break. HTML textarea raw text exposes that terminal editing-value case:
    /// its final preserved document-space suffix is outside the right-aligned
    /// editing line measure even though an ordinary terminal `pre-wrap` line
    /// retains its advance. CSS Text's general terminal rule remains in the
    /// public materializer used by intrinsic sizing and non-control layout.
    /// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
    pub(in crate::layout) fn materialize_line_with_terminal_pre_wrap_hang(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        terminal_pre_wrap_hang: bool,
        font_system: &mut FontSystem,
        _block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        let mut items = self.line_measured_items_for_graph_range(range, font_system);
        // Do not mutate the selected source sequence for CSS Text Phase II.
        // In particular, a collapsed separator before `br` remains available
        // to bidi, extraction, and decoration ownership even though it has no
        // used advance at the selected line edge.
        let trimmed_width = trailing_collapsible_measured_width(&items);
        normalize_materialized_control_characters(
            &mut items,
            selected_break.is_some_and(|opportunity| opportunity.soft_hyphen),
            font_system,
        );
        if self.source_character_before(range.start) == Some('\u{00ad}')
            && materialized_items_have_joining_behavior(&items)
        {
            // A selected soft-hyphen break divides one shaping run into two
            // independently painted line fragments. Preserve joining context
            // at the new line edge; the matching trailing ZWJ is inserted
            // with the used hyphenate character above. This is observable for
            // Arabic-family scripts even when the used marker is not joining.
            // <https://drafts.csswg.org/css-text-4/#hyphenate-character>
            prepend_materialized_line_joiner(&mut items, font_system);
        }
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
        let tab_advance_adjustment =
            selected_line_tab_advance_adjustment(&items, font_system, |item| item.width);
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
        let fitting_width = (widths.fitting_width + tab_advance_adjustment
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

    fn source_character_before(&self, position: InlineGraphPosition) -> Option<char> {
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

    pub(in crate::layout) fn break_opportunities_after(
        &self,
        start: InlineGraphPosition,
    ) -> impl Iterator<Item = InlineBreakOpportunity> + '_ {
        self.opportunities
            .iter()
            .cloned()
            .filter(move |opportunity| opportunity.position > start)
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

    pub(in crate::layout) fn borrowed_line_measurement_for_full_run_range(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        font_system: &mut FontSystem,
    ) -> Option<BorrowedInlineLineMeasurement> {
        if range.start.byte_offset != 0 || range.end.byte_offset != 0 {
            return None;
        }
        let run_range = self.run_indices_for_graph_range(range)?;
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
            selected_line_tab_advance_adjustment(runs, font_system, |run| run.width);
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
                fragment.set_preserves_source_shaping(selected_shaped.is_some());
                let shaped = selected_shaped.or_else(|| {
                    font_system.shape_unwrapped_line(
                        fragment.text(),
                        fragment.style(),
                        fragment.style().line_height,
                    )
                });
                let width = shaped
                    .as_ref()
                    .map(ShapedInlineLine::advance_width)
                    .unwrap_or(0.0);
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
        let widths = inline_content_width_for_line_items(&self.runs, font_system, |run| run.width);
        let tab_advance_adjustment =
            selected_line_tab_advance_adjustment(&self.runs, font_system, |run| run.width);
        let hanging_widths = hanging_punctuation_widths_for_line_items(
            font_system,
            &self.runs,
            block_style,
            true,
            true,
            false,
        );
        let max_content = (widths.content_width + tab_advance_adjustment
            - hanging_widths.start
            - hanging_widths.end)
            .max(0.0);
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
        InlineIntrinsicContribution {
            min_content,
            max_content: max_content.max(min_content),
        }
    }

    pub(in crate::layout) fn intrinsic_segment_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> f32 {
        if let Some(measurement) =
            self.borrowed_line_measurement_for_full_run_range(range, selected_break, font_system)
        {
            return measurement.content_width;
        }
        let materialized = self.materialize_line(range, selected_break, font_system, block_style);
        if materialized.items.is_empty() {
            return 0.0;
        }
        materialized.content_width
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

pub(in crate::layout) fn opportunity_is_soft_wrap(opportunity: InlineBreakOpportunity) -> bool {
    !matches!(opportunity.kind, InlineBreakKind::Forced)
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
                fragment.style().hyphenate_character.used_text(),
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
            if fragment.text().chars().any(character_has_joining_behavior))
    })
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
}
