use super::*;
use crate::text::{
    character_is_autospace_alpha, character_is_autospace_ideograph, character_is_autospace_numeric,
    trim_css_collapsible_whitespace,
};

/// Push CSS Text-normalized inline words into the shared inline item stream.
///
/// CSS Text white-space processing, segment breaks, visible control handling,
/// and preserved-space tokenization must be identical for normal inline
/// content, generated content, and page-margin text before all consumers build
/// an `InlineOpportunityGraph`:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
pub(super) fn push_inline_words_for_style(
    text: &str,
    style: &ComputedStyle,
    link_target: Option<String>,
    baseline_shift: f32,
    output: &mut Vec<InlineItem>,
) {
    let normalized_style;
    let style = if anonymous_inline_content_needs_normalized_style(style) {
        normalized_style = normalized_anonymous_inline_content_style(style);
        &normalized_style
    } else {
        style
    };
    push_inline_text_run(text, style, link_target, baseline_shift, output);
}

fn push_inline_text_run(
    text: &str,
    style: &ComputedStyle,
    link_target: Option<String>,
    baseline_shift: f32,
    output: &mut Vec<InlineItem>,
) {
    if !text.is_empty() {
        output.push(InlineItem::Word(Box::new(InlineWord {
            text: text.to_string(),
            style: style.clone(),
            baseline_shift,
            link_target: link_target.clone(),
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
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
struct InlineWhitespaceProcessor {
    output: Vec<InlineItem>,
    run: String,
    run_meta: Option<InlineTextRunMeta>,
    run_is_document_space: bool,
    last_text_character: Option<char>,
    pending_segment_break: Option<InlineTextRunMeta>,
    pending_forced_segment_break: bool,
}

#[derive(Clone)]
struct InlineTextRunMeta {
    style: ComputedStyle,
    baseline_shift: f32,
    link_target: Option<String>,
    mergeable: bool,
    source: InlineTextSource,
    hanging_edges: InlineHangingEdges,
}

#[derive(Clone, Copy)]
struct IntrinsicInlineCollectionContext<'a> {
    baseline_shift: f32,
    block_style: &'a ComputedStyle,
    propagated_decoration: css::TextDecoration,
}

impl<'a> IntrinsicInlineCollectionContext<'a> {
    fn with_baseline_shift(self, baseline_shift: f32) -> Self {
        Self {
            baseline_shift,
            ..self
        }
    }

    fn with_block_style(self, block_style: &'a ComputedStyle) -> Self {
        Self {
            block_style,
            ..self
        }
    }

    fn with_propagated_decoration(self, propagated_decoration: css::TextDecoration) -> Self {
        Self {
            propagated_decoration,
            ..self
        }
    }
}

impl InlineWhitespaceProcessor {
    fn push_item(&mut self, item: InlineItem) {
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
                self.discard_pending_segment_breaks();
                self.emit_forced_break();
            }
        }
    }

    fn push_word(&mut self, word: InlineWord) {
        let meta = InlineTextRunMeta {
            style: word.style,
            baseline_shift: word.baseline_shift,
            link_target: word.link_target,
            mergeable: word.mergeable,
            source: word.source,
            hanging_edges: word.hanging_edges,
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

    fn push_segment_break(&mut self, meta: &InlineTextRunMeta, forced: bool) {
        if forced || meta.style.white_space.preserves_newlines() {
            if self.pending_forced_segment_break {
                self.emit_forced_break();
            }
            self.flush_run();
            self.pending_segment_break = None;
            self.pending_forced_segment_break = true;
        } else if meta.style.white_space.collapses_spaces() {
            self.flush_run();
            self.pending_segment_break = Some(meta.clone());
        } else {
            self.push_text_character('\n', meta);
        }
    }

    fn push_collapsible_space(&mut self, meta: &InlineTextRunMeta) {
        if self.pending_forced_segment_break || self.pending_segment_break.is_some() {
            return;
        }
        self.flush_run();
        if self.output_ends_at_space_or_line_start() {
            return;
        }
        self.push_word_run(" ", meta);
        self.last_text_character = Some(' ');
    }

    fn push_text_character(&mut self, character: char, meta: &InlineTextRunMeta) {
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

    fn resolve_pending_before_character(&mut self, next: char) {
        if self.pending_forced_segment_break {
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

    fn resolve_pending_before_boundary(&mut self) {
        if self.pending_forced_segment_break {
            self.emit_forced_break();
        }
        if let Some(meta) = self.pending_segment_break.take()
            && self.last_text_character.is_some()
        {
            self.push_collapsible_space(&meta);
        }
    }

    fn emit_forced_break(&mut self) {
        self.flush_run();
        self.pending_forced_segment_break = false;
        self.pending_segment_break = None;
        trim_trailing_inline_spaces(&mut self.output);
        self.output.push(InlineItem::Break);
        self.last_text_character = None;
    }

    fn flush(&mut self) {
        self.flush_run();
        self.discard_pending_segment_breaks();
    }

    fn flush_run(&mut self) {
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

    fn push_word_run(&mut self, text: &str, meta: &InlineTextRunMeta) {
        if text.is_empty() {
            return;
        }
        let text = text_with_visible_control_characters(text);
        self.output.push(InlineItem::Word(Box::new(InlineWord {
            text,
            style: meta.style.clone(),
            baseline_shift: meta.baseline_shift,
            link_target: meta.link_target.clone(),
            mergeable: meta.mergeable,
            source: meta.source,
            hanging_edges: meta.hanging_edges,
        })));
    }

    fn run_meta_matches(&self, meta: &InlineTextRunMeta) -> bool {
        self.run_meta.as_ref().is_some_and(|current| {
            current.style == meta.style
                && current.baseline_shift == meta.baseline_shift
                && current.link_target == meta.link_target
                && current.mergeable == meta.mergeable
                && current.source == meta.source
                && current.hanging_edges == meta.hanging_edges
        })
    }

    fn discard_pending_segment_breaks(&mut self) {
        self.pending_segment_break = None;
        self.pending_forced_segment_break = false;
    }

    fn reset_text_context(&mut self) {
        self.last_text_character = None;
        self.discard_pending_segment_breaks();
    }

    fn output_ends_at_space_or_line_start(&self) -> bool {
        for item in self.output.iter().rev() {
            match item {
                InlineItem::Atom(atom) if atom.content.is_box_edge() => {}
                InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => {}
                InlineItem::Word(_) => return inline_item_is_collapsible_space(item),
                InlineItem::Break => return true,
                InlineItem::Atom(_) | InlineItem::Float(_) => return false,
            }
        }
        true
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn intrinsic_inline_contribution_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> inline_layout::InlineIntrinsicContribution {
        self.intrinsic_inline_measurement_for_element(
            element,
            style,
            stylesheets,
            child_boxes,
            f32::MAX,
        )
        .contribution
    }

    pub(super) fn intrinsic_inline_contribution_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> inline_layout::InlineIntrinsicContribution {
        self.intrinsic_inline_measurement_for_boxes(children, style, stylesheets, f32::MAX)
            .contribution
    }

    /// Measure inline content through durable graph-selected fragments.
    ///
    /// CSS Sizing derives intrinsic inline contributions from CSS Text break
    /// opportunities, and CSS Flexbox consumes the same content as selected
    /// line fragments for hypothetical cross sizes:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic>,
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>,
    /// <https://www.w3.org/TR/css-inline-3/#line-box>, and
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>.
    pub(in crate::layout) fn intrinsic_inline_measurement_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let items =
            self.intrinsic_inline_items_for_element(element, style, stylesheets, child_boxes);
        self.intrinsic_inline_measurement_for_items(items, style, available_width)
    }

    pub(in crate::layout) fn intrinsic_inline_measurement_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let items = self.intrinsic_inline_items_for_boxes(children, style, stylesheets);
        self.intrinsic_inline_measurement_for_items(items, style, available_width)
    }

    pub(in crate::layout) fn intrinsic_inline_measurement_for_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let mut items = Vec::new();
        self.push_inline_words(text, style, None, 0.0, &mut items);
        self.intrinsic_inline_measurement_for_items(items, style, available_width)
    }

    fn intrinsic_inline_items_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> Vec<InlineItem> {
        let mut items = Vec::new();
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(style, None, 0.0, &mut items);
        }
        if child_boxes.is_none() {
            self.push_generated_pseudo_items(
                element,
                style.before_style.as_deref(),
                None,
                0.0,
                GeneratedPseudoCounterMode::Rollback,
                &mut items,
            );
        }
        if let Some(child_boxes) = child_boxes {
            if style.content.is_generated() {
                self.push_intrinsic_element_content_items_from_boxes(
                    element,
                    style,
                    child_boxes,
                    stylesheets,
                    None,
                    0.0,
                    style.text_decoration,
                    &mut items,
                );
            } else {
                self.collect_intrinsic_inline_box_items(
                    child_boxes,
                    stylesheets,
                    None,
                    IntrinsicInlineCollectionContext {
                        baseline_shift: 0.0,
                        block_style: style,
                        propagated_decoration: style.text_decoration,
                    },
                    &mut items,
                );
            }
        } else {
            self.collect_intrinsic_element_content_or_inline_items(
                element,
                style,
                stylesheets,
                None,
                0.0,
                &mut items,
            );
        }
        if child_boxes.is_none() {
            self.push_generated_pseudo_items(
                element,
                style.after_style.as_deref(),
                None,
                0.0,
                GeneratedPseudoCounterMode::Rollback,
                &mut items,
            );
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(style, None, 0.0, &mut items);
        }
        items
    }

    fn intrinsic_inline_items_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) -> Vec<InlineItem> {
        let mut items = Vec::new();
        self.collect_intrinsic_inline_box_items(
            children,
            stylesheets,
            None,
            IntrinsicInlineCollectionContext {
                baseline_shift: 0.0,
                block_style: style,
                propagated_decoration: style.text_decoration,
            },
            &mut items,
        );
        items
    }

    fn intrinsic_inline_measurement_for_items(
        &mut self,
        mut items: Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        normalize_inline_whitespace_items(&mut items);
        insert_text_autospace_items(&mut items);
        trim_inline_item_edges(&mut items);
        let context = InlineParagraphContext {
            block_style,
            stylesheets: &[],
            available_width: available_width.max(1.0),
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
        };
        let mut output = inline_layout::InlineIntrinsicMeasurement::default();
        output.sequence.available_width = context.available_width;
        let mut paragraph = Vec::new();
        let mut starts_after_forced_break = false;
        for item in items {
            match inline_item_boundary_role(&item) {
                InlineBoundaryRole::ForcedBreak => {
                    self.flush_intrinsic_inline_measurement_paragraph(
                        &mut paragraph,
                        context,
                        true,
                        starts_after_forced_break,
                        &mut output,
                    );
                    starts_after_forced_break = true;
                }
                role if role == InlineBoundaryRole::Float || role.is_page_scope() => {
                    let flushed = self.flush_intrinsic_inline_measurement_paragraph(
                        &mut paragraph,
                        context,
                        false,
                        starts_after_forced_break,
                        &mut output,
                    );
                    if flushed {
                        starts_after_forced_break = false;
                    }
                }
                _ => paragraph.push(item),
            }
        }
        self.flush_intrinsic_inline_measurement_paragraph(
            &mut paragraph,
            context,
            false,
            starts_after_forced_break,
            &mut output,
        );
        output
    }

    /// Estimate inline block-size from graph-selected line fragments.
    ///
    /// CSS Flexbox computes a flex item's hypothetical cross size by laying
    /// the item out as an in-flow block, while CSS Inline and CSS Text define
    /// the line boxes and break opportunities used by that block layout:
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>,
    /// <https://www.w3.org/TR/css-inline-3/#line-box>, and
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>.
    pub(in crate::layout) fn intrinsic_inline_block_metrics_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_width: f32,
    ) -> (f32, usize) {
        let measurement = self.intrinsic_inline_measurement_for_element(
            element,
            style,
            stylesheets,
            child_boxes,
            available_width,
        );
        (measurement.height(), measurement.line_count())
    }

    pub(in crate::layout) fn intrinsic_inline_block_metrics_for_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> (f32, usize) {
        let measurement = self.intrinsic_inline_measurement_for_boxes(
            children,
            style,
            stylesheets,
            available_width,
        );
        (measurement.height(), measurement.line_count())
    }

    fn flush_intrinsic_inline_measurement_paragraph(
        &mut self,
        paragraph: &mut Vec<InlineItem>,
        context: InlineParagraphContext<'_>,
        force_empty_line: bool,
        starts_after_forced_break: bool,
        output: &mut inline_layout::InlineIntrinsicMeasurement,
    ) -> bool {
        trim_inline_item_edges(paragraph);
        if paragraph.is_empty() {
            if force_empty_line {
                let line_index = output.sequence.records.len();
                output
                    .sequence
                    .records
                    .push(inline_layout::InlineLineRecord {
                        paragraph_index: output.paragraphs.len(),
                        block_line_index: line_index,
                        paragraph_line_index: 0,
                        fragment: None,
                        is_first_formatted_line: line_index == 0,
                        is_last_line_in_paragraph: true,
                        is_forced_empty: true,
                        paragraph_last_hanging_width: 0.0,
                        used_indent: used_line_indent(
                            line_index,
                            starts_after_forced_break,
                            context.hanging_indent,
                            context.block_style,
                            context.available_width,
                        ),
                        available_width: context.available_width,
                        line_height: context.block_style.line_height,
                    });
                return true;
            }
            return false;
        }
        let paragraph_index = output.paragraphs.len();
        let paragraph_start_line_index = output.sequence.records.len();
        let graph = self.build_inline_opportunity_graph(paragraph.iter(), context.block_style);
        let contribution = graph.intrinsic_contribution(&mut self.font_system, context.block_style);
        let (lines, next_line_count) = self.select_inline_lines_from_graph(
            &graph,
            context,
            paragraph_start_line_index,
            starts_after_forced_break,
        );
        output.contribution.min_content = output
            .contribution
            .min_content
            .max(contribution.min_content);
        output.contribution.max_content = output
            .contribution
            .max_content
            .max(contribution.max_content);
        let paragraph_last_hanging_width = lines
            .last()
            .map(|line| {
                last_hanging_punctuation_width_for_line_items(
                    &mut self.font_system,
                    &line.items,
                    context.block_style,
                )
            })
            .unwrap_or(0.0);
        let line_count = lines.len();
        for (offset, line) in lines.into_iter().enumerate() {
            let line_index = paragraph_start_line_index + offset;
            output
                .sequence
                .records
                .push(inline_layout::InlineLineRecord {
                    paragraph_index,
                    block_line_index: line_index,
                    paragraph_line_index: offset,
                    is_first_formatted_line: line_index == 0,
                    is_last_line_in_paragraph: offset + 1 == line_count,
                    is_forced_empty: false,
                    paragraph_last_hanging_width,
                    used_indent: line.indent,
                    available_width: line.available_width,
                    line_height: line.metrics.height.max(context.block_style.line_height),
                    fragment: Some(line),
                });
        }
        debug_assert_eq!(output.sequence.records.len(), next_line_count);
        output
            .paragraphs
            .push(inline_layout::InlineMeasuredParagraph {
                graph,
                contribution,
            });
        paragraph.clear();
        true
    }

    pub(super) fn layout_inline_items_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        padding: (f32, f32),
        link_target: Option<&str>,
        marker: Option<&ListMarker>,
    ) {
        let (padding_left, padding_right) = padding;
        let available_width =
            (self.content_right - self.content_left - padding_left - padding_right).max(1.0);
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Outside
        {
            if self.cursor_y - style.font_size < self.page_bottom() {
                self.push_page();
            }
            self.paint_outside_marker(
                marker,
                style,
                self.content_left + padding_left,
                self.content_right - padding_right,
                self.cursor_y,
            );
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(style, link_target.clone(), 0.0, &mut items);
        }
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Inside
            && (marker.image.is_some() || !trim_css_collapsible_whitespace(&marker.text).is_empty())
        {
            self.push_inside_marker_items(marker, style, link_target.clone(), &mut items);
        }
        self.push_generated_pseudo_items(
            element,
            style.before_style.as_deref(),
            link_target.clone(),
            0.0,
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        self.collect_element_content_or_inline_items(
            element,
            style,
            stylesheets,
            link_target.clone(),
            0.0,
            &mut items,
        );
        self.push_generated_pseudo_items(
            element,
            style.after_style.as_deref(),
            link_target.clone(),
            0.0,
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(style, link_target, 0.0, &mut items);
        }
        self.layout_inline_items(
            items,
            style,
            available_width,
            padding_left,
            0.0,
            stylesheets,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn layout_run_in_inline_items_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        children: &[box_tree::FormattingBox<'_>],
        link_target: Option<&str>,
        marker: Option<&ListMarker>,
    ) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Outside
        {
            if self.cursor_y - style.font_size < self.page_bottom() {
                self.push_page();
            }
            self.paint_outside_marker(
                marker,
                style,
                self.content_left,
                self.content_right,
                self.cursor_y,
            );
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(style, link_target.clone(), 0.0, &mut items);
        }
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Inside
            && (marker.image.is_some() || !trim_css_collapsible_whitespace(&marker.text).is_empty())
        {
            self.push_inside_marker_items(marker, style, link_target.clone(), &mut items);
        }
        self.collect_inline_box_items(
            run_in_children,
            stylesheets,
            link_target.clone(),
            0.0,
            style,
            style.text_decoration,
            &mut items,
        );
        if style.content.is_generated() {
            self.push_element_content_items_from_boxes(
                element,
                style,
                children,
                stylesheets,
                link_target.clone(),
                0.0,
                style,
                style.text_decoration,
                &mut items,
            );
        } else {
            self.collect_inline_box_items(
                children,
                stylesheets,
                link_target.clone(),
                0.0,
                style,
                style.text_decoration,
                &mut items,
            );
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(style, link_target, 0.0, &mut items);
        }
        if !items.is_empty() {
            self.layout_inline_items(items, style, available_width, 0.0, 0.0, stylesheets);
        }
    }

    fn collect_intrinsic_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        output: &mut Vec<InlineItem>,
    ) {
        let sibling_tags = element_sibling_tags(element);
        let mut element_index = 0usize;
        for child in &element.children {
            match &child.kind {
                NodeKind::Text(text) => {
                    self.push_inline_words(
                        text,
                        style,
                        inherited_link.clone(),
                        baseline_shift,
                        output,
                    );
                }
                NodeKind::Element(child_element) => {
                    let child_signature = ElementSignature::with_siblings(
                        child_element.tag.clone(),
                        child_element.attrs.clone(),
                        element_index,
                        sibling_tags.clone(),
                    );
                    element_index += 1;
                    let mut child_style = self.style_for_layout_element_with_parent_font_metrics(
                        child_element,
                        child_signature,
                        stylesheets,
                        Some(style),
                    );
                    if child_style.float != Float::None
                        || matches!(child_style.position, Position::Absolute | Position::Fixed)
                        || child_style.display.is_none()
                        || child_style.display.is_block_level()
                    {
                        continue;
                    }
                    child_style.text_decoration = child_style
                        .text_decoration
                        .with_propagated_lines(style.text_decoration);
                    let link = child_element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_baseline_shift = baseline_shift
                        + self.vertical_align_baseline_shift_for_inline_style(&child_style, style);
                    if let Some(mut atom) = self.intrinsic_inline_atom_for_element(
                        child_element,
                        &child_style,
                        &[],
                        None,
                        stylesheets,
                        baseline_shift,
                        link.clone(),
                    ) {
                        atom.baseline_shift +=
                            self.vertical_align_baseline_shift_for_atom(&atom, style);
                        output.push(InlineItem::Atom(Box::new(atom)));
                        continue;
                    }
                    let scope = self.begin_inline_element_scope(
                        child_element,
                        &child_style,
                        link.clone(),
                        child_baseline_shift,
                        InlineElementScopeOptions::DOM_INTRINSIC,
                        output,
                    );
                    self.push_generated_pseudo_items(
                        child_element,
                        child_style.before_style.as_deref(),
                        link.clone(),
                        child_baseline_shift,
                        GeneratedPseudoCounterMode::Rollback,
                        output,
                    );
                    self.collect_intrinsic_element_content_or_inline_items(
                        child_element,
                        &child_style,
                        stylesheets,
                        link.clone(),
                        child_baseline_shift,
                        output,
                    );
                    self.push_generated_pseudo_items(
                        child_element,
                        child_style.after_style.as_deref(),
                        link.clone(),
                        child_baseline_shift,
                        GeneratedPseudoCounterMode::Rollback,
                        output,
                    );
                    self.end_inline_element_scope(scope, &child_style, output);
                }
            }
        }
    }

    fn collect_intrinsic_element_content_or_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        output: &mut Vec<InlineItem>,
    ) {
        if style.content.is_generated() {
            self.push_intrinsic_element_content_items_from_dom(
                element,
                style,
                stylesheets,
                inherited_link,
                baseline_shift,
                output,
            );
        } else {
            self.collect_intrinsic_inline_items(
                element,
                style,
                stylesheets,
                inherited_link,
                baseline_shift,
                output,
            );
        }
    }

    fn push_intrinsic_element_content_items_from_dom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_intrinsic_inline_items(
                        element,
                        style,
                        stylesheets,
                        inherited_link.clone(),
                        baseline_shift,
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                inherited_link.clone(),
                baseline_shift,
                alt_text.clone(),
                output,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_intrinsic_element_content_items_from_boxes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        propagated_decoration: css::TextDecoration,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_intrinsic_inline_box_items(
                        children,
                        stylesheets,
                        inherited_link.clone(),
                        IntrinsicInlineCollectionContext {
                            baseline_shift,
                            block_style: style,
                            propagated_decoration,
                        },
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                inherited_link.clone(),
                baseline_shift,
                alt_text.clone(),
                output,
            );
        }
    }

    /// Push a regular inline box edge into the inline formatting stream.
    ///
    /// CSS Inline treats inline-level margin, border, and padding as part of
    /// the inline box fragments. Keeping the start/end edges as explicit atomic
    /// items lets line fitting account for positive, negative, and zero-net
    /// inline margins while preserving the nonnegative border/padding paint:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting> and
    /// <https://www.w3.org/TR/css-break-3/#break-decoration>.
    fn push_inline_box_edge_item(
        &mut self,
        style: &ComputedStyle,
        edge: InlineBoxEdge,
        baseline_shift: f32,
        link_target: Option<String>,
        output: &mut Vec<InlineItem>,
    ) {
        let width = inline_box_edge_width(style, edge);
        if !inline_box_edge_has_nonzero_component(style, edge) {
            return;
        }
        let (_, border, padding) = inline_box_edge_components(style, edge);
        let edge_fragment = InlineBoxEdgeFragment {
            logical_edge: match edge {
                InlineBoxEdge::Start => InlineLogicalEdge::Start,
                InlineBoxEdge::End => InlineLogicalEdge::End,
            },
            physical_side: inline_box_edge_physical_side(style, edge),
            advance: width,
            paint_extent: (border + padding).max(0.0),
        };
        let baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
        output.push(InlineItem::Atom(Box::new(InlineAtom {
            content: InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge_fragment)),
            style: style.clone(),
            escaped_positioned_layers: None,
            width,
            height: style.line_height,
            baseline_offset,
            baseline_shift,
            link_target,
            alt_text: None,
        })));
    }

    fn begin_inline_element_scope(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        options: InlineElementScopeOptions,
        output: &mut Vec<InlineItem>,
    ) -> InlineElementScopeState {
        let counter_scope = self.begin_counter_scope(element, style);
        let inline_box_start = output.len();
        if options.fragment_edges.owns_start {
            self.push_inline_box_edge_item(
                style,
                InlineBoxEdge::Start,
                baseline_shift,
                None,
                output,
            );
        }
        let pushed_page_scope = options.push_page_scope && style.page_name_specified;
        if pushed_page_scope {
            output.push(InlineItem::PageScopeStart(style.page_name.clone()));
        }
        self.push_bidi_scope_start(style, link_target.clone(), baseline_shift, output);
        if options.push_inside_marker
            && style.display.is_list_item()
            && style.list_style_position == ListStylePosition::Inside
            && let Some(marker) =
                self.marker_for_list_item(element, style, self.containing_block_direction)
        {
            self.push_inside_marker_items(&marker, style, link_target.clone(), output);
        }
        InlineElementScopeState {
            inline_box_start,
            link_target,
            baseline_shift,
            pushed_page_scope,
            mark_hanging_edges: options.mark_hanging_edges,
            fragment_edges: options.fragment_edges,
            counter_scope,
        }
    }

    fn end_inline_element_scope(
        &mut self,
        state: InlineElementScopeState,
        style: &ComputedStyle,
        output: &mut Vec<InlineItem>,
    ) {
        self.push_bidi_scope_end(style, state.link_target, state.baseline_shift, output);
        if state.pushed_page_scope {
            output.push(InlineItem::PageScopeEnd);
        }
        if state.fragment_edges.owns_end {
            self.push_inline_box_edge_item(
                style,
                InlineBoxEdge::End,
                state.baseline_shift,
                None,
                output,
            );
        }
        if state.mark_hanging_edges {
            mark_inline_box_hanging_edges(
                output,
                state.inline_box_start,
                style,
                state.fragment_edges,
            );
        }
        self.end_counter_scope(state.counter_scope);
    }

    pub(in crate::layout) fn collect_element_content_or_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        output: &mut Vec<InlineItem>,
    ) {
        if style.content.is_generated() {
            self.push_element_content_items_from_dom(
                element,
                style,
                stylesheets,
                inherited_link,
                baseline_shift,
                output,
            );
        } else {
            self.collect_inline_items(
                element,
                style,
                stylesheets,
                inherited_link,
                baseline_shift,
                output,
            );
        }
    }

    fn push_element_content_items_from_dom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_inline_items(
                        element,
                        style,
                        stylesheets,
                        inherited_link.clone(),
                        baseline_shift,
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                inherited_link.clone(),
                baseline_shift,
                alt_text.clone(),
                output,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_element_content_items_from_boxes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        block_style: &ComputedStyle,
        propagated_decoration: css::TextDecoration,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(parts) = style.content.generated_parts().map(|parts| parts.to_vec()) else {
            return;
        };
        let alt_text = self.generated_alt_text(element, style);
        let mut used_contents = false;
        for part in &parts {
            if matches!(part, GeneratedContentPart::Contents) {
                if !used_contents {
                    used_contents = true;
                    self.collect_inline_box_items(
                        children,
                        stylesheets,
                        inherited_link.clone(),
                        baseline_shift,
                        block_style,
                        propagated_decoration,
                        output,
                    );
                }
                continue;
            }
            self.push_generated_content_part(
                element,
                part,
                style,
                inherited_link.clone(),
                baseline_shift,
                alt_text.clone(),
                output,
            );
        }
    }

    pub(super) fn collect_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        output: &mut Vec<InlineItem>,
    ) {
        let sibling_tags = element_sibling_tags(element);
        let mut element_index = 0usize;
        for child in &element.children {
            match &child.kind {
                NodeKind::Text(text) => {
                    self.push_inline_words(
                        text,
                        style,
                        inherited_link.clone(),
                        baseline_shift,
                        output,
                    );
                }
                NodeKind::Element(child_element) => {
                    let child_signature = ElementSignature::with_siblings(
                        child_element.tag.clone(),
                        child_element.attrs.clone(),
                        element_index,
                        sibling_tags.clone(),
                    );
                    element_index += 1;
                    let mut child_style = self.style_for_layout_element_with_parent_font_metrics(
                        child_element,
                        child_signature.clone(),
                        stylesheets,
                        Some(style),
                    );
                    if child_style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat {
                            element: child_element.clone(),
                            signature: child_signature,
                            style: child_style,
                        })));
                        continue;
                    }
                    if matches!(child_style.position, Position::Absolute | Position::Fixed) {
                        self.layout_positioned_inline_descendant(
                            child_element,
                            &child_style,
                            stylesheets,
                            None,
                            None,
                            style,
                            output,
                        );
                        continue;
                    }
                    child_style.text_decoration = child_style
                        .text_decoration
                        .with_propagated_lines(style.text_decoration);
                    if child_style.display.is_none()
                        || child_style.display.is_block_level()
                        || child_style.display.is_table()
                    {
                        continue;
                    }
                    let link = child_element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_baseline_shift = baseline_shift
                        + self.vertical_align_baseline_shift_for_inline_style(&child_style, style);
                    let scope = self.begin_inline_element_scope(
                        child_element,
                        &child_style,
                        link.clone(),
                        child_baseline_shift,
                        InlineElementScopeOptions::DOM_PAINT,
                        output,
                    );
                    self.push_generated_pseudo_items(
                        child_element,
                        child_style.before_style.as_deref(),
                        link.clone(),
                        child_baseline_shift,
                        GeneratedPseudoCounterMode::Commit,
                        output,
                    );
                    self.collect_element_content_or_inline_items(
                        child_element,
                        &child_style,
                        stylesheets,
                        link.clone(),
                        child_baseline_shift,
                        output,
                    );
                    self.push_generated_pseudo_items(
                        child_element,
                        child_style.after_style.as_deref(),
                        link.clone(),
                        child_baseline_shift,
                        GeneratedPseudoCounterMode::Commit,
                        output,
                    );
                    self.end_inline_element_scope(scope, &child_style, output);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn collect_inline_box_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        baseline_shift: f32,
        block_style: &ComputedStyle,
        propagated_decoration: css::TextDecoration,
        output: &mut Vec<InlineItem>,
    ) {
        for (child_index, child) in children.iter().enumerate() {
            if let Some((element, _, style, child_boxes)) = child.element_parts()
                && matches!(style.position, Position::Absolute | Position::Fixed)
            {
                let table_fragment = match child {
                    box_tree::FormattingBox::AtomicInline(box_) => box_.table_fragment.as_ref(),
                    box_tree::FormattingBox::Table(box_) => Some(&box_.fragment),
                    _ => None,
                };
                self.layout_positioned_inline_descendant(
                    element,
                    style,
                    stylesheets,
                    Some(child_boxes),
                    table_fragment,
                    block_style,
                    output,
                );
                continue;
            }
            match child {
                box_tree::FormattingBox::Text(box_) => {
                    let mut text_style = box_.style.clone();
                    text_style.text_decoration = text_style
                        .text_decoration
                        .with_propagated_lines(propagated_decoration);
                    let text = if child_index + 1 == children.len() {
                        trim_terminal_preserved_segment_breaks(&box_.text, &text_style)
                    } else {
                        box_.text.as_str()
                    };
                    self.push_inline_words(
                        text,
                        &text_style,
                        inherited_link.clone(),
                        baseline_shift,
                        output,
                    );
                }
                box_tree::FormattingBox::Inline(box_) => {
                    if box_.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat {
                            element: box_.element.clone(),
                            signature: box_.signature.clone(),
                            style: box_.style.clone(),
                        })));
                        continue;
                    }
                    let mut inline_style = box_.style.clone();
                    inline_style.text_decoration = inline_style
                        .text_decoration
                        .with_propagated_lines(propagated_decoration);
                    let link = box_
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_baseline_shift = baseline_shift
                        + self.vertical_align_baseline_shift_for_inline_style(
                            &inline_style,
                            block_style,
                        );
                    let scope = self.begin_inline_element_scope(
                        box_.element,
                        &inline_style,
                        link.clone(),
                        child_baseline_shift,
                        InlineElementScopeOptions::BOX_PAINT
                            .with_fragment_edges(box_.fragment_edges),
                        output,
                    );
                    if inline_style.content.is_generated() {
                        self.push_element_content_items_from_boxes(
                            box_.element,
                            &inline_style,
                            &box_.children,
                            stylesheets,
                            link.clone(),
                            child_baseline_shift,
                            block_style,
                            inline_style.text_decoration,
                            output,
                        );
                    } else {
                        self.collect_inline_box_items(
                            &box_.children,
                            stylesheets,
                            link.clone(),
                            child_baseline_shift,
                            block_style,
                            inline_style.text_decoration,
                            output,
                        );
                    }
                    self.end_inline_element_scope(scope, &inline_style, output);
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    if box_.style.float != Float::None {
                        output.push(InlineItem::Float(Box::new(InlineFloat {
                            element: box_.element.clone(),
                            signature: box_.signature.clone(),
                            style: box_.style.clone(),
                        })));
                        continue;
                    }
                    let link = box_
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    if let Some(mut atom) = self.inline_atom_for_element(
                        box_.element,
                        &box_.signature,
                        &box_.style,
                        &box_.children,
                        box_.table_fragment.as_ref(),
                        stylesheets,
                        baseline_shift,
                        link.clone(),
                    ) {
                        atom.baseline_shift +=
                            self.vertical_align_baseline_shift_for_atom(&atom, block_style);
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.element, &box_.style);
                        self.push_inline_words(&text, &box_.style, link, baseline_shift, output);
                    }
                }
                box_tree::FormattingBox::Line(box_) => {
                    for text in &box_.children {
                        self.push_inline_words(
                            &text.text,
                            &text.style,
                            inherited_link.clone(),
                            baseline_shift,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => self.collect_inline_box_items(
                    &box_.children,
                    stylesheets,
                    inherited_link.clone(),
                    baseline_shift,
                    block_style,
                    box_.style
                        .text_decoration
                        .with_propagated_lines(propagated_decoration),
                    output,
                ),
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Table(_)
                | box_tree::FormattingBox::Flex(_)
                | box_tree::FormattingBox::Replaced(_) => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_positioned_inline_descendant(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        block_style: &ComputedStyle,
        output: &[InlineItem],
    ) {
        let source_was_inline_level =
            style.abspos_static_source_was_inline_level || style.display.is_inline_level();
        if source_was_inline_level {
            let mut positioned_style = style.clone();
            positioned_style.abspos_static_source_was_inline_level = true;
            let static_position = self.inline_static_position_from_hypothetical_placeholder(
                element,
                &positioned_style,
                stylesheets,
                child_boxes,
                table_fragment,
                block_style,
                output,
            );
            self.layout_positioned_block_with_inline_static_position(
                element,
                &positioned_style,
                stylesheets,
                child_boxes,
                table_fragment,
                static_position,
            );
            return;
        }

        let static_y_offset = self.block_static_position_y_offset_from_buffer(output, block_style);
        self.layout_positioned_block_with_block_static_y_offset(
            element,
            style,
            stylesheets,
            child_boxes,
            table_fragment,
            static_y_offset,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn inline_static_position_from_hypothetical_placeholder(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        block_style: &ComputedStyle,
        output: &[InlineItem],
    ) -> InlineStaticPosition {
        let placeholder = self.inline_static_position_placeholder_atom(
            element,
            style,
            stylesheets,
            child_boxes,
            table_fragment,
        );
        let mut hypothetical_items = Vec::with_capacity(output.len() + 1);
        hypothetical_items.extend_from_slice(output);
        hypothetical_items.push(InlineItem::Atom(Box::new(placeholder)));
        let available_width = (self.content_right - self.content_left).max(1.0);
        // CSS Positioned Layout defines the static-position rectangle as the
        // box's hypothetical normal-flow position. Carrying a non-painting
        // placeholder through ordinary inline line selection keeps forced
        // breaks, wrapping, and line metrics aligned with the same CSS Text
        // machinery used for real inline content:
        // https://www.w3.org/TR/css-position-3/#staticpos-rect
        // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
        let sequence = self.collect_inline_line_sequence(
            hypothetical_items,
            block_style,
            available_width,
            0.0,
            0.0,
        );
        self.inline_static_position_from_placeholder_sequence(&sequence, block_style)
            .unwrap_or_else(|| InlineStaticPosition {
                start_x: self.content_left,
                baseline_y: self.inline_static_baseline_y_from_buffer(output, style),
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn inline_static_position_placeholder_atom(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> InlineAtom {
        let available_width = (self.content_right - self.content_left).max(style.font_size);
        let mut placeholder_style = self.style_with_current_viewport_lengths(style);
        apply_used_box_metrics(&mut placeholder_style, available_width);
        let horizontal_non_content = placeholder_style.padding.left
            + placeholder_style.padding.right
            + horizontal_border_width(&placeholder_style);
        let positioned_available_outer_width =
            (available_width - placeholder_style.margin.left - placeholder_style.margin.right)
                .max(placeholder_style.font_size);
        let content_width = self.used_intrinsic_or_shrink_to_fit_width(
            element,
            &placeholder_style,
            stylesheets,
            positioned_available_outer_width,
            horizontal_non_content,
            child_boxes,
            table_fragment,
        );
        let border_box_width = content_width + horizontal_non_content;
        let line_baseline_offset = self
            .font_system
            .rendered_first_line_baseline_offset(&placeholder_style);

        InlineAtom {
            content: InlineAtomContent::StaticPositionPlaceholder,
            style: placeholder_style.clone(),
            escaped_positioned_layers: None,
            width: border_box_width
                + placeholder_style.margin.left
                + placeholder_style.margin.right,
            height: placeholder_style.line_height
                + placeholder_style.margin.top
                + placeholder_style.margin.bottom,
            baseline_offset: line_baseline_offset,
            baseline_shift: 0.0,
            link_target: None,
            alt_text: None,
        }
    }

    fn inline_static_position_from_placeholder_sequence(
        &mut self,
        sequence: &inline_layout::InlineLineSequence,
        block_style: &ComputedStyle,
    ) -> Option<InlineStaticPosition> {
        let saved_cursor_y = self.cursor_y;
        let context = sequence.context(block_style);
        let mut plaintext_direction_state = None;
        let mut line_top = self.cursor_y;
        for record in &sequence.records {
            if let Some(fragment) = &record.fragment
                && fragment.items.iter().any(|item| {
                    matches!(
                        &item.item,
                        InlineLineItem::Atom(atom)
                            if matches!(atom.content, InlineAtomContent::StaticPositionPlaceholder)
                    )
                })
            {
                self.cursor_y = line_top;
                let position = self
                    .prepare_inline_line_record(record, context, &mut plaintext_direction_state)
                    .and_then(|prepared| {
                        prepared.paint_items.iter().find_map(|item| {
                            let PreparedInlinePaintItem::Atom(atom) = item else {
                                return None;
                            };
                            matches!(
                                atom.atom.content,
                                InlineAtomContent::StaticPositionPlaceholder
                            )
                            .then_some(InlineStaticPosition {
                                start_x: atom.content_rect.x(),
                                baseline_y: line_top - prepared.metrics.baseline_offset,
                            })
                        })
                    });
                self.cursor_y = saved_cursor_y;
                return position;
            }
            line_top -= record.line_height;
        }
        self.cursor_y = saved_cursor_y;
        None
    }

    fn collect_intrinsic_inline_box_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        inherited_link: Option<String>,
        context: IntrinsicInlineCollectionContext<'_>,
        output: &mut Vec<InlineItem>,
    ) {
        for (child_index, child) in children.iter().enumerate() {
            if let Some((_, _, style, _)) = child.element_parts()
                && (matches!(style.position, Position::Absolute | Position::Fixed)
                    || style.float != Float::None)
            {
                continue;
            }
            match child {
                box_tree::FormattingBox::Text(box_) => {
                    let mut text_style = box_.style.clone();
                    text_style.text_decoration = text_style
                        .text_decoration
                        .with_propagated_lines(context.propagated_decoration);
                    let text = if child_index + 1 == children.len() {
                        trim_terminal_preserved_segment_breaks(&box_.text, &text_style)
                    } else {
                        box_.text.as_str()
                    };
                    self.push_inline_words(
                        text,
                        &text_style,
                        inherited_link.clone(),
                        context.baseline_shift,
                        output,
                    );
                }
                box_tree::FormattingBox::Inline(box_) => {
                    let mut inline_style = box_.style.clone();
                    inline_style.text_decoration = inline_style
                        .text_decoration
                        .with_propagated_lines(context.propagated_decoration);
                    let link = box_
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    let child_baseline_shift = context.baseline_shift
                        + self.vertical_align_baseline_shift_for_inline_style(
                            &inline_style,
                            context.block_style,
                        );
                    let scope = self.begin_inline_element_scope(
                        box_.element,
                        &inline_style,
                        link.clone(),
                        child_baseline_shift,
                        InlineElementScopeOptions::BOX_INTRINSIC
                            .with_fragment_edges(box_.fragment_edges),
                        output,
                    );
                    if inline_style.content.is_generated() {
                        self.push_intrinsic_element_content_items_from_boxes(
                            box_.element,
                            &inline_style,
                            &box_.children,
                            stylesheets,
                            link.clone(),
                            child_baseline_shift,
                            inline_style.text_decoration,
                            output,
                        );
                    } else {
                        self.collect_intrinsic_inline_box_items(
                            &box_.children,
                            stylesheets,
                            link.clone(),
                            context
                                .with_baseline_shift(child_baseline_shift)
                                .with_block_style(&inline_style)
                                .with_propagated_decoration(inline_style.text_decoration),
                            output,
                        );
                    }
                    self.end_inline_element_scope(scope, &inline_style, output);
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    let link = box_
                        .element
                        .attrs
                        .get("href")
                        .cloned()
                        .or_else(|| inherited_link.clone());
                    if let Some(mut atom) = self.intrinsic_inline_atom_for_element(
                        box_.element,
                        &box_.style,
                        &box_.children,
                        box_.table_fragment.as_ref(),
                        stylesheets,
                        context.baseline_shift,
                        link,
                    ) {
                        atom.baseline_shift +=
                            self.vertical_align_baseline_shift_for_atom(&atom, context.block_style);
                        output.push(InlineItem::Atom(Box::new(atom)));
                    } else {
                        let text = inline_text_for_style(box_.element, &box_.style);
                        self.push_inline_words(
                            &text,
                            &box_.style,
                            inherited_link.clone(),
                            context.baseline_shift,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::Line(box_) => {
                    for text in &box_.children {
                        self.push_inline_words(
                            &text.text,
                            &text.style,
                            inherited_link.clone(),
                            context.baseline_shift,
                            output,
                        );
                    }
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => self
                    .collect_intrinsic_inline_box_items(
                        &box_.children,
                        stylesheets,
                        inherited_link.clone(),
                        context
                            .with_block_style(&box_.style)
                            .with_propagated_decoration(
                                box_.style
                                    .text_decoration
                                    .with_propagated_lines(context.propagated_decoration),
                            ),
                        output,
                    ),
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::InlineSplitBlockContext(_)
                | box_tree::FormattingBox::Table(_)
                | box_tree::FormattingBox::Flex(_)
                | box_tree::FormattingBox::Replaced(_) => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn intrinsic_inline_atom_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> Option<InlineAtom> {
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        if let Content::Replacement {
            image:
                GeneratedContentPart::Image {
                    url,
                    base_url,
                    root_url,
                },
            ..
        } = &style.content
        {
            let image = used_generated_image(
                url,
                style,
                available_width,
                base_url.as_deref(),
                root_url.as_deref(),
                self.resource_cache,
            )?;
            return Some(InlineAtom {
                content: InlineAtomContent::Image(image.decoded),
                style: style.clone(),
                escaped_positioned_layers: None,
                width: image.border_box_width + style.margin.left + style.margin.right,
                height: image.border_box_height + style.margin.top + style.margin.bottom,
                baseline_offset: image.border_box_height,
                baseline_shift,
                link_target,
                alt_text: self.generated_alt_text(element, style),
            });
        }
        let (width, height, baseline_offset) = match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => {
                let containing_block_height =
                    self.definite_block_size_stack.last().copied().flatten();
                let (width, height) = used_canvas_size_with_height_basis(
                    element,
                    style,
                    available_width,
                    containing_block_height,
                );
                (
                    width + style.margin.left + style.margin.right,
                    height + style.margin.top + style.margin.bottom,
                    height,
                )
            }
            Some(ReplacedElementKind::Image) => used_image(
                element,
                style,
                available_width,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
            .map(|image| {
                (
                    image.border_box_width + style.margin.left + style.margin.right,
                    image.border_box_height + style.margin.top + style.margin.bottom,
                    image.border_box_height,
                )
            })?,
            Some(ReplacedElementKind::Svg) => {
                let (width, height, _) = svg_rect(element)?;
                (
                    width + style.margin.left + style.margin.right,
                    height + style.margin.top + style.margin.bottom,
                    height,
                )
            }
            None if style.display.is_table() => {
                let fragment = table_fragment?;
                let horizontal_extras =
                    style.padding.left + style.padding.right + horizontal_border_width(style);
                let (min_width, width) = self.table_parent_intrinsic_content_widths_from_fragment(
                    element,
                    style,
                    stylesheets,
                    fragment,
                    available_width,
                );
                let content_width = intrinsic::shrink_to_fit_width(
                    min_width,
                    width,
                    (available_width - horizontal_extras).max(0.0),
                );
                (
                    constrain_width(style, content_width, available_width)
                        + horizontal_extras
                        + style.margin.left
                        + style.margin.right,
                    style.line_height,
                    style.line_height,
                )
            }
            None if style.display.is_flex() && style.display.is_inline_level() => {
                let box_metrics = used_box_metrics(style, available_width);
                let horizontal_extras = box_metrics.horizontal_non_content();
                let (min_width, width) = self.estimate_flex_intrinsic_widths(
                    element,
                    style,
                    stylesheets,
                    available_width,
                    Some(children),
                );
                let content_width = intrinsic::content_width_from_intrinsic(
                    style,
                    available_width,
                    horizontal_extras,
                    min_width,
                    width,
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                );
                (
                    constrain_width(style, content_width, available_width)
                        + horizontal_extras
                        + style.margin.left
                        + style.margin.right,
                    style.line_height,
                    style.line_height,
                )
            }
            None if style.display.is_grid() && style.display.is_inline_level() => {
                return Some(self.intrinsic_inline_grid_atom_for_element(
                    element,
                    style,
                    children,
                    stylesheets,
                    baseline_shift,
                    link_target,
                ));
            }
            None if style.display.is_atomic_inline() => {
                let box_metrics = used_box_metrics(style, available_width);
                let horizontal_extras = box_metrics.horizontal_non_content();
                let vertical_extras = box_metrics.vertical_non_content();
                let contribution = if children.is_empty() {
                    let text = inline_text_for_style(element, style);
                    let contribution = self
                        .intrinsic_inline_measurement_for_text(&text, style, available_width)
                        .contribution;
                    inline_layout::InlineIntrinsicContribution {
                        min_content: contribution.min_content,
                        max_content: contribution.max_content,
                    }
                } else {
                    self.intrinsic_inline_contribution_for_element(
                        element,
                        style,
                        stylesheets,
                        Some(children),
                    )
                };
                let content_width = intrinsic::content_width_from_intrinsic(
                    style,
                    available_width,
                    horizontal_extras,
                    contribution.min_content,
                    contribution.max_content,
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                );
                let content_width = if style.box_values.width.is_auto() {
                    content_width.max(style.font_size)
                } else {
                    content_width
                };
                let measured_content_height = if children.is_empty() {
                    let text = inline_text_for_style(element, style);
                    self.intrinsic_inline_measurement_for_text(&text, style, content_width)
                        .height()
                        .max(style.line_height)
                } else {
                    self.intrinsic_inline_measurement_for_element(
                        element,
                        style,
                        stylesheets,
                        Some(children),
                        content_width,
                    )
                    .height()
                    .max(style.line_height)
                };
                let content_height =
                    used_content_height_or_auto(style, measured_content_height, vertical_extras)
                        .unwrap_or(measured_content_height);
                let content_height = constrain_height(style, content_height, available_width);
                let border_box_height = content_height + vertical_extras;
                (
                    constrain_width(style, content_width, available_width)
                        + horizontal_extras
                        + style.margin.left
                        + style.margin.right,
                    border_box_height + style.margin.top + style.margin.bottom,
                    border_box_height,
                )
            }
            None => return None,
        };
        Some(InlineAtom {
            content: InlineAtomContent::Svg {
                fill: Color::TRANSPARENT,
            },
            style: style.clone(),
            escaped_positioned_layers: None,
            width,
            height,
            baseline_offset,
            baseline_shift,
            link_target,
            alt_text: None,
        })
    }

    fn inline_static_baseline_y_from_buffer(
        &mut self,
        output: &[InlineItem],
        fallback_style: &ComputedStyle,
    ) -> f32 {
        if let Some(atom) = output.iter().rev().find_map(|item| match item {
            InlineItem::Atom(atom) if !atom.content.is_inline_edge() => Some(atom),
            _ => None,
        }) {
            let borders = used_border_widths(&atom.style);
            let atom_baseline_offset =
                atom.style.margin.top + atom.baseline_offset - atom.baseline_shift;
            let parent_baseline_offset = self
                .font_system
                .rendered_first_line_baseline_offset(&atom.style);
            let line_baseline_offset = atom_baseline_offset.max(parent_baseline_offset);
            return self.cursor_y - line_baseline_offset
                + atom.baseline_offset
                + atom.baseline_shift
                - borders.top
                - atom.style.padding.top
                - atom.style.font_size;
        }

        self.cursor_y
            - self
                .font_system
                .rendered_first_line_baseline_offset(fallback_style)
    }

    fn block_static_position_y_offset_from_buffer(
        &mut self,
        output: &[InlineItem],
        block_style: &ComputedStyle,
    ) -> f32 {
        if output.is_empty() {
            return 0.0;
        }
        let available_width = (self.content_right - self.content_left).max(1.0);
        // CSS Positioned Layout removes the abspos from flow, but CSS 2.2
        // computes auto inset static position from its hypothetical normal-flow
        // box. For a block-level source after inline content, the placeholder
        // sits after the line boxes already selected for the buffered inline
        // run:
        // https://www.w3.org/TR/css-position-3/#absolute-positioning
        // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
        let line_height = self
            .collect_inline_line_sequence(output.to_vec(), block_style, available_width, 0.0, 0.0)
            .total_height();
        // Split inline edge atoms preserve decoration boundaries, but do not
        // themselves occupy the line that selects the static position.
        let has_buffered_content = output.iter().any(|item| match item {
            InlineItem::Word(_) => true,
            InlineItem::Atom(atom) => !atom.content.is_inline_edge(),
            InlineItem::Float(_)
            | InlineItem::Break
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => false,
        });
        line_height
            + if has_buffered_content {
                block_style.line_height
            } else {
                0.0
            }
    }

    pub(in crate::layout) fn push_generated_pseudo_items(
        &mut self,
        element: &Element,
        pseudo_style: Option<&ComputedStyle>,
        link_target: Option<String>,
        baseline_shift: f32,
        counter_mode: GeneratedPseudoCounterMode,
        output: &mut Vec<InlineItem>,
    ) {
        let Some(pseudo_style) = pseudo_style else {
            return;
        };
        let Some(content) = pseudo_style.content.generated_parts() else {
            return;
        };
        let counter_snapshot = (counter_mode == GeneratedPseudoCounterMode::Rollback)
            .then(|| self.counter_set.clone());
        let counter_scope = self.begin_pseudo_counter_scope(pseudo_style);
        let alt_text = self.generated_alt_text(element, pseudo_style);
        let is_block = pseudo_style.display.is_block_level();
        if is_block
            && output
                .last()
                .is_some_and(|item| !matches!(item, InlineItem::Break))
        {
            trim_trailing_inline_spaces(output);
            output.push(InlineItem::Break);
        }
        let start_len = output.len();
        self.push_bidi_scope_start(pseudo_style, link_target.clone(), baseline_shift, output);
        let scope_start_len = output.len();
        for part in content {
            self.push_generated_content_part(
                element,
                part,
                pseudo_style,
                link_target.clone(),
                baseline_shift,
                alt_text.clone(),
                output,
            );
        }
        let emitted_content = output.len() > scope_start_len;
        if emitted_content {
            self.push_bidi_scope_end(pseudo_style, link_target, baseline_shift, output);
        } else {
            output.truncate(start_len);
        }
        if emitted_content
            && is_block
            && output
                .last()
                .is_some_and(|item| !matches!(item, InlineItem::Break))
        {
            trim_trailing_inline_spaces(output);
            output.push(InlineItem::Break);
        }
        self.end_counter_scope(counter_scope);
        if let Some(counter_snapshot) = counter_snapshot {
            self.counter_set = counter_snapshot;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_generated_content_part(
        &mut self,
        element: &Element,
        part: &GeneratedContentPart,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        alt_text: Option<String>,
        output: &mut Vec<InlineItem>,
    ) {
        match part {
            GeneratedContentPart::Text(text) => {
                self.push_inline_words(text, style, link_target, baseline_shift, output);
            }
            GeneratedContentPart::Contents => {
                let text = inline_text_for_style(element, style);
                self.push_inline_words(&text, style, link_target, baseline_shift, output);
            }
            GeneratedContentPart::Attr { .. }
            | GeneratedContentPart::Counter { .. }
            | GeneratedContentPart::Counters { .. } => {
                let text = evaluate_generated_content_text(
                    element,
                    std::slice::from_ref(part),
                    self.counter_set.stacks(),
                    &self.counter_styles,
                );
                self.push_inline_words(&text, style, link_target, baseline_shift, output);
            }
            GeneratedContentPart::Quote(quote) => {
                let text = self.generated_quote_text(*quote, style);
                self.push_inline_words(&text, style, link_target, baseline_shift, output);
            }
            GeneratedContentPart::Leader(text) => {
                output.push(InlineItem::Atom(Box::new(InlineAtom {
                    content: InlineAtomContent::Leader(text.clone()),
                    style: style.clone(),
                    escaped_positioned_layers: None,
                    width: 0.0,
                    height: style.line_height,
                    baseline_offset: style.font_size,
                    baseline_shift,
                    link_target,
                    alt_text: None,
                })));
            }
            GeneratedContentPart::Image {
                url,
                base_url,
                root_url,
            } => {
                if let Some(atom) = self.generated_image_atom_for_url(
                    url,
                    base_url.as_deref(),
                    root_url.as_deref(),
                    style,
                    baseline_shift,
                    link_target,
                    alt_text,
                ) {
                    output.push(InlineItem::Atom(Box::new(atom)));
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn generated_image_atom_for_url(
        &mut self,
        url: &str,
        base_url: Option<&Path>,
        root_url: Option<&Path>,
        style: &ComputedStyle,
        baseline_shift: f32,
        link_target: Option<String>,
        alt_text: Option<String>,
    ) -> Option<InlineAtom> {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        apply_used_box_metrics(&mut used_style, available_width);
        let style = &used_style;
        let image = used_generated_image(
            url,
            style,
            available_width,
            base_url,
            root_url,
            self.resource_cache,
        )?;
        Some(InlineAtom {
            content: InlineAtomContent::Image(image.decoded),
            style: style.clone(),
            escaped_positioned_layers: None,
            width: image.border_box_width + style.margin.left + style.margin.right,
            height: image.border_box_height + style.margin.top + style.margin.bottom,
            baseline_offset: image.border_box_height,
            baseline_shift,
            link_target,
            alt_text,
        })
    }

    pub(super) fn generated_alt_text(
        &self,
        element: &Element,
        style: &ComputedStyle,
    ) -> Option<String> {
        style.content.alt().map(|alt| {
            evaluate_generated_alt_text(
                element,
                alt,
                self.counter_set.stacks(),
                &self.counter_styles,
            )
        })
    }

    fn generated_quote_text(&mut self, quote: GeneratedQuote, style: &ComputedStyle) -> String {
        match quote {
            GeneratedQuote::Open => {
                let text = quote_pair(style, self.quote_depth).0;
                self.quote_depth += 1;
                text
            }
            GeneratedQuote::Close => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                quote_pair(style, self.quote_depth).1
            }
            GeneratedQuote::NoOpen => {
                self.quote_depth += 1;
                String::new()
            }
            GeneratedQuote::NoClose => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                String::new()
            }
        }
    }

    /// Push UBA start controls for a CSS `unicode-bidi` inline scope.
    ///
    /// CSS Writing Modes defines `unicode-bidi` as adding embedding,
    /// isolation, override, or plaintext bidi controls around generated inline
    /// boxes:
    /// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
    pub(super) fn push_bidi_scope_start(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        output: &mut Vec<InlineItem>,
    ) {
        self.push_bidi_scope_start_with_source(
            style,
            link_target,
            baseline_shift,
            InlineTextSource::Normal,
            output,
        );
    }

    pub(super) fn push_bidi_scope_start_with_source(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        source: InlineTextSource,
        output: &mut Vec<InlineItem>,
    ) {
        if let Some((start, _)) = bidi_control_scope_for_style(style) {
            self.push_bidi_control_text(start, style, link_target, baseline_shift, source, output);
        }
    }

    /// Push UBA end controls for a CSS `unicode-bidi` inline scope.
    ///
    /// CSS Writing Modes scopes embedding, isolation, and override controls to
    /// the element's inline box and terminates them with UAX #9 PDF/PDI
    /// controls:
    /// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
    pub(super) fn push_bidi_scope_end(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        output: &mut Vec<InlineItem>,
    ) {
        self.push_bidi_scope_end_with_source(
            style,
            link_target,
            baseline_shift,
            InlineTextSource::Normal,
            output,
        );
    }

    pub(super) fn push_bidi_scope_end_with_source(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        source: InlineTextSource,
        output: &mut Vec<InlineItem>,
    ) {
        if let Some((_, end)) = bidi_control_scope_for_style(style) {
            self.push_bidi_control_text(end, style, link_target, baseline_shift, source, output);
        }
    }

    /// Push invisible bidi control text without CSS text transforms.
    ///
    /// Directional formatting controls are UAX #9 algorithmic input; they
    /// affect ordering but do not create visible CSS text or PDF glyphs:
    /// <https://www.unicode.org/reports/tr9/#Directional_Formatting_Characters>.
    fn push_bidi_control_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        source: InlineTextSource,
        output: &mut Vec<InlineItem>,
    ) {
        if !text.is_empty() {
            output.push(InlineItem::Word(Box::new(InlineWord {
                text: text.to_string(),
                style: style.clone(),
                baseline_shift,
                link_target,
                mergeable: true,
                source,
                hanging_edges: InlineHangingEdges::default(),
            })));
        }
    }

    pub(super) fn push_inline_words(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        output: &mut Vec<InlineItem>,
    ) {
        push_inline_words_for_style(text, style, link_target, baseline_shift, output);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn inline_atom_for_element(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> Option<InlineAtom> {
        if let Content::Replacement {
            image:
                GeneratedContentPart::Image {
                    url,
                    base_url,
                    root_url,
                },
            ..
        } = &style.content
        {
            let alt_text = self.generated_alt_text(element, style);
            return self.generated_image_atom_for_url(
                url,
                base_url.as_deref(),
                root_url.as_deref(),
                style,
                baseline_shift,
                link_target,
                alt_text,
            );
        }
        match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => {
                let available_width = (self.content_right - self.content_left).max(1.0);
                let style = self.style_with_current_viewport_lengths(style);
                let containing_block_height =
                    self.definite_block_size_stack.last().copied().flatten();
                let (width, height) = used_canvas_size_with_height_basis(
                    element,
                    &style,
                    available_width,
                    containing_block_height,
                );
                let atom_width = width + style.margin.left + style.margin.right;
                Some(InlineAtom {
                    content: InlineAtomContent::Canvas,
                    style,
                    escaped_positioned_layers: None,
                    width: atom_width,
                    height,
                    baseline_offset: height,
                    baseline_shift,
                    link_target,
                    alt_text: None,
                })
            }
            Some(ReplacedElementKind::Image) => {
                let available_width = (self.content_right - self.content_left).max(1.0);
                let mut used_style = self.style_with_current_viewport_lengths(style);
                apply_used_box_metrics(&mut used_style, available_width);
                let style = &used_style;
                let image = used_image(
                    element,
                    style,
                    available_width,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                )?;
                Some(InlineAtom {
                    content: InlineAtomContent::Image(image.decoded),
                    style: style.clone(),
                    escaped_positioned_layers: None,
                    width: image.border_box_width + style.margin.left + style.margin.right,
                    height: image.border_box_height + style.margin.top + style.margin.bottom,
                    baseline_offset: image.border_box_height,
                    baseline_shift,
                    link_target,
                    alt_text: element.attrs.get("alt").cloned(),
                })
            }
            Some(ReplacedElementKind::Svg) => {
                let (width, height, fill) = svg_rect(element)?;
                Some(InlineAtom {
                    content: InlineAtomContent::Svg { fill },
                    style: style.clone(),
                    escaped_positioned_layers: None,
                    width: width + style.margin.left + style.margin.right,
                    height,
                    baseline_offset: height,
                    baseline_shift,
                    link_target,
                    alt_text: None,
                })
            }
            None if style.display.is_table() => self.inline_table_atom_for_element(
                element,
                style,
                children,
                table_fragment?,
                stylesheets,
                baseline_shift,
                link_target,
            ),
            None if style.display.is_flex() && style.display.is_inline_level() => {
                Some(self.inline_flex_atom_for_element(
                    element,
                    signature,
                    style,
                    children,
                    stylesheets,
                    baseline_shift,
                    link_target,
                ))
            }
            None if style.display.is_grid() && style.display.is_inline_level() => {
                Some(self.inline_grid_atom_for_element(
                    element,
                    style,
                    children,
                    stylesheets,
                    baseline_shift,
                    link_target,
                ))
            }
            None if style.display.is_atomic_inline() => {
                if has_non_inline_formatting_box(children)
                    || has_atomic_inline_formatting_box(children)
                    || has_inline_container_formatting_box(children)
                    || has_out_of_flow_formatting_box(children)
                {
                    return Some(self.inline_fragment_atom_for_children(
                        style,
                        children,
                        stylesheets,
                        baseline_shift,
                        link_target,
                    ));
                }
                let available_width = (self.content_right
                    - self.content_left
                    - style.margin.left
                    - style.margin.right)
                    .max(style.font_size);
                let mut used_style = self.style_with_current_viewport_lengths(style);
                let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
                let style = &used_style;
                let border_widths = box_metrics.border;
                let horizontal_extras = box_metrics.horizontal_non_content();
                let vertical_extras = box_metrics.vertical_non_content();
                let intrinsic = self.intrinsic_inline_measurement_for_element(
                    element,
                    style,
                    stylesheets,
                    Some(children),
                    available_width,
                );
                let requested_content_width = intrinsic::content_width_from_intrinsic(
                    style,
                    available_width,
                    horizontal_extras,
                    intrinsic.contribution.min_content,
                    intrinsic.contribution.max_content,
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                );
                let requested_content_width = if style.box_values.width.is_auto() {
                    requested_content_width.max(style.font_size)
                } else {
                    requested_content_width
                };
                let content_width =
                    constrain_width(style, requested_content_width, available_width)
                        .max(style.font_size);
                let mut sequence_items = Vec::new();
                self.push_generated_pseudo_items(
                    element,
                    style.before_style.as_deref(),
                    link_target.clone(),
                    0.0,
                    GeneratedPseudoCounterMode::Commit,
                    &mut sequence_items,
                );
                if style.content.is_generated() {
                    self.push_element_content_items_from_boxes(
                        element,
                        style,
                        children,
                        stylesheets,
                        link_target.clone(),
                        0.0,
                        style,
                        style.text_decoration,
                        &mut sequence_items,
                    );
                } else {
                    self.collect_inline_box_items(
                        children,
                        stylesheets,
                        link_target.clone(),
                        0.0,
                        style,
                        style.text_decoration,
                        &mut sequence_items,
                    );
                }
                self.push_generated_pseudo_items(
                    element,
                    style.after_style.as_deref(),
                    link_target.clone(),
                    0.0,
                    GeneratedPseudoCounterMode::Commit,
                    &mut sequence_items,
                );
                let sequence = self.collect_inline_line_sequence(
                    sequence_items,
                    style,
                    content_width,
                    0.0,
                    0.0,
                );
                let measured_content_height = sequence.total_height().max(style.line_height);
                let requested_content_height =
                    used_content_height_or_auto(style, measured_content_height, vertical_extras)
                        .unwrap_or(measured_content_height);
                // CSS Sizing applies `height` to the content box; line-height
                // can overflow explicit-height inline-blocks but must not
                // increase their used height:
                // <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
                let content_height =
                    constrain_height(style, requested_content_height, available_width);
                let border_box_height = content_height + vertical_extras;
                let baseline_offset = self
                    .inline_box_sequence_baseline_offset(&sequence, style, border_widths)
                    .unwrap_or(border_box_height);
                Some(InlineAtom {
                    content: InlineAtomContent::InlineBox { sequence },
                    style: style.clone(),
                    escaped_positioned_layers: None,
                    width: content_width
                        + horizontal_extras
                        + style.margin.left
                        + style.margin.right,
                    height: border_box_height + style.margin.top + style.margin.bottom,
                    baseline_offset,
                    baseline_shift,
                    link_target,
                    alt_text: None,
                })
            }
            None => None,
        }
    }

    pub(super) fn inline_fragment_atom_for_children(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> InlineAtom {
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
        let style = &used_style;
        let borders = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content();
        let vertical_extras = box_metrics.vertical_non_content();
        let contribution =
            self.intrinsic_inline_contribution_for_boxes(children, style, stylesheets);
        let preferred_min = contribution.min_content.max(style.font_size);
        let preferred = self
            .inline_boxes_max_content_width(children, stylesheets, available_width)
            .max(self.inline_block_float_row_max_content_width(
                children,
                stylesheets,
                available_width,
            ))
            .max(style.font_size);
        let requested_content_width = intrinsic::content_width_from_intrinsic(
            style,
            available_width,
            horizontal_extras,
            preferred_min,
            preferred,
            intrinsic::IntrinsicAutoWidth::ShrinkToFit,
        );
        let content_width =
            constrain_width(style, requested_content_width, available_width).max(style.font_size);

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let top = 10_000.0;
        let content_left = borders.left + style.padding.left;
        let content_top = top - borders.top - style.padding.top;
        self.current_page = Page::new(content_width + horizontal_extras, top);
        self.content_left = content_left;
        self.content_right = content_left + content_width;
        self.cursor_y = content_top;
        self.truncate_page_start_margins = false;
        let establishes_positioning_containing_block =
            matches!(style.position, Position::Relative | Position::Sticky)
                || !style.transform.is_empty();
        if establishes_positioning_containing_block {
            // CSS Positioned Layout uses the padding box of a positioned
            // or transformed ancestor as the containing block for absolute
            // descendants. This inline-block fragment is laid out in a
            // temporary page whose border-box origin is (0, top).
            self.containing_blocks
                .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                    borders.left,
                    top - borders.top,
                    content_width + style.padding.left + style.padding.right,
                    used_content_height_or_auto(style, top, 0.0)
                        .unwrap_or(style.line_height)
                        .max(0.0)
                        + style.padding.top
                        + style.padding.bottom,
                )));
        }
        self.push_page_name_scope_suppression();
        self.push_float_context();
        let previous_block_static_position_y_offset = self.block_static_position_y_offset;
        // The inline-block fragment is laid out on a temporary page, while an
        // absolutely positioned descendant's containing block can still be an
        // ancestor outside that temporary coordinate space. For auto vertical
        // insets, CSS 2.2 uses the hypothetical normal-flow static position;
        // preserve the temporary-flow y instead of clamping it to the outer
        // containing block top.
        // <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height>.
        self.block_static_position_y_offset = Some(0.0);
        if !has_non_inline_formatting_box(children) && formatting_box_has_inline_content(children) {
            // CSS 2.2 lays out inline-block contents as a separate formatting
            // context. When that context contains inline-level children, they
            // must form inline line boxes rather than being replayed as
            // independent blocks:
            // <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>.
            self.layout_anonymous_block(style, children, stylesheets, None);
        } else {
            self.layout_flow_root_child_boxes(children, stylesheets);
        }
        if has_auto_height(style)
            && let Some(float_bottom) = self.current_float_context_lowest_bottom()
        {
            self.cursor_y = self.cursor_y.min(float_bottom);
        }
        self.block_static_position_y_offset = previous_block_static_position_y_offset;
        self.pop_float_context();
        self.pop_page_name_scope_suppression();
        if establishes_positioning_containing_block {
            self.containing_blocks.pop();
        }
        let measured_content_height = (content_top - self.cursor_y).max(style.line_height);
        let requested_content_height =
            used_content_height_or_auto(style, measured_content_height, vertical_extras)
                .unwrap_or(measured_content_height);
        // CSS Sizing applies explicit `height` to the content box of the
        // atomic inline-block fragment; internal line/block contents may
        // overflow but do not increase the used height:
        // <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
        let content_height = constrain_height(style, requested_content_height, available_width);
        let border_box_height = content_height + vertical_extras;
        let border_box = PageTopRect::new(
            0.0,
            top,
            content_width + horizontal_extras,
            border_box_height,
        )
        .paint_clip();
        let border_bottom = border_box.y();
        let policy = StackingContextPolicy::for_atomic(style, PaintBand::Inline, border_box);
        let escaped_positioned_layers =
            if matches!(policy.child_layer_policy, ChildLayerPolicy::EscapeAll)
                && positioned_layer_start < self.positioned_layers.len()
            {
                // CSS 2.2 Appendix E treats inline-blocks as atomically
                // painted inline-level pseudo stacking contexts, but
                // positioned descendants still participate in the parent
                // stacking context rather than being captured by that pseudo
                // context:
                // <https://www.w3.org/TR/CSS22/zindex.html>.
                self.positioned_layers.split_off(positioned_layer_start)
            } else {
                Vec::new()
            };
        self.flush_positioned_layers_since(positioned_layer_start);
        let fragment = self
            .current_page
            .paint_fragment()
            .translated(PaintVector::new(0.0, -border_bottom));
        let escaped_positioned_layers = escaped_positioned_layers
            .into_iter()
            .map(|layer| layer.translated(PaintVector::new(0.0, -border_bottom)))
            .collect::<Vec<_>>();
        let escaped_positioned_layers = (!escaped_positioned_layers.is_empty())
            .then(|| escaped_positioned_layers.into_boxed_slice());
        // CSS 2.2 defines an inline-block baseline as the baseline of its
        // last in-flow line box, falling back to the bottom margin edge if no
        // such line exists:
        // <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>.
        let baseline_offset = fragment
            .last_line_y()
            .map(|line_y| (border_box_height - line_y).max(0.0))
            .unwrap_or(border_box_height);
        self.restore(snapshot);

        InlineAtom {
            content: InlineAtomContent::InlineFragment(fragment),
            style: style.clone(),
            escaped_positioned_layers,
            width: content_width + horizontal_extras + style.margin.left + style.margin.right,
            height: border_box_height + style.margin.top + style.margin.bottom,
            baseline_offset,
            baseline_shift,
            link_target,
            alt_text: None,
        }
    }

    /// Estimates max-content width contributed by consecutive floated children.
    ///
    /// CSS 2.2 places consecutive floats on the same line while space permits,
    /// and shrink-to-fit inline-block sizing uses max-content width as the
    /// unconstrained preferred width:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>.
    fn inline_block_float_row_max_content_width(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        let mut row_width = 0.0f32;
        let mut max_row_width = 0.0f32;
        for child in children {
            let Some((child_element, _, child_style, child_children)) = child.element_parts()
            else {
                continue;
            };
            if child_style.float == Float::None {
                row_width = 0.0;
                continue;
            }
            row_width += self.float_margin_box_width(
                child_element,
                child_style,
                stylesheets,
                available_width,
                Some(child_children),
            );
            max_row_width = max_row_width.max(row_width);
        }
        max_row_width
    }

    /// Lays out normalized children inside an atomic flow-root fragment.
    ///
    /// CSS 2.2 defines `inline-block` as an inline-level box whose contents
    /// establish a block formatting context, and floats are positioned in that
    /// current block formatting context instead of being replayed as ordinary
    /// block children:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    fn layout_flow_root_child_boxes(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
    ) {
        let mut float_run = self.float_run_state();
        for child in children {
            if let Some((child_element, child_signature, child_style, child_children)) =
                child.element_parts()
                && self.layout_floating_child(
                    child_element,
                    child_signature.clone(),
                    child_style,
                    Some(child_children),
                    stylesheets,
                    &mut float_run,
                )
            {
                continue;
            }
            self.flush_float_run(&mut float_run);
            self.layout_formatting_box(child, stylesheets);
        }
        self.flush_float_run(&mut float_run);
    }

    /// Return an inline-block text fast-path baseline from its last line box.
    ///
    /// CSS 2.2 defines the baseline of an `inline-block` with in-flow line
    /// boxes as the baseline of its last line box, not the bottom border edge.
    /// The bottom edge is only the fallback for inline-blocks without a
    /// suitable line box:
    /// <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>.
    fn inline_box_sequence_baseline_offset(
        &mut self,
        sequence: &inline_layout::InlineLineSequence,
        style: &ComputedStyle,
        borders: css::Edges,
    ) -> Option<f32> {
        if sequence.records.is_empty() {
            return None;
        }
        let preceding_line_height = sequence
            .records
            .iter()
            .take(sequence.records.len().saturating_sub(1))
            .map(|line| line.line_height)
            .sum::<f32>();
        Some(
            borders.top
                + style.padding.top
                + preceding_line_height
                + self.inline_box_text_line_layout_baseline_offset(style),
        )
    }

    /// Return a text line's CSS layout baseline offset from its line-box top.
    ///
    /// Inline layout aligns atomic inline boxes against the same baseline
    /// coordinate used for ordinary text fragments. PDF text emission applies
    /// a backend-specific rendered-baseline projection later, but that
    /// projection must not affect CSS line-box sizing:
    /// <https://www.w3.org/TR/css-inline-3/#line-box>.
    fn inline_box_text_line_layout_baseline_offset(&mut self, style: &ComputedStyle) -> f32 {
        let font_id = self.font_system.resolve_style(style);
        let line_height = self.font_system.line_height_for_font(font_id, style);
        let adjustment =
            self.font_system
                .font_ascent_baseline_adjustment(font_id, style, line_height);
        style.font_size - adjustment
    }
}

/// Remove preserved segment breaks at the end of a block container.
///
/// CSS Text preserves segment breaks for `pre`, `pre-wrap`, and
/// `break-spaces`, but the final segment break in a block container terminates
/// the current line rather than generating an extra empty line box. Trimming
/// only the terminal segment-break suffix here leaves interior authored line
/// breaks intact:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>.
fn trim_terminal_preserved_segment_breaks<'t>(text: &'t str, style: &ComputedStyle) -> &'t str {
    if !style.white_space.preserves_newlines() {
        return text;
    }
    let mut end = text.len();
    while let Some((break_offset, break_len)) = last_segment_break_before(&text[..end]) {
        let suffix = &text[break_offset + break_len..end];
        if !suffix.chars().all(is_css_collapsible_whitespace) {
            break;
        }
        end = break_offset;
        if text[..end].ends_with('\r') {
            end -= '\r'.len_utf8();
        }
    }
    &text[..end]
}

fn last_segment_break_before(text: &str) -> Option<(usize, usize)> {
    text.char_indices()
        .rev()
        .find(|(_, character)| matches!(*character, '\n' | '\r' | INLINE_BREAK))
        .map(|(offset, character)| (offset, character.len_utf8()))
}

/// Return whether a block container's own bidi value needs inline controls.
///
/// HTML's UA stylesheet sets `unicode-bidi: isolate` on many block containers,
/// but a block formatting context already separates its inline formatting
/// context from surrounding inline content. Literal UAX #9 controls are still
/// needed for block-level overrides and plaintext paragraph direction because
/// those values affect the inline content inside the block:
/// <https://html.spec.whatwg.org/multipage/rendering.html#bidi-rendering> and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(super) fn block_bidi_scope_needs_inline_controls(style: &ComputedStyle) -> bool {
    matches!(
        style.unicode_bidi,
        UnicodeBidi::Embed
            | UnicodeBidi::BidiOverride
            | UnicodeBidi::IsolateOverride
            | UnicodeBidi::Plaintext
    )
}

#[derive(Debug, Clone, Copy)]
enum InlineBoxEdge {
    Start,
    End,
}

#[derive(Debug, Clone, Copy)]
struct InlineElementScopeOptions {
    push_page_scope: bool,
    push_inside_marker: bool,
    mark_hanging_edges: bool,
    fragment_edges: box_tree::InlineBoxFragmentEdges,
}

impl InlineElementScopeOptions {
    const DOM_INTRINSIC: Self = Self {
        push_page_scope: false,
        push_inside_marker: false,
        mark_hanging_edges: true,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };
    const DOM_PAINT: Self = Self {
        push_page_scope: true,
        push_inside_marker: false,
        mark_hanging_edges: true,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };
    const BOX_PAINT: Self = Self {
        push_page_scope: true,
        push_inside_marker: true,
        mark_hanging_edges: true,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };
    const BOX_INTRINSIC: Self = Self {
        push_page_scope: false,
        push_inside_marker: false,
        mark_hanging_edges: true,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };

    fn with_fragment_edges(mut self, fragment_edges: box_tree::InlineBoxFragmentEdges) -> Self {
        self.fragment_edges = fragment_edges;
        self
    }
}

#[derive(Debug)]
struct InlineElementScopeState {
    inline_box_start: usize,
    link_target: Option<String>,
    baseline_shift: f32,
    pushed_page_scope: bool,
    mark_hanging_edges: bool,
    fragment_edges: box_tree::InlineBoxFragmentEdges,
    counter_scope: CounterScopeState,
}

/// Return the inline-axis contribution of one regular inline box edge.
///
/// CSS 2.2 says horizontal margin, border, and padding of inline boxes are
/// respected at the start and end of the inline box. The values may be
/// negative for margins, which WPT references use to emulate hanging
/// punctuation:
/// <https://www.w3.org/TR/CSS22/box.html#inline-boxes>.
fn inline_box_edge_width(style: &ComputedStyle, edge: InlineBoxEdge) -> f32 {
    let (margin, border, padding) = inline_box_edge_components(style, edge);
    margin + border + padding
}

fn inline_box_edge_has_nonzero_component(style: &ComputedStyle, edge: InlineBoxEdge) -> bool {
    let (margin, border, padding) = inline_box_edge_components(style, edge);
    margin.abs() > 0.001 || border.abs() > 0.001 || padding.abs() > 0.001
}

fn inline_box_edge_components(style: &ComputedStyle, edge: InlineBoxEdge) -> (f32, f32, f32) {
    let side = inline_box_edge_physical_side(style, edge);
    let borders = used_border_widths(style);
    match side {
        PhysicalSide::Top => (style.margin.top, borders.top, style.padding.top),
        PhysicalSide::Right => (style.margin.right, borders.right, style.padding.right),
        PhysicalSide::Bottom => (style.margin.bottom, borders.bottom, style.padding.bottom),
        PhysicalSide::Left => (style.margin.left, borders.left, style.padding.left),
    }
}

fn inline_box_edge_physical_side(style: &ComputedStyle, edge: InlineBoxEdge) -> PhysicalSide {
    match edge {
        InlineBoxEdge::Start => inline_start_side(style.writing_mode, style.direction),
        InlineBoxEdge::End => inline_end_side(style.writing_mode, style.direction),
    }
}

/// Mark the text items blocked by an inline box's edge decorations.
///
/// CSS Text disallows hanging punctuation when inline-start or inline-end
/// padding/border separates the glyph from the line edge. The text fragment
/// itself does not own ancestor inline-box border/padding, so inline
/// collection records that edge on the first/last visible text item:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
fn mark_inline_box_hanging_edges(
    output: &mut [InlineItem],
    inline_box_start: usize,
    style: &ComputedStyle,
    fragment_edges: box_tree::InlineBoxFragmentEdges,
) {
    let items = &mut output[inline_box_start..];
    let blocks_start = fragment_edges.owns_start && inline_box_blocks_hanging_start(style);
    let blocks_end = fragment_edges.owns_end && inline_box_blocks_hanging_end(style);
    let has_blocking_edge = blocks_start || blocks_end;
    let mut marked_visible_item = false;
    if blocks_start && let Some(word) = items.iter_mut().find_map(visible_hanging_edge_word_mut) {
        word.hanging_edges.blocks_start = true;
        marked_visible_item = true;
    }
    if blocks_end
        && let Some(word) = items
            .iter_mut()
            .rev()
            .find_map(visible_hanging_edge_word_mut)
    {
        word.hanging_edges.blocks_end = true;
        marked_visible_item = true;
    }
    if has_blocking_edge
        && !marked_visible_item
        && let Some(word) = output[..inline_box_start]
            .iter_mut()
            .rev()
            .find_map(visible_hanging_edge_word_mut)
    {
        word.hanging_edges.blocks_end = true;
    }
}

fn visible_hanging_edge_word_mut(item: &mut InlineItem) -> Option<&mut InlineWord> {
    let InlineItem::Word(word) = item else {
        return None;
    };
    let text = trim_css_collapsible_whitespace(&word.text);
    if text.is_empty() || text.chars().all(character_is_bidi_format_control) {
        return None;
    }
    Some(word)
}

fn inline_box_blocks_hanging_start(style: &ComputedStyle) -> bool {
    match style.direction {
        Direction::Ltr => style.padding.left != 0.0 || style.border_widths.left != 0.0,
        Direction::Rtl => style.padding.right != 0.0 || style.border_widths.right != 0.0,
    }
}

fn inline_box_blocks_hanging_end(style: &ComputedStyle) -> bool {
    match style.direction {
        Direction::Ltr => style.padding.right != 0.0 || style.border_widths.right != 0.0,
        Direction::Rtl => style.padding.left != 0.0 || style.border_widths.left != 0.0,
    }
}

/// Insert CSS Text Level 4 automatic spacing into inline text item streams.
///
/// `text-autospace` creates layout spacing between Han ideographs and adjacent
/// non-ideographic letters or numbers. The spacing is modeled as an atomic
/// inline edge so it affects line fitting and paint positions without adding
/// selectable text or synthetic glyphs to the PDF output:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>.
pub(in crate::layout) fn insert_text_autospace_items(items: &mut Vec<InlineItem>) {
    let mut output = Vec::with_capacity(items.len());
    let mut previous_text = None::<AutospaceTextEdge>;
    for item in std::mem::take(items) {
        match item {
            InlineItem::Word(word) => {
                push_autospaced_word(&mut output, *word, &mut previous_text);
            }
            InlineItem::Atom(atom) => {
                if !atom.content.is_inline_edge() || atom.width.abs() > 0.0 {
                    previous_text = None;
                }
                output.push(InlineItem::Atom(atom));
            }
            InlineItem::Float(float) => {
                previous_text = None;
                output.push(InlineItem::Float(float));
            }
            InlineItem::Break => {
                previous_text = None;
                output.push(InlineItem::Break);
            }
            InlineItem::PageScopeStart(scope) => output.push(InlineItem::PageScopeStart(scope)),
            InlineItem::PageScopeEnd => output.push(InlineItem::PageScopeEnd),
        }
    }
    *items = output;
}

fn push_autospaced_word(
    output: &mut Vec<InlineItem>,
    word: InlineWord,
    previous_text: &mut Option<AutospaceTextEdge>,
) {
    if word.style.text_autospace.is_none() {
        push_autospace_boundary(output, previous_text, &word);
        *previous_text = AutospaceTextEdge::from_word_end(&word);
        output.push(InlineItem::Word(Box::new(word)));
        return;
    }

    let mut run = String::new();
    let mut run_end = None::<char>;
    let mut run_start_index = 0usize;
    for (index, character) in word.text.char_indices() {
        if let Some(previous) = run_end
            && text_autospace_boundary_needs_spacing(
                &word.style.text_autospace,
                previous,
                character,
            )
        {
            push_autospaced_word_run(
                output,
                &word,
                &mut run,
                previous,
                run_start_index,
                previous_text,
            );
            push_text_autospace_atom(output, &word.style, word.baseline_shift);
            *previous_text = None;
            run_start_index = index;
        }
        run.push(character);
        run_end = Some(character);
    }
    if let Some(last_character) = run_end {
        push_autospaced_word_run(
            output,
            &word,
            &mut run,
            last_character,
            run_start_index,
            previous_text,
        );
    }
}

fn push_autospaced_word_run(
    output: &mut Vec<InlineItem>,
    word: &InlineWord,
    run: &mut String,
    last_character: char,
    run_start_index: usize,
    previous_text: &mut Option<AutospaceTextEdge>,
) {
    if run.is_empty() {
        return;
    }
    let run_word = InlineWord {
        text: std::mem::take(run),
        style: word.style.clone(),
        baseline_shift: word.baseline_shift,
        link_target: word.link_target.clone(),
        mergeable: word.mergeable && run_start_index == 0,
        source: word.source,
        hanging_edges: word.hanging_edges,
    };
    push_autospace_boundary(output, previous_text, &run_word);
    *previous_text = Some(AutospaceTextEdge {
        character: last_character,
        style: run_word.style.clone(),
        baseline_shift: run_word.baseline_shift,
    });
    output.push(InlineItem::Word(Box::new(run_word)));
}

fn push_autospace_boundary(
    output: &mut Vec<InlineItem>,
    previous_text: &mut Option<AutospaceTextEdge>,
    word: &InlineWord,
) {
    let Some(current_character) = word.text.chars().next() else {
        return;
    };
    if let Some(previous) = previous_text
        && text_autospace_boundary_needs_spacing(
            &previous.style.text_autospace,
            previous.character,
            current_character,
        )
        && text_autospace_boundary_needs_spacing(
            &word.style.text_autospace,
            previous.character,
            current_character,
        )
    {
        push_text_autospace_atom(output, &previous.style, previous.baseline_shift);
    }
}

fn push_text_autospace_atom(
    output: &mut Vec<InlineItem>,
    style: &ComputedStyle,
    baseline_shift: f32,
) {
    output.push(InlineItem::Atom(Box::new(InlineAtom {
        content: InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace),
        style: style.clone(),
        escaped_positioned_layers: None,
        width: style.font_size / 8.0,
        height: 0.0,
        baseline_offset: 0.0,
        baseline_shift,
        link_target: None,
        alt_text: None,
    })));
}

fn quote_pair(style: &ComputedStyle, depth: usize) -> (String, String) {
    match &style.quotes {
        Quotes::None => (String::new(), String::new()),
        Quotes::Pairs(pairs) => pairs
            .get(depth)
            .or_else(|| pairs.last())
            .cloned()
            .unwrap_or_else(default_quote_pair),
        Quotes::Auto { .. } => {
            let (open, close) = quotes::language_quote_pair(style.quotes.auto_language(), depth);
            (open.to_string(), close.to_string())
        }
    }
}

fn default_quote_pair() -> (String, String) {
    ("“".to_string(), "”".to_string())
}

fn text_autospace_boundary_needs_spacing(
    autospace: &TextAutospace,
    first: char,
    second: char,
) -> bool {
    if autospace.is_none() {
        return false;
    }
    let first_is_ideograph = character_is_autospace_ideograph(first);
    let second_is_ideograph = character_is_autospace_ideograph(second);
    if first_is_ideograph == second_is_ideograph {
        return false;
    }
    let other = if first_is_ideograph { second } else { first };
    (autospace.ideograph_alpha && character_is_autospace_alpha(other))
        || (autospace.ideograph_numeric && character_is_autospace_numeric(other))
}

#[derive(Clone)]
struct AutospaceTextEdge {
    character: char,
    style: ComputedStyle,
    baseline_shift: f32,
}

impl AutospaceTextEdge {
    fn from_word_end(word: &InlineWord) -> Option<Self> {
        word.text.chars().last().map(|character| Self {
            character,
            style: word.style.clone(),
            baseline_shift: word.baseline_shift,
        })
    }
}

/// Return whether an atomic inline box contains nested inline formatting boxes.
///
/// CSS 2.2 lays out inline-block contents as an independent formatting
/// context. When the contents include inline child boxes, preserving those
/// boxes is required so descendant styles participate in line construction:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>.
fn has_inline_container_formatting_box(children: &[box_tree::FormattingBox<'_>]) -> bool {
    children.iter().any(|child| match child {
        box_tree::FormattingBox::Inline(_) | box_tree::FormattingBox::Line(_) => true,
        box_tree::FormattingBox::Text(_) | box_tree::FormattingBox::Replaced(_) => false,
        _ => has_inline_container_formatting_box(child.children()),
    })
}

/// Return whether an atomic inline box contains positioned descendants.
///
/// CSS Positioned Layout removes absolutely positioned and fixed descendants
/// from normal flow, but they still paint in their containing stacking context.
/// Inline-block layout must therefore use the fragment-backed path whenever
/// such descendants exist, even if no in-flow child requires a block formatting
/// context:
/// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>.
fn has_out_of_flow_formatting_box(children: &[box_tree::FormattingBox<'_>]) -> bool {
    children.iter().any(|child| {
        box_tree::is_out_of_flow_box(child) || has_out_of_flow_formatting_box(child.children())
    })
}
