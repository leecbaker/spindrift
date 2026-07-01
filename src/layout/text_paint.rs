use super::*;
use crate::css::{
    TextDecoration, TextDecorationSkipSelf, TextEmphasisSkip, TextEmphasisStyle, TextOrientation,
};
use crate::text::trim_start_css_collapsible_whitespace;
use crate::text::{
    character_is_text_decoration_spacer, typographic_unit_is_upright_in_mixed_orientation,
    typographic_unit_ranges,
};

/// Used values for one CSS text-decoration stroke.
///
/// CSS Text Decoration resolves line style, color, thickness, offset, and
/// skip-ink before painting each decoration line:
/// <https://www.w3.org/TR/css-text-decor-4/#painting>.
#[derive(Debug, Clone, Copy)]
struct PreparedTextDecorationStroke {
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    line_y: f32,
    inline_start: f32,
    inline_length: f32,
    block_position: f32,
    thickness: f32,
    color: Color,
    style: TextDecorationStyle,
    skip_ink: TextDecorationSkipInk,
    skip_spaces: TextDecorationSkipSpaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextDecorationStrokeAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextDecorationSide {
    Left,
    Right,
}

#[derive(Debug, Clone)]
struct PreparedTextEmphasisMark {
    mark: String,
    #[allow(dead_code)]
    source_text: String,
    x: f32,
    y: f32,
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn inline_space_width(&mut self, style: &ComputedStyle) -> f32 {
        self.font_system
            .measure_text(" ", style)
            .max(style.font_size * 0.25)
    }

