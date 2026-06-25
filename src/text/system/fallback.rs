use super::*;

impl FontSystem {
    pub(super) fn resolve_family_fallback_for_character(
        &mut self,
        style: &ComputedStyle,
        character: char,
    ) -> Option<usize> {
        if let FontFamily::Names(names) = &style.font_family {
            for name in names {
                if let Some(font_id) = self.resolve_single_family(
                    name,
                    style.font_weight,
                    style.font_style,
                    style.font_width,
                ) && self.document_fonts.font_has_character(font_id, character)
                {
                    return Some(font_id);
                }
            }
        }

        if let Some(font_id) = self.resolve_generic_family(
            &style.font_family,
            style.font_weight,
            style.font_style,
            style.font_width,
        ) && self.document_fonts.font_has_character(font_id, character)
        {
            return Some(font_id);
        }

        if style.font_family != FontFamily::SansSerif
            && let Some(font_id) = self.resolve_generic_family(
                &FontFamily::SansSerif,
                style.font_weight,
                style.font_style,
                style.font_width,
            )
            && self.document_fonts.font_has_character(font_id, character)
        {
            return Some(font_id);
        }

        self.resolve_system_fallback_for_character(
            character,
            style.font_weight,
            style.font_style,
            style.font_width,
        )
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
        if let Some(id) =
            self.load_document_font_for_families(&families, weight, style, width, None, &cache_key)
        {
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

        if let Some(generic) = known_font_family(name) {
            return self.resolve_generic_family(&generic, weight, style, width);
        }

        if let Some(id) = self.load_document_font_for_families(
            &[family_query(name)],
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

    pub(super) fn resolve_system_fallback_for_character(
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

        let request = FontRequest {
            family_list: self
                .visible_fallback_families
                .iter()
                .map(|name| FontFamilyRequest::Named(normalize_family(name)))
                .collect(),
            attributes: font_request_attributes(weight, style, width),
        };
        for family_name in self.visible_fallback_families.clone() {
            for font in self.query_fonts(
                &[FontiqueQueryFamily::Named(&family_name)],
                weight,
                style,
                width,
            ) {
                if !DocumentFontRegistry::font_query_has_character(&font, character) {
                    continue;
                }
                if let Some(font_id) = self.document_font_from_query_font(font, None, &request) {
                    let font = self.document_fonts.get(font_id)?;
                    log::debug!(
                        "resolved fallback font for U+{:04X} to {} ({})",
                        character as u32,
                        font.family,
                        font.post_script_name
                    );
                    self.fallback_cache.insert(cache_key, Some(font_id));
                    return Some(font_id);
                }
            }
        }

        self.fallback_cache.insert(cache_key, None);
        None
    }
}
