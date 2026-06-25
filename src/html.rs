use crate::{Css, Document, RenderOptions, Result, css, dom, layout, resource, timing::DebugTimer};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Html {
    source: String,
    base_url: Option<PathBuf>,
    root_url: Option<PathBuf>,
    stylesheets: Vec<Css>,
}

impl Html {
    pub fn from_string(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            base_url: None,
            root_url: None,
            stylesheets: Vec::new(),
        }
    }

    pub async fn from_file_async<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let _timer = DebugTimer::start(format!("reading HTML file {}", path.display()));
        let source = resource::read_to_string(path).await?;
        Ok(Self {
            source,
            base_url: resource::resource_parent(path),
            root_url: None,
            stylesheets: Vec::new(),
        })
    }

    /// Creates an HTML document from a URL.
    ///
    /// HTML file-backed documents use URL-valued stylesheet, image, and font
    /// references. This currently supports local `file://` URLs only, following
    /// the URL Standard's `file` scheme model:
    /// <https://url.spec.whatwg.org/#file-scheme>.
    pub async fn from_url_async(url: impl AsRef<str>) -> Result<Self> {
        let url = url.as_ref();
        if let Some(path) = resource::file_url_to_path(url) {
            return Self::from_file_async(path).await;
        }
        if url.starts_with("http://") {
            let location = PathBuf::from(url);
            let source = resource::read_to_string(&location).await?;
            return Ok(Self {
                source,
                base_url: resource::resource_parent(&location),
                root_url: http_origin(url).map(PathBuf::from),
                stylesheets: Vec::new(),
            });
        }
        Err(crate::Error::InvalidInput(format!(
            "unsupported URL for HTML input: {url}"
        )))
    }

    /// Sets the base URL used to resolve document-relative resources.
    ///
    /// HTML's document base URL is the fallback URL used by relative links and
    /// CSS `url()` references:
    /// <https://html.spec.whatwg.org/multipage/urls-and-fetching.html#document-base-url>.
    /// File-backed documents already have a base from their path, so this only
    /// fills the document base for string-backed input while still setting the
    /// local root used for root-relative file URLs.
    pub fn with_base_url<P: AsRef<Path>>(mut self, base_url: P) -> Self {
        let base_url = base_url.as_ref().to_path_buf();
        if self.base_url.is_none() {
            self.base_url = Some(base_url.clone());
        }
        self.root_url = Some(base_url);
        self
    }

    pub fn with_stylesheet(mut self, stylesheet: Css) -> Self {
        self.stylesheets.push(stylesheet);
        self
    }

    pub async fn render_async(&self, options: &RenderOptions) -> Result<Document> {
        let render_timer = DebugTimer::start("rendering HTML document");
        let font_system_load = layout::start_font_system_load();
        let mut options = options.clone();
        let mut root = {
            let _timer = DebugTimer::start("parsing HTML document");
            dom::parse(&self.source)
        };
        dom::mark_target_fragment(&mut root, options.target_fragment.as_deref());
        let mut stylesheets = {
            let _timer = DebugTimer::start("collecting embedded stylesheets");
            self.embedded_styles()
        };
        let linked_stylesheets = {
            let _timer = DebugTimer::start("loading linked stylesheets");
            self.linked_stylesheets_async(&root).await?
        };
        stylesheets.extend(linked_stylesheets);
        stylesheets.extend(self.stylesheets.iter().cloned());
        let stylesheets = {
            let _timer = DebugTimer::start(format!(
                "resolving imports for {} stylesheet(s)",
                stylesheets.len()
            ));
            resolve_stylesheet_imports_async(stylesheets).await?
        };
        log::debug!("applying {} stylesheet(s)", stylesheets.len());
        {
            let _timer = DebugTimer::start("applying stylesheet options");
            for stylesheet in &stylesheets {
                css::apply_stylesheet_options(stylesheet, &mut options);
            }
        }
        let mut parsed_stylesheets = vec![css::html5_user_agent_stylesheet()];
        if options.presentational_hints {
            parsed_stylesheets.push(css::html5_presentational_hints_stylesheet());
        }
        {
            let _timer = DebugTimer::start(format!(
                "parsing {} author stylesheet(s)",
                stylesheets.len()
            ));
            parsed_stylesheets.extend(stylesheets.iter().map(css::parse_stylesheet));
        }
        let font_system_load = {
            let _timer = DebugTimer::start("loading @font-face sources");
            font_system_load.load_stylesheet_fonts(&parsed_stylesheets)
        };
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
            resource::ResourceCache::preload(resource_paths).await
        };
        let mut document = {
            let _timer = DebugTimer::start("laying out document");
            layout::layout_dom_async(
                &root,
                &parsed_stylesheets,
                &options,
                self.base_url(),
                self.root_url(),
                &resource_cache,
                font_system_load,
            )
            .await
        };
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

    pub async fn write_pdf_bytes_async(&self, options: &RenderOptions) -> Result<Vec<u8>> {
        let _timer = DebugTimer::start("rendering and writing PDF bytes");
        self.render_async(options).await?.write_pdf_bytes()
    }

    pub async fn write_pdf_async<P: AsRef<Path>>(
        &self,
        target: P,
        options: &RenderOptions,
    ) -> Result<()> {
        let bytes = self.write_pdf_bytes_async(options).await?;
        let _timer = DebugTimer::start(format!("writing PDF file {}", target.as_ref().display()));
        tokio::fs::write(target, bytes).await?;
        Ok(())
    }

    pub fn base_url(&self) -> Option<&Path> {
        self.base_url.as_deref()
    }

    pub fn root_url(&self) -> Option<&Path> {
        self.root_url.as_deref()
    }

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

    async fn linked_stylesheets_async(&self, root: &dom::Node) -> Result<Vec<Css>> {
        let mut styles = Vec::new();
        for href in dom::stylesheet_links(root) {
            let Some(path) =
                resource::resolve_url_path(&href, self.base_url.as_deref(), self.root_url())
            else {
                log::debug!("skipping linked stylesheet without base URL: {href}");
                continue;
            };
            log::debug!("loading linked stylesheet {}", path.display());
            styles.push(
                Css::from_file_async(path)
                    .await?
                    .with_root_url(self.root_url.clone()),
            );
        }
        Ok(styles)
    }
}

