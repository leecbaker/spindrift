use super::font_registry::FontSupportKind;
use super::*;
use crate::css::{
    BaselineShift, ComputedLengthPercentage, FontSizeAdjust, FontSizeAdjustMetric,
    FontSizeAdjustValue,
};
use crate::{RenderedLine, RenderedTextRun};

#[derive(Debug, Clone)]
struct FontSizeAdjustmentRange {
    range: Range<usize>,
    font_size: f32,
}

#[derive(Debug, Clone, Copy)]
struct RenderedRunTabContext<'a> {
    style: &'a ComputedStyle,
}

impl FontSystem {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::from_seed(Self::sync_seed())
    }

    pub(crate) fn into_fonts(self) -> Vec<DocumentFont> {
        self.document_fonts.into_fonts()
    }

    fn font_feature_context_for_style(&self, style: &ComputedStyle) -> Option<FontFeatureContext> {
        let family = font_feature_family(&style.font_family);
        let face_defaults = family
            .as_ref()
            .and_then(|family| {
                self.font_feature_defaults_by_family
                    .get(&family.trim().to_ascii_lowercase())
            })
            .cloned();
        if face_defaults.is_none() && self.font_feature_values.values.is_empty() {
            return None;
        }
        Some(FontFeatureContext {
            family,
            face_defaults,
            font_feature_values: self.font_feature_values.clone(),
        })
    }

    pub(crate) fn resolve_style(&mut self, style: &ComputedStyle) -> Option<usize> {
        if let Some(id) = self.resolve_font_family(
            &style.font_family,
            style.font_weight,
            style.font_style,
            style.font_width,
        ) {
            return Some(id);
        }
        self.resolve_generic_family(
            &FontFamily::SansSerif,
            style.font_weight,
            style.font_style,
            style.font_width,
        )
    }

    fn font_size_adjust_target_ratio(
        &mut self,
        style: &ComputedStyle,
    ) -> Option<(FontSizeAdjustMetric, f32)> {
        let FontSizeAdjust::Value { metric, value } = style.font_size_adjust else {
            return None;
        };
        let ratio = match value {
            FontSizeAdjustValue::Number(value) => value,
            FontSizeAdjustValue::FromFont => {
                let font_id = self.resolve_style(style)?;
                let font = self.document_fonts.get(font_id)?;
                font_size_adjust_metric_ratio(font, metric)?
            }
        };
        ratio.is_finite().then_some((metric, ratio))
    }

    fn adjusted_font_size_for_font(
        &mut self,
        style: &ComputedStyle,
        font_id: usize,
    ) -> Option<f32> {
        let (metric, target_ratio) = self.font_size_adjust_target_ratio(style)?;
        let font = self.document_fonts.get(font_id)?;
        let selected_ratio = font_size_adjust_metric_ratio(font, metric)?;
        if selected_ratio <= 0.0 {
            return None;
        }
        let adjusted = style.font_size * target_ratio / selected_ratio;
        adjusted.is_finite().then_some(adjusted)
    }

    fn font_size_adjusted_size_for_font_id(
        &self,
        style: &ComputedStyle,
        font_id: usize,
    ) -> Option<f32> {
        let FontSizeAdjust::Value { metric, value } = style.font_size_adjust else {
            return None;
        };
        let target_ratio = match value {
            FontSizeAdjustValue::Number(value) => value,
            FontSizeAdjustValue::FromFont => {
                let font = self.document_fonts.get(font_id)?;
                font_size_adjust_metric_ratio(font, metric)?
            }
        };
        let font = self.document_fonts.get(font_id)?;
        let selected_ratio = font_size_adjust_metric_ratio(font, metric)?;
        if selected_ratio <= 0.0 {
            return None;
        }
        let adjusted = style.font_size * target_ratio / selected_ratio;
        adjusted.is_finite().then_some(adjusted)
    }

    pub(crate) fn measure_text(&mut self, text: &str, style: &ComputedStyle) -> f32 {
        self.shape_unwrapped_line(text, style, style.line_height)
            .map(|line| line.advance_width())
            .unwrap_or(0.0)
    }

    /// Measure a line excluding CSS Text inline-end hanging advances.
    ///
    /// CSS Text excludes trailing "other space separators" from line measure
    /// in collapsing white-space modes, and excludes `letter-spacing` at the
    /// start and end of a line while preserving painted text:
    /// <https://www.w3.org/TR/css-text-3/#white-space-phase-2> and
    /// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
    #[cfg(test)]
    pub(crate) fn measure_line_text(&mut self, text: &str, style: &ComputedStyle) -> f32 {
        self.shape_unwrapped_line(text, style, style.line_height)
            .map(|line| shaped_line_measure_width(&line, style))
            .unwrap_or(0.0)
    }

    /// Return visual text ranges for one unwrapped bidi paragraph.
    ///
    /// CSS Writing Modes delegates inline bidirectional reordering to the
    /// Unicode Bidirectional Algorithm. Parley exposes visual cluster order
    /// after applying UAX #9, including formatting controls inserted for CSS
    /// `unicode-bidi`:
    /// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi> and
    /// <https://www.unicode.org/reports/tr9/>.
    pub(crate) fn visual_ranges_for_unwrapped_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
    ) -> Vec<Range<usize>> {
        if text.is_empty() {
            return Vec::new();
        }
        let emoji_text = text_with_font_variant_emoji(text, style);
        let bidi_text = text_with_css_bidi_controls(emoji_text.as_ref(), style);
        let shaped_text = bidi_text.as_str();
        let feature_context = self.font_feature_context_for_style(style);
        let mut builder = self.parley_layout_context.ranged_builder(
            &mut self.parley_font_context,
            shaped_text,
            1.0,
            false,
        );
        push_parley_default_style(&mut builder, style);
        push_parley_text_spacing_default_with_context(
            &mut builder,
            shaped_text,
            style,
            feature_context.as_ref(),
        );
        let mut layout = builder.build(shaped_text);
        layout.break_all_lines(None);
        layout
            .lines()
            .next()
            .map(|line| {
                let mut ranges = visual_ranges_for_line(line)
                    .into_iter()
                    .filter_map(|range| bidi_text.original_range(range))
                    .collect::<Vec<_>>();
                merge_join_control_visual_ranges(text, &mut ranges);
                ranges
            })
            .filter(|ranges| !ranges.is_empty())
            .unwrap_or_else(|| std::iter::once(0..text.len()).collect())
    }

    /// Returns the used CSS `ch` advance for a style's selected font.
    ///
    /// CSS Values defines `1ch` as the used advance of the "0" glyph in the
    /// element's font, falling back to 0.5em when measuring that glyph is not
    /// possible:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn ch_advance(&mut self, style: &ComputedStyle) -> f32 {
        let mut metric_style = style.clone();
        metric_style.letter_spacing = ComputedLengthPercentage::ZERO;
        let advance = self.measure_text("0", &metric_style);
        if advance > 0.0 {
            advance
        } else {
            style.font_size * 0.5
        }
    }

    pub(crate) fn used_line_height(&mut self, style: &ComputedStyle) -> f32 {
        if !style.line_height_is_normal {
            return style.line_height;
        }
        let font_id = self.resolve_style(style);
        self.line_height_for_font(font_id, style)
    }

    pub(crate) fn line_height_for_font(
        &self,
        font_id: Option<usize>,
        style: &ComputedStyle,
    ) -> f32 {
        if !style.line_height_is_normal {
            return style.line_height;
        }
        let Some(font) = font_id.and_then(|id| self.document_fonts.get(id)) else {
            return style.line_height;
        };
        let font_height = (font.ascender as f32 - font.descender as f32).max(0.0) * style.font_size
            / font.units_per_em.max(1) as f32;
        font_height.max(style.font_size)
    }

    /// Resolve font metrics used by CSS text-decoration painting.
    ///
    /// CSS Text Decoration uses underline and strikeout metrics for
    /// font-derived line placement and `from-font` thickness; OpenType stores
    /// these in the `post` and `OS/2` tables exposed by `ttf-parser`.
    /// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-width-property>
    /// and
    /// <https://www.w3.org/TR/css-text-decor-3/#text-underline-position-property>.
    pub(crate) fn text_decoration_metrics(
        &self,
        font_id: Option<usize>,
        style: &ComputedStyle,
    ) -> TextDecorationFontMetrics {
        let fallback = TextDecorationFontMetrics {
            underline_position: -style.font_size / 9.0,
            underline_thickness: (style.font_size / 16.0).max(0.5),
            strikeout_position: style.font_size * 0.3,
            strikeout_thickness: (style.font_size / 16.0).max(0.5),
            descender_depth: style.font_size * 0.2,
        };
        let Some(font) = font_id.and_then(|id| self.document_fonts.get(id)) else {
            return fallback;
        };
        let scale = style.font_size / font.units_per_em.max(1) as f32;
        let mut metrics = TextDecorationFontMetrics {
            underline_position: fallback.underline_position,
            underline_thickness: fallback.underline_thickness,
            strikeout_position: fallback.strikeout_position,
            strikeout_thickness: fallback.strikeout_thickness,
            descender_depth: (-font.descender as f32 * scale).max(0.0),
        };

        if let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) {
            if let Some(underline) = face.underline_metrics() {
                metrics.underline_position = underline.position as f32 * scale;
                metrics.underline_thickness = (underline.thickness as f32 * scale).abs().max(0.5);
            }
            if let Some(strikeout) = face.strikeout_metrics() {
                metrics.strikeout_position = strikeout.position as f32 * scale;
                metrics.strikeout_thickness = (strikeout.thickness as f32 * scale).abs().max(0.5);
            }
        }

        metrics
    }

    /// Compute glyph ink boxes for CSS text-decoration skip-ink.
    ///
    /// CSS Text Decoration Level 4 defines skip-ink in terms of decoration
    /// strokes avoiding glyph ink. This helper maps shaped PDF glyph runs back
    /// to OpenType glyph bounding boxes in CSS layout units:
    /// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-skip-ink-property>.
    pub(crate) fn glyph_ink_boxes_for_runs(
        &self,
        runs: &[RenderedTextRun],
        baseline_y: f32,
    ) -> Vec<GlyphInkBox> {
        let mut boxes = Vec::new();
        for run in runs {
            let Some(font_id) = run.font_id else {
                continue;
            };
            let Some(font) = self.document_fonts.get(font_id) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
                continue;
            };
            let Some(glyphs) = &run.glyphs else {
                continue;
            };
            let scale = run.font_size / font.units_per_em.max(1) as f32;
            let mut pen_x = 0.0;
            for glyph in glyphs {
                if let Some(bbox) = face.glyph_bounding_box(ttf_parser::GlyphId(glyph.id)) {
                    let origin_x = pen_x + glyph.x_offset;
                    let origin_y = glyph.y_offset;
                    let corners = [
                        (
                            origin_x + bbox.x_min as f32 * scale,
                            origin_y + bbox.y_min as f32 * scale,
                        ),
                        (
                            origin_x + bbox.x_min as f32 * scale,
                            origin_y + bbox.y_max as f32 * scale,
                        ),
                        (
                            origin_x + bbox.x_max as f32 * scale,
                            origin_y + bbox.y_min as f32 * scale,
                        ),
                        (
                            origin_x + bbox.x_max as f32 * scale,
                            origin_y + bbox.y_max as f32 * scale,
                        ),
                    ];
                    let mut x_min = f32::INFINITY;
                    let mut x_max = f32::NEG_INFINITY;
                    let mut y_min = f32::INFINITY;
                    let mut y_max = f32::NEG_INFINITY;
                    for (x, y) in corners {
                        let transformed_x =
                            run.x_offset + run.text_matrix.a * x + run.text_matrix.c * y;
                        let transformed_y = baseline_y
                            + run.y_offset
                            + run.text_matrix.b * x
                            + run.text_matrix.d * y;
                        x_min = x_min.min(transformed_x);
                        x_max = x_max.max(transformed_x);
                        y_min = y_min.min(transformed_y);
                        y_max = y_max.max(transformed_y);
                    }
                    boxes.push(GlyphInkBox {
                        x_min,
                        x_max,
                        y_min,
                        y_max,
                    });
                }
                pen_x += glyph.x_advance;
            }
        }
        boxes
    }

    pub(crate) fn shape_text_runs_with_parley(
        &mut self,
        text: &str,
        style: &ComputedStyle,
    ) -> Vec<RenderedTextRun> {
        let emoji_text = text_with_font_variant_emoji(text, style);
        let text = text_without_font_neutral_default_ignorables(emoji_text.as_ref());
        let text = text.as_ref();
        if text.is_empty() {
            return Vec::new();
        }
        if let Some(resolved_spans) = self.unicode_range_resolved_text_spans(text, style) {
            let spans = resolved_spans
                .iter()
                .filter_map(|span| {
                    text.get(span.range.clone()).map(|text| StyledTextSpan {
                        text,
                        style: &span.style,
                    })
                })
                .collect::<Vec<_>>();
            if !spans.is_empty() {
                return self.shape_styled_text_runs_with_parley(&spans);
            }
        }
        if text_needs_edge_join_context(text) {
            return self.shape_styled_text_runs_with_parley(&[StyledTextSpan { text, style }]);
        }
        let feature_context = self.font_feature_context_for_style(style);
        let mut builder = self.parley_layout_context.ranged_builder(
            &mut self.parley_font_context,
            text,
            1.0,
            false,
        );
        push_parley_default_style(&mut builder, style);
        push_parley_text_spacing_default_with_context(
            &mut builder,
            text,
            style,
            feature_context.as_ref(),
        );
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        let Some(line) = layout.lines().next() else {
            return Vec::new();
        };
        let adjustment_ranges = self.font_size_adjustment_ranges_for_line(&line, style);
        if !adjustment_ranges.is_empty() {
            let mut builder = self.parley_layout_context.ranged_builder(
                &mut self.parley_font_context,
                text,
                1.0,
                false,
            );
            push_parley_default_style(&mut builder, style);
            push_parley_text_spacing_default_with_context(
                &mut builder,
                text,
                style,
                feature_context.as_ref(),
            );
            for adjustment in &adjustment_ranges {
                builder.push(
                    StyleProperty::FontSize(adjustment.font_size),
                    adjustment.range.clone(),
                );
            }
            let mut layout = builder.build(text);
            layout.break_all_lines(None);
            let Some(line) = layout.lines().next() else {
                return Vec::new();
            };
            return self.rendered_text_runs_for_parley_line(text, line, style);
        }
        self.rendered_text_runs_for_parley_line(text, line, style)
    }

    fn font_size_adjustment_ranges_for_line<B: parley::style::Brush>(
        &mut self,
        line: &parley::Line<'_, B>,
        style: &ComputedStyle,
    ) -> Vec<FontSizeAdjustmentRange> {
        if matches!(style.font_size_adjust, FontSizeAdjust::None) {
            return Vec::new();
        }
        let mut ranges = Vec::new();
        for run in line.runs() {
            let Some(font_id) =
                self.document_font_from_parley_font_data_for_style(run.font(), style)
            else {
                continue;
            };
            let Some(font_size) = self.adjusted_font_size_for_font(style, font_id) else {
                continue;
            };
            if (font_size - style.font_size).abs() > 0.01 {
                ranges.push(FontSizeAdjustmentRange {
                    range: run.text_range(),
                    font_size,
                });
            }
        }
        ranges
    }

    fn styled_font_size_adjustment_ranges_for_line<B: parley::style::Brush>(
        &mut self,
        line: &parley::Line<'_, B>,
        ranges: &[(Range<usize>, &ComputedStyle)],
        default_style: &ComputedStyle,
    ) -> Vec<FontSizeAdjustmentRange> {
        let mut adjustments = Vec::new();
        for run in line.runs() {
            let run_range = run.text_range();
            let run_style =
                style_for_text_range(ranges, run_range.clone()).unwrap_or(default_style);
            if matches!(run_style.font_size_adjust, FontSizeAdjust::None) {
                continue;
            }
            let Some(font_id) =
                self.document_font_from_parley_font_data_for_style(run.font(), run_style)
            else {
                continue;
            };
            let Some(font_size) = self.adjusted_font_size_for_font(run_style, font_id) else {
                continue;
            };
            if (font_size - run_style.font_size).abs() > 0.01 {
                adjustments.push(FontSizeAdjustmentRange {
                    range: run_range,
                    font_size,
                });
            }
        }
        adjustments
    }

    pub(super) fn rendered_text_runs_for_parley_line<B: parley::style::Brush>(
        &mut self,
        text: &str,
        line: parley::Line<'_, B>,
        style: &ComputedStyle,
    ) -> Vec<RenderedTextRun> {
        let mut rendered_runs = Vec::new();
        let mut tab_contexts = Vec::new();
        for run in line.runs() {
            let run_text = text
                .get(run.text_range())
                .map(text_without_variation_selectors)
                .unwrap_or_default();
            let x_offset = run
                .visual_clusters()
                .next()
                .and_then(|cluster| cluster.visual_offset())
                .unwrap_or(0.0);
            let Some(font_id) =
                self.document_font_from_parley_font_data_for_style(run.font(), style)
            else {
                continue;
            };
            if self.document_fonts.support_kind_for_run(font_id, &run_text)
                == FontSupportKind::ColorOrEmojiOnlyFallback
                && let Some(fallback_font_id) =
                    self.visible_text_fallback_for_run(&run_text, style, font_id)
                && let Some(fallback_font) = self.document_fonts.get(fallback_font_id)
                && let Some(glyphs) = shape_text_with_document_font(
                    fallback_font,
                    &run_text,
                    run.font_size(),
                    style.used_letter_spacing(),
                    style.used_word_spacing(),
                )
                && !glyphs.is_empty()
            {
                rendered_runs.push(RenderedTextRun {
                    text: run_text.to_string(),
                    x_offset,
                    y_offset: 0.0,
                    text_matrix: crate::RenderedTextMatrix::IDENTITY,
                    font_size: run.font_size(),
                    font_id: Some(fallback_font_id),
                    glyphs: Some(glyphs),
                });
                tab_contexts.push(RenderedRunTabContext { style });
                continue;
            }
            let Some(font) = self.document_fonts.get(font_id) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
                continue;
            };
            let units_per_em = font.units_per_em.max(1) as f32;
            let scale = run.font_size() / units_per_em;
            let mut glyphs = Vec::new();
            for cluster in run.visual_clusters() {
                let cluster_text = text.get(cluster.text_range()).unwrap_or_default();
                let emitted_cluster_text = text_without_glyph_output_controls(cluster_text);
                let cluster_glyphs = cluster.glyphs().collect::<Vec<_>>();
                let default_ignorable_only =
                    cluster_is_default_ignorable_only(cluster_text, &emitted_cluster_text);
                if default_ignorable_only
                    && !default_ignorable_cluster_has_shaping_glyph(
                        &face,
                        &run_text,
                        &emitted_cluster_text,
                        cluster_glyphs
                            .iter()
                            .filter_map(|glyph| {
                                u16::try_from(glyph.id)
                                    .ok()
                                    .map(|glyph_id| (glyph_id, glyph.advance))
                            })
                            .collect::<Vec<_>>()
                            .as_slice(),
                    )
                {
                    continue;
                }
                if emitted_cluster_text == "\t" {
                    glyphs.push(synthesized_tab_glyph(&face, scale));
                    continue;
                }
                let mut first_cluster_glyph = true;
                for glyph in cluster_glyphs {
                    let Ok(glyph_id) = u16::try_from(glyph.id) else {
                        continue;
                    };
                    let unicode = if first_cluster_glyph {
                        if default_ignorable_only {
                            String::new()
                        } else {
                            emitted_cluster_text.clone()
                        }
                    } else {
                        String::new()
                    };
                    let emitted_glyph_id = unicode
                        .chars()
                        .next()
                        .filter(|_| unicode.chars().count() == 1)
                        .and_then(|character| css_space_separator_blank_glyph(&face, character))
                        .map(|glyph| glyph.0)
                        .unwrap_or(glyph_id);
                    first_cluster_glyph = false;
                    glyphs.push(RenderedGlyph {
                        id: emitted_glyph_id,
                        x_advance: glyph.advance,
                        nominal_x_advance: face
                            .glyph_hor_advance(ttf_parser::GlyphId(emitted_glyph_id))
                            .map(|advance| advance as f32 * scale)
                            .unwrap_or(glyph.advance),
                        x_offset: glyph.x,
                        y_offset: -glyph.y,
                        unicode,
                    });
                }
            }
            if glyphs.is_empty() {
                continue;
            }
            let mut font_size = run.font_size();
            apply_synthetic_position_fallback(&mut glyphs, &mut font_size, style, &face, &run_text);
            rendered_runs.push(RenderedTextRun {
                text: run_text.to_string(),
                x_offset,
                y_offset: 0.0,
                text_matrix: crate::RenderedTextMatrix::IDENTITY,
                font_size,
                font_id: Some(font_id),
                glyphs: Some(glyphs),
            });
            tab_contexts.push(RenderedRunTabContext { style });
        }
        self.apply_css_tab_stops(&mut rendered_runs, &tab_contexts);
        rendered_runs
    }

    /// Split a named font stack into range-limited shaping spans.
    ///
    /// CSS Fonts applies `@font-face unicode-range` during font matching, while
    /// CSS Text still requires one shaping context across join controls and
    /// cursive-script neighbors. Parley/fontique does not expose the descriptor
    /// on registration, so Quire resolves range-limited named families before
    /// shaping and passes the result back to Parley as styled ranges:
    /// <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>,
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order>, and
    /// <https://www.w3.org/TR/alreq/#h_joining_enforcement>.
    fn unicode_range_resolved_text_spans(
        &mut self,
        text: &str,
        style: &ComputedStyle,
    ) -> Option<Vec<UnicodeRangeResolvedSpan>> {
        let FontFamily::Names(names) = &style.font_family else {
            return None;
        };
        let names = names.clone();
        if names.is_empty()
            || !names.iter().any(|name| {
                self.resolve_single_family(
                    name,
                    style.font_weight,
                    style.font_style,
                    style.font_width,
                )
                .is_some_and(|font_id| self.document_fonts.font_has_unicode_range(font_id))
            })
        {
            return None;
        }

        let mut selections = Vec::<(Range<usize>, Option<String>)>::new();
        let mut previous_family = None::<String>;
        for (start, character) in text.char_indices() {
            let end = start + character.len_utf8();
            let family = if character_is_join_control(character) {
                previous_family
                    .clone()
                    .or_else(|| self.next_unicode_range_family_for_text(text, end, style, &names))
            } else {
                self.unicode_range_family_for_character(character, style, &names)
            };
            if let Some(family) = &family {
                previous_family = Some(family.clone());
            }
            selections.push((start..end, family));
        }
        if selections.is_empty() || selections.iter().all(|(_, family)| family.is_none()) {
            return None;
        }

        let mut spans = Vec::<UnicodeRangeResolvedSpan>::new();
        for (range, family) in selections {
            let mut span_style = style.clone();
            if let Some(family) = family {
                span_style.font_family = FontFamily::Names(vec![family]);
            }
            if let Some(previous) = spans.last_mut()
                && previous.style == span_style
                && previous.range.end == range.start
            {
                previous.range.end = range.end;
                continue;
            }
            spans.push(UnicodeRangeResolvedSpan {
                range,
                style: span_style,
            });
        }

        spans
            .iter()
            .any(|span| span.style.font_family != style.font_family)
            .then_some(spans)
    }

    fn next_unicode_range_family_for_text(
        &mut self,
        text: &str,
        start: usize,
        style: &ComputedStyle,
        names: &[String],
    ) -> Option<String> {
        text.get(start..)?.chars().find_map(|character| {
            (!character_is_join_control(character))
                .then(|| self.unicode_range_family_for_character(character, style, names))
                .flatten()
        })
    }

    fn unicode_range_family_for_character(
        &mut self,
        character: char,
        style: &ComputedStyle,
        names: &[String],
    ) -> Option<String> {
        for name in names {
            let Some(font_id) = self.resolve_single_family(
                name,
                style.font_weight,
                style.font_style,
                style.font_width,
            ) else {
                continue;
            };
            if self.document_fonts.font_has_character(font_id, character) {
                return Some(name.clone());
            }
        }
        None
    }

    #[allow(dead_code)]
    #[cfg(test)]
    pub(super) fn shaped_measured_line_from_parley_line<B: parley::style::Brush>(
        &mut self,
        text: &str,
        line_text: &str,
        line: parley::Line<'_, B>,
        style: &ComputedStyle,
        line_height: f32,
    ) -> Option<ShapedInlineLine> {
        let runs = position_shaped_runs(self.rendered_text_runs_for_parley_line(text, line, style));
        let baseline_adjustment = self.shaped_runs_baseline_adjustment(&runs, style, line_height);
        let mut shaped = ShapedInlineLine {
            text: line_text.to_string(),
            width: 0.0,
            offset: 0.0,
            aligned_by_parley: false,
            line_height,
            baseline_adjustment,
            runs,
        };
        if shaped.runs.is_empty() {
            return None;
        }
        shaped.width = shaped_line_measure_width(&shaped, style);
        Some(shaped)
    }

    /// Shape one unwrapped CSS line and keep the shaped run data that produced
    /// its advance.
    ///
    /// CSS Text line breaking and CSS Fonts shaping use the same formatted text
    /// input. Returning the shaped line as the measurement artifact keeps the
    /// glyph advances, fallback font ids, and bidi visual order available to
    /// later painting instead of measuring through a throwaway shape pass:
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
    /// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>.
    pub(crate) fn shape_unwrapped_line(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
    ) -> Option<ShapedInlineLine> {
        let runs = position_shaped_runs(self.shape_text_runs_with_parley(text, style));
        let baseline_adjustment = self.shaped_runs_baseline_adjustment(&runs, style, line_height);
        let mut shaped = ShapedInlineLine {
            text: text.to_string(),
            width: 0.0,
            offset: 0.0,
            aligned_by_parley: false,
            line_height,
            baseline_adjustment,
            runs,
        };
        if shaped.runs.is_empty() {
            return None;
        }
        shaped.width = shaped.advance_width();
        Some(shaped)
    }

    #[cfg(test)]
    pub(crate) fn shape_measured_line(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
    ) -> Option<ShapedInlineLine> {
        self.shape_unwrapped_line(text, style, line_height)
            .map(|mut shaped| {
                shaped.width = shaped_line_measure_width(&shaped, style);
                shaped
            })
    }

    /// Shape styled inline fragments into a durable CSS line artifact.
    ///
    /// CSS Text permits shaping across inline element boundaries unless a
    /// boundary condition interrupts shaping. This helper keeps Parley's
    /// resolved visual glyph runs, fallback document font ids, and glyph
    /// advances as the layout artifact consumed by painting and PDF emission:
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping>,
    /// <https://www.w3.org/TR/css-writing-modes-4/#bidi-linebox>, and
    /// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>.
    pub(crate) fn shape_styled_inline_fragments(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        text_summary: String,
        width: f32,
        line_height: f32,
    ) -> Option<ShapedInlineLine> {
        if spans.is_empty() {
            return None;
        }
        let runs = position_shaped_runs(self.shape_styled_text_runs_with_parley(spans));
        let first_style = spans.first().map(|span| span.style)?;
        let baseline_adjustment =
            self.shaped_runs_baseline_adjustment(&runs, first_style, line_height);
        (!runs.is_empty()).then_some(ShapedInlineLine {
            text: text_summary,
            width,
            offset: 0.0,
            aligned_by_parley: false,
            line_height,
            baseline_adjustment,
            runs,
        })
    }

    /// Shape adjacent styled text spans as one CSS Text shaping context.
    ///
    /// CSS Text requires shaping to see across inline element boundaries unless
    /// a boundary condition explicitly interrupts shaping. Parley accepts
    /// ranged styles, allowing the renderer to preserve font/style runs for
    /// PDF emission while still giving OpenType shaping adjacent cursive-script
    /// context across those ranges:
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
    /// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>.
    pub(crate) fn shape_styled_text_runs_with_parley(
        &mut self,
        spans: &[StyledTextSpan<'_>],
    ) -> Vec<RenderedTextRun> {
        if spans.is_empty() {
            return Vec::new();
        }
        let mut text = String::new();
        let mut ranges: Vec<(Range<usize>, &ComputedStyle)> = Vec::with_capacity(spans.len());
        let mut synthetic_join_controls = Vec::new();
        let spans = spans
            .iter()
            .filter(|span| !span.text.is_empty())
            .copied()
            .collect::<Vec<_>>();
        for (index, span) in spans.iter().enumerate() {
            if span.text.is_empty() {
                continue;
            }
            if let Some((range, style)) = ranges.last_mut()
                && *style == span.style
            {
                push_text_with_font_variant_emoji(&mut text, span.text, span.style);
                range.end = text.len();
                continue;
            }
            let start = text.len();
            if index > 0
                && spans.get(index - 1).is_some_and(|previous| {
                    previous.style != span.style
                        && span_boundary_needs_join_control(previous.text, span.text)
                })
            {
                push_synthetic_join_control(&mut text, &mut synthetic_join_controls);
            }
            push_text_with_font_variant_emoji(&mut text, span.text, span.style);
            if spans.get(index + 1).is_some_and(|next| {
                next.style != span.style && span_boundary_needs_join_control(span.text, next.text)
            }) {
                push_synthetic_join_control(&mut text, &mut synthetic_join_controls);
            }
            let range = start..text.len();
            ranges.push((range, span.style));
        }
        if text.is_empty() || ranges.is_empty() {
            return Vec::new();
        }
        push_edge_join_context(&mut text, &mut ranges, &mut synthetic_join_controls);
        if ranges.len() == 1 && synthetic_join_controls.is_empty() {
            return self.shape_text_runs_with_parley(&text, ranges[0].1);
        }

        let default_style = ranges[0].1;
        let default_feature_context = self.font_feature_context_for_style(default_style);
        let feature_contexts = ranges
            .iter()
            .map(|(_, style)| self.font_feature_context_for_style(style))
            .collect::<Vec<_>>();
        let mut builder = self.parley_layout_context.ranged_builder(
            &mut self.parley_font_context,
            &text,
            1.0,
            false,
        );
        push_parley_default_style(&mut builder, default_style);
        push_parley_text_spacing_default_with_context(
            &mut builder,
            &text,
            default_style,
            default_feature_context.as_ref(),
        );
        for ((range, style), feature_context) in ranges.iter().zip(&feature_contexts) {
            push_parley_style_range(&mut builder, style, range.clone());
            push_parley_text_spacing_range_with_context(
                &mut builder,
                &text[range.clone()],
                style,
                range.clone(),
                feature_context.as_ref(),
            );
        }
        let mut layout = builder.build(&text);
        layout.break_all_lines(None);
        let Some(line) = layout.lines().next() else {
            return Vec::new();
        };
        let adjustment_ranges =
            self.styled_font_size_adjustment_ranges_for_line(&line, &ranges, default_style);
        if !adjustment_ranges.is_empty() {
            let mut builder = self.parley_layout_context.ranged_builder(
                &mut self.parley_font_context,
                &text,
                1.0,
                false,
            );
            push_parley_default_style(&mut builder, default_style);
            push_parley_text_spacing_default_with_context(
                &mut builder,
                &text,
                default_style,
                default_feature_context.as_ref(),
            );
            for ((range, style), feature_context) in ranges.iter().zip(&feature_contexts) {
                push_parley_style_range(&mut builder, style, range.clone());
                push_parley_text_spacing_range_with_context(
                    &mut builder,
                    &text[range.clone()],
                    style,
                    range.clone(),
                    feature_context.as_ref(),
                );
            }
            for adjustment in &adjustment_ranges {
                builder.push(
                    StyleProperty::FontSize(adjustment.font_size),
                    adjustment.range.clone(),
                );
            }
            layout = builder.build(&text);
            layout.break_all_lines(None);
        }
        let Some(line) = layout.lines().next() else {
            return Vec::new();
        };
        let mut rendered_runs = Vec::new();
        let mut tab_contexts = Vec::new();
        for run in line.runs() {
            let run_range = run.text_range();
            let raw_run_text = text.get(run_range.clone()).unwrap_or_default();
            let run_text = text_without_variation_selectors(&text_without_synthetic_join_controls(
                &text,
                run_range.clone(),
                &synthetic_join_controls,
            ));
            let run_style =
                style_for_text_range(&ranges, run_range.clone()).unwrap_or(default_style);
            let x_offset = run
                .visual_clusters()
                .next()
                .and_then(|cluster| cluster.visual_offset())
                .unwrap_or(0.0);
            let Some(font_id) =
                self.document_font_from_parley_font_data_for_style(run.font(), run_style)
            else {
                continue;
            };
            if self.document_fonts.support_kind_for_run(font_id, &run_text)
                == FontSupportKind::ColorOrEmojiOnlyFallback
                && let Some(fallback_font_id) =
                    self.visible_text_fallback_for_run(&run_text, run_style, font_id)
                && let Some(fallback_font) = self.document_fonts.get(fallback_font_id)
                && let Some(glyphs) = shape_text_with_document_font(
                    fallback_font,
                    raw_run_text,
                    run.font_size(),
                    run_style.used_letter_spacing(),
                    run_style.used_word_spacing(),
                )
                && !glyphs.is_empty()
            {
                rendered_runs.push(RenderedTextRun {
                    text: run_text,
                    x_offset,
                    y_offset: 0.0,
                    text_matrix: crate::RenderedTextMatrix::IDENTITY,
                    font_size: run.font_size(),
                    font_id: Some(fallback_font_id),
                    glyphs: Some(glyphs_without_synthetic_join_controls(
                        glyphs,
                        raw_run_text,
                        run_range.start,
                        &synthetic_join_controls,
                    )),
                });
                tab_contexts.push(RenderedRunTabContext { style: run_style });
                continue;
            }
            let Some(font) = self.document_fonts.get(font_id) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
                continue;
            };
            let units_per_em = font.units_per_em.max(1) as f32;
            let scale = run.font_size() / units_per_em;
            let mut glyphs = Vec::new();
            for cluster in run.visual_clusters() {
                if range_is_synthetic_only(cluster.text_range(), &synthetic_join_controls) {
                    continue;
                }
                let raw_cluster_text = text.get(cluster.text_range()).unwrap_or_default();
                let cleaned_cluster_text =
                    text_without_glyph_output_controls(&text_without_synthetic_join_controls(
                        &text,
                        cluster.text_range(),
                        &synthetic_join_controls,
                    ));
                let cluster_glyphs = cluster.glyphs().collect::<Vec<_>>();
                let default_ignorable_only =
                    cluster_is_default_ignorable_only(raw_cluster_text, &cleaned_cluster_text);
                if default_ignorable_only
                    && !default_ignorable_cluster_has_shaping_glyph(
                        &face,
                        raw_run_text,
                        &cleaned_cluster_text,
                        cluster_glyphs
                            .iter()
                            .filter_map(|glyph| {
                                u16::try_from(glyph.id)
                                    .ok()
                                    .map(|glyph_id| (glyph_id, glyph.advance))
                            })
                            .collect::<Vec<_>>()
                            .as_slice(),
                    )
                {
                    continue;
                }
                if cleaned_cluster_text == "\t" {
                    glyphs.push(synthesized_tab_glyph(&face, scale));
                    continue;
                }
                let mut first_cluster_glyph = true;
                for glyph in cluster_glyphs {
                    let Ok(glyph_id) = u16::try_from(glyph.id) else {
                        continue;
                    };
                    let unicode = if first_cluster_glyph {
                        if default_ignorable_only {
                            String::new()
                        } else {
                            cleaned_cluster_text.clone()
                        }
                    } else {
                        String::new()
                    };
                    if unicode.is_empty() && glyph.advance == 0.0 {
                        first_cluster_glyph = false;
                        continue;
                    }
                    if unicode.is_empty()
                        && !synthetic_join_controls.is_empty()
                        && face
                            .glyph_index('\u{0640}')
                            .is_some_and(|nominal| nominal.0 == glyph_id)
                    {
                        first_cluster_glyph = false;
                        continue;
                    }
                    let emitted_glyph_id = unicode
                        .chars()
                        .next()
                        .filter(|_| unicode.chars().count() == 1)
                        .and_then(|character| css_space_separator_blank_glyph(&face, character))
                        .map(|glyph| glyph.0)
                        .unwrap_or(glyph_id);
                    first_cluster_glyph = false;
                    glyphs.push(RenderedGlyph {
                        id: emitted_glyph_id,
                        x_advance: glyph.advance,
                        nominal_x_advance: face
                            .glyph_hor_advance(ttf_parser::GlyphId(emitted_glyph_id))
                            .map(|advance| advance as f32 * scale)
                            .unwrap_or(glyph.advance),
                        x_offset: glyph.x,
                        y_offset: -glyph.y,
                        unicode,
                    });
                }
            }
            if glyphs.is_empty() {
                continue;
            }
            let mut font_size = run.font_size();
            apply_synthetic_position_fallback(
                &mut glyphs,
                &mut font_size,
                run_style,
                &face,
                &run_text,
            );
            rendered_runs.push(RenderedTextRun {
                text: run_text,
                x_offset,
                y_offset: 0.0,
                text_matrix: crate::RenderedTextMatrix::IDENTITY,
                font_size,
                font_id: Some(font_id),
                glyphs: Some(glyphs),
            });
            tab_contexts.push(RenderedRunTabContext { style: run_style });
        }
        self.apply_css_tab_stops(&mut rendered_runs, &tab_contexts);
        rendered_runs
    }

    fn visible_text_fallback_for_run(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        current_font_id: usize,
    ) -> Option<usize> {
        let mut fallback_font_id = None;
        for character in text
            .chars()
            .filter(|character| !character_is_default_ignorable_code_point(*character))
        {
            let candidate = self.resolve_family_fallback_for_character(style, character)?;
            if candidate == current_font_id {
                return None;
            }
            match fallback_font_id {
                Some(existing) if existing != candidate => return None,
                Some(_) => {}
                None => fallback_font_id = Some(candidate),
            }
        }
        fallback_font_id
    }

    /// Resolve preserved CSS tab characters to used tab-stop advances.
    ///
    /// CSS Text defines preserved tabs as invisible advances to the next
    /// periodic tab stop, where numeric `tab-size` values are multiples of the
    /// selected U+0020 advance and length values are absolute computed lengths:
    /// <https://www.w3.org/TR/css-text-3/#tab-size-property>.
    fn apply_css_tab_stops(
        &self,
        runs: &mut [RenderedTextRun],
        contexts: &[RenderedRunTabContext<'_>],
    ) {
        if !runs.iter().any(|run| run.text.contains('\t')) {
            return;
        }

        for run_index in 0..runs.len() {
            let Some(style) = contexts.get(run_index).map(|context| context.style) else {
                continue;
            };
            let Some(font_id) = runs[run_index].font_id else {
                continue;
            };
            let Some(font) = self.document_fonts.get(font_id) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
                continue;
            };
            let scale = runs[run_index].font_size / font.units_per_em.max(1) as f32;
            let space_advance =
                tab_stop_space_advance(&face, runs[run_index].font_size, scale, style);
            let tab_period = style.tab_size.used_tab_stop_advance(space_advance);
            let run_x_offset = runs[run_index].x_offset;
            let mut pen_x = 0.0;
            let mut following_run_shift = 0.0;

            let Some(glyphs) = runs[run_index].glyphs.as_mut() else {
                continue;
            };
            for glyph in glyphs {
                if glyph.unicode == "\t" {
                    let old_advance = glyph.x_advance;
                    let used_advance = tab_stop_advance(tab_period, run_x_offset + pen_x);
                    glyph.x_advance = used_advance;
                    glyph.nominal_x_advance = space_advance;
                    glyph.x_offset = 0.0;
                    glyph.y_offset = 0.0;
                    following_run_shift += used_advance - old_advance;
                }
                pen_x += glyph.x_advance;
            }

            if following_run_shift.abs() > 0.01 {
                for following_run in runs.iter_mut().skip(run_index + 1) {
                    following_run.x_offset += following_run_shift;
                }
            }
        }
    }

    pub(crate) fn font_ascent_baseline_adjustment(
        &self,
        font_id: Option<usize>,
        style: &ComputedStyle,
        _line_height: f32,
    ) -> f32 {
        let used_font_size = font_id
            .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
            .unwrap_or(style.font_size);
        self.font_ascent_baseline_adjustment_for_font_size(font_id, style, used_font_size)
    }

    fn shaped_runs_baseline_adjustment(
        &self,
        runs: &[ShapedInlineRun],
        style: &ComputedStyle,
        _line_height: f32,
    ) -> f32 {
        let Some(run) = runs.iter().find(|run| run.font_id.is_some()) else {
            return 0.0;
        };
        self.font_ascent_baseline_adjustment_for_font_size(run.font_id, style, run.font_size)
    }

    fn font_ascent_baseline_adjustment_for_font_size(
        &self,
        font_id: Option<usize>,
        style: &ComputedStyle,
        used_font_size: f32,
    ) -> f32 {
        let Some(font) = font_id.and_then(|id| self.document_fonts.get(id)) else {
            return 0.0;
        };
        let ascent = font.ascender as f32 * used_font_size / font.units_per_em as f32;
        // CSS Inline positions glyphs using selected-font metrics inside the
        // CSS line box. The used font size can differ from the computed
        // `font-size` when CSS Fonts `font-size-adjust` is active, while the
        // line box anchor remains based on computed font size:
        // https://www.w3.org/TR/css-fonts-5/#font-size-adjust-prop
        //
        // `used_font_size` and scaled OpenType metrics are
        // already in PDF point/layout units here, so no CSS px conversion is
        // applied:
        // https://www.w3.org/TR/CSS22/visudet.html#line-height
        style.font_size - ascent
    }

    /// Return the rendered first-line text baseline offset from line-box top.
    ///
    /// CSS Inline Layout aligns inline-level boxes to line baselines. Formatting
    /// contexts that synthesize baselines must use the same selected-font ascent
    /// projection as text painting so their exported baselines match rendered
    /// text lines:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
    pub(crate) fn rendered_first_line_baseline_offset(&mut self, style: &ComputedStyle) -> f32 {
        let font_id = self.resolve_style(style);
        let line_height = self.line_height_for_font(font_id, style);
        let adjustment = self.font_ascent_baseline_adjustment(font_id, style, line_height);
        style.font_size - adjustment
    }

    /// Return the selected font's x-height in layout units.
    ///
    /// CSS 2.2 `vertical-align: middle` aligns against half of the parent's
    /// x-height. Font metrics are preferred, with the glyph bounding box for
    /// `x` as a fallback:
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
    pub(crate) fn x_height_for_style(&mut self, style: &ComputedStyle) -> Option<f32> {
        let font_id = self.resolve_style(style);
        let used_font_size = font_id
            .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
            .unwrap_or(style.font_size);
        let font = font_id.and_then(|id| self.document_fonts.get(id))?;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        let units_per_em = font.units_per_em.max(1) as f32;
        let height = face
            .x_height()
            .map(|height| height as f32)
            .or_else(|| glyph_bbox_height(&face, 'x'))?;
        Some(height * used_font_size / units_per_em)
    }

    /// Return the selected font's recommended super/subscript baseline shift.
    ///
    /// OpenType OS/2 script offsets are used when available. Positive return
    /// values raise the inline box; negative values lower it:
    /// <https://learn.microsoft.com/en-us/typography/opentype/spec/os2#ysubscriptxoff-y-subscript-yoff-y-subscript-xsize-y-subscript-ysize>.
    pub(crate) fn script_vertical_align_shift(
        &mut self,
        style: &ComputedStyle,
        baseline_shift: BaselineShift,
    ) -> Option<f32> {
        let font_id = self.resolve_style(style);
        let used_font_size = font_id
            .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
            .unwrap_or(style.font_size);
        let font = font_id.and_then(|id| self.document_fonts.get(id))?;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        let units_per_em = font.units_per_em.max(1) as f32;
        let offset = match baseline_shift {
            BaselineShift::Super => face.superscript_metrics()?.y_offset as f32,
            BaselineShift::Sub => -(face.subscript_metrics()?.y_offset as f32),
            _ => return None,
        };
        let shift = offset * used_font_size / units_per_em;
        (shift.is_finite() && shift.abs() > 0.01).then_some(shift)
    }

    /// Convert a rendered PDF text line back to the CSS line alignment coordinate.
    ///
    /// CSS 2.2 positions inline content using line-box font metrics, while the
    /// PDF backend stores text after applying the font ascent adjustment used
    /// for glyph emission. This helper reverses that adjustment for layout
    /// code that must align atomic inline fragments to shaped text.
    /// https://www.w3.org/TR/CSS22/visudet.html#line-height
    pub(crate) fn rendered_line_alignment_y(&self, line: &RenderedLine) -> f32 {
        let adjustment = line
            .font_id
            .and_then(|font_id| self.document_fonts.get(font_id))
            .map(|font| {
                let ascent = font.ascender as f32 * line.font_size / font.units_per_em as f32;
                line.font_size - ascent
            })
            .unwrap_or(0.0);
        line.y() + line.font_size - adjustment
    }
}

