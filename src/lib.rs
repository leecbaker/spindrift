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
mod units;

pub use css::{Color, Css};
pub use document::{
    Bookmark, BookmarkState, Document, DocumentFont, DocumentMetadata, Page, PaintOperation,
    PaintPoint, PaintRect, PaintSize, PdfVariant, RenderedCornerRadius, RenderedGlyph,
    RenderedImage, RenderedLine, RenderedLink, RenderedPath, RenderedPathCommand,
    RenderedPathFillRule, RenderedRect, RenderedRoundedRect, RenderedRoundedRectRadii,
    RenderedStroke, RenderedTextMatrix, RenderedTextRun,
};
pub use error::{Error, Result};
pub use html::{Html, InputSyntax};
pub use layout::{PageMargins, PageSize, RenderOptions};
pub use resource::file_url_to_path;
pub use units::{
    BorderBoxLength, BorderBoxSize, ContentBoxLength, ContentBoxSize, LayoutLength, LayoutSize,
    NonContentLength, RasterPixelSize, SemanticLengthExt, border_box_pt, border_box_size_pt,
    border_box_to_content_box_length, border_box_to_content_box_size, content_box_pt,
    content_box_size_pt, content_box_to_border_box_length, content_box_to_border_box_size,
    layout_in, layout_points, layout_pt, layout_px, non_content_pt, raster_natural_layout_size,
};
