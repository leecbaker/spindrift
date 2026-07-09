use crate::dom::{Element, ElementId};
use crate::image_store::{DocumentImageStore, ImageId, ImageMetadata};
use crate::svg::{SharedSvgAsset, parse_inline_svg_with_transform_overrides, parse_svg_bytes};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

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
/// ```
/// let policy = quire::FetchErrorPolicy::Allow;
/// assert_eq!(policy, quire::FetchErrorPolicy::Allow);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FetchErrorPolicy {
    /// Abort rendering when a required resource cannot be fetched.
    #[default]
    Fail,
    /// Continue rendering when an optional resource cannot be fetched.
    Allow,
}

/// Policy used for filesystem and HTTP(S) document resources.
///
/// HTTP redirects are followed by default. Primary resources are fatal by
/// default, unlike WeasyPrint's permissive URL fetcher; HTML image/background
/// preloads always recover as unavailable visual assets.
///
/// ```
/// let policy = quire::ResourcePolicy {
///     follow_http_redirects: false,
///     error_policy: quire::FetchErrorPolicy::Fail,
/// };
/// assert!(!policy.follow_http_redirects);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourcePolicy {
    /// Whether HTTP(S) fetches follow redirects.
    pub follow_http_redirects: bool,
    /// How fetch failures affect rendering.
    pub error_policy: FetchErrorPolicy,
}

