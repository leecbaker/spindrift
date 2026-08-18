use super::super::*;
use super::graph::{
    InlineFragmentContinuation, InlineGraphPosition, InlineLineFragment, MeasuredInlineItem,
};
use super::mixed::{CommittedInlineFloatReplay, InlineTextBoxMetrics};
use crate::css::{BoxDecorationBreak, TextBoxTrim, TextEdgeMetric};
use crate::layout::inline_collect::{
    insert_text_autospace_items, normalize_inline_whitespace_items, visible_hanging_edge_word_mut,
};
use crate::layout::text_paint::TextDecorationOriginFragmentGeometry;
use crate::units::{ContentBoxLength, content_box_pt, layout_points, layout_pt};
use std::rc::Rc;

/// The content-box extent occupied by a selected line sequence along the
/// physical inline axis.
///
/// This is intrinsic inline-content occupancy, not the used physical height
/// of its containing block. In a vertical writing mode those quantities share
/// an axis and units but have distinct sizing roles: a definite `height` must
/// not be replaced by the shorter (or overflowing) selected text sequence.
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct OccupiedPhysicalInlineExtent(ContentBoxLength);

impl OccupiedPhysicalInlineExtent {
    fn new(value: ContentBoxLength) -> Self {
        Self(value)
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0.points()
    }
}

/// The source-coordinate bounds painted from an inline line sequence.
#[derive(Clone, Copy, Debug)]
pub(in crate::layout) struct InlineLineSequenceSlice {
    pub(in crate::layout) block_top: f32,
    pub(in crate::layout) top: f32,
    pub(in crate::layout) bottom: f32,
}

fn text_combine_upright_text(text: &str, style: &ComputedStyle) -> Option<String> {
    if !matches!(
        style.text_layout_policy(),
        css::TextLayoutPolicy::Vertical(_)
    ) {
        return None;
    }
    match style.text_combine_upright {
        css::TextCombineUpright::None => None,
        css::TextCombineUpright::All => {
            // CSS Writing Modes processes edge white space in a tate-chu-yoko
            // composition exactly as it would in the horizontal inline-block
            // used to lay the composition out.  Collapsible white space has
            // already been normalized by the parent collector, but preserved
            // white space (for example `white-space: pre`) remains visible
            // input to the nested horizontal sequence.
            // <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
            let text = if style.white_space.collapses_spaces() {
                text.trim_matches(crate::text::is_css_collapsible_whitespace)
                    .to_owned()
            } else {
                text.to_owned()
            };
            (!text.is_empty()).then_some(text)
        }
        css::TextCombineUpright::Digits(limit)
            if !text.is_empty()
                // The CSS limit counts typographic character units, not UTF-8
                // bytes.  Every ASCII digit is one unit; using byte length
                // here would be incorrect for a non-ASCII typographic unit
                // admitted by a future value extension.
                // <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
                && text.chars().count() <= usize::from(limit)
                && text.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            Some(text.to_owned())
        }
        css::TextCombineUpright::Digits(_) => None,
    }
}

/// Undo the full-width transform before compressing a multi-character tate-chu-yoko run.
///
/// A single full-width character already fills the em square and remains
/// unchanged. For a longer composition, CSS Writing Modes first returns the
/// full-width ASCII forms to their ordinary forms, so width-alternative
/// OpenType features can select the appropriate compressed glyphs.
/// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
fn reverse_full_width_transform_for_text_combine(text: &str) -> String {
    if text.chars().nth(1).is_none() {
        return text.to_owned();
    }
    text.chars()
        .map(|character| match character {
            '\u{3000}' => ' ',
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(u32::from(character) - 0xfee0)
                .expect("full-width ASCII maps to a valid scalar"),
            _ => character,
        })
        .collect()
}

/// Returns whether two normalized text items may belong to one
/// `text-combine-upright` composition.
///
/// Text combine is scoped to an inline formatting context rather than to the
/// implementation's word-token boundaries.  The collector may split one
/// author text run around whitespace processing or generated-content
/// expansion, so retain a composition only while every paint- and
/// bidi-relevant property is identical.  In particular, do not join link or
/// decoration boundaries: the outer atomic inline owns exactly one link and
/// decoration range. `InlineHangingEdges` is deliberately excluded: it is
/// paint-edge ownership assigned after whitespace normalization, rather than
/// a source inline boundary. Actual inline box edges remain `InlineItem`s and
/// therefore still delimit this run.
/// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
fn text_combine_upright_words_are_compatible(first: &InlineWord, next: &InlineWord) -> bool {
    first.style.as_ref() == next.style.as_ref()
        && first.baseline_shift == next.baseline_shift
        && first.visual_offset == next.visual_offset
        && first.link_target == next.link_target
        && first.mergeable == next.mergeable
        && first.source == next.source
        && first.ancestor_inline_decorations == next.ancestor_inline_decorations
}

/// Returns the exclusive end of the contiguous normalized text run starting
/// at `start` that may be considered by `text-combine-upright`.
///
/// Only adjacent text items participate. In particular, a transparent inline
/// box-edge atom remains a source boundary even though it does not paint.
fn text_combine_upright_contiguous_word_end(items: &[InlineItem], start: usize) -> usize {
    let Some(InlineItem::Word(first_word)) = items.get(start) else {
        return start;
    };

    let mut end = start + 1;
    while let Some(InlineItem::Word(next)) = items.get(end) {
        if !text_combine_upright_words_are_compatible(first_word, next)
            || text_combine_upright_text_has_bidi_controls(&next.text)
        {
            break;
        }
        end += 1;
    }
    end
}

/// Directional formatting characters are invisible UBA input, not text that
/// CSS Writing Modes permits a tate-chu-yoko run to absorb.  They are emitted
/// as ordinary inline words by the bidi-scope collector, so make them an
/// explicit composition boundary before selecting `all` or `digits`.
fn text_combine_upright_text_has_bidi_controls(text: &str) -> bool {
    text.chars().any(|character| {
        matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
    })
}

/// Whether an item can create a later in-flow line in the surrounding clamp
/// stream. Floats and static-position placeholders are source-order layout
/// participants but do not themselves make a clamp overflow boundary.
fn inline_item_can_continue_line_clamp(item: &InlineItem) -> bool {
    match item {
        InlineItem::Word(_) | InlineItem::Break(_) => true,
        InlineItem::Atom(atom) => {
            !matches!(atom.content(), InlineAtomContent::StaticPositionPlaceholder)
        }
        InlineItem::Float(_) | InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => false,
    }
}

fn inline_items_have_later_line_clamp_source(items: &[InlineItem]) -> bool {
    items.iter().any(inline_item_can_continue_line_clamp)
}

fn inline_context_with_later_line_clamp_source(
    mut context: InlineParagraphContext<'_>,
    has_later_source: bool,
) -> InlineParagraphContext<'_> {
    if has_later_source {
        context.clamp_continuation = css::ClampContinuation::LaterInFlowContent;
    }
    context
}

