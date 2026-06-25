use super::super::*;
use super::graph::*;
use crate::text::measured_break_opportunities;

#[derive(Debug, Clone, Copy)]
struct InlineFloatBand {
    left_offset: f32,
    width: f32,
}

impl<'a> LayoutBuilder<'a> {
    fn inline_float_band_for_line(
        &self,
        line_index: usize,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
    ) -> InlineFloatBand {
        let line_top = self.cursor_y - block_style.line_height * line_index as f32;
        let band = self.current_float_band(line_top, block_style.line_height);
        let left_offset = (band.left - self.content_left - padding_left).max(0.0);
        let right_offset = (self.content_right - band.right).max(0.0);
        InlineFloatBand {
            left_offset,
            width: (available_width - left_offset - right_offset).max(1.0),
        }
    }

    pub(in crate::layout) fn layout_mixed_inline_paragraph(
        &mut self,
        items: &[InlineItem],
        context: InlineParagraphContext<'_>,
        mut line_index: usize,
        starts_after_forced_break: bool,
        plaintext_direction_state: &mut Option<Direction>,
    ) -> usize {
        let block_style = context.block_style;
        let padding_left = context.padding_left;
        let paragraph_start_line_index = line_index;
        let graph = self.build_inline_opportunity_graph(items);
        let (line_boxes, next_line_index) = self.select_inline_lines_from_graph(
            &graph,
            context,
            line_index,
            starts_after_forced_break,
        );
        line_index = next_line_index;
        let line_count = line_boxes.len();
        let paragraph_last_hanging_width = line_boxes
            .last()
            .map(|line_box| {
                last_hanging_punctuation_width_for_line_items(
                    &mut self.font_system,
                    &measured_inline_items(&line_box.items),
                    block_style,
                )
            })
            .unwrap_or(0.0);
        let mut painted = 0;
        while painted < line_count {
            let fragment_count =
                self.fragment_line_count(line_count, painted, block_style, |index| {
                    line_boxes[index]
                        .metrics
                        .height
                        .max(block_style.line_height)
                });
            if fragment_count == 0 {
                self.push_page();
                continue;
            }
            for (offset, line_box) in line_boxes[painted..painted + fragment_count]
                .iter()
                .enumerate()
            {
                let line_box_index = painted + offset;
                let is_first_formatted_line = paragraph_start_line_index + line_box_index == 0;
                let is_last_line = line_box_index + 1 == line_count;
                let line_text = line_box.text.clone();
                let text_align = text_align_for_inline_line_text_with_state(
                    block_style,
                    is_last_line,
                    &line_text,
                    plaintext_direction_state,
                );
                let mut metrics = line_box.metrics;
                let line_available_width = (line_box.available_width - line_box.indent).max(1.0);
                let hanging_widths = hanging_punctuation_widths_for_line_items(
                    &mut self.font_system,
                    &measured_inline_items(&line_box.items),
                    block_style,
                    is_first_formatted_line,
                    is_last_line,
                    line_box.metrics.width > line_available_width,
                );
                if line_box_uses_hanging_punctuation_alignment(block_style) {
                    let own_hanging_width = hanging_widths.end;
                    let reserve_width = if text_align == TextAlign::Right {
                        context.hanging_punctuation_reserve
                    } else {
                        0.0
                    };
                    let end_width = if is_last_line {
                        own_hanging_width.max(reserve_width)
                    } else if text_align == TextAlign::Right {
                        own_hanging_width.max(
                            paragraph_last_hanging_width.max(context.hanging_punctuation_reserve),
                        )
                    } else {
                        own_hanging_width
                    };
                    metrics.width = (metrics.width - hanging_widths.start - end_width).max(0.0);
                } else if !is_last_line && text_align == TextAlign::Right {
                    metrics.width = (metrics.width
                        - paragraph_last_hanging_width.max(context.hanging_punctuation_reserve))
                    .max(0.0);
                }
                let line_items =
                    self.visual_ordered_mixed_inline_line_items(&line_box.items, block_style);
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
                        is_first_line: is_first_formatted_line,
                        is_last_line,
                    },
                );
            }
            painted += fragment_count;
            if painted < line_count {
                self.push_page();
            }
        }
        line_index
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
        mut line_index: usize,
        starts_after_forced_break: bool,
    ) -> (Vec<InlineLineFragment>, usize) {
        if graph.is_empty() {
            return (Vec::new(), line_index);
        }
        let paragraph_start_line_index = line_index;
        let mut fragments = Vec::new();
        let mut start = 0usize;
        while start < graph.runs.len() {
            while start < graph.runs.len()
                && inline_line_item_is_collapsible_space(&graph.runs[start].item)
            {
                start += 1;
            }
            if start >= graph.runs.len() {
                break;
            }
            let line_starts_after_forced_break =
                starts_after_forced_break && line_index == paragraph_start_line_index;
            if let Some((split_fragments, consumed_runs)) = self.split_oversized_graph_text_run(
                graph,
                start,
                context,
                line_index,
                line_starts_after_forced_break,
            ) {
                line_index += split_fragments.len();
                fragments.extend(split_fragments);
                start += consumed_runs;
                continue;
            }
            let end = self.select_inline_line_end(
                graph,
                start,
                context,
                line_index,
                line_starts_after_forced_break,
            );
            let end = end.max(start + 1).min(graph.runs.len());
            let is_soft_break = end < graph.runs.len();
            fragments.push(self.materialize_inline_line_fragment(
                graph,
                start..end,
                context,
                line_index,
                line_starts_after_forced_break,
                is_soft_break,
            ));
            line_index += 1;
            start = end;
        }
        (fragments, line_index)
    }

    fn split_oversized_graph_text_run(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: usize,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
    ) -> Option<(Vec<InlineLineFragment>, usize)> {
        let block_style = context.block_style;
        if !block_style.white_space.allows_soft_wrap() {
            return None;
        }
        let mut text_index = start;
        while matches!(
            graph.runs.get(text_index).map(|run| &run.item),
            Some(InlineLineItem::Atom(atom)) if matches!(atom.content, InlineAtomContent::InlineEdge)
        ) {
            text_index += 1;
        }
        let InlineLineItem::Fragment(fragment) = &graph.runs.get(text_index)?.item else {
            return None;
        };
        let prefix_items = graph.line_items(start..text_index);
        let prefix_measured_items = graph.line_measured_items(start..text_index);
        let prefix_width = graph.line_width(start..text_index);
        let band = self.inline_float_band_for_line(
            line_index,
            block_style,
            context.available_width,
            context.padding_left,
        );
        let line_indent = used_line_indent(
            line_index,
            starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width,
        );
        let line_available_width = (band.width - line_indent).max(1.0);
        let first_hanging_punctuation_width = start_hanging_punctuation_width_for_candidate_line(
            &mut self.font_system,
            &prefix_items,
            &graph.runs[text_index].item,
            block_style,
            line_index == 0,
        );
        let first_line_available_width =
            (line_available_width + first_hanging_punctuation_width - prefix_width).max(1.0);
        let lines = self
            .font_system
            .break_text(&fragment.text, &fragment.style, first_line_available_width)
            .into_iter()
            .filter(|line| !line.text.is_empty())
            .collect::<Vec<_>>();
        let lines = if lines.len() <= 1
            && !(fragment.style.hyphens == Hyphens::None && fragment.text.contains('\u{00ad}'))
        {
            self.break_graph_fragment_at_measured_opportunities(
                fragment,
                first_line_available_width,
            )
        } else {
            lines
        };
        if lines.len() <= 1 {
            return None;
        }
        let split_count = lines.len();
        let mut output = Vec::with_capacity(split_count);
        for (split_index, split_line) in lines.into_iter().enumerate() {
            let mut items = if split_index == 0 {
                prefix_items.clone()
            } else {
                Vec::new()
            };
            items.push(InlineLineItem::Fragment(InlineFragment {
                text: split_line.text,
                style: fragment.style.clone(),
                baseline_shift: fragment.baseline_shift,
                link_target: fragment.link_target.clone(),
                mergeable: false,
                hanging_edges: InlineHangingEdges {
                    blocks_start: fragment.hanging_edges.blocks_start && split_index == 0,
                    blocks_end: fragment.hanging_edges.blocks_end && split_index + 1 == split_count,
                },
            }));
            let mut measured_items = if split_index == 0 {
                prefix_measured_items.clone()
            } else {
                Vec::new()
            };
            measured_items.push(MeasuredInlineItem {
                item: items.last().cloned().expect("split fragment item"),
                width: split_line.width,
                shaped: split_line.shaped.clone(),
            });
            let mut width = split_line.width + if split_index == 0 { prefix_width } else { 0.0 };
            width -= trim_trailing_inline_line_spaces(&mut items, &mut self.font_system);
            if split_index + 1 < split_count {
                width -= trim_trailing_pre_wrap_hanging_inline_line_spaces(
                    &mut items,
                    &mut self.font_system,
                );
            }
            width -= trailing_hanging_space_separator_width_for_line_items(
                &items,
                &mut self.font_system,
            );
            width -= trailing_letter_spacing_width_for_line_items(&items);
            width = width.max(0.0);
            if split_index + 1 < split_count {
                show_trailing_soft_hyphen_for_line(&mut items);
            }
            let line_number = line_index + split_index;
            let split_band = self.inline_float_band_for_line(
                line_number,
                block_style,
                context.available_width,
                context.padding_left,
            );
            let split_indent = used_line_indent(
                line_number,
                (starts_after_forced_break && split_index == 0)
                    || split_line.starts_after_forced_break,
                context.hanging_indent,
                block_style,
                split_band.width,
            );
            let split_metrics = self.mixed_inline_line_metrics(&items, block_style, width);
            output.push(InlineLineFragment {
                text: graph.runs[start].break_text.clone(),
                items: measured_items,
                metrics: InlineLineMetrics {
                    width,
                    offset: 0.0,
                    aligned_by_parley: false,
                    height: split_line.line_height,
                    baseline_offset: split_metrics.baseline_offset,
                },
                hanging_widths: HangingPunctuationWidths::default(),
                indent: split_band.left_offset + split_indent,
                available_width: split_band.left_offset + split_band.width,
            });
        }
        Some((output, text_index + 1 - start))
    }

    fn break_graph_fragment_at_measured_opportunities(
        &mut self,
        fragment: &InlineFragment,
        available_width: f32,
    ) -> Vec<TextLine> {
        let mut opportunities = measured_break_opportunities(&fragment.text, &fragment.style);
        opportunities.retain(|opportunity| {
            *opportunity > 0
                && *opportunity <= fragment.text.len()
                && fragment.text.is_char_boundary(*opportunity)
        });
        opportunities.sort_unstable();
        opportunities.dedup();
        if opportunities.last().copied() != Some(fragment.text.len()) {
            opportunities.push(fragment.text.len());
        }
        let mut lines = Vec::new();
        let mut line_start = 0usize;
        let mut segment_start = 0usize;
        let mut line_width = 0.0f32;
        for opportunity in opportunities {
            if opportunity <= segment_start {
                continue;
            }
            let segment = &fragment.text[segment_start..opportunity];
            let segment_width = self.font_system.measure_text(segment, &fragment.style);
            if !inline_items_fit_line(line_width, segment_width, available_width)
                && segment_start > line_start
            {
                let text = fragment.text[line_start..segment_start].to_string();
                let width = line_width;
                lines.push(TextLine::new(text, width, fragment.style.line_height));
                line_start = segment_start;
                line_width = 0.0;
            }
            line_width += segment_width;
            segment_start = opportunity;
        }
        if line_start < fragment.text.len() {
            let text = fragment.text[line_start..].to_string();
            lines.push(TextLine::new(text, line_width, fragment.style.line_height));
        }
        lines
    }

    fn select_inline_line_end(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: usize,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
    ) -> usize {
        let block_style = context.block_style;
        let band = self.inline_float_band_for_line(
            line_index,
            block_style,
            context.available_width,
            context.padding_left,
        );
        let line_indent = used_line_indent(
            line_index,
            starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width,
        );
        let line_available_width = (band.width - line_indent).max(1.0);
        let mut end = start;
        let mut line_width = 0.0f32;
        while end < graph.runs.len() {
            let run = &graph.runs[end];
            let line = graph.line_items(start..end);
            let first_hanging_punctuation_width =
                start_hanging_punctuation_width_for_candidate_line(
                    &mut self.font_system,
                    &line,
                    &run.item,
                    block_style,
                    line_index == 0,
                );
            let remaining_allows_last = graph.runs[end + 1..].iter().all(|run| {
                inline_line_item_is_collapsible_space(&run.item)
                    || inline_line_item_is_pre_wrap_hanging_space(&run.item)
            });
            let final_hanging_punctuation_width = end_hanging_punctuation_width_for_candidate_line(
                &mut self.font_system,
                &line,
                &run.item,
                block_style,
                remaining_allows_last,
                true,
            );
            let following_edge_width = graph.runs[end + 1..]
                .iter()
                .map_while(|run| match &run.item {
                    InlineLineItem::Atom(atom)
                        if matches!(atom.content, InlineAtomContent::InlineEdge) =>
                    {
                        Some(run.width)
                    }
                    _ => None,
                })
                .sum::<f32>();
            let candidate_fits = inline_items_fit_line(
                line_width,
                run.width + following_edge_width,
                line_available_width
                    + first_hanging_punctuation_width
                    + final_hanging_punctuation_width,
            );
            let final_preserved_space = end + 1 == graph.runs.len()
                && inline_line_item_is_pre_wrap_hanging_space(&run.item);
            if block_style.white_space.allows_soft_wrap()
                && end > start
                && !final_preserved_space
                && !candidate_fits
                && let Some(boundary) =
                    self.best_inline_graph_break_before(graph, start, end, block_style)
            {
                return boundary;
            }
            line_width += run.width;
            end += 1;
        }
        end
    }

    fn best_inline_graph_break_before(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: usize,
        before_run: usize,
        block_style: &ComputedStyle,
    ) -> Option<usize> {
        if let Some(boundary) = (start + 1..=before_run).rev().find(|boundary| {
            matches!(
                &graph.runs[*boundary].item,
                InlineLineItem::Fragment(fragment)
                    if fragment.style.white_space == WhiteSpace::BreakSpaces
                        && fragment.text.chars().all(is_css_collapsible_whitespace)
            )
        }) {
            return Some(boundary);
        }
        (start + 1..=before_run).rev().find(|boundary| {
            let Some(opportunity) = graph.break_opportunity_before(*boundary) else {
                return false;
            };
            if opportunity.emergency {
                return true;
            }
            let line = graph.line_items(start..*boundary);
            let item = &graph.runs[*boundary].item;
            !mixed_inline_item_starts_with_suppressed_line_start_punctuation(item)
                && mixed_inline_soft_wrap_allowed_before_item(&line, item, block_style)
        })
    }

    fn materialize_inline_line_fragment(
        &mut self,
        graph: &InlineOpportunityGraph,
        range: std::ops::Range<usize>,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
        is_soft_break: bool,
    ) -> InlineLineFragment {
        let block_style = context.block_style;
        let band = self.inline_float_band_for_line(
            line_index,
            block_style,
            context.available_width,
            context.padding_left,
        );
        let line_indent = used_line_indent(
            line_index,
            starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width,
        );
        let mut measured_items = graph.line_measured_items(range.clone());
        let mut items = measured_inline_items(&measured_items);
        let mut width = graph.line_width(range.clone());
        width -= trim_trailing_inline_line_spaces(&mut items, &mut self.font_system);
        measured_items.truncate(items.len());
        if is_soft_break {
            width -= trim_trailing_pre_wrap_hanging_inline_line_spaces(
                &mut items,
                &mut self.font_system,
            );
            measured_items.truncate(items.len());
        }
        width -=
            trailing_hanging_space_separator_width_for_line_items(&items, &mut self.font_system);
        width -= trailing_letter_spacing_width_for_line_items(&items);
        width = width.max(0.0);
        if is_soft_break {
            show_trailing_soft_hyphen_for_line(&mut items);
            measured_items = measured_items
                .into_iter()
                .zip(items.iter().cloned())
                .map(|(mut measured, item)| {
                    measured.item = item;
                    measured
                })
                .collect();
        }
        measured_items = strip_zero_width_space_from_measured_items(measured_items);
        items = measured_inline_items(&measured_items);
        let metrics = self.mixed_inline_line_metrics(&items, block_style, width);
        let text = graph.text(range);
        InlineLineFragment {
            items: measured_items,
            metrics,
            hanging_widths: HangingPunctuationWidths::default(),
            indent: band.left_offset + line_indent,
            available_width: band.left_offset + band.width,
            text,
        }
    }

    /// Return mixed inline line items in UBA visual order.
    ///
    /// CSS Writing Modes applies the Unicode Bidirectional Algorithm to inline
    /// content. Atomic inline boxes participate as object replacement
    /// characters in the bidi stream, then paint as indivisible inline-level
    /// boxes in the resolved visual order:
    /// <https://www.w3.org/TR/css-writing-modes-4/#bidi-algo> and
    /// <https://www.unicode.org/reports/tr9/#L1>.
    pub(in crate::layout) fn visual_ordered_mixed_inline_line(
        &mut self,
        items: &[InlineLineItem],
        block_style: &ComputedStyle,
    ) -> Vec<InlineLineItem> {
        if !mixed_inline_line_needs_bidi_ordering(items, block_style) {
            return items.to_vec();
        }
        let (text, ranged_items) = mixed_inline_line_bidi_text(items);
        let visual_ranges = self
            .font_system
            .visual_ranges_for_unwrapped_text(&text, block_style);
        let mut output = Vec::new();
        for visual_range in visual_ranges {
            for ranged in &ranged_items {
                let start = ranged.range.start.max(visual_range.start);
                let end = ranged.range.end.min(visual_range.end);
                if start >= end {
                    continue;
                }
                match &ranged.item {
                    InlineLineItem::Fragment(fragment) => {
                        let relative_start = start - ranged.range.start;
                        let relative_end = end - ranged.range.start;
                        let Some(mut text) =
                            char_boundary_slice(&fragment.text, relative_start..relative_end)
                        else {
                            continue;
                        };
                        text = text_without_bidi_format_controls(&text).into_owned();
                        if !text.is_empty() {
                            let mut fragment = fragment.clone();
                            fragment.text = text;
                            output.push(InlineLineItem::Fragment(fragment));
                        }
                    }
                    InlineLineItem::Atom(atom) => output.push(InlineLineItem::Atom(atom.clone())),
                }
            }
        }
        if output.is_empty() {
            items
                .iter()
                .filter_map(|item| match item {
                    InlineLineItem::Fragment(fragment) => {
                        let text = text_without_bidi_format_controls(&fragment.text).into_owned();
                        (!text.is_empty()).then(|| {
                            let mut fragment = fragment.clone();
                            fragment.text = text;
                            InlineLineItem::Fragment(fragment)
                        })
                    }
                    InlineLineItem::Atom(atom) => Some(InlineLineItem::Atom(atom.clone())),
                })
                .collect()
        } else {
            output
        }
    }

    pub(in crate::layout) fn visual_ordered_mixed_inline_line_items(
        &mut self,
        items: &[MeasuredInlineItem],
        block_style: &ComputedStyle,
    ) -> Vec<MeasuredInlineItem> {
        let line_items = measured_inline_items(items);
        if !mixed_inline_line_needs_bidi_ordering(&line_items, block_style) {
            return items.to_vec();
        }
        self.visual_ordered_mixed_inline_line(&line_items, block_style)
            .into_iter()
            .map(|item| {
                let width = match &item {
                    InlineLineItem::Fragment(fragment) => self
                        .font_system
                        .shape_unwrapped_line(
                            &fragment.text,
                            &fragment.style,
                            fragment.style.line_height,
                        )
                        .map(|line| line.advance_width())
                        .unwrap_or(0.0),
                    InlineLineItem::Atom(atom) => atom.width,
                };
                MeasuredInlineItem {
                    item,
                    width,
                    shaped: None,
                }
            })
            .collect()
    }

    /// Return a mixed inline line item's baseline offset from its margin-box top.
    ///
    /// CSS Inline Layout aligns inline-level boxes by their baselines inside a
    /// line box. Text baselines come from the selected font metrics, while
    /// atomic inline boxes expose their own atomic baseline:
    /// <https://www.w3.org/TR/css-inline-3/#line-box>.
    fn inline_line_item_baseline_offset(&mut self, item: &InlineLineItem) -> f32 {
        match item {
            InlineLineItem::Fragment(fragment) => {
                if matches!(fragment.style.vertical_align, VerticalAlign::Top) {
                    return 0.0;
                }
                self.inline_style_baseline_offset(&fragment.style, fragment.baseline_shift)
            }
            InlineLineItem::Atom(atom) => {
                atom.style.margin.top + atom.baseline_offset - atom.baseline_shift
            }
        }
    }

    /// Return a mixed inline item ascent/descent pair around its baseline.
    ///
    /// CSS Inline Layout defines line box height from the logical extents of
    /// inline-level boxes placed around the shared line baseline. Text
    /// fragments keep the CSS `line-height` logical box even when selected font
    /// ink metrics are taller; CSS 2.2 permits negative leading, so glyph ink
    /// can overflow without increasing the line box:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
    fn inline_line_item_baseline_extents(&mut self, item: &InlineLineItem) -> (f32, f32) {
        let baseline = self.inline_line_item_baseline_offset(item).max(0.0);
        let descent = match item {
            InlineLineItem::Fragment(_) => inline_line_item_height(item) - baseline,
            InlineLineItem::Atom(_) => (inline_line_item_height(item) - baseline).max(0.0),
        };
        (baseline, descent)
    }

    /// Return the parent line strut ascent/descent pair around its baseline.
    ///
    /// The strut participates in every inline formatting context line. Text
    /// painting in this renderer uses the selected-font ascent as the line
    /// baseline coordinate, while `line-height` remains the used block-axis
    /// line advance:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
    /// <https://www.w3.org/TR/css-inline-3/#line-height-property>.
    fn inline_style_line_extents(
        &mut self,
        style: &ComputedStyle,
        baseline_shift: f32,
    ) -> (f32, f32) {
        let line_height = self.font_system.used_line_height(style);
        let baseline = self
            .inline_style_baseline_offset(style, baseline_shift)
            .max(0.0);
        let descent = line_height - baseline;
        (baseline, descent)
    }

    fn inline_style_baseline_offset(&mut self, style: &ComputedStyle, baseline_shift: f32) -> f32 {
        let font_id = self.font_system.resolve_style(style);
        let line_height = self.font_system.line_height_for_font(font_id, style);
        let adjustment =
            self.font_system
                .font_ascent_baseline_adjustment(font_id, style, line_height);
        style.font_size - adjustment - baseline_shift
    }

    /// Return line metrics for mixed inline line-box participants.
    ///
    /// CSS Inline Layout creates every line box from the parent strut plus the
    /// inline-level boxes placed on that line. Soft-wrapped fragments and
    /// hard-break fragments must therefore use the same strut and baseline
    /// calculation:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
    fn mixed_inline_line_metrics(
        &mut self,
        items: &[InlineLineItem],
        block_style: &ComputedStyle,
        width: f32,
    ) -> InlineLineMetrics {
        let (baseline_offset, descent) =
            self.mixed_inline_line_baseline_extents(items, block_style);
        let text_only_height = items
            .iter()
            .all(|item| matches!(item, InlineLineItem::Fragment(_)))
            .then(|| {
                items
                    .iter()
                    .filter_map(|item| match item {
                        InlineLineItem::Fragment(fragment) => Some(fragment.style.line_height),
                        InlineLineItem::Atom(_) => None,
                    })
                    .fold(block_style.line_height, f32::max)
            });
        InlineLineMetrics {
            width,
            offset: 0.0,
            aligned_by_parley: false,
            height: text_only_height.unwrap_or(baseline_offset + descent),
            baseline_offset,
        }
    }

    fn mixed_inline_line_baseline_extents(
        &mut self,
        items: &[InlineLineItem],
        block_style: &ComputedStyle,
    ) -> (f32, f32) {
        let (mut baseline_offset, mut descent) = self.inline_style_line_extents(block_style, 0.0);
        for item in items {
            let (item_baseline_offset, item_descent) = self.inline_line_item_baseline_extents(item);
            baseline_offset = baseline_offset.max(item_baseline_offset);
            descent = descent.max(item_descent);
        }
        (baseline_offset, descent)
    }
}