fn http_origin(url: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?;
    Some(format!("http://{authority}"))
}

fn resource_paths(
    root: &dom::Node,
    source_stylesheets: &[Css],
    parsed_stylesheets: &[css::Stylesheet],
    base_url: Option<&Path>,
    root_url: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    collect_html_resource_paths(root, base_url, root_url, &mut paths);
    for stylesheet in source_stylesheets {
        paths.extend(resource::css_url_paths(
            stylesheet.source(),
            stylesheet.base_url(),
            stylesheet.root_url(),
        ));
    }
    for stylesheet in parsed_stylesheets {
        for font_face in &stylesheet.font_faces {
            for source in &font_face.sources {
                let css::FontFaceSource::Url {
                    value,
                    base_url,
                    root_url,
                } = source;
                if let Some(path) =
                    resource::resolve_url_path(value, base_url.as_deref(), root_url.as_deref())
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
    base_url: Option<&Path>,
    root_url: Option<&Path>,
    paths: &mut Vec<PathBuf>,
) {
    let dom::NodeKind::Element(element) = &node.kind else {
        return;
    };
    if matches!(element.tag.as_str(), "img" | "input")
        && let Some(src) = element.attrs.get("src")
        && let Some(path) = resource::resolve_url_path(src, base_url, root_url)
    {
        paths.push(path);
    }
    if matches!(element.tag.as_str(), "object" | "embed")
        && let Some(src) = element
            .attrs
            .get("data")
            .or_else(|| element.attrs.get("src"))
        && let Some(path) = resource::resolve_url_path(src, base_url, root_url)
    {
        paths.push(path);
    }
    for child in &element.children {
        collect_html_resource_paths(child, base_url, root_url, paths);
    }
}

async fn resolve_stylesheet_imports_async(stylesheets: Vec<Css>) -> Result<Vec<Css>> {
    let mut resolved = Vec::new();
    for stylesheet in stylesheets {
        resolved.extend(stylesheet.with_imports_async().await?);
    }
    Ok(resolved)
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
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
    use crate::dom::{parse, text_content};

    #[tokio::test]
    async fn extracts_basic_text() {
        assert_eq!(
            text_content(&parse(
                "<style>p{}</style><h1>Hello</h1><p>Rust &amp; PDF</p>"
            )),
            "Hello Rust & PDF"
        );
    }

    #[tokio::test]
    async fn embedded_styles_unwrap_xhtml_cdata_sections() {
        let html = Html::from_string(
            "<style type=\"text/css\"><![CDATA[\n  div { background: green }\n]]></style>",
        );
        let styles = html.embedded_styles();

        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].source(), "\n  div { background: green }\n");
    }
}
