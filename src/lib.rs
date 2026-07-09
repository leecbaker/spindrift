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

pub(crate) use css::Color;
pub use css::{Css, MediaType};
pub use document::{
    Bookmark, BookmarkState, Document, DocumentMetadata, FontEmbeddingMode, LinkAnnotation, Page,
    PdfCompression, PdfOptions, PdfProfile,
};
pub(crate) use document::{
    DocumentFont, PaintOperation, PaintPoint, PaintRect, RenderedGlyph, RenderedImage,
    RenderedLine, RenderedPath, RenderedPathCommand, RenderedPathFillRule, RenderedRect,
    RenderedRoundedRect, RenderedStroke, RenderedTextMatrix, RenderedTextRun,
};
#[cfg(test)]
pub(crate) use document::{PaintSize, RenderedCornerRadius, RenderedRoundedRectRadii};
pub use error::{Error, Result};
pub use html::{Html, InputSyntax};
pub use layout::{PageMargins, PageSize, RenderOptions};
pub use resource::{FetchErrorPolicy, ResourcePolicy};
pub(crate) use units::LayoutSize;
pub use url::Url;

#[cfg(test)]
#[path = "tests/smoke.rs"]
mod smoke_tests;
