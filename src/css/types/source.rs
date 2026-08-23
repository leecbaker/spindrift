use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::pin::Pin;

use cssparser::{
    AtRuleParser, BasicParseErrorKind, Parser, ParserInput, ParserState, StyleSheetParser, Token,
};
use url::Url;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
/// A stylesheet source and its resource-loading context.
///
/// ```no_run
/// use quire::{Css, Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let stylesheet = Css::from_file("print.css").await?.with_user_origin();
/// let html = Html::from_file("report.html")
///     .await?
///     .with_stylesheet(stylesheet);
/// let mut output = File::create("report.pdf")?;
/// html.write_pdf(&mut output, &RenderOptions::default(), &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct Css {
    source: String,
    origin: StylesheetOrigin,
    base_url: Option<Url>,
    root_url: Option<Url>,
    layer_order_prefix: Vec<LayerName>,
    import_layer_name: Option<LayerName>,
    /// The stylesheet selector context used by `:scope` and CSS Nesting's
    /// top-level `&`. This is distinct from the implicit `@scope` anchor.
    selector_scope_anchor: StylesheetScopeAnchor,
    scope_anchor: StylesheetScopeAnchor,
    specificity_override: Option<u32>,
    resource_policy: crate::ResourcePolicy,
}

impl Css {
    /// Creates an author-origin stylesheet from CSS source text.
    ///
    /// ```
    /// use quire::Css;
    ///
    /// let stylesheet = Css::from_string("article { margin: 1in }");
    /// ```
    pub fn from_string(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            origin: StylesheetOrigin::Author,
            base_url: None,
            root_url: None,
            layer_order_prefix: Vec::new(),
            import_layer_name: None,
            selector_scope_anchor: StylesheetScopeAnchor::DocumentRoot,
            scope_anchor: StylesheetScopeAnchor::DocumentRoot,
            specificity_override: None,
            resource_policy: crate::ResourcePolicy::default(),
        }
    }

    /// Asynchronously loads an author-origin stylesheet from a local file.
    ///
    /// ```no_run
    /// # async fn load() -> quire::Result<()> {
    /// let stylesheet = quire::Css::from_file("styles.css").await?;
    /// # let _ = stylesheet;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn from_file<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        let path = path.as_ref();
        log::debug!("reading CSS file {}", path.display());
        let url = crate::resource::file_url_from_path(path)?;
        Ok(Self {
            source: crate::resource::read_to_string(&url).await?,
            origin: StylesheetOrigin::Author,
            base_url: Some(url),
            root_url: None,
            layer_order_prefix: Vec::new(),
            import_layer_name: None,
            selector_scope_anchor: StylesheetScopeAnchor::DocumentRoot,
            scope_anchor: StylesheetScopeAnchor::DocumentRoot,
            specificity_override: None,
            resource_policy: crate::ResourcePolicy::default(),
        })
    }

    /// Loads an author stylesheet from a local-file, HTTP(S), or `data:` URL.
    ///
    /// ```no_run
    /// # async fn load() -> Result<(), Box<dyn std::error::Error>> {
    /// let url = "https://example.test/styles.css".parse()?;
    /// let stylesheet = quire::Css::from_url(url).await?;
    /// # let _ = stylesheet;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_url(url: Url) -> crate::Result<Self> {
        Self::from_url_with_resource_policy(url, crate::ResourcePolicy::default()).await
    }

    /// Loads an author stylesheet from a URL with an explicit resource policy.
    ///
    /// ```no_run
    /// # async fn load() -> Result<(), Box<dyn std::error::Error>> {
    /// let url = "https://example.test/styles.css".parse()?;
    /// let policy = quire::ResourcePolicy {
    ///     follow_http_redirects: false,
    ///     ..Default::default()
    /// };
    /// let stylesheet = quire::Css::from_url_with_resource_policy(url, policy).await?;
    /// # let _ = stylesheet;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_url_with_resource_policy(
        url: Url,
        resource_policy: crate::ResourcePolicy,
    ) -> crate::Result<Self> {
        let fetcher = crate::resource::ResourceFetcher::new(resource_policy)?;
        Self::from_url_with_fetcher(url, &fetcher).await
    }

    pub(crate) async fn from_url_with_fetcher(
        url: Url,
        fetcher: &crate::resource::ResourceFetcher,
    ) -> crate::Result<Self> {
        let (stylesheet, _) = Self::load_url_with_fetcher(url, fetcher).await?;
        Ok(stylesheet)
    }

    /// Loads a stylesheet that was referenced by HTML or CSS syntax.
    ///
    /// A `data:` URL has explicit response MIME metadata. HTML linked
    /// stylesheet processing accepts it only when that metadata is `text/css`;
    /// a different MIME type is a successfully fetched but inapplicable style
    /// resource, not a document-fetch failure.
    /// <https://html.spec.whatwg.org/multipage/links.html#link-type-stylesheet>
    pub(crate) async fn from_link_url_with_fetcher(
        url: Url,
        fetcher: &crate::resource::ResourceFetcher,
    ) -> crate::Result<Option<Self>> {
        let is_data_url = url.scheme() == "data";
        let (stylesheet, content_type) = Self::load_url_with_fetcher(url, fetcher).await?;
        if is_data_url && !content_type.as_deref().is_some_and(is_css_mime_type) {
            log::debug!(
                "ignoring data stylesheet {} with non-CSS MIME type {:?}",
                stylesheet
                    .base_url()
                    .expect("loaded stylesheet has a base URL"),
                content_type
            );
            return Ok(None);
        }
        Ok(Some(stylesheet))
    }

    async fn load_url_with_fetcher(
        url: Url,
        fetcher: &crate::resource::ResourceFetcher,
    ) -> crate::Result<(Self, Option<String>)> {
        if crate::resource::fetch_url(&url).is_none() {
            return Err(crate::Error::InvalidInput(format!(
                "unsupported URL for stylesheet input: {url}"
            )));
        }
        let fetched = fetcher.fetch(&url).await?;
        let source = String::from_utf8(fetched.bytes).map_err(|error| {
            crate::Error::InvalidInput(format!(
                "resource {} is not UTF-8: {error}",
                fetched.final_url
            ))
        })?;
        let content_type = fetched.content_type;
        Ok((
            Self {
                source,
                origin: StylesheetOrigin::Author,
                base_url: Some(fetched.final_url),
                root_url: None,
                layer_order_prefix: Vec::new(),
                import_layer_name: None,
                selector_scope_anchor: StylesheetScopeAnchor::DocumentRoot,
                scope_anchor: StylesheetScopeAnchor::DocumentRoot,
                specificity_override: None,
                resource_policy: fetcher.policy(),
            },
            content_type,
        ))
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    /// Marks this stylesheet as a user-origin stylesheet.
    ///
    /// CSS Cascade Level 5 sorts normal user-origin declarations below normal
    /// author declarations, while user `!important` declarations outrank
    /// author `!important` declarations:
    /// <https://www.w3.org/TR/css-cascade-5/#cascade-origin>.
    ///
    /// ```
    /// let stylesheet = quire::Css::from_string("p { color: navy }").with_user_origin();
    /// ```
    pub fn with_user_origin(mut self) -> Self {
        self.origin = StylesheetOrigin::User;
        self
    }

    /// Marks this stylesheet as an author-origin stylesheet.
    ///
    /// Author origin is the default for styles supplied by the document or the
    /// public stylesheet API:
    /// <https://www.w3.org/TR/css-cascade-5/#cascade-origin>.
    pub(crate) fn with_author_origin(mut self) -> Self {
        self.origin = StylesheetOrigin::Author;
        self
    }

    /// Marks this stylesheet as a user-agent-origin stylesheet.
    ///
    /// This is primarily useful for tests and custom embedding; Quire's
    /// built-in HTML defaults are already loaded as UA-origin rules:
    /// <https://www.w3.org/TR/css-cascade-5/#cascade-origin>.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn with_user_agent_origin(mut self) -> Self {
        self.origin = StylesheetOrigin::UserAgent;
        self
    }

    /// Sets the policy used to load this stylesheet's `@import` resources.
    ///
    /// ```
    /// let stylesheet = quire::Css::from_string("@import url(theme.css);")
    ///     .with_resource_policy(quire::ResourcePolicy::default());
    /// ```
    pub fn with_resource_policy(mut self, resource_policy: crate::ResourcePolicy) -> Self {
        self.resource_policy = resource_policy;
        self
    }

    pub(crate) fn origin(&self) -> StylesheetOrigin {
        self.origin
    }

    /// Overrides selector specificity for every rule in this stylesheet.
    ///
    /// HTML presentational hints participate in the author origin with zero
    /// specificity, so normal author CSS can override them as specified by
    /// HTML's rendering model and CSS Cascade:
    /// <https://html.spec.whatwg.org/multipage/rendering.html#presentational-hints>
    /// and <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
    pub(crate) fn with_specificity_override(mut self, specificity: u32) -> Self {
        self.specificity_override = Some(specificity);
        self
    }

    pub(crate) fn with_base_url(mut self, base_url: Option<Url>) -> Self {
        self.base_url = base_url;
        self
    }

    /// Sets a local directory as the base URL for stylesheet-relative resources.
    ///
    /// CSS resolves relative URLs against the stylesheet's base URL:
    /// <https://www.w3.org/TR/css-values-4/#urls>.
    ///
    /// ```no_run
    /// # fn configure() -> quire::Result<()> {
    /// let stylesheet = quire::Css::from_string("img { content: url(icon.svg) }")
    ///     .with_base_path("assets")?;
    /// # let _ = stylesheet;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_base_path<P: AsRef<Path>>(mut self, base_path: P) -> crate::Result<Self> {
        self.base_url = Some(crate::resource::directory_url_from_path(
            base_path.as_ref(),
        )?);
        Ok(self)
    }

    pub(crate) fn with_root_url(mut self, root_url: Option<Url>) -> Self {
        self.root_url = root_url;
        self
    }

    pub(crate) fn base_url(&self) -> Option<&Url> {
        self.base_url.as_ref()
    }

    pub(crate) fn root_url(&self) -> Option<&Url> {
        self.root_url.as_ref()
    }

    pub(crate) fn layer_order_prefix(&self) -> &[LayerName] {
        &self.layer_order_prefix
    }

    pub(crate) fn import_layer_name(&self) -> Option<&LayerName> {
        self.import_layer_name.as_ref()
    }

    pub(crate) fn scope_anchor(&self) -> StylesheetScopeAnchor {
        self.scope_anchor
    }

    pub(crate) fn selector_scope_anchor(&self) -> StylesheetScopeAnchor {
        self.selector_scope_anchor
    }

    pub(crate) fn with_selector_scope_anchor(
        mut self,
        scope_anchor: StylesheetScopeAnchor,
    ) -> Self {
        self.selector_scope_anchor = scope_anchor;
        self
    }

    pub(crate) fn with_scope_anchor(mut self, scope_anchor: StylesheetScopeAnchor) -> Self {
        self.scope_anchor = scope_anchor;
        self
    }

    pub(crate) fn specificity_override(&self) -> Option<u32> {
        self.specificity_override
    }

    fn with_layer_context(
        mut self,
        layer_order_prefix: Vec<LayerName>,
        layer_name: Option<LayerName>,
    ) -> Self {
        self.layer_order_prefix = layer_order_prefix;
        self.import_layer_name = layer_name;
        self
    }

    fn with_origin(mut self, origin: StylesheetOrigin) -> Self {
        self.origin = origin;
        self
    }

    pub(crate) async fn with_imports(&self) -> crate::Result<Vec<Self>> {
        let mut stylesheets = Vec::new();
        let mut seen = HashSet::new();
        let fetcher = crate::resource::ResourceFetcher::new(self.resource_policy)?;
        self.collect_with_imports(&fetcher, &mut seen, &mut stylesheets)
            .await?;
        Ok(stylesheets)
    }

    fn collect_with_imports<'a>(
        &'a self,
        fetcher: &'a crate::resource::ResourceFetcher,
        seen: &'a mut HashSet<Url>,
        stylesheets: &'a mut Vec<Self>,
    ) -> Pin<Box<dyn Future<Output = crate::Result<()>> + 'a>> {
        Box::pin(async move {
            for import in imported_stylesheet_rules(
                self.source(),
                self.base_url(),
                self.root_url(),
                self.import_layer_name(),
                self.layer_order_prefix(),
            ) {
                if seen.insert(import.url.clone()) {
                    match Css::from_link_url_with_fetcher(import.url.clone(), fetcher).await {
                        Ok(Some(stylesheet)) => {
                            stylesheet
                                .with_origin(self.origin)
                                .with_root_url(self.root_url.clone())
                                .with_selector_scope_anchor(self.selector_scope_anchor)
                                .with_scope_anchor(self.scope_anchor)
                                .with_layer_context(import.layer_order_prefix, import.layer_name)
                                .collect_with_imports(fetcher, seen, stylesheets)
                                .await?;
                        }
                        Ok(None) => {
                            log::debug!(
                                "ignoring imported stylesheet {} with non-CSS MIME type",
                                import.url
                            );
                        }
                        Err(error) if fetcher.allows_fetch_errors() => {
                            log::debug!(
                                "failed to load imported stylesheet {}: {error}",
                                import.url
                            );
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            stylesheets.push(self.clone());
            Ok(())
        })
    }
}

fn is_css_mime_type(content_type: &str) -> bool {
    content_type
        .split_once(';')
        .map_or(content_type, |(essence, _)| essence)
        .trim()
        .eq_ignore_ascii_case("text/css")
}

#[derive(Debug)]
struct StylesheetImport {
    url: Url,
    layer_name: Option<LayerName>,
    layer_order_prefix: Vec<LayerName>,
}

/// Extracts top-level `@import` rules and their Cascade 5 layer context.
///
/// CSS Cascade Level 5 allows imports to be placed into a cascade layer with
/// `@import url(...) layer(name)` or an anonymous layer with `layer`. The
/// imported stylesheet participates as if its rules appeared at the import
/// location, so this scanner tracks preceding top-level layer declarations as
/// the layer-order prefix for the imported sheet:
/// <https://www.w3.org/TR/css-cascade-5/#layering> and
/// <https://www.w3.org/TR/css-cascade-5/#at-import>.
fn imported_stylesheet_rules(
    source: &str,
    base_url: Option<&Url>,
    root_url: Option<&Url>,
    parent_layer: Option<&LayerName>,
    inherited_layer_prefix: &[LayerName],
) -> Vec<StylesheetImport> {
    let mut imports = Vec::new();
    let mut layer_order = inherited_layer_prefix.to_vec();
    if let Some(parent_layer) = parent_layer {
        push_unique_layer_name(&mut layer_order, parent_layer.clone());
    }

    for at_rule in top_level_at_rules(source) {
        if at_rule.name.eq_ignore_ascii_case("layer") {
            for name in parse_import_layer_name_list(parent_layer, &at_rule.prelude) {
                push_unique_layer_name(&mut layer_order, name);
            }
            continue;
        }
        if !at_rule.name.eq_ignore_ascii_case("import") {
            continue;
        }
        let Some(parsed) = parse_import_prelude(&at_rule.prelude) else {
            continue;
        };
        if !parsed.applies {
            continue;
        }
        let layer_name = match parsed.layer {
            ImportLayer::None => parent_layer.cloned(),
            ImportLayer::Named(name) => Some(qualify_import_layer_name(parent_layer, name)),
            ImportLayer::Anonymous => Some(qualify_import_layer_name(
                parent_layer,
                LayerName::anonymous(),
            )),
        };
        if let Some(layer_name) = &layer_name {
            push_unique_layer_name(&mut layer_order, layer_name.clone());
        }
        if let Some(url) = crate::resource::resolve_fetchable_url(&parsed.url, base_url, root_url) {
            imports.push(StylesheetImport {
                url,
                layer_name,
                layer_order_prefix: layer_order.clone(),
            });
        }
    }

    imports
}

#[derive(Debug)]
struct TopLevelAtRule {
    name: String,
    prelude: String,
}

fn top_level_at_rules(source: &str) -> Vec<TopLevelAtRule> {
    let mut input = ParserInput::new(source);
    let mut css_parser = Parser::new(&mut input);
    let mut parser = ImportRuleCollector;
    StyleSheetParser::new(&mut css_parser, &mut parser)
        .flatten()
        .flatten()
        .collect()
}

struct ImportRuleCollector;

impl<'i> AtRuleParser<'i> for ImportRuleCollector {
    type Prelude = TopLevelAtRule;
    type AtRule = Option<TopLevelAtRule>;
    type Error = BasicParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        name: cssparser::CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        let start = input.position();
        while input.next_including_whitespace_and_comments().is_ok() {}
        Ok(TopLevelAtRule {
            name: name.to_string(),
            prelude: input.slice_from(start).trim().to_string(),
        })
    }

    fn rule_without_block(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
    ) -> Result<Self::AtRule, ()> {
        Ok(Some(prelude))
    }

    fn parse_block<'t>(
        &mut self,
        _prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, cssparser::ParseError<'i, Self::Error>> {
        while input.next_including_whitespace_and_comments().is_ok() {}
        Ok(None)
    }
}

