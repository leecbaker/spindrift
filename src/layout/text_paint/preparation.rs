use std::rc::Rc;

use super::*;
use crate::text::TextTypesettingPlan;

struct InlineTextPrepSpan<'a, F: InlineFragmentAccess> {
    fragment: &'a F,
    text: &'a str,
}

impl<'a, F: InlineFragmentAccess> InlineTextPrepSpan<'a, F> {
    fn new(fragment: &'a F) -> Self {
        Self {
            fragment,
            text: fragment.text(),
        }
    }
}

/// Reconstruct plain text for a paint group that shares a transformed
/// separator's glyph stream with adjacent text.
///
/// CSS Text transforms the separator only for styling. PDF output therefore
/// needs one group-level ActualText value whenever the group contains an
/// explicit replacement, even though its shaped text contains U+0020 or
/// U+3000 instead.
/// <https://drafts.csswg.org/css-text-4/#word-space-transform>
fn word_space_transform_actual_text<F: InlineFragmentAccess>(fragments: &[F]) -> Option<Rc<str>> {
    let mut actual_text = String::new();
    let mut has_transformed_separator = false;
    for fragment in fragments {
        match fragment.source() {
            InlineTextSource::WordSpaceTransform(separator) => {
                actual_text.push_str(separator.extraction_text().unwrap_or(""));
                has_transformed_separator = true;
            }
            _ => actual_text.push_str(fragment.text()),
        }
    }
    has_transformed_separator.then(|| Rc::from(actual_text))
}

fn inline_text_prep_span_is_join_control_only<F: InlineFragmentAccess>(
    span: &InlineTextPrepSpan<'_, F>,
) -> bool {
    !span.text.is_empty() && span.text.chars().all(character_is_join_control)
}

fn can_shape_inline_text_prep_spans_together<F: InlineFragmentAccess>(
    left: &InlineTextPrepSpan<'_, F>,
    right: &InlineTextPrepSpan<'_, F>,
) -> bool {
    if inline_text_prep_span_is_join_control_only(left) {
        return !inline_bidi_isolation_boundary_breaks_shaping(left.fragment, right.fragment);
    }
    if inline_text_prep_span_is_join_control_only(right) {
        return !inline_bidi_isolation_boundary_breaks_shaping(left.fragment, right.fragment);
    }
    left.fragment.style().vertical_align == right.fragment.style().vertical_align
        && left.fragment.style().writing_mode == right.fragment.style().writing_mode
        && left.fragment.style().language == right.fragment.style().language
        // A text group is also the smallest paint subtree emitted by this
        // path. Do not let boundary shaping merge across an opacity stacking
        // context, whose paint must be composited atomically.
        // <https://www.w3.org/TR/css-color-4/#transparency>
        && left.fragment.style().opacity == right.fragment.style().opacity
        && inline_ancestor_decorations_have_same_text_paint_effect(
            left.fragment.ancestor_inline_decorations(),
            right.fragment.ancestor_inline_decorations(),
        )
        && left.fragment.resolved_bidi_direction() == right.fragment.resolved_bidi_direction()
        && !inline_bidi_isolation_boundary_breaks_shaping(left.fragment, right.fragment)
}

/// Whether two decoration chains have the same origins in propagation order.
///
/// Declaration values are intentionally not compared here.  Two nested boxes
/// can declare equal-looking lines, yet CSS Text gives each decorating box an
/// independent propagated origin.
fn text_decoration_origin_chains_match(
    left: &[css::TextDecorationLayer],
    right: &[css::TextDecorationLayer],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| Rc::ptr_eq(&left.origin_style, &right.origin_style))
}

