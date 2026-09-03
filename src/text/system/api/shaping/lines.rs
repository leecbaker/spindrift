use std::borrow::Cow;
use std::rc::Rc;

use icu_segmenter::GraphemeClusterSegmenter;

use super::super::*;
#[cfg(test)]
use crate::text::trim_trailing_css_hanging_space_separators;
use crate::units::SemanticLengthExt;

/// Formatting controls used to preserve an already-resolved visual order while
/// shaping without starting a second UAX #9 paragraph.
const VISUAL_ORDER_GUARD_PREFIX: &str = "\u{202d}";
const VISUAL_ORDER_GUARD_SUFFIX: &str = "\u{202c}";
/// Keep source-shaping reuse bounded for documents with many unique long
/// strings. The cache is an optimization only; eviction cannot affect layout.
const UNTRACKED_INLINE_LINE_CACHE_CAPACITY: usize = 64;
const UNTRACKED_STYLED_INLINE_LINE_CACHE_CAPACITY: usize = 32;

/// Return the OpenType shaping direction selected by UAX #9 for a visual run.
///
/// CSS `direction` establishes the paragraph level, while UAX #9 then
/// resolves a level for each visual run. Joining scripts must be shaped in
/// logical character order using that resolved run direction, or a strong RTL
/// run in an LTR paragraph receives incorrect contextual forms:
/// <https://drafts.csswg.org/css-writing-modes-4/#bidi-algo> and
/// <https://www.unicode.org/reports/tr9/#Reordering_Resolved_Levels>.
fn resolved_bidi_shaping_direction(direction: ResolvedBidiDirection) -> Direction {
    match direction {
        ResolvedBidiDirection::Ltr => Direction::Ltr,
        ResolvedBidiDirection::Rtl => Direction::Rtl,
    }
}

