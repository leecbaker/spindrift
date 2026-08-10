//! A from-scratch Rust HTML/CSS to PDF renderer inspired by WeasyPrint.
//!
//! The public API covers source loading, rendering, PDF serialization, and
//! read-only semantic inspection of rendered documents. Layout, paint, font,
//! and PDF-writer records remain crate implementation details.
//!
//! # Examples
//!
//! ## Convert an HTML file to a PDF
//!
//! Load a local HTML document and write its PDF to a local file:
//!
//! ```no_run
//! use quire::{Html, PdfOptions, RenderOptions};
//! use std::fs::File;
//!
//! # async fn convert() -> quire::Result<()> {
//! let mut output = File::create("document.pdf")?;
//! Html::from_file("document.html")
//!     .await?
//!     .write_pdf(
//!         &mut output,
//!         &RenderOptions::default(),
//!         &PdfOptions::default(),
//!     )
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Render a report with explicit resource handling
//!
//! The rendering pipeline keeps document and stylesheet loading, CSS layout,
//! and PDF serialization separate. This lets an application share a resource
//! policy, supply a user-origin stylesheet, inspect the rendered document, and
//! choose its PDF output settings:
//!
//! ```no_run
//! use quire::{
//!     Css, FetchErrorPolicy, Html, HttpRequestTimeout, PdfOptions, RenderOptions,
//!     ResourcePolicy,
//! };
//! use std::{fs::File, time::Duration};
//!
//! # async fn render_report() -> quire::Result<()> {
//! let resource_policy = ResourcePolicy {
//!     follow_http_redirects: false,
//!     http_timeout: HttpRequestTimeout::try_from(Duration::from_secs(5))
//!         .expect("five seconds is non-zero"),
//!     error_policy: FetchErrorPolicy::Allow,
//! };
//!
//! let stylesheet = Css::from_file("print.css")
//!     .await?
//!     .with_user_origin()
//!     .with_resource_policy(resource_policy);
//! let html = Html::from_file("report.html")
//!     .await?
//!     .with_resource_policy(resource_policy)
//!     .with_stylesheet(stylesheet);
//!
//! let document = html.render(&RenderOptions::default()).await?;
//! if let Some(title) = document.metadata().title() {
//!     println!("Rendered {title}");
//! }
//! for bookmark in document.bookmarks() {
//!     println!("Bookmark: {}", bookmark.label());
//! }
//!
//! let mut output = File::create("report.pdf")?;
//! document.write_pdf(&mut output, &PdfOptions::default())?;
//! # Ok(())
//! # }
//! ```

mod color;
mod css;
mod document;
mod dom;
mod error;
mod html;
mod image_store;
mod layout;
mod pdf;
mod resource;
mod svg;
mod text;
mod timing;
mod units;

pub(crate) use css::CssColor;
pub use css::{
    ColorSchemePreference, Css, CssViewportSize, ForcedColorPalette, ForcedColorsMode,
    MediaEnvironment, MediaType,
};
pub(crate) use document::Page;
#[cfg(test)]
pub(crate) use document::PaintStrokeWidth;
pub use document::{
    Bookmark, BookmarkState, Document, DocumentDate, DocumentMetadata, FontEmbeddingMode,
    PdfCompression, PdfOptions, PdfProfile,
};
pub use error::{Error, Result};
pub use html::{Html, InputSyntax};
pub use layout::RenderOptions;
pub use resource::{
    FetchErrorPolicy, HttpRequestTimeout, InvalidHttpRequestTimeout, ResourcePolicy,
};
pub(crate) use units::LayoutSize;
/// A parsed URL used as an HTML or stylesheet source.
///
/// ```no_run
/// use quire::{Html, PdfOptions, RenderOptions, Url};
/// use std::fs::File;
///
/// # async fn render() -> Result<(), Box<dyn std::error::Error>> {
/// let source = Url::parse("https://example.test/report.html")?;
/// let html = Html::from_url(source).await?;
/// let mut output = File::create("report.pdf")?;
/// html.write_pdf(&mut output, &RenderOptions::default(), &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
pub use url::Url;

#[cfg(test)]
#[path = "tests/smoke.rs"]
mod smoke_tests;