impl<'i> cssparser::QualifiedRuleParser<'i> for ImportRuleCollector {
    type Prelude = ();
    type QualifiedRule = Option<TopLevelAtRule>;
    type Error = BasicParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        while input.next_including_whitespace_and_comments().is_ok() {}
        Ok(())
    }

    fn parse_block<'t>(
        &mut self,
        _prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, cssparser::ParseError<'i, Self::Error>> {
        while input.next_including_whitespace_and_comments().is_ok() {}
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedImportPrelude {
    url: String,
    layer: ImportLayer,
    applies: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImportLayer {
    None,
    Named(LayerName),
    Anonymous,
}

fn parse_import_prelude(prelude: &str) -> Option<ParsedImportPrelude> {
    let mut input = ParserInput::new(prelude);
    let mut parser = Parser::new(&mut input);
    let url = parser.expect_url_or_string().ok()?.to_string();
    let mut layer = ImportLayer::None;
    let mut applies = true;
    let mut saw_layer = false;
    let mut saw_supports = false;
    let mut media_start = None;

    while !parser.is_exhausted() {
        let start = parser.position();
        let Ok(token) = parser.next_including_whitespace_and_comments() else {
            return None;
        };
        match token {
            Token::WhiteSpace(_) | Token::Comment(_) => {}
            Token::Ident(name) if name.eq_ignore_ascii_case("layer") => {
                if saw_layer {
                    return None;
                }
                layer = ImportLayer::Anonymous;
                saw_layer = true;
            }
            Token::Function(name) if name.eq_ignore_ascii_case("layer") => {
                if saw_layer {
                    return None;
                }
                let parsed_layer = parser.parse_nested_block(
                    |input| -> Result<LayerName, cssparser::ParseError<'_, ()>> {
                        parse_layer_name_from_parser(input)
                            .ok_or_else(|| input.new_custom_error(()))
                    },
                );
                layer = ImportLayer::Named(parsed_layer.ok()?);
                saw_layer = true;
            }
            Token::Function(name) if name.eq_ignore_ascii_case("supports") => {
                if saw_supports {
                    return None;
                }
                let condition = parser
                    .parse_nested_block(|input| {
                        let start = input.position();
                        while input.next_including_whitespace_and_comments().is_ok() {}
                        Ok::<_, cssparser::ParseError<'_, ()>>(
                            input.slice_from(start).trim().to_string(),
                        )
                    })
                    .ok()?;
                applies &= import_supports_condition_applies(&condition);
                saw_supports = true;
            }
            _ => {
                media_start = Some(start);
                // `slice_from` ends at the parser's current position. Consume
                // the rest of the prelude before extracting the complete media
                // query list rather than evaluating only its first token.
                while parser.next_including_whitespace_and_comments().is_ok() {}
                break;
            }
        }
    }
    if let Some(media_start) = media_start {
        let media_query_list = parser.slice_from(media_start).trim();
        applies &= crate::css::media_rule_applies(media_query_list);
    }

    Some(ParsedImportPrelude {
        url,
        layer,
        applies,
    })
}