fn font_feature_family(font_family: &FontFamily) -> Option<String> {
    match font_family {
        FontFamily::SansSerif => Some("sans-serif".to_string()),
        FontFamily::Serif => Some("serif".to_string()),
        FontFamily::Monospace => Some("monospace".to_string()),
        FontFamily::Names(names) => names.first().cloned(),
    }
}

fn style_for_text_range<'a>(
    ranges: &[(Range<usize>, &'a ComputedStyle)],
    run_range: Range<usize>,
) -> Option<&'a ComputedStyle> {
    ranges
        .iter()
        .find(|(range, _)| {
            range.start <= run_range.start
                && (run_range.start < range.end || run_range.start == run_range.end)
        })
        .map(|(_, style)| *style)
}

fn merge_join_control_visual_ranges(text: &str, ranges: &mut Vec<Range<usize>>) {
    let mut merged = Vec::<Range<usize>>::with_capacity(ranges.len());
    for range in ranges.drain(..) {
        if let Some(previous) = merged.last_mut()
            && previous.start == range.end
            && (range_text_is_join_control_only(text, &range)
                || range_text_is_join_control_only(text, previous))
        {
            previous.start = range.start;
            continue;
        }
        if let Some(previous) = merged.last_mut()
            && previous.end == range.start
            && (range_text_is_join_control_only(text, &range)
                || range_text_is_join_control_only(text, previous))
        {
            previous.end = range.end;
            continue;
        }
        merged.push(range);
    }
    *ranges = merged;
}

