use super::*;

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
}

/// Mutable result state for one balance-candidate search.
#[derive(Default)]
struct BalancedLineSearchState {
    examined: usize,
    exhausted_budget: bool,
    best: Option<BalancedLineCandidate>,
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
        let block_advance = block_style.line_height * line_index as f32;
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
        let position = self.inline_line_physical_position(line_index, block_style);
        if block_style.writing_mode != WritingMode::HorizontalTb {
            let band = self.current_logical_float_band(
                block_style.writing_mode,
                block_style.direction,
                position.content_left + padding_left,
                block_style.line_height,
                position.cursor_y,
                available_width,
            );
            return InlineFloatBand::new(band.inline_start(), band.available_inline_size());
        }
        let band = self.current_float_band(position.cursor_y, block_style.line_height);
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
        let paragraph_start_line_index = line_index;
        let graph = self.build_inline_opportunity_graph(items, block_style);
        let graph = if line_index == 0 {
            self.graph_with_first_letter_pseudo(&graph, block_style)
        } else {
            graph
        };
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
                    is_first_formatted_line: next_record_line_index == 0,
                    is_last_line_in_paragraph: false,
                    is_forced_empty: true,
                    starts_after_preserved_segment_break: false,
                    clear_after: Clear::None,
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
            let line_height = line_box.metrics.height.max(block_style.line_height);
            let used_indent = line_box.indent;
            let available_width = line_box.available_width;
            let is_phantom = inline_line_fragment_is_phantom(&line_box);
            records.push(InlineLineRecord {
                paragraph_index: 0,
                block_line_index,
                paragraph_line_index: records.len(),
                fragment: Some(line_box),
                is_phantom,
                is_first_formatted_line: block_line_index == 0,
                is_last_line_in_paragraph: offset + 1 == line_count,
                is_forced_empty: false,
                starts_after_preserved_segment_break: false,
                clear_after: Clear::None,
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
        mut line_index: usize,
        starts_after_forced_break: bool,
    ) -> SelectedInlineLines {
        if graph.is_empty() {
            return SelectedInlineLines {
                fragments: Vec::new(),
                next_line_index: line_index,
                has_float_side_effects: false,
            };
        }
        // Once an inline-source float is positioned, it remains in the graph
        // as a zero-advance source-order marker while its exclusion changes
        // the available bands.  Keeping its position separate lets a retry
        // select the whole affected line against that new band instead of
        // treating preceding inline content as an in-flow prefix of the
        // float.
        let placed_inline_float_positions = Vec::new();
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
        let paragraph_has_soft_wrap =
            graph
                .break_opportunities_after(graph.start_position())
                .any(|opportunity| {
                    opportunity.position < graph.end_position()
                        && !matches!(opportunity.kind, InlineBreakKind::Forced)
                        && graph.float_at_position(opportunity.position).is_none()
                });
        let paragraph_start_line_index = line_index;
        let mut fragments = Vec::new();
        let mut has_float_side_effects = false;
        let mut start = graph.start_position();
        let graph_end = graph.end_position();
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
            if let Some(float) = graph.float_at_position(start).cloned() {
                self.place_inline_waiting_float(&float, context, line_index);
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
                is_first_formatted_line: paragraph_start_line_index == 0 && fragments.is_empty(),
            };
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
                    self.select_inline_line_end(graph, start, context, line_index, line_identity)
                });
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
            if is_final_clamped_line && selected_end.position < graph_end && balanced_plan.is_none()
            {
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
                    if marker_selected.position > start {
                        selected_end = marker_selected;
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
            let mut end = if (!paragraph_has_soft_wrap && !selected_forced_break)
                || selected_end.position <= start
            {
                graph_end
            } else {
                selected_end.position.min(graph_end)
            };
            let end_is_unplaced_float_boundary =
                selected_end.break_opportunity.is_some_and(|opportunity| {
                    matches!(opportunity.kind, InlineBreakKind::AtomicBoundary)
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
            let current_available_width = (band.width() - line_indent).max(0.0);
            let full_available_width = (context.available_width
                - used_line_indent_for_formatted_line(
                    line_identity.is_first_formatted_line,
                    line_identity.starts_after_forced_break,
                    context.hanging_indent,
                    context.block_style,
                    context.available_width,
                ))
            .max(0.0);
            let selected_range_allows_soft_wrap = graph.runs[start.run_index..end.run_index]
                .iter()
                .any(|run| inline_line_item_allows_soft_wrap(&run.item));
            if selected_range_allows_soft_wrap
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
                && materialized.fitting_width > current_available_width + INLINE_FLOAT_EPSILON
                && materialized.fitting_width <= full_available_width + INLINE_FLOAT_EPSILON
            {
                line_index += 1;
                balanced_plan = None;
                continue;
            }
            let selected_range = InlineGraphRange { start, end };
            if !paragraph_has_soft_wrap
                && let Some(fragment) = self.try_select_unbreakable_line_with_inline_floats(
                    graph,
                    selected_range,
                    selected_end,
                    context,
                    line_index,
                    line_identity,
                )
            {
                fragments.push(SelectedInlineLine {
                    fragment,
                    line_index,
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
                if float_position <= start {
                    if let Some(float) = graph.float_at_position(float_position).cloned() {
                        self.place_inline_waiting_float(&float, context, line_index);
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
                    line_identity,
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
                        fragments.push(SelectedInlineLine {
                            fragment: combined.fragment,
                            line_index,
                        });
                        line_index += 1;
                        balanced_plan_index += 1;
                        start = end;
                        continue;
                    }
                    if !suffix_is_empty && !placement.fits_remaining_band() {
                        self.restore(placement_snapshot);
                        prefix.suppress_float_adjust = false;
                        fragments.push(SelectedInlineLine {
                            fragment: prefix,
                            line_index,
                        });
                        line_index += 1;
                        balanced_plan_index += 1;
                        start = float_position;
                        continue;
                    }
                    fragments.push(SelectedInlineLine {
                        fragment: prefix,
                        line_index,
                    });
                    line_index += 1;
                    balanced_plan_index += 1;
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
                let mut fragment = self.materialize_inline_line_fragment(
                    graph,
                    InlineGraphRange {
                        start,
                        end: float_position,
                    },
                    context,
                    line_index,
                    line_identity,
                    break_opportunity,
                );
                self.register_initial_letter_exclusion_for_line(&mut fragment, context, line_index);
                fragments.push(SelectedInlineLine {
                    fragment,
                    line_index,
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
                line_index,
                line_identity,
                break_opportunity,
            );
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
            self.register_initial_letter_exclusion_for_line(&mut fragment, context, line_index);
            fragments.push(SelectedInlineLine {
                fragment,
                line_index,
            });
            line_index += 1;
            balanced_plan_index += 1;
            start = end;
        }
        if context
            .block_style
            .line_clamp
            .as_ref()
            .is_some_and(|line_clamp| line_index == line_clamp.max_lines)
            && start < graph_end
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
        if let Some(line_clamp) = &context.block_style.line_clamp {
            clamped_has_overflow = normal.len() > line_clamp.max_lines;
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
        // fragment's font, decoration, or line-height.
        let mut style = context.block_style.clone();
        style.line_height = 0.0;
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

    fn balance_line_plan_group(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        normal: &[BalancedLinePlanEntry],
        output: &mut [BalancedLinePlanEntry],
        group_range: std::ops::Range<usize>,
        includes_clamp_ellipsis: bool,
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
        let group_end_position = group.last().expect("non-empty balance group").end.position;
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

    fn register_initial_letter_exclusion_for_line(
        &mut self,
        fragment: &mut InlineLineFragment,
        context: InlineParagraphContext<'_>,
        line_index: usize,
    ) {
        if line_index != 0 || context.block_style.writing_mode != WritingMode::HorizontalTb {
            return;
        }
        let Some((item_width, style)) = fragment.items().iter().find_map(|item| {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                return None;
            };
            (!fragment.style().initial_letter.is_normal()).then_some((item.width, fragment.style()))
        }) else {
            return;
        };
        let Some((size, sink)) = style.initial_letter.specified() else {
            return;
        };
        let impacted_lines = (size.ceil() as u32).max(sink).max(1);
        if impacted_lines <= 1 {
            return;
        }
        let border_widths = used_border_widths(style);
        let exclusion_width = (item_width
            + style.margin.left
            + border_widths.left
            + style.padding.left
            + style.padding.right
            + border_widths.right
            + style.margin.right)
            .max(0.0);
        if exclusion_width <= INLINE_FLOAT_EPSILON {
            return;
        }
        let inline_start = inline_start_side(
            context.block_style.writing_mode,
            context.block_style.direction,
        );
        if inline_start != PhysicalSide::Left {
            return;
        }
        fragment.suppress_float_adjust = true;
        let x = self.content_left + context.padding_left + fragment.indent;
        let height = impacted_lines as f32 * context.block_style.line_height;
        let shape = FloatShape::from_rect(
            self.next_float_id(),
            Float::Left,
            UsedFloatSide::Left,
            self.next_paint_source_order(),
            self.current_float_page_index(),
            PageTopRect::new(x, self.cursor_y, exclusion_width, height),
        );
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
        line_index: usize,
        line_identity: SelectedLineIdentity,
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
            line_identity,
            break_opportunity,
        );
        let snapshot = self.snapshot();
        let mut search_start = range.start;
        while let Some(float_position) = graph.first_float_position_in_range(InlineGraphRange {
            start: search_start,
            end: range.end,
        }) {
            // A float after an unbreakable inline prefix cannot affect that
            // prefix's line box. Preserve the whole unbreakable source run
            // as one line and place a fitting float at that line's top. CSS
            // 2.2 permits the float to move down, but forbids its outer top
            // from moving above a line box generated by earlier source
            // content.
            // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
            if float_position > range.start {
                if context.block_style.white_space == WhiteSpace::NoWrap {
                    if !self.try_place_inline_float_in_line_band(
                        graph,
                        float_position,
                        context,
                        line_index,
                        line_identity,
                    ) {
                        self.restore(snapshot);
                        return None;
                    }
                } else {
                    self.restore(snapshot);
                    return None;
                }
            } else if float_position == range.start
                && !self.try_place_inline_float_in_line_band(
                    graph,
                    float_position,
                    context,
                    line_index,
                    line_identity,
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
        self.containing_block_direction = block_style.direction;
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
                    let fits_remaining_band = shape.left() + INLINE_FLOAT_EPSILON >= remaining_left
                        && shape.right() <= remaining_right + INLINE_FLOAT_EPSILON;
                    let accepted = shape.page_index == self.pages.len()
                        && (shape.top() - target_top).abs() <= INLINE_FLOAT_EPSILON
                        && (fits_remaining_band
                            || (prefix_width <= INLINE_FLOAT_EPSILON
                                && shape.right() - shape.left()
                                    > remaining_right - remaining_left + INLINE_FLOAT_EPSILON));
                    accepted.then_some(InlineFloatPlacement::new(
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
                inline_positioning_source_margin_edge_left(
                    graph,
                    position,
                    source.id,
                    line_left,
                    prefix_width,
                )
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
        line_identity: SelectedLineIdentity,
    ) -> SelectedInlineLineEnd {
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
        let mut regular_fit = None::<SelectedInlineLineEnd>;
        let mut emergency_fit = None::<SelectedInlineLineEnd>;
        let opportunities = graph.break_opportunities_after(start).collect::<Vec<_>>();
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
                (materialized.fitting_width - hanging_widths.start - hanging_widths.end).max(0.0)
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
        line_identity: SelectedLineIdentity,
        break_opportunity: Option<InlineBreakOpportunity>,
    ) -> InlineLineFragment {
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
        let terminal_pre_wrap_hang = line_identity.starts_after_forced_break
            && break_opportunity.is_none()
            && range.end == graph.end_position();
        let mut materialized = graph.materialize_line_with_terminal_pre_wrap_hang(
            range,
            break_opportunity,
            terminal_pre_wrap_hang,
            &mut self.font_system,
            block_style,
        );
        let line_available_width = (band.width() - line_indent).max(0.0);
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
        .with_edge_effects(materialized.edge_effects.clone())
    }
}

fn inline_positioning_source_margin_edge_left(
    graph: &InlineOpportunityGraph,
    float_position: InlineGraphPosition,
    source_id: InlinePositioningContainingBlockId,
    line_left: f32,
    prefix_width: f32,
) -> Option<f32> {
    // CSS 2.2 uses the nearest positioned inline ancestor's padding box, not
    // the float's own inline position, as the containing block for abspos
    // descendants:
    // <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
    let mut width_from_source_start_to_float = 0.0;
    for run in graph.runs[..float_position.run_index].iter().rev() {
        width_from_source_start_to_float += run.width;
        if inline_run_is_positioning_source_start_edge(run, source_id) {
            return Some(line_left + prefix_width - width_from_source_start_to_float);
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
    use super::balanced_remaining_space_variance;

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
}