/// Preserve an ancestor traversal's source-order continuation when the
/// computed style crosses the layout-style/zoom boundary.  The continuation
/// is a layout-only property of `LineLimitTraversal`, not a cascaded declaration.
fn clamp_continuation_for_style(style: &ComputedStyle) -> css::ClampContinuation {
    if style.automatic_block_boundary_marker.is_some() {
        // A block-flow controller selected the preceding inline endpoint
        // because a following sibling is outside the retained source prefix.
        // Carry that fact into line selection so the terminal marker is
        // fitted and painted on this otherwise complete inline graph.
        // <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
        return css::ClampContinuation::LaterInFlowContent;
    }
    if style
        .automatic_block_size_traversal
        .as_ref()
        .is_some_and(css::AutomaticBlockSizeTraversal::terminal_marker_when_full)
    {
        // A finite automatic allowance can end exactly at this child. The
        // controller recorded that later in-flow source is then discarded,
        // so the final retained inline line must fit and paint its marker.
        // <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
        return css::ClampContinuation::LaterInFlowContent;
    }
    style
        .line_limit_traversal
        .as_ref()
        .map_or(css::ClampContinuation::None, |clamp| clamp.continuation)
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn begin_clamp_line_slot_capture(&mut self) {
        self.clamp_line_slot_captures
            .push(ClampLineSlotCapture::default());
    }

    pub(in crate::layout) fn finish_clamp_line_slot_capture(&mut self) -> ClampLineSlotCapture {
        self.clamp_line_slot_captures
            .pop()
            .expect("each block line-slot capture must be balanced")
    }

    pub(in crate::layout) fn record_clamp_line_slots(&mut self, count: usize) {
        if let Some(capture) = self.clamp_line_slot_captures.last_mut() {
            capture.line_slots += count;
        }
    }

    fn record_inline_line_sequence_outcome(&mut self, sequence: &InlineLineSequence) {
        let outcome = sequence.layout_outcome();
        self.record_clamp_line_slots(outcome.clamp_line_slots);
        if let Some(capture) = self.clamp_line_slot_captures.last_mut() {
            capture.block_advance = content_box_pt(
                capture.block_advance.points() + outcome.clamp_block_advance.points(),
            );
        }
        if outcome.has_local_continuation_cutoff
            && let Some(capture) = self.clamp_line_slot_captures.last_mut()
        {
            capture.has_local_continuation_cutoff = true;
        }
    }

    pub(in crate::layout) fn layout_inline_items(
        &mut self,
        items: Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        hanging_indent: f32,
        stylesheets: &Stylesheets<'_>,
    ) -> InlineLayoutOutcome {
        self.layout_inline_items_with_first_formatted_line_policy(
            items,
            block_style,
            available_width,
            padding_left,
            hanging_indent,
            stylesheets,
            true,
        )
    }

    /// Select a replay-safe simple vertical inline stream.
    ///
    /// This deliberately accepts only ordinary words and atomic inlines.  The
    /// excluded boundaries and floats have source-order or formatting-context
    /// effects which must remain owned by the general inline layout path.
    /// The returned record sequence is therefore safe to select during an
    /// orthogonal block's intrinsic sizing and replay during its final paint.
    /// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
    pub(in crate::layout) fn select_replay_safe_vertical_inline_sequence(
        &mut self,
        items: &mut Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        hanging_indent: f32,
        stylesheets: &Stylesheets<'_>,
    ) -> Option<InlineLineSequence> {
        if !matches!(
            block_style.writing_mode,
            WritingMode::VerticalRl | WritingMode::VerticalLr
        ) || !items
            .iter()
            .all(|item| matches!(item, InlineItem::Word(_) | InlineItem::Atom(_)))
        {
            return None;
        }

        // Keep the item preparation and paragraph context in lockstep with
        // `layout_inline_items_with_first_formatted_line_policy`. The narrow
        // item set above excludes boundaries and floats, whose page and flow
        // effects require that general path.
        let used_block_style = css::LayoutStyle::from_computed(block_style).into_zoomed();
        let block_style = &used_block_style;
        self.prepare_inline_items_for_layout(items);
        let context = InlineParagraphContext {
            block_style,
            line_clamp: used_line_clamp_for_style(block_style),
            clamp_continuation: clamp_continuation_for_style(block_style),
            stylesheets,
            initial_first_formatted_line: true,
            available_width,
            padding_left,
            hanging_indent,
            hanging_punctuation_reserve: last_hanging_punctuation_width_for_inline_items(
                &mut self.font_system,
                items,
                block_style,
            ),
        };
        let sequence = self.collect_inline_line_sequence_for_items(items, context);
        (!sequence.has_flow_side_effects && !sequence.has_local_continuation_cutoff)
            .then_some(sequence)
    }

    /// Lay out a simple vertical inline stream and retain the selected line
    /// sequence for its block formatting context caller.
    ///
    /// An inside list marker, when present, participates in the first line
    /// just like principal inline content. Keeping this selected sequence lets
    /// the caller use the same logical-inline measurement for vertical block
    /// geometry that this method paints, rather than re-collecting generated
    /// content and text after layout.
    /// <https://drafts.csswg.org/css-lists-3/#marker-position>
    /// <https://drafts.csswg.org/css-writing-modes-4/#vertical-layout>
    pub(in crate::layout) fn try_layout_committed_vertical_inline_sequence(
        &mut self,
        items: &mut Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        hanging_indent: f32,
        stylesheets: &Stylesheets<'_>,
    ) -> Option<InlineLineSequence> {
        let sequence = self.select_replay_safe_vertical_inline_sequence(
            items,
            block_style,
            available_width,
            padding_left,
            hanging_indent,
            stylesheets,
        )?;
        self.paint_inline_line_sequence(&sequence, block_style);
        Some(sequence)
    }

    /// Lay out one anonymous inline run with its originating block's
    /// first-formatted-line state.
    ///
    /// CSS 2.2 anonymous blocks created around mixed inline/block children
    /// are layout artifacts. They must not make a later inline run become a
    /// new first formatted line for `text-indent`:
    /// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_inline_items_with_first_formatted_line_policy(
        &mut self,
        mut items: Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        hanging_indent: f32,
        stylesheets: &Stylesheets<'_>,
        initial_first_formatted_line: bool,
    ) -> InlineLayoutOutcome {
        // Inline line boxes consume the block's line-height and indentation
        // directly. Materialize their used CSS `zoom` scale at this boundary;
        // run styles have the same idempotent conversion when they are shaped.
        // <https://drafts.csswg.org/css-viewport/#zoom-property>
        let used_block_style = css::LayoutStyle::from_computed(block_style).into_zoomed();
        let block_style = &used_block_style;
        if initial_first_formatted_line
            && block_style
                .first_letter_style
                .as_deref()
                .is_some_and(|first_letter| !first_letter.initial_letter.is_normal())
        {
            self.discard_provisional_initial_letter_exclusions(block_style);
        }
        if block_style.writing_mode == WritingMode::HorizontalTb
            && initial_first_formatted_line
            && block_style
                .first_letter_style
                .as_deref()
                .is_some_and(|first_letter| !first_letter.initial_letter.is_normal())
        {
            self.clear_initial_letter_exclusions_for_new_initial(block_style);
        }
        self.prepare_inline_items_for_layout(&mut items);
        let context = InlineParagraphContext {
            block_style,
            line_clamp: used_line_clamp_for_style(block_style),
            clamp_continuation: clamp_continuation_for_style(block_style),
            stylesheets,
            initial_first_formatted_line,
            available_width,
            padding_left,
            hanging_indent,
            hanging_punctuation_reserve: last_hanging_punctuation_width_for_inline_items(
                &mut self.font_system,
                &items,
                block_style,
            ),
        };
        // A pre-collected line sequence has no page-group transition model.
        // Named inline page scopes must go through the boundary-aware
        // paragraph path below, which flushes before both entering and leaving
        // each scope.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        let automatic_clamp_requires_collected_reflow = context.line_clamp.is_none()
            && matches!(
                block_style.used_continuation(),
                css::UsedContinuation::LineClamp(css::LineClampContainer {
                    cutoff: css::ClampPointRule::AutomaticBlockSize,
                    ..
                }) | css::UsedContinuation::Discard(css::DiscardFragmentationContainer {
                    max_lines: css::MaxLines::None,
                    ..
                })
            );
        if !inline_items_have_page_scope(&items)
            && (automatic_clamp_requires_collected_reflow
                || !self.current_text_box_line_trim().is_empty()
                || inline_items_can_fragment_as_collected_lines(&items))
        {
            let sequence = self.collect_inline_line_sequence_for_items(&items, context);
            self.paint_inline_line_sequence(&sequence, block_style);
            return sequence.layout_outcome();
        }
        let mut outcome = InlineLayoutOutcome::default();
        let mut paragraph = Vec::<InlineItem>::new();
        let mut line_index = 0usize;
        let mut next_paragraph_starts_after_forced_break = false;
        let mut page_scopes = Vec::new();
        let mut plaintext_direction_state = None;
        for (item_index, item) in items.iter().cloned().enumerate() {
            // This direct layout path also splits the shared source stream at
            // preserved breaks. Carry source that follows the boundary into
            // the terminal-line selector just as the collected-line path
            // does; otherwise a final `white-space: pre` line commits before
            // it can reserve the block ellipsis.
            let context_before_boundary = inline_context_with_later_line_clamp_source(
                context,
                inline_items_have_later_line_clamp_source(&items[item_index + 1..]),
            );
            match inline_item_boundary_role(&item) {
                InlineBoundaryRole::ForcedBreak => {
                    let clear = inline_break_clear(&item);
                    let force_empty_line = clear == Clear::None;
                    let paragraph_outcome = self.flush_inline_item_paragraph(
                        &mut paragraph,
                        context_before_boundary,
                        line_index,
                        force_empty_line,
                        next_paragraph_starts_after_forced_break,
                        &mut plaintext_direction_state,
                    );
                    line_index = paragraph_outcome.next_line_index;
                    outcome.include(paragraph_outcome);
                    line_index = self.apply_inline_break_clearance(clear, context, line_index);
                    if context.block_style.unicode_bidi == UnicodeBidi::Plaintext {
                        plaintext_direction_state = None;
                    }
                    next_paragraph_starts_after_forced_break = true;
                }
                InlineBoundaryRole::PageScopeStart => {
                    let InlineItem::PageScopeStart(page_name) = item else {
                        unreachable!("page-scope boundary role must come from PageScopeStart")
                    };
                    trim_inline_item_edges(&mut paragraph);
                    let flushed_paragraph = !paragraph.is_empty();
                    let paragraph_outcome = self.flush_inline_item_paragraph(
                        &mut paragraph,
                        context_before_boundary,
                        line_index,
                        false,
                        next_paragraph_starts_after_forced_break,
                        &mut plaintext_direction_state,
                    );
                    line_index = paragraph_outcome.next_line_index;
                    outcome.include(paragraph_outcome);
                    if flushed_paragraph {
                        next_paragraph_starts_after_forced_break = false;
                    }
                    page_scopes.push(self.enter_inline_page_name_scope(page_name.as_deref()));
                }
                InlineBoundaryRole::PageScopeEnd => {
                    trim_inline_item_edges(&mut paragraph);
                    let flushed_paragraph = !paragraph.is_empty();
                    let paragraph_outcome = self.flush_inline_item_paragraph(
                        &mut paragraph,
                        context_before_boundary,
                        line_index,
                        false,
                        next_paragraph_starts_after_forced_break,
                        &mut plaintext_direction_state,
                    );
                    line_index = paragraph_outcome.next_line_index;
                    outcome.include(paragraph_outcome);
                    if flushed_paragraph {
                        next_paragraph_starts_after_forced_break = false;
                    }
                    if let Some(scope) = page_scopes.pop() {
                        self.exit_inline_page_name_scope(scope);
                    }
                }
                InlineBoundaryRole::Float => {
                    paragraph.push(item);
                }
                _ => paragraph.push(item),
            }
        }
        let paragraph_outcome = self.flush_inline_item_paragraph(
            &mut paragraph,
            context,
            line_index,
            false,
            next_paragraph_starts_after_forced_break,
            &mut plaintext_direction_state,
        );
        outcome.include(paragraph_outcome);
        while let Some(scope) = page_scopes.pop() {
            self.exit_inline_page_name_scope(scope);
        }
        outcome
    }

    /// Collect reusable line fragments for one inline formatting context.
    ///
    /// CSS Inline creates line boxes from inline-level content, CSS Text
    /// chooses break opportunities, CSS Fragmentation then chooses which line
    /// boxes fit a fragmentainer using `orphans` and `widows`, and PDF text
    /// emission consumes the same shaped fragments for painting:
    /// <https://www.w3.org/TR/css-inline-3/#line-box>,
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>,
    /// <https://www.w3.org/TR/css-break-3/#widows-orphans>, and
    /// ISO 32000-2:2020, 9.4 "Text".
    pub(in crate::layout) fn collect_inline_line_sequence(
        &mut self,
        mut items: Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        hanging_indent: f32,
    ) -> InlineLineSequence {
        self.prepare_inline_items_for_layout(&mut items);
        if block_style.writing_mode == WritingMode::HorizontalTb
            && block_style
                .first_letter_style
                .as_deref()
                .is_some_and(|first_letter| !first_letter.initial_letter.is_normal())
        {
            // This is the durable sequence-building path for a block's
            // first formatted line. Clear a preceding in-flow initial before
            // fitting this new one, while leaving ordinary following blocks
            // free to wrap around the existing exclusion.
            self.clear_initial_letter_exclusions_for_new_initial(block_style);
        }
        let context = InlineParagraphContext {
            block_style,
            line_clamp: used_line_clamp_for_style(block_style),
            clamp_continuation: clamp_continuation_for_style(block_style),
            stylesheets: &css::EMPTY_STYLESHEETS,
            initial_first_formatted_line: true,
            available_width,
            padding_left,
            hanging_indent,
            hanging_punctuation_reserve: last_hanging_punctuation_width_for_inline_items(
                &mut self.font_system,
                &items,
                block_style,
            ),
        };
        self.collect_inline_line_sequence_for_items(&items, context)
    }

    pub(in crate::layout) fn collect_inline_line_sequence_with_text_box_trim(
        &mut self,
        items: Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        hanging_indent: f32,
    ) -> InlineLineSequence {
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(block_style);
        self.with_text_box_line_trim_scope(text_box_line_trim, |layout| {
            layout.collect_inline_line_sequence(
                items,
                block_style,
                available_width,
                padding_left,
                hanging_indent,
            )
        })
    }

    /// Form `text-combine-upright` runs after CSS Text whitespace processing
    /// but before UAX #14 opportunity selection and shaping of the containing
    /// paragraph.  The horizontal child sequence is intentionally retained as
    /// an atomic inline rather than faking a late paint rotation, so it cannot
    /// split across lines or participate as multiple vertical glyph units.
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
    pub(in crate::layout) fn form_text_combine_upright_atoms(
        &mut self,
        items: &mut Vec<InlineItem>,
    ) {
        let mut output = Vec::with_capacity(items.len());
        let source_items = std::mem::take(items);
        let mut index = 0;
        while index < source_items.len() {
            let InlineItem::Word(first_word) = &source_items[index] else {
                output.push(source_items[index].clone());
                index += 1;
                continue;
            };

            // Bidi controls delimit the source scope that they establish.
            // They must remain in the UAX #9 input stream even when the
            // surrounding visible text is eligible for `text-combine-upright`.
            if text_combine_upright_text_has_bidi_controls(&first_word.text) {
                output.push(source_items[index].clone());
                index += 1;
                continue;
            }

            let style = first_word.style.as_ref();
            if !matches!(
                style.text_combine_upright,
                css::TextCombineUpright::All | css::TextCombineUpright::Digits(_)
            ) || !matches!(
                style.text_layout_policy(),
                css::TextLayoutPolicy::Vertical(_)
            ) {
                output.push(source_items[index].clone());
                index += 1;
                continue;
            }

            // Accumulate a scoped normalized text run.  `all` can include
            // collapsed interior spaces; `digits` is rejected below unless
            // the complete run is an eligible ASCII digit sequence.
            let end = text_combine_upright_contiguous_word_end(&source_items, index);
            let source_text = source_items[index..end]
                .iter()
                .map(|item| match item {
                    InlineItem::Word(word) => word.text.as_str(),
                    _ => unreachable!("a TCY text run contains only words"),
                })
                .collect::<String>();

            let Some(text) = text_combine_upright_text(&source_text, style) else {
                output.extend(source_items[index..end].iter().cloned());
                index = end;
                continue;
            };
            // CSS Text's source transformation precedes the text-combine
            // full-width reversal. Apply it once to the composed source,
            // then leave the nested horizontal sequence with already-used
            // text so it cannot transform the run a second time.
            // <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
            // <https://drafts.csswg.org/css-text-3/#text-transform-property>
            let transformed_text = transform_text(&text, style);
            let horizontal_text = reverse_full_width_transform_for_text_combine(&transformed_text);

            let mut horizontal_style = style.clone();
            horizontal_style.writing_mode = WritingMode::HorizontalTb;
            horizontal_style.text_orientation = css::TextOrientation::Mixed;
            horizontal_style.text_combine_upright = css::TextCombineUpright::None;
            horizontal_style.text_transform = css::TextTransform::NONE;
            // CSS Writing Modes composes the nested run like a horizontal
            // inline-block with `line-height: 1em`, ignoring letter spacing.
            // Preserve the rest of the selected font state, including author
            // feature settings and white-space handling.
            // <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
            horizontal_style.line_height_value = css::ComputedLineHeight::Number(1.0);
            horizontal_style.line_height = horizontal_style.font_size;
            horizontal_style.letter_spacing = css::ComputedLengthPercentage::ZERO;
            // Tate-chu-yoko composes its horizontal run in a one-em square.
            // Center the uncompressed run there before the paint-time scale
            // maps an over-wide run back to that same square.
            // <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
            horizontal_style.text_align = TextAlign::Center;
            // For two through four typographic characters, CSS Writing Modes
            // selects the matching OpenType width alternative before any
            // residual geometric compression. Fonts may provide narrower,
            // better-shaped forms than a uniform scale can produce.
            // <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
            let compression_feature = match horizontal_text.chars().count() {
                2 => Some(*b"hwid"),
                3 => Some(*b"twid"),
                4 => Some(*b"qwid"),
                _ => None,
            };
            if let Some(feature) = compression_feature {
                horizontal_style
                    .font_feature_settings
                    .0
                    .push(css::FontFeatureSetting::new(feature, 1));
            }
            let horizontal_width = self
                .font_system
                .measure_text(&horizontal_text, &horizontal_style)
                .max(horizontal_style.font_size)
                .max(1.0);
            let horizontal_word = InlineWord {
                text: horizontal_text,
                style: inline_style(&horizontal_style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                // The outer atomic box owns the link rectangle.  Retaining a
                // second link on the nested line would duplicate annotations.
                link_target: None,
                mergeable: false,
                // The nested horizontal sequence is already the used text of
                // one composition. Generated-content provenance belongs to
                // the outer atom; retaining it here would make the normal
                // horizontal line builder apply generated-text source rules a
                // second time and diverge from an equivalent authored run.
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Rc::clone(&first_word.ancestor_inline_decorations),
            };
            let sequence = self.collect_inline_line_sequence_with_text_box_trim(
                vec![InlineItem::Word(Box::new(horizontal_word))],
                &horizontal_style,
                horizontal_width,
                0.0,
                0.0,
            );
            let em = style.font_size.max(0.0);
            // A combined composition aligns its one-em square to the parent
            // inline box's central baseline before `vertical-align` shifts it.
            // The atom has no margin, so its logical block-start-to-baseline
            // offset is exactly half of the square's block extent.
            // <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
            let baseline_offset = em * 0.5;
            let atom = InlineAtom::new(
                InlineAtomContent::TextCombineUpright {
                    sequence,
                    horizontal_style: Box::new(horizontal_style),
                    inline_scale: (em / horizontal_width).min(1.0),
                },
                style.clone(),
                None,
                InlineSize::new(em, em),
                baseline_offset,
                first_word.baseline_shift,
                first_word.link_target.as_deref().map(ToOwned::to_owned),
                None,
            )
            .with_visual_offset(first_word.visual_offset);
            output.push(InlineItem::Atom(Box::new(atom)));
            index = end;
        }
        *items = output;
    }

    /// Apply the shared CSS Text preprocessing before line selection.
    fn prepare_inline_items_for_layout(&mut self, items: &mut Vec<InlineItem>) {
        normalize_inline_whitespace_items(items);
        self.form_text_combine_upright_atoms(items);
        insert_text_autospace_items(
            &mut self.font_system,
            &mut self.autospace_items_scratch,
            items,
        );
        trim_inline_item_edges(items);
    }

    fn collect_inline_line_sequence_for_items(
        &mut self,
        items: &[InlineItem],
        context: InlineParagraphContext<'_>,
    ) -> InlineLineSequence {
        // The automatic block-size cutoff is selected before `text-wrap:
        // balance` redistributes the surviving source. Measuring an already
        // balanced *complete* stream lets discarded content alter line block
        // advances and can select an earlier clamp point. Probe with ordinary
        // wrapping, then reselect the terminal source endpoint with the
        // authored balancing policy below.
        // <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
        // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
        let automatic_balance_probe = context.line_clamp.is_none()
            && matches!(
                context.block_style.text_wrap_style,
                css::TextWrapStyle::Balance
            );
        let mut unbalanced_probe_style = context.block_style.clone();
        if automatic_balance_probe {
            unbalanced_probe_style.text_wrap_style = css::TextWrapStyle::Auto;
        }
        let probe_context = if automatic_balance_probe {
            InlineParagraphContext {
                block_style: &unbalanced_probe_style,
                ..context
            }
        } else {
            context
        };
        let sequence = self.collect_inline_line_sequence_for_items_once(items, probe_context);
        let automatic_clamp = context
            .line_clamp
            .is_none()
            .then(|| self.select_automatic_inline_clamp(&sequence, context.block_style))
            .flatten();
        // Balancing can increase the selected source's block extent even when
        // the initial stable (unbalanced) pass fit in full. In that case the
        // balanced result must be measured before deciding that there is no
        // clamp point; otherwise discarded source survives simply because it
        // was absent from the first cutoff decision.
        // <https://drafts.csswg.org/css-overflow-4/#continue>
        let (mut selected_clamp, mut clamped) = match automatic_clamp {
            Some(selected_clamp) => {
                let automatic_context = InlineParagraphContext {
                    line_clamp: Some(css::InlineLineClamp::Automatic(selected_clamp)),
                    ..context
                };
                (
                    selected_clamp,
                    self.collect_inline_line_sequence_for_items_once(items, automatic_context),
                )
            }
            None if automatic_balance_probe => {
                let balanced = self.collect_inline_line_sequence_for_items_once(items, context);
                let Some(selected_clamp) =
                    self.select_automatic_inline_clamp(&balanced, context.block_style)
                else {
                    return balanced;
                };
                let automatic_context = InlineParagraphContext {
                    line_clamp: Some(css::InlineLineClamp::Automatic(selected_clamp)),
                    ..context
                };
                (
                    selected_clamp,
                    self.collect_inline_line_sequence_for_items_once(items, automatic_context),
                )
            }
            None => return sequence,
        };
        // Re-select from the source graph with the selected endpoint. The
        // terminal line therefore reserves and fits the marker through the
        // ordinary line-selection path instead of truncating materialized
        // records after their source ranges and Phase-II whitespace effects
        // have already been committed.
        // Balancing is performed after the initial source cutoff. It can move
        // a tall inline box onto the terminal line, so re-measure that
        // surviving sequence and narrow the endpoint when it no longer fits.
        // The selected endpoint cannot grow here: the first unbalanced pass
        // already selected the furthest source point admitted by the used
        // block-size constraint.
        // <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
        if automatic_balance_probe {
            while let Some(reclamp) =
                self.select_automatic_inline_clamp(&clamped, context.block_style)
            {
                if css::InlineLineClamp::Automatic(reclamp).max_lines()
                    >= css::InlineLineClamp::Automatic(selected_clamp).max_lines()
                {
                    break;
                }
                selected_clamp = reclamp;
                let reclamped_context = InlineParagraphContext {
                    line_clamp: Some(css::InlineLineClamp::Automatic(selected_clamp)),
                    ..context
                };
                clamped =
                    self.collect_inline_line_sequence_for_items_once(items, reclamped_context);
            }
        }
        // Forced-break collection can encounter a source boundary after the
        // selected terminal line. That boundary must not leave an empty line
        // record in the committed sequence: automatic clamping selects a
        // source endpoint, not merely paint suppression for later records.
        // Keeping this filter at the controller boundary also protects block
        // auto-sizing, which consumes record block advances.
        // <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
        let automatic_controller = css::InlineLineClamp::Automatic(selected_clamp);
        clamped
            .records
            .retain(|record| !automatic_controller.excludes_line(record.block_line_index));
        clamped.has_local_continuation_cutoff = true;
        clamped
    }

    /// Select an automatic clamp endpoint from the measured, used block
    /// advances of the complete inline sequence. The return type carries an
    /// endpoint, not a synthesized line quota.
    /// <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
    fn select_automatic_inline_clamp<'style>(
        &self,
        sequence: &InlineLineSequence,
        style: &'style ComputedStyle,
    ) -> Option<css::AutomaticLineClamp<'style>> {
        // A multicol container owns several local regions. Collapse is
        // inapplicable there, while discard must wait for the multicol
        // planner to select overflow after the complete first set of columns
        // rather than clamping at a single-column height.
        // <https://drafts.csswg.org/css-overflow-4/#continue>
        if style_establishes_multicol_formatting_context(style) {
            return None;
        }
        if let Some(css::AutomaticBlockBoundaryMarker(marker)) =
            style.automatic_block_boundary_marker.as_ref()
        {
            let record = sequence.records.last()?;
            return Some(
                record
                    .fragment
                    .as_ref()
                    .and_then(|fragment| fragment.source_end)
                    .map(|source_end| {
                        css::AutomaticLineClamp::after_measured_source_line(
                            record.block_line_index,
                            css::InlineSourceEndpoint::at_graph_boundary(
                                source_end.run_index,
                                source_end.byte_offset,
                            ),
                            marker,
                        )
                    })
                    .unwrap_or_else(|| {
                        css::AutomaticLineClamp::after_measured_line(
                            record.block_line_index,
                            marker,
                        )
                    }),
            );
        }
        let inherited_automatic = style.automatic_block_size_traversal.as_ref();
        let marker = match inherited_automatic {
            // An ancestor's automatic controller owns the cutoff while this
            // eligible descendant supplies the same-formatting-context line
            // stream. The descendant's computed longhands remain untouched.
            Some(traversal) => traversal.marker(),
            None => match style.used_continuation() {
                css::UsedContinuation::LineClamp(container)
                    if matches!(container.cutoff, css::ClampPointRule::AutomaticBlockSize) =>
                {
                    container.marker
                }
                // A discard container captures its first local Category-3 break.
                // For an unforced break, the finite used block constraint selects
                // that endpoint exactly as it does for an automatic clamp. The
                // subsequent source is omitted locally; no page or column cursor
                // is materialized here.
                // <https://drafts.csswg.org/css-overflow-4/#continue-discard>
                css::UsedContinuation::Discard(discard)
                    if matches!(discard.max_lines, css::MaxLines::None) =>
                {
                    discard.marker
                }
                _ => return None,
            },
        };
        // This direct inline controller can resolve absolute and font-relative
        // block constraints. Percentage-dependent constraints require the
        // containing block's final percentage basis and are selected by the
        // enclosing block-flow controller instead.
        // The style-resolution path normally resolves `lh` before this
        // point. Re-resolve cloned size values at this layout boundary as a
        // defensive adapter for root and anonymous styles that retain a
        // deferred line-height unit until their final used line height is
        // known.
        if let Some(traversal) = inherited_automatic {
            let finite_constraint = traversal.remaining().points();
            return self.select_automatic_inline_clamp_with_constraint(
                sequence,
                finite_constraint,
                marker,
                traversal.terminal_marker_when_full(),
            );
        }
        let mut direct_used_height = style.box_values.height.clone();
        let mut direct_used_min_height = style.box_values.min_height.clone();
        let mut direct_used_max_height = style.box_values.max_height.clone();
        let used_line_height = layout_pt(style.line_height);
        direct_used_height.resolve_line_height_relative_lengths(used_line_height);
        direct_used_min_height.resolve_line_height_relative_lengths(used_line_height);
        direct_used_max_height.resolve_line_height_relative_lengths(used_line_height);
        // `line-clamp: auto` observes the final used block-size constraint:
        // `min-height` wins when it exceeds a specified height or max-height.
        // A lone min-height is not an upper bound and therefore cannot create
        // an automatic cutoff.
        // <https://drafts.csswg.org/css-sizing-3/#min-size-auto>
        let direct_upper_constraint = direct_used_height
            .length_if_no_percent()
            .into_iter()
            .chain(direct_used_max_height.length_if_no_percent())
            .reduce(f32::min);
        let finite_constraint = direct_upper_constraint
            .map(|upper| {
                direct_used_min_height
                    .length_if_no_percent()
                    .map_or(upper, |min| upper.max(min))
            })
            .or_else(|| {
                // The top stack entry describes descendants of the current
                // formatting context. A percentage height on this context's
                // own inline contents resolves against the containing
                // context immediately below it.
                self.definite_block_size_stack
                    .iter()
                    .rev()
                    .nth(1)
                    .copied()
                    .and_then(|basis| {
                        let upper = used_length_percentage_or_auto(
                            style.box_values.height.value().clone(),
                            basis,
                        )
                        .map(SemanticLengthExt::points);
                        let max_height =
                            used_max_height(style, basis).map(SemanticLengthExt::points);
                        let upper = upper.into_iter().chain(max_height).reduce(f32::min)?;
                        let min_height =
                            used_min_height(style, basis).map(SemanticLengthExt::points);
                        Some(min_height.map_or(upper, |min| upper.max(min)))
                    })
            })?;
        self.select_automatic_inline_clamp_with_constraint(
            sequence,
            finite_constraint,
            marker,
            false,
        )
    }

    /// Select from one measured sequence using a finite used content-box
    /// block-size constraint. The source-endpoint controller is shared by an
    /// automatic clamp container and by an eligible descendant receiving its
    /// remaining layout allowance.
    fn select_automatic_inline_clamp_with_constraint<'style>(
        &self,
        sequence: &InlineLineSequence,
        finite_constraint: f32,
        marker: &'style css::BlockEllipsis,
        terminal_marker_when_full: bool,
    ) -> Option<css::AutomaticLineClamp<'style>> {
        let mut used_block_size = 0.0;
        let mut preceding_line: Option<(usize, Option<InlineGraphPosition>)> = None;
        for record in &sequence.records {
            let candidate = used_block_size + record.block_advance();
            if candidate > finite_constraint + 0.01 {
                return Some(match preceding_line {
                    Some((block_line_index, Some(source_end))) => {
                        css::AutomaticLineClamp::after_measured_source_line(
                            block_line_index,
                            css::InlineSourceEndpoint::at_graph_boundary(
                                source_end.run_index,
                                source_end.byte_offset,
                            ),
                            marker,
                        )
                    }
                    Some((block_line_index, None)) => {
                        css::AutomaticLineClamp::after_measured_line(block_line_index, marker)
                    }
                    None => css::AutomaticLineClamp::at_container_start(marker),
                });
            }
            used_block_size = candidate;
            preceding_line = Some((
                record.block_line_index,
                record
                    .fragment
                    .as_ref()
                    .and_then(|fragment| fragment.source_end),
            ));
        }
        if terminal_marker_when_full && used_block_size >= finite_constraint - 0.01 {
            return preceding_line.map(|(block_line_index, source_end)| match source_end {
                Some(source_end) => css::AutomaticLineClamp::after_measured_source_line(
                    block_line_index,
                    css::InlineSourceEndpoint::at_graph_boundary(
                        source_end.run_index,
                        source_end.byte_offset,
                    ),
                    marker,
                ),
                None => css::AutomaticLineClamp::after_measured_line(block_line_index, marker),
            });
        }
        None
    }

    fn collect_inline_line_sequence_for_items_once(
        &mut self,
        items: &[InlineItem],
        context: InlineParagraphContext<'_>,
    ) -> InlineLineSequence {
        let mut records = Vec::new();
        let mut paragraph = Vec::<InlineItem>::new();
        // Explicit breaks are collected as separate opportunity graphs. Keep
        // cloneable inline scopes open across that collection boundary so the
        // preceding and following graphs receive their fragment-local end and
        // start edges respectively.
        // <https://www.w3.org/TR/css-break-3/#break-decoration>
        let mut active_fragment_scopes = Vec::<Option<InlineFragmentContinuation>>::new();
        let mut pending_fragment_starts = Vec::<InlineFragmentContinuation>::new();
        let mut pending_clone_fragment_start_edge = false;
        let mut cursor = InlineLineSequenceCursor {
            paragraph_index: 0,
            line_index: 0,
            physical_block_offset: 0.0,
            starts_after_forced_break: false,
            starts_after_preserved_segment_break: false,
            has_flow_side_effects: false,
            pending_inline_float_replay: None,
        };
        for (item_index, item) in items.iter().enumerate() {
            if paragraph.is_empty() && !pending_fragment_starts.is_empty() {
                for continuation in pending_fragment_starts.drain(..) {
                    paragraph.push(continuation.start_item());
                }
                pending_clone_fragment_start_edge = true;
            }
            // The collector splits one block's inline stream at preserved
            // breaks, floats, and page-scope boundaries. Let a paragraph
            // that reaches the terminal slot see real later source before it
            // is selected, so the final source range can be refit with the
            // marker reservation rather than patched after paint.
            let context_before_boundary = inline_context_with_later_line_clamp_source(
                context,
                inline_items_have_later_line_clamp_source(&items[item_index + 1..]),
            );
            match inline_item_boundary_role(item) {
                InlineBoundaryRole::ForcedBreak => {
                    for continuation in active_fragment_scopes.iter().flatten().rev() {
                        paragraph.push(continuation.end_item());
                    }
                    if active_fragment_scopes.iter().any(Option::is_some) {
                        mark_last_visible_inline_word_clone_end(&mut paragraph);
                    }
                    let clear = inline_break_clear(item);
                    let force_empty_line = clear == Clear::None;
                    let record_count_before_break = records.len();
                    cursor = self.collect_inline_paragraph_lines(
                        &mut paragraph,
                        context_before_boundary,
                        cursor,
                        force_empty_line,
                        &mut records,
                    );
                    if clear != Clear::None {
                        if records.len() > record_count_before_break {
                            if let Some(record) = records.last_mut() {
                                record.clear_after = clear;
                            }
                        } else {
                            records.push(clearance_only_inline_line_record(cursor, context, clear));
                        }
                    }
                    cursor.paragraph_index += 1;
                    cursor.starts_after_forced_break = true;
                    cursor.starts_after_preserved_segment_break = matches!(
                        item,
                        InlineItem::Break(InlineBreak {
                            origin: InlineBreakOrigin::PreservedSegment,
                            ..
                        })
                    );
                    pending_fragment_starts =
                        active_fragment_scopes.iter().flatten().cloned().collect();
                }
                role if role == InlineBoundaryRole::Float || role.is_page_scope() => {
                    let next_cursor = self.collect_inline_paragraph_lines(
                        &mut paragraph,
                        context_before_boundary,
                        cursor,
                        false,
                        &mut records,
                    );
                    if next_cursor.line_index != cursor.line_index {
                        cursor = next_cursor;
                        cursor.paragraph_index += 1;
                        cursor.starts_after_forced_break = false;
                        cursor.starts_after_preserved_segment_break = false;
                    }
                    if role == InlineBoundaryRole::Float {
                        paragraph.push(item.clone());
                    }
                }
                _ => {
                    update_inline_fragment_continuation_scopes(&mut active_fragment_scopes, item);
                    paragraph.push(item.clone());
                    if pending_clone_fragment_start_edge
                        && mark_first_visible_inline_word_clone_start(&mut paragraph)
                    {
                        pending_clone_fragment_start_edge = false;
                    }
                }
            }
        }
        cursor = self.collect_inline_paragraph_lines(
            &mut paragraph,
            context,
            cursor,
            false,
            &mut records,
        );
        recover_css_bidi_scope_continuations_across_forced_breaks(&mut records);
        let (records, fragment_text_box_trim) =
            self.with_text_box_line_trim_applied(records, context.block_style);
        InlineLineSequence {
            records,
            available_width: context.available_width,
            padding_left: context.padding_left,
            hanging_indent: context.hanging_indent,
            hanging_punctuation_reserve: context.hanging_punctuation_reserve,
            fragment_text_box_trim,
            has_flow_side_effects: cursor.has_flow_side_effects,
            replay_float_scope: ReplayFloatScope::InheritContainingBlock,
            has_local_continuation_cutoff: false,
        }
    }

    pub(in crate::layout) fn current_text_box_line_trim(&self) -> TextBoxLineTrim {
        self.text_box_line_trim_stack
            .last()
            .cloned()
            .unwrap_or_default()
    }

    pub(in crate::layout) fn push_text_box_line_trim_scope(
        &mut self,
        trim: TextBoxLineTrim,
    ) -> bool {
        if trim.is_empty() {
            return false;
        }
        self.text_box_line_trim_stack.push(trim);
        true
    }

    pub(in crate::layout) fn pop_text_box_line_trim_scope(&mut self, pushed: bool) {
        if pushed {
            self.text_box_line_trim_stack.pop();
        }
    }

    pub(in crate::layout) fn with_text_box_line_trim_scope<R>(
        &mut self,
        trim: TextBoxLineTrim,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let pushed = self.push_text_box_line_trim_scope(trim);
        let result = f(self);
        self.pop_text_box_line_trim_scope(pushed);
        result
    }

    /// Return the trim requested by the active ancestor and this block style.
    ///
    /// CSS Inline 3 says that if multiple ancestor block containers trim the
    /// same side of a line, the innermost block container's `text-box-edge`
    /// supplies the metric for that side:
    /// <https://drafts.csswg.org/css-inline-3/#text-box-trim>.
    pub(in crate::layout) fn effective_text_box_line_trim_for_style(
        &mut self,
        style: &ComputedStyle,
    ) -> TextBoxLineTrim {
        let inherited = self.current_text_box_line_trim();
        let own = self.text_box_line_trim_for_style(style);
        // `text-box-edge` is inherited independently from `text-box-trim`.
        // Propagated trim sides keep their request, but resolve metrics
        // against the containing block that owns the affected line.
        let inherited_metric = if (inherited.trims_block_start && !own.trims_block_start)
            || (inherited.trims_block_end && !own.trims_block_end)
        {
            self.text_box_trim_amounts_for_style(style)
        } else {
            TextBoxLineTrim::default()
        };
        TextBoxLineTrim {
            trims_block_start: own.trims_block_start || inherited.trims_block_start,
            trims_block_end: own.trims_block_end || inherited.trims_block_end,
            block_start: if own.trims_block_start {
                own.block_start
            } else {
                inherited_metric.block_start
            },
            block_end: if own.trims_block_end {
                own.block_end
            } else {
                inherited_metric.block_end
            },
        }
    }

    pub(in crate::layout) fn text_box_line_trim_for_style(
        &mut self,
        style: &ComputedStyle,
    ) -> TextBoxLineTrim {
        if matches!(style.text_box_trim, TextBoxTrim::None) {
            return TextBoxLineTrim::default();
        }
        let trim = self.text_box_trim_amounts_for_style(style);
        TextBoxLineTrim {
            trims_block_start: style.text_box_trim.trims_start(),
            trims_block_end: style.text_box_trim.trims_end(),
            block_start: if style.text_box_trim.trims_start() {
                trim.block_start
            } else {
                0.0
            },
            block_end: if style.text_box_trim.trims_end() {
                trim.block_end
            } else {
                0.0
            },
        }
    }

    /// Resolve the block-start and block-end trim amounts from CSS Inline
    /// `<text-edge>` metrics.
    ///
    /// The `text` and `ideographic` metrics map to Quire's existing em/content
    /// box. `cap`, `ex`, `alphabetic`, and `ideographic-ink` use selected-font
    /// metrics where available, with spec-compatible synthesis fallbacks:
    /// <https://drafts.csswg.org/css-inline-3/#text-edges>.
    fn text_box_trim_amounts_for_style(&mut self, style: &ComputedStyle) -> TextBoxLineTrim {
        let pair = style.text_box_edge.resolved_pair(style.line_fit_edge);
        let metrics = self.inline_text_box_metrics(style, 0.0);
        let over_edge = self.text_edge_over_position(style, metrics, pair.over);
        let under_edge = self.text_edge_under_position(style, metrics, pair.under);
        TextBoxLineTrim {
            trims_block_start: true,
            trims_block_end: true,
            block_start: over_edge.clamp(0.0, metrics.line_block_size),
            block_end: (metrics.line_block_size - under_edge).clamp(0.0, metrics.line_block_size),
        }
    }

    /// Resolve inline-box content-box trimming against the untrimmed content
    /// box rather than the line box.
    ///
    /// CSS Inline applies `text-box-trim` to inline boxes by shifting their
    /// content edges to the selected `text-box-edge` metrics:
    /// <https://drafts.csswg.org/css-inline-3/#text-box-trim>.
    pub(in crate::layout) fn inline_text_box_content_trim_for_style(
        &mut self,
        style: &ComputedStyle,
        metrics: InlineTextBoxMetrics,
    ) -> TextBoxLineTrim {
        if matches!(style.text_box_trim, TextBoxTrim::None)
            || !style.display.is_inline_level()
            || style.display.is_atomic_inline()
        {
            return TextBoxLineTrim::default();
        }
        let pair = style.text_box_edge.resolved_pair(style.line_fit_edge);
        let content_over_edge = metrics.block_start_leading;
        let content_under_edge = metrics.block_start_leading + metrics.content_block_size;
        let over_edge = self.text_edge_over_position(style, metrics, pair.over);
        let under_edge = self.text_edge_under_position(style, metrics, pair.under);
        let block_start = (over_edge - content_over_edge).clamp(0.0, metrics.content_block_size);
        let block_end = (content_under_edge - under_edge).clamp(0.0, metrics.content_block_size);
        TextBoxLineTrim {
            trims_block_start: style.text_box_trim.trims_start(),
            trims_block_end: style.text_box_trim.trims_end(),
            block_start: if style.text_box_trim.trims_start() {
                block_start
            } else {
                0.0
            },
            block_end: if style.text_box_trim.trims_end() {
                block_end
            } else {
                0.0
            },
        }
    }

    pub(in crate::layout) fn text_edge_over_position(
        &mut self,
        style: &ComputedStyle,
        metrics: InlineTextBoxMetrics,
        edge: TextEdgeMetric,
    ) -> f32 {
        match edge {
            TextEdgeMetric::Text | TextEdgeMetric::Ideographic => metrics.block_start_leading,
            TextEdgeMetric::IdeographicInk => self
                .font_system
                .ideographic_ink_extents_for_style(style)
                .map(|extents| metrics.line_baseline_offset - layout_points(extents.above_baseline))
                .unwrap_or(metrics.block_start_leading),
            TextEdgeMetric::Cap => {
                let cap_height = self.font_system.used_cap_height_for_style(style).points();
                metrics.line_baseline_offset - cap_height
            }
            TextEdgeMetric::Ex => {
                let x_height = self.font_system.used_x_height_for_style(style).points();
                metrics.line_baseline_offset - x_height
            }
            TextEdgeMetric::Alphabetic => metrics.block_start_leading,
        }
    }

    pub(in crate::layout) fn text_edge_under_position(
        &mut self,
        style: &ComputedStyle,
        metrics: InlineTextBoxMetrics,
        edge: TextEdgeMetric,
    ) -> f32 {
        match edge {
            TextEdgeMetric::Text | TextEdgeMetric::Ideographic => {
                metrics.block_start_leading + metrics.content_block_size
            }
            TextEdgeMetric::IdeographicInk => self
                .font_system
                .ideographic_ink_extents_for_style(style)
                .map(|extents| metrics.line_baseline_offset + layout_points(extents.below_baseline))
                .unwrap_or(metrics.block_start_leading + metrics.content_block_size),
            TextEdgeMetric::Alphabetic => metrics.line_baseline_offset,
            TextEdgeMetric::Cap | TextEdgeMetric::Ex => {
                metrics.block_start_leading + metrics.content_block_size
            }
        }
    }

    pub(in crate::layout) fn with_text_box_line_trim_applied(
        &self,
        mut records: Vec<InlineLineRecord>,
        block_style: &ComputedStyle,
    ) -> (Vec<InlineLineRecord>, TextBoxLineTrim) {
        let trim = self.current_text_box_line_trim();
        if trim.is_empty() {
            return (records, TextBoxLineTrim::default());
        }
        if block_style.box_decoration_break == BoxDecorationBreak::Clone {
            return (records, trim);
        }
        if trim.trims_block_start
            && trim.block_start > 0.0
            && let Some(record) = records
                .iter_mut()
                .find(|record| record.fragment.is_some() && !record.is_phantom)
        {
            record.block_start_trim = trim.block_start;
        }
        if trim.trims_block_end
            && trim.block_end > 0.0
            && let Some(record) = records
                .iter_mut()
                .rev()
                .find(|record| record.fragment.is_some() && !record.is_phantom)
        {
            record.block_end_trim = trim.block_end;
        }
        (records, TextBoxLineTrim::default())
    }

    pub(in crate::layout) fn apply_line_block_start_trim_for_paint(
        &mut self,
        line: &InlineLineRecord,
        writing_mode: WritingMode,
    ) {
        if line.block_start_trim <= 0.0 {
            return;
        }
        match writing_mode {
            WritingMode::HorizontalTb => {
                self.cursor_y += line.block_start_trim;
            }
            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                self.content_left += line.block_start_trim;
                self.content_right += line.block_start_trim;
            }
            WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                self.content_left -= line.block_start_trim;
                self.content_right -= line.block_start_trim;
            }
        }
    }

    pub(in crate::layout) fn paint_inline_line_sequence(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
    ) {
        let mut plaintext_direction_state = None;
        self.paint_inline_line_sequence_with_state(
            sequence,
            block_style,
            &mut plaintext_direction_state,
        );
    }

    pub(in crate::layout) fn paint_inline_line_sequence_with_state(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        plaintext_direction_state: &mut Option<Direction>,
    ) {
        // A captured line sequence is a replay artifact, not an instruction
        // to inherit whichever float stack happens to be active later.  Keep
        // the scope boundary around the whole slice so fragmentation retries,
        // ruby levels, and atomic formatting contexts share the same rule.
        self.with_replay_float_scope(sequence.replay_float_scope, |layout| {
            layout.paint_inline_line_sequence_with_state_in_scope(
                sequence,
                block_style,
                plaintext_direction_state,
            );
        });
    }

    fn paint_inline_line_sequence_with_state_in_scope(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        plaintext_direction_state: &mut Option<Direction>,
    ) {
        self.record_inline_line_sequence_outcome(sequence);
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let mut painted = 0usize;
        let mut plaintext_paragraph_index = None;
        while painted < sequence.records.len() {
            let mut oversized_line_left_final_page_space = false;
            // A speculative multicolumn balance pass must reject a candidate
            // height that would require relaxing widows/orphans. The committed
            // pass may relax only after reaching a real empty fragmentainer.
            let at_fragmentainer_start = self.cursor_is_at_page_top();
            let allow_unavoidable_relaxation =
                at_fragmentainer_start && self.multicol_balance_probe_depth == 0;
            let selection = sequence.fragment_break_selection(
                painted,
                self.cursor_y - self.page_bottom(),
                allow_unavoidable_relaxation,
                block_style.orphans.get(),
                block_style.widows.get(),
            );
            let mut fragment_count = selection.line_count().unwrap_or(0);
            if fragment_count == 0
                && self
                    .fragmentainer_override
                    .is_some_and(|override_| override_.relax_widows_orphans)
            {
                fragment_count = sequence
                    .fragment_break_selection(
                        painted,
                        self.cursor_y - self.page_bottom(),
                        self.cursor_is_at_page_top(),
                        1,
                        1,
                    )
                    .line_count()
                    .unwrap_or(0);
            }
            if fragment_count == 0 && self.out_of_flow_prebreak_suppression_depth > 0 {
                fragment_count = 1;
            }
            if fragment_count == 0 {
                if self.multicol_balance_probe_depth > 0 {
                    if selection.is_no_room() && !at_fragmentainer_start {
                        self.push_page();
                        continue;
                    }
                    // Do not manufacture an unbounded sequence of temporary
                    // columns while probing an invalid balance height. The
                    // probe observes this as overflowing its current column.
                    self.mark_current_page_flow_content();
                    self.cursor_y = self.page_bottom() - css::CSS_PX_TO_PT;
                    break;
                }
                // An isolated atomic inline (notably an inline-table) has
                // already captured its own formatting context. When its line
                // moves as a unit, replay the enclosing block's destination
                // geometry rather than preserving the source page's canvas
                // translation through the generic line-break path.
                // <https://www.w3.org/TR/css-break-3/#box-splitting>
                let continuation = (self.active_fragmentainer_kind() == FragmentainerKind::Page
                    && sequence.records[painted].has_isolated_atomic_inline_fragment())
                .then(|| self.block_page_break_continuation_context());
                let source_page_count = self.pages.len();
                self.push_page();
                if self.pages.len() != source_page_count
                    && let Some(continuation) = continuation
                {
                    self.replay_fragment_continuation_on_page(
                        &continuation,
                        self.current_page_context,
                    );
                }
                continue;
            }
            let context = sequence.context(block_style);
            let mut stack = InlineLineStackCursor::new(
                block_style,
                self.content_left,
                self.content_right,
                self.cursor_y,
            );
            let fragment_records = sequence.fragment_records_for_paint(painted, fragment_count);
            for line in &fragment_records {
                if block_style.unicode_bidi == UnicodeBidi::Plaintext
                    && plaintext_paragraph_index != Some(line.paragraph_index)
                {
                    *plaintext_direction_state = None;
                    plaintext_paragraph_index = Some(line.paragraph_index);
                }
                stack.advance(line.block_before);
                stack.apply(self);
                self.apply_line_block_start_trim_for_paint(line, block_style.writing_mode);
                let line_top = self.cursor_y;
                let oversized_page_line = self.active_fragmentainer_kind()
                    == FragmentainerKind::Page
                    && self.out_of_flow_prebreak_suppression_depth == 0
                    // An already-isolated atomic formatting context, such as
                    // an `inline-table`, owns one monolithic paint fragment.
                    // Do not turn that atom into page-relative slices merely
                    // because its containing line exceeds the page area.
                    // <https://drafts.csswg.org/css-display-3/#atomic-inline>
                    && !line.has_isolated_atomic_inline_fragment()
                    && line.height() > line_top - self.page_bottom() + 0.01;
                if oversized_page_line {
                    // A line box is monolithic, but when it is taller than an
                    // empty page CSS Fragmentation slices its continuous paint
                    // through the crossed pages and resumes following flow at
                    // the line's continuous block-end.
                    // <https://www.w3.org/TR/css-break-3/#monolithic>
                    let snapshot = self.snapshot();
                    let checkpoint = self.current_page.paint_checkpoint();
                    self.paint_collected_inline_line(
                        line,
                        context,
                        plaintext_direction_state,
                        None,
                    );
                    let fragment = self.current_page.take_paint_fragment_since(checkpoint);
                    for (page_index, fragment) in
                        super::super::block::continuous_fragmentainer_paint_slices(
                            &snapshot, fragment,
                        )
                    {
                        if page_index == self.pages.len() {
                            self.current_page.append_paint_fragment_owned(
                                fragment,
                                PaintTranslation::identity(),
                            );
                        } else {
                            self.pending_paint_fragments.push(PendingPaintFragment {
                                page_index,
                                fragment,
                                kind: PendingPaintFragmentKind::InFlowOverflow,
                            });
                        }
                    }
                    self.mark_current_page_flow_content();
                    self.consume_definite_block_size_through_fragmentainers(
                        line_top,
                        line.height(),
                    );
                    oversized_line_left_final_page_space =
                        self.cursor_y > self.page_bottom() + 0.01;
                    stack = InlineLineStackCursor::new(
                        block_style,
                        self.content_left,
                        self.content_right,
                        self.cursor_y,
                    );
                    stack = self.apply_collected_line_clearance(stack, line.clear_after, context);
                } else {
                    self.paint_collected_inline_line(
                        line,
                        context,
                        plaintext_direction_state,
                        None,
                    );
                    // A non-phantom inline line owns normal-flow space in the
                    // current fragmentainer even when its paint is deferred
                    // (for example while fixed descendants are replayed).
                    // Record that occupancy before a following forced break
                    // chooses its destination page context.
                    // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                    if !line.is_phantom {
                        self.mark_current_page_flow_content();
                    }
                    stack.advance(line.height());
                    stack = self.apply_collected_line_clearance(stack, line.clear_after, context);
                }
            }
            stack.apply(self);
            painted += fragment_count;
            if painted < sequence.records.len() {
                self.content_left = saved_content_left;
                self.content_right = saved_content_right;
                if self.out_of_flow_prebreak_suppression_depth == 0
                    && !oversized_line_left_final_page_space
                {
                    self.push_page();
                }
            }
        }
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
    }

    pub(in crate::layout) fn paint_inline_line_sequence_multicolumn(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        plan: MulticolumnInlineLayoutPlan,
    ) {
        let mut sequence = sequence.clone();
        // The multicol planner owns a discard container's first local region
        // break. Reuse the ordinary terminal-line marker path at that typed
        // endpoint, rather than creating a page/column continuation or
        // placing a marker at a block-only boundary.
        let discard_marker = match block_style.used_continuation() {
            css::UsedContinuation::Discard(discard) => Some(discard.marker),
            css::UsedContinuation::Ordinary | css::UsedContinuation::LineClamp(_) => None,
        };
        if let Some(marker) = discard_marker
            && let Some(last_fragment) = plan.rows.last().and_then(|row| row.columns.last())
            && last_fragment.range.end_index < sequence.records.len()
            && let Some(endpoint) = sequence
                .records
                .get(last_fragment.range.end_index.saturating_sub(1))
                .map(|record| record.block_line_index)
        {
            let context = InlineParagraphContext {
                line_clamp: Some(css::InlineLineClamp::Automatic(
                    css::AutomaticLineClamp::after_measured_line(endpoint, marker),
                )),
                ..sequence.context(block_style)
            };
            if let Some(record) = sequence
                .records
                .get_mut(last_fragment.range.end_index.saturating_sub(1))
                && let Some(fragment) = record.fragment.as_mut()
            {
                self.append_line_clamp_ellipsis(fragment, context, endpoint);
            }
        }
        self.record_inline_line_sequence_outcome(&sequence);
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let mut plaintext_direction_state = None;
        let context = sequence.context(block_style);
        let mut rule_paint_point = self
            .current_page
            .paint_band_insertion_point(PaintBand::InFlowBlock);

        for (row_index, row) in plan.rows.iter().enumerate() {
            let geometry = plan.geometry;
            let column_block_top = self.cursor_y;
            for (column_index, fragment) in row.columns.iter().enumerate() {
                let column_left = saved_content_left
                    + (geometry.column_width + geometry.column_gap) * column_index as f32;
                self.content_left = column_left;
                self.content_right = column_left + geometry.column_width;
                self.cursor_y = column_block_top;
                let mut stack = InlineLineStackCursor::new(
                    block_style,
                    self.content_left,
                    self.content_right,
                    self.cursor_y,
                );
                let fragment_records = sequence.fragment_records_for_paint(
                    fragment.range.start_index,
                    fragment.range.record_count(),
                );
                for line in &fragment_records {
                    stack.advance(line.block_before);
                    stack.apply(self);
                    self.apply_line_block_start_trim_for_paint(line, block_style.writing_mode);
                    self.paint_collected_inline_line(
                        line,
                        context,
                        &mut plaintext_direction_state,
                        None,
                    );
                    stack.advance(line.height());
                }
            }

            let column_block_bottom = column_block_top - row.block_extent.points();
            let rule_primitives = multicol_gap_decoration_primitives(
                block_style,
                saved_content_left,
                column_block_top,
                column_block_bottom,
                geometry.column_width,
                geometry.column_gap,
                row.decorated_column_count,
            );
            self.current_page
                .insert_primitives_at_paint_band_point(rule_paint_point, rule_primitives);
            self.cursor_y = column_block_bottom;
            if row_index + 1 == plan.rows.len() {
                continue;
            }
            debug_assert!(geometry.wrap_column_rows);
            self.content_left = saved_content_left;
            self.content_right = saved_content_right;
            self.push_page();
            rule_paint_point = self
                .current_page
                .paint_band_insertion_point(PaintBand::InFlowBlock);
        }
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
    }

    pub(in crate::layout) fn paint_inline_line_sequence_with_outside_marker(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        marker: &ListMarker,
        content_inline_start: f32,
        content_inline_end: f32,
    ) {
        self.record_inline_line_sequence_outcome(sequence);
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let mut plaintext_direction_state = None;
        let mut painted = 0usize;
        let mut marker_painted = false;
        while painted < sequence.records.len() {
            let mut fragment_count = sequence
                .fragment_break_selection(
                    painted,
                    self.cursor_y - self.page_bottom(),
                    self.cursor_is_at_page_top(),
                    block_style.orphans.get(),
                    block_style.widows.get(),
                )
                .line_count()
                .unwrap_or(0);
            if fragment_count == 0
                && self
                    .fragmentainer_override
                    .is_some_and(|override_| override_.relax_widows_orphans)
            {
                fragment_count = sequence
                    .fragment_break_selection(
                        painted,
                        self.cursor_y - self.page_bottom(),
                        self.cursor_is_at_page_top(),
                        1,
                        1,
                    )
                    .line_count()
                    .unwrap_or(0);
            }
            if fragment_count == 0 && self.out_of_flow_prebreak_suppression_depth > 0 {
                fragment_count = 1;
            }
            if fragment_count == 0 {
                self.push_page();
                continue;
            }
            let context = sequence.context(block_style);
            let mut stack = InlineLineStackCursor::new(
                block_style,
                self.content_left,
                self.content_right,
                self.cursor_y,
            );
            let fragment_records = sequence.fragment_records_for_paint(painted, fragment_count);
            for line in &fragment_records {
                stack.advance(line.block_before);
                stack.apply(self);
                self.apply_line_block_start_trim_for_paint(line, block_style.writing_mode);
                if !marker_painted && !self.outside_marker_anchor_is_pending(marker) {
                    let formatted_line_block_start = PageTopBlockPosition::new(self.cursor_y);
                    let fallback_baseline_offset =
                        self.inline_box_text_line_layout_baseline_offset(block_style);
                    let line_baseline_offset = line
                        .fragment
                        .as_ref()
                        .map_or(fallback_baseline_offset, |fragment| {
                            fragment.metrics.baseline_offset
                        });
                    self.paint_outside_marker(
                        marker,
                        block_style,
                        OutsideMarkerAnchor {
                            principal_line_inline_span: PageInlineSpan::from_edges(
                                content_inline_start,
                                content_inline_end,
                            ),
                            formatted_line_block_start,
                            alphabetic_baseline: formatted_line_block_start
                                .toward_block_end(layout_pt(line_baseline_offset)),
                        },
                    );
                    marker_painted = true;
                }
                self.paint_collected_inline_line(
                    line,
                    context,
                    &mut plaintext_direction_state,
                    None,
                );
                stack.advance(line.height());
                stack = self.apply_collected_line_clearance(stack, line.clear_after, context);
            }
            stack.apply(self);
            painted += fragment_count;
            if painted < sequence.records.len() {
                self.content_left = saved_content_left;
                self.content_right = saved_content_right;
                if self.out_of_flow_prebreak_suppression_depth == 0 {
                    self.push_page();
                }
            }
        }
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
    }

    pub(in crate::layout) fn paint_inline_line_sequence_slice(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        block_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) {
        self.paint_inline_line_sequence_slice_inner(
            sequence,
            block_style,
            InlineLineSequenceSlice {
                block_top,
                top: slice_top,
                bottom: slice_bottom,
            },
            None,
            NestedInlinePaintFloatPolicy::ReapplyActiveFloatBands,
        );
    }

    pub(in crate::layout) fn paint_inline_line_sequence_slice_with_text_source(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        slice: InlineLineSequenceSlice,
        text_source: RenderedLineSource,
        float_policy: NestedInlinePaintFloatPolicy,
    ) {
        self.paint_inline_line_sequence_slice_inner(
            sequence,
            block_style,
            slice,
            Some(text_source),
            float_policy,
        );
    }

    fn paint_inline_line_sequence_slice_inner(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        slice: InlineLineSequenceSlice,
        text_source: Option<RenderedLineSource>,
        float_policy: NestedInlinePaintFloatPolicy,
    ) {
        let saved_cursor_y = self.cursor_y;
        let saved_left = self.content_left;
        let saved_right = self.content_right;
        let mut plaintext_direction_state = None;
        let context = sequence.context(block_style);
        let (fragment_block_top, fragment_records) =
            sequence.fragment_records_for_slice_paint(slice.block_top, slice.top, slice.bottom);
        let mut stack =
            InlineLineStackCursor::new(block_style, saved_left, saved_right, fragment_block_top);
        for line in &fragment_records {
            stack.advance(line.block_before);
            let line_top = stack.cursor_y;
            let line_bottom = line_top - line.height();
            if line_top >= slice.bottom && line_bottom <= slice.top {
                stack.apply(self);
                self.apply_line_block_start_trim_for_paint(line, block_style.writing_mode);
                self.paint_collected_inline_line_with_float_policy(
                    line,
                    context,
                    &mut plaintext_direction_state,
                    text_source,
                    float_policy,
                );
            }
            stack.advance(line.height());
        }
        self.content_left = saved_left;
        self.content_right = saved_right;
        self.cursor_y = saved_cursor_y;
    }

    /// Paint a selected inline sequence inside a fixed generated box.
    ///
    /// CSS Paged Media margin boxes have fixed generated content rectangles,
    /// so they reuse CSS Text line preparation and paint emission without
    /// normal block pagination or float exclusion:
    /// <https://www.w3.org/TR/css-page-3/#margin-boxes> and
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order>.
    pub(in crate::layout) fn paint_inline_line_sequence_in_fixed_box(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        content_left: f32,
        available_width: f32,
        block_top: f32,
    ) {
        let saved_left = self.content_left;
        let saved_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        self.content_left = content_left;
        self.content_right = content_left + available_width;
        let context = sequence.context(block_style);
        let mut plaintext_direction_state = None;
        let mut stack = InlineLineStackCursor::new(
            block_style,
            self.content_left,
            self.content_right,
            block_top,
        );
        let fragment_records = sequence.fragment_records_for_paint(0, sequence.records.len());
        for line in &fragment_records {
            stack.advance(line.block_before);
            stack.apply(self);
            self.apply_line_block_start_trim_for_paint(line, block_style.writing_mode);
            if let Some(prepared) =
                self.prepare_inline_line_record(line, context, &mut plaintext_direction_state)
            {
                self.paint_prepared_inline_line(&prepared);
            }
            stack.advance(line.height());
        }
        self.content_left = saved_left;
        self.content_right = saved_right;
        self.cursor_y = saved_cursor_y;
    }

    fn collect_inline_paragraph_lines(
        &mut self,
        paragraph: &mut Vec<InlineItem>,
        context: InlineParagraphContext<'_>,
        cursor: InlineLineSequenceCursor,
        force_empty_line: bool,
        output: &mut Vec<InlineLineRecord>,
    ) -> InlineLineSequenceCursor {
        let paragraph_index = cursor.paragraph_index;
        let line_index = cursor.line_index;
        let starts_after_forced_break = cursor.starts_after_forced_break;
        let is_first_formatted_line = context.initial_first_formatted_line && line_index == 0;
        trim_inline_item_edges(paragraph);
        if paragraph.is_empty() {
            if force_empty_line
                && !context
                    .line_clamp
                    .is_some_and(|clamp| clamp.excludes_line(line_index))
            {
                output.push(InlineLineRecord {
                    paragraph_index,
                    block_line_index: line_index,
                    paragraph_line_index: 0,
                    fragment: None,
                    is_phantom: false,
                    is_first_formatted_line,
                    is_last_line_in_paragraph: true,
                    is_forced_empty: true,
                    starts_after_preserved_segment_break: cursor
                        .starts_after_preserved_segment_break,
                    clear_after: Clear::None,
                    block_before: 0.0,
                    block_start_trim: 0.0,
                    block_end_trim: 0.0,
                    paragraph_last_hanging_width: 0.0,
                    used_indent: used_line_indent_for_formatted_line(
                        is_first_formatted_line,
                        starts_after_forced_break,
                        context.hanging_indent,
                        context.block_style,
                        context.available_width,
                    ),
                    available_width: context.available_width,
                    line_height: context.block_style.line_height,
                    decoration_origin_fragments: Default::default(),
                });
                return InlineLineSequenceCursor {
                    line_index: line_index + 1,
                    ..cursor
                };
            }
            return cursor;
        }

        let paragraph_start_line_index = line_index;
        let graph = self.build_inline_opportunity_graph(paragraph.iter(), context.block_style);
        let graph = if is_first_formatted_line {
            self.graph_with_first_letter_pseudo(&graph, context.block_style)
        } else {
            graph
        };
        // Collected-line layout is used for explicit breaks, columns, and
        // fragmentation. Its first graph must participate in the same
        // initial-letter selection lifecycle as the direct paragraph path;
        // otherwise companion source is selected at the full measure and the
        // exclusion appears only after that source has committed.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
        if is_first_formatted_line {
            self.register_provisional_initial_letter_exclusion(
                &graph,
                context,
                line_index,
                starts_after_forced_break,
                false,
            );
        }
        let selected_lines = self.select_inline_lines_from_graph_with_block_offset_and_replay(
            &graph,
            context,
            line_index,
            starts_after_forced_break,
            cursor.physical_block_offset,
            cursor.pending_inline_float_replay,
        );
        let line_boxes = selected_lines.fragments;
        let next_line_index = selected_lines.next_line_index;
        // A float-only graph is normally followed by source in the same
        // opportunity graph. An explicit break is different: it starts the
        // following source in a fresh graph, so retain the committed float
        // transaction until that graph has selected its first source row.
        let pending_inline_float_replay = line_boxes
            .is_empty()
            .then_some(selected_lines.trailing_inline_float_replay)
            .flatten();
        // A float-only source range has no in-flow fragment, but a following
        // forced break still creates and advances an empty line box. Preserve
        // that line in the collected representation used whenever explicit
        // breaks require fragmentation-aware line painting.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        // <https://drafts.csswg.org/css-inline-3/#line-boxes>
        if force_empty_line
            && line_boxes.is_empty()
            // A preserved break after source already excluded by an
            // automatic/numeric cutoff cannot manufacture a new empty line.
            // It is beyond the same continuation endpoint and therefore has
            // no block-size contribution to the clamped container.
            // <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
            && !context
                .line_clamp
                .is_some_and(|clamp| clamp.excludes_line(next_line_index))
        {
            output.push(InlineLineRecord {
                paragraph_index,
                block_line_index: next_line_index,
                paragraph_line_index: 0,
                fragment: None,
                is_phantom: false,
                is_first_formatted_line: context.initial_first_formatted_line
                    && next_line_index == 0,
                is_last_line_in_paragraph: true,
                is_forced_empty: true,
                starts_after_preserved_segment_break: cursor.starts_after_preserved_segment_break,
                clear_after: Clear::None,
                block_before: 0.0,
                block_start_trim: 0.0,
                block_end_trim: 0.0,
                paragraph_last_hanging_width: 0.0,
                used_indent: used_line_indent_for_formatted_line(
                    context.initial_first_formatted_line && next_line_index == 0,
                    starts_after_forced_break,
                    context.hanging_indent,
                    context.block_style,
                    context.available_width,
                ),
                available_width: context.available_width,
                line_height: context.block_style.line_height,
                decoration_origin_fragments: Default::default(),
            });
            paragraph.clear();
            return InlineLineSequenceCursor {
                line_index: next_line_index + 1,
                physical_block_offset: selected_lines.next_physical_block_offset,
                has_flow_side_effects: cursor.has_flow_side_effects
                    || selected_lines.has_float_side_effects,
                pending_inline_float_replay,
                ..cursor
            };
        }
        let paragraph_last_hanging_width = line_boxes
            .last()
            .map(|line_box| {
                last_hanging_punctuation_width_for_line_items(
                    &mut self.font_system,
                    &line_box.items,
                    context.block_style,
                )
            })
            .map(SemanticLengthExt::points)
            .unwrap_or(0.0);
        let line_count = line_boxes.len();
        let mut next_record_line_index = paragraph_start_line_index;
        for (offset, line_box) in line_boxes.into_iter().enumerate() {
            while next_record_line_index < line_box.line_index {
                output.push(InlineLineRecord {
                    paragraph_index,
                    block_line_index: next_record_line_index,
                    paragraph_line_index: next_record_line_index - paragraph_start_line_index,
                    fragment: None,
                    is_phantom: false,
                    is_first_formatted_line: context.initial_first_formatted_line
                        && next_record_line_index == 0,
                    is_last_line_in_paragraph: false,
                    // Float exclusions can consume a physical line before a
                    // graph range fits the following available float band.
                    is_forced_empty: true,
                    starts_after_preserved_segment_break: false,
                    clear_after: Clear::None,
                    block_before: 0.0,
                    block_start_trim: 0.0,
                    block_end_trim: 0.0,
                    paragraph_last_hanging_width,
                    used_indent: 0.0,
                    available_width: context.available_width,
                    line_height: context.block_style.line_height,
                    decoration_origin_fragments: Default::default(),
                });
                next_record_line_index += 1;
            }
            let line_box_index = line_box.line_index;
            // Atomic inline boxes contribute a margin-box extent outside the
            // text metrics. Text fragments' specified `line-height` is
            // already reflected in `line_box.metrics`; reapplying it here
            // would discard a selected text-box edge or trim.
            // <https://drafts.csswg.org/css-inline-3/#line-height-property>
            let item_line_height = line_box
                .fragment
                .items()
                .iter()
                // An initial letter spans multiple lines through its own
                // exclusion geometry; it must not make the originating
                // collected line record taller than the parent strut.
                // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
                .filter(|item| {
                    !LayoutBuilder::inline_line_item_is_initial_letter(&item.item)
                        && matches!(
                            &item.item,
                            InlineLineItem::Atom(atom)
                                if !matches!(atom.content(), InlineAtomContent::InlineEdge(_))
                        )
                })
                .map(|item| inline_line_item_logical_block_size(&item.item, context.block_style))
                .fold(0.0_f32, f32::max);
            let line_height = line_box
                .metrics
                .height
                .max(context.block_style.line_height)
                .max(item_line_height);
            let used_indent = line_box.indent;
            let available_width = line_box.available_width;
            let is_phantom = inline_line_fragment_is_phantom(&line_box);
            output.push(InlineLineRecord {
                paragraph_index,
                block_line_index: line_box_index,
                paragraph_line_index: line_box_index - paragraph_start_line_index,
                fragment: Some(line_box.fragment),
                is_phantom,
                // A CSS Inline phantom line is not a formatted line. In
                // particular, edge-only positioning metadata must not
                // consume `::first-line`, text-indent, or line-clamp state.
                // <https://drafts.csswg.org/css-inline-3/#phantom-line-boxes>
                is_first_formatted_line: context.initial_first_formatted_line
                    && line_box_index == 0
                    && !is_phantom,
                is_last_line_in_paragraph: offset + 1 == line_count,
                is_forced_empty: false,
                starts_after_preserved_segment_break: offset == 0
                    && cursor.starts_after_preserved_segment_break,
                clear_after: Clear::None,
                // Retain the physical float-slab advance selected with this
                // source row. A preserved break can move selection into a
                // fresh graph, but the durable collected record owns the
                // resulting page-local gap during paint and fragmentation.
                block_before: line_box.block_before,
                block_start_trim: 0.0,
                block_end_trim: 0.0,
                paragraph_last_hanging_width,
                used_indent,
                available_width,
                line_height,
                decoration_origin_fragments: Default::default(),
            });
            next_record_line_index = line_box_index + 1;
        }
        paragraph.clear();
        InlineLineSequenceCursor {
            line_index: next_line_index,
            physical_block_offset: selected_lines.next_physical_block_offset,
            has_flow_side_effects: cursor.has_flow_side_effects
                || selected_lines.has_float_side_effects,
            pending_inline_float_replay,
            ..cursor
        }
    }

    fn paint_collected_inline_line(
        &mut self,
        line: &InlineLineRecord,
        context: InlineParagraphContext<'_>,
        plaintext_direction_state: &mut Option<Direction>,
        text_source: Option<RenderedLineSource>,
    ) {
        self.paint_collected_inline_line_with_float_policy(
            line,
            context,
            plaintext_direction_state,
            text_source,
            NestedInlinePaintFloatPolicy::ReapplyActiveFloatBands,
        );
    }

    fn paint_collected_inline_line_with_float_policy(
        &mut self,
        line: &InlineLineRecord,
        context: InlineParagraphContext<'_>,
        plaintext_direction_state: &mut Option<Direction>,
        text_source: Option<RenderedLineSource>,
        float_policy: NestedInlinePaintFloatPolicy,
    ) {
        // A phantom line has no in-flow layout or ordinary paint effect, but
        // its selected inline edges still establish the containing-block
        // geometry of positioned descendants.  Replaying an escaped layer
        // from that edge is therefore an out-of-flow paint effect, not a
        // reason for the line to acquire height or participate in flow.
        // <https://drafts.csswg.org/css-inline-3/#phantom-line-boxes>
        if line.is_phantom
            && !line.has_inline_layout_effects()
            && !line.has_positioned_descendant_replay()
        {
            return;
        }
        let line_height = line.height();
        let Some(_) = &line.fragment else {
            self.record_in_flow_line_baseline(line, context.block_style);
            return;
        };
        if self.cursor_y - line_height < self.page_bottom() - 0.01
            && self.out_of_flow_prebreak_suppression_depth == 0
        {
            self.push_page();
        }
        self.commit_inline_line_footnote_calls(line);
        let mut paint_context = context;
        let mut paint_line = line.clone();
        let float_replay = line.fragment.as_ref().map(|fragment| fragment.float_replay);
        let reuses_selected_float_band = float_replay
            .is_some_and(|replay| replay.reuses_selected_band_on(self.current_float_page_index()));
        // Horizontal collected layout has a distinct float-placement replay
        // path that can commit a same-page CSS float between selection and
        // paint. Keep that established behavior intact; vertical lines are
        // the logical-axis path where the frozen band prevents a second,
        // physically projected adjustment.
        let replay_selected_vertical_band = context.block_style.writing_mode
            != WritingMode::HorizontalTb
            && float_replay.is_some_and(|replay| {
                replay.selected_float_page_index() == self.current_float_page_index()
            });
        if float_policy == NestedInlinePaintFloatPolicy::ReapplyActiveFloatBands
            && !reuses_selected_float_band
            && !replay_selected_vertical_band
        {
            if context.block_style.writing_mode == WritingMode::HorizontalTb {
                let band = self.current_float_band(PageBlockSpan::new(self.cursor_y, line_height));
                let left_offset = (band.left() - self.content_left - context.padding_left).max(0.0);
                let right_offset = (self.content_right - band.right()).max(0.0);
                if left_offset > 0.0 || right_offset > 0.0 {
                    let available_width =
                        (band.right() - self.content_left - context.padding_left).max(1.0);
                    paint_line.available_width = line.available_width.min(available_width);
                    paint_line.used_indent = line.used_indent.max(left_offset);
                    paint_context.available_width = paint_line.available_width;
                }
            } else {
                let band = self.current_logical_float_band(
                    context.block_style.writing_mode,
                    context.block_style.used_direction(),
                    FloatBandQuery {
                        horizontal_slab: PageInlineSpan::new(
                            self.content_left + context.padding_left,
                            line_height,
                        ),
                        vertical_slab: vertical_physical_inline_span(
                            context.block_style.writing_mode,
                            context.block_style.used_direction(),
                            PageTopBlockPosition::new(self.cursor_y),
                            layout_pt(context.available_width),
                        ),
                    },
                );
                if band.inline_span.start() > 0.0
                    || band.inline_span.end() < context.available_width - 0.01
                {
                    paint_line.available_width = line.available_width.min(band.inline_span.end());
                    paint_line.used_indent = line.used_indent.max(band.inline_span.start());
                    paint_context.available_width = paint_line.available_width;
                }
            }
        }
        self.record_in_flow_line_baseline(&paint_line, context.block_style);
        if let Some(prepared) =
            self.prepare_inline_line_record(&paint_line, paint_context, plaintext_direction_state)
        {
            self.paint_prepared_inline_line_with_text_source(&prepared, text_source);
        }
    }

    /// Record calls only after pagination has accepted their selected line.
    ///
    /// A selected line can be replayed by tables, slices, and fragmentation
    /// retries. `handle_footnote_call` deduplicates each pass, so every
    /// committed source call contributes one reservation/body while intrinsic
    /// and graph construction remain side-effect free.
    fn commit_inline_line_footnote_calls(&mut self, line: &InlineLineRecord) {
        let Some(fragment) = &line.fragment else {
            return;
        };
        for item in fragment.items() {
            if let InlineLineItem::Fragment(fragment) = &item.item
                && let Some(element) = fragment.source().footnote_call()
            {
                self.handle_footnote_call(element);
            }
        }
    }

    /// Record the baseline of a real in-flow line box, even if it paints nothing.
    ///
    /// CSS 2.2 makes an inline-block's baseline the baseline of its last
    /// in-flow line box when `overflow` is visible. Empty line boxes that end
    /// with a preserved newline are not phantom for that purpose, and
    /// zero-sized text still creates a line box even when no glyph paint is
    /// emitted:
    /// <https://drafts.csswg.org/css2/#leading> and
    /// <https://drafts.csswg.org/css2/#inline-formatting>.
    fn record_in_flow_line_baseline(
        &mut self,
        line: &InlineLineRecord,
        block_style: &ComputedStyle,
    ) {
        if line.is_phantom {
            return;
        }
        let baseline_offset = if let Some(fragment) = &line.fragment {
            fragment.metrics.baseline_offset
        } else if line.is_forced_empty {
            self.inline_box_text_line_layout_baseline_offset(block_style)
        } else {
            return;
        };
        self.last_in_flow_line_baseline_y = Some(self.cursor_y - baseline_offset);
        // A forced empty record preserves line-height and baseline behavior
        // for the block, but it has no principal inline content. In
        // particular it must not replace an outside marker's no-line fallback
        // anchor; CSS Lists leaves that float-adjacent case undefined.
        // <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
        let has_principal_inline_content = line
            .fragment
            .as_ref()
            .is_some_and(inline_line_fragment_has_principal_content);
        if !line.is_forced_empty && has_principal_inline_content {
            self.anchor_pending_outside_markers_to_in_flow_line(
                PageTopBlockPosition::new(self.cursor_y),
                layout_pt(baseline_offset),
            );
        }
    }

    /// Prepare one graph-selected line record for painting.
    ///
    /// CSS Text alignment, hanging punctuation, and `unicode-bidi: plaintext`
    /// are paint-time decisions over the selected logical line. Keeping them in
    /// this record-level preparer lets text-only, mixed, generated, and sliced
    /// lines share the same positioned paint artifact:
    /// <https://www.w3.org/TR/css-text-3/#text-align-property>,
    /// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>, and
    /// <https://www.w3.org/TR/css-writing-modes-4/#valdef-unicode-bidi-plaintext>.
    pub(in crate::layout) fn prepare_inline_line_record(
        &mut self,
        line: &InlineLineRecord,
        context: InlineParagraphContext<'_>,
        plaintext_direction_state: &mut Option<Direction>,
    ) -> Option<PreparedInlineLine> {
        let line_box = line.fragment.as_ref()?;
        let block_style = context.block_style;
        if block_style.unicode_bidi == UnicodeBidi::Plaintext
            && line.paragraph_index != 0
            && line.paragraph_line_index == 0
        {
            // Callers may prepare collected records individually (for
            // generated boxes and tests), so keep the plaintext paragraph
            // boundary at the record API as well as the sequence paint loop.
            *plaintext_direction_state = None;
        }
        let padding_left = context.padding_left;
        let line_text = line_box.text();
        let text_align = text_align_for_inline_line_text_with_state(
            block_style,
            line.is_last_line_in_paragraph,
            line_text,
            plaintext_direction_state,
        );
        let inherited_line_direction = if block_style.unicode_bidi == UnicodeBidi::Plaintext {
            (*plaintext_direction_state).unwrap_or(block_style.used_direction())
        } else {
            block_style.used_direction()
        };
        let line_direction = inherited_line_direction;
        let mut metrics = line_box.metrics;
        // A left float shifts the physical left edge of an RTL line but does
        // not indent its logical inline start at the right edge. Inline line
        // fragments currently carry that physical shift in `used_indent` for
        // LTR painting; do not apply it a second time as an RTL logical
        // indent after a replayed inline float.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        let paint_indent = line_box.source_paint_indent.unwrap_or_else(|| {
            if line_box
                .float_replay
                .reuses_selected_band_on(self.current_float_page_index())
                && line_direction == Direction::Rtl
                && line.used_indent > 0.0
            {
                0.0
            } else {
                line.used_indent
            }
        });
        let line_available_width = (line.available_width - paint_indent).max(0.0);
        let hanging_widths = hanging_punctuation_widths_for_line_items(
            &mut self.font_system,
            &line_box.items,
            block_style,
            line.is_first_formatted_line,
            line.is_last_line_in_paragraph,
            line_box.metrics.width > line_available_width,
        );
        if line_box_uses_hanging_punctuation_alignment(block_style) {
            let own_hanging_width = hanging_widths.end;
            let reserve_width = if text_align == TextAlign::Right {
                context.hanging_punctuation_reserve
            } else {
                0.0
            };
            let end_width = if line.is_last_line_in_paragraph {
                own_hanging_width.max(reserve_width)
            } else if text_align == TextAlign::Right {
                own_hanging_width.max(
                    line.paragraph_last_hanging_width
                        .max(context.hanging_punctuation_reserve),
                )
            } else {
                own_hanging_width
            };
            metrics.width = (metrics.width - hanging_widths.start - end_width).max(0.0);
        } else if !line.is_last_line_in_paragraph && text_align == TextAlign::Right {
            metrics.width = (metrics.width
                - line
                    .paragraph_last_hanging_width
                    .max(context.hanging_punctuation_reserve))
            .max(0.0);
        }
        // CSS Text Phase II excludes a hanging Unicode space-separator suffix
        // from the formatted line's measure, but it remains part of the
        // inline box for painting. In particular, an inline background must
        // cover an ideographic space that hangs past the selected line edge.
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
        let mut bidi_scope_continuations = line_box.bidi_scope_continuations.clone();
        let has_css_bidi_control = line_box.items().iter().any(|item| {
            matches!(&item.item, InlineLineItem::Fragment(fragment)
                if fragment.source() == InlineTextSource::BidiControl)
        });
        if block_bidi_scope_needs_inline_controls(block_style)
            && !has_css_bidi_control
            && bidi_scope_continuations.prefix.is_empty()
            && bidi_scope_continuations.suffix.is_empty()
            && let Some((start, end)) = bidi_control_scope_for_style(block_style)
        {
            // Low-level callers can construct a selected line directly,
            // bypassing inline collection where CSS scopes normally become
            // explicit UAX #9 controls. Recover that same non-paint input at
            // the selected-line boundary without modifying its source text.
            bidi_scope_continuations.prefix.push_str(start);
            bidi_scope_continuations.suffix.push_str(end);
        }
        if line_direction == Direction::Rtl && line_box.edge_effects.retained_break_spaces_end {
            // UAX #9 rule L1 would otherwise reset a selected trailing U+0020
            // to the paragraph level, moving it to the physical left of the
            // retained LTR run. CSS Text keeps this logical line-end space at
            // the visual inline start, with its normal `break-spaces` advance.
            // The virtual LRM affects ordering only; the selected source item
            // remains the one painted, decorated, and exposed for extraction.
            // <https://www.w3.org/TR/css-text-3/#valdef-white-space-break-spaces>
            // <https://www.unicode.org/reports/tr9/#L1>
            bidi_scope_continuations
                .trailing_line_edge_context
                .push('\u{200e}');
        }
        let mut line_items = self.visual_ordered_mixed_inline_line_items(
            line_box.items(),
            block_style,
            line_direction,
            &bidi_scope_continuations,
        );
        debug_assert!(line_box.edge_effects.source_effects.iter().all(|effect| {
            line_box
                .items()
                .get(effect.item_index)
                .and_then(|item| match &item.item {
                    InlineLineItem::Fragment(fragment) => Some(fragment.text()),
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
                })
                .is_some_and(|text| {
                    text.is_char_boundary(effect.source_range.start)
                        && text.is_char_boundary(effect.source_range.end)
                        && effect.source_range.start < effect.source_range.end
                })
        }));
        if line_box.edge_effects.collapsed_end_trim_width > 0.0 {
            trim_visual_line_end_collapsible_spaces(&mut line_items, line_direction);
        }
        // CSS Text Phase II excludes the hanging suffix from fitting and
        // alignment, but it remains selected source for painting. In
        // particular, inline backgrounds and decorations extend across that
        // suffix even when it hangs beyond the formatted line measure.
        // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
        let mut paint_fragment = InlineLineFragment::new(
            line_items,
            metrics,
            hanging_widths,
            line_box.indent,
            line.available_width,
            line_box.float_replay.selected_float_page_index(),
            line_text,
        )
        .with_edge_effects(line_box.edge_effects.clone())
        .with_bidi_scope_continuations(line_box.bidi_scope_continuations.clone());
        if let Some(source_paint_indent) = line_box.source_paint_indent {
            paint_fragment = paint_fragment.with_source_paint_indent(source_paint_indent);
        }
        paint_fragment.text_box_trim = TextBoxLineTrim {
            trims_block_start: line.block_start_trim > 0.0,
            trims_block_end: line.block_end_trim > 0.0,
            block_start: line.block_start_trim,
            block_end: line.block_end_trim,
        };
        paint_fragment.float_replay = line_box.float_replay;
        self.prepare_inline_line_fragment(
            &paint_fragment,
            InlinePaintContext {
                block_style,
                direction: line_direction,
                available_width: line.available_width,
                padding_left,
                line_indent: paint_indent,
                text_align,
                is_first_line: line.is_first_formatted_line,
                line_block_size: line.height(),
            },
        )
        .map(|mut prepared| {
            prepared.decoration_origin_fragments = Rc::clone(&line.decoration_origin_fragments);
            prepared
        })
    }

    /// Flushes a paragraph of inline items before a hard inline boundary.
    ///
    /// CSS Inline Layout forms line boxes from consecutive inline-level
    /// content, while CSS Paged Media can force a new page group before a later
    /// inline box with an explicit `page` value. Flushing here keeps the page
    /// switch between completed line boxes:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    fn flush_inline_item_paragraph(
        &mut self,
        paragraph: &mut Vec<InlineItem>,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        force_empty_line: bool,
        starts_after_forced_break: bool,
        plaintext_direction_state: &mut Option<Direction>,
    ) -> InlineLayoutOutcome {
        trim_inline_item_edges(paragraph);
        if paragraph.is_empty() {
            if force_empty_line
                && !context
                    .line_clamp
                    .is_some_and(|clamp| clamp.excludes_line(line_index))
            {
                if self.cursor_y - context.block_style.line_height < self.page_bottom() {
                    self.push_page();
                }
                self.cursor_y -= context.block_style.line_height;
                return InlineLayoutOutcome {
                    next_line_index: line_index + 1,
                    clamp_line_slots: 1,
                    clamp_block_advance: content_box_pt(context.block_style.line_height),
                    has_non_phantom_line: true,
                    has_flow_effects: true,
                    has_local_continuation_cutoff: false,
                };
            }
            return InlineLayoutOutcome {
                next_line_index: line_index,
                clamp_line_slots: 0,
                clamp_block_advance: content_box_pt(0.0),
                has_non_phantom_line: false,
                has_flow_effects: false,
                has_local_continuation_cutoff: false,
            };
        }
        // A forced break starts a new UAX #9 plaintext paragraph, unlike a
        // soft wrap which retains the direction established by the paragraph's
        // first selected line.
        // <https://www.w3.org/TR/css-writing-modes-4/#valdef-unicode-bidi-plaintext>
        // <https://www.unicode.org/reports/tr9/#P2>
        if starts_after_forced_break && context.block_style.unicode_bidi == UnicodeBidi::Plaintext {
            *plaintext_direction_state = None;
        }
        let outcome = self.layout_inline_paragraph(
            paragraph,
            context,
            line_index,
            starts_after_forced_break,
            plaintext_direction_state,
        );
        paragraph.clear();
        // A leading float is out of flow and therefore produces no selected
        // in-flow fragment by itself. A following forced break nevertheless
        // terminates an empty line box; otherwise each leading float causes
        // the first `br` to disappear and following content is one line too
        // close to the float.
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>
        // <https://drafts.csswg.org/css-inline-3/#line-boxes>
        if force_empty_line
            && !outcome.has_non_phantom_line
            && !context
                .line_clamp
                .is_some_and(|clamp| clamp.excludes_line(outcome.next_line_index))
        {
            if context.block_style.writing_mode == WritingMode::HorizontalTb
                && self.cursor_y - context.block_style.line_height < self.page_bottom()
            {
                self.push_page();
            }
            let mut stack = InlineLineStackCursor::new(
                context.block_style,
                self.content_left,
                self.content_right,
                self.cursor_y,
            );
            stack.advance(context.block_style.line_height);
            stack.apply(self);
            return InlineLayoutOutcome {
                next_line_index: outcome.next_line_index + 1,
                clamp_line_slots: outcome.clamp_line_slots + 1,
                clamp_block_advance: content_box_pt(
                    outcome.clamp_block_advance.points() + context.block_style.line_height,
                ),
                has_non_phantom_line: true,
                has_flow_effects: true,
                has_local_continuation_cutoff: outcome.has_local_continuation_cutoff,
            };
        }
        outcome
    }

    fn apply_inline_break_clearance(
        &mut self,
        clear: Clear,
        context: InlineParagraphContext<'_>,
        line_index: usize,
    ) -> usize {
        if clear == Clear::None {
            return line_index;
        }
        let clearance = self.resolve_block_clearance(BlockClearanceRequest::coincident_edges(
            clear,
            context.block_style.writing_mode,
            context.block_style.used_direction(),
            PageTopBlockPosition::new(self.cursor_y),
        ));
        // Clearance places the following line at the cleared float edge.
        // Keep the tolerance above only for deciding whether clearance was
        // applied; it must not perturb the used geometry.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        self.cursor_y = clearance.used_border_edge.points();
        line_index
    }

    fn apply_collected_line_clearance(
        &mut self,
        stack: InlineLineStackCursor,
        clear: Clear,
        context: InlineParagraphContext<'_>,
    ) -> InlineLineStackCursor {
        if clear == Clear::None {
            return stack;
        }
        stack.apply(self);
        self.apply_inline_break_clearance(clear, context, 0);
        InlineLineStackCursor::new(
            context.block_style,
            self.content_left,
            self.content_right,
            self.cursor_y,
        )
    }
}

