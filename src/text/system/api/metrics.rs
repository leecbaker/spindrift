use read_fonts::tables::variations::DeltaSetIndex;
use read_fonts::types::{F2Dot14, Fixed, Tag};
use read_fonts::{FontRef, TableProvider};

use super::*;
use crate::css::{BaselineMetric, BaselineShift, FontSizeAdjustMetric, TextLayoutPolicy};
#[cfg(test)]
use crate::document::paint::text::RenderedLine;
use crate::document::paint::text::RenderedTextRun;
use crate::units::{LayoutLength, layout_points, layout_pt};

impl FontSystem {
    /// Return a selected CSS baseline position measured from the inline
    /// content area's block-start edge. `BASE` coordinates are retained in
    /// design units and converted only after the selected face, variation
    /// instance, script, and typographic axis are known.
    ///
    /// Format 3 deltas are evaluated in normalized `fvar`/`avar` space. This
    /// deliberately uses the same CSS standard-axis and low-level variation
    /// precedence as shaping.
    /// <https://drafts.csswg.org/css-inline-3/#baseline-tables>
    /// <https://learn.microsoft.com/en-us/typography/opentype/spec/base>
    pub(crate) fn baseline_offset_for_style(
        &mut self,
        style: &ComputedStyle,
        metric: BaselineMetric,
    ) -> LayoutLength {
        let metric = baseline_metric_for_typography(metric, style.text_layout_policy());
        // Baseline selection uses the same first available metric face as
        // shaping and inline content-area metrics. A BASE table describes a
        // selected font; it cannot bypass `unicode-range` eligibility by
        // choosing an otherwise loadable earlier face in the family list.
        let font_id = self.resolve_metric_font_for_style(style);
        let used_font_size = font_id
            .and_then(|id| self.font_size_adjusted_size_for_font_id(style, id))
            .unwrap_or(style.font_size);
        let Some(font_id) = font_id else {
            return layout_pt(synthesized_baseline_offset(
                metric,
                style.font_size,
                style.font_size,
                self.used_x_height_for_style(style).points(),
            ));
        };
        self.baseline_offset_for_font(style, font_id, used_font_size, metric)
            .unwrap_or_else(|| layout_pt(style.font_size))
    }

    /// Return one selected font's CSS baseline position measured from the
    /// block-start of its em-box.
    ///
    /// Inline layout must use the same font selected for a shaped run: a
    /// `unicode-range` or ordinary fallback face can have different BASE,
    /// ascent, and descent metrics from the first available face in the
    /// authored family list.
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>
    /// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>
    fn baseline_offset_for_font(
        &self,
        style: &ComputedStyle,
        font_id: usize,
        used_font_size: f32,
        metric: BaselineMetric,
    ) -> Option<LayoutLength> {
        let metric = baseline_metric_for_typography(metric, style.text_layout_policy());
        let x_height = self
            .x_height_for_font(font_id, used_font_size)
            .unwrap_or_else(|| layout_pt(used_font_size * 0.5))
            .points();
        let font = self.document_fonts.get(font_id)?;
        let units_per_em = font.units_per_em.max(1) as f32;
        let coordinate_value = |coordinate: crate::document::OpenTypeBaselineCoordinate| {
            let delta = coordinate
                .variation_index
                .map(|index| self.base_variation_delta(font_id, index, style))
                .unwrap_or(0.0);
            f32::from(coordinate.design_units) + delta
        };
        let tagged_coordinate =
            |tag| baseline_coordinate_for_style(font, style, tag).map(coordinate_value);
        let (central_coordinate, ideographic_under_coordinate) =
            derive_ideographic_design_coordinates(
                tagged_coordinate(*b"idtp"),
                tagged_coordinate(*b"ideo"),
                units_per_em,
            );
        // BASE coordinates are located from the font's design-space origin,
        // whereas CSS inline content-area coordinates are located from the
        // selected layout ascent. Appendix A's em-over/em-under construction
        // is intentionally *not* a CSS layout metric: CSS Inline defines it
        // only for Canvas TextMetrics consistency. In particular, a BASE
        // central baseline must not move text-over/text-under or re-anchor
        // the alphabetic baseline inside an inline box.
        // <https://drafts.csswg.org/css-inline-3/#calculating-em-over-and-em-under>
        let content_over_coordinate = font.layout_metrics.ascender as f32;
        let base_offset = |coordinate| {
            css_content_baseline_offset(
                content_over_coordinate,
                coordinate,
                used_font_size,
                units_per_em,
            )
        };
        let tagged_offset = |tag| tagged_coordinate(tag).map(base_offset);
        let alphabetic = tagged_offset(*b"romn")
            .unwrap_or_else(|| font.layout_metrics.ascender as f32 * used_font_size / units_per_em);
        let central = central_coordinate.map(base_offset);
        let ideographic = ideographic_under_coordinate.map(base_offset);
        let baseline_coordinate = match metric {
            BaselineMetric::Central => central,
            BaselineMetric::Ideographic => ideographic,
            _ => baseline_tag_for_metric(metric).and_then(|tag| tagged_offset(*tag)),
        };
        let measured_baseline = baseline_coordinate.is_none().then(|| {
            self.measured_baseline_offset_for_style(
                font_id,
                style,
                metric,
                alphabetic,
                used_font_size,
            )
        });
        let offset = match metric {
            BaselineMetric::TextTop => 0.0,
            BaselineMetric::TextBottom => used_font_size,
            BaselineMetric::Central => baseline_coordinate.unwrap_or(used_font_size / 2.0),
            BaselineMetric::Middle => baseline_coordinate.unwrap_or(alphabetic - x_height / 2.0),
            _ => baseline_coordinate
                .or(measured_baseline.flatten())
                .unwrap_or_else(|| {
                    synthesized_baseline_offset(metric, alphabetic, used_font_size, x_height)
                }),
        };
        Some(layout_pt(offset))
    }

