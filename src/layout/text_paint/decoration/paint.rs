use super::super::positioning::rendered_text_line_width;
use super::*;
use crate::css::BoxDecorationBreak;
use crate::layout::text_paint::TextDecorationOriginFragmentGeometry;

/// Resolve decoration endpoint adjustments for one fragment of the
/// decorating box.
///
/// For `slice`, the percentage basis is the complete decorating-box chain and
/// positive adjustments continue into later fragments when they consume an
/// earlier fragment. Negative adjustments extend only the outer endpoint. For
/// `clone`, each fragment has its own basis and both edges are adjusted.
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-inset-property>
pub(in crate::layout) fn text_decoration_fragment_insets(
    decoration: &TextDecorationLayer,
    fragment: &TextDecorationOriginFragmentGeometry,
) -> (f32, f32) {
    debug_assert!(std::rc::Rc::ptr_eq(
        &decoration.origin_style,
        &fragment.origin_style
    ));
    let percentage_basis = match fragment.origin_style.box_decoration_break {
        BoxDecorationBreak::Slice => fragment.complete_inline_range.extent(),
        BoxDecorationBreak::Clone => fragment.fragment_inline_range.extent(),
    };
    let (start, end) = decoration
        .decoration
        .inset
        .clone()
        .used(percentage_basis, fragment.origin_style.font_size);
    match fragment.origin_style.box_decoration_break {
        BoxDecorationBreak::Clone => (start, end),
        BoxDecorationBreak::Slice => {
            let start = if start.is_sign_positive() {
                (start - fragment.fragment_inline_range.start().points()).max(0.0)
            } else if fragment.is_first_fragment {
                start
            } else {
                0.0
            };
            let end = if end.is_sign_positive() {
                (end - (fragment.complete_inline_range.end().points()
                    - fragment.fragment_inline_range.end().points()))
                .max(0.0)
            } else if fragment.is_last_fragment {
                end
            } else {
                0.0
            };
            (start, end)
        }
    }
}

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
        emphasis_style.text_decoration_origins.clear();
        emphasis_style.text_decoration = ComputedStyle::initial().text_decoration;
        emphasis_style.text_shadow.clear();
        emphasis_style.text_emphasis_style = TextEmphasisStyle::None;
        // Emphasis marks are independently shaped annotations. They inherit
        // the text's font selection but never the inter-character spacing
        // that belongs between the annotated source characters.
        // <https://drafts.csswg.org/css-text-decor-4/#emphasis-marks>
        emphasis_style.letter_spacing = crate::css::ComputedLengthPercentage::ZERO;
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
        self.paint_text_decoration_lines_for_phase_with_color_and_line_geometries(
            x,
            baseline_y,
            width,
            style,
            runs,
            phase,
            None,
            &[],
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
        color_override: Option<CssColor>,
    ) {
        self.paint_text_decoration_lines_for_phase_with_color_and_line_geometries(
            x,
            baseline_y,
            width,
            style,
            runs,
            phase,
            color_override,
            &[],
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_text_decoration_lines_for_phase_with_color_and_line_geometries(
        &mut self,
        x: f32,
        baseline_y: f32,
        width: f32,
        style: &ComputedStyle,
        runs: &[RenderedTextRun],
        phase: TextDecorationPaintPhase,
        color_override: Option<CssColor>,
        line_geometries: &[TextDecorationOriginLineGeometry],
    ) {
        let decorations = active_text_decoration_layers(style);
        if decorations.is_empty() || width <= 0.0 {
            return;
        }
        for decoration in &decorations {
            let line_geometry = line_geometries.iter().find(|geometry| {
                std::rc::Rc::ptr_eq(&geometry.layer.origin_style, &decoration.origin_style)
            });
            self.paint_text_decoration_layer(
                x,
                baseline_y,
                text_decoration_physical_inline_span(x, baseline_y, width, style),
                style,
                runs,
                decoration,
                phase,
                color_override,
                line_geometry,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn paint_text_decoration_layer(
        &mut self,
        x: f32,
        baseline_y: f32,
        inline_span: TextInlineSpan,
        style: &ComputedStyle,
        runs: &[RenderedTextRun],
        decoration: &TextDecorationLayer,
        phase: TextDecorationPaintPhase,
        color_override: Option<CssColor>,
        line_geometry: Option<&TextDecorationOriginLineGeometry>,
    ) {
        if !decoration.decoration.has_visible_line() || inline_span.length() <= 0.0 {
            return;
        }
        // A line decoration's declared values belong to its origin. The
        // current selected text contributes the considered-text metrics used
        // by this per-line geometry adapter; line-level aggregation extends
        // the same representation across adjacent prepared groups.
        //
        // CSS Text Decoration Level 3 § 2, Line Decoration.
        // <https://www.w3.org/TR/css-text-decor-3/#line-decoration>
        let origin_style = decoration.origin_style.as_ref();
        let color = color_override.unwrap_or_else(|| {
            decoration
                .decoration
                .color
                .resolve(decoration.origin_style.color)
        });
        let (inset_start, inset_end) = line_geometry
            .map(|geometry| text_decoration_fragment_insets(decoration, &geometry.origin_fragment))
            .unwrap_or_else(|| {
                decoration
                    .decoration
                    .inset
                    .clone()
                    .used(layout_pt(inline_span.length()), origin_style.font_size)
            });
        let (baseline, geometry) = if let Some(line_geometry) = line_geometry {
            // `inline_span` is page-relative, while glyph runs and their ink
            // boxes are positioned from the prepared line reference. Retain
            // both coordinates rather than moving the reference to the
            // selected decoration endpoint.
            (line_geometry.line_reference, line_geometry.geometry)
        } else {
            let considered_font_id = self.font_system.resolve_style(style);
            let considered_metrics = self
                .font_system
                .text_decoration_metrics(considered_font_id, style);
            (
                PaintPoint::new(x, baseline_y),
                TextDecorationLineGeometry::from_origin_and_considered_text(
                    origin_style,
                    style,
                    considered_metrics,
                ),
            )
        };
        let ink_boxes = self.font_system.glyph_ink_boxes_for_runs(runs, baseline.y);
        let selected_glyphs =
            line_geometry.map(|geometry| geometry.glyph_sequence.glyphs.as_slice());
        let positioned_ink_boxes =
            line_geometry.map(|geometry| geometry.positioned_ink_boxes.as_slice());
        let receiver_spans = line_geometry.map(|geometry| geometry.receiver_spans.as_slice());
        for stroke in prepare_text_decoration_strokes(TextDecorationPreparationInput {
            baseline,
            inline_span,
            inset_start,
            inset_end,
            style,
            inset_style: origin_style,
            inset_inline_axis: line_geometry.map_or_else(
                || TextDecorationInlineAxis::for_style(origin_style),
                |geometry| geometry.origin_inline_axis,
            ),
            decoration: decoration.decoration.clone(),
            phase,
            color,
            color_override,
            geometry,
        }) {
            self.paint_text_decoration_stroke_with_selected_glyphs(
                stroke,
                runs,
                &ink_boxes,
                selected_glyphs,
                positioned_ink_boxes,
                receiver_spans,
            );
        }
    }

    /// Paint one CSS text decoration stroke.
    ///
    /// CSS Text Decoration defines solid, double, dotted, dashed, and wavy
    /// decoration styles; PDF paths/strokes are the backend representation for
    /// non-rectangular strokes:
    /// <https://www.w3.org/TR/css-text-decor-3/#text-decoration-style-property>.
    #[allow(dead_code)]
    pub(in crate::layout) fn paint_text_decoration_stroke(
        &mut self,
        stroke: PreparedTextDecorationStroke,
        runs: &[RenderedTextRun],
        ink_boxes: &[GlyphInkBox],
    ) {
        self.paint_text_decoration_stroke_with_selected_glyphs(
            stroke, runs, ink_boxes, None, None, None,
        );
    }

    fn paint_text_decoration_stroke_with_selected_glyphs(
        &mut self,
        stroke: PreparedTextDecorationStroke,
        runs: &[RenderedTextRun],
        ink_boxes: &[GlyphInkBox],
        selected_glyphs: Option<&[TextDecorationPositionedGlyph]>,
        positioned_ink_boxes: Option<&[TextDecorationPositionedInkBox]>,
        receiver_spans: Option<&[TextInlineSpan]>,
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
        let segments = text_decoration_segments_with_selected_glyphs(
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
            selected_glyphs,
            positioned_ink_boxes,
            receiver_spans,
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
        color: CssColor,
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

    pub(in crate::layout) fn push_text_decoration_rect(
        &mut self,
        rect: PaintRect,
        color: CssColor,
    ) {
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
        color: CssColor,
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
                PaintStrokeWidth::new(thickness.max(0.5)),
                None,
            ),
        );
    }
}

/// Convert the legacy text-paint `(origin, width)` boundary into an ordered
/// physical inline span. Prepared vertical decoration receivers bypass this
/// helper because they retain their logical inline-start provenance.
fn text_decoration_physical_inline_span(
    x: f32,
    baseline_y: f32,
    width: f32,
    style: &ComputedStyle,
) -> TextInlineSpan {
    let local_span = TextInlineSpan::from_start_and_length(0.0, width);
    VerticalInlineAxis::for_style(style)
        .map(|axis| axis.project_span_from_start(layout_pt(baseline_y), local_span))
        .unwrap_or_else(|| TextInlineSpan::from_start_and_length(x, width))
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::layout::text_paint::TextDecorationLogicalInlineRange;

    fn layer_and_geometry(
        break_mode: BoxDecorationBreak,
        start: css::ComputedLengthPercentage,
        end: css::ComputedLengthPercentage,
        fragment_start: f32,
        fragment_end: f32,
        total_end: f32,
    ) -> (TextDecorationLayer, TextDecorationOriginFragmentGeometry) {
        let mut style = ComputedStyle::initial();
        style.box_decoration_break = break_mode;
        style.text_decoration.underline = true;
        style.text_decoration.inset = css::TextDecorationInset::Lengths { start, end };
        style.rebuild_own_text_decoration_origin();
        let layer = style
            .text_decoration_origins
            .effective_layers_vec()
            .into_iter()
            .next()
            .expect("visible line creates own origin");
        let geometry = TextDecorationOriginFragmentGeometry {
            origin_style: Rc::clone(&layer.origin_style),
            complete_inline_range: TextDecorationLogicalInlineRange::from_edges(
                layout_pt(0.0),
                layout_pt(total_end),
            ),
            fragment_inline_range: TextDecorationLogicalInlineRange::from_edges(
                layout_pt(fragment_start),
                layout_pt(fragment_end),
            ),
            is_first_fragment: fragment_start == 0.0,
            is_last_fragment: fragment_end == total_end,
        };
        (layer, geometry)
    }

    fn geometry_for_layer(
        layer: &TextDecorationLayer,
        fragment_start: f32,
        fragment_end: f32,
        total_end: f32,
    ) -> TextDecorationOriginFragmentGeometry {
        TextDecorationOriginFragmentGeometry {
            origin_style: Rc::clone(&layer.origin_style),
            complete_inline_range: TextDecorationLogicalInlineRange::from_edges(
                layout_pt(0.0),
                layout_pt(total_end),
            ),
            fragment_inline_range: TextDecorationLogicalInlineRange::from_edges(
                layout_pt(fragment_start),
                layout_pt(fragment_end),
            ),
            is_first_fragment: fragment_start == 0.0,
            is_last_fragment: fragment_end == total_end,
        }
    }

    #[test]
    fn slice_carries_positive_start_inset_across_three_fragments() {
        let (layer, first) = layer_and_geometry(
            BoxDecorationBreak::Slice,
            css::ComputedLengthPercentage::from_points(25.0),
            css::ComputedLengthPercentage::ZERO,
            0.0,
            10.0,
            30.0,
        );
        let middle = geometry_for_layer(&layer, 10.0, 20.0, 30.0);
        let last = geometry_for_layer(&layer, 20.0, 30.0, 30.0);

        assert_eq!(text_decoration_fragment_insets(&layer, &first), (25.0, 0.0));
        assert_eq!(
            text_decoration_fragment_insets(&layer, &middle),
            (15.0, 0.0)
        );
        assert_eq!(text_decoration_fragment_insets(&layer, &last), (5.0, 0.0));
    }

    #[test]
    fn clone_resolves_percentages_against_each_fragment() {
        let (layer, fragment) = layer_and_geometry(
            BoxDecorationBreak::Clone,
            css::ComputedLengthPercentage::from_percent(0.25),
            css::ComputedLengthPercentage::from_percent(-0.10),
            10.0,
            30.0,
            60.0,
        );

        assert_eq!(
            text_decoration_fragment_insets(&layer, &fragment),
            (5.0, -2.0)
        );
    }

    #[test]
    fn slice_negative_end_belongs_only_to_last_fragment() {
        let (layer, middle) = layer_and_geometry(
            BoxDecorationBreak::Slice,
            css::ComputedLengthPercentage::ZERO,
            css::ComputedLengthPercentage::from_points(-4.0),
            10.0,
            20.0,
            30.0,
        );
        let last = geometry_for_layer(&layer, 20.0, 30.0, 30.0);

        assert_eq!(text_decoration_fragment_insets(&layer, &middle), (0.0, 0.0));
        assert_eq!(text_decoration_fragment_insets(&layer, &last), (0.0, -4.0));
    }
}