impl Default for ResourcePolicy {
    fn default() -> Self {
        Self {
            follow_http_redirects: true,
            error_policy: FetchErrorPolicy::Fail,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResourceFetcher {
    policy: ResourcePolicy,
    http_client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub(crate) struct FetchedResource {
    pub(crate) bytes: Vec<u8>,
    pub(crate) final_url: Url,
    pub(crate) access_control_allow_origin: Option<String>,
    pub(crate) access_control_allow_credentials: bool,
}

impl ResourceFetcher {
    pub(crate) fn new(policy: ResourcePolicy) -> crate::Result<Self> {
        let redirect = if policy.follow_http_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        };
        let http_client = reqwest::Client::builder()
            .redirect(redirect)
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

    pub(crate) fn allows_fetch_errors(&self) -> bool {
        self.policy.error_policy == FetchErrorPolicy::Allow
    }

    pub(crate) fn policy(&self) -> ResourcePolicy {
        self.policy
    }

    pub(crate) async fn fetch(&self, location: &Url) -> crate::Result<FetchedResource> {
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
                    access_control_allow_origin: None,
                    access_control_allow_credentials: false,
                })
            }
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

#[derive(Debug, Clone)]
pub(crate) struct ResourceCache {
    bytes: HashMap<Url, Arc<Vec<u8>>>,
    cors_response_metadata: HashMap<Url, CorsResponseMetadata>,
    image_store: RefCell<DocumentImageStore>,
    svg_assets: RefCell<HashMap<ElementId, Option<SharedSvgAsset>>>,
    /// CSS transforms cascaded onto descendants of inline SVG roots.
    ///
    /// Inline SVG is an atomic HTML layout box, but its descendants still
    /// participate in the document's CSS cascade.  The SVG adapter consumes
    /// these overrides while constructing its own SVG scene, without mutating
    /// the source DOM used by HTML selector matching.
    svg_transform_overrides: RefCell<HashMap<ElementId, String>>,
    /// Used iframe content-box viewport dimensions recorded during the parent
    /// measurement layout. They let nested browsing contexts lay out against
    /// their actual embedding viewport on the final pass.
    iframe_viewports: RefCell<HashMap<ElementId, (f32, f32)>>,
    image_assets: RefCell<HashMap<Url, Option<ResourceImageAsset>>>,
    oriented_image_assets: RefCell<HashMap<Url, Option<ResourceImageAsset>>>,
    data_image_assets: RefCell<HashMap<String, Option<ResourceImageAsset>>>,
    oriented_data_image_assets: RefCell<HashMap<String, Option<ResourceImageAsset>>>,
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
            svg_transform_overrides: RefCell::new(HashMap::new()),
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

    /// Installs the document-cascaded CSS transforms for inline SVG descendants.
    ///
    /// This is called once before layout starts, before any inline SVG asset
    /// can be cached. See CSS Transforms Level 1, §7.3, for CSS `transform` on
    /// SVG elements and SVG 2, §6.6, for presentation-attribute precedence.
    pub(crate) fn set_inline_svg_transform_overrides(&self, overrides: HashMap<ElementId, String>) {
        debug_assert!(self.svg_assets.borrow().is_empty());
        *self.svg_transform_overrides.borrow_mut() = overrides;
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
    ) -> Option<ResourceImageAsset> {
        let url = fetch_url(url)?;
        let assets = if apply_orientation {
            &self.oriented_image_assets
        } else {
            &self.image_assets
        };
        if let Some(asset) = assets.borrow().get(&url) {
            return asset.clone();
        }
        let asset = self.bytes.get(&url).cloned().and_then(|bytes| {
            parse_svg_bytes(&bytes)
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
        assets.borrow_mut().insert(url, asset.clone());
        asset
    }

    pub(crate) fn data_image_asset_with_orientation(
        &self,
        source: &str,
        apply_orientation: bool,
    ) -> Option<ResourceImageAsset> {
        let assets = if apply_orientation {
            &self.oriented_data_image_assets
        } else {
            &self.data_image_assets
        };
        if let Some(asset) = assets.borrow().get(source) {
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
                    parse_svg_bytes(&bytes)
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
        assets.borrow_mut().insert(source.to_owned(), asset.clone());
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
        let overrides = self.svg_transform_overrides.borrow();
        let asset = match parse_inline_svg_with_transform_overrides(element, &overrides) {
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

pub(crate) async fn read_to_string(location: &Url) -> crate::Result<String> {
    let fetcher = ResourceFetcher::new(ResourcePolicy::default())?;
    fetcher
        .read_to_string(location)
        .await
        .map(|(source, _)| source)
}

pub(crate) fn file_url_from_path(path: &Path) -> crate::Result<Url> {
    let path = absolute_path(path)?;
    Url::from_file_path(&path).map_err(|_| {
        crate::Error::InvalidInput(format!(
            "could not convert path to file URL: {}",
            path.display()
        ))
    })
}

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
    matches!(url.scheme(), "file" | "http" | "https").then(|| {
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

fn absolute_path(path: &Path) -> crate::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn css_urls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut urls = Vec::new();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        if !bytes[index..]
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"url("))
        {
            index += 1;
            continue;
        }
        index += 4;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let Some(url) = (if matches!(bytes.get(index), Some(b'"' | b'\'')) {
            let quote = bytes[index];
            index += 1;
            let start = index;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            let value = source.get(start..index).map(str::to_string);
            if bytes.get(index) == Some(&quote) {
                index += 1;
            }
            value
        } else {
            let start = index;
            while index < bytes.len() && bytes[index] != b')' {
                index += 1;
            }
            source
                .get(start..index)
                .map(|value| value.trim().to_string())
        }) else {
            break;
        };
        while bytes.get(index).is_some_and(|byte| *byte != b')') {
            index += 1;
        }
        if bytes.get(index) == Some(&b')') {
            index += 1;
        }
        if !url.is_empty() {
            urls.push(url);
        }
    }
    // CSS Images permits a bare string as an `image-set()` image source. It
    // resolves exactly like a `url()` source, but is not discoverable by the
    // ordinary URL-token scan above. Preload quoted candidates so layout's
    // immutable resource cache can resolve the selected image later.
    // <https://drafts.csswg.org/css-images-4/#image-set-notation>
    let lower = source.to_ascii_lowercase();
    let mut search = 0;
    while let Some(found) = lower[search..].find("image-set(") {
        let mut cursor = search + found + "image-set(".len();
        let end = source[cursor..]
            .find(')')
            .map(|offset| cursor + offset)
            .unwrap_or(source.len());
        while cursor < end {
            if matches!(bytes.get(cursor), Some(b'\'' | b'\"')) {
                let quote = bytes[cursor];
                cursor += 1;
                let start = cursor;
                while cursor < end && bytes[cursor] != quote {
                    cursor += 1;
                }
                if let Some(value) = source.get(start..cursor)
                    && !value.is_empty()
                {
                    urls.push(value.to_string());
                }
            }
            cursor += 1;
        }
        search = end.saturating_add(1);
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_policy_defaults_to_strict_redirect_following() {
        let policy = ResourcePolicy::default();

        assert!(policy.follow_http_redirects);
        assert_eq!(policy.error_policy, FetchErrorPolicy::Fail);
        assert_eq!(FetchErrorPolicy::default(), FetchErrorPolicy::Fail);
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

    #[test]
    fn discovers_url_with_request_modifiers_for_preloading() {
        let source = r#".test { background-image: url("http://www.example.test/image.png?pipe=header(Access-Control-Allow-Origin,*)" cross-origin(anonymous)); }"#;

        assert_eq!(
            css_urls(source),
            vec!["http://www.example.test/image.png?pipe=header(Access-Control-Allow-Origin,*)"]
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
}
