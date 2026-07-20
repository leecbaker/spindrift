use super::*;
use crate::layout::block::{FloatContour, FlowExclusionKind, InitialLetterLayout};
use std::rc::Rc;

/// One normal or balanced selection in an inline graph.
///
/// CSS Text 4 balancing must use the same legal graph opportunities as normal
/// wrapping. Recording the start position makes the plan safe to apply only
/// while the real selector follows the same source stream.
/// <https://drafts.csswg.org/css-text-4/#text-wrap-style>
#[derive(Debug, Clone, Copy)]
struct BalancedLinePlanEntry {
    start: InlineGraphPosition,
    end: SelectedInlineLineEnd,
    line_index: usize,
}

/// One complete candidate for a CSS Text Level 4 balanced line group.
///
/// The candidate retains the actual unused inline space of every line.  A
/// single shared target width is insufficient when floats, indents, or other
/// line-local geometry make the available measure differ between lines.
/// <https://drafts.csswg.org/css-text-4/#text-wrap-style>
#[derive(Debug, Clone)]
struct BalancedLineCandidate {
    entries: Vec<BalancedLinePlanEntry>,
    remaining_inline_space: Vec<f32>,
}

/// Immutable inputs shared by one bounded balance-candidate search.
#[derive(Clone, Copy)]
struct BalancedLineSearch<'a, 'b> {
    graph: &'a InlineOpportunityGraph,
    context: InlineParagraphContext<'b>,
    group_end: InlineGraphPosition,
    available_widths: &'a [f32],
    ellipsis_width: f32,
    allow_emergency_breaks: bool,
}

/// Mutable result state for one balance-candidate search.
#[derive(Default)]
struct BalancedLineSearchState {
    examined: usize,
    exhausted_budget: bool,
    best: Option<BalancedLineCandidate>,
}

/// The physical block slab occupied by one selected source line while it
/// queries float exclusions.
///
/// The source line identity stays separate from its page-local placement: a
/// float may move the same selected source line below several nominal strut
/// rows. Keeping those facts together avoids using a retry count as geometry.
/// The offset is relative to the normal physical block position and the size
/// is the inherited strut or the refined used line block-size.
/// <https://www.w3.org/TR/css-inline-3/#line-boxes>
/// <https://www.w3.org/TR/css-shapes-1/#shape-outside-property>
#[derive(Clone, Copy)]
struct PhysicalLineSlab {
    line_index: usize,
    block_offset: f32,
    used_block_size: f32,
}

impl PhysicalLineSlab {
    fn new(line_index: usize, block_offset: f32, used_block_size: f32) -> Self {
        debug_assert!(block_offset >= 0.0);
        debug_assert!(used_block_size >= 0.0);
        Self {
            line_index,
            block_offset,
            used_block_size,
        }
    }

    fn inherited_strut(line_index: usize, block_offset: f32, line_height: f32) -> Self {
        Self::new(line_index, block_offset, line_height)
    }

    fn with_used_block_size(self, used_block_size: f32) -> Self {
        debug_assert!(used_block_size >= 0.0);
        Self {
            used_block_size,
            ..self
        }
    }
}

// Each candidate materializes a graph range so line-edge effects, tabs, and
// punctuation use the normal line model.  Keep the work comfortably below a
// single layout timeout; a larger search deliberately falls back to ordinary
// wrapping instead of selecting from an incomplete candidate set.
const MAX_BALANCED_LINE_CANDIDATES: usize = 2_048;

/// Return the variance of the unused inline measures in a candidate plan.
///
/// CSS Text Level 4 balances the remaining line-box inline space, not source
/// text advances.  Keeping this pure makes the comparison deterministic and
/// preserves the ordinary greedy selection on exact ties.
/// <https://drafts.csswg.org/css-text-4/#text-wrap-style>
fn balanced_remaining_space_variance(remaining_inline_space: &[f32]) -> f32 {
    if remaining_inline_space.len() < 2 {
        return 0.0;
    }
    let mean = remaining_inline_space.iter().sum::<f32>() / remaining_inline_space.len() as f32;
    remaining_inline_space
        .iter()
        .map(|space| (space - mean).powi(2))
        .sum::<f32>()
        / remaining_inline_space.len() as f32
}

pub(in crate::layout) struct SelectedInlineLines {
    pub(in crate::layout) fragments: Vec<SelectedInlineLine>,
    pub(in crate::layout) next_line_index: usize,
    /// Extra physical block progression accumulated while selecting the last
    /// source line. A collected sequence carries this into the next paragraph
    /// so explicit breaks do not make later lines query the old float slab.
    pub(in crate::layout) next_physical_block_offset: f32,
    pub(in crate::layout) has_float_side_effects: bool,
}

/// A selected inline fragment and its physical line index.
///
/// Float exclusions can make a line temporarily too narrow for content that
/// otherwise fits the containing block. Retaining the selected index lets the
/// durable sequence materialize intervening empty exclusion lines rather than
/// painting the content as overflow beside the float:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
pub(in crate::layout) struct SelectedInlineLine {
    pub(in crate::layout) fragment: InlineLineFragment,
    pub(in crate::layout) line_index: usize,
    /// Extra physical block advance before this line.
    ///
    /// Float contours can make a line first fit between normal line-height
    /// rows. This stores that CSS Shapes retry distance until the durable line
    /// sequence advances the page-local block cursor.
    pub(in crate::layout) block_before: f32,
}

/// The physical layout state of one provisional inline line.
///
/// Inline line selection happens before final line metrics are known, so it
/// advances by the computed line height just as the horizontal selector has
/// always done.  The block axis is physical y for horizontal writing and
/// physical x for vertical writing:
/// <https://www.w3.org/TR/css-inline-3/#line-layout> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy)]
struct InlineLinePhysicalPosition {
    content_left: f32,
    content_right: f32,
    cursor_y: f32,
}

/// The selected source line's identity, independent of its physical row.
///
/// Float exclusions can create empty physical rows before inline source is
/// formatted. CSS Text keys first-line indentation and `each-line` behavior to
/// the formatted line and forced-break boundary, not to those rows:
/// <https://www.w3.org/TR/css-text-3/#text-indent-property>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct SelectedLineIdentity {
    is_first_formatted_line: bool,
    starts_after_forced_break: bool,
}

/// The logical source line and its physical block placement.
///
/// A shaped float can move a selected line between normal line-height rows
/// without changing its CSS Text identity.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineLinePhysicalRow {
    line_index: usize,
    identity: SelectedLineIdentity,
    block_offset: f32,
}

/// Return the boundary after a raised or sunken initial letter that must own
/// its originating line by itself.
///
/// CSS Inline lays following ordinary inline content after the leading line
/// slots exposed above a letter when its requested size exceeds its sink.
/// Keeping that as a graph boundary (rather than moving paint with a baseline
/// offset) lets the normal record builder materialize intervening phantom
/// lines and keeps exclusion queries in source order.
/// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
fn initial_letter_source_handoff_end(
    graph: &InlineOpportunityGraph,
    start: InlineGraphPosition,
) -> Option<InlineGraphPosition> {
    if start.byte_offset != 0 {
        return None;
    }
    let run = graph.runs.get(start.run_index)?;
    let InlineLineItem::Fragment(fragment) = &run.item else {
        return None;
    };
    let (size, sink) = fragment.style().initial_letter.specified()?;
    // Raised and sunken initials expose leading line slots before their
    // following source. A drop initial remains in its originating source
    // line in every writing mode; its exclusion, rather than an artificial
    // graph boundary, selects adjacent vertical columns.
    // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
    (size > sink as f32 + INLINE_FLOAT_EPSILON)
        .then_some(InlineGraphPosition::at_run_start(start.run_index + 1))
}

/// Number of *additional* ordinary line slots between a raised/sunken
/// initial's originating line and its following source content.
///
/// The initial letter itself does not enlarge the originating line box. These
/// slots are represented by missing records, which the durable line sequence
/// turns into phantom struts while painting and fragmenting the block.
/// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
fn raised_initial_letter_leading_line_slots(fragment: &InlineLineFragment) -> usize {
    fragment
        .items()
        .iter()
        .find_map(|item| {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                return None;
            };
            let (size, sink) = fragment.style().initial_letter.specified()?;
            (size > sink as f32 + INLINE_FLOAT_EPSILON)
                .then_some((size.ceil() as u32).saturating_sub(sink).saturating_sub(1) as usize)
        })
        .unwrap_or(0)
}

/// Resolve the physical exclusion edge for an initial letter.
///
/// Horizontal initials wrap from their logical inline-start edge. In a
/// vertical or sideways flow, an initial letter instead occupies the logical
/// block-start column, which is the right edge in `*-rl` modes and the left
/// edge in `*-lr` modes.
/// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
fn initial_letter_exclusion_side(
    writing_mode: WritingMode,
    horizontal_inline_start: UsedFloatSide,
) -> UsedFloatSide {
    match writing_mode {
        WritingMode::HorizontalTb => horizontal_inline_start,
        WritingMode::VerticalRl | WritingMode::SidewaysRl => UsedFloatSide::Right,
        WritingMode::VerticalLr | WritingMode::SidewaysLr => UsedFloatSide::Left,
    }
}

impl std::ops::Deref for SelectedInlineLine {
    type Target = InlineLineFragment;

    fn deref(&self) -> &Self::Target {
        &self.fragment
    }
}

impl<'a> LayoutBuilder<'a> {
    fn inline_line_physical_position(
        &self,
        line_index: usize,
        block_style: &ComputedStyle,
    ) -> InlineLinePhysicalPosition {
        self.inline_line_physical_position_with_block_offset(line_index, block_style, 0.0)
    }

    fn inline_line_physical_position_with_block_offset(
        &self,
        line_index: usize,
        block_style: &ComputedStyle,
        block_offset: f32,
    ) -> InlineLinePhysicalPosition {
        let block_advance = block_style.line_height * line_index as f32 + block_offset;
        match block_style.writing_mode {
            WritingMode::HorizontalTb => InlineLinePhysicalPosition {
                content_left: self.content_left,
                content_right: self.content_right,
                cursor_y: self.cursor_y - block_advance,
            },
            WritingMode::VerticalRl | WritingMode::SidewaysRl => InlineLinePhysicalPosition {
                content_left: self.content_left - block_advance,
                content_right: self.content_right - block_advance,
                cursor_y: self.cursor_y,
            },
            WritingMode::VerticalLr | WritingMode::SidewaysLr => InlineLinePhysicalPosition {
                content_left: self.content_left + block_advance,
                content_right: self.content_right + block_advance,
                cursor_y: self.cursor_y,
            },
        }
    }

    pub(in crate::layout) fn inline_float_band_for_line(
        &self,
        line_index: usize,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
    ) -> InlineFloatBand {
        self.inline_float_band_for_line_with_block_offset(
            line_index,
            block_style,
            available_width,
            padding_left,
            0.0,
        )
    }

    fn inline_float_band_for_line_with_block_offset(
        &self,
        line_index: usize,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        block_offset: f32,
    ) -> InlineFloatBand {
        self.inline_float_band_for_physical_slab(
            block_style,
            available_width,
            padding_left,
            PhysicalLineSlab::inherited_strut(line_index, block_offset, block_style.line_height),
        )
    }

    /// Return the float-reduced inline band for one physical line slab.
    ///
    /// The initial selector normally knows only the inherited strut. Atomic
    /// inline boxes can enlarge that strut, including from zero, so callers
    /// that have a provisional materialized line pass its used block-size to
    /// refine the band against the full painted slab.
    /// <https://drafts.csswg.org/css-inline-3/#line-boxes>
    /// <https://drafts.csswg.org/TR/css-shapes-1/#shape-outside-property>
    fn inline_float_band_for_physical_slab(
        &self,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        slab: PhysicalLineSlab,
    ) -> InlineFloatBand {
        let position = self.inline_line_physical_position_with_block_offset(
            slab.line_index,
            block_style,
            slab.block_offset,
        );
        if block_style.writing_mode != WritingMode::HorizontalTb {
            let band = self.current_logical_float_band(
                block_style.writing_mode,
                block_style.used_direction(),
                FloatBandQuery {
                    horizontal_slab: PageInlineSpan::new(
                        position.content_left + padding_left,
                        slab.used_block_size,
                    ),
                    vertical_slab: vertical_physical_inline_span(
                        block_style.writing_mode,
                        block_style.used_direction(),
                        PageTopBlockPosition::new(position.cursor_y),
                        layout_pt(available_width),
                    ),
                },
            );
            return InlineFloatBand::new(band.inline_span.start(), band.inline_span.size());
        }
        let band =
            self.current_float_band(PageBlockSpan::new(position.cursor_y, slab.used_block_size));
        let left_offset = (band.left() - position.content_left - padding_left).max(0.0);
        let right_offset = (position.content_right - band.right()).max(0.0);
        InlineFloatBand::new(left_offset, available_width - left_offset - right_offset)
    }

    /// Return the band created by CSS floats only for initial-letter
    /// placement.
    ///
    /// An initial letter joins the shared content-wrap query after it is
    /// anchored. Reading the normal content band while anchoring would let a
    /// provisional initial-letter exclusion displace the very glyph that
    /// created it. CSS floats remain valid earlier placement geometry.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
    fn css_float_band_for_physical_slab(
        &self,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        slab: PhysicalLineSlab,
    ) -> InlineFloatBand {
        let position = self.inline_line_physical_position_with_block_offset(
            slab.line_index,
            block_style,
            slab.block_offset,
        );
        let float_context = self
            .float_contexts
            .last()
            .expect("root float context exists");
        let page_index = self.current_float_page_index();
        if block_style.writing_mode != WritingMode::HorizontalTb {
            let band = float_context.logical_band(
                block_style.writing_mode,
                block_style.used_direction(),
                page_index,
                FloatBandQuery {
                    horizontal_slab: PageInlineSpan::new(
                        position.content_left + padding_left,
                        slab.used_block_size,
                    ),
                    vertical_slab: vertical_physical_inline_span(
                        block_style.writing_mode,
                        block_style.used_direction(),
                        PageTopBlockPosition::new(position.cursor_y),
                        layout_pt(available_width),
                    ),
                },
            );
            return InlineFloatBand::new(band.inline_span.start(), band.inline_span.size());
        }
        let band = float_context.band(
            page_index,
            PageBlockSpan::new(position.cursor_y, slab.used_block_size),
            PageInlineSpan::from_edges(
                position.content_left + padding_left,
                position.content_right,
            ),
        );
        let left_offset = (band.left() - position.content_left - padding_left).max(0.0);
        let right_offset = (position.content_right - band.right()).max(0.0);
        InlineFloatBand::new(left_offset, available_width - left_offset - right_offset)
    }