    /// Resolve the selected CSS content-area alphabetic baseline position.
    /// The content area remains one em tall and is anchored at the selected
    /// CSS layout ascent; BASE supplies baseline positions within it.
    fn content_baseline_offset_for_style(&mut self, style: &ComputedStyle) -> LayoutLength {
        self.baseline_offset_for_style(style, BaselineMetric::Alphabetic)
    }

    /// Measure font outlines only when `BASE` lacks a requested mathematical
    /// or hanging baseline. CSS Inline Appendix A permits the center of a
    /// minus sign for math and script-appropriate ink tops for hanging.
    ///
    /// Measurements stay in design coordinates until this final conversion,
    /// so they scale with the actual selected face and its used font size.
    /// <https://drafts.csswg.org/css-inline-3/#baseline-synthesis>
    fn measured_baseline_offset_for_style(
        &self,
        font_id: usize,
        style: &ComputedStyle,
        metric: BaselineMetric,
        alphabetic: f32,
        used_font_size: f32,
    ) -> Option<f32> {
        let font = self.document_fonts.get(font_id)?;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        let design_coordinate = match metric {
            BaselineMetric::Mathematical => ['\u{2212}', '+'].into_iter().find_map(|character| {
                let glyph = face.glyph_index(character)?;
                let bounds = face.glyph_bounding_box(glyph)?;
                Some((f32::from(bounds.y_min) + f32::from(bounds.y_max)) / 2.0)
            }),
            BaselineMetric::Hanging => hanging_measurement_characters(style.language.as_deref())
                .iter()
                .find_map(|&character| {
                    let glyph = face.glyph_index(character)?;
                    face.glyph_bounding_box(glyph)
                        .map(|bounds| f32::from(bounds.y_max))
                }),
            _ => None,
        }?;
        Some(alphabetic - design_coordinate * used_font_size / f32::from(font.units_per_em.max(1)))
    }

    fn base_variation_delta(
        &self,
        font_id: usize,
        index: crate::document::OpenTypeVariationIndex,
        style: &ComputedStyle,
    ) -> f32 {
        let Some(font) = self.document_fonts.get(font_id) else {
            return 0.0;
        };
        let Ok(font_ref) = FontRef::from_index(&font.data, font.face_index) else {
            return 0.0;
        };
        let (Ok(base), Ok(fvar)) = (font_ref.base(), font_ref.fvar()) else {
            return 0.0;
        };
        let Some(Ok(store)) = base.item_var_store() else {
            return 0.0;
        };
        let Ok(axes) = fvar.axes() else {
            return 0.0;
        };
        let axis_count = axes.len();
        let mut normalized = vec![F2Dot14::ZERO; axis_count];
        let avar = font_ref.avar().ok();
        fvar.user_to_normalized(
            avar.as_ref(),
            self.used_variation_coordinates(style, font_id),
            &mut normalized,
        );
        store
            .compute_delta(
                DeltaSetIndex {
                    outer: index.outer,
                    inner: index.inner,
                },
                &normalized,
            )
            .map_or(0.0, |delta| delta as f32)
    }

    fn used_variation_coordinates(
        &self,
        style: &ComputedStyle,
        font_id: usize,
    ) -> Vec<(Tag, Fixed)> {
        let mut coordinates = vec![
            (
                Tag::new(b"wdth"),
                Fixed::from_f64(style.font_width.0 as f64 / 10.0),
            ),
            (
                Tag::new(b"wght"),
                Fixed::from_f64(style.font_weight.0 as f64),
            ),
        ];
        let effective_style = style.font_style;
        if let Some(angle) =
            effective_style
                .oblique_angle()
                .or(matches!(effective_style, FontStyle::Italic).then_some(14.0))
        {
            coordinates.push((Tag::new(b"slnt"), Fixed::from_f64(-f64::from(angle))));
        }
        let mut apply = |settings: &FontVariationSettings| {
            for setting in &settings.0 {
                let tag = Tag::new(&setting.tag);
                let value = Fixed::from_f64(f64::from(f32::from_bits(setting.value)));
                if let Some(existing) = coordinates
                    .iter_mut()
                    .find(|(existing, _)| *existing == tag)
                {
                    existing.1 = value;
                } else {
                    coordinates.push((tag, value));
                }
            }
        };
        if let Some(settings) = self.document_fonts.selected_face_variations(font_id) {
            apply(&settings);
        }
        apply(&style.font_variation_settings);
        coordinates
    }
    pub(crate) fn ch_advance(&mut self, style: &ComputedStyle) -> LayoutLength {
        if matches!(
            style.text_layout_policy(),
            TextLayoutPolicy::Vertical(TextOrientation::Upright)
        ) {
            if let Some(advance) = self.vertical_upright_ch_advance(style) {
                return layout_pt(advance);
            }
            return layout_pt(style.font_size);
        }

        layout_pt(
            self.font_glyph_advance_for_style(style, '0')
                .unwrap_or(style.font_size * 0.5),
        )
    }