/// Reopen CSS-generated `unicode-bidi` scopes after a forced line break.
///
/// Explicit breaks split collection into independently selected graphs, while
/// an inline CSS scope can continue across that boundary. Each new graph must
/// therefore receive the generated formatting controls active at the end of
/// the preceding graph. The controls remain virtual UAX #9 input: they do not
/// alter the retained author source or produce paint glyphs.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>
/// <https://www.unicode.org/reports/tr9/#Explicit_Levels_and_Directions>
fn recover_css_bidi_scope_continuations_across_forced_breaks(records: &mut [InlineLineRecord]) {
    let mut active_scopes = Vec::<CssBidiScopeContinuation>::new();
    let mut previous_paragraph_index = None;

    for record in records {
        if previous_paragraph_index.is_some_and(|previous| previous != record.paragraph_index)
            && let Some(fragment) = record.fragment.as_mut()
        {
            let carried_prefix = active_scopes
                .iter()
                .map(|scope| scope.start)
                .collect::<String>();
            if !carried_prefix.is_empty()
                && !fragment
                    .bidi_scope_continuations
                    .prefix
                    .starts_with(&carried_prefix)
            {
                fragment
                    .bidi_scope_continuations
                    .prefix
                    .insert_str(0, &carried_prefix);
            }
        }

        if let Some(fragment) = record.fragment.as_ref() {
            for item in fragment.items() {
                let InlineLineItem::Fragment(text) = &item.item else {
                    continue;
                };
                if text.source() != InlineTextSource::BidiControl {
                    continue;
                }
                for character in text.text().chars() {
                    update_css_bidi_scope_continuation_stack(&mut active_scopes, character);
                }
            }
        }
        previous_paragraph_index = Some(record.paragraph_index);
    }
}

