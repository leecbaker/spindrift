use super::super::*;
use super::graph::*;
use super::{InlineLineRecord, InlineLineSequence};

#[derive(Debug, Clone, Copy)]
struct InlineFloatBand {
    span: LogicalInlineSpan,
}

impl InlineFloatBand {
    fn new(left_offset: f32, width: f32) -> Self {
        Self {
            span: LogicalInlineSpan::new(left_offset, width.max(1.0)),
        }
    }

    fn left_offset(self) -> f32 {
        self.span.start()
    }

    fn width(self) -> f32 {
        self.span.size()
    }

    fn end(self) -> f32 {
        self.span.end()
    }
}

const INLINE_FLOAT_EPSILON: f32 = 0.01;

#[derive(Debug, Clone, Copy)]
struct SelectedInlineLineEnd {
    position: InlineGraphPosition,
    break_opportunity: Option<InlineBreakOpportunity>,
}

#[derive(Debug, Clone, Copy)]
struct InlineFloatPlacement {
    /// Physical horizontal line-box band that accepted the inline float.
    ///
    /// CSS 2.2 floats shorten line boxes in the current block formatting
    /// context. This span is page-local physical `x` after writing-mode and
    /// `direction` have already been resolved for the horizontal line:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    line_span: PageInlineSpan,
    /// Inline advance consumed by text before the same-line float.
    ///
    /// The same-line optimization only runs for horizontal LTR suffix layout,
    /// so this is a physical advance from the line span's left edge in the
    /// already-resolved line-box coordinate system:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>.
    prefix_width: f32,
    /// Physical margin-box span of the placed inline float.
    ///
    /// The span comes from the durable float exclusion shape and is used to
    /// split the remaining line into the gap before/after the float.
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    float_span: PageInlineSpan,
    side: UsedFloatSide,
}

impl InlineFloatPlacement {
    fn new(
        line_left: f32,
        line_right: f32,
        prefix_width: f32,
        float_left: f32,
        float_right: f32,
        side: UsedFloatSide,
    ) -> Self {
        Self {
            line_span: PageInlineSpan::from_edges(line_left, line_right),
            prefix_width: prefix_width.max(0.0),
            float_span: PageInlineSpan::from_edges(float_left, float_right),
            side,
        }
    }

    fn line_right(self) -> f32 {
        self.line_span.right_x()
    }

    fn prefix_right(self) -> f32 {
        self.line_span.left_x() + self.prefix_width
    }

    fn float_left(self) -> f32 {
        self.float_span.left_x()
    }

    fn float_right(self) -> f32 {
        self.float_span.right_x()
    }
}

#[derive(Debug, Clone)]
struct CombinedInlineFloatLine {
    end: InlineGraphPosition,
    fragment: InlineLineFragment,
}