    /// Returns a representative glyph's horizontal advance through the same
    /// font-selection and shaping path used for inline text.
    ///
    /// CSS font-relative units must agree with the actual shaped text run.
    /// In particular, a generic family may be resolved by the shaping engine
    /// through a platform alias, so consulting the document-font fallback
    /// registry directly can give `1ch` a different face from the one that
    /// paints a table cell. A present zero-advance glyph remains distinct from
    /// a missing glyph because a successful shaping run still contains it.
    /// <https://www.w3.org/TR/css-values-4/#ch>
    fn font_glyph_advance_for_style(
        &mut self,
        style: &ComputedStyle,
        character: char,
    ) -> Option<f32> {
        let mut glyphs = self
            .shape_text_runs_with_parley(&character.to_string(), style)
            .into_iter()
            .flat_map(|run| run.glyphs);
        let first = glyphs.next()?;
        Some(
            std::iter::once(first)
                .chain(glyphs)
                .map(|glyph| glyph.x_advance)
                .sum(),
        )
    }

    /// Select the verified face and glyph used to measure a font-relative
    /// metric character.
    ///
    /// Metric characters participate in the element's ordinary font-stack
    /// selection, including `unicode-range` and character fallback. Looking
    /// up U+0020's line-metric face instead can select a face that cannot
    /// render the metric character at all.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(super) fn metric_glyph_match_for_style(
        &mut self,
        style: &ComputedStyle,
        character: char,
    ) -> Option<CharacterFontMatch> {
        self.character_font_match(style, character)
    }

    /// Return a glyph advance from the face selected for the metric
    /// character, retaining the selected face's effective CSS font size.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    fn selected_font_glyph_advance_for_style(
        &mut self,
        style: &ComputedStyle,
        character: char,
    ) -> Option<f32> {
        let matched = self.metric_glyph_match_for_style(style, character)?;
        let font_size = self.used_font_size_for_font(style, matched.font_id)?;
        let font = self.document_fonts.get(matched.font_id)?;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        let units_per_em = font.units_per_em.max(1) as f32;
        let advance = face.glyph_hor_advance(matched.glyph_id.raw())?;
        Some(advance as f32 * font_size / units_per_em)
    }

    /// Return the used CSS `ic` advance as a semantic layout length.
    ///
    /// CSS Values defines `ic` from the selected font's U+6C34 advance, with
    /// a one-em fallback. The font parser uses scalar units internally, but
    /// callers resolve a CSS length and should retain its unit identity:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn ic_advance_for_style(&mut self, style: &ComputedStyle) -> LayoutLength {
        if matches!(style.text_layout_policy(), TextLayoutPolicy::Vertical(_)) {
            return layout_pt(
                self.vertical_glyph_advance_for_style(style, '水')
                    .unwrap_or(style.font_size),
            );
        }
        layout_pt(
            self.selected_font_glyph_advance_for_style(style, '水')
                .unwrap_or(style.font_size),
        )
    }
}

/// Script-appropriate ink samples for Appendix A hanging-baseline
/// measurement. The fallback is intentionally empty for scripts without a
/// specified representative character, allowing the normative .6em fallback.
fn hanging_measurement_characters(language: Option<&str>) -> &'static [char] {
    match opentype_script_for_language(language) {
        Some([b'd', b'e', b'v', b'2']) => &['क'],
        Some([b'b', b'n', b'g', b'2']) => &['ক'],
        Some([b'g', b'u', b'r', b'2']) => &['ਕ'],
        Some([b'g', b'j', b'r', b'2']) => &['ક'],
        Some([b'o', b'r', b'y', b'2']) => &['କ'],
        Some([b'm', b'l', b'm', b'2']) => &['ക'],
        Some([b't', b'e', b'l', b'2']) => &['క'],
        Some([b'k', b'n', b'd', b'2']) => &['ಕ'],
        Some([b't', b'i', b'b', b't']) => &['ཀ'],
        Some([b'h', b'e', b'b', b'r']) => &['ה'],
        _ => &[],
    }
}

impl FontSystem {
    pub(crate) fn used_line_height(&mut self, style: &ComputedStyle) -> LayoutLength {
        if !style.line_height_is_normal() {
            return layout_pt(style.line_height);
        }
        let font_id = self.resolve_metric_font_for_style(style);
        self.line_height_for_font(font_id, style)
    }

