use super::*;
use std::borrow::Cow;

#[derive(Debug, Clone)]
pub(in crate::text) struct FontSizeAdjustmentRange {
    pub(in crate::text) range: Range<usize>,
    pub(in crate::text) font_size: f32,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::text) struct RenderedRunTabContext<'a> {
    pub(in crate::text) style: &'a ComputedStyle,
}

impl FontSystem {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::from_seed(Self::sync_seed())
    }

    pub(crate) fn into_fonts(self) -> Vec<DocumentFont> {
        self.document_fonts.into_fonts()
    }

    pub(in crate::text) fn font_feature_context_for_style(
        &self,
        style: &ComputedStyle,
    ) -> Option<FontFeatureContext> {
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

    pub(in crate::text) fn font_size_adjust_target_ratio(
        &mut self,
        style: &ComputedStyle,
    ) -> Option<(FontSizeAdjustMetric, f32)> {
        let FontSizeAdjust::Value { metric, value } = style.font_size_adjust else {
            return None;
        };
        let ratio = match value {
            FontSizeAdjustValue::Number(value) => value,
            FontSizeAdjustValue::FromFont => {
                let font_id = self.resolve_metric_font_for_style(style)?;
                let font = self.document_fonts.get(font_id)?;
                font_size_adjust_metric_ratio(font, metric)?
            }
        };
        ratio.is_finite().then_some((metric, ratio))
    }

    pub(in crate::text) fn adjusted_font_size_for_font(
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

    pub(in crate::text) fn font_size_adjusted_size_for_font_id(
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
    /// element's font. In vertical writing with upright text orientation, that
    /// advance is the vertical inline-axis advance, falling back to 1em when
    /// the selected face has no vertical metric for "0". Otherwise it falls
    /// back to 0.5em when measuring that glyph is not possible:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn ch_advance(&mut self, style: &ComputedStyle) -> f32 {
        if style.writing_mode != WritingMode::HorizontalTb
            && style.text_orientation == TextOrientation::Upright
        {
            if let Some(advance) = self.vertical_upright_ch_advance(style) {
                return advance;
            }
            return style.font_size;
        }

        let mut metric_style = style.clone();
        metric_style.letter_spacing = ComputedLengthPercentage::ZERO;
        let advance = self.measure_text("0", &metric_style);
        if advance > 0.0 {
            advance
        } else {
            style.font_size * 0.5
        }
    }

    pub(in crate::text) fn vertical_upright_ch_advance(
        &mut self,
        style: &ComputedStyle,
    ) -> Option<f32> {
        let font_id = self.resolve_metric_font_for_style(style)?;
        let used_font_size = self
            .font_size_adjusted_size_for_font_id(style, font_id)
            .unwrap_or(style.font_size);
        let font = self.document_fonts.get(font_id)?;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        let units_per_em = font.units_per_em.max(1) as f32;
        let advance = face
            .glyph_index('0')
            .and_then(|glyph| face.glyph_ver_advance(glyph))
            .map(|advance| advance as f32)
            .filter(|advance| *advance > 0.0)
            .unwrap_or(units_per_em);
        Some(advance * used_font_size / units_per_em)
    }

    pub(crate) fn used_line_height(&mut self, style: &ComputedStyle) -> f32 {
        if !style.line_height_is_normal {
            return style.line_height;
        }
        let font_id = self.resolve_metric_font_for_style(style);
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

    pub(in crate::text) fn font_size_adjustment_ranges_for_line<B: parley::style::Brush>(
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

    pub(in crate::text) fn styled_font_size_adjustment_ranges_for_line<B: parley::style::Brush>(
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

    pub(in crate::text) fn rendered_text_runs_for_parley_line<B: parley::style::Brush>(
        &mut self,
        text: &str,
        line: parley::Line<'_, B>,
        style: &ComputedStyle,
    ) -> Vec<RenderedTextRun> {
        let run_count = line.runs().size_hint().0;
        let mut rendered_runs = Vec::with_capacity(run_count);
        let mut tab_contexts = Vec::with_capacity(run_count);
        for run in line.runs() {
            let run_text = text
                .get(run.text_range())
                .map(text_without_variation_selectors)
                .unwrap_or_else(|| Cow::Borrowed(""));
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
            if self
                .document_fonts
                .support_kind_for_run(font_id, run_text.as_ref())
                == FontSupportKind::ColorOrEmojiOnlyFallback
                && let Some(fallback_font_id) =
                    self.visible_text_fallback_for_run(run_text.as_ref(), style, font_id)
                && let Some(fallback_font) = self.document_fonts.get(fallback_font_id)
                && let Some(glyphs) = shape_text_with_document_font(
                    fallback_font,
                    run_text.as_ref(),
                    run.font_size(),
                    style.used_letter_spacing(),
                    style.used_word_spacing(),
                )
                && !glyphs.is_empty()
            {
                rendered_runs.push(RenderedTextRun {
                    text: run_text.into_owned(),
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
                let default_ignorable_only =
                    cluster_is_default_ignorable_only(cluster_text, emitted_cluster_text.as_ref());
                if default_ignorable_only
                    && !default_ignorable_cluster_has_shaping_glyph(
                        &face,
                        run_text.as_ref(),
                        emitted_cluster_text.as_ref(),
                        cluster.glyphs().filter_map(|glyph| {
                            u16::try_from(glyph.id)
                                .ok()
                                .map(|glyph_id| (glyph_id, glyph.advance))
                        }),
                    )
                {
                    continue;
                }
                if emitted_cluster_text.as_ref() == "\t" {
                    glyphs.push(synthesized_tab_glyph(&face, scale));
                    continue;
                }
                let mut first_cluster_glyph = true;
                for glyph in cluster.glyphs() {
                    let Ok(glyph_id) = u16::try_from(glyph.id) else {
                        continue;
                    };
                    let unicode = if first_cluster_glyph {
                        if default_ignorable_only {
                            String::new()
                        } else {
                            emitted_cluster_text.as_ref().to_owned()
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
            apply_synthetic_position_fallback(
                &mut glyphs,
                &mut font_size,
                style,
                &face,
                run_text.as_ref(),
            );
            rendered_runs.push(RenderedTextRun {
                text: run_text.into_owned(),
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
    pub(in crate::text) fn unicode_range_resolved_text_spans(
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

    pub(in crate::text) fn next_unicode_range_family_for_text(
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

    pub(in crate::text) fn unicode_range_family_for_character(
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
        let mut runs = position_shaped_runs(self.shape_text_runs_with_parley(text, style));
        self.apply_vertical_upright_advances(&mut runs, style);
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
        let first_style = spans.first().map(|span| span.style)?;
        let mut runs = position_shaped_runs(self.shape_styled_text_runs_with_parley(spans));
        self.apply_vertical_upright_advances(&mut runs, first_style);
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
}
