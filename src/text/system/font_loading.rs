use super::*;

impl FontSystem {
    pub(crate) fn start_loading() -> FontSystemLoad {
        FontSystemLoad {
            parley_font_context: tokio::task::spawn_blocking(load_parley_font_context),
        }
    }

    #[cfg(test)]
    pub(super) fn sync_seed() -> FontSystemSeed {
        let loaded = load_parley_font_context();
        FontSystemSeed {
            parley_font_context: loaded.parley_font_context,
            registered_font_faces: HashMap::new(),
            font_feature_values: FontFeatureValues::default(),
            font_feature_defaults_by_family: HashMap::new(),
            visible_fallback_families: loaded.visible_fallback_families,
        }
    }

    pub(super) fn from_seed(seed: FontSystemSeed) -> Self {
        Self {
            parley_font_context: seed.parley_font_context,
            parley_layout_context: ParleyLayoutContext::new(),
            document_fonts: DocumentFontRegistry::new(seed.registered_font_faces),
            family_cache: HashMap::new(),
            fallback_cache: HashMap::new(),
            font_feature_values: seed.font_feature_values,
            font_feature_defaults_by_family: seed.font_feature_defaults_by_family,
            visible_fallback_families: seed.visible_fallback_families,
        }
    }
}

impl FontSystemLoad {
    pub(crate) fn load_stylesheet_fonts(self, stylesheets: &[Stylesheet]) -> FontSystemSeedLoad {
        let font_faces = stylesheets
            .iter()
            .flat_map(|stylesheet| stylesheet.font_faces.iter().cloned())
            .collect::<Vec<_>>();
        log::trace!(
            "preparing to load {} @font-face rule(s) from {} stylesheet(s)",
            font_faces.len(),
            stylesheets.len()
        );
        let mut font_feature_values = FontFeatureValues::default();
        for stylesheet in stylesheets {
            font_feature_values.extend(stylesheet.font_feature_values.clone());
        }
        FontSystemSeedLoad {
            parley_font_context: self.parley_font_context,
            font_faces: tokio::spawn(load_font_faces(font_faces)),
            font_feature_values,
        }
    }
}

impl FontSystemSeedLoad {
    pub(crate) async fn finish(self) -> FontSystem {
        let (loaded_context, font_faces) = tokio::join!(self.parley_font_context, self.font_faces);
        let font_faces = match font_faces {
            Ok(font_faces) => font_faces,
            Err(error) => {
                log::warn!("@font-face loading task failed: {error}");
                Vec::new()
            }
        };
        let loaded_context = match loaded_context {
            Ok(loaded_context) => loaded_context,
            Err(error) => {
                log::warn!("Parley font context loading task failed: {error}");
                load_parley_font_context()
            }
        };
        if font_faces.is_empty() {
            return FontSystem::from_seed(FontSystemSeed {
                parley_font_context: loaded_context.parley_font_context,
                registered_font_faces: HashMap::new(),
                font_feature_values: self.font_feature_values,
                font_feature_defaults_by_family: HashMap::new(),
                visible_fallback_families: loaded_context.visible_fallback_families,
            });
        }

        let seed =
            tokio::task::spawn_blocking(|| register_loaded_font_faces(loaded_context, font_faces))
                .await;
        match seed {
            Ok((loaded_context, registered_font_faces)) => FontSystem::from_seed(FontSystemSeed {
                parley_font_context: loaded_context.parley_font_context,
                font_feature_defaults_by_family: registered_font_face_defaults_by_family(
                    &registered_font_faces,
                ),
                registered_font_faces: registered_font_faces
                    .into_iter()
                    .map(|face| (face.key, face.metadata))
                    .collect(),
                font_feature_values: self.font_feature_values,
                visible_fallback_families: loaded_context.visible_fallback_families,
            }),
            Err(error) => {
                log::warn!("@font-face registration task failed: {error}");
                let loaded_context = load_parley_font_context();
                FontSystem::from_seed(FontSystemSeed {
                    parley_font_context: loaded_context.parley_font_context,
                    registered_font_faces: HashMap::new(),
                    font_feature_values: self.font_feature_values,
                    font_feature_defaults_by_family: HashMap::new(),
                    visible_fallback_families: loaded_context.visible_fallback_families,
                })
            }
        }
    }
}

