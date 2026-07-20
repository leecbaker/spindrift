use super::*;
use crate::css::FontVariationSettings;
use crate::units::{LayoutLength, SemanticLengthExt, layout_pt};
use std::borrow::Cow;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub(in crate::text) struct FontSizeAdjustmentRange {
    pub(in crate::text) range: Range<usize>,
    pub(in crate::text) font_size: f32,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::text) struct RenderedRunTabContext<'a> {
    /// The computed style of the preserved tab itself. Its `tab-size` value
    /// selects the tab period.
    pub(in crate::text) style: &'a ComputedStyle,
    /// The nearest block container's computed style. Numeric `tab-size`
    /// values use this style's U+0020 advance, including text spacing.
    pub(in crate::text) metric_style: &'a ComputedStyle,
}

impl FontSystem {
    /// Run one Parley shaping pass with layout storage retained for the next
    /// pass in this document.
    ///
    /// Parley lines borrow their layout, while Quire converts them into owned
    /// glyph runs before this closure returns. Taking the scratch out of
    /// [`FontSystem`] keeps those borrows separate from the mutable font
    /// system needed during conversion and guarantees restoration on every
    /// normal closure return, including its early returns.
    pub(in crate::text) fn with_reusable_parley_layout<T>(
        &mut self,
        shape: impl FnOnce(&mut Self, &mut ParleyLayout<FontPalette>) -> T,
    ) -> T {
        let mut layout = std::mem::take(&mut self.parley_layout_scratch);
        let result = shape(self, &mut layout);
        self.parley_layout_scratch = layout;
        result
    }

    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::from_seed(Self::sync_seed())
    }

    pub(crate) fn into_fonts(self) -> Vec<DocumentFont> {
        self.document_fonts.into_fonts()
    }

    pub(in crate::text) fn font_feature_context_for_style(
        &mut self,
        style: &ComputedStyle,
    ) -> Option<FontFeatureContext> {
        // `font-feature-settings` defaults in an @font-face rule belong to
        // the selected face, rather than its family. Resolve the authored
        // face directly: a descriptor applies while shaping the face's own
        // supported characters, even when the face cannot render U+0020 and
        // is therefore unsuitable for a metric-only probe.
        // <https://www.w3.org/TR/css-fonts-4/#font-face-font-feature-settings>
        let selected_face = self
            .resolve_font_family(
                &style.font_family,
                style.font_weight,
                style.font_style,
                style.font_width,
            )
            .and_then(|font_id| self.document_fonts.selected_face_features(font_id));
        let (family, face_defaults) = selected_face
            .map(|(family, defaults)| (Some(family), Some(defaults)))
            .unwrap_or_else(|| (font_feature_family(&style.font_family), None));
        if face_defaults.is_none() && self.font_feature_values.values.is_empty() {
            return None;
        }
        Some(FontFeatureContext {
            family,
            face_defaults,
            font_feature_values: self.font_feature_values.clone(),
        })
    }

    /// Merge selected-face descriptor coordinates into the style's explicit
    /// variation map. The element property remains later in the map and thus
    /// wins duplicate tags, while descriptor coordinates override registered
    /// CSS-axis defaults during shaping.
    pub(in crate::text) fn style_with_selected_face_variations(
        &mut self,
        style: &ComputedStyle,
    ) -> ComputedStyle {
        // Descriptor defaults belong to the authored face selected from the
        // CSS family, not to the font used for U+0020 line metrics. A
        // selected face may not contain U+0020 while still rendering its
        // supported glyphs with these variation coordinates.
        // <https://www.w3.org/TR/css-fonts-4/#font-feature-variation-resolution>
        let Some(font_id) = self.resolve_font_family(
            &style.font_family,
            style.font_weight,
            style.font_style,
            style.font_width,
        ) else {
            return style.clone();
        };
        let Some(descriptor) = self.document_fonts.selected_face_variations(font_id) else {
            return style.clone();
        };
        if descriptor.0.is_empty() {
            return style.clone();
        }
        let mut resolved = style.clone();
        let mut variations = descriptor.0;
        for setting in &style.font_variation_settings.0 {
            if let Some(existing) = variations
                .iter_mut()
                .find(|existing| existing.tag == setting.tag)
            {
                *existing = *setting;
            } else {
                variations.push(*setting);
            }
        }
        variations.sort_by_key(|setting| setting.tag);
        resolved.font_variation_settings = FontVariationSettings(variations);
        resolved
    }

    /// Prepare the style passed to the shaping backend after CSS face
    /// selection. A fixed selected face cannot receive faux bold or italic
    /// when the corresponding `font-synthesis` gate is disabled. Keeping the
    /// gate at this boundary preserves the backend's complete glyph stream
    /// (including GPOS kerning), instead of replacing it with a simplified
    /// post-shaping fallback.
    /// <https://www.w3.org/TR/css-fonts-4/#font-synthesis-intro>
    pub(in crate::text) fn shaping_style_for_selected_face(
        &mut self,
        style: &ComputedStyle,
    ) -> ComputedStyle {
        let mut shaping_style = self.style_with_selected_face_variations(style);
        let Some(font_id) = self.resolve_metric_font_for_style(style) else {
            return shaping_style;
        };
        let selected_attributes = self.document_fonts.selected_face_fixed_attributes(font_id);
        let is_registered_css_family = named_font_families(&style.font_family)
            .iter()
            .any(|family| self.document_fonts.has_registered_css_family(family));
        let intrinsic_attributes = is_registered_css_family
            .then(|| {
                self.document_fonts.get(font_id).and_then(|font| {
                    let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
                    Some((
                        FontWeight::from_number(face.weight().to_number() as f32)?,
                        if face.is_italic() {
                            FontStyle::Italic
                        } else {
                            FontStyle::Normal
                        },
                    ))
                })
            })
            .flatten();
        let (weight, face_style) = match (selected_attributes, intrinsic_attributes) {
            (Some((weight, style)), _) => (weight, style),
            (None, Some((weight, style))) => (Some(weight), style),
            (None, None) => return shaping_style,
        };
        if !style.font_synthesis.weight
            && let Some(weight) = weight
            && weight != style.font_weight
        {
            shaping_style.font_weight = weight;
        }
        if !style.font_synthesis.style && face_style != style.font_style {
            shaping_style.font_style = face_style;
        }
        shaping_style
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
        self.resolve_system_fallback_for_character(
            'M',
            style.font_weight,
            style.font_style,
            style.font_width,
        )
    }

    /// Resolve the face's used shaping size. An element-level
    /// `font-size-adjust` takes precedence over the `@font-face size-adjust`
    /// descriptor; otherwise the descriptor scales this face alone.
    /// <https://www.w3.org/TR/css-fonts-5/#font-size-adjust-prop>
    /// <https://www.w3.org/TR/css-fonts-5/#descdef-font-face-size-adjust>
    pub(in crate::text) fn used_font_size_for_font(
        &mut self,
        style: &ComputedStyle,
        font_id: usize,
    ) -> Option<f32> {
        self.font_size_adjusted_size_for_font_id(style, font_id)
    }

    pub(in crate::text) fn font_size_adjusted_size_for_font_id(
        &mut self,
        style: &ComputedStyle,
        font_id: usize,
    ) -> Option<f32> {
        let adjusted = match style.font_size_adjust {
            FontSizeAdjust::None => {
                let factor = self.document_fonts.font_size_adjust(font_id)?;
                style.font_size * factor
            }
            FontSizeAdjust::Value { metric, value } => {
                let target_ratio = match value {
                    FontSizeAdjustValue::Number(value) => value,
                    FontSizeAdjustValue::FromFont => {
                        // `from-font` resolves the requested aspect value
                        // from the element's first available face, not from
                        // each fallback run. The selected run's own metric
                        // is the denominator below, so fallback glyphs keep
                        // the primary face's intended apparent size.
                        // <https://www.w3.org/TR/css-fonts-5/#font-size-adjust-prop>
                        let primary_font_id = self.resolve_metric_font_for_style(style)?;
                        let font = self.document_fonts.get(primary_font_id)?;
                        font_size_adjust_metric_ratio(font, metric)?
                    }
                };
                let font = self.document_fonts.get(font_id)?;
                let selected_ratio = font_size_adjust_metric_ratio(font, metric)?;
                if selected_ratio <= 0.0 {
                    return None;
                }
                style.font_size * target_ratio / selected_ratio
            }
        };
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

    /// Returns the used CSS `ch` advance for a style's selected font.
    ///
    /// CSS Values defines `1ch` as the used advance of the "0" glyph in the
    /// element's font. In vertical writing with upright text orientation, that
    /// advance is the vertical inline-axis advance, falling back to 1em when
    /// the selected face has no vertical metric for "0". Otherwise it falls
    /// back to 0.5em when measuring that glyph is not possible:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
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

    /// Returns the CSS `ic` basis: the selected font's U+6C34 WATER advance,
    /// with the specification's one-em fallback when no such glyph is usable.
    /// <https://www.w3.org/TR/css-values-4/#ic>
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
            self.font_glyph_advance_for_style(style, '水')
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
            self.font_glyph_advance_for_style(style, '水')
                .unwrap_or(style.font_size),
        )
    }

    /// Finds the first CSS font-stack face that is eligible to render one
    /// character, including `unicode-range` restrictions.
    pub(in crate::text) fn character_font_match(
        &mut self,
        style: &ComputedStyle,
        character: char,
    ) -> Option<CharacterFontMatch> {
        let families = match &style.font_family {
            FontFamily::List(families) => families.as_slice(),
            family => std::slice::from_ref(family),
        };
        for family in families {
            let matched = match family {
                FontFamily::Names(names) => names.iter().find_map(|name| {
                    self.resolve_single_family(
                        name,
                        style.font_weight,
                        style.font_style,
                        style.font_width,
                    )
                    .and_then(|font_id| {
                        self.document_fonts.character_font_match(font_id, character)
                    })
                }),
                _ => self
                    .resolve_font_family(
                        family,
                        style.font_weight,
                        style.font_style,
                        style.font_width,
                    )
                    .and_then(|font_id| {
                        self.document_fonts.character_font_match(font_id, character)
                    }),
            };
            if matched.is_some() {
                return matched;
            }
        }
        self.resolve_system_fallback_for_character(
            character,
            style.font_weight,
            style.font_style,
            style.font_width,
        )
        .and_then(|font_id| self.document_fonts.character_font_match(font_id, character))
    }

    /// Finds the first CSS font-stack face that is eligible to render one
    /// character, including `unicode-range` restrictions.
    pub(in crate::text) fn font_for_character(
        &mut self,
        style: &ComputedStyle,
        character: char,
    ) -> Option<usize> {
        self.character_font_match(style, character)
            .map(|matched| matched.font_id)
    }

    pub(in crate::text) fn vertical_upright_ch_advance(
        &mut self,
        style: &ComputedStyle,
    ) -> Option<f32> {
        self.vertical_glyph_advance_for_style(style, '0')
    }

    /// Returns a selected glyph's vertical advance for vertical CSS layout.
    /// `ic` uses this for U+6C34 and `ch` uses it for upright U+0030.
    /// <https://www.w3.org/TR/css-values-4/#ch>
    /// <https://www.w3.org/TR/css-values-4/#ic>
    fn vertical_glyph_advance_for_style(
        &mut self,
        style: &ComputedStyle,
        character: char,
    ) -> Option<f32> {
        let font_id = self.font_for_character(style, character)?;
        let used_font_size = self
            .font_size_adjusted_size_for_font_id(style, font_id)
            .unwrap_or(style.font_size);
        let font = self.document_fonts.get(font_id)?;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        let units_per_em = font.units_per_em.max(1) as f32;
        let advance = face
            .glyph_index(character)
            .and_then(|glyph| face.glyph_ver_advance(glyph))
            .map(|advance| advance as f32)
            .filter(|advance| *advance > 0.0)
            .unwrap_or(units_per_em);
        Some(advance * used_font_size / units_per_em)
    }

    /// Return the used line height as a semantic layout length.
    ///
    /// CSS Inline defines the used line-height as a length, including the
    /// font-metric-derived `normal` value. Keep that identity until text
    /// shaping or coordinate placement needs raw layout points:
    /// <https://www.w3.org/TR/css-inline-3/#line-height-property>.
    pub(crate) fn used_line_height(&mut self, style: &ComputedStyle) -> LayoutLength {
        if !style.line_height_is_normal {
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
        if !style.line_height_is_normal {
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
    /// CSS 2.2 defines `line-height: normal` from font metrics, while CSS Fonts
    /// lets `unicode-range` and fallback matching choose different fonts for
    /// individual glyph runs. The baseline remains anchored to the style's
    /// first available font, but the normal inline box must enclose the union
    /// of participating fallback-font line boxes:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
    /// <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>.
    pub(crate) fn resolved_inline_text_metrics(
        &mut self,
        style: &ComputedStyle,
        shaped: Option<&ShapedInlineLine>,
    ) -> ResolvedInlineTextMetrics {
        let selected_font_id = self.resolve_metric_font_for_style(style);
        let mut content = self
            .content_extents_for_style_font(selected_font_id, style)
            .unwrap_or_else(|| FontRunVerticalExtents::from_points(style.font_size, 0.0));
        let mut line = self.line_extents_for_style_font(selected_font_id, style, content);

        if !style.line_height_is_normal {
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
            content = content.union(run_content);
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
            content = content.union(run_content);
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
        if style.line_height_is_normal {
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
                        let transformed = run
                            .text_matrix
                            .transform_local_point(crate::document::TextRunPoint::new(x, y));
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

    pub(crate) fn shape_text_runs_with_parley(
        &mut self,
        text: &str,
        style: &ComputedStyle,
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
        ranges: &[(Range<usize>, &ComputedStyle)],
        default_style: &ComputedStyle,
    ) -> Vec<FontSizeAdjustmentRange> {
        let mut adjustments = Vec::new();
        for run in line.runs() {
            let run_range = run.text_range();
            let run_style =
                style_for_text_range(ranges, run_range.clone()).unwrap_or(default_style);
            let fallback_character = parley_run_fallback_character(text, run_range.clone());
            let Some(font_id) = self.document_font_from_parley_font_data_for_style(
                run.font(),
                run_style,
                fallback_character,
            ) else {
                continue;
            };
            let Some(font_size) = self.used_font_size_for_font(run_style, font_id) else {
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
                text_matrix: crate::RenderedTextMatrix::IDENTITY,
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
                    .run_has_color_glyph(font_id, run_text.as_ref())
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
                    text_matrix: crate::RenderedTextMatrix::IDENTITY,
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
                text_matrix: crate::RenderedTextMatrix::IDENTITY,
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
                FontFamily::Names(names) => {
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
                            return Some(FontFamily::Names(vec![name.clone()]));
                        }
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
        let mut untracked_style = style.clone();
        untracked_style.letter_spacing = crate::css::ComputedLengthPercentage::ZERO;
        self.shape_unwrapped_line(text, &untracked_style, line_height)
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

fn named_font_families(family: &FontFamily) -> Vec<String> {
    match family {
        FontFamily::Names(names) => names.clone(),
        FontFamily::List(families) => families.iter().flat_map(named_font_families).collect(),
        _ => Vec::new(),
    }
}

pub(super) fn parley_run_fallback_character(text: &str, range: Range<usize>) -> char {
    text.get(range)
        .and_then(|text| text.chars().find(|character| !character.is_control()))
        .unwrap_or(' ')
}

/// Remove UAX #9 formatting controls from paint and extraction summaries.
///
/// The controls remain in the Parley input while visual order is resolved, but
/// CSS never paints them and PDF text extraction must not expose the synthetic
/// guard used by already-visual text groups:
/// <https://www.unicode.org/reports/tr9/#Directional_Formatting_Characters>.
fn strip_bidi_format_controls_from_shaped_runs(runs: &mut [ShapedInlineRun]) {
    for run in runs {
        run.text = text_without_bidi_format_controls(&run.text)
            .into_owned()
            .into();
    }
}
