use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use url::Url;

use crate::css::layout_pt;
use crate::timing::DebugTimer;
use crate::{
    Css, Document, DocumentDate, PdfOptions, RenderOptions, ResourcePolicy, Result, css, dom,
    layout, resource,
};

/// Input markup syntax used for document parsing.
///
/// Quire defaults to automatic syntax selection: HTML parsing unless the
/// source begins with an XML declaration or its URL names an XML/XHTML
/// document. This follows HTML's distinction between `text/html` parsing and
/// XML/XHTML parsing:
/// <https://html.spec.whatwg.org/multipage/parsing.html#the-input-byte-stream>
/// and <https://www.w3.org/TR/xml/#NT-XMLDecl>.
///
/// ```no_run
/// use quire::{Html, InputSyntax, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render_xhtml() -> quire::Result<()> {
/// let html = Html::from_file("document.xhtml")
///     .await?
///     .with_input_syntax(InputSyntax::Xml);
/// let mut output = File::create("document.pdf")?;
/// html.write_pdf(
///     &mut output,
///     &RenderOptions::default(),
///     &PdfOptions::default(),
/// )
/// .await?;
/// # Ok(())
/// # }
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
/// ```no_run
/// use quire::{Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let html = Html::from_file("document.html").await?;
/// let mut output = File::create("document.pdf")?;
/// html.write_pdf(
///     &mut output,
///     &RenderOptions::default(),
///     &PdfOptions::default(),
/// )
/// .await?;
/// # Ok(())
/// # }
/// ```
pub struct Html {
    source: String,
    input_syntax: InputSyntax,
    /// URL of the loaded document itself, deliberately distinct from the
    /// effective base URL used to resolve relative references.
    document_url: Option<Url>,
    base_url: Option<Url>,
    root_url: Option<Url>,
    stylesheets: Vec<Css>,
    resource_policy: ResourcePolicy,
    iframe_depth: u8,
    /// An embedded document has a scrolling viewport distinct from the
    /// unfragmented canvas used to lay out its static contents.
    iframe_viewport: Option<layout::IframeEmbeddingContext>,
    /// Legacy body-margin values from this document's immediate container
    /// frame. They are cascade context for the embedded document, not a
    /// property of the iframe's own layout box.
    /// <https://html.spec.whatwg.org/multipage/rendering.html#the-page>
    iframe_container_body_margins: Option<css::HtmlContainerFrameBodyMargins>,
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
            document_url: None,
            base_url: None,
            root_url: None,
            stylesheets: Vec::new(),
            resource_policy: ResourcePolicy::default(),
            iframe_depth: 0,
            iframe_viewport: None,
            iframe_container_body_margins: None,
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
            document_url: Some(url.clone()),
            base_url: Some(url),
            root_url: None,
            stylesheets: Vec::new(),
            resource_policy: ResourcePolicy::default(),
            iframe_depth: 0,
            iframe_viewport: None,
            iframe_container_body_margins: None,
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
            document_url: Some(final_url.clone()),
            root_url: resource::origin_url(&final_url),
            base_url: Some(final_url),
            stylesheets: Vec::new(),
            resource_policy,
            iframe_depth: 0,
            iframe_viewport: None,
            iframe_container_body_margins: None,
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
        // HTML's color-scheme meta value establishes the document page's
        // supported schemes. Model that document input as the first
        // document-level root declaration, so ordinary author
        // `color-scheme` rules that follow it can still override it through
        // the cascade.
        // <https://html.spec.whatwg.org/multipage/semantics.html#meta-color-scheme>
        let mut stylesheets = stylesheets;
        if document_syntax == dom::DocumentSyntax::Html
            && let Some(content) = dom::first_meta_content(&root, "color-scheme")
            && css::ComputedColorScheme::parse(&content).is_some()
        {
            stylesheets.insert(
                0,
                Css::from_string(format!("html {{ color-scheme: {content} }}")),
            );
        }
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
        let user_agent_stylesheet = css::html5_user_agent_stylesheet();
        let html_important_stylesheet = (document_syntax == dom::DocumentSyntax::Html)
            .then(css::html_document_important_user_agent_stylesheet);
        let mut parsed_stylesheets = Vec::new();
        if document_syntax == dom::DocumentSyntax::Html || document_is_xhtml(&root) {
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
        let layout_stylesheets = css::Stylesheets::for_document(
            user_agent_stylesheet,
            html_important_stylesheet,
            &parsed_stylesheets,
        )
        .with_html_container_frame_body_margins(self.iframe_container_body_margins)
        .with_image_set_resolution_dppx(options.device_resolution_dppx());
        let font_system_load = {
            let _timer = DebugTimer::start("loading @font-face sources");
            font_system_load
                .load_stylesheet_fonts_with_fetcher(&layout_stylesheets, resource_fetcher.clone())
        };
        // Images and CSS image resources are optional visual subresources:
        // CSS Images keeps their replaced box in layout when fetching fails.
        // Keep the primary document, linked stylesheets, and font loading on
        // the caller's strict fetcher above.
        // <https://drafts.csswg.org/css-images-3/#image-fallbacks>
        resolve_html_image_sources(&mut root, &media_environment);
        let mut visual_asset_policy = resource_fetcher.policy();
        visual_asset_policy.error_policy = resource::FetchErrorPolicy::Allow;
        let visual_asset_fetcher = resource::ResourceFetcher::new(visual_asset_policy)?;
        let mut resource_cache = {
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
        resource_cache
            .preload_external_svg_uses(
                &visual_asset_fetcher,
                &root,
                self.base_url(),
                self.root_url(),
            )
            .await;
        resolve_embedded_rendering_states(
            &mut root,
            self.base_url(),
            self.root_url(),
            &resource_cache,
        );
        let font_system = {
            let _timer = DebugTimer::start("finishing document font system");
            font_system_load.finish_checked().await?
        };
        let empty_iframe_documents = HashMap::new();
        let measurement_document = {
            let _timer = DebugTimer::start("measuring iframe viewports");
            layout::layout_prepared_dom(layout::PreparedDomLayout {
                root: &root,
                stylesheets: layout_stylesheets,
                options: &options,
                document_url: self.document_url.as_ref(),
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
                stylesheets: layout_stylesheets,
                options: &options,
                document_url: self.document_url.as_ref(),
                base_url: self.base_url(),
                root_url: self.root_url(),
                resource_cache: &resource_cache,
                iframe_documents: &iframe_documents,
                iframe_viewport: self.iframe_viewport,
                font_system,
            })
        };
        let mut image_store = resource_cache.take_image_store();
        image_store.set_output_resolution_dppx(options.device_resolution_dppx());
        image_store.finalize();
        document.image_store = Box::new(image_store);
        {
            let _timer = DebugTimer::start("extracting document metadata");
            document.metadata.title = dom::first_element_text(&root, "title");
            document.metadata.author = dom::first_meta_content(&root, "author");
            document.metadata.creator = dom::first_meta_content(&root, "generator");
            document.metadata.language = dom::document_language(&root);
            document.metadata.description = dom::first_meta_content(&root, "description");
            document.metadata.keywords = document_keywords(&root);
            document.metadata.created = metadata_date(&root, "dcterms.created");
            document.metadata.modified = metadata_date(&root, "dcterms.modified");
        }
        log::info!("rendered {} page(s)", document.pages.len());
        render_timer.finish();
        Ok(document)
    }

    /// Asynchronously renders this document and serializes it as a PDF into
    /// `writer`.
    ///
    /// ```no_run
    /// # async fn render(output: &mut Vec<u8>) -> quire::Result<()> {
    /// let html = quire::Html::from_string("<p>Hello</p>");
    /// html
    ///     .write_pdf(
    ///         output,
    ///         &quire::RenderOptions::default(),
    ///         &quire::PdfOptions::default(),
    ///     )
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn write_pdf<W: Write>(
        &self,
        writer: &mut W,
        render_options: &RenderOptions,
        pdf_options: &PdfOptions,
    ) -> Result<()> {
        let _timer = DebugTimer::start("rendering and writing PDF");
        self.render(render_options)
            .await?
            .write_pdf(writer, pdf_options)
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
                dom::StylesheetSource::Embedded { css, scope_anchor } => styles.push(
                    Css::from_string(embedded_style_css(&css))
                        .with_base_url(self.base_url.clone())
                        .with_root_url(self.root_url.clone())
                        .with_scope_anchor(scope_anchor),
                ),
                dom::StylesheetSource::Link { href, scope_anchor } => {
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
                    match Css::from_link_url_with_fetcher(path, resource_fetcher).await {
                        Ok(Some(stylesheet)) => styles.push(
                            stylesheet
                                .with_root_url(self.root_url.clone())
                                .with_scope_anchor(scope_anchor),
                        ),
                        Ok(None) => {
                            log::debug!("ignoring linked stylesheet {href} with non-CSS MIME type");
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
        viewports: HashMap<dom::ElementId, layout::IframeEmbeddingContext>,
    ) -> HashMap<dom::ElementId, Document> {
        const MAX_IFRAME_DEPTH: u8 = 8;
        if self.iframe_depth >= MAX_IFRAME_DEPTH {
            return HashMap::new();
        }
        let mut sources = Vec::new();
        collect_iframe_sources(root, &mut sources);
        let mut documents = HashMap::new();
        for source in sources {
            let Some(context) = viewports.get(&source.element_id).copied() else {
                continue;
            };
            let width = context.viewport.width();
            let height = context.viewport.height();
            let context = layout::IframeEmbeddingContext {
                viewport: layout::PageSize::from_points(width.max(1.0), height.max(1.0)),
                effective_zoom: context.effective_zoom,
            };
            let mut iframe_options = options.clone();
            iframe_options.page_size =
                layout::PageSize::from_points(width.max(1.0), height.max(10_000.0));
            iframe_options.iframe_page_margins = Some(layout::PageMargins::all(layout_pt(0.0)));
            let iframe_result = match source.source {
                // `srcdoc` wins over `src` and its URL is `about:srcdoc`; its
                // fallback base URL is inherited from the embedding document.
                // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#attr-iframe-srcdoc>
                IframeSource::Srcdoc(srcdoc) => Ok(Html {
                    source: srcdoc,
                    input_syntax: InputSyntax::Html,
                    document_url: None,
                    base_url: self.base_url.clone(),
                    root_url: self.root_url.clone(),
                    stylesheets: Vec::new(),
                    resource_policy: self.resource_policy,
                    iframe_depth: self.iframe_depth + 1,
                    iframe_viewport: Some(context),
                    iframe_container_body_margins: Some(source.container_body_margins),
                }),
                IframeSource::Url(url_source) => {
                    // Resource fetching deliberately removes a URL fragment
                    // because it does not identify a distinct network
                    // resource. Keep it for the embedded browsing context's
                    // initial fragment navigation.
                    let Some(resolved_url) =
                        resource::resolve_url(&url_source, self.base_url(), self.root_url())
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
                    iframe.iframe_viewport = Some(context);
                    iframe.iframe_container_body_margins = Some(source.container_body_margins);
                    match Box::pin(iframe.render(&iframe_options)).await {
                        Ok(mut document) => {
                            document.materialize_images_for_embedding();
                            documents.insert(source.element_id, document);
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

/// Whether an XML document is an XHTML document governed by HTML's rendering
/// rules.
///
/// The HTML rendering rules define presentational hints in the XHTML
/// namespace, so an XHTML document receives them even when it was parsed with
/// XML syntax. Other XML vocabularies retain the ordinary CSS cascade without
/// HTML's legacy attribute mappings.
/// <https://html.spec.whatwg.org/multipage/rendering.html#rendering>
fn document_is_xhtml(root: &dom::Node) -> bool {
    const XHTML_NAMESPACE_URL: &str = "http://www.w3.org/1999/xhtml";

    let dom::NodeKind::Element(document) = &root.kind else {
        return false;
    };
    document.children.iter().any(|child| {
        matches!(
            &child.kind,
            dom::NodeKind::Element(element)
                if element.tag == "html" && element.namespace_url == XHTML_NAMESPACE_URL
        )
    })
}

fn document_keywords(root: &dom::Node) -> Vec<String> {
    let mut keywords = Vec::new();
    for content in dom::meta_contents(root, "keywords") {
        for keyword in content
            .split(',')
            .map(|keyword| keyword.trim_matches(is_html_space))
            .filter(|keyword| !keyword.is_empty())
        {
            if !keywords.iter().any(|existing| existing == keyword) {
                keywords.push(keyword.to_string());
            }
        }
    }
    keywords
}

fn metadata_date(root: &dom::Node, name: &str) -> Option<DocumentDate> {
    dom::meta_contents(root, name)
        .into_iter()
        .find_map(|content| match DocumentDate::parse(content.clone()) {
            Some(date) => Some(date),
            None => {
                log::warn!("invalid date in <meta name={name:?}>: {content:?}");
                None
            }
        })
}

fn is_html_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\u{000C}' | '\r')
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

/// Select the static image source before visual-resource discovery.
///
/// A `<picture>` contributes its first applicable direct `<source>` before
/// the fallback `<img>`. The selected candidate is stored on the image so
/// preloading, resource availability, intrinsic sizing, and painting cannot
/// accidentally make separate source choices.
/// <https://html.spec.whatwg.org/multipage/images.html#the-picture-element>
fn resolve_html_image_sources(node: &mut dom::Node, media_environment: &css::MediaEnvironment) {
    let dom::NodeKind::Element(element) = &mut node.kind else {
        return;
    };

    if has_html_picture_semantics(element) {
        let mut selected = None;
        let mut image_index = None;
        for (index, child) in element.children.iter().enumerate() {
            let dom::NodeKind::Element(child) = &child.kind else {
                continue;
            };
            if has_html_img_rendering_semantics(child) {
                image_index = Some(index);
                break;
            }
            if selected.is_none() && has_html_source_semantics(child) {
                selected = picture_source_candidate(child, media_environment);
            }
        }
        if let Some(index) = image_index
            && let Some(image) = element.children[index].as_element_mut()
        {
            image.selected_image_source = selected
                .or_else(|| dom::selected_image_source_from_attributes(image, media_environment));
        }
    } else if has_html_img_rendering_semantics(element) && element.selected_image_source.is_none() {
        element.selected_image_source =
            dom::selected_image_source_from_attributes(element, media_environment);
    }

    for child in &mut element.children {
        resolve_html_image_sources(child, media_environment);
    }
}

fn has_html_picture_semantics(element: &dom::Element) -> bool {
    element.tag == "picture"
        && (element.namespace_url == "http://www.w3.org/1999/xhtml"
            || (element.document_syntax == dom::DocumentSyntax::Html
                && element.namespace_url.is_empty()))
}

fn has_html_source_semantics(element: &dom::Element) -> bool {
    element.tag == "source"
        && (element.namespace_url == "http://www.w3.org/1999/xhtml"
            || (element.document_syntax == dom::DocumentSyntax::Html
                && element.namespace_url.is_empty()))
}

fn picture_source_candidate(
    source: &dom::Element,
    media_environment: &css::MediaEnvironment,
) -> Option<dom::SelectedImageSource> {
    if source
        .attrs
        .get("media")
        .is_some_and(|media| !css::media_rule_applies_in_environment(media, media_environment))
    {
        return None;
    }
    if source.attrs.get("type").is_some_and(|mime_type| {
        !mime_type.trim().is_empty()
            && !crate::image_store::supports_html_source_image_mime_type(mime_type)
    }) {
        return None;
    }
    let mut selected = dom::selected_source_set_candidate(
        None,
        source.attrs.get("srcset").map(String::as_str),
        source.attrs.get("sizes").map(String::as_str),
        media_environment,
    )?;
    let dimensions = dom::ImageDimensionAttributes {
        width: source.attrs.get("width").cloned(),
        height: source.attrs.get("height").cloned(),
    };
    if dimensions.width.is_some() || dimensions.height.is_some() {
        selected = selected.with_dimensions(dimensions);
    }
    Some(selected)
}

/// Resolve the static representation selected for HTML `<object>` and `<img>`.
///
/// In a live browser this selection can change while the resource fetch is in
/// flight. Paged output has one deterministic layout pass, so Quire selects
/// the external representation only when a preloaded resource can be decoded
/// as an image that this renderer can paint; every other outcome exposes the
/// element's fallback subtree to the CSS formatting model.
/// <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-object-element>
fn resolve_embedded_rendering_states(
    node: &mut dom::Node,
    base_url: Option<&Url>,
    root_url: Option<&Url>,
    resource_cache: &resource::ResourceCache,
) {
    let dom::NodeKind::Element(element) = &mut node.kind else {
        return;
    };
    for child in &mut element.children {
        resolve_embedded_rendering_states(child, base_url, root_url, resource_cache);
    }
    if has_html_object_rendering_semantics(element) {
        element.object_rendering =
            if object_has_supported_static_image(element, base_url, root_url, resource_cache) {
                dom::ObjectRendering::Image
            } else {
                dom::ObjectRendering::Fallback
            };
    }
    if has_html_img_rendering_semantics(element) {
        element.image_rendering = if crate::layout::asset_helpers::static_html_img_is_available(
            element,
            base_url,
            root_url,
            resource_cache,
        ) {
            dom::ImageRendering::Image
        } else if element.attrs.get("alt").is_some_and(|alt| !alt.is_empty()) {
            dom::ImageRendering::AltText
        } else {
            dom::ImageRendering::Empty
        };
        // HTML `<img>` source children are ignored. In the stable failed-image
        // state, the alternative text is the only fallback subtree exposed to
        // CSS layout.
        element.children = match element.image_rendering {
            dom::ImageRendering::AltText => {
                vec![dom::Node::text(element.attrs.get("alt").expect(
                    "alternative-text state requires a non-empty alt attribute",
                ))]
            }
            dom::ImageRendering::Image | dom::ImageRendering::Empty => Vec::new(),
        };
    }
}

fn has_html_object_rendering_semantics(element: &dom::Element) -> bool {
    element.tag == "object"
        && (element.namespace_url == "http://www.w3.org/1999/xhtml"
            || (element.document_syntax == dom::DocumentSyntax::Html
                && element.namespace_url.is_empty()))
}

fn has_html_img_rendering_semantics(element: &dom::Element) -> bool {
    element.tag == "img"
        && (element.namespace_url == "http://www.w3.org/1999/xhtml"
            || (element.document_syntax == dom::DocumentSyntax::Html
                && element.namespace_url.is_empty()))
}

/// Return whether the object can use Quire's existing static replaced-image
/// renderer. A declared unsupported MIME type selects fallback before resource
/// decoding, matching HTML's permitted early fallback for unsupported types.
/// <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-object-element>
fn object_has_supported_static_image(
    element: &dom::Element,
    base_url: Option<&Url>,
    root_url: Option<&Url>,
    resource_cache: &resource::ResourceCache,
) -> bool {
    if element
        .attrs
        .get("type")
        .is_some_and(|mime_type| !crate::image_store::supports_declared_image_mime_type(mime_type))
    {
        return false;
    }
    let Some(data) = element
        .attrs
        .get("data")
        .map(String::as_str)
        .map(str::trim)
        .filter(|data| !data.is_empty())
    else {
        return false;
    };
    if data.starts_with("data:") {
        return resource_cache
            .data_image_asset_with_orientation(
                data,
                crate::image_store::RasterOrientationPolicy::Encoded,
                crate::svg::SvgImageContext::default(),
            )
            .is_some();
    }
    resource::resolve_url(data, base_url, root_url)
        .and_then(|url| {
            resource_cache.image_asset_url_with_orientation(
                &url,
                crate::image_store::RasterOrientationPolicy::Encoded,
                crate::svg::SvgImageContext::default(),
            )
        })
        .is_some()
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
    if element.tag == "img"
        && let Some((src, _density)) = dom::selected_img_source(element)
        && let Some(path) = resource::resolve_fetchable_url(src, base_url, root_url)
    {
        paths.push(path);
    }
    if matches!(element.tag.as_str(), "input" | "video")
        && let Some(src) = element
            .attrs
            .get("src")
            .or_else(|| element.attrs.get("poster"))
        && let Some(path) = resource::resolve_fetchable_url(src, base_url, root_url)
    {
        paths.push(path);
    }
    if element.tag == "object"
        && let Some(src) = element
            .attrs
            .get("data")
            .map(String::as_str)
            .map(str::trim)
            .filter(|data| !data.is_empty())
        && let Some(path) = resource::resolve_fetchable_url(src, base_url, root_url)
    {
        paths.push(path);
    }
    if element.tag == "embed"
        && let Some(src) = element.attrs.get("src")
        && let Some(path) = resource::resolve_fetchable_url(src, base_url, root_url)
    {
        paths.push(path);
    }
    // External SVG `<use>` references are visual subresources. Preload the
    // document before SVG scene construction; the SVG adapter expands only
    // same-origin cached targets and never performs parser-time I/O.
    if element.namespace_url == "http://www.w3.org/2000/svg"
        && element.tag == "use"
        && let Some(href) = element.attrs.get("href")
        && !href.starts_with('#')
        && let Some(path) = resource::resolve_fetchable_url(href, base_url, root_url)
        && path.fragment().is_none()
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

struct IframeSourceRequest {
    element_id: dom::ElementId,
    source: IframeSource,
    container_body_margins: css::HtmlContainerFrameBodyMargins,
}

fn collect_iframe_sources(node: &dom::Node, sources: &mut Vec<IframeSourceRequest>) {
    let dom::NodeKind::Element(element) = &node.kind else {
        return;
    };
    if element.tag.eq_ignore_ascii_case("iframe") {
        let container_body_margins =
            css::HtmlContainerFrameBodyMargins::from_iframe_attributes(&element.attrs);
        // `srcdoc` takes precedence even if its value is the empty string.
        // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#attr-iframe-srcdoc>
        if let Some(source) = element.attrs.get("srcdoc") {
            sources.push(IframeSourceRequest {
                element_id: element.id,
                source: IframeSource::Srcdoc(source.clone()),
                container_body_margins,
            });
        } else if let Some(source) = element.attrs.get("src") {
            sources.push(IframeSourceRequest {
                element_id: element.id,
                source: IframeSource::Url(source.clone()),
                container_body_margins,
            });
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
    // Preserve ordinary style text byte-for-byte. In particular, the newline
    // after an unterminated quote is a CSS Syntax BadString boundary; trimming
    // it would incorrectly turn the value into an EOF-closed string.
    let cdata_candidate = source.trim();
    let unwrapped = cdata_candidate
        .strip_prefix("<![CDATA[")
        .and_then(|value| value.strip_suffix("]]>"))
        .unwrap_or(source);
    unwrapped.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CssColor, FetchErrorPolicy, ResourcePolicy, dom};

    #[test]
    fn embedded_style_text_preserves_a_bad_string_newline() {
        let source = "\ncolor: var(--tone, \"\n";
        assert_eq!(embedded_style_css(source), source);
    }

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

    #[test]
    fn static_image_state_exposes_only_non_empty_alt_text_as_fallback_content() {
        fn image_node(src: Option<&str>, alt: Option<&str>) -> dom::Node {
            let mut image = dom::Node::element("img");
            {
                let element = image.as_element_mut().expect("image element");
                if let Some(src) = src {
                    element.attrs.insert("src".to_string(), src.to_string());
                }
                if let Some(alt) = alt {
                    element.attrs.insert("alt".to_string(), alt.to_string());
                }
            }
            image
        }

        let mut root = dom::Node::element("body");
        root.as_element_mut()
            .expect("body element")
            .children
            .extend([
                image_node(Some("about:invalid"), Some("fallback text")),
                image_node(Some("about:invalid"), Some("")),
                image_node(Some("about:invalid"), None),
            ]);
        resolve_embedded_rendering_states(
            &mut root,
            None,
            None,
            &resource::ResourceCache::default(),
        );

        let dom::NodeKind::Element(root) = root.kind else {
            panic!("body element");
        };
        let images = root
            .children
            .iter()
            .map(|node| match &node.kind {
                dom::NodeKind::Element(element) => element,
                dom::NodeKind::Text(_) => panic!("expected image element"),
            })
            .collect::<Vec<_>>();
        assert_eq!(images[0].image_rendering, dom::ImageRendering::AltText);
        assert!(matches!(images[0].children.as_slice(), [dom::Node {
            kind: dom::NodeKind::Text(text)
        }] if text == "fallback text"));
        assert_eq!(images[1].image_rendering, dom::ImageRendering::Empty);
        assert!(images[1].children.is_empty());
        assert_eq!(images[2].image_rendering, dom::ImageRendering::Empty);
        assert!(images[2].children.is_empty());
    }

    #[test]
    fn static_image_state_keeps_decoded_data_images_replaced() {
        let mut image = dom::Node::element("img");
        image
            .as_element_mut()
            .expect("image element")
            .attrs
            .insert(
                "src".to_string(),
                "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC".to_string(),
            );
        resolve_embedded_rendering_states(
            &mut image,
            None,
            None,
            &resource::ResourceCache::default(),
        );

        assert_eq!(
            image.as_element().expect("image element").image_rendering,
            dom::ImageRendering::Image
        );
    }

    #[test]
    fn picture_selects_the_first_applicable_direct_source_before_preloading() {
        fn element(tag: &str, attrs: &[(&str, &str)]) -> dom::Node {
            let mut node = dom::Node::element(tag);
            let element = node.as_element_mut().expect("element");
            for (name, value) in attrs {
                element
                    .attrs
                    .insert((*name).to_string(), (*value).to_string());
            }
            node
        }

        let mut picture = element("picture", &[]);
        picture.as_element_mut().expect("picture").children = vec![
            element("source", &[("media", "screen"), ("srcset", "screen.png")]),
            element(
                "source",
                &[
                    ("type", "image/not-supported"),
                    ("srcset", "unsupported.png"),
                ],
            ),
            element(
                "source",
                &[("srcset", "small.png 0.5x, selected.png 1x, large.png 2x")],
            ),
            element("img", &[("src", "fallback.png")]),
        ];

        resolve_html_image_sources(&mut picture, &css::MediaEnvironment::default());
        let image = picture.as_element().expect("picture").children[3]
            .as_element()
            .expect("image");
        assert_eq!(
            image.selected_image_source.as_ref().map(|source| (
                source.url.as_str(),
                dom::image_density_order(source.density)
            )),
            Some(("selected.png", 1.0))
        );
    }

    #[test]
    fn picture_source_type_uses_recoverable_html_mime_parsing() {
        fn selected_url(type_: &str) -> Option<String> {
            let mut picture = dom::Node::element("picture");
            let mut source = dom::Node::element("source");
            {
                let source = source.as_element_mut().expect("source element");
                source
                    .attrs
                    .insert("srcset".to_string(), "source.png".to_string());
                source.attrs.insert("type".to_string(), type_.to_string());
            }
            let mut image = dom::Node::element("img");
            image
                .as_element_mut()
                .expect("image element")
                .attrs
                .insert("src".to_string(), "fallback.png".to_string());
            picture.as_element_mut().expect("picture element").children = vec![source, image];

            resolve_html_image_sources(&mut picture, &css::MediaEnvironment::default());
            picture.as_element().expect("picture element").children[1]
                .as_element()
                .expect("image element")
                .selected_image_source
                .as_ref()
                .map(|source| source.url.clone())
        }

        for accepted in [
            "",
            " ",
            " Image/GIF ",
            "image/gif;",
            "image/gif;encodings",
            "image/gif;encodings=",
            "image/gif;encodings=foobar",
        ] {
            assert_eq!(
                selected_url(accepted).as_deref(),
                Some("source.png"),
                "{accepted}"
            );
        }
        for rejected in [
            "image\\gif",
            "gif",
            "*/*",
            "image/*",
            "image/gif, image/png",
            "image/gif image/png",
            "text/plain",
            "image/not-supported",
        ] {
            assert_eq!(
                selected_url(rejected).as_deref(),
                Some("fallback.png"),
                "{rejected}"
            );
        }
    }

    #[test]
    fn picture_falls_back_to_the_img_candidate_when_no_source_applies() {
        fn element(tag: &str, attrs: &[(&str, &str)]) -> dom::Node {
            let mut node = dom::Node::element(tag);
            let element = node.as_element_mut().expect("element");
            for (name, value) in attrs {
                element
                    .attrs
                    .insert((*name).to_string(), (*value).to_string());
            }
            node
        }

        let mut picture = element("picture", &[]);
        picture.as_element_mut().expect("picture").children = vec![
            element("source", &[("media", "screen"), ("srcset", "screen.png")]),
            element(
                "img",
                &[("srcset", "low.png 0.5x, normal.png 1x, high.png 2x")],
            ),
        ];

        resolve_html_image_sources(&mut picture, &css::MediaEnvironment::default());
        let image = picture.as_element().expect("picture").children[1]
            .as_element()
            .expect("image");
        assert_eq!(
            image.selected_image_source.as_ref().map(|source| (
                source.url.as_str(),
                dom::image_density_order(source.density)
            )),
            Some(("normal.png", 1.0))
        );
    }

    #[test]
    fn picture_source_dimensions_replace_img_dimension_attributes() {
        fn element(tag: &str, attrs: &[(&str, &str)]) -> dom::Node {
            let mut node = dom::Node::element(tag);
            let element = node.as_element_mut().expect("element");
            for (name, value) in attrs {
                element
                    .attrs
                    .insert((*name).to_string(), (*value).to_string());
            }
            node
        }

        let mut picture = element("picture", &[]);
        picture.as_element_mut().expect("picture").children = vec![
            element("source", &[("srcset", "selected.png"), ("width", "200")]),
            element(
                "img",
                &[("src", "fallback.png"), ("width", "100"), ("height", "50")],
            ),
        ];

        resolve_html_image_sources(&mut picture, &css::MediaEnvironment::default());
        let image = picture.as_element().expect("picture").children[1]
            .as_element()
            .expect("image");
        assert_eq!(
            image.selected_image_dimensions(),
            Some(&dom::ImageDimensionAttributes {
                width: Some("200".to_string()),
                height: None,
            })
        );
    }

    #[tokio::test]
    async fn failed_image_alt_text_uses_normal_text_layout_and_overflow() {
        let document = Html::from_string(
            r#"<style>
                @page { size: 240px 160px; margin: 0 }
                body { margin: 0 }
                img {
                    border: solid;
                    display: block;
                    width: 150px;
                    padding: 10px;
                    overflow: scroll;
                }
            </style>
            <img src="about:invalid" alt="I have scrollbar ................................................................">"#,
        )
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let text = document.pages[0]
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("I have scrollbar"), "lines={text:?}");
        assert!(document.pages[0].images().is_empty());

        let mut pdf = Vec::new();
        document
            .write_pdf(&mut pdf, &PdfOptions::default())
            .unwrap();
        let pdf = String::from_utf8_lossy(&pdf);
        assert!(
            pdf.contains("/ToUnicode"),
            "fallback text must remain extractable"
        );
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
    async fn linked_data_stylesheet_with_default_text_plain_mime_is_ignored() {
        let document = Html::from_string(
            "<link rel=\"stylesheet\" href=\"data:,p%7Bcolor%3Ared%7D\"><p>Text</p>",
        )
        .render(&RenderOptions::default())
        .await
        .unwrap();

        assert_eq!(document.pages[0].lines()[0].color, CssColor::BLACK);
    }

    #[tokio::test]
    async fn linked_text_css_data_stylesheet_applies_percent_encoded_source() {
        let document = Html::from_string(
            "<link rel=\"stylesheet\" href=\"data:text/css,p%7Bcolor%3Ared%7D\"><p>Text</p>",
        )
        .render(&RenderOptions::default())
        .await
        .unwrap();

        assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
    }

    #[tokio::test]
    async fn linked_text_css_data_stylesheet_applies_base64_source() {
        let document = Html::from_string(
            "<link rel=\"stylesheet\" href=\"data:text/css;base64,cCB7IGNvbG9yOiByZWQgfQ==\"><p>Text</p>",
        )
        .render(&RenderOptions::default())
        .await
        .unwrap();

        assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
    }

    #[tokio::test]
    async fn noscript_renders_fallback_markup_but_script_contents_remain_hidden() {
        let document = Html::from_string(
            "<script>hidden script content</script><noscript><span>fallback content</span></noscript>",
        )
        .render(&RenderOptions::default())
        .await
        .unwrap();
        let lines = document.pages[0].lines();

        assert!(lines.iter().any(|line| line.text == "fallback content"));
        assert!(
            !lines
                .iter()
                .any(|line| line.text.contains("hidden script content"))
        );
    }

    #[tokio::test]
    async fn head_noscript_style_applies_to_fallback_content() {
        let document = Html::from_string(
            "<head><noscript><style>span { color: green }</style></noscript></head>\
             <body><noscript><span>fallback content</span></noscript></body>",
        )
        .render(&RenderOptions::default())
        .await
        .unwrap();
        let fallback = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == "fallback content")
            .expect("noscript fallback content should render");

        assert_eq!(fallback.color, CssColor::new(0, 128, 0));
    }

    #[tokio::test]
    async fn imported_text_css_data_stylesheet_applies() {
        let document = Html::from_string("<p>Text</p>")
            .with_stylesheet(Css::from_string(
                "@import url(data:text/css,p%7Bcolor%3Ared%7D);",
            ))
            .render(&RenderOptions::default())
            .await
            .unwrap();

        assert_eq!(document.pages[0].lines()[0].color, CssColor::new(255, 0, 0));
    }

    #[tokio::test]
    async fn eof_immediately_after_selector_in_data_stylesheet_does_not_abort_rendering() {
        Html::from_string("<link rel=\"stylesheet\" href=\"data:,a{\">")
            .render(&RenderOptions::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn eof_after_selector_content_in_data_stylesheet_does_not_abort_rendering() {
        Html::from_string("<link rel=\"stylesheet\" href=\"data:text/css,a{xyz\">")
            .render(&RenderOptions::default())
            .await
            .unwrap();
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
            crate::document::paint::images::RenderedImageSource::Stored { .. }
        ));
        let mut pdf = Vec::new();
        assert!(
            document
                .write_pdf(&mut pdf, &crate::PdfOptions::default())
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
        let mut pdf = Vec::new();
        document
            .write_pdf(&mut pdf, &crate::PdfOptions::default())
            .unwrap();
        assert!(!pdf.is_empty());
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
    async fn xml_rendering_does_not_apply_html_presentational_hints() {
        let document = Html::from_xml_string(
            "<html><body marginwidth=\"100\"><p style=\"margin: 0; font-size: 10pt; line-height: 10pt\">Text</p></body></html>",
        )
        .with_stylesheet(Css::from_string("@page { size: 160pt 100pt; margin: 10pt }"))
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let text = document.pages[0]
            .lines()
            .iter()
            .find(|line| line.text == "Text")
            .expect("the XML paragraph should render");
        assert_eq!(text.x(), 16.0);
    }

    #[tokio::test]
    async fn xhtml_rendering_applies_image_dimension_presentational_hints() {
        let document = Html::from_xml_string(
            "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><img width=\"100\" height=\"50\" src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\" /></body></html>",
        )
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let image = &document.pages[0].images()[0];
        assert!((image.width() - 75.0).abs() < 0.001);
        assert!((image.height() - 37.5).abs() < 0.001);
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