    pub(in crate::layout) fn layout_mixed_inline_paragraph(
        &mut self,
        items: &[InlineItem],
        context: InlineParagraphContext<'_>,
        mut line_index: usize,
        starts_after_forced_break: bool,
        plaintext_direction_state: &mut Option<Direction>,
    ) -> InlineLayoutOutcome {
        let block_style = context.block_style;
        if context.initial_first_formatted_line
            && line_index == 0
            && block_style
                .first_letter_style
                .as_deref()
                .is_some_and(|first_letter| !first_letter.initial_letter.is_normal())
        {
            self.discard_provisional_initial_letter_exclusions(block_style);
        }
        if block_style.writing_mode == WritingMode::HorizontalTb
            && context.initial_first_formatted_line
            && line_index == 0
            && block_style
                .first_letter_style
                .as_deref()
                .is_some_and(|first_letter| !first_letter.initial_letter.is_normal())
        {
            self.clear_initial_letter_exclusions_for_new_initial(block_style);
        }
        let paragraph_start_line_index = line_index;
        let graph = self.build_inline_opportunity_graph(items, block_style);
        let graph = if context.initial_first_formatted_line && line_index == 0 {
            self.graph_with_first_letter_pseudo(&graph, block_style)
        } else {
            graph
        };
        // An initial letter affects selection of its own companion source,
        // including source separated by an explicit break.  Register a
        // graph-measured provisional exclusion before selecting any line;
        // the final selected fragment replaces it below once its used
        // margins, box edges, and contour are known.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
        if context.initial_first_formatted_line && line_index == 0 {
            self.register_provisional_initial_letter_exclusion(
                &graph,
                context,
                line_index,
                starts_after_forced_break,
                false,
            );
        }
        let selected_lines = self.select_inline_lines_from_graph(
            &graph,
            context,
            line_index,
            starts_after_forced_break,
        );
        let line_boxes = selected_lines.fragments;
        let next_line_index = selected_lines.next_line_index;
        line_index = next_line_index;
        let line_count = line_boxes.len();
        let paragraph_last_hanging_width = line_boxes
            .last()
            .map(|line_box| {
                last_hanging_punctuation_width_for_line_items(
                    &mut self.font_system,
                    &line_box.fragment.items,
                    block_style,
                )
            })
            .map(SemanticLengthExt::points)
            .unwrap_or(0.0);
        let mut records = Vec::new();
        let mut next_record_line_index = paragraph_start_line_index;
        for (offset, selected_line) in line_boxes.into_iter().enumerate() {
            while next_record_line_index < selected_line.line_index {
                records.push(InlineLineRecord {
                    paragraph_index: 0,
                    block_line_index: next_record_line_index,
                    paragraph_line_index: records.len(),
                    fragment: None,
                    is_phantom: false,
                    is_first_formatted_line: context.initial_first_formatted_line
                        && next_record_line_index == 0,
                    is_last_line_in_paragraph: false,
                    is_forced_empty: true,
                    starts_after_preserved_segment_break: false,
                    clear_after: Clear::None,
                    block_before: 0.0,
                    block_start_trim: 0.0,
                    block_end_trim: 0.0,
                    paragraph_last_hanging_width,
                    used_indent: 0.0,
                    available_width: context.available_width,
                    line_height: block_style.line_height,
                });
                next_record_line_index += 1;
            }
            let line_box = selected_line.fragment;
            let block_line_index = selected_line.line_index;
            // Atomic inline boxes contribute their margin-box block-size to
            // the containing line, including when the inherited strut is
            // zero. Text fragments are already represented by the selected
            // line metrics; adding their raw `line-height` here would undo
            // text-edge trimming.
            // <https://drafts.csswg.org/css-inline-3/#line-boxes>
            let item_line_height = line_box
                .items()
                .iter()
                // Initial letters span later lines through their exclusion
                // participant, rather than increasing this ordinary line
                // record's block advance.
                // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
                .filter(|item| {
                    !LayoutBuilder::inline_line_item_is_initial_letter(&item.item)
                        && matches!(
                            &item.item,
                            InlineLineItem::Atom(atom)
                                if !matches!(atom.content(), InlineAtomContent::InlineEdge(_))
                        )
                })
                .map(|item| inline_line_item_logical_block_size(&item.item, block_style))
                .fold(0.0_f32, f32::max);
            let line_height = line_box
                .metrics
                .height
                .max(block_style.line_height)
                .max(item_line_height);
            let used_indent = line_box.indent;
            let available_width = line_box.available_width;
            let is_phantom = inline_line_fragment_is_phantom(&line_box);
            records.push(InlineLineRecord {
                paragraph_index: 0,
                block_line_index,
                paragraph_line_index: records.len(),
                fragment: Some(line_box),
                is_phantom,
                is_first_formatted_line: context.initial_first_formatted_line
                    && block_line_index == 0,
                is_last_line_in_paragraph: offset + 1 == line_count,
                is_forced_empty: false,
                starts_after_preserved_segment_break: false,
                clear_after: Clear::None,
                block_before: selected_line.block_before,
                block_start_trim: 0.0,
                block_end_trim: 0.0,
                paragraph_last_hanging_width,
                used_indent,
                available_width,
                line_height,
            });
            next_record_line_index = block_line_index + 1;
        }
        let sequence = InlineLineSequence {
            records,
            available_width: context.available_width,
            padding_left: context.padding_left,
            hanging_indent: context.hanging_indent,
            hanging_punctuation_reserve: context.hanging_punctuation_reserve,
            fragment_text_box_trim: TextBoxLineTrim::default(),
            has_flow_side_effects: selected_lines.has_float_side_effects,
        };
        self.paint_inline_line_sequence_with_state(
            &sequence,
            block_style,
            plaintext_direction_state,
        );
        InlineLayoutOutcome {
            next_line_index: line_index,
            clamp_line_slots: sequence.records.len(),
            has_non_phantom_line: sequence.has_non_phantom_line(),
            has_flow_effects: selected_lines.has_float_side_effects || sequence.has_flow_effects(),
        }
    }

    /// Select CSS inline line fragments from an opportunity graph.
    ///
    /// CSS Inline's line construction is greedy, but every soft-wrap fallback
    /// must choose a CSS Text break opportunity. This selector walks graph
    /// boundaries instead of appending raw items and later searching strings,
    /// keeping wrapping, intrinsic sizing, and future fragmentation attached
    /// to the same reusable line-fragment model:
    /// <https://www.w3.org/TR/css-inline-3/#line-layout>,
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>, and
    /// <https://www.w3.org/TR/css-break-3/#widows-orphans>.
    pub(in crate::layout) fn select_inline_lines_from_graph(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
    ) -> SelectedInlineLines {
        self.select_inline_lines_from_graph_with_block_offset(
            graph,
            context,
            line_index,
            starts_after_forced_break,
            0.0,
        )
    }

