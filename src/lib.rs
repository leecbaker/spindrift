//! A from-scratch Rust HTML/CSS to PDF renderer inspired by WeasyPrint.
//!
//! The public API covers source loading, rendering, PDF serialization, and
//! read-only semantic inspection of rendered documents. Layout, paint, font,
//! and PDF-writer records remain crate implementation details.

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
#[cfg(test)]
pub(crate) use document::PaintStrokeWidth;
pub use document::{
    Bookmark, BookmarkState, Document, DocumentMetadata, FontEmbeddingMode, LinkAnnotation, Page,
    PdfCompression, PdfOptions, PdfProfile,
};
pub use error::{Error, Result};
pub use html::{Html, InputSyntax};
pub use layout::{PageMargins, PageSize, RenderOptions};
pub use resource::{FetchErrorPolicy, ResourcePolicy};
pub(crate) use units::LayoutSize;
pub use url::Url;

#[cfg(test)]
#[path = "tests/smoke.rs"]
mod smoke_tests;
