use super::*;
use crate::css::{BaselineShift, FontSizeAdjust, FontSizeAdjustMetric, FontSizeAdjustValue};
#[cfg(test)]
use crate::document::paint::text::RenderedLine;
use crate::document::paint::text::RenderedTextRun;
use crate::units::{LayoutLength, SemanticLengthExt, layout_pt};

impl FontSystem {
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

    /// Returns the horizontal U+6C34 advance irrespective of writing mode.
    /// <https://www.w3.org/TR/css-values-4/#ic>
    pub(crate) fn horizontal_ic_advance_for_style(
        &mut self,
        style: &ComputedStyle,
    ) -> LayoutLength {
        layout_pt(
            self.selected_font_glyph_advance_for_style(style, '水')
                .unwrap_or(style.font_size),
        )
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
    /// `line-height: normal`, by contrast, may enclose participating fallback
    /// font line boxes. The baseline remains anchored to the style's first
    /// available font while the line box incorporates those fallback runs:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
    /// <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>.
    pub(crate) fn resolved_inline_text_metrics(
        &mut self,
        style: &ComputedStyle,
        shaped: Option<&ShapedInlineLine>,
    ) -> ResolvedInlineTextMetrics {
        let selected_font_id = self.resolve_metric_font_for_style(style);
        let content = self
            .content_extents_for_style_font(selected_font_id, style)
            .unwrap_or_else(|| FontRunVerticalExtents::from_points(style.font_size, 0.0));
        let mut line = self.line_extents_for_style_font(selected_font_id, style, content);

        if !style.line_height_is_normal() {
            return ResolvedInlineTextMetrics { content, line };
        }

        let Some(shaped) = shaped else {
            return ResolvedInlineTextMetrics { content, line };
        };

        // A preserved tab is a CSS layout advance, not a shaped font glyph.
        // It must neither select a fallback font nor expand the normal
        // line-height box.
        // <https://drafts.csswg.org/css-text-3/#tab-size-property>
        for run in shaped.runs.iter().filter(|run| {
            run.font_id.is_some() && run.text.chars().any(|character| character != '\t')
        }) {
            let Some(run_content) = self.content_extents_for_font(run.font_id, run.font_size)
            else {
                continue;
            };
            let run_line =
                self.normal_line_extents_for_font(run.font_id, run.font_size, run_content);
            line = line.union(run_line);
        }

        // Parley's shaped runs normally retain the selected document font, but
        // an `@font-face unicode-range` split can be flattened before its
        // fallback run reaches this metric pass. Resolve the source characters
        // through the CSS Fonts stack as well, so `line-height: normal`
        // encloses every eligible face rather than only the first face in the
        // family list.
        // <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>
        for character in shaped.text.chars().filter(|character| *character != '\t') {
            let Some(font_id) = self.font_for_character(style, character) else {
                continue;
            };
            let used_font_size = self
                .font_size_adjusted_size_for_font_id(style, font_id)
                .unwrap_or(style.font_size);
            let Some(run_content) = self.content_extents_for_font(Some(font_id), used_font_size)
            else {
                continue;
            };
            line = line.union(self.normal_line_extents_for_font(
                Some(font_id),
                used_font_size,
                run_content,
            ));
        }

        ResolvedInlineTextMetrics { content, line }
    }

    fn content_extents_for_style_font(
        &mut self,
        font_id: Option<usize>,
        style: &ComputedStyle,
    ) -> Option<FontRunVerticalExtents> {
        let used_font_size = font_id
            .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
            .unwrap_or(style.font_size);
        self.content_extents_for_font(font_id, used_font_size)
    }

    fn content_extents_for_font(
        &self,
        font_id: Option<usize>,
        font_size: f32,
    ) -> Option<FontRunVerticalExtents> {
        let font = font_id.and_then(|id| self.document_fonts.get(id))?;
        let units_per_em = font.units_per_em.max(1) as f32;
        if !font_size.is_finite() || font_size < 0.0 {
            return None;
        }

        // The CSS inline content area is the font's em box, even when an
        // OpenType face's ascender and descender span more or less than one
        // em. Font metrics locate the alphabetic baseline *inside* that box;
        // they instead determine `line-height: normal` separately below.
        // <https://www.w3.org/TR/CSS22/visudet.html#line-height>
        // <https://drafts.csswg.org/css-inline-3/#inline-height>
        let above_baseline = font.layout_metrics.ascender as f32 * font_size / units_per_em;
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
        if style.line_height_is_normal() {
            let used_font_size = font_id
                .and_then(|font_id| self.font_size_adjusted_size_for_font_id(style, font_id))
                .unwrap_or(style.font_size);
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

fn layout_to_program_ascent_delta(font: &DocumentFont, used_font_size: f32) -> f32 {
    let units_per_em = font.units_per_em.max(1) as f32;
    let layout_ascent = font.layout_metrics.ascender as f32 * used_font_size / units_per_em;
    let program_ascent = font.program_metrics.ascender as f32 * used_font_size / units_per_em;
    layout_ascent - program_ascent
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
