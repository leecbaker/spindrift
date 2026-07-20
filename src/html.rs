use crate::css::layout_pt;
use crate::{
    Css, Document, PdfOptions, RenderOptions, ResourcePolicy, Result, css, dom, layout, resource,
    timing::DebugTimer,
};
use std::collections::HashMap;
use std::path::Path;
use url::Url;

/// Input markup syntax used for document parsing.
///
/// Quire defaults to automatic syntax selection: HTML parsing unless the
/// source begins with an XML declaration or its URL names an XML/XHTML
/// document. This follows HTML's distinction between `text/html` parsing and
/// XML/XHTML parsing:
/// <https://html.spec.whatwg.org/multipage/parsing.html#the-input-byte-stream>
/// and <https://www.w3.org/TR/xml/#NT-XMLDecl>.
///
/// ```
/// let syntax = quire::InputSyntax::Xml;
/// assert_eq!(syntax, quire::InputSyntax::Xml);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputSyntax {
    /// Infer HTML or XML syntax from the source.
    #[default]
    Auto,
    /// Parse the source as HTML.
    Html,
    /// Parse the source as XML/XHTML.
    Xml,
}

#[derive(Debug, Clone, PartialEq)]
/// An HTML or XML source document and its rendering configuration.
///
/// ```
/// let document = quire::Html::from_string("<h1>Hello</h1>");
/// ```
pub struct Html {
    source: String,
    input_syntax: InputSyntax,
    base_url: Option<Url>,
    root_url: Option<Url>,
    stylesheets: Vec<Css>,
    resource_policy: ResourcePolicy,
    iframe_depth: u8,
    /// An embedded document has a scrolling viewport distinct from the
    /// unfragmented canvas used to lay out its static contents.
    iframe_viewport: Option<layout::PageSize>,
}