fn range_text_is_join_control_only(text: &str, range: &Range<usize>) -> bool {
    text.get(range.clone())
        .is_some_and(|slice| !slice.is_empty() && slice.chars().all(character_is_join_control))
}

/// Return a CSS Fonts 5 `font-size-adjust` metric ratio for a selected face.
///
/// Ratios are metric values divided by units-per-em, matching the aspect-value
/// used-size formula defined for `font-size-adjust`:
/// <https://www.w3.org/TR/css-fonts-5/#font-size-adjust-prop>.
fn font_size_adjust_metric_ratio(font: &DocumentFont, metric: FontSizeAdjustMetric) -> Option<f32> {
    let units_per_em = font.units_per_em.max(1) as f32;
    let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
    let value = match metric {
        FontSizeAdjustMetric::ExHeight => face
            .x_height()
            .map(|height| height as f32)
            .or_else(|| glyph_bbox_height(&face, 'x'))?,
        FontSizeAdjustMetric::CapHeight => face
            .capital_height()
            .map(|height| height as f32)
            .filter(|height| *height > 0.0)
            .or_else(|| (font.cap_height > 0).then_some(font.cap_height as f32))
            .or_else(|| glyph_bbox_height(&face, 'H'))?,
        FontSizeAdjustMetric::ChWidth => glyph_advance_width(&face, '0')?,
        FontSizeAdjustMetric::IcWidth => glyph_advance_width(&face, '水')?,
        FontSizeAdjustMetric::IcHeight => face
            .glyph_index('水')
            .and_then(|glyph| face.glyph_ver_advance(glyph))
            .map(|advance| advance as f32)
            .or_else(|| glyph_bbox_height(&face, '水'))
            .unwrap_or(units_per_em),
    };
    (value.is_finite() && value > 0.0).then_some(value / units_per_em)
}

