use super::*;
use cssparser::{Parser, ParserInput, Token};
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::pin::Pin;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
/// A stylesheet source and its resource-loading context.
///
/// ```
/// use quire::Css;
///
/// let stylesheet = Css::from_string("p { color: navy }");
/// ```
pub struct Css {
    source: String,
    origin: StylesheetOrigin,
    base_url: Option<Url>,
    root_url: Option<Url>,
    layer_order_prefix: Vec<String>,
    import_layer_name: Option<String>,
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

    pub(crate) fn layer_order_prefix(&self) -> &[String] {
        &self.layer_order_prefix
    }

    pub(crate) fn import_layer_name(&self) -> Option<&str> {
        self.import_layer_name.as_deref()
    }

    pub(crate) fn specificity_override(&self) -> Option<u32> {
        self.specificity_override
    }

    fn with_layer_context(
        mut self,
        layer_order_prefix: Vec<String>,
        layer_name: Option<String>,
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
    layer_name: Option<String>,
    layer_order_prefix: Vec<String>,
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
    parent_layer: Option<&str>,
    inherited_layer_prefix: &[String],
) -> Vec<StylesheetImport> {
    let mut imports = Vec::new();
    let mut layer_order = inherited_layer_prefix.to_vec();
    if let Some(parent_layer) = parent_layer {
        push_unique_layer_name(&mut layer_order, parent_layer.to_string());
    }
    let mut anonymous_import_count = 0usize;

    for at_rule in top_level_at_rules(source) {
        if at_rule.name.eq_ignore_ascii_case("layer") {
            for name in parse_import_layer_name_list(parent_layer, at_rule.prelude) {
                push_unique_layer_name(&mut layer_order, name);
            }
            continue;
        }
        if !at_rule.name.eq_ignore_ascii_case("import") {
            continue;
        }
        let Some(parsed) = parse_import_prelude(at_rule.prelude) else {
            continue;
        };
        if !parsed.applies {
            continue;
        }
        let layer_name = match parsed.layer {
            ImportLayer::None => parent_layer.map(ToOwned::to_owned),
            ImportLayer::Named(name) => qualify_import_layer_name(parent_layer, &name),
            ImportLayer::Anonymous => {
                let name = if let Some(parent_layer) = parent_layer {
                    format!("{parent_layer}.__import_anonymous_layer_{anonymous_import_count}")
                } else {
                    format!("__import_anonymous_layer_{anonymous_import_count}")
                };
                anonymous_import_count = anonymous_import_count.saturating_add(1);
                Some(name)
            }
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
struct TopLevelAtRule<'a> {
    name: &'a str,
    prelude: &'a str,
}

fn top_level_at_rules(source: &str) -> Vec<TopLevelAtRule<'_>> {
    let mut rules = Vec::new();
    let mut index = 0usize;
    let bytes = source.as_bytes();
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_css_comment(source, index);
            }
            b'\'' | b'"' => {
                index = skip_css_string(source, index);
            }
            b'{' => {
                index = find_matching_top_level_brace(source, index)
                    .map(|close| close.saturating_add(1))
                    .unwrap_or(bytes.len());
            }
            b'@' => {
                let name_start = index + 1;
                let name_end = scan_css_identifier(source, name_start);
                if name_end == name_start {
                    index += 1;
                    continue;
                }
                let prelude_start = name_end;
                let Some(prelude_end) = find_top_level_at_rule_end(source, prelude_start) else {
                    break;
                };
                rules.push(TopLevelAtRule {
                    name: &source[name_start..name_end],
                    prelude: source[prelude_start..prelude_end].trim(),
                });
                index = if bytes.get(prelude_end) == Some(&b'{') {
                    find_matching_top_level_brace(source, prelude_end)
                        .map(|close| close.saturating_add(1))
                        .unwrap_or(bytes.len())
                } else {
                    prelude_end.saturating_add(1)
                };
            }
            _ => index += 1,
        }
    }
    rules
}

fn skip_css_comment(source: &str, start: usize) -> usize {
    source[start + 2..]
        .find("*/")
        .map(|offset| start + 2 + offset + 2)
        .unwrap_or(source.len())
}

fn skip_css_whitespace_and_comments(source: &str, mut index: usize) -> usize {
    let bytes = source.as_bytes();
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
        } else if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index = skip_css_comment(source, index);
        } else {
            break;
        }
    }
    index
}

fn skip_css_string(source: &str, start: usize) -> usize {
    let quote = source.as_bytes()[start];
    let mut index = start + 1;
    let bytes = source.as_bytes();
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = index.saturating_add(2);
        } else if bytes[index] == quote {
            return index + 1;
        } else {
            index += 1;
        }
    }
    source.len()
}

fn skip_balanced_parentheses(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = open;
    let bytes = source.as_bytes();
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_css_comment(source, index);
                continue;
            }
            b'\'' | b'"' => {
                index = skip_css_string(source, index);
                continue;
            }
            b'(' => depth = depth.saturating_add(1),
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index.saturating_add(1));
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn find_matching_top_level_brace(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = open;
    let bytes = source.as_bytes();
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_css_comment(source, index);
                continue;
            }
            b'\'' | b'"' => {
                index = skip_css_string(source, index);
                continue;
            }
            b'{' => depth = depth.saturating_add(1),
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn scan_css_identifier(source: &str, start: usize) -> usize {
    source[start..]
        .find(|character: char| {
            !(character == '-' || character == '_' || character.is_ascii_alphanumeric())
        })
        .map(|offset| start + offset)
        .unwrap_or(source.len())
}