#[derive(Clone, Copy)]
struct CssBidiScopeContinuation {
    start: char,
    is_isolate: bool,
}

fn update_css_bidi_scope_continuation_stack(
    scopes: &mut Vec<CssBidiScopeContinuation>,
    character: char,
) {
    let start = match character {
        '\u{202a}' | '\u{202b}' | '\u{202d}' | '\u{202e}' => Some(false),
        '\u{2066}' | '\u{2067}' | '\u{2068}' => Some(true),
        _ => None,
    };
    if let Some(is_isolate) = start {
        scopes.push(CssBidiScopeContinuation {
            start: character,
            is_isolate,
        });
        return;
    }

    match character {
        '\u{202c}' if scopes.last().is_some_and(|scope| !scope.is_isolate) => {
            scopes.pop();
        }
        '\u{2069}' => {
            if let Some(isolate_index) = scopes.iter().rposition(|scope| scope.is_isolate) {
                scopes.truncate(isolate_index);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::InlineIntrinsicMeasurement;
    use super::*;

    fn measured_fragment(text: &str, style: &ComputedStyle) -> MeasuredInlineItem {
        MeasuredInlineItem {
            item: InlineLineItem::Fragment(InlineFragment::new(
                text,
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
        }
    }

    fn measured_box_edge(style: &ComputedStyle) -> MeasuredInlineItem {
        MeasuredInlineItem {
            item: InlineLineItem::Atom(InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                    logical_edge: InlineLogicalEdge::End,
                    physical_side: PhysicalSide::Right,
                    positioning_containing_block_id: None,
                    advance: 0.0,
                    paint_extent: 0.0,
                })),
                style.clone(),
                None,
                InlineSize::new(0.0, style.line_height),
                0.0,
                0.0,
                None,
                None,
            )),
            width: 0.0,
            shaped: None,
        }
    }

    fn fragment_text(item: &MeasuredInlineItem) -> Option<&str> {
        match &item.item {
            InlineLineItem::Fragment(fragment) => Some(fragment.text()),
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
        }
    }

    fn line_record(is_phantom: bool, is_forced_empty: bool) -> InlineLineRecord {
        InlineLineRecord {
            paragraph_index: 0,
            block_line_index: 0,
            paragraph_line_index: 0,
            fragment: None,
            is_phantom,
            is_first_formatted_line: false,
            is_last_line_in_paragraph: false,
            is_forced_empty,
            starts_after_preserved_segment_break: false,
            clear_after: Clear::None,
            block_before: 0.0,
            block_start_trim: 0.0,
            block_end_trim: 0.0,
            paragraph_last_hanging_width: 0.0,
            used_indent: 0.0,
            available_width: 100.0,
            line_height: 10.0,
            decoration_origin_fragments: Default::default(),
        }
    }

    #[test]
    fn inline_layout_outcome_accumulates_clamp_slots_across_paragraphs() {
        let mut outcome = InlineLayoutOutcome {
            next_line_index: 1,
            clamp_line_slots: 1,
            clamp_block_advance: content_box_pt(10.0),
            has_non_phantom_line: true,
            has_flow_effects: true,
            has_local_continuation_cutoff: false,
        };
        outcome.include(InlineLayoutOutcome {
            next_line_index: 3,
            clamp_line_slots: 2,
            clamp_block_advance: content_box_pt(20.0),
            has_non_phantom_line: true,
            has_flow_effects: true,
            has_local_continuation_cutoff: false,
        });

        assert_eq!(outcome.next_line_index, 3);
        assert_eq!(outcome.clamp_line_slots, 3);
    }

    #[test]
    fn transparent_inline_records_do_not_create_fragmentation_lines() {
        let sequence = InlineLineSequence {
            // A collapsed space bracketed by zero-width inline scope markers
            // remains in source order, followed by a real forced empty line.
            records: vec![line_record(true, false), line_record(false, true)],
            ..InlineLineSequence::default()
        };

        assert!(!sequence.records[0].participates_in_widows_orphans());
        assert!(sequence.records[1].participates_in_widows_orphans());
        let fragment = sequence
            .fragment_break_selection(0, 0.0, false, 1, 1)
            .selected_fragment()
            .expect("transparent record should remain part of the selected source range");
        assert_eq!(fragment.range.start_index, 0);
        assert_eq!(fragment.range.record_count(), 1);
        assert_eq!(fragment.in_flow_line_count, 0);
        assert_eq!(
            fragment.constraint_outcome,
            InlineFragmentConstraintOutcome::Rule3Satisfied
        );
    }

    #[test]
    fn phantom_records_do_not_create_vertical_intrinsic_columns() {
        let measurement = InlineIntrinsicMeasurement {
            sequence: InlineLineSequence {
                // Collapsed-space-only records remain in source order but do
                // not create CSS line boxes, even when a vertical intrinsic
                // size maps those boxes to physical columns.
                records: vec![line_record(true, false), line_record(false, true)],
                ..InlineLineSequence::default()
            },
            ..InlineIntrinsicMeasurement::default()
        };
        let mut style = ComputedStyle::initial();

        for writing_mode in [WritingMode::VerticalLr, WritingMode::VerticalRl] {
            style.writing_mode = writing_mode;
            assert_eq!(measurement.logical_block_span(&style), 10.0);
        }
    }

    #[test]
    fn ltr_end_trim_removes_all_collapsible_spaces_through_box_edges() {
        let style = ComputedStyle::initial();
        let mut items = vec![
            measured_fragment("content", &style),
            measured_fragment(" ", &style),
            measured_box_edge(&style),
            measured_fragment(" ", &style),
        ];

        trim_visual_line_end_collapsible_spaces(&mut items, Direction::Ltr);

        assert_eq!(items.len(), 2);
        assert_eq!(fragment_text(&items[0]), Some("content"));
        assert!(matches!(
            &items[1].item,
            InlineLineItem::Atom(atom)
                if matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_)))
        ));
    }

    #[test]
    fn rtl_end_trim_removes_all_collapsible_spaces_through_box_edges() {
        let style = ComputedStyle::initial();
        let mut items = vec![
            measured_fragment(" ", &style),
            measured_box_edge(&style),
            measured_fragment(" ", &style),
            measured_fragment("content", &style),
        ];

        trim_visual_line_end_collapsible_spaces(&mut items, Direction::Rtl);

        assert_eq!(items.len(), 2);
        assert!(matches!(
            &items[0].item,
            InlineLineItem::Atom(atom)
                if matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_)))
        ));
        assert_eq!(fragment_text(&items[1]), Some("content"));
    }

    fn ordinary_line_sequence(line_count: usize) -> InlineLineSequence {
        InlineLineSequence {
            records: (0..line_count).map(|_| line_record(false, false)).collect(),
            ..InlineLineSequence::default()
        }
    }

    #[test]
    fn selected_line_stack_exposes_its_logical_block_contribution() {
        let sequence = ordinary_line_sequence(2);

        assert_eq!(sequence.logical_block_stack_extent(), content_box_pt(20.0));
    }

    fn four_column_geometry() -> MulticolumnInlinePaintGeometry {
        MulticolumnInlinePaintGeometry {
            column_count: 4,
            column_gap: 10.0,
            column_width: 20.0,
            column_height: 40.0,
            used_column_set_height: 40.0,
            wrap_column_rows: false,
            shrink_final_row: false,
        }
    }

    #[test]
    fn multicolumn_row_plan_commits_widow_legal_fragments_and_one_extent() {
        let sequence = ordinary_line_sequence(9);
        let mut style = ComputedStyle::initial();
        style.orphans = css::Orphans::try_new(1).expect("nonzero line count");
        style.widows = css::Widows::try_new(3).expect("nonzero line count");

        let rows = sequence.multicolumn_inline_row_plans(40.0, four_column_geometry(), &style);

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            row.block_extent,
            MulticolumnRowBlockExtent::new(layout_pt(40.0))
        );
        assert_eq!(row.decorated_column_count, 3);
        assert_eq!(
            row.columns
                .iter()
                .map(|fragment| (fragment.range.start_index, fragment.range.record_count()))
                .collect::<Vec<_>>(),
            vec![(0, 4), (4, 2), (6, 3)]
        );
        assert_eq!(
            row.columns
                .iter()
                .map(|fragment| fragment.block_extent)
                .collect::<Vec<_>>(),
            vec![
                MulticolumnColumnFragmentBlockExtent::new(layout_pt(40.0)),
                MulticolumnColumnFragmentBlockExtent::new(layout_pt(20.0)),
                MulticolumnColumnFragmentBlockExtent::new(layout_pt(30.0)),
            ]
        );
        // Rules deliberately take the single row extent above, never these
        // unequal source-fragment extents. Their distinct types prevent that
        // substitution at the paint adapter.
        assert!(row.columns.iter().all(|fragment| {
            fragment.constraint_outcome == InlineFragmentConstraintOutcome::Rule3Satisfied
        }));
    }

    #[test]
    fn multicolumn_row_plan_records_rule_three_relaxation_for_progress() {
        let sequence = ordinary_line_sequence(9);
        let mut style = ComputedStyle::initial();
        style.orphans = css::Orphans::try_new(3).expect("nonzero line count");
        style.widows = css::Widows::try_new(3).expect("nonzero line count");

        let rows = sequence.multicolumn_inline_row_plans(40.0, four_column_geometry(), &style);

        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .columns
                .iter()
                .map(|fragment| (fragment.range.start_index, fragment.range.record_count()))
                .collect::<Vec<_>>(),
            vec![(0, 4), (4, 3), (7, 2)]
        );
        assert_eq!(
            rows[0].columns[1].constraint_outcome,
            InlineFragmentConstraintOutcome::Rule3RelaxedForProgress
        );
    }
}