fn glyph_advance_width(face: &ttf_parser::Face<'_>, character: char) -> Option<f32> {
    face.glyph_index(character)
        .and_then(|glyph| face.glyph_hor_advance(glyph))
        .map(|advance| advance as f32)
        .filter(|advance| *advance > 0.0)
}

fn glyph_bbox_height(face: &ttf_parser::Face<'_>, character: char) -> Option<f32> {
    face.glyph_index(character)
        .and_then(|glyph| face.glyph_bounding_box(glyph))
        .map(|bbox| (bbox.y_max - bbox.y_min).abs() as f32)
        .filter(|height| *height > 0.0)
}

fn position_shaped_runs(runs: Vec<RenderedTextRun>) -> Vec<ShapedInlineRun> {
    runs.into_iter()
        .filter_map(|mut run| {
            let glyphs = run.glyphs.take().unwrap_or_default();
            if glyphs.is_empty() {
                return None;
            }
            Some(ShapedInlineRun {
                text: run.text,
                x_offset: run.x_offset,
                font_size: run.font_size,
                font_id: run.font_id,
                glyphs: glyphs
                    .into_iter()
                    .map(|glyph| ShapedInlineGlyph {
                        source_text: glyph.unicode.clone(),
                        paints: true,
                        rendered: glyph,
                    })
                    .collect(),
                paints: true,
            })
        })
        .collect()
}

