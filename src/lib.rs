//! A from-scratch Rust HTML/CSS to PDF renderer inspired by WeasyPrint.
//!
//! This crate currently implements the first milestone of the porting plan:
//! a public API, a CLI-facing renderer, basic text extraction, simple paged
//! layout, and deterministic PDF output using embedded system fonts.

mod css;
mod document;
mod dom;
mod error;
mod html;
mod layout;
mod pdf;
mod resource;
mod text;
mod timing;

pub use css::{Color, Css};
pub use document::{
    Bookmark, BookmarkState, Document, DocumentFont, DocumentMetadata, Page, PaintOperation,
    PaintPoint, PaintRect, PaintSize, PdfVariant, RenderedCornerRadius, RenderedGlyph,
    RenderedImage, RenderedLine, RenderedLink, RenderedPath, RenderedPathCommand,
    RenderedPathFillRule, RenderedRect, RenderedRoundedRect, RenderedRoundedRectRadii,
    RenderedStroke, RenderedTextMatrix, RenderedTextRun,
};
pub use error::{Error, Result};
pub use html::Html;
pub use layout::{PageMargins, PageSize, RenderOptions};
pub use resource::file_url_to_path;