/// Used geometry for a balanced or sequential multi-column inline paint pass.
///
/// CSS Multi-column computes column count, width, gap, and used set height
/// before fragment painting consumes the selected inline line sequence:
/// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct MulticolumnInlinePaintGeometry {
    pub(in crate::layout) column_count: usize,
    pub(in crate::layout) column_gap: f32,
    pub(in crate::layout) column_width: f32,
    pub(in crate::layout) column_height: f32,
    pub(in crate::layout) used_column_set_height: f32,
    /// Continue a completed row of inner columns in the next outer
    /// fragmentainer instead of creating inline-direction overflow columns.
    pub(in crate::layout) wrap_column_rows: bool,
    /// Let the final row of an auto-height fragmented multicol shrink to its
    /// balanced content height instead of occupying the outer fragment limit.
    pub(in crate::layout) shrink_final_row: bool,
}

/// A non-negative block-axis extent shared by every column box in one
/// multi-column row.
///
/// CSS Multi-column requires column boxes in the same multicol line to have
/// one column height. Keeping that extent distinct from an individual line
/// fragment's consumed height prevents a later paint pass from shrinking a
/// column rule to a suffix of the row.
/// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct MulticolumnRowBlockExtent(LayoutLength);

impl MulticolumnRowBlockExtent {
    fn new(value: LayoutLength) -> Self {
        debug_assert!(value.get().is_finite() && value.get() >= 0.0);
        Self(value)
    }

