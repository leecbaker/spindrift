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

use crate::dom::{self, DocumentSyntax, Element, ElementId, Node, NodeKind};
use crate::image_store::{DocumentImageStore, ImageId, ImageMetadata, RasterOrientationPolicy};
use crate::layout::IframeEmbeddingContext;
use crate::svg::{
    SharedSvgAsset, SvgImageContext, SvgPresentationOverrides,
    parse_inline_svg_with_presentation_overrides, parse_svg_bytes_with_image_context,
};

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
    resource_metadata: HashMap<Url, CachedResourceMetadata>,
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
    external_svg_uses: ExternalSvgUseResolver,
    /// Used iframe content-box viewport dimensions recorded during the parent
    /// measurement layout. They let nested browsing contexts lay out against
    /// their actual embedding viewport on the final pass.
    iframe_viewports: RefCell<HashMap<ElementId, IframeEmbeddingContext>>,
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

#[derive(Debug, Clone, Default)]
struct CachedResourceMetadata {
    final_url: Option<Url>,
    content_type: Option<String>,
}

/// The fetch visibility of an image response as it affects `image-orientation`.
///
/// An opaque cross-origin image must not reveal whether it carries orientation
/// metadata through `image-orientation: none`; origin-clean image responses
/// may select their encoded, unrotated representation.
/// <https://github.com/w3c/csswg-drafts/issues/5165>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFetchTaint {
    OriginClean,
    Opaque,
}

