use super::super::positioning::rendered_text_line_width;
use super::*;

impl<'a> LayoutBuilder<'a> {
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
            for pass in text_shadow_paint_passes(shadow.clone(), color) {
                let mut shadow_line = line.clone();
                let offset = PaintDisplacement::new(
                    shadow.offset_x.length_points(),
                    -shadow.offset_y.length_points(),
                ) + pass.offset;
                shadow_line.translate_origin(PaintTranslation::new(offset.x, offset.y));
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
            let _ = self.paint_text_runs(&mark.mark, mark.position, &emphasis_style);
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
        let (inset_start, inset_end) = decoration.inset.clone().used(style.font_size);
        let font_id = self.font_system.resolve_style(style);
        let metrics = self.font_system.text_decoration_metrics(font_id, style);
        let ink_boxes = self.font_system.glyph_ink_boxes_for_runs(runs, baseline_y);
        for stroke in prepare_text_decoration_strokes(TextDecorationPreparationInput {
            baseline: PaintPoint::new(x, baseline_y),
            inline_span: TextInlineSpan::from_start_and_length(x, width),
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
            baseline,
            inline_span,
            block_position,
            thickness,
            color,
            style,
            skip_ink,
            skip_spaces,
        } = stroke;
        let line_x = baseline.x;
        let line_y = baseline.y;
        let inline_start = inline_span.start;
        let inline_length = inline_span.length();
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
        let rect = match axis {
            TextDecorationStrokeAxis::Horizontal => {
                paint_space_rect(inline_start, block_position, inline_length, thickness)
            }
            TextDecorationStrokeAxis::Vertical => {
                paint_space_rect(block_position, inline_start, thickness, inline_length)
            }
        };
        self.push_text_decoration_rect(rect, color);
    }

    pub(in crate::layout) fn push_text_decoration_rect(&mut self, rect: PaintRect, color: Color) {
        self.push_rect_in_band(
            PaintBand::Inline,
            RenderedRect::from_paint_rect(rect, Some(color)),
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
