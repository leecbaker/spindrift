use std::borrow::Cow;

use icu_segmenter::GraphemeClusterSegmenter;

use super::super::*;
use crate::css::WhiteSpace;
use crate::document::paint::text::RenderedTextMatrix;
use crate::units::SemanticLengthExt;

pub(in crate::text) struct FontSizeAdjustmentRange {
    pub(in crate::text) range: Range<usize>,
    pub(in crate::text) font_size: f32,
}

/// One authored styled span after `@font-face unicode-range` face selection.
///
/// The shaping view borrows the authored computed style and owns only a
/// changed `unicode-range` family. The metric style always remains the
/// author's original style.
struct ResolvedStyledTextSpan<'a> {
    text: &'a str,
    shaping_style: SelectedFaceStyleView<'a>,
    metric_style: &'a ComputedStyle,
}

/// One contiguous source fragment assigned to a single shaping and metric
/// style before synthetic boundary controls are inserted.
struct StyledShapingSpan<'span, 'source> {
    text: String,
    shaping_style: &'span SelectedFaceStyleView<'source>,
    metric_style: &'source ComputedStyle,
}

/// The styled text buffer passed to Parley and its durable source provenance.
///
/// `text` may contain emoji presentation selectors and synthetic join
/// controls. `source_positions` maps its non-synthetic clusters back to the
/// authored CSS Text stream before Parley cluster ranges are converted into
/// [`ShapedGlyphRun`] provenance.
struct MappedStyledShapingText<'span, 'source> {
    text: String,
    authored_text: String,
    ranges: Vec<(Range<usize>, &'span SelectedFaceStyleView<'source>)>,
    metric_ranges: Vec<(Range<usize>, &'source ComputedStyle)>,
    shaping_contexts: ShapingContextMap,
    source_positions: Vec<StyledTextSourcePosition>,
}

/// A concrete family selected for one emoji-presentation grapheme cluster.
///
/// The range remains in shaping-input coordinates, so synthesized presentation
/// selectors stay with their preceding base through Parley cluster matching.
pub(in crate::text) struct EmojiPresentationFamilyRange {
    pub(in crate::text) range: Range<usize>,
    pub(in crate::text) source: String,
}

fn unicode_range_resolved_span(
    range: Range<usize>,
    selected_family: Option<FontFamily>,
) -> UnicodeRangeResolvedSpan {
    UnicodeRangeResolvedSpan {
        range,
        selected_family,
    }
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
        shaping_ranges: &[(Range<usize>, &SelectedFaceStyleView<'_>)],
        metric_ranges: &[(Range<usize>, &ComputedStyle)],
        default_style: &SelectedFaceStyleView<'_>,
    ) -> Vec<FontSizeAdjustmentRange> {
        let mut adjustments = Vec::new();
        for run in line.runs() {
            let run_range = run.text_range();
            let run_style =
                style_for_text_range(shaping_ranges, run_range.clone()).unwrap_or(default_style);
            let metric_style = style_for_text_range(metric_ranges, run_range.clone())
                .unwrap_or(default_style.authored());
            let fallback_character = parley_run_fallback_character(text, run_range.clone());
            let Some(font_id) = self.document_font_from_parley_font_data_for_family(
                run.font(),
                run_style.authored(),
                run_style.font_family(),
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
        letter_spacing: ShapingLetterSpacing,
    ) -> Option<(ShapedGlyphRun, ShaperInlineAdvance)> {
        self.rehome_control_fallback_run_for_family(
            style,
            &style.font_family,
            request,
            letter_spacing,
        )
    }

    fn rehome_control_fallback_run_for_selected_face(
        &mut self,
        style: &SelectedFaceStyleView<'_>,
        request: ControlFallbackRehomeRequest,
        letter_spacing: ShapingLetterSpacing,
    ) -> Option<(ShapedGlyphRun, ShaperInlineAdvance)> {
        self.rehome_control_fallback_run_for_family(
            style.authored(),
            style.font_family(),
            request,
            letter_spacing,
        )
    }

    fn rehome_control_fallback_run_for_family(
        &mut self,
        style: &ComputedStyle,
        family: &FontFamily,
        request: ControlFallbackRehomeRequest,
        letter_spacing: ShapingLetterSpacing,
    ) -> Option<(ShapedGlyphRun, ShaperInlineAdvance)> {
        let selected_font_id =
            self.font_for_character_in_family(style, family, request.character)?;
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
        glyphs[0].x_advance += letter_spacing.requested_for(style);
        let dropped_advance = ShaperInlineAdvance::from_parley(
            (request.shaper_advance.points() - glyphs[0].x_advance).max(0.0),
        );
        Some((
            ShapedGlyphRun {
                text: request.text,
                x_offset: request.visual_position.points(),
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
        letter_spacing: ShapingLetterSpacing,
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
                dropped_default_ignorable_runs.push(SourceOnlyRun {
                    visual_position: ShaperVisualInlinePosition::from_parley(x_offset),
                    shaper_advance: ShaperInlineAdvance::from_parley(run.advance()),
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
                        visual_position: ShaperVisualInlinePosition::from_parley(x_offset),
                        shaper_advance: ShaperInlineAdvance::from_parley(run.advance()),
                        source_range: Some(run_range.clone()),
                    },
                    letter_spacing,
                )
            {
                if !dropped_advance.is_zero() {
                    dropped_default_ignorable_runs.push(SourceOnlyRun {
                        visual_position: ShaperVisualInlinePosition::from_parley(x_offset),
                        shaper_advance: dropped_advance,
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
                    letter_spacing.requested_for(style),
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
                        cluster_text,
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
                    if glyph_is_join_control_artifact(&face, glyph_id, &unicode, cluster_text) {
                        first_cluster_glyph = false;
                        continue;
                    }
                    if glyph_is_non_painting_shaping_artifact(
                        &face,
                        glyph_id,
                        glyph.advance,
                        &unicode,
                    ) {
                        first_cluster_glyph = false;
                        continue;
                    }
                    first_cluster_glyph = false;
                    glyphs.push(RenderedGlyph {
                        kind: RenderedGlyphKind::Paint(glyph_id),
                        x_advance: glyph.advance,
                        nominal_x_advance: face
                            .glyph_hor_advance(ttf_parser::GlyphId(glyph_id))
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
            run.x_offset = corrected_visual_inline_position(
                ShaperVisualInlinePosition::from_parley(run.x_offset),
                &dropped_default_ignorable_runs,
            )
            .points();
        }
        if style.white_space == WhiteSpace::BreakSpaces {
            // `break-spaces` retains each preserved separator as its own CSS text
            // processing unit.  Do not let a glyph-placement adjustment on the
            // first following text glyph pull that glyph back into the retained
            // separator's physical advance; the same source split at an inline
            // boundary must keep the identical visible edge.
            // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
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
        self.apply_css_tab_stops(&mut rendered_runs, &tab_contexts, 0.0, letter_spacing);
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

        let mut previous_family = None::<FontFamily>;
        let mut selected_family = None::<FontFamily>;
        let mut selected_span_start = 0usize;
        let mut has_selected_character = false;
        let mut spans = Vec::<UnicodeRangeResolvedSpan>::new();
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
            if !has_selected_character {
                selected_family = family;
                selected_span_start = start;
                has_selected_character = true;
            } else if selected_family != family {
                spans.push(unicode_range_resolved_span(
                    selected_span_start..start,
                    selected_family,
                ));
                selected_family = family;
                selected_span_start = start;
            }
        }
        if !has_selected_character || spans.is_empty() && selected_family.is_none() {
            return None;
        }
        spans.push(unicode_range_resolved_span(
            selected_span_start..text.len(),
            selected_family,
        ));

        spans
            .iter()
            .any(|span| span.selected_family.as_ref() != Some(&style.font_family))
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
    pub(in crate::text) fn apply_upright_vertical_metrics(
        &self,
        runs: &mut [ShapedInlineRun],
        typesetting_plan: &TextTypesettingPlan,
    ) {
        // Shaping spans need not be stored in visual order: neutral punctuation
        // can split an LTR sequence into runs that Parley returns in source
        // order. Their offsets, however, are visual-inline positions. Apply
        // each preceding metric delta in that coordinate order.
        let mut visual_run_order = (0..runs.len()).collect::<Vec<_>>();
        visual_run_order.sort_by(|&left, &right| {
            runs[left]
                .x_offset
                .total_cmp(&runs[right].x_offset)
                .then_with(|| left.cmp(&right))
        });
        let mut preceding_advance_delta = 0.0;
        for run_index in visual_run_order {
            let run = &mut runs[run_index];
            // Parley records every visual run origin in the horizontal
            // shaping coordinate system. Rebase a later run after converting
            // an earlier upright unit to its vertical advance, otherwise a
            // font/style boundary retains the old horizontal pen position.
            // <https://www.w3.org/TR/css-writing-modes-4/#vertical-font-features>
            run.x_offset += preceding_advance_delta;
            let original_advance = run
                .glyphs
                .iter()
                .map(|glyph| glyph.rendered.x_advance)
                .sum::<f32>();
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
                let Some(VerticalUnitTypesetting::UprightVertical) = glyph
                    .source_range
                    .as_ref()
                    .and_then(|range| typesetting_plan.unanimous_typesetting_for_range(range))
                else {
                    continue;
                };
                if glyph.rendered.unicode == "\t" {
                    continue;
                }
                let Some(glyph_id) = glyph.rendered.painted_id() else {
                    continue;
                };
                let glyph_id = ttf_parser::GlyphId(glyph_id);
                let synthesized_vertical_advance =
                    (i32::from(face.ascender()) - i32::from(face.descender())).max(1) as f32;
                let vertical_advance = face
                    .glyph_ver_advance(glyph_id)
                    .map(|advance| advance as f32)
                    .filter(|advance| *advance > 0.0)
                    .unwrap_or(synthesized_vertical_advance)
                    * scale;
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
                let glyph_vertical_origin = face
                    .glyph_y_origin(glyph_id)
                    .map(f32::from)
                    .or_else(|| {
                        face.glyph_bounding_box(glyph_id)
                            .zip(face.glyph_ver_side_bearing(glyph_id))
                            .map(|(bounds, top_side_bearing)| {
                                f32::from(bounds.y_max) + f32::from(top_side_bearing)
                            })
                    })
                    .map(|origin| origin * scale);
                // Only an OpenType-provided glyph-specific origin alters the
                // horizontal shaping baseline. When neither VORG nor a
                // vertical side bearing is available, the synthesized
                // vertical advance determines the CSS typographic-unit
                // origin; translating the glyph by that same advance would
                // apply the unit placement twice.
                if let Some(vertical_origin) = glyph_vertical_origin {
                    glyph.rendered.y_offset -= vertical_origin;
                }
                let extra_spacing =
                    (glyph.rendered.x_advance - glyph.rendered.nominal_x_advance).max(0.0);
                glyph.rendered.nominal_x_advance = vertical_advance;
                glyph.rendered.x_advance = vertical_advance + extra_spacing;
            }
            let used_advance = run
                .glyphs
                .iter()
                .map(|glyph| glyph.rendered.x_advance)
                .sum::<f32>();
            preceding_advance_delta += used_advance - original_advance;
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
    #[allow(dead_code)] // Computed mode remains available for legacy artifacts.
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

    pub(in crate::text) fn shape_styled_text_runs_with_parley_at_tab_origin_with_letter_spacing(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
        letter_spacing: ShapingLetterSpacing,
    ) -> Vec<ShapedGlyphRun> {
        let resolved_spans = self.resolved_styled_text_spans(spans);
        let Some(MappedStyledShapingText {
            text,
            authored_text,
            ranges,
            metric_ranges,
            shaping_contexts,
            source_positions,
        }) = Self::mapped_styled_shaping_text(&resolved_spans)
        else {
            return Vec::new();
        };
        // The normalized text is only the input to Parley's glyph selection.
        // `source_positions` maps the font-selectable buffer back to its
        // original CSS Text byte ranges.
        let shaping_text = text_with_shaping_compatibility_normalization(&text);
        let shaping_text = shaping_text.as_ref();
        let default_style = ranges[0].1;
        let default_authored_style = default_style.authored();
        let has_cursive_face_transition = text.chars().any(character_has_cursive_shaping_behavior)
            && ranges.windows(2).any(|pair| {
                matches!(
                    pair[0].1.boundary_effect(pair[1].1),
                    InlineBoundaryEffect::ShapingInputChange
                )
            });
        self.with_reusable_parley_layout(|this, layout| {
            let parley_styles = ranges
                .iter()
                .map(|(_, style)| this.shaping_style_for_selected_face_view(style))
                .collect::<Vec<_>>();
            let default_parley_style = &parley_styles[0];
            let default_feature_context =
                this.font_feature_context_for_selected_face(default_style);
            let feature_contexts = ranges
                .iter()
                .map(|(_, style)| this.font_feature_context_for_selected_face(style))
                .collect::<Vec<_>>();
            let mut font_family_sources = Vec::<String>::with_capacity(ranges.len());
            for (range, style) in &ranges {
                let authored = style.authored();
                let selected = this.resolved_parley_font_family_source_for_family(
                    style.font_family(),
                    authored.font_weight,
                    authored.font_style,
                    authored.font_width,
                );
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
            let emoji_family_ranges = ranges
                .iter()
                .flat_map(|(style_range, style)| {
                    this.emoji_presentation_family_ranges(
                        &text[style_range.clone()],
                        style.authored(),
                        style.font_family(),
                    )
                    .into_iter()
                    .map(|mut emoji_range| {
                        emoji_range.range.start += style_range.start;
                        emoji_range.range.end += style_range.start;
                        emoji_range
                    })
                })
                .collect::<Vec<_>>();
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
                default_authored_style,
                letter_spacing.requested_for(default_authored_style),
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
                    style.authored(),
                    range.clone(),
                    letter_spacing.requested_for(style.authored()),
                    feature_context.as_ref(),
                );
            }
            for emoji_range in &emoji_family_ranges {
                builder.push(
                    StyleProperty::FontFamily(ParleyFontFamily::from(emoji_range.source.as_str())),
                    emoji_range.range.clone(),
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
                    default_authored_style,
                    letter_spacing.requested_for(default_authored_style),
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
                        style.authored(),
                        range.clone(),
                        letter_spacing.requested_for(style.authored()),
                        feature_context.as_ref(),
                    );
                }
                for emoji_range in &emoji_family_ranges {
                    builder.push(
                        StyleProperty::FontFamily(ParleyFontFamily::from(
                            emoji_range.source.as_str(),
                        )),
                        emoji_range.range.clone(),
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
                    &shaping_contexts,
                );
                let run_text = match text_without_variation_selectors(&run_text_without_synthetic) {
                    Cow::Borrowed(_) => run_text_without_synthetic,
                    Cow::Owned(text) => text,
                };
                let authored_run_text = styled_authored_text_for_internal_range(
                    run_range.clone(),
                    &source_positions,
                    &authored_text,
                )
                .unwrap_or(run_text.as_ref());
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
                    dropped_default_ignorable_runs.push(SourceOnlyRun {
                        visual_position: ShaperVisualInlinePosition::from_parley(x_offset),
                        shaper_advance: ShaperInlineAdvance::from_parley(run.advance()),
                        text: authored_run_text.into(),
                    });
                    continue;
                }
                let fallback_character =
                    parley_run_fallback_character(shaping_text, run_range.clone());
                let Some(font_id) = this.document_font_from_parley_font_data_for_family(
                    run.font(),
                    run_style.authored(),
                    run_style.font_family(),
                    fallback_character,
                ) else {
                    continue;
                };
                let contextual_glyphs = this.selected_face_contextual_glyphs(
                    &text,
                    run_range.clone(),
                    run_style,
                    has_cursive_face_transition,
                    letter_spacing,
                );
                if let ControlFallbackCluster::RehomeSimpleVisibleFragment { character } =
                    control_fallback_cluster
                    && let Some((rehomed_run, dropped_advance)) = this
                        .rehome_control_fallback_run_for_selected_face(
                            run_style,
                            ControlFallbackRehomeRequest {
                                character,
                                fallback_font_id: font_id,
                                text: run_text.clone().into(),
                                font_size: run.font_size(),
                                visual_position: ShaperVisualInlinePosition::from_parley(x_offset),
                                shaper_advance: ShaperInlineAdvance::from_parley(run.advance()),
                                source_range: styled_cluster_source_range(
                                    run_range.clone(),
                                    &source_positions,
                                ),
                            },
                            letter_spacing,
                        )
                {
                    if !dropped_advance.is_zero() {
                        dropped_default_ignorable_runs.push(SourceOnlyRun {
                            visual_position: ShaperVisualInlinePosition::from_parley(x_offset),
                            shaper_advance: dropped_advance,
                            text: Rc::from(""),
                        });
                    }
                    rehomed_control_fallback_runs.push(rendered_runs.len());
                    rendered_runs.push(rehomed_run);
                    tab_contexts.push(RenderedRunTabContext {
                        style: run_style.authored(),
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
                    && let Some(fallback_font_id) = this
                        .visible_text_fallback_for_run_for_selected_face(
                            &run_text, run_style, font_id,
                        )
                    && let Some(fallback_font) = this.document_fonts.get(fallback_font_id)
                    && let Some(glyphs) = shape_text_with_document_font(
                        fallback_font,
                        raw_run_text,
                        run.font_size(),
                        letter_spacing.requested_for(run_style.authored()),
                        run_style.authored().used_word_spacing().points(),
                    )
                    && !glyphs.is_empty()
                {
                    let glyphs = glyphs_without_synthetic_join_controls(
                        glyphs,
                        raw_run_text,
                        run_range.start,
                        &shaping_contexts,
                    );
                    let glyph_source_ranges = vec![None; glyphs.len()];
                    rendered_runs.push(ShapedGlyphRun {
                        text: run_text.into(),
                        x_offset,
                        y_offset: 0.0,
                        text_matrix: RenderedTextMatrix::IDENTITY,
                        font_size: run.font_size(),
                        font_id: Some(fallback_font_id),
                        font_palette: run_style.authored().font_palette.clone(),
                        glyphs,
                        glyph_source_ranges,
                    });
                    tab_contexts.push(RenderedRunTabContext {
                        style: run_style.authored(),
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
                // Parley may retain a single shaping run across authored
                // inline boundaries when the changed property does not affect
                // glyph shaping. Keep the style associated with every emitted
                // glyph so a preserved tab can retain its own `tab-size`.
                let mut glyph_tab_styles = Vec::new();
                // Parley keeps paint-only style transitions on glyphs rather
                // than splitting the underlying shaping run. Retain the
                // palette selected by each cluster so CSS `font-palette`
                // remains independent for adjacent inline elements.
                let mut glyph_palettes = Vec::new();
                for cluster in run.visual_clusters() {
                    let cluster_range = cluster.text_range();
                    let cluster_style = style_for_text_range(&ranges, cluster_range.clone())
                        .unwrap_or(run_style)
                        .authored();
                    let cluster_palette = cluster.first_style().brush.clone();
                    let raw_cluster_text = text.get(cluster_range.clone()).unwrap_or_default();
                    let source_range = if raw_cluster_text.chars().all(character_is_join_control) {
                        styled_join_control_cluster_source_range(
                            cluster_range.clone(),
                            &source_positions,
                            &authored_text,
                        )
                    } else {
                        styled_cluster_source_range(cluster_range.clone(), &source_positions)
                    };
                    let cluster_text_without_synthetic = text_without_synthetic_join_controls(
                        &text,
                        cluster_range.clone(),
                        &shaping_contexts,
                    );
                    let cleaned_cluster_text =
                        text_without_glyph_output_controls(&cluster_text_without_synthetic);
                    let default_ignorable_only = cluster_is_default_ignorable_only(
                        raw_cluster_text,
                        cleaned_cluster_text.as_ref(),
                    );
                    // A visible contextual glyph may be clustered with an
                    // authored or virtual join control. Emit the adjacent
                    // authored typographic unit as its Unicode payload; the
                    // control itself remains source text only.
                    let contextual_source_text = default_ignorable_only
                        .then(|| {
                            source_range
                                .as_ref()
                                .and_then(|range| authored_text.get(range.clone()))
                        })
                        .flatten();
                    if default_ignorable_only
                        && !default_ignorable_cluster_has_shaping_glyph(
                            &face,
                            raw_cluster_text,
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
                        glyph_tab_styles.push(cluster_style);
                        continue;
                    }
                    let mut first_cluster_glyph = true;
                    for glyph in cluster.glyphs() {
                        let Ok(glyph_id) = u16::try_from(glyph.id) else {
                            continue;
                        };
                        let candidate_unicode = if first_cluster_glyph {
                            if default_ignorable_only {
                                String::new()
                            } else {
                                cleaned_cluster_text.as_ref().to_owned()
                            }
                        } else {
                            String::new()
                        };
                        if glyph_is_join_control_artifact(
                            &face,
                            glyph_id,
                            &candidate_unicode,
                            raw_cluster_text,
                        ) {
                            continue;
                        }
                        if glyph_is_non_painting_shaping_artifact(
                            &face,
                            glyph_id,
                            glyph.advance,
                            &candidate_unicode,
                        ) {
                            continue;
                        }
                        let unicode = if first_cluster_glyph && default_ignorable_only {
                            contextual_source_text.unwrap_or_default().to_owned()
                        } else {
                            candidate_unicode
                        };
                        first_cluster_glyph = false;
                        glyphs.push(RenderedGlyph {
                            kind: RenderedGlyphKind::Paint(glyph_id),
                            x_advance: glyph.advance,
                            nominal_x_advance: face
                                .glyph_hor_advance(ttf_parser::GlyphId(glyph_id))
                                .map(|advance| advance as f32 * scale)
                                .unwrap_or(glyph.advance),
                            x_offset: glyph.x,
                            y_offset: -glyph.y,
                            unicode,
                        });
                        glyph_source_ranges.push(source_range.clone());
                        glyph_palettes.push(cluster_palette.clone());
                        glyph_tab_styles.push(cluster_style);
                    }
                }
                if glyphs.is_empty() {
                    continue;
                }
                if let Some(contextual_glyphs) = contextual_glyphs
                    && contextual_glyphs.len() == glyphs.len()
                {
                    // Preserve the ranged backend's paint origin and authored
                    // cluster ownership, but replace its isolated glyph forms
                    // with forms selected in the complete logical context.
                    for (glyph, contextual) in glyphs.iter_mut().zip(contextual_glyphs) {
                        let unicode = std::mem::take(&mut glyph.unicode);
                        *glyph = contextual;
                        glyph.unicode = unicode;
                    }
                }
                let mut font_size = run.font_size();
                apply_synthetic_position_fallback(
                    &mut glyphs,
                    &mut font_size,
                    run_style.authored(),
                    &face,
                    &run_text,
                );
                debug_assert_eq!(glyphs.len(), glyph_source_ranges.len());
                debug_assert_eq!(glyphs.len(), glyph_palettes.len());
                debug_assert_eq!(glyphs.len(), glyph_tab_styles.len());
                let mut group_palette = glyph_palettes[0].clone();
                let mut group_style = glyph_tab_styles[0];
                let mut group_glyphs = Vec::new();
                let mut group_source_ranges = Vec::new();
                let mut group_text = String::new();
                let mut group_x_offset = x_offset;
                let mut next_x_offset = x_offset;
                let mut group_has_tab = false;
                for (((glyph, source_range), palette), glyph_style) in glyphs
                    .into_iter()
                    .zip(glyph_source_ranges)
                    .zip(glyph_palettes)
                    .zip(glyph_tab_styles)
                {
                    // A tab's `tab-size` belongs to the style which owns the
                    // tab, but ordinary paint-equivalent style boundaries do
                    // not split the durable shaping run. In particular, a
                    // join control may have a distinct authored font family
                    // while remaining part of its neighbours' one shaping
                    // context.
                    if palette != group_palette
                        || (!std::ptr::eq(glyph_style, group_style)
                            && (glyph.unicode == "\t" || group_has_tab))
                    {
                        rendered_runs.push(ShapedGlyphRun {
                            text: std::mem::take(&mut group_text).into(),
                            x_offset: group_x_offset,
                            y_offset: 0.0,
                            text_matrix: RenderedTextMatrix::IDENTITY,
                            font_size,
                            font_id: Some(font_id),
                            font_palette: group_palette.clone(),
                            glyphs: std::mem::take(&mut group_glyphs),
                            glyph_source_ranges: std::mem::take(&mut group_source_ranges),
                        });
                        tab_contexts.push(RenderedRunTabContext {
                            style: group_style,
                            metric_style: tab_metric_style,
                        });
                        group_palette = palette.clone();
                        group_style = glyph_style;
                        group_x_offset = next_x_offset;
                        group_has_tab = false;
                    }
                    next_x_offset += glyph.x_advance;
                    group_text.push_str(&glyph.unicode);
                    group_has_tab |= glyph.unicode == "\t";
                    group_glyphs.push(glyph);
                    group_source_ranges.push(source_range);
                }
                rendered_runs.push(ShapedGlyphRun {
                    text: group_text.into(),
                    x_offset: group_x_offset,
                    y_offset: 0.0,
                    text_matrix: RenderedTextMatrix::IDENTITY,
                    font_size,
                    font_id: Some(font_id),
                    font_palette: group_palette,
                    glyphs: group_glyphs,
                    glyph_source_ranges: group_source_ranges,
                });
                tab_contexts.push(RenderedRunTabContext {
                    style: group_style,
                    metric_style: tab_metric_style,
                });
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
                    if grouped_glyph_text != authored_run_text {
                        rendered_runs[rendered_run_start].text = authored_run_text.into();
                    }
                }
            }
            for run in &mut rendered_runs {
                run.x_offset = corrected_visual_inline_position(
                    ShaperVisualInlinePosition::from_parley(run.x_offset),
                    &dropped_default_ignorable_runs,
                )
                .points();
            }
            stitch_dropped_join_control_runs(&mut rendered_runs, &dropped_default_ignorable_runs);
            for index in rehomed_control_fallback_runs.into_iter().rev() {
                if stitch_rehomed_control_fallback_run(&mut rendered_runs, index) {
                    tab_contexts.remove(index + 1);
                }
            }
            this.apply_css_tab_stops(
                &mut rendered_runs,
                &tab_contexts,
                tab_origin,
                letter_spacing,
            );
            rendered_runs
        })
    }

    /// Resolve authored styled spans through `unicode-range` face selection.
    ///
    /// The selected face is a shaping input, while the authored style remains
    /// the source of line metrics such as `font-size-adjust: from-font`.
    /// <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>
    fn resolved_styled_text_spans<'a>(
        &mut self,
        spans: &[StyledTextSpan<'a>],
    ) -> Vec<ResolvedStyledTextSpan<'a>> {
        // The common case emits exactly one resolved span for each authored
        // span. Reserve that shape up front, then reserve a known expansion
        // before appending a `unicode-range` split so this durable result does
        // not repeatedly grow while building Parley's input.
        let mut resolved_spans = Vec::with_capacity(spans.len());
        for span in spans.iter().filter(|span| !span.text.is_empty()) {
            if let Some(unicode_range_resolved) =
                self.unicode_range_resolved_text_spans(span.text, span.style)
            {
                resolved_spans.reserve(unicode_range_resolved.len());
                for resolved in unicode_range_resolved {
                    if let Some(text) = span.text.get(resolved.range) {
                        resolved_spans.push(ResolvedStyledTextSpan {
                            text,
                            shaping_style: SelectedFaceStyleView::new(
                                span.style,
                                resolved.selected_family,
                            ),
                            metric_style: span.style,
                        });
                    }
                }
            } else {
                resolved_spans.push(ResolvedStyledTextSpan {
                    text: span.text,
                    shaping_style: SelectedFaceStyleView::new(span.style, None),
                    metric_style: span.style,
                });
            }
        }
        resolved_spans
    }

    /// Build Parley's styled input and retain a map to authored CSS text.
    ///
    /// Join controls and emoji presentation selectors influence font and
    /// OpenType selection without becoming independently selectable source
    /// text. This method makes that internal/authored coordinate boundary
    /// explicit before any Parley cluster is converted to a durable glyph run.
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order>
    fn mapped_styled_shaping_text<'span, 'source>(
        resolved_spans: &'span [ResolvedStyledTextSpan<'source>],
    ) -> Option<MappedStyledShapingText<'span, 'source>> {
        let mut ranges = Vec::with_capacity(resolved_spans.len());
        let mut metric_ranges = Vec::with_capacity(resolved_spans.len());
        let mut shaping_contexts = ShapingContextMap::default();
        let authored_text = resolved_spans
            .iter()
            .map(|span| span.text)
            .collect::<String>();

        // Join controls are default-ignorable shaping instructions, not
        // visible font content. Their own inline style must therefore not
        // create a separate font-selection range: an author may isolate a
        // U+200C/U+200D in a span with a deliberately unrelated fallback
        // face, while the adjacent Arabic text still shapes as one context.
        // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
        // <https://www.w3.org/TR/alreq/#h_joining-enforcement>
        let mut shaping_spans = Vec::<StyledShapingSpan<'_, '_>>::new();
        let mut pending_join_controls = String::new();
        for span in resolved_spans {
            // Default-ignorables are font-neutral, but not source-neutral.
            // Excluding them from Parley's font-selection buffer avoids
            // giving it a second line-breaking authority. They remain in
            // `authored_text`, and `styled_text_source_positions` restores
            // their authored byte coordinates when clusters are selected.
            // In particular, a standalone U+200B must never turn into an
            // empty Parley style range.
            let span_text = text_without_font_neutral_default_ignorables(span.text);
            let span_text = span_text.as_ref();
            if span_text.is_empty() {
                continue;
            }
            if span_text.chars().all(character_is_join_control) {
                if let Some(previous) = shaping_spans.last_mut() {
                    previous.text.push_str(span_text);
                } else {
                    pending_join_controls.push_str(span_text);
                }
                continue;
            }
            let mut text = std::mem::take(&mut pending_join_controls);
            text.push_str(span_text);
            if let Some(previous) = shaping_spans.last_mut()
                && previous
                    .shaping_style
                    .has_same_effective_style(&span.shaping_style)
                && previous.metric_style == span.metric_style
                // Equal shaping inputs do not imply that there was no CSS
                // inline boundary. Keep independently authored styles as
                // separate shaping spans so boundary shaping can add its
                // synthetic context before the backend sees a flat string.
                // Unicode-range resolution may split one authored style; its
                // pieces retain the same style identity and may still merge.
                // <https://drafts.csswg.org/css-text-3/#boundary-shaping>
                && std::ptr::eq(previous.shaping_style.authored(), span.shaping_style.authored())
                && std::ptr::eq(previous.metric_style, span.metric_style)
            {
                previous.text.push_str(&text);
            } else {
                shaping_spans.push(StyledShapingSpan {
                    text,
                    shaping_style: &span.shaping_style,
                    metric_style: span.metric_style,
                });
            }
        }
        if !pending_join_controls.is_empty()
            && let Some(previous) = shaping_spans.last_mut()
        {
            previous.text.push_str(&pending_join_controls);
        }

        let mut text = String::new();
        for (index, span) in shaping_spans.iter().enumerate() {
            let start = text.len();
            if index > 0
                && shaping_spans.get(index - 1).is_some_and(|previous| {
                    matches!(
                        previous.shaping_style.boundary_effect(span.shaping_style),
                        InlineBoundaryEffect::ShapingInputChange
                    ) && span_boundary_needs_join_control(&previous.text, &span.text)
                })
            {
                push_synthetic_join_control(&mut text, &mut shaping_contexts);
            }
            push_text_with_font_variant_emoji(&mut text, &span.text, span.shaping_style.authored());
            if shaping_spans.get(index + 1).is_some_and(|next| {
                matches!(
                    next.shaping_style.boundary_effect(span.shaping_style),
                    InlineBoundaryEffect::ShapingInputChange
                ) && span_boundary_needs_join_control(&span.text, &next.text)
            }) {
                push_synthetic_join_control(&mut text, &mut shaping_contexts);
            }
            ranges.push((start..text.len(), span.shaping_style));
            metric_ranges.push((start..text.len(), span.metric_style));
        }
        if text.is_empty() || ranges.is_empty() {
            return None;
        }
        shaping_contexts.add_authored_join_controls(&text);
        debug_assert!(ranges.iter().all(|(range, _)| {
            range.start < range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end)
        }));
        let source_positions =
            styled_text_source_positions(&text, &authored_text, &shaping_contexts);
        Some(MappedStyledShapingText {
            text,
            authored_text,
            ranges,
            metric_ranges,
            shaping_contexts,
            source_positions,
        })
    }

    /// Shape a selected font-face range with its complete cursive context.
    ///
    /// CSS Fonts limits the face that paints a character, not the neighboring
    /// characters that OpenType sees while selecting its contextual form.
    /// A ranged backend style otherwise shapes every face run in isolation,
    /// which makes an Arabic letter lose its joining form when its neighbors
    /// are selected from a different `unicode-range` face.  The returned
    /// glyphs retain only the selected source range; callers keep the
    /// backend's range placement and authored source provenance.
    /// <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>
    /// <https://www.w3.org/TR/css-text-3/#boundary-shaping>
    fn selected_face_contextual_glyphs(
        &mut self,
        text: &str,
        range: Range<usize>,
        style: &SelectedFaceStyleView<'_>,
        has_cursive_face_transition: bool,
        letter_spacing: ShapingLetterSpacing,
    ) -> Option<Vec<RenderedGlyph>> {
        has_cursive_face_transition.then_some(())?;
        let range_text = text.get(range.clone())?;
        range_text
            .chars()
            .any(character_has_cursive_shaping_behavior)
            .then_some(())?;

        let mut contextual_style = style.authored().clone();
        contextual_style.font_family = style.font_family().clone();
        // This cannot borrow the reusable layout scratch: callers are
        // converting a borrowed Parley line from the primary ranged pass.
        // A small local layout keeps the contextual probe independent.
        let parley_style = self.shaping_style_for_selected_face_view(style);
        let font_family_source = self.resolved_parley_font_family_source_for_family(
            style.font_family(),
            contextual_style.font_weight,
            contextual_style.font_style,
            contextual_style.font_width,
        );
        let feature_context = self.font_feature_context_for_selected_face(style);
        let mut layout = ParleyLayout::default();
        let mut builder: parley::RangedBuilder<'_, FontPalette> = self
            .parley_layout_context
            .ranged_builder(&mut self.parley_font_context, text, 1.0, false);
        push_parley_default_style(&mut builder, &parley_style, &font_family_source);
        push_parley_text_spacing_default_with_context(
            &mut builder,
            text,
            &contextual_style,
            letter_spacing.requested_for(&contextual_style),
            feature_context.as_ref(),
        );
        builder.build_into(&mut layout, text);
        layout.break_all_lines(None);
        let line = layout.lines().next()?;
        let glyphs = self
            .rendered_text_runs_for_parley_line(text, line, &contextual_style, letter_spacing)
            .into_iter()
            .flat_map(|run| run.glyphs.into_iter().zip(run.glyph_source_ranges))
            .filter_map(|(glyph, source_range)| {
                source_range
                    .is_some_and(|source_range| {
                        source_range.start < range.end && range.start < source_range.end
                    })
                    .then_some(glyph)
            })
            .collect::<Vec<_>>();
        (!glyphs.is_empty()).then_some(glyphs)
    }

    /// Select concrete families for presentation-sensitive grapheme clusters.
    ///
    /// CSS Fonts matches a variation selector with its preceding base, rather
    /// than selecting one face for an entire text run. This is observable when
    /// a keycap or emoji occurs after ordinary text in the same run.
    /// <https://www.w3.org/TR/css-fonts-4/#font-variant-emoji-prop>
    /// <https://www.w3.org/TR/css-fonts-4/#cluster-matching>
    pub(in crate::text) fn emoji_presentation_family_ranges(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        family: &FontFamily,
    ) -> Vec<EmojiPresentationFamilyRange> {
        let boundaries = GraphemeClusterSegmenter::new()
            .segment_str(text)
            .collect::<Vec<_>>();
        let mut ranges = Vec::new();
        for range in boundaries.windows(2).map(|pair| pair[0]..pair[1]) {
            let cluster = &text[range.clone()];
            let characters = cluster
                .chars()
                .filter(|character| !character_is_bidi_format_control(*character))
                .collect::<Vec<_>>();
            let Some((base_index, base)) =
                characters
                    .iter()
                    .copied()
                    .enumerate()
                    .find(|(_, character)| {
                        emoji_presentation_participating_code_point(*character)
                            || character_is_emoji(*character)
                    })
            else {
                continue;
            };
            let selector = characters[base_index + 1..]
                .iter()
                .copied()
                .find(|character| matches!(character, '\u{fe0e}' | '\u{fe0f}'));
            let requested = match selector {
                Some('\u{fe0e}') => EmojiPresentationCapability::Text,
                Some('\u{fe0f}') => EmojiPresentationCapability::Emoji,
                None if style.font_variant_emoji == FontVariantEmoji::Unicode => {
                    if character_has_emoji_presentation(base) {
                        EmojiPresentationCapability::Emoji
                    } else {
                        EmojiPresentationCapability::Text
                    }
                }
                None => {
                    // `normal` deliberately leaves presentation to ordinary
                    // platform matching when no author selector is present.
                    continue;
                }
                Some(_) => unreachable!("only emoji presentation selectors are selected"),
            };
            let selected = self
                .emoji_presentation_font_candidates_for_family(style, family, base)
                .into_iter()
                .find(|font_id| {
                    self.document_fonts
                        .emoji_presentation_capability(*font_id, base, selector)
                        == Some(requested)
                })
                .or_else(|| {
                    // CSS Fonts falls back to the base glyph when no face
                    // supports the selector. Do not force a presentation face
                    // in that case.
                    self.emoji_presentation_font_candidates_for_family(style, family, base)
                        .into_iter()
                        .find(|font_id| self.document_fonts.font_has_character(*font_id, base))
                });
            if let Some(font_id) = selected
                && let Some(font) = self.document_fonts.get(font_id)
            {
                ranges.push(EmojiPresentationFamilyRange {
                    range,
                    source: parley_font_family_source(&FontFamily::named(font.family.clone())),
                });
            }
        }
        ranges
    }

    /// Return the CSS stack's usable faces in precedence order, then the
    /// platform fallback. This is deliberately kept at the font-selection
    /// boundary so that shaping, metric lookup, and PDF embedding refer to
    /// the same concrete document font.
    fn emoji_presentation_font_candidates_for_family(
        &mut self,
        style: &ComputedStyle,
        family: &FontFamily,
        character: char,
    ) -> Vec<usize> {
        let families = match family {
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
        self.visible_text_fallback_for_run_in_family(
            text,
            style,
            &style.font_family,
            current_font_id,
        )
    }

    fn visible_text_fallback_for_run_for_selected_face(
        &mut self,
        text: &str,
        style: &SelectedFaceStyleView<'_>,
        current_font_id: usize,
    ) -> Option<usize> {
        self.visible_text_fallback_for_run_in_family(
            text,
            style.authored(),
            style.font_family(),
            current_font_id,
        )
    }

    fn visible_text_fallback_for_run_in_family(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        family: &FontFamily,
        current_font_id: usize,
    ) -> Option<usize> {
        let mut fallback_font_id = None;
        for character in text
            .chars()
            .filter(|character| !character_is_default_ignorable_code_point(*character))
        {
            let candidate =
                self.resolve_family_fallback_for_character_in_family(style, family, character)?;
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
    shaping_contexts: &ShapingContextMap,
) -> Vec<StyledTextSourcePosition> {
    let mut positions = Vec::new();
    let mut authored = authored_text.char_indices().peekable();
    for (start, character) in internal_text.char_indices() {
        let end = start + character.len_utf8();
        if shaping_contexts.is_synthetic_at(start)
            && shaping_contexts.is_synthetic_at(end.saturating_sub(1))
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

fn styled_authored_text_for_internal_range<'a>(
    internal: Range<usize>,
    positions: &[StyledTextSourcePosition],
    authored_text: &'a str,
) -> Option<&'a str> {
    let authored = styled_cluster_source_range(internal, positions)?;
    authored_text.get(authored)
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

/// Attribute a control-only Parley cluster to the typographic unit whose
/// contextual form it carries.
///
/// HarfBuzz is permitted to associate an Arabic/N'Ko/Mongolian glyph with the
/// following U+200C/U+200D cluster rather than the preceding letter. The
/// control has no layout ownership of its own; for source slicing it belongs
/// to the preceding authored typographic unit, or to the following unit at a
/// stream start. This is input-range provenance, not a glyph-ID heuristic, so
/// it remains valid after OpenType substitution.
/// <https://drafts.csswg.org/css-text-3/#boundary-shaping>
fn styled_join_control_cluster_source_range(
    cluster: Range<usize>,
    positions: &[StyledTextSourcePosition],
    authored_text: &str,
) -> Option<Range<usize>> {
    positions
        .iter()
        .rev()
        .filter(|position| {
            authored_text
                .get(position.authored.clone())
                .is_some_and(|text| !text.chars().all(character_is_join_control))
        })
        .find(|position| position.internal.end <= cluster.start)
        .or_else(|| {
            positions
                .iter()
                .filter(|position| {
                    authored_text
                        .get(position.authored.clone())
                        .is_some_and(|text| !text.chars().all(character_is_join_control))
                })
                .find(|position| cluster.end <= position.internal.start)
        })
        .map(|position| position.authored.clone())
}

mod tests {
    #[cfg(test)]
    use super::{
        InlineBoundaryEffect, SelectedFaceStyleView, ShapingContextMap,
        styled_cluster_source_range, styled_join_control_cluster_source_range,
        styled_text_source_positions,
    };
    #[cfg(test)]
    use crate::CssColor;
    #[cfg(test)]
    use crate::css::{ComputedStyle, FontFamily, FontStyle};

    #[test]
    fn selected_face_view_borrows_the_authored_style_when_unmodified() {
        let style = ComputedStyle::initial();
        let borrowed = SelectedFaceStyleView::new(&style, None);

        assert!(std::ptr::eq(borrowed.authored(), &style));
        assert!(std::ptr::eq(borrowed.font_family(), &style.font_family));

        let selected_family = FontFamily::Serif;
        let selected = SelectedFaceStyleView::new(&style, Some(selected_family.clone()));
        assert_eq!(selected.font_family(), &selected_family);
        assert!(!borrowed.has_same_effective_style(&selected));
    }

    #[test]
    fn boundary_effect_ignores_paint_only_changes_but_tracks_font_style() {
        let base = ComputedStyle::initial();
        let mut color = base.clone();
        color.color = CssColor::new(255, 0, 0);
        let mut italic = base.clone();
        italic.font_style = FontStyle::DEFAULT_OBLIQUE;

        let base_view = SelectedFaceStyleView::new(&base, None);
        let color_view = SelectedFaceStyleView::new(&color, None);
        let italic_view = SelectedFaceStyleView::new(&italic, None);

        assert_eq!(
            base_view.boundary_effect(&color_view),
            InlineBoundaryEffect::PaintOnly
        );
        assert_eq!(
            base_view.boundary_effect(&italic_view),
            InlineBoundaryEffect::ShapingInputChange
        );
    }

    #[test]
    fn styled_cluster_ranges_exclude_synthetic_join_context() {
        let internal = "ا\u{200d}ل\u{0640}سلا\u{200d}م";
        let synthetic = ShapingContextMap::from_synthetic_ranges(&[2..5, 7..9, 15..18]);
        let positions = styled_text_source_positions(internal, "السلام", &synthetic);

        assert_eq!(styled_cluster_source_range(5..7, &positions), Some(2..4),);
        assert_eq!(styled_cluster_source_range(9..13, &positions), Some(4..8),);
        assert_eq!(styled_cluster_source_range(2..5, &positions), None,);
    }

    #[test]
    fn join_control_cluster_borrows_the_preceding_authored_unit() {
        let positions = styled_text_source_positions(
            "ع\u{200d}\u{200d}ع\u{200d}\u{200d}ع",
            "ع\u{200d}\u{200d}ع\u{200d}\u{200d}ع",
            &ShapingContextMap::default(),
        );

        assert_eq!(
            styled_join_control_cluster_source_range(
                13..16,
                &positions,
                "ع\u{200d}\u{200d}ع\u{200d}\u{200d}ع"
            ),
            Some(8..10)
        );
        assert_eq!(
            styled_join_control_cluster_source_range(
                5..8,
                &positions,
                "ع\u{200d}\u{200d}ع\u{200d}\u{200d}ع"
            ),
            Some(0..2)
        );
    }
}