    pub(crate) fn line_height_for_font(
        &self,
        font_id: Option<usize>,
        style: &ComputedStyle,
    ) -> LayoutLength {
        if !style.line_height_is_normal() {
            return layout_pt(style.line_height);
        }
        let Some(font) = font_id.and_then(|id| self.document_fonts.get(id)) else {
            return layout_pt(style.line_height);
        };
        let font_height = (font.layout_metrics.ascender as f32
            - font.layout_metrics.descender as f32
            + font.layout_metrics.line_gap as f32)
            .max(0.0)
            * style.font_size
            / font.units_per_em.max(1) as f32;
        layout_pt(font_height.max(style.font_size))
    }

    /// Resolve scaled vertical metrics for a CSS inline text box.
    ///
    /// CSS 2.2 keeps the non-replaced inline content area separate from the
    /// line-height box. Quire's content-area policy uses the primary metric
    /// face's em box, independent of `line-height` and the glyph runs selected
    /// by fallback. CSS 2.2 leaves the multi-font content-area choice
    /// unspecified, but it requires the choice not to change with
    /// `line-height`; backgrounds, borders, and padding consume this metric.
    ///
    /// This result is the parent strut. Inline layout unions it with extents
    /// for the exact font runs selected during shaping, so fallback faces can
    /// contribute their own glyph-and-leading geometry without changing the
    /// authored inline box's content-area policy:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
    /// <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>.
    pub(crate) fn resolved_inline_text_metrics(
        &mut self,
        style: &ComputedStyle,
    ) -> ResolvedInlineTextMetrics {
        let selected_font_id = self.resolve_metric_font_for_style(style);
        let content = self
            .content_extents_for_style_font(selected_font_id, style)
            .unwrap_or_else(|| FontRunVerticalExtents::from_points(style.font_size, 0.0));
        let line = self.line_extents_for_style_font(selected_font_id, style, content);

        ResolvedInlineTextMetrics { content, line }
    }

    /// Resolve vertical metrics for one font already selected by shaping.
    ///
    /// This is the font-run counterpart to [`Self::resolved_inline_text_metrics`].
    /// It deliberately receives the run's used size instead of recalculating
    /// `font-size-adjust`, because the shaped run is the source of truth for
    /// the selected `@font-face` and its size adjustment.
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height>
    /// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>
    pub(crate) fn resolved_inline_text_metrics_for_selected_font(
        &self,
        style: &ComputedStyle,
        font_id: usize,
        used_font_size: f32,
    ) -> Option<ResolvedInlineTextMetrics> {
        let content_baseline_offset = self
            .baseline_offset_for_font(style, font_id, used_font_size, BaselineMetric::Alphabetic)?
            .points();
        let content =
            self.content_extents_for_font(Some(font_id), used_font_size, content_baseline_offset)?;
        let line =
            self.line_extents_for_style_font_at_size(Some(font_id), style, used_font_size, content);

        Some(ResolvedInlineTextMetrics { content, line })
    }

    fn content_extents_for_style_font(
        &mut self,
        font_id: Option<usize>,
        style: &ComputedStyle,
    ) -> Option<FontRunVerticalExtents> {
        let used_font_size = font_id
            .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
            .unwrap_or(style.font_size);
        let content_baseline_offset = self.content_baseline_offset_for_style(style).points();
        self.content_extents_for_font(font_id, used_font_size, content_baseline_offset)
    }

    fn content_extents_for_font(
        &self,
        font_id: Option<usize>,
        font_size: f32,
        content_baseline_offset: f32,
    ) -> Option<FontRunVerticalExtents> {
        font_id.and_then(|id| self.document_fonts.get(id))?;
        if !font_size.is_finite() || font_size < 0.0 {
            return None;
        }

        // The CSS inline content area is the font's em box, even when an
        // OpenType face's ascender and descender span more or less than one
        // em. Font metrics locate the alphabetic baseline *inside* that box;
        // they instead determine `line-height: normal` separately below.
        // <https://www.w3.org/TR/CSS22/visudet.html#line-height>
        // <https://drafts.csswg.org/css-inline-3/#inline-height>
        let above_baseline = content_baseline_offset;
        let below_baseline = font_size - above_baseline;
        Some(FontRunVerticalExtents::from_points(
            above_baseline,
            below_baseline,
        ))
    }

    fn line_extents_for_style_font(
        &mut self,
        font_id: Option<usize>,
        style: &ComputedStyle,
        content: FontRunVerticalExtents,
    ) -> FontRunVerticalExtents {
        let used_font_size = font_id
            .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
            .unwrap_or(style.font_size);
        self.line_extents_for_style_font_at_size(font_id, style, used_font_size, content)
    }

    fn line_extents_for_style_font_at_size(
        &self,
        font_id: Option<usize>,
        style: &ComputedStyle,
        used_font_size: f32,
        content: FontRunVerticalExtents,
    ) -> FontRunVerticalExtents {
        if style.line_height_is_normal() {
            self.normal_line_extents_for_font(font_id, used_font_size, content)
        } else {
            self.explicit_line_extents_for_content(style.line_height, content)
        }
    }

