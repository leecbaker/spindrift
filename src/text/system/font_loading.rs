use super::*;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::sync::Mutex;
use tokio::sync::OnceCell;

type SharedFontSourceCache =
    Arc<tokio::sync::Mutex<HashMap<FontSourceCacheKey, Arc<OnceCell<FontiqueBlob<u8>>>>>>;
type SharedFontProgramCache = Arc<Mutex<HashMap<[u8; 32], Vec<FontiqueBlob<u8>>>>>;

#[derive(Clone)]
struct FontProgramCache {
    sources: SharedFontSourceCache,
    programs: SharedFontProgramCache,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum FontSourceCacheKey {
    Data(String),
    Url(url::Url),
}

impl FontProgramCache {
    fn new() -> Self {
        Self {
            sources: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            programs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn load_source<F, Fut>(
        &self,
        key: FontSourceCacheKey,
        load: F,
    ) -> crate::Result<FontiqueBlob<u8>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = crate::Result<Vec<u8>>>,
    {
        let cell = {
            let mut sources = self.sources.lock().await;
            Arc::clone(
                sources
                    .entry(key)
                    .or_insert_with(|| Arc::new(OnceCell::new())),
            )
        };
        cell.get_or_try_init(|| async {
            let data = woff::decode_if_woff(load().await?);
            Ok::<_, crate::Error>(self.intern_program(data))
        })
        .await
        .cloned()
    }

    fn intern_program(&self, data: Vec<u8>) -> FontiqueBlob<u8> {
        let digest: [u8; 32] = Sha256::digest(&data).into();
        let mut programs = self
            .programs
            .lock()
            .expect("font program cache mutex must not be poisoned");
        let candidates = programs.entry(digest).or_default();
        if let Some(existing) = candidates
            .iter()
            .find(|existing| existing.as_ref() == data.as_slice())
        {
            return existing.clone();
        }
        let blob = FontiqueBlob::new(Arc::new(data));
        candidates.push(blob.clone());
        blob
    }
}

impl FontSystem {
    pub(crate) fn start_loading() -> FontSystemLoad {
        #[cfg(not(target_arch = "wasm32"))]
        {
            FontSystemLoad {
                parley_font_context: tokio::task::spawn_blocking(load_parley_font_context),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            FontSystemLoad {
                parley_font_context: load_parley_font_context(),
            }
        }
    }

    #[cfg(test)]
    pub(super) fn sync_seed() -> FontSystemSeed {
        let loaded = load_parley_font_context();
        FontSystemSeed {
            parley_font_context: loaded.parley_font_context,
            registered_font_faces: HashMap::new(),
            font_feature_values: FontFeatureValues::default(),
            font_palette_values: FontPaletteValues::default(),
        }
    }

    pub(super) fn from_seed(seed: FontSystemSeed) -> Self {
        Self {
            parley_font_context: seed.parley_font_context,
            parley_layout_context: ParleyLayoutContext::new(),
            parley_layout_scratch: ParleyLayout::default(),
            document_fonts: DocumentFontRegistry::new(seed.registered_font_faces),
            family_cache: HashMap::new(),
            fallback_cache: HashMap::new(),
            font_feature_values: seed.font_feature_values,
            font_palette_values: seed.font_palette_values,
        }
    }
}

impl FontSystemLoad {
    #[cfg(test)]
    pub(crate) fn load_stylesheet_fonts<Collection: StylesheetCollection + ?Sized>(
        self,
        stylesheets: &Collection,
    ) -> FontSystemSeedLoad {
        let fetcher = crate::resource::ResourceFetcher::new(crate::ResourcePolicy::default())
            .expect("default resource policy must create an HTTP client");
        self.load_stylesheet_fonts_with_fetcher(stylesheets, fetcher)
    }

    pub(crate) fn load_stylesheet_fonts_with_fetcher<Collection: StylesheetCollection + ?Sized>(
        self,
        stylesheets: &Collection,
        resource_fetcher: crate::resource::ResourceFetcher,
    ) -> FontSystemSeedLoad {
        let stylesheets = stylesheets.stylesheet_view();
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
        let mut font_palette_values = FontPaletteValues::default();
        for stylesheet in stylesheets.iter() {
            font_feature_values.extend(stylesheet.font_feature_values.clone());
            font_palette_values.extend(stylesheet.font_palette_values.clone());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            FontSystemSeedLoad {
                parley_font_context: self.parley_font_context,
                font_faces: tokio::spawn(load_font_faces(font_faces, resource_fetcher)),
                font_feature_values,
                font_palette_values,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            FontSystemSeedLoad {
                parley_font_context: self.parley_font_context,
                font_faces,
                resource_fetcher,
                font_feature_values,
                font_palette_values,
            }
        }
    }
}

impl FontSystemSeedLoad {
    #[cfg(test)]
    pub(crate) async fn finish(self) -> FontSystem {
        self.finish_inner(false).await.unwrap_or_else(|error| {
            log::warn!("@font-face loading failed: {error}");
            FontSystem::from_seed(FontSystemSeed {
                parley_font_context: load_parley_font_context().parley_font_context,
                registered_font_faces: HashMap::new(),
                font_feature_values: FontFeatureValues::default(),
                font_palette_values: FontPaletteValues::default(),
            })
        })
    }

    pub(crate) async fn finish_checked(self) -> crate::Result<FontSystem> {
        self.finish_inner(true).await
    }

    async fn finish_inner(self, fail_on_font_error: bool) -> crate::Result<FontSystem> {
        let (mut loaded_context, font_faces, font_feature_values, font_palette_values) = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let Self {
                    parley_font_context,
                    font_faces,
                    font_feature_values,
                    font_palette_values,
                } = self;
                // Both tasks were started before this future is polled. Awaiting them
                // in sequence therefore only chooses the order in which their results
                // are observed; it does not serialize their loading work.
                let loaded_context = parley_font_context.await;
                let font_faces = font_faces.await;
                let font_faces = match font_faces {
                    Ok(Ok(font_faces)) => font_faces,
                    Ok(Err(error)) if fail_on_font_error => return Err(error),
                    Ok(Err(error)) => {
                        log::warn!("@font-face loading failed: {error}");
                        Vec::new()
                    }
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
                (
                    loaded_context,
                    font_faces,
                    font_feature_values,
                    font_palette_values,
                )
            }

            #[cfg(target_arch = "wasm32")]
            {
                let Self {
                    parley_font_context,
                    font_faces,
                    resource_fetcher,
                    font_feature_values,
                    font_palette_values,
                } = self;
                let font_faces = match load_font_faces(font_faces, resource_fetcher).await {
                    Ok(font_faces) => font_faces,
                    Err(error) if fail_on_font_error => return Err(error),
                    Err(error) => {
                        log::warn!("@font-face loading failed: {error}");
                        Vec::new()
                    }
                };
                (
                    parley_font_context,
                    font_faces,
                    font_feature_values,
                    font_palette_values,
                )
            }
        };
        if font_faces.is_empty() {
            return Ok(FontSystem::from_seed(FontSystemSeed {
                parley_font_context: loaded_context.parley_font_context,
                registered_font_faces: HashMap::new(),
                font_feature_values,
                font_palette_values,
            }));
        }

        let registered_font_faces = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let registration = tokio::task::spawn_blocking(|| {
                    register_loaded_font_faces(loaded_context, font_faces)
                })
                .await;
                match registration {
                    Ok((context, font_faces)) => {
                        loaded_context = context;
                        font_faces
                    }
                    Err(error) => {
                        log::warn!("@font-face registration task failed: {error}");
                        loaded_context = load_parley_font_context();
                        Vec::new()
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            {
                let (context, font_faces) = register_loaded_font_faces(loaded_context, font_faces);
                loaded_context = context;
                font_faces
            }
        };
        Ok(FontSystem::from_seed(FontSystemSeed {
            parley_font_context: loaded_context.parley_font_context,
            registered_font_faces: registered_font_faces
                .into_iter()
                .map(|face| (face.key, face.metadata))
                .collect(),
            font_feature_values,
            font_palette_values,
        }))
    }
}

fn load_parley_font_context() -> LoadedParleyFontContext {
    let started = std::time::Instant::now();
    let mut context = ParleyFontContext::new();
    let family_count = context.collection.family_names().count();
    log::debug!(
        "loaded Parley/fontique font context with {} family name(s) in {:.3?}",
        family_count,
        started.elapsed()
    );
    LoadedParleyFontContext {
        parley_font_context: context,
    }
}

async fn load_font_faces(
    font_faces: Vec<CssFontFace>,
    resource_fetcher: crate::resource::ResourceFetcher,
) -> crate::Result<Vec<LoadedFontFace>> {
    log::trace!(
        "starting async load of {} @font-face rule(s)",
        font_faces.len()
    );
    let cache = FontProgramCache::new();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let handles = font_faces
            .into_iter()
            .map(|font_face| {
                tokio::spawn(load_font_face(
                    font_face,
                    resource_fetcher.clone(),
                    cache.clone(),
                ))
            })
            .collect::<Vec<_>>();

        let mut loaded = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(Ok(font_face)) => loaded.push(font_face),
                Ok(Err(error)) => return Err(error),
                Err(error) => {
                    return Err(crate::Error::InvalidInput(format!(
                        "@font-face source loading task failed: {error}"
                    )));
                }
            }
        }
        Ok(loaded)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let mut loaded = Vec::with_capacity(font_faces.len());
        for font_face in font_faces {
            loaded.push(load_font_face(font_face, resource_fetcher.clone(), cache.clone()).await?);
        }
        Ok(loaded)
    }
}

async fn load_font_face(
    font_face: CssFontFace,
    resource_fetcher: crate::resource::ResourceFetcher,
    cache: FontProgramCache,
) -> crate::Result<LoadedFontFace> {
    log::trace!(
        "loading @font-face family {} with {} source(s)",
        font_face.family,
        font_face.sources.len()
    );
    let data = load_first_font_face_source(&font_face, &resource_fetcher, &cache).await?;
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
    Ok(LoadedFontFace { font_face, data })
}

async fn load_first_font_face_source(
    font_face: &CssFontFace,
    resource_fetcher: &crate::resource::ResourceFetcher,
    cache: &FontProgramCache,
) -> crate::Result<Option<FontiqueBlob<u8>>> {
    for source in &font_face.sources {
        log::trace!(
            "trying @font-face family {} source {:?}",
            font_face.family,
            source
        );
        match load_font_source(source, resource_fetcher, cache).await {
            Ok(Some(data)) => {
                log::trace!(
                    "@font-face family {} source {:?} loaded and decoded ({} byte(s))",
                    font_face.family,
                    source,
                    data.len()
                );
                return Ok(Some(data));
            }
            Ok(None) => {}
            // CSS Fonts makes a failed source fail this face, not the document
            // or any sibling face.  In particular, the fallback list must be
            // tried even when the document's primary-resource policy is
            // otherwise strict.
            // <https://drafts.csswg.org/css-fonts-4/#font-face-loading>
            Err(error) => {
                log::debug!(
                    "failed to load @font-face family {} source {:?}: {error}",
                    font_face.family,
                    source
                );
            }
        }
        log::trace!(
            "@font-face family {} source {:?} did not load",
            font_face.family,
            source
        );
    }
    Ok(None)
}

async fn load_font_source(
    source: &FontFaceSource,
    resource_fetcher: &crate::resource::ResourceFetcher,
    cache: &FontProgramCache,
) -> crate::Result<Option<FontiqueBlob<u8>>> {
    match source {
        FontFaceSource::Url {
            value,
            base_url,
            root_url,
        } => {
            log::trace!(
                "loading @font-face URL source value={value:?} base_url={} root_url={}",
                display_optional_url(base_url.as_ref()),
                display_optional_url(root_url.as_ref())
            );
            load_font_url(
                value,
                base_url.as_ref(),
                root_url.as_ref(),
                resource_fetcher,
                cache,
            )
            .await
        }
    }
}

async fn load_font_url(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_fetcher: &crate::resource::ResourceFetcher,
    cache: &FontProgramCache,
) -> crate::Result<Option<FontiqueBlob<u8>>> {
    if is_data_url(value)? {
        let value = value.to_string();
        let blob = cache
            .load_source(
                FontSourceCacheKey::Data(value.clone()),
                move || async move { decode_font_data_url(&value) },
            )
            .await?;
        log::trace!("decoded @font-face data URL ({} byte(s))", blob.len());
        return Ok(Some(blob));
    }
    let url =
        crate::resource::resolve_fetchable_url(value, base_url, root_url).ok_or_else(|| {
            crate::Error::InvalidInput(format!(
                "could not resolve @font-face URL source value={value:?} base_url={} root_url={}",
                display_optional_url(base_url),
                display_optional_url(root_url)
            ))
        })?;
    log::trace!("resolved @font-face URL source value={value:?} to {}", url);
    let fetcher = resource_fetcher.clone();
    let blob = cache
        .load_source(FontSourceCacheKey::Url(url.clone()), move || async move {
            let fetched = fetcher.fetch(&url).await?;
            log::trace!(
                "loaded @font-face resource {} ({} byte(s))",
                fetched.final_url,
                fetched.bytes.len()
            );
            Ok(fetched.bytes)
        })
        .await?;
    Ok(Some(blob))
}

/// Recognize authored `data:` sources with Fetch's data-URL parser.
///
/// A malformed data URL is still a data source and must fail this source,
/// rather than be resolved as a relative URL. CSS Fonts then permits a later
/// `src` candidate to load.
fn is_data_url(value: &str) -> crate::Result<bool> {
    match data_url::DataUrl::process(value) {
        Ok(_) => Ok(true),
        Err(data_url::DataUrlError::NotADataUrl) => Ok(false),
        Err(error) => Err(invalid_font_data_url(value, error)),
    }
}

/// Decode an `@font-face` data URL according to Fetch's data-URL processor.
///
/// The decoded MIME type is deliberately not used for font selection: font
/// registration validates the program bytes after this source loader returns.
fn decode_font_data_url(value: &str) -> crate::Result<Vec<u8>> {
    let data_url =
        data_url::DataUrl::process(value).map_err(|error| invalid_font_data_url(value, error))?;
    let (bytes, _) = data_url
        .decode_to_vec()
        .map_err(|error| invalid_font_data_url(value, error))?;
    Ok(bytes)
}

fn invalid_font_data_url(error_value: &str, error: impl std::fmt::Display) -> crate::Error {
    crate::Error::InvalidInput(format!(
        "failed to decode @font-face data URL {error_value:?}: {error}"
    ))
}

fn display_optional_url(url: Option<&url::Url>) -> String {
    url.map(ToString::to_string)
        .unwrap_or_else(|| "<none>".to_string())
}

fn register_loaded_font_faces(
    mut loaded_context: LoadedParleyFontContext,
    font_faces: Vec<LoadedFontFace>,
) -> (LoadedParleyFontContext, Vec<RegisteredFontFace>) {
    let registered_faces = font_faces
        .into_iter()
        .flat_map(|loaded| {
            register_loaded_font_face(
                &mut loaded_context.parley_font_context,
                &loaded.font_face,
                loaded.data,
            )
        })
        .collect::<Vec<_>>();
    (loaded_context, registered_faces)
}

fn register_loaded_font_face(
    parley_font_context: &mut ParleyFontContext,
    font_face: &CssFontFace,
    data: Option<FontiqueBlob<u8>>,
) -> Vec<RegisteredFontFace> {
    let Some(blob) = data else {
        log::warn!("unable to load @font-face family {}", font_face.family);
        return Vec::new();
    };
    log::trace!(
        "registering @font-face family {} with {} byte(s)",
        font_face.family,
        blob.len()
    );
    let axis_defaults = fontique_fixed_standard_axis_defaults(
        font_face.weight,
        font_face.weight_is_variable,
        font_face.width,
        font_face.width_is_variable,
    );
    let registered = parley_font_context.collection.register_fonts(
        blob,
        Some(FontInfoOverride {
            family_name: Some(fontique_family_name(&font_face.family)),
            width: (!font_face.width_is_variable).then(|| fontique_width(font_face.width)),
            style: Some(fontique_style(font_face.style)),
            weight: (!font_face.weight_is_variable).then(|| fontique_weight(font_face.weight)),
            // Fontique ignores a supplied axis tag when the program does not
            // expose that axis, preserving fixed-descriptor behavior for
            // ordinary non-variable fonts.
            axes: (!axis_defaults.is_empty()).then_some(axis_defaults.as_slice()),
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
        .flat_map(|(family_id, fonts)| {
            let family = parley_font_context.collection.family(family_id);
            fonts.into_iter().filter_map(move |font| {
                let family_index = family.as_ref()?.fonts().iter().position(|candidate| {
                    candidate.source().id() == font.source().id()
                        && candidate.index() == font.index()
                })?;
                Some(RegisteredFontFace {
                    key: RegisteredFontFaceKey {
                        family_id: family_id.to_u64(),
                        family_index,
                    },
                    metadata: RegisteredFontFaceMetadata {
                        family: font_face.family.clone(),
                        weight: font_face.weight,
                        style: font_face.style,
                        weight_is_variable: font_face.weight_is_variable,
                        feature_defaults: FontFaceFeatureDefaults::from_font_face(font_face),
                        unicode_range: font_face.unicode_range.clone(),
                        size_adjust: font_face.size_adjust,
                        ascent_override: font_face.ascent_override,
                        descent_override: font_face.descent_override,
                        line_gap_override: font_face.line_gap_override,
                        font_variation_settings: font_face.font_variation_settings.clone(),
                    },
                })
            })
        })
        .collect()
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

    /// Resolve a CSS generic-family candidate that can be represented by
    /// Quire's PDF Type 0 outline-font output. Explicit named-family and
    /// `@font-face` selection deliberately use the unrestricted loader above:
    /// those authorship choices must surface an embedding error rather than be
    /// silently replaced.
    pub(super) fn load_outline_embeddable_document_font_for_families(
        &mut self,
        families: &[FontiqueQueryFamily<'_>],
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
        request: &FontRequest,
    ) -> Option<usize> {
        let fonts = self.query_fonts(families, weight, style, width);
        for font in fonts
            .iter()
            .filter(|font| !font.synthesis.any())
            .filter(|font| DocumentFontRegistry::font_query_allows_outline_embedding(font))
            .cloned()
        {
            if let Some(id) = self.document_font_from_query_font(font, None, request) {
                return Some(id);
            }
        }
        for font in fonts
            .into_iter()
            .filter(DocumentFontRegistry::font_query_allows_outline_embedding)
        {
            if let Some(id) = self.document_font_from_query_font(font, None, request) {
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
        query.set_families(families.iter().cloned());
        query.set_attributes(fontique_attributes(weight, style, width));
        query.matches_with(|font| {
            fonts.push(font.clone());
            FontiqueQueryStatus::Continue
        });
        fonts
    }

    /// Query the platform's installed-font fallback for a character.
    ///
    /// CSS Fonts leaves the installed fallback procedure to the user agent
    /// after the authored family list has been exhausted. Fontique delegates
    /// that procedure to the platform backend, which uses CoreText on macOS.
    /// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>
    pub(super) fn query_platform_fallback_fonts(
        &mut self,
        character: char,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
        locale: Option<&fontique::Language>,
    ) -> Vec<FontiqueQueryFont> {
        let mut fonts = Vec::new();
        let mut query = self
            .parley_font_context
            .collection
            .query(&mut self.parley_font_context.source_cache);
        query.set_attributes(fontique_attributes(weight, style, width));
        if character_is_emoji(character) {
            // Fontique's script fallback intentionally has no Common-script
            // sample for CoreText to pass to CTFontCreateForString. Its
            // platform `emoji` generic provides that native selection path.
            query.set_families([FontiqueQueryFamily::Generic(FontiqueGenericFamily::Emoji)]);
        }
        query.set_fallbacks(FontiqueFallbackKey::new(
            fontique_script_for_character(character),
            locale,
        ));
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
        self.document_font_from_query_font_with_synthesis(font, family_override, request, true)
    }

    /// Resolve one Fontique result for PDF emission, retaining only CSS
    /// weight synthesis that the computed style permits.
    pub(crate) fn document_font_from_query_font_with_synthesis(
        &mut self,
        font: FontiqueQueryFont,
        family_override: Option<&str>,
        request: &FontRequest,
        synthesize_weight: bool,
    ) -> Option<usize> {
        self.document_fonts.document_font_from_query(
            &mut self.parley_font_context.collection,
            font,
            family_override,
            request,
            synthesize_weight,
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
        fallback_character: char,
    ) -> Option<usize> {
        self.document_font_from_parley_font_data_for_family(
            font_data,
            style,
            &style.font_family,
            fallback_character,
        )
    }

    pub(super) fn document_font_from_parley_font_data_for_family(
        &mut self,
        font_data: &parley::FontData,
        style: &ComputedStyle,
        family: &FontFamily,
        fallback_character: char,
    ) -> Option<usize> {
        let request = FontRequest::from_family(
            family,
            style.font_weight,
            style.font_style,
            style.font_width,
        );
        let synthesize_weight = style.font_synthesis.weight;
        if let Some(font_id) =
            self.document_fonts
                .cached_parley_font(font_data, &request, synthesize_weight)
        {
            return Some(font_id);
        }

        let families = font_families_for_style(family, style.font_weight);
        if let Some(font) = self.match_parley_font_data(
            font_data,
            &families,
            style.font_weight,
            style.font_style,
            style.font_width,
        ) {
            let font_id = self.document_font_from_query_font_with_synthesis(
                font,
                None,
                &request,
                synthesize_weight,
            )?;
            self.document_fonts
                .cache_parley_font(font_data, &request, synthesize_weight, font_id);
            return Some(font_id);
        }

        if let Some(font) = self.match_platform_fallback_parley_font_data(
            font_data,
            fallback_character,
            style.font_weight,
            style.font_style,
            style.font_width,
        ) {
            let font_id = self.document_font_from_query_font_with_synthesis(
                font,
                None,
                &request,
                synthesize_weight,
            )?;
            self.document_fonts
                .cache_parley_font(font_data, &request, synthesize_weight, font_id);
            return Some(font_id);
        }

        // Preserve an unknown platform fallback rather than scanning every
        // installed family. This is only reached when the backend did not
        // expose a matching fallback face for the run's script.
        let font_id = self.document_font_from_parley_font_data(font_data)?;
        self.document_fonts
            .cache_parley_font(font_data, &request, synthesize_weight, font_id);
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

    fn match_platform_fallback_parley_font_data(
        &mut self,
        font_data: &parley::FontData,
        character: char,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Option<FontiqueQueryFont> {
        let fonts = self.query_platform_fallback_fonts(character, weight, style, width, None);
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

fn fontique_script_for_character(character: char) -> FontiqueScript {
    let script = CodePointMapData::<IcuScript>::new().get(character);
    let tag = PropertyNamesShort::<IcuScript>::new()
        .get_locale_script(script)
        .map(|script| script.into_raw())
        .unwrap_or_else(|| FontiqueScript::UNKNOWN.to_bytes());
    FontiqueScript::from_bytes(tag)
}

fn font_families_for_style(
    family: &FontFamily,
    weight: FontWeight,
) -> Cow<'_, [FontiqueQueryFamily<'_>]> {
    match family {
        FontFamily::Named(name) => {
            // A quoted generic-looking name is still a named family in CSS.
            // Do not turn `"serif"` or `"fantasy"` back into a generic while
            // matching the concrete font selected by Parley.
            Cow::Owned(vec![FontiqueQueryFamily::Named(fontique_family_name(
                name.as_str(),
            ))])
        }
        FontFamily::List(families) => Cow::Owned(
            families
                .iter()
                .flat_map(|family| match family {
                    FontFamily::Named(name) => vec![FontiqueQueryFamily::Named(
                        fontique_family_name(name.as_str()),
                    )],
                    generic => generic_query_families(generic, weight)
                        .unwrap_or(&[])
                        .to_vec(),
                })
                .collect(),
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_data_urls_decode_percent_encoded_bodies() {
        assert_eq!(
            decode_font_data_url("data:font/ttf,%00%01%00%00").unwrap(),
            [0, 1, 0, 0]
        );
    }

    #[test]
    fn font_data_urls_use_fetch_scheme_and_base64_rules() {
        assert!(is_data_url("\nDaTa:font/ttf;base64,S G V s b G8\r").unwrap());
        assert_eq!(
            decode_font_data_url("\nDaTa:font/ttf;base64,S G V s b G8\r").unwrap(),
            b"Hello"
        );
    }

    #[test]
    fn font_data_url_fragments_are_not_part_of_the_font_program() {
        assert_eq!(
            decode_font_data_url("data:font/ttf,%00%01%00%00#not-a-font-byte").unwrap(),
            [0, 1, 0, 0]
        );
    }

    #[test]
    fn malformed_data_urls_fail_as_data_sources() {
        assert!(is_data_url("data:font/ttf;base64,%%% ").unwrap());
        assert!(decode_font_data_url("data:font/ttf;base64,%%% ").is_err());
        assert!(is_data_url("data:font/ttf;base64").is_err());
    }

    #[test]
    fn font_program_cache_reuses_byte_identical_programs() {
        let cache = FontProgramCache::new();
        let first = cache.intern_program(vec![0, 1, 2, 3]);
        let second = cache.intern_program(vec![0, 1, 2, 3]);

        assert_eq!(first.id(), second.id());
    }
}
