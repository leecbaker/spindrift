use super::*;

/// Push CSS Text-normalized inline words into the shared inline item stream.
///
/// CSS Text white-space processing, segment breaks, visible control handling,
/// and preserved-space tokenization must be identical for normal inline
/// content, generated content, and page-margin text before all consumers build
/// an `InlineOpportunityGraph`:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
pub(in crate::layout) fn push_inline_words_for_style(
    text: &str,
    style: &ComputedStyle,
    link_target: Option<String>,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
    output: &mut Vec<InlineItem>,
) {
    let normalized_style;
    let style = if anonymous_inline_content_needs_normalized_style(style) {
        normalized_style = normalized_anonymous_inline_content_style(style);
        &normalized_style
    } else {
        style
    };
    push_inline_text_run(
        text,
        style,
        link_target,
        baseline_shift,
        visual_offset,
        output,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn word_styles(items: &[InlineItem]) -> Vec<InlineStyle> {
        items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.style.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn whitespace_normalization_reuses_inline_word_style_handles() {
        let style = ComputedStyle::initial();
        let mut items = Vec::new();

        push_inline_text_run(
            "A\tB",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        normalize_inline_whitespace_items(&mut items);

        let styles = word_styles(&items);
        assert_eq!(styles.len(), 3);
        assert!(Rc::ptr_eq(&styles[0], &styles[1]));
        assert!(Rc::ptr_eq(&styles[0], &styles[2]));
    }

    #[test]
    fn inline_word_style_mutation_is_copy_on_write() {
        let mut style = ComputedStyle::initial();
        style.font_size = 12.0;
        let shared_style = inline_style(&style);
        let mut first = InlineWord {
            text: "A".to_string(),
            style: shared_style.clone(),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        };
        let second = InlineWord {
            text: "B".to_string(),
            style: shared_style,
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        };

        Rc::make_mut(&mut first.style).font_size = 20.0;

        assert_eq!(first.style.font_size, 20.0);
        assert_eq!(second.style.font_size, 12.0);
        assert!(!Rc::ptr_eq(&first.style, &second.style));
    }
}

pub(in crate::layout) fn push_inline_text_run(
    text: &str,
    style: &ComputedStyle,
    link_target: Option<String>,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
    output: &mut Vec<InlineItem>,
) {
    push_inline_text_run_with_source(
        text,
        style,
        link_target,
        baseline_shift,
        visual_offset,
        InlineTextSource::Normal,
        output,
    );
}

pub(in crate::layout) fn push_generated_inline_words_for_style(
    text: &str,
    style: &ComputedStyle,
    link_target: Option<String>,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
    output: &mut Vec<InlineItem>,
) {
    let normalized_style;
    let style = if anonymous_inline_content_needs_normalized_style(style) {
        normalized_style = normalized_anonymous_inline_content_style(style);
        &normalized_style
    } else {
        style
    };
    push_inline_text_run_with_source(
        text,
        style,
        link_target,
        baseline_shift,
        visual_offset,
        InlineTextSource::Generated,
        output,
    );
}

pub(in crate::layout) fn push_inline_text_run_with_source(
    text: &str,
    style: &ComputedStyle,
    link_target: Option<String>,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
    source: InlineTextSource,
    output: &mut Vec<InlineItem>,
) {
    if !text.is_empty() {
        output.push(InlineItem::Word(Box::new(InlineWord {
            text: text.to_string(),
            style: inline_style(style),
            baseline_shift,
            visual_offset,
            link_target,
            mergeable: true,
            source,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        })));
    }
}

/// Normalize one collected inline paragraph with CSS Text whitespace phases.
///
/// Inline collection preserves source text, generated content, inline edges,
/// bidi controls, and atomic boxes as a single item stream. This processor runs
/// before autospace and graph construction so segment-break transformation and
/// whitespace collapse can see across text nodes and transparent inline box
/// edges:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(in crate::layout) fn normalize_inline_whitespace_items(items: &mut Vec<InlineItem>) {
    let mut processor = InlineWhitespaceProcessor::default();
    for item in std::mem::take(items) {
        processor.push_item(item);
    }
    processor.flush();
    *items = processor.output;
}

#[derive(Default)]
pub(in crate::layout) struct InlineWhitespaceProcessor {
    pub(in crate::layout) output: Vec<InlineItem>,
    pub(in crate::layout) run: String,
    pub(in crate::layout) run_meta: Option<InlineTextRunMeta>,
    pub(in crate::layout) run_is_document_space: bool,
    pub(in crate::layout) last_text_character: Option<char>,
    pub(in crate::layout) pending_segment_break: Option<InlineTextRunMeta>,
    pub(in crate::layout) pending_forced_segment_break: Option<PendingForcedSegmentBreak>,
}

#[derive(Clone)]
pub(in crate::layout) struct InlineTextRunMeta {
    pub(in crate::layout) style: InlineStyle,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
    pub(in crate::layout) link_target: Option<String>,
    pub(in crate::layout) mergeable: bool,
    pub(in crate::layout) source: InlineTextSource,
    pub(in crate::layout) hanging_edges: InlineHangingEdges,
    pub(in crate::layout) ancestor_inline_decorations: Vec<InlineAncestorDecoration>,
}

#[derive(Clone, Copy, Default)]
pub(in crate::layout) struct InlinePlacement {
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
}

impl InlinePlacement {
    pub(in crate::layout) fn new(baseline_shift: f32, visual_offset: InlineVisualOffset) -> Self {
        Self {
            baseline_shift,
            visual_offset,
        }
    }

    pub(in crate::layout) fn zero() -> Self {
        Self::default()
    }

    pub(in crate::layout) fn with_added_baseline_shift(self, baseline_shift: f32) -> Self {
        Self {
            baseline_shift: self.baseline_shift + baseline_shift,
            ..self
        }
    }

    pub(in crate::layout) fn with_added_visual_offset(
        self,
        visual_offset: InlineVisualOffset,
    ) -> Self {
        Self {
            visual_offset: self.visual_offset.plus(visual_offset),
            ..self
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::layout) struct PendingForcedSegmentBreak {
    pub(in crate::layout) preserve_at_end: bool,
    pub(in crate::layout) clear: Clear,
}

#[derive(Clone, Copy)]
pub(in crate::layout) struct IntrinsicInlineCollectionContext<'a> {
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
    pub(in crate::layout) block_style: &'a ComputedStyle,
    pub(in crate::layout) propagated_decoration: css::TextDecoration,
}

impl<'a> IntrinsicInlineCollectionContext<'a> {
    pub(in crate::layout) fn with_baseline_shift(self, baseline_shift: f32) -> Self {
        Self {
            baseline_shift,
            ..self
        }
    }

    pub(in crate::layout) fn with_visual_offset(self, visual_offset: InlineVisualOffset) -> Self {
        Self {
            visual_offset,
            ..self
        }
    }

    pub(in crate::layout) fn with_block_style(self, block_style: &'a ComputedStyle) -> Self {
        Self {
            block_style,
            ..self
        }
    }

    pub(in crate::layout) fn with_propagated_decoration(
        self,
        propagated_decoration: css::TextDecoration,
    ) -> Self {
        Self {
            propagated_decoration,
            ..self
        }
    }
}

impl InlineWhitespaceProcessor {
    pub(in crate::layout) fn push_item(&mut self, item: InlineItem) {
        let role = inline_item_boundary_role(&item);
        match role {
            InlineBoundaryRole::Text => {
                let InlineItem::Word(word) = item else {
                    unreachable!("text boundary role must come from a word")
                };
                self.push_word(*word);
            }
            InlineBoundaryRole::TransparentTextBoundary
            | InlineBoundaryRole::PageScopeStart
            | InlineBoundaryRole::PageScopeEnd => {
                debug_assert!(role.is_transparent_to_whitespace());
                self.flush_run();
                self.output.push(item);
            }
            InlineBoundaryRole::OpaqueAtomic
            | InlineBoundaryRole::IndependentFormattingContext
            | InlineBoundaryRole::Float => {
                self.resolve_pending_before_boundary();
                self.flush_run();
                self.output.push(item);
                if role.resets_text_context() {
                    self.reset_text_context();
                }
            }
            InlineBoundaryRole::ForcedBreak => {
                let clear = match item {
                    InlineItem::Break(break_) => break_.clear,
                    _ => Clear::None,
                };
                self.discard_pending_segment_breaks();
                self.emit_forced_break_with_clear(clear);
            }
        }
    }

    pub(in crate::layout) fn push_word(&mut self, word: InlineWord) {
        let meta = InlineTextRunMeta {
            style: word.style,
            baseline_shift: word.baseline_shift,
            visual_offset: word.visual_offset,
            link_target: word.link_target,
            mergeable: word.mergeable,
            source: word.source,
            hanging_edges: word.hanging_edges,
            ancestor_inline_decorations: word.ancestor_inline_decorations,
        };
        let text = dom::decode_entities_public(&word.text);
        let mut chars = text.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                self.push_segment_break(&meta, false);
            } else if character == '\n' || character == '\u{000c}' {
                self.push_segment_break(&meta, false);
            } else if character == INLINE_BREAK {
                self.push_segment_break(&meta, true);
            } else if meta.style.white_space.collapses_spaces() && matches!(character, ' ' | '\t') {
                self.push_collapsible_space(&meta);
            } else {
                self.push_text_character(character, &meta);
            }
        }
    }

    pub(in crate::layout) fn push_segment_break(&mut self, meta: &InlineTextRunMeta, forced: bool) {
        if forced || meta.style.white_space.preserves_newlines() {
            if self.pending_forced_segment_break.is_some() {
                self.emit_forced_break();
            }
            self.flush_run();
            self.pending_segment_break = None;
            self.pending_forced_segment_break = Some(PendingForcedSegmentBreak {
                preserve_at_end: forced
                    || meta.source == InlineTextSource::Generated
                    || meta.style.white_space == WhiteSpace::Pre,
                clear: if meta.source == InlineTextSource::Generated {
                    meta.style.clear
                } else {
                    Clear::None
                },
            });
        } else if meta.style.white_space.collapses_spaces() {
            self.flush_run();
            self.pending_segment_break = Some(meta.clone());
        } else {
            self.push_text_character('\n', meta);
        }
    }

    pub(in crate::layout) fn push_collapsible_space(&mut self, meta: &InlineTextRunMeta) {
        if self.pending_forced_segment_break.is_some() || self.pending_segment_break.is_some() {
            return;
        }
        self.flush_run();
        if self.output_ends_at_space_or_line_start() {
            return;
        }
        self.push_word_run(" ", meta);
        self.last_text_character = Some(' ');
    }

    pub(in crate::layout) fn push_text_character(
        &mut self,
        character: char,
        meta: &InlineTextRunMeta,
    ) {
        self.resolve_pending_before_character(character);
        if meta.style.white_space == WhiteSpace::BreakSpaces {
            self.flush_run();
            let mut buffer = [0; 4];
            self.push_word_run(character.encode_utf8(&mut buffer), meta);
        } else {
            let character_is_document_space = matches!(character, ' ' | '\t');
            if self.run.is_empty() {
                self.run_meta = Some(meta.clone());
                self.run_is_document_space = character_is_document_space;
            } else if self.run_is_document_space != character_is_document_space
                || !self.run_meta_matches(meta)
            {
                self.flush_run();
                self.run_meta = Some(meta.clone());
                self.run_is_document_space = character_is_document_space;
            }
            self.run.push(character);
        }
        if !character_is_bidi_format_control(character) {
            self.last_text_character = Some(character);
        }
    }

    pub(in crate::layout) fn resolve_pending_before_character(&mut self, next: char) {
        if self.pending_forced_segment_break.is_some() {
            self.emit_forced_break();
        }
        let Some(meta) = self.pending_segment_break.take() else {
            return;
        };
        if self
            .last_text_character
            .is_some_and(character_is_autospace_ideograph)
            && character_is_autospace_ideograph(next)
        {
            return;
        }
        self.push_collapsible_space(&meta);
    }

    pub(in crate::layout) fn resolve_pending_before_boundary(&mut self) {
        if self.pending_forced_segment_break.is_some() {
            self.emit_forced_break();
        }
        if let Some(meta) = self.pending_segment_break.take() {
            // CSS Text segment breaks collapse across inline-level atomic
            // boundaries just as they do between text runs. Atomic inline boxes
            // reset character context for CJK autospace suppression, but they
            // still occupy the inline stream for line-edge whitespace tests:
            // <https://www.w3.org/TR/css-text-3/#white-space-processing>,
            // <https://www.w3.org/TR/css-inline-3/#atomic-inline>.
            if !self.output_ends_at_space_or_line_start() {
                self.push_collapsible_space(&meta);
            }
        }
    }

    pub(in crate::layout) fn emit_forced_break(&mut self) {
        let clear = self
            .pending_forced_segment_break
            .map(|break_| break_.clear)
            .unwrap_or(Clear::None);
        self.emit_forced_break_with_clear(clear);
    }

    pub(in crate::layout) fn emit_forced_break_with_clear(&mut self, clear: Clear) {
        self.flush_run();
        self.pending_forced_segment_break = None;
        self.pending_segment_break = None;
        trim_trailing_inline_spaces(&mut self.output);
        self.output.push(InlineItem::Break(InlineBreak { clear }));
        self.last_text_character = None;
    }

    pub(in crate::layout) fn flush(&mut self) {
        self.flush_run();
        if self
            .pending_forced_segment_break
            .is_some_and(|break_| break_.preserve_at_end)
        {
            self.emit_forced_break();
        } else {
            self.discard_pending_segment_breaks();
        }
    }

    pub(in crate::layout) fn flush_run(&mut self) {
        if self.run.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.run);
        let meta = self
            .run_meta
            .take()
            .expect("non-empty inline text run must carry metadata");
        self.push_word_run(&text, &meta);
    }

    pub(in crate::layout) fn push_word_run(&mut self, text: &str, meta: &InlineTextRunMeta) {
        if text.is_empty() {
            return;
        }
        let text = text_with_visible_control_characters(text);
        self.output.push(InlineItem::Word(Box::new(InlineWord {
            text,
            style: meta.style.clone(),
            baseline_shift: meta.baseline_shift,
            visual_offset: meta.visual_offset,
            link_target: meta.link_target.clone(),
            mergeable: meta.mergeable,
            source: meta.source,
            hanging_edges: meta.hanging_edges,
            ancestor_inline_decorations: meta.ancestor_inline_decorations.clone(),
        })));
    }

    pub(in crate::layout) fn run_meta_matches(&self, meta: &InlineTextRunMeta) -> bool {
        self.run_meta.as_ref().is_some_and(|current| {
            current.style.as_ref() == meta.style.as_ref()
                && current.baseline_shift == meta.baseline_shift
                && current.link_target == meta.link_target
                && current.mergeable == meta.mergeable
                && current.source == meta.source
                && current.hanging_edges == meta.hanging_edges
                && current.ancestor_inline_decorations == meta.ancestor_inline_decorations
        })
    }

    pub(in crate::layout) fn discard_pending_segment_breaks(&mut self) {
        self.pending_segment_break = None;
        self.pending_forced_segment_break = None;
    }

    pub(in crate::layout) fn reset_text_context(&mut self) {
        self.last_text_character = None;
        self.discard_pending_segment_breaks();
    }

    pub(in crate::layout) fn output_ends_at_space_or_line_start(&self) -> bool {
        for item in self.output.iter().rev() {
            match item {
                InlineItem::Atom(atom) if atom.content().is_box_edge() => {}
                InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => {}
                // Floats are out of flow in CSS 2.2, so they must not block
                // CSS Text collapsible whitespace from looking through the
                // zero-width marker left in the inline stream:
                // <https://www.w3.org/TR/css-text-3/#white-space-processing>
                // and <https://www.w3.org/TR/CSS22/visuren.html#float-position>.
                InlineItem::Float(_) => {}
                InlineItem::Word(_) => return inline_item_is_collapsible_space(item),
                InlineItem::Break(_) => return true,
                InlineItem::Atom(_) => return false,
            }
        }
        true
    }
}
