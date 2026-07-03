use super::super::*;
use super::graph::InlineLineFragment;
use super::mixed::InlineTextBoxMetrics;
use crate::css::{BoxDecorationBreak, TextBoxTrim, TextEdgeMetric};
use crate::layout::inline_collect::{
    insert_text_autospace_items, normalize_inline_whitespace_items,
};

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_inline_items(
        &mut self,
        mut items: Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        hanging_indent: f32,
        stylesheets: &[Stylesheet],
    ) {
        normalize_inline_whitespace_items(&mut items);
        insert_text_autospace_items(&mut items);
        trim_inline_item_edges(&mut items);
        let context = InlineParagraphContext {
            block_style,
            stylesheets,
            available_width,
            padding_left,
            hanging_indent,
            hanging_punctuation_reserve: last_hanging_punctuation_width_for_inline_items(
                &mut self.font_system,
                &items,
                block_style,
            ),
        };
        if !self.current_text_box_line_trim().is_empty()
            || inline_items_can_fragment_as_collected_lines(&items)
        {
            let sequence = self.collect_inline_line_sequence_for_items(&items, context);
            self.paint_inline_line_sequence(&sequence, block_style);
            return;
        }
        let mut paragraph = Vec::new();
        let mut line_index = 0usize;
        let mut next_paragraph_starts_after_forced_break = false;
        let mut page_scopes = Vec::new();
        let mut plaintext_direction_state = None;
        for item in items {
            match inline_item_boundary_role(&item) {
                InlineBoundaryRole::ForcedBreak => {
                    let clear = inline_break_clear(&item);
                    let force_empty_line = clear == Clear::None;
                    line_index = self.flush_inline_item_paragraph(
                        &mut paragraph,
                        context,
                        line_index,
                        force_empty_line,
                        next_paragraph_starts_after_forced_break,
                        &mut plaintext_direction_state,
                    );
                    line_index = self.apply_inline_break_clearance(clear, context, line_index);
                    next_paragraph_starts_after_forced_break = true;
                }
                InlineBoundaryRole::PageScopeStart => {
                    let InlineItem::PageScopeStart(page_name) = item else {
                        unreachable!("page-scope boundary role must come from PageScopeStart")
                    };
                    trim_inline_item_edges(&mut paragraph);
                    let flushed_paragraph = !paragraph.is_empty();
                    line_index = self.flush_inline_item_paragraph(
                        &mut paragraph,
                        context,
                        line_index,
                        false,
                        next_paragraph_starts_after_forced_break,
                        &mut plaintext_direction_state,
                    );
                    if flushed_paragraph {
                        next_paragraph_starts_after_forced_break = false;
                    }
                    page_scopes.push(self.enter_inline_page_name_scope(page_name.as_deref()));
                }
                InlineBoundaryRole::PageScopeEnd => {
                    trim_inline_item_edges(&mut paragraph);
                    let flushed_paragraph = !paragraph.is_empty();
                    line_index = self.flush_inline_item_paragraph(
                        &mut paragraph,
                        context,
                        line_index,
                        false,
                        next_paragraph_starts_after_forced_break,
                        &mut plaintext_direction_state,
                    );
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
        let _ = self.flush_inline_item_paragraph(
            &mut paragraph,
            context,
            line_index,
            false,
            next_paragraph_starts_after_forced_break,
            &mut plaintext_direction_state,
        );
        while let Some(scope) = page_scopes.pop() {
            self.exit_inline_page_name_scope(scope);
        }
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
        normalize_inline_whitespace_items(&mut items);
        insert_text_autospace_items(&mut items);
        trim_inline_item_edges(&mut items);
        let context = InlineParagraphContext {
            block_style,
            stylesheets: &[],
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

    fn collect_inline_line_sequence_for_items(
        &mut self,
        items: &[InlineItem],
        context: InlineParagraphContext<'_>,
    ) -> InlineLineSequence {
        let mut records = Vec::new();
        let mut paragraph = Vec::new();
        let mut cursor = InlineLineSequenceCursor {
            paragraph_index: 0,
            line_index: 0,
            starts_after_forced_break: false,
        };
        for item in items {
            match inline_item_boundary_role(item) {
                InlineBoundaryRole::ForcedBreak => {
                    let clear = inline_break_clear(item);
                    let force_empty_line = clear == Clear::None;
                    let record_count_before_break = records.len();
                    cursor = self.collect_inline_paragraph_lines(
                        &mut paragraph,
                        context,
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
                }
                role if role == InlineBoundaryRole::Float || role.is_page_scope() => {
                    let next_cursor = self.collect_inline_paragraph_lines(
                        &mut paragraph,
                        context,
                        cursor,
                        false,
                        &mut records,
                    );
                    if next_cursor.line_index != cursor.line_index {
                        cursor = next_cursor;
                        cursor.paragraph_index += 1;
                        cursor.starts_after_forced_break = false;
                    }
                }
                _ => paragraph.push(item),
            }
        }
        let _ = self.collect_inline_paragraph_lines(
            &mut paragraph,
            context,
            cursor,
            false,
            &mut records,
        );
        let (records, fragment_text_box_trim) =
            self.with_text_box_line_trim_applied(records, context.block_style);
        InlineLineSequence {
            records,
            available_width: context.available_width,
            padding_left: context.padding_left,
            hanging_indent: context.hanging_indent,
            hanging_punctuation_reserve: context.hanging_punctuation_reserve,
            fragment_text_box_trim,
        }
    }

    pub(in crate::layout) fn current_text_box_line_trim(&self) -> TextBoxLineTrim {
        self.text_box_line_trim_stack
            .last()
            .copied()
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
        let metrics = self.inline_text_box_metrics(style, None, 0.0);
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
        let content_over_edge = metrics.half_leading;
        let content_under_edge = metrics.half_leading + metrics.content_block_size;
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
            TextEdgeMetric::Text | TextEdgeMetric::Ideographic => metrics.half_leading,
            TextEdgeMetric::IdeographicInk => self
                .font_system
                .ideographic_ink_extents_for_style(style)
                .map(|(above, _)| metrics.line_baseline_offset - above)
                .unwrap_or(metrics.half_leading),
            TextEdgeMetric::Cap => {
                let cap_height = self.font_system.used_cap_height_for_style(style);
                metrics.line_baseline_offset - cap_height
            }
            TextEdgeMetric::Ex => {
                let x_height = self.font_system.used_x_height_for_style(style);
                metrics.line_baseline_offset - x_height
            }
            TextEdgeMetric::Alphabetic => metrics.half_leading,
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
                metrics.half_leading + metrics.content_block_size
            }
            TextEdgeMetric::IdeographicInk => self
                .font_system
                .ideographic_ink_extents_for_style(style)
                .map(|(_, below)| metrics.line_baseline_offset + below)
                .unwrap_or(metrics.half_leading + metrics.content_block_size),
            TextEdgeMetric::Alphabetic => metrics.line_baseline_offset,
            TextEdgeMetric::Cap | TextEdgeMetric::Ex => {
                metrics.half_leading + metrics.content_block_size
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
            && let Some(record) = records.iter_mut().find(|record| record.fragment.is_some())
        {
            record.block_start_trim = trim.block_start;
        }
        if trim.trims_block_end
            && trim.block_end > 0.0
            && let Some(record) = records
                .iter_mut()
                .rev()
                .find(|record| record.fragment.is_some())
        {
            record.block_end_trim = trim.block_end;
        }
        (records, TextBoxLineTrim::default())
    }

    fn apply_line_block_start_trim_for_paint(
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
            WritingMode::VerticalRl => {
                self.content_left += line.block_start_trim;
                self.content_right += line.block_start_trim;
            }
            WritingMode::VerticalLr => {
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
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let mut painted = 0usize;
        while painted < sequence.records.len() {
            let mut fragment_count = sequence.fitting_line_count(
                painted,
                self.cursor_y - self.page_bottom(),
                self.cursor_is_at_page_top(),
                block_style.orphans,
                block_style.widows,
            );
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
                stack.apply(self);
                self.apply_line_block_start_trim_for_paint(line, block_style.writing_mode);
                self.paint_collected_inline_line(line, context, plaintext_direction_state, None);
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

    pub(in crate::layout) fn paint_inline_line_sequence_multicolumn(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        column_count: usize,
        column_gap: f32,
        column_width: f32,
        column_height: f32,
    ) {
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let column_block_top = self.cursor_y;
        let mut painted = 0usize;
        let mut plaintext_direction_state = None;
        let context = sequence.context(block_style);

        for column_index in 0..column_count {
            if painted >= sequence.records.len() {
                break;
            }
            let column_left =
                saved_content_left + (column_width + column_gap) * column_index as f32;
            self.content_left = column_left;
            self.content_right = column_left + column_width;
            self.cursor_y = column_block_top;
            let fragment_count = sequence.fitting_line_count(
                painted,
                column_height,
                true,
                block_style.orphans,
                block_style.widows,
            );
            if fragment_count == 0 {
                continue;
            }
            let mut stack = InlineLineStackCursor::new(
                block_style,
                self.content_left,
                self.content_right,
                self.cursor_y,
            );
            let fragment_records = sequence.fragment_records_for_paint(painted, fragment_count);
            for line in &fragment_records {
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
            painted += fragment_count;
        }

        let column_block_bottom = column_block_top - column_height;
        for primitive in multicol_gap_decoration_primitives(
            block_style,
            saved_content_left,
            column_block_top,
            column_block_bottom,
            column_width,
            column_gap,
            column_count,
        ) {
            self.push_primitive_in_band(PaintBand::BackgroundBorder, primitive);
        }
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
        self.cursor_y = column_block_bottom;
    }

    pub(in crate::layout) fn paint_inline_line_sequence_with_outside_marker(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        marker: &ListMarker,
        content_inline_start: f32,
        content_inline_end: f32,
    ) {
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let mut plaintext_direction_state = None;
        let mut painted = 0usize;
        let mut marker_painted = false;
        while painted < sequence.records.len() {
            let mut fragment_count = sequence.fitting_line_count(
                painted,
                self.cursor_y - self.page_bottom(),
                self.cursor_is_at_page_top(),
                block_style.orphans,
                block_style.widows,
            );
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
                stack.apply(self);
                self.apply_line_block_start_trim_for_paint(line, block_style.writing_mode);
                if !marker_painted {
                    self.paint_outside_marker(
                        marker,
                        block_style,
                        content_inline_start,
                        content_inline_end,
                        self.cursor_y,
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
            block_top,
            slice_top,
            slice_bottom,
            None,
        );
    }

    pub(in crate::layout) fn paint_inline_line_sequence_slice_with_text_source(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        block_top: f32,
        slice_top: f32,
        slice_bottom: f32,
        text_source: RenderedLineSource,
    ) {
        self.paint_inline_line_sequence_slice_inner(
            sequence,
            block_style,
            block_top,
            slice_top,
            slice_bottom,
            Some(text_source),
        );
    }

    fn paint_inline_line_sequence_slice_inner(
        &mut self,
        sequence: &InlineLineSequence,
        block_style: &ComputedStyle,
        block_top: f32,
        slice_top: f32,
        slice_bottom: f32,
        text_source: Option<RenderedLineSource>,
    ) {
        let saved_cursor_y = self.cursor_y;
        let saved_left = self.content_left;
        let saved_right = self.content_right;
        let mut plaintext_direction_state = None;
        let context = sequence.context(block_style);
        let (fragment_block_top, fragment_records) =
            sequence.fragment_records_for_slice_paint(block_top, slice_top, slice_bottom);
        let mut stack =
            InlineLineStackCursor::new(block_style, saved_left, saved_right, fragment_block_top);
        for line in &fragment_records {
            let line_top = stack.cursor_y;
            let line_bottom = line_top - line.height();
            if line_top >= slice_bottom && line_bottom <= slice_top {
                stack.apply(self);
                self.apply_line_block_start_trim_for_paint(line, block_style.writing_mode);
                self.paint_collected_inline_line(
                    line,
                    context,
                    &mut plaintext_direction_state,
                    text_source,
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
        paragraph: &mut Vec<&InlineItem>,
        context: InlineParagraphContext<'_>,
        cursor: InlineLineSequenceCursor,
        force_empty_line: bool,
        output: &mut Vec<InlineLineRecord>,
    ) -> InlineLineSequenceCursor {
        let paragraph_index = cursor.paragraph_index;
        let line_index = cursor.line_index;
        let starts_after_forced_break = cursor.starts_after_forced_break;
        trim_inline_item_edges(paragraph);
        if paragraph.is_empty() {
            if force_empty_line {
                output.push(InlineLineRecord {
                    paragraph_index,
                    block_line_index: line_index,
                    paragraph_line_index: 0,
                    fragment: None,
                    is_first_formatted_line: line_index == 0,
                    is_last_line_in_paragraph: true,
                    is_forced_empty: true,
                    clear_after: Clear::None,
                    block_start_trim: 0.0,
                    block_end_trim: 0.0,
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
                return InlineLineSequenceCursor {
                    line_index: line_index + 1,
                    ..cursor
                };
            }
            return cursor;
        }

        let paragraph_start_line_index = line_index;
        let graph =
            self.build_inline_opportunity_graph(paragraph.iter().copied(), context.block_style);
        let (line_boxes, next_line_index) = self.select_inline_lines_from_graph(
            &graph,
            context,
            line_index,
            starts_after_forced_break,
        );
        let paragraph_last_hanging_width = line_boxes
            .last()
            .map(|line_box| {
                last_hanging_punctuation_width_for_line_items(
                    &mut self.font_system,
                    &line_box.items,
                    context.block_style,
                )
            })
            .unwrap_or(0.0);
        let line_count = line_boxes.len();
        for (offset, line_box) in line_boxes.into_iter().enumerate() {
            let line_box_index = paragraph_start_line_index + offset;
            let line_height = line_box.metrics.height.max(context.block_style.line_height);
            let used_indent = line_box.indent;
            let available_width = line_box.available_width;
            output.push(InlineLineRecord {
                paragraph_index,
                block_line_index: line_box_index,
                paragraph_line_index: offset,
                fragment: Some(line_box),
                is_first_formatted_line: line_box_index == 0,
                is_last_line_in_paragraph: offset + 1 == line_count,
                is_forced_empty: false,
                clear_after: Clear::None,
                block_start_trim: 0.0,
                block_end_trim: 0.0,
                paragraph_last_hanging_width,
                used_indent,
                available_width,
                line_height,
            });
        }
        paragraph.clear();
        InlineLineSequenceCursor {
            line_index: next_line_index,
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
        let line_height = line.height();
        let Some(_) = &line.fragment else {
            self.record_in_flow_line_baseline(line, context.block_style);
            return;
        };
        if self.cursor_y - line_height < self.page_bottom()
            && self.out_of_flow_prebreak_suppression_depth == 0
        {
            self.push_page();
        }
        let mut paint_context = context;
        let mut paint_line = line.clone();
        let suppress_float_adjust = line
            .fragment
            .as_ref()
            .is_some_and(|fragment| fragment.suppress_float_adjust);
        if !suppress_float_adjust {
            if context.block_style.writing_mode == WritingMode::HorizontalTb {
                let band = self.current_float_band(self.cursor_y, line_height);
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
                    context.block_style.direction,
                    self.content_left + context.padding_left,
                    line_height,
                    self.cursor_y,
                    context.available_width,
                );
                if band.inline_start() > 0.0 || band.inline_end() < context.available_width - 0.01 {
                    paint_line.available_width = line.available_width.min(band.inline_end());
                    paint_line.used_indent = line.used_indent.max(band.inline_start());
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
        let baseline_offset = if let Some(fragment) = &line.fragment {
            fragment.metrics.baseline_offset
        } else if line.is_forced_empty {
            self.inline_box_text_line_layout_baseline_offset(block_style)
        } else {
            return;
        };
        self.last_in_flow_line_baseline_y = Some(self.cursor_y - baseline_offset);
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
        let padding_left = context.padding_left;
        let line_text = line_box.text();
        let text_align = text_align_for_inline_line_text_with_state(
            block_style,
            line.is_last_line_in_paragraph,
            line_text,
            plaintext_direction_state,
        );
        let line_direction = if block_style.unicode_bidi == UnicodeBidi::Plaintext {
            (*plaintext_direction_state).unwrap_or(block_style.direction)
        } else {
            block_style.direction
        };
        let mut metrics = line_box.metrics;
        let line_available_width = (line.available_width - line.used_indent).max(1.0);
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
        let line_items = self.visual_ordered_mixed_inline_line_items(line_box.items(), block_style);
        let paint_fragment = InlineLineFragment::new(
            line_items,
            metrics,
            hanging_widths,
            line_box.indent,
            line.available_width,
            line_box.suppress_float_adjust,
            line_text,
        );
        self.prepare_inline_line_fragment(
            &paint_fragment,
            InlinePaintContext {
                block_style,
                direction: line_direction,
                available_width: line.available_width,
                padding_left,
                line_indent: line.used_indent,
                text_align,
                is_first_line: line.is_first_formatted_line,
            },
        )
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
    ) -> usize {
        trim_inline_item_edges(paragraph);
        if paragraph.is_empty() {
            if force_empty_line {
                if self.cursor_y - context.block_style.line_height < self.page_bottom() {
                    self.push_page();
                }
                self.cursor_y -= context.block_style.line_height;
                return line_index + 1;
            }
            return line_index;
        }
        let next_line_index = self.layout_inline_paragraph(
            paragraph,
            context,
            line_index,
            starts_after_forced_break,
            plaintext_direction_state,
        );
        paragraph.clear();
        next_line_index
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
        let before_clear_page_index = self.pages.len();
        let before_clear_top = self.cursor_y;
        let cleared_top = self.clear_active_floats_top(
            clear,
            context.block_style.writing_mode,
            context.block_style.direction,
            self.cursor_y,
        );
        let clearance_moved =
            self.pages.len() != before_clear_page_index || cleared_top < before_clear_top - 0.01;
        if clearance_moved {
            self.applied_clearance_count += 1;
        }
        self.cursor_y = if clearance_moved {
            cleared_top - 0.01
        } else {
            cleared_top
        };
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
}

#[derive(Debug, Clone, Copy)]
struct InlineLineSequenceCursor {
    paragraph_index: usize,
    line_index: usize,
    starts_after_forced_break: bool,
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
        is_first_formatted_line: false,
        is_last_line_in_paragraph: true,
        is_forced_empty: false,
        clear_after: clear,
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
    }
}

#[derive(Debug, Clone, Copy)]
struct InlineLineStackCursor {
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
    fn new(
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

    fn apply(self, layout: &mut LayoutBuilder<'_>) {
        layout.content_left = self.content_left;
        layout.content_right = self.content_right;
        layout.cursor_y = self.cursor_y;
    }

    fn advance(&mut self, line_block_size: f32) {
        match self.writing_mode {
            WritingMode::HorizontalTb => {
                self.cursor_y -= line_block_size;
            }
            WritingMode::VerticalRl => {
                self.content_left -= line_block_size;
                self.content_right -= line_block_size;
            }
            WritingMode::VerticalLr => {
                self.content_left += line_block_size;
                self.content_right += line_block_size;
            }
        }
    }
}

impl InlineLineSequence {
    pub(in crate::layout) fn context<'a>(
        &self,
        block_style: &'a ComputedStyle,
    ) -> InlineParagraphContext<'a> {
        InlineParagraphContext {
            block_style,
            stylesheets: &[],
            available_width: self.available_width,
            padding_left: self.padding_left,
            hanging_indent: self.hanging_indent,
            hanging_punctuation_reserve: self.hanging_punctuation_reserve,
        }
    }

    pub(in crate::layout) fn total_height(&self) -> f32 {
        self.fragment_height(0, self.records.len())
    }

    pub(in crate::layout) fn line_count(&self) -> usize {
        self.records.len()
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
        if self.records.is_empty() {
            return 0.0;
        }
        if column_count <= 1 {
            return self.total_height();
        }

        let mut previous_candidate = 0.0;
        for count in 1..=self.records.len() {
            let candidate = self.fragment_height(0, count);
            if candidate <= previous_candidate + 0.01 {
                continue;
            }
            previous_candidate = candidate;
            if self.multicolumn_height_paints_all_lines(
                candidate,
                column_count,
                block_style.orphans,
                block_style.widows,
            ) {
                return candidate;
            }
        }

        self.total_height()
    }

    pub(in crate::layout) fn fragment_records_for_paint(
        &self,
        start_index: usize,
        count: usize,
    ) -> Vec<InlineLineRecord> {
        let end_index = start_index.saturating_add(count).min(self.records.len());
        let mut records = self.records[start_index..end_index].to_vec();
        self.apply_fragment_text_box_trim_to_records(&mut records);
        records
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
        let mut line_top = block_top;
        for (index, record) in self.records.iter().enumerate() {
            let line_bottom = line_top - record.height();
            if line_top >= slice_bottom && line_bottom <= slice_top {
                return Some((index, line_top));
            }
            line_top = line_bottom;
        }
        None
    }

    fn uncloned_slice_visible_line_count(
        &self,
        start_index: usize,
        start_block_top: f32,
        slice_bottom: f32,
    ) -> usize {
        let mut line_top = start_block_top;
        let mut count = 0;
        for record in &self.records[start_index..] {
            let line_bottom = line_top - record.height();
            if line_top < slice_bottom || line_bottom > start_block_top {
                break;
            }
            count += 1;
            if line_bottom <= slice_bottom {
                break;
            }
            line_top = line_bottom;
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
        line.height()
    }

    fn multicolumn_height_paints_all_lines(
        &self,
        column_height: f32,
        column_count: usize,
        orphans: usize,
        widows: usize,
    ) -> bool {
        let mut painted = 0usize;
        for _ in 0..column_count {
            if painted >= self.records.len() {
                return true;
            }
            let fragment_count =
                self.fitting_line_count(painted, column_height, true, orphans, widows);
            if fragment_count == 0 {
                return false;
            }
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
            && let Some(record) = records.iter_mut().find(|record| record.fragment.is_some())
        {
            record.block_start_trim = self.fragment_text_box_trim.block_start;
        }
        if self.fragment_text_box_trim.trims_block_end
            && self.fragment_text_box_trim.block_end > 0.0
            && let Some(record) = records
                .iter_mut()
                .rev()
                .find(|record| record.fragment.is_some())
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
                        .is_some_and(|record| record.fragment.is_some())
                })
            })
            .flatten();
        let block_end_index = (self.fragment_text_box_trim.trims_block_end
            && self.fragment_text_box_trim.block_end > 0.0)
            .then(|| {
                (start_index..end_index).rev().find(|index| {
                    self.records
                        .get(*index)
                        .is_some_and(|record| record.fragment.is_some())
                })
            })
            .flatten();
        Some((block_start_index, block_end_index))
    }

    /// Return the number of selected line records that fit in a fragmentainer.
    ///
    /// The calculation is pure over the selected sequence so normal pagination,
    /// table-cell slicing, and inline-block slice painting share the same line
    /// heights and CSS Fragmentation widows/orphans decisions:
    /// <https://www.w3.org/TR/css-break-3/#widows-orphans>.
    ///
    pub(in crate::layout) fn fitting_line_count(
        &self,
        start_index: usize,
        available_height: f32,
        at_fragmentainer_start: bool,
        orphans: usize,
        widows: usize,
    ) -> usize {
        let remaining_total = self.records.len().saturating_sub(start_index);
        if remaining_total == 0 {
            return 0;
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
            return usize::from(at_fragmentainer_start);
        }
        if fitting >= remaining_total {
            return remaining_total;
        }

        // CSS Fragmentation 3 defines `orphans` and `widows` as constraints on
        // unforced line breaks inside a block container.
        // https://www.w3.org/TR/css-break-3/#widows-orphans
        let orphans = orphans.min(remaining_total).max(1);
        let widows = widows.min(remaining_total).max(1);
        if fitting < orphans && !at_fragmentainer_start {
            return 0;
        }

        let remaining_after_break = remaining_total - fitting;
        if remaining_after_break < widows && fitting > orphans {
            return (remaining_total - widows).max(orphans);
        }

        fitting
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
    pub(in crate::layout) is_first_formatted_line: bool,
    pub(in crate::layout) is_last_line_in_paragraph: bool,
    pub(in crate::layout) is_forced_empty: bool,
    pub(in crate::layout) clear_after: Clear,
    pub(in crate::layout) block_start_trim: f32,
    pub(in crate::layout) block_end_trim: f32,
    pub(in crate::layout) paragraph_last_hanging_width: f32,
    pub(in crate::layout) used_indent: f32,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) line_height: f32,
}

impl InlineLineRecord {
    pub(in crate::layout) fn height(&self) -> f32 {
        (self.line_height - self.block_start_trim - self.block_end_trim).max(0.0)
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
                InlineItem::Float(_) | InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd
            )
        })
}

fn inline_break_clear(item: &InlineItem) -> Clear {
    match item {
        InlineItem::Break(break_) => break_.clear,
        _ => Clear::None,
    }
}
