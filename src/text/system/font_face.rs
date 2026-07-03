use super::*;

impl FontSystem {
    pub(super) fn resolve_font_family(
        &mut self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Option<usize> {
        let FontFamily::Names(names) = family else {
            return self.resolve_generic_family(family, weight, style, width);
        };
        let cache_key = FontRequest::from_family(family, weight, style, width);
        if let Some(id) = self.family_cache.get(&cache_key) {
            return Some(*id);
        }

        let mut fallback_family = None;
        for name in names {
            log::trace!(
                "resolving CSS font-family name {} weight={} style={:?} width={}",
                name,
                weight.0,
                style,
                width.0
            );
            if let Some(known) = known_font_family(name) {
                log::trace!(
                    "CSS font-family name {} is generic alias {:?}; deferring to fallback",
                    name,
                    known
                );
                fallback_family.get_or_insert(known);
                continue;
            }

            if let Some(id) = self.load_document_font_for_families(
                &[family_query(name)],
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

        if let Some(known) = fallback_family {
            log::trace!(
                "resolving deferred CSS generic font-family fallback {:?}",
                known
            );
            let id = self.resolve_generic_family(&known, weight, style, width)?;
            self.family_cache.insert(cache_key, id);
            return Some(id);
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
        if let FontFamily::Names(names) = &style.font_family {
            let mut fallback_family = None;
            for name in names {
                if let Some(known) = known_font_family(name) {
                    fallback_family.get_or_insert(known);
                    continue;
                }
                if let Some(id) = self.resolve_single_family(
                    name,
                    style.font_weight,
                    style.font_style,
                    style.font_width,
                ) {
                    return Some(id);
                }
            }
            if let Some(known) = fallback_family {
                return self.resolve_generic_family(
                    &known,
                    style.font_weight,
                    style.font_style,
                    style.font_width,
                );
            }
        } else if let Some(id) = self.resolve_generic_family(
            &style.font_family,
            style.font_weight,
            style.font_style,
            style.font_width,
        ) {
            return Some(id);
        }

        self.resolve_generic_family(
            &FontFamily::SansSerif,
            style.font_weight,
            style.font_style,
            style.font_width,
        )
    }
}
