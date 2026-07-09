use super::*;
use std::borrow::Cow;
use std::rc::Rc;

struct InlineTextPrepSpan<'a, F: InlineFragmentAccess> {
    fragment: &'a F,
    text: Cow<'a, str>,
}

impl<'a, F: InlineFragmentAccess> InlineTextPrepSpan<'a, F> {
    fn new(fragment: &'a F) -> Self {
        Self {
            fragment,
            text: Cow::Borrowed(fragment.text()),
        }
    }

    fn prepend_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut owned = String::with_capacity(text.len() + self.text.len());
        owned.push_str(text);
        owned.push_str(&self.text);
        self.text = Cow::Owned(owned);
    }

    fn append_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        match &mut self.text {
            Cow::Borrowed(existing) => {
                let mut owned = String::with_capacity(existing.len() + text.len());
                owned.push_str(existing);
                owned.push_str(text);
                self.text = Cow::Owned(owned);
            }
            Cow::Owned(existing) => existing.push_str(text),
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
        || text.chars().any(character_has_joining_behavior)
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
        self.prepare_inline_text_group_with_summary_policy(fragments, x, false, x)
    }

    pub(in crate::layout) fn prepare_inline_text_group_with_summary_policy<
        F: InlineFragmentAccess,
    >(
        &mut self,
        fragments: &[F],
        x: f32,
        preserve_leading_summary_space: bool,
        tab_origin: f32,
    ) -> Option<PreparedInlineTextGroup> {
        let first = fragments.first()?;
        let mut shaped_runs = Vec::new();
        let mut width = 0.0f32;
        let mut shaping_groups = Vec::<Vec<InlineTextPrepSpan<'_, F>>>::new();
        let mut pending_join_controls = String::new();

        for fragment in fragments {
            if inline_fragment_is_join_control_only(fragment) {
                let join_control_span = InlineTextPrepSpan::new(fragment);
                if let Some(group) = shaping_groups.last_mut()
                    && let Some(last) = group.last_mut()
                    && can_shape_inline_text_prep_spans_together(last, &join_control_span)
                {
                    last.append_text(fragment.text());
                } else {
                    pending_join_controls.push_str(fragment.text());
                }
                continue;
            }
            let mut span = InlineTextPrepSpan::new(fragment);
            if !pending_join_controls.is_empty() {
                span.prepend_text(&pending_join_controls);
                pending_join_controls.clear();
            }
            if let Some(group) = shaping_groups.last_mut()
                && let Some(last) = group.last()
                && can_shape_inline_text_prep_spans_together(last, &span)
            {
                group.push(span);
                continue;
            }
            shaping_groups.push(vec![span]);
        }
        if !pending_join_controls.is_empty()
            && let Some(group) = shaping_groups.last_mut()
            && let Some(last) = group.last_mut()
        {
            last.append_text(&pending_join_controls);
        }

        for group in &shaping_groups {
            let spans = group
                .iter()
                .map(|span| StyledTextSpan {
                    text: span.text.as_ref(),
                    style: span.fragment.style(),
                })
                .collect::<Vec<_>>();
            let group_text = spans.iter().map(|span| span.text).collect::<String>();
            let resolved_direction = group
                .first()
                .and_then(|span| span.fragment.resolved_bidi_direction())
                .unwrap_or(ResolvedBidiDirection::Ltr);
            // A join-control-only neighbor may have been folded into this
            // span above. Its cached source shape predates that control and
            // cannot be reused: U+200C/U+200D changes the OpenType joining
            // form of otherwise identical visible text.
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
            // <https://www.w3.org/TR/alreq/#h_joining-enforcement>
            let reused_selected_shape =
                if group.len() == 1 && group[0].text.as_ref() == group[0].fragment.text() {
                    group[0].fragment.selected_shaped().cloned()
                } else {
                    compose_selected_source_shapes(group, &group_text)
                };
            let shaped = reused_selected_shape.or_else(|| {
                self.font_system.shape_visually_ordered_inline_fragments(
                    &spans,
                    group_text,
                    0.0,
                    first.style().line_height,
                    tab_origin + width,
                    resolved_direction,
                )
            });
            if let Some(mut shaped) = shaped {
                let group_width = shaped.advance_width();
                for mut run in shaped.runs.drain(..) {
                    run.x_offset += width;
                    shaped_runs.push(run);
                }
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
            .font_ascent_baseline_adjustment(first_font_id, first.style(), line_height)
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
    ) -> Option<PreparedInlineTextGroup> {
        let mut group = self.prepare_inline_text_group_with_summary_policy(
            fragments,
            x,
            preserve_leading_summary_space,
            x,
        )?;
        let separator_count = justifiable_fragment_space_count(fragments);
        let added_width = group
            .shaped
            .apply_inter_word_justification(extra_per_separator, separator_count);
        group.set_width(group.width() + added_width);
        Some(group)
    }
}