fn synthesized_tab_glyph(face: &ttf_parser::Face<'_>, scale: f32) -> RenderedGlyph {
    let glyph_id = face.glyph_index(' ').unwrap_or(ttf_parser::GlyphId(0));
    let nominal_x_advance = face
        .glyph_hor_advance(glyph_id)
        .map(|advance| advance as f32 * scale)
        .unwrap_or(0.0);
    RenderedGlyph {
        id: glyph_id.0,
        x_advance: nominal_x_advance,
        nominal_x_advance,
        x_offset: 0.0,
        y_offset: 0.0,
        unicode: "\t".to_string(),
    }
}

fn tab_stop_space_advance(
    face: &ttf_parser::Face<'_>,
    font_size: f32,
    scale: f32,
    style: &ComputedStyle,
) -> f32 {
    face.glyph_index(' ')
        .and_then(|glyph| face.glyph_hor_advance(glyph))
        .map(|advance| advance as f32 * scale)
        .unwrap_or(font_size * 0.25)
        + style.used_word_spacing()
}

fn tab_stop_advance(period: f32, current_x: f32) -> f32 {
    if period <= 0.0 || !period.is_finite() || !current_x.is_finite() {
        return 0.0;
    }
    let next_stop = (current_x / period).floor().mul_add(period, period);
    (next_stop - current_x).max(0.0)
}