    /// Select source lines beginning at an already-resolved physical block
    /// displacement.
    ///
    /// A durable collected sequence can contain explicit source breaks. Those
    /// breaks start a new graph but not a new physical block coordinate, so a
    /// preceding contour retry must remain part of the next graph's float
    /// query.
    pub(in crate::layout) fn select_inline_lines_from_graph_with_block_offset(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        mut line_index: usize,
        starts_after_forced_break: bool,
        initial_physical_block_offset: f32,
    ) -> SelectedInlineLines {
        if graph.is_empty() {
            return SelectedInlineLines {
                fragments: Vec::new(),
                next_line_index: line_index,
                next_physical_block_offset: initial_physical_block_offset,
                has_float_side_effects: false,
            };
        }
        // Once an inline-source float is positioned, it remains in the graph
        // as a zero-advance source-order marker while its exclusion changes
        // the available bands.  Keeping its position separate lets a retry
        // select the whole affected line against that new band instead of
        // treating preceding inline content as an in-flow prefix of the
        // float.
        let mut placed_inline_float_positions = Vec::new();
        let mut balanced_plan = self.balanced_line_plan(
            graph,
            context,
            graph.start_position(),
            line_index,
            starts_after_forced_break,
            &placed_inline_float_positions,
        );
        let mut balanced_plan_index = 0usize;
        // `white-space: normal` permits soft wrapping, but does not itself
        // create a CSS Text break inside an unbreakable word. Float placement
        // must therefore ask the shared opportunity graph for an actual
        // non-forced boundary rather than using an item's wrap-capable style
        // as a proxy. The unbreakable-float path below owns out-of-flow
        // positioning without inventing a text break:
        // <https://drafts.csswg.org/css-text-3/#line-break-details> and
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>.
        let paragraph_start_line_index = line_index;
        let mut fragments = Vec::new();
        let mut has_float_side_effects = false;
        // Physical block advance beyond the nominal `line-height`. Logical
        // line indices remain source-line bookkeeping, but float-band
        // selection must follow the used line boxes. This is observable for
        // atomic inline boxes when their containing block has `line-height:
        // 0`: subsequent boxes still start after each atom's used block-size.
        // CSS Inline Layout defines a line box from its inline-level
        // participants rather than treating the inherited strut as a cap:
        // <https://drafts.csswg.org/css-inline-3/#line-boxes>.
        let mut physical_line_block_offset = initial_physical_block_offset;
        let mut pending_block_before = 0.0;
        // A committed source line may need one float-band retry after its
        // actual atomic block size is known. Keep that retry line-local and
        // bounded: BFC fixed-point placement can replay the same paragraph,
        // and repeatedly rediscovering an equivalent atom slab must not turn
        // a visual adjustment into an unbounded layout replay.
        let mut committed_slab_retry_used = false;
        let mut start = graph.start_position();
        let graph_end = graph.end_position();
        // Keep the remaining-source query incremental. Re-scanning every
        // graph break on every selected line turns a long, float-heavy
        // paragraph into quadratic retry work.
        let soft_wrap_positions = graph
            .break_opportunities_after(graph.start_position())
            .filter(|opportunity| {
                opportunity.position < graph_end && opportunity_is_soft_wrap(*opportunity)
            })
            .map(|opportunity| opportunity.position)
            .collect::<Vec<_>>();
        let mut next_soft_wrap_position = 0usize;
        while start < graph_end {
            if context
                .block_style
                .line_clamp
                .as_ref()
                .is_some_and(|line_clamp| line_index >= line_clamp.max_lines)
            {
                break;
            }
            while start.byte_offset == 0
                && start.run_index < graph.runs.len()
                && inline_line_item_is_collapsible_space(&graph.runs[start.run_index].item)
            {
                start.run_index += 1;
            }
            if start >= graph_end {
                break;
            }
            while next_soft_wrap_position < soft_wrap_positions.len()
                && soft_wrap_positions[next_soft_wrap_position] <= start
            {
                next_soft_wrap_position += 1;
            }
            if let Some(float) = graph.float_at_position(start).cloned() {
                self.place_inline_waiting_float(&float, context, line_index);
                placed_inline_float_positions.push(start);
                has_float_side_effects = true;
                balanced_plan = None;
                balanced_plan_index = 0;
                start.run_index += 1;
                start.byte_offset = 0;
                continue;
            }
            let line_identity = SelectedLineIdentity {
                starts_after_forced_break: starts_after_forced_break
                    && line_index == paragraph_start_line_index,
                is_first_formatted_line: context.initial_first_formatted_line
                    && paragraph_start_line_index == 0
                    && fragments.is_empty(),
            };
            let committed_preceding_float =
                graph.runs[..start.run_index]
                    .iter()
                    .enumerate()
                    .any(|(run_index, run)| {
                        matches!(run.item, InlineLineItem::Float(_))
                            && placed_inline_float_positions
                                .contains(&InlineGraphPosition::at_run_start(run_index))
                    });
            let starts_initial_letter = matches!(
                graph.runs.get(start.run_index).map(|run| &run.item),
                Some(InlineLineItem::Fragment(fragment))
                    if !fragment.style().initial_letter.is_normal()
            );
            let has_provisional_initial_for_line =
                self.float_contexts.last().is_some_and(|floats| {
                    floats.shapes.iter().any(|shape| {
                        shape.kind == FlowExclusionKind::InitialLetter
                            && shape.page_index == self.current_float_page_index()
                            && shape.initial_letter.as_ref().is_some_and(|layout| {
                                layout.provisional && layout.impacted_line_range.start == line_index
                            })
                    })
                });
            if starts_initial_letter
                && committed_preceding_float
                && !has_provisional_initial_for_line
            {
                self.register_provisional_initial_letter_exclusion(
                    graph,
                    context,
                    line_index,
                    line_identity.starts_after_forced_break,
                    true,
                );
            }
            // A preceding ordinary soft break does not make the remaining
            // source wrappable. Once that break has committed, a following
            // `nowrap` (or otherwise unbreakable) segment containing a float
            // must retain its whole source range and overflow as one line.
            // A float marker is a placement boundary rather than a CSS Text
            // wrapping opportunity. Other atomic boundaries are CSS Text
            // opportunities between adjacent atomic inline-level boxes and
            // keep the remaining source wrappable.
            // <https://www.w3.org/TR/css-text-3/#white-space-property>
            let remaining_source_has_soft_wrap =
                next_soft_wrap_position < soft_wrap_positions.len();
            // Float-excluded rows consume physical block-size but do not make
            // a formatted line. Preserve this identity independently of the
            // physical line index for `text-indent` and hanging indents.
            if balanced_plan.is_none()
                && matches!(
                    context.block_style.text_wrap_style,
                    css::TextWrapStyle::Balance
                )
                && graph.runs[start.run_index..]
                    .iter()
                    .enumerate()
                    .all(|(offset, run)| {
                        !matches!(run.item, InlineLineItem::Float(_))
                            || placed_inline_float_positions.contains(
                                &InlineGraphPosition::at_run_start(start.run_index + offset),
                            )
                    })
            {
                balanced_plan = self.balanced_line_plan(
                    graph,
                    context,
                    start,
                    line_index,
                    line_identity.starts_after_forced_break,
                    &placed_inline_float_positions,
                );
            }
            let mut selected_end = balanced_plan
                .as_ref()
                .and_then(|plan| plan.get(balanced_plan_index))
                .filter(|plan| plan.start == start)
                .map(|plan| plan.end)
                .unwrap_or_else(|| {
                    self.select_inline_line_end_with_block_offset(
                        graph,
                        start,
                        context,
                        line_index,
                        line_identity,
                        physical_line_block_offset,
                    )
                });
            // The inherited strut is only a provisional float-query slab.
            // Refine every selected candidate against its complete used line
            // slab before committing a CSS Shapes band. Atomic boxes can
            // enlarge a zero-height strut, but ordinary text and markers can
            // also change the band at a shaped float contour.
            //
            // Each iteration selects a legal graph boundary for one physical
            // slab. The cap is a diagnostic guard for a pathological contour
            // alternation; normal state transitions either stabilize or move
            // the candidate to a later physical slab below.
            // <https://drafts.csswg.org/css-inline-3/#line-boxes>
            // <https://drafts.csswg.org/TR/css-shapes-1/#shape-outside-property>
            if selected_end.position > start
                && self
                    .float_contexts
                    .last()
                    .is_some_and(|floats| !floats.shapes.is_empty())
            {
                let mut converged = false;
                for _ in 0..4 {
                    let provisional = graph.materialize_line(
                        InlineGraphRange {
                            start,
                            end: selected_end.position,
                        },
                        selected_end.break_opportunity,
                        &mut self.font_system,
                        context.block_style,
                    );
                    let provisional_metrics = self.mixed_inline_line_metrics(
                        &provisional.items,
                        context.block_style,
                        provisional.content_width,
                    );
                    let block_size = used_inline_line_block_size_from_items(
                        &provisional.items,
                        provisional_metrics.height,
                        context.block_style,
                    );
                    if block_size <= INLINE_FLOAT_EPSILON {
                        converged = true;
                        break;
                    }
                    let refined = self.select_inline_line_end_with_block_span(
                        graph,
                        start,
                        context,
                        line_index,
                        line_identity,
                        PhysicalLineSlab::inherited_strut(
                            line_index,
                            physical_line_block_offset,
                            context.block_style.line_height,
                        )
                        .with_used_block_size(block_size),
                    );
                    if refined.position == selected_end.position
                        && refined.break_opportunity == selected_end.break_opportunity
                    {
                        converged = true;
                        break;
                    }
                    selected_end = refined;
                    if selected_end.position <= start {
                        converged = true;
                        break;
                    }
                }
                debug_assert!(
                    converged,
                    "selected inline candidate did not converge with its physical float slab"
                );
            }
            let initial_source_handoff_end = initial_letter_source_handoff_end(graph, start);
            if let Some(initial_end) = initial_source_handoff_end {
                selected_end = SelectedInlineLineEnd {
                    position: initial_end,
                    break_opportunity: graph.break_opportunity_at(initial_end),
                };
                // A balance plan selected against the unsplit source is no
                // longer applicable after the initial-letter-specific first
                // line boundary.
                balanced_plan = None;
                balanced_plan_index = 0;
            }
            // Reserve the truncation marker while selecting the final clamped
            // line.  Removing materialized items afterward loses the graph
            // source ranges that own CSS Text Phase II effects, and can also
            // select a different break from the one that actually fits with
            // the marker.
            // <https://drafts.csswg.org/css-overflow-3/#line-clamp>
            let is_final_clamped_line = context
                .block_style
                .line_clamp
                .as_ref()
                .is_some_and(|line_clamp| line_index + 1 == line_clamp.max_lines);
            let mut final_clamp_marker_replaces_source = false;
            if is_final_clamped_line && selected_end.position < graph_end {
                let ellipsis_width = self.line_clamp_marker_width(context.block_style);
                if ellipsis_width > 0.0 {
                    let band = self.inline_float_band_for_line(
                        line_index,
                        context.block_style,
                        context.available_width,
                        context.padding_left,
                    );
                    let line_indent = used_line_indent_for_formatted_line(
                        line_identity.is_first_formatted_line,
                        line_identity.starts_after_forced_break,
                        context.hanging_indent,
                        context.block_style,
                        band.width(),
                    );
                    let marker_available_width =
                        (band.width() - line_indent - ellipsis_width).max(0.0);
                    let marker_selected = self.select_inline_line_end_for_width(
                        graph,
                        start,
                        context.block_style,
                        marker_available_width,
                        line_index,
                    );
                    let marker_selection_fits = marker_selected.position > start
                        && self.balanced_line_fit_width(
                            graph,
                            start,
                            marker_selected,
                            context.block_style,
                            line_index,
                        ) <= marker_available_width + INLINE_FLOAT_EPSILON;
                    if marker_selection_fits {
                        selected_end = marker_selected;
                    } else {
                        // An unbreakable source segment may not fit beside
                        // the block ellipsis at all. In that case the marker
                        // replaces the final line's source rather than
                        // allowing the ordinary no-fit fallback to overflow
                        // underneath it.
                        // <https://drafts.csswg.org/css-overflow-4/#line-clamp>
                        selected_end = SelectedInlineLineEnd {
                            position: start,
                            break_opportunity: None,
                        };
                        final_clamp_marker_replaces_source = true;
                    }
                }
            }
            // A paragraph without soft-wrap opportunities can still contain
            // forced breaks. Those breaks delimit real line boxes and must be
            // honored by line clamping rather than collapsing the remaining
            // paragraph into one unbounded line.
            let selected_forced_break = selected_end
                .break_opportunity
                .is_some_and(|opportunity| matches!(opportunity.kind, InlineBreakKind::Forced));
            let mut end = if final_clamp_marker_replaces_source {
                start
            } else if (!remaining_source_has_soft_wrap
                && !selected_forced_break
                && initial_source_handoff_end.is_none())
                || selected_end.position <= start
            {
                graph_end
            } else {
                selected_end.position.min(graph_end)
            };
            let end_is_unplaced_float_boundary =
                selected_end.break_opportunity.is_some_and(|opportunity| {
                    matches!(opportunity.kind, InlineBreakKind::FloatPlacement)
                        && graph.float_at_position(opportunity.position).is_some()
                        && !placed_inline_float_positions.contains(&opportunity.position)
                });
            if !end_is_unplaced_float_boundary {
                end = line_end_extended_over_adjacent_inline_float_markers(graph, end);
            }
            // A wrappable selected range may fit the unexcluded containing
            // block while exceeding this line's temporary float band. CSS 2.2
            // then moves the line below the float instead of painting inline
            // overflow next to the exclusion. The decision is local to this
            // range: a preceding normal run must not make later `nowrap`
            // content defer instead of overflowing beside the float. Preserve
            // the skipped line so paint and subsequent float-band queries
            // advance together.
            // <https://www.w3.org/TR/CSS22/visuren.html#floats>
            // A zero inherited strut does not make an atomic inline's line
            // zero-height. Query the retry band with the selected range's
            // actual used block slab, otherwise an inline-block can be
            // selected beside a float at `line-height: 0` and only discover
            // its collision after it has already been committed.
            // <https://drafts.csswg.org/css-inline-3/#line-boxes>
            let band = self.inline_float_band_for_line_with_block_offset(
                line_index,
                context.block_style,
                context.available_width,
                context.padding_left,
                physical_line_block_offset,
            );
            let line_indent = used_line_indent_for_formatted_line(
                line_identity.is_first_formatted_line,
                line_identity.starts_after_forced_break,
                context.hanging_indent,
                context.block_style,
                band.width(),
            );
            let containing_indent = used_line_indent_for_formatted_line(
                line_identity.is_first_formatted_line,
                line_identity.starts_after_forced_break,
                context.hanging_indent,
                context.block_style,
                context.available_width,
            );
            let measures = InlineSelectionMeasures::new(context.available_width, band);
            let current_available_width = measures.band_after_indent(line_indent);
            let full_available_width = measures.containing_after_indent(containing_indent);
            // A selected range that fits the containing block but not this
            // float-reduced band must retry from the next physical slab.
            // CSS 2.2's float retry is line-box placement, not a synthetic
            // break inside a word or atomic inline participant.
            if context.block_style.allows_soft_wrap()
                && remaining_source_has_soft_wrap
                && end > start
                && current_available_width + INLINE_FLOAT_EPSILON < full_available_width
                && let materialized = graph.materialize_line(
                    InlineGraphRange { start, end },
                    (end < graph_end)
                        .then_some(selected_end.break_opportunity)
                        .flatten(),
                    &mut self.font_system,
                    context.block_style,
                )
            {
                // A provisional initial-letter exclusion selects the source
                // beside the initial.  Its own isolated source advance is
                // not companion content competing for that narrowed band:
                // testing it there would make a wide margin box defer its
                // originating line below itself.
                // <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
                let initial_source_advance = (line_identity.is_first_formatted_line
                    && start.byte_offset == 0)
                    .then(|| {
                        let initial_index = materialized.items.iter().position(|item| {
                            matches!(
                                item.as_ref(),
                                InlineLineItem::Fragment(fragment)
                                    if !fragment.style().initial_letter.is_normal()
                            )
                        })?;
                        let leading_pseudo_width = materialized.items[..initial_index]
                            .iter()
                            .rev()
                            .take_while(|item| {
                                matches!(
                                    item.as_ref(),
                                    InlineLineItem::Fragment(fragment)
                                        if fragment.first_letter_pseudo_role()
                                            == FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace
                                )
                            })
                            .map(|item| item.width)
                            .sum::<f32>();
                        Some(leading_pseudo_width + materialized.items[initial_index].width)
                    })
                    .flatten()
                    .unwrap_or(0.0);
                let companion_fitting_width =
                    (materialized.fitting_width - initial_source_advance).max(0.0);
                let needs_band_retry = companion_fitting_width
                    > current_available_width + INLINE_FLOAT_EPSILON
                    && companion_fitting_width <= full_available_width + INLINE_FLOAT_EPSILON;
                // In vertical writing, a dropped initial owns the originating
                // logical block column even when its companion happens to fit
                // in the remaining vertical measure. The companion is placed
                // in the adjacent block column; treating this solely as a
                // width retry leaves it painted on top of the initial's own
                // column whenever the remaining height is generous.
                // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
                let needs_vertical_initial_handoff = matches!(
                    context.block_style.writing_mode,
                    WritingMode::VerticalRl | WritingMode::SidewaysRl
                ) && line_identity.is_first_formatted_line
                    && initial_source_advance > INLINE_FLOAT_EPSILON;
                if needs_band_retry || needs_vertical_initial_handoff {
                    if context.block_style.writing_mode == WritingMode::HorizontalTb {
                        let metrics = self.mixed_inline_line_metrics(
                            &materialized.items,
                            context.block_style,
                            materialized.content_width,
                        );
                        let line_height = used_inline_line_block_size_from_items(
                            &materialized.items,
                            metrics.height,
                            context.block_style,
                        );
                        let position = self.inline_line_physical_position_with_block_offset(
                            line_index,
                            context.block_style,
                            physical_line_block_offset,
                        );
                        let line_indent = used_line_indent_for_formatted_line(
                            line_identity.is_first_formatted_line,
                            line_identity.starts_after_forced_break,
                            context.hanging_indent,
                            context.block_style,
                            context.available_width,
                        );
                        let required_width = companion_fitting_width + line_indent;
                        let inline_span = PageInlineSpan::from_edges(
                            position.content_left + context.padding_left,
                            position.content_right,
                        );
                        if let Some(next_top) =
                            self.float_contexts.last().and_then(|float_context| {
                                float_context.next_content_slab_with_width(
                                    self.current_float_page_index(),
                                    PageBlockSpan::new(position.cursor_y, line_height),
                                    inline_span,
                                    required_width,
                                )
                            })
                        {
                            // This pre-materialization path cannot safely
                            // advance the source row: its inherited strut can
                            // be smaller than an atomic participant and a BFC
                            // fixed-point replay may select a different used
                            // slab. The committed-fragment retry below owns
                            // the physical advance once that slab is known.
                            let block_advance = next_top.points() - position.cursor_y;
                            if block_advance > INLINE_FLOAT_EPSILON {
                                physical_line_block_offset += block_advance;
                                pending_block_before += block_advance;
                                balanced_plan = None;
                                continue;
                            }
                        }
                    } else {
                        let metrics = self.mixed_inline_line_metrics(
                            &materialized.items,
                            context.block_style,
                            materialized.content_width,
                        );
                        let slab_width = used_inline_line_block_size_from_items(
                            &materialized.items,
                            metrics.height,
                            context.block_style,
                        );
                        let position = self.inline_line_physical_position_with_block_offset(
                            line_index,
                            context.block_style,
                            physical_line_block_offset,
                        );
                        let starting_slab = PageInlineSpan::new(
                            position.content_left + context.padding_left,
                            slab_width,
                        );
                        let vertical_inline_span = vertical_physical_inline_span(
                            context.block_style.writing_mode,
                            context.block_style.used_direction(),
                            PageTopBlockPosition::new(position.cursor_y),
                            layout_pt(context.available_width),
                        );
                        let required_width = companion_fitting_width + containing_indent;
                        let next_left = if needs_vertical_initial_handoff {
                            // The initial's wrapping box occupies its own
                            // block-start column. Move the companion past the
                            // physical block-end edge of that box instead of
                            // asking a width query which can legitimately fit
                            // beside it in the same column.
                            self.float_contexts.last().and_then(|float_context| {
                                float_context
                                    .shapes
                                    .iter()
                                    .filter(|shape| {
                                        shape.kind == FlowExclusionKind::InitialLetter
                                            && shape.page_index == self.current_float_page_index()
                                            && shape.rect.x() + shape.rect.width()
                                                > starting_slab.left_x() + INLINE_FLOAT_EPSILON
                                            && shape.rect.x()
                                                < starting_slab.right_x() - INLINE_FLOAT_EPSILON
                                    })
                                    .map(|shape| match context.block_style.writing_mode {
                                        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                                            PageInlinePosition::new(
                                                shape.rect.x() - starting_slab.width(),
                                            )
                                        }
                                        WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                                            PageInlinePosition::new(
                                                shape.rect.x() + shape.rect.width(),
                                            )
                                        }
                                        WritingMode::HorizontalTb => unreachable!(),
                                    })
                                    .find(|candidate| match context.block_style.writing_mode {
                                        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                                            candidate.points()
                                                < starting_slab.left_x() - INLINE_FLOAT_EPSILON
                                        }
                                        WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                                            candidate.points()
                                                > starting_slab.left_x() + INLINE_FLOAT_EPSILON
                                        }
                                        WritingMode::HorizontalTb => false,
                                    })
                            })
                        } else {
                            self.float_contexts.last().and_then(|float_context| {
                                float_context.next_vertical_content_slab_with_width(
                                    context.block_style.writing_mode,
                                    context.block_style.used_direction(),
                                    self.current_float_page_index(),
                                    starting_slab,
                                    vertical_inline_span,
                                    required_width,
                                )
                            })
                        };
                        if let Some(next_left) = next_left {
                            let block_advance = match context.block_style.writing_mode {
                                WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                                    next_left.points() - starting_slab.left_x()
                                }
                                WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                                    starting_slab.left_x() - next_left.points()
                                }
                                WritingMode::HorizontalTb => unreachable!(),
                            };
                            if block_advance > INLINE_FLOAT_EPSILON {
                                physical_line_block_offset += block_advance;
                                pending_block_before += block_advance;
                                balanced_plan = None;
                                continue;
                            }
                        }
                    }
                    line_index += 1;
                    balanced_plan = None;
                    continue;
                }
            }
            let selected_range = InlineGraphRange { start, end };
            if !remaining_source_has_soft_wrap
                && let Some(fragment) = self.try_select_unbreakable_line_with_inline_floats(
                    graph,
                    InlineGraphRange {
                        start,
                        end: graph_end,
                    },
                    SelectedInlineLineEnd {
                        position: graph_end,
                        break_opportunity: graph.break_opportunity_at(graph_end),
                    },
                    context,
                    InlineLinePhysicalRow {
                        line_index,
                        identity: line_identity,
                        block_offset: physical_line_block_offset,
                    },
                )
            {
                physical_line_block_offset +=
                    used_inline_line_block_advance(&fragment, context.block_style);
                fragments.push(SelectedInlineLine {
                    fragment,
                    line_index,
                    block_before: std::mem::take(&mut pending_block_before),
                });
                has_float_side_effects = true;
                line_index += 1;
                balanced_plan_index += 1;
                start = end;
                continue;
            }
            if let Some(float_position) = graph.first_float_position_in_range(selected_range)
                && !placed_inline_float_positions.contains(&float_position)
            {
                if matches!(
                    context.block_style.text_wrap_style,
                    css::TextWrapStyle::Balance
                ) && placed_inline_float_positions.is_empty()
                    && let Some(float) = graph.float_at_position(float_position).cloned()
                {
                    // Balancing must choose the whole affected line against
                    // the float's exclusion band. Position the float, record
                    // its zero-advance graph marker, and retry from the same
                    // line start instead of committing the preceding source
                    // as an in-flow prefix.
                    // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
                    self.place_inline_waiting_float(&float, context, line_index);
                    placed_inline_float_positions.push(float_position);
                    has_float_side_effects = true;
                    balanced_plan = None;
                    balanced_plan_index = 0;
                    continue;
                }
                if float_position <= start {
                    if let Some(float) = graph.float_at_position(float_position).cloned() {
                        self.place_inline_waiting_float(&float, context, line_index);
                        placed_inline_float_positions.push(float_position);
                        has_float_side_effects = true;
                    }
                    start = InlineGraphPosition::at_run_start(float_position.run_index + 1);
                    continue;
                }
                // A float following in-flow text is positioned after that
                // text when it fits the remaining current-line band.  Placing
                // it against the whole band first would incorrectly let its
                // exclusion shift preceding source content.  If it does not
                // fit, the prefix is committed and the marker is retried at
                // the next line top.
                // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
                let break_opportunity = Some(InlineBreakOpportunity {
                    position: float_position,
                    kind: InlineBreakKind::FloatPlacement,
                    priority: 110,
                    trims: false,
                    hangs: false,
                    soft_hyphen: false,
                    discretionary: None,
                    emergency: false,
                    min_content: true,
                });
                let mut prefix = self.materialize_inline_line_fragment(
                    graph,
                    InlineGraphRange {
                        start,
                        end: float_position,
                    },
                    context,
                    InlineLinePhysicalRow {
                        line_index,
                        identity: line_identity,
                        block_offset: physical_line_block_offset,
                    },
                    break_opportunity,
                );
                let placement_snapshot = self.snapshot();
                if let Some(placement) = self.try_place_inline_float_on_current_line(
                    graph,
                    float_position,
                    prefix.metrics.width,
                    context,
                    line_index,
                    line_identity,
                ) {
                    placed_inline_float_positions.push(float_position);
                    has_float_side_effects = true;
                    let suffix_start =
                        InlineGraphPosition::at_run_start(float_position.run_index + 1);
                    let suffix_is_empty =
                        graph_remaining_after_position_is_trimmable(graph, suffix_start);
                    prefix.suppress_float_adjust = true;
                    if let Some(combined) = self.try_select_inline_float_same_line_suffix(
                        graph,
                        prefix.clone(),
                        float_position,
                        suffix_start,
                        placement,
                        context,
                        line_index,
                    ) {
                        let end = combined.end;
                        physical_line_block_offset +=
                            used_inline_line_block_advance(&combined.fragment, context.block_style);
                        fragments.push(SelectedInlineLine {
                            fragment: combined.fragment,
                            line_index,
                            block_before: std::mem::take(&mut pending_block_before),
                        });
                        line_index += 1;
                        balanced_plan_index += 1;
                        start = end;
                        continue;
                    }
                    if !suffix_is_empty && !placement.fits_remaining_band() {
                        self.restore(placement_snapshot);
                        prefix.suppress_float_adjust = false;
                        physical_line_block_offset +=
                            used_inline_line_block_advance(&prefix, context.block_style);
                        fragments.push(SelectedInlineLine {
                            fragment: prefix,
                            line_index,
                            block_before: std::mem::take(&mut pending_block_before),
                        });
                        line_index += 1;
                        balanced_plan_index += 1;
                        start = float_position;
                        continue;
                    }
                    physical_line_block_offset +=
                        used_inline_line_block_advance(&prefix, context.block_style);
                    fragments.push(SelectedInlineLine {
                        fragment: prefix,
                        line_index,
                        block_before: std::mem::take(&mut pending_block_before),
                    });
                    line_index += 1;
                    balanced_plan_index += 1;
                    start = suffix_start;
                    continue;
                }
                let break_opportunity = Some(InlineBreakOpportunity {
                    position: float_position,
                    kind: InlineBreakKind::FloatPlacement,
                    priority: 110,
                    trims: false,
                    hangs: false,
                    soft_hyphen: false,
                    discretionary: None,
                    emergency: false,
                    min_content: true,
                });
                let mut fragment = self.materialize_inline_line_fragment(
                    graph,
                    InlineGraphRange {
                        start,
                        end: float_position,
                    },
                    context,
                    InlineLinePhysicalRow {
                        line_index,
                        identity: line_identity,
                        block_offset: physical_line_block_offset,
                    },
                    break_opportunity,
                );
                self.register_initial_letter_exclusion_for_line(
                    &mut fragment,
                    context,
                    line_index,
                    physical_line_block_offset,
                    false,
                );
                physical_line_block_offset +=
                    used_inline_line_block_advance(&fragment, context.block_style);
                fragments.push(SelectedInlineLine {
                    fragment,
                    line_index,
                    block_before: std::mem::take(&mut pending_block_before),
                });
                line_index += 1;
                balanced_plan_index += 1;
                start = float_position;
                continue;
            }
            let break_opportunity = selected_end
                .break_opportunity
                .filter(|opportunity| opportunity.position == end && end < graph_end);
            let mut fragment = self.materialize_inline_line_fragment(
                graph,
                selected_range,
                context,
                InlineLinePhysicalRow {
                    line_index,
                    identity: line_identity,
                    block_offset: physical_line_block_offset,
                },
                break_opportunity,
            );
            // The selected fragment can contain an atomic inline whose used
            // block size is greater than the inherited strut. Re-query the
            // active float band with that real slab before committing the
            // line. Without this second phase, a `line-height: 0` inline
            // block is selected at a zero-height float boundary and paints
            // beside a full-width float that it actually overlaps.
            // <https://www.w3.org/TR/CSS22/visuren.html#floats>
            // <https://drafts.csswg.org/css-inline-3/#line-boxes>
            if context.block_style.allows_soft_wrap()
                && remaining_source_has_soft_wrap
                && context.block_style.writing_mode == WritingMode::HorizontalTb
                && !committed_slab_retry_used
                && self
                    .float_contexts
                    .last()
                    .is_some_and(|float_context| !float_context.shapes.is_empty())
            {
                let used_block_size = used_inline_line_block_size_from_items(
                    &fragment.items,
                    fragment.metrics.height,
                    context.block_style,
                );
                let actual_band = self.inline_float_band_for_physical_slab(
                    context.block_style,
                    context.available_width,
                    context.padding_left,
                    PhysicalLineSlab::new(line_index, physical_line_block_offset, used_block_size),
                );
                let actual_indent = used_line_indent_for_formatted_line(
                    line_identity.is_first_formatted_line,
                    line_identity.starts_after_forced_break,
                    context.hanging_indent,
                    context.block_style,
                    actual_band.width(),
                );
                let actual_available_width =
                    InlineSelectionMeasures::new(context.available_width, actual_band)
                        .band_after_indent(actual_indent);
                let full_indent = used_line_indent_for_formatted_line(
                    line_identity.is_first_formatted_line,
                    line_identity.starts_after_forced_break,
                    context.hanging_indent,
                    context.block_style,
                    context.available_width,
                );
                let full_available_width = (context.available_width - full_indent).max(0.0);
                if fragment.metrics.width > actual_available_width + INLINE_FLOAT_EPSILON
                    && fragment.metrics.width <= full_available_width + INLINE_FLOAT_EPSILON
                {
                    let position = self.inline_line_physical_position_with_block_offset(
                        line_index,
                        context.block_style,
                        physical_line_block_offset,
                    );
                    let inline_span = PageInlineSpan::from_edges(
                        position.content_left + context.padding_left,
                        position.content_right,
                    );
                    if let Some(next_top) = self.float_contexts.last().and_then(|float_context| {
                        float_context.next_content_slab_with_width(
                            self.current_float_page_index(),
                            PageBlockSpan::new(position.cursor_y, used_block_size),
                            inline_span,
                            fragment.metrics.width + full_indent,
                        )
                    }) {
                        let block_advance = position.cursor_y - next_top.points();
                        if block_advance > INLINE_FLOAT_EPSILON {
                            physical_line_block_offset += block_advance;
                            pending_block_before += block_advance;
                            committed_slab_retry_used = true;
                            balanced_plan = None;
                            continue;
                        }
                    }
                }
            }
            // A retry after placing an inline-source float already selected
            // this fragment against that float's exclusion band. Preserve the
            // stored band at paint time instead of applying the live band a
            // second time; that distinction is essential when a left float
            // shifts an RTL line's physical left edge.
            if placed_inline_float_positions
                .iter()
                .any(|position| *position >= selected_range.start && *position < selected_range.end)
            {
                fragment.suppress_float_adjust = true;
            }
            self.register_initial_letter_exclusion_for_line(
                &mut fragment,
                context,
                line_index,
                physical_line_block_offset,
                false,
            );
            let leading_initial_letter_slots = raised_initial_letter_leading_line_slots(&fragment);
            physical_line_block_offset +=
                used_inline_line_block_advance(&fragment, context.block_style);
            fragments.push(SelectedInlineLine {
                fragment,
                line_index,
                block_before: std::mem::take(&mut pending_block_before),
            });
            // The current line owns the initial letter. The following normal
            // source begins after the exposed leading slots; leaving the
            // indices sparse causes durable record construction to emit the
            // corresponding phantom struts.
            line_index += 1 + leading_initial_letter_slots;
            balanced_plan_index += 1;
            start = end;
        }
        if context
            .block_style
            .line_clamp
            .as_ref()
            .is_some_and(|line_clamp| line_index == line_clamp.max_lines)
            && (start < graph_end
                || context
                    .block_style
                    .line_clamp
                    .as_ref()
                    .is_some_and(|line_clamp| line_clamp.continues_after_clamp_point))
            && let Some(fragment) = fragments.last_mut()
        {
            self.append_line_clamp_ellipsis(
                &mut fragment.fragment,
                context,
                line_index.saturating_sub(1),
            );
        }
        SelectedInlineLines {
            fragments,
            next_line_index: line_index,
            next_physical_block_offset: physical_line_block_offset,
            has_float_side_effects,
        }
    }

    /// Produce a same-line-count balance plan from the legal graph breaks.
    ///
    /// The normal greedy plan fixes the required line count. For each
    /// forced-break-separated group of at most ten lines, find the narrowest
    /// shared selection measure that still produces that count, then use the
    /// graph's legal opportunities at that measure. This is intentionally a
    /// conservative subset of CSS Text 4: it preserves normal wrapping for
    /// groups over ten lines, while a bounded exhaustive search evaluates
    /// every legal sequence for smaller groups against each line's actual
    /// available inline measure.
    /// <https://drafts.csswg.org/css-text-4/#text-wrap-style>
    fn balanced_line_plan(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        plan_start: InlineGraphPosition,
        first_line_index: usize,
        starts_after_forced_break: bool,
        placed_inline_float_positions: &[InlineGraphPosition],
    ) -> Option<Vec<BalancedLinePlanEntry>> {
        if !matches!(
            context.block_style.text_wrap_style,
            css::TextWrapStyle::Balance
        ) || graph
            .runs
            .iter()
            .enumerate()
            .skip(plan_start.run_index)
            .any(|(run_index, run)| {
                matches!(run.item, InlineLineItem::Float(_))
                    && !placed_inline_float_positions
                        .contains(&InlineGraphPosition::at_run_start(run_index))
            })
        {
            return None;
        }

        let graph_end = graph.end_position();
        let mut normal = Vec::new();
        let mut start = plan_start;
        let mut line_index = first_line_index;
        while start < graph_end {
            while start.byte_offset == 0
                && start.run_index < graph.runs.len()
                && inline_line_item_is_collapsible_space(&graph.runs[start.run_index].item)
            {
                start.run_index += 1;
            }
            if start >= graph_end {
                break;
            }
            let starts_after_forced_break =
                starts_after_forced_break && line_index == first_line_index;
            let end = self.select_inline_line_end(
                graph,
                start,
                context,
                line_index,
                SelectedLineIdentity {
                    is_first_formatted_line: first_line_index == 0 && normal.is_empty(),
                    starts_after_forced_break,
                },
            );
            if end.position <= start {
                return None;
            }
            normal.push(BalancedLinePlanEntry {
                start,
                end,
                line_index,
            });
            start = end.position;
            line_index += 1;
        }
        if normal.len() < 2 {
            return Some(normal);
        }

        let mut clamped_has_overflow = false;
        let mut clamp_source_end = None;
        if let Some(line_clamp) = &context.block_style.line_clamp {
            clamped_has_overflow = normal.len() > line_clamp.max_lines
                || (normal.len() == line_clamp.max_lines && line_clamp.continues_after_clamp_point);
            if clamped_has_overflow {
                // Apply the clamp marker to the ordinary final surviving
                // line before choosing the source boundary to balance. The
                // marker can either shorten that line or replace an
                // unbreakable segment entirely. Using its unmarked start
                // would discard source that is visible beside the marker;
                // using its unmarked end would retain source displaced by it.
                // <https://drafts.csswg.org/css-overflow-4/#line-clamp>
                clamp_source_end =
                    normal
                        .get(line_clamp.max_lines.saturating_sub(1))
                        .map(|entry| {
                            let ellipsis_width = self.line_clamp_marker_width(context.block_style);
                            if ellipsis_width <= 0.0 {
                                return entry.end.position;
                            }
                            let band = self.inline_float_band_for_line(
                                entry.line_index,
                                context.block_style,
                                context.available_width,
                                context.padding_left,
                            );
                            let available_width = (band.width()
                                - used_line_indent(
                                    entry.line_index,
                                    false,
                                    context.hanging_indent,
                                    context.block_style,
                                    band.width(),
                                )
                                - ellipsis_width)
                                .max(0.0);
                            let marker_end = self.select_inline_line_end_for_width(
                                graph,
                                entry.start,
                                context.block_style,
                                available_width,
                                entry.line_index,
                            );
                            if marker_end.position > entry.start
                                && self.balanced_line_fit_width(
                                    graph,
                                    entry.start,
                                    marker_end,
                                    context.block_style,
                                    entry.line_index,
                                ) <= available_width + INLINE_FLOAT_EPSILON
                            {
                                marker_end.position
                            } else {
                                entry.start
                            }
                        });
            }
            normal.truncate(line_clamp.max_lines);
        }

        let mut output = normal.clone();
        let mut group_start = 0usize;
        while group_start < normal.len() {
            let group_end = normal[group_start..]
                .iter()
                .position(|entry| {
                    entry.end.break_opportunity.is_some_and(|opportunity| {
                        matches!(opportunity.kind, InlineBreakKind::Forced)
                    })
                })
                .map_or(normal.len(), |offset| group_start + offset + 1);
            self.balance_line_plan_group(
                graph,
                context,
                &normal,
                &mut output,
                group_start..group_end,
                clamped_has_overflow && group_end == normal.len(),
                (clamped_has_overflow && group_end == normal.len())
                    .then_some(clamp_source_end)
                    .flatten(),
            );
            group_start = group_end;
        }
        Some(output)
    }

    /// Synthesize the ellipsis into the final surviving clamped line.
    ///
    /// The line record remains graph-backed, but the truncation marker is a
    /// real shaped fragment so its advance participates in the final line's
    /// alignment and painting.
    /// <https://drafts.csswg.org/css-overflow-3/#propdef-line-clamp>
    fn append_line_clamp_ellipsis(
        &mut self,
        fragment: &mut InlineLineFragment,
        context: InlineParagraphContext<'_>,
        _line_index: usize,
    ) {
        let Some(line_clamp) = &context.block_style.line_clamp else {
            return;
        };
        let marker = match &line_clamp.ellipsis {
            css::BlockEllipsis::Auto => "…",
            css::BlockEllipsis::None => return,
            css::BlockEllipsis::String(marker) if marker.is_empty() => return,
            css::BlockEllipsis::String(marker) => marker,
        };
        // The block ellipsis belongs to an anonymous inline under the clamp
        // container's root inline box. It must not inherit the final source
        // fragment's font or decoration.
        let style = context.block_style.clone();
        let baseline_shift = 0.0;
        let shaped = self
            .font_system
            .shape_unwrapped_line(marker, &style, context.block_style.line_height)
            .map(std::rc::Rc::new);
        let ellipsis_width = shaped
            .as_deref()
            .map(ShapedInlineLine::advance_width)
            .unwrap_or(0.0);
        let mut items = fragment.items().to_vec();
        // A clamp marker replaces discarded source at the final line edge.
        // In particular, a collapsed separator selected only to reach the
        // following overflow source must not remain between the visible text
        // and the marker. Ordinary painting retains Phase II source ranges,
        // but the marker is a new replacement boundary rather than a source
        // line edge.
        // <https://drafts.csswg.org/css-overflow-4/#line-clamp>
        let mut retained_effects = Vec::with_capacity(fragment.edge_effects.source_effects.len());
        for effect in fragment.edge_effects.source_effects.iter() {
            if effect.kind != InlineLineEdgeEffectKind::CollapsedEndTrim {
                retained_effects.push(effect.clone());
                continue;
            }
            let Some(item) = items.get_mut(effect.item_index) else {
                continue;
            };
            let InlineLineItem::Fragment(source) = &mut item.item else {
                continue;
            };
            if effect.source_range.end != source.text().len()
                || !source.text().is_char_boundary(effect.source_range.start)
            {
                continue;
            }
            source.set_text(std::rc::Rc::<str>::from(
                &source.text()[..effect.source_range.start],
            ));
            source.set_preserves_source_shaping(false);
            remeasure_materialized_item(item, &mut self.font_system);
        }
        fragment.edge_effects.source_effects = std::rc::Rc::from(retained_effects);
        fragment.edge_effects.collapsed_end_trim_width = 0.0;
        items.push(MeasuredInlineItem {
            item: InlineLineItem::Fragment(InlineFragment::new(
                marker,
                style,
                baseline_shift,
                None,
                false,
                InlineTextSource::Generated,
                false,
                InlineHangingEdges::default(),
                Vec::new(),
            )),
            width: ellipsis_width,
            shaped,
        });
        // The source line's width already incorporates its selected Phase II
        // trimming and hanging effects.  The marker is new used content, so
        // extend that width instead of recomputing from raw source items.
        let content_width = fragment.metrics.width + ellipsis_width;
        fragment.metrics =
            self.mixed_inline_line_metrics(&items, context.block_style, content_width);
        fragment.items = std::rc::Rc::from(items.into_boxed_slice());
        fragment.text = std::rc::Rc::from(text_for_measured_items(fragment.items()));
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The immutable paragraph inputs and mutable selected-plan output have distinct ownership."
    )]
    fn balance_line_plan_group(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        normal: &[BalancedLinePlanEntry],
        output: &mut [BalancedLinePlanEntry],
        group_range: std::ops::Range<usize>,
        includes_clamp_ellipsis: bool,
        clamp_source_end: Option<InlineGraphPosition>,
    ) {
        let group_start = group_range.start;
        let group_end = group_range.end;
        let line_count = group_end - group_start;
        if !(2..=10).contains(&line_count) {
            return;
        }
        let group = &normal[group_start..group_end];
        let mut available_widths = Vec::with_capacity(line_count);
        for entry in group {
            let band = self.inline_float_band_for_line(
                entry.line_index,
                context.block_style,
                context.available_width,
                context.padding_left,
            );
            let indent = used_line_indent(
                entry.line_index,
                false,
                context.hanging_indent,
                context.block_style,
                band.width(),
            );
            available_widths.push((band.width() - indent).max(0.0));
        }
        if available_widths.iter().any(|width| !width.is_finite()) {
            return;
        }
        let group_end_position = clamp_source_end
            .unwrap_or_else(|| group.last().expect("non-empty balance group").end.position);
        let ellipsis_width = if includes_clamp_ellipsis {
            self.line_clamp_marker_width(context.block_style)
        } else {
            0.0
        };
        let normal_remaining = group
            .iter()
            .zip(&available_widths)
            .enumerate()
            .map(|(index, (entry, available))| {
                let ellipsis = if index + 1 == line_count {
                    ellipsis_width
                } else {
                    0.0
                };
                (available
                    - self.balanced_line_fit_width(
                        graph,
                        entry.start,
                        entry.end,
                        context.block_style,
                        entry.line_index,
                    )
                    - ellipsis)
                    .max(0.0)
            })
            .collect::<Vec<_>>();
        let normal_score = balanced_remaining_space_variance(&normal_remaining);
        let mut state = BalancedLineSearchState::default();
        self.search_balanced_line_group(
            &BalancedLineSearch {
                graph,
                context,
                group_end: group_end_position,
                available_widths: &available_widths,
                ellipsis_width,
                // `overflow-wrap` is a last-resort fallback. If ordinary
                // wrapping established this line count without it, balancing
                // must not introduce an emergency word split merely to make
                // the rag smaller.
                // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
                allow_emergency_breaks: group.iter().any(|entry| {
                    entry
                        .end
                        .break_opportunity
                        .is_some_and(|opportunity| opportunity.emergency)
                }),
            },
            group[0].start,
            group[0].line_index,
            &mut BalancedLineCandidate {
                entries: Vec::with_capacity(line_count),
                remaining_inline_space: Vec::with_capacity(line_count),
            },
            &mut state,
        );
        if !state.exhausted_budget
            && let Some(candidate) = state.best
            && candidate.entries.len() == line_count
            && balanced_remaining_space_variance(&candidate.remaining_inline_space)
                + INLINE_FLOAT_EPSILON
                < normal_score
        {
            output[group_start..group_end].copy_from_slice(&candidate.entries);
        }
    }

    /// Measure a candidate's used advance with the same edge effects and
    /// hanging punctuation treatment as ordinary graph selection.
    fn balanced_line_fit_width(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        end: SelectedInlineLineEnd,
        block_style: &ComputedStyle,
        line_index: usize,
    ) -> f32 {
        let range = InlineGraphRange {
            start,
            end: end.position,
        };
        let remaining_allows_last =
            graph_remaining_allows_last_hanging_punctuation(graph, end.position);
        let applies_first_line_style = line_index == 0 && block_style.first_line_style.is_some();
        if !applies_first_line_style
            && let Some(measurement) = graph.borrowed_line_measurement_for_full_run_range(
                range,
                end.break_opportunity,
                &mut self.font_system,
                block_style,
            )
        {
            let runs = &graph.runs[measurement.run_range];
            if runs.is_empty() {
                return 0.0;
            }
            let hanging_widths = hanging_punctuation_widths_for_line_items(
                &mut self.font_system,
                runs,
                block_style,
                line_index == 0,
                remaining_allows_last,
                true,
            );
            return (measurement.fitting_width - hanging_widths.start - hanging_widths.end)
                .max(0.0);
        }
        let mut materialized = graph.materialize_line(
            range,
            end.break_opportunity,
            &mut self.font_system,
            block_style,
        );
        if applies_first_line_style {
            let mut items = measured_inline_items(&materialized.items);
            // `::first-line` participates in line fitting, including balanced
            // candidate evaluation. Painting applies the same pseudo after a
            // line is selected, but balancing must shape this candidate with
            // the used first-line style before deciding its break point.
            // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo> and
            // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
            apply_first_line_pseudos_to_line_items(&mut items, block_style, false);
            materialized.items = items
                .into_iter()
                .map(|item| {
                    let shaped = match &item {
                        InlineLineItem::Fragment(fragment) => self
                            .font_system
                            .shape_unwrapped_line(
                                fragment.text(),
                                fragment.style(),
                                fragment.style().line_height,
                            )
                            .map(std::rc::Rc::new),
                        InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
                    };
                    let width = shaped
                        .as_deref()
                        .map(ShapedInlineLine::advance_width)
                        .unwrap_or_else(|| match &item {
                            InlineLineItem::Atom(atom) => {
                                inline_atom_logical_inline_size(atom, block_style)
                            }
                            InlineLineItem::Fragment(_) | InlineLineItem::Float(_) => 0.0,
                        });
                    MeasuredInlineItem {
                        item,
                        width,
                        shaped,
                    }
                })
                .collect();
            // Recomposition for `::first-line` has produced fresh shaper
            // advances. Reapply the single visual-boundary pass before this
            // candidate is measured, exactly as final paint will do.
            apply_visual_tracking_boundaries(&mut materialized.items);
            let widths = inline_content_width_for_line_items(
                &materialized.items,
                &mut self.font_system,
                |item| item.width,
            );
            materialized.fitting_width = widths.fitting_width;
        }
        let hanging_widths = hanging_punctuation_widths_for_line_items(
            &mut self.font_system,
            &materialized.items,
            block_style,
            line_index == 0,
            remaining_allows_last,
            true,
        );
        (materialized.fitting_width - hanging_widths.start - hanging_widths.end).max(0.0)
    }

    /// Enumerate same-count legal break sequences for one balance group.
    ///
    /// The CSS Text 4 limit of ten lines keeps this bounded in the common
    /// case.  The explicit candidate cap makes pathological prose retain its
    /// ordinary sequence rather than risking unbounded layout work.
    fn search_balanced_line_group(
        &mut self,
        search: &BalancedLineSearch<'_, '_>,
        mut start: InlineGraphPosition,
        line_index: usize,
        candidate: &mut BalancedLineCandidate,
        state: &mut BalancedLineSearchState,
    ) {
        // CSS Text Phase I collapses a leading document-space run at every
        // selected line start. The ordinary selector performs this before
        // choosing its next edge; the balance search must normalize its
        // recursive candidate starts identically or its stored plan will no
        // longer match the real source stream after the first line.
        // <https://drafts.csswg.org/css-text-3/#white-space-phase-1>
        while start.byte_offset == 0
            && start.run_index < search.graph.runs.len()
            && inline_line_item_is_collapsible_space(&search.graph.runs[start.run_index].item)
        {
            start.run_index += 1;
        }
        if state.examined >= MAX_BALANCED_LINE_CANDIDATES {
            state.exhausted_budget = true;
            return;
        }
        // Count every visited partial sequence, not only completed plans:
        // otherwise a broad first branch can spend unbounded work before it
        // reaches its first leaf.
        state.examined += 1;
        let candidate_index = candidate.entries.len();
        if candidate_index == search.available_widths.len() {
            if start == search.group_end {
                let score = balanced_remaining_space_variance(&candidate.remaining_inline_space);
                if state.best.as_ref().is_none_or(|best| {
                    score + INLINE_FLOAT_EPSILON
                        < balanced_remaining_space_variance(&best.remaining_inline_space)
                }) {
                    state.best = Some(candidate.clone());
                }
            }
            return;
        }

        let is_final_line = candidate_index + 1 == search.available_widths.len();
        let mut ends = search
            .graph
            .break_opportunities_after(start)
            .filter(|opportunity| {
                self.mixed_graph_opportunity_allowed(search.graph, *opportunity)
                    && (search.allow_emergency_breaks || !opportunity.emergency)
                    && opportunity.position <= search.group_end
                    && (is_final_line == (opportunity.position == search.group_end))
            })
            .map(|opportunity| SelectedInlineLineEnd {
                position: opportunity.position,
                break_opportunity: (opportunity.position < search.graph.end_position())
                    .then_some(opportunity),
            })
            .collect::<Vec<_>>();
        if is_final_line && !ends.iter().any(|end| end.position == search.group_end) {
            ends.push(SelectedInlineLineEnd {
                position: search.group_end,
                break_opportunity: (search.group_end < search.graph.end_position())
                    .then(|| {
                        search
                            .graph
                            .opportunities
                            .iter()
                            .find(|opportunity| opportunity.position == search.group_end)
                            .copied()
                    })
                    .flatten(),
            });
        }
        for end in ends {
            if end.position <= start {
                continue;
            }
            let used_width = self.balanced_line_fit_width(
                search.graph,
                start,
                end,
                search.context.block_style,
                line_index,
            );
            let ellipsis = if is_final_line && search.ellipsis_width > 0.0 {
                search.ellipsis_width
            } else {
                0.0
            };
            if used_width + ellipsis > search.available_widths[candidate_index] + 0.5 {
                continue;
            }
            candidate.entries.push(BalancedLinePlanEntry {
                start,
                end,
                line_index,
            });
            candidate
                .remaining_inline_space
                .push((search.available_widths[candidate_index] - used_width - ellipsis).max(0.0));
            self.search_balanced_line_group(search, end.position, line_index + 1, candidate, state);
            candidate.entries.pop();
            candidate.remaining_inline_space.pop();
        }
    }

    /// Measure the block overflow marker using the clamp container's root
    /// inline style, not the last source fragment's style.
    /// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
    fn line_clamp_marker_width(&mut self, block_style: &ComputedStyle) -> f32 {
        let Some(line_clamp) = &block_style.line_clamp else {
            return 0.0;
        };
        let marker = match &line_clamp.ellipsis {
            css::BlockEllipsis::Auto => "…",
            css::BlockEllipsis::None => return 0.0,
            css::BlockEllipsis::String(marker) => marker,
        };
        if marker.is_empty() {
            return 0.0;
        }
        self.font_system
            .shape_unwrapped_line(marker, block_style, block_style.line_height)
            .map(|shaped| shaped.advance_width())
            .unwrap_or(0.0)
    }

    /// Materialize the initial pseudo alone so its exclusion participates in
    /// selecting the source line that follows it.
    ///
    /// Both direct and collected-line layout use this lifecycle. In
    /// particular, explicit source breaks must not defer registration until
    /// after the reusable line records have already been selected.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
    pub(in crate::layout) fn register_provisional_initial_letter_exclusion(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
        preceding_floats_are_committed: bool,
    ) {
        let Some(initial_run_index) = graph.runs.iter().position(|run| {
            matches!(
                &run.item,
                InlineLineItem::Fragment(fragment)
                    if !fragment.style().initial_letter.is_normal()
            )
        }) else {
            return;
        };
        // A source-order float preceding the first-letter pseudo must be
        // placed before the initial computes its own line-band anchor. A
        // speculative initial at the containing-block edge would otherwise
        // participate in that float's placement, then measure itself against
        // the displacement it caused. The final materialized initial is
        // registered immediately after the real float has been placed.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
        if !preceding_floats_are_committed
            && graph.runs[..initial_run_index]
                .iter()
                .any(|run| matches!(run.item, InlineLineItem::Float(_)))
        {
            return;
        }
        let pseudo_start_run_index = graph.runs[..initial_run_index]
            .iter()
            .rposition(|run| {
                !matches!(
                    run.item,
                    InlineLineItem::Fragment(ref fragment)
                        if fragment.first_letter_pseudo_role()
                            == FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace
                )
            })
            .map_or(0, |index| index + 1);
        let start = InlineGraphPosition::at_run_start(pseudo_start_run_index);
        let end = InlineGraphPosition::at_run_start(initial_run_index + 1);
        let mut provisional = self.materialize_inline_line_fragment(
            graph,
            InlineGraphRange { start, end },
            context,
            InlineLinePhysicalRow {
                line_index,
                identity: SelectedLineIdentity {
                    is_first_formatted_line: true,
                    starts_after_forced_break,
                },
                block_offset: 0.0,
            },
            graph.break_opportunity_at(end),
        );
        self.register_initial_letter_exclusion_for_line(
            &mut provisional,
            context,
            line_index,
            0.0,
            true,
        );
    }

    fn register_initial_letter_exclusion_for_line(
        &mut self,
        fragment: &mut InlineLineFragment,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        block_offset: f32,
        provisional: bool,
    ) {
        let Some((initial_index, item_width, style)) = fragment
            .items()
            .iter()
            .enumerate()
            .find_map(|(index, item)| {
                let InlineLineItem::Fragment(fragment) = &item.item else {
                    return None;
                };
                (!fragment.style().initial_letter.is_normal()).then_some((
                    index,
                    item.width,
                    fragment.style(),
                ))
            })
        else {
            return;
        };
        let style = style.clone();
        let Some((_size, sink)) = style.initial_letter.specified() else {
            return;
        };
        if !provisional {
            let page_index = self.current_float_page_index();
            self.float_contexts
                .last_mut()
                .expect("root float context exists")
                .shapes
                .retain(|shape| {
                    shape.kind != FlowExclusionKind::InitialLetter
                        || shape.page_index != page_index
                        || shape
                            .initial_letter
                            .as_ref()
                            .is_none_or(|layout| layout.impacted_line_range.start != line_index)
                });
        }
        let border_widths = used_border_widths(&style);
        // The exclusion lasts for the initial letter's *used margin box*, not
        // merely for the abstract `initial-letter` size.  An Ahem drop cap,
        // for example, has a 60pt used glyph box beside an 18pt strut and
        // therefore intersects four line slabs even though its specified
        // size is `3`.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-wrap>
        // Align the initial glyph to the surrounding text edge, not to the
        // outer line-box edge. The parent strut's block-start leading is part
        // of the initial-letter margin-box span, so its final intersected
        // line slab remains excluded.
        let block_start_alignment_inset =
            ((context.block_style.line_height - context.block_style.font_size) * 0.5).max(0.0);
        let horizontal = context.block_style.writing_mode == WritingMode::HorizontalTb;
        let margin_box_block_size = if horizontal {
            style.font_size
                + style.margin.top
                + border_widths.top
                + style.padding.top
                + style.padding.bottom
                + border_widths.bottom
                + style.margin.bottom
                + block_start_alignment_inset
        } else {
            // The vertical physical block span is the initial letter's
            // margin box. The root-strut alignment allowance is projected
            // along the vertical inline slab below; adding it here makes a
            // vertical initial exclude extra block columns as though its
            // margin box had absorbed the parent's leading.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
            style.font_size
                + style.margin.left
                + border_widths.left
                + style.padding.left
                + style.padding.right
                + border_widths.right
                + style.margin.right
        }
        .max(0.0);
        let impacted_lines = ((margin_box_block_size / context.block_style.line_height.max(0.01))
            .ceil() as u32)
            .max(sink)
            .max(1);
        if impacted_lines <= 1 {
            return;
        }
        let leading_pseudo_inline_size = if horizontal {
            fragment.items()[..initial_index]
                .iter()
                .rev()
                .take_while(|item| {
                    matches!(
                        item.as_ref(),
                        InlineLineItem::Fragment(prefix)
                            if prefix.first_letter_pseudo_role()
                                == FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace
                    )
                })
                .map(|item| item.width)
                .sum()
        } else {
            0.0
        };
        let mut margin_box_inline_size = if horizontal {
            item_width
                + style.margin.left
                + border_widths.left
                + style.padding.left
                + style.padding.right
                + border_widths.right
                + style.margin.right
        } else {
            item_width
                + style.margin.top
                + border_widths.top
                + style.padding.top
                + style.padding.bottom
                + border_widths.bottom
                + style.margin.bottom
        }
        .max(0.0);
        margin_box_inline_size += leading_pseudo_inline_size;
        if let css::InitialLetterWrap::Offset(offset) = &style.initial_letter_wrap {
            // The explicit `initial-letter-wrap` offset resolves against the
            // final initial-letter fragment's logical inline width, including
            // its used inline box edges.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-wrap>
            margin_box_inline_size = (margin_box_inline_size
                + offset
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        margin_box_inline_size,
                    )))
                    .unwrap_or(layout_pt(0.0))
                    .points())
            .max(0.0);
        }
        if style.initial_letter_wrap == css::InitialLetterWrap::Grid {
            // The grid wrapping mode enlarges the rectangular wrap area to a
            // whole character-cell increment of the containing line's text
            // grid. Use a shaped representative cell so proportional fonts
            // retain the surrounding font's actual used advance instead of
            // assuming that `1em` is square.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-wrap>
            let grid_increment = self
                .font_system
                .shape_unwrapped_line("0", context.block_style, context.block_style.line_height)
                .map(|shaped| shaped.advance_width())
                .unwrap_or(context.block_style.font_size)
                .max(INLINE_FLOAT_EPSILON);
            margin_box_inline_size =
                (margin_box_inline_size / grid_increment).ceil() * grid_increment;
        }
        if margin_box_inline_size <= INLINE_FLOAT_EPSILON {
            return;
        }
        let inline_start = inline_start_side(
            context.block_style.writing_mode,
            context.block_style.used_direction(),
        );
        let inline_start_side = match inline_start {
            PhysicalSide::Left => UsedFloatSide::Left,
            PhysicalSide::Right => UsedFloatSide::Right,
            PhysicalSide::Top => UsedFloatSide::Top,
            PhysicalSide::Bottom => UsedFloatSide::Bottom,
        };
        // Initial letters in vertical writing modes occupy a block-start
        // column. Model that column through the physical float-side query
        // used by CSS Shapes, while retaining initial-letter-specific used
        // geometry below. This is deliberately distinct from the vertical
        // inline-start side used for glyph positioning.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
        let side =
            initial_letter_exclusion_side(context.block_style.writing_mode, inline_start_side);
        fragment.suppress_float_adjust = true;
        let placement_slab = PhysicalLineSlab::inherited_strut(
            line_index,
            block_offset,
            context.block_style.line_height,
        )
        .with_used_block_size(margin_box_block_size);
        let placement_position = self.inline_line_physical_position_with_block_offset(
            line_index,
            context.block_style,
            block_offset,
        );
        let css_float_band = self.css_float_band_for_physical_slab(
            context.block_style,
            context.available_width,
            context.padding_left,
            placement_slab,
        );
        let initial_text_indent = used_line_indent_for_formatted_line(
            true,
            false,
            0.0,
            context.block_style,
            css_float_band.width(),
        );
        // The first-line indent expands the initial letter's exclusion from
        // the containing block's inline-start edge. It is not a translation
        // of the initial letter's own margin box: following lines must remain
        // aligned to the same containing-block edge.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-edge-effects>
        let horizontal_exclusion_inline_size =
            (margin_box_inline_size + 2.0 * initial_text_indent).max(0.0);
        let companion_uses_initial_exclusion = horizontal
            && fragment.indent
                > css_float_band.left_offset() + initial_text_indent + INLINE_FLOAT_EPSILON;
        let inline_float_offset = match side {
            UsedFloatSide::Left => css_float_band.left_offset(),
            UsedFloatSide::Right => (context.available_width - css_float_band.end()).max(0.0),
            UsedFloatSide::Top | UsedFloatSide::Bottom => 0.0,
        };
        // A preceding same-side CSS float already contributes the source
        // advance that a bare RTL initial otherwise needs to supply itself.
        // Keep those two representations distinct: applying both moves the
        // initial and its companion past the containing block's inline end.
        let rtl_initial_source_advance =
            if side == UsedFloatSide::Right && inline_float_offset <= INLINE_FLOAT_EPSILON {
                item_width
            } else {
                0.0
            };
        let (x, top, width, height) = if horizontal {
            match side {
                UsedFloatSide::Left => (
                    placement_position.content_left + context.padding_left + inline_float_offset,
                    placement_position.cursor_y,
                    horizontal_exclusion_inline_size,
                    margin_box_block_size,
                ),
                UsedFloatSide::Right => (
                    placement_position.content_right
                        - inline_float_offset
                        - horizontal_exclusion_inline_size,
                    placement_position.cursor_y,
                    horizontal_exclusion_inline_size,
                    margin_box_block_size,
                ),
                UsedFloatSide::Top | UsedFloatSide::Bottom => unreachable!(),
            }
        } else {
            let nominal_x = match context.block_style.writing_mode {
                WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                    placement_position.content_right - margin_box_block_size
                }
                WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                    placement_position.content_left
                }
                WritingMode::HorizontalTb => unreachable!(),
            };
            let x = self
                .float_contexts
                .last()
                .expect("root float context exists")
                .initial_letter_block_start_avoiding_x(
                    self.current_float_page_index(),
                    context.block_style.writing_mode,
                    PageTopRect::new(
                        nominal_x,
                        placement_position.cursor_y,
                        margin_box_block_size,
                        margin_box_inline_size,
                    ),
                );
            (
                x,
                placement_position.cursor_y,
                margin_box_block_size,
                margin_box_inline_size,
            )
        };
        if horizontal {
            let glyph_inline_offset = match side {
                UsedFloatSide::Left => {
                    // The companion source is selected in the shared
                    // content-wrap band, which may already begin after this
                    // initial's provisional exclusion. The initial glyph
                    // itself remains anchored in the CSS-float-only band;
                    // otherwise it paints one initial-letter width to its
                    // own inline end in collected/fragmented layout.
                    // <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
                    initial_text_indent
                        + style.margin.left
                        + css_float_band.left_offset()
                        + initial_text_indent
                        - fragment.indent
                }
                // The visual RTL source run is ordered to the left of the
                // initial glyph. Move the glyph across its own used advance;
                // the logical indent is already represented by the shared
                // wrap edge, so it does not translate this isolated glyph.
                UsedFloatSide::Right => {
                    style.font_size * 0.5 + rtl_initial_source_advance + style.margin.right
                }
                UsedFloatSide::Top | UsedFloatSide::Bottom => unreachable!(),
            };
            // Initial-letter alignment uses the surrounding root strut's
            // text edge rather than the outer line-box edge. Move the glyph
            // by that block-start half-leading while retaining its margin-box
            // exclusion at the aligned line edge.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
            // Ruby annotations and other ordinary line participants may
            // enlarge the line's outer box, but an initial remains aligned to
            // the containing strut's text edge rather than moving with that
            // annotation-only expansion.
            let glyph_block_offset = -block_start_alignment_inset - style.margin.top;
            let mut items = fragment.items().to_vec();
            if let Some(initial) = items.iter_mut().find_map(|item| {
                let InlineLineItem::Fragment(initial) = &mut item.item else {
                    return None;
                };
                (!initial.style().initial_letter.is_normal()).then_some(initial)
            }) {
                initial.visual_offset = initial.visual_offset.plus(InlineVisualOffset {
                    vector: InlineVector::new(glyph_inline_offset, glyph_block_offset),
                });
            }
            let leading_pseudo_is_out_of_flow = companion_uses_initial_exclusion
                || (side == UsedFloatSide::Right
                    && leading_pseudo_inline_size > INLINE_FLOAT_EPSILON);
            if leading_pseudo_is_out_of_flow {
                for item in &mut items[..initial_index] {
                    let InlineLineItem::Fragment(prefix) = &mut item.item else {
                        continue;
                    };
                    if prefix.first_letter_pseudo_role()
                        == FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace
                    {
                        prefix.set_out_of_flow_paint_inline_advance(layout_pt(item.width));
                        prefix.set_out_of_flow_paint_block_size(layout_pt(style.font_size));
                        let prefix_visual_offset = if side == UsedFloatSide::Right {
                            margin_box_inline_size - leading_pseudo_inline_size
                        } else {
                            -margin_box_inline_size
                        };
                        prefix.visual_offset = prefix.visual_offset.plus(InlineVisualOffset {
                            vector: InlineVector::new(prefix_visual_offset, 0.0),
                        });
                        item.width = 0.0;
                    }
                }
            }
            if let Some(initial) = items.iter_mut().find(|item| {
                matches!(
                    item.as_ref(),
                    InlineLineItem::Fragment(fragment)
                        if !fragment.style().initial_letter.is_normal()
                )
            }) {
                // When the companion source was selected against this
                // initial's provisional exclusion, its line origin already
                // begins at the margin-box content-side edge. Give the
                // initial zero source advance in that representation; adding
                // the margin-box width again would leave a second initial
                // width before the following text. Without a provisional
                // exclusion, the initial owns that advance normally.
                // <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
                initial.width = if companion_uses_initial_exclusion {
                    0.0
                } else {
                    margin_box_inline_size - leading_pseudo_inline_size
                };
            }
            if side == UsedFloatSide::Right {
                // The initial-letter pseudo is isolated from the adjacent
                // normal source for shaping, but visual RTL order places that
                // source after the initial run's original advance. Move the
                // normal tail into the margin-box's content-side band while
                // retaining its source measurement and bidi order.
                for item in &mut items {
                    let InlineLineItem::Fragment(tail) = &mut item.item else {
                        continue;
                    };
                    if tail.style().initial_letter.is_normal()
                        && tail.first_letter_pseudo_role()
                            == FirstLetterPseudoFragmentRole::Ordinary
                    {
                        tail.visual_offset = tail.visual_offset.plus(InlineVisualOffset {
                            vector: InlineVector::new(
                                -style.font_size + rtl_initial_source_advance,
                                0.0,
                            ),
                        });
                    }
                }
            } else if initial_text_indent > INLINE_FLOAT_EPSILON
                || leading_pseudo_inline_size > INLINE_FLOAT_EPSILON
            {
                // The first-line indent expands the initial's wrap edge, but
                // does not create a second gap between the isolated initial
                // glyph and its adjacent source. The first companion run is
                // selected in that expanded band, so project it back across
                // the single indent or pseudo-prefix edge at paint time.
                // Later wrapped lines retain the full exclusion edge.
                for item in &mut items {
                    let InlineLineItem::Fragment(tail) = &mut item.item else {
                        continue;
                    };
                    if tail.style().initial_letter.is_normal()
                        && tail.first_letter_pseudo_role()
                            == FirstLetterPseudoFragmentRole::Ordinary
                    {
                        tail.visual_offset = tail.visual_offset.plus(InlineVisualOffset {
                            vector: InlineVector::new(
                                -initial_text_indent - leading_pseudo_inline_size,
                                0.0,
                            ),
                        });
                    }
                }
            }
            let content_width = items.iter().map(|item| item.width).sum();
            fragment.metrics =
                self.mixed_inline_line_metrics(&items, context.block_style, content_width);
            fragment.items = Rc::from(items.into_boxed_slice());
        } else {
            // A vertical initial letter occupies the adjacent block slab,
            // rather than advancing the source line's vertical inline
            // cursor.  Its following source text consequently starts at the
            // same logical inline edge as the initial itself; the shared
            // logical exclusion narrows the affected block columns.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
            let mut items = fragment.items().to_vec();
            let has_initial = if let Some(initial) = items.iter_mut().find(|item| {
                matches!(
                    item.as_ref(),
                    InlineLineItem::Fragment(fragment)
                        if !fragment.style().initial_letter.is_normal()
                )
            }) {
                // The vertical line formatter positions a source run at the
                // logical line origin. An initial letter is aligned to the
                // adjacent block slab's text edge instead; project that
                // root-strut edge into the physical x axis before painting.
                let (physical_x_offset, physical_y_offset) = match context.block_style.writing_mode
                {
                    WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                        // Vertical glyph painting starts from the inline
                        // glyph origin (rather than from its ink edge).
                        // Project that origin onto the initial letter's
                        // containing block-edge: the Ahem advance leaves a
                        // fifth-em leading-side projection between those
                        // coordinates. Keeping this in the writing-mode
                        // adapter makes the adjacent exclusion and painted
                        // glyph share one logical anchor.
                        (style.font_size * 0.2, 0.0)
                    }
                    WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                        // The opposite block progression uses the trailing
                        // glyph edge. Its root-strut projection is the
                        // remaining advance after the initial's half-leading
                        // and the normal line's quarter-leading.
                        (
                            (style.font_size * 0.25
                                - block_start_alignment_inset
                                - context.block_style.line_height * 0.25)
                                .max(0.0),
                            margin_box_inline_size,
                        )
                    }
                    WritingMode::HorizontalTb => unreachable!(),
                };
                if physical_x_offset != 0.0 || physical_y_offset != 0.0 {
                    let InlineLineItem::Fragment(initial) = &mut initial.item else {
                        unreachable!("initial-letter item remains a text fragment");
                    };
                    initial.visual_offset = initial.visual_offset.plus(InlineVisualOffset {
                        vector: InlineVector::new(physical_x_offset, physical_y_offset),
                    });
                }
                initial.width = 0.0;
                true
            } else {
                false
            };
            if has_initial {
                if matches!(
                    context.block_style.writing_mode,
                    WritingMode::VerticalLr | WritingMode::SidewaysLr
                ) {
                    // The source run remains on the originating logical
                    // line, while the initial consumes the first physical
                    // inline slab. Keep the source's paint origin adjacent
                    // to that slab without increasing the normal line box.
                    // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
                    for item in &mut items {
                        let InlineLineItem::Fragment(tail) = &mut item.item else {
                            continue;
                        };
                        if tail.style().initial_letter.is_normal() {
                            tail.visual_offset = tail.visual_offset.plus(InlineVisualOffset {
                                vector: InlineVector::new(0.0, -margin_box_inline_size),
                            });
                        }
                    }
                }
                let content_width = items.iter().map(|item| item.width).sum();
                fragment.metrics =
                    self.mixed_inline_line_metrics(&items, context.block_style, content_width);
                fragment.items = Rc::from(items.into_boxed_slice());
            }
        }
        let wrapping_box = PageTopRect::new(x, top, width, height);
        let margin_box = if horizontal {
            PageTopRect::new(
                x,
                top,
                width,
                (height - block_start_alignment_inset).max(0.0),
            )
        } else {
            let margin_box_width = (width - block_start_alignment_inset).max(0.0);
            let margin_box_x = match context.block_style.writing_mode {
                WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                    x + block_start_alignment_inset
                }
                WritingMode::VerticalLr | WritingMode::SidewaysLr => x,
                WritingMode::HorizontalTb => unreachable!(),
            };
            PageTopRect::new(margin_box_x, top, margin_box_width, height)
        };
        let layout = InitialLetterLayout {
            source_order: self.next_paint_source_order(),
            page_index: self.current_float_page_index(),
            writing_mode: context.block_style.writing_mode,
            direction: context.block_style.used_direction(),
            used_font_size: style.font_size,
            provisional,
            block_start_alignment_inset,
            margin_box,
            wrapping_box,
            impacted_line_range: line_index..line_index.saturating_add(impacted_lines as usize),
            contour: FloatContour::Rect,
        };
        let shape = FloatShape::initial_letter_rect(self.next_float_id(), side, layout);
        let mut run = self.float_run_state();
        self.push_float_shape(shape, &mut run);
    }

    /// Place inline floats without making them break opportunities in an
    /// unbreakable line.
    ///
    /// CSS Text forbids soft wrap opportunities for `white-space: nowrap`,
    /// while CSS 2.2 still positions inline floats at the current line top
    /// according to the active float band:
    /// <https://www.w3.org/TR/css-text-3/#white-space-property> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#float-position>.
    pub(in crate::layout) fn try_select_unbreakable_line_with_inline_floats(
        &mut self,
        graph: &InlineOpportunityGraph,
        range: InlineGraphRange,
        selected_end: SelectedInlineLineEnd,
        context: InlineParagraphContext<'_>,
        row: InlineLinePhysicalRow,
    ) -> Option<InlineLineFragment> {
        graph.first_float_position_in_range(range)?;
        let break_opportunity = selected_end.break_opportunity.filter(|opportunity| {
            opportunity.position == range.end && range.end < graph.end_position()
        });
        let mut fragment =
            self.materialize_inline_line_fragment(graph, range, context, row, break_opportunity);
        let snapshot = self.snapshot();
        let mut search_start = range.start;
        while let Some(float_position) = graph.first_float_position_in_range(InlineGraphRange {
            start: search_start,
            end: range.end,
        }) {
            // A float after an unbreakable inline prefix cannot affect that
            // prefix's line box. Preserve the whole unbreakable source run
            // as one line and place a fitting float at that line's top. This
            // path is selected from graph opportunities, so it covers a
            // descendant `nowrap` span even when the containing block itself
            // still permits wrapping. CSS 2.2 permits the float to move down,
            // but forbids its outer top from moving above a line box generated
            // by earlier source content.
            // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
            if float_position > range.start {
                if !self.try_place_inline_float_in_line_band(
                    graph,
                    float_position,
                    context,
                    row.line_index,
                    row.identity,
                ) {
                    self.restore(snapshot);
                    return None;
                }
            } else if float_position == range.start
                && !self.try_place_inline_float_in_line_band(
                    graph,
                    float_position,
                    context,
                    row.line_index,
                    row.identity,
                )
            {
                self.restore(snapshot);
                return None;
            }
            search_start = InlineGraphPosition::at_run_start(float_position.run_index + 1);
            if search_start >= range.end {
                break;
            }
        }
        fragment.suppress_float_adjust = true;
        Some(fragment)
    }

    pub(in crate::layout) fn place_inline_waiting_float(
        &mut self,
        float: &InlineFloat,
        context: InlineParagraphContext<'_>,
        line_index: usize,
    ) {
        let position = self.inline_line_physical_position(line_index, context.block_style);
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        let line_left = position.content_left + context.padding_left;
        self.content_left = position.content_left;
        self.content_right = position.content_right;
        self.cursor_y = position.cursor_y;
        let mut run = self.float_run_state();
        let pushed_containing_block = self.push_inline_float_positioning_containing_block(
            float,
            None,
            line_left,
            0.0,
            position.cursor_y,
        );
        let generated_float_children = [];
        self.layout_floating_child(
            float.element(),
            float.signature().clone(),
            float.style(),
            float
                .is_generated_content()
                .then_some(generated_float_children.as_slice()),
            None,
            context.stylesheets,
            &mut run,
        );
        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
        self.cursor_y = saved_cursor_y;
    }

    /// Position an inline-source float against the whole current line band.
    ///
    /// The source-order marker stays in the graph with zero advance; callers
    /// retry selection after this succeeds so both the prefix and suffix flow
    /// around the new exclusion.
    /// <https://www.w3.org/TR/CSS22/visuren.html#float-position>
    pub(in crate::layout) fn try_place_inline_float_in_line_band(
        &mut self,
        graph: &InlineOpportunityGraph,
        float_position: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        line_identity: SelectedLineIdentity,
    ) -> bool {
        if context.block_style.writing_mode != WritingMode::HorizontalTb {
            // A source-order float still establishes a physical CSS float in
            // vertical and sideways text. Its later line-wrap effect is
            // queried through `content_logical_band`, but placement itself
            // uses the existing floated-child path and the line's projected
            // physical containing block.
            // <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
            let Some(float) = graph.float_at_position(float_position).cloned() else {
                return false;
            };
            let snapshot = self.snapshot();
            let saved_content_left = self.content_left;
            let saved_content_right = self.content_right;
            let saved_cursor_y = self.cursor_y;
            let saved_direction = self.containing_block_direction;
            let position = self.inline_line_physical_position(line_index, context.block_style);
            self.content_left = position.content_left;
            self.content_right = position.content_right;
            self.cursor_y = position.cursor_y;
            self.containing_block_direction = context.block_style.used_direction();
            let shape_count_before = self
                .float_contexts
                .last()
                .map_or(0, |float_context| float_context.shapes.len());
            let mut run = self.float_run_state();
            let pushed_containing_block = self.push_inline_float_positioning_containing_block(
                &float,
                Some((graph, float_position)),
                position.content_left + context.padding_left,
                0.0,
                position.cursor_y,
            );
            let generated_float_children = [];
            let placed = self.layout_floating_child(
                float.element(),
                float.signature().clone(),
                float.style(),
                float
                    .is_generated_content()
                    .then_some(generated_float_children.as_slice()),
                None,
                context.stylesheets,
                &mut run,
            );
            if pushed_containing_block {
                self.containing_blocks.pop();
            }
            let accepted = placed
                && self.pages.len() == snapshot.pages.len()
                && self.float_contexts.last().is_some_and(|float_context| {
                    float_context.shapes.len() > shape_count_before
                        && float_context.shapes.last().is_some_and(|shape| {
                            shape.is_css_float()
                                && shape.page_index == self.current_float_page_index()
                        })
                });
            if accepted {
                self.content_left = saved_content_left;
                self.content_right = saved_content_right;
                self.cursor_y = saved_cursor_y;
                self.containing_block_direction = saved_direction;
            } else {
                self.restore(snapshot);
            }
            return accepted;
        }
        let Some(float) = graph.float_at_position(float_position).cloned() else {
            return false;
        };
        let block_style = context.block_style;
        let band = self.inline_float_band_for_line(
            line_index,
            block_style,
            context.available_width,
            context.padding_left,
        );
        let line_indent = used_line_indent_for_formatted_line(
            line_identity.is_first_formatted_line,
            line_identity.starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width(),
        );
        let line_left = self.content_left + context.padding_left + band.left_offset() + line_indent;
        let line_right =
            self.content_left + context.padding_left + band.left_offset() + band.width();
        if line_right - line_left <= INLINE_FLOAT_EPSILON {
            return false;
        }

        let snapshot = self.snapshot();
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        let saved_direction = self.containing_block_direction;
        let target_top = self.cursor_y - block_style.line_height * line_index as f32;

        self.content_left = line_left;
        self.content_right = line_right;
        self.cursor_y = target_top;
        self.containing_block_direction = block_style.used_direction();
        let mut run = self.float_run_state();
        let pushed_containing_block = self.push_inline_float_positioning_containing_block(
            &float,
            Some((graph, float_position)),
            line_left,
            0.0,
            target_top,
        );
        let generated_float_children = [];
        let placed = self.layout_floating_child(
            float.element(),
            float.signature().clone(),
            float.style(),
            float
                .is_generated_content()
                .then_some(generated_float_children.as_slice()),
            None,
            context.stylesheets,
            &mut run,
        );
        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        let accepted = if placed && self.pages.len() == snapshot.pages.len() {
            self.float_contexts
                .last()
                .and_then(|context| context.shapes.last())
                .is_some_and(|shape| {
                    let float_inline_span = shape.margin_box_inline_span();
                    let float_block_span = shape.margin_box_block_span();
                    let float_width = float_inline_span.width();
                    let band_width = line_right - line_left;
                    shape.page_index == self.pages.len()
                        && (float_block_span.top_y() - target_top).abs() <= INLINE_FLOAT_EPSILON
                        && ((float_inline_span.left_x() + INLINE_FLOAT_EPSILON >= line_left
                            && float_inline_span.right_x() <= line_right + INLINE_FLOAT_EPSILON)
                            || float_width > band_width + INLINE_FLOAT_EPSILON)
                })
        } else {
            false
        };
        if accepted {
            self.content_left = saved_content_left;
            self.content_right = saved_content_right;
            self.cursor_y = saved_cursor_y;
            self.containing_block_direction = saved_direction;
            true
        } else {
            self.restore(snapshot);
            false
        }
    }

    pub(in crate::layout) fn try_place_inline_float_on_current_line(
        &mut self,
        graph: &InlineOpportunityGraph,
        float_position: InlineGraphPosition,
        prefix_width: f32,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        line_identity: SelectedLineIdentity,
    ) -> Option<InlineFloatPlacement> {
        if context.block_style.writing_mode != WritingMode::HorizontalTb {
            return None;
        }
        let float = graph.float_at_position(float_position).cloned()?;
        let block_style = context.block_style;
        let band = self.inline_float_band_for_line(
            line_index,
            block_style,
            context.available_width,
            context.padding_left,
        );
        let line_indent = used_line_indent_for_formatted_line(
            line_identity.is_first_formatted_line,
            line_identity.starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width(),
        );
        let line_left = self.content_left + context.padding_left + band.left_offset() + line_indent;
        let line_right =
            self.content_left + context.padding_left + band.left_offset() + band.width();
        let (remaining_left, remaining_right) = match block_style.used_direction() {
            Direction::Ltr => ((line_left + prefix_width).min(line_right), line_right),
            Direction::Rtl => (line_left, (line_right - prefix_width).max(line_left)),
        };
        if remaining_right - remaining_left <= INLINE_FLOAT_EPSILON {
            return None;
        }

        let snapshot = self.snapshot();
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        let saved_direction = self.containing_block_direction;
        let target_top = self.cursor_y - block_style.line_height * line_index as f32;

        self.content_left = remaining_left;
        self.content_right = remaining_right;
        self.cursor_y = target_top;
        self.containing_block_direction = block_style.used_direction();
        let mut run = self.float_run_state();
        let pushed_containing_block = self.push_inline_float_positioning_containing_block(
            &float,
            Some((graph, float_position)),
            line_left,
            prefix_width,
            target_top,
        );
        let generated_float_children = [];
        let placed = self.layout_floating_child(
            float.element(),
            float.signature().clone(),
            float.style(),
            float
                .is_generated_content()
                .then_some(generated_float_children.as_slice()),
            None,
            context.stylesheets,
            &mut run,
        );
        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        let accepted_shape = if placed && self.pages.len() == snapshot.pages.len() {
            self.float_contexts.last().and_then(|context| {
                context.shapes.last().and_then(|shape| {
                    let float_inline_span = shape.margin_box_inline_span();
                    let float_block_span = shape.margin_box_block_span();
                    let fits_remaining_band = float_inline_span.left_x() + INLINE_FLOAT_EPSILON
                        >= remaining_left
                        && float_inline_span.right_x() <= remaining_right + INLINE_FLOAT_EPSILON;
                    let accepted = shape.page_index == self.pages.len()
                        && (float_block_span.top_y() - target_top).abs() <= INLINE_FLOAT_EPSILON
                        && (fits_remaining_band
                            || (prefix_width <= INLINE_FLOAT_EPSILON
                                && float_inline_span.width()
                                    > remaining_right - remaining_left + INLINE_FLOAT_EPSILON));
                    accepted.then_some(InlineFloatPlacement::new(
                        line_left,
                        line_right,
                        prefix_width,
                        float_inline_span.left_x(),
                        float_inline_span.right_x(),
                        fits_remaining_band,
                        shape.side,
                    ))
                })
            })
        } else {
            None
        };
        if let Some(placement) = accepted_shape {
            self.content_left = saved_content_left;
            self.content_right = saved_content_right;
            self.cursor_y = saved_cursor_y;
            self.containing_block_direction = saved_direction;
            Some(placement)
        } else {
            self.restore(snapshot);
            None
        }
    }

    fn push_inline_float_positioning_containing_block(
        &mut self,
        float: &InlineFloat,
        graph_position: Option<(&InlineOpportunityGraph, InlineGraphPosition)>,
        line_left: f32,
        prefix_width: f32,
        line_top: f32,
    ) -> bool {
        let Some(source) = float.positioning_containing_block() else {
            return false;
        };
        let style = &source.style;
        let border_widths = used_border_widths(style);
        let horizontal_non_padding =
            style.margin.left + border_widths.left + border_widths.right + style.margin.right;
        let padding_box_width =
            (prefix_width - horizontal_non_padding).max(style.padding.left + style.padding.right);
        let source_margin_edge_left = graph_position
            .and_then(|(graph, position)| {
                inline_positioning_source_margin_edge_left(graph, position, source.id, line_left)
            })
            .unwrap_or(line_left);
        let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
            source_margin_edge_left + style.margin.left + border_widths.left,
            line_top - border_widths.top,
            padding_box_width,
            style.line_height + style.padding.top + style.padding.bottom,
        ));
        self.containing_blocks.push(containing_block);
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn try_select_inline_float_same_line_suffix(
        &mut self,
        graph: &InlineOpportunityGraph,
        prefix: InlineLineFragment,
        float_position: InlineGraphPosition,
        suffix_start: InlineGraphPosition,
        placement: InlineFloatPlacement,
        context: InlineParagraphContext<'_>,
        line_index: usize,
    ) -> Option<CombinedInlineFloatLine> {
        if context.block_style.used_direction() != Direction::Ltr
            || suffix_start >= graph.end_position()
        {
            return None;
        }
        let prefix_right = placement.prefix_right();
        let (float_gap, suffix_available_width) = match placement.side {
            UsedFloatSide::Left | UsedFloatSide::Top => (
                (placement.float_right() - prefix_right).max(0.0),
                (placement.line_right() - placement.float_right()).max(0.0),
            ),
            UsedFloatSide::Right | UsedFloatSide::Bottom => {
                (0.0, (placement.float_left() - prefix_right).max(0.0))
            }
        };
        if suffix_available_width <= INLINE_FLOAT_EPSILON {
            return None;
        }
        let selected_end = self.select_inline_line_end_for_width(
            graph,
            suffix_start,
            context.block_style,
            suffix_available_width,
            line_index,
        );
        let end = selected_end.position.min(graph.end_position());
        if end <= suffix_start {
            return None;
        }
        // This helper combines one already-placed float with ordinary inline
        // suffix content. A later cleared float is not ordinary zero-advance
        // content: it must re-enter the main source-order loop so clearance
        // can select a later fragmentainer and register its own exclusion.
        // Otherwise the line fragment retains its marker but never lays the
        // float out, dropping it from both paint and fragmentation.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        if graph
            .first_float_position_in_range(InlineGraphRange {
                start: suffix_start,
                end,
            })
            .and_then(|position| graph.float_at_position(position))
            .is_some_and(|float| float.style().clear != Clear::None)
        {
            return None;
        }
        let suffix = graph.materialize_line(
            InlineGraphRange {
                start: suffix_start,
                end,
            },
            selected_end
                .break_opportunity
                .filter(|opportunity| opportunity.position == end && end < graph.end_position()),
            &mut self.font_system,
            context.block_style,
        );
        if suffix.items.is_empty() {
            return None;
        }

        let float = graph.float_at_position(float_position).cloned()?;
        let mut combined_items = prefix.items().to_vec();
        combined_items.push(MeasuredInlineItem {
            item: InlineLineItem::Float(float),
            width: float_gap,
            shaped: None,
        });
        combined_items.extend(suffix.items);
        let width = combined_items.iter().map(|item| item.width).sum::<f32>();
        let metrics = self.mixed_inline_line_metrics(&combined_items, context.block_style, width);
        if (metrics.height - prefix.metrics.height).abs() > INLINE_FLOAT_EPSILON {
            return None;
        }
        let mut text = prefix.text().to_string();
        text.push_str(&suffix.text);
        Some(CombinedInlineFloatLine {
            end,
            fragment: InlineLineFragment::new(
                combined_items,
                metrics,
                prefix.hanging_widths,
                prefix.indent,
                prefix.available_width,
                prefix.selected_float_page_index,
                true,
                text,
            ),
        })
    }

    pub(in crate::layout) fn select_inline_line_end(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        line_identity: SelectedLineIdentity,
    ) -> SelectedInlineLineEnd {
        self.select_inline_line_end_with_block_offset(
            graph,
            start,
            context,
            line_index,
            line_identity,
            0.0,
        )
    }

    fn select_inline_line_end_with_block_offset(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        line_identity: SelectedLineIdentity,
        block_offset: f32,
    ) -> SelectedInlineLineEnd {
        self.select_inline_line_end_with_block_span(
            graph,
            start,
            context,
            line_index,
            line_identity,
            PhysicalLineSlab::inherited_strut(
                line_index,
                block_offset,
                context.block_style.line_height,
            ),
        )
    }

    fn select_inline_line_end_with_block_span(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        line_identity: SelectedLineIdentity,
        slab: PhysicalLineSlab,
    ) -> SelectedInlineLineEnd {
        let block_style = context.block_style;
        debug_assert_eq!(slab.line_index, line_index);
        let band = self.inline_float_band_for_physical_slab(
            block_style,
            context.available_width,
            context.padding_left,
            slab,
        );
        let line_indent = used_line_indent_for_formatted_line(
            line_identity.is_first_formatted_line,
            line_identity.starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width(),
        );
        let line_available_width = (band.width() - line_indent).max(0.0);
        self.select_inline_line_end_for_width(
            graph,
            start,
            block_style,
            line_available_width,
            line_index,
        )
    }

    pub(in crate::layout) fn select_inline_line_end_for_width(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        block_style: &ComputedStyle,
        line_available_width: f32,
        line_index: usize,
    ) -> SelectedInlineLineEnd {
        // `wrap-inside: avoid` ranks otherwise-legal break opportunities by
        // lexical containment. The smallest depth is least avoided; ties keep
        // the ordinary last-fitting-line policy.
        // <https://drafts.csswg.org/css-text-4/#wrap-inside-property>
        let mut regular_fit = None::<(u16, SelectedInlineLineEnd)>;
        let mut emergency_fit = None::<(u16, SelectedInlineLineEnd)>;
        let opportunities = graph.break_opportunities_after(start).collect::<Vec<_>>();
        if let Some(selected) = self.select_monotonic_regular_line_end(
            graph,
            start,
            block_style,
            line_available_width,
            &opportunities,
        ) {
            return selected;
        }
        for opportunity in opportunities {
            if !self.mixed_graph_opportunity_allowed(graph, opportunity) {
                continue;
            }
            let range = InlineGraphRange {
                start,
                end: opportunity.position,
            };
            let selected_break =
                (opportunity.position < graph.end_position()).then_some(opportunity);
            let remaining_allows_last =
                graph_remaining_allows_last_hanging_punctuation(graph, opportunity.position);
            let fit_width = if let Some(measurement) = graph
                .borrowed_line_measurement_for_full_run_range(
                    range,
                    selected_break,
                    &mut self.font_system,
                    block_style,
                ) {
                let runs = &graph.runs[measurement.run_range];
                if runs.is_empty() {
                    continue;
                }
                let hanging_widths = hanging_punctuation_widths_for_line_items(
                    &mut self.font_system,
                    runs,
                    block_style,
                    line_index == 0,
                    remaining_allows_last,
                    true,
                );
                (measurement.fitting_width - hanging_widths.start - hanging_widths.end).max(0.0)
            } else {
                let materialized = graph.materialize_line_for_available_width(
                    range,
                    selected_break,
                    line_available_width,
                    &mut self.font_system,
                    block_style,
                );
                if materialized.items.is_empty() {
                    continue;
                }
                let hanging_widths = hanging_punctuation_widths_for_line_items(
                    &mut self.font_system,
                    &materialized.items,
                    block_style,
                    line_index == 0,
                    remaining_allows_last,
                    true,
                );
                (materialized.fitting_width - hanging_widths.start - hanging_widths.end).max(0.0)
            };
            if fit_width <= line_available_width + 0.5 {
                let avoid_depth = graph.wrap_inside_avoid_depth(opportunity.position);
                let selected = SelectedInlineLineEnd {
                    position: opportunity.position,
                    break_opportunity: (opportunity.position < graph.end_position())
                        .then_some(opportunity),
                };
                // An automatic opportunity inside the paragraph's final word
                // is unnecessary when that whole word fits on a fresh line.
                // Keep the preceding ordinary boundary instead of using a
                // discretionary hyphen merely to fill spare space on the
                // current line. This is the CSS Text hyphenation preference
                // exercised by the final-word case; a word that cannot fit
                // in the line measure still uses its legal dictionary break.
                // <https://drafts.csswg.org/css-text-3/#hyphenation>
                if opportunity.soft_hyphen
                    && let Some((previous_depth, previous)) = regular_fit
                    && previous_depth == avoid_depth
                    && previous
                        .break_opportunity
                        .is_some_and(|previous| !previous.soft_hyphen)
                    && graph
                        .source_character_before(previous.position)
                        .is_some_and(is_css_collapsible_whitespace)
                {
                    let remainder = graph.materialize_line(
                        InlineGraphRange {
                            start: previous.position,
                            end: graph.end_position(),
                        },
                        None,
                        &mut self.font_system,
                        block_style,
                    );
                    if !remainder.text.chars().any(char::is_whitespace)
                        && remainder.fitting_width <= line_available_width + 0.5
                    {
                        return previous;
                    }
                    // If the final word needs hyphenation even on a fresh
                    // line, choose the preceding ordinary boundary whenever
                    // that fresh line can use a later dictionary opportunity.
                    // It avoids degrading `1 example` from `1` / `exam-` /
                    // `ple` to `1 ex-` / `ample` simply to fill the first
                    // line's spare inline measure.
                    let fresh = self.select_inline_line_end_for_width(
                        graph,
                        previous.position,
                        block_style,
                        line_available_width,
                        line_index.saturating_add(1),
                    );
                    if fresh.position > selected.position {
                        return previous;
                    }
                }
                if opportunity.emergency {
                    if regular_fit.is_none()
                        && emergency_fit.is_none_or(|(fit_depth, fit)| {
                            avoid_depth < fit_depth
                                || (avoid_depth == fit_depth && selected.position > fit.position)
                        })
                    {
                        emergency_fit = Some((avoid_depth, selected));
                    }
                } else if regular_fit.is_none_or(|(fit_depth, fit)| {
                    avoid_depth < fit_depth
                        || (avoid_depth == fit_depth && selected.position > fit.position)
                }) && !regular_fit.is_some_and(|(fit_depth, fit)| {
                    fit_depth == avoid_depth
                        && graph.soft_hyphen_precedes_literal_hyphen(
                            fit.break_opportunity.expect("fitting graph boundary"),
                            opportunity,
                        )
                }) {
                    regular_fit = Some((avoid_depth, selected));
                }
                if matches!(opportunity.kind, InlineBreakKind::Forced) {
                    return selected;
                }
            } else if opportunity.emergency && regular_fit.is_some() {
                continue;
            } else if let Some((_, position)) = regular_fit.or(emergency_fit) {
                return position;
            } else {
                return SelectedInlineLineEnd {
                    position: opportunity.position,
                    break_opportunity: (opportunity.position < graph.end_position())
                        .then_some(opportunity),
                };
            }
        }
        regular_fit
            .or(emergency_fit)
            .map(|(_, selected)| selected)
            .unwrap_or_else(|| SelectedInlineLineEnd {
                position: graph.end_position(),
                break_opportunity: None,
            })
    }

    /// Select a line end by binary search when every candidate has a monotonic
    /// used advance.
    ///
    /// A `word-break: break-all` paragraph can have an opportunity after every
    /// typographic unit. Re-materializing every prefix makes a long, ordinary
    /// line quadratic, even though its used width can only grow (or remain
    /// equal at a trailing trimmed space). Restrict this shortcut to source
    /// text without discretionary, hanging, spacing-trim, or atomic effects;
    /// those cases retain the general candidate-by-candidate algorithm below.
    /// <https://drafts.csswg.org/css-text-3/#line-breaking>
    /// <https://drafts.csswg.org/css-text-3/#word-break-property>
    fn select_monotonic_regular_line_end(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        block_style: &ComputedStyle,
        line_available_width: f32,
        opportunities: &[InlineBreakOpportunity],
    ) -> Option<SelectedInlineLineEnd> {
        if opportunities.is_empty()
            || opportunities
                .iter()
                .any(|opportunity| graph.wrap_inside_avoid_depth(opportunity.position) != 0)
            || block_style.hanging_punctuation.first
            || block_style.hanging_punctuation.last
            || block_style.hanging_punctuation.force_end
            || block_style.hanging_punctuation.allow_end
            || !graph.runs.iter().all(|run| {
                matches!(
                    &run.item,
                    InlineLineItem::Fragment(fragment)
                        if matches!(fragment.style().word_break, css::WordBreak::BreakAll)
                            && matches!(
                                fragment.style().text_spacing_trim.resolved(),
                                TextSpacingTrim::SpaceAll | TextSpacingTrim::Normal
                            )
                            && !fragment.text().contains('\t')
                            && fragment.text().chars().all(|character| {
                                character.is_ascii_alphanumeric()
                                    || character.is_ascii_whitespace()
                            })
                )
            })
            || !opportunities.iter().all(|opportunity| {
                matches!(
                    opportunity.kind,
                    InlineBreakKind::SoftWrap
                        | InlineBreakKind::PreservedSpace
                        | InlineBreakKind::Forced
                ) && (opportunity.kind != InlineBreakKind::Forced
                    || opportunity.position == graph.end_position())
                    && !opportunity.hangs
                    && !opportunity.soft_hyphen
                    && opportunity.discretionary.is_none()
                    && !opportunity.emergency
            })
        {
            return None;
        }

        let mut first_too_wide = opportunities.len();
        let mut lower = 0usize;
        let mut upper = opportunities.len();
        while lower < upper {
            let candidate_index = lower + (upper - lower) / 2;
            let opportunity = opportunities[candidate_index];
            let selected_break =
                (opportunity.position < graph.end_position()).then_some(opportunity);
            let measured = graph.materialize_line_for_available_width(
                InlineGraphRange {
                    start,
                    end: opportunity.position,
                },
                selected_break,
                line_available_width,
                &mut self.font_system,
                block_style,
            );
            if measured.fitting_width <= line_available_width + 0.5 {
                lower = candidate_index + 1;
            } else {
                first_too_wide = candidate_index;
                upper = candidate_index;
            }
        }

        let opportunity = opportunities[first_too_wide.saturating_sub(1)];
        Some(SelectedInlineLineEnd {
            position: opportunity.position,
            break_opportunity: (opportunity.position < graph.end_position()).then_some(opportunity),
        })
    }

    pub(in crate::layout) fn mixed_graph_opportunity_allowed(
        &mut self,
        graph: &InlineOpportunityGraph,
        opportunity: InlineBreakOpportunity,
    ) -> bool {
        // Float-placement checkpoints are consumed by the source-order float
        // handler after a candidate line range is selected. They are not CSS
        // Text line ends: treating one as a regular fitting candidate splits
        // a `nowrap` run immediately before its float marker.
        if matches!(opportunity.kind, InlineBreakKind::FloatPlacement) {
            return false;
        }
        if opportunity.position.byte_offset > 0
            || opportunity.emergency
            || matches!(
                opportunity.kind,
                InlineBreakKind::Forced
                    | InlineBreakKind::PreservedSpace
                    | InlineBreakKind::BreakSpaces
                    | InlineBreakKind::Hyphenation
            )
        {
            return true;
        }
        let Some(item) = graph
            .runs
            .iter()
            .skip(opportunity.position.run_index)
            .find(|run| {
                !matches!(
                    &run.item,
                    InlineLineItem::Atom(atom) if atom.content().is_box_edge()
                )
            })
            .map(|run| &run.item)
        else {
            return true;
        };
        !mixed_inline_item_starts_with_suppressed_line_start_punctuation(item)
    }

    pub(in crate::layout) fn materialize_inline_line_fragment(
        &mut self,
        graph: &InlineOpportunityGraph,
        range: InlineGraphRange,
        context: InlineParagraphContext<'_>,
        row: InlineLinePhysicalRow,
        break_opportunity: Option<InlineBreakOpportunity>,
    ) -> InlineLineFragment {
        let block_style = context.block_style;
        let band = self.inline_float_band_for_line_with_block_offset(
            row.line_index,
            block_style,
            context.available_width,
            context.padding_left,
            row.block_offset,
        );
        let line_indent = used_line_indent_for_formatted_line(
            row.identity.is_first_formatted_line,
            row.identity.starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width(),
        );
        let terminal_pre_wrap_hang = row.identity.starts_after_forced_break
            && break_opportunity.is_none()
            && range.end == graph.end_position();
        let line_available_width = (band.width() - line_indent).max(0.0);
        let mut materialized = graph
            .materialize_line_with_terminal_pre_wrap_hang_for_available_width(
                range,
                break_opportunity,
                terminal_pre_wrap_hang,
                line_available_width,
                &mut self.font_system,
                block_style,
            );
        resolve_materialized_line_leaders(
            &mut materialized,
            &mut self.font_system,
            line_available_width,
        );
        let metrics = self.mixed_inline_line_metrics(
            &materialized.items,
            block_style,
            materialized.content_width,
        );
        let bidi_scope_continuations = graph.bidi_scope_continuations_for_range(range);
        InlineLineFragment::new(
            materialized.items,
            metrics,
            HangingPunctuationWidths::default(),
            band.left_offset() + line_indent,
            band.end(),
            self.current_float_page_index(),
            false,
            materialized.text,
        )
        .with_edge_effects(materialized.edge_effects.clone())
        .with_bidi_scope_continuations(bidi_scope_continuations)
    }
}

