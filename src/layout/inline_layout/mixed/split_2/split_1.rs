use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn inline_float_band_for_line(
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
                    clear_after: Clear::None,
                    block_start_trim: 0.0,
                    block_end_trim: 0.0,
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
            fragment_text_box_trim: TextBoxLineTrim::default(),
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
            let mut end = if selected_end.position <= start {
                graph_end
            } else {
                selected_end.position.min(graph_end)
            };
            end = line_end_extended_over_adjacent_inline_float_markers(graph, end);
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
                let placement_snapshot = self.snapshot();
                if let Some(placement) = self.try_place_inline_float_on_current_line(
                    graph,
                    float_position,
                    prefix.metrics.width,
                    context,
                    line_index,
                    line_starts_after_forced_break,
                ) {
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
                        fragments.push(combined.fragment);
                        line_index += 1;
                        start = end;
                        continue;
                    }
                    if !suffix_is_empty && !placement.fits_remaining_band() {
                        self.restore(placement_snapshot);
                        prefix.suppress_float_adjust = false;
                        fragments.push(prefix);
                        line_index += 1;
                        start = float_position;
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
    pub(in crate::layout) fn try_select_unbreakable_line_with_inline_floats(
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

    pub(in crate::layout) fn place_inline_waiting_float(
        &mut self,
        float: &InlineFloat,
        context: InlineParagraphContext<'_>,
        line_index: usize,
    ) {
        let saved_cursor_y = self.cursor_y;
        let target_top = self.cursor_y - context.block_style.line_height * line_index as f32;
        let line_left = self.content_left + context.padding_left;
        self.cursor_y = target_top;
        let mut run = self.float_run_state();
        let pushed_containing_block =
            self.push_inline_float_positioning_containing_block(float, line_left, 0.0, target_top);
        self.layout_floating_child(
            float.element(),
            float.signature().clone(),
            float.style(),
            None,
            None,
            context.stylesheets,
            &mut run,
        );
        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        self.cursor_y = saved_cursor_y;
    }

    pub(in crate::layout) fn try_place_unbreakable_inline_float(
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
        let pushed_containing_block =
            self.push_inline_float_positioning_containing_block(&float, line_left, 0.0, target_top);
        let placed = self.layout_floating_child(
            float.element(),
            float.signature().clone(),
            float.style(),
            None,
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

    pub(in crate::layout) fn try_place_inline_float_on_current_line(
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
        let pushed_containing_block = self.push_inline_float_positioning_containing_block(
            &float,
            line_left,
            prefix_width,
            target_top,
        );
        let placed = self.layout_floating_child(
            float.element(),
            float.signature().clone(),
            float.style(),
            None,
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
                    let fits_remaining_band = shape.left() + INLINE_FLOAT_EPSILON >= remaining_left
                        && shape.right() <= remaining_right + INLINE_FLOAT_EPSILON;
                    (shape.page_index == self.pages.len()
                        && (shape.top() - target_top).abs() <= INLINE_FLOAT_EPSILON
                        && (fits_remaining_band
                            || shape.right() - shape.left()
                                > remaining_right - remaining_left + INLINE_FLOAT_EPSILON))
                        .then_some(InlineFloatPlacement::new(
                            line_left,
                            line_right,
                            prefix_width,
                            shape.left(),
                            shape.right(),
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
        line_left: f32,
        prefix_width: f32,
        line_top: f32,
    ) -> bool {
        let Some(style) = float.positioning_containing_block_style() else {
            return false;
        };
        let border_widths = used_border_widths(style);
        let horizontal_non_padding =
            style.margin.left + border_widths.left + border_widths.right + style.margin.right;
        let padding_box_width =
            (prefix_width - horizontal_non_padding).max(style.padding.left + style.padding.right);
        // A float after an inline start edge is placed while line selection is
        // still ahead of painting. Rebase edge-only prefixes to the generated
        // inline line slot that will own the positioned ancestor's padding box.
        let line_top = if prefix_width > 0.0
            && prefix_width
                <= horizontal_non_padding + style.padding.left + style.padding.right + 0.01
        {
            line_top - 2.0 * style.line_height
        } else {
            line_top
        };
        let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
            line_left + style.margin.left + border_widths.left,
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

    pub(in crate::layout) fn select_inline_line_end_for_width(
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
                (materialized.content_width
                    - materialized.hanging_space_width
                    - hanging_widths.start
                    - hanging_widths.end)
                    .max(0.0)
            };
            if fit_width <= line_available_width + 0.5 {
                let selected = SelectedInlineLineEnd {
                    position: opportunity.position,
                    break_opportunity: (opportunity.position < graph.end_position())
                        .then_some(opportunity),
                };
                if opportunity.emergency {
                    if regular_fit.is_none()
                        && emergency_fit.is_none_or(|fit| selected.position > fit.position)
                    {
                        emergency_fit = Some(selected);
                    }
                } else if regular_fit.is_none_or(|fit| selected.position > fit.position) {
                    regular_fit = Some(selected);
                }
                if matches!(opportunity.kind, InlineBreakKind::Forced) {
                    return selected;
                }
            } else if opportunity.emergency && regular_fit.is_some() {
                continue;
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

    pub(in crate::layout) fn mixed_graph_opportunity_allowed(
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
        InlineLineFragment::new(
            materialized.items,
            metrics,
            HangingPunctuationWidths::default(),
            band.left_offset() + line_indent,
            band.end(),
            false,
            materialized.text,
        )
    }
}
