use super::super::*;
use crate::css::WhiteSpace;
use crate::document::paint::text::RenderedTextMatrix;
use crate::units::SemanticLengthExt;
use std::borrow::Cow;

pub(in crate::text) struct FontSizeAdjustmentRange {
    pub(in crate::text) range: Range<usize>,
    pub(in crate::text) font_size: f32,
}

impl FontSystem {
    pub(in crate::text) fn font_size_adjustment_ranges_for_line<B: parley::style::Brush>(
        &mut self,
        line: &parley::Line<'_, B>,
        text: &str,
        style: &ComputedStyle,
    ) -> Vec<FontSizeAdjustmentRange> {
        let mut ranges = Vec::new();
        for run in line.runs() {
            let fallback_character = parley_run_fallback_character(text, run.text_range());
            let Some(font_id) = self.document_font_from_parley_font_data_for_style(
                run.font(),
                style,
                fallback_character,
            ) else {
                continue;
            };
            let Some(font_size) = self.used_font_size_for_font(style, font_id) else {
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
        text: &str,
        shaping_ranges: &[(Range<usize>, &ComputedStyle)],
        metric_ranges: &[(Range<usize>, &ComputedStyle)],
        default_style: &ComputedStyle,
    ) -> Vec<FontSizeAdjustmentRange> {
        let mut adjustments = Vec::new();
        for run in line.runs() {
            let run_range = run.text_range();
            let run_style =
                style_for_text_range(shaping_ranges, run_range.clone()).unwrap_or(default_style);
            let metric_style =
                style_for_text_range(metric_ranges, run_range.clone()).unwrap_or(default_style);
            let fallback_character = parley_run_fallback_character(text, run_range.clone());
            let Some(font_id) = self.document_font_from_parley_font_data_for_style(
                run.font(),
                run_style,
                fallback_character,
            ) else {
                continue;
            };
            let Some(font_size) = self.used_font_size_for_font(metric_style, font_id) else {
                continue;
            };
            if (font_size - metric_style.font_size).abs() > 0.01 {
                adjustments.push(FontSizeAdjustmentRange {
                    range: run_range,
                    font_size,
                });
            }
        }
        adjustments
    }

    /// Re-shape a provably simple visible fallback fragment with the CSS face
    /// selected for that scalar, retaining its control-bearing source text for
    /// PDF extraction and reporting the omitted fallback advance separately.
    pub(in crate::text) fn rehome_control_fallback_run(
        &mut self,
        style: &ComputedStyle,
        request: ControlFallbackRehomeRequest,
    ) -> Option<(ShapedGlyphRun, f32)> {
        let selected_font_id = self.font_for_character(style, request.character)?;
        (selected_font_id != request.fallback_font_id).then_some(())?;
        let selected_font = self.document_fonts.get(selected_font_id)?;
        let mut glyphs = shape_text_with_document_font(
            selected_font,
            &request.character.to_string(),
            request.font_size,
            0.0,
            0.0,
        )?;
        if glyphs.len() != 1 {
            return None;
        }
        glyphs[0].x_advance += style.used_letter_spacing().points();
        let dropped_advance = (request.parley_advance - glyphs[0].x_advance).max(0.0);
        Some((
            ShapedGlyphRun {
                text: request.text,
                x_offset: request.x_offset,
                y_offset: 0.0,
                text_matrix: RenderedTextMatrix::IDENTITY,
                font_size: request.font_size,
                font_id: Some(selected_font_id),
                font_palette: style.font_palette.clone(),
                glyphs,
                glyph_source_ranges: vec![request.source_range],
            },
            dropped_advance,
        ))
    }

    pub(in crate::text) fn rendered_text_runs_for_parley_line<B: parley::style::Brush>(
        &mut self,
        text: &str,
        line: parley::Line<'_, B>,
        style: &ComputedStyle,
    ) -> Vec<ShapedGlyphRun> {
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
            let run_text = text
                .get(run_range.clone())
                .map(text_without_variation_selectors)
                .unwrap_or_else(|| Cow::Borrowed(""));
            // CSS `font-size: 0` gives text no glyph ink or advance. Do not
            // serialize a zero-scale PDF text operation: PDF consumers may
            // handle a zero `Tf` state inconsistently, and the run has no
            // paint or extraction contribution.
            if run.font_size() <= 0.01 {
                continue;
            }
            let x_offset = run
                .visual_clusters()
                .next()
                .and_then(|cluster| cluster.visual_offset())
                .unwrap_or(0.0);
            if control_fallback_cluster == ControlFallbackCluster::DropControlOnly {
                dropped_default_ignorable_runs.push(DroppedDefaultIgnorableRun {
                    x_offset,
                    advance: run.advance(),
                    text: run_text.clone().into_owned().into(),
                });
                continue;
            }
            let fallback_character = parley_run_fallback_character(text, run_range.clone());
            let Some(font_id) = self.document_font_from_parley_font_data_for_style(
                run.font(),
                style,
                fallback_character,
            ) else {
                continue;
            };
            if let ControlFallbackCluster::RehomeSimpleVisibleFragment { character } =
                control_fallback_cluster
                && let Some((rehomed_run, dropped_advance)) = self.rehome_control_fallback_run(
                    style,
                    ControlFallbackRehomeRequest {
                        character,
                        fallback_font_id: font_id,
                        text: run_text.clone().into_owned().into(),
                        font_size: run.font_size(),
                        x_offset,
                        parley_advance: run.advance(),
                        source_range: Some(run_range.clone()),
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
                    style,
                    metric_style: style,
                });
                continue;
            }
            if !run_text.contains('\t')
                && self
                    .document_fonts
                    .support_kind_for_run(font_id, run_text.as_ref())
                    == FontSupportKind::ColorOrEmojiOnlyFallback
                && !self
                    .document_fonts
                    .run_has_emoji_presentation_glyph(font_id, run_text.as_ref())
                && let Some(fallback_font_id) =
                    self.visible_text_fallback_for_run(run_text.as_ref(), style, font_id)
                && let Some(fallback_font) = self.document_fonts.get(fallback_font_id)
                && let Some(glyphs) = shape_text_with_document_font(
                    fallback_font,
                    run_text.as_ref(),
                    run.font_size(),
                    style.used_letter_spacing().points(),
                    style.used_word_spacing().points(),
                )
                && !glyphs.is_empty()
            {
                let glyph_source_ranges = vec![None; glyphs.len()];
                rendered_runs.push(ShapedGlyphRun {
                    text: run_text.into_owned().into(),
                    x_offset,
                    y_offset: 0.0,
                    text_matrix: RenderedTextMatrix::IDENTITY,
                    font_size: run.font_size(),
                    font_id: Some(fallback_font_id),
                    font_palette: style.font_palette.clone(),
                    glyphs,
                    glyph_source_ranges,
                });
                tab_contexts.push(RenderedRunTabContext {
                    style,
                    metric_style: style,
                });
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
            let mut glyph_source_ranges = Vec::new();
            let mut emitted_source_ranges = Vec::<Range<usize>>::new();
            for cluster in run.visual_clusters() {
                let cluster_range = cluster.text_range();
                let cluster_text = text.get(cluster_range.clone()).unwrap_or_default();
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
                    let provisional_advance = cluster.glyphs().map(|glyph| glyph.advance).sum();
                    glyphs.push(synthesized_tab_glyph(provisional_advance));
                    glyph_source_ranges.push(Some(cluster_range));
                    continue;
                }
                let mut first_cluster_glyph = true;
                for glyph in cluster.glyphs() {
                    let Ok(glyph_id) = u16::try_from(glyph.id) else {
                        continue;
                    };
                    let unicode = if first_cluster_glyph {
                        if default_ignorable_only
                            || emitted_source_ranges
                                .iter()
                                .any(|range| range == &cluster_range)
                        {
                            String::new()
                        } else {
                            emitted_source_ranges.push(cluster_range.clone());
                            emitted_cluster_text.as_ref().to_owned()
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
                    let emitted_glyph_id =
                        if matches!(style.text_layout_policy(), TextLayoutPolicy::Vertical(_)) {
                            // The shaper has already selected any OpenType
                            // vertical alternate. Replacing a Unicode space
                            // separator with the horizontal U+0020 glyph here
                            // would discard that selection, including the `vert`
                            // form required for transformed vertical units.
                            // <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>
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
                    glyph_source_ranges.push(Some(cluster_range.clone()));
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
            rendered_runs.push(ShapedGlyphRun {
                text: run_text.into_owned().into(),
                x_offset,
                y_offset: 0.0,
                text_matrix: RenderedTextMatrix::IDENTITY,
                font_size,
                font_id: Some(font_id),
                font_palette: style.font_palette.clone(),
                glyphs,
                glyph_source_ranges,
            });
            tab_contexts.push(RenderedRunTabContext {
                style,
                metric_style: style,
            });
        }
        for run in &mut rendered_runs {
            run.x_offset =
                corrected_visual_run_x_offset(run.x_offset, &dropped_default_ignorable_runs);
        }
        // `break-spaces` retains each preserved separator as its own CSS text
        // processing unit.  Do not let a glyph-placement adjustment on the
        // first following text glyph pull that glyph back into the retained
        // separator's physical advance; the same source split at an inline
        // boundary must keep the identical visible edge.
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
        if style.white_space == WhiteSpace::BreakSpaces {
            let mut previous_was_spacer = false;
            for run in &mut rendered_runs {
                for glyph in &mut run.glyphs {
                    let is_spacer = !glyph.unicode.is_empty()
                        && glyph
                            .unicode
                            .chars()
                            .all(character_is_text_decoration_spacer);
                    if previous_was_spacer && !is_spacer {
                        glyph.x_offset = 0.0;
                    }
                    previous_was_spacer = is_spacer;
                }
            }
        }
        stitch_dropped_join_control_runs(&mut rendered_runs, &dropped_default_ignorable_runs);
        for index in rehomed_control_fallback_runs.into_iter().rev() {
            if stitch_rehomed_control_fallback_run(&mut rendered_runs, index) {
                tab_contexts.remove(index + 1);
            }
        }
        self.apply_css_tab_stops(&mut rendered_runs, &tab_contexts, 0.0);
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
        let names = named_font_families(&style.font_family);
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

        let mut selections = Vec::<(Range<usize>, Option<FontFamily>)>::new();
        let mut previous_family = None::<FontFamily>;
        for (start, character) in text.char_indices() {
            let end = start + character.len_utf8();
            let family = if character_is_join_control(character) {
                previous_family
                    .clone()
                    .or_else(|| self.next_unicode_range_family_for_text(text, end, style))
            } else {
                self.unicode_range_family_for_character(character, style)
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
                span_style.font_family = family;
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
    ) -> Option<FontFamily> {
        text.get(start..)?.chars().find_map(|character| {
            (!character_is_join_control(character))
                .then(|| self.unicode_range_family_for_character(character, style))
                .flatten()
        })
    }

    pub(in crate::text) fn unicode_range_family_for_character(
        &mut self,
        character: char,
        style: &ComputedStyle,
    ) -> Option<FontFamily> {
        let families = match &style.font_family {
            FontFamily::List(families) => families.as_slice(),
            family => std::slice::from_ref(family),
        };
        for family in families {
            match family {
                FontFamily::Named(name) => {
                    let Some(font_id) = self.resolve_single_family(
                        name.as_str(),
                        style.font_weight,
                        style.font_style,
                        style.font_width,
                    ) else {
                        continue;
                    };
                    if self.document_fonts.font_has_character(font_id, character) {
                        return Some(FontFamily::Named(name.clone()));
                    }
                }
                generic => {
                    // A generic family is resolved by the platform after all
                    // explicitly range-limited named faces have declined the
                    // character. Keep it as the backend family source rather
                    // than retaining a rejected preceding face in the stack.
                    return Some(generic.clone());
                }
            }
        }
        None
    }
}

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
    #[cfg(test)]
    pub(crate) fn shape_styled_text_runs_with_parley(
        &mut self,
        spans: &[StyledTextSpan<'_>],
    ) -> Vec<ShapedGlyphRun> {
        self.shape_styled_text_runs_with_parley_with_letter_spacing(
            spans,
            ShapingLetterSpacing::Computed,
        )
    }

    pub(in crate::text) fn shape_styled_text_runs_with_parley_with_letter_spacing(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        letter_spacing: ShapingLetterSpacing,
    ) -> Vec<ShapedGlyphRun> {
        let Some(tab_metric_style) = spans.first().map(|span| span.style) else {
            return Vec::new();
        };
        self.shape_styled_text_runs_with_parley_at_tab_origin_with_letter_spacing(
            spans,
            0.0,
            tab_metric_style,
            letter_spacing,
        )
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
        self.shape_styled_text_runs_with_parley_at_tab_origin_with_letter_spacing(
            spans,
            tab_origin,
            tab_metric_style,
            ShapingLetterSpacing::Computed,
        )
    }

    fn shape_styled_text_runs_with_parley_at_tab_origin_with_letter_spacing(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
        letter_spacing: ShapingLetterSpacing,
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
        // Keep the authored style with each resolved shaping span. A
        // `unicode-range` fallback changes the face used for glyph selection,
        // but it must not change the primary font from which
        // `font-size-adjust: from-font` obtains its target aspect value.
        let mut unicode_range_spans =
            Vec::<(String, Cow<'_, ComputedStyle>, &ComputedStyle)>::new();
        for span in spans.iter().filter(|span| !span.text.is_empty()) {
            if let Some(resolved_spans) =
                self.unicode_range_resolved_text_spans(span.text, span.style)
            {
                for resolved in resolved_spans {
                    if let Some(text) = span.text.get(resolved.range) {
                        unicode_range_spans.push((
                            text.to_string(),
                            Cow::Owned(resolved.style),
                            span.style,
                        ));
                    }
                }
            } else {
                unicode_range_spans.push((
                    span.text.to_string(),
                    Cow::Borrowed(span.style),
                    span.style,
                ));
            }
        }
        let metric_styles = unicode_range_spans
            .iter()
            .map(|(_, _, metric_style)| *metric_style)
            .collect::<Vec<_>>();
        let spans = unicode_range_spans
            .iter()
            .map(|(text, style, _)| StyledTextSpan {
                text: text.as_str(),
                style: style.as_ref(),
            })
            .collect::<Vec<_>>();
        let mut ranges: Vec<(Range<usize>, &ComputedStyle)> = Vec::with_capacity(spans.len());
        let mut metric_ranges: Vec<(Range<usize>, &ComputedStyle)> =
            Vec::with_capacity(spans.len());
        let mut synthetic_join_controls = Vec::new();
        let spans = spans
            .iter()
            .zip(metric_styles)
            .filter(|(span, _)| !span.text.is_empty())
            .map(|(span, metric_style)| (*span, metric_style))
            .collect::<Vec<_>>();
        let authored_text = spans.iter().map(|(span, _)| span.text).collect::<String>();
        // Join controls are default-ignorable shaping instructions, not
        // visible font content. Their own inline style must therefore not
        // create a separate font-selection range: an author may isolate a
        // U+200C/U+200D in a span with a deliberately unrelated fallback
        // face, while the adjacent Arabic text still shapes as one context.
        // Keep each control in the input stream but assign it to an adjacent
        // visible span for font selection.
        // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
        // <https://www.w3.org/TR/alreq/#h_joining-enforcement>
        let mut shaping_spans = Vec::<(String, &ComputedStyle, &ComputedStyle)>::new();
        let mut pending_join_controls = String::new();
        for (span, metric_style) in &spans {
            // Default-ignorables are font-neutral, but not source-neutral.
            // Excluding them from Parley's font-selection buffer avoids
            // giving it a second line-breaking authority. They remain in
            // `authored_text`, and `styled_text_source_positions` restores
            // their authored byte coordinates when clusters are selected.
            // In particular, a standalone U+200B must never turn into an
            // empty Parley style range.
            // <https://www.w3.org/TR/css-text-3/#line-break-details>
            // <https://www.w3.org/TR/css-text-3/#text-processing-order>
            let span_text = text_without_font_neutral_default_ignorables(span.text);
            let span_text = span_text.as_ref();
            if span_text.is_empty() {
                continue;
            }
            if span_text.chars().all(character_is_join_control) {
                if let Some((previous_text, _, _)) = shaping_spans.last_mut() {
                    previous_text.push_str(span_text);
                } else {
                    pending_join_controls.push_str(span_text);
                }
                continue;
            }
            let mut text_with_pending = std::mem::take(&mut pending_join_controls);
            text_with_pending.push_str(span_text);
            if let Some((previous_text, previous_style, previous_metric_style)) =
                shaping_spans.last_mut()
                && *previous_style == span.style
                && *previous_metric_style == *metric_style
            {
                previous_text.push_str(&text_with_pending);
            } else {
                shaping_spans.push((text_with_pending, span.style, *metric_style));
            }
        }
        if !pending_join_controls.is_empty()
            && let Some((previous_text, _, _)) = shaping_spans.last_mut()
        {
            previous_text.push_str(&pending_join_controls);
        }
        for (index, (span_text, style, metric_style)) in shaping_spans.iter().enumerate() {
            let start = text.len();
            if index > 0
                && shaping_spans
                    .get(index - 1)
                    .is_some_and(|(previous, previous_style, _)| {
                        previous_style != style
                            && span_boundary_needs_join_control(previous, span_text)
                    })
            {
                push_synthetic_join_control(&mut text, &mut synthetic_join_controls);
            }
            push_text_with_font_variant_emoji(&mut text, span_text, style);
            if shaping_spans
                .get(index + 1)
                .is_some_and(|(next, next_style, _)| {
                    next_style != style && span_boundary_needs_join_control(span_text, next)
                })
            {
                push_synthetic_join_control(&mut text, &mut synthetic_join_controls);
            }
            ranges.push((start..text.len(), style));
            metric_ranges.push((start..text.len(), metric_style));
        }
        if text.is_empty() || ranges.is_empty() {
            return Vec::new();
        }
        push_edge_join_context(&mut text, &mut ranges, &mut synthetic_join_controls);
        debug_assert!(ranges.iter().all(|(range, _)| {
            range.start < range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end)
        }));
        let source_positions =
            styled_text_source_positions(&text, &authored_text, &synthetic_join_controls);
        // The normalized text is only the input to Parley's glyph selection.
        // `authored_text` remains the CSS Text stream for line breaking and
        // PDF extraction; `source_positions` maps the font-selectable buffer
        // back to its original byte ranges.
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
                letter_spacing.requested_for(default_style),
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
                    letter_spacing.requested_for(style),
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
                &metric_ranges,
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
                    letter_spacing.requested_for(default_style),
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
                        letter_spacing.requested_for(style),
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
                if run.font_size() <= 0.01 {
                    continue;
                }
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
                        .run_has_emoji_presentation_glyph(font_id, raw_run_text)
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
                        text_matrix: RenderedTextMatrix::IDENTITY,
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
                        text_matrix: RenderedTextMatrix::IDENTITY,
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
                // CSS source stream. A ligature can be attached only to its
                // first source cluster, and U+200C/U+200D affect shaping
                // without producing a standalone glyph. When palette
                // grouping leaves this Parley run intact, retain its authored
                // (synthetic controls already removed) text so the PDF paint
                // boundary can detect and preserve any missing source text
                // with `/ActualText`.
                // ISO 32000-2:2020, 14.9.4.4, "ActualText".
                if rendered_runs.len() == rendered_run_start + 1 {
                    let grouped_glyph_text = rendered_runs[rendered_run_start]
                        .glyphs
                        .iter()
                        .map(|glyph| glyph.unicode.as_str())
                        .collect::<String>();
                    if grouped_glyph_text != run_text {
                        rendered_runs[rendered_run_start].text = run_text.into();
                    }
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

    /// Resolve an emoji face by the effective Unicode presentation selector.
    /// Both monochrome and color fonts can cover the same base scalar, so
    /// ordinary cmap fallback alone cannot implement `font-variant-emoji`.
    ///
    /// The candidate order is the CSS family list followed by the platform
    /// fallback for the scalar. In particular, a generic family such as
    /// `serif` can provide the text presentation while the platform emoji
    /// fallback provides the color presentation.
    /// <https://www.w3.org/TR/css-fonts-4/#font-variant-emoji-prop>
    pub(in crate::text) fn emoji_presentation_family_source(
        &mut self,
        text: &str,
        style: &ComputedStyle,
    ) -> Option<String> {
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
        let base_text = base.to_string();
        self.emoji_presentation_font_candidates(style, base)
            .into_iter()
            .find(|font_id| {
                self.document_fonts.font_has_character(*font_id, base)
                    && self
                        .document_fonts
                        .run_has_emoji_presentation_glyph(*font_id, &base_text)
                        == requested_color
            })
            .and_then(|font_id| self.document_fonts.get(font_id))
            .map(|font| parley_font_family_source(&FontFamily::named(font.family.clone())))
    }

    /// Return the CSS stack's usable faces in precedence order, then the
    /// platform fallback. This is deliberately kept at the font-selection
    /// boundary so that shaping, metric lookup, and PDF embedding refer to
    /// the same concrete document font.
    fn emoji_presentation_font_candidates(
        &mut self,
        style: &ComputedStyle,
        character: char,
    ) -> Vec<usize> {
        let families = match &style.font_family {
            FontFamily::List(families) => families.clone(),
            family => vec![family.clone()],
        };
        let mut candidates = Vec::new();
        for family in families {
            match family {
                FontFamily::Named(name) => {
                    if let Some(font_id) = self.resolve_single_family(
                        name.as_str(),
                        style.font_weight,
                        style.font_style,
                        style.font_width,
                    ) {
                        candidates.push(font_id);
                    }
                }
                generic => {
                    if let Some(font_id) = self.resolve_generic_family(
                        &generic,
                        style.font_weight,
                        style.font_style,
                        style.font_width,
                    ) {
                        candidates.push(font_id);
                    }
                }
            }
        }
        if let Some(font_id) = self.resolve_system_fallback_for_style_character(style, character) {
            candidates.push(font_id);
        }
        // Keep CSS fallback order: it is observable when two faces offer the
        // requested presentation. `dedup` after sorting would incorrectly
        // reorder the author-provided stack.
        candidates.dedup();
        candidates
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
        loop {
            let Some((authored_start, authored_character)) = authored.next() else {
                return Vec::new();
            };
            let authored_end = authored
                .peek()
                .map_or(authored_text.len(), |(next, _)| *next);
            if character_is_font_neutral_default_ignorable(authored_character) {
                continue;
            }
            if character != authored_character {
                return Vec::new();
            }
            positions.push(StyledTextSourcePosition {
                internal: start..end,
                authored: authored_start..authored_end,
            });
            break;
        }
    }
    if authored.all(|(_, character)| character_is_font_neutral_default_ignorable(character)) {
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
