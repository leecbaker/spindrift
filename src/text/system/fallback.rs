use super::*;

impl FontSystem {
    pub(super) fn resolve_family_fallback_for_character_in_family(
        &mut self,
        style: &ComputedStyle,
        family: &FontFamily,
        character: char,
    ) -> Option<usize> {
        if let FontFamily::List(families) = family {
            for family in families {
                if let Some(font_id) = self.resolve_font_family(
                    family,
                    style.font_weight,
                    style.font_style,
                    style.font_width,
                ) && self.document_fonts.font_has_character(font_id, character)
                {
                    return Some(font_id);
                }
            }
        }
        if let FontFamily::Named(name) = family
            && let Some(font_id) = self.resolve_single_family(
                name.as_str(),
                style.font_weight,
                style.font_style,
                style.font_width,
            )
            && self.document_fonts.font_has_character(font_id, character)
        {
            return Some(font_id);
        }

        if let Some(font_id) = self.resolve_generic_family(
            family,
            style.font_weight,
            style.font_style,
            style.font_width,
        ) && self.document_fonts.font_has_character(font_id, character)
        {
            return Some(font_id);
        }

        self.resolve_system_fallback_for_style_character(style, character)
    }

    pub(super) fn resolve_generic_family(
        &mut self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Option<usize> {
        let families = generic_query_families(family, weight)?;
        let cache_key = FontRequest::generic(family, weight, style, width);
        if let Some(id) = self.family_cache.get(&cache_key) {
            return Some(*id);
        }
        if let Some(id) = self.load_outline_embeddable_document_font_for_families(
            families, weight, style, width, &cache_key,
        ) {
            let font = self.document_fonts.get(id)?;
            log::debug!(
                "resolved CSS generic font-family {:?} to system font {} ({})",
                family,
                font.family,
                font.post_script_name
            );
            self.family_cache.insert(cache_key, id);
            return Some(id);
        }
        if *family != FontFamily::SansSerif {
            return self.resolve_generic_family(&FontFamily::SansSerif, weight, style, width);
        }
        None
    }

    /// Resolve the family source supplied to Parley for a CSS style.
    ///
    /// Every resolved family is replaced with Quire's concrete selection.
    /// Passing a raw named stack or generic keyword to Parley would invoke a
    /// second platform matcher, which can select a different font program
    /// from the one used for CSS matching, metrics, and PDF planning.
    /// <https://www.w3.org/TR/css-fonts-4/#font-family-prop>
    pub(crate) fn resolved_parley_font_family_source(&mut self, style: &ComputedStyle) -> String {
        self.resolved_parley_font_family_source_for_family(
            &style.font_family,
            style.font_weight,
            style.font_style,
            style.font_width,
        )
    }

    pub(in crate::text) fn resolved_parley_font_family_source_for_family(
        &mut self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> String {
        self.resolved_parley_font_family_source_for_family_optional(family, weight, style, width)
            .unwrap_or_else(|| {
                self.resolved_parley_font_family_source_for_family_optional(
                    &FontFamily::SansSerif,
                    weight,
                    style,
                    width,
                )
                .unwrap_or_else(|| parley_font_family_source(&FontFamily::SansSerif))
            })
    }

    fn resolved_parley_font_family_source_for_family_optional(
        &mut self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Option<String> {
        match family {
            FontFamily::List(families) => {
                let source = families
                    .iter()
                    .filter_map(|family| {
                        self.resolved_parley_font_family_source_for_family_optional(
                            family, weight, style, width,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                (!source.is_empty()).then_some(source)
            }
            FontFamily::Named(name) => self
                .resolve_single_family(name.as_str(), weight, style, width)
                .and_then(|font_id| self.document_fonts.get(font_id))
                .map(|font| parley_font_family_source(&FontFamily::named(font.family.clone()))),
            _ => self
                .resolve_generic_family(family, weight, style, width)
                .and_then(|font_id| self.document_fonts.get(font_id))
                .map(|font| parley_font_family_source(&FontFamily::named(font.family.clone()))),
        }
    }

    pub(super) fn resolve_single_family(
        &mut self,
        name: &str,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Option<usize> {
        let cache_key = FontRequest::single_name(name, weight, style, width);
        if let Some(id) = self.family_cache.get(&cache_key) {
            return Some(*id);
        }

        if let Some(family) = standard_ui_family_alias(name) {
            return self.resolve_generic_family(&family, weight, style, width);
        }
        if is_private_standard_ui_family_name(name) {
            return None;
        }

        if let Some(id) = self.load_document_font_for_families(
            &[FontiqueQueryFamily::Named(fontique_family_name(name))],
            weight,
            style,
            width,
            None,
            &cache_key,
        ) {
            self.family_cache.insert(cache_key, id);
            return Some(id);
        }

        None
    }

    pub(crate) fn resolve_system_fallback_for_character(
        &mut self,
        character: char,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Option<usize> {
        let cache_key = FallbackRequest::new(character, weight, style, width);
        if let Some(font_id) = self.fallback_cache.get(&cache_key) {
            return *font_id;
        }

        // CSS Fonts forbids installed-font fallback for Private Use Area code
        // points. A missing-glyph representation is required instead.
        // <https://www.w3.org/TR/css-fonts-4/#character-handling-issues>
        if character_is_private_use(character) {
            self.fallback_cache.insert(cache_key, None);
            return None;
        }

        let request = FontRequest::single_name("<platform fallback>", weight, style, width);
        for font in self.query_platform_fallback_fonts(character, weight, style, width, None) {
            if !DocumentFontRegistry::font_query_has_character(&font, character) {
                continue;
            }
            if let Some(font_id) = self.document_font_from_query_font_with_synthesis(
                font,
                None,
                &request,
                true,
                true,
                standard_font_variation_coordinates(weight, style, width),
            ) {
                let font = self.document_fonts.get(font_id)?;
                log::debug!(
                    "resolved platform fallback font for U+{:04X} to {} ({})",
                    character as u32,
                    font.family,
                    font.post_script_name
                );
                self.fallback_cache.insert(cache_key, Some(font_id));
                return Some(font_id);
            }
        }

        self.fallback_cache.insert(cache_key, None);
        None
    }

    pub(super) fn resolve_system_fallback_for_style_character(
        &mut self,
        style: &ComputedStyle,
        character: char,
    ) -> Option<usize> {
        let locale = style
            .language
            .as_deref()
            .and_then(|language| language.parse::<fontique::Language>().ok());
        let request = FontRequest::single_name(
            "<platform fallback>",
            style.font_weight,
            style.font_style,
            style.font_width,
        );
        let variation_coordinates =
            effective_font_variation_coordinates(&ParleyStyleView::new(style));
        for font in self.query_platform_fallback_fonts(
            character,
            style.font_weight,
            style.font_style,
            style.font_width,
            locale.as_ref(),
        ) {
            if DocumentFontRegistry::font_query_has_character(&font, character)
                && let Some(font_id) = self.document_font_from_query_font_with_synthesis(
                    font,
                    None,
                    &request,
                    style.font_synthesis.weight,
                    style.font_synthesis.style,
                    variation_coordinates.clone(),
                )
            {
                return Some(font_id);
            }
        }
        None
    }
}

fn character_is_private_use(character: char) -> bool {
    matches!(
        character as u32,
        0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD
    )
}
