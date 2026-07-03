use super::*;
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
#[derive(Debug, Clone)]
pub(in crate::layout) struct MaterializedInlineGraphLine {
    pub(in crate::layout) items: Vec<MeasuredInlineItem>,
    pub(in crate::layout) text: String,
    pub(in crate::layout) content_width: f32,
    // Retained for line-materialization tests that verify CSS Text trimming
    // behavior before all consumers need the trimmed amount.
    #[allow(dead_code)]
    pub(in crate::layout) trimmed_width: f32,
    pub(in crate::layout) hanging_space_width: f32,
    pub(in crate::layout) trailing_tracking_width: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct InlineContentWidth {
    pub(in crate::layout) content_width: f32,
    pub(in crate::layout) trailing_space_width: f32,
    pub(in crate::layout) trailing_tracking_width: f32,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct BorrowedInlineLineMeasurement {
    pub(in crate::layout) run_range: std::ops::Range<usize>,
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
    let consumed_pre_wrap_width = (line.hanging_space_width - old_trailing_space_width).max(0.0);
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
        if pattern.is_empty() || pattern_width <= 0.0 || leader_share <= 0.0 {
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
    line.trailing_tracking_width = widths.trailing_tracking_width;
    line.hanging_space_width = consumed_pre_wrap_width + widths.trailing_space_width;
    line.content_width = widths.content_width;
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
            WritingMode::VerticalRl | WritingMode::VerticalLr => self
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
            WritingMode::VerticalRl | WritingMode::VerticalLr => self.height(),
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
            text: Rc::from(text.into()),
        }
    }

    pub(in crate::layout) fn items(&self) -> &[MeasuredInlineItem] {
        &self.items
    }

    pub(in crate::layout) fn text(&self) -> &str {
        &self.text
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Build the inline opportunity graph for one mixed inline paragraph.
    ///
    /// Text transform is applied exactly once while normalizing `InlineItem`s
    /// into graph runs. Unicode break opportunities come from the existing
    /// ICU/Parley-backed text helpers; Reasyprint records CSS policy metadata
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
    let opportunities = inline_break_opportunities_for_runs(&runs);
    InlineOpportunityGraph {
        runs,
        opportunities,
    }
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
    let break_text = text_with_hyphenation_controls(text, &word.style);
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
        word.style.clone(),
        word.baseline_shift,
        word.link_target.clone(),
        word.mergeable,
        word.source,
        false,
        hanging_edges,
        word.ancestor_inline_decorations.clone(),
    )
    .with_visual_offset(word.visual_offset);
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
        let mut items = self.line_measured_items_for_graph_range(range, font_system);
        let trimmed_width = trim_trailing_collapsible_measured_items(&mut items);
        let consumed_pre_wrap_width = if selected_break
            .is_some_and(|opportunity| opportunity.hangs || opportunity_is_soft_wrap(opportunity))
        {
            trim_trailing_pre_wrap_hanging_measured_items(&mut items)
        } else {
            0.0
        };
        normalize_materialized_control_characters(
            &mut items,
            selected_break.is_some_and(|opportunity| opportunity.soft_hyphen),
            font_system,
        );
        let widths = inline_content_width_for_line_items(&items, font_system, |item| item.width);
        let hanging_space_width = consumed_pre_wrap_width + widths.trailing_space_width;
        let text = text_for_measured_items(&items);
        MaterializedInlineGraphLine {
            items,
            text,
            content_width: widths.content_width,
            trimmed_width,
            hanging_space_width,
            trailing_tracking_width: widths.trailing_tracking_width,
        }
    }

    pub(in crate::layout) fn break_opportunities_after(
        &self,
        start: InlineGraphPosition,
    ) -> impl Iterator<Item = InlineBreakOpportunity> + '_ {
        self.opportunities
            .iter()
            .copied()
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
        let mut run_range = self.run_indices_for_graph_range(range)?;
        while run_range.end > run_range.start
            && inline_line_item_is_collapsible_space(&self.runs[run_range.end - 1].item)
        {
            run_range.end -= 1;
        }
        if selected_break
            .is_some_and(|opportunity| opportunity.hangs || opportunity_is_soft_wrap(opportunity))
        {
            while run_range.end > run_range.start
                && inline_line_item_is_pre_wrap_hanging_space(&self.runs[run_range.end - 1].item)
            {
                run_range.end -= 1;
            }
        }
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
        Some(BorrowedInlineLineMeasurement {
            run_range,
            content_width: widths.content_width,
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
                let mut fragment = fragment.clone();
                let mut hanging_edges = fragment.hanging_edges();
                fragment.set_text(fragment.text()[start..end].to_string());
                hanging_edges.blocks_start = hanging_edges.blocks_start && start == 0;
                hanging_edges.blocks_end = hanging_edges.blocks_end && end == text_len;
                fragment = fragment.with_hanging_edges(hanging_edges);
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
        let hanging_widths = hanging_punctuation_widths_for_line_items(
            font_system,
            &self.runs,
            block_style,
            true,
            true,
            false,
        );
        let max_content =
            (widths.content_width - hanging_widths.start - hanging_widths.end).max(0.0);

        let mut min_content = 0.0_f32;
        let mut segment_start = self.start_position();
        for opportunity in self
            .opportunities
            .iter()
            .copied()
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
            min_content =
                min_content.max(self.intrinsic_segment_width(range, font_system, block_style));
            segment_start = opportunity.position;
        }
        min_content = min_content.max(self.intrinsic_segment_width(
            InlineGraphRange {
                start: segment_start,
                end: self.end_position(),
            },
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
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> f32 {
        if let Some(measurement) =
            self.borrowed_line_measurement_for_full_run_range(range, None, font_system)
        {
            return measurement.content_width;
        }
        let materialized = self.materialize_line(range, None, font_system, block_style);
        if materialized.items.is_empty() {
            return 0.0;
        }
        materialized.content_width
    }
}

pub(in crate::layout) fn opportunity_is_soft_wrap(opportunity: InlineBreakOpportunity) -> bool {
    !matches!(opportunity.kind, InlineBreakKind::Forced)
}

pub(in crate::layout) fn trim_trailing_collapsible_measured_items(
    items: &mut Vec<MeasuredInlineItem>,
) -> f32 {
    let mut trimmed_width = 0.0;
    while let Some(MeasuredInlineItem {
        item: InlineLineItem::Fragment(fragment),
        width,
        ..
    }) = items.last()
        && fragment.style().white_space.collapses_spaces()
        && fragment.text().chars().all(is_css_collapsible_whitespace)
    {
        trimmed_width += *width;
        items.pop();
    }
    trimmed_width
}

pub(in crate::layout) fn trim_trailing_pre_wrap_hanging_measured_items(
    items: &mut Vec<MeasuredInlineItem>,
) -> f32 {
    let mut trimmed_width = 0.0;
    while let Some(MeasuredInlineItem { item, width, .. }) = items.last()
        && inline_line_item_is_pre_wrap_hanging_space(item)
    {
        trimmed_width += *width;
        items.pop();
    }
    trimmed_width
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
            && let Some(text) = normalize_materialized_fragment_text(
                fragment.text(),
                Some(index) == trailing_soft_hyphen_index,
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
            style: shared_style.clone(),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
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
}
