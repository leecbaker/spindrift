use super::super::*;
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
    pub(in crate::layout) break_text: String,
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

pub(in crate::layout) fn measured_inline_items(
    items: &[MeasuredInlineItem],
) -> Vec<InlineLineItem> {
    items.iter().map(|item| item.item.clone()).collect()
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
    pub(in crate::layout) index: usize,
    pub(in crate::layout) kind: InlineBreakKind,
    pub(in crate::layout) priority: u8,
    pub(in crate::layout) trims: bool,
    pub(in crate::layout) hangs: bool,
    pub(in crate::layout) soft_hyphen: bool,
    pub(in crate::layout) emergency: bool,
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
    pub(in crate::layout) lines: Vec<InlineLineFragment>,
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
    pub(in crate::layout) contribution: InlineIntrinsicContribution,
    pub(in crate::layout) height: f32,
    pub(in crate::layout) line_count: usize,
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
    pub(in crate::layout) fn build_inline_opportunity_graph(
        &mut self,
        items: &[InlineItem],
    ) -> InlineOpportunityGraph {
        build_inline_opportunity_graph(&mut self.font_system, items)
    }
}

pub(in crate::layout) fn build_inline_opportunity_graph(
    font_system: &mut FontSystem,
    items: &[InlineItem],
) -> InlineOpportunityGraph {
    let mut runs = Vec::new();
    let mut transform_state = TextTransformState::default();
    for item in items {
        match item {
            InlineItem::Word(word) => {
                let text = transform_text_with_state(&word.text, &word.style, &mut transform_state);
                push_text_graph_runs(font_system, &mut runs, word, &text);
            }
            InlineItem::Atom(atom) => {
                transform_state.force_word_boundary();
                runs.push(InlineParagraphRun {
                    item: InlineLineItem::Atom((**atom).clone()),
                    width: atom.width,
                    shaped: None,
                    break_text: match atom.content {
                        InlineAtomContent::InlineEdge | InlineAtomContent::Leader(_) => {
                            String::new()
                        }
                        _ => OBJECT_REPLACEMENT_CHARACTER.to_string(),
                    },
                });
            }
            InlineItem::Break
            | InlineItem::Float(_)
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => {
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
    let fragment = InlineFragment {
        text: text.to_string(),
        style: word.style.clone(),
        baseline_shift: word.baseline_shift,
        link_target: word.link_target.clone(),
        mergeable: word.mergeable,
        hanging_edges: word.hanging_edges,
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
        break_text: fragment.text.clone(),
        item: InlineLineItem::Fragment(fragment),
        width,
        shaped,
    });
}

impl InlineOpportunityGraph {
    pub(in crate::layout) fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub(in crate::layout) fn line_items(
        &self,
        range: std::ops::Range<usize>,
    ) -> Vec<InlineLineItem> {
        self.runs[range]
            .iter()
            .map(|run| run.item.clone())
            .collect()
    }

    pub(in crate::layout) fn line_measured_items(
        &self,
        range: std::ops::Range<usize>,
    ) -> Vec<MeasuredInlineItem> {
        self.runs[range]
            .iter()
            .map(|run| MeasuredInlineItem {
                item: run.item.clone(),
                width: run.width,
                shaped: run.shaped.clone(),
            })
            .collect()
    }

    pub(in crate::layout) fn line_width(&self, range: std::ops::Range<usize>) -> f32 {
        self.runs[range].iter().map(|run| run.width).sum()
    }

    pub(in crate::layout) fn text(&self, range: std::ops::Range<usize>) -> String {
        self.runs[range]
            .iter()
            .map(|run| run.break_text.as_str())
            .collect()
    }

    pub(in crate::layout) fn break_opportunity_before(
        &self,
        run_index: usize,
    ) -> Option<InlineBreakOpportunity> {
        self.opportunities
            .iter()
            .copied()
            .filter(|opportunity| opportunity.index == run_index)
            .max_by_key(|opportunity| opportunity.priority)
    }

    pub(in crate::layout) fn intrinsic_contribution(
        &self,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> InlineIntrinsicContribution {
        if self.runs.is_empty() {
            return InlineIntrinsicContribution::default();
        }
        let line_items = self.line_items(0..self.runs.len());
        let hanging_widths = hanging_punctuation_widths_for_line_items(
            font_system,
            &line_items,
            block_style,
            true,
            true,
            false,
        );
        let max_content = (self.line_width(0..self.runs.len())
            - trailing_hanging_space_separator_width_for_line_items(&line_items, font_system)
            - trailing_letter_spacing_width_for_line_items(&line_items)
            - hanging_widths.start
            - hanging_widths.end)
            .max(0.0);

        let mut min_content = 0.0_f32;
        let mut current_segment = 0.0_f32;
        for (run_index, run) in self.runs.iter().enumerate() {
            match &run.item {
                InlineLineItem::Fragment(fragment) => {
                    let segment_widths = intrinsic::transformed_min_content_segment_widths(
                        font_system,
                        &fragment.text,
                        &fragment.style,
                    );
                    match segment_widths.as_slice() {
                        [] => {}
                        [width] => current_segment += *width,
                        [first, middle @ .., last] => {
                            current_segment += *first;
                            min_content = min_content.max(current_segment);
                            for width in middle {
                                min_content = min_content.max(*width);
                            }
                            current_segment = *last;
                        }
                    }
                }
                InlineLineItem::Atom(_) => {
                    current_segment += run.width;
                }
            }
            let next_run = run_index + 1;
            if next_run < self.runs.len() && self.break_opportunity_before(next_run).is_some() {
                min_content = min_content.max(current_segment);
                current_segment = 0.0;
            }
        }
        min_content = min_content.max(current_segment);

        InlineIntrinsicContribution {
            min_content,
            max_content: max_content.max(min_content),
        }
    }
}

fn inline_break_opportunities_for_runs(runs: &[InlineParagraphRun]) -> Vec<InlineBreakOpportunity> {
    let mut opportunities = Vec::new();
    for boundary in 1..runs.len() {
        let previous = &runs[boundary - 1].item;
        let next = &runs[boundary].item;
        if let Some(opportunity) = inline_break_opportunity_at_boundary(boundary, previous, next) {
            opportunities.push(opportunity);
        }
    }
    if !runs.is_empty() {
        opportunities.push(InlineBreakOpportunity {
            index: runs.len(),
            kind: InlineBreakKind::Forced,
            priority: u8::MAX,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
        });
    }
    opportunities
}

fn inline_break_opportunity_at_boundary(
    boundary: usize,
    previous: &InlineLineItem,
    next: &InlineLineItem,
) -> Option<InlineBreakOpportunity> {
    if inline_line_item_is_collapsible_space(next)
        || inline_line_item_is_pre_wrap_hanging_space(next)
    {
        return Some(InlineBreakOpportunity {
            index: boundary,
            kind: InlineBreakKind::PreservedSpace,
            priority: 220,
            trims: true,
            hangs: inline_line_item_is_pre_wrap_hanging_space(next),
            soft_hyphen: false,
            emergency: false,
        });
    }
    if matches!(
        previous,
        InlineLineItem::Fragment(fragment)
            if fragment.style.white_space == WhiteSpace::BreakSpaces
                && fragment.text.chars().all(is_css_collapsible_whitespace)
    ) {
        return Some(InlineBreakOpportunity {
            index: boundary,
            kind: InlineBreakKind::BreakSpaces,
            priority: 210,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
        });
    }
    if matches!(
        previous,
        InlineLineItem::Fragment(fragment) if fragment.text.ends_with('\u{00ad}')
    ) {
        return Some(InlineBreakOpportunity {
            index: boundary,
            kind: InlineBreakKind::Hyphenation,
            priority: 200,
            trims: false,
            hangs: false,
            soft_hyphen: true,
            emergency: false,
        });
    }
    if matches!(previous, InlineLineItem::Atom(_)) || matches!(next, InlineLineItem::Atom(_)) {
        return Some(InlineBreakOpportunity {
            index: boundary,
            kind: InlineBreakKind::AtomicBoundary,
            priority: 120,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
        });
    }
    Some(InlineBreakOpportunity {
        index: boundary,
        kind: InlineBreakKind::SoftWrap,
        priority: 100,
        trims: false,
        hangs: false,
        soft_hyphen: false,
        emergency: false,
    })
}
