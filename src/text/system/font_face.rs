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
                self.family_cache.insert(cache_key.clone(), id);
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
}