/// Evaluates the `<supports-condition>` carried by an `@import` `supports()`
/// function.
///
/// The function's parentheses themselves delimit a declaration condition, so
/// `supports(display: block)` has the same meaning as the parenthesized
/// `(display: block)` condition in an `@supports` rule. Other condition forms
/// (such as logical and selector conditions) are already accepted directly by
/// the shared evaluator.
/// <https://www.w3.org/TR/css-cascade-5/#at-import>
/// <https://www.w3.org/TR/css-conditional-3/#at-supports>
fn import_supports_condition_applies(condition: &str) -> bool {
    crate::css::supports_condition_applies(condition)
        || crate::css::supports_condition_applies(&format!("({condition})"))
}

fn parse_layer_name_from_parser(input: &mut Parser<'_, '_>) -> Option<LayerName> {
    crate::css::parse::parse_layer_name(input).ok()
}

fn parse_import_layer_name_list(parent_layer: Option<&LayerName>, prelude: &str) -> Vec<LayerName> {
    let mut input = ParserInput::new(prelude);
    let mut parser = Parser::new(&mut input);
    crate::css::parse::parse_layer_name_list(&mut parser)
        .map(|names| {
            names
                .into_iter()
                .map(|name| qualify_import_layer_name(parent_layer, name))
                .collect()
        })
        .unwrap_or_default()
}

fn qualify_import_layer_name(parent_layer: Option<&LayerName>, name: LayerName) -> LayerName {
    parent_layer.map_or(name.clone(), |parent| parent.nested(name))
}

fn push_unique_layer_name(layer_order: &mut Vec<LayerName>, name: LayerName) {
    if !layer_order.iter().any(|existing| existing == &name) {
        layer_order.push(name);
    }
}
