use crate::dom::{Element, ElementId};
use crate::image_store::{DocumentImageStore, ImageId, ImageMetadata};
use crate::svg::{
    SharedSvgAsset, SvgImageContext, SvgPresentationOverrides,
    parse_inline_svg_with_presentation_overrides, parse_svg_bytes_with_image_context,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use url::Url;

/// Controls how a resource fetch failure affects rendering.
///
/// Quire defaults to [`FetchErrorPolicy::Fail`] for primary documents,
/// explicit stylesheets, and fonts. Layout preloads visual assets separately:
/// a missing image remains an unavailable replaced object so the document can
/// continue rendering. [`FetchErrorPolicy::Allow`] additionally recovers from
/// optional stylesheet and font failures; it never makes a primary HTML or
/// explicitly supplied stylesheet source optional.
///
/// ```no_run
/// use quire::{FetchErrorPolicy, Html, PdfOptions, RenderOptions, ResourcePolicy};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let resource_policy = ResourcePolicy {
///     error_policy: FetchErrorPolicy::Allow,
///     ..ResourcePolicy::default()
/// };
/// let html = Html::from_file("document.html")
///     .await?
///     .with_resource_policy(resource_policy);
/// let mut output = File::create("document.pdf")?;
/// html.write_pdf(&mut output, &RenderOptions::default(), &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FetchErrorPolicy {
    /// Abort rendering when a required resource cannot be fetched.
    #[default]
    Fail,
    /// Continue rendering when an optional resource cannot be fetched.
    Allow,
}

/// A non-zero time limit for a single HTTP(S) resource request.
///
/// The limit covers sending the request and receiving its response body. It
/// does not apply to `data:` or `file:` resources.
///
/// ```no_run
/// use quire::{Html, HttpRequestTimeout, PdfOptions, RenderOptions, ResourcePolicy};
/// use std::{fs::File, time::Duration};
///
/// # async fn render() -> quire::Result<()> {
/// let resource_policy = ResourcePolicy {
///     http_timeout: HttpRequestTimeout::try_from(Duration::from_secs(5))
///         .expect("five seconds is non-zero"),
///     ..ResourcePolicy::default()
/// };
/// let html = Html::from_file("document.html")
///     .await?
///     .with_resource_policy(resource_policy);
/// let mut output = File::create("document.pdf")?;
/// html.write_pdf(&mut output, &RenderOptions::default(), &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HttpRequestTimeout(Duration);

impl HttpRequestTimeout {
    /// The ten-second HTTP request limit used by [`ResourcePolicy::default`].
    pub const DEFAULT: Self = Self(Duration::from_secs(10));

    /// Returns the duration enforced for an HTTP(S) request.
    pub const fn duration(self) -> Duration {
        self.0
    }
}

impl Default for HttpRequestTimeout {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TryFrom<Duration> for HttpRequestTimeout {
    type Error = InvalidHttpRequestTimeout;

    /// Rejects a zero duration, which would make every HTTP request time out
    /// immediately.
    fn try_from(duration: Duration) -> Result<Self, Self::Error> {
        if duration.is_zero() {
            Err(InvalidHttpRequestTimeout)
        } else {
            Ok(Self(duration))
        }
    }
}

/// Error returned when constructing an [`HttpRequestTimeout`] from zero.
///
/// ```no_run
/// use quire::{
///     Error, Html, HttpRequestTimeout, InvalidHttpRequestTimeout, PdfOptions, RenderOptions,
///     ResourcePolicy,
/// };
/// use std::{fs::File, time::Duration};
///
/// # async fn render() -> quire::Result<()> {
/// let timeout = HttpRequestTimeout::try_from(Duration::from_secs(5))
///     .map_err(|error: InvalidHttpRequestTimeout| Error::InvalidInput(error.to_string()))?;
/// let html = Html::from_file("document.html")
///     .await?
///     .with_resource_policy(ResourcePolicy {
///         http_timeout: timeout,
///         ..ResourcePolicy::default()
///     });
/// let mut output = File::create("document.pdf")?;
/// html.write_pdf(&mut output, &RenderOptions::default(), &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidHttpRequestTimeout;

impl fmt::Display for InvalidHttpRequestTimeout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HTTP request timeout must be greater than zero")
    }
}

impl std::error::Error for InvalidHttpRequestTimeout {}