impl<'a> LayoutBuilder<'a> {
    fn inline_float_band_for_line(
        &self,
        line_index: usize,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
    ) -> InlineFloatBand {
        if block_style.writing_mode != WritingMode::HorizontalTb {
            let band = self.current_logical_float_band(
                block_style.writing_mode,
                block_style.direction,
                self.content_left + padding_left,
                block_style.line_height,
                self.cursor_y,
                available_width,
            );
            return InlineFloatBand::new(band.inline_start(), band.available_inline_size());
        }
        let line_top = self.cursor_y - block_style.line_height * line_index as f32;
        let band = self.current_float_band(line_top, block_style.line_height);
        let left_offset = (band.left() - self.content_left - padding_left).max(0.0);
        let right_offset = (self.content_right - band.right()).max(0.0);
        InlineFloatBand::new(left_offset, available_width - left_offset - right_offset)
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
        let paragraph_start_line_index = line_index;
        let graph = self.build_inline_opportunity_graph(items, block_style);
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
                    &line_box.items,
                    block_style,
                )
            })
            .unwrap_or(0.0);
        let records = line_boxes
            .into_iter()
            .enumerate()
            .map(|(offset, line_box)| {
                let block_line_index = paragraph_start_line_index + offset;
                let line_height = line_box.metrics.height.max(block_style.line_height);
                let used_indent = line_box.indent;
                let available_width = line_box.available_width;
                InlineLineRecord {
                    paragraph_index: 0,
                    block_line_index,
                    paragraph_line_index: offset,
                    fragment: Some(line_box),
                    is_first_formatted_line: block_line_index == 0,
                    is_last_line_in_paragraph: offset + 1 == line_count,
                    is_forced_empty: false,
                    paragraph_last_hanging_width,
                    used_indent,
                    available_width,
                    line_height,
                }
            })
            .collect();
        let sequence = InlineLineSequence {
            records,
            available_width: context.available_width,
            padding_left: context.padding_left,
            hanging_indent: context.hanging_indent,
            hanging_punctuation_reserve: context.hanging_punctuation_reserve,
        };
        self.paint_inline_line_sequence_with_state(
            &sequence,
            block_style,
            plaintext_direction_state,
        );
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
        let mut start = graph.start_position();
        let graph_end = graph.end_position();
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
            if let Some(float) = graph.float_at_position(start).cloned() {
                self.place_inline_waiting_float(&float, context, line_index);
                start.run_index += 1;
                start.byte_offset = 0;
                continue;
            }
            let line_starts_after_forced_break =
                starts_after_forced_break && line_index == paragraph_start_line_index;
            let selected_end = self.select_inline_line_end(
                graph,
                start,
                context,
                line_index,
                line_starts_after_forced_break,
            );
            let end = if selected_end.position <= start {
                graph_end
            } else {
                selected_end.position.min(graph_end)
            };
            let selected_range = InlineGraphRange { start, end };
            if !context.block_style.white_space.allows_soft_wrap()
                && let Some(fragment) = self.try_select_unbreakable_line_with_inline_floats(
                    graph,
                    selected_range,
                    selected_end,
                    context,
                    line_index,
                    line_starts_after_forced_break,
                )
            {
                fragments.push(fragment);
                line_index += 1;
                start = end;
                continue;
            }
            if let Some(float_position) = graph.first_float_position_in_range(selected_range) {
                if float_position <= start {
                    if let Some(float) = graph.float_at_position(float_position).cloned() {
                        self.place_inline_waiting_float(&float, context, line_index);
                    }
                    start = InlineGraphPosition::at_run_start(float_position.run_index + 1);
                    continue;
                }
                let break_opportunity = Some(InlineBreakOpportunity {
                    position: float_position,
                    kind: InlineBreakKind::AtomicBoundary,
                    priority: 110,
                    trims: false,
                    hangs: false,
                    soft_hyphen: false,
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
                    line_index,
                    line_starts_after_forced_break,
                    break_opportunity,
                );
                if let Some(placement) = self.try_place_inline_float_on_current_line(
                    graph,
                    float_position,
                    prefix.metrics.width,
                    context,
                    line_index,
                    line_starts_after_forced_break,
                ) {
                    prefix.suppress_float_adjust = true;
                    let suffix_start =
                        InlineGraphPosition::at_run_start(float_position.run_index + 1);
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
                        fragments.push(combined.fragment);
                        line_index += 1;
                        start = end;
                        continue;
                    }
                    fragments.push(prefix);
                    line_index += 1;
                    start = suffix_start;
                    continue;
                }
                let break_opportunity = Some(InlineBreakOpportunity {
                    position: float_position,
                    kind: InlineBreakKind::AtomicBoundary,
                    priority: 110,
                    trims: false,
                    hangs: false,
                    soft_hyphen: false,
                    emergency: false,
                    min_content: true,
                });
                fragments.push(self.materialize_inline_line_fragment(
                    graph,
                    InlineGraphRange {
                        start,
                        end: float_position,
                    },
                    context,
                    line_index,
                    line_starts_after_forced_break,
                    break_opportunity,
                ));
                line_index += 1;
                start = float_position;
                continue;
            }
            let break_opportunity = selected_end
                .break_opportunity
                .filter(|opportunity| opportunity.position == end && end < graph_end);
            fragments.push(self.materialize_inline_line_fragment(
                graph,
                selected_range,
                context,
                line_index,
                line_starts_after_forced_break,
                break_opportunity,
            ));
            line_index += 1;
            start = end;
        }
        (fragments, line_index)
    }

    /// Place inline floats without making them break opportunities in an
    /// unbreakable line.
    ///
    /// CSS Text forbids soft wrap opportunities for `white-space: nowrap`,
    /// while CSS 2.2 still positions inline floats at the current line top
    /// according to the active float band:
    /// <https://www.w3.org/TR/css-text-3/#white-space-property> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#float-position>.
    fn try_select_unbreakable_line_with_inline_floats(
        &mut self,
        graph: &InlineOpportunityGraph,
        range: InlineGraphRange,
        selected_end: SelectedInlineLineEnd,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
    ) -> Option<InlineLineFragment> {
        graph.first_float_position_in_range(range)?;
        let break_opportunity = selected_end.break_opportunity.filter(|opportunity| {
            opportunity.position == range.end && range.end < graph.end_position()
        });
        let mut fragment = self.materialize_inline_line_fragment(
            graph,
            range,
            context,
            line_index,
            starts_after_forced_break,
            break_opportunity,
        );
        let snapshot = self.snapshot();
        let mut search_start = range.start;
        while let Some(float_position) = graph.first_float_position_in_range(InlineGraphRange {
            start: search_start,
            end: range.end,
        }) {
            if !self.try_place_unbreakable_inline_float(
                graph,
                float_position,
                context,
                line_index,
                starts_after_forced_break,
            ) {
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

    fn place_inline_waiting_float(
        &mut self,
        float: &InlineFloat,
        context: InlineParagraphContext<'_>,
        line_index: usize,
    ) {
        let saved_cursor_y = self.cursor_y;
        self.cursor_y -= context.block_style.line_height * line_index as f32;
        let mut run = self.float_run_state();
        self.layout_floating_child(
            &float.element,
            float.signature.clone(),
            &float.style,
            None,
            context.stylesheets,
            &mut run,
        );
        self.cursor_y = saved_cursor_y;
    }

    fn try_place_unbreakable_inline_float(
        &mut self,
        graph: &InlineOpportunityGraph,
        float_position: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
    ) -> bool {
        if context.block_style.writing_mode != WritingMode::HorizontalTb {
            return false;
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
        let line_indent = used_line_indent(
            line_index,
            starts_after_forced_break,
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
        self.containing_block_direction = block_style.direction;
        let mut run = self.float_run_state();
        let placed = self.layout_floating_child(
            &float.element,
            float.signature.clone(),
            &float.style,
            None,
            context.stylesheets,
            &mut run,
        );
        let accepted = if placed && self.pages.len() == snapshot.pages.len() {
            self.float_contexts
                .last()
                .and_then(|context| context.shapes.last())
                .is_some_and(|shape| {
                    let float_width = shape.right() - shape.left();
                    let band_width = line_right - line_left;
                    shape.page_index == self.pages.len()
                        && (shape.top() - target_top).abs() <= INLINE_FLOAT_EPSILON
                        && ((shape.left() + INLINE_FLOAT_EPSILON >= line_left
                            && shape.right() <= line_right + INLINE_FLOAT_EPSILON)
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

    fn try_place_inline_float_on_current_line(
        &mut self,
        graph: &InlineOpportunityGraph,
        float_position: InlineGraphPosition,
        prefix_width: f32,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
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
        let line_indent = used_line_indent(
            line_index,
            starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width(),
        );
        let line_left = self.content_left + context.padding_left + band.left_offset() + line_indent;
        let line_right =
            self.content_left + context.padding_left + band.left_offset() + band.width();
        let (remaining_left, remaining_right) = match block_style.direction {
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
        self.containing_block_direction = block_style.direction;
        let mut run = self.float_run_state();
        let placed = self.layout_floating_child(
            &float.element,
            float.signature.clone(),
            &float.style,
            None,
            context.stylesheets,
            &mut run,
        );
        let accepted_shape = if placed && self.pages.len() == snapshot.pages.len() {
            self.float_contexts.last().and_then(|context| {
                context.shapes.last().and_then(|shape| {
                    (shape.page_index == self.pages.len()
                        && (shape.top() - target_top).abs() <= INLINE_FLOAT_EPSILON
                        && shape.left() + INLINE_FLOAT_EPSILON >= remaining_left
                        && shape.right() <= remaining_right + INLINE_FLOAT_EPSILON)
                        .then_some(InlineFloatPlacement::new(
                            line_left,
                            line_right,
                            prefix_width,
                            shape.left(),
                            shape.right(),
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

    #[allow(clippy::too_many_arguments)]
    fn try_select_inline_float_same_line_suffix(
        &mut self,
        graph: &InlineOpportunityGraph,
        prefix: InlineLineFragment,
        float_position: InlineGraphPosition,
        suffix_start: InlineGraphPosition,
        placement: InlineFloatPlacement,
        context: InlineParagraphContext<'_>,
        line_index: usize,
    ) -> Option<CombinedInlineFloatLine> {
        if context.block_style.direction != Direction::Ltr || suffix_start >= graph.end_position() {
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
        let mut combined_items = prefix.items.clone();
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
        let mut text = prefix.text;
        text.push_str(&suffix.text);
        Some(CombinedInlineFloatLine {
            end,
            fragment: InlineLineFragment {
                items: combined_items,
                metrics,
                hanging_widths: prefix.hanging_widths,
                indent: prefix.indent,
                available_width: prefix.available_width,
                suppress_float_adjust: true,
                text,
            },
        })
    }

    fn select_inline_line_end(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
    ) -> SelectedInlineLineEnd {
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
            band.width(),
        );
        let line_available_width = (band.width() - line_indent).max(1.0);
        self.select_inline_line_end_for_width(
            graph,
            start,
            block_style,
            line_available_width,
            line_index,
        )
    }

    fn select_inline_line_end_for_width(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        block_style: &ComputedStyle,
        line_available_width: f32,
        line_index: usize,
    ) -> SelectedInlineLineEnd {
        let mut regular_fit = None::<SelectedInlineLineEnd>;
        let mut emergency_fit = None::<SelectedInlineLineEnd>;
        let opportunities = graph.break_opportunities_after(start).collect::<Vec<_>>();
        for opportunity in opportunities {
            if !block_style.white_space.allows_soft_wrap()
                && !matches!(opportunity.kind, InlineBreakKind::Forced)
            {
                continue;
            }
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
                (measurement.content_width - hanging_widths.start - hanging_widths.end).max(0.0)
            } else {
                let materialized = graph.materialize_line(
                    range,
                    selected_break,
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
                (materialized.content_width - hanging_widths.start - hanging_widths.end).max(0.0)
            };
            if fit_width <= line_available_width + 0.5 {
                let selected = SelectedInlineLineEnd {
                    position: opportunity.position,
                    break_opportunity: (opportunity.position < graph.end_position())
                        .then_some(opportunity),
                };
                if opportunity.emergency {
                    if regular_fit.is_none() {
                        emergency_fit = Some(selected);
                    }
                } else {
                    regular_fit = Some(selected);
                }
                if matches!(opportunity.kind, InlineBreakKind::Forced) {
                    return selected;
                }
            } else if let Some(position) = regular_fit.or(emergency_fit) {
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
            .unwrap_or_else(|| SelectedInlineLineEnd {
                position: graph.end_position(),
                break_opportunity: None,
            })
    }

    fn mixed_graph_opportunity_allowed(
        &mut self,
        graph: &InlineOpportunityGraph,
        opportunity: InlineBreakOpportunity,
    ) -> bool {
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
                    InlineLineItem::Atom(atom) if atom.content.is_box_edge()
                )
            })
            .map(|run| &run.item)
        else {
            return true;
        };
        !mixed_inline_item_starts_with_suppressed_line_start_punctuation(item)
    }

    fn materialize_inline_line_fragment(
        &mut self,
        graph: &InlineOpportunityGraph,
        range: InlineGraphRange,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
        break_opportunity: Option<InlineBreakOpportunity>,
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
            band.width(),
        );
        let mut materialized =
            graph.materialize_line(range, break_opportunity, &mut self.font_system, block_style);
        let line_available_width = (band.width() - line_indent).max(1.0);
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
        InlineLineFragment {
            items: materialized.items,
            metrics,
            hanging_widths: HangingPunctuationWidths::default(),
            indent: band.left_offset() + line_indent,
            available_width: band.end(),
            suppress_float_adjust: false,
            text: materialized.text,
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
        if items
            .iter()
            .all(|item| matches!(item, InlineLineItem::Fragment(_)))
            && items.iter().any(|item| match item {
                InlineLineItem::Fragment(fragment) => fragment.text.chars().any(|character| {
                    character_is_join_control(character) || character_is_arabic_tatweel(character)
                }),
                InlineLineItem::Atom(_) | InlineLineItem::Float(_) => false,
            })
        {
            return items
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
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
                })
                .collect();
        }
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
                    InlineLineItem::Float(_) => {}
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
                    InlineLineItem::Float(_) => None,
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
        if !mixed_inline_line_needs_bidi_ordering(items, block_style) {
            return items.to_vec();
        }
        let line_items = measured_inline_items(items);
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
                    InlineLineItem::Atom(atom) => {
                        inline_atom_logical_inline_size(atom, block_style)
                    }
                    InlineLineItem::Float(_) => 0.0,
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
    fn inline_line_item_baseline_offset(
        &mut self,
        item: &InlineLineItem,
        block_style: &ComputedStyle,
    ) -> f32 {
        match item {
            InlineLineItem::Fragment(fragment) => {
                if fragment.style.vertical_align.aligns_to_line_box_edge() {
                    return 0.0;
                }
                self.inline_style_baseline_offset(&fragment.style, fragment.baseline_shift)
            }
            InlineLineItem::Atom(atom) => inline_atom_logical_baseline_offset(atom, block_style),
            InlineLineItem::Float(_) => 0.0,
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
    fn inline_line_item_baseline_extents(
        &mut self,
        item: &InlineLineItem,
        block_style: &ComputedStyle,
    ) -> (f32, f32) {
        let baseline = self
            .inline_line_item_baseline_offset(item, block_style)
            .max(0.0);
        let descent = match item {
            InlineLineItem::Fragment(_) => {
                inline_line_item_logical_block_size(item, block_style) - baseline
            }
            InlineLineItem::Atom(_) => {
                (inline_line_item_logical_block_size(item, block_style) - baseline).max(0.0)
            }
            InlineLineItem::Float(_) => 0.0,
        };
        (baseline, descent)
    }

    /// Return whether the item aligns to the line box edge instead of the
    /// shared baseline.
    ///
    /// CSS 2.2 defines `vertical-align: top` and `bottom` as alignment of the
    /// box's margin edge with the line box edge. Those boxes still contribute
    /// to the line box block-size, but they must not add their ascent/descent
    /// to the baseline-aligned strut:
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    fn inline_line_item_aligns_to_line_box_edge(item: &InlineLineItem) -> bool {
        let vertical_align = match item {
            InlineLineItem::Fragment(fragment) => fragment.style.vertical_align,
            InlineLineItem::Atom(atom) => atom.style.vertical_align,
            InlineLineItem::Float(_) => VerticalAlign::BASELINE,
        };
        vertical_align.aligns_to_line_box_edge()
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
    fn mixed_inline_line_metrics<T>(
        &mut self,
        items: &[T],
        block_style: &ComputedStyle,
        width: f32,
    ) -> InlineLineMetrics
    where
        T: AsRef<InlineLineItem>,
    {
        let (baseline_offset, descent) =
            self.mixed_inline_line_baseline_extents(items, block_style);
        let edge_aligned_height = self.mixed_inline_line_edge_aligned_height(items, block_style);
        let text_only_height = items
            .iter()
            .all(|item| matches!(item.as_ref(), InlineLineItem::Fragment(_)))
            .then(|| {
                items
                    .iter()
                    .filter_map(|item| match item.as_ref() {
                        InlineLineItem::Fragment(fragment) => Some(fragment.style.line_height),
                        InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
                    })
                    .fold(block_style.line_height, f32::max)
            });
        InlineLineMetrics {
            width,
            height: text_only_height
                .unwrap_or(baseline_offset + descent)
                .max(edge_aligned_height),
            baseline_offset,
        }
    }

    fn mixed_inline_line_baseline_extents<T>(
        &mut self,
        items: &[T],
        block_style: &ComputedStyle,
    ) -> (f32, f32)
    where
        T: AsRef<InlineLineItem>,
    {
        let (mut baseline_offset, mut descent) = self.inline_style_line_extents(block_style, 0.0);
        for item in items {
            let item = item.as_ref();
            if Self::inline_line_item_aligns_to_line_box_edge(item) {
                continue;
            }
            let (item_baseline_offset, item_descent) =
                self.inline_line_item_baseline_extents(item, block_style);
            baseline_offset = baseline_offset.max(item_baseline_offset);
            descent = descent.max(item_descent);
        }
        (baseline_offset, descent)
    }

    fn mixed_inline_line_edge_aligned_height<T>(
        &mut self,
        items: &[T],
        block_style: &ComputedStyle,
    ) -> f32
    where
        T: AsRef<InlineLineItem>,
    {
        let mut height: f32 = 0.0;
        for item in items {
            let item = item.as_ref();
            if Self::inline_line_item_aligns_to_line_box_edge(item) {
                height = height.max(inline_line_item_logical_block_size(item, block_style));
            }
        }
        height
    }
}

#[derive(Debug, Clone)]
struct RangedMixedInlineLineItem {
    item: InlineLineItem,
    range: std::ops::Range<usize>,
}

fn mixed_inline_line_needs_bidi_ordering<T>(items: &[T], block_style: &ComputedStyle) -> bool
where
    T: AsRef<InlineLineItem>,
{
    block_style.direction == Direction::Rtl
        || inline_bidi_scope_affects_line_ordering(block_style)
        || items.iter().any(|item| match item.as_ref() {
            InlineLineItem::Fragment(fragment) => {
                contains_bidi_text(&fragment.text)
                    || inline_bidi_scope_affects_line_ordering(&fragment.style)
            }
            InlineLineItem::Atom(atom) => {
                atom.style.direction != block_style.direction
                    || inline_bidi_scope_affects_line_ordering(&atom.style)
            }
            InlineLineItem::Float(_) => false,
        })
}

fn graph_remaining_allows_last_hanging_punctuation(
    graph: &InlineOpportunityGraph,
    position: InlineGraphPosition,
) -> bool {
    let Some(run_range) = graph.run_indices_for_graph_range(InlineGraphRange {
        start: position,
        end: graph.end_position(),
    }) else {
        return true;
    };
    run_range.into_iter().all(|run_index| {
        let run = &graph.runs[run_index];
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                let start = if run_index == position.run_index {
                    position.byte_offset.min(fragment.text.len())
                } else {
                    0
                };
                fragment
                    .text
                    .get(start..)
                    .is_none_or(|text| text.chars().all(is_css_collapsible_whitespace))
            }
            InlineLineItem::Atom(atom) => atom.content.is_box_edge(),
            InlineLineItem::Float(_) => false,
        }
    })
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
            InlineLineItem::Float(_) => {}
        }
        let end = text.len();
        ranged.push(RangedMixedInlineLineItem {
            item: item.clone(),
            range: start..end,
        });
    }
    (text, ranged)
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
