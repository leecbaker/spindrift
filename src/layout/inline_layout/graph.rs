use super::super::*;
use super::items::InlineLineSequence;
use crate::text::{
    grapheme_cluster_inner_boundaries, inline_atomic_boundary_allows_soft_wrap,
    is_css_preserved_document_space, measured_break_opportunities,
    text_break_is_min_content_eligible,
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
    pub(in crate::layout) shaped: Option<ShapedInlineLine>,
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
    pub(in crate::layout) shaped: Option<ShapedInlineLine>,
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
#[allow(dead_code)]
pub(in crate::layout) struct MaterializedInlineGraphLine {
    pub(in crate::layout) items: Vec<MeasuredInlineItem>,
    pub(in crate::layout) text: String,
    pub(in crate::layout) content_width: f32,
    pub(in crate::layout) trimmed_width: f32,
    pub(in crate::layout) hanging_space_width: f32,
    pub(in crate::layout) trailing_tracking_width: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct InlineContentWidth {
    content_width: f32,
    trailing_space_width: f32,
    trailing_tracking_width: f32,
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

fn inline_content_width_for_line_items<T, F>(
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
                InlineLineItem::Atom(atom) if matches!(atom.content, InlineAtomContent::Leader(_))
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
        let InlineAtomContent::Leader(pattern) = &atom.content else {
            resolved_items.push(item);
            continue;
        };

        let pattern_width = font_system.measure_text(pattern, &atom.style);
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
        let fragment = InlineFragment {
            text,
            style: atom.style.clone(),
            baseline_shift: atom.baseline_shift,
            link_target: atom.link_target.clone(),
            mergeable: false,
            source: InlineTextSource::Normal,
            generated_leader: true,
            hanging_edges: InlineHangingEdges::default(),
        };
        let shaped = font_system.shape_unwrapped_line(
            &fragment.text,
            &fragment.style,
            fragment.style.line_height,
        );
        let width = shaped
            .as_ref()
            .map(ShapedInlineLine::advance_width)
            .unwrap_or(0.0);
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
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineBreakKind {
    Forced,
    SoftWrap,
    PreservedSpace,
    BreakSpaces,
    Hyphenation,
    Emergency,
    AtomicBoundary,
    HangingPunctuation,
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
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineMeasuredParagraph {
    pub(in crate::layout) graph: InlineOpportunityGraph,
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
#[allow(dead_code)]
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
    pub(in crate::layout) items: Vec<MeasuredInlineItem>,
    pub(in crate::layout) metrics: InlineLineMetrics,
    pub(in crate::layout) hanging_widths: HangingPunctuationWidths,
    pub(in crate::layout) indent: f32,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) suppress_float_adjust: bool,
    pub(in crate::layout) text: String,
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
            InlineItem::Break | InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => {
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

fn push_text_graph_runs(
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

fn push_text_graph_run_segment(
    font_system: &mut FontSystem,
    runs: &mut Vec<InlineParagraphRun>,
    word: &InlineWord,
    text: &str,
    hanging_edges: InlineHangingEdges,
) {
    if text.is_empty() {
        return;
    }
    let fragment = InlineFragment {
        text: text.to_string(),
        style: word.style.clone(),
        baseline_shift: word.baseline_shift,
        link_target: word.link_target.clone(),
        mergeable: word.mergeable,
        source: word.source,
        generated_leader: false,
        hanging_edges,
    };
    let shaped = font_system.shape_unwrapped_line(
        &fragment.text,
        &fragment.style,
        fragment.style.line_height,
    );
    let width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
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
                &fragment.text,
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

    fn measured_run_slice_for_graph_range(
        &self,
        run_index: usize,
        range: InlineGraphRange,
        font_system: &mut FontSystem,
    ) -> Option<MeasuredInlineItem> {
        let run = self.runs.get(run_index)?;
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                let text_len = fragment.text.len();
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
                    || !fragment.text.is_char_boundary(start)
                    || !fragment.text.is_char_boundary(end)
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
                fragment.text = fragment.text[start..end].to_string();
                fragment.hanging_edges.blocks_start =
                    fragment.hanging_edges.blocks_start && start == 0;
                fragment.hanging_edges.blocks_end =
                    fragment.hanging_edges.blocks_end && end == text_len;
                let shaped = font_system.shape_unwrapped_line(
                    &fragment.text,
                    &fragment.style,
                    fragment.style.line_height,
                );
                let width = shaped
                    .as_ref()
                    .map(ShapedInlineLine::advance_width)
                    .unwrap_or(0.0);
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

    fn intrinsic_segment_width(
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

fn opportunity_is_soft_wrap(opportunity: InlineBreakOpportunity) -> bool {
    !matches!(opportunity.kind, InlineBreakKind::Forced)
}

fn trim_trailing_collapsible_measured_items(items: &mut Vec<MeasuredInlineItem>) -> f32 {
    let mut trimmed_width = 0.0;
    while let Some(MeasuredInlineItem {
        item: InlineLineItem::Fragment(fragment),
        width,
        ..
    }) = items.last()
        && fragment.style.white_space.collapses_spaces()
        && fragment.text.chars().all(is_css_collapsible_whitespace)
    {
        trimmed_width += *width;
        items.pop();
    }
    trimmed_width
}

fn trim_trailing_pre_wrap_hanging_measured_items(items: &mut Vec<MeasuredInlineItem>) -> f32 {
    let mut trimmed_width = 0.0;
    while let Some(MeasuredInlineItem { item, width, .. }) = items.last()
        && inline_line_item_is_pre_wrap_hanging_space(item)
    {
        trimmed_width += *width;
        items.pop();
    }
    trimmed_width
}

fn normalize_materialized_control_characters(
    items: &mut Vec<MeasuredInlineItem>,
    visible_trailing_soft_hyphen: bool,
    font_system: &mut FontSystem,
) {
    let trailing_soft_hyphen_index = visible_trailing_soft_hyphen
        .then(|| {
            items.iter().rposition(|item| {
                matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text.is_empty())
            })
        })
        .flatten();
    let mut index = 0;
    while index < items.len() {
        let mut remove = false;
        if let InlineLineItem::Fragment(fragment) = &mut items[index].item
            && let Some(text) = normalize_materialized_fragment_text(
                &fragment.text,
                Some(index) == trailing_soft_hyphen_index,
            )
        {
            fragment.text = text;
            remove = fragment.text.is_empty();
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

fn fragment_text_needs_materialized_normalization(
    text: &str,
    visible_trailing_soft_hyphen: bool,
) -> bool {
    const SOFT_HYPHEN: char = '\u{00ad}';
    const ZERO_WIDTH_SPACE: char = '\u{200b}';
    text.contains(ZERO_WIDTH_SPACE)
        || text.contains(SOFT_HYPHEN)
        || (visible_trailing_soft_hyphen && text.ends_with(SOFT_HYPHEN))
}

fn normalize_materialized_fragment_text(
    text: &str,
    visible_trailing_soft_hyphen: bool,
) -> Option<String> {
    const SOFT_HYPHEN: char = '\u{00ad}';
    const ZERO_WIDTH_SPACE: char = '\u{200b}';
    let has_zero_width_space = text.contains(ZERO_WIDTH_SPACE);
    let has_soft_hyphen = text.contains(SOFT_HYPHEN);
    if !has_zero_width_space && !has_soft_hyphen {
        return None;
    }
    let mut normalized = if has_zero_width_space {
        text.replace(ZERO_WIDTH_SPACE, "")
    } else {
        text.to_string()
    };
    if !has_soft_hyphen {
        return Some(normalized);
    }
    if visible_trailing_soft_hyphen && normalized.ends_with(SOFT_HYPHEN) {
        normalized.pop();
        normalized = normalized.replace(SOFT_HYPHEN, "");
        normalized.push('-');
        Some(normalized)
    } else {
        Some(normalized.replace(SOFT_HYPHEN, ""))
    }
}

fn remeasure_materialized_item(item: &mut MeasuredInlineItem, font_system: &mut FontSystem) {
    let InlineLineItem::Fragment(fragment) = &item.item else {
        return;
    };
    item.shaped = font_system.shape_unwrapped_line(
        &fragment.text,
        &fragment.style,
        fragment.style.line_height,
    );
    item.width = item
        .shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
}

fn text_for_measured_items(items: &[MeasuredInlineItem]) -> String {
    items
        .iter()
        .filter_map(|item| match &item.item {
            InlineLineItem::Fragment(fragment) => Some(fragment.text.as_str()),
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
        })
        .collect()
}

fn inline_break_opportunities_for_runs(runs: &[InlineParagraphRun]) -> Vec<InlineBreakOpportunity> {
    let mut opportunities = Vec::new();
    for (run_index, run) in runs.iter().enumerate() {
        opportunities.extend(inline_break_opportunities_inside_run(run_index, run));
    }
    opportunities.extend(inline_break_opportunities_across_transparent_edges(runs));
    for boundary in 1..runs.len() {
        if let Some(opportunity) = inline_break_opportunity_at_boundary(boundary, runs) {
            opportunities.push(opportunity);
        }
    }
    if !runs.is_empty() {
        opportunities.push(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(runs.len()),
            kind: InlineBreakKind::Forced,
            priority: u8::MAX,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
            min_content: false,
        });
    }
    opportunities.sort_by_key(|opportunity| (opportunity.position, opportunity.priority));
    opportunities.dedup_by(|left, right| {
        left.position == right.position
            && left.kind == right.kind
            && left.emergency == right.emergency
            && left.min_content == right.min_content
    });
    opportunities
}

fn inline_break_opportunities_inside_run(
    run_index: usize,
    run: &InlineParagraphRun,
) -> Vec<InlineBreakOpportunity> {
    let InlineLineItem::Fragment(fragment) = &run.item else {
        return Vec::new();
    };
    if !fragment.style.white_space.allows_soft_wrap() {
        return Vec::new();
    }
    let text = fragment.text.as_str();
    let mut output = Vec::new();
    for position in measured_break_opportunities(text, &fragment.style) {
        if position == 0
            || position >= text.len()
            || !text.is_char_boundary(position)
            || text[position..].starts_with('\u{200b}')
        {
            continue;
        }
        output.push(inline_text_break_opportunity(
            run_index,
            text,
            &fragment.style,
            position,
            false,
            text_break_is_min_content_eligible(text, &fragment.style, position),
        ));
    }
    if matches!(fragment.style.overflow_wrap, css::OverflowWrap::BreakWord) {
        for position in grapheme_cluster_inner_boundaries(text) {
            if position == 0
                || position >= text.len()
                || output
                    .iter()
                    .any(|opportunity| opportunity.position.byte_offset == position)
            {
                continue;
            }
            output.push(inline_text_break_opportunity(
                run_index,
                text,
                &fragment.style,
                position,
                true,
                false,
            ));
        }
    }
    output
}

fn inline_text_break_opportunity(
    run_index: usize,
    text: &str,
    style: &ComputedStyle,
    byte_offset: usize,
    emergency: bool,
    min_content: bool,
) -> InlineBreakOpportunity {
    let soft_hyphen = text[..byte_offset].ends_with('\u{00ad}');
    let hangs = style.white_space == WhiteSpace::PreWrap
        && text[..byte_offset]
            .chars()
            .next_back()
            .is_some_and(is_css_preserved_document_space);
    InlineBreakOpportunity {
        position: InlineGraphPosition {
            run_index,
            byte_offset,
        },
        kind: if soft_hyphen {
            InlineBreakKind::Hyphenation
        } else if emergency {
            InlineBreakKind::Emergency
        } else {
            InlineBreakKind::SoftWrap
        },
        priority: if soft_hyphen {
            200
        } else if emergency {
            10
        } else {
            100
        },
        trims: false,
        hangs,
        soft_hyphen,
        emergency,
        min_content,
    }
}

fn inline_break_opportunity_at_boundary(
    boundary: usize,
    runs: &[InlineParagraphRun],
) -> Option<InlineBreakOpportunity> {
    let previous = &runs[boundary - 1].item;
    let next = &runs[boundary].item;
    if inline_line_item_is_collapsible_space(next)
        || inline_line_item_is_pre_wrap_hanging_space(next)
    {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::PreservedSpace,
            priority: 220,
            trims: true,
            hangs: inline_line_item_is_pre_wrap_hanging_space(next),
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        });
    }
    if matches!(
        previous,
        InlineLineItem::Fragment(fragment)
            if inline_fragment_is_pre_wrap_hanging_space(fragment)
    ) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::PreservedSpace,
            priority: 220,
            trims: false,
            hangs: true,
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        });
    }
    if matches!(
        previous,
        InlineLineItem::Fragment(fragment)
            if fragment.style.white_space == WhiteSpace::BreakSpaces
                && fragment.text.chars().all(is_css_collapsible_whitespace)
    ) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::BreakSpaces,
            priority: 210,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        });
    }
    if matches!(
        previous,
        InlineLineItem::Fragment(fragment) if fragment.text.ends_with('\u{00ad}')
    ) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::Hyphenation,
            priority: 200,
            trims: false,
            hangs: false,
            soft_hyphen: true,
            emergency: false,
            min_content: true,
        });
    }
    if inline_line_item_is_float_marker(previous) || inline_line_item_is_float_marker(next) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::AtomicBoundary,
            priority: 110,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        });
    }
    if inline_line_item_is_css_atomic(previous) || inline_line_item_is_css_atomic(next) {
        return inline_atomic_boundary_opportunity(boundary, runs);
    }
    if let (InlineLineItem::Fragment(previous), InlineLineItem::Fragment(next)) = (previous, next) {
        return inline_fragment_boundary_opportunity(boundary, previous, next);
    }
    None
}

