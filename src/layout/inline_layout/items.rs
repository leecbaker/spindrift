use super::super::*;
use super::graph::{InlineLineFragment, measured_inline_items};
use crate::layout::inline_collect::insert_text_autospace_items;

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
        insert_text_autospace_items(&mut items);
        trim_inline_item_edges(&mut items);
        let context = InlineParagraphContext {
            block_style,
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
            let plan = self.collect_inline_fragmentation_plan_for_items(&items, context);
            self.paint_inline_fragmentation_plan(&plan, block_style);
            return;
        }
        let mut paragraph = Vec::new();
        let mut line_index = 0usize;
        let mut next_paragraph_starts_after_forced_break = false;
        let mut page_scopes = Vec::new();
        let mut plaintext_direction_state = None;
        for item in items {
            match item {
                InlineItem::Break => {
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
                InlineItem::PageScopeStart(page_name) => {
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
                InlineItem::PageScopeEnd => {
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
                InlineItem::Float(float) => {
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
                    let mut run = self.float_run_state();
                    self.layout_floating_child(
                        &float.element,
                        float.signature.clone(),
                        &float.style,
                        None,
                        stylesheets,
                        &mut run,
                    );
                }
                item => paragraph.push(item),
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
    pub(in crate::layout) fn collect_inline_fragmentation_plan(
        &mut self,
        mut items: Vec<InlineItem>,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        hanging_indent: f32,
    ) -> InlineFragmentationPlan {
        insert_text_autospace_items(&mut items);
        trim_inline_item_edges(&mut items);
        let context = InlineParagraphContext {
            block_style,
            available_width,
            padding_left,
            hanging_indent,
            hanging_punctuation_reserve: last_hanging_punctuation_width_for_inline_items(
                &mut self.font_system,
                &items,
                block_style,
            ),
        };
        self.collect_inline_fragmentation_plan_for_items(&items, context)
    }

    fn collect_inline_fragmentation_plan_for_items(
        &mut self,
        items: &[InlineItem],
        context: InlineParagraphContext<'_>,
    ) -> InlineFragmentationPlan {
        let mut lines = Vec::new();
        let mut paragraph = Vec::new();
        let mut line_index = 0usize;
        let mut next_paragraph_starts_after_forced_break = false;
        for item in items {
            match item {
                InlineItem::Break => {
                    line_index = self.collect_inline_paragraph_lines(
                        &mut paragraph,
                        context,
                        line_index,
                        true,
                        next_paragraph_starts_after_forced_break,
                        &mut lines,
                    );
                    next_paragraph_starts_after_forced_break = true;
                }
                item => paragraph.push(item.clone()),
            }
        }
        let _ = self.collect_inline_paragraph_lines(
            &mut paragraph,
            context,
            line_index,
            false,
            next_paragraph_starts_after_forced_break,
            &mut lines,
        );
        InlineFragmentationPlan {
            lines,
            available_width: context.available_width,
            padding_left: context.padding_left,
            hanging_indent: context.hanging_indent,
            hanging_punctuation_reserve: context.hanging_punctuation_reserve,
        }
    }

    pub(in crate::layout) fn paint_inline_fragmentation_plan(
        &mut self,
        plan: &InlineFragmentationPlan,
        block_style: &ComputedStyle,
    ) {
        let mut plaintext_direction_state = None;
        let mut painted = 0usize;
        while painted < plan.lines.len() {
            let fragment_count =
                self.fragment_line_count(plan.lines.len(), painted, block_style, |index| {
                    plan.lines[index].height(block_style)
                });
            if fragment_count == 0 {
                self.push_page();
                continue;
            }
            let context = plan.context(block_style);
            for line in &plan.lines[painted..painted + fragment_count] {
                self.paint_collected_inline_line(line, context, &mut plaintext_direction_state);
            }
            painted += fragment_count;
            if painted < plan.lines.len() {
                self.push_page();
            }
        }
    }

    pub(in crate::layout) fn paint_inline_fragmentation_plan_with_outside_marker(
        &mut self,
        plan: &InlineFragmentationPlan,
        block_style: &ComputedStyle,
        marker: &ListMarker,
        content_inline_start: f32,
        content_inline_end: f32,
    ) {
        let mut plaintext_direction_state = None;
        let mut painted = 0usize;
        let mut marker_painted = false;
        while painted < plan.lines.len() {
            let fragment_count =
                self.fragment_line_count(plan.lines.len(), painted, block_style, |index| {
                    plan.lines[index].height(block_style)
                });
            if fragment_count == 0 {
                self.push_page();
                continue;
            }
            let context = plan.context(block_style);
            for line in &plan.lines[painted..painted + fragment_count] {
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
            }
            painted += fragment_count;
            if painted < plan.lines.len() {
                self.push_page();
            }
        }
    }

    pub(in crate::layout) fn paint_inline_fragmentation_plan_slice(
        &mut self,
        plan: &InlineFragmentationPlan,
        block_style: &ComputedStyle,
        block_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) {
        let saved_cursor_y = self.cursor_y;
        let mut line_top = block_top;
        let mut plaintext_direction_state = None;
        let context = plan.context(block_style);
        for line in &plan.lines {
            let line_height = line.height(block_style);
            let line_bottom = line_top - line_height;
            if line_top >= slice_bottom && line_bottom <= slice_top {
                self.cursor_y = line_top;
                self.paint_collected_inline_line(line, context, &mut plaintext_direction_state);
            }
            line_top = line_bottom;
        }
        self.cursor_y = saved_cursor_y;
    }

    fn collect_inline_paragraph_lines(
        &mut self,
        paragraph: &mut Vec<InlineItem>,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        force_empty_line: bool,
        starts_after_forced_break: bool,
        output: &mut Vec<InlineFragmentedLine>,
    ) -> usize {
        trim_inline_item_edges(paragraph);
        if paragraph.is_empty() {
            if force_empty_line {
                output.push(InlineFragmentedLine {
                    fragment: None,
                    is_first_formatted_line: line_index == 0,
                    is_last_line_in_paragraph: true,
                    paragraph_last_hanging_width: 0.0,
                    line_height: context.block_style.line_height,
                });
                return line_index + 1;
            }
            return line_index;
        }

        let paragraph_start_line_index = line_index;
        let graph = self.build_inline_opportunity_graph(paragraph);
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
                    &measured_inline_items(&line_box.items),
                    context.block_style,
                )
            })
            .unwrap_or(0.0);
        let line_count = line_boxes.len();
        for (offset, line_box) in line_boxes.into_iter().enumerate() {
            let line_box_index = paragraph_start_line_index + offset;
            output.push(InlineFragmentedLine {
                fragment: Some(line_box),
                is_first_formatted_line: line_box_index == 0,
                is_last_line_in_paragraph: offset + 1 == line_count,
                paragraph_last_hanging_width,
                line_height: context.block_style.line_height,
            });
        }
        paragraph.clear();
        next_line_index
    }

    fn paint_collected_inline_line(
        &mut self,
        line: &InlineFragmentedLine,
        context: InlineParagraphContext<'_>,
        plaintext_direction_state: &mut Option<Direction>,
    ) {
        let Some(line_box) = &line.fragment else {
            self.cursor_y -= line.line_height;
            return;
        };
        let block_style = context.block_style;
        let padding_left = context.padding_left;
        let line_text = line_box.text.clone();
        let text_align = text_align_for_inline_line_text_with_state(
            block_style,
            line.is_last_line_in_paragraph,
            &line_text,
            plaintext_direction_state,
        );
        let mut metrics = line_box.metrics;
        let line_available_width = (line_box.available_width - line_box.indent).max(1.0);
        let hanging_widths = hanging_punctuation_widths_for_line_items(
            &mut self.font_system,
            &measured_inline_items(&line_box.items),
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
            available_width: line_box.available_width,
            text: line_text,
        };
        self.paint_mixed_inline_line(
            &paint_fragment,
            InlinePaintContext {
                block_style,
                available_width: line_box.available_width,
                padding_left,
                line_indent: line_box.indent,
                text_align,
                is_first_line: line.is_first_formatted_line,
                is_last_line: line.is_last_line_in_paragraph,
            },
        );
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
                self.paint_inline_item_line(
                    &[],
                    InlineLineMetrics {
                        width: 0.0,
                        offset: 0.0,
                        aligned_by_parley: false,
                        height: context.block_style.line_height,
                        baseline_offset: 0.0,
                    },
                    InlinePaintContext {
                        block_style: context.block_style,
                        available_width: context.available_width,
                        padding_left: context.padding_left,
                        line_indent: used_line_indent(
                            line_index,
                            starts_after_forced_break,
                            context.hanging_indent,
                            context.block_style,
                            context.available_width,
                        ),
                        text_align: text_align_for_inline_line(context.block_style, true),
                        is_first_line: line_index == 0,
                        is_last_line: true,
                    },
                );
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

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFragmentationPlan {
    lines: Vec<InlineFragmentedLine>,
    available_width: f32,
    padding_left: f32,
    hanging_indent: f32,
    hanging_punctuation_reserve: f32,
}

impl InlineFragmentationPlan {
    fn context<'a>(&self, block_style: &'a ComputedStyle) -> InlineParagraphContext<'a> {
        InlineParagraphContext {
            block_style,
            available_width: self.available_width,
            padding_left: self.padding_left,
            hanging_indent: self.hanging_indent,
            hanging_punctuation_reserve: self.hanging_punctuation_reserve,
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFragmentedLine {
    pub(in crate::layout) fragment: Option<InlineLineFragment>,
    pub(in crate::layout) is_first_formatted_line: bool,
    pub(in crate::layout) is_last_line_in_paragraph: bool,
    pub(in crate::layout) paragraph_last_hanging_width: f32,
    pub(in crate::layout) line_height: f32,
}

impl InlineFragmentedLine {
    fn height(&self, block_style: &ComputedStyle) -> f32 {
        self.fragment
            .as_ref()
            .map(|fragment| fragment.metrics.height.max(block_style.line_height))
            .unwrap_or(self.line_height)
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