    fn points(self) -> f32 {
        self.0.get()
    }
}

/// A non-negative block-axis extent consumed by one selected line fragment.
///
/// This is deliberately distinct from [`MulticolumnRowBlockExtent`]: it
/// describes source content, while a row extent describes the shared column
/// box and its rule geometry.
/// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct MulticolumnColumnFragmentBlockExtent(LayoutLength);

impl MulticolumnColumnFragmentBlockExtent {
    fn new(value: LayoutLength) -> Self {
        debug_assert!(value.get().is_finite() && value.get() >= 0.0);
        Self(value)
    }
}

/// An ordered, non-empty range of collected inline-line records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct InlineLineRecordRange {
    pub(in crate::layout) start_index: usize,
    end_index: usize,
}

impl InlineLineRecordRange {
    fn new(start_index: usize, end_index: usize, record_limit: usize) -> Self {
        debug_assert!(start_index < end_index);
        debug_assert!(end_index <= record_limit);
        Self {
            start_index,
            end_index,
        }
    }

    fn record_count(self) -> usize {
        self.end_index - self.start_index
    }
}

/// Whether a selected class-B break retained CSS Fragmentation rule 3.
///
/// A relaxed selection is only produced after the planner cannot prevent
/// overflow with a legal class-B break, as required by the ordered fallback
/// in CSS Fragmentation.
/// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineFragmentConstraintOutcome {
    Rule3Satisfied,
    Rule3RelaxedForProgress,
}

/// One committed inline-line fragment inside a column box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InlineLineFragmentPlan {
    pub(in crate::layout) range: InlineLineRecordRange,
    pub(in crate::layout) in_flow_line_count: usize,
    pub(in crate::layout) constraint_outcome: InlineFragmentConstraintOutcome,
    pub(in crate::layout) block_extent: MulticolumnColumnFragmentBlockExtent,
}

/// One committed row of anonymous column boxes for an inline-only multicol.
///
/// The row owns both the selected content fragments and their shared column
/// block extent. Its decoration count is likewise selected once so paint does
/// not infer row geometry from whichever column happened to be painted last.
/// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct MulticolumnInlineRowPlan {
    pub(in crate::layout) columns: Vec<InlineLineFragmentPlan>,
    pub(in crate::layout) block_extent: MulticolumnRowBlockExtent,
    pub(in crate::layout) decorated_column_count: usize,
}

/// Committed inline multicolumn rows and their paint geometry.
///
/// The line sequence is measured once to select every needed row, its column
/// fragments, and the principal box's used block extent. Painting consumes
/// these decisions instead of independently rebalancing the same source
/// records.
/// <https://www.w3.org/TR/css-multicol-1/#filling-columns>
#[derive(Debug, Clone)]
pub(in crate::layout) struct MulticolumnInlineLayoutPlan {
    pub(in crate::layout) geometry: MulticolumnInlinePaintGeometry,
    pub(in crate::layout) rows: Vec<MulticolumnInlineRowPlan>,
}

impl<'a> LayoutBuilder<'a> {
    /// Resolve the used geometry for a collected inline multicolumn sequence.
    ///
    /// This is the sole authority for the first row's balanced height and the
    /// auto-height principal box's used block extent.  A definite CSS height
    /// remains a fragmentainer constraint; an automatic height is established
    /// by the selected column row itself.
    /// <https://www.w3.org/TR/css-multicol-1/#filling-columns>
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn plan_multicolumn_inline_layout(
        &self,
        sequence: &InlineLineSequence,
        style: &ComputedStyle,
        column_count: usize,
        column_gap: f32,
        column_width: f32,
        available_width: f32,
        content_height: Option<f32>,
    ) -> MulticolumnInlineLayoutPlan {
        let auto_fill_max_height = (content_height.is_none()
            && style.column_fill == css::ColumnFill::Auto)
            .then(|| {
                used_max_height(style, PercentageBasis::definite(layout_pt(available_width)))
                    .map(SemanticLengthExt::points)
            })
            .flatten();
        let repeated_block_end_decoration =
            if style.box_decoration_break == css::BoxDecorationBreak::Clone {
                style.padding.bottom + used_border_widths(style).bottom
            } else {
                0.0
            };
        let remaining_parent_height =
            (self.cursor_y - self.page_bottom() - repeated_block_end_decoration)
                .max(css::CSS_PX_TO_PT);
        let balanced_height = sequence.balanced_multicolumn_height(column_count, style);
        let sequential_auto_height = sequence.total_height().max(style.line_height);
        let natural_column_height = content_height
            .or(auto_fill_max_height)
            .unwrap_or(match style.column_fill {
                css::ColumnFill::Auto => sequential_auto_height,
                css::ColumnFill::Balance | css::ColumnFill::BalanceAll => balanced_height,
            });
        let fragmented_by_parent = self.active_fragmentainer_kind() == FragmentainerKind::Column
            && natural_column_height > remaining_parent_height + 0.01;
        let definite_fragment_height = content_height.map(|height| {
            if fragmented_by_parent {
                height.min(remaining_parent_height)
            } else {
                height
            }
        });
        let unconstrained_column_height = match style.column_fill {
            css::ColumnFill::Auto => definite_fragment_height
                .or(auto_fill_max_height)
                .unwrap_or(sequential_auto_height),
            css::ColumnFill::Balance | css::ColumnFill::BalanceAll => definite_fragment_height
                .map(|limit| balanced_height.min(limit))
                .unwrap_or(balanced_height),
        };
        let column_height = if fragmented_by_parent {
            unconstrained_column_height.min(remaining_parent_height)
        } else {
            unconstrained_column_height
        }
        .max(style.line_height.min(remaining_parent_height));
        let first_row_height = matches!(
            style.column_fill,
            css::ColumnFill::Balance | css::ColumnFill::BalanceAll
        )
        .then_some(balanced_height)
        .filter(|height| *height <= column_height + 0.01)
        .unwrap_or(column_height);
        let used_column_set_height = if let Some(height) = definite_fragment_height {
            height
        } else if let Some(max_height) = auto_fill_max_height {
            sequence
                .total_height()
                .min(max_height)
                .max(style.line_height)
        } else {
            // An auto-height balanced set ends at the selected row, not at a
            // provisional fragmentainer capacity.
            first_row_height
        };
        let local_discard_region_set =
            matches!(style.used_continuation(), css::UsedContinuation::Discard(_));
        let geometry = MulticolumnInlinePaintGeometry {
            column_count,
            column_gap,
            column_width,
            column_height,
            used_column_set_height,
            // A discard container captures the first region break after its
            // initial column set. It never turns that local Category-3
            // break into an outer page/column advancement.
            wrap_column_rows: fragmented_by_parent && !local_discard_region_set,
            shrink_final_row: content_height.is_none(),
        };
        MulticolumnInlineLayoutPlan {
            rows: sequence.multicolumn_inline_row_plans(first_row_height, geometry, style),
            geometry,
        }
    }
}

/// Graph-selected inline line records for a block container.
///
/// The sequence is the durable CSS Text/CSS Fragmentation handoff: line
/// selection comes from the inline opportunity graph, while page fitting and
/// slice painting consume these already-selected records. This keeps forced
/// empty lines, line heights, indents, and hanging punctuation reserves aligned
/// with CSS Text line breaking and CSS Fragmentation widows/orphans handling:
/// <https://www.w3.org/TR/css-text-3/#line-breaking> and
/// <https://www.w3.org/TR/css-break-3/#widows-orphans>.
#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct InlineLineSequence {
    pub(in crate::layout) records: Vec<InlineLineRecord>,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) padding_left: f32,
    pub(in crate::layout) hanging_indent: f32,
    pub(in crate::layout) hanging_punctuation_reserve: f32,
    pub(in crate::layout) fragment_text_box_trim: TextBoxLineTrim,
    pub(in crate::layout) has_flow_side_effects: bool,
    /// Declares whether replaying these selected lines may observe floats from
    /// the formatting context that happens to be active at paint time.
    ///
    /// Captured nested formatting contexts retain this decision with their
    /// durable line records so a parent float band cannot be applied again
    /// when their already-positioned atom is painted.
    pub(in crate::layout) replay_float_scope: ReplayFloatScope,
    /// A locally selected automatic clamp point or Category-3 discard break
    /// suppresses later in-flow source without materializing a page/column.
    pub(in crate::layout) has_local_continuation_cutoff: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct InlineLayoutOutcome {
    pub(in crate::layout) next_line_index: usize,
    /// Number of line-selection slots consumed by this inline layout pass.
    ///
    /// This is deliberately distinct from physical cursor movement: line clamp
    /// applies to the selector's line stream, not margins, replaced sizes, or
    /// fragmentainer transitions.
    pub(in crate::layout) clamp_line_slots: usize,
    /// Measured content-box block advance of the selected source lines.
    ///
    /// Automatic line clamping debits this typed value when a mixed block-flow
    /// walker crosses an inline-run boundary. It is deliberately distinct
    /// from page cursor movement, margins, and fragmentainer transitions.
    pub(in crate::layout) clamp_block_advance: ContentBoxLength,
    pub(in crate::layout) has_non_phantom_line: bool,
    pub(in crate::layout) has_flow_effects: bool,
    pub(in crate::layout) has_local_continuation_cutoff: bool,
}

impl InlineLayoutOutcome {
    pub(in crate::layout) fn include(&mut self, other: Self) {
        self.next_line_index = other.next_line_index;
        self.clamp_line_slots += other.clamp_line_slots;
        self.clamp_block_advance =
            content_box_pt(self.clamp_block_advance.points() + other.clamp_block_advance.points());
        self.has_non_phantom_line |= other.has_non_phantom_line;
        self.has_flow_effects |= other.has_flow_effects;
        self.has_local_continuation_cutoff |= other.has_local_continuation_cutoff;
    }
}

#[derive(Debug, Clone, Copy)]
struct InlineLineSequenceCursor {
    paragraph_index: usize,
    line_index: usize,
    /// Extra physical block displacement selected by an earlier graph. This
    /// survives explicit breaks because they split source graphs, not the
    /// line stack's page-local coordinate system.
    physical_block_offset: f32,
    starts_after_forced_break: bool,
    starts_after_preserved_segment_break: bool,
    has_flow_side_effects: bool,
    /// A float-only graph can be followed by a forced break. Keep its
    /// committed transaction until the next graph has reselected source
    /// against the float exclusion.
    pending_inline_float_replay: Option<CommittedInlineFloatReplay>,
}

fn clearance_only_inline_line_record(
    cursor: InlineLineSequenceCursor,
    context: InlineParagraphContext<'_>,
    clear: Clear,
) -> InlineLineRecord {
    InlineLineRecord {
        paragraph_index: cursor.paragraph_index,
        block_line_index: cursor.line_index,
        paragraph_line_index: 0,
        fragment: None,
        is_phantom: false,
        is_first_formatted_line: false,
        is_last_line_in_paragraph: true,
        is_forced_empty: false,
        starts_after_preserved_segment_break: false,
        clear_after: clear,
        block_before: 0.0,
        block_start_trim: 0.0,
        block_end_trim: 0.0,
        paragraph_last_hanging_width: 0.0,
        used_indent: used_line_indent(
            cursor.line_index,
            cursor.starts_after_forced_break,
            context.hanging_indent,
            context.block_style,
            context.available_width,
        ),
        available_width: context.available_width,
        line_height: 0.0,
        decoration_origin_fragments: Default::default(),
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineLineStackCursor {
    writing_mode: WritingMode,
    content_left: f32,
    content_right: f32,
    cursor_y: f32,
}

impl InlineLineStackCursor {
    /// Track line-box stack progression in physical coordinates.
    ///
    /// CSS Inline stacks line boxes in the block axis, while CSS Writing Modes
    /// maps that logical block axis to physical y in horizontal writing and to
    /// physical x in vertical writing:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(in crate::layout) fn new(
        block_style: &ComputedStyle,
        content_left: f32,
        content_right: f32,
        cursor_y: f32,
    ) -> Self {
        Self {
            writing_mode: block_style.writing_mode,
            content_left,
            content_right,
            cursor_y,
        }
    }

    pub(in crate::layout) fn apply(self, layout: &mut LayoutBuilder<'_>) {
        layout.content_left = self.content_left;
        layout.content_right = self.content_right;
        layout.cursor_y = self.cursor_y;
    }

    pub(in crate::layout) fn advance(&mut self, line_block_size: f32) {
        match self.writing_mode {
            WritingMode::HorizontalTb => {
                self.cursor_y -= line_block_size;
            }
            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                self.content_left -= line_block_size;
                self.content_right -= line_block_size;
            }
            WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                self.content_left += line_block_size;
                self.content_right += line_block_size;
            }
        }
    }
}

impl InlineLineSequence {
    /// Mark a captured line sequence with the float visibility of its owning
    /// formatting context.
    ///
    /// Selection has already occurred with the same scope.  Isolated nested
    /// sequences freeze their own selected band so a parent float cannot
    /// become visible merely because the sequence is painted later.
    /// <https://www.w3.org/TR/CSS22/visuren.html#block-boxes>
    pub(in crate::layout) fn with_replay_float_scope(mut self, scope: ReplayFloatScope) -> Self {
        self.replay_float_scope = scope;
        if matches!(scope, ReplayFloatScope::IsolatedFormattingContext) {
            for record in &mut self.records {
                if let Some(fragment) = &mut record.fragment {
                    fragment.freeze_float_band();
                }
            }
        }
        self
    }

