use super::*;
use crate::css::{TextDecoration, TextDecorationSkipSelf, TextEmphasisSkip, TextEmphasisStyle};
use crate::text::character_is_text_decoration_spacer;

/// Used values for one CSS text-decoration stroke.
///
/// CSS Text Decoration resolves line style, color, thickness, offset, and
/// skip-ink before painting each decoration line:
/// <https://www.w3.org/TR/css-text-decor-4/#painting>.
#[derive(Debug, Clone, Copy)]
struct TextDecorationStroke {
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
    style: TextDecorationStyle,
    skip_ink: TextDecorationSkipInk,
    skip_spaces: TextDecorationSkipSpaces,
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
        let plan = self.inline_fragmentation_plan_for_text(
            text,
            style,
            available_width,
            padding_left,
            link_target,
        );
        self.paint_inline_fragmentation_plan(&plan, style);
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
        let plan = self.inline_fragmentation_plan_for_text(
            text,
            style,
            available_width,
            padding_left,
            link_target,
        );
        self.paint_inline_fragmentation_plan_slice(
            &plan,
            style,
            block_top,
            slice_top,
            slice_bottom,
        );
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
        let plan = self.collect_inline_fragmentation_plan(
            items,
            style,
            available_width,
            padding_left,
            0.0,
        );
        self.paint_inline_fragmentation_plan_slice(
            &plan,
            style,
            block_top,
            slice_top,
            slice_bottom,
        );
    }

    pub(in crate::layout) fn inline_fragmentation_plan_for_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        link_target: Option<&str>,
    ) -> inline_layout::InlineFragmentationPlan {
        let text = transform_text(text, style);
        self.inline_fragmentation_plan_for_prepared_text(
            &text,
            style,
            available_width,
            padding_left,
            link_target,
        )
    }

    pub(in crate::layout) fn inline_fragmentation_plan_for_prepared_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        link_target: Option<&str>,
    ) -> inline_layout::InlineFragmentationPlan {
        let mut items = Vec::new();
        self.push_inline_words(
            text,
            style,
            link_target.map(str::to_string),
            0.0,
            &mut items,
        );
        self.collect_inline_fragmentation_plan(items, style, available_width, padding_left, 0.0)
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
            let plan = self.collect_inline_fragmentation_plan(
                items,
                style,
                available_width,
                padding_left,
                0.0,
            );
            self.paint_inline_fragmentation_plan(&plan, style);
            return;
        }

        let plan = self.inline_fragmentation_plan_for_prepared_text(
            &text,
            style,
            available_width,
            padding_left,
            link_target,
        );
        self.paint_inline_fragmentation_plan_with_outside_marker(
            &plan,
            style,
            marker,
            self.content_left + padding_left,
            self.content_right - padding_right,
        );
    }

    pub(in crate::layout) fn fragment_line_count<F>(
        &self,
        total_lines: usize,
        start_index: usize,
        style: &ComputedStyle,
        mut line_height: F,
    ) -> usize
    where
        F: FnMut(usize) -> f32,
    {
        let remaining_total = total_lines.saturating_sub(start_index);
        if remaining_total == 0 {
            return 0;
        }

        let available_height = self.cursor_y - self.page_bottom();
        let mut used_height = 0.0;
        let mut fitting = 0;
        for index in start_index..total_lines {
            let height = line_height(index);
            if used_height + height > available_height + 0.01 {
                break;
            }
            used_height += height;
            fitting += 1;
        }

        if fitting == 0 {
            return usize::from(self.cursor_is_at_page_top());
        }
        if fitting >= remaining_total {
            return remaining_total;
        }

        // CSS Fragmentation 3 defines `orphans` and `widows` as constraints on
        // unforced line breaks inside a block container.
        // https://www.w3.org/TR/css-break-3/#widows-orphans
        let orphans = style.orphans.min(remaining_total).max(1);
        let widows = style.widows.min(remaining_total).max(1);
        if fitting < orphans && !self.cursor_is_at_page_top() {
            return 0;
        }

        let remaining_after_break = remaining_total - fitting;
        if remaining_after_break < widows && fitting > orphans {
            return (remaining_total - widows).max(orphans);
        }

        fitting
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
        let rendered_runs = shaped.rendered_runs();
        if rendered_runs.is_empty() {
            return None;
        }
        debug_assert!(shaped.advance_width().is_finite());
        let first_font_id = shaped.first_font_id();
        let y = y + shaped.baseline_adjustment;
        let rendered_line = RenderedLine {
            text: shaped.text.clone(),
            x,
            y,
            font_size: rendered_line_font_size(&rendered_runs, style.font_size),
            font_id: first_font_id,
            color: style.color,
            runs: rendered_runs,
        };
        self.paint_text_shadows(&rendered_line, style);
        self.paint_text_decoration_lines_for_phase(
            rendered_line.x,
            rendered_line.y,
            shaped.advance_width(),
            style,
            &rendered_line.runs,
            TextDecorationPaintPhase::BeforeText,
        );
        self.push_line_in_band(PaintBand::Inline, rendered_line.clone());
        self.paint_emphasis_marks_for_line(&rendered_line, style);
        self.paint_text_decoration_lines_for_phase(
            rendered_line.x,
            rendered_line.y,
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
    pub(super) fn prepare_inline_text_group(
        &mut self,
        fragments: &[InlineFragment],
        x: f32,
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

        let text_summary = inline_fragment_text_summary(&visible_fragments);
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
            x,
            y: y + baseline_adjustment,
            width,
            style: first.style.clone(),
            link_target: first.link_target.clone(),
            shaped,
        })
    }

    pub(super) fn prepare_justified_inline_text_group(
        &mut self,
        fragments: &[InlineFragment],
        x: f32,
        extra_per_separator: f32,
    ) -> Option<PreparedInlineTextGroup> {
        let mut group = self.prepare_inline_text_group(fragments, x)?;
        let separator_count = justifiable_fragment_space_count(fragments);
        let added_width = group
            .shaped
            .apply_inter_word_justification(extra_per_separator, separator_count);
        group.width += added_width;
        Some(group)
    }

    /// Paint a prepared inline text group without reshaping.
    ///
    /// PDF text emission must use the same glyph ids, advances, and fallback
    /// font ids chosen during CSS inline layout preparation:
    /// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm> and
    /// ISO 32000-2:2020, 9.4 "Text".
    pub(super) fn paint_prepared_inline_text_group(&mut self, group: &PreparedInlineTextGroup) {
        let rendered_runs = group.shaped.rendered_runs();
        if rendered_runs.is_empty() {
            return;
        }
        let first_font_id = group.shaped.first_font_id();
        let rendered_line = RenderedLine {
            text: group.shaped.text.clone(),
            x: group.x,
            y: group.y,
            font_size: rendered_line_font_size(&rendered_runs, group.style.font_size),
            font_id: first_font_id,
            color: group.style.color,
            runs: rendered_runs,
        };
        let decoration_runs = rendered_line.runs.clone();
        self.paint_text_shadows(&rendered_line, &group.style);
        self.paint_text_decoration_lines_for_phase(
            group.x,
            group.y,
            group.width,
            &group.style,
            &decoration_runs,
            TextDecorationPaintPhase::BeforeText,
        );
        self.push_line_in_band(PaintBand::Inline, rendered_line.clone());
        self.paint_emphasis_marks_for_line(&rendered_line, &group.style);
        self.paint_text_decoration_lines_for_phase(
            group.x,
            group.y,
            group.width,
            &group.style,
            &decoration_runs,
            TextDecorationPaintPhase::AfterText,
        );

        if let Some(target) = &group.link_target {
            self.current_page.push_link(RenderedLink {
                x: group.x,
                y: group.y - 2.0,
                width: group.width,
                height: group.style.font_size + 4.0,
                target: target.clone(),
            });
        }
    }

    /// Paint one inline fragment's background and border for a line box.
    ///
    /// CSS Backgrounds and Borders applies backgrounds and borders to inline
    /// boxes on each generated line box fragment. CSS Text hanging separators
    /// remain part of the fragment for painting even when excluded from line
    /// measurement:
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-color> and
    /// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
    pub(super) fn paint_inline_fragment_background(
        &mut self,
        fragment: &InlineFragment,
        mut x: f32,
        y: f32,
        mut width: f32,
        height: f32,
    ) {
        if fragment.style.visibility != Visibility::Visible
            || !fragment.style.display.is_inline_level()
            || width <= 0.0
            || height <= 0.0
            || (fragment.style.background_color.is_none()
                && fragment.style.background_image.is_none()
                && used_border_width(&fragment.style) == 0.0)
        {
            return;
        }
        let mut style = fragment.style.clone();
        apply_inline_fragment_edge_painting(&mut style, fragment.hanging_edges, &mut x, &mut width);
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
                shadow_line.x += shadow.offset_x + pass.x_offset;
                shadow_line.y -= shadow.offset_y + pass.y_offset;
                shadow_line.color = pass.color;
                self.paint_text_decoration_lines_for_phase_with_color(
                    shadow_line.x,
                    shadow_line.y,
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

    fn paint_emphasis_marks_for_line(&mut self, line: &RenderedLine, style: &ComputedStyle) {
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
        let vertical = style.writing_mode != WritingMode::HorizontalTb;
        for run in &line.runs {
            let Some(glyphs) = &run.glyphs else {
                continue;
            };
            let mut pen_x = line.x + run.x_offset;
            for glyph in glyphs {
                let receives_mark = glyph.unicode.chars().any(|character| {
                    character_receives_text_emphasis_mark_with_skip(
                        character,
                        style.text_emphasis_skip,
                    )
                });
                if receives_mark {
                    let mark_width = self.font_system.measure_text(mark, &emphasis_style);
                    let mark_x = if vertical {
                        let side_offset = if style.text_emphasis_position.right {
                            style.font_size * 0.55
                        } else {
                            -style.font_size * 0.55 - mark_width
                        };
                        line.x + run.x_offset + glyph.x_offset + side_offset
                    } else {
                        pen_x + glyph.x_offset + (glyph.x_advance - mark_width) / 2.0
                    };
                    let mark_y = if vertical {
                        line.y
                    } else if style.text_emphasis_position.over {
                        line.y + style.font_size * 0.55
                    } else {
                        line.y - style.font_size * 0.35
                    };
                    let _ = self.paint_text_runs(mark, mark_x, mark_y, &emphasis_style);
                }
                pen_x += glyph.x_advance;
            }
        }
    }

    /// Paint CSS text decoration lines for one rendered text line.
    ///
    /// CSS Text Decoration defines underline, overline, and line-through as
    /// decoration lines painted with the element's `text-decoration-color`,
    /// `text-decoration-style`, and `text-decoration-thickness`:
    /// <https://www.w3.org/TR/css-text-decor-3/#line-decoration>.
    pub(super) fn paint_text_decoration_lines(
        &mut self,
        x: f32,
        baseline_y: f32,
        width: f32,
        style: &ComputedStyle,
        runs: &[RenderedTextRun],
    ) {
        self.paint_text_decoration_lines_for_phase(
            x,
            baseline_y,
            width,
            style,
            runs,
            TextDecorationPaintPhase::All,
        );
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
        let (x, width) = match style.direction {
            Direction::Ltr => (x + inset_start, width - inset_start - inset_end),
            Direction::Rtl => (x + inset_end, width - inset_start - inset_end),
        };
        let width = width.max(0.0);
        if width <= 0.0 {
            return;
        }
        let font_id = self.font_system.resolve_style(style);
        let metrics = self.font_system.text_decoration_metrics(font_id, style);
        let ink_boxes = self.font_system.glyph_ink_boxes_for_runs(runs, baseline_y);
        let underline_thickness =
            used_text_decoration_thickness(decoration.thickness, style.font_size, &metrics, false);
        let strikeout_thickness =
            used_text_decoration_thickness(decoration.thickness, style.font_size, &metrics, true);
        if phase.paints_before_text()
            && decoration.underline
            && !text_decoration_skip_self_suppresses(style, TextDecorationLineKind::Underline)
        {
            let y = used_underline_y(
                baseline_y,
                decoration.underline_position,
                decoration.underline_offset,
                style.font_size,
                &metrics,
                underline_thickness,
            );
            self.paint_text_decoration_stroke(
                TextDecorationStroke {
                    x,
                    y,
                    width,
                    thickness: underline_thickness,
                    color,
                    style: decoration.style,
                    skip_ink: decoration.skip_ink,
                    skip_spaces: decoration.skip_spaces,
                },
                runs,
                &ink_boxes,
            );
        }
        if phase.paints_before_text()
            && decoration.overline
            && !text_decoration_skip_self_suppresses(style, TextDecorationLineKind::Overline)
        {
            self.paint_text_decoration_stroke(
                TextDecorationStroke {
                    x,
                    y: baseline_y + style.font_size,
                    width,
                    thickness: underline_thickness,
                    color,
                    style: decoration.style,
                    skip_ink: decoration.skip_ink,
                    skip_spaces: decoration.skip_spaces,
                },
                runs,
                &ink_boxes,
            );
        }
        if phase.paints_after_text()
            && decoration.line_through
            && !text_decoration_skip_self_suppresses(style, TextDecorationLineKind::LineThrough)
        {
            self.paint_text_decoration_stroke(
                TextDecorationStroke {
                    x,
                    y: baseline_y + metrics.strikeout_position,
                    width,
                    thickness: strikeout_thickness,
                    color,
                    style: decoration.style,
                    skip_ink: decoration.skip_ink,
                    skip_spaces: decoration.skip_spaces,
                },
                runs,
                &ink_boxes,
            );
        }
        if phase.paints_before_text() && decoration.spelling_error {
            let y = used_underline_y(
                baseline_y,
                decoration.underline_position,
                decoration.underline_offset,
                style.font_size,
                &metrics,
                underline_thickness,
            );
            self.paint_text_decoration_stroke(
                TextDecorationStroke {
                    x,
                    y,
                    width,
                    thickness: underline_thickness,
                    color: color_override.unwrap_or(Color::new(255, 0, 0)),
                    style: TextDecorationStyle::Wavy,
                    skip_ink: TextDecorationSkipInk::None,
                    skip_spaces: TextDecorationSkipSpaces::NONE,
                },
                runs,
                &ink_boxes,
            );
        }
        if phase.paints_before_text() && decoration.grammar_error {
            let y = used_underline_y(
                baseline_y,
                decoration.underline_position,
                decoration.underline_offset,
                style.font_size,
                &metrics,
                underline_thickness,
            );
            self.paint_text_decoration_stroke(
                TextDecorationStroke {
                    x,
                    y,
                    width,
                    thickness: underline_thickness,
                    color: color_override.unwrap_or(Color::new(0, 128, 0)),
                    style: TextDecorationStyle::Wavy,
                    skip_ink: TextDecorationSkipInk::None,
                    skip_spaces: TextDecorationSkipSpaces::NONE,
                },
                runs,
                &ink_boxes,
            );
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
        stroke: TextDecorationStroke,
        runs: &[RenderedTextRun],
        ink_boxes: &[GlyphInkBox],
    ) {
        let TextDecorationStroke {
            x,
            y,
            width,
            thickness,
            color,
            style,
            skip_ink,
            skip_spaces,
        } = stroke;
        let segments = text_decoration_segments(
            TextDecorationSegmentInputs {
                x,
                width,
                y,
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
                for (segment_x, segment_width) in segments {
                    self.push_text_decoration_rect(
                        segment_x,
                        y + stripe,
                        segment_width,
                        stripe,
                        color,
                    );
                    self.push_text_decoration_rect(
                        segment_x,
                        y - stripe,
                        segment_width,
                        stripe,
                        color,
                    );
                }
            }
            TextDecorationStyle::Dotted => {
                let dot = thickness.max(1.0);
                let step = dot * 2.0;
                for (segment_x, segment_width) in segments {
                    let mut cursor = segment_x;
                    while cursor < segment_x + segment_width {
                        self.push_text_decoration_rect(
                            cursor,
                            y,
                            dot.min(segment_x + segment_width - cursor),
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
                for (segment_x, segment_width) in segments {
                    let mut cursor = segment_x;
                    while cursor < segment_x + segment_width {
                        self.push_text_decoration_rect(
                            cursor,
                            y,
                            dash.min(segment_x + segment_width - cursor),
                            thickness,
                            color,
                        );
                        cursor += dash + gap;
                    }
                }
            }
            TextDecorationStyle::Wavy => {
                for (segment_x, segment_width) in segments {
                    self.push_text_decoration_wavy_path(
                        segment_x,
                        y,
                        segment_width,
                        thickness,
                        color,
                    );
                }
            }
            TextDecorationStyle::Solid | TextDecorationStyle::Double => {
                for (segment_x, segment_width) in segments {
                    self.push_text_decoration_rect(segment_x, y, segment_width, thickness, color);
                }
            }
        }
    }

    fn push_text_decoration_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        self.push_rect_in_band(
            PaintBand::Inline,
            RenderedRect {
                x,
                y,
                width,
                height,
                fill: Some(color),
                stroke: None,
                stroke_width: 0.0,
            },
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
        x: f32,
        y: f32,
        width: f32,
        thickness: f32,
        color: Color,
    ) {
        if width <= 0.0 || thickness <= 0.0 {
            return;
        }
        let amplitude = (thickness * 1.25).max(1.0);
        let half_wave = (amplitude * 2.0).max(2.0);
        let center_y = y + thickness / 2.0;
        let mut commands = vec![RenderedPathCommand::MoveTo(x, center_y)];
        let mut cursor = x;
        let mut crest = true;
        while cursor < x + width {
            let next = (cursor + half_wave).min(x + width);
            let control_x = (cursor + next) / 2.0;
            let control_y = if crest {
                center_y + amplitude
            } else {
                center_y - amplitude
            };
            commands.push(RenderedPathCommand::CurveTo {
                x1: control_x,
                y1: control_y,
                x2: control_x,
                y2: control_y,
                x3: next,
                y3: center_y,
            });
            cursor = next;
            crest = !crest;
        }
        self.push_path_in_band(
            PaintBand::Inline,
            RenderedPath {
                commands,
                fill: None,
                stroke: Some(color),
                stroke_width: thickness.max(0.5),
                fill_rule: RenderedPathFillRule::NonZero,
                clip: None,
            },
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
        let run_width = run
            .glyphs
            .as_ref()
            .map(|glyphs| glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>())
            .unwrap_or_else(|| run.text.chars().count() as f32 * run.font_size * 0.5);
        width.max(run.x_offset + run_width)
    })
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
    if shadow.blur_radius <= 0.0 {
        return vec![TextShadowPaintPass {
            x_offset: 0.0,
            y_offset: 0.0,
            color,
        }];
    }

    let radius = shadow.blur_radius.max(0.0);
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
    if skip.punctuation && character.is_ascii_punctuation() {
        return false;
    }
    if skip.symbols && character.is_ascii() && !character.is_ascii_alphanumeric() {
        return false;
    }
    if skip.narrow && character.is_ascii() {
        return false;
    }
    true
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

/// Adjust inline fragment background/border painting for sliced inline boxes.
///
/// CSS Fragmentation defines `box-decoration-break: slice` as the initial
/// behavior: inline-start decorations are painted only on the first fragment,
/// inline-end decorations only on the last fragment, while top/bottom
/// decorations continue on every line fragment:
/// <https://www.w3.org/TR/css-break-3/#break-decoration>.
fn apply_inline_fragment_edge_painting(
    style: &mut ComputedStyle,
    edges: InlineHangingEdges,
    x: &mut f32,
    width: &mut f32,
) {
    let borders = used_border_widths(style);
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
fn inline_fragment_text_summary(fragments: &[InlineFragment]) -> String {
    fragments
        .iter()
        .map(|fragment| fragment.text.as_str())
        .collect()
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
        TextDecorationThickness::LengthPercentage(value) => {
            (value.length + value.percent * font_size).max(0.5)
        }
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
    x: f32,
    width: f32,
    y: f32,
    thickness: f32,
    skip_ink: TextDecorationSkipInk,
    skip_spaces: TextDecorationSkipSpaces,
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
) -> Vec<(f32, f32)> {
    let TextDecorationSegmentInputs {
        x,
        width,
        y,
        thickness,
        skip_ink,
        skip_spaces,
    } = inputs;
    if width <= 0.0 {
        return Vec::new();
    }

    let stroke_min_y = y;
    let stroke_max_y = y + thickness;
    let padding = thickness.max(0.5);
    let mut skips = text_decoration_space_skip_ranges(x, width, skip_spaces, runs);
    if skip_ink != TextDecorationSkipInk::None {
        skips.extend(
            ink_boxes
                .iter()
                .filter(|ink| ink.y_min <= stroke_max_y && ink.y_max >= stroke_min_y)
                .filter_map(|ink| {
                    let start = (x + ink.x_min - padding).max(x);
                    let end = (x + ink.x_max + padding).min(x + width);
                    (end > start).then_some((start, end))
                }),
        );
    }
    if skips.is_empty() {
        return vec![(x, width)];
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
    let mut cursor = x;
    for (start, end) in merged {
        if start > cursor {
            segments.push((cursor, start - cursor));
        }
        cursor = cursor.max(end);
    }
    if cursor < x + width {
        segments.push((cursor, x + width - cursor));
    }
    segments
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
    x: f32,
    width: f32,
    skip_spaces: TextDecorationSkipSpaces,
    runs: &[RenderedTextRun],
) -> Vec<(f32, f32)> {
    if width <= 0.0 || skip_spaces == TextDecorationSkipSpaces::NONE {
        return Vec::new();
    }

    let glyphs = text_decoration_positioned_glyphs(x, width, runs);
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
            let start = (glyph.x_start - previous_extra_spacing).max(x);
            let end = glyph.x_end.min(x + width);
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
            let start = glyph.x_start.max(x);
            let end = glyph.x_end.min(x + width);
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
                (glyph.x_start - previous_extra_spacing).max(x),
                glyph.x_end.min(x + width),
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
    x_start: f32,
    x_end: f32,
    extra_spacing: f32,
}

/// Return shaped glyph advances positioned in line visual order.
///
/// CSS Text Decoration clips decorations in the coordinate space of the
/// rendered line, so this preserves run offsets and shaped glyph advances
/// instead of remeasuring flattened source text:
/// <https://www.w3.org/TR/css-text-decor-4/#painting>.
fn text_decoration_positioned_glyphs(
    x: f32,
    width: f32,
    runs: &[RenderedTextRun],
) -> Vec<TextDecorationPositionedGlyph> {
    let line_end = x + width;
    let mut positioned = Vec::new();
    for run in runs {
        let Some(glyphs) = &run.glyphs else {
            continue;
        };
        let mut pen_x = x + run.x_offset;
        for glyph in glyphs {
            let start = pen_x + glyph.x_offset;
            let end = (pen_x + glyph.x_advance).max(start);
            if end > x && start < line_end {
                positioned.push(TextDecorationPositionedGlyph {
                    unicode: glyph.unicode.clone(),
                    x_start: start.max(x),
                    x_end: end.min(line_end),
                    extra_spacing: (glyph.x_advance - glyph.nominal_x_advance).max(0.0),
                });
            }
            pen_x += glyph.x_advance;
        }
    }
    positioned
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
        TextUnderlineOffset::LengthPercentage(value) => value.length + value.percent * font_size,
    }
}
