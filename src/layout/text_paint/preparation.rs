use super::*;
use std::rc::Rc;

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
        return !inline_box_edge_breaks_shaping(right.fragment.style())
            && !inline_bidi_isolation_boundary_breaks_shaping(left.fragment, right.fragment);
    }
    if inline_text_prep_span_is_join_control_only(right) {
        return !inline_box_edge_breaks_shaping(left.fragment.style())
            && !inline_bidi_isolation_boundary_breaks_shaping(left.fragment, right.fragment);
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
        && !inline_box_edge_breaks_shaping(left.fragment.style())
        && !inline_box_edge_breaks_shaping(right.fragment.style())
        && !inline_bidi_isolation_boundary_breaks_shaping(left.fragment, right.fragment)
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
    let mut result = group
        .iter()
        .find_map(|span| span.fragment.selected_shaped())?
        .clone();
    result.runs.clear();
    let mut width = 0.0;
    for span in group {
        let Some(shaped) = span.fragment.selected_shaped() else {
            if inline_text_prep_span_is_join_control_only(span) {
                continue;
            }
            return None;
        };
        if !span.fragment.preserves_source_shaping() {
            return None;
        }
        for mut run in shaped.runs.clone() {
            run.x_offset += width;
            result.runs.push(run);
        }
        width += shaped.advance_width();
    }
    result.text = Rc::from(text);
    result.width = width;
    Some(result)
}

/// Return a complete shaped source shared by every member of this paint group.
///
/// A strict source slice intentionally rejects a range that cuts through an
/// OpenType cluster. That is correct for soft-wrap selection, but not for a
/// transparent inline boundary: the complete selected source remains present
/// in this group and must be emitted once with its original cluster geometry.
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
fn complete_boundary_shaped_source<F: InlineFragmentAccess>(
    group: &[InlineTextPrepSpan<'_, F>],
) -> Option<ShapedInlineLine> {
    let first = group.first()?;
    let source = first.fragment.boundary_shaped_source()?;
    if !group.iter().all(|span| {
        span.fragment
            .boundary_shaped_source()
            .is_some_and(|candidate| std::ptr::eq(source, candidate))
    }) {
        return None;
    }

    let mut selected_ranges = group
        .iter()
        .map(|span| span.fragment.boundary_shaped_range().cloned())
        .collect::<Option<Vec<_>>>()?;
    let mut source_ranges = source.fragment_ranges.to_vec();
    selected_ranges.sort_by_key(|range| range.start);
    source_ranges.sort_by_key(|range| range.start);
    (selected_ranges == source_ranges).then(|| source.shaped.as_ref().clone())
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
        self.prepare_inline_text_group_with_summary_policy(fragments, x, false, x, tab_metric_style)
    }

    pub(in crate::layout) fn prepare_inline_text_group_with_summary_policy<
        F: InlineFragmentAccess,
    >(
        &mut self,
        fragments: &[F],
        x: f32,
        preserve_leading_summary_space: bool,
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
    ) -> Option<PreparedInlineTextGroup> {
        let first = fragments.first()?;
        let mut shaped_runs = Vec::new();
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
                .any(crate::text::character_has_joining_behavior);
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
            let reused_boundary_shape = (!group_text.contains('\t'))
                .then(|| complete_boundary_shaped_source(group))
                .flatten();
            let reused_selected_shape = reused_boundary_shape.or_else(|| {
                if !group_text.contains('\t')
                    && group.len() == 1
                    && !has_join_control
                    && !has_joining_behavior
                    && group[0].text == group[0].fragment.text()
                {
                    group[0].fragment.selected_shaped().cloned()
                } else if !group_text.contains('\t')
                    && !has_join_control
                    && !has_joining_behavior
                    && group.iter().all(|span| span.text == span.fragment.text())
                {
                    compose_selected_source_shapes(group, &group_text)
                } else {
                    None
                }
            });
            let shaped = reused_selected_shape.or_else(|| {
                self.font_system.shape_visually_ordered_inline_fragments(
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
                shaped_runs.extend(shaped.runs.drain(..).map(|mut run| {
                    run.x_offset += width;
                    run
                }));
                width += group_width;
            }
        }

        let text_summary = inline_fragment_text_summary(fragments, preserve_leading_summary_space);
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
        let shaped = ShapedInlineLine {
            text: text_summary.into(),
            width,
            offset: 0.0,
            aligned_by_parley: false,
            line_height,
            baseline_adjustment,
            runs: shaped_runs,
        };
        let metrics =
            self.inline_text_box_metrics(first.style(), Some(&shaped), first.baseline_shift());
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
        Some(PreparedInlineTextGroup {
            bounds: PhysicalInlineTextBounds::new(InlinePoint::new(x, y), width),
            style: first.style().clone(),
            paint_opacity,
            paint_scope_ancestry,
            link_target: first.link_target().map(ToOwned::to_owned),
            link_paint_rect: None,
            decoration_paint_rect: None,
            shaped,
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
        tab_metric_style: &ComputedStyle,
    ) -> Option<PreparedInlineTextGroup> {
        let mut group = self.prepare_inline_text_group_with_summary_policy(
            fragments,
            x,
            preserve_leading_summary_space,
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

    fn shared_source() -> Rc<BoundaryShapedSource> {
        Rc::new(BoundaryShapedSource {
            shaped: Rc::new(ShapedInlineLine {
                text: Rc::from("علا"),
                width: 30.0,
                offset: 0.0,
                aligned_by_parley: false,
                line_height: 20.0,
                baseline_adjustment: 0.0,
                runs: Vec::new(),
            }),
            fragment_ranges: Rc::from(vec![0..2, 2..4, 4..6].into_boxed_slice()),
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

        let partial = vec![
            InlineTextPrepSpan::new(&left),
            InlineTextPrepSpan::new(&middle),
        ];
        assert!(complete_boundary_shaped_source(&partial).is_none());
    }
}
