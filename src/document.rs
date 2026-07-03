mod paint;

pub(crate) use paint::{
    PagePaintTree, PaintBand, PaintBlendMode, PaintCheckpoint, PaintClip, PaintClipPathEffect,
    PaintDisplayItem, PaintEffectScope, PaintEffectStep, PaintEffects, PaintFilterEffect,
    PaintFragment, PaintMaskEffect, PaintPrimitive, PaintStackingContext, PaintTransform,
    PaintVector, RenderedLineSource, RenderedPathCommandPoints, StackLevel, paint_point_to_pdf,
    paint_rect_to_pdf,
};
pub use paint::{
    PaintOperation, PaintPoint, PaintRect, PaintSize, RenderedCornerRadius, RenderedGlyph,
    RenderedImage, RenderedImageSourceRect, RenderedLine, RenderedLink, RenderedPath,
    RenderedPathClip, RenderedPathClipPath, RenderedPathCommand, RenderedPathFillRule,
    RenderedRect, RenderedRoundedRect, RenderedRoundedRectRadii, RenderedStroke,
    RenderedTextMatrix, RenderedTextRun,
};

use crate::{Error, Result, pdf, timing::DebugTimer};
use fontique::Blob as FontiqueBlob;
use std::fmt;
use std::ops::Deref;
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub pages: Vec<Page>,
    pub metadata: DocumentMetadata,
    pub fonts: Vec<DocumentFont>,
    pub bookmarks: Vec<Bookmark>,
}

impl Document {
    pub fn write_pdf_bytes(&self) -> Result<Vec<u8>> {
        self.write_pdf_bytes_with_options(&crate::RenderOptions::default())
    }

    pub fn write_pdf_bytes_with_options(&self, options: &crate::RenderOptions) -> Result<Vec<u8>> {
        let _timer =
            DebugTimer::start(format!("writing {} page(s) to PDF bytes", self.pages.len()));
        if self.pages.is_empty() {
            return Err(Error::InvalidInput("document has no pages".to_string()));
        }
        {
            let _timer = DebugTimer::start("validating paint operations");
            self.validate_paint_operations()?;
        }
        Ok(pdf::write_document(self, options.pdf_variant))
    }

    pub fn write_pdf<P: AsRef<Path>>(&self, target: P) -> Result<()> {
        self.write_pdf_with_options(target, &crate::RenderOptions::default())
    }

    pub fn write_pdf_with_options<P: AsRef<Path>>(
        &self,
        target: P,
        options: &crate::RenderOptions,
    ) -> Result<()> {
        let target = target.as_ref();
        log::debug!("writing PDF file {}", target.display());
        std::fs::write(target, self.write_pdf_bytes_with_options(options)?)?;
        Ok(())
    }

    pub fn validate_paint_operations(&self) -> Result<()> {
        for (page_index, page) in self.pages.iter().enumerate() {
            page.validate_paint_operations(page_index)?;
        }
        Ok(())
    }
}

/// Selects the PDF variant and conformance-identification metadata to emit.
///
/// PDF/A identification is defined by ISO 19005's PDF/A extension schema. The
/// variant value controls only explicit writer behavior, such as PDF header
/// version, XMP identification fields, and stricter font-planning hooks; it is
/// not by itself a guarantee that every PDF/A requirement has been satisfied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PdfVariant {
    Pdf,
    PdfA1B,
    #[default]
    PdfA2B,
    PdfA3B,
    PdfA2U,
    PdfA3U,
}

impl PdfVariant {
    pub const fn pdf_version(self) -> (u8, u8) {
        match self {
            Self::Pdf | Self::PdfA1B => (1, 4),
            Self::PdfA2B | Self::PdfA3B | Self::PdfA2U | Self::PdfA3U => (1, 7),
        }
    }

    pub const fn is_pdfa(self) -> bool {
        !matches!(self, Self::Pdf)
    }