impl Default for ResourceCache {
    fn default() -> Self {
        Self {
            bytes: HashMap::new(),
            resource_metadata: HashMap::new(),
            cors_response_metadata: HashMap::new(),
            image_store: RefCell::new(DocumentImageStore::default()),
            svg_assets: RefCell::new(HashMap::new()),
            svg_presentation_overrides: RefCell::new(SvgPresentationOverrides::new()),
            external_svg_uses: ExternalSvgUseResolver::default(),
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

/// A statically preloaded external SVG document available to an inline
/// `<use>` expansion.
///
/// SVG 2 treats `use` with an external `href` as a structurally external
/// reference. The graph is deliberately resolved before `usvg` normalizes the
/// inline SVG, since `usvg` only resolves fragment identifiers in one XML
/// document. <https://www.w3.org/TR/SVG2/struct.html#UseElement>
#[derive(Debug, Clone)]
struct ExternalSvgDocument {
    source: Node,
    url: Url,
}

/// Read-only catalog of same-origin external SVG documents used by inline
/// SVG `<use>` elements.
///
/// It contains only bytes fetched during Quire's visual-resource preload; SVG
/// parsing never performs I/O. <https://www.w3.org/TR/SVG2/linking.html#processingURL>
#[derive(Debug, Clone, Default)]
pub(crate) struct ExternalSvgUseResolver {
    base_url: Option<Url>,
    root_url: Option<Url>,
    documents: HashMap<Url, ExternalSvgDocument>,
}

impl ExternalSvgUseResolver {
    /// Expand a serialized inline SVG into one self-contained XML document.
    /// Any failed import stays as an unresolved `<use>`, which `usvg` omits.
    pub(crate) fn expand_inline_svg(&self, source: String) -> String {
        if self.documents.is_empty() {
            return source;
        }
        let Ok(mut document) = dom::parse_with_syntax(&source, DocumentSyntax::Xml) else {
            return source;
        };
        let Some(root) = first_svg_element_mut(&mut document) else {
            return source;
        };
        let Some(referring_url) = self.base_url.as_ref().or(self.root_url.as_ref()) else {
            return source;
        };
        let mut state = SvgExternalImportState::default();
        self.rewrite_external_uses(root, referring_url, &mut state);
        if !state.imports.is_empty() {
            let mut defs = svg_element("defs");
            defs.children = state.imports;
            root.children.insert(
                0,
                Node {
                    kind: NodeKind::Element(defs),
                },
            );
        }
        crate::svg::serialize_inline_svg(root)
    }

    fn rewrite_external_uses(
        &self,
        node: &mut Element,
        referring_url: &Url,
        state: &mut SvgExternalImportState,
    ) {
        if node.namespace_url == SVG_NAMESPACE
            && node.tag == "use"
            && let Some(href) = svg_href(node).map(str::to_owned)
            && let Some((document_url, fragment)) = self.external_href(&href, referring_url)
            && let Some(local_id) = self.import_target(&document_url, &fragment, state)
        {
            set_svg_href(node, &format!("#{local_id}"));
        }
        for child in &mut node.children {
            if let NodeKind::Element(child) = &mut child.kind {
                self.rewrite_external_uses(child, referring_url, state);
            }
        }
    }

    fn import_target(
        &self,
        document_url: &Url,
        fragment: &str,
        state: &mut SvgExternalImportState,
    ) -> Option<String> {
        let document = self.documents.get(document_url)?;
        let source = svg_fragment_containing_target(&document.source, fragment)?;
        let prefix = if let Some(prefix) = state.prefixes.get(document_url) {
            prefix.clone()
        } else {
            let prefix = format!("quire-external-{}-", state.prefixes.len());
            state.prefixes.insert(document_url.clone(), prefix.clone());
            let mut source = source;
            namespace_svg_ids(&mut source, &prefix);
            if let NodeKind::Element(source) = &mut source.kind {
                self.rewrite_external_uses(source, &document.url, state);
            }
            state.imports.push(source);
            prefix
        };
        let target = format!("{prefix}{fragment}");
        imported_svg_id_exists(state, &target).then_some(target)
    }

    fn external_href(&self, href: &str, referring_url: &Url) -> Option<(Url, String)> {
        if href.starts_with('#') {
            return None;
        }
        let resolved = resolve_url(href, Some(referring_url), Some(referring_url))?;
        let fragment = resolved.fragment()?.to_owned();
        (!fragment.is_empty()).then_some(())?;
        let document_url = fetch_url(&resolved)?;
        same_svg_resource_origin(referring_url, &document_url).then_some(())?;
        self.documents
            .contains_key(&document_url)
            .then_some((document_url, fragment))
    }
}

#[derive(Default)]
struct SvgExternalImportState {
    prefixes: HashMap<Url, String>,
    imports: Vec<Node>,
}

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const XLINK_NAMESPACE: &str = "http://www.w3.org/1999/xlink";

fn first_svg_element_mut(node: &mut Node) -> Option<&mut Element> {
    let NodeKind::Element(element) = &mut node.kind else {
        return None;
    };
    if element.namespace_url == SVG_NAMESPACE && element.tag == "svg" {
        return Some(element);
    }
    element.children.iter_mut().find_map(first_svg_element_mut)
}

fn svg_fragment_containing_target(node: &Node, target: &str) -> Option<Node> {
    fn visit(node: &Node, target: &str, owner: Option<&Element>) -> Option<Node> {
        let NodeKind::Element(element) = &node.kind else {
            return None;
        };
        let owner = (element.namespace_url == SVG_NAMESPACE && element.tag == "svg")
            .then_some(element)
            .or(owner);
        if element.attrs.get("id").is_some_and(|id| id == target)
            && element.namespace_url == SVG_NAMESPACE
        {
            return Some(Node {
                kind: NodeKind::Element(
                    owner
                        .map(svg_owner_import_defs)
                        .unwrap_or_else(|| element.clone()),
                ),
            });
        }
        element
            .children
            .iter()
            .find_map(|child| visit(child, target, owner))
    }
    visit(node, target, None)
}

/// Import an owning SVG's local definition tree into the host definitions.
/// This retains sibling paint servers and SVG-local styles without wrapping a
/// symbol in a nested viewport, which the SVG backend does not instantiate.
fn svg_owner_import_defs(owner: &Element) -> Element {
    let mut defs = svg_element("defs");
    defs.children = owner.children.clone();
    defs
}

fn svg_element(tag: &str) -> Element {
    let mut node = Node::element(tag);
    let NodeKind::Element(element) = &mut node.kind else {
        unreachable!("new node is an element");
    };
    element.namespace_url = SVG_NAMESPACE.to_owned();
    element.document_syntax = DocumentSyntax::Xml;
    element.clone()
}

fn svg_href(element: &Element) -> Option<&str> {
    element.attrs.get("href").map(String::as_str).or_else(|| {
        element
            .namespace_attrs
            .iter()
            .find(|attribute| {
                attribute.namespace_url == XLINK_NAMESPACE && attribute.local_name == "href"
            })
            .map(|attribute| attribute.value.as_str())
    })
}

fn set_svg_href(element: &mut Element, value: &str) {
    element.attrs.insert("href".to_owned(), value.to_owned());
    for attribute in &mut element.namespace_attrs {
        if attribute.local_name == "href" {
            attribute.value = value.to_owned();
        }
    }
}

fn namespace_svg_ids(node: &mut Node, prefix: &str) {
    let mut identifiers = HashMap::new();
    collect_svg_ids(node, prefix, &mut identifiers);
    rewrite_svg_identifiers(node, &identifiers);
}

fn collect_svg_ids(node: &Node, prefix: &str, identifiers: &mut HashMap<String, String>) {
    let NodeKind::Element(element) = &node.kind else {
        return;
    };
    if let Some(id) = element.attrs.get("id") {
        identifiers.insert(id.clone(), format!("{prefix}{id}"));
    }
    for child in &element.children {
        collect_svg_ids(child, prefix, identifiers);
    }
}

fn rewrite_svg_identifiers(node: &mut Node, identifiers: &HashMap<String, String>) {
    let NodeKind::Element(element) = &mut node.kind else {
        return;
    };
    for (name, value) in &mut element.attrs {
        if name == "id"
            && let Some(replacement) = identifiers.get(value)
        {
            *value = replacement.clone();
        } else {
            *value = rewrite_svg_identifier_value(value, identifiers);
        }
    }
    for attribute in &mut element.namespace_attrs {
        if attribute.local_name == "id"
            && attribute.namespace_url.is_empty()
            && let Some(replacement) = identifiers.get(&attribute.value)
        {
            attribute.value = replacement.clone();
        } else {
            attribute.value = rewrite_svg_identifier_value(&attribute.value, identifiers);
        }
    }
    for child in &mut element.children {
        rewrite_svg_identifiers(child, identifiers);
    }
}

fn rewrite_svg_identifier_value(value: &str, identifiers: &HashMap<String, String>) -> String {
    let mut rewritten = value.to_owned();
    if let Some(id) = value.strip_prefix('#')
        && let Some(replacement) = identifiers.get(id)
    {
        rewritten = format!("#{replacement}");
    }
    for (id, replacement) in identifiers {
        rewritten = rewritten.replace(&format!("url(#{id})"), &format!("url(#{replacement})"));
    }
    rewritten
}

fn imported_svg_id_exists(state: &SvgExternalImportState, target: &str) -> bool {
    fn contains(node: &Node, target: &str) -> bool {
        let NodeKind::Element(element) = &node.kind else {
            return false;
        };
        element.attrs.get("id").is_some_and(|id| id == target)
            || element.children.iter().any(|child| contains(child, target))
    }
    state.imports.iter().any(|node| contains(node, target))
}

/// Discover document URLs referenced by inline SVG `<use>` elements. The
/// caller supplies the current document URL because each nested external SVG
/// resolves relative references against the fetched document's final URL.
fn collect_svg_use_document_urls(
    node: &Node,
    base_url: Option<&Url>,
    root_url: Option<&Url>,
    pending: &mut VecDeque<(Url, Url)>,
) {
    let NodeKind::Element(element) = &node.kind else {
        return;
    };
    if element.namespace_url == SVG_NAMESPACE
        && element.tag == "use"
        && let Some(href) = svg_href(element)
        && !href.starts_with('#')
        && let Some(url) = resolve_url(href, base_url, root_url)
        && url.fragment().is_some_and(|fragment| !fragment.is_empty())
        && let Some(fetch_url) = fetch_url(&url)
        && let Some(referring_url) = base_url.or(root_url)
    {
        pending.push_back((fetch_url, referring_url.clone()));
    }
    for child in &element.children {
        collect_svg_use_document_urls(child, base_url, root_url, pending);
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
    pub(crate) fn record_iframe_viewport(
        &self,
        element: ElementId,
        context: IframeEmbeddingContext,
    ) {
        self.iframe_viewports.borrow_mut().insert(element, context);
    }

    pub(crate) fn take_iframe_viewports(&self) -> HashMap<ElementId, IframeEmbeddingContext> {
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
        let mut cache = Self::default();
        let mut seen = HashSet::new();
        let mut pending = urls.into_iter().collect::<VecDeque<_>>();
        while let Some(url) = pending.pop_front() {
            let Some(fetch_url) = fetch_url(&url) else {
                continue;
            };
            if !seen.insert(fetch_url.clone()) {
                continue;
            }
            match fetcher.fetch(&fetch_url).await {
                Ok(fetched) => {
                    // An SVG loaded as an HTML/CSS image is secure-static:
                    // nested string URL references must not trigger preload
                    // fetches merely because the outer SVG happened to be
                    // fetched successfully.  `usvg` receives only the
                    // self-contained `data:` resolver in this mode.  A
                    // future Static policy can explicitly discover already
                    // authorized cache entries at this boundary.
                    cache.cache_fetched_resource(fetch_url, fetched);
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

    /// Complete the static external-`<use>` graph after ordinary visual
    /// resource preload. Each discovered document is parsed from cached bytes
    /// and may enqueue further same-origin SVG `<use>` documents.
    pub(crate) async fn preload_external_svg_uses(
        &mut self,
        fetcher: &ResourceFetcher,
        root: &Node,
        base_url: Option<&Url>,
        root_url: Option<&Url>,
    ) {
        let mut pending = VecDeque::new();
        collect_svg_use_document_urls(root, base_url, root_url, &mut pending);
        let mut seen = HashSet::new();
        let mut resolver = ExternalSvgUseResolver {
            base_url: base_url.cloned(),
            root_url: root_url.cloned(),
            documents: HashMap::new(),
        };
        while let Some((url, referring_url)) = pending.pop_front() {
            let Some(fetch_url) = fetch_url(&url) else {
                continue;
            };
            if !same_svg_resource_origin(&referring_url, &fetch_url)
                || !seen.insert(fetch_url.clone())
            {
                continue;
            }
            if !self.bytes.contains_key(&fetch_url) {
                match fetcher.fetch(&fetch_url).await {
                    Ok(fetched) => self.cache_fetched_resource(fetch_url.clone(), fetched),
                    Err(error) => {
                        log::debug!("failed to preload external SVG use {url}: {error}");
                        continue;
                    }
                }
            }
            let Some((source, document_url)) = self.external_svg_document_source(&fetch_url) else {
                continue;
            };
            let syntax = self.external_svg_document_syntax(&fetch_url);
            let Ok(document) = dom::parse_with_syntax(source, syntax) else {
                log::debug!("failed to parse external SVG use document {document_url}");
                continue;
            };
            collect_svg_use_document_urls(
                &document,
                Some(&document_url),
                Some(&document_url),
                &mut pending,
            );
            let external = ExternalSvgDocument {
                source: document,
                url: document_url.clone(),
            };
            resolver
                .documents
                .insert(fetch_url.clone(), external.clone());
            resolver.documents.insert(document_url, external);
        }
        self.external_svg_uses = resolver;
    }

    fn cache_fetched_resource(&mut self, fetch_url: Url, fetched: FetchedResource) {
        let cors_metadata = CorsResponseMetadata {
            allow_origin: fetched.access_control_allow_origin,
            allow_credentials: fetched.access_control_allow_credentials,
        };
        let metadata = CachedResourceMetadata {
            final_url: Some(fetched.final_url.clone()),
            content_type: fetched.content_type,
        };
        let bytes = Arc::new(fetched.bytes);
        self.bytes.insert(fetch_url.clone(), Arc::clone(&bytes));
        self.bytes.insert(fetched.final_url.clone(), bytes);
        self.resource_metadata
            .insert(fetch_url.clone(), metadata.clone());
        self.resource_metadata
            .insert(fetched.final_url.clone(), metadata);
        self.cors_response_metadata
            .insert(fetch_url, cors_metadata.clone());
        self.cors_response_metadata
            .insert(fetched.final_url, cors_metadata);
    }

    fn external_svg_document_source(&self, url: &Url) -> Option<(&str, Url)> {
        let bytes = self.bytes.get(url)?;
        let source = std::str::from_utf8(bytes).ok()?;
        let final_url = self
            .resource_metadata
            .get(url)
            .and_then(|metadata| metadata.final_url.clone())
            .unwrap_or_else(|| url.clone());
        Some((source, final_url))
    }

    fn external_svg_document_syntax(&self, url: &Url) -> DocumentSyntax {
        let mime_type = self
            .resource_metadata
            .get(url)
            .and_then(|metadata| metadata.content_type.as_deref())
            .map(str::trim)
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if matches!(
            mime_type.as_str(),
            "image/svg+xml" | "application/xml" | "text/xml"
        ) || url.path().ends_with(".svg")
            || url.path().ends_with(".xml")
        {
            DocumentSyntax::Xml
        } else {
            DocumentSyntax::Html
        }
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

    /// Determine the response taint after CSS request modifiers authorize the
    /// cached response. A no-CORS cross-origin image is usable but opaque;
    /// same-origin and CORS-authorized images are origin-clean.
    pub(crate) fn css_image_fetch_taint(
        &self,
        url: &Url,
        document_url: Option<&Url>,
        modifiers: &crate::css::RequestUrlModifiers,
    ) -> Option<ImageFetchTaint> {
        if !self.allows_css_image_request(url, document_url, modifiers) {
            return None;
        }
        let url = fetch_url(url)?;
        let origin_clean = document_url.is_some_and(|document_url| {
            urls_have_same_origin(&url, document_url) || modifiers.cross_origin.is_some()
        });
        Some(if origin_clean {
            ImageFetchTaint::OriginClean
        } else {
            ImageFetchTaint::Opaque
        })
    }

    pub(crate) fn image_asset_url_with_orientation(
        &self,
        url: &Url,
        orientation_policy: RasterOrientationPolicy,
        image_context: SvgImageContext,
    ) -> Option<ResourceImageAsset> {
        let url = fetch_url(url)?;
        let assets = if orientation_policy.applies_metadata_orientation() {
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
                            orientation_policy,
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
        orientation_policy: RasterOrientationPolicy,
        image_context: SvgImageContext,
    ) -> Option<ResourceImageAsset> {
        let assets = if orientation_policy.applies_metadata_orientation() {
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
                        .resolve_data_url_with_orientation(source, bytes, orientation_policy)
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
        let asset = match parse_inline_svg_with_presentation_overrides(
            element,
            &overrides,
            &self.external_svg_uses,
        ) {
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

#[cfg(test)]
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

pub(crate) fn same_svg_resource_origin(parent: &Url, child: &Url) -> bool {
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
    fn image_fetch_taint_distinguishes_same_origin_opaque_and_cors_images() {
        let document = Url::parse("https://document.example.test/page.html").unwrap();
        let same_origin = Url::parse("https://document.example.test/image.png").unwrap();
        let cross_origin = Url::parse("https://images.example.test/image.png").unwrap();
        let mut cache = ResourceCache::default();
        for url in [&same_origin, &cross_origin] {
            cache.bytes.insert(url.clone(), Arc::new(vec![0]));
        }

        assert_eq!(
            cache.css_image_fetch_taint(
                &same_origin,
                Some(&document),
                &crate::css::RequestUrlModifiers::default(),
            ),
            Some(ImageFetchTaint::OriginClean)
        );
        assert_eq!(
            cache.css_image_fetch_taint(
                &cross_origin,
                Some(&document),
                &crate::css::RequestUrlModifiers::default(),
            ),
            Some(ImageFetchTaint::Opaque)
        );

        cache.cors_response_metadata.insert(
            cross_origin.clone(),
            CorsResponseMetadata {
                allow_origin: Some("https://document.example.test".to_string()),
                allow_credentials: false,
            },
        );
        let anonymous = crate::css::RequestUrlModifiers {
            cross_origin: Some(crate::css::CrossOriginRequestMode::Anonymous),
            integrity: None,
            referrer_policy: None,
        };
        assert_eq!(
            cache.css_image_fetch_taint(&cross_origin, Some(&document), &anonymous),
            Some(ImageFetchTaint::OriginClean)
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

    fn external_svg_resolver<'a>(
        base_url: &str,
        documents: impl IntoIterator<Item = (&'a str, &'a str, DocumentSyntax)>,
    ) -> ExternalSvgUseResolver {
        let mut resolver = ExternalSvgUseResolver {
            base_url: Some(Url::parse(base_url).unwrap()),
            root_url: None,
            documents: HashMap::new(),
        };
        for (url, source, syntax) in documents {
            let url = Url::parse(url).unwrap();
            resolver.documents.insert(
                url.clone(),
                ExternalSvgDocument {
                    source: dom::parse_with_syntax(source, syntax).unwrap(),
                    url,
                },
            );
        }
        resolver
    }

    #[test]
    fn external_svg_use_imports_an_html_symbol_as_a_local_reference() {
        let resolver = external_svg_resolver(
            "https://example.test/page.html",
            [(
                "https://example.test/assets/symbols.html",
                r#"<html><svg xmlns="http://www.w3.org/2000/svg"><symbol id="green"><rect width="100" height="100" fill="green"/></symbol></svg></html>"#,
                DocumentSyntax::Html,
            )],
        );

        let expanded = resolver.expand_inline_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><use href="assets/symbols.html#green"/></svg>"#.to_owned(),
        );

        assert!(expanded.contains("id=\"quire-external-0-green\""));
        assert!(expanded.contains("href=\"#quire-external-0-green\""));
        assert!(!expanded.contains("symbols.html#green"));
        let asset = crate::svg::parse_svg_bytes(expanded.as_bytes()).unwrap();
        assert_eq!(
            asset.opaque_viewport_fill(),
            Some(crate::CssColor::new(0, 128, 0))
        );
    }

    #[test]
    fn external_svg_use_recursively_imports_same_origin_documents() {
        let resolver = external_svg_resolver(
            "https://example.test/page.html",
            [
                (
                    "https://example.test/assets/outer.svg",
                    r#"<svg xmlns="http://www.w3.org/2000/svg"><symbol id="outer"><use href="inner.svg#green"/></symbol></svg>"#,
                    DocumentSyntax::Xml,
                ),
                (
                    "https://example.test/assets/inner.svg",
                    r#"<svg xmlns="http://www.w3.org/2000/svg"><symbol id="green"><rect width="100" height="100" fill="green"/></symbol></svg>"#,
                    DocumentSyntax::Xml,
                ),
            ],
        );

        let expanded = resolver.expand_inline_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><use href="assets/outer.svg#outer"/></svg>"#
                .to_owned(),
        );

        assert!(expanded.contains("#quire-external-0-outer"));
        assert!(expanded.contains("#quire-external-1-green"));
        let asset = crate::svg::parse_svg_bytes(expanded.as_bytes()).unwrap();
        assert_eq!(
            asset.opaque_viewport_fill(),
            Some(crate::CssColor::new(0, 128, 0))
        );
    }

    #[test]
    fn external_svg_use_omits_cross_origin_documents() {
        let resolver = external_svg_resolver(
            "https://example.test/page.html",
            [(
                "https://other.test/symbols.svg",
                r#"<svg xmlns="http://www.w3.org/2000/svg"><symbol id="green"><rect width="100" height="100" fill="green"/></symbol></svg>"#,
                DocumentSyntax::Xml,
            )],
        );

        let expanded = resolver.expand_inline_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><use href="https://other.test/symbols.svg#green"/></svg>"#.to_owned(),
        );

        assert!(!expanded.contains("quire-external"));
        assert!(crate::svg::parse_svg_bytes(expanded.as_bytes()).is_ok());
    }

    #[test]
    fn external_svg_use_keeps_missing_fragments_unresolved() {
        let resolver = external_svg_resolver(
            "https://example.test/page.html",
            [(
                "https://example.test/symbols.svg",
                r#"<svg xmlns="http://www.w3.org/2000/svg"><symbol id="present"/></svg>"#,
                DocumentSyntax::Xml,
            )],
        );

        let expanded = resolver.expand_inline_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><use href="symbols.svg#missing"/></svg>"#
                .to_owned(),
        );

        assert!(!expanded.contains("quire-external"));
        assert!(expanded.contains("symbols.svg#missing"));
    }

    #[test]
    fn external_svg_use_namespaces_duplicate_document_ids() {
        let resolver = external_svg_resolver(
            "https://example.test/page.html",
            [
                (
                    "https://example.test/first.svg",
                    r#"<svg xmlns="http://www.w3.org/2000/svg"><symbol id="green"><rect width="10" height="10" fill="green"/></symbol></svg>"#,
                    DocumentSyntax::Xml,
                ),
                (
                    "https://example.test/second.svg",
                    r#"<svg xmlns="http://www.w3.org/2000/svg"><symbol id="green"><rect width="10" height="10" fill="green"/></symbol></svg>"#,
                    DocumentSyntax::Xml,
                ),
            ],
        );

        let expanded = resolver.expand_inline_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><use href="first.svg#green"/><use href="second.svg#green"/></svg>"#.to_owned(),
        );

        assert!(expanded.contains("id=\"quire-external-0-green\""));
        assert!(expanded.contains("id=\"quire-external-1-green\""));
        assert!(expanded.contains("href=\"#quire-external-0-green\""));
        assert!(expanded.contains("href=\"#quire-external-1-green\""));
    }

    #[test]
    fn external_svg_use_handles_cycles_without_recursive_imports() {
        let resolver = external_svg_resolver(
            "https://example.test/page.html",
            [
                (
                    "https://example.test/first.svg",
                    r#"<svg xmlns="http://www.w3.org/2000/svg"><symbol id="first"><use href="second.svg#second"/></symbol></svg>"#,
                    DocumentSyntax::Xml,
                ),
                (
                    "https://example.test/second.svg",
                    r#"<svg xmlns="http://www.w3.org/2000/svg"><symbol id="second"><use href="first.svg#first"/></symbol></svg>"#,
                    DocumentSyntax::Xml,
                ),
            ],
        );

        let expanded = resolver.expand_inline_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><use href="first.svg#first"/></svg>"#
                .to_owned(),
        );

        assert!(expanded.contains("#quire-external-0-first"));
        assert!(crate::svg::parse_svg_bytes(expanded.as_bytes()).is_ok());
    }

    #[test]
    fn external_svg_response_mime_selects_xml_parsing() {
        let mut cache = ResourceCache::default();
        let url = Url::parse("https://example.test/document.html").unwrap();
        cache.resource_metadata.insert(
            url.clone(),
            CachedResourceMetadata {
                final_url: Some(url.clone()),
                content_type: Some("image/svg+xml; charset=utf-8".to_owned()),
            },
        );

        assert_eq!(
            cache.external_svg_document_syntax(&url),
            DocumentSyntax::Xml
        );
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
                RasterOrientationPolicy::Encoded,
                SvgImageContext::from_used_color_scheme(crate::css::UsedColorScheme::Light),
            )
            .unwrap();
        let dark = cache
            .image_asset_url_with_orientation(
                &url,
                RasterOrientationPolicy::Encoded,
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