    fn normal_line_extents_for_font(
        &self,
        font_id: Option<usize>,
        font_size: f32,
        content: FontRunVerticalExtents,
    ) -> FontRunVerticalExtents {
        let line_height = font_id
            .and_then(|id| self.document_fonts.get(id))
            .map(|font| {
                (font.layout_metrics.ascender as f32 - font.layout_metrics.descender as f32
                    + font.layout_metrics.line_gap as f32)
                    .max(0.0)
                    * font_size
                    / font.units_per_em.max(1) as f32
            })
            .unwrap_or(font_size)
            .max(font_size);
        self.explicit_line_extents_for_content(line_height, content)
    }

    fn explicit_line_extents_for_content(
        &self,
        line_height: f32,
        content: FontRunVerticalExtents,
    ) -> FontRunVerticalExtents {
        let leading = (line_height - layout_points(content.block_size())) / 2.0;
        content.with_symmetric_leading(layout_pt(leading))
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
            descender_depth: (-font.program_metrics.descender as f32 * scale).max(0.0),
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
                if let Some(glyph_id) = glyph.painted_id()
                    && let Some(bbox) = face.glyph_bounding_box(ttf_parser::GlyphId(glyph_id))
                {
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
                        let transformed = run.text_matrix.transform_local_point(
                            crate::document::paint::text::TextRunPoint::new(x, y),
                        );
                        let transformed_x = run.x_offset + transformed.x;
                        let transformed_y = baseline_y + run.y_offset + transformed.y;
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
}

impl FontSystem {
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
        self.x_height_for_font(font_id?, used_font_size)
    }

    /// Return a specific selected font's x-height at its already-resolved
    /// shaping size. This keeps per-run baseline synthesis aligned with the
    /// exact font that supplied the glyphs.
    fn x_height_for_font(&self, font_id: usize, used_font_size: f32) -> Option<LayoutLength> {
        let font = self.document_fonts.get(font_id)?;
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

/// CSS Inline defines `middle` as x-middle except for upright typography,
/// where alphabetic and x-height are not meaningful and `middle` uses the
/// central baseline instead.
/// <https://drafts.csswg.org/css-inline-3/#baseline-metrics>
fn baseline_metric_for_typography(
    metric: BaselineMetric,
    policy: TextLayoutPolicy,
) -> BaselineMetric {
    if matches!(
        (metric, policy),
        (
            BaselineMetric::Middle,
            TextLayoutPolicy::Vertical(TextOrientation::Upright)
        )
    ) {
        BaselineMetric::Central
    } else {
        metric
    }
}

fn layout_to_program_ascent_delta(font: &DocumentFont, used_font_size: f32) -> f32 {
    let units_per_em = font.units_per_em.max(1) as f32;
    let layout_ascent = font.layout_metrics.ascender as f32 * used_font_size / units_per_em;
    let program_ascent = font.program_metrics.ascender as f32 * used_font_size / units_per_em;
    layout_ascent - program_ascent
}

fn baseline_tag_for_metric(metric: BaselineMetric) -> Option<&'static [u8; 4]> {
    match metric {
        BaselineMetric::Alphabetic => Some(b"romn"),
        BaselineMetric::Ideographic => Some(b"ideo"),
        // `central` has no OpenType BASE tag: it is derived from `idtp` and
        // `ideo` in `baseline_offset_for_style`.
        BaselineMetric::Central => None,
        BaselineMetric::Mathematical => Some(b"math"),
        BaselineMetric::Hanging => Some(b"hang"),
        BaselineMetric::TextBottom | BaselineMetric::Middle | BaselineMetric::TextTop => None,
    }
}

/// Derive CSS's ideographic-under and central baselines from OpenType's
/// ideographic-over (`idtp`) and ideographic-under (`ideo`) coordinates.
///
/// These are OpenType Y coordinates, so ideographic-over is one em *above*
/// ideographic-under. CSS Inline Appendix A defines central as their midpoint
/// and permits the missing partner to be inferred one em away when a font
/// exposes only one.
fn derive_ideographic_design_coordinates(
    ideographic_over: Option<f32>,
    ideographic_under: Option<f32>,
    em: f32,
) -> (Option<f32>, Option<f32>) {
    let central = match (ideographic_over, ideographic_under) {
        (Some(over), Some(under)) => Some((over + under) / 2.0),
        (Some(over), None) => Some(over - em / 2.0),
        (None, Some(under)) => Some(under + em / 2.0),
        (None, None) => None,
    };
    let ideographic_under = ideographic_under.or_else(|| ideographic_over.map(|over| over - em));
    (central, ideographic_under)
}

/// Convert one BASE design-space coordinate into the CSS inline content area.
/// Unlike Canvas's em-over/em-under metrics, CSS line layout remains anchored
/// to the selected layout ascent.
/// <https://drafts.csswg.org/css-inline-3/#calculating-em-over-and-em-under>
fn css_content_baseline_offset(
    content_over_coordinate: f32,
    baseline_coordinate: f32,
    used_font_size: f32,
    units_per_em: f32,
) -> f32 {
    (content_over_coordinate - baseline_coordinate) * used_font_size / units_per_em
}

/// Select the OpenType `BASE` axis and script matching CSS typography. A
/// language's script selects an exact BaseScript record; only when that record
/// is absent do we use the font's `DFLT` script record.
fn baseline_coordinate_for_style(
    font: &DocumentFont,
    style: &ComputedStyle,
    tag: [u8; 4],
) -> Option<crate::document::OpenTypeBaselineCoordinate> {
    let axis = baseline_axis_for_policy(&font.baselines, style.text_layout_policy());
    let preferred = style
        .font_language_override
        .opentype_tag()
        .and_then(opentype_script_for_language_override)
        .or_else(|| opentype_script_for_language(style.language.as_deref()));
    let script = select_baseline_script(axis, preferred)?;
    script
        .coordinates
        .iter()
        .find_map(|(candidate, coordinate)| (*candidate == tag).then_some(*coordinate))
}

/// Pick the BASE table axis for CSS's typographic mode. Sideways text uses
/// the horizontal table; mixed and upright vertical typography use the
/// vertical table.
fn baseline_axis_for_policy(
    baselines: &crate::document::OpenTypeBaselineTable,
    policy: TextLayoutPolicy,
) -> &crate::document::OpenTypeBaselineAxis {
    match policy {
        TextLayoutPolicy::Vertical(TextOrientation::Mixed | TextOrientation::Upright) => {
            &baselines.vertical
        }
        TextLayoutPolicy::Horizontal
        | TextLayoutPolicy::Vertical(TextOrientation::Sideways)
        | TextLayoutPolicy::Sideways(_) => &baselines.horizontal,
    }
}

fn select_baseline_script(
    axis: &crate::document::OpenTypeBaselineAxis,
    preferred: Option<[u8; 4]>,
) -> Option<&crate::document::OpenTypeBaselineScript> {
    preferred
        .and_then(|wanted| axis.scripts.iter().find(|script| script.script == wanted))
        .or_else(|| axis.scripts.iter().find(|script| script.script == *b"DFLT"))
}

/// BCP 47 language-to-OpenType script selection for the scripts whose BASE
/// tables commonly differ. Script subtags take precedence; a primary-language
/// fallback is used only when no script subtag is declared.
fn opentype_script_for_language(language: Option<&str>) -> Option<[u8; 4]> {
    let language = language?.replace('_', "-");
    let mut parts = language.split('-');
    let primary = parts.next()?.to_ascii_lowercase();
    if let Some(script) = parts.find(|part| part.len() == 4) {
        return Some(match script.to_ascii_lowercase().as_str() {
            "latn" => *b"latn",
            "cyrl" => *b"cyrl",
            "grek" => *b"grek",
            "arab" => *b"arab",
            "hebr" => *b"hebr",
            "deva" => *b"dev2",
            "beng" => *b"bng2",
            "guru" => *b"gur2",
            "gujr" => *b"gjr2",
            "orya" => *b"ory2",
            "mlym" => *b"mlm2",
            "telu" => *b"tel2",
            "knda" => *b"knd2",
            "tibt" => *b"tibt",
            "hani" | "hans" | "hant" => *b"hani",
            "hang" => *b"hang",
            "kana" => *b"kana",
            "thai" => *b"thai",
            _ => return None,
        });
    }
    Some(match primary.as_str() {
        "ja" => *b"kana",
        "ko" => *b"hang",
        "zh" => *b"hani",
        "ar" | "fa" | "ur" => *b"arab",
        "he" | "yi" => *b"hebr",
        "hi" | "mr" | "ne" => *b"dev2",
        "bn" => *b"bng2",
        "pa" => *b"gur2",
        "gu" => *b"gjr2",
        "or" => *b"ory2",
        "ml" => *b"mlm2",
        "te" => *b"tel2",
        "kn" => *b"knd2",
        "bo" | "dz" => *b"tibt",
        "th" => *b"thai",
        "ru" | "uk" | "bg" | "sr" => *b"cyrl",
        "el" => *b"grek",
        _ => *b"latn",
    })
}

/// OpenType language-system overrides replace shaping's language system. BASE
/// values are script-specific rather than language-system-specific, but the
/// override still provides the best script hint when the element has no BCP 47
/// language. Keep this conversion at the baseline-table selection boundary so
/// shaping and baseline choice consume the same CSS inputs.
/// <https://drafts.csswg.org/css-fonts-4/#font-language-override-prop>
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/base#basescript-table>
fn opentype_script_for_language_override(tag: [u8; 4]) -> Option<[u8; 4]> {
    Some(match &tag {
        b"JAN " | b"JPN " => *b"kana",
        b"KOR " => *b"hang",
        b"ZHS " | b"ZHT " | b"CHN " => *b"hani",
        b"ARA " | b"FAR " | b"URD " => *b"arab",
        b"IWR " | b"YID " => *b"hebr",
        b"HIN " | b"MAR " | b"NEP " => *b"dev2",
        b"RUS " | b"SRB " | b"UKR " => *b"cyrl",
        b"ELL " => *b"grek",
        b"THA " => *b"thai",
        // Common OpenType Latin language-system tags. Keep this list
        // deliberately explicit: an unknown raw tag must still fall through
        // to the document language instead of being guessed as Latin.
        b"DEU " | b"ENG " | b"ENU " | b"ESP " | b"FRA " | b"ITA " | b"NLD " | b"NOR " | b"PTB "
        | b"PTG " | b"SVE " | b"TRK " => *b"latn",
        // An unknown OpenType language-system tag does not establish a
        // script. Let the element language select BASE in that case instead
        // of incorrectly treating, for example, an unrecognized Arabic or
        // Indic language system as Latin.
        _ => return None,
    })
}

/// CSS Inline Appendix A synthesis when the selected baseline table does not
/// define a requested metric. Values are positions from the content area's
/// block-start edge, not arbitrary box midpoints.
/// <https://drafts.csswg.org/css-inline-3/#baseline-synthesis>
fn synthesized_baseline_offset(
    metric: BaselineMetric,
    alphabetic: f32,
    em: f32,
    x_height: f32,
) -> f32 {
    match metric {
        BaselineMetric::TextTop => 0.0,
        BaselineMetric::TextBottom => em,
        BaselineMetric::Alphabetic => alphabetic,
        BaselineMetric::Middle => alphabetic - x_height / 2.0,
        BaselineMetric::Central => em / 2.0,
        // In the absence of a BASE value, Appendix A places the hanging and
        // mathematical baselines from the text-edge/font-metric fallbacks.
        BaselineMetric::Hanging => alphabetic - em * 0.6,
        BaselineMetric::Ideographic => em,
        BaselineMetric::Mathematical => alphabetic - x_height / 2.0,
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod baseline_tests {
    use super::*;
    use crate::css::{ContentLanguage, FontLanguageOverride, SidewaysOrientation};

    #[test]
    fn baseline_synthesis_uses_font_metric_relationships() {
        assert_eq!(
            synthesized_baseline_offset(BaselineMetric::Alphabetic, 8.0, 10.0, 5.0),
            8.0
        );
        assert_eq!(
            synthesized_baseline_offset(BaselineMetric::Middle, 8.0, 10.0, 5.0),
            5.5
        );
        assert_eq!(
            synthesized_baseline_offset(BaselineMetric::Central, 8.0, 10.0, 5.0),
            5.0
        );
        assert_eq!(
            synthesized_baseline_offset(BaselineMetric::TextTop, 8.0, 10.0, 5.0),
            0.0
        );
        assert_eq!(
            synthesized_baseline_offset(BaselineMetric::TextBottom, 8.0, 10.0, 5.0),
            10.0
        );
    }

    #[test]
    fn declared_script_subtag_precedes_primary_language_fallback() {
        assert_eq!(
            opentype_script_for_language(Some("zh-Hant-TW")),
            Some(*b"hani")
        );
        assert_eq!(
            opentype_script_for_language(Some("sr-Cyrl")),
            Some(*b"cyrl")
        );
        assert_eq!(opentype_script_for_language(Some("ja")), Some(*b"kana"));
        assert_eq!(opentype_script_for_language(Some("en")), Some(*b"latn"));
        assert_eq!(
            opentype_script_for_language(Some("bn-Beng")),
            Some(*b"bng2")
        );
    }

    #[test]
    fn language_override_supplies_a_script_hint_without_a_document_language() {
        assert_eq!(
            opentype_script_for_language_override(*b"JAN "),
            Some(*b"kana")
        );
        assert_eq!(
            opentype_script_for_language_override(*b"ZHS "),
            Some(*b"hani")
        );
        assert_eq!(
            opentype_script_for_language_override(*b"DEU "),
            Some(*b"latn")
        );
        assert_eq!(opentype_script_for_language_override(*b"XXX "), None);
    }

    #[test]
    fn language_override_replaces_the_content_language_script_hint() {
        let mut style = ComputedStyle::initial();
        style.language = ContentLanguage::from_html_attribute("ja");
        style.font_language_override = FontLanguageOverride::OpenType(*b"RUS ");
        let axis = crate::document::OpenTypeBaselineAxis {
            scripts: vec![
                crate::document::OpenTypeBaselineScript {
                    script: *b"kana",
                    default_baseline: None,
                    coordinates: Vec::new(),
                },
                crate::document::OpenTypeBaselineScript {
                    script: *b"cyrl",
                    default_baseline: None,
                    coordinates: Vec::new(),
                },
            ],
        };
        let preferred = style
            .font_language_override
            .opentype_tag()
            .and_then(opentype_script_for_language_override)
            .or_else(|| opentype_script_for_language(style.language.as_deref()));
        assert_eq!(
            select_baseline_script(&axis, preferred).unwrap().script,
            *b"cyrl"
        );

        style.font_language_override = FontLanguageOverride::OpenType(*b"XXX ");
        let preferred = style
            .font_language_override
            .opentype_tag()
            .and_then(opentype_script_for_language_override)
            .or_else(|| opentype_script_for_language(style.language.as_deref()));
        assert_eq!(
            select_baseline_script(&axis, preferred).unwrap().script,
            *b"kana"
        );
    }

    #[test]
    fn css_metrics_map_to_their_opentype_base_tags() {
        assert_eq!(
            baseline_tag_for_metric(BaselineMetric::Alphabetic),
            Some(b"romn")
        );
        assert_eq!(
            baseline_tag_for_metric(BaselineMetric::Ideographic),
            Some(b"ideo")
        );
        assert_eq!(baseline_tag_for_metric(BaselineMetric::Central), None);
        assert_eq!(
            baseline_tag_for_metric(BaselineMetric::Mathematical),
            Some(b"math")
        );
        assert_eq!(
            baseline_tag_for_metric(BaselineMetric::Hanging),
            Some(b"hang")
        );
    }

    #[test]
    fn central_and_ideographic_fallbacks_follow_appendix_a_relationships() {
        // In OpenType's upward design coordinates, central is half an em
        // below ideographic-over and ideographic-under is a full em below.
        assert_eq!(
            derive_ideographic_design_coordinates(Some(12.0), None, 10.0),
            (Some(7.0), Some(2.0))
        );
        assert_eq!(
            derive_ideographic_design_coordinates(Some(12.0), Some(2.0), 10.0),
            (Some(7.0), Some(2.0))
        );
        // The suggested hanging fallback is .6em above alphabetic.
        assert_eq!(
            synthesized_baseline_offset(BaselineMetric::Hanging, 8.0, 10.0, 5.0),
            2.0
        );
    }

    #[test]
    fn css_baseline_coordinates_remain_anchored_at_the_layout_ascent() {
        // BaselineDiagnostic's ascender is 800 and alphabetic BASE coordinate
        // is 50. Its central baseline is 350, but Appendix A's em-over value
        // (850) is a Canvas metric and must not move this CSS baseline.
        assert_eq!(
            css_content_baseline_offset(800.0, 50.0, 240.0, 1000.0),
            180.0
        );
        assert_eq!(
            css_content_baseline_offset(800.0, 650.0, 240.0, 1000.0),
            36.0
        );
    }

    #[test]
    fn baseline_axis_follows_the_css_typographic_mode() {
        let table = crate::document::OpenTypeBaselineTable {
            horizontal: crate::document::OpenTypeBaselineAxis {
                scripts: vec![crate::document::OpenTypeBaselineScript {
                    script: *b"hori",
                    default_baseline: None,
                    coordinates: Vec::new(),
                }],
            },
            vertical: crate::document::OpenTypeBaselineAxis {
                scripts: vec![crate::document::OpenTypeBaselineScript {
                    script: *b"vert",
                    default_baseline: None,
                    coordinates: Vec::new(),
                }],
            },
        };
        assert_eq!(
            baseline_axis_for_policy(&table, TextLayoutPolicy::Horizontal).scripts[0].script,
            *b"hori"
        );
        assert_eq!(
            baseline_axis_for_policy(
                &table,
                TextLayoutPolicy::Sideways(SidewaysOrientation::Right)
            )
            .scripts[0]
                .script,
            *b"hori"
        );
        assert_eq!(
            baseline_axis_for_policy(
                &table,
                TextLayoutPolicy::Vertical(TextOrientation::Sideways)
            )
            .scripts[0]
                .script,
            *b"hori"
        );
        assert_eq!(
            baseline_axis_for_policy(&table, TextLayoutPolicy::Vertical(TextOrientation::Mixed))
                .scripts[0]
                .script,
            *b"vert"
        );
    }

    #[test]
    fn middle_uses_central_only_for_upright_typography() {
        assert_eq!(
            baseline_metric_for_typography(BaselineMetric::Middle, TextLayoutPolicy::Horizontal),
            BaselineMetric::Middle
        );
        assert_eq!(
            baseline_metric_for_typography(
                BaselineMetric::Middle,
                TextLayoutPolicy::Vertical(TextOrientation::Mixed)
            ),
            BaselineMetric::Middle
        );
        assert_eq!(
            baseline_metric_for_typography(
                BaselineMetric::Middle,
                TextLayoutPolicy::Vertical(TextOrientation::Upright)
            ),
            BaselineMetric::Central
        );
    }

    #[test]
    fn script_selection_uses_only_the_matching_or_default_record() {
        let dflt = crate::document::OpenTypeBaselineScript {
            script: *b"DFLT",
            default_baseline: None,
            coordinates: Vec::new(),
        };
        let latn = crate::document::OpenTypeBaselineScript {
            script: *b"latn",
            default_baseline: None,
            coordinates: Vec::new(),
        };
        let axis = crate::document::OpenTypeBaselineAxis {
            scripts: vec![dflt.clone(), latn.clone()],
        };
        assert_eq!(select_baseline_script(&axis, Some(*b"latn")), Some(&latn));
        assert_eq!(select_baseline_script(&axis, Some(*b"hani")), Some(&dflt));

        let no_default = crate::document::OpenTypeBaselineAxis {
            scripts: vec![latn],
        };
        assert_eq!(select_baseline_script(&no_default, Some(*b"hani")), None);
    }
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
        FontFamily::Named(name) => Some(name.as_str().to_owned()),
    }
}

pub(in crate::text) fn style_for_text_range<'a, T>(
    ranges: &[(Range<usize>, &'a T)],
    run_range: Range<usize>,
) -> Option<&'a T> {
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