fn find_top_level_at_rule_end(source: &str, start: usize) -> Option<usize> {
    let mut index = start;
    let bytes = source.as_bytes();
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_css_comment(source, index);
            }
            b'\'' | b'"' => {
                index = skip_css_string(source, index);
            }
            b';' | b'{' => return Some(index),
            _ => index += 1,
        }
    }
    None
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
    Named(String),
    Anonymous,
}

fn parse_import_prelude(prelude: &str) -> Option<ParsedImportPrelude> {
    let mut input = ParserInput::new(prelude);
    let mut parser = Parser::new(&mut input);
    let url = parser.expect_url_or_string().ok()?.to_string();
    let conditional_prelude = parser.slice_from(parser.position()).trim().to_string();
    let mut layer = ImportLayer::None;
    let mut applies = true;

    while !parser.is_exhausted() {
        let Ok(token) = parser.next_including_whitespace_and_comments() else {
            break;
        };
        match token {
            Token::WhiteSpace(_) | Token::Comment(_) => {}
            Token::Ident(name) if name.eq_ignore_ascii_case("layer") => {
                layer = ImportLayer::Anonymous;
            }
            Token::Function(name) if name.eq_ignore_ascii_case("layer") => {
                let parsed_layer = parser.parse_nested_block(
                    |input| -> Result<String, cssparser::ParseError<'_, ()>> {
                        parse_layer_name_from_parser(input)
                            .ok_or_else(|| input.new_custom_error(()))
                    },
                );
                if let Ok(name) = parsed_layer {
                    layer = ImportLayer::Named(name);
                }
            }
            Token::Function(name) if name.eq_ignore_ascii_case("supports") => {
                let condition = parser
                    .parse_nested_block(|input| {
                        let start = input.position();
                        while !input.is_exhausted() {
                            input.next_including_whitespace_and_comments()?;
                        }
                        Ok::<_, cssparser::ParseError<'_, ()>>(
                            input.slice_from(start).trim().to_string(),
                        )
                    })
                    .ok();
                if let Some(condition) = condition {
                    applies &= crate::css::supports_condition_applies(&condition);
                }
            }
            Token::Ident(_) => {}
            _ => {}
        }
    }
    if let Some(media_query_list) = import_media_query_list(&conditional_prelude) {
        applies &= crate::css::media_rule_applies(media_query_list);
    }

    Some(ParsedImportPrelude {
        url,
        layer,
        applies,
    })
}

/// Extracts the trailing media query list from an `@import` prelude.
///
/// CSS Cascade Level 5 defines `@import` as a URL followed by optional
/// `layer()`/`supports()` conditions and then an optional media query list.
/// The media list is evaluated with the same print-context Media Queries
/// subset used for ordinary `@media` rules:
/// <https://www.w3.org/TR/css-cascade-5/#at-import> and
/// <https://www.w3.org/TR/mediaqueries-4/#mq-list>.
fn import_media_query_list(prelude_after_url: &str) -> Option<&str> {
    let mut index = 0usize;
    while index < prelude_after_url.len() {
        index = skip_css_whitespace_and_comments(prelude_after_url, index);
        if index >= prelude_after_url.len() {
            return None;
        }
        let ident_end = scan_css_identifier(prelude_after_url, index);
        if ident_end == index {
            let media = prelude_after_url[index..].trim();
            return (!media.is_empty()).then_some(media);
        }
        let ident = &prelude_after_url[index..ident_end];
        let after_ident = skip_css_whitespace_and_comments(prelude_after_url, ident_end);
        if ident.eq_ignore_ascii_case("layer") {
            index = if prelude_after_url.as_bytes().get(after_ident) == Some(&b'(') {
                skip_balanced_parentheses(prelude_after_url, after_ident)
                    .unwrap_or(prelude_after_url.len())
            } else {
                ident_end
            };
            continue;
        }
        if ident.eq_ignore_ascii_case("supports")
            && prelude_after_url.as_bytes().get(after_ident) == Some(&b'(')
        {
            index = skip_balanced_parentheses(prelude_after_url, after_ident)
                .unwrap_or(prelude_after_url.len());
            continue;
        }
        let media = prelude_after_url[index..].trim();
        return (!media.is_empty()).then_some(media);
    }
    None
}

fn parse_layer_name_from_parser(input: &mut Parser<'_, '_>) -> Option<String> {
    let first = input.expect_ident().ok()?.to_string();
    let mut name = first;
    while input.try_parse(|input| input.expect_delim('.')).is_ok() {
        let next = input.expect_ident().ok()?;
        name.push('.');
        name.push_str(next);
    }
    input.is_exhausted().then_some(name)
}

fn parse_import_layer_name_list(parent_layer: Option<&str>, prelude: &str) -> Vec<String> {
    prelude
        .split(',')
        .filter_map(|name| qualify_import_layer_name(parent_layer, name))
        .collect()
}

fn qualify_import_layer_name(parent_layer: Option<&str>, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    match parent_layer {
        Some(parent_layer) if !parent_layer.is_empty() => Some(format!("{parent_layer}.{name}")),
        _ => Some(name.to_string()),
    }
}

fn push_unique_layer_name(layer_order: &mut Vec<String>, name: String) {
    if !layer_order.iter().any(|existing| existing == &name) {
        layer_order.push(name);
    }
}