/// Return the used block advance not represented by the selector's nominal
/// line-height index.
///
/// The durable inline-line sequence uses the greater of the strut, resolved
/// line metrics, and atomic inline margin-box block-size. Keep float-band
/// selection in the same coordinate system so a later row is queried beside
/// the float at the row it will actually paint on.
/// <https://drafts.csswg.org/css-inline-3/#line-boxes>
fn used_inline_line_block_advance(
    fragment: &InlineLineFragment,
    block_style: &ComputedStyle,
) -> f32 {
    (used_inline_line_block_size_from_items(&fragment.items, fragment.metrics.height, block_style)
        - block_style.line_height)
        .max(0.0)
}

fn used_inline_line_block_size_from_items(
    items: &[MeasuredInlineItem],
    metrics_height: f32,
    block_style: &ComputedStyle,
) -> f32 {
    let atomic_block_size = items
        .iter()
        // An initial letter is an in-flow inline, but CSS Inline explicitly
        // excludes it from the logical height of its originating line box.
        // Its multi-line block extent belongs to its exclusion geometry, not
        // to this line's normal block-stack advance.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
        .filter(|item| {
            !LayoutBuilder::inline_line_item_is_initial_letter(&item.item)
                && matches!(
                    &item.item,
                    InlineLineItem::Atom(atom)
                        if !matches!(atom.content(), InlineAtomContent::InlineEdge(_))
                )
        })
        // Text fragments already contribute their selected line-box extents
        // through `metrics_height`. Reintroducing their raw `line-height`
        // here would erase `text-box-trim` (and `line-fit-edge`) after the
        // line-metrics pass. Only atomic inline margin boxes need this
        // independent minimum-size contribution.
        // <https://drafts.csswg.org/css-inline-3/#line-box>
        .map(|item| inline_line_item_logical_block_size(&item.item, block_style))
        .fold(0.0_f32, f32::max);
    metrics_height
        .max(atomic_block_size)
        .max(block_style.line_height)
}

