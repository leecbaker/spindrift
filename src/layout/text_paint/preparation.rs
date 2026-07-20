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
            && !inline_box_bidi_isolation_breaks_shaping(right.fragment.style());
    }
    if inline_text_prep_span_is_join_control_only(right) {
        return !inline_box_edge_breaks_shaping(left.fragment.style())
            && !inline_box_bidi_isolation_breaks_shaping(left.fragment.style());
    }
    left.fragment.style().vertical_align == right.fragment.style().vertical_align
        && left.fragment.style().writing_mode == right.fragment.style().writing_mode
        && left.fragment.style().language == right.fragment.style().language
        && left.fragment.resolved_bidi_direction() == right.fragment.resolved_bidi_direction()
        && !inline_box_edge_breaks_shaping(left.fragment.style())
        && !inline_box_edge_breaks_shaping(right.fragment.style())
        && !inline_box_bidi_isolation_breaks_shaping(left.fragment.style())
        && !inline_box_bidi_isolation_breaks_shaping(right.fragment.style())
}

/// Join visual fragments that all retain glyphs from selected source slices.
///
/// Inline bidi ordering may split one selected source range into several
/// paint fragments. Re-shaping their visual text would discard the original
/// cursive forms; composing their already-shaped visual runs keeps the source
/// shaping while preserving the selected line's paint order:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/css-writing-modes-4/#bidi-linebox>.
fn compose_selected_source_shapes<F: InlineFragmentAccess>(
    group: &[InlineTextPrepSpan<'_, F>],
    text: &str,
) -> Option<ShapedInlineLine> {
    if group.is_empty()
        || !group
            .iter()
            .all(|span| span.fragment.preserves_source_shaping())
    {
        return None;
    }
    let mut result = group.first()?.fragment.selected_shaped()?.clone();
    result.runs.clear();
    let mut width = 0.0;
    for span in group {
        let shaped = span.fragment.selected_shaped()?;
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
            // visual span above. Only an explicitly selected source shape
            // retains the logical joining context through that reordering;
            // an ordinary cached shape predates the folded U+200C/U+200D.
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
            // <https://www.w3.org/TR/alreq/#h_joining-enforcement>
            // A cached source shape can have resolved a preserved tab before
            // the selected forced break. Re-shape tab-containing selected
            // groups from this line's content edge.
            // <https://www.w3.org/TR/css-text-3/#tab-size-property>
            let has_join_control = group_text
                .chars()
                .any(crate::text::character_is_join_control);
            // Visual reordering reverses the order of the separate inline
            // fragments in an RTL run, while retaining the source order
            // inside each fragment. Keep join controls as their own spans so
            // their logical position can be restored for cursive shaping.
            // The resulting glyph stream remains visual, as produced by the
            // RTL shaper, and is therefore ready for painting.
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
            // <https://www.unicode.org/reports/tr9/#Reordering_Resolved_Levels>
            let logical_joining_spans = (has_join_control
                && resolved_direction == ResolvedBidiDirection::Rtl)
                .then(|| spans.iter().rev().copied().collect::<Vec<_>>());
            let shaping_spans = logical_joining_spans.as_deref().unwrap_or(&spans);
            let reused_selected_shape = if !group_text.contains('\t')
                && group.len() == 1
                && !has_join_control
                && group[0].text == group[0].fragment.text()
            {
                group[0].fragment.selected_shaped().cloned()
            } else if !group_text.contains('\t')
                && !has_join_control
                && group.iter().all(|span| span.text == span.fragment.text())
            {
                compose_selected_source_shapes(group, &group_text)
            } else {
                None
            };
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

        let first_font_id = self.font_system.resolve_style(first.style());
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
        Some(PreparedInlineTextGroup {
            bounds: PhysicalInlineTextBounds::new(InlinePoint::new(x, y), width),
            style: first.style().clone(),
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