fn inline_line_item_is_css_atomic(item: &InlineLineItem) -> bool {
    matches!(
        item,
        InlineLineItem::Atom(atom)
            if !matches!(
                atom.content,
                InlineAtomContent::InlineEdge(_) | InlineAtomContent::Leader(_)
            )
    )
}

fn inline_atomic_boundary_opportunity(
    boundary: usize,
    runs: &[InlineParagraphRun],
) -> Option<InlineBreakOpportunity> {
    let (before, style) = inline_break_context_before_boundary(runs, boundary)?;
    let after = inline_break_context_after_boundary(runs, boundary)?;
    inline_atomic_boundary_allows_soft_wrap(&before, &after, style).then_some(
        InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::AtomicBoundary,
            priority: 120,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        },
    )
}

fn inline_break_context_before_boundary(
    runs: &[InlineParagraphRun],
    boundary: usize,
) -> Option<(String, &ComputedStyle)> {
    for run in runs[..boundary].iter().rev() {
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                return Some((fragment.text.clone(), &fragment.style));
            }
            InlineLineItem::Atom(atom) if atom.content.is_box_edge() => {}
            InlineLineItem::Atom(atom) if inline_line_item_is_css_atomic(&run.item) => {
                return Some((OBJECT_REPLACEMENT_CHARACTER.to_string(), &atom.style));
            }
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return None,
        }
    }
    None
}