    pub(super) fn layout_text_block(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        padding_left: f32,
        padding_right: f32,
        link_target: Option<&str>,
    ) {
        let available_width =
            (self.content_right - self.content_left - padding_left - padding_right).max(1.0);
        let sequence = self.inline_line_sequence_for_text(
            text,
            style,
            available_width,
            padding_left,
            link_target,
        );
        self.paint_inline_line_sequence(&sequence, style);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_text_block_slice(
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
            (self.content_right - self.content_left - padding_left - padding_right).max(1.0);
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
    pub(super) fn paint_element_inline_block_slice(
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
            (self.content_right - self.content_left - padding_left - padding_right).max(1.0);
        let mut items = Vec::new();
        let link_target = link_target.map(str::to_string);
        self.push_generated_pseudo_items(
            element,
            style.before_style.as_deref(),
            link_target.clone(),
            0.0,
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        self.collect_element_content_or_inline_items(
            element,
            style,
            stylesheets,
            link_target.clone(),
            0.0,
            &mut items,
        );
        self.push_generated_pseudo_items(
            element,
            style.after_style.as_deref(),
            link_target,
            0.0,
            GeneratedPseudoCounterMode::Commit,
            &mut items,
        );
        let sequence =
            self.collect_inline_line_sequence(items, style, available_width, padding_left, 0.0);
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

    #[allow(dead_code)]
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
            &mut items,
        );
        self.collect_inline_line_sequence(items, style, available_width, padding_left, 0.0)
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
            &mut items,
        );
        self.collect_inline_line_sequence(items, style, available_width, padding_left, 0.0)
    }

    pub(super) fn layout_list_text_block(
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
            (self.content_right - self.content_left - padding_left - padding_right).max(1.0);
        let text = transform_text(text, style);
        if marker.position == ListStylePosition::Inside {
            let link_target = link_target.map(str::to_string);
            let mut items = Vec::new();
            if block_bidi_scope_needs_inline_controls(style) {
                self.push_bidi_scope_start(style, link_target.clone(), 0.0, &mut items);
            }
            self.push_inside_marker_items(marker, style, link_target.clone(), &mut items);
            self.push_inline_words(&text, style, link_target, 0.0, &mut items);
            if block_bidi_scope_needs_inline_controls(style) {
                self.push_bidi_scope_end(style, None, 0.0, &mut items);
            }
            let sequence =
                self.collect_inline_line_sequence(items, style, available_width, padding_left, 0.0);
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

    pub(super) fn paint_text_runs(
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
    pub(super) fn paint_shaped_inline_line(
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
    pub(super) fn prepare_inline_text_group(
        &mut self,
        fragments: &[InlineFragment],
        x: f32,
    ) -> Option<PreparedInlineTextGroup> {
        self.prepare_inline_text_group_with_summary_policy(fragments, x, false)
    }

    pub(in crate::layout) fn prepare_inline_text_group_with_summary_policy(
        &mut self,
        fragments: &[InlineFragment],
        x: f32,
        preserve_leading_summary_space: bool,
    ) -> Option<PreparedInlineTextGroup> {
        let visible_fragments = fragments.to_vec();
        let first = visible_fragments.first()?;
        let mut shaped_runs = Vec::new();
        let mut width = 0.0f32;
        let mut shaping_groups = Vec::<Vec<InlineFragment>>::new();
        let mut pending_join_controls = String::new();

        for fragment in &visible_fragments {
            if inline_fragment_is_join_control_only(fragment) {
                if let Some(group) = shaping_groups.last_mut()
                    && let Some(last) = group.last_mut()
                    && can_shape_inline_fragments_together(last, fragment)
                {
                    last.text.push_str(&fragment.text);
                } else {
                    pending_join_controls.push_str(&fragment.text);
                }
                continue;
            }
            let mut fragment = fragment.clone();
            if !pending_join_controls.is_empty() {
                fragment.text.insert_str(0, &pending_join_controls);
                pending_join_controls.clear();
            }
            if let Some(group) = shaping_groups.last_mut()
                && let Some(last) = group.last()
                && can_shape_inline_fragments_together(last, &fragment)
            {
                group.push(fragment);
                continue;
            }
            shaping_groups.push(vec![fragment]);
        }
        if !pending_join_controls.is_empty()
            && let Some(group) = shaping_groups.last_mut()
            && let Some(last) = group.last_mut()
        {
            last.text.push_str(&pending_join_controls);
        }

        for group in &shaping_groups {
            let spans = group
                .iter()
                .map(|fragment| StyledTextSpan {
                    text: fragment.text.as_str(),
                    style: &fragment.style,
                })
                .collect::<Vec<_>>();
            let group_text = spans.iter().map(|span| span.text).collect::<String>();
            if let Some(mut shaped) = self.font_system.shape_styled_inline_fragments(
                &spans,
                group_text,
                0.0,
                first.style.line_height,
            ) {
                let group_width = shaped.advance_width();
                for mut run in shaped.runs.drain(..) {
                    run.x_offset += width;
                    shaped_runs.push(run);
                }
                width += group_width;
            }
        }

        let text_summary =
            inline_fragment_text_summary(&visible_fragments, preserve_leading_summary_space);
        if shaped_runs.is_empty() || text_summary.is_empty() {
            return None;
        }

        let first_font_id = shaped_runs.iter().find_map(|run| run.font_id);
        let y = self.cursor_y - first.style.font_size + first.baseline_shift;
        let line_height = self
            .font_system
            .line_height_for_font(first_font_id, &first.style);
        let baseline_adjustment = self.font_system.font_ascent_baseline_adjustment(
            first_font_id,
            &first.style,
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
        Some(PreparedInlineTextGroup {
            bounds: PhysicalInlineTextBounds::new(x, y + baseline_adjustment, width),
            style: first.style.clone(),
            link_target: first.link_target.clone(),
            shaped,
        })
    }

    pub(in crate::layout) fn prepare_justified_inline_text_group_with_summary_policy(
        &mut self,
        fragments: &[InlineFragment],
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
    pub(super) fn paint_prepared_inline_text_group(&mut self, group: &PreparedInlineTextGroup) {
        let rendered_runs = positioned_rendered_runs_for_writing_mode(&group.shaped, &group.style);
        if rendered_runs.is_empty() {
            return;
        }
        let first_font_id = group.shaped.first_font_id();
        let text_origin = group.bounds.text_origin();
        let rendered_line = RenderedLine::from_paint_origin(
            group.shaped.text.clone(),
            text_origin,
            rendered_line_font_size(&rendered_runs, group.style.font_size),
            first_font_id,
            group.style.color,
            rendered_runs,
        );
        let decoration_runs = rendered_line.runs.clone();
        self.paint_text_shadows(&rendered_line, &group.style);
        self.paint_text_decoration_lines_for_phase(
            group.x(),
            group.y(),
            group.width(),
            &group.style,
            &decoration_runs,
            TextDecorationPaintPhase::BeforeText,
        );
        self.push_line_in_band(PaintBand::Inline, rendered_line.clone());
        self.paint_prepared_text_emphasis_marks_for_line(&rendered_line, &group.style);
        self.paint_text_decoration_lines_for_phase(
            group.x(),
            group.y(),
            group.width(),
            &group.style,
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
    pub(super) fn paint_inline_fragment_background(
        &mut self,
        fragment: &InlineFragment,
        mut x: f32,
        mut y: f32,
        mut width: f32,
        mut height: f32,
    ) {
        if fragment.style.visibility != Visibility::Visible
            || !fragment.style.display.is_inline_level()
            || fragment.style.display.is_atomic_inline()
            || width <= 0.0
            || height <= 0.0
            || (fragment.style.background_color.is_none()
                && fragment.style.background_image.is_none()
                && used_border_width(&fragment.style) == 0.0)
        {
            return;
        }
        let mut style = fragment.style.clone();
        apply_inline_fragment_edge_painting(
            &mut style,
            fragment.hanging_edges,
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

    fn paint_text_shadows(&mut self, line: &RenderedLine, style: &ComputedStyle) {
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
                    shadow.offset_x.length + pass.x_offset,
                    -shadow.offset_y.length - pass.y_offset,
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

    fn paint_prepared_text_emphasis_marks_for_line(
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

    fn paint_text_decoration_lines_for_phase(
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
    fn paint_text_decoration_lines_for_phase_with_color(
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
    fn paint_text_decoration_layer(
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
    fn paint_text_decoration_stroke(
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

    fn push_text_decoration_rect_for_axis(
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

    fn push_text_decoration_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
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
    fn push_text_decoration_wavy_path(
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

fn rendered_line_font_size(rendered_runs: &[RenderedTextRun], fallback: f32) -> f32 {
    rendered_runs
        .iter()
        .find(|run| !run.text.is_empty())
        .map(|run| run.font_size)
        .unwrap_or(fallback)
}

fn rendered_text_line_width(line: &RenderedLine) -> f32 {
    line.runs.iter().fold(0.0_f32, |width, run| {
        let run_width = if run.text_matrix.is_identity() {
            run.glyphs
                .as_ref()
                .map(|glyphs| glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>())
                .unwrap_or_else(|| run.text.chars().count() as f32 * run.font_size * 0.5)
        } else {
            run.font_size
        };
        width.max(run.x_offset + run_width)
    })
}

pub(in crate::layout) fn positioned_rendered_runs_for_writing_mode(
    shaped: &ShapedInlineLine,
    style: &ComputedStyle,
) -> Vec<RenderedTextRun> {
    position_rendered_runs_for_writing_mode(shaped.rendered_runs(), style)
}

pub(in crate::layout) fn position_rendered_runs_for_writing_mode(
    runs: Vec<RenderedTextRun>,
    style: &ComputedStyle,
) -> Vec<RenderedTextRun> {
    if style.writing_mode == WritingMode::HorizontalTb {
        return runs;
    }
    let placement_direction = if matches!(style.text_orientation, TextOrientation::Upright) {
        Direction::Ltr
    } else {
        style.direction
    };
    let advance_sign = match placement_direction {
        Direction::Ltr => -1.0,
        Direction::Rtl => 1.0,
    };
    let sideways_matrix = match placement_direction {
        Direction::Ltr => RenderedTextMatrix::ROTATE_CW,
        Direction::Rtl => RenderedTextMatrix::ROTATE_CCW,
    };
    runs.into_iter()
        .flat_map(|run| {
            vertical_positioned_text_runs(
                run,
                style.text_orientation,
                advance_sign,
                sideways_matrix,
            )
        })
        .collect()
}

fn vertical_positioned_text_runs(
    mut run: RenderedTextRun,
    text_orientation: TextOrientation,
    advance_sign: f32,
    sideways_matrix: RenderedTextMatrix,
) -> Vec<RenderedTextRun> {
    let Some(glyphs) = run.glyphs.take() else {
        let text_matrix = if matches!(text_orientation, TextOrientation::Upright) {
            RenderedTextMatrix::IDENTITY
        } else {
            sideways_matrix
        };
        return vec![RenderedTextRun {
            y_offset: advance_sign * run.x_offset,
            text_matrix,
            glyphs: None,
            ..run
        }];
    };
    if glyphs.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::new();
    let mut pending_sideways: Option<RenderedTextRun> = None;
    let mut cursor = run.x_offset;
    let mut cluster_text = String::new();
    let mut cluster_glyphs = Vec::new();
    let mut consumed_text_bytes = 0usize;
    let unit_ends = typographic_unit_ranges(&run.text)
        .into_iter()
        .map(|range| range.end)
        .collect::<Vec<_>>();
    let mut unit_index = 0usize;
    let mut cluster_end = unit_ends.first().copied().unwrap_or(run.text.len());
    for glyph in glyphs {
        if !glyph.unicode.is_empty()
            && !cluster_glyphs.is_empty()
            && consumed_text_bytes >= cluster_end
        {
            flush_vertical_cluster(
                &run,
                &mut output,
                &mut pending_sideways,
                cursor,
                text_orientation,
                advance_sign,
                sideways_matrix,
                std::mem::take(&mut cluster_text),
                std::mem::take(&mut cluster_glyphs),
            );
            while unit_index + 1 < unit_ends.len() && consumed_text_bytes >= cluster_end {
                unit_index += 1;
                cluster_end = unit_ends[unit_index];
            }
        }
        if !glyph.unicode.is_empty() {
            consumed_text_bytes += glyph.unicode.len();
            cluster_text.push_str(&glyph.unicode);
        }
        cursor += glyph.x_advance;
        cluster_glyphs.push(glyph);
    }
    if !cluster_glyphs.is_empty() {
        flush_vertical_cluster(
            &run,
            &mut output,
            &mut pending_sideways,
            cursor,
            text_orientation,
            advance_sign,
            sideways_matrix,
            cluster_text,
            cluster_glyphs,
        );
    }
    if let Some(run) = pending_sideways {
        output.push(run);
    }
    output
}

#[allow(clippy::too_many_arguments)]
fn flush_vertical_cluster(
    source: &RenderedTextRun,
    output: &mut Vec<RenderedTextRun>,
    pending_sideways: &mut Option<RenderedTextRun>,
    cursor_after_cluster: f32,
    text_orientation: TextOrientation,
    advance_sign: f32,
    sideways_matrix: RenderedTextMatrix,
    text: String,
    glyphs: Vec<RenderedGlyph>,
) {
    let advance = glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>();
    let cluster_start = cursor_after_cluster - advance;
    if vertical_text_cluster_is_upright(text_orientation, &text) {
        if let Some(run) = pending_sideways.take() {
            output.push(run);
        }
        output.push(RenderedTextRun {
            text,
            x_offset: 0.0,
            y_offset: advance_sign * cluster_start,
            text_matrix: RenderedTextMatrix::IDENTITY,
            font_size: source.font_size,
            font_id: source.font_id,
            glyphs: Some(glyphs),
        });
        return;
    }
    match pending_sideways {
        Some(run) => {
            run.text.push_str(&text);
            if let Some(existing_glyphs) = &mut run.glyphs {
                existing_glyphs.extend(glyphs);
            }
        }
        None => {
            *pending_sideways = Some(RenderedTextRun {
                text,
                x_offset: 0.0,
                y_offset: advance_sign * cluster_start,
                text_matrix: sideways_matrix,
                font_size: source.font_size,
                font_id: source.font_id,
                glyphs: Some(glyphs),
            });
        }
    }
}

/// Return whether a shaped text cluster is painted upright in vertical writing.
///
/// CSS Writing Modes defines `text-orientation` as the policy for orienting
/// typographic character units in vertical lines. `mixed` uses Unicode
/// Vertical_Orientation through the shared text property policy:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
fn vertical_text_cluster_is_upright(text_orientation: TextOrientation, text: &str) -> bool {
    match text_orientation {
        TextOrientation::Sideways => false,
        TextOrientation::Upright => !text.is_empty(),
        TextOrientation::Mixed => typographic_unit_is_upright_in_mixed_orientation(text),
    }
}

#[derive(Debug, Clone, Copy)]
struct TextShadowPaintPass {
    x_offset: f32,
    y_offset: f32,
    color: Color,
}

/// Build vector replay passes for a CSS `text-shadow`.
///
/// CSS Text Decoration Level 3 defines shadow layers as applying to the
/// composited text and decoration ink. PDF has no portable text-blur primitive,
/// so blurred shadows are approximated by bounded translucent vector replays
/// while zero-blur shadows remain crisp single-pass text:
/// <https://www.w3.org/TR/css-text-decor-3/#text-shadow-property>.
fn text_shadow_paint_passes(
    shadow: crate::css::TextShadow,
    color: Color,
) -> Vec<TextShadowPaintPass> {
    if shadow.blur_radius.length <= 0.0 {
        return vec![TextShadowPaintPass {
            x_offset: 0.0,
            y_offset: 0.0,
            color,
        }];
    }

    let radius = shadow.blur_radius.length.max(0.0);
    let samples = [
        (0.0, 0.0, 0.22),
        (1.0, 0.0, 0.08),
        (-1.0, 0.0, 0.08),
        (0.0, 1.0, 0.08),
        (0.0, -1.0, 0.08),
        (0.707, 0.707, 0.06),
        (-0.707, 0.707, 0.06),
        (0.707, -0.707, 0.06),
        (-0.707, -0.707, 0.06),
        (1.0, 1.0, 0.04),
        (-1.0, 1.0, 0.04),
        (1.0, -1.0, 0.04),
        (-1.0, -1.0, 0.04),
    ];
    samples
        .into_iter()
        .map(|(x, y, alpha)| TextShadowPaintPass {
            x_offset: x * radius * 0.45,
            y_offset: y * radius * 0.45,
            color: color_with_alpha_factor(color, alpha),
        })
        .collect()
}

fn color_with_alpha_factor(color: Color, factor: f32) -> Color {
    Color {
        a: (color.a * factor).clamp(0.0, 1.0),
        ..color
    }
}

/// Build prepared CSS text-emphasis annotations for one rendered line.
///
/// CSS Text Decoration attaches one emphasis mark to each eligible
/// typographic character unit. Building annotation records before paint keeps
/// mark selection aligned with CSS Text unit policy and lets writing-mode
/// placement use the same positioned rendered runs as normal text:
/// <https://www.w3.org/TR/css-text-decor-3/#text-emphasis-style-property> and
/// <https://www.w3.org/TR/css-text-3/#typographic-character-unit>.
fn prepared_text_emphasis_marks_for_line(
    line: &RenderedLine,
    style: &ComputedStyle,
    mark: &str,
    mark_width: f32,
) -> Vec<PreparedTextEmphasisMark> {
    if mark.is_empty() {
        return Vec::new();
    }
    let mut marks = Vec::new();
    for run in &line.runs {
        let Some(glyphs) = &run.glyphs else {
            continue;
        };
        for unit in rendered_text_run_typographic_units(&run.text, glyphs) {
            if !text_emphasis_unit_receives_mark(&unit.text, style.text_emphasis_skip) {
                continue;
            }
            let (x, y) = text_emphasis_mark_position(line, run, &unit, style, mark_width);
            marks.push(PreparedTextEmphasisMark {
                mark: mark.to_string(),
                source_text: unit.text,
                x,
                y,
            });
        }
    }
    marks
}

#[derive(Debug, Clone)]
struct RenderedTextUnit {
    text: String,
    start: f32,
    end: f32,
}

fn rendered_text_run_typographic_units(
    text: &str,
    glyphs: &[RenderedGlyph],
) -> Vec<RenderedTextUnit> {
    let unit_ranges = typographic_unit_ranges(text);
    let Some(first_range) = unit_ranges.first() else {
        return Vec::new();
    };
    let mut units = Vec::new();
    let mut unit_index = 0usize;
    let mut unit_end = first_range.end;
    let mut consumed_text_bytes = 0usize;
    let mut pending_text = String::new();
    let mut pending_start: Option<f32> = None;
    let mut pending_end = 0.0;
    let mut cursor = 0.0;

    for glyph in glyphs {
        if !glyph.unicode.is_empty() && pending_start.is_some() && consumed_text_bytes >= unit_end {
            push_rendered_text_unit(
                &mut units,
                &mut pending_text,
                &mut pending_start,
                &mut pending_end,
            );
            while unit_index + 1 < unit_ranges.len() && consumed_text_bytes >= unit_end {
                unit_index += 1;
                unit_end = unit_ranges[unit_index].end;
            }
        }

        let glyph_start = cursor + glyph.x_offset;
        let glyph_end = glyph_start + glyph.x_advance;
        pending_start = Some(pending_start.map_or(glyph_start, |start| start.min(glyph_start)));
        pending_end = pending_end.max(glyph_end);
        if !glyph.unicode.is_empty() {
            consumed_text_bytes += glyph.unicode.len();
            pending_text.push_str(&glyph.unicode);
        }
        cursor += glyph.x_advance;
    }

    push_rendered_text_unit(
        &mut units,
        &mut pending_text,
        &mut pending_start,
        &mut pending_end,
    );
    units
}

fn push_rendered_text_unit(
    units: &mut Vec<RenderedTextUnit>,
    text: &mut String,
    start: &mut Option<f32>,
    end: &mut f32,
) {
    let Some(start_value) = start.take() else {
        return;
    };
    units.push(RenderedTextUnit {
        text: std::mem::take(text),
        start: start_value,
        end: *end,
    });
    *end = 0.0;
}

fn text_emphasis_unit_receives_mark(text: &str, skip: TextEmphasisSkip) -> bool {
    text.chars()
        .find(|character| {
            !character_is_unicode_mark(*character)
                && !character_is_default_ignorable_code_point(*character)
        })
        .is_some_and(|character| character_receives_text_emphasis_mark_with_skip(character, skip))
}

fn text_emphasis_mark_position(
    line: &RenderedLine,
    run: &RenderedTextRun,
    unit: &RenderedTextUnit,
    style: &ComputedStyle,
    mark_width: f32,
) -> (f32, f32) {
    let vertical = style.writing_mode != WritingMode::HorizontalTb;
    if !vertical {
        let center = (unit.start + unit.end) / 2.0;
        let x = line.x() + run.x_offset + center - mark_width / 2.0;
        let y = if style.text_emphasis_position.over {
            line.y() + style.font_size * 0.55
        } else {
            line.y() - style.font_size * 0.35
        };
        return (x, y);
    }

    let side_offset = if style.text_emphasis_position.right {
        style.font_size * 0.55
    } else {
        -style.font_size * 0.55 - mark_width
    };
    let inline_anchor = if run.text_matrix.is_identity() {
        unit.start
    } else {
        (unit.start + unit.end) / 2.0
    };
    let (x, y) = transformed_text_run_point(line, run, inline_anchor, 0.0);
    (x + side_offset, y)
}

fn transformed_text_run_point(
    line: &RenderedLine,
    run: &RenderedTextRun,
    x: f32,
    y: f32,
) -> (f32, f32) {
    (
        line.x() + run.x_offset + run.text_matrix.a * x + run.text_matrix.c * y,
        line.y() + run.y_offset + run.text_matrix.b * x + run.text_matrix.d * y,
    )
}

fn character_receives_text_emphasis_mark_with_skip(
    character: char,
    skip: TextEmphasisSkip,
) -> bool {
    if !character_receives_text_emphasis_mark(character) {
        return false;
    }
    if skip.spaces && character_is_text_decoration_spacer(character) {
        return false;
    }
    if skip.punctuation && character_is_unicode_punctuation(character) {
        return false;
    }
    if skip.symbols && character_is_unicode_symbol(character) {
        return false;
    }
    if skip.narrow && character.is_ascii() {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::ComputedLengthPercentage;

    fn glyph(unicode: &str, advance: f32) -> RenderedGlyph {
        RenderedGlyph {
            id: 1,
            x_advance: advance,
            nominal_x_advance: advance,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: unicode.to_string(),
        }
    }

    fn rendered_line_with_run(
        text: &str,
        glyphs: Vec<RenderedGlyph>,
        run_y_offset: f32,
        matrix: RenderedTextMatrix,
    ) -> RenderedLine {
        RenderedLine::from_paint_origin(
            text.to_string(),
            paint_space_point(10.0, 20.0),
            10.0,
            None,
            Color::BLACK,
            vec![RenderedTextRun {
                text: text.to_string(),
                x_offset: 0.0,
                y_offset: run_y_offset,
                text_matrix: matrix,
                font_size: 10.0,
                font_id: None,
                glyphs: Some(glyphs),
            }],
        )
    }

    fn decoration_metrics() -> TextDecorationFontMetrics {
        TextDecorationFontMetrics {
            underline_position: -1.0,
            underline_thickness: 2.0,
            strikeout_position: 3.0,
            strikeout_thickness: 1.5,
            descender_depth: 2.0,
        }
    }

    fn prepared_decoration_strokes_for_style(
        style: &ComputedStyle,
        decoration: TextDecoration,
        phase: TextDecorationPaintPhase,
    ) -> Vec<PreparedTextDecorationStroke> {
        prepare_text_decoration_strokes(TextDecorationPreparationInput {
            x: 10.0,
            baseline_y: 20.0,
            width: 40.0,
            inset_start: 0.0,
            inset_end: 0.0,
            style,
            decoration,
            phase,
            color: Color::BLACK,
            color_override: None,
            metrics: decoration_metrics(),
        })
    }

    #[test]
    fn prepared_decoration_horizontal_positions_match_legacy_offsets() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        let mut decoration = style.text_decoration;
        decoration.underline = true;
        decoration.overline = true;
        decoration.line_through = true;

        let before = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );
        let after = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::AfterText,
        );

        assert_eq!(before.len(), 2);
        assert_eq!(after.len(), 1);
        assert_eq!(before[0].axis, TextDecorationStrokeAxis::Horizontal);
        assert!((before[0].inline_start - 10.0).abs() < 0.01);
        assert!((before[0].inline_length - 40.0).abs() < 0.01);
        assert!((before[0].block_position - 19.0).abs() < 0.01);
        assert!((before[1].block_position - 30.0).abs() < 0.01);
        assert!((after[0].block_position - 23.0).abs() < 0.01);
    }

    #[test]
    fn prepared_decoration_vertical_underline_resolves_to_logical_side() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        style.writing_mode = WritingMode::VerticalRl;
        let mut decoration = style.text_decoration;
        decoration.underline = true;
        decoration.underline_position.left = true;
        decoration.underline_position.auto = false;

        let left = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );
        decoration.underline_position.left = false;
        decoration.underline_position.right = true;
        let right = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );

        assert_eq!(left.len(), 1);
        assert_eq!(right.len(), 1);
        assert_eq!(left[0].axis, TextDecorationStrokeAxis::Vertical);
        assert!((left[0].inline_start + 20.0).abs() < 0.01, "{left:?}");
        assert!(left[0].block_position < 10.0, "{left:?}");
        assert!(right[0].block_position > 10.0, "{right:?}");
    }

    #[test]
    fn prepared_decoration_vertical_offset_moves_away_from_text() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        style.writing_mode = WritingMode::VerticalRl;
        let mut decoration = style.text_decoration;
        decoration.underline = true;
        decoration.underline_position.left = true;
        decoration.underline_position.auto = false;

        let without_offset = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );
        decoration.underline_offset =
            TextUnderlineOffset::LengthPercentage(ComputedLengthPercentage::from_length(4.0));
        let with_offset = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );

        assert!(with_offset[0].block_position < without_offset[0].block_position);
    }

    #[test]
    fn prepared_decoration_skip_spaces_uses_rotated_run_offsets() {
        let runs = vec![RenderedTextRun {
            text: " A".to_string(),
            x_offset: 0.0,
            y_offset: 0.0,
            text_matrix: RenderedTextMatrix::ROTATE_CW,
            font_size: 10.0,
            font_id: None,
            glyphs: Some(vec![glyph(" ", 10.0), glyph("A", 10.0)]),
        }];

        let ranges = text_decoration_space_skip_ranges(
            TextDecorationStrokeAxis::Vertical,
            10.0,
            100.0,
            80.0,
            30.0,
            TextDecorationSkipSpaces::START_END,
            &runs,
        );

        assert_eq!(ranges.len(), 1);
        assert!((ranges[0].0 - 90.0).abs() < 0.01, "{ranges:?}");
        assert!((ranges[0].1 - 100.0).abs() < 0.01, "{ranges:?}");
    }

    #[test]
    fn prepared_decoration_errors_use_wavy_annotation_path() {
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        let mut decoration = style.text_decoration;
        decoration.spelling_error = true;
        decoration.grammar_error = true;

        let strokes = prepared_decoration_strokes_for_style(
            &style,
            decoration,
            TextDecorationPaintPhase::BeforeText,
        );

        assert_eq!(strokes.len(), 2);
        assert!(
            strokes
                .iter()
                .all(|stroke| stroke.style == TextDecorationStyle::Wavy)
        );
        assert!(
            strokes
                .iter()
                .any(|stroke| stroke.color == Color::new(255, 0, 0))
        );
        assert!(
            strokes
                .iter()
                .any(|stroke| stroke.color == Color::new(0, 128, 0))
        );
    }

    #[test]
    fn prepared_emphasis_annotations_use_typographic_units() {
        let text = "e\u{301}A";
        let line = rendered_line_with_run(
            text,
            vec![glyph("e", 8.0), glyph("\u{301}", 0.0), glyph("A", 10.0)],
            0.0,
            RenderedTextMatrix::IDENTITY,
        );
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;

        let marks = prepared_text_emphasis_marks_for_line(&line, &style, "•", 2.0);

        assert_eq!(
            marks
                .iter()
                .map(|mark| mark.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["e\u{301}", "A"]
        );
        assert_eq!(marks.len(), 2);
        assert!((marks[0].x - 13.0).abs() < 0.01, "{marks:?}");
        assert!((marks[1].x - 22.0).abs() < 0.01, "{marks:?}");
    }

    #[test]
    fn prepared_emphasis_annotations_apply_unicode_skip_policy() {
        let text = "A!★";
        let line = rendered_line_with_run(
            text,
            vec![glyph("A", 10.0), glyph("!", 10.0), glyph("★", 10.0)],
            0.0,
            RenderedTextMatrix::IDENTITY,
        );
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;

        let default_marks = prepared_text_emphasis_marks_for_line(&line, &style, "•", 2.0);
        assert_eq!(
            default_marks
                .iter()
                .map(|mark| mark.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "★"]
        );

        style.text_emphasis_skip.symbols = true;
        let symbol_skipping_marks = prepared_text_emphasis_marks_for_line(&line, &style, "•", 2.0);
        assert_eq!(
            symbol_skipping_marks
                .iter()
                .map(|mark| mark.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["A"]
        );

        let punctuation_with_mark = rendered_line_with_run(
            "!\u{301}",
            vec![glyph("!", 10.0), glyph("\u{301}", 0.0)],
            0.0,
            RenderedTextMatrix::IDENTITY,
        );
        let marks = prepared_text_emphasis_marks_for_line(&punctuation_with_mark, &style, "•", 2.0);
        assert!(marks.is_empty(), "{marks:?}");
    }

    #[test]
    fn prepared_vertical_emphasis_uses_logical_side_and_run_offset() {
        let line = rendered_line_with_run(
            "中",
            vec![glyph("中", 10.0)],
            -12.0,
            RenderedTextMatrix::IDENTITY,
        );
        let mut style = ComputedStyle::initial();
        style.font_size = 10.0;
        style.writing_mode = WritingMode::VerticalRl;

        let right_marks = prepared_text_emphasis_marks_for_line(&line, &style, "﹅", 2.0);
        style.text_emphasis_position.right = false;
        let left_marks = prepared_text_emphasis_marks_for_line(&line, &style, "﹅", 2.0);

        assert_eq!(right_marks.len(), 1);
        assert_eq!(left_marks.len(), 1);
        assert!(
            right_marks[0].x > left_marks[0].x,
            "{right_marks:?} {left_marks:?}"
        );
        assert!((right_marks[0].y - 8.0).abs() < 0.01, "{right_marks:?}");
        assert!((left_marks[0].y - 8.0).abs() < 0.01, "{left_marks:?}");
    }
}

fn active_text_decoration_layers(style: &ComputedStyle) -> Vec<TextDecoration> {
    if !style.text_decoration_layers.is_empty() {
        return style.text_decoration_layers.clone();
    }
    if style.text_decoration.has_visible_line() {
        return vec![style.text_decoration];
    }
    Vec::new()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextDecorationPaintPhase {
    BeforeText,
    AfterText,
    All,
}

impl TextDecorationPaintPhase {
    fn paints_before_text(self) -> bool {
        matches!(self, Self::BeforeText | Self::All)
    }

    fn paints_after_text(self) -> bool {
        matches!(self, Self::AfterText | Self::All)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextDecorationLineKind {
    Underline,
    Overline,
    LineThrough,
}

fn text_decoration_skip_self_suppresses(
    style: &ComputedStyle,
    line: TextDecorationLineKind,
) -> bool {
    match style.text_decoration.skip_self {
        TextDecorationSkipSelf::Auto | TextDecorationSkipSelf::NoSkip => false,
        TextDecorationSkipSelf::SkipAll => true,
        TextDecorationSkipSelf::Lines {
            underline,
            overline,
            line_through,
        } => match line {
            TextDecorationLineKind::Underline => underline,
            TextDecorationLineKind::Overline => overline,
            TextDecorationLineKind::LineThrough => line_through,
        },
    }
}

#[derive(Debug, Clone, Copy)]
struct TextDecorationPreparationInput<'a> {
    x: f32,
    baseline_y: f32,
    width: f32,
    inset_start: f32,
    inset_end: f32,
    style: &'a ComputedStyle,
    decoration: TextDecoration,
    phase: TextDecorationPaintPhase,
    color: Color,
    color_override: Option<Color>,
    metrics: TextDecorationFontMetrics,
}

/// Prepare CSS text-decoration strokes for one rendered inline line.
///
/// CSS Text Decoration paints decoration lines relative to the decorated text's
/// inline axis. Preparing strokes in an axis-aware form before emitting PDF
/// primitives lets horizontal and vertical writing share the same skip and
/// style pipeline:
/// <https://www.w3.org/TR/css-text-decor-3/#line-decoration> and
/// <https://www.w3.org/TR/css-writing-modes-4/#line-directions>.
fn prepare_text_decoration_strokes(
    input: TextDecorationPreparationInput<'_>,
) -> Vec<PreparedTextDecorationStroke> {
    let TextDecorationPreparationInput {
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
    } = input;
    if width <= 0.0 {
        return Vec::new();
    }

    let axis = if style.writing_mode == WritingMode::HorizontalTb {
        TextDecorationStrokeAxis::Horizontal
    } else {
        TextDecorationStrokeAxis::Vertical
    };
    let Some((inline_start, inline_length)) =
        text_decoration_inline_span(axis, x, baseline_y, width, inset_start, inset_end, style)
    else {
        return Vec::new();
    };

    let underline_thickness =
        used_text_decoration_thickness(decoration.thickness, style.font_size, &metrics, false);
    let strikeout_thickness =
        used_text_decoration_thickness(decoration.thickness, style.font_size, &metrics, true);
    let mut strokes = Vec::new();

    if phase.paints_before_text()
        && decoration.underline
        && !text_decoration_skip_self_suppresses(style, TextDecorationLineKind::Underline)
    {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            line_x: x,
            line_y: baseline_y,
            inline_start,
            inline_length,
            block_position: text_decoration_block_position(
                axis,
                x,
                baseline_y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                underline_thickness,
                TextDecorationPreparedLineKind::Underline,
            ),
            thickness: underline_thickness,
            color,
            style: decoration.style,
            skip_ink: decoration.skip_ink,
            skip_spaces: decoration.skip_spaces,
        });
    }

    if phase.paints_before_text()
        && decoration.overline
        && !text_decoration_skip_self_suppresses(style, TextDecorationLineKind::Overline)
    {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            line_x: x,
            line_y: baseline_y,
            inline_start,
            inline_length,
            block_position: text_decoration_block_position(
                axis,
                x,
                baseline_y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                underline_thickness,
                TextDecorationPreparedLineKind::Overline,
            ),
            thickness: underline_thickness,
            color,
            style: decoration.style,
            skip_ink: decoration.skip_ink,
            skip_spaces: decoration.skip_spaces,
        });
    }

    if phase.paints_after_text()
        && decoration.line_through
        && !text_decoration_skip_self_suppresses(style, TextDecorationLineKind::LineThrough)
    {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            line_x: x,
            line_y: baseline_y,
            inline_start,
            inline_length,
            block_position: text_decoration_block_position(
                axis,
                x,
                baseline_y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                strikeout_thickness,
                TextDecorationPreparedLineKind::LineThrough,
            ),
            thickness: strikeout_thickness,
            color,
            style: decoration.style,
            skip_ink: decoration.skip_ink,
            skip_spaces: decoration.skip_spaces,
        });
    }

    if phase.paints_before_text() && decoration.spelling_error {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            line_x: x,
            line_y: baseline_y,
            inline_start,
            inline_length,
            block_position: text_decoration_block_position(
                axis,
                x,
                baseline_y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                underline_thickness,
                TextDecorationPreparedLineKind::Underline,
            ),
            thickness: underline_thickness,
            color: color_override.unwrap_or(Color::new(255, 0, 0)),
            style: TextDecorationStyle::Wavy,
            skip_ink: TextDecorationSkipInk::None,
            skip_spaces: TextDecorationSkipSpaces::NONE,
        });
    }

    if phase.paints_before_text() && decoration.grammar_error {
        strokes.push(PreparedTextDecorationStroke {
            axis,
            line_x: x,
            line_y: baseline_y,
            inline_start,
            inline_length,
            block_position: text_decoration_block_position(
                axis,
                x,
                baseline_y,
                style,
                decoration.underline_position,
                decoration.underline_offset,
                &metrics,
                underline_thickness,
                TextDecorationPreparedLineKind::Underline,
            ),
            thickness: underline_thickness,
            color: color_override.unwrap_or(Color::new(0, 128, 0)),
            style: TextDecorationStyle::Wavy,
            skip_ink: TextDecorationSkipInk::None,
            skip_spaces: TextDecorationSkipSpaces::NONE,
        });
    }

    strokes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextDecorationPreparedLineKind {
    Underline,
    Overline,
    LineThrough,
}

fn text_decoration_inline_span(
    axis: TextDecorationStrokeAxis,
    x: f32,
    baseline_y: f32,
    width: f32,
    inset_start: f32,
    inset_end: f32,
    style: &ComputedStyle,
) -> Option<(f32, f32)> {
    let length = (width - inset_start - inset_end).max(0.0);
    if length <= 0.0 {
        return None;
    }
    match axis {
        TextDecorationStrokeAxis::Horizontal => {
            let start = match style.direction {
                Direction::Ltr => x + inset_start,
                Direction::Rtl => x + inset_end,
            };
            Some((start, length))
        }
        TextDecorationStrokeAxis::Vertical => {
            if vertical_text_advance_sign(style) < 0.0 {
                Some((baseline_y - width + inset_end, length))
            } else {
                Some((baseline_y + inset_start, length))
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn text_decoration_block_position(
    axis: TextDecorationStrokeAxis,
    x: f32,
    baseline_y: f32,
    style: &ComputedStyle,
    underline_position: TextUnderlinePosition,
    underline_offset: TextUnderlineOffset,
    metrics: &TextDecorationFontMetrics,
    thickness: f32,
    kind: TextDecorationPreparedLineKind,
) -> f32 {
    match axis {
        TextDecorationStrokeAxis::Horizontal => match kind {
            TextDecorationPreparedLineKind::Underline => used_underline_y(
                baseline_y,
                underline_position,
                underline_offset,
                style.font_size,
                metrics,
                thickness,
            ),
            TextDecorationPreparedLineKind::Overline => baseline_y + style.font_size,
            TextDecorationPreparedLineKind::LineThrough => baseline_y + metrics.strikeout_position,
        },
        TextDecorationStrokeAxis::Vertical => {
            let offset = used_text_underline_offset(underline_offset, style.font_size).max(0.0);
            match kind {
                TextDecorationPreparedLineKind::Underline => {
                    vertical_text_decoration_side_position(
                        x,
                        style,
                        resolve_vertical_underline_side(underline_position, style.writing_mode),
                        thickness,
                        offset,
                    )
                }
                TextDecorationPreparedLineKind::Overline => vertical_text_decoration_side_position(
                    x,
                    style,
                    opposite_text_decoration_side(resolve_vertical_underline_side(
                        underline_position,
                        style.writing_mode,
                    )),
                    thickness,
                    offset,
                ),
                TextDecorationPreparedLineKind::LineThrough => x + style.font_size * 0.5,
            }
        }
    }
}

fn vertical_text_advance_sign(style: &ComputedStyle) -> f32 {
    let placement_direction = if matches!(style.text_orientation, TextOrientation::Upright) {
        Direction::Ltr
    } else {
        style.direction
    };
    match placement_direction {
        Direction::Ltr => -1.0,
        Direction::Rtl => 1.0,
    }
}

fn resolve_vertical_underline_side(
    position: TextUnderlinePosition,
    writing_mode: WritingMode,
) -> TextDecorationSide {
    if position.left {
        return TextDecorationSide::Left;
    }
    if position.right {
        return TextDecorationSide::Right;
    }
    match writing_mode {
        WritingMode::HorizontalTb | WritingMode::VerticalRl => TextDecorationSide::Right,
        WritingMode::VerticalLr => TextDecorationSide::Left,
    }
}

fn opposite_text_decoration_side(side: TextDecorationSide) -> TextDecorationSide {
    match side {
        TextDecorationSide::Left => TextDecorationSide::Right,
        TextDecorationSide::Right => TextDecorationSide::Left,
    }
}

fn vertical_text_decoration_side_position(
    x: f32,
    style: &ComputedStyle,
    side: TextDecorationSide,
    thickness: f32,
    offset: f32,
) -> f32 {
    match side {
        TextDecorationSide::Left => x - thickness - offset,
        TextDecorationSide::Right => x + style.font_size + offset,
    }
}

/// Adjust inline fragment background/border painting for sliced inline boxes.
///
/// CSS Fragmentation defines `box-decoration-break: slice` as the initial
/// behavior: inline-start decorations are painted only on the first fragment,
/// inline-end decorations only on the last fragment, while top/bottom
/// decorations continue on every line fragment. CSS 2.2 positions non-replaced
/// inline padding and borders from the content-area edges, not from the
/// line-height box:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>,
/// <https://www.w3.org/TR/CSS22/visudet.html#line-height>, and
/// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>.
fn apply_inline_fragment_edge_painting(
    style: &mut ComputedStyle,
    edges: InlineHangingEdges,
    x: &mut f32,
    y: &mut f32,
    width: &mut f32,
    height: &mut f32,
) {
    let borders = used_border_widths(style);
    let top_extra = borders.top + style.padding.top;
    let bottom_extra = borders.bottom + style.padding.bottom;
    *y -= bottom_extra;
    *height += top_extra + bottom_extra;
    let start_extra = match style.direction {
        Direction::Ltr => borders.left + style.padding.left,
        Direction::Rtl => borders.right + style.padding.right,
    };
    let end_extra = match style.direction {
        Direction::Ltr => borders.right + style.padding.right,
        Direction::Rtl => borders.left + style.padding.left,
    };
    if edges.blocks_start {
        match style.direction {
            Direction::Ltr => *x -= start_extra,
            Direction::Rtl => {}
        }
        *width += start_extra;
    } else {
        match style.direction {
            Direction::Ltr => {
                style.border_widths.left = 0.0;
                style.border_styles.left = BorderStyle::None;
                style.padding.left = 0.0;
            }
            Direction::Rtl => {
                style.border_widths.right = 0.0;
                style.border_styles.right = BorderStyle::None;
                style.padding.right = 0.0;
            }
        }
    }
    if edges.blocks_end {
        match style.direction {
            Direction::Ltr => {}
            Direction::Rtl => *x -= end_extra,
        }
        *width += end_extra;
    } else {
        match style.direction {
            Direction::Ltr => {
                style.border_widths.right = 0.0;
                style.border_styles.right = BorderStyle::None;
                style.padding.right = 0.0;
            }
            Direction::Rtl => {
                style.border_widths.left = 0.0;
                style.border_styles.left = BorderStyle::None;
                style.padding.left = 0.0;
            }
        }
    }
}

/// Build the debug/extraction summary for a painted inline-fragment group.
///
/// CSS Text collapses document white space at inline box boundaries before
/// paint groups are prepared, while Parley-shaped glyph runs preserve the
/// actual Unicode clusters emitted to PDF. `RenderedLine::text` is a line
/// summary used by layout tests and diagnostics, so it keeps internal
/// collapsed spaces even when style or bidi boundaries split one line into
/// several PDF text objects:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
fn inline_fragment_text_summary(
    fragments: &[InlineFragment],
    preserve_leading_summary_space: bool,
) -> String {
    let mut summary = String::new();
    for (index, fragment) in fragments.iter().enumerate() {
        if index == 0
            && !preserve_leading_summary_space
            && fragment.style.white_space.collapses_spaces()
        {
            summary.push_str(trim_start_css_collapsible_whitespace(&fragment.text));
        } else {
            summary.push_str(&fragment.text);
        }
    }
    summary
}

/// Resolve CSS `text-decoration-thickness` to a used line thickness.
///
/// CSS Text Decoration Level 4 defines `auto`, `from-font`, and
/// length-percentage values. `from-font` uses OpenType decoration metrics when
/// available:
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-width-property>.
fn used_text_decoration_thickness(
    thickness: TextDecorationThickness,
    font_size: f32,
    metrics: &TextDecorationFontMetrics,
    line_through: bool,
) -> f32 {
    match thickness {
        TextDecorationThickness::Auto => (font_size / 16.0).max(0.5),
        TextDecorationThickness::FromFont if line_through => metrics.strikeout_thickness,
        TextDecorationThickness::FromFont => metrics.underline_thickness,
        TextDecorationThickness::LengthPercentage(value) => value
            .used_length_with_percentage_basis(font_size)
            .unwrap_or(value.length + value.percent * font_size)
            .max(0.5),
    }
}

/// Resolve the CSS underline baseline position for horizontal writing.
///
/// `text-underline-position: under` places the underline below descenders in
/// horizontal writing; vertical-writing side placement is handled separately
/// once vertical text layout exists:
/// <https://www.w3.org/TR/css-text-decor-3/#text-underline-position-property>.
fn used_underline_y(
    baseline_y: f32,
    position: TextUnderlinePosition,
    offset: TextUnderlineOffset,
    font_size: f32,
    metrics: &TextDecorationFontMetrics,
    thickness: f32,
) -> f32 {
    let font_position = metrics.underline_position;
    let under_position = -metrics.descender_depth - thickness;
    let base_offset = if position.under {
        font_position.min(under_position)
    } else {
        font_position
    };
    baseline_y + base_offset - used_text_underline_offset(offset, font_size)
}

#[derive(Debug, Clone, Copy)]
struct TextDecorationSegmentInputs {
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    line_y: f32,
    inline_start: f32,
    inline_length: f32,
    block_position: f32,
    thickness: f32,
    skip_ink: TextDecorationSkipInk,
    skip_spaces: TextDecorationSkipSpaces,
}

#[derive(Debug, Clone, Copy)]
struct TextDecorationSegment {
    start: f32,
    length: f32,
}

/// Split a text-decoration stroke around skipped spaces and glyph ink.
///
/// CSS Text Decoration Level 4 defines both `text-decoration-skip-spaces` and
/// `text-decoration-skip-ink` as clipping behavior applied to decoration
/// strokes:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-spaces-property>
/// and
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-skip-ink-property>.
fn text_decoration_segments(
    inputs: TextDecorationSegmentInputs,
    runs: &[RenderedTextRun],
    ink_boxes: &[GlyphInkBox],
) -> Vec<TextDecorationSegment> {
    let TextDecorationSegmentInputs {
        axis,
        line_x,
        line_y,
        inline_start,
        inline_length,
        block_position,
        thickness,
        skip_ink,
        skip_spaces,
    } = inputs;
    if inline_length <= 0.0 {
        return Vec::new();
    }

    let inline_end = inline_start + inline_length;
    let padding = thickness.max(0.5);
    let mut skips = text_decoration_space_skip_ranges(
        axis,
        line_x,
        line_y,
        inline_start,
        inline_length,
        skip_spaces,
        runs,
    );
    if skip_ink != TextDecorationSkipInk::None {
        skips.extend(
            ink_boxes
                .iter()
                .filter(|ink| {
                    text_decoration_ink_intersects_cross_axis(
                        axis,
                        line_x,
                        block_position,
                        thickness,
                        ink,
                    )
                })
                .filter_map(|ink| {
                    let (ink_start, ink_end) = text_decoration_ink_inline_range(axis, line_x, ink);
                    let start = (ink_start - padding).max(inline_start);
                    let end = (ink_end + padding).min(inline_end);
                    (end > start).then_some((start, end))
                }),
        );
    }
    if skips.is_empty() {
        return vec![TextDecorationSegment {
            start: inline_start,
            length: inline_length,
        }];
    }

    skips.sort_by(|left, right| {
        left.0
            .partial_cmp(&right.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut merged = Vec::<(f32, f32)>::new();
    for (start, end) in skips {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
            continue;
        }
        merged.push((start, end));
    }

    let mut segments = Vec::new();
    let mut cursor = inline_start;
    for (start, end) in merged {
        if start > cursor {
            segments.push(TextDecorationSegment {
                start: cursor,
                length: start - cursor,
            });
        }
        cursor = cursor.max(end);
    }
    if cursor < inline_end {
        segments.push(TextDecorationSegment {
            start: cursor,
            length: inline_end - cursor,
        });
    }
    segments
}

fn text_decoration_ink_intersects_cross_axis(
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    block_position: f32,
    thickness: f32,
    ink: &GlyphInkBox,
) -> bool {
    match axis {
        TextDecorationStrokeAxis::Horizontal => {
            ink.y_min <= block_position + thickness && ink.y_max >= block_position
        }
        TextDecorationStrokeAxis::Vertical => {
            line_x + ink.x_min <= block_position + thickness && line_x + ink.x_max >= block_position
        }
    }
}

fn text_decoration_ink_inline_range(
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    ink: &GlyphInkBox,
) -> (f32, f32) {
    match axis {
        TextDecorationStrokeAxis::Horizontal => (line_x + ink.x_min, line_x + ink.x_max),
        TextDecorationStrokeAxis::Vertical => (ink.y_min, ink.y_max),
    }
}

/// Return decoration clipping ranges for CSS `text-decoration-skip-spaces`.
///
/// CSS Text Decoration Level 4 defines spacers as Unicode `Zs` characters
/// except U+202F, and `all` also skips word separators plus adjacent
/// letter/word spacing. Shaped glyph advances are used here so bidi,
/// ligatures, fallback fonts, and letter spacing clip the painted decoration at
/// used-value positions:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-spaces-property>.
fn text_decoration_space_skip_ranges(
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    line_y: f32,
    inline_start: f32,
    inline_length: f32,
    skip_spaces: TextDecorationSkipSpaces,
    runs: &[RenderedTextRun],
) -> Vec<(f32, f32)> {
    if inline_length <= 0.0 || skip_spaces == TextDecorationSkipSpaces::NONE {
        return Vec::new();
    }

    let glyphs =
        text_decoration_positioned_glyphs(axis, line_x, line_y, inline_start, inline_length, runs);
    if glyphs.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    if skip_spaces.skips_all() {
        for (index, glyph) in glyphs.iter().enumerate() {
            if !text_decoration_glyph_is_spacer(&glyph.unicode) {
                continue;
            }
            let previous_extra_spacing = if index > 0 {
                glyphs[index - 1].extra_spacing
            } else {
                0.0
            };
            let start = (glyph.inline_start - previous_extra_spacing).max(inline_start);
            let end = glyph.inline_end.min(inline_start + inline_length);
            if end > start {
                ranges.push((start, end));
            }
        }
        return ranges;
    }

    if skip_spaces.skips_line_start() {
        for glyph in &glyphs {
            if !text_decoration_glyph_is_spacer(&glyph.unicode) {
                break;
            }
            let start = glyph.inline_start.max(inline_start);
            let end = glyph.inline_end.min(inline_start + inline_length);
            if end > start {
                ranges.push((start, end));
            }
        }
    }

    if skip_spaces.skips_line_end() {
        let mut trailing = Vec::new();
        for index in (0..glyphs.len()).rev() {
            let glyph = &glyphs[index];
            if !text_decoration_glyph_is_spacer(&glyph.unicode) {
                break;
            }
            let previous_extra_spacing = if index > 0 && trailing.is_empty() {
                glyphs[index - 1].extra_spacing
            } else {
                0.0
            };
            trailing.push((
                (glyph.inline_start - previous_extra_spacing).max(inline_start),
                glyph.inline_end.min(inline_start + inline_length),
            ));
        }
        for (start, end) in trailing.into_iter().rev() {
            if end > start {
                ranges.push((start, end));
            }
        }
    }

    ranges
}

#[derive(Debug, Clone)]
struct TextDecorationPositionedGlyph {
    unicode: String,
    inline_start: f32,
    inline_end: f32,
    extra_spacing: f32,
}

/// Return shaped glyph advances positioned in line visual order.
///
/// CSS Text Decoration clips decorations in the coordinate space of the
/// rendered line, so this preserves run offsets and shaped glyph advances
/// instead of remeasuring flattened source text:
/// <https://www.w3.org/TR/css-text-decor-4/#painting>.
fn text_decoration_positioned_glyphs(
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    line_y: f32,
    inline_start: f32,
    inline_length: f32,
    runs: &[RenderedTextRun],
) -> Vec<TextDecorationPositionedGlyph> {
    let inline_end = inline_start + inline_length;
    let mut positioned = Vec::new();
    for run in runs {
        let Some(glyphs) = &run.glyphs else {
            continue;
        };
        let mut pen_x = 0.0;
        for glyph in glyphs {
            let local_start = pen_x + glyph.x_offset;
            let local_end = (pen_x + glyph.x_advance).max(local_start);
            let (start, end) = text_decoration_glyph_inline_range(
                axis,
                line_x,
                line_y,
                run,
                local_start,
                local_end,
            );
            if end > inline_start && start < inline_end {
                positioned.push(TextDecorationPositionedGlyph {
                    unicode: glyph.unicode.clone(),
                    inline_start: start.max(inline_start),
                    inline_end: end.min(inline_end),
                    extra_spacing: (glyph.x_advance - glyph.nominal_x_advance).max(0.0),
                });
            }
            pen_x += glyph.x_advance;
        }
    }
    positioned
}

fn text_decoration_glyph_inline_range(
    axis: TextDecorationStrokeAxis,
    line_x: f32,
    line_y: f32,
    run: &RenderedTextRun,
    local_start: f32,
    local_end: f32,
) -> (f32, f32) {
    let (start, end) = match axis {
        TextDecorationStrokeAxis::Horizontal => (
            line_x + run.x_offset + run.text_matrix.a * local_start,
            line_x + run.x_offset + run.text_matrix.a * local_end,
        ),
        TextDecorationStrokeAxis::Vertical if run.text_matrix.is_identity() => {
            let baseline = line_y + run.y_offset;
            (
                baseline - run.font_size * 0.5,
                baseline + run.font_size * 0.5,
            )
        }
        TextDecorationStrokeAxis::Vertical => (
            line_y + run.y_offset + run.text_matrix.b * local_start,
            line_y + run.y_offset + run.text_matrix.b * local_end,
        ),
    };
    if start <= end {
        (start, end)
    } else {
        (end, start)
    }
}

fn text_decoration_glyph_is_spacer(unicode: &str) -> bool {
    !unicode.is_empty() && unicode.chars().all(character_is_text_decoration_spacer)
}

/// Resolve CSS `text-underline-offset` to a used offset.
///
/// CSS Text Decoration Level 4 defines underline offset as `auto` or a
/// length-percentage, applied away from the text in horizontal writing:
/// <https://www.w3.org/TR/css-text-decor-4/#text-underline-offset-property>.
fn used_text_underline_offset(offset: TextUnderlineOffset, font_size: f32) -> f32 {
    match offset {
        TextUnderlineOffset::Auto => 0.0,
        TextUnderlineOffset::LengthPercentage(value) => value
            .used_length_with_percentage_basis(font_size)
            .unwrap_or(value.length + value.percent * font_size),
    }
}