#[derive(Debug, Clone)]
struct RangedMixedInlineLineItem {
    item: InlineLineItem,
    range: std::ops::Range<usize>,
}

fn mixed_inline_line_needs_bidi_ordering(
    items: &[InlineLineItem],
    block_style: &ComputedStyle,
) -> bool {
    block_style.direction == Direction::Rtl
        || inline_bidi_scope_affects_line_ordering(block_style)
        || items.iter().any(|item| match item {
            InlineLineItem::Fragment(fragment) => {
                contains_bidi_text(&fragment.text)
                    || inline_bidi_scope_affects_line_ordering(&fragment.style)
            }
            InlineLineItem::Atom(atom) => {
                atom.style.direction != block_style.direction
                    || inline_bidi_scope_affects_line_ordering(&atom.style)
            }
        })
}

fn strip_zero_width_space_from_measured_items(
    items: Vec<MeasuredInlineItem>,
) -> Vec<MeasuredInlineItem> {
    const ZERO_WIDTH_SPACE: char = '\u{200b}';
    items
        .into_iter()
        .filter_map(|mut measured| {
            if let InlineLineItem::Fragment(fragment) = &mut measured.item
                && fragment.text.contains(ZERO_WIDTH_SPACE)
            {
                fragment.text = fragment.text.replace(ZERO_WIDTH_SPACE, "");
                if fragment.text.is_empty() {
                    return None;
                }
            }
            Some(measured)
        })
        .collect()
}