fn inline_break_context_after_boundary(
    runs: &[InlineParagraphRun],
    boundary: usize,
) -> Option<String> {
    for run in &runs[boundary..] {
        match &run.item {
            InlineLineItem::Fragment(fragment) => return Some(fragment.text.clone()),
            InlineLineItem::Atom(atom) if atom.content.is_box_edge() => {}
            InlineLineItem::Atom(_) if inline_line_item_is_css_atomic(&run.item) => {
                return Some(OBJECT_REPLACEMENT_CHARACTER.to_string());
            }
            InlineLineItem::Float(_) | InlineLineItem::Atom(_) => return None,
        }
    }
    None
}

fn inline_line_item_is_float_marker(item: &InlineLineItem) -> bool {
    matches!(item, InlineLineItem::Float(_))
}

fn inline_break_opportunities_across_transparent_edges(
    runs: &[InlineParagraphRun],
) -> Vec<InlineBreakOpportunity> {
    let mut opportunities = Vec::new();
    for edge_start in 1..runs.len() {
        if !inline_line_item_is_transparent_box_edge(&runs[edge_start].item)
            || inline_line_item_is_transparent_box_edge(&runs[edge_start - 1].item)
        {
            continue;
        }
        let edge_end = (edge_start + 1..runs.len())
            .find(|index| !inline_line_item_is_transparent_box_edge(&runs[*index].item))
            .unwrap_or(runs.len());
        let Some(previous) = previous_text_fragment_before(runs, edge_start) else {
            continue;
        };
        let Some(next) = runs.get(edge_end).and_then(|run| match &run.item {
            InlineLineItem::Fragment(fragment) => Some(fragment),
            _ => None,
        }) else {
            continue;
        };
        if let Some(opportunity) = inline_fragment_boundary_opportunity(edge_start, previous, next)
        {
            opportunities.push(opportunity);
        }
    }
    opportunities
}