    pub(in crate::layout) fn context<'a>(
        &self,
        block_style: &'a ComputedStyle,
    ) -> InlineParagraphContext<'a> {
        InlineParagraphContext {
            block_style,
            line_clamp: used_line_clamp_for_style(block_style),
            clamp_continuation: css::ClampContinuation::None,
            stylesheets: &css::EMPTY_STYLESHEETS,
            initial_first_formatted_line: true,
            available_width: self.available_width,
            padding_left: self.padding_left,
            hanging_indent: self.hanging_indent,
            hanging_punctuation_reserve: self.hanging_punctuation_reserve,
        }
    }

    pub(in crate::layout) fn total_height(&self) -> f32 {
        self.fragment_height(0, self.records.len())
    }

    /// Return the selected line stack's logical block contribution.
    ///
    /// A vertical formatting context maps this logical block stack to its
    /// physical width. Keeping the conversion on the durable sequence makes
    /// an orthogonal auto-sized box consume the exact records that its final
    /// paint replays.
    /// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
    pub(in crate::layout) fn logical_block_stack_extent(&self) -> ContentBoxLength {
        content_box_pt(self.total_height().max(0.0))
    }

    /// Return the physical inline extent occupied by the selected text.
    ///
    /// CSS Writing Modes maps the logical inline axis to physical width in
    /// horizontal writing modes and physical height in vertical writing modes.
    /// Fixed generated boxes, such as CSS Paged Media margin boxes, can use
    /// this intrinsic extent for `vertical-align` placement before painting
    /// the selected line sequence. It is not a used box size:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
    /// <https://www.w3.org/TR/css-page-3/#page-margin-boxes>.
    pub(in crate::layout) fn occupied_physical_inline_extent(
        &self,
        block_style: &ComputedStyle,
    ) -> OccupiedPhysicalInlineExtent {
        let extent = match block_style.writing_mode {
            WritingMode::HorizontalTb => self.total_height(),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self
                .records
                .iter()
                .filter_map(|record| record.fragment.as_ref())
                .map(|fragment| fragment.metrics.width)
                .fold(0.0, f32::max)
                .max(0.0),
        };
        OccupiedPhysicalInlineExtent::new(content_box_pt(extent))
    }

    /// Return the first line's logical block size for fixed-box placement.
    ///
    /// Vertical writing modes position the first line column from the physical
    /// block-start edge before advancing subsequent columns in the block axis.
    pub(in crate::layout) fn fixed_box_first_line_block_size(&self) -> f32 {
        self.records
            .first()
            .map(InlineLineRecord::height)
            .unwrap_or(0.0)
    }

    pub(in crate::layout) fn line_count(&self) -> usize {
        self.records.len()
    }

    /// Return the first painted line baseline in sequence-local block coordinates.
    ///
    /// CSS Flexbox and CSS Grid export baselines from their participating
    /// in-flow line boxes. Keeping the query on the collected line sequence
    /// makes those layout modes use the same line-record metrics as inline
    /// painting, including text-box trimming and synthesized inline metrics:
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>,
    /// <https://www.w3.org/TR/css-grid-1/#grid-baselines>, and
    /// <https://www.w3.org/TR/css-inline-3/#baseline-layout>.
    pub(in crate::layout) fn first_line_baseline_offset(&self, fallback: f32) -> f32 {
        self.records
            .first()
            .and_then(|record| record.fragment.as_ref())
            .map(|fragment| fragment.metrics.baseline_offset)
            .unwrap_or(fallback)
    }

    /// Return the last painted line baseline in sequence-local block coordinates.
    ///
    /// The returned value includes the block-size advance of all preceding line
    /// records, so callers can add border/padding once and directly store a
    /// content-box baseline.
    pub(in crate::layout) fn last_line_baseline_offset(&self, fallback: f32) -> f32 {
        let preceding = self
            .records
            .iter()
            .take(self.records.len().saturating_sub(1))
            .map(InlineLineRecord::height)
            .sum::<f32>();
        preceding
            + self
                .records
                .last()
                .and_then(|record| record.fragment.as_ref())
                .map(|fragment| fragment.metrics.baseline_offset)
                .unwrap_or(fallback)
    }

    pub(in crate::layout) fn has_non_phantom_line(&self) -> bool {
        self.records.iter().any(|record| !record.is_phantom)
    }

    pub(in crate::layout) fn has_flow_effects(&self) -> bool {
        self.has_flow_side_effects
            || self.has_non_phantom_line()
            || self
                .records
                .iter()
                .any(InlineLineRecord::has_inline_layout_effects)
    }

    pub(in crate::layout) fn layout_outcome(&self) -> InlineLayoutOutcome {
        InlineLayoutOutcome {
            next_line_index: self.records.len(),
            clamp_line_slots: self.records.len(),
            clamp_block_advance: content_box_pt(
                self.records
                    .iter()
                    .map(InlineLineRecord::block_advance)
                    .sum(),
            ),
            has_non_phantom_line: self.has_non_phantom_line(),
            has_flow_effects: self.has_flow_effects(),
            has_local_continuation_cutoff: self.has_local_continuation_cutoff,
        }
    }

    // Used by tests to lock down preserved forced-break accounting before
    // production fragmentation needs this value directly.
    #[allow(dead_code)]
    pub(in crate::layout) fn forced_empty_line_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| record.is_forced_empty)
            .count()
    }

    pub(in crate::layout) fn line_height(&self, index: usize) -> f32 {
        self.fragment_line_height(index, 0, self.records.len())
    }

    /// Return the balanced block size for an inline sequence in multicolumn layout.
    ///
    /// CSS Multi-column layout balances auto-height column boxes by choosing a
    /// column block size that can fit the content across the used columns. Reuse
    /// the same line-fitting logic as pagination so widows and orphans affect
    /// column breaks consistently with CSS Fragmentation:
    /// <https://www.w3.org/TR/css-multicol-1/#filling-columns> and
    /// <https://www.w3.org/TR/css-break-3/#widows-orphans>.
    pub(in crate::layout) fn balanced_multicolumn_height(
        &self,
        column_count: usize,
        block_style: &ComputedStyle,
    ) -> f32 {
        self.balanced_multicolumn_height_from(0, column_count, block_style)
    }

    /// Return the balanced height for the unpainted suffix of a fragmented
    /// inline multicol sequence.
    ///
    /// Each non-final outer fragment uses its available block-size limit. The
    /// last fragment balances only the remaining lines across its columns:
    /// <https://www.w3.org/TR/css-multicol-1/#filling-columns>.
    pub(in crate::layout) fn balanced_multicolumn_height_from(
        &self,
        start_index: usize,
        column_count: usize,
        block_style: &ComputedStyle,
    ) -> f32 {
        if start_index >= self.records.len() {
            return 0.0;
        }
        if column_count <= 1 {
            return self.fragment_height(start_index, self.records.len() - start_index);
        }

        let mut previous_candidate = 0.0;
        for count in 1..=self.records.len() - start_index {
            let candidate = self.fragment_height(start_index, count);
            if candidate <= previous_candidate + 0.01 {
                continue;
            }
            previous_candidate = candidate;
            if self.multicolumn_height_paints_all_lines_from(
                start_index,
                candidate,
                column_count,
                block_style.orphans.get(),
                block_style.widows.get(),
            ) {
                return candidate;
            }
        }

        self.fragment_height(start_index, self.records.len() - start_index)
    }

    pub(in crate::layout) fn fragment_records_for_paint(
        &self,
        start_index: usize,
        count: usize,
    ) -> Vec<InlineLineRecord> {
        let end_index = start_index.saturating_add(count).min(self.records.len());
        let mut records = self.records[start_index..end_index].to_vec();
        self.populate_text_decoration_origin_fragments(start_index, &mut records);
        self.apply_fragment_text_box_trim_to_records(&mut records);
        records
    }

    /// Attach decorating-box fragment extents to the records selected for
    /// paint.  Decoration receiver provenance is deliberately not consulted:
    /// descendants receive an origin's line, while the origin's own sequence
    /// of inline fragments supplies `text-decoration-inset` percentage bases.
    ///
    /// CSS Text Decoration Level 4 § 2.9.1 defines those bases in terms of
    /// decorating-box fragments and their complete inline extent:
    /// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-inset-property>
    fn populate_text_decoration_origin_fragments(
        &self,
        selected_start: usize,
        selected_records: &mut [InlineLineRecord],
    ) {
        #[derive(Clone)]
        struct OriginExtent {
            origin_style: Rc<ComputedStyle>,
            per_record: Vec<f32>,
        }

        let mut origins: Vec<OriginExtent> = Vec::new();
        for (record_index, record) in self.records.iter().enumerate() {
            let Some(line_fragment) = &record.fragment else {
                continue;
            };
            for item in line_fragment.items() {
                let InlineLineItem::Fragment(fragment) = &item.item else {
                    continue;
                };
                for layer in &fragment.style().text_decoration_layers {
                    let origin = origins
                        .iter_mut()
                        .find(|candidate| Rc::ptr_eq(&candidate.origin_style, &layer.origin_style));
                    let origin = match origin {
                        Some(origin) => origin,
                        None => {
                            origins.push(OriginExtent {
                                origin_style: Rc::clone(&layer.origin_style),
                                per_record: vec![0.0; self.records.len()],
                            });
                            origins.last_mut().expect("just pushed decoration origin")
                        }
                    };
                    if layer.origin_style.display.is_block_level() {
                        // A block decorating box owns the selected line
                        // fragment, including a discarded edge-space source
                        // item.  Its used fragment span is the line's Phase
                        // II content measure, rather than the sum of raw
                        // source advances.
                        origin.per_record[record_index] = line_fragment.metrics.width;
                    } else {
                        origin.per_record[record_index] += item.width;
                    }
                }
            }
        }

        // Decoration origins are keyed by their stable cascade-time `Rc`, not
        // by declaration equality: equal nested declarations remain distinct.
        for (offset, record) in selected_records.iter_mut().enumerate() {
            let record_index = selected_start + offset;
            let geometries = origins
                .iter()
                .filter_map(|origin| {
                    let fragment_inline_extent = origin.per_record[record_index];
                    (fragment_inline_extent > 0.0).then(|| {
                        let preceding_inline_extent =
                            origin.per_record[..record_index].iter().sum::<f32>();
                        let total_inline_extent = origin.per_record.iter().sum::<f32>();
                        let following_inline_extent =
                            total_inline_extent - preceding_inline_extent - fragment_inline_extent;
                        TextDecorationOriginFragmentGeometry {
                            origin_style: Rc::clone(&origin.origin_style),
                            total_inline_extent: layout_pt(total_inline_extent),
                            fragment_inline_extent: layout_pt(fragment_inline_extent),
                            preceding_inline_extent: layout_pt(preceding_inline_extent),
                            following_inline_extent: layout_pt(following_inline_extent.max(0.0)),
                            is_first_fragment: preceding_inline_extent <= f32::EPSILON,
                            is_last_fragment: following_inline_extent <= f32::EPSILON,
                        }
                    })
                })
                .collect::<Vec<_>>();
            record.decoration_origin_fragments = Rc::from(geometries.into_boxed_slice());
        }
    }

    pub(in crate::layout) fn fragment_records_for_slice_paint(
        &self,
        block_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) -> (f32, Vec<InlineLineRecord>) {
        let Some((start_index, start_block_top)) =
            self.first_slice_visible_line(block_top, slice_top, slice_bottom)
        else {
            return (block_top, Vec::new());
        };
        let count = if self.fragment_text_box_trim.is_empty() {
            self.uncloned_slice_visible_line_count(start_index, start_block_top, slice_bottom)
        } else {
            self.cloned_slice_visible_line_count(start_index, start_block_top, slice_bottom)
        };
        (
            start_block_top,
            self.fragment_records_for_paint(start_index, count),
        )
    }

    fn first_slice_visible_line(
        &self,
        block_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) -> Option<(usize, f32)> {
        let mut line_block_start = block_top;
        for index in 0..self.records.len() {
            let line_top = line_block_start - self.records[index].block_before;
            let line_height = if self.fragment_text_box_trim.is_empty() {
                self.records[index].height()
            } else {
                self.fragment_line_height(index, index, 1)
            };
            let line_bottom = line_top - line_height;
            if line_top >= slice_bottom && line_bottom <= slice_top {
                return Some((index, line_block_start));
            }
            line_block_start = line_bottom;
        }
        None
    }

    fn uncloned_slice_visible_line_count(
        &self,
        start_index: usize,
        start_block_top: f32,
        slice_bottom: f32,
    ) -> usize {
        let mut line_block_start = start_block_top;
        let mut count = 0;
        for record in &self.records[start_index..] {
            let line_top = line_block_start - record.block_before;
            let line_bottom = line_top - record.height();
            if line_top < slice_bottom || line_bottom > start_block_top {
                break;
            }
            count += 1;
            if line_bottom <= slice_bottom {
                break;
            }
            line_block_start = line_bottom;
        }
        count
    }

    fn cloned_slice_visible_line_count(
        &self,
        start_index: usize,
        start_block_top: f32,
        slice_bottom: f32,
    ) -> usize {
        let mut count = 0;
        for index in start_index..self.records.len() {
            let candidate_count = index - start_index + 1;
            let fragment_bottom =
                start_block_top - self.fragment_height(start_index, candidate_count);
            if fragment_bottom < slice_bottom - 0.01 {
                break;
            }
            count = candidate_count;
            if fragment_bottom <= slice_bottom + 0.01 {
                break;
            }
        }
        count.max(usize::from(start_index < self.records.len()))
    }

    fn fragment_height(&self, start_index: usize, count: usize) -> f32 {
        (start_index..start_index.saturating_add(count).min(self.records.len()))
            .map(|index| self.fragment_line_height(index, start_index, count))
            .sum()
    }

    fn fragment_line_height(&self, index: usize, start_index: usize, count: usize) -> f32 {
        let Some(record) = self.records.get(index) else {
            return 0.0;
        };
        let mut line = record.clone();
        if let Some((block_start_index, block_end_index)) =
            self.fragment_text_box_trim_indices(start_index, count)
        {
            if Some(index) == block_start_index {
                line.block_start_trim = self.fragment_text_box_trim.block_start;
            }
            if Some(index) == block_end_index {
                line.block_end_trim = self.fragment_text_box_trim.block_end;
            }
        }
        line.block_advance()
    }

    /// Commit the line fragments and shared block extent for every row that
    /// this inline multicolumn pass will paint.
    ///
    /// The plan is deliberately formed before any row paints: CSS
    /// Fragmentation's class-B decision and CSS Multi-column's common column
    /// height are one layout decision, not independent paint-time estimates.
    /// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
    /// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
    fn multicolumn_inline_row_plans(
        &self,
        first_row_fit_height: f32,
        geometry: MulticolumnInlinePaintGeometry,
        block_style: &ComputedStyle,
    ) -> Vec<MulticolumnInlineRowPlan> {
        let mut rows = Vec::new();
        let mut next_start_index = 0;
        while next_start_index < self.records.len() {
            let row_fit_height = if rows.is_empty() {
                first_row_fit_height
            } else {
                matches!(
                    block_style.column_fill,
                    css::ColumnFill::Balance | css::ColumnFill::BalanceAll
                )
                .then(|| {
                    self.balanced_multicolumn_height_from(
                        next_start_index,
                        geometry.column_count,
                        block_style,
                    )
                })
                .filter(|height| *height <= geometry.column_height + 0.01)
                .unwrap_or(geometry.column_height)
            };
            let mut columns = Vec::with_capacity(geometry.column_count);
            for _ in 0..geometry.column_count {
                if next_start_index >= self.records.len() {
                    break;
                }
                // Rule 3 is relaxed only when this finite column cannot make
                // progress otherwise. The selection records that fact rather
                // than making the paint path infer it from a zero count.
                let fragment = self
                    .fragment_break_selection(
                        next_start_index,
                        row_fit_height,
                        true,
                        block_style.orphans.get(),
                        block_style.widows.get(),
                    )
                    .selected_fragment()
                    .expect("a nonempty multicolumn row must select progress");
                debug_assert_eq!(fragment.range.start_index, next_start_index);
                next_start_index = fragment.range.end_index;
                columns.push(fragment);
            }
            debug_assert!(!columns.is_empty());
            let is_final_row = next_start_index >= self.records.len();
            let block_extent = if geometry.shrink_final_row && is_final_row {
                row_fit_height
            } else {
                geometry.used_column_set_height
            };
            rows.push(MulticolumnInlineRowPlan {
                decorated_column_count: multicol_decorated_column_count(
                    block_style,
                    columns.len(),
                    geometry.column_count,
                ),
                columns,
                block_extent: MulticolumnRowBlockExtent::new(layout_pt(block_extent)),
            });
            if is_final_row || !geometry.wrap_column_rows {
                break;
            }
        }
        rows
    }

    fn multicolumn_height_paints_all_lines_from(
        &self,
        start_index: usize,
        column_height: f32,
        column_count: usize,
        orphans: usize,
        widows: usize,
    ) -> bool {
        let mut painted = start_index;
        for _ in 0..column_count {
            if painted >= self.records.len() {
                return true;
            }
            let selection =
                self.fragment_break_selection(painted, column_height, false, orphans, widows);
            let Some(fragment_count) = selection.line_count() else {
                return false;
            };
            painted += fragment_count;
        }
        painted >= self.records.len()
    }

    fn apply_fragment_text_box_trim_to_records(&self, records: &mut [InlineLineRecord]) {
        if self.fragment_text_box_trim.is_empty() || records.is_empty() {
            return;
        }
        if self.fragment_text_box_trim.trims_block_start
            && self.fragment_text_box_trim.block_start > 0.0
            && let Some(record) = records
                .iter_mut()
                .find(|record| record.fragment.is_some() && !record.is_phantom)
        {
            record.block_start_trim = self.fragment_text_box_trim.block_start;
        }
        if self.fragment_text_box_trim.trims_block_end
            && self.fragment_text_box_trim.block_end > 0.0
            && let Some(record) = records
                .iter_mut()
                .rev()
                .find(|record| record.fragment.is_some() && !record.is_phantom)
        {
            record.block_end_trim = self.fragment_text_box_trim.block_end;
        }
    }

    fn fragment_text_box_trim_indices(
        &self,
        start_index: usize,
        count: usize,
    ) -> Option<(Option<usize>, Option<usize>)> {
        if self.fragment_text_box_trim.is_empty() {
            return None;
        }
        let end_index = start_index.saturating_add(count).min(self.records.len());
        if start_index >= end_index {
            return None;
        }
        let block_start_index = (self.fragment_text_box_trim.trims_block_start
            && self.fragment_text_box_trim.block_start > 0.0)
            .then(|| {
                (start_index..end_index).find(|index| {
                    self.records
                        .get(*index)
                        .is_some_and(|record| record.fragment.is_some() && !record.is_phantom)
                })
            })
            .flatten();
        let block_end_index = (self.fragment_text_box_trim.trims_block_end
            && self.fragment_text_box_trim.block_end > 0.0)
            .then(|| {
                (start_index..end_index).rev().find(|index| {
                    self.records
                        .get(*index)
                        .is_some_and(|record| record.fragment.is_some() && !record.is_phantom)
                })
            })
            .flatten();
        Some((block_start_index, block_end_index))
    }

    /// Select the next fragment's line records under the CSS Fragmentation
    /// widows/orphans constraints.
    ///
    /// A balancing probe passes `allow_unavoidable_relaxation: false`: a
    /// candidate column height that cannot host a legal class-B break is not a
    /// fitting balanced height. Painting permits the narrowly-scoped fallback
    /// at a fragmentainer start so an undersized finite fragmentainer can
    /// still make progress.
    /// <https://www.w3.org/TR/css-break-3/#widows-orphans>
    pub(in crate::layout) fn fragment_break_selection(
        &self,
        start_index: usize,
        available_height: f32,
        allow_unavoidable_relaxation: bool,
        orphans: usize,
        widows: usize,
    ) -> InlineFragmentBreakSelection {
        let remaining_record_count = self.records.len().saturating_sub(start_index);
        if remaining_record_count == 0 {
            return InlineFragmentBreakSelection::NoLines;
        }

        let mut fitting = 0;
        for index in start_index..self.records.len() {
            let candidate_count = index - start_index + 1;
            let candidate_height = self.fragment_height(start_index, candidate_count);
            if candidate_height > available_height + 0.01 {
                break;
            }
            fitting += 1;
        }

        if fitting == 0 {
            return if allow_unavoidable_relaxation {
                self.selected_fragment(
                    start_index,
                    1,
                    InlineFragmentConstraintOutcome::Rule3RelaxedForProgress,
                )
            } else {
                InlineFragmentBreakSelection::NoRoom
            };
        }
        if fitting >= remaining_record_count {
            return self.selected_fragment(
                start_index,
                remaining_record_count,
                InlineFragmentConstraintOutcome::Rule3Satisfied,
            );
        }

        // CSS Fragmentation 3 defines `orphans` and `widows` as constraints on
        // in-flow line boxes at unforced breaks inside a block container.
        // Float-only records are still kept in the selected record range so
        // their placement is committed with surrounding flow, but an
        // out-of-flow float is not itself an orphan or widow.
        // https://www.w3.org/TR/css-break-3/#widows-orphans
        let remaining_total = self.records[start_index..]
            .iter()
            .filter(|record| record.participates_in_widows_orphans())
            .count();
        let fitting_line_count = self.records[start_index..start_index + fitting]
            .iter()
            .filter(|record| record.participates_in_widows_orphans())
            .count();
        if fitting_line_count == 0 || remaining_total == 0 {
            return self.selected_fragment(
                start_index,
                fitting,
                InlineFragmentConstraintOutcome::Rule3Satisfied,
            );
        }
        let orphans = orphans.max(1);
        let widows = widows.max(1);
        // A block with no spare line beyond either requirement has no legal
        // class-B break. It must move as a unit when possible, rather than
        // treating one satisfied orphan line as permission to strand a widow.
        if remaining_total <= orphans || remaining_total <= widows {
            return if allow_unavoidable_relaxation {
                self.selected_fragment(
                    start_index,
                    fitting,
                    InlineFragmentConstraintOutcome::Rule3RelaxedForProgress,
                )
            } else {
                InlineFragmentBreakSelection::KeepTogether
            };
        }
        if fitting_line_count < orphans {
            return if allow_unavoidable_relaxation {
                self.selected_fragment(
                    start_index,
                    fitting,
                    InlineFragmentConstraintOutcome::Rule3RelaxedForProgress,
                )
            } else {
                InlineFragmentBreakSelection::KeepTogether
            };
        }
        let remaining_after_fitting = remaining_total - fitting_line_count;
        if remaining_after_fitting >= widows {
            return self.selected_fragment(
                start_index,
                fitting,
                InlineFragmentConstraintOutcome::Rule3Satisfied,
            );
        }

        let preferred = remaining_total.saturating_sub(widows);
        if preferred >= orphans {
            return self.selected_fragment(
                start_index,
                self.record_count_through_in_flow_lines(
                    start_index,
                    fitting,
                    preferred.min(fitting_line_count),
                ),
                InlineFragmentConstraintOutcome::Rule3Satisfied,
            );
        }

        // The source block is large enough for both requirements, but this
        // particular continuation cannot satisfy them simultaneously. Keep
        // the required preceding lines and leave as many following lines as
        // possible, which is CSS Fragmentation's unforced-break relaxation.
        self.selected_fragment(
            start_index,
            self.record_count_through_in_flow_lines(start_index, fitting, orphans),
            InlineFragmentConstraintOutcome::Rule3RelaxedForProgress,
        )
    }

    fn selected_fragment(
        &self,
        start_index: usize,
        record_count: usize,
        constraint_outcome: InlineFragmentConstraintOutcome,
    ) -> InlineFragmentBreakSelection {
        let end_index = start_index
            .saturating_add(record_count)
            .min(self.records.len());
        debug_assert!(start_index < end_index);
        let in_flow_line_count = self.records[start_index..end_index]
            .iter()
            .filter(|record| record.participates_in_widows_orphans())
            .count();
        InlineFragmentBreakSelection::Selected(InlineLineFragmentPlan {
            range: InlineLineRecordRange::new(start_index, end_index, self.records.len()),
            in_flow_line_count,
            constraint_outcome,
            block_extent: MulticolumnColumnFragmentBlockExtent::new(layout_pt(
                self.fragment_height(start_index, record_count),
            )),
        })
    }

    /// Include trailing float-only records after the selected in-flow line.
    ///
    /// Their paint/placement belongs to this fragmentainer, but they are not
    /// line boxes for CSS Fragmentation's `orphans` and `widows` constraints.
    fn record_count_through_in_flow_lines(
        &self,
        start_index: usize,
        maximum_count: usize,
        requested_line_count: usize,
    ) -> usize {
        let end_index = start_index
            .saturating_add(maximum_count)
            .min(self.records.len());
        let mut count = 0;
        let mut line_count = 0;
        for record in &self.records[start_index..end_index] {
            count += 1;
            if record.participates_in_widows_orphans() {
                line_count += 1;
                if line_count == requested_line_count {
                    break;
                }
            }
        }
        while start_index + count < end_index
            && !self.records[start_index + count].participates_in_widows_orphans()
        {
            count += 1;
        }
        count
    }
}