fn mixed_inline_line_bidi_text(
    items: &[InlineLineItem],
) -> (String, Vec<RangedMixedInlineLineItem>) {
    let mut text = String::new();
    let mut ranged = Vec::new();
    for item in items {
        let start = text.len();
        match item {
            InlineLineItem::Fragment(fragment) => text.push_str(&fragment.text),
            InlineLineItem::Atom(atom) => {
                if !matches!(atom.content, InlineAtomContent::Leader(_)) {
                    text.push(OBJECT_REPLACEMENT_CHARACTER);
                }
            }
        }
        let end = text.len();
        ranged.push(RangedMixedInlineLineItem {
            item: item.clone(),
            range: start..end,
        });
    }
    (text, ranged)
}

/// Return whether CSS Text permits a soft wrap before one mixed inline item.
///
/// CSS Text applies Unicode line breaking across text and atomic inline
/// boundaries by representing atomic inline boxes as U+FFFC. The mixed inline
/// line builder must therefore ask the line breaker about the actual boundary
/// instead of treating every fragment/atom item boundary as breakable:
/// <https://www.w3.org/TR/css-text-3/#line-break-details>.
fn mixed_inline_soft_wrap_allowed_before_item(
    line: &[InlineLineItem],
    item: &InlineLineItem,
    block_style: &ComputedStyle,
) -> bool {
    if inline_line_item_is_collapsible_space(item)
        || inline_line_item_is_pre_wrap_hanging_space(item)
    {
        return true;
    }
    if tracked_text_boundary_allows_soft_wrap(line, item) {
        return true;
    }
    let before = mixed_inline_line_break_text(line);
    let after = mixed_inline_item_break_text(item);
    inline_atomic_boundary_allows_soft_wrap(&before, &after, block_style)
}

