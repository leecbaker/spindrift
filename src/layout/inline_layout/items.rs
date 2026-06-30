use super::super::*;
use super::graph::InlineLineFragment;
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
        if inline_items_can_fragment_as_collected_lines(&items) {
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
                    line_index = self.flush_inline_item_paragraph(
                        &mut paragraph,
                        context,
                        line_index,
                        true,
                        next_paragraph_starts_after_forced_break,
                        &mut plaintext_direction_state,
                    );
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
                    cursor = self.collect_inline_paragraph_lines(
                        &mut paragraph,
                        context,
                        cursor,
                        true,
                        &mut records,
                    );
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
        InlineLineSequence {
            records,
            available_width: context.available_width,
            padding_left: context.padding_left,
            hanging_indent: context.hanging_indent,
            hanging_punctuation_reserve: context.hanging_punctuation_reserve,
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
            for line in &sequence.records[painted..painted + fragment_count] {
                stack.apply(self);
                self.paint_collected_inline_line(line, context, plaintext_direction_state);
                stack.advance(line.height());
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
            for line in &sequence.records[painted..painted + fragment_count] {
                stack.apply(self);
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
                self.paint_collected_inline_line(line, context, &mut plaintext_direction_state);
                stack.advance(line.height());
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
        let saved_cursor_y = self.cursor_y;
        let saved_left = self.content_left;
        let saved_right = self.content_right;
        let mut stack = InlineLineStackCursor::new(block_style, saved_left, saved_right, block_top);
        let mut plaintext_direction_state = None;
        let context = sequence.context(block_style);
        for line in &sequence.records {
            let line_top = stack.cursor_y;
            let line_bottom = line_top - line.height();
            if line_top >= slice_bottom && line_bottom <= slice_top {
                stack.apply(self);
                self.paint_collected_inline_line(line, context, &mut plaintext_direction_state);
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
        for line in &sequence.records {
            stack.apply(self);
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
    ) {
        let line_height = line.height();
        let Some(_) = &line.fragment else {
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
        if let Some(prepared) =
            self.prepare_inline_line_record(&paint_line, paint_context, plaintext_direction_state)
        {
            self.paint_prepared_inline_line(&prepared);
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
        let padding_left = context.padding_left;
        let line_text = line_box.text.clone();
        let text_align = text_align_for_inline_line_text_with_state(
            block_style,
            line.is_last_line_in_paragraph,
            &line_text,
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
        let line_items = self.visual_ordered_mixed_inline_line_items(&line_box.items, block_style);
        let paint_fragment = InlineLineFragment {
            items: line_items,
            metrics,
            hanging_widths,
            indent: line_box.indent,
            available_width: line.available_width,
            suppress_float_adjust: line_box.suppress_float_adjust,
            text: line_text,
        };
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
}

#[derive(Debug, Clone, Copy)]
struct InlineLineSequenceCursor {
    paragraph_index: usize,
    line_index: usize,
    starts_after_forced_break: bool,
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
    fn context<'a>(&self, block_style: &'a ComputedStyle) -> InlineParagraphContext<'a> {
        InlineParagraphContext {
            block_style,
            stylesheets: &[],
            available_width: self.available_width,
            padding_left: self.padding_left,
            hanging_indent: self.hanging_indent,
            hanging_punctuation_reserve: self.hanging_punctuation_reserve,
        }
    }

    #[allow(dead_code)]
    pub(in crate::layout) fn total_height(&self) -> f32 {
        self.records.iter().map(InlineLineRecord::height).sum()
    }

    pub(in crate::layout) fn line_count(&self) -> usize {
        self.records.len()
    }

    #[allow(dead_code)]
    pub(in crate::layout) fn forced_empty_line_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| record.is_forced_empty)
            .count()
    }

    pub(in crate::layout) fn line_height(&self, index: usize) -> f32 {
        self.records
            .get(index)
            .map(InlineLineRecord::height)
            .unwrap_or(0.0)
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

        let mut used_height = 0.0;
        let mut fitting = 0;
        for index in start_index..self.records.len() {
            let height = self.line_height(index);
            if used_height + height > available_height + 0.01 {
                break;
            }
            used_height += height;
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
#[allow(dead_code)]
pub(in crate::layout) struct InlineLineRecord {
    pub(in crate::layout) paragraph_index: usize,
    pub(in crate::layout) block_line_index: usize,
    pub(in crate::layout) paragraph_line_index: usize,
    pub(in crate::layout) fragment: Option<InlineLineFragment>,
    pub(in crate::layout) is_first_formatted_line: bool,
    pub(in crate::layout) is_last_line_in_paragraph: bool,
    pub(in crate::layout) is_forced_empty: bool,
    pub(in crate::layout) paragraph_last_hanging_width: f32,
    pub(in crate::layout) used_indent: f32,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) line_height: f32,
}

impl InlineLineRecord {
    fn height(&self) -> f32 {
        self.line_height
    }
}

fn inline_items_can_fragment_as_collected_lines(items: &[InlineItem]) -> bool {
    items.iter().any(|item| matches!(item, InlineItem::Break))
        && items.iter().all(|item| {
            !matches!(
                item,
                InlineItem::Float(_) | InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd
            )
        })
}