/// Policy used for filesystem and HTTP(S) document resources.
///
/// HTTP redirects are followed by default. Primary resources are fatal by
/// default, unlike WeasyPrint's permissive URL fetcher; HTML image/background
/// preloads always recover as unavailable visual assets.
///
/// ```no_run
/// use quire::{Html, HttpRequestTimeout, PdfOptions, RenderOptions, ResourcePolicy};
/// use std::{fs::File, time::Duration};
///
/// # async fn render() -> quire::Result<()> {
/// let resource_policy = ResourcePolicy {
///     follow_http_redirects: false,
///     http_timeout: HttpRequestTimeout::try_from(Duration::from_secs(5))
///         .expect("five seconds is non-zero"),
///     ..ResourcePolicy::default()
/// };
/// let html = Html::from_file("document.html")
///     .await?
///     .with_resource_policy(resource_policy);
/// let mut output = File::create("document.pdf")?;
/// html.write_pdf(&mut output, &RenderOptions::default(), &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourcePolicy {
    /// Whether HTTP(S) fetches follow redirects.
    pub follow_http_redirects: bool,
    /// The non-zero time limit for each HTTP(S) resource request.
    pub http_timeout: HttpRequestTimeout,
    /// How fetch failures affect rendering.
    pub error_policy: FetchErrorPolicy,
}

