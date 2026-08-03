use super::super::*;
#[cfg(test)]
use crate::text::trim_trailing_css_hanging_space_separators;
use crate::units::{LayoutLength, SemanticLengthExt, layout_pt};
use std::borrow::Cow;
use std::rc::Rc;

impl FontSystem {
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
    ) -> Vec<BidiVisualRange> {
        if text.is_empty() {
            return Vec::new();
        }
        let emoji_text = text_with_font_variant_emoji(text, style);
        let bidi_text = text_with_css_bidi_controls(emoji_text.as_ref(), style);
        let shaped_text = bidi_text.as_str();
        self.with_reusable_parley_layout(|this, layout| {
            let feature_context = this.font_feature_context_for_style(style);
            let font_family_source = this
                .emoji_presentation_family_source(emoji_text.as_ref(), style)
                .unwrap_or_else(|| this.resolved_parley_font_family_source(style));
            let mut builder: parley::RangedBuilder<'_, FontPalette> = this
                .parley_layout_context
                .ranged_builder(&mut this.parley_font_context, shaped_text, 1.0, false);
            push_parley_default_style(&mut builder, style, &font_family_source);
            push_parley_text_spacing_default_with_context(
                &mut builder,
                shaped_text,
                style,
                ShapingLetterSpacing::Computed.requested_for(style),
                feature_context.as_ref(),
            );
            builder.build_into(layout, shaped_text);
            layout.break_all_lines(None);
            layout
                .lines()
                .next()
                .map(|line| {
                    visual_ranges_for_line(line)
                        .into_iter()
                        .filter_map(|visual_range| {
                            bidi_text.original_range(visual_range.range).map(|range| {
                                BidiVisualRange {
                                    range,
                                    direction: visual_range.direction,
                                }
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .filter(|ranges| !ranges.is_empty())
                .unwrap_or_else(|| {
                    std::iter::once(BidiVisualRange {
                        range: 0..text.len(),
                        direction: ResolvedBidiDirection::Ltr,
                    })
                    .collect()
                })
        })
    }

    pub(crate) fn shape_text_runs_with_parley(
        &mut self,
        text: &str,
        style: &ComputedStyle,
    ) -> Vec<ShapedGlyphRun> {
        self.shape_text_runs_with_parley_with_letter_spacing(
            text,
            style,
            ShapingLetterSpacing::Computed,
        )
    }

    fn shape_text_runs_with_parley_with_letter_spacing(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        letter_spacing: ShapingLetterSpacing,
    ) -> Vec<ShapedGlyphRun> {
        let emoji_text = text_with_font_variant_emoji(text, style);
        let text = text_without_font_neutral_default_ignorables(emoji_text.as_ref());
        let text = text.as_ref();
        if text.is_empty() {
            return Vec::new();
        }
        // Keep source text for CSS Text processing and ToUnicode extraction,
        // but give the shaping engine its compatibility-normalized glyph
        // selection input. This preserves byte ranges for the line returned by
        // Parley.
        let shaping_text = text_with_shaping_compatibility_normalization(text);
        let shaping_text = shaping_text.as_ref();
        if self
            .unicode_range_resolved_text_spans(text, style)
            .is_some()
        {
            // The styled path retains the authored style separately from the
            // range-selected shaping face. `font-size-adjust: from-font`
            // needs that authored style to obtain its primary metric.
            return self.shape_styled_text_runs_with_parley_with_letter_spacing(
                &[StyledTextSpan { text, style }],
                letter_spacing,
            );
        }
        if text_needs_edge_join_context(text) {
            return self.shape_styled_text_runs_with_parley_with_letter_spacing(
                &[StyledTextSpan { text, style }],
                letter_spacing,
            );
        }
        self.with_reusable_parley_layout(|this, layout| {
            let parley_style = this.shaping_style_for_selected_face(style);
            let feature_context = this.font_feature_context_for_style(style);
            let font_family_source = this
                .emoji_presentation_family_source(text, style)
                .unwrap_or_else(|| this.resolved_parley_font_family_source(style));
            let mut builder: parley::RangedBuilder<'_, FontPalette> = this
                .parley_layout_context
                .ranged_builder(&mut this.parley_font_context, shaping_text, 1.0, false);
            push_parley_default_style(&mut builder, &parley_style, &font_family_source);
            push_parley_text_spacing_default_with_context(
                &mut builder,
                shaping_text,
                style,
                letter_spacing.requested_for(style),
                feature_context.as_ref(),
            );
            builder.build_into(layout, shaping_text);
            layout.break_all_lines(None);
            let Some(line) = layout.lines().next() else {
                return Vec::new();
            };
            let adjustment_ranges =
                this.font_size_adjustment_ranges_for_line(&line, shaping_text, style);
            if !adjustment_ranges.is_empty() {
                let mut builder: parley::RangedBuilder<'_, FontPalette> = this
                    .parley_layout_context
                    .ranged_builder(&mut this.parley_font_context, shaping_text, 1.0, false);
                push_parley_default_style(&mut builder, &parley_style, &font_family_source);
                push_parley_text_spacing_default_with_context(
                    &mut builder,
                    shaping_text,
                    style,
                    letter_spacing.requested_for(style),
                    feature_context.as_ref(),
                );
                for adjustment in &adjustment_ranges {
                    builder.push(
                        StyleProperty::FontSize(adjustment.font_size),
                        adjustment.range.clone(),
                    );
                }
                builder.build_into(layout, shaping_text);
                layout.break_all_lines(None);
                let Some(line) = layout.lines().next() else {
                    return Vec::new();
                };
                return this.rendered_text_runs_for_parley_line(text, line, style);
            }
            this.rendered_text_runs_for_parley_line(text, line, style)
        })
    }

    pub(crate) fn shape_unwrapped_line(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
    ) -> Option<ShapedInlineLine> {
        self.shape_unwrapped_line_with_letter_spacing(
            text,
            style,
            line_height,
            ShapingLetterSpacing::Computed,
        )
    }

    fn shape_unwrapped_line_with_letter_spacing(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
        letter_spacing: ShapingLetterSpacing,
    ) -> Option<ShapedInlineLine> {
        let mut runs = position_shaped_runs(self.shape_text_runs_with_parley_with_letter_spacing(
            text,
            style,
            letter_spacing,
        ));
        self.apply_vertical_upright_advances(&mut runs, style);
        let baseline_adjustment = self
            .shaped_runs_baseline_adjustment(&runs, style, line_height)
            .points();
        let mut shaped = ShapedInlineLine {
            text: Rc::from(text),
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

    /// Shape an inline layout artifact without backend-owned tracking.
    ///
    /// CSS Text resolves nonzero `letter-spacing` at final visual
    /// typographic-unit boundaries. Graph layout therefore retains an
    /// untracked glyph stream and represents every used advance explicitly as
    /// `InlineFragment::leading_tracking`, after line selection and bidi
    /// reordering: <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
    pub(crate) fn shape_untracked_inline_line(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
    ) -> Option<ShapedInlineLine> {
        self.shape_unwrapped_line_with_letter_spacing(
            text,
            style,
            line_height,
            ShapingLetterSpacing::Suppressed,
        )
    }

    /// Shape text whose UAX #9 visual order has already been resolved.
    ///
    /// Mixed inline layout first resolves one complete line, including the
    /// formatting controls contributed by CSS `unicode-bidi` scopes. Its
    /// resulting visual slices must not establish a second paragraph base
    /// direction or a second embedding/isolate/override scope while they are
    /// measured for painting. Re-running the bidi algorithm on such a slice
    /// changes the resolution of neutral characters at the slice edges.
    ///
    /// The caller supplies visual clusters in CSS logical-text order only
    /// where that order is already their display order. An LTR override keeps
    /// the sequence from being reordered again; RTL slices then receive UAX #9
    /// L4 glyph mirroring directly on their selected font glyphs:
    /// <https://www.w3.org/TR/css-writing-modes-4/#bidi-algo> and
    /// <https://www.unicode.org/reports/tr9/#Reordering_Resolved_Levels>.
    pub(crate) fn shape_visual_ordered_line(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
        resolved_direction: ResolvedBidiDirection,
    ) -> Option<ShapedInlineLine> {
        // UAX #9 visual reordering and OpenType's cursive shaping direction
        // are separate inputs. An LTR override preserves the already-resolved
        // order of neutral punctuation, but would make HarfBuzz shape Arabic
        // and other joining scripts left-to-right. Those scripts must retain
        // their logical CSS direction while shaping:
        // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
        // <https://www.unicode.org/reports/tr9/#reordering-resolved-levels>.
        if text.chars().any(character_has_joining_behavior) {
            let logical_paint_style = Self::visual_bidi_paint_style(style, style.used_direction());
            return self.shape_unwrapped_line(text, &logical_paint_style, line_height);
        }
        let visual_paint_style = Self::visual_bidi_paint_style(style, style.used_direction());
        let mut guarded_text = String::with_capacity(text.len() + 2 * '\u{202d}'.len_utf8());
        guarded_text.push('\u{202d}');
        guarded_text.push_str(text);
        guarded_text.push('\u{202c}');
        self.shape_unwrapped_line(&guarded_text, &visual_paint_style, line_height)
            .map(|mut shaped| {
                shaped.text = Rc::from(text);
                strip_bidi_format_controls_from_shaped_runs(&mut shaped.runs);
                self.apply_resolved_bidi_glyph_mirroring(&mut shaped, resolved_direction);
                shaped
            })
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
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
    ) -> Option<ShapedInlineLine> {
        if spans.is_empty() {
            return None;
        }
        let first_style = spans.first().map(|span| span.style)?;
        let mut runs = position_shaped_runs(self.shape_styled_text_runs_with_parley_at_tab_origin(
            spans,
            tab_origin,
            tab_metric_style,
        ));
        self.apply_vertical_upright_advances(&mut runs, first_style);
        let baseline_adjustment = self
            .shaped_runs_baseline_adjustment(&runs, first_style, line_height)
            .points();
        (!runs.is_empty()).then_some(ShapedInlineLine {
            text: text_summary.into(),
            width,
            offset: 0.0,
            aligned_by_parley: false,
            line_height,
            baseline_adjustment,
            runs,
        })
    }

    /// Shape styled text whose visual bidi order has already been resolved.
    ///
    /// The original styles remain the source of every font, OpenType, spacing,
    /// and metric property. An explicit LTR override prevents a second UAX #9
    /// reordering pass, while RTL slices receive L4 glyph mirroring after
    /// shaping because the caller already supplied the final visual order:
    /// <https://www.w3.org/TR/css-writing-modes-4/#bidi-algo>.
    #[allow(clippy::too_many_arguments)] // The explicit shaping context preserves call-site units.
    pub(crate) fn shape_visually_ordered_inline_fragments(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        text_summary: String,
        width: f32,
        line_height: f32,
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
        resolved_direction: ResolvedBidiDirection,
    ) -> Option<ShapedInlineLine> {
        spans.first()?;
        if spans
            .iter()
            .flat_map(|span| span.text.chars())
            .any(character_has_joining_behavior)
        {
            let logical_paint_styles = spans
                .iter()
                .map(|span| Self::visual_bidi_paint_style(span.style, span.style.used_direction()))
                .collect::<Vec<_>>();
            let logical_paint_spans = spans
                .iter()
                .zip(&logical_paint_styles)
                .map(|(span, style)| StyledTextSpan {
                    text: span.text,
                    style,
                })
                .collect::<Vec<_>>();
            let logical_tab_metric_style =
                Self::visual_bidi_paint_style(tab_metric_style, tab_metric_style.used_direction());
            return self.shape_styled_inline_fragments(
                &logical_paint_spans,
                text_summary,
                width,
                line_height,
                tab_origin,
                &logical_tab_metric_style,
            );
        }
        let visual_paint_styles = spans
            .iter()
            .map(|span| Self::visual_bidi_paint_style(span.style, span.style.used_direction()))
            .collect::<Vec<_>>();
        let visual_paint_spans = spans
            .iter()
            .zip(&visual_paint_styles)
            .map(|(span, style)| StyledTextSpan {
                text: span.text,
                style,
            })
            .collect::<Vec<_>>();
        let visual_tab_metric_style =
            Self::visual_bidi_paint_style(tab_metric_style, tab_metric_style.used_direction());
        let first_style = visual_paint_spans.first()?.style;
        let mut guarded_spans = Vec::with_capacity(spans.len() + 2);
        guarded_spans.push(StyledTextSpan {
            text: "\u{202d}",
            style: first_style,
        });
        guarded_spans.extend_from_slice(&visual_paint_spans);
        guarded_spans.push(StyledTextSpan {
            text: "\u{202c}",
            style: first_style,
        });
        let mut shaped = self.shape_styled_inline_fragments(
            &guarded_spans,
            text_summary,
            width,
            line_height,
            tab_origin,
            &visual_tab_metric_style,
        )?;
        strip_bidi_format_controls_from_shaped_runs(&mut shaped.runs);
        self.apply_resolved_bidi_glyph_mirroring(&mut shaped, resolved_direction);
        Some(shaped)
    }

    /// Return the style used to shape text after the containing line has
    /// already resolved CSS bidi scopes through UAX #9.
    ///
    /// The selected visual fragment must retain font and OpenType inputs, but
    /// it must not inject `unicode-bidi` controls a second time. Non-joining
    /// visual slices are guarded with LRO by their caller; joining text keeps
    /// its logical CSS shaping direction while remaining unscoped:
    /// <https://drafts.csswg.org/css-writing-modes-4/#bidi-algo> and
    /// <https://www.unicode.org/reports/tr9/#L4>.
    fn visual_bidi_paint_style(style: &ComputedStyle, direction: Direction) -> ComputedStyle {
        let mut visual_style = style.clone();
        visual_style.unicode_bidi = UnicodeBidi::Normal;
        visual_style.direction = direction;
        visual_style
    }

    /// Apply UAX #9 L4 to an already visually ordered RTL line without
    /// changing its Unicode source text or running UAX #9 a second time.
    ///
    /// Call this exactly once when logical shaping crosses into a selected
    /// visual slice. Cached source slices are shaped before the UBA chooses
    /// their final level, so they require the same presentation correction as
    /// freshly shaped visual slices:
    /// <https://www.unicode.org/reports/tr9/#L4>.
    pub(crate) fn apply_resolved_bidi_glyph_mirroring(
        &self,
        shaped: &mut ShapedInlineLine,
        resolved_direction: ResolvedBidiDirection,
    ) {
        if resolved_direction != ResolvedBidiDirection::Rtl {
            return;
        }
        for run in &mut shaped.runs {
            let Some(font_id) = run.font_id else {
                continue;
            };
            let Some(font) = self.document_fonts.get(font_id) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
                continue;
            };
            let scale = run.font_size / font.units_per_em.max(1) as f32;
            for glyph in &mut run.glyphs {
                if glyph.rendered.is_advance_only() {
                    continue;
                }
                let mut characters = glyph.rendered.unicode.chars();
                let Some(character) = characters.next() else {
                    continue;
                };
                if characters.next().is_some() {
                    continue;
                }
                let Some(mirrored) = bidi_mirroring_glyph(character) else {
                    continue;
                };
                let Some(mirrored_id) = face.glyph_index(mirrored) else {
                    continue;
                };
                let old_nominal = glyph.rendered.nominal_x_advance;
                let extra_advance = glyph.rendered.x_advance - old_nominal;
                let mirrored_nominal = face
                    .glyph_hor_advance(mirrored_id)
                    .map(|advance| advance as f32 * scale)
                    .unwrap_or(old_nominal);
                glyph.rendered.kind = RenderedGlyphKind::Paint(mirrored_id.0);
                glyph.rendered.nominal_x_advance = mirrored_nominal;
                glyph.rendered.x_advance = mirrored_nominal + extra_advance;
            }
        }
    }
}

fn strip_bidi_format_controls_from_shaped_runs(runs: &mut [ShapedInlineRun]) {
    for run in runs {
        run.text = text_without_bidi_format_controls(&run.text)
            .into_owned()
            .into();
    }
}

pub(in crate::text) fn position_shaped_runs(runs: Vec<ShapedGlyphRun>) -> Vec<ShapedInlineRun> {
    runs.into_iter()
        .filter_map(|run| {
            if run.glyphs.is_empty() {
                return None;
            }
            Some(ShapedInlineRun {
                text: run.text,
                x_offset: run.x_offset,
                font_size: run.font_size,
                font_id: run.font_id,
                font_palette: run.font_palette,
                glyphs: run
                    .glyphs
                    .into_iter()
                    .enumerate()
                    .map(|(glyph_index, glyph)| ShapedInlineGlyph {
                        paints: !glyph.is_advance_only(),
                        rendered: glyph,
                        source_range: run.glyph_source_ranges.get(glyph_index).cloned().flatten(),
                    })
                    .collect(),
                paints: true,
            })
        })
        .collect()
}

#[cfg(test)]
pub(in crate::text) fn shaped_line_measure_width(
    line: &ShapedInlineLine,
    style: &ComputedStyle,
) -> f32 {
    let measured_text = trim_trailing_css_hanging_space_separators(&line.text, style);
    let hanging_width = shaped_suffix_advance(line, measured_text.len());
    (line.advance_width()
        - hanging_width
        - line_end_letter_spacing_width(measured_text, style).points())
    .max(0.0)
}

#[cfg(test)]
pub(in crate::text) fn shaped_suffix_advance(line: &ShapedInlineLine, prefix_len: usize) -> f32 {
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
        if glyph.source_text().is_empty() {
            continue;
        }
        if remaining.ends_with(glyph.source_text()) {
            width += glyph.rendered.x_advance;
            remaining = &remaining[..remaining.len() - glyph.source_text().len()];
        }
    }
    if remaining.is_empty() { width } else { 0.0 }
}
