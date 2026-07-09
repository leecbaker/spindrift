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
        let FontFamily::Names(names) = family else {
            return self.resolve_generic_family(family, weight, style, width);
        };
        let cache_key = FontRequest::from_family(family, weight, style, width);
        if let Some(id) = self.family_cache.get(&cache_key) {
            return Some(*id);
        }

        for name in names {
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
            if let Some(id) = self.load_document_font_for_families(
                &[FontiqueQueryFamily::Named(name)],
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
        }

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
        if let FontFamily::List(families) = &style.font_family {
            return families.iter().find_map(|family| {
                self.resolve_font_family(
                    family,
                    style.font_weight,
                    style.font_style,
                    style.font_width,
                )
            });
        }
        if let FontFamily::Names(names) = &style.font_family {
            for name in names {
                if let Some(id) = self.resolve_single_family(
                    name,
                    style.font_weight,
                    style.font_style,
                    style.font_width,
                ) {
                    return Some(id);
                }
            }
        } else if let Some(id) = self.resolve_generic_family(
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
}