impl Html {
    /// Creates a document from markup source, inferring its input syntax.
    ///
    /// ```
    /// let document = quire::Html::from_string("<p>Hello</p>");
    /// ```
    pub fn from_string(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            input_syntax: InputSyntax::Auto,
            base_url: None,
            root_url: None,
            stylesheets: Vec::new(),
            resource_policy: ResourcePolicy::default(),
            iframe_depth: 0,
            iframe_viewport: None,
        }
    }

    /// Creates an XML/XHTML document from markup source.
    ///
    /// ```
    /// let document = quire::Html::from_xml_string("<page><p>Hello</p></page>");
    /// ```
    pub fn from_xml_string(source: impl Into<String>) -> Self {
        Self::from_string(source).with_input_syntax(InputSyntax::Xml)
    }

    /// Asynchronously loads a document from a local file, inferring its input syntax.
    ///
    /// ```no_run
    /// # async fn load() -> quire::Result<()> {
    /// let document = quire::Html::from_file("document.html").await?;
    /// # let _ = document;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let _timer = DebugTimer::start(format!("reading HTML file {}", path.display()));
        let url = resource::file_url_from_path(path)?;
        let source = resource::read_to_string(&url).await?;
        Ok(Self {
            source,
            input_syntax: InputSyntax::Auto,
            base_url: Some(url),
            root_url: None,
            stylesheets: Vec::new(),
            resource_policy: ResourcePolicy::default(),
            iframe_depth: 0,
            iframe_viewport: None,
        })
    }

    /// Asynchronously loads an XML/XHTML document from a local file.
    ///
    /// ```no_run
    /// # async fn load() -> quire::Result<()> {
    /// let document = quire::Html::from_xml_file("document.xhtml").await?;
    /// # let _ = document;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn from_xml_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self::from_file(path)
            .await?
            .with_input_syntax(InputSyntax::Xml))
    }

    /// Creates an HTML document from a URL.
    ///
    /// The URL is also the document base used to resolve stylesheet, image, and
    /// font references, following the URL Standard:
    /// <https://url.spec.whatwg.org/>.
    ///
    /// ```no_run
    /// # async fn load() -> Result<(), Box<dyn std::error::Error>> {
    /// let url = "https://example.test/document.html".parse()?;
    /// let document = quire::Html::from_url(url).await?;
    /// # let _ = document;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_url(url: Url) -> Result<Self> {
        Self::from_url_with_resource_policy(url, ResourcePolicy::default()).await
    }

    /// Creates an HTML document from a URL using an explicit resource policy.
    ///
    /// ```no_run
    /// # async fn load() -> Result<(), Box<dyn std::error::Error>> {
    /// let url = "https://example.test/document.html".parse()?;
    /// let document = quire::Html::from_url_with_resource_policy(
    ///     url,
    ///     quire::ResourcePolicy::default(),
    /// ).await?;
    /// # let _ = document;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_url_with_resource_policy(
        url: Url,
        resource_policy: ResourcePolicy,
    ) -> Result<Self> {
        if resource::fetch_url(&url).is_none() {
            return Err(crate::Error::InvalidInput(format!(
                "unsupported URL for HTML input: {url}"
            )));
        }
        let fetcher = resource::ResourceFetcher::new(resource_policy)?;
        let (source, final_url) = fetcher.read_to_string(&url).await?;
        Ok(Self {
            source,
            input_syntax: InputSyntax::Auto,
            root_url: resource::origin_url(&final_url),
            base_url: Some(final_url),
            stylesheets: Vec::new(),
            resource_policy,
            iframe_depth: 0,
            iframe_viewport: None,
        })
    }

    /// Selects the parser syntax for this document.
    ///
    /// ```
    /// let document = quire::Html::from_string("<page />")
    ///     .with_input_syntax(quire::InputSyntax::Xml);
    /// ```
    pub fn with_input_syntax(mut self, input_syntax: InputSyntax) -> Self {
        self.input_syntax = input_syntax;
        self
    }

    /// Sets the base URL used to resolve document-relative resources.
    ///
    /// HTML's document base URL is the fallback URL used by relative links and
    /// CSS `url()` references:
    /// <https://html.spec.whatwg.org/multipage/urls-and-fetching.html#document-base-url>.
    /// File-backed documents already have a base from their path, so this only
    /// fills the document base for string-backed input while still setting the
    /// local root used for root-relative file URLs.
    ///
    /// ```
    /// # fn configure() -> Result<(), Box<dyn std::error::Error>> {
    /// let base_url = quire::Url::parse("https://example.test/guide/")?;
    /// let document = quire::Html::from_string("<img src=\"cover.png\">")
    ///     .with_base_url(base_url);
    /// # let _ = document;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_base_url(mut self, base_url: Url) -> Self {
        if self.base_url.is_none() {
            self.base_url = Some(base_url.clone());
        }
        self.root_url = resource::origin_url(&base_url).or(Some(base_url));
        self
    }

    /// Sets a local directory as the base and root for document-relative resources.
    ///
    /// ```no_run
    /// # fn configure() -> quire::Result<()> {
    /// let document = quire::Html::from_string("<img src=\"cover.png\">")
    ///     .with_base_path("assets")?;
    /// # let _ = document;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_base_path<P: AsRef<Path>>(mut self, base_path: P) -> Result<Self> {
        let base_url = resource::directory_url_from_path(base_path.as_ref())?;
        if self.base_url.is_none() {
            self.base_url = Some(base_url.clone());
        }
        self.root_url = Some(base_url);
        Ok(self)
    }

    /// Adds an author stylesheet to apply when rendering this document.
    ///
    /// ```
    /// let document = quire::Html::from_string("<p>Hello</p>")
    ///     .with_stylesheet(quire::Css::from_string("p { color: navy }"));
    /// ```
    pub fn with_stylesheet(mut self, stylesheet: Css) -> Self {
        self.stylesheets.push(stylesheet);
        self
    }

    /// Sets the policy used for every render-time external resource.
    ///
    /// ```
    /// let document = quire::Html::from_string("<p>Hello</p>")
    ///     .with_resource_policy(quire::ResourcePolicy::default());
    /// ```
    pub fn with_resource_policy(mut self, resource_policy: ResourcePolicy) -> Self {
        self.resource_policy = resource_policy;
        self
    }

    /// Asynchronously renders this document with the supplied options.
    ///
    /// ```no_run
    /// # async fn render() -> quire::Result<()> {
    /// let html = quire::Html::from_string("<p>Hello</p>");
    /// let document = html.render(&quire::RenderOptions::default()).await?;
    /// # let _ = document;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn render(&self, options: &RenderOptions) -> Result<Document> {
        let render_timer = DebugTimer::start("rendering HTML document");
        let resource_fetcher = resource::ResourceFetcher::new(self.resource_policy)?;
        let font_system_load = layout::start_font_system_load();
        let mut options = options.clone();
        let document_syntax = self.document_syntax();
        let mut root = {
            let _timer = DebugTimer::start(format!("parsing {document_syntax:?} document"));
            dom::parse_with_syntax(&self.source, document_syntax)?
        };
        // An explicit rendering target is useful to API callers and overrides
        // a source URL fragment. Otherwise, normal document navigation uses
        // the URL's fragment for both `:target` and static target scrolling.
        // <https://html.spec.whatwg.org/multipage/browsing-the-web.html#scroll-to-fragid>
        let target_fragment = options.target_fragment.as_deref().or_else(|| {
            self.base_url
                .as_ref()
                .and_then(|base_url| base_url.fragment())
        });
        dom::mark_target_fragment(&mut root, target_fragment);
        let mut stylesheets = {
            let _timer = DebugTimer::start("loading author stylesheets in document order");
            self.author_stylesheets(&root, &resource_fetcher).await?
        };
        stylesheets.extend(
            self.stylesheets
                .iter()
                .cloned()
                .map(|stylesheet| stylesheet.with_resource_policy(self.resource_policy)),
        );
        let stylesheets = {
            let _timer = DebugTimer::start(format!(
                "resolving imports for {} stylesheet(s)",
                stylesheets.len()
            ));
            resolve_stylesheet_imports(stylesheets).await?
        };
        // Media features describe the renderer-provided output viewport. An
        // author `@page` rule may subsequently choose the page box used for
        // layout, but it must not feed back into evaluation of `@media` rules
        // in the same stylesheet.
        // <https://www.w3.org/TR/mediaqueries-4/#width>
        let media_environment = options.media_environment();
        log::debug!("applying {} stylesheet(s)", stylesheets.len());
        {
            let _timer = DebugTimer::start("applying stylesheet options");
            for stylesheet in &stylesheets {
                css::apply_stylesheet_options(stylesheet, &mut options);
            }
        }
        let mut parsed_stylesheets = vec![css::html5_user_agent_stylesheet()];
        if document_syntax == dom::DocumentSyntax::Html {
            parsed_stylesheets.push(css::html_document_important_user_agent_stylesheet());
        }
        if options.presentational_hints {
            parsed_stylesheets.push(css::html5_presentational_hints_stylesheet_with_urls(
                self.base_url.as_ref(),
                self.root_url.as_ref(),
            ));
        }
        {
            let _timer = DebugTimer::start(format!(
                "parsing {} author stylesheet(s)",
                stylesheets.len()
            ));
            parsed_stylesheets.extend(stylesheets.iter().map(|stylesheet| {
                css::parse_stylesheet_with_media_environment(stylesheet, &media_environment)
            }));
        }
        let font_system_load = {
            let _timer = DebugTimer::start("loading @font-face sources");
            font_system_load
                .load_stylesheet_fonts_with_fetcher(&parsed_stylesheets, resource_fetcher.clone())
        };
        // Images and CSS image resources are optional visual subresources:
        // CSS Images keeps their replaced box in layout when fetching fails.
        // Keep the primary document, linked stylesheets, and font loading on
        // the caller's strict fetcher above.
        // <https://drafts.csswg.org/css-images-3/#image-fallbacks>
        let mut visual_asset_policy = resource_fetcher.policy();
        visual_asset_policy.error_policy = resource::FetchErrorPolicy::Allow;
        let visual_asset_fetcher = resource::ResourceFetcher::new(visual_asset_policy)?;
        let resource_cache = {
            let resource_paths = resource_paths(
                &root,
                &stylesheets,
                &parsed_stylesheets,
                self.base_url(),
                self.root_url(),
            );
            let _timer = DebugTimer::start(format!(
                "preloading {} referenced resource(s)",
                resource_paths.len()
            ));
            resource::ResourceCache::preload(&visual_asset_fetcher, resource_paths).await?
        };
        let font_system = {
            let _timer = DebugTimer::start("finishing document font system");
            font_system_load.finish_checked().await?
        };
        let empty_iframe_documents = HashMap::new();
        let measurement_document = {
            let _timer = DebugTimer::start("measuring iframe viewports");
            layout::layout_prepared_dom(layout::PreparedDomLayout {
                root: &root,
                stylesheets: &parsed_stylesheets,
                options: &options,
                base_url: self.base_url(),
                root_url: self.root_url(),
                resource_cache: &resource_cache,
                iframe_documents: &empty_iframe_documents,
                iframe_viewport: self.iframe_viewport,
                font_system: font_system.clone(),
            })
        };
        // An iframe renders a complete `Html` document, which may itself
        // contain iframes. Box this recursive future boundary so nested
        // documents do not make `Html::render`'s future infinitely sized.
        let iframe_documents = Box::pin(self.render_iframe_documents(
            &root,
            &options,
            resource_cache.take_iframe_viewports(),
        ))
        .await;
        let mut document = if iframe_documents.is_empty() {
            measurement_document
        } else {
            let _timer = DebugTimer::start("laying out document with iframe subdocuments");
            layout::layout_prepared_dom(layout::PreparedDomLayout {
                root: &root,
                stylesheets: &parsed_stylesheets,
                options: &options,
                base_url: self.base_url(),
                root_url: self.root_url(),
                resource_cache: &resource_cache,
                iframe_documents: &iframe_documents,
                iframe_viewport: self.iframe_viewport,
                font_system,
            })
        };
        let mut image_store = resource_cache.take_image_store();
        image_store.finalize();
        document.image_store = Box::new(image_store);
        {
            let _timer = DebugTimer::start("extracting document metadata");
            document.metadata.title = dom::first_element_text(&root, "title");
            document.metadata.author = dom::first_meta_content(&root, "author");
            document.metadata.creator = dom::first_meta_content(&root, "generator");
        }
        log::info!("rendered {} page(s)", document.pages.len());
        render_timer.finish();
        Ok(document)
    }

    /// Asynchronously renders and serializes this document as PDF bytes.
    ///
    /// ```no_run
    /// # async fn render() -> quire::Result<()> {
    /// let html = quire::Html::from_string("<p>Hello</p>");
    /// let pdf = html
    ///     .write_pdf_bytes(
    ///         &quire::RenderOptions::default(),
    ///         &quire::PdfOptions::default(),
    ///     )
    ///     .await?;
    /// # let _ = pdf;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn write_pdf_bytes(
        &self,
        render_options: &RenderOptions,
        pdf_options: &PdfOptions,
    ) -> Result<Vec<u8>> {
        let _timer = DebugTimer::start("rendering and writing PDF bytes");
        self.render(render_options)
            .await?
            .write_pdf_bytes(pdf_options)
    }

    /// Asynchronously renders and serializes this document to a PDF file.
    ///
    /// ```no_run
    /// # async fn render() -> quire::Result<()> {
    /// let html = quire::Html::from_string("<p>Hello</p>");
    /// html
    ///     .write_pdf(
    ///         "document.pdf",
    ///         &quire::RenderOptions::default(),
    ///         &quire::PdfOptions::default(),
    ///     )
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn write_pdf<P: AsRef<Path>>(
        &self,
        target: P,
        render_options: &RenderOptions,
        pdf_options: &PdfOptions,
    ) -> Result<()> {
        let bytes = self.write_pdf_bytes(render_options, pdf_options).await?;
        let _timer = DebugTimer::start(format!("writing PDF file {}", target.as_ref().display()));
        tokio::fs::write(target, bytes).await?;
        Ok(())
    }

    /// Returns the base URL used to resolve document-relative resources.
    ///
    /// ```
    /// let document = quire::Html::from_string("<p>Hello</p>");
    /// assert!(document.base_url().is_none());
    /// ```
    pub fn base_url(&self) -> Option<&Url> {
        self.base_url.as_ref()
    }

    pub(crate) fn root_url(&self) -> Option<&Url> {
        self.root_url.as_ref()
    }

    #[cfg(test)]
    fn embedded_styles(&self) -> Vec<Css> {
        let mut styles = Vec::new();
        let mut rest = self.source.as_str();
        while let Some(start) = find_ascii_case_insensitive(rest, "<style") {
            rest = &rest[start + "<style".len()..];
            let Some(tag_end) = rest.find('>') else {
                break;
            };
            rest = &rest[tag_end + 1..];
            let Some(end) = find_ascii_case_insensitive(rest, "</style>") else {
                break;
            };
            styles.push(
                Css::from_string(embedded_style_css(&rest[..end]))
                    .with_base_url(self.base_url.clone())
                    .with_root_url(self.root_url.clone()),
            );
            rest = &rest[end + "</style>".len()..];
        }
        styles
    }

    async fn author_stylesheets(
        &self,
        root: &dom::Node,
        resource_fetcher: &resource::ResourceFetcher,
    ) -> Result<Vec<Css>> {
        let mut styles = Vec::new();
        for source in dom::stylesheet_sources_in_document_order(root) {
            match source {
                dom::StylesheetSource::Embedded(css) => styles.push(
                    Css::from_string(embedded_style_css(&css))
                        .with_base_url(self.base_url.clone())
                        .with_root_url(self.root_url.clone()),
                ),
                dom::StylesheetSource::Link(href) => {
                    let Some(path) =
                        resource::resolve_fetchable_url(&href, self.base_url(), self.root_url())
                    else {
                        if !resource_fetcher.allows_fetch_errors() {
                            return Err(crate::Error::InvalidInput(format!(
                                "could not resolve linked stylesheet URL {href:?}"
                            )));
                        }
                        log::debug!("skipping linked stylesheet without base URL: {href}");
                        continue;
                    };
                    log::debug!("loading linked stylesheet {path}");
                    match Css::from_url_with_fetcher(path, resource_fetcher).await {
                        Ok(stylesheet) => {
                            styles.push(stylesheet.with_root_url(self.root_url.clone()))
                        }
                        Err(error) if resource_fetcher.allows_fetch_errors() => {
                            log::debug!("failed to load linked stylesheet {href}: {error}");
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        Ok(styles)
    }

    fn document_syntax(&self) -> dom::DocumentSyntax {
        match self.input_syntax {
            InputSyntax::Auto
                if starts_with_xml_declaration(&self.source) || self.url_indicates_xml_syntax() =>
            {
                dom::DocumentSyntax::Xml
            }
            InputSyntax::Auto | InputSyntax::Html => dom::DocumentSyntax::Html,
            InputSyntax::Xml => dom::DocumentSyntax::Xml,
        }
    }

    fn url_indicates_xml_syntax(&self) -> bool {
        self.base_url
            .as_ref()
            .and_then(|url| Path::new(url.path()).extension())
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "xml" | "xhtml" | "xht"
                )
            })
    }

    /// Render resolved iframe documents after the parent measurement pass has
    /// established each embedding content-box viewport. Failed resources are
    /// intentionally omitted so the iframe's normal fallback behavior remains
    /// available in the final parent layout.
    async fn render_iframe_documents(
        &self,
        root: &dom::Node,
        options: &RenderOptions,
        viewports: HashMap<dom::ElementId, (f32, f32)>,
    ) -> HashMap<dom::ElementId, Document> {
        const MAX_IFRAME_DEPTH: u8 = 8;
        if self.iframe_depth >= MAX_IFRAME_DEPTH {
            return HashMap::new();
        }
        let mut sources = Vec::new();
        collect_iframe_sources(root, &mut sources);
        let mut documents = HashMap::new();
        for (element_id, source) in sources {
            let Some((width, height)) = viewports.get(&element_id).copied() else {
                continue;
            };
            let mut iframe_options = options.clone();
            iframe_options.page_size =
                layout::PageSize::from_points(width.max(1.0), height.max(10_000.0));
            iframe_options.set_margin(layout_pt(0.0));
            let iframe_result = match source {
                // `srcdoc` wins over `src` and its URL is `about:srcdoc`; its
                // fallback base URL is inherited from the embedding document.
                // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#attr-iframe-srcdoc>
                IframeSource::Srcdoc(source) => Ok(Html {
                    source,
                    input_syntax: InputSyntax::Html,
                    base_url: self.base_url.clone(),
                    root_url: self.root_url.clone(),
                    stylesheets: Vec::new(),
                    resource_policy: self.resource_policy,
                    iframe_depth: self.iframe_depth + 1,
                    iframe_viewport: Some(layout::PageSize::from_points(
                        width.max(1.0),
                        height.max(1.0),
                    )),
                }),
                IframeSource::Url(source) => {
                    // Resource fetching deliberately removes a URL fragment
                    // because it does not identify a distinct network
                    // resource. Keep it for the embedded browsing context's
                    // initial fragment navigation.
                    let Some(resolved_url) =
                        resource::resolve_url(&source, self.base_url(), self.root_url())
                    else {
                        continue;
                    };
                    iframe_options.target_fragment = resolved_url.fragment().map(str::to_owned);
                    let Some(url) = resource::fetch_url(&resolved_url) else {
                        continue;
                    };
                    Html::from_url_with_resource_policy(url, self.resource_policy).await
                }
            };
            match iframe_result {
                Ok(mut iframe) => {
                    iframe.iframe_depth = self.iframe_depth + 1;
                    iframe.iframe_viewport = Some(layout::PageSize::from_points(
                        width.max(1.0),
                        height.max(1.0),
                    ));
                    match Box::pin(iframe.render(&iframe_options)).await {
                        Ok(mut document) => {
                            document.materialize_images_for_embedding();
                            documents.insert(element_id, document);
                        }
                        Err(error) => log::debug!("failed to render iframe subdocument: {error}"),
                    }
                }
                Err(error) => log::debug!("failed to load iframe subdocument: {error}"),
            }
        }
        documents
    }
}

fn resource_paths(
    root: &dom::Node,
    source_stylesheets: &[Css],
    parsed_stylesheets: &[css::Stylesheet],
    base_url: Option<&Url>,
    root_url: Option<&Url>,
) -> Vec<Url> {
    let mut paths = Vec::new();
    collect_html_resource_paths(root, base_url, root_url, &mut paths);
    paths.extend(source_stylesheets.iter().flat_map(|stylesheet| {
        resource::css_resource_urls(
            stylesheet.source(),
            stylesheet.base_url(),
            stylesheet.root_url(),
        )
    }));
    for stylesheet in parsed_stylesheets {
        for font_face in &stylesheet.font_faces {
            for source in &font_face.sources {
                let css::FontFaceSource::Url {
                    value,
                    base_url,
                    root_url,
                } = source;
                if let Some(path) =
                    resource::resolve_fetchable_url(value, base_url.as_ref(), root_url.as_ref())
                {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

fn collect_html_resource_paths(
    node: &dom::Node,
    base_url: Option<&Url>,
    root_url: Option<&Url>,
    paths: &mut Vec<Url>,
) {
    let dom::NodeKind::Element(element) = &node.kind else {
        return;
    };
    // Inline style attributes can introduce CSS image URLs just like embedded
    // and linked stylesheets. Preload them before layout so URL backgrounds
    // have the same resource availability as stylesheet backgrounds.
    if let Some(style) = element.attrs.get("style") {
        paths.extend(resource::css_resource_urls(style, base_url, root_url));
    }
    // `background` is a legacy presentational hint on table structure boxes.
    // It becomes `background-image` during cascade, but assets are collected
    // before cascade/layout so it must participate in this HTML preload pass.
    if matches!(
        element.tag.as_str(),
        "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
    ) && let Some(background) = element.attrs.get("background")
        && let Some(path) = resource::resolve_fetchable_url(background, base_url, root_url)
    {
        paths.push(path);
    }
    if matches!(element.tag.as_str(), "img" | "input" | "video")
        && let Some(src) = element
            .attrs
            .get("src")
            .or_else(|| element.attrs.get("poster"))
        && let Some(path) = resource::resolve_fetchable_url(src, base_url, root_url)
    {
        paths.push(path);
    }
    if element.tag == "img"
        && !element.attrs.contains_key("src")
        && let Some(srcset) = element.attrs.get("srcset")
        && let Some(src) = srcset.split(',').next().and_then(|candidate| {
            candidate
                .split_ascii_whitespace()
                .next()
                .filter(|value| !value.is_empty())
        })
        && let Some(path) = resource::resolve_fetchable_url(src, base_url, root_url)
    {
        paths.push(path);
    }
    if matches!(element.tag.as_str(), "object" | "embed")
        && let Some(src) = element
            .attrs
            .get("data")
            .or_else(|| element.attrs.get("src"))
        && let Some(path) = resource::resolve_fetchable_url(src, base_url, root_url)
    {
        paths.push(path);
    }
    for child in &element.children {
        collect_html_resource_paths(child, base_url, root_url, paths);
    }
}

enum IframeSource {
    Srcdoc(String),
    Url(String),
}

fn collect_iframe_sources(node: &dom::Node, sources: &mut Vec<(dom::ElementId, IframeSource)>) {
    let dom::NodeKind::Element(element) = &node.kind else {
        return;
    };
    if element.tag.eq_ignore_ascii_case("iframe") {
        // `srcdoc` takes precedence even if its value is the empty string.
        // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#attr-iframe-srcdoc>
        if let Some(source) = element.attrs.get("srcdoc") {
            sources.push((element.id, IframeSource::Srcdoc(source.clone())));
        } else if let Some(source) = element.attrs.get("src") {
            sources.push((element.id, IframeSource::Url(source.clone())));
        }
    }
    for child in &element.children {
        collect_iframe_sources(child, sources);
    }
}

async fn resolve_stylesheet_imports(stylesheets: Vec<Css>) -> Result<Vec<Css>> {
    let mut resolved = Vec::new();
    for stylesheet in stylesheets {
        resolved.extend(stylesheet.with_imports().await?);
    }
    Ok(resolved)
}

#[cfg(test)]
fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn starts_with_xml_declaration(source: &str) -> bool {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let Some(rest) = source.strip_prefix("<?xml") else {
        return false;
    };
    rest.as_bytes().first().is_some_and(u8::is_ascii_whitespace)
}

/// Normalizes source text from an embedded HTML/XHTML `style` element.
///
/// HTML defines `style` contents as CSS text, while XHTML-compatible WPT files
/// commonly wrap that CSS in XML CDATA delimiters. CSS Syntax only accepts the
/// stylesheet contents, so the XML wrapper has to be removed before parsing:
/// <https://html.spec.whatwg.org/multipage/semantics.html#the-style-element>
/// and <https://www.w3.org/TR/css-syntax-3/#style-rules>.
fn embedded_style_css(source: &str) -> String {
    let trimmed = source.trim();
    let unwrapped = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|value| value.strip_suffix("]]>"))
        .unwrap_or(trimmed);
    unwrapped.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FetchErrorPolicy, ResourcePolicy, dom};

    #[tokio::test]
    async fn missing_image_is_an_optional_visual_subresource() {
        let strict = Html::from_string("<img src=\"missing-resource.png\">")
            .with_base_path(".")
            .unwrap();
        assert!(strict.render(&RenderOptions::default()).await.is_ok());

        let allowing = Html::from_string("<img src=\"missing-resource.png\">")
            .with_base_path(".")
            .unwrap()
            .with_resource_policy(ResourcePolicy {
                error_policy: FetchErrorPolicy::Allow,
                ..ResourcePolicy::default()
            });
        assert!(allowing.render(&RenderOptions::default()).await.is_ok());
    }

    #[tokio::test]
    async fn missing_linked_stylesheet_requires_explicit_fetch_error_recovery() {
        let source = "<link rel=\"stylesheet\" href=\"missing-stylesheet.css\"><p>Text</p>";
        let strict = Html::from_string(source).with_base_path(".").unwrap();
        assert!(strict.render(&RenderOptions::default()).await.is_err());

        let allowing = Html::from_string(source)
            .with_base_path(".")
            .unwrap()
            .with_resource_policy(ResourcePolicy {
                error_policy: FetchErrorPolicy::Allow,
                ..ResourcePolicy::default()
            });
        assert!(allowing.render(&RenderOptions::default()).await.is_ok());
    }

    #[tokio::test]
    async fn rendered_data_image_uses_document_store_after_layout() {
        let html = Html::from_string(
            "<img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\">",
        );
        let document = html.render(&RenderOptions::default()).await.unwrap();

        assert_eq!(document.image_store.len(), 1);
        assert!(matches!(
            document.pages[0].images[0].source,
            crate::document::RenderedImageSource::Stored { .. }
        ));
        assert!(
            document
                .write_pdf_bytes(&crate::PdfOptions::default())
                .is_ok()
        );
    }

    #[tokio::test]
    async fn rendered_gradient_is_kept_as_a_vector_paint_source() {
        let html = Html::from_string(
            "<div style=\"width: 24pt; height: 24pt; background: linear-gradient(in srgb, red, blue) no-repeat\"></div>",
        );
        let document = html.render(&RenderOptions::default()).await.unwrap();

        assert_eq!(document.image_store.len(), 0);
        assert_eq!(document.pages[0].gradient_patterns.len(), 1);
        assert!(
            !document
                .write_pdf_bytes(&crate::PdfOptions::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn embedded_styles_unwrap_xhtml_cdata_sections() {
        let html = Html::from_string(
            "<style type=\"text/css\"><![CDATA[\n  div { background: green }\n]]></style>",
        );
        let styles = html.embedded_styles();

        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].source(), "\n  div { background: green }\n");
    }

    #[test]
    fn auto_syntax_detects_xml_declaration() {
        let html = Html::from_string(r#"<?xml version="1.0" encoding="UTF-8"?><html/>"#);

        assert_eq!(html.document_syntax(), dom::DocumentSyntax::Xml);
    }

    #[test]
    fn auto_syntax_detects_xml_declaration_after_utf8_bom() {
        let html = Html::from_string("\u{feff}<?xml version=\"1.0\"?><html/>");

        assert_eq!(html.document_syntax(), dom::DocumentSyntax::Xml);
    }

    #[test]
    fn auto_syntax_ignores_xml_stylesheet_processing_instruction() {
        let html = Html::from_string(r#"<?xml-stylesheet href="style.css"?><html></html>"#);

        assert_eq!(html.document_syntax(), dom::DocumentSyntax::Html);
    }

    #[test]
    fn explicit_html_syntax_overrides_xml_declaration_detection() {
        let html = Html::from_string(r#"<?xml version="1.0"?><html></html>"#)
            .with_input_syntax(InputSyntax::Html);

        assert_eq!(html.document_syntax(), dom::DocumentSyntax::Html);
    }

    #[tokio::test]
    async fn explicit_xml_render_reports_xml_parse_errors() {
        let error = Html::from_xml_string("<html><body></html>")
            .render(&RenderOptions::default())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("XML parse error"));
    }

    #[tokio::test]
    async fn auto_xml_declaration_render_reports_xml_parse_errors() {
        let error = Html::from_string(r#"<?xml version="1.0"?><html><body></html>"#)
            .render(&RenderOptions::default())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("XML parse error"));
    }
}