#[cfg(test)]
fn shaped_line_measure_width(line: &ShapedInlineLine, style: &ComputedStyle) -> f32 {
    let measured_text = trim_trailing_css_hanging_space_separators(&line.text, style);
    let hanging_width = shaped_suffix_advance(line, measured_text.len());
    (line.advance_width() - hanging_width - line_end_letter_spacing_width(measured_text, style))
        .max(0.0)
}

#[cfg(test)]
fn shaped_suffix_advance(line: &ShapedInlineLine, prefix_len: usize) -> f32 {
    if prefix_len >= line.text.len() {
        return 0.0;
    }
    let mut remaining = &line.text[prefix_len..];
    let mut width = 0.0;
    for glyph in line
        .runs
        .iter()
        .rev()
        .flat_map(|run| run.glyphs.iter().rev())
    {
        if remaining.is_empty() {
            break;
        }
        if glyph.source_text.is_empty() {
            continue;
        }
        if remaining.ends_with(&glyph.source_text) {
            width += glyph.rendered.x_advance;
            remaining = &remaining[..remaining.len() - glyph.source_text.len()];
        }
    }
    if remaining.is_empty() { width } else { 0.0 }
}

/// Return whether a styled-span boundary needs shaping-only ZWJ context.
///
/// CSS Text requires cursive shaping across non-breaking inline boundaries;
/// U+200D ZERO WIDTH JOINER is the Unicode mechanism for preserving joining
/// behavior when a shaping engine splits style/font runs:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.unicode.org/reports/tr44/#Joining_Type>.
pub(crate) fn span_boundary_needs_join_control(left: &str, right: &str) -> bool {
    if left
        .chars()
        .next_back()
        .is_some_and(character_is_join_control)
        || right.chars().next().is_some_and(character_is_join_control)
    {
        return false;
    }
    let Some(left) = left
        .chars()
        .rev()
        .find(|character| !character_is_join_control(*character))
    else {
        return false;
    };
    let Some(right) = right
        .chars()
        .find(|character| !character_is_join_control(*character))
    else {
        return false;
    };
    character_can_join_following(left) && character_can_join_preceding(right)
}