/// Append paint-only decoration provenance for one shared shaping group.
///
/// The source ranges index the group text passed to the shaper, so their
/// physical positions are recovered from the returned glyph clusters before
/// those runs are appended to the larger prepared group.  This is deliberately
/// independent of `can_shape_inline_text_prep_spans_together`: a transparent
/// inline boundary may retain joining context while still changing which
/// lexical receiver owns a decoration.
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>
/// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
fn append_text_decoration_provenance<F: InlineFragmentAccess>(
    provenance: &mut Vec<PreparedTextDecorationProvenanceSegment>,
    group: &[InlineTextPrepSpan<'_, F>],
    shaped: &ShapedInlineLine,
    group_inline_offset: f32,
) {
    let mut source_start = 0;
    for span in group {
        let source_end = source_start + span.text.len();
        let source_range = source_start..source_end;
        source_start = source_end;
        let Some((start, end)) = shaped.source_range_inline_span(source_range) else {
            // An advance-only source (notably a no-break space) can occupy
            // inline measure without yielding a paintable glyph cluster. A
            // single lexical receiver owns that complete shaped group, so it
            // still needs a decoration receiver range.  Do not generalize
            // this fallback across multiple lexical spans: their boundary
            // positions require cluster provenance to remain distinct.
            if group.len() != 1 || shaped.advance_width() <= 0.0 {
                continue;
            }
            let layers = span
                .fragment
                .style()
                .text_decoration_origins
                .effective_layers_vec();
            provenance.push(PreparedTextDecorationProvenanceSegment {
                layers,
                receivers: vec![PreparedTextDecorationReceiver {
                    inline_span: TextInlineSpan::from_start_and_length(
                        group_inline_offset,
                        shaped.advance_width(),
                    ),
                    style: span.fragment.style().clone(),
                }],
            });
            continue;
        };
        if end <= start {
            continue;
        }
        let layers = span
            .fragment
            .style()
            .text_decoration_origins
            .effective_layers_vec();
        let receiver = PreparedTextDecorationReceiver {
            inline_span: TextInlineSpan::new(
                group_inline_offset + start,
                group_inline_offset + end,
            ),
            style: span.fragment.style().clone(),
        };
        if let Some(segment) = provenance
            .last_mut()
            .filter(|segment| text_decoration_origin_chains_match(&segment.layers, &layers))
        {
            if let Some(previous) = segment.receivers.last_mut().filter(|previous| {
                previous.style == receiver.style
                    && receiver.inline_span.start <= previous.inline_span.end + 0.01
            }) {
                previous.inline_span.end = previous.inline_span.end.max(receiver.inline_span.end);
            } else {
                segment.receivers.push(receiver);
            }
        } else {
            provenance.push(PreparedTextDecorationProvenanceSegment {
                layers,
                receivers: vec![receiver],
            });
        }
    }
}

/// Join visual fragments that retain glyphs from selected source slices.
///
/// Inline bidi ordering may split one selected source range into several
/// paint fragments. Re-shaping their visual text would discard the original
/// cursive forms; composing their already-shaped visual runs keeps the source
/// shaping while preserving the selected line's paint order. A control-only
/// fragment can own a U+200C/U+200D source range without owning a paintable
/// glyph, so it is deliberately permitted to have no selected source slice:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/css-writing-modes-4/#bidi-linebox>.
fn compose_selected_source_shapes<F: InlineFragmentAccess>(
    group: &[InlineTextPrepSpan<'_, F>],
    text: &str,
) -> Option<ShapedInlineLine> {
    if group.is_empty() {
        return None;
    }
    let source = group
        .iter()
        .find_map(|span| span.fragment.source_shaped_selection())?
        .selected();
    let run_capacity = group
        .iter()
        .filter_map(|span| span.fragment.source_shaped_selection())
        .map(|selection| selection.selected().runs.len())
        .sum();
    let mut selections = Vec::with_capacity(group.len());
    for span in group {
        let Some(selection) = span.fragment.source_shaped_selection() else {
            if inline_text_prep_span_is_join_control_only(span) {
                continue;
            }
            return None;
        };
        if !selection.is_reusable_for(span.text, span.fragment.resolved_bidi_direction()) {
            return None;
        }
        selections.push(selection);
    }
    if let Some(shaped) = SourceShapedSelection::combine_contiguous(&selections) {
        return Some(shaped);
    }
    let typesetting_plan = TextTypesettingPlan::resolve(text, group.first()?.fragment.style());

    let mut result = ShapedInlineLine {
        text: Rc::from(text),
        width: 0.0,
        offset: source.offset,
        aligned_by_parley: source.aligned_by_parley,
        line_height: source.line_height,
        baseline_adjustment: source.baseline_adjustment,
        typesetting_plan,
        runs: Vec::with_capacity(run_capacity),
        monotonic_source_advance_index: Default::default(),
    };
    let mut width = 0.0;
    for span in group {
        let Some(selection) = span.fragment.source_shaped_selection() else {
            if inline_text_prep_span_is_join_control_only(span) {
                continue;
            }
            return None;
        };
        if !selection.is_reusable_for(span.text, span.fragment.resolved_bidi_direction()) {
            return None;
        }
        let shaped = selection.selected();
        for run in &shaped.runs {
            let mut run = run.clone();
            run.x_offset += width;
            result.runs.push(run);
        }
        width += shaped.advance_width();
    }
    result.width = width;
    Some(result)
}

/// Return a complete shaped source covered by every member of this paint group.
///
/// A strict source slice intentionally rejects a range that cuts through an
/// OpenType cluster. That is correct for soft-wrap selection, but not for a
/// transparent inline boundary: the complete selected source remains present
/// in this group and must be emitted once with its original cluster geometry.
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
fn complete_boundary_shaped_source<F: InlineFragmentAccess>(
    group: &[InlineTextPrepSpan<'_, F>],
) -> Option<ShapedInlineLine> {
    let source = group
        .iter()
        .find_map(|span| span.fragment.boundary_shaped_source())?;
    let all_share_source = group.iter().all(|span| {
        span.fragment
            .boundary_shaped_source()
            .is_some_and(|candidate| std::ptr::eq(source, candidate))
    });
    if !all_share_source {
        let group_text = group.iter().map(|span| span.text).collect::<String>();
        // A visual bidi slice can clear an individual fragment's boundary
        // provenance while retaining all of its text.  The complete source is
        // still the only shape that represents the group when its authored
        // text is intact, including ZWJ/ZWNJ that participate in joining but
        // have no painted glyph of their own.
        // <https://www.w3.org/TR/css-text-3/#boundary-shaping>
        if group_text == source.shaped.text.as_ref() {
            return Some(source.shaped.as_ref().clone());
        }
        return None;
    }

    let mut selected_ranges = group
        .iter()
        .map(|span| span.fragment.boundary_shaped_range().cloned())
        .collect::<Option<Vec<_>>>()?;
    selected_ranges.sort_by_key(|range| range.start);
    let source_length = source.shaped.text.len();
    let fully_covers_source = selected_ranges
        .iter()
        .try_fold(0, |end, range| (range.start == end).then_some(range.end))
        == Some(source_length);
    fully_covers_source.then(|| source.shaped.as_ref().clone())
}

impl<'a> LayoutBuilder<'a> {
    /// Prepare adjacent inline fragments as one shaped text group.
    ///
    /// CSS Text boundary shaping can span eligible inline element boundaries.
    /// Preparation owns trimming, join-control grouping, Parley shaping, and
    /// final line-baseline positioning; later paint code only consumes the
    /// stored shaped artifact:
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
    /// <https://www.w3.org/TR/css-inline-3/#line-box>.
    #[cfg(test)]
    pub(in crate::layout) fn prepare_inline_text_group(
        &mut self,
        fragments: &[InlineFragment],
        x: f32,
    ) -> Option<PreparedInlineTextGroup> {
        let tab_metric_style = fragments.first()?.style();
        self.prepare_inline_text_group_with_summary_policy(
            fragments,
            x,
            false,
            false,
            x,
            tab_metric_style,
        )
    }

    pub(in crate::layout) fn prepare_inline_text_group_with_summary_policy<
        F: InlineFragmentAccess,
    >(
        &mut self,
        fragments: &[F],
        x: f32,
        preserve_leading_summary_space: bool,
        synthesize_leading_summary_space: bool,
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
    ) -> Option<PreparedInlineTextGroup> {
        let first = fragments.first()?;
        let mut shaped_runs = Vec::new();
        let mut decoration_provenance = Vec::new();
        let mut width = 0.0f32;
        let mut shaping_groups = Vec::<Vec<InlineTextPrepSpan<'_, F>>>::new();

        for fragment in fragments {
            let span = InlineTextPrepSpan::new(fragment);
            if let Some(group) = shaping_groups.last_mut()
                && let Some(last) = group.last()
                && can_shape_inline_text_prep_spans_together(last, &span)
            {
                group.push(span);
                continue;
            }
            shaping_groups.push(vec![span]);
        }

        for group in &shaping_groups {
            let spans = group
                .iter()
                .map(|span| StyledTextSpan {
                    text: span.text,
                    style: span.fragment.style(),
                })
                .collect::<Vec<_>>();
            let group_text = spans.iter().map(|span| span.text).collect::<String>();
            let resolved_direction = group
                .first()
                .and_then(|span| span.fragment.resolved_bidi_direction())
                .unwrap_or(ResolvedBidiDirection::Ltr);
            // A join-control-only neighbor may have been folded into this
            // visual span above. A selected source shape retains the logical
            // joining context through that reordering; re-shaping visual
            // fragments cannot recover a U+200C/U+200D's source position
            // after UAX #9 removes it from visual clusters.
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
            // <https://www.w3.org/TR/alreq/#h_joining-enforcement>
            // A cached source shape can have resolved a preserved tab before
            // the selected forced break. Re-shape tab-containing selected
            // groups from this line's content edge.
            // <https://www.w3.org/TR/css-text-3/#tab-size-property>
            let has_join_control = group_text
                .chars()
                .any(crate::text::character_is_join_control);
            let has_joining_behavior = group_text
                .chars()
                .any(crate::text::character_has_cursive_shaping_behavior);
            // Visual reordering reverses the order of the separate inline
            // fragments in an RTL run, while retaining the source order
            // inside each fragment. Keep join controls as their own spans so
            // their logical position can be restored for cursive shaping.
            // The resulting glyph stream remains visual, as produced by the
            // RTL shaper, and is therefore ready for painting.
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
            // <https://www.unicode.org/reports/tr9/#Reordering_Resolved_Levels>
            let logical_joining_spans = ((has_join_control || has_joining_behavior)
                && resolved_direction == ResolvedBidiDirection::Rtl)
                .then(|| {
                    let shared_boundary_source = group
                        .first()
                        .and_then(|span| span.fragment.boundary_shaped_source());
                    let all_share_boundary_source = shared_boundary_source.is_some_and(|source| {
                        group.iter().all(|span| {
                            span.fragment
                                .boundary_shaped_source()
                                .is_some_and(|candidate| std::ptr::eq(source, candidate))
                        })
                    });
                    if all_share_boundary_source {
                        let mut indices = (0..group.len()).collect::<Vec<_>>();
                        indices.sort_by_key(|&index| {
                            group[index]
                                .fragment
                                .boundary_shaped_range()
                                .expect("shared boundary source has ranges")
                                .start
                        });
                        indices
                            .into_iter()
                            .map(|index| spans[index])
                            .collect::<Vec<_>>()
                    } else {
                        // UAX #9 gives join controls no visual position. When
                        // an author puts one in its own transparent inline span,
                        // it can consequently appear after the visible
                        // character that precedes it in visual order. Keep
                        // that control with the preceding visual unit before
                        // reversing RTL units back into their shaping order.
                        // This restores `beh ZWNJ` rather than turning it
                        // into `ZWNJ beh`.
                        // <https://www.unicode.org/reports/tr9/#X9> and
                        // <https://www.w3.org/TR/alreq/#h_joining-enforcement>
                        let mut units = Vec::<Vec<StyledTextSpan<'_>>>::new();
                        for span in &spans {
                            if span.text.chars().all(character_is_join_control)
                                && let Some(unit) = units.last_mut()
                            {
                                unit.push(*span);
                            } else {
                                units.push(vec![*span]);
                            }
                        }
                        units.into_iter().rev().flatten().collect()
                    }
                });
            let shaping_spans = logical_joining_spans.as_deref().unwrap_or(&spans);
            // A selected source glyph stream retains the source run's RTL
            // character placement. The visual item sequence has already
            // performed UAX #9 L2, so a non-joining RTL group must be shaped
            // from that sequence instead of borrowing the logical source
            // placement. Joining text and join controls are the exception:
            // their contextual forms cannot be reconstructed from separated
            // visual fragments.
            // <https://www.unicode.org/reports/tr9/#L2>
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping>
            let can_reuse_boundary_shape = resolved_direction != ResolvedBidiDirection::Rtl
                || has_join_control
                || has_joining_behavior;
            let reused_boundary_shape = (can_reuse_boundary_shape && !group_text.contains('\t'))
                .then(|| complete_boundary_shaped_source(group))
                .flatten();
            let reused_selected_shape = reused_boundary_shape.or_else(|| {
                if !group_text.contains('\t')
                    && group.len() == 1
                    && group[0].text == group[0].fragment.text()
                    && group[0]
                        .fragment
                        .source_shaped_selection()
                        .is_some_and(|selection| {
                            selection.is_reusable_for(
                                group[0].text,
                                group[0].fragment.resolved_bidi_direction(),
                            )
                        })
                {
                    group[0]
                        .fragment
                        .source_shaped_selection()
                        .map(|selection| selection.selected().clone())
                } else if !group_text.contains('\t')
                    && group.iter().all(|span| span.text == span.fragment.text())
                {
                    // A typed source selection carries the complete logical
                    // source and resolved visual context for every span, so
                    // cursive text is precisely the case where composition
                    // must reuse it rather than independently re-shape.
                    compose_selected_source_shapes(group, &group_text)
                } else {
                    None
                }
            });
            let shaped = reused_selected_shape.or_else(|| {
                self.font_system
                    .shape_untracked_visually_ordered_inline_fragments(
                        shaping_spans,
                        group_text,
                        0.0,
                        first.style().line_height,
                        tab_origin + width,
                        tab_metric_style,
                        resolved_direction,
                    )
            });
            if let Some(mut shaped) = shaped {
                let group_width = shaped.advance_width();
                append_text_decoration_provenance(
                    &mut decoration_provenance,
                    group,
                    &shaped,
                    width,
                );
                shaped_runs.extend(shaped.runs.drain(..).map(|mut run| {
                    run.x_offset += width;
                    run
                }));
                width += group_width;
            }
        }

        let text_summary = inline_fragment_text_summary(
            fragments,
            preserve_leading_summary_space,
            synthesize_leading_summary_space,
        );
        if shaped_runs.is_empty() || text_summary.is_empty() {
            return None;
        }

        // The shaped run, rather than the style fallback alone, owns the
        // glyph program that will receive the paint-origin conversion. This
        // is especially important when a nested atomic inline and adjacent
        // source both resolve through a fallback face.
        let first_font_id = shaped_runs
            .first()
            .and_then(|run| run.font_id)
            .or_else(|| self.font_system.resolve_style(first.style()));
        let line_height = self
            .font_system
            .line_height_for_font(first_font_id, first.style())
            .points();
        let baseline_adjustment = self
            .font_system
            .layout_to_program_baseline_adjustment(first_font_id, first.style(), line_height)
            .points();
        let typesetting_plan = TextTypesettingPlan::resolve(&text_summary, first.style());
        let shaped = ShapedInlineLine {
            text: text_summary.into(),
            width,
            offset: 0.0,
            aligned_by_parley: false,
            line_height,
            baseline_adjustment,
            typesetting_plan,
            runs: shaped_runs,
            monotonic_source_advance_index: Default::default(),
        };
        let metrics = self.inline_text_box_metrics(first.style(), first.baseline_shift());
        let y = self.cursor_y - metrics.line_baseline_offset + first.baseline_shift();
        let paint_opacity = first
            .ancestor_inline_decorations()
            .iter()
            .fold(first.style().opacity.value(), |opacity, decoration| {
                opacity * decoration.style.opacity.value()
            });
        let paint_scope_ancestry = Rc::from(
            first
                .ancestor_inline_decorations()
                .iter()
                .filter_map(|decoration| decoration.paint_effect_scope_id)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        // Inline descendants inherit their text style, but their lexical
        // inline owner's `position` and `z-index` stay on the zero-width
        // scope edges. Preserve the innermost positioned scope so the paint
        // phase can place this otherwise ordinary text run in its stacking
        // band.
        // <https://www.w3.org/TR/CSS22/zindex.html>
        let positioned_paint_style =
            inline_style_establishes_positioned_stacking_context(first.style())
                .then(|| first.style().clone())
                .or_else(|| {
                    first
                        .ancestor_inline_decorations()
                        .iter()
                        .rev()
                        .find(|decoration| {
                            inline_style_establishes_positioned_stacking_context(&decoration.style)
                        })
                        .map(|decoration| decoration.style.clone())
                });
        let text_box_trim = self
            .inline_text_box_content_trim_for_style(first.style(), metrics)
            .is_empty()
            .then(|| {
                first
                    .ancestor_inline_decorations()
                    .iter()
                    .rev()
                    .find_map(|decoration| {
                        let decoration_metrics =
                            self.inline_text_box_metrics(&decoration.style, first.baseline_shift());
                        let trim = self.inline_text_box_content_trim_for_style(
                            &decoration.style,
                            decoration_metrics,
                        );
                        (!trim.is_empty()).then_some(trim)
                    })
            })
            .flatten()
            .unwrap_or_else(|| self.inline_text_box_content_trim_for_style(first.style(), metrics));
        Some(PreparedInlineTextGroup {
            bounds: PhysicalInlineTextBounds::new(InlinePoint::new(x, y), width),
            style: first.style().clone(),
            line_block_size: metrics.line_block_size,
            decoration_provenance,
            text_box_trim,
            paint_opacity,
            paint_scope_ancestry,
            positioned_paint_style,
            link_target: first.link_target().map(ToOwned::to_owned),
            link_paint_rect: None,
            decoration_paint_rect: None,
            shaped,
            actual_text: word_space_transform_actual_text(fragments),
            source: first.source(),
            source_run: Rc::clone(first.source_run()),
        })
    }

    pub(in crate::layout) fn prepare_justified_inline_text_group_with_summary_policy<
        F: InlineFragmentAccess,
    >(
        &mut self,
        fragments: &[F],
        x: f32,
        extra_per_separator: f32,
        preserve_leading_summary_space: bool,
        synthesize_leading_summary_space: bool,
        tab_metric_style: &ComputedStyle,
    ) -> Option<PreparedInlineTextGroup> {
        let mut group = self.prepare_inline_text_group_with_summary_policy(
            fragments,
            x,
            preserve_leading_summary_space,
            synthesize_leading_summary_space,
            x,
            tab_metric_style,
        )?;
        let separator_count = justifiable_fragment_space_count(fragments);
        let added_width = group
            .shaped
            .apply_inter_word_justification(extra_per_separator, separator_count);
        group.set_width(group.width() + added_width);
        Some(group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{FontFamily, TextDecorationLayer};

    fn provenance_fragment(text: &str, style: ComputedStyle) -> InlineFragment {
        InlineFragment::new(
            text,
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        )
    }

    fn shape_provenance_text(text: &str, style: &ComputedStyle) -> ShapedInlineLine {
        FontSystem::new()
            .shape_untracked_inline_line(text, style, style.line_height)
            .expect("test text should shape")
    }

    fn decoration_layer(origin_style: Rc<ComputedStyle>) -> TextDecorationLayer {
        TextDecorationLayer {
            decoration: origin_style.text_decoration.clone(),
            origin_style,
        }
    }

    #[test]
    fn matching_decoration_origin_chains_share_one_provenance_segment() {
        let mut origin = ComputedStyle::initial();
        origin.text_decoration.underline = true;
        let origin = Rc::new(origin);
        let mut receiver_style = ComputedStyle::initial();
        receiver_style
            .text_decoration_origins
            .set_propagated(vec![decoration_layer(Rc::clone(&origin))]);
        let left = provenance_fragment("a", receiver_style.clone());
        let right = provenance_fragment("b", receiver_style.clone());
        let plain = provenance_fragment("c", ComputedStyle::initial());
        let spans = vec![
            InlineTextPrepSpan::new(&left),
            InlineTextPrepSpan::new(&right),
            InlineTextPrepSpan::new(&plain),
        ];
        let shaped = shape_provenance_text("abc", &receiver_style);
        let mut provenance = Vec::new();
        append_text_decoration_provenance(&mut provenance, &spans, &shaped, 0.0);

        assert_eq!(provenance.len(), 2);
        assert_eq!(provenance[0].receivers.len(), 1);
        assert!(Rc::ptr_eq(&provenance[0].layers[0].origin_style, &origin,));
        assert!(provenance[1].layers.is_empty());
        assert_eq!(provenance[1].receivers.len(), 1);
    }

    #[test]
    fn equal_looking_decoration_origins_remain_separate_provenance_segments() {
        let mut declaration = ComputedStyle::initial();
        declaration.text_decoration.underline = true;
        let outer_origin = Rc::new(declaration.clone());
        let inner_origin = Rc::new(declaration);
        let mut outer_receiver = ComputedStyle::initial();
        outer_receiver
            .text_decoration_origins
            .set_propagated(vec![decoration_layer(Rc::clone(&outer_origin))]);
        let mut inner_receiver = ComputedStyle::initial();
        inner_receiver
            .text_decoration_origins
            .set_propagated(vec![decoration_layer(Rc::clone(&inner_origin))]);
        let left = provenance_fragment("a", outer_receiver.clone());
        let right = provenance_fragment("b", inner_receiver);
        let spans = vec![
            InlineTextPrepSpan::new(&left),
            InlineTextPrepSpan::new(&right),
        ];
        let shaped = shape_provenance_text("ab", &outer_receiver);
        let mut provenance = Vec::new();
        append_text_decoration_provenance(&mut provenance, &spans, &shaped, 0.0);

        assert_eq!(provenance.len(), 2);
        assert!(!Rc::ptr_eq(
            &provenance[0].layers[0].origin_style,
            &provenance[1].layers[0].origin_style,
        ));
        assert!(text_decoration_origin_chains_match(
            &provenance[0].layers,
            &provenance[0].layers,
        ));
        assert!(!text_decoration_origin_chains_match(
            &provenance[0].layers,
            &provenance[1].layers,
        ));
    }

    fn shared_source() -> Rc<BoundaryShapedSource> {
        Rc::new(BoundaryShapedSource {
            shaped: Rc::new(ShapedInlineLine {
                text: Rc::from("علا"),
                width: 30.0,
                offset: 0.0,
                aligned_by_parley: false,
                line_height: 20.0,
                baseline_adjustment: 0.0,
                typesetting_plan: TextTypesettingPlan::Horizontal,
                runs: Vec::new(),
                monotonic_source_advance_index: Default::default(),
            }),
        })
    }

    #[test]
    fn complete_boundary_source_requires_every_original_fragment() {
        let style = ComputedStyle::initial();
        let source = shared_source();
        let mut left = InlineFragment::new(
            "ع",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut middle = left.clone();
        middle.set_text("ل");
        let mut right = left.clone();
        right.set_text("ا");
        left.set_boundary_shaped_source(Rc::clone(&source), 0..2);
        middle.set_boundary_shaped_source(Rc::clone(&source), 2..4);
        right.set_boundary_shaped_source(source, 4..6);

        let complete = vec![
            InlineTextPrepSpan::new(&left),
            InlineTextPrepSpan::new(&middle),
            InlineTextPrepSpan::new(&right),
        ];
        let complete_shape = complete_boundary_shaped_source(&complete)
            .expect("complete boundary group should reuse its full shape");
        assert_eq!(complete_shape.text.as_ref(), "علا");

        // Visual reordering may place a retained fragment before the source
        // owner, while its text still completes the original logical source.
        let leading_without_provenance = InlineFragment::new(
            "ع",
            ComputedStyle::initial(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let reordered = vec![
            InlineTextPrepSpan::new(&leading_without_provenance),
            InlineTextPrepSpan::new(&middle),
            InlineTextPrepSpan::new(&right),
        ];
        let reordered_shape = complete_boundary_shaped_source(&reordered)
            .expect("complete logical text should find a source owned by a later fragment");
        assert_eq!(reordered_shape.text.as_ref(), "علا");

        let partial = vec![
            InlineTextPrepSpan::new(&left),
            InlineTextPrepSpan::new(&middle),
        ];
        assert!(complete_boundary_shaped_source(&partial).is_none());
    }

    #[test]
    fn source_selection_reuses_arabic_glyphs_only_with_typed_provenance() {
        let mut font_system = FontSystem::new();
        let mut style = ComputedStyle::initial();
        style.font_family = FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.4;
        let source = Rc::new(
            font_system
                .shape_untracked_inline_line("عائلة", &style, style.line_height)
                .expect("Arabic source should shape"),
        );
        let selection = SourceShapedSelection::from_source(Rc::clone(&source), 0.."عائل".len())
            .expect("cluster-aligned Arabic source range should slice");
        let expected = selection.selected().clone();
        let mut provenanced = InlineFragment::new(
            "عائل",
            style.clone(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        provenanced.set_source_shaped_selection(Some(selection));

        let provenanced_group = vec![InlineTextPrepSpan::new(&provenanced)];
        assert_eq!(
            compose_selected_source_shapes(&provenanced_group, "عائل"),
            Some(expected),
            "paint preparation must retain the full-word source glyph forms"
        );

        let unprovenanced = InlineFragment::new(
            "عائل",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let unprovenanced_group = vec![InlineTextPrepSpan::new(&unprovenanced)];
        assert!(
            compose_selected_source_shapes(&unprovenanced_group, "عائل").is_none(),
            "an independently shaped fragment must take the ordinary fallback path"
        );
    }
}
