use super::*;

impl FontSystem {
    pub(crate) fn resolve_font_family(
        &mut self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Option<usize> {
        if let FontFamily::List(families) = family {
            return families
                .iter()
                .find_map(|family| self.resolve_font_family(family, weight, style, width));
        }
        let FontFamily::Named(name) = family else {
            return self.resolve_generic_family(family, weight, style, width);
        };
        let cache_key = FontRequest::from_family(family, weight, style, width);
        if let Some(id) = self.family_cache.get(&cache_key) {
            return Some(*id);
        }

        let name = name.as_str();
        log::trace!(
            "resolving CSS font-family name {} weight={} style={:?} width={}",
            name,
            weight.0,
            style,
            width.0
        );
        if let Some(family) = standard_ui_family_alias(name)
            && let Some(id) = self.resolve_generic_family(&family, weight, style, width)
        {
            self.family_cache.insert(cache_key, id);
            return Some(id);
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
            &FontRequest::single_name(name, weight, style, width),
        ) {
            let font = self.document_fonts.get(id)?;
            log::debug!(
                "resolved CSS font-family {} to font {} ({})",
                name,
                font.family,
                font.post_script_name
            );
            self.family_cache.insert(cache_key, id);
            self.family_cache
                .insert(FontRequest::single_name(name, weight, style, width), id);
            return Some(id);
        }
        log::trace!(
            "CSS font-family name {} did not resolve to a document/system font candidate",
            name
        );

        log::trace!("CSS font-family {:?} did not resolve", family);
        None
    }

    /// Resolve the first available font used for CSS line metrics.
    ///
    /// CSS 2.2 positions the explicit `line-height` inline box from the element's
    /// selected font metrics, while CSS Fonts `unicode-range` only decides which
    /// face is used for each character. Metric-only lookups therefore walk the
    /// authored family list directly and avoid full-stack caches that may have
    /// been populated while shaping a later fallback glyph:
    /// <https://www.w3.org/TR/CSS22/visudet.html#line-height> and
    /// <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>.
    pub(crate) fn resolve_metric_font_for_style(&mut self, style: &ComputedStyle) -> Option<usize> {
        self.resolve_metric_font_for_family(style, &style.font_family)
    }

    pub(super) fn resolve_metric_font_for_family(
        &mut self,
        style: &ComputedStyle,
        family: &FontFamily,
    ) -> Option<usize> {
        // The element's first available font is the first face in its font
        // list that can render U+0020.  A `unicode-range` descriptor can make
        // an otherwise loadable face unavailable for that purpose, so this
        // must share the character-selection path used for shaping instead of
        // merely returning the first resolvable family.
        // <https://www.w3.org/TR/css-fonts-4/#first-available-font>
        self.resolve_family_fallback_for_character_in_family(style, family, ' ')
    }
}