/// Append a shaping-only ZWJ and remember its byte range for output cleanup.
///
/// CSS Text shaping may need join controls that are not present in the DOM
/// text. Tracking synthetic controls separately preserves the original text
/// for PDF extraction while still giving OpenType shaping the required
/// joining context:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
fn push_synthetic_join_control(text: &mut String, synthetic_ranges: &mut Vec<Range<usize>>) {
    let start = text.len();
    text.push('\u{200d}');
    synthetic_ranges.push(start..text.len());
}

fn text_needs_edge_join_context(text: &str) -> bool {
    text_needs_leading_join_context(text) || text_needs_trailing_join_context(text)
}

fn text_needs_leading_join_context(text: &str) -> bool {
    let mut characters = text.chars();
    if characters.next() != Some('\u{200d}') {
        return false;
    }
    characters
        .find(|character| !character_is_join_control(*character))
        .is_some_and(character_can_join_preceding)
}

fn text_needs_trailing_join_context(text: &str) -> bool {
    let mut characters = text.chars().rev();
    if characters.next() != Some('\u{200d}') {
        return false;
    }
    characters
        .find(|character| !character_is_join_control(*character))
        .is_some_and(character_can_join_following)
}

/// Add shaping-only tatweel at run edges requested by explicit ZWJ.
///
/// U+200D at the start or end of an isolated shaping run asks the shaper to
/// form a connection to a neighboring joining context. Some shaping backends
/// do not apply that edge context without a concrete joining neighbor, so the
/// renderer supplies U+0640 ARABIC TATWEEL as shaping-only context and removes
/// it from emitted glyph text:
/// <https://www.w3.org/TR/css-text-3/#text-encoding> and
/// <https://www.w3.org/TR/alreq/#h_joining_enforcement>.
fn push_edge_join_context(
    text: &mut String,
    ranges: &mut Vec<(Range<usize>, &ComputedStyle)>,
    synthetic_ranges: &mut Vec<Range<usize>>,
) {
    let mut insertions = Vec::new();
    for (range, _) in ranges.iter() {
        let Some(slice) = text.get(range.clone()) else {
            continue;
        };
        if let Some(index) = leading_join_context_insertion_index(slice) {
            insertions.push(range.start + index);
        }
        if let Some(index) = trailing_join_context_insertion_index(slice) {
            insertions.push(range.start + index);
        }
    }
    insertions.sort_unstable();
    insertions.dedup();
    for index in insertions.into_iter().rev() {
        insert_synthetic_join_context(text, ranges, synthetic_ranges, index);
    }
}

fn range_is_synthetic_only(range: Range<usize>, synthetic_ranges: &[Range<usize>]) -> bool {
    range.start < range.end
        && synthetic_ranges
            .iter()
            .any(|synthetic| synthetic.start <= range.start && range.end <= synthetic.end)
}

fn leading_join_context_insertion_index(text: &str) -> Option<usize> {
    if !text.starts_with('\u{200d}') {
        return None;
    }
    text.char_indices()
        .find(|(_, character)| !character_is_join_control(*character))
        .and_then(|(index, character)| character_can_join_preceding(character).then_some(index))
}

fn trailing_join_context_insertion_index(text: &str) -> Option<usize> {
    if !text.ends_with('\u{200d}') {
        return None;
    }
    text.char_indices()
        .rev()
        .find(|(_, character)| !character_is_join_control(*character))
        .and_then(|(index, character)| {
            character_can_join_following(character).then_some(index + character.len_utf8())
        })
}