fn load_parley_font_context() -> LoadedParleyFontContext {
    let started = std::time::Instant::now();
    let mut context = ParleyFontContext::new();
    let visible_fallback_families = visible_fallback_family_names(&mut context.collection);
    install_visible_common_script_fallbacks(&mut context, &visible_fallback_families);
    let family_count = context.collection.family_names().count();
    log::debug!(
        "loaded Parley/fontique font context with {} family name(s) in {:.3?}",
        family_count,
        started.elapsed()
    );
    LoadedParleyFontContext {
        parley_font_context: context,
        visible_fallback_families,
    }
}

fn visible_fallback_family_names(collection: &mut fontique::Collection) -> Vec<String> {
    let mut families = collection
        .family_names()
        .map(|name| (fallback_family_score(name), name.to_string()))
        .collect::<Vec<_>>();
    families.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    families.into_iter().map(|(_, name)| name).collect()
}

fn install_visible_common_script_fallbacks(context: &mut ParleyFontContext, families: &[String]) {
    let family_ids = families
        .iter()
        .filter_map(|name| context.collection.family_id(name))
        .collect::<Vec<_>>();
    if family_ids.is_empty() {
        return;
    }

    for script in [
        FontiqueScript::COMMON,
        FontiqueScript::INHERITED,
        FontiqueScript::UNKNOWN,
        FontiqueScript::from_bytes(*b"Latn"),
    ] {
        context.collection.set_fallbacks(
            FontiqueFallbackKey::new(script, None),
            family_ids.iter().copied(),
        );
    }
}

async fn load_font_faces(font_faces: Vec<CssFontFace>) -> Vec<LoadedFontFace> {
    log::trace!(
        "starting async load of {} @font-face rule(s)",
        font_faces.len()
    );
    let mut handles = Vec::with_capacity(font_faces.len());
    for font_face in font_faces {
        handles.push(tokio::spawn(load_font_face(font_face)));
    }

    let mut loaded = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.await {
            Ok(font_face) => loaded.push(font_face),
            Err(error) => log::warn!("@font-face source loading task failed: {error}"),
        }
    }
    loaded
}

async fn load_font_face(font_face: CssFontFace) -> LoadedFontFace {
    log::trace!(
        "loading @font-face family {} with {} source(s)",
        font_face.family,
        font_face.sources.len()
    );
    let data = load_first_font_face_source(&font_face).await;
    match &data {
        Some(data) => log::trace!(
            "loaded @font-face family {} source data ({} byte(s))",
            font_face.family,
            data.len()
        ),
        None => log::trace!(
            "no loadable source found for @font-face family {}",
            font_face.family
        ),
    }
    LoadedFontFace { font_face, data }
}

async fn load_first_font_face_source(font_face: &CssFontFace) -> Option<Vec<u8>> {
    for source in &font_face.sources {
        log::trace!(
            "trying @font-face family {} source {:?}",
            font_face.family,
            source
        );
        if let Some(data) = load_font_source_async(source).await {
            log::trace!(
                "@font-face family {} source {:?} loaded before WOFF decode ({} byte(s))",
                font_face.family,
                source,
                data.len()
            );
            return Some(woff::decode_if_woff(data));
        }
        log::trace!(
            "@font-face family {} source {:?} did not load",
            font_face.family,
            source
        );
    }
    None
}

async fn load_font_source_async(source: &FontFaceSource) -> Option<Vec<u8>> {
    match source {
        FontFaceSource::Url {
            value,
            base_url,
            root_url,
        } => {
            log::trace!(
                "loading @font-face URL source value={value:?} base_url={} root_url={}",
                display_optional_path(base_url.as_deref()),
                display_optional_path(root_url.as_deref())
            );
            load_font_url_async(value, base_url.as_deref(), root_url.as_deref()).await
        }
    }
}

async fn load_font_url_async(
    value: &str,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
) -> Option<Vec<u8>> {
    if value.starts_with("data:") {
        let decoded = decode_data_url(value);
        if let Some(data) = &decoded {
            log::trace!("decoded @font-face data URL ({} byte(s))", data.len());
        } else {
            log::trace!("failed to decode @font-face data URL");
        }
        return decoded;
    }
    let Some(path) = crate::resource::resolve_url_path(value, base_url, root_url) else {
        log::trace!(
            "could not resolve @font-face URL source value={value:?} base_url={} root_url={}",
            display_optional_path(base_url),
            display_optional_path(root_url)
        );
        return None;
    };
    log::trace!(
        "resolved @font-face URL source value={value:?} to {}",
        path.display()
    );
    match crate::resource::read_bytes(&path).await {
        Ok(data) => {
            log::trace!(
                "loaded @font-face resource {} ({} byte(s))",
                path.display(),
                data.len()
            );
            Some(data)
        }
        Err(error) => {
            log::debug!("failed to read font {}: {}", path.display(), error);
            log::trace!(
                "failed to load @font-face resource {}: {}",
                path.display(),
                error
            );
            None
        }
    }
}