/// Return whether a visual bidi slice must be shaped from its logical source.
///
/// Cursive characters need their resolved UAX #9 direction while shaping to
/// retain their contextual forms. Join controls have the same requirement:
/// U+200C/U+200D can carry the only shaping context in a slice whose visible
/// characters are Arabic Presentation Forms, which are deliberately
/// `Joining_Type=Non_Joining`. Such a slice must not enter the LRO-guarded
/// already-visual-order path.
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.unicode.org/reports/tr9/#L2>.
fn text_requires_logical_bidi_shaping(text: &str) -> bool {
    text.chars().any(|character| {
        character_has_cursive_shaping_behavior(character) || character_is_join_control(character)
    })
}

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

    /// Return visual text ranges for one selected logical bidi line.
    ///
    /// CSS Writing Modes delegates inline bidirectional reordering to the
    /// Unicode Bidirectional Algorithm. The caller supplies the resolved CSS
    /// paragraph direction; CSS `unicode-bidi` controls are already present
    /// in the selected logical source:
    /// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi> and
    /// <https://www.unicode.org/reports/tr9/>.
    pub(crate) fn visual_ranges_for_unwrapped_text(
        &mut self,
        text: &str,
        base_direction: Direction,
    ) -> Vec<BidiVisualRange> {
        resolve_bidi_visual_ranges(text, base_direction)
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
        self.with_reusable_parley_layout(|this, layout| {
            let parley_style = this.shaping_style_for_selected_face(style);
            let feature_context = this.font_feature_context_for_style(style);
            let font_family_source = this.resolved_parley_font_family_source(style);
            let emoji_family_ranges =
                this.emoji_presentation_family_ranges(text, style, &style.font_family);
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
                let Some(line) = layout.lines().next() else {
                    return Vec::new();
                };
                return this.rendered_text_runs_for_parley_line(text, line, style, letter_spacing);
            }
            this.rendered_text_runs_for_parley_line(text, line, style, letter_spacing)
        })
    }

    pub(crate) fn shape_unwrapped_line(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
    ) -> Option<ShapedInlineLine> {
        self.shape_text_request(TextShapingRequest::from_html_computed_style(
            text,
            style,
            line_height,
        ))
    }

    /// Shape one source-neutral text request through Spindrift's document font
    /// registry.
    ///
    /// This deliberately performs no HTML line-box construction. Callers
    /// retain their own placement model around the returned glyph stream;
    /// SVG therefore reaches the exact same face selection, fallback,
    /// shaping, and PDF-subset identities as HTML.
    pub(crate) fn shape_text_request(
        &mut self,
        request: TextShapingRequest<'_>,
    ) -> Option<ShapedInlineLine> {
        self.shape_unwrapped_line_with_letter_spacing(
            request.text,
            request.style,
            request.line_height,
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
        let typesetting_plan = TextTypesettingPlan::resolve(text, style);
        self.apply_upright_vertical_metrics(&mut runs, &typesetting_plan);
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
            typesetting_plan,
            runs,
            monotonic_source_advance_index: Default::default(),
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
    /// untracked glyph stream and stores boundary spacing separately from the
    /// shaped base advance, after line selection and bidi reordering:
    /// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
    pub(crate) fn shape_untracked_inline_line(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
    ) -> Option<ShapedInlineLine> {
        if let Some(entry) = self.untracked_inline_line_cache.iter().rev().find(|entry| {
            entry.text.as_ref() == text
                && entry.line_height_bits == line_height.to_bits()
                && entry.style.as_ref() == style
        }) {
            return entry.shaped.clone();
        }
        let shaped = self.shape_unwrapped_line_with_letter_spacing(
            text,
            style,
            line_height,
            ShapingLetterSpacing::Suppressed,
        );
        if !text.is_empty() {
            if self.untracked_inline_line_cache.len() == UNTRACKED_INLINE_LINE_CACHE_CAPACITY {
                self.untracked_inline_line_cache.remove(0);
            }
            self.untracked_inline_line_cache
                .push(UntrackedInlineLineCacheEntry {
                    text: Rc::from(text),
                    style: Rc::new(style.clone()),
                    line_height_bits: line_height.to_bits(),
                    shaped: shaped.clone(),
                });
        }
        shaped
    }

    /// Shape a graph fragment with an exact immutable style identity.
    ///
    /// Collected words from one inline scope share the same `Rc` style. This
    /// small cache therefore removes repeated shaping of recurring words
    /// without guessing which subset of computed CSS affects a glyph stream.
    pub(crate) fn shape_untracked_inline_line_with_style_identity(
        &mut self,
        text: &str,
        style: &Rc<ComputedStyle>,
        line_height: f32,
    ) -> Option<ShapedInlineLine> {
        if let Some(entry) = self.untracked_inline_line_cache.iter().rev().find(|entry| {
            entry.text.as_ref() == text
                && entry.line_height_bits == line_height.to_bits()
                && (Rc::ptr_eq(&entry.style, style) || entry.style.as_ref() == style.as_ref())
        }) {
            return entry.shaped.clone();
        }
        self.shape_untracked_inline_line(text, style, line_height)
    }

    /// Shape a logical selected-line slice with its UAX #9 scope restored.
    ///
    /// Line breaking may end inside an authored or CSS-generated isolate.
    /// The selected source alone is then not a valid UBA paragraph: shaping
    /// it directly can make an unclosed isolate consume the remainder of the
    /// backend line and corrupt its measured advance. The graph supplies the
    /// non-painting context needed to balance that slice. Provenance is
    /// remapped to the authored range before the result is retained as a
    /// layout artifact.
    /// <https://www.unicode.org/reports/tr9/#Explicit_Levels_and_Directions>
    pub(crate) fn shape_bidi_scoped_logical_line(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
        prefix: &str,
        suffix: &str,
    ) -> Option<ShapedInlineLine> {
        if prefix.is_empty() && suffix.is_empty() {
            return self.shape_untracked_inline_line(text, style, line_height);
        }
        let authored_start = prefix.len();
        let authored_end = authored_start + text.len();
        let mut scoped_text = String::with_capacity(prefix.len() + text.len() + suffix.len());
        scoped_text.push_str(prefix);
        scoped_text.push_str(text);
        scoped_text.push_str(suffix);
        self.shape_untracked_inline_line(&scoped_text, style, line_height)
            .map(|mut shaped| {
                shaped.text = Rc::from(text);
                remap_shaped_source_ranges_to_authored_slice(
                    &mut shaped.runs,
                    authored_start,
                    authored_end,
                );
                strip_bidi_format_controls_from_shaped_runs(&mut shaped.runs);
                shaped.width = shaped.advance_width();
                shaped
            })
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
    #[allow(dead_code)] // Computed mode remains available for legacy artifacts.
    pub(crate) fn shape_visual_ordered_line(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
        resolved_direction: ResolvedBidiDirection,
    ) -> Option<ShapedInlineLine> {
        self.shape_visual_ordered_line_with_letter_spacing(
            text,
            style,
            line_height,
            resolved_direction,
            ShapingLetterSpacing::Computed,
        )
    }

    /// Shape an already-resolved visual line without backend-owned tracking.
    pub(crate) fn shape_untracked_visual_ordered_line(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
        resolved_direction: ResolvedBidiDirection,
    ) -> Option<ShapedInlineLine> {
        self.shape_visual_ordered_line_with_letter_spacing(
            text,
            style,
            line_height,
            resolved_direction,
            ShapingLetterSpacing::Suppressed,
        )
    }

    fn shape_visual_ordered_line_with_letter_spacing(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
        resolved_direction: ResolvedBidiDirection,
        letter_spacing: ShapingLetterSpacing,
    ) -> Option<ShapedInlineLine> {
        // UAX #9 visual reordering and OpenType's cursive shaping direction
        // are separate inputs. An LTR override preserves the already-resolved
        // order of neutral punctuation, but would make HarfBuzz shape Arabic
        // and other joining scripts left-to-right. Those scripts retain
        // logical character order, but their OpenType shaping direction must
        // be the UAX #9-resolved direction rather than the CSS paragraph
        // direction:
        // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
        // <https://www.unicode.org/reports/tr9/#reordering-resolved-levels>.
        // Parley's paragraph builder does not retain an LRO as a visual-order
        // barrier for every astral RTL script. A selected LTR override is
        // already in display order, so shape its independent typographic
        // units in that order rather than permitting a second UBA pass to
        // reverse the full run. This retains cluster integrity while avoiding
        // a per-scalar fallback for combining sequences.
        if resolved_direction == ResolvedBidiDirection::Ltr
            && resolve_bidi_visual_ranges(text, Direction::Ltr)
                .iter()
                .any(|range| range.direction == ResolvedBidiDirection::Rtl)
        {
            return self.shape_already_ordered_ltr_units(text, style, line_height, letter_spacing);
        }
        if text_requires_logical_bidi_shaping(text) {
            let logical_paint_style = Self::visual_bidi_paint_style(
                style,
                resolved_bidi_shaping_direction(resolved_direction),
            );
            return self.shape_unwrapped_line_with_letter_spacing(
                text,
                &logical_paint_style,
                line_height,
                letter_spacing,
            );
        }
        let visual_paint_style = Self::visual_bidi_paint_style(style, style.used_direction());
        let mut guarded_text = String::with_capacity(
            text.len() + VISUAL_ORDER_GUARD_PREFIX.len() + VISUAL_ORDER_GUARD_SUFFIX.len(),
        );
        guarded_text.push_str(VISUAL_ORDER_GUARD_PREFIX);
        guarded_text.push_str(text);
        guarded_text.push_str(VISUAL_ORDER_GUARD_SUFFIX);
        self.shape_unwrapped_line_with_letter_spacing(
            &guarded_text,
            &visual_paint_style,
            line_height,
            letter_spacing,
        )
        .map(|mut shaped| {
            shaped.text = Rc::from(text);
            remap_visual_order_guard_source_ranges(&mut shaped.runs, text.len());
            rebase_guarded_visual_run_origins(&mut shaped.runs);
            strip_bidi_format_controls_from_shaped_runs(&mut shaped.runs);
            self.apply_resolved_bidi_glyph_mirroring(&mut shaped, resolved_direction);
            shaped
        })
    }

    /// Shape a visual LTR override one typographic unit at a time.
    ///
    /// The caller has already applied UAX #9 L2 and requires this order to be
    /// preserved. Keeping a unit intact preserves combining sequences and
    /// CSS Text's joining-boundary invariant while avoiding a backend UBA
    /// pass over the complete preordered RTL sequence.
    fn shape_already_ordered_ltr_units(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        line_height: f32,
        letter_spacing: ShapingLetterSpacing,
    ) -> Option<ShapedInlineLine> {
        let visual_style = Self::visual_bidi_paint_style(style, Direction::Ltr);
        let mut runs = Vec::new();
        let mut width = 0.0;
        let boundaries = GraphemeClusterSegmenter::new()
            .segment_str(text)
            .collect::<Vec<_>>();
        for range in boundaries.windows(2).map(|pair| pair[0]..pair[1]) {
            let mut unit = self.shape_unwrapped_line_with_letter_spacing(
                &text[range.clone()],
                &visual_style,
                line_height,
                letter_spacing,
            )?;
            for run in &mut unit.runs {
                run.x_offset += width;
                for glyph in &mut run.glyphs {
                    if let Some(source_range) = &mut glyph.source_range {
                        source_range.start += range.start;
                        source_range.end += range.start;
                    }
                }
            }
            width += unit.advance_width();
            runs.extend(unit.runs);
        }
        (!runs.is_empty()).then(|| {
            let baseline_adjustment = self
                .shaped_runs_baseline_adjustment(&runs, style, line_height)
                .points();
            ShapedInlineLine {
                text: Rc::from(text),
                width,
                offset: 0.0,
                aligned_by_parley: false,
                line_height,
                baseline_adjustment,
                typesetting_plan: TextTypesettingPlan::resolve(text, &visual_style),
                runs,
                monotonic_source_advance_index: Default::default(),
            }
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
    #[allow(dead_code)] // Computed mode remains available for legacy artifacts.
    pub(crate) fn shape_styled_inline_fragments(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        text_summary: String,
        width: f32,
        line_height: f32,
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
    ) -> Option<ShapedInlineLine> {
        self.shape_styled_inline_fragments_with_letter_spacing(
            spans,
            text_summary,
            width,
            line_height,
            tab_origin,
            tab_metric_style,
            ShapingLetterSpacing::Computed,
        )
    }

    /// Shape formatted inline fragments without backend-owned tracking.
    ///
    /// The graph preserves this glyph stream through selection, visual bidi
    /// ordering, tab remeasurement, and paint preparation. CSS Text tracking
    /// is added only by `apply_visual_tracking_boundaries` after that order is
    /// final.
    pub(crate) fn shape_untracked_styled_inline_fragments(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        text_summary: String,
        width: f32,
        line_height: f32,
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
    ) -> Option<ShapedInlineLine> {
        self.shape_styled_inline_fragments_with_letter_spacing(
            spans,
            text_summary,
            width,
            line_height,
            tab_origin,
            tab_metric_style,
            ShapingLetterSpacing::Suppressed,
        )
    }

    /// Shape a complete graph source stream using an exact ordered list of
    /// immutable span styles as its cache key.
    ///
    /// The cache is intentionally narrower than the generic styled shaper:
    /// graph construction owns `Rc<ComputedStyle>` for every source span and
    /// can therefore prove that no shaping-affecting CSS input was guessed.
    pub(crate) fn shape_untracked_styled_inline_fragments_with_style_identities(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        text_summary: String,
        line_height: f32,
        tab_metric_style: &ComputedStyle,
        span_styles: &[Rc<ComputedStyle>],
    ) -> Option<ShapedInlineLine> {
        debug_assert_eq!(spans.len(), span_styles.len());
        debug_assert!(
            spans
                .iter()
                .zip(span_styles)
                .all(|(span, style)| std::ptr::eq(span.style, style.as_ref()))
        );
        let uniform_style = span_styles.first().filter(|first| {
            span_styles
                .iter()
                .all(|style| style.as_ref() == first.as_ref())
        });
        if let Some(entry) = self
            .untracked_styled_inline_line_cache
            .iter()
            .rev()
            .find(|entry| {
                entry.text.as_ref() == text_summary
                    && entry.line_height_bits == line_height.to_bits()
                    && entry.tab_metric_style == *tab_metric_style
                    && (entry
                        .uniform_style
                        .as_ref()
                        .zip(uniform_style)
                        .is_some_and(|(cached, style)| cached == style.as_ref())
                        || (entry.uniform_style.is_none()
                            && uniform_style.is_none()
                            && entry.span_styles.len() == span_styles.len()
                            && entry
                                .span_styles
                                .iter()
                                .zip(span_styles)
                                .all(|(cached, style)| Rc::ptr_eq(cached, style))))
            })
        {
            return entry.shaped.clone();
        }
        let shaped = self.shape_untracked_styled_inline_fragments(
            spans,
            text_summary.clone(),
            0.0,
            line_height,
            0.0,
            tab_metric_style,
        );
        if !text_summary.is_empty() {
            if self.untracked_styled_inline_line_cache.len()
                == UNTRACKED_STYLED_INLINE_LINE_CACHE_CAPACITY
            {
                self.untracked_styled_inline_line_cache.remove(0);
            }
            self.untracked_styled_inline_line_cache
                .push(UntrackedStyledInlineLineCacheEntry {
                    text: text_summary.into(),
                    uniform_style: uniform_style.map(|style| style.as_ref().clone()),
                    span_styles: if uniform_style.is_none() {
                        span_styles.to_vec()
                    } else {
                        Vec::new()
                    },
                    line_height_bits: line_height.to_bits(),
                    tab_metric_style: tab_metric_style.clone(),
                    shaped: shaped.clone(),
                });
        }
        shaped
    }

    #[allow(clippy::too_many_arguments)] // Explicit shaping inputs preserve CSS line context.
    fn shape_styled_inline_fragments_with_letter_spacing(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        text_summary: String,
        width: f32,
        line_height: f32,
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
        letter_spacing: ShapingLetterSpacing,
    ) -> Option<ShapedInlineLine> {
        if spans.is_empty() {
            return None;
        }
        let first_style = spans.first().map(|span| span.style)?;
        let mut runs = position_shaped_runs(
            self.shape_styled_text_runs_with_parley_at_tab_origin_with_letter_spacing(
                spans,
                tab_origin,
                tab_metric_style,
                letter_spacing,
            ),
        );
        let typesetting_plan = TextTypesettingPlan::resolve(&text_summary, first_style);
        self.apply_upright_vertical_metrics(&mut runs, &typesetting_plan);
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
            typesetting_plan,
            runs,
            monotonic_source_advance_index: Default::default(),
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
    #[allow(dead_code)] // Computed mode remains available for legacy artifacts.
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
        self.shape_visually_ordered_inline_fragments_with_letter_spacing(
            spans,
            text_summary,
            width,
            line_height,
            tab_origin,
            tab_metric_style,
            resolved_direction,
            ShapingLetterSpacing::Computed,
        )
    }

    /// Shape a final visual fragment sequence without backend-owned tracking.
    #[allow(clippy::too_many_arguments)] // Explicit shaping inputs preserve CSS line context.
    pub(crate) fn shape_untracked_visually_ordered_inline_fragments(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        text_summary: String,
        width: f32,
        line_height: f32,
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
        resolved_direction: ResolvedBidiDirection,
    ) -> Option<ShapedInlineLine> {
        self.shape_visually_ordered_inline_fragments_with_letter_spacing(
            spans,
            text_summary,
            width,
            line_height,
            tab_origin,
            tab_metric_style,
            resolved_direction,
            ShapingLetterSpacing::Suppressed,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn shape_visually_ordered_inline_fragments_with_letter_spacing(
        &mut self,
        spans: &[StyledTextSpan<'_>],
        text_summary: String,
        width: f32,
        line_height: f32,
        tab_origin: f32,
        tab_metric_style: &ComputedStyle,
        resolved_direction: ResolvedBidiDirection,
        letter_spacing: ShapingLetterSpacing,
    ) -> Option<ShapedInlineLine> {
        spans.first()?;
        if resolved_direction == ResolvedBidiDirection::Ltr
            && spans.len() == 1
            && spans[0].text == text_summary
            && resolve_bidi_visual_ranges(spans[0].text, Direction::Ltr)
                .iter()
                .any(|range| range.direction == ResolvedBidiDirection::Rtl)
        {
            return self.shape_visual_ordered_line_with_letter_spacing(
                spans[0].text,
                spans[0].style,
                line_height,
                resolved_direction,
                letter_spacing,
            );
        }
        let visual_direction = resolved_bidi_shaping_direction(resolved_direction);
        if spans
            .iter()
            .any(|span| text_requires_logical_bidi_shaping(span.text))
        {
            let logical_paint_styles = spans
                .iter()
                .map(|span| Self::visual_bidi_paint_style(span.style, visual_direction))
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
                Self::visual_bidi_paint_style(tab_metric_style, visual_direction);
            return self.shape_styled_inline_fragments_with_letter_spacing(
                &logical_paint_spans,
                text_summary,
                width,
                line_height,
                tab_origin,
                &logical_tab_metric_style,
                letter_spacing,
            );
        }
        let visual_paint_styles = spans
            .iter()
            .map(|span| Self::visual_bidi_paint_style(span.style, Direction::Ltr))
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
            Self::visual_bidi_paint_style(tab_metric_style, Direction::Ltr);
        let first_style = visual_paint_spans.first()?.style;
        let mut guarded_spans = Vec::with_capacity(spans.len() + 2);
        guarded_spans.push(StyledTextSpan {
            text: VISUAL_ORDER_GUARD_PREFIX,
            style: first_style,
        });
        guarded_spans.extend_from_slice(&visual_paint_spans);
        guarded_spans.push(StyledTextSpan {
            text: VISUAL_ORDER_GUARD_SUFFIX,
            style: first_style,
        });
        let mut guarded_text = String::with_capacity(
            VISUAL_ORDER_GUARD_PREFIX.len() + text_summary.len() + VISUAL_ORDER_GUARD_SUFFIX.len(),
        );
        guarded_text.push_str(VISUAL_ORDER_GUARD_PREFIX);
        guarded_text.push_str(&text_summary);
        guarded_text.push_str(VISUAL_ORDER_GUARD_SUFFIX);
        let mut shaped = self.shape_styled_inline_fragments_with_letter_spacing(
            &guarded_spans,
            guarded_text,
            width,
            line_height,
            tab_origin,
            &visual_tab_metric_style,
            letter_spacing,
        )?;
        let authored_len = text_summary.len();
        remap_visual_order_guard_source_ranges(&mut shaped.runs, authored_len);
        rebase_guarded_visual_run_origins(&mut shaped.runs);
        strip_bidi_format_controls_from_shaped_runs(&mut shaped.runs);
        shaped.text = text_summary.into();
        shaped.typesetting_plan = TextTypesettingPlan::resolve(&shaped.text, first_style);
        shaped.width = shaped.advance_width();
        self.apply_resolved_bidi_glyph_mirroring(&mut shaped, resolved_direction);
        Some(shaped)
    }

    /// Build the shaping style for text whose UAX #9 order has already been
    /// resolved for the enclosing CSS inline sequence.
    ///
    /// The source style continues to select font, feature, spacing, and metric
    /// inputs, but its CSS bidi scope must not be introduced a second time.
    /// Non-joining visual slices are protected by an LRO guard and therefore
    /// use LTR paragraph inputs; logical joining slices instead use the level
    /// resolved for their original run.
    /// <https://drafts.csswg.org/css-writing-modes-4/#bidi-algo> and
    /// <https://www.unicode.org/reports/tr9/#L4>.
    fn visual_bidi_paint_style(style: &ComputedStyle, direction: Direction) -> ComputedStyle {
        let mut visual_style = style.clone();
        visual_style.unicode_bidi = UnicodeBidi::Normal;
        visual_style.direction = direction;
        visual_style
    }

    /// Normalize mirrored glyph presentation from an already-resolved UAX #9
    /// level without changing its Unicode source text or running UAX #9 a
    /// second time.
    ///
    /// Call this exactly once when logical shaping crosses into a selected
    /// visual slice. Cached source slices can have been shaped under an
    /// inline fragment's own paragraph direction before the UBA chooses the
    /// final level. An LTR resolved level must therefore restore the source
    /// glyph as well as an RTL level applying UAX #9 L4:
    /// <https://www.unicode.org/reports/tr9/#L4>.
    pub(crate) fn apply_resolved_bidi_glyph_mirroring(
        &self,
        shaped: &mut ShapedInlineLine,
        resolved_direction: ResolvedBidiDirection,
    ) {
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
                let target_character = match resolved_direction {
                    ResolvedBidiDirection::Ltr => character,
                    ResolvedBidiDirection::Rtl => {
                        bidi_mirroring_glyph(character).unwrap_or(character)
                    }
                };
                if target_character == character && resolved_direction == ResolvedBidiDirection::Ltr
                {
                    continue;
                }
                let Some(target_id) = face.glyph_index(target_character) else {
                    continue;
                };
                let old_nominal = glyph.rendered.nominal_x_advance;
                let extra_advance = glyph.rendered.x_advance - old_nominal;
                let target_nominal = face
                    .glyph_hor_advance(target_id)
                    .map(|advance| advance as f32 * scale)
                    .unwrap_or(old_nominal);
                glyph.rendered.kind = RenderedGlyphKind::Paint(target_id.0);
                glyph.rendered.nominal_x_advance = target_nominal;
                glyph.rendered.x_advance = target_nominal + extra_advance;
            }
        }
    }
}

/// Translate provenance from the temporary LRO/PDF-guarded input to the
/// unguarded text retained by [`ShapedInlineLine`].
///
/// A backend cluster can include an adjacent, zero-source-width formatting
/// control. Intersecting instead of requiring containment preserves the
/// authored portion of that cluster. Guard-only artifacts are non-painting and
/// removed so they cannot make source slicing reject an otherwise complete
/// selection.
fn remap_visual_order_guard_source_ranges(runs: &mut [ShapedInlineRun], authored_len: usize) {
    let authored_start = VISUAL_ORDER_GUARD_PREFIX.len();
    let authored_end = authored_start + authored_len;
    remap_shaped_source_ranges_to_authored_slice(runs, authored_start, authored_end);
}

/// Remove a synthetic bidi guard's shaping-space advance from its authored
/// visual result.
///
/// The LRO/PDF controls are not CSS text and must have no used advance. A
/// font is permitted to assign them a glyph advance, however, and Parley then
/// reports following visual runs relative to that synthetic cluster. Once the
/// guard-only provenance has been removed, normalize the remaining runs back
/// to their authored visual start.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#bidi-algo>
fn rebase_guarded_visual_run_origins(runs: &mut [ShapedInlineRun]) {
    let Some(visual_start) = runs.iter().map(|run| run.x_offset).min_by(f32::total_cmp) else {
        return;
    };
    for run in runs {
        run.x_offset -= visual_start;
    }
}

/// Retain source ownership inside a temporary non-painting bidi wrapper.
fn remap_shaped_source_ranges_to_authored_slice(
    runs: &mut [ShapedInlineRun],
    authored_start: usize,
    authored_end: usize,
) {
    for run in runs {
        for glyph in &mut run.glyphs {
            glyph.source_range = glyph.source_range.take().and_then(|range| {
                let start = range.start.max(authored_start);
                let end = range.end.min(authored_end);
                (start < end).then_some(start - authored_start..end - authored_start)
            });
        }
        run.glyphs
            .retain(|glyph| glyph.source_range.is_some() || glyph.paints);
    }
}

fn strip_bidi_format_controls_from_shaped_runs(runs: &mut [ShapedInlineRun]) {
    for run in runs {
        if let Cow::Owned(text) = text_without_bidi_format_controls(&run.text) {
            run.text = text.into();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_rtl_run_shapes_rtl_even_in_an_ltr_paragraph() {
        assert_eq!(
            resolved_bidi_shaping_direction(ResolvedBidiDirection::Rtl),
            Direction::Rtl
        );
        assert_eq!(
            resolved_bidi_shaping_direction(ResolvedBidiDirection::Ltr),
            Direction::Ltr
        );
    }

    #[test]
    fn join_controls_keep_presentation_form_slices_in_logical_shaping_order() {
        assert!(!cursive_boundary_needs_context('\u{fedf}', '\u{fe8e}'));
        assert!(text_requires_logical_bidi_shaping(
            "\u{fedf}\u{200c}\u{fe8e}"
        ));
    }

    #[test]
    fn stripping_bidi_controls_keeps_control_free_run_text_shared() {
        let text: Rc<str> = Rc::from("plain text");
        let mut runs = [ShapedInlineRun {
            text: Rc::clone(&text),
            x_offset: 0.0,
            font_size: 12.0,
            font_id: None,
            font_palette: FontPalette::Normal,
            glyphs: Vec::new(),
            paints: false,
        }];

        strip_bidi_format_controls_from_shaped_runs(&mut runs);

        assert!(Rc::ptr_eq(&runs[0].text, &text));
    }
}