fn insert_synthetic_join_context(
    text: &mut String,
    ranges: &mut [(Range<usize>, &ComputedStyle)],
    synthetic_ranges: &mut Vec<Range<usize>>,
    index: usize,
) {
    let context_len = '\u{0640}'.len_utf8();
    text.insert(index, '\u{0640}');
    for (range, _) in ranges.iter_mut() {
        if range.start >= index {
            range.start += context_len;
            range.end += context_len;
        } else if range.end >= index {
            range.end += context_len;
        }
    }
    for range in synthetic_ranges.iter_mut() {
        if range.start >= index {
            range.start += context_len;
            range.end += context_len;
        } else if range.end >= index {
            range.end += context_len;
        }
    }
    synthetic_ranges.push(index..index + context_len);
}

/// Remove shaping-only join controls from emitted text content.
///
/// PDF ToUnicode data should reflect the document text, not internal shaping
/// controls inserted to satisfy CSS Text boundary shaping:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and ISO 32000-2
/// section 9.10.3.
fn text_without_synthetic_join_controls(
    text: &str,
    range: Range<usize>,
    synthetic_ranges: &[Range<usize>],
) -> String {
    let Some(slice) = text.get(range.clone()) else {
        return String::new();
    };
    let mut output = String::new();
    for (offset, character) in slice.char_indices() {
        let index = range.start + offset;
        if !synthetic_ranges
            .iter()
            .any(|synthetic| synthetic.contains(&index))
        {
            output.push(character);
        }
    }
    output
}

/// Remove shaping-only join-control glyph records from fallback-shaped output.
///
/// The fallback shaper maps one input character to one glyph, so synthetic ZWJ
/// code points can be dropped without changing visible glyph advances:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
fn glyphs_without_synthetic_join_controls(
    glyphs: Vec<RenderedGlyph>,
    raw_text: &str,
    run_start: usize,
    synthetic_ranges: &[Range<usize>],
) -> Vec<RenderedGlyph> {
    let mut output = Vec::with_capacity(glyphs.len());
    let mut glyphs = glyphs.into_iter();
    for (offset, character) in raw_text.char_indices() {
        let Some(mut glyph) = glyphs.next() else {
            break;
        };
        let index = run_start + offset;
        if synthetic_ranges
            .iter()
            .any(|synthetic| synthetic.contains(&index))
            || character_is_default_ignorable_code_point(character)
        {
            continue;
        } else {
            glyph.unicode = character.to_string();
            output.push(glyph);
        }
    }
    output.extend(glyphs);
    output
}

/// Remove default-ignorable controls that must not affect font fallback.
///
/// CSS Text line breaking still operates on the original text. This shaping
/// cleanup only removes default-ignorable controls that are neutral for glyph
/// selection and bidi ordering, preventing controls such as CGJ from making a
/// visible Ahem glyph fall back to another font:
/// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
/// <https://www.w3.org/TR/css-text-3/#line-break-details>.
fn text_without_font_neutral_default_ignorables(text: &str) -> Cow<'_, str> {
    if !text
        .chars()
        .any(character_is_font_neutral_default_ignorable)
    {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter(|character| !character_is_font_neutral_default_ignorable(*character))
            .collect(),
    )
}

fn text_with_font_variant_emoji<'a>(text: &'a str, style: &ComputedStyle) -> Cow<'a, str> {
    if matches!(
        style.font_variant_emoji,
        FontVariantEmoji::Normal | FontVariantEmoji::Unicode
    ) {
        return Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(text.len());
    push_text_with_font_variant_emoji(&mut output, text, style);
    if output == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(output)
    }
}

fn push_text_with_font_variant_emoji(output: &mut String, text: &str, style: &ComputedStyle) {
    let selector = match style.font_variant_emoji {
        FontVariantEmoji::Text => '\u{fe0e}',
        FontVariantEmoji::Emoji => '\u{fe0f}',
        FontVariantEmoji::Normal | FontVariantEmoji::Unicode => {
            output.push_str(text);
            return;
        }
    };
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        output.push(character);
        if emoji_presentation_participating_code_point(character)
            && !chars
                .peek()
                .is_some_and(|next| matches!(*next, '\u{fe0e}' | '\u{fe0f}'))
        {
            output.push(selector);
        }
    }
}

fn text_without_variation_selectors(text: &str) -> String {
    text.chars()
        .filter(
            |character| !matches!(character, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}'),
        )
        .collect()
}

fn text_without_glyph_output_controls(text: &str) -> String {
    text.chars()
        .filter(|character| {
            !character_is_join_control(*character)
                && !matches!(
                    character,
                    '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}'
                )
        })
        .collect()
}

fn emoji_presentation_participating_code_point(character: char) -> bool {
    matches!(
        character as u32,
        0x00a9
            | 0x00ae
            | 0x203c
            | 0x2049
            | 0x2122
            | 0x2139
            | 0x2194..=0x21aa
            | 0x231a..=0x231b
            | 0x2328
            | 0x23cf
            | 0x23e9..=0x23f3
            | 0x23f8..=0x23fa
            | 0x24c2
            | 0x25aa..=0x25ab
            | 0x25b6
            | 0x25c0
            | 0x25fb..=0x25fe
            | 0x2600..=0x27bf
            | 0x2934..=0x2935
            | 0x2b05..=0x2b55
            | 0x3030
            | 0x303d
            | 0x3297
            | 0x3299
            | 0x1f000..=0x1faff
    )
}

fn apply_synthetic_position_fallback(
    glyphs: &mut [RenderedGlyph],
    font_size: &mut f32,
    style: &ComputedStyle,
    face: &ttf_parser::Face<'_>,
    text: &str,
) {
    let (scale, shift) = match style.font_variant_position {
        FontVariantPosition::Sub => (0.65, -*font_size * 0.2),
        FontVariantPosition::Super => (0.65, *font_size * 0.35),
        FontVariantPosition::Normal => return,
    };
    if opentype_position_feature_substituted(glyphs, face, text) {
        return;
    }
    *font_size *= scale;
    for glyph in glyphs {
        glyph.x_advance *= scale;
        glyph.nominal_x_advance *= scale;
        glyph.x_offset *= scale;
        glyph.y_offset = glyph.y_offset * scale + shift;
    }
}

fn opentype_position_feature_substituted(
    glyphs: &[RenderedGlyph],
    face: &ttf_parser::Face<'_>,
    text: &str,
) -> bool {
    let mut visible_glyphs = glyphs
        .iter()
        .filter(|glyph| !glyph.unicode.is_empty())
        .filter(|glyph| {
            glyph
                .unicode
                .chars()
                .any(|character| !character_is_default_ignorable_code_point(character))
        });
    text.chars()
        .filter(|character| !character_is_default_ignorable_code_point(*character))
        .zip(&mut visible_glyphs)
        .any(|(character, glyph)| {
            face.glyph_index(character)
                .is_some_and(|nominal| nominal.0 != glyph.id)
        })
}

/// Return whether a shaped glyph cluster represents only default-ignorable code points.
///
/// CSS text shaping must preserve controls such as ZWJ/ZWNJ and variation
/// selectors in shaping input, while PDF painting must not emit visible
/// fallback glyphs for clusters made only from Unicode default-ignorable code
/// points:
/// <https://www.w3.org/TR/css-text-3/#text-encoding>,
/// <https://www.unicode.org/reports/tr44/#Default_Ignorable_Code_Point>, and
/// ISO 32000-2 section 9.10.3.
fn cluster_is_default_ignorable_only(raw_text: &str, emitted_text: &str) -> bool {
    !raw_text.is_empty()
        && raw_text
            .chars()
            .all(character_is_default_ignorable_code_point)
        && (emitted_text.is_empty()
            || emitted_text
                .chars()
                .all(character_is_default_ignorable_code_point))
}

fn default_ignorable_cluster_has_shaping_glyph(
    face: &ttf_parser::Face<'_>,
    run_text: &str,
    emitted_cluster_text: &str,
    glyphs: &[(u16, f32)],
) -> bool {
    run_text
        .chars()
        .any(|character| !character_is_default_ignorable_code_point(character))
        && glyphs.iter().any(|(glyph_id, advance)| {
            *advance != 0.0
                && !emitted_cluster_text.chars().any(|character| {
                    face.glyph_index(character)
                        .is_some_and(|nominal| nominal.0 == *glyph_id)
                })
        })
}
