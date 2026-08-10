use super::*;
use crate::css::{FontSizeAdjust, FontSizeAdjustValue, FontVariationSettings};
use crate::text::system::api::{font_feature_family, font_size_adjust_metric_ratio};

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

    /// Return whether the selected face advertises a caps substitution feature.
    ///
    /// CSS may synthesize small caps only when the selected face cannot provide
    /// the relevant OpenType feature itself.
    /// <https://drafts.csswg.org/css-fonts-4/#font-variant-caps-prop>
    pub(crate) fn selected_font_supports_caps_feature(&mut self, style: &ComputedStyle) -> bool {
        let Some(font_id) = self.resolve_metric_font_for_style(style) else {
            return false;
        };
        let Some(font) = self.document_fonts.get(font_id) else {
            return false;
        };
        let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
            return false;
        };
        let Some(gsub) = face.tables().gsub else {
            return false;
        };
        [b"smcp", b"c2sc", b"pcap", b"c2pc"]
            .into_iter()
            .any(|wanted| {
                let wanted = ttf_parser::Tag::from_bytes(wanted);
                gsub.features
                    .into_iter()
                    .any(|feature| feature.tag == wanted)
            })
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
            FontSizeAdjust::None => self
                .document_fonts
                .font_size_adjust(font_id)
                .map_or(style.font_size, |factor| style.font_size * factor),
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
                FontFamily::Named(name) => self
                    .resolve_single_family(
                        name.as_str(),
                        style.font_weight,
                        style.font_style,
                        style.font_width,
                    )
                    .and_then(|font_id| {
                        self.document_fonts.character_font_match(font_id, character)
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

    /// Returns a metric glyph's vertical advance for vertical CSS layout.
    /// `ic` uses U+6C34 and `ch` uses upright U+0030. The face is selected
    /// for that character, including font-stack fallback and `unicode-range`.
    /// <https://www.w3.org/TR/css-values-4/#ch>
    /// <https://www.w3.org/TR/css-values-4/#ic>
    pub(super) fn vertical_glyph_advance_for_style(
        &mut self,
        style: &ComputedStyle,
        character: char,
    ) -> Option<f32> {
        let matched = self.metric_glyph_match_for_style(style, character)?;
        let used_font_size = self
            .font_size_adjusted_size_for_font_id(style, matched.font_id)
            .unwrap_or(style.font_size);
        let font = self.document_fonts.get(matched.font_id)?;
        let face = ttf_parser::Face::parse(&font.data, font.face_index).ok()?;
        let units_per_em = font.units_per_em.max(1) as f32;
        let advance = face
            .glyph_ver_advance(matched.glyph_id.raw())
            .map(|advance| advance as f32)
            .unwrap_or(units_per_em);
        Some(advance * used_font_size / units_per_em)
    }
}

pub(super) fn named_font_families(family: &FontFamily) -> Vec<String> {
    match family {
        FontFamily::Named(name) => vec![name.as_str().to_owned()],
        FontFamily::List(families) => families.iter().flat_map(named_font_families).collect(),
        _ => Vec::new(),
    }
}

pub(super) fn parley_run_fallback_character(text: &str, range: Range<usize>) -> char {
    text.get(range)
        .and_then(|text| text.chars().find(|character| !character.is_control()))
        .unwrap_or(' ')
}
