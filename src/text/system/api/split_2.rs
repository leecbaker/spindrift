use super::*;
use crate::units::SemanticLengthExt;
use std::borrow::Cow;

impl FontSystem {
    pub(in crate::text) fn apply_vertical_upright_advances(
        &self,
        runs: &mut [ShapedInlineRun],
        style: &ComputedStyle,
    ) {
        let text_orientation = match style.text_layout_policy() {
            TextLayoutPolicy::Vertical(orientation) => orientation,
            TextLayoutPolicy::Horizontal | TextLayoutPolicy::Sideways(_) => return,
        };
        if matches!(text_orientation, TextOrientation::Sideways) {
            return;
        }

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
            let units_per_em = font.units_per_em.max(1) as f32;
            let scale = run.font_size / units_per_em;
            for glyph in &mut run.glyphs {
                let upright = matches!(text_orientation, TextOrientation::Upright)
                    || (matches!(text_orientation, TextOrientation::Mixed)
                        && crate::text::typographic_unit_is_upright_in_mixed_orientation(
                            glyph.source_text(),
                        ));
                if !upright {
                    continue;
                }
                if glyph.source_text() == "\t" {
                    continue;
                }
                let Some(glyph_id) = glyph.rendered.painted_id() else {
                    continue;
                };
                let glyph_id = ttf_parser::GlyphId(glyph_id);
                let synthesized_vertical_advance =
                    (i32::from(face.ascender()) - i32::from(face.descender())).max(1) as f32;
                // CSS Text gives several Unicode space separators a used
                // advance independent of the selected glyph. In particular,
                // U+3000 IDEOGRAPHIC SPACE is one em even when a font's
                // vertical substitute is a narrow blank glyph. Preserve that
                // CSS advance before consulting OpenType vertical metrics.
                // <https://www.w3.org/TR/css-text-3/#white-space-processing>
                let source_text = glyph.source_text();
                let vertical_advance = (source_text.chars().count() == 1)
                    .then(|| source_text.chars().next())
                    .flatten()
                    .and_then(|character| {
                        css_space_separator_advance(&face, character, run.font_size, scale)
                    })
                    .unwrap_or_else(|| {
                        face.glyph_ver_advance(glyph_id)
                            .map(|advance| advance as f32)
                            .filter(|advance| *advance > 0.0)
                            .unwrap_or(synthesized_vertical_advance)
                            * scale
                    });
                // Parley shapes this stream on a horizontal baseline. CSS
                // Writing Modes instead positions upright glyphs from their
                // OpenType vertical origin. Convert the shaped baseline
                // offset here, while the selected face and glyph metrics are
                // still available; text painting turns it into a run origin
                // because PDF text positioning has no per-glyph y advance.
                //
                // OpenType defines the vertical origin with VORG when
                // present, otherwise from the glyph top side bearing. A font
                // that supplies neither vertical metric has no glyph-specific
                // vertical origin. Its used vertical advance is therefore the
                // stable typographic-unit origin; deriving one from horizontal
                // ink bounds would make the same one-em unit start at a
                // different position for every glyph.
                // <https://learn.microsoft.com/en-us/typography/opentype/spec/vorg>
                // <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>
                let vertical_origin = face
                    .glyph_y_origin(glyph_id)
                    .map(f32::from)
                    .or_else(|| {
                        face.glyph_bounding_box(glyph_id)
                            .zip(face.glyph_ver_side_bearing(glyph_id))
                            .map(|(bounds, top_side_bearing)| {
                                f32::from(bounds.y_max) + f32::from(top_side_bearing)
                            })
                    })
                    .map(|origin| origin * scale)
                    .unwrap_or(vertical_advance);
                glyph.rendered.y_offset -= vertical_origin;
                let extra_spacing =
                    (glyph.rendered.x_advance - glyph.rendered.nominal_x_advance).max(0.0);
                glyph.rendered.nominal_x_advance = vertical_advance;
                glyph.rendered.x_advance = vertical_advance + extra_spacing;
            }
        }
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
    ) -> Vec<ShapedGlyphRun> {
        let Some(tab_metric_style) = spans.first().map(|span| span.style) else {
            return Vec::new();
        };
        self.shape_styled_text_runs_with_parley_at_tab_origin(spans, 0.0, tab_metric_style)
    }

    /// Shape styled text with tabs measured from a CSS line's content edge.
    ///
    /// Preserved tabs advance to stops measured from the block content edge,
    /// not from text alignment, indentation, or a fragment boundary:
    /// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>.
    pub(crate) fn shape_styled_text_runs_with_parley_at_tab_origin(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
    ) -> Vec<ShapedGlyphRun> {
        if spans.is_empty() {
            return Vec::new();
        }
        let mut text = String::new();
        // Expand each authored inline span through the same `unicode-range`
        // face selection used by the single-style shaping path. Styled inline
        // layout is the normal route for DOM text, so applying the split only
        // in `shape_text_runs_with_parley` lets Parley select an unrestricted
        // registered face here instead.
        // <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>
        let mut unicode_range_spans = Vec::<(String, ComputedStyle)>::new();
        for span in spans.iter().filter(|span| !span.text.is_empty()) {
            if let Some(resolved_spans) =
                self.unicode_range_resolved_text_spans(span.text, span.style)
            {
                for resolved in resolved_spans {
                    if let Some(text) = span.text.get(resolved.range) {
                        unicode_range_spans.push((text.to_string(), resolved.style));
                    }
                }
            } else {
                unicode_range_spans.push((span.text.to_string(), span.style.clone()));
            }
        }
        let spans = unicode_range_spans
            .iter()
            .map(|(text, style)| StyledTextSpan {
                text: text.as_str(),
                style,
            })
            .collect::<Vec<_>>();
        let mut ranges: Vec<(Range<usize>, &ComputedStyle)> = Vec::with_capacity(spans.len());
        let mut synthetic_join_controls = Vec::new();
        let spans = spans
            .iter()
            .filter(|span| !span.text.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        let authored_text = spans.iter().map(|span| span.text).collect::<String>();
        // Join controls are default-ignorable shaping instructions, not
        // visible font content. Their own inline style must therefore not
        // create a separate font-selection range: an author may isolate a
        // U+200C/U+200D in a span with a deliberately unrelated fallback
        // face, while the adjacent Arabic text still shapes as one context.
        // Keep each control in the input stream but assign it to an adjacent
        // visible span for font selection.
        // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
        // <https://www.w3.org/TR/alreq/#h_joining-enforcement>
        let mut shaping_spans = Vec::<(String, &ComputedStyle)>::new();
        let mut pending_join_controls = String::new();
        for span in &spans {
            let span_text = text_without_font_neutral_default_ignorables(span.text);
            let span_text = span_text.as_ref();
            if !span_text.is_empty() && span_text.chars().all(character_is_join_control) {
                if let Some((previous_text, _)) = shaping_spans.last_mut() {
                    previous_text.push_str(span_text);
                } else {
                    pending_join_controls.push_str(span_text);
                }
                continue;
            }
            let mut text_with_pending = std::mem::take(&mut pending_join_controls);
            text_with_pending.push_str(span_text);
            if let Some((previous_text, previous_style)) = shaping_spans.last_mut()
                && *previous_style == span.style
            {
                previous_text.push_str(&text_with_pending);
            } else {
                shaping_spans.push((text_with_pending, span.style));
            }
        }
        if !pending_join_controls.is_empty()
            && let Some((previous_text, _)) = shaping_spans.last_mut()
        {
            previous_text.push_str(&pending_join_controls);
        }
        for (index, (span_text, style)) in shaping_spans.iter().enumerate() {
            let start = text.len();
            if index > 0
                && shaping_spans
                    .get(index - 1)
                    .is_some_and(|(previous, previous_style)| {
                        previous_style != style
                            && span_boundary_needs_join_control(previous, span_text)
                    })
            {
                push_synthetic_join_control(&mut text, &mut synthetic_join_controls);
            }
            push_text_with_font_variant_emoji(&mut text, span_text, style);
            if shaping_spans
                .get(index + 1)
                .is_some_and(|(next, next_style)| {
                    next_style != style && span_boundary_needs_join_control(span_text, next)
                })
            {
                push_synthetic_join_control(&mut text, &mut synthetic_join_controls);
            }
            ranges.push((start..text.len(), style));
        }
        if text.is_empty() || ranges.is_empty() {
            return Vec::new();
        }
        push_edge_join_context(&mut text, &mut ranges, &mut synthetic_join_controls);
        let source_positions =
            styled_text_source_positions(&text, &authored_text, &synthetic_join_controls);
        // The normalized text is only the input to Parley's glyph selection.
        // `text` remains the authored stream used for cluster-to-source
        // mapping, line breaking, and PDF extraction; the normalization above
        // preserves every byte range.
        let shaping_text = text_with_shaping_compatibility_normalization(&text);
        let shaping_text = shaping_text.as_ref();
        let default_style = ranges[0].1;
        self.with_reusable_parley_layout(|this, layout| {
            let parley_styles = ranges
                .iter()
                .map(|(_, style)| this.shaping_style_for_selected_face(style))
                .collect::<Vec<_>>();
            let default_parley_style = &parley_styles[0];
            let default_feature_context = this.font_feature_context_for_style(default_style);
            let feature_contexts = ranges
                .iter()
                .map(|(_, style)| this.font_feature_context_for_style(style))
                .collect::<Vec<_>>();
            let mut font_family_sources = Vec::<String>::with_capacity(ranges.len());
            for (range, style) in &ranges {
                let selected = this
                    .emoji_presentation_family_source(&text[range.clone()], style)
                    .unwrap_or_else(|| this.resolved_parley_font_family_source(style));
                // A closing bidi formatting control has no glyph or family
                // choice of its own. Keeping the preceding selected source
                // prevents it from merging the final visible cluster into an
                // enclosing element's unselected authored stack.
                // <https://www.w3.org/TR/css-writing-modes-4/#bidi-algo>
                if text[range.clone()]
                    .chars()
                    .all(character_is_bidi_format_control)
                    && let Some(previous) = font_family_sources.last()
                {
                    font_family_sources.push(previous.clone());
                } else {
                    font_family_sources.push(selected);
                }
            }
            // RangedBuilder uses the default properties at the zero-length
            // boundary before applying the first explicit range. Keep the
            // default family source in sync with the first selected range so
            // a presentation-selected emoji at byte zero does not retain the
            // authored stack's first face.
            let default_font_family_source = font_family_sources[0].clone();
            let mut builder: parley::RangedBuilder<'_, FontPalette> = this
                .parley_layout_context
                .ranged_builder(&mut this.parley_font_context, shaping_text, 1.0, false);
            push_parley_default_style(
                &mut builder,
                default_parley_style,
                &default_font_family_source,
            );
            push_parley_text_spacing_default_with_context(
                &mut builder,
                shaping_text,
                default_style,
                default_feature_context.as_ref(),
            );
            for ((((range, style), parley_style), feature_context), font_family_source) in ranges
                .iter()
                .zip(&parley_styles)
                .zip(&feature_contexts)
                .zip(&font_family_sources)
            {
                push_parley_style_range(
                    &mut builder,
                    parley_style,
                    font_family_source,
                    range.clone(),
                );
                push_parley_text_spacing_range_with_context(
                    &mut builder,
                    &shaping_text[range.clone()],
                    style,
                    range.clone(),
                    feature_context.as_ref(),
                );
            }
            builder.build_into(layout, shaping_text);
            layout.break_all_lines(None);
            let Some(line) = layout.lines().next() else {
                return Vec::new();
            };
            let adjustment_ranges = this.styled_font_size_adjustment_ranges_for_line(
                &line,
                shaping_text,
                &ranges,
                default_style,
            );
            if !adjustment_ranges.is_empty() {
                let mut builder: parley::RangedBuilder<'_, FontPalette> = this
                    .parley_layout_context
                    .ranged_builder(&mut this.parley_font_context, shaping_text, 1.0, false);
                push_parley_default_style(
                    &mut builder,
                    default_parley_style,
                    &default_font_family_source,
                );
                push_parley_text_spacing_default_with_context(
                    &mut builder,
                    shaping_text,
                    default_style,
                    default_feature_context.as_ref(),
                );
                for ((((range, style), parley_style), feature_context), font_family_source) in
                    ranges
                        .iter()
                        .zip(&parley_styles)
                        .zip(&feature_contexts)
                        .zip(&font_family_sources)
                {
                    push_parley_style_range(
                        &mut builder,
                        parley_style,
                        font_family_source,
                        range.clone(),
                    );
                    push_parley_text_spacing_range_with_context(
                        &mut builder,
                        &shaping_text[range.clone()],
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
                builder.build_into(layout, shaping_text);
                layout.break_all_lines(None);
            }
            let Some(line) = layout.lines().next() else {
                return Vec::new();
            };
            let run_count = line.runs().size_hint().0;
            let mut rendered_runs = Vec::with_capacity(run_count);
            let mut tab_contexts = Vec::with_capacity(run_count);
            let mut dropped_default_ignorable_runs = Vec::new();
            let mut rehomed_control_fallback_runs = Vec::new();
            for run in line.runs() {
                let run_range = run.text_range();
                let raw_run_text = text.get(run_range.clone()).unwrap_or_default();
                let control_fallback_cluster = classify_control_fallback_cluster(
                    raw_run_text,
                    run.visual_clusters()
                        .flat_map(|cluster| cluster.glyphs())
                        .any(|glyph| glyph.x != 0.0 || glyph.y != 0.0),
                );
                let run_text_without_synthetic = text_without_synthetic_join_controls(
                    &text,
                    run_range.clone(),
                    &synthetic_join_controls,
                );
                let run_text = match text_without_variation_selectors(&run_text_without_synthetic) {
                    Cow::Borrowed(_) => run_text_without_synthetic,
                    Cow::Owned(text) => text,
                };
                let run_style =
                    style_for_text_range(&ranges, run_range.clone()).unwrap_or(default_style);
                let x_offset = run
                    .visual_clusters()
                    .next()
                    .and_then(|cluster| cluster.visual_offset())
                    .unwrap_or(0.0);
                if control_fallback_cluster == ControlFallbackCluster::DropControlOnly {
                    dropped_default_ignorable_runs.push(DroppedDefaultIgnorableRun {
                        x_offset,
                        advance: run.advance(),
                        text: run_text.clone().into(),
                    });
                    continue;
                }
                let fallback_character =
                    parley_run_fallback_character(shaping_text, run_range.clone());
                let Some(font_id) = this.document_font_from_parley_font_data_for_style(
                    run.font(),
                    run_style,
                    fallback_character,
                ) else {
                    continue;
                };
                if let ControlFallbackCluster::RehomeSimpleVisibleFragment { character } =
                    control_fallback_cluster
                    && let Some((rehomed_run, dropped_advance)) = this.rehome_control_fallback_run(
                        run_style,
                        ControlFallbackRehomeRequest {
                            character,
                            fallback_font_id: font_id,
                            text: run_text.clone().into(),
                            font_size: run.font_size(),
                            x_offset,
                            parley_advance: run.advance(),
                            source_range: styled_cluster_source_range(
                                run_range.clone(),
                                &source_positions,
                            ),
                        },
                    )
                {
                    if dropped_advance != 0.0 {
                        dropped_default_ignorable_runs.push(DroppedDefaultIgnorableRun {
                            x_offset,
                            advance: dropped_advance,
                            text: Rc::from(""),
                        });
                    }
                    rehomed_control_fallback_runs.push(rendered_runs.len());
                    rendered_runs.push(rehomed_run);
                    tab_contexts.push(RenderedRunTabContext {
                        style: run_style,
                        metric_style: tab_metric_style,
                    });
                    continue;
                }
                if !raw_run_text.contains('\t')
                    && this.document_fonts.support_kind_for_run(font_id, &run_text)
                        == FontSupportKind::ColorOrEmojiOnlyFallback
                    && !this
                        .document_fonts
                        .run_has_color_glyph(font_id, raw_run_text)
                    && let Some(fallback_font_id) =
                        this.visible_text_fallback_for_run(&run_text, run_style, font_id)
                    && let Some(fallback_font) = this.document_fonts.get(fallback_font_id)
                    && let Some(glyphs) = shape_text_with_document_font(
                        fallback_font,
                        raw_run_text,
                        run.font_size(),
                        run_style.used_letter_spacing().points(),
                        run_style.used_word_spacing().points(),
                    )
                    && !glyphs.is_empty()
                {
                    let glyphs = glyphs_without_synthetic_join_controls(
                        glyphs,
                        raw_run_text,
                        run_range.start,
                        &synthetic_join_controls,
                    );
                    let glyph_source_ranges = vec![None; glyphs.len()];
                    rendered_runs.push(ShapedGlyphRun {
                        text: run_text.into(),
                        x_offset,
                        y_offset: 0.0,
                        text_matrix: crate::RenderedTextMatrix::IDENTITY,
                        font_size: run.font_size(),
                        font_id: Some(fallback_font_id),
                        font_palette: run_style.font_palette.clone(),
                        glyphs,
                        glyph_source_ranges,
                    });
                    tab_contexts.push(RenderedRunTabContext {
                        style: run_style,
                        metric_style: tab_metric_style,
                    });
                    continue;
                }
                let Some(font) = this.document_fonts.get(font_id) else {
                    continue;
                };
                let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
                    continue;
                };
                let units_per_em = font.units_per_em.max(1) as f32;
                let scale = run.font_size() / units_per_em;
                // A styled Parley run may include authored default-ignorable
                // controls which deliberately emit no Unicode on any glyph.
                // Remember the output boundary so that an unsplit paint run
                // can retain its complete logical source text below.
                let rendered_run_start = rendered_runs.len();
                let mut glyphs = Vec::new();
                let mut glyph_source_ranges = Vec::new();
                // Parley keeps paint-only style transitions on glyphs rather
                // than splitting the underlying shaping run. Retain the
                // palette selected by each cluster so CSS `font-palette`
                // remains independent for adjacent inline elements.
                let mut glyph_palettes = Vec::new();
                for cluster in run.visual_clusters() {
                    let cluster_range = cluster.text_range();
                    let cluster_palette = cluster.first_style().brush.clone();
                    let source_range =
                        styled_cluster_source_range(cluster_range.clone(), &source_positions);
                    if range_is_synthetic_only(cluster_range.clone(), &synthetic_join_controls) {
                        continue;
                    }
                    let raw_cluster_text = text.get(cluster_range.clone()).unwrap_or_default();
                    let cluster_text_without_synthetic = text_without_synthetic_join_controls(
                        &text,
                        cluster_range.clone(),
                        &synthetic_join_controls,
                    );
                    let cleaned_cluster_text =
                        text_without_glyph_output_controls(&cluster_text_without_synthetic);
                    let default_ignorable_only = cluster_is_default_ignorable_only(
                        raw_cluster_text,
                        cleaned_cluster_text.as_ref(),
                    );
                    if default_ignorable_only
                        && !default_ignorable_cluster_has_shaping_glyph(
                            &face,
                            raw_run_text,
                            cleaned_cluster_text.as_ref(),
                            cluster.glyphs().filter_map(|glyph| {
                                u16::try_from(glyph.id)
                                    .ok()
                                    .map(|glyph_id| (glyph_id, glyph.advance))
                            }),
                        )
                    {
                        continue;
                    }
                    if cleaned_cluster_text.as_ref() == "\t" {
                        let provisional_advance = cluster.glyphs().map(|glyph| glyph.advance).sum();
                        glyphs.push(synthesized_tab_glyph(provisional_advance));
                        glyph_source_ranges.push(source_range);
                        glyph_palettes.push(cluster_palette);
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
                                cleaned_cluster_text.as_ref().to_owned()
                            }
                        } else {
                            String::new()
                        };
                        if glyph_is_non_painting_shaping_artifact(
                            &face,
                            glyph_id,
                            glyph.advance,
                            &unicode,
                        ) {
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
                        let emitted_glyph_id = if matches!(
                            run_style.text_layout_policy(),
                            TextLayoutPolicy::Vertical(_)
                        ) {
                            // Preserve the selected vertical alternate rather
                            // than replacing it with the horizontal U+0020
                            // glyph. See the equivalent single-style shaping
                            // path for the CSS Writing Modes rationale.
                            glyph_id
                        } else {
                            unicode
                                .chars()
                                .next()
                                .filter(|_| unicode.chars().count() == 1)
                                .and_then(|character| {
                                    css_space_separator_blank_glyph(&face, character)
                                })
                                .map(|glyph| glyph.0)
                                .unwrap_or(glyph_id)
                        };
                        first_cluster_glyph = false;
                        glyphs.push(RenderedGlyph {
                            kind: RenderedGlyphKind::Paint(emitted_glyph_id),
                            x_advance: glyph.advance,
                            nominal_x_advance: face
                                .glyph_hor_advance(ttf_parser::GlyphId(emitted_glyph_id))
                                .map(|advance| advance as f32 * scale)
                                .unwrap_or(glyph.advance),
                            x_offset: glyph.x,
                            y_offset: -glyph.y,
                            unicode,
                        });
                        glyph_source_ranges.push(source_range.clone());
                        glyph_palettes.push(cluster_palette.clone());
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
                debug_assert_eq!(glyphs.len(), glyph_source_ranges.len());
                debug_assert_eq!(glyphs.len(), glyph_palettes.len());
                let mut group_palette = glyph_palettes[0].clone();
                let mut group_glyphs = Vec::new();
                let mut group_source_ranges = Vec::new();
                let mut group_text = String::new();
                let mut group_x_offset = x_offset;
                let mut next_x_offset = x_offset;
                let mut push_group = |glyphs: &mut Vec<RenderedGlyph>,
                                      source_ranges: &mut Vec<Option<Range<usize>>>,
                                      text: &mut String,
                                      palette: &FontPalette,
                                      x_offset| {
                    if glyphs.is_empty() {
                        return;
                    }
                    rendered_runs.push(ShapedGlyphRun {
                        text: std::mem::take(text).into(),
                        x_offset,
                        y_offset: 0.0,
                        text_matrix: crate::RenderedTextMatrix::IDENTITY,
                        font_size,
                        font_id: Some(font_id),
                        font_palette: palette.clone(),
                        glyphs: std::mem::take(glyphs),
                        glyph_source_ranges: std::mem::take(source_ranges),
                    });
                    tab_contexts.push(RenderedRunTabContext {
                        style: run_style,
                        metric_style: tab_metric_style,
                    });
                };
                for ((glyph, source_range), palette) in glyphs
                    .into_iter()
                    .zip(glyph_source_ranges)
                    .zip(glyph_palettes)
                {
                    if palette != group_palette {
                        push_group(
                            &mut group_glyphs,
                            &mut group_source_ranges,
                            &mut group_text,
                            &group_palette,
                            group_x_offset,
                        );
                        group_palette = palette.clone();
                        group_x_offset = next_x_offset;
                    }
                    next_x_offset += glyph.x_advance;
                    group_text.push_str(&glyph.unicode);
                    group_glyphs.push(glyph);
                    group_source_ranges.push(source_range);
                }
                push_group(
                    &mut group_glyphs,
                    &mut group_source_ranges,
                    &mut group_text,
                    &group_palette,
                    group_x_offset,
                );
                // Glyph Unicode is a PDF ToUnicode summary, not the complete
                // CSS source stream: U+200C/U+200D affect shaping but have no
                // standalone visible glyph. When palette grouping leaves this
                // Parley run intact, keep its authored (synthetic controls
                // already removed) text on the shaped run for extraction and
                // diagnostics.
                if run_text.chars().any(character_is_join_control)
                    && rendered_runs.len() == rendered_run_start + 1
                {
                    rendered_runs[rendered_run_start].text = run_text.into();
                }
            }
            for run in &mut rendered_runs {
                run.x_offset =
                    corrected_visual_run_x_offset(run.x_offset, &dropped_default_ignorable_runs);
            }
            stitch_dropped_join_control_runs(&mut rendered_runs, &dropped_default_ignorable_runs);
            for index in rehomed_control_fallback_runs.into_iter().rev() {
                if stitch_rehomed_control_fallback_run(&mut rendered_runs, index) {
                    tab_contexts.remove(index + 1);
                }
            }
            this.apply_css_tab_stops(&mut rendered_runs, &tab_contexts, tab_origin);
            rendered_runs
        })
    }

    /// Resolve an explicit named emoji family stack by the effective Unicode
    /// presentation selector. Both monochrome and color fonts can cover the
    /// same base scalar, so ordinary cmap fallback alone cannot implement
    /// `font-variant-emoji`.
    /// <https://www.w3.org/TR/css-fonts-4/#font-variant-emoji-prop>
    pub(in crate::text) fn emoji_presentation_family_source(
        &mut self,
        text: &str,
        style: &ComputedStyle,
    ) -> Option<String> {
        let names = match &style.font_family {
            FontFamily::Names(names) => names.clone(),
            FontFamily::List(families) => families
                .iter()
                .flat_map(|family| match family {
                    FontFamily::Names(names) => names.clone(),
                    _ => Vec::new(),
                })
                .collect(),
            _ => Vec::new(),
        };
        if names.len() < 2 {
            return None;
        }
        // CSS bidi isolation may wrap the first authored scalar in a
        // directional formatting control. That control participates in UAX
        // #9 but cannot select an emoji face; retain variation selectors so
        // their presentation override remains observable.
        let mut characters = text
            .chars()
            .filter(|character| !character_is_bidi_format_control(*character))
            .peekable();
        let base = characters.next()?;
        if !character_is_emoji(base) {
            return None;
        }
        let requested_color = match characters.peek().copied() {
            Some('\u{fe0f}') => true,
            Some('\u{fe0e}') => false,
            _ => match style.font_variant_emoji {
                FontVariantEmoji::Emoji => true,
                FontVariantEmoji::Text => false,
                FontVariantEmoji::Normal | FontVariantEmoji::Unicode => {
                    character_has_emoji_presentation(base)
                }
            },
        };
        names.into_iter().find_map(|name| {
            let font_id = self.resolve_single_family(
                &name,
                style.font_weight,
                style.font_style,
                style.font_width,
            )?;
            let is_color_face = self
                .document_fonts
                .run_has_color_glyph(font_id, &base.to_string());
            (self.document_fonts.font_has_character(font_id, base)
                && is_color_face == requested_color)
                .then(|| parley_font_family_source(&FontFamily::Names(vec![name])))
        })
    }

    pub(in crate::text) fn visible_text_fallback_for_run(
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
    /// periodic tab stop. A tab's own computed `tab-size` selects the period,
    /// while numeric values are multiples of the nearest block container's
    /// U+0020 advance; length values are absolute computed lengths:
    /// <https://www.w3.org/TR/css-text-3/#tab-size-property>.
    pub(in crate::text) fn apply_css_tab_stops(
        &mut self,
        runs: &mut [ShapedGlyphRun],
        contexts: &[RenderedRunTabContext<'_>],
        tab_origin: f32,
    ) {
        if !runs.iter().any(|run| run.text.contains('\t')) {
            return;
        }

        let mut logical_line_cursor = 0.0;
        for run_index in 0..runs.len() {
            let Some(context) = contexts.get(run_index) else {
                continue;
            };
            let space_advance = self.css_tab_stop_space_advance(context.metric_style);
            let tab_period = context
                .style
                .tab_size
                .used_tab_stop_advance(space_advance.points())
                .points();
            let run_x_offset = runs[run_index].x_offset;
            // A visual shaping pass can split immediately before a preserved
            // tab and give that later run an origin of zero. Carry the
            // selected line's cursor across that split; otherwise the tab is
            // incorrectly treated as leading and advances a whole period.
            // <https://www.w3.org/TR/css-text-3/#tab-size-property>
            let tab_run_x_offset = if run_x_offset <= 0.01 && logical_line_cursor > 0.01 {
                logical_line_cursor
            } else {
                run_x_offset
            };
            // Parley reports a run beginning with a tab after its provisional
            // tab glyph advance. CSS instead resolves the tab from the cursor
            // before that glyph, at the block content edge for a leading tab.
            // Subsequent tabs in the same run retain the advances accumulated
            // from their preceding glyphs.
            let starts_with_tab = runs[run_index]
                .text
                .chars()
                .find(|character| !character_is_bidi_format_control(*character))
                .filter(|character| *character == '\t')
                .is_some();
            let mut pen_x = if starts_with_tab && logical_line_cursor <= 0.01 {
                -tab_run_x_offset
            } else {
                0.0
            };
            let mut following_run_shift = 0.0;

            for glyph in &mut runs[run_index].glyphs {
                if glyph.unicode == "\t" {
                    let old_advance = glyph.x_advance;
                    let used_advance =
                        tab_stop_advance(tab_period, tab_origin + tab_run_x_offset + pen_x)
                            .points();
                    glyph.x_advance = used_advance;
                    glyph.nominal_x_advance = space_advance.points();
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
            logical_line_cursor = logical_line_cursor.max(
                tab_run_x_offset
                    + runs[run_index]
                        .glyphs
                        .iter()
                        .map(|glyph| glyph.x_advance)
                        .sum::<f32>(),
            );
        }
    }

    fn css_tab_stop_space_advance(&mut self, style: &ComputedStyle) -> LayoutLength {
        let direct_advance = self.character_font_match(style, ' ').and_then(|matched| {
            let (data, face_index, units_per_em) = self
                .document_fonts
                .get(matched.font_id)
                .map(|font| (font.data.clone(), font.face_index, font.units_per_em))?;
            let face = ttf_parser::Face::parse(&data, face_index).ok()?;
            let advance = face.glyph_hor_advance(matched.glyph_id.raw())? as f32;
            let used_font_size = self
                .font_size_adjusted_size_for_font_id(style, matched.font_id)
                .unwrap_or(style.font_size);
            Some(advance * used_font_size / units_per_em.max(1) as f32)
        });
        layout_pt(
            direct_advance
                .or_else(|| {
                    self.shape_unwrapped_line(" ", style, style.line_height)
                        .map(|line| line.advance_width())
                })
                .filter(|advance| *advance > 0.0 && advance.is_finite())
                .unwrap_or(style.font_size * 0.25)
                + style.used_letter_spacing().points()
                + style.used_word_spacing().points(),
        )
    }

    pub(crate) fn layout_to_program_baseline_adjustment(
        &mut self,
        font_id: Option<usize>,
        style: &ComputedStyle,
        _line_height: f32,
    ) -> LayoutLength {
        let used_font_size = font_id
            .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
            .unwrap_or(style.font_size);
        self.layout_to_program_baseline_adjustment_for_font_size(font_id, style, used_font_size)
    }

    /// Return the paint-origin adjustment for a shaped line's CSS baseline.
    ///
    /// CSS 2.2 positions inline boxes from the metrics of the style's first
    /// available font; fallback glyph runs must not move the `line-height`
    /// box baseline, even though their glyphs and advances are preserved for
    /// painting:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
    pub(in crate::text) fn shaped_runs_baseline_adjustment(
        &mut self,
        _runs: &[ShapedInlineRun],
        style: &ComputedStyle,
        line_height: f32,
    ) -> LayoutLength {
        let font_id = self.resolve_metric_font_for_style(style);
        self.layout_to_program_baseline_adjustment(font_id, style, line_height)
    }

    /// Convert a CSS layout baseline to the selected font program's glyph origin.
    ///
    /// CSS metric overrides belong to `layout_metrics`; the embedded OpenType
    /// program retains `program_metrics`.
    pub(crate) fn layout_to_program_baseline_adjustment_for_font_size(
        &self,
        font_id: Option<usize>,
        _style: &ComputedStyle,
        used_font_size: f32,
    ) -> LayoutLength {
        let Some(font) = font_id.and_then(|id| self.document_fonts.get(id)) else {
            return layout_pt(0.0);
        };
        // CSS layout anchors a line to its resolved layout metrics, including
        // `@font-face` metric overrides. PDF glyph programs instead retain
        // their native coordinates. Both ascents use the selected face's used
        // size, including `font-size-adjust`; convert only between those two
        // coordinate systems. Using the em-box top would incorrectly raise
        // glyphs for faces whose native ascender is shorter than one em.
        // <https://www.w3.org/TR/css-fonts-4/#font-metrics>
        // <https://www.w3.org/TR/CSS22/visudet.html#line-height>
        layout_pt(layout_to_program_ascent_delta(font, used_font_size))
    }

    /// Return the rendered first-line text baseline offset from line-box top.
    ///
    /// CSS Inline Layout aligns inline-level boxes to line baselines. Formatting
    /// contexts that synthesize baselines use CSS layout metrics. Metric
    /// override descriptors change this baseline without changing the native
    /// glyph-coordinate adjustment used by text painting:
    /// <https://www.w3.org/TR/css-inline-3/#line-box> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>.
    pub(crate) fn rendered_first_line_baseline_offset(
        &mut self,
        style: &ComputedStyle,
    ) -> LayoutLength {
        let font_id = self.resolve_metric_font_for_style(style);
        let used_font_size = font_id
            .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
            .unwrap_or(style.font_size);
        let ascent = font_id
            .and_then(|font_id| self.document_fonts.get(font_id))
            .map(|font| {
                font.layout_metrics.ascender as f32 * used_font_size
                    / font.units_per_em.max(1) as f32
            })
            .unwrap_or(style.font_size);
        layout_pt(ascent)
    }

    /// Return the selected font's x-height in layout units when the font can
    /// provide or synthesize one from glyph ink.
    ///
    /// CSS 2.2 `vertical-align: middle` aligns against half of the parent's
    /// x-height, and CSS Inline uses x-height for the `ex` text edge. Font
    /// metrics are preferred, with the glyph bounding box for `x` as a
    /// selected-font fallback:
    /// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align> and
    /// <https://drafts.csswg.org/css-inline-3/#text-edges>.
    pub(crate) fn x_height_for_style(&mut self, style: &ComputedStyle) -> Option<LayoutLength> {
        // A face excluded from ordinary text by `unicode-range` is not the
        // first available font for CSS font-relative metrics. Space provides
        // a stable ordinary-text selection without requiring an `x` glyph to
        // be present in the candidate face.
        // <https://www.w3.org/TR/css-values-4/#ex>
        let font_id = self.resolve_metric_font_for_style(style);
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
        Some(layout_pt(height * used_font_size / units_per_em))
    }

    /// Return the used x-height for CSS layout, synthesizing from `0.5em` when
    /// no selected font metric or representative glyph ink box is available.
    ///
    /// CSS Inline permits synthesized text-edge metrics when the selected font
    /// cannot provide the requested data:
    /// <https://drafts.csswg.org/css-inline-3/#text-edges>.
    pub(crate) fn used_x_height_for_style(&mut self, style: &ComputedStyle) -> LayoutLength {
        self.x_height_for_style(style)
            .unwrap_or_else(|| layout_pt(style.font_size * 0.5))
    }

    /// Return the selected font's cap-height in layout units when the font can
    /// provide or synthesize one from glyph ink.
    ///
    /// CSS Inline text-edge metrics use cap-height for the `cap` over-edge.
    /// OpenType `sCapHeight`/font parser metadata is preferred, with the
    /// glyph bounding box for `H` as a fallback:
    /// <https://drafts.csswg.org/css-inline-3/#text-edges>.
    pub(crate) fn cap_height_for_style(&mut self, style: &ComputedStyle) -> Option<LayoutLength> {
        let font_id = self.resolve_metric_font_for_style(style);
        let used_font_size = font_id
            .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
            .unwrap_or(style.font_size);
        let font = font_id.and_then(|id| self.document_fonts.get(id))?;
        let units_per_em = font.units_per_em.max(1) as f32;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        // `FontRecord::cap_height` falls back to ascender metadata for uses
        // that need a broad font extent. CSS `cap`, however, specifically
        // requires cap-height; when OpenType does not expose `sCapHeight`,
        // measure a representative capital rather than treating ascender as a
        // cap metric.
        // <https://www.w3.org/TR/css-values-4/#cap>
        let height = face
            .capital_height()
            .map(|height| height as f32)
            .filter(|height| *height > 0.0)
            .or_else(|| glyph_bbox_height(&face, 'H'))?;
        Some(layout_pt(height * used_font_size / units_per_em))
    }

    /// Return the used cap-height for CSS Inline layout, synthesizing from
    /// `0.7em` when no selected font metric or representative glyph ink box is
    /// available.
    ///
    /// CSS Inline permits synthesized text-edge metrics when the selected font
    /// cannot provide the requested data:
    /// <https://drafts.csswg.org/css-inline-3/#text-edges>.
    pub(crate) fn used_cap_height_for_style(&mut self, style: &ComputedStyle) -> LayoutLength {
        self.cap_height_for_style(style)
            .unwrap_or_else(|| layout_pt(style.font_size * 0.7))
    }

    /// Return the selected fallback font's representative ideographic ink
    /// extents around the baseline in layout units.
    ///
    /// CSS Inline's `ideographic-ink` text edge is an ink edge for ideographic
    /// glyphs. Quire synthesizes it from the OpenType bounding box of U+6C34
    /// WATER shaped through the normal CSS Fonts fallback stack, falling back
    /// to the ideographic em edge when no such glyph box is available:
    /// <https://drafts.csswg.org/css-inline-3/#text-edges>.
    pub(crate) fn ideographic_ink_extents_for_style(
        &mut self,
        style: &ComputedStyle,
    ) -> Option<FontRunVerticalExtents> {
        let shaped = self.shape_unwrapped_line("水", style, style.line_height)?;
        let run = shaped
            .runs
            .iter()
            .find(|run| run.paints && run.text.contains('水'))?;
        let font_id = run.font_id?;
        let font = self.document_fonts.get(font_id)?;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        let glyph = run
            .glyphs
            .iter()
            .find(|glyph| glyph.source_text() == "水")?;
        let bbox = face.glyph_bounding_box(ttf_parser::GlyphId(glyph.rendered.painted_id()?))?;
        let scale = run.font_size / font.units_per_em.max(1) as f32;
        let above = (bbox.y_max as f32 * scale).max(0.0);
        let below = (-bbox.y_min as f32 * scale).max(0.0);
        (above.is_finite() && below.is_finite() && above + below > 0.0)
            .then_some(FontRunVerticalExtents::from_points(above, below))
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
        let font_id = self.resolve_metric_font_for_style(style);
        let used_font_size = font_id
            .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
            .unwrap_or(style.font_size);
        let font = font_id.and_then(|id| self.document_fonts.get(id))?;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        let units_per_em = font.units_per_em.max(1) as f32;
        match baseline_shift {
            BaselineShift::Super => {
                let metric = face.superscript_metrics()?.y_offset.unsigned_abs() as f32
                    * used_font_size
                    / units_per_em;
                Some(metric.max(style.font_size * 0.45))
            }
            BaselineShift::Sub => {
                let metric = face.subscript_metrics()?.y_offset.unsigned_abs() as f32
                    * used_font_size
                    / units_per_em;
                Some(-metric.max(style.font_size * 0.4))
            }
            _ => None,
        }
    }

    /// Convert a rendered PDF text line back to the CSS line alignment coordinate.
    ///
    /// CSS 2.2 positions inline content using line-box font metrics, while the
    /// PDF backend stores text after applying the font ascent adjustment used
    /// for glyph emission. This helper reverses that adjustment for layout
    /// code that must align atomic inline fragments to shaped text.
    /// https://www.w3.org/TR/CSS22/visudet.html#line-height
    #[cfg(test)]
    pub(crate) fn rendered_line_alignment_y(&self, line: &RenderedLine) -> LayoutLength {
        layout_pt(line.y() + line.font_size - line.glyph_origin_adjustment().y)
    }
}

/// Return the signed distance from a CSS layout baseline to a native glyph
/// program baseline at one used font size.
///
/// <https://www.w3.org/TR/css-fonts-4/#font-metrics>
fn layout_to_program_ascent_delta(font: &DocumentFont, used_font_size: f32) -> f32 {
    let units_per_em = font.units_per_em.max(1) as f32;
    let layout_ascent = font.layout_metrics.ascender as f32 * used_font_size / units_per_em;
    let program_ascent = font.program_metrics.ascender as f32 * used_font_size / units_per_em;
    layout_ascent - program_ascent
}

#[derive(Debug, Clone)]
struct StyledTextSourcePosition {
    internal: Range<usize>,
    authored: Range<usize>,
}

/// Map Parley's augmented styled-shaping buffer back to authored text.
///
/// Styled shaping inserts join controls and edge context solely for OpenType
/// shaping. Their byte positions are not positions in the CSS text stream,
/// which is retained separately for line selection and PDF extraction. Keep
/// cluster provenance in that authored coordinate system so selected source
/// slices never use an offset into the synthetic buffer:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/css-text-3/#text-processing-order>.
fn styled_text_source_positions(
    internal_text: &str,
    authored_text: &str,
    synthetic_ranges: &[Range<usize>],
) -> Vec<StyledTextSourcePosition> {
    let mut positions = Vec::new();
    let mut authored = authored_text.char_indices().peekable();
    for (start, character) in internal_text.char_indices() {
        let end = start + character.len_utf8();
        if synthetic_ranges
            .iter()
            .any(|synthetic| synthetic.start <= start && end <= synthetic.end)
        {
            continue;
        }
        let Some((authored_start, authored_character)) = authored.next() else {
            return Vec::new();
        };
        if character != authored_character {
            return Vec::new();
        }
        let authored_end = authored
            .peek()
            .map_or(authored_text.len(), |(next, _)| *next);
        positions.push(StyledTextSourcePosition {
            internal: start..end,
            authored: authored_start..authored_end,
        });
    }
    if authored.next().is_none() {
        positions
    } else {
        Vec::new()
    }
}

fn styled_cluster_source_range(
    cluster: Range<usize>,
    positions: &[StyledTextSourcePosition],
) -> Option<Range<usize>> {
    let mut authored_start = None;
    let mut authored_end = None;
    for position in positions {
        if position.internal.end <= cluster.start || cluster.end <= position.internal.start {
            continue;
        }
        if position.internal.start < cluster.start || cluster.end < position.internal.end {
            return None;
        }
        authored_start.get_or_insert(position.authored.start);
        authored_end = Some(position.authored.end);
    }
    authored_start
        .zip(authored_end)
        .map(|(start, end)| start..end)
}

pub(in crate::text) fn font_feature_family(font_family: &FontFamily) -> Option<String> {
    match font_family {
        FontFamily::SansSerif => Some("sans-serif".to_string()),
        FontFamily::Serif => Some("serif".to_string()),
        FontFamily::Monospace => Some("monospace".to_string()),
        FontFamily::SystemUi => Some("system-ui".to_string()),
        FontFamily::UiSerif => Some("ui-serif".to_string()),
        FontFamily::UiSansSerif => Some("ui-sans-serif".to_string()),
        FontFamily::UiMonospace => Some("ui-monospace".to_string()),
        FontFamily::UiRounded => Some("ui-rounded".to_string()),
        FontFamily::List(families) => families.first().and_then(font_feature_family),
        FontFamily::Names(names) => names.first().cloned(),
    }
}

pub(in crate::text) fn style_for_text_range<'a>(
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

/// Return a CSS Fonts 5 `font-size-adjust` metric ratio for a selected face.
///
/// Ratios are metric values divided by units-per-em, matching the aspect-value
/// used-size formula defined for `font-size-adjust`:
/// <https://www.w3.org/TR/css-fonts-5/#font-size-adjust-prop>.
pub(in crate::text) fn font_size_adjust_metric_ratio(
    font: &DocumentFont,
    metric: FontSizeAdjustMetric,
) -> Option<f32> {
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
        // CSS Fonts defines the `ic-width` metric as the advance of U+6C34;
        // if that glyph is unavailable, its fallback metric is one em.
        // <https://www.w3.org/TR/css-fonts-5/#font-size-adjust-prop>
        FontSizeAdjustMetric::IcWidth => glyph_advance_width(&face, '水').unwrap_or(units_per_em),
        FontSizeAdjustMetric::IcHeight => face
            .glyph_index('水')
            .and_then(|glyph| face.glyph_ver_advance(glyph))
            .map(|advance| advance as f32)
            .or_else(|| glyph_bbox_height(&face, '水'))
            .unwrap_or(units_per_em),
    };
    (value.is_finite() && value > 0.0).then_some(value / units_per_em)
}

pub(in crate::text) fn glyph_advance_width(
    face: &ttf_parser::Face<'_>,
    character: char,
) -> Option<f32> {
    face.glyph_index(character)
        .and_then(|glyph| face.glyph_hor_advance(glyph))
        .map(|advance| advance as f32)
        .filter(|advance| *advance > 0.0)
}

pub(in crate::text) fn glyph_bbox_height(
    face: &ttf_parser::Face<'_>,
    character: char,
) -> Option<f32> {
    face.glyph_index(character)
        .and_then(|glyph| face.glyph_bounding_box(glyph))
        .map(|bbox| (bbox.y_max - bbox.y_min).abs() as f32)
        .filter(|height| *height > 0.0)
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

/// Create a preserved CSS tab as a layout advance without a paintable glyph.
///
/// Its provisional advance is copied from Parley's cluster so CSS tab-stop
/// resolution can rebase every following run from the exact cursor that the
/// shaper used. The used advance is resolved later against the selected line
/// cursor. This deliberately cannot fall back to `.notdef` when the shaper's
/// temporary font run does not include U+0020.
pub(in crate::text) fn synthesized_tab_glyph(provisional_advance: f32) -> RenderedGlyph {
    RenderedGlyph {
        kind: RenderedGlyphKind::AdvanceOnly,
        x_advance: provisional_advance,
        nominal_x_advance: provisional_advance,
        x_offset: 0.0,
        y_offset: 0.0,
        unicode: "\t".to_string(),
    }
}

pub(in crate::text) fn tab_stop_advance(period: f32, current_x: f32) -> LayoutLength {
    if period <= 0.0 || !period.is_finite() || !current_x.is_finite() {
        return layout_pt(0.0);
    }
    let next_stop = (current_x / period).floor().mul_add(period, period);
    layout_pt((next_stop - current_x).max(0.0))
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
pub(in crate::text) fn push_synthetic_join_control(
    text: &mut String,
    synthetic_ranges: &mut Vec<Range<usize>>,
) {
    let start = text.len();
    text.push('\u{200d}');
    synthetic_ranges.push(start..text.len());
}

pub(in crate::text) fn text_needs_edge_join_context(text: &str) -> bool {
    text_needs_leading_join_context(text) || text_needs_trailing_join_context(text)
}

pub(in crate::text) fn text_needs_leading_join_context(text: &str) -> bool {
    let mut characters = text.chars();
    if characters.next() != Some('\u{200d}') {
        return false;
    }
    characters
        .find(|character| !character_is_join_control(*character))
        .is_some_and(character_can_join_preceding)
}

pub(in crate::text) fn text_needs_trailing_join_context(text: &str) -> bool {
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
pub(in crate::text) fn push_edge_join_context(
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
        } else if text[..range.start].ends_with('\u{200d}')
            && slice
                .chars()
                .next()
                .is_some_and(character_can_join_preceding)
        {
            // A joiner can be owned by a separately styled span (including a
            // fallback font face selected solely for U+200D).  The adjacent
            // Arabic span still needs a concrete joining neighbor; retain the
            // authored joiner and add only the shaping-only tatweel context at
            // the styled boundary.
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
            // <https://www.w3.org/TR/alreq/#h_joining-enforcement>
            insertions.push(range.start);
        }
        if let Some(index) = trailing_join_context_insertion_index(slice) {
            insertions.push(range.start + index);
        } else if text[range.end..].starts_with('\u{200d}')
            && slice
                .chars()
                .next_back()
                .is_some_and(character_can_join_following)
        {
            insertions.push(range.end);
        }
    }
    insertions.sort_unstable();
    insertions.dedup();
    for index in insertions.into_iter().rev() {
        insert_synthetic_join_context(text, ranges, synthetic_ranges, index);
    }
}

pub(in crate::text) fn range_is_synthetic_only(
    range: Range<usize>,
    synthetic_ranges: &[Range<usize>],
) -> bool {
    range.start < range.end
        && synthetic_ranges
            .iter()
            .any(|synthetic| synthetic.start <= range.start && range.end <= synthetic.end)
}

pub(in crate::text) fn leading_join_context_insertion_index(text: &str) -> Option<usize> {
    if !text.starts_with('\u{200d}') {
        return None;
    }
    text.char_indices()
        .find(|(_, character)| !character_is_join_control(*character))
        .and_then(|(index, character)| character_can_join_preceding(character).then_some(index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styled_cluster_ranges_exclude_synthetic_join_context() {
        let internal = "ا\u{200d}ل\u{0640}سلا\u{200d}م";
        let synthetic = vec![2..5, 7..9, 15..18];
        let positions = styled_text_source_positions(internal, "السلام", &synthetic);

        assert_eq!(styled_cluster_source_range(5..7, &positions), Some(2..4),);
        assert_eq!(styled_cluster_source_range(9..13, &positions), Some(4..8),);
        assert_eq!(styled_cluster_source_range(2..5, &positions), None,);
    }
}