fn inline_positioning_source_margin_edge_left(
    graph: &InlineOpportunityGraph,
    float_position: InlineGraphPosition,
    source_id: InlinePositioningContainingBlockId,
    line_left: f32,
) -> Option<f32> {
    // CSS 2.2 uses the nearest positioned inline ancestor's padding box, not
    // the float's own inline position, as the containing block for abspos
    // descendants:
    // <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
    for source_run_index in (0..float_position.run_index).rev() {
        let run = &graph.runs[source_run_index];
        if inline_run_is_positioning_source_start_edge(run, source_id) {
            // The start-edge atom carries the source's own padding advance,
            // whereas the containing-block origin is its margin edge. Its
            // coordinate is therefore the accumulated advance *before* that
            // edge. This retains a positioned outer inline's preceding
            // padding for a nested source, without treating the source's own
            // padding (or later text before the float) as an origin offset.
            let source_margin_edge_offset = graph.runs[..source_run_index]
                .iter()
                .map(|preceding| preceding.width)
                .sum::<f32>();
            return Some(line_left + source_margin_edge_offset);
        }
    }
    None
}

fn inline_run_is_positioning_source_start_edge(
    run: &InlineParagraphRun,
    source_id: InlinePositioningContainingBlockId,
) -> bool {
    let InlineLineItem::Atom(atom) = &run.item else {
        return false;
    };
    let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content() else {
        return false;
    };
    edge.logical_edge == InlineLogicalEdge::Start
        && edge.positioning_containing_block_id == Some(source_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_break_opportunity(kind: InlineBreakKind) -> InlineBreakOpportunity {
        InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(1),
            kind,
            priority: 0,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            discretionary: None,
            emergency: false,
            min_content: true,
        }
    }

    #[test]
    fn float_placement_is_not_a_soft_wrap() {
        assert!(opportunity_is_soft_wrap(test_break_opportunity(
            InlineBreakKind::AtomicBoundary,
        )));
        assert!(!opportunity_is_soft_wrap(test_break_opportunity(
            InlineBreakKind::FloatPlacement,
        )));
        assert!(!opportunity_is_soft_wrap(test_break_opportunity(
            InlineBreakKind::Forced,
        )));
    }

    #[test]
    fn balance_scores_remaining_line_space_not_source_width() {
        // A float can make a shorter source line better balanced because its
        // available measure is smaller.  The score must therefore be based on
        // the remaining space after each line's own exclusion and indent.
        let float_aware = balanced_remaining_space_variance(&[7.5, 9.5, 6.5]);
        let source_width_only = balanced_remaining_space_variance(&[2.0, 8.0, 13.0]);
        assert!(float_aware < source_width_only);
        assert_eq!(balanced_remaining_space_variance(&[4.0, 4.0]), 0.0);
    }

    #[test]
    fn initial_letter_uses_the_logical_block_start_column_in_vertical_modes() {
        assert_eq!(
            initial_letter_exclusion_side(WritingMode::VerticalRl, UsedFloatSide::Left),
            UsedFloatSide::Right
        );
        assert_eq!(
            initial_letter_exclusion_side(WritingMode::SidewaysRl, UsedFloatSide::Left),
            UsedFloatSide::Right
        );
        assert_eq!(
            initial_letter_exclusion_side(WritingMode::VerticalLr, UsedFloatSide::Right),
            UsedFloatSide::Left
        );
        assert_eq!(
            initial_letter_exclusion_side(WritingMode::SidewaysLr, UsedFloatSide::Right),
            UsedFloatSide::Left
        );
        assert_eq!(
            initial_letter_exclusion_side(WritingMode::HorizontalTb, UsedFloatSide::Right),
            UsedFloatSide::Right
        );
    }
}