    pub(crate) const fn pdfa_identification(self) -> Option<PdfAIdentification> {
        match self {
            Self::Pdf => None,
            Self::PdfA1B => Some(PdfAIdentification {
                part: 1,
                conformance: "B",
            }),
            Self::PdfA2B => Some(PdfAIdentification {
                part: 2,
                conformance: "B",
            }),
            Self::PdfA3B => Some(PdfAIdentification {
                part: 3,
                conformance: "B",
            }),
            Self::PdfA2U => Some(PdfAIdentification {
                part: 2,
                conformance: "U",
            }),
            Self::PdfA3U => Some(PdfAIdentification {
                part: 3,
                conformance: "U",
            }),
        }
    }
}

impl fmt::Display for PdfVariant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pdf => "pdf",
            Self::PdfA1B => "pdf/a-1b",
            Self::PdfA2B => "pdf/a-2b",
            Self::PdfA3B => "pdf/a-3b",
            Self::PdfA2U => "pdf/a-2u",
            Self::PdfA3U => "pdf/a-3u",
        })
    }
}

impl FromStr for PdfVariant {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "pdf" => Ok(Self::Pdf),
            "pdf/a-1b" => Ok(Self::PdfA1B),
            "pdf/a-2b" => Ok(Self::PdfA2B),
            "pdf/a-3b" => Ok(Self::PdfA3B),
            "pdf/a-2u" => Ok(Self::PdfA2U),
            "pdf/a-3u" => Ok(Self::PdfA3U),
            _ => Err(format!(
                "unsupported PDF variant {value:?}; expected one of pdf, pdf/a-1b, pdf/a-2b, pdf/a-3b, pdf/a-2u, pdf/a-3u"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PdfAIdentification {
    pub part: u8,
    pub conformance: &'static str,
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
    target: PaintPoint,
    pub state: BookmarkState,
}

impl Bookmark {
    pub fn new(
        level: u32,
        label: String,
        page_index: usize,
        x: f32,
        y: f32,
        state: BookmarkState,
    ) -> Self {
        Self {
            level,
            label,
            page_index,
            target: PaintPoint::new(x, y),
            state,
        }
    }

    pub fn x(&self) -> f32 {
        self.target.x
    }

    pub fn y(&self) -> f32 {
        self.target.y
    }

    pub(crate) fn target(&self) -> PaintPoint {
        self.target
    }

    pub(crate) fn translate_target(&mut self, x_offset: f32, y_offset: f32) {
        self.target.x += x_offset;
        self.target.y += y_offset;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookmarkState {
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    size: PaintSize,
    pub rotation: i32,
    pub(crate) operations: Vec<PaintOperation>,
    pub(crate) rects: Vec<RenderedRect>,
    pub(crate) rounded_rects: Vec<RenderedRoundedRect>,
    pub(crate) paths: Vec<RenderedPath>,
    pub(crate) strokes: Vec<RenderedStroke>,
    pub(crate) lines: Vec<RenderedLine>,
    pub(crate) links: Vec<RenderedLink>,
    pub(crate) images: Vec<RenderedImage>,
    paint_tree: Option<paint::PagePaintTree>,
}

impl Page {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: PaintSize::new(width.max(0.0), height.max(0.0)),
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

    pub fn width(&self) -> f32 {
        self.size.width
    }

    pub fn height(&self) -> f32 {
        self.size.height
    }

    pub fn operations(&self) -> &[PaintOperation] {
        &self.operations
    }

    pub fn rects(&self) -> &[RenderedRect] {
        &self.rects
    }

    pub fn rounded_rects(&self) -> &[RenderedRoundedRect] {
        &self.rounded_rects
    }

    pub fn paths(&self) -> &[RenderedPath] {
        &self.paths
    }

    pub fn strokes(&self) -> &[RenderedStroke] {
        &self.strokes
    }

    pub fn lines(&self) -> &[RenderedLine] {
        &self.lines
    }

    pub fn links(&self) -> &[RenderedLink] {
        &self.links
    }

    pub fn images(&self) -> &[RenderedImage] {
        &self.images
    }

    pub(crate) fn paint_size(&self) -> PaintSize {
        self.size
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