fn display_optional_path(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "<none>".to_string())
}

fn register_loaded_font_faces(
    mut loaded_context: LoadedParleyFontContext,
    font_faces: Vec<LoadedFontFace>,
) -> (LoadedParleyFontContext, Vec<RegisteredFontFace>) {
    let mut registered_faces = Vec::new();
    for loaded in font_faces {
        registered_faces.extend(register_loaded_font_face(
            &mut loaded_context.parley_font_context,
            &loaded.font_face,
            loaded.data,
        ));
    }
    (loaded_context, registered_faces)
}

fn register_loaded_font_face(
    parley_font_context: &mut ParleyFontContext,
    font_face: &CssFontFace,
    data: Option<Vec<u8>>,
) -> Vec<RegisteredFontFace> {
    let Some(data) = data else {
        log::warn!("unable to load @font-face family {}", font_face.family);
        return Vec::new();
    };
    log::trace!(
        "registering @font-face family {} with {} byte(s)",
        font_face.family,
        data.len()
    );
    let blob = FontiqueBlob::new(Arc::new(data));
    let registered = parley_font_context.collection.register_fonts(
        blob.clone(),
        Some(FontInfoOverride {
            family_name: Some(&font_face.family),
            width: Some(fontique_width(font_face.width)),
            style: Some(fontique_style(font_face.style)),
            weight: Some(fontique_weight(font_face.weight)),
            axes: None,
        }),
    );
    let font_count = registered
        .iter()
        .map(|(_, fonts)| fonts.len())
        .sum::<usize>();
    if font_count == 0 {
        log::warn!("unable to load @font-face family {}", font_face.family);
        log::trace!(
            "fontique registered zero faces for @font-face family {}",
            font_face.family
        );
        return Vec::new();
    }

    log::debug!(
        "registered @font-face family {} with {} face(s)",
        font_face.family,
        font_count
    );
    registered
        .into_iter()
        .flat_map(|(_, fonts)| fonts)
        .map(|font| RegisteredFontFace {
            key: FontBlobFaceKey {
                blob_id: blob.id(),
                face_index: font.index(),
            },
            metadata: RegisteredFontFaceMetadata {
                family: font_face.family.clone(),
                feature_defaults: FontFaceFeatureDefaults::from_font_face(font_face),
                unicode_range: font_face.unicode_range.clone(),
            },
        })
        .collect()
}

fn registered_font_face_defaults_by_family(
    faces: &[RegisteredFontFace],
) -> HashMap<String, FontFaceFeatureDefaults> {
    let mut defaults = HashMap::new();
    for face in faces {
        if !face.metadata.feature_defaults.is_normal() {
            defaults.insert(
                face.metadata.family.trim().to_ascii_lowercase(),
                face.metadata.feature_defaults.clone(),
            );
        }
    }
    defaults
}

