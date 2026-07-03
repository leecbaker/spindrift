use super::*;
use crate::css::BoxDecorationBreak;
use crate::layout::inline_collect::InlinePlacement;
use std::borrow::Cow;

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
        && !inline_box_edge_breaks_shaping(left.fragment.style())
        && !inline_box_edge_breaks_shaping(right.fragment.style())
        && !inline_box_bidi_isolation_breaks_shaping(left.fragment.style())
        && !inline_box_bidi_isolation_breaks_shaping(right.fragment.style())
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn inline_space_width(&mut self, style: &ComputedStyle) -> f32 {
        self.font_system
            .measure_text(" ", style)
            .max(style.font_size * 0.25)
    }

    pub(in crate::layout) fn layout_text_block(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
    ) {
        if self.layout_multicol_text_block(
            text,
            style,
            padding_left,
            padding_right,
            link_target,
            style.box_values.height.length_if_no_percent(),
        ) {
            return;
        }
        let available_width =
            (self.current_content_logical_inline_size() - padding_left - padding_right).max(1.0);
        let sequence = self.inline_line_sequence_for_text(
            text,
            style,
            available_width,
            padding_left,
            link_target,
        );
        self.paint_inline_line_sequence(&sequence, style);
    }

    pub(in crate::layout) fn layout_multicol_text_block(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
        content_height: Option<f32>,
    ) -> bool {
        let available_width = self.current_content_logical_inline_size().max(1.0);
        let gap = used_multicol_column_gap(style.column_gap, available_width, style.font_size);
        let Some(column_count) =
            used_multicol_column_count(style, available_width, gap).filter(|count| *count > 1)
        else {
            return false;
        };
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((available_width - total_gap) / column_count as f32).max(1.0);
        let available_column_width = (column_width - padding_left - padding_right).max(1.0);
        let text = transform_text(text, style);
        let mut sequence_style = style.clone();
        sequence_style.box_decoration_break = BoxDecorationBreak::Clone;
        let sequence = self.inline_line_sequence_for_prepared_text(
            &text,
            &sequence_style,
            available_column_width,
            padding_left,
            link_target,
        );
        let column_height = content_height
            .unwrap_or_else(|| sequence.balanced_multicolumn_height(column_count, style))
            .max(style.line_height);
        self.paint_inline_line_sequence_multicolumn(
            &sequence,
            style,
            column_count,
            gap,
            column_width,
            column_height,
        );
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn paint_text_block_slice(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
        block_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) {
        let available_width =
            (self.current_content_logical_inline_size() - padding_left - padding_right).max(1.0);
        let sequence = self.inline_line_sequence_for_text(
            text,
            style,
            available_width,
            padding_left,
            link_target,
        );
        self.paint_inline_line_sequence_slice(&sequence, style, block_top, slice_top, slice_bottom);
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn paint_element_inline_block_slice(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
        block_top: f32,
        slice_top: f32,
        slice_bottom: f32,
    ) {
        let available_width =
            (self.current_content_logical_inline_size() - padding_left - padding_right).max(1.0);
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
        self.push_generated_pseudo_items(
            element,
            style,
            style.before_style.as_deref(),
            link_target.clone(),
            0.0,
            InlineVisualOffset::zero(),
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        self.collect_element_content_or_inline_items(
            element,
            style,
            stylesheets,
            link_target.clone(),
            InlinePlacement::zero(),
            &mut items,
        );
        self.push_generated_pseudo_items(
            element,
            style,
            style.after_style.as_deref(),
            link_target,
            0.0,
            InlineVisualOffset::zero(),
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_width,
            padding_left,
            0.0,
        );
        self.paint_inline_line_sequence_slice(&sequence, style, block_top, slice_top, slice_bottom);
    }

    pub(in crate::layout) fn inline_line_sequence_for_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        link_target: Option<&str>,
    ) -> inline_layout::InlineLineSequence {
        let text = transform_text(text, style);
        self.inline_line_sequence_for_prepared_text(
            &text,
            style,
            available_width,
            padding_left,
            link_target,
        )
    }

    pub(in crate::layout) fn inline_line_sequence_for_raw_inline_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        link_target: Option<&str>,
    ) -> inline_layout::InlineLineSequence {
        let mut items = Vec::new();
        self.push_inline_words(
            text,
            style,
            link_target.map(str::to_string),
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_width,
            padding_left,
            0.0,
        )
    }

    pub(in crate::layout) fn inline_line_sequence_for_prepared_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        link_target: Option<&str>,
    ) -> inline_layout::InlineLineSequence {
        let mut items = Vec::new();
        self.push_inline_words(
            text,
            style,
            link_target.map(str::to_string),
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_width,
            padding_left,
            0.0,
        )
    }

    pub(in crate::layout) fn layout_list_text_block(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
        marker: Option<&ListMarker>,
    ) {
        let Some(marker) = marker else {
            self.layout_text_block(text, style, padding_left, padding_right, link_target);
            return;
        };

        let available_width =
            (self.current_content_logical_inline_size() - padding_left - padding_right).max(1.0);
        let text = transform_text(text, style);
        if marker.position == ListStylePosition::Inside {
            let link_target = link_target.map(str::to_string);
            let mut items = Vec::new();
            if block_bidi_scope_needs_inline_controls(style) {
                self.push_bidi_scope_start(
                    style,
                    link_target.clone(),
                    0.0,
                    InlineVisualOffset::zero(),
                    &mut items,
                );
            }
            self.push_inside_marker_items(marker, style, link_target.clone(), &mut items);
            self.push_inline_words(
                &text,
                style,
                link_target,
                0.0,
                InlineVisualOffset::zero(),
                &mut items,
            );
            if block_bidi_scope_needs_inline_controls(style) {
                self.push_bidi_scope_end(style, None, 0.0, InlineVisualOffset::zero(), &mut items);
            }
            let sequence = self.collect_inline_line_sequence_with_text_box_trim(
                items,
                style,
                available_width,
                padding_left,
                0.0,
            );
            self.paint_inline_line_sequence(&sequence, style);
            return;
        }

        let sequence = self.inline_line_sequence_for_prepared_text(
            &text,
            style,
            available_width,
            padding_left,
            link_target,
        );
        self.paint_inline_line_sequence_with_outside_marker(
            &sequence,
            style,
            marker,
            self.content_left + padding_left,
            self.content_right - padding_right,
        );
    }

    pub(in crate::layout) fn paint_text_runs(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        style: &ComputedStyle,
    ) -> Option<RenderedLine> {
        let line_height = self.font_system.used_line_height(style);
        let shaped = self
            .font_system
            .shape_unwrapped_line(text, style, line_height)?;
        self.paint_shaped_inline_line(&shaped, x, y, style)
    }

    /// Paint a previously shaped inline line without reshaping.
    ///
    /// CSS Text and CSS Fonts require the glyph run selected during shaping to
    /// remain the glyph run emitted by the renderer. Reusing
    /// `ShapedInlineLine` here keeps fallback font ids, glyph ids, advances,
    /// and ToUnicode cluster summaries stable through PDF output:
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
    /// ISO 32000-2:2020, 9.10.3 "ToUnicode CMaps".
    pub(in crate::layout) fn paint_shaped_inline_line(
        &mut self,
        shaped: &ShapedInlineLine,
        x: f32,
        y: f32,
        style: &ComputedStyle,
    ) -> Option<RenderedLine> {
        let rendered_runs = positioned_rendered_runs_for_writing_mode(shaped, style);
        if rendered_runs.is_empty() {
            return None;
        }
        debug_assert!(shaped.advance_width().is_finite());
        let first_font_id = shaped.first_font_id();
        let y = y + shaped.baseline_adjustment;
        let rendered_line = RenderedLine::from_paint_origin(
            shaped.text.clone(),
            paint_space_point(x, y),
            rendered_line_font_size(&rendered_runs, style.font_size),
            first_font_id,
            style.color,
            rendered_runs,
        );
        self.paint_text_shadows(&rendered_line, style);
        self.paint_text_decoration_lines_for_phase(
            rendered_line.x(),
            rendered_line.y(),
            shaped.advance_width(),
            style,
            &rendered_line.runs,
            TextDecorationPaintPhase::BeforeText,
        );
        self.push_line_in_band(PaintBand::Inline, rendered_line.clone());
        self.paint_prepared_text_emphasis_marks_for_line(&rendered_line, style);
        self.paint_text_decoration_lines_for_phase(
            rendered_line.x(),
            rendered_line.y(),
            shaped.advance_width(),
            style,
            &rendered_line.runs,
            TextDecorationPaintPhase::AfterText,
        );
        Some(rendered_line)
    }

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
        self.prepare_inline_text_group_with_summary_policy(fragments, x, false)
    }

    pub(in crate::layout) fn prepare_inline_text_group_with_summary_policy<
        F: InlineFragmentAccess,
    >(
        &mut self,
        fragments: &[F],
        x: f32,
        preserve_leading_summary_space: bool,
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
            if let Some(mut shaped) = self.font_system.shape_styled_inline_fragments(
                &spans,
                group_text,
                0.0,
                first.style().line_height,
            ) {
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
            .line_height_for_font(first_font_id, first.style());
        let baseline_adjustment = self.font_system.font_ascent_baseline_adjustment(
            first_font_id,
            first.style(),
            line_height,
        );
        let shaped = ShapedInlineLine {
            text: text_summary,
            width,
            offset: 0.0,
            aligned_by_parley: false,
            line_height,
            baseline_adjustment,
            runs: shaped_runs,
        };
        let metrics =
            self.inline_text_box_metrics(first.style(), Some(&shaped), first.baseline_shift());
        let y = self.cursor_y - metrics.line_baseline_offset;
        Some(PreparedInlineTextGroup {
            bounds: PhysicalInlineTextBounds::new(x, y, width),
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
        )?;
        let separator_count = justifiable_fragment_space_count(fragments);
        let added_width = group
            .shaped
            .apply_inter_word_justification(extra_per_separator, separator_count);
        group.set_width(group.width() + added_width);
        Some(group)
    }

    /// Paint a prepared inline text group without reshaping.
    ///
    /// PDF text emission must use the same glyph ids, advances, and fallback
    /// font ids chosen during CSS inline layout preparation:
    /// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm> and
    /// ISO 32000-2:2020, 9.4 "Text".
    pub(in crate::layout) fn paint_prepared_inline_text_group(
        &mut self,
        group: &PreparedInlineTextGroup,
    ) {
        let source = match group.source {
            InlineTextSource::Normal | InlineTextSource::Generated => RenderedLineSource::Normal,
            InlineTextSource::Marker => RenderedLineSource::Marker,
        };
        self.paint_prepared_inline_text_group_with_source(group, source);
    }

    pub(in crate::layout) fn paint_prepared_inline_text_group_with_source(
        &mut self,
        group: &PreparedInlineTextGroup,
        source: RenderedLineSource,
    ) {
        let rendered_runs = positioned_rendered_runs_for_writing_mode(&group.shaped, &group.style);
        if rendered_runs.is_empty() {
            return;
        }
        let first_font_id = group.shaped.first_font_id();
        let text_origin = group.bounds.text_origin();
        let rendered_line = RenderedLine::from_paint_origin_with_source(
            group.shaped.text.clone(),
            text_origin,
            rendered_line_font_size(&rendered_runs, group.style.font_size),
            first_font_id,
            group.style.color,
            rendered_runs,
            source,
        );
        let decoration_runs = rendered_line.runs.clone();
        let mut decoration_style = group.style.clone();
        let (decoration_x, decoration_baseline_y, decoration_width, decoration_style) =
            if let Some(rect) = group.decoration_paint_rect {
                match group.style.writing_mode {
                    WritingMode::HorizontalTb => {
                        decoration_style.font_size = rect.height().max(1.0);
                        (
                            rect.origin.x,
                            rect.origin.y,
                            rect.width(),
                            &decoration_style,
                        )
                    }
                    WritingMode::VerticalRl | WritingMode::VerticalLr => {
                        decoration_style.font_size = rect.width().max(1.0);
                        (
                            rect.origin.x,
                            rect.origin.y,
                            rect.height(),
                            &decoration_style,
                        )
                    }
                }
            } else {
                (group.x(), group.y(), group.width(), &group.style)
            };
        self.paint_text_shadows(&rendered_line, &group.style);
        self.paint_text_decoration_lines_for_phase(
            decoration_x,
            decoration_baseline_y,
            decoration_width,
            decoration_style,
            &decoration_runs,
            TextDecorationPaintPhase::BeforeText,
        );
        self.push_line_in_band(PaintBand::Inline, rendered_line.clone());
        self.paint_prepared_text_emphasis_marks_for_line(&rendered_line, &group.style);
        self.paint_text_decoration_lines_for_phase(
            decoration_x,
            decoration_baseline_y,
            decoration_width,
            decoration_style,
            &decoration_runs,
            TextDecorationPaintPhase::AfterText,
        );

        if let Some(target) = &group.link_target {
            self.current_page.push_link(RenderedLink::from_paint_rect(
                group.link_paint_rect(),
                target.clone(),
            ));
        }
    }

    /// Paint one inline fragment's background and border for a line box.
    ///
    /// CSS Backgrounds and Borders applies backgrounds and borders to inline
    /// boxes on each generated line box fragment. CSS 2.2 defines the inline
    /// box content area independently from line-height; vertical padding and
    /// borders start at the content-area edges rather than shrinking into
    /// glyph content. CSS Text hanging separators remain part of the fragment
    /// for painting even when excluded from line measurement:
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>,
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>,
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-color> and
    /// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
    pub(in crate::layout) fn paint_inline_fragment_background(
        &mut self,
        fragment: &InlineFragment,
        mut x: f32,
        mut y: f32,
        mut width: f32,
        mut height: f32,
    ) {
        if fragment.style().visibility != Visibility::Visible
            || !fragment.style().display.is_inline_level()
            || fragment.style().display.is_atomic_inline()
            || width <= 0.0
            || height <= 0.0
            || (fragment.style().background_color.is_none()
                && fragment.style().background_image.is_none()
                && used_border_width(fragment.style()) == 0.0)
        {
            return;
        }
        let mut style = fragment.style().clone();
        apply_inline_fragment_edge_painting(
            &mut style,
            fragment.hanging_edges(),
            &mut x,
            &mut y,
            &mut width,
            &mut height,
        );
        let (rects, rounded_rects, paths, strokes) = block_paint_ops(x, y, width, height, &style);
        for rect in rects {
            self.push_rect_in_band(PaintBand::Inline, rect);
        }
        for rounded_rect in rounded_rects {
            self.push_rounded_rect_in_band(PaintBand::Inline, rounded_rect);
        }
        for path in paths {
            self.push_path_in_band(PaintBand::Inline, path);
        }
        self.extend_strokes_in_band(PaintBand::Inline, strokes);
    }

    pub(in crate::layout) fn paint_text_shadows(
        &mut self,
        line: &RenderedLine,
        style: &ComputedStyle,
    ) {
        if style.text_shadow.is_empty() || line.runs.is_empty() {
            return;
        }
        for shadow in style.text_shadow.iter().rev() {
            let color = shadow.color.resolve(style.color);
            if shadow.inset || !color.is_visible() {
                continue;
            }
            for pass in text_shadow_paint_passes(*shadow, color) {
                let mut shadow_line = line.clone();
                shadow_line.translate_origin(PaintVector::new(
                    shadow.offset_x.length_points() + pass.x_offset,
                    -shadow.offset_y.length_points() - pass.y_offset,
                ));
                shadow_line.color = pass.color;
                self.paint_text_decoration_lines_for_phase_with_color(
                    shadow_line.x(),
                    shadow_line.y(),
                    rendered_text_line_width(&shadow_line),
                    style,
                    &shadow_line.runs,
                    TextDecorationPaintPhase::All,
                    Some(pass.color),
                );
                self.push_line_in_band(PaintBand::Inline, shadow_line);
            }
        }
    }

    pub(in crate::layout) fn paint_prepared_text_emphasis_marks_for_line(
        &mut self,
        line: &RenderedLine,
        style: &ComputedStyle,
    ) {
        let Some(mark) = style
            .text_emphasis_style
            .mark_for_writing_mode(style.writing_mode)
        else {
            return;
        };
        if mark.is_empty() {
            return;
        }
        let mut emphasis_style = style.clone();
        emphasis_style.text_decoration_layers.clear();
        emphasis_style.text_decoration = ComputedStyle::initial().text_decoration;
        emphasis_style.text_shadow.clear();
        emphasis_style.text_emphasis_style = TextEmphasisStyle::None;
        emphasis_style.color = style.text_emphasis_color.unwrap_or(style.color);
        emphasis_style.font_size = (style.font_size * 0.5).max(1.0);
        let mark_width = self.font_system.measure_text(mark, &emphasis_style);
        for mark in prepared_text_emphasis_marks_for_line(line, style, mark, mark_width) {
            let _ = self.paint_text_runs(&mark.mark, mark.x, mark.y, &emphasis_style);
        }
    }

    pub(in crate::layout) fn paint_text_decoration_lines_for_phase(
        &mut self,
        x: f32,
        baseline_y: f32,
        width: f32,
        style: &ComputedStyle,
        runs: &[RenderedTextRun],
        phase: TextDecorationPaintPhase,
    ) {
        self.paint_text_decoration_lines_for_phase_with_color(
            x, baseline_y, width, style, runs, phase, None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn paint_text_decoration_lines_for_phase_with_color(
        &mut self,
        x: f32,
        baseline_y: f32,
        width: f32,
        style: &ComputedStyle,
        runs: &[RenderedTextRun],
        phase: TextDecorationPaintPhase,
        color_override: Option<Color>,
    ) {
        let decorations = active_text_decoration_layers(style);
        if decorations.is_empty() || width <= 0.0 {
            return;
        }
        for decoration in decorations {
            self.paint_text_decoration_layer(
                x,
                baseline_y,
                width,
                style,
                runs,
                decoration,
                phase,
                color_override,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn paint_text_decoration_layer(
        &mut self,
        x: f32,
        baseline_y: f32,
        width: f32,
        style: &ComputedStyle,
        runs: &[RenderedTextRun],
        decoration: TextDecoration,
        phase: TextDecorationPaintPhase,
        color_override: Option<Color>,
    ) {
        if !decoration.has_visible_line() || width <= 0.0 {
            return;
        }
        let color = color_override.or(decoration.color).unwrap_or(style.color);
        let (inset_start, inset_end) = decoration.inset.used(style.font_size);
        let font_id = self.font_system.resolve_style(style);
        let metrics = self.font_system.text_decoration_metrics(font_id, style);
        let ink_boxes = self.font_system.glyph_ink_boxes_for_runs(runs, baseline_y);
        for stroke in prepare_text_decoration_strokes(TextDecorationPreparationInput {
            x,
            baseline_y,
            width,
            inset_start,
            inset_end,
            style,
            decoration,
            phase,
            color,
            color_override,
            metrics,
        }) {
            self.paint_text_decoration_stroke(stroke, runs, &ink_boxes);
        }
    }

    /// Paint one CSS text decoration stroke.
    ///
    /// CSS Text Decoration defines solid, double, dotted, dashed, and wavy
    /// decoration styles; PDF paths/strokes are the backend representation for
    /// non-rectangular strokes:
    /// <https://www.w3.org/TR/css-text-decor-3/#text-decoration-style-property>.
    pub(in crate::layout) fn paint_text_decoration_stroke(
        &mut self,
        stroke: PreparedTextDecorationStroke,
        runs: &[RenderedTextRun],
        ink_boxes: &[GlyphInkBox],
    ) {
        let PreparedTextDecorationStroke {
            axis,
            line_x,
            line_y,
            inline_start,
            inline_length,
            block_position,
            thickness,
            color,
            style,
            skip_ink,
            skip_spaces,
        } = stroke;
        let segments = text_decoration_segments(
            TextDecorationSegmentInputs {
                axis,
                line_x,
                line_y,
                inline_start,
                inline_length,
                block_position,
                thickness,
                skip_ink,
                skip_spaces,
            },
            runs,
            ink_boxes,
        );
        match style {
            TextDecorationStyle::Double if thickness >= 1.5 => {
                let stripe = (thickness / 3.0).max(0.5);
                for segment in segments {
                    self.push_text_decoration_rect_for_axis(
                        axis,
                        segment.start,
                        block_position + stripe,
                        segment.length,
                        stripe,
                        color,
                    );
                    self.push_text_decoration_rect_for_axis(
                        axis,
                        segment.start,
                        block_position - stripe,
                        segment.length,
                        stripe,
                        color,
                    );
                }
            }
            TextDecorationStyle::Dotted => {
                let dot = thickness.max(1.0);
                let step = dot * 2.0;
                for segment in segments {
                    let mut cursor = segment.start;
                    while cursor < segment.start + segment.length {
                        self.push_text_decoration_rect_for_axis(
                            axis,
                            cursor,
                            block_position,
                            dot.min(segment.start + segment.length - cursor),
                            dot,
                            color,
                        );
                        cursor += step;
                    }
                }
            }
            TextDecorationStyle::Dashed => {
                let dash = (thickness * 3.0).max(3.0);
                let gap = thickness.max(1.0);
                for segment in segments {
                    let mut cursor = segment.start;
                    while cursor < segment.start + segment.length {
                        self.push_text_decoration_rect_for_axis(
                            axis,
                            cursor,
                            block_position,
                            dash.min(segment.start + segment.length - cursor),
                            thickness,
                            color,
                        );
                        cursor += dash + gap;
                    }
                }
            }
            TextDecorationStyle::Wavy => {
                for segment in segments {
                    self.push_text_decoration_wavy_path(
                        axis,
                        segment.start,
                        block_position,
                        segment.length,
                        thickness,
                        color,
                    );
                }
            }
            TextDecorationStyle::Solid | TextDecorationStyle::Double => {
                for segment in segments {
                    self.push_text_decoration_rect_for_axis(
                        axis,
                        segment.start,
                        block_position,
                        segment.length,
                        thickness,
                        color,
                    );
                }
            }
        }
    }

    pub(in crate::layout) fn push_text_decoration_rect_for_axis(
        &mut self,
        axis: TextDecorationStrokeAxis,
        inline_start: f32,
        block_position: f32,
        inline_length: f32,
        thickness: f32,
        color: Color,
    ) {
        match axis {
            TextDecorationStrokeAxis::Horizontal => {
                self.push_text_decoration_rect(
                    inline_start,
                    block_position,
                    inline_length,
                    thickness,
                    color,
                );
            }
            TextDecorationStrokeAxis::Vertical => {
                self.push_text_decoration_rect(
                    block_position,
                    inline_start,
                    thickness,
                    inline_length,
                    color,
                );
            }
        }
    }

    pub(in crate::layout) fn push_text_decoration_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
    ) {
        self.push_rect_in_band(
            PaintBand::Inline,
            RenderedRect::from_paint_rect(paint_space_rect(x, y, width, height), Some(color)),
        );
    }

    /// Paint a CSS `wavy` text decoration as a stroked PDF path.
    ///
    /// CSS Text Decoration defines `wavy` as a wavy line. PDF has no native
    /// text-decoration primitive, so the renderer serializes the wave as a
    /// stroked path using ISO 32000 path construction operators.
    /// <https://www.w3.org/TR/css-text-decor-3/#text-decoration-style-property>.
    pub(in crate::layout) fn push_text_decoration_wavy_path(
        &mut self,
        axis: TextDecorationStrokeAxis,
        inline_start: f32,
        block_position: f32,
        inline_length: f32,
        thickness: f32,
        color: Color,
    ) {
        if inline_length <= 0.0 || thickness <= 0.0 {
            return;
        }
        let amplitude = (thickness * 1.25).max(1.0);
        let half_wave = (amplitude * 2.0).max(2.0);
        let center = block_position + thickness / 2.0;
        let mut commands = match axis {
            TextDecorationStrokeAxis::Horizontal => {
                vec![RenderedPathCommand::move_to(paint_space_point(
                    inline_start,
                    center,
                ))]
            }
            TextDecorationStrokeAxis::Vertical => {
                vec![RenderedPathCommand::move_to(paint_space_point(
                    center,
                    inline_start,
                ))]
            }
        };
        let mut cursor = inline_start;
        let mut crest = true;
        while cursor < inline_start + inline_length {
            let next = (cursor + half_wave).min(inline_start + inline_length);
            let control_inline = (cursor + next) / 2.0;
            let control_block = if crest {
                center + amplitude
            } else {
                center - amplitude
            };
            commands.push(match axis {
                TextDecorationStrokeAxis::Horizontal => RenderedPathCommand::curve_to(
                    paint_space_point(control_inline, control_block),
                    paint_space_point(control_inline, control_block),
                    paint_space_point(next, center),
                ),
                TextDecorationStrokeAxis::Vertical => RenderedPathCommand::curve_to(
                    paint_space_point(control_block, control_inline),
                    paint_space_point(control_block, control_inline),
                    paint_space_point(center, next),
                ),
            });
            cursor = next;
            crest = !crest;
        }
        self.push_path_in_band(
            PaintBand::Inline,
            RenderedPath::new(
                commands,
                None,
                RenderedPathFillRule::NonZero,
                Some(color),
                thickness.max(0.5),
                None,
            ),
        );
    }
}

pub(in crate::layout) fn rendered_line_font_size(
    rendered_runs: &[RenderedTextRun],
    fallback: f32,
) -> f32 {
    rendered_runs
        .iter()
        .find(|run| !run.text.is_empty())
        .map(|run| run.font_size)
        .unwrap_or(fallback)
}
