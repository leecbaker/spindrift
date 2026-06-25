mod paint;

pub(crate) use paint::{
    PagePaintTree, PaintBand, PaintCheckpoint, PaintClip, PaintDisplayItem, PaintEffects,
    PaintFragment, PaintPrimitive, PaintStackingContext, PaintTransform,
};
pub use paint::{
    PaintOperation, RenderedCornerRadius, RenderedGlyph, RenderedImage, RenderedImageSourceRect,
    RenderedLine, RenderedLink, RenderedPath, RenderedPathClip, RenderedPathClipPath,
    RenderedPathCommand, RenderedPathFillRule, RenderedRect, RenderedRoundedRect,
    RenderedRoundedRectRadii, RenderedStroke, RenderedTextRun,
};

use crate::{Error, Result, pdf, timing::DebugTimer};
use fontique::Blob as FontiqueBlob;
use std::fmt;
use std::ops::Deref;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub pages: Vec<Page>,
    pub metadata: DocumentMetadata,
    pub fonts: Vec<DocumentFont>,
    pub bookmarks: Vec<Bookmark>,
}

impl Document {
    pub fn write_pdf_bytes(&self) -> Result<Vec<u8>> {
        let _timer =
            DebugTimer::start(format!("writing {} page(s) to PDF bytes", self.pages.len()));
        if self.pages.is_empty() {
            return Err(Error::InvalidInput("document has no pages".to_string()));
        }
        {
            let _timer = DebugTimer::start("validating paint operations");
            self.validate_paint_operations()?;
        }
        Ok(pdf::write_document(self))
    }

    pub fn write_pdf<P: AsRef<Path>>(&self, target: P) -> Result<()> {
        let target = target.as_ref();
        log::debug!("writing PDF file {}", target.display());
        std::fs::write(target, self.write_pdf_bytes()?)?;
        Ok(())
    }

    pub fn validate_paint_operations(&self) -> Result<()> {
        for (page_index, page) in self.pages.iter().enumerate() {
            page.validate_paint_operations(page_index)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub creator: Option<String>,
    pub producer: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bookmark {
    pub level: u32,
    pub label: String,
    pub page_index: usize,
    pub x: f32,
    pub y: f32,
    pub state: BookmarkState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookmarkState {
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub width: f32,
    pub height: f32,
    pub rotation: i32,
    pub operations: Vec<PaintOperation>,
    pub rects: Vec<RenderedRect>,
    pub rounded_rects: Vec<RenderedRoundedRect>,
    pub paths: Vec<RenderedPath>,
    pub strokes: Vec<RenderedStroke>,
    pub lines: Vec<RenderedLine>,
    pub links: Vec<RenderedLink>,
    pub images: Vec<RenderedImage>,
    paint_tree: Option<paint::PagePaintTree>,
}

impl Page {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            rotation: 0,
            operations: Vec::new(),
            rects: Vec::new(),
            rounded_rects: Vec::new(),
            paths: Vec::new(),
            strokes: Vec::new(),
            lines: Vec::new(),
            links: Vec::new(),
            images: Vec::new(),
            paint_tree: Some(paint::PagePaintTree::new()),
        }
    }

    /// Return whether the page contains any visible PDF painting primitive.
    ///
    /// PDF content streams are ordered drawing operators (§8.2, ISO 32000-1),
    /// while CSS painting defines the source paint order for all visual
    /// primitives (CSS 2.2 Appendix E). This predicate therefore follows the
    /// complete primitive storage model instead of checking only text/rects.
    pub(crate) fn has_paint_content(&self) -> bool {
        !self.lines.is_empty()
            || !self.rects.is_empty()
            || !self.rounded_rects.is_empty()
            || !self.paths.is_empty()
            || !self.strokes.is_empty()
            || !self.images.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontProgramKind {
    TrueType,
    OpenTypeCff,
}

#[derive(Clone)]
pub struct DocumentFontData {
    blob: FontiqueBlob<u8>,
}

impl DocumentFontData {
    pub(crate) fn from_blob(blob: FontiqueBlob<u8>) -> Self {
        Self { blob }
    }

    pub fn as_slice(&self) -> &[u8] {
        self.blob.as_ref()
    }

    pub fn len(&self) -> usize {
        self.blob.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blob.is_empty()
    }

    pub(crate) fn blob_id(&self) -> u64 {
        self.blob.id()
    }
}

impl AsRef<[u8]> for DocumentFontData {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Deref for DocumentFontData {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl fmt::Debug for DocumentFontData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DocumentFontData")
            .field("blob_id", &self.blob_id())
            .field("len", &self.len())
            .finish()
    }
}

impl PartialEq for DocumentFontData {
    fn eq(&self, other: &Self) -> bool {
        self.blob == other.blob
    }
}

impl Eq for DocumentFontData {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentFont {
    pub id: usize,
    pub family: String,
    pub post_script_name: String,
    pub program_kind: FontProgramKind,
    pub data: DocumentFontData,
    pub face_index: u32,
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
    pub cap_height: i16,
    pub italic_angle: i16,
    pub bbox: [i16; 4],
}