impl Default for ResourcePolicy {
    fn default() -> Self {
        Self {
            follow_http_redirects: true,
            http_timeout: HttpRequestTimeout::DEFAULT,
            error_policy: FetchErrorPolicy::Fail,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResourceFetcher {
    policy: ResourcePolicy,
    #[cfg(not(target_arch = "wasm32"))]
    http_client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub(crate) struct FetchedResource {
    pub(crate) bytes: Vec<u8>,
    pub(crate) final_url: Url,
    /// MIME type supplied by the resource response, when that response has
    /// explicit type metadata.  This is required for destinations such as
    /// HTML linked stylesheets, whose processing depends on response MIME
    /// metadata rather than only the destination that initiated the fetch.
    pub(crate) content_type: Option<String>,
    pub(crate) access_control_allow_origin: Option<String>,
    pub(crate) access_control_allow_credentials: bool,
}

impl ResourceFetcher {
    pub(crate) fn new(policy: ResourcePolicy) -> crate::Result<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let redirect = if policy.follow_http_redirects {
                reqwest::redirect::Policy::limited(10)
            } else {
                reqwest::redirect::Policy::none()
            };
            let http_client = reqwest::Client::builder()
                .redirect(redirect)
                .timeout(policy.http_timeout.duration())
                .build()
                .map_err(|error| {
                    crate::Error::InvalidInput(format!(
                        "failed to create HTTP resource client: {error}"
                    ))
                })?;
            Ok(Self {
                policy,
                http_client,
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(Self { policy })
        }
    }

    pub(crate) fn allows_fetch_errors(&self) -> bool {
        self.policy.error_policy == FetchErrorPolicy::Allow
    }

    pub(crate) fn policy(&self) -> ResourcePolicy {
        self.policy
    }

    pub(crate) async fn fetch(&self, location: &Url) -> crate::Result<FetchedResource> {
        let location = fetch_url(location).ok_or_else(|| {
            crate::Error::InvalidInput(format!(
                "unsupported resource URL scheme {:?}: {location}",
                location.scheme()
            ))
        })?;

        #[cfg(target_arch = "wasm32")]
        {
            if location.scheme() == "data" {
                return fetch_data_url(&location);
            }
            return Err(crate::Error::InvalidInput(format!(
                "resource URL scheme {:?} is unavailable when targeting wasm32",
                location.scheme()
            )));
        }

        #[cfg(not(target_arch = "wasm32"))]
        match location.scheme() {
            "http" | "https" => {
                log::trace!("fetching HTTP resource {location}");
                let response = self
                    .http_client
                    .get(location.clone())
                    .send()
                    .await
                    .map_err(|error| {
                        crate::Error::InvalidInput(format!("failed to fetch {location}: {error}"))
                    })?;
                let final_url = response.url().clone();
                let status = response.status();
                if !status.is_success() {
                    return Err(crate::Error::InvalidInput(format!(
                        "HTTP fetch for {location} returned {status}"
                    )));
                }
                let access_control_allow_origin = response
                    .headers()
                    .get(reqwest::header::ACCESS_CONTROL_ALLOW_ORIGIN)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let access_control_allow_credentials = response
                    .headers()
                    .get(reqwest::header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                    .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"true"));
                let bytes =
                    response
                        .bytes()
                        .await
                        .map(|bytes| bytes.to_vec())
                        .map_err(|error| {
                            crate::Error::InvalidInput(format!(
                                "failed to read response body for {final_url}: {error}"
                            ))
                        })?;
                log::trace!(
                    "fetched HTTP resource {location} as {final_url} ({} byte(s))",
                    bytes.len()
                );
                Ok(FetchedResource {
                    bytes,
                    final_url,
                    content_type,
                    access_control_allow_origin,
                    access_control_allow_credentials,
                })
            }
            "file" => {
                let path = location.to_file_path().map_err(|_| {
                    crate::Error::InvalidInput(format!(
                        "could not convert file URL to path: {location}"
                    ))
                })?;
                log::trace!("reading filesystem resource {}", path.display());
                let bytes = tokio::fs::read(&path).await?;
                log::trace!(
                    "read filesystem resource {} ({} byte(s))",
                    path.display(),
                    bytes.len()
                );
                Ok(FetchedResource {
                    bytes,
                    final_url: location.clone(),
                    content_type: None,
                    access_control_allow_origin: None,
                    access_control_allow_credentials: false,
                })
            }
            "data" => fetch_data_url(&location),
            scheme => Err(crate::Error::InvalidInput(format!(
                "unsupported resource URL scheme {scheme:?}: {location}"
            ))),
        }
    }

    pub(crate) async fn read_to_string(&self, location: &Url) -> crate::Result<(String, Url)> {
        let fetched = self.fetch(location).await?;
        let source = String::from_utf8(fetched.bytes).map_err(|error| {
            crate::Error::InvalidInput(format!(
                "resource {} is not UTF-8: {error}",
                fetched.final_url
            ))
        })?;
        Ok((source, fetched.final_url))
    }
}

/// Fetch a `data:` URL without a network request.
///
/// Fetch defines `data:` URLs as resources with a decoded byte body and MIME
/// type metadata. The URL fragment is removed by [`fetch_url`] before this
/// function receives the URL, so it cannot become part of the resource body.
/// <https://fetch.spec.whatwg.org/#data-url-processor>
fn fetch_data_url(location: &Url) -> crate::Result<FetchedResource> {
    debug_assert_eq!(location.scheme(), "data");
    let data_url = data_url::DataUrl::process(location.as_str()).map_err(|error| {
        crate::Error::InvalidInput(format!("failed to parse data URL {location}: {error}"))
    })?;
    let content_type = data_url.mime_type().to_string();
    let (bytes, _) = data_url.decode_to_vec().map_err(|error| {
        crate::Error::InvalidInput(format!("failed to decode data URL {location}: {error}"))
    })?;
    log::trace!(
        "decoded data URL {location} as {content_type} ({} byte(s))",
        bytes.len()
    );
    Ok(FetchedResource {
        bytes,
        final_url: location.clone(),
        content_type: Some(content_type),
        access_control_allow_origin: None,
        access_control_allow_credentials: false,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct ResourceCache {
    bytes: HashMap<Url, Arc<Vec<u8>>>,
    cors_response_metadata: HashMap<Url, CorsResponseMetadata>,
    image_store: RefCell<DocumentImageStore>,
    svg_assets: RefCell<HashMap<ElementId, Option<SharedSvgAsset>>>,
    /// CSS presentation values cascaded onto descendants of inline SVG roots.
    ///
    /// Inline SVG is an atomic HTML layout box, but its descendants still
    /// participate in the document's CSS cascade.  The SVG adapter consumes
    /// these overrides while constructing its own SVG scene, without mutating
    /// the source DOM used by HTML selector matching.
    svg_presentation_overrides: RefCell<SvgPresentationOverrides>,
    /// Used iframe content-box viewport dimensions recorded during the parent
    /// measurement layout. They let nested browsing contexts lay out against
    /// their actual embedding viewport on the final pass.
    iframe_viewports: RefCell<HashMap<ElementId, (f32, f32)>>,
    image_assets: RefCell<HashMap<(Url, SvgImageContext), Option<ResourceImageAsset>>>,
    oriented_image_assets: RefCell<HashMap<(Url, SvgImageContext), Option<ResourceImageAsset>>>,
    data_image_assets: RefCell<HashMap<(String, SvgImageContext), Option<ResourceImageAsset>>>,
    oriented_data_image_assets:
        RefCell<HashMap<(String, SvgImageContext), Option<ResourceImageAsset>>>,
    placeholder_rgb: Rc<[u8]>,
}

#[derive(Debug, Clone, Default)]
struct CorsResponseMetadata {
    allow_origin: Option<String>,
    allow_credentials: bool,
}

impl Default for ResourceCache {
    fn default() -> Self {
        Self {
            bytes: HashMap::new(),
            cors_response_metadata: HashMap::new(),
            image_store: RefCell::new(DocumentImageStore::default()),
            svg_assets: RefCell::new(HashMap::new()),
            svg_presentation_overrides: RefCell::new(SvgPresentationOverrides::new()),
            iframe_viewports: RefCell::new(HashMap::new()),
            image_assets: RefCell::new(HashMap::new()),
            oriented_image_assets: RefCell::new(HashMap::new()),
            data_image_assets: RefCell::new(HashMap::new()),
            oriented_data_image_assets: RefCell::new(HashMap::new()),
            // Layout only needs intrinsic dimensions for stored images. This
            // tiny placeholder keeps legacy paint constructors allocation-free
            // until their raw-pixel parameters are removed.
            placeholder_rgb: Rc::from(vec![0, 0, 0].into_boxed_slice()),
        }
    }
}

/// A decoded raster resource or parsed vector SVG resource.
#[derive(Debug, Clone)]
pub(crate) enum ResourceImageAsset {
    Raster {
        image_id: ImageId,
        metadata: ImageMetadata,
    },
    Svg(SharedSvgAsset),
}

impl ResourceCache {
    pub(crate) fn record_iframe_viewport(&self, element: ElementId, width: f32, height: f32) {
        self.iframe_viewports
            .borrow_mut()
            .insert(element, (width.max(0.0), height.max(0.0)));
    }

    pub(crate) fn take_iframe_viewports(&self) -> HashMap<ElementId, (f32, f32)> {
        std::mem::take(&mut *self.iframe_viewports.borrow_mut())
    }

    /// Installs document-cascaded CSS presentation values for inline SVG descendants.
    ///
    /// This is called once before layout starts, before any inline SVG asset
    /// can be cached. See CSS Transforms Level 1, §7.3, for CSS `transform` on
    /// SVG elements and SVG 2, §6.6, for presentation-attribute precedence.
    pub(crate) fn set_inline_svg_presentation_overrides(
        &self,
        overrides: SvgPresentationOverrides,
    ) {
        debug_assert!(self.svg_assets.borrow().is_empty());
        *self.svg_presentation_overrides.borrow_mut() = overrides;
    }

    pub(crate) async fn preload(
        fetcher: &ResourceFetcher,
        urls: impl IntoIterator<Item = Url>,
    ) -> crate::Result<Self> {
        const MAX_SVG_RESOURCE_DEPTH: usize = 8;
        const MAX_NESTED_SVG_RESOURCES: usize = 256;
        const MAX_SVG_RESOURCE_BYTES: usize = 8 * 1024 * 1024;
        const MAX_NESTED_SVG_BYTES: usize = 32 * 1024 * 1024;
        let mut cache = Self::default();
        let mut seen = HashSet::new();
        let mut pending = urls
            .into_iter()
            .map(|url| (url, 0_usize, false))
            .collect::<VecDeque<_>>();
        let mut nested_count = 0_usize;
        let mut nested_bytes = 0_usize;
        while let Some((url, depth, nested)) = pending.pop_front() {
            let Some(fetch_url) = fetch_url(&url) else {
                continue;
            };
            if !seen.insert(fetch_url.clone()) {
                continue;
            }
            if nested && nested_count >= MAX_NESTED_SVG_RESOURCES {
                log::debug!("SVG nested resource count limit exceeded for {fetch_url}");
                continue;
            }
            match fetcher.fetch(&fetch_url).await {
                Ok(fetched) => {
                    let cors_metadata = CorsResponseMetadata {
                        allow_origin: fetched.access_control_allow_origin,
                        allow_credentials: fetched.access_control_allow_credentials,
                    };
                    let bytes = fetched.bytes;
                    if nested
                        && (bytes.len() > MAX_SVG_RESOURCE_BYTES
                            || nested_bytes.saturating_add(bytes.len()) > MAX_NESTED_SVG_BYTES)
                    {
                        log::debug!("SVG nested resource limit exceeded for {fetch_url}");
                        continue;
                    }
                    if nested {
                        nested_count += 1;
                        nested_bytes += bytes.len();
                    }
                    let dependencies = (depth < MAX_SVG_RESOURCE_DEPTH
                        && nested_count < MAX_NESTED_SVG_RESOURCES)
                        .then(|| svg_dependency_urls(&bytes, &fetched.final_url))
                        .flatten()
                        .unwrap_or_default();
                    let bytes = Arc::new(bytes);
                    cache.bytes.insert(fetch_url.clone(), Arc::clone(&bytes));
                    cache.bytes.insert(fetched.final_url.clone(), bytes);
                    cache
                        .cors_response_metadata
                        .insert(fetch_url.clone(), cors_metadata.clone());
                    cache
                        .cors_response_metadata
                        .insert(fetched.final_url.clone(), cors_metadata);
                    for dependency in dependencies {
                        pending.push_back((dependency, depth + 1, true));
                    }
                }
                Err(error) => {
                    if !fetcher.allows_fetch_errors() {
                        return Err(error);
                    }
                    log::debug!("failed to preload resource {url}: {error}");
                }
            }
        }
        Ok(cache)
    }

    /// Verify the policy modifiers attached to a CSS image source before its
    /// cached bytes are decoded. Cross-origin requests must be explicitly
    /// authorized by the response, and integrity metadata must match the
    /// selected resource bytes.
    /// <https://drafts.csswg.org/css-values-5/#request-url-modifiers>
    pub(crate) fn allows_css_image_request(
        &self,
        url: &Url,
        document_url: Option<&Url>,
        modifiers: &crate::css::RequestUrlModifiers,
    ) -> bool {
        let Some(url) = fetch_url(url) else {
            return false;
        };
        let Some(bytes) = self.bytes.get(&url) else {
            return false;
        };
        if let Some(integrity) = &modifiers.integrity
            && !integrity_matches(bytes, integrity)
        {
            return false;
        }
        let Some(mode) = modifiers.cross_origin else {
            return true;
        };
        let Some(document_url) = document_url else {
            return false;
        };
        if urls_have_same_origin(&url, document_url) {
            return true;
        }
        let Some(metadata) = self.cors_response_metadata.get(&url) else {
            return false;
        };
        let document_origin = serialized_origin(document_url);
        let allows_origin = metadata
            .allow_origin
            .as_deref()
            .is_some_and(|allow_origin| allow_origin == "*" || allow_origin == document_origin);
        allows_origin
            && (mode != crate::css::CrossOriginRequestMode::UseCredentials
                || metadata.allow_credentials
                    && metadata.allow_origin.as_deref() == Some(document_origin.as_str()))
    }

    pub(crate) fn image_asset_url_with_orientation(
        &self,
        url: &Url,
        apply_orientation: bool,
        image_context: SvgImageContext,
    ) -> Option<ResourceImageAsset> {
        let url = fetch_url(url)?;
        let assets = if apply_orientation {
            &self.oriented_image_assets
        } else {
            &self.image_assets
        };
        let cache_key = (url.clone(), image_context);
        if let Some(asset) = assets.borrow().get(&cache_key) {
            return asset.clone();
        }
        let asset = self.bytes.get(&url).cloned().and_then(|bytes| {
            parse_svg_bytes_with_image_context(&bytes, image_context)
                .map(|asset| ResourceImageAsset::Svg(Rc::new(asset)))
                .ok()
                .or_else(|| {
                    self.image_store
                        .borrow_mut()
                        .resolve_url_with_orientation(
                            url.clone(),
                            Rc::from(bytes.as_slice()),
                            apply_orientation,
                        )
                        .map(|(image_id, metadata)| ResourceImageAsset::Raster {
                            image_id,
                            metadata,
                        })
                })
        });
        assets.borrow_mut().insert(cache_key, asset.clone());
        asset
    }

    pub(crate) fn data_image_asset_with_orientation(
        &self,
        source: &str,
        apply_orientation: bool,
        image_context: SvgImageContext,
    ) -> Option<ResourceImageAsset> {
        let assets = if apply_orientation {
            &self.oriented_data_image_assets
        } else {
            &self.data_image_assets
        };
        let cache_key = (source.to_owned(), image_context);
        if let Some(asset) = assets.borrow().get(&cache_key) {
            return asset.clone();
        }
        let asset = data_url::DataUrl::process(source)
            .ok()
            .and_then(|url| {
                let is_svg =
                    url.mime_type().type_ == "image" && url.mime_type().subtype == "svg+xml";
                url.decode_to_vec().ok().map(|(bytes, _)| (is_svg, bytes))
            })
            .and_then(|(is_svg, bytes)| {
                let bytes: Rc<[u8]> = Rc::from(bytes);
                if is_svg {
                    parse_svg_bytes_with_image_context(&bytes, image_context)
                        .ok()
                        .map(|asset| ResourceImageAsset::Svg(Rc::new(asset)))
                } else {
                    self.image_store
                        .borrow_mut()
                        .resolve_data_url_with_orientation(source, bytes, apply_orientation)
                        .map(|(image_id, metadata)| ResourceImageAsset::Raster {
                            image_id,
                            metadata,
                        })
                }
            });
        assets.borrow_mut().insert(cache_key, asset.clone());
        asset
    }

    pub(crate) fn image_placeholder_rgb(&self) -> Rc<[u8]> {
        Rc::clone(&self.placeholder_rgb)
    }

    /// Parse an inline SVG once for all layout and paint consumers.
    pub(crate) fn inline_svg_asset(&self, element: &Element) -> Option<SharedSvgAsset> {
        if let Some(asset) = self.svg_assets.borrow().get(&element.id) {
            return asset.clone();
        }
        let overrides = self.svg_presentation_overrides.borrow();
        let asset = match parse_inline_svg_with_presentation_overrides(element, &overrides) {
            Ok(asset) => Some(Rc::new(asset)),
            Err(error) => {
                log::debug!("failed to parse inline SVG: {error}");
                None
            }
        };
        self.svg_assets
            .borrow_mut()
            .insert(element.id, asset.clone());
        asset
    }

    pub(crate) fn take_image_store(&self) -> DocumentImageStore {
        std::mem::take(&mut *self.image_store.borrow_mut())
    }

    pub(crate) fn register_generated_image_recipe(
        &self,
        image: crate::image_store::GeneratedRasterImage,
    ) -> ImageId {
        self.image_store.borrow_mut().register_generated(image)
    }

    /// Borrow one decoded document image for layout-time consumers such as CSS
    /// Shapes alpha contours. The raster remains owned by the image store and
    /// is released immediately after `consume` returns.
    pub(crate) fn with_rasterized_image<T>(
        &self,
        image: ImageId,
        consume: impl FnOnce(crate::image_store::RasterImage) -> T,
    ) -> Option<T> {
        self.image_store.borrow().with_rasterized(image, consume)
    }
}

fn svg_dependency_urls(bytes: &[u8], parent: &Url) -> Option<Vec<Url>> {
    let document = usvg::roxmltree::Document::parse(std::str::from_utf8(bytes).ok()?).ok()?;
    let root = document.root_element();
    (root.tag_name().name() == "svg").then_some(())?;
    Some(
        root.descendants()
            .filter(|node| node.is_element() && matches!(node.tag_name().name(), "image" | "use"))
            .filter_map(|node| {
                node.attribute("href")
                    .or_else(|| node.attribute(("http://www.w3.org/1999/xlink", "href")))
            })
            .filter_map(|href| resolve_fetchable_url(href, Some(parent), Some(parent)))
            .filter(|url| same_svg_resource_origin(parent, url))
            .collect(),
    )
}

fn same_svg_resource_origin(parent: &Url, child: &Url) -> bool {
    match parent.scheme() {
        "file" => child.scheme() == "file",
        "http" | "https" => parent.origin() == child.origin(),
        _ => false,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn read_to_string(location: &Url) -> crate::Result<String> {
    let fetcher = ResourceFetcher::new(ResourcePolicy::default())?;
    fetcher
        .read_to_string(location)
        .await
        .map(|(source, _)| source)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn file_url_from_path(path: &Path) -> crate::Result<Url> {
    let path = absolute_path(path)?;
    Url::from_file_path(&path).map_err(|_| {
        crate::Error::InvalidInput(format!(
            "could not convert path to file URL: {}",
            path.display()
        ))
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn directory_url_from_path(path: &Path) -> crate::Result<Url> {
    let path = absolute_path(path)?;
    Url::from_directory_path(&path).map_err(|_| {
        crate::Error::InvalidInput(format!(
            "could not convert directory path to file URL: {}",
            path.display()
        ))
    })
}

pub(crate) fn resolve_url(
    value: &str,
    base_url: Option<&Url>,
    root_url: Option<&Url>,
) -> Option<Url> {
    let resolved = Url::parse(value).ok().or_else(|| {
        if value.starts_with('/') {
            root_url
                .and_then(|root_url| root_url.join(value.trim_start_matches('/')).ok())
                .or_else(|| base_url.and_then(|base_url| base_url.join(value).ok()))
        } else {
            base_url.and_then(|base_url| base_url.join(value).ok())
        }
    });
    if let Some(url) = &resolved {
        log::trace!("resolved resource URL value={value:?} to {url}");
    } else {
        log::trace!("could not resolve resource URL value={value:?}");
    }
    resolved
}

pub(crate) fn resolve_fetchable_url(
    value: &str,
    base_url: Option<&Url>,
    root_url: Option<&Url>,
) -> Option<Url> {
    (!value.starts_with('#'))
        .then(|| resolve_url(value, base_url, root_url))
        .flatten()
        .and_then(|url| fetch_url(&url))
}

pub(crate) fn css_resource_urls(
    source: &str,
    base_url: Option<&Url>,
    root_url: Option<&Url>,
) -> Vec<Url> {
    css_urls(source)
        .into_iter()
        .filter_map(|url| resolve_fetchable_url(&url, base_url, root_url))
        .collect()
}

pub(crate) fn fetch_url(url: &Url) -> Option<Url> {
    matches!(url.scheme(), "data" | "file" | "http" | "https").then(|| {
        let mut url = url.clone();
        url.set_fragment(None);
        url
    })
}

fn urls_have_same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn serialized_origin(url: &Url) -> String {
    format!(
        "{}://{}{}",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        url.port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default(),
    )
}

fn integrity_matches(bytes: &[u8], metadata: &str) -> bool {
    use base64::Engine;

    let digest = Sha256::digest(bytes);
    metadata.split_ascii_whitespace().any(|candidate| {
        let Some(encoded) = candidate.strip_prefix("sha256-") else {
            return false;
        };
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .is_ok_and(|expected| expected.as_slice() == digest.as_slice())
    })
}

pub(crate) fn origin_url(url: &Url) -> Option<Url> {
    let origin = url.origin();
    (!origin.is_tuple()).then_some(())?;
    Url::parse(&origin.ascii_serialization()).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn absolute_path(path: &Path) -> crate::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn css_urls(source: &str) -> Vec<String> {
    crate::css::component_values::css_image_candidate_urls(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_policy_defaults_to_strict_redirect_following() {
        let policy = ResourcePolicy::default();

        assert!(policy.follow_http_redirects);
        assert_eq!(policy.http_timeout, HttpRequestTimeout::DEFAULT);
        assert_eq!(
            policy.http_timeout.duration(),
            std::time::Duration::from_secs(10)
        );
        assert_eq!(policy.error_policy, FetchErrorPolicy::Fail);
        assert_eq!(FetchErrorPolicy::default(), FetchErrorPolicy::Fail);
    }

    #[test]
    fn http_request_timeout_rejects_zero_duration() {
        assert_eq!(
            HttpRequestTimeout::try_from(std::time::Duration::ZERO),
            Err(InvalidHttpRequestTimeout)
        );
    }

    #[test]
    fn resource_fetcher_preserves_http_request_timeout() {
        let fetcher = ResourceFetcher::new(ResourcePolicy {
            http_timeout: HttpRequestTimeout::try_from(std::time::Duration::from_millis(20))
                .unwrap(),
            ..ResourcePolicy::default()
        })
        .unwrap();

        assert_eq!(
            fetcher.policy().http_timeout.duration(),
            std::time::Duration::from_millis(20)
        );
    }

    #[tokio::test]
    async fn strict_and_allow_policies_diverge_for_missing_local_resources() {
        let missing = file_url_from_path(std::path::Path::new(
            "tests/fixtures/resource-policy-missing.png",
        ))
        .unwrap();
        let strict_fetcher = ResourceFetcher::new(ResourcePolicy::default()).unwrap();
        assert!(
            ResourceCache::preload(&strict_fetcher, [missing.clone()])
                .await
                .is_err()
        );

        let allowing_fetcher = ResourceFetcher::new(ResourcePolicy {
            error_policy: FetchErrorPolicy::Allow,
            ..ResourcePolicy::default()
        })
        .unwrap();

        let cache = ResourceCache::preload(&allowing_fetcher, [missing])
            .await
            .unwrap();

        assert!(cache.bytes.is_empty());
    }

    #[tokio::test]
    async fn fetches_data_url_bytes_and_mime_type_without_its_fragment() {
        let fetcher = ResourceFetcher::new(ResourcePolicy::default()).unwrap();
        let url = Url::parse("data:text/plain;base64,SGVsbG8=#section").unwrap();

        let fetched = fetcher.fetch(&url).await.unwrap();

        assert_eq!(fetched.bytes, b"Hello");
        assert_eq!(fetched.content_type.as_deref(), Some("text/plain"));
        assert_eq!(
            fetched.final_url.as_str(),
            "data:text/plain;base64,SGVsbG8="
        );
    }

    #[tokio::test]
    async fn malformed_data_url_is_a_fetch_failure() {
        let fetcher = ResourceFetcher::new(ResourcePolicy::default()).unwrap();
        let url = Url::parse("data:text/css").unwrap();

        assert!(fetcher.fetch(&url).await.is_err());
    }

    #[test]
    fn discovers_url_with_request_modifiers_for_preloading() {
        let source = r#".test { background-image: url("http://www.example.test/image.png?pipe=header(Access-Control-Allow-Origin,*)" cross-origin(anonymous)); }"#;

        assert_eq!(
            css_urls(source),
            vec!["http://www.example.test/image.png?pipe=header(Access-Control-Allow-Origin,*)"]
        );
    }

    #[test]
    fn discovers_bare_image_set_sources_without_preloading_type_descriptors() {
        let source = r#".test { background-image: image-set(
            linear-gradient(red, blue) 1x,
            "first.png" 2x type("image/png"),
            "escaped\ name.png" 3x,
            url("fourth.png") 4x
        ); }"#;

        assert_eq!(
            css_urls(source),
            vec![
                "first.png".to_string(),
                "escaped name.png".to_string(),
                "fourth.png".to_string(),
            ]
        );
    }

    #[test]
    fn discovers_escaped_and_nested_image_set_candidates_without_type_strings() {
        let source = r#".test { background-image: image-set(
            "escaped\2epng" 1x type("image/not-a-url"),
            image(url("nested.png")) 2x,
            url("quoted\20url.png") 3x
        ); }"#;

        assert_eq!(
            css_urls(source),
            vec![
                "escaped.png".to_string(),
                "nested.png".to_string(),
                "quoted url.png".to_string(),
            ]
        );
    }

    #[test]
    fn svg_dependencies_allow_only_same_origin_image_and_use_urls() {
        let parent = Url::parse("https://example.test/assets/root.svg").unwrap();
        let bytes = br#"<svg xmlns="http://www.w3.org/2000/svg"><image href="nested.png"/><use href="https://example.test/shared.svg#icon"/><image href="https://elsewhere.test/blocked.png"/></svg>"#;

        let dependencies = svg_dependency_urls(bytes, &parent).unwrap();

        assert_eq!(dependencies.len(), 2);
        assert!(
            dependencies
                .iter()
                .all(|url| url.origin() == parent.origin())
        );
        assert!(dependencies.iter().all(|url| url.fragment().is_none()));
    }

    #[test]
    fn svg_image_cache_keeps_color_scheme_variants_separate() {
        let mut cache = ResourceCache::default();
        let url = Url::parse("https://example.test/image.svg").unwrap();
        cache.bytes.insert(
            url.clone(),
            Arc::new(
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><style>:root { color: blue } @media (prefers-color-scheme: dark) { :root { color: purple } }</style><rect width="32" height="32" fill="currentColor"/></svg>"#
                    .to_vec(),
            ),
        );
        let light = cache
            .image_asset_url_with_orientation(
                &url,
                false,
                SvgImageContext::from_used_color_scheme(crate::css::UsedColorScheme::Light),
            )
            .unwrap();
        let dark = cache
            .image_asset_url_with_orientation(
                &url,
                false,
                SvgImageContext::from_used_color_scheme(crate::css::UsedColorScheme::Dark),
            )
            .unwrap();
        let ResourceImageAsset::Svg(light) = light else {
            panic!("expected SVG image");
        };
        let ResourceImageAsset::Svg(dark) = dark else {
            panic!("expected SVG image");
        };

        assert_eq!(
            light.opaque_viewport_fill(),
            Some(crate::CssColor::new(0, 0, 255))
        );
        assert_eq!(
            dark.opaque_viewport_fill(),
            Some(crate::CssColor::new(128, 0, 128))
        );
        assert!(!Rc::ptr_eq(&light, &dark));
    }
}