fn previous_text_fragment_before(
    runs: &[InlineParagraphRun],
    before_run: usize,
) -> Option<&InlineFragment> {
    for run in runs[..before_run].iter().rev() {
        match &run.item {
            InlineLineItem::Fragment(fragment) => return Some(fragment),
            InlineLineItem::Atom(atom) if atom.content.is_box_edge() => {}
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return None,
        }
    }
    None
}

fn inline_line_item_is_transparent_box_edge(item: &InlineLineItem) -> bool {
    matches!(item, InlineLineItem::Atom(atom) if atom.content.is_box_edge())
}

fn inline_fragment_boundary_allows_soft_wrap(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> bool {
    if !previous.style.white_space.allows_soft_wrap()
        || previous.text.is_empty()
        || next.text.is_empty()
    {
        return false;
    }
    let boundary = previous.text.len();
    let mut combined = String::with_capacity(previous.text.len() + next.text.len());
    combined.push_str(&previous.text);
    combined.push_str(&next.text);
    measured_break_opportunities(&combined, &previous.style)
        .binary_search(&boundary)
        .is_ok()
}

fn inline_fragment_boundary_opportunity(
    boundary: usize,
    previous: &InlineFragment,
    next: &InlineFragment,
) -> Option<InlineBreakOpportunity> {
    if !inline_fragment_boundary_allows_soft_wrap(previous, next)
        && !inline_fragment_boundary_has_tracking_opportunity(previous, next)
    {
        return None;
    }
    Some(InlineBreakOpportunity {
        position: InlineGraphPosition::at_run_start(boundary),
        kind: InlineBreakKind::SoftWrap,
        priority: 100,
        trims: false,
        hangs: false,
        soft_hyphen: false,
        emergency: false,
        min_content: inline_fragment_boundary_is_min_content_eligible(previous, next),
    })
}

fn inline_fragment_boundary_has_tracking_opportunity(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> bool {
    !previous.text.is_empty()
        && !next.text.is_empty()
        && (previous.style.used_letter_spacing() != 0.0 || next.style.used_letter_spacing() != 0.0)
}

fn inline_fragment_boundary_is_min_content_eligible(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> bool {
    let mut combined = String::with_capacity(previous.text.len() + next.text.len());
    combined.push_str(&previous.text);
    combined.push_str(&next.text);
    text_break_is_min_content_eligible(&combined, &previous.style, previous.text.len())
}