impl FontSystem {
    pub(super) fn load_document_font_for_families(
        &mut self,
        families: &[FontiqueQueryFamily<'_>],
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
        family_override: Option<&str>,
        request: &FontRequest,
    ) -> Option<usize> {
        log::trace!(
            "querying document font families {:?} weight={} style={:?} width={} override={:?}",
            families,
            weight.0,
            style,
            width.0,
            family_override
        );
        let fonts = self.query_fonts(families, weight, style, width);
        log::trace!(
            "font query for families {:?} returned {} candidate(s)",
            families,
            fonts.len()
        );
        for font in fonts.iter().filter(|font| !font.synthesis.any()).cloned() {
            if let Some(id) = self.document_font_from_query_font(font, family_override, request) {
                return Some(id);
            }
        }
        for font in fonts {
            if let Some(id) = self.document_font_from_query_font(font, family_override, request) {
                return Some(id);
            }
        }
        None
    }

    pub(crate) fn query_fonts(
        &mut self,
        families: &[FontiqueQueryFamily<'_>],
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Vec<FontiqueQueryFont> {
        let mut fonts = Vec::new();
        let mut query = self
            .parley_font_context
            .collection
            .query(&mut self.parley_font_context.source_cache);
        query.set_families(families.iter().copied());
        query.set_attributes(fontique_attributes(weight, style, width));
        query.matches_with(|font| {
            fonts.push(font.clone());
            FontiqueQueryStatus::Continue
        });
        fonts
    }

    pub(crate) fn document_font_from_query_font(
        &mut self,
        font: FontiqueQueryFont,
        family_override: Option<&str>,
        request: &FontRequest,
    ) -> Option<usize> {
        self.document_fonts.document_font_from_query(
            &mut self.parley_font_context.collection,
            font,
            family_override,
            request,
        )
    }

    pub(super) fn document_font_from_parley_font_data(
        &mut self,
        font_data: &parley::FontData,
    ) -> Option<usize> {
        self.document_fonts.document_font_from_parley(font_data)
    }

    pub(super) fn document_font_from_parley_font_data_for_style(
        &mut self,
        font_data: &parley::FontData,
        style: &ComputedStyle,
    ) -> Option<usize> {
        let request = FontRequest::from_family(
            &style.font_family,
            style.font_weight,
            style.font_style,
            style.font_width,
        );
        if let Some(font_id) = self.document_fonts.cached_parley_font(font_data, &request) {
            return Some(font_id);
        }

        let families = font_families_for_style(&style.font_family, style.font_weight);
        if let Some(font) = self.match_parley_font_data(
            font_data,
            &families,
            style.font_weight,
            style.font_style,
            style.font_width,
        ) {
            let font_id = self.document_font_from_query_font(font, None, &request)?;
            self.document_fonts
                .cache_parley_font(font_data, &request, font_id);
            return Some(font_id);
        }

        for family_name in self.visible_fallback_families.clone() {
            let families = [family_query(&family_name)];
            if let Some(font) = self.match_parley_font_data(
                font_data,
                &families,
                style.font_weight,
                style.font_style,
                style.font_width,
            ) {
                let font_id = self.document_font_from_query_font(font, None, &request)?;
                self.document_fonts
                    .cache_parley_font(font_data, &request, font_id);
                return Some(font_id);
            }
        }

        let font_id = self.document_font_from_parley_font_data(font_data)?;
        self.document_fonts
            .cache_parley_font(font_data, &request, font_id);
        Some(font_id)
    }

    fn match_parley_font_data(
        &mut self,
        font_data: &parley::FontData,
        families: &[FontiqueQueryFamily<'_>],
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Option<FontiqueQueryFont> {
        let fonts = self.query_fonts(families, weight, style, width);
        fonts
            .iter()
            .filter(|font| !font.synthesis.any())
            .find(|font| font_matches_parley_font_data(font, font_data))
            .cloned()
            .or_else(|| {
                fonts
                    .into_iter()
                    .find(|font| font_matches_parley_font_data(font, font_data))
            })
    }
}

fn font_families_for_style(
    family: &FontFamily,
    weight: FontWeight,
) -> Cow<'_, [FontiqueQueryFamily<'_>]> {
    match family {
        FontFamily::Names(names) => {
            Cow::Owned(names.iter().map(|name| family_query(name)).collect())
        }
        generic => Cow::Borrowed(generic_query_families(generic, weight).unwrap_or(&[])),
    }
}

fn font_matches_parley_font_data(font: &FontiqueQueryFont, font_data: &parley::FontData) -> bool {
    font.blob.id() == font_data.data.id() && font.index == font_data.index
}

pub(super) fn post_script_name_for_face(
    face: &ttf_parser::Face<'_>,
    family: &str,
    synthesize_bold: bool,
    synthesize_italic: bool,
) -> String {
    let mut name = opentype_name(face, ttf_parser::name_id::POST_SCRIPT_NAME)
        .or_else(|| opentype_name(face, ttf_parser::name_id::FULL_NAME))
        .unwrap_or_else(|| family.to_string());
    let normalized = name.to_ascii_lowercase();
    if (face.is_bold() || synthesize_bold) && !font_label_has_bold(&normalized) {
        name.push_str("-Bold");
    }
    if (face.is_italic() || synthesize_italic) && !font_label_has_italic(&normalized) {
        name.push_str("-Italic");
    }
    let sanitized = sanitize_pdf_name(&name);
    if sanitized.is_empty() {
        sanitize_pdf_name(family)
    } else {
        sanitized
    }
}

fn font_label_has_bold(label: &str) -> bool {
    label.contains("bold") || label.contains("black") || label.contains("heavy")
}

fn font_label_has_italic(label: &str) -> bool {
    label.contains("italic") || label.contains("oblique")
}