/// Return whether a nonzero `letter-spacing` text boundary is soft-wrappable.
///
/// CSS Text models tracking as spacing between typographic character units.
/// A boundary split by text-empty inline boxes still has that inter-character
/// spacing and must be available to line fitting, which is observable in
/// intrinsic-size WPTs using `width: max-content`:
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
fn tracked_text_boundary_allows_soft_wrap(line: &[InlineLineItem], item: &InlineLineItem) -> bool {
    let InlineLineItem::Fragment(current) = item else {
        return false;
    };
    if current.text.is_empty() {
        return false;
    }
    for previous in line.iter().rev() {
        match previous {
            InlineLineItem::Fragment(previous) => {
                return !previous.text.is_empty()
                    && (previous.style.used_letter_spacing() != 0.0
                        || current.style.used_letter_spacing() != 0.0);
            }
            InlineLineItem::Atom(atom) if matches!(atom.content, InlineAtomContent::InlineEdge) => {
            }
            InlineLineItem::Atom(_) => return false,
        }
    }
    false
}

fn mixed_inline_line_break_text(items: &[InlineLineItem]) -> String {
    let mut text = String::new();
    for item in items {
        text.push_str(&mixed_inline_item_break_text(item));
    }
    text
}

/// Return whether an item starts with punctuation that CSS Text keeps off line start.
///
/// UAX #14 LB13 and CSS Text tailoring suppress line breaks before commas,
/// stops, closing punctuation, and matching quote/bracket categories. Mixed
/// inline layout uses this when inline box edge atoms would otherwise hide the
/// text boundary from the segmenter:
/// <https://www.unicode.org/reports/tr14/#LB13> and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
fn mixed_inline_item_starts_with_suppressed_line_start_punctuation(item: &InlineLineItem) -> bool {
    let InlineLineItem::Fragment(fragment) = item else {
        return false;
    };
    let Some(character) = fragment
        .text
        .chars()
        .find(|character| !is_css_collapsible_whitespace(*character))
    else {
        return false;
    };
    character_is_hangable_stop_or_comma(character)
        || character_is_last_hangable_punctuation(character)
}

fn mixed_inline_item_break_text(item: &InlineLineItem) -> String {
    match item {
        InlineLineItem::Fragment(fragment) => fragment.text.clone(),
        InlineLineItem::Atom(atom) => match atom.content {
            InlineAtomContent::InlineEdge | InlineAtomContent::Leader(_) => String::new(),
            _ => OBJECT_REPLACEMENT_CHARACTER.to_string(),
        },
    }
}