/// The result of choosing a class-B break for an inline formatting context.
///
/// `KeepTogether` is deliberately distinct from zero lines: balancing needs
/// to reject that column height, while actual layout may advance to another
/// fragmentainer before it is forced to relax the constraint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum InlineFragmentBreakSelection {
    NoLines,
    NoRoom,
    KeepTogether,
    Selected(InlineLineFragmentPlan),
}

impl InlineFragmentBreakSelection {
    pub(in crate::layout) fn line_count(self) -> Option<usize> {
        match self {
            Self::Selected(fragment) => Some(fragment.range.record_count()),
            Self::NoLines | Self::NoRoom | Self::KeepTogether => None,
        }
    }

    fn selected_fragment(self) -> Option<InlineLineFragmentPlan> {
        match self {
            Self::Selected(fragment) => Some(fragment),
            Self::NoLines | Self::NoRoom | Self::KeepTogether => None,
        }
    }

    fn is_no_room(self) -> bool {
        self == Self::NoRoom
    }
}

/// One graph-selected inline line plus paragraph-local fragmentation metadata.
///
/// Empty forced-break lines are represented with `fragment: None` so CSS Text
/// preserved/forced line breaks contribute to page fitting even when they paint
/// no glyphs:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>.
#[derive(Debug, Clone)]
// Line index metadata is written today and asserted in tests; it is retained
// for page fragmentation and widows/orphans work even though current painting
// does not read every field.
#[allow(dead_code)]
pub(in crate::layout) struct InlineLineRecord {
    pub(in crate::layout) paragraph_index: usize,
    pub(in crate::layout) block_line_index: usize,
    pub(in crate::layout) paragraph_line_index: usize,
    pub(in crate::layout) fragment: Option<InlineLineFragment>,
    pub(in crate::layout) is_phantom: bool,
    pub(in crate::layout) is_first_formatted_line: bool,
    pub(in crate::layout) is_last_line_in_paragraph: bool,
    pub(in crate::layout) is_forced_empty: bool,
    /// Source-sensitive CSS Text Phase II line-start state.
    pub(in crate::layout) starts_after_preserved_segment_break: bool,
    pub(in crate::layout) clear_after: Clear,
    /// Physical block-axis distance inserted before this line's line box.
    ///
    /// This models float retries that land at an analytic shape boundary
    /// between ordinary line-height rows.
    pub(in crate::layout) block_before: f32,
    pub(in crate::layout) block_start_trim: f32,
    pub(in crate::layout) block_end_trim: f32,
    pub(in crate::layout) paragraph_last_hanging_width: f32,
    pub(in crate::layout) used_indent: f32,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) line_height: f32,
    /// Decorating-box fragment geometry populated from the enclosing selected
    /// line sequence immediately before paint.  This keeps percentage bases
    /// independent of shaped text receiver spans.
    pub(in crate::layout) decoration_origin_fragments:
        Rc<[crate::layout::text_paint::TextDecorationOriginFragmentGeometry]>,
}

impl InlineLineRecord {
    pub(in crate::layout) fn block_advance(&self) -> f32 {
        self.block_before + self.height()
    }

    pub(in crate::layout) fn height(&self) -> f32 {
        if self.is_phantom
            && !self
                .fragment
                .as_ref()
                .is_some_and(inline_line_fragment_preserves_line_height)
        {
            return 0.0;
        }
        (self.line_height - self.block_start_trim - self.block_end_trim).max(0.0)
    }

    fn has_inline_layout_effects(&self) -> bool {
        self.fragment
            .as_ref()
            .is_some_and(inline_line_fragment_has_layout_effects)
    }

    /// Whether this otherwise phantom record must be prepared so an inline
    /// edge can replay escaped absolutely positioned descendants.
    ///
    /// This is deliberately separate from `has_inline_layout_effects`: the
    /// descendants are out of flow, so their replay must not give the source
    /// line a block advance, fragmentation, clamp, or baseline effect.
    fn has_positioned_descendant_replay(&self) -> bool {
        self.fragment.as_ref().is_some_and(|fragment| {
            fragment.items().iter().any(|item| {
                matches!(
                    &item.item,
                    InlineLineItem::Atom(atom)
                        if atom
                            .escaped_positioned_layers()
                            .is_some_and(|layers| !layers.is_empty())
                )
            })
        })
    }

    pub(in crate::layout) fn participates_in_widows_orphans(&self) -> bool {
        // CSS Fragmentation counts the line boxes generated by in-flow
        // content.  A transparent record can still carry source-order paint
        // metadata (for example, a collapsed space between two zero-width
        // inline scope edges), but CSS Inline does not let it manufacture a
        // line box.  Preserved whitespace and forced empty lines are marked
        // non-phantom when records are collected, so they continue to count.
        // <https://drafts.csswg.org/css-inline/#invisible-line-boxes>
        // <https://www.w3.org/TR/css-break-3/#widows-orphans>
        !self.is_phantom
    }

    /// Whether this line carries an atomic formatting context whose paint was
    /// already isolated before line placement, such as an `inline-table`.
    ///
    /// CSS Display makes these atomic inline-level boxes indivisible in the
    /// inline formatting context. Their containing line can overflow a page,
    /// but the generic oversized-line continuation path must not split the
    /// atom's precomposed paint fragment across page canvases.
    /// <https://drafts.csswg.org/css-display-3/#atomic-inline>
    fn has_isolated_atomic_inline_fragment(&self) -> bool {
        self.fragment.as_ref().is_some_and(|fragment| {
            fragment.items().iter().any(|item| {
                matches!(
                    item.item,
                    InlineLineItem::Atom(ref atom)
                        if matches!(atom.content(), InlineAtomContent::InlineFragment { .. })
                )
            })
        })
    }
}

/// Return whether a selected line box is phantom for margin collapse.
///
/// CSS Inline ignores a line box for CSS 2 margin collapse if it contains no
/// text, no preserved whitespace, no in-flow content, and no inline box with
/// non-zero inline-axis margin, padding, or border:
/// <https://drafts.csswg.org/css-inline/#invisible-line-boxes> and
/// <https://drafts.csswg.org/css2/#inline-formatting>.
pub(in crate::layout) fn inline_line_fragment_is_phantom(fragment: &InlineLineFragment) -> bool {
    if fragment.items().is_empty() {
        return fragment.text().is_empty();
    }
    fragment.items().iter().all(measured_inline_item_is_phantom)
}

/// Whether a selected line supplies actual principal inline content.
///
/// Box-edge bookkeeping can retain line metrics around a float without
/// creating a principal line to which an outside marker may attach. Text and
/// non-phantom atomic inline content, on the other hand, are eligible.
/// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
fn inline_line_fragment_has_principal_content(fragment: &InlineLineFragment) -> bool {
    fragment.items().iter().any(|item| match &item.item {
        InlineLineItem::Fragment(fragment) => !inline_fragment_is_phantom(fragment),
        InlineLineItem::Atom(atom) => {
            !matches!(atom.content(), InlineAtomContent::InlineEdge(_))
                && !inline_atom_is_phantom(atom)
        }
        InlineLineItem::Float(_) => false,
    })
}

fn inline_line_fragment_preserves_line_height(fragment: &InlineLineFragment) -> bool {
    fragment.items().iter().any(|item| match &item.item {
        // A zero-width, unpainted edge from an empty inline is transparent
        // when deciding whether an otherwise invisible line box has height.
        // Its inherited line-height alone must not manufacture a line box;
        // only a real inline-axis box-model edge can do that.
        // <https://drafts.csswg.org/css-inline/#invisible-line-boxes>
        InlineLineItem::Atom(atom) => {
            matches!(
                atom.content(),
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
            ) && !inline_atom_is_phantom(atom)
        }
        InlineLineItem::Fragment(fragment) => !inline_fragment_is_phantom(fragment),
        InlineLineItem::Float(_) => false,
    })
}

fn inline_line_fragment_has_layout_effects(fragment: &InlineLineFragment) -> bool {
    fragment
        .items()
        .iter()
        .any(|item| matches!(item.item, InlineLineItem::Float(_)))
}

fn measured_inline_item_is_phantom(item: &MeasuredInlineItem) -> bool {
    match &item.item {
        InlineLineItem::Fragment(fragment) => inline_fragment_is_phantom(fragment),
        InlineLineItem::Atom(atom) => inline_atom_is_phantom(atom),
        InlineLineItem::Float(_) => true,
    }
}

fn inline_fragment_is_phantom(fragment: &InlineFragment) -> bool {
    fragment.text().is_empty()
        || (fragment.style().white_space.collapses_spaces()
            && fragment.text().chars().all(is_css_collapsible_whitespace))
}

fn inline_atom_is_phantom(atom: &InlineAtom) -> bool {
    match atom.content() {
        InlineAtomContent::StaticPositionPlaceholder => true,
        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) => {
            edge.advance.abs() <= 0.001 && edge.paint_extent <= 0.001
        }
        InlineAtomContent::InlineEdge(_) => false,
        InlineAtomContent::Leader(_)
        | InlineAtomContent::Canvas
        | InlineAtomContent::Iframe(_)
        | InlineAtomContent::Image(_)
        | InlineAtomContent::Gradient { .. }
        | InlineAtomContent::Svg { .. }
        | InlineAtomContent::InlineBox { .. }
        | InlineAtomContent::Ruby { .. }
        | InlineAtomContent::TextCombineUpright { .. }
        | InlineAtomContent::InlineFragment { .. } => false,
    }
}

fn inline_items_can_fragment_as_collected_lines(items: &[InlineItem]) -> bool {
    items
        .iter()
        .any(|item| matches!(item, InlineItem::Break(_)))
        && items
            .iter()
            .all(|item| inline_break_clear(item) == Clear::None)
        && items.iter().all(|item| {
            !matches!(
                item,
                InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd
            )
        })
}

fn inline_items_have_page_scope(items: &[InlineItem]) -> bool {
    items.iter().any(|item| {
        matches!(
            item,
            InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd
        )
    })
}

/// Update the lexical inline-scope stack used to bridge separately collected
/// forced-break paragraphs. Only `clone` scopes materialize continuation
/// chrome, but every source edge occupies a stack slot so nesting remains
/// paired with the matching end edge.
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
fn update_inline_fragment_continuation_scopes(
    scopes: &mut Vec<Option<InlineFragmentContinuation>>,
    item: &InlineItem,
) {
    let InlineItem::Atom(atom) = item else {
        return;
    };
    let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content() else {
        return;
    };
    // Positioned bidi-isolate markers are lexical containing-block metadata
    // inside their isolate controls, not cloneable decoration boundaries.
    if edge.is_positioning_marker() {
        return;
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

/// Source collection marks the DOM scope's outermost visible words. A forced
/// break creates an additional fragment-local scope, so its synthetic edges
/// must mark the selected paragraph's first and last visible words as owning
/// the matching cloned inline sides.
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
fn mark_first_visible_inline_word_clone_start(items: &mut [InlineItem]) -> bool {
    let Some(word) = items.iter_mut().find_map(visible_hanging_edge_word_mut) else {
        return false;
    };
    word.hanging_edges.blocks_start = true;
    true
}

fn mark_last_visible_inline_word_clone_end(items: &mut [InlineItem]) {
    if let Some(word) = items
        .iter_mut()
        .rev()
        .find_map(visible_hanging_edge_word_mut)
    {
        word.hanging_edges.blocks_end = true;
    }
}

fn inline_break_clear(item: &InlineItem) -> Clear {
    match item {
        InlineItem::Break(break_) => break_.clear,
        _ => Clear::None,
    }
}

fn trim_visual_line_end_collapsible_spaces(
    items: &mut Vec<MeasuredInlineItem>,
    direction: Direction,
) {
    // CSS Text Phase II trims collapsed whitespace at the visual line end
    // through inline box edges.  Retain those edges for their decoration
    // ownership while removing qualifying text immediately, so index shifts
    // cannot invalidate a separately collected removal list.
    match direction {
        Direction::Ltr => {
            let mut index = items.len();
            while let Some(previous) = index.checked_sub(1) {
                match &items[previous].item {
                    InlineLineItem::Atom(atom)
                        if matches!(
                            atom.content(),
                            InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                        ) =>
                    {
                        index = previous;
                    }
                    InlineLineItem::Fragment(fragment)
                        if inline_fragment_is_collapsible_space(fragment) =>
                    {
                        items.remove(previous);
                        index = previous;
                    }
                    _ => break,
                }
            }
        }
        Direction::Rtl => {
            let mut index = 0;
            while let Some(item) = items.get(index) {
                match &item.item {
                    InlineLineItem::Atom(atom)
                        if matches!(
                            atom.content(),
                            InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                        ) =>
                    {
                        index += 1;
                    }
                    InlineLineItem::Fragment(fragment)
                        if inline_fragment_is_collapsible_space(fragment) =>
                    {
                        items.remove(index);
                    }
                    _ => break,
                }
            }
        }
    }
}

#[cfg(test)]
mod text_combine_upright_tests {
    use super::*;
    use std::rc::Rc;

    fn vertical_style(value: css::TextCombineUpright) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        style.text_combine_upright = value;
        style
    }

    fn word(text: &str, style: &ComputedStyle) -> InlineWord {
        InlineWord {
            text: text.to_owned(),
            style: inline_style(style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        }
    }

    fn inline_box_edge(style: &ComputedStyle) -> InlineItem {
        InlineItem::Atom(Box::new(InlineAtom::new(
            InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                logical_edge: InlineLogicalEdge::End,
                physical_side: PhysicalSide::Right,
                positioning_containing_block_id: None,
                advance: 0.0,
                paint_extent: 0.0,
            })),
            style.clone(),
            None,
            InlineSize::new(0.0, style.line_height),
            0.0,
            0.0,
            None,
            None,
        )))
    }

    #[test]
    fn text_combine_reverses_full_width_ascii_only_for_multi_character_runs() {
        assert_eq!(reverse_full_width_transform_for_text_combine("０"), "０");
        assert_eq!(reverse_full_width_transform_for_text_combine("００"), "00");
        assert_eq!(reverse_full_width_transform_for_text_combine("ＡＢ"), "AB");
    }

    #[test]
    fn all_forms_one_trimmed_atomic_run_after_whitespace_processing() {
        let style = vertical_style(css::TextCombineUpright::All);
        assert_eq!(
            text_combine_upright_text("  5 6  ", &style),
            Some("5 6".into())
        );
    }

    #[test]
    fn all_preserves_preformatted_edge_white_space_for_the_nested_line() {
        let mut style = vertical_style(css::TextCombineUpright::All);
        style.white_space = css::WhiteSpace::Pre;
        assert_eq!(
            text_combine_upright_text("  5 6  ", &style),
            Some("  5 6  ".into())
        );
    }

    #[test]
    fn digits_only_forms_a_run_within_its_selected_limit() {
        let style = vertical_style(css::TextCombineUpright::Digits(2));
        assert_eq!(text_combine_upright_text("12", &style), Some("12".into()));
        assert_eq!(text_combine_upright_text("123", &style), None);
        assert_eq!(text_combine_upright_text("1A", &style), None);
    }

    #[test]
    fn horizontal_typographic_mode_does_not_form_text_combine_atoms() {
        let mut style = vertical_style(css::TextCombineUpright::All);
        style.writing_mode = WritingMode::HorizontalTb;
        assert_eq!(text_combine_upright_text("12", &style), None);
    }

    #[test]
    fn scoped_run_accepts_normalized_words_but_not_link_or_bidi_boundaries() {
        let style = vertical_style(css::TextCombineUpright::All);
        let first = word("1", &style);
        let second = word("2", &style);
        assert!(text_combine_upright_words_are_compatible(&first, &second));

        let mut linked = word("3", &style);
        linked.link_target = Some(Rc::from("https://example.invalid/"));
        assert!(!text_combine_upright_words_are_compatible(&first, &linked));
        assert!(text_combine_upright_text_has_bidi_controls("\u{2068}"));
        assert!(!text_combine_upright_text_has_bidi_controls("12"));
    }

    #[test]
    fn tcy_preformatted_space_run_ignores_hanging_edges_but_not_box_edges() {
        let mut style = vertical_style(css::TextCombineUpright::All);
        style.white_space = css::WhiteSpace::Pre;

        let leading_space = word("  ", &style);
        let mut digits = word("12", &style);
        digits.hanging_edges = InlineHangingEdges {
            blocks_start: true,
            blocks_end: true,
        };
        let trailing_space = word("  ", &style);

        assert!(text_combine_upright_words_are_compatible(
            &leading_space,
            &digits
        ));
        let continuous_items = vec![
            InlineItem::Word(Box::new(leading_space.clone())),
            InlineItem::Word(Box::new(digits.clone())),
            InlineItem::Word(Box::new(trailing_space)),
        ];
        assert_eq!(
            text_combine_upright_contiguous_word_end(&continuous_items, 0),
            3
        );

        let separated_items = vec![
            InlineItem::Word(Box::new(leading_space)),
            inline_box_edge(&style),
            InlineItem::Word(Box::new(digits)),
        ];
        assert_eq!(
            text_combine_upright_contiguous_word_end(&separated_items, 0),
            1
        );
    }
}
