pub(crate) mod paint;

pub use paint::annotations::LinkAnnotation;
pub(crate) use paint::geometry::PaintStrokeWidth;

use paint::annotations::RenderedLink;
use paint::display_list::PagePaintTree;
use paint::geometry::{PaintPoint, PaintSize};
use paint::images::RenderedImage;
use paint::page::{OpaqueTextCoverage, PaintOperation};
use paint::paths::{RenderedGradient, RenderedPath, RenderedPathPaint};
use paint::patterns::{RenderedGradientPattern, RenderedImagePattern, RenderedSvgPattern};
use paint::shapes::{RenderedRect, RenderedRoundedRect, RenderedStroke};
use paint::text::RenderedLine;

use crate::{CssColor, Error, Result, image_store::DocumentImageStore, pdf, timing::DebugTimer};
use fontique::Blob as FontiqueBlob;
use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
/// The fully rendered, inspectable document before PDF serialization.
///
/// ```no_run
/// # fn inspect(document: &quire::Document) {
/// let page_count = document.pages().len();
/// # let _ = page_count;
/// # }
/// ```
pub struct Document {
    pub(crate) pages: Vec<Page>,
    pub(crate) metadata: DocumentMetadata,
    pub(crate) fonts: Vec<DocumentFont>,
    pub(crate) bookmarks: Vec<Bookmark>,
    pub(crate) image_store: Box<DocumentImageStore>,
}

impl Document {
    /// Materialize page images before the document is embedded in an iframe.
    ///
    /// Page paint stores only document-local image IDs. An embedded page is
    /// replayed into its parent's paint tree, where the same numeric ID may
    /// name a different image; converting to inline samples preserves the
    /// child browsing context's resource identity.
    /// <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>
    pub(crate) fn materialize_images_for_embedding(&mut self) {
        for page in &mut self.pages {
            for image in &mut page.images {
                image.materialize_store_backing(&self.image_store);
            }
        }
    }

    /// Returns the rendered pages in document order.
    ///
    /// ```no_run
    /// # fn inspect(document: &quire::Document) {
    /// let pages = document.pages();
    /// # let _ = pages;
    /// # }
    /// ```
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// Returns document-wide PDF metadata extracted during rendering.
    ///
    /// ```no_run
    /// # fn inspect(document: &quire::Document) {
    /// let title = document.metadata().title();
    /// # let _ = title;
    /// # }
    /// ```
    pub fn metadata(&self) -> &DocumentMetadata {
        &self.metadata
    }

    /// Returns document bookmarks in source order.
    ///
    /// ```no_run
    /// # fn inspect(document: &quire::Document) {
    /// for bookmark in document.bookmarks() {
    ///     println!("{}", bookmark.label());
    /// }
    /// # }
    /// ```
    pub fn bookmarks(&self) -> &[Bookmark] {
        &self.bookmarks
    }

    /// Serializes this document as PDF bytes using the supplied PDF options.
    ///
    /// ```no_run
    /// # fn write(document: &quire::Document) -> quire::Result<()> {
    /// let pdf = document.write_pdf_bytes(&quire::PdfOptions::default())?;
    /// # let _ = pdf;
    /// # Ok(())
    /// # }
    /// ```
    pub fn write_pdf_bytes(&self, options: &PdfOptions) -> Result<Vec<u8>> {
        let _timer =
            DebugTimer::start(format!("writing {} page(s) to PDF bytes", self.pages.len()));
        if self.pages.is_empty() {
            return Err(Error::InvalidInput("document has no pages".to_string()));
        }
        {
            let _timer = DebugTimer::start("validating paint operations");
            self.validate_paint_operations()?;
        }
        pdf::write_document(self, options)
    }

    /// Serializes this document to a PDF file using the supplied PDF options.
    ///
    /// ```no_run
    /// # fn write(document: &quire::Document) -> quire::Result<()> {
    /// document.write_pdf("document.pdf", &quire::PdfOptions::default())?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_pdf<P: AsRef<Path>>(&self, target: P, options: &PdfOptions) -> Result<()> {
        let target = target.as_ref();
        log::debug!("writing PDF file {}", target.display());
        std::fs::write(target, self.write_pdf_bytes(options)?)?;
        Ok(())
    }

    pub(crate) fn validate_paint_operations(&self) -> Result<()> {
        for (page_index, page) in self.pages.iter().enumerate() {
            page.validate_paint_operations(page_index)?;
        }
        Ok(())
    }
}

/// Controls whether Quire applies Flate compression to generated PDF streams.
///
/// ISO 32000-1:2008, 7.4.4 defines the `/FlateDecode` stream filter. Disabling
/// compression is useful when inspecting generated PDF syntax, but increases
/// output size substantially for images and embedded font programs.
///
/// ```
/// let compression = quire::PdfCompression::Uncompressed;
/// assert_eq!(compression, quire::PdfCompression::Uncompressed);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PdfCompression {
    /// Apply Flate compression to eligible PDF streams.
    #[default]
    Compressed,
    /// Leave PDF streams uncompressed for inspection or debugging.
    Uncompressed,
}

/// Controls whether PDF embedding keeps complete font programs or subsets them.
///
/// `Subset` is the default because it produces smaller PDFs and emits a compact CID mapping.
/// `Full` embeds complete programs where the selected PDF profile permits it.
///
/// ISO 32000-2:2020, 9.7 and 9.9 define Type 0/CIDFont and embedded font
/// program requirements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontEmbeddingMode {
    /// Embed only the glyphs used by the document.
    #[default]
    Subset,
    /// Embed complete font programs where the selected PDF profile permits it.
    Full,
}

/// Selects the PDF output profile and conformance-identification metadata to emit.
///
/// PDF/A identification is defined by ISO 19005's PDF/A extension schema. The
/// profile controls only explicit writer behavior, such as PDF header
/// version, XMP identification fields, and stricter font-planning hooks; it is
/// not by itself a guarantee that every PDF/A requirement has been satisfied.
///
/// ```
/// let profile = quire::PdfProfile::PdfA2B;
/// assert!(profile.is_pdfa());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PdfProfile {
    /// A regular PDF document without a PDF/A conformance target.
    #[default]
    Pdf,
    /// A PDF/A-1b document.
    PdfA1B,
    /// A PDF/A-2b document.
    PdfA2B,
    /// A PDF/A-3b document.
    PdfA3B,
    /// A PDF/A-2u document.
    PdfA2U,
    /// A PDF/A-3u document.
    PdfA3U,
}

impl PdfProfile {
    /// Returns the PDF version required by this profile.
    ///
    /// ```
    /// assert_eq!(quire::PdfProfile::PdfA2B.pdf_version(), (1, 7));
    /// ```
    pub const fn pdf_version(self) -> (u8, u8) {
        match self {
            Self::Pdf | Self::PdfA1B => (1, 4),
            Self::PdfA2B | Self::PdfA3B | Self::PdfA2U | Self::PdfA3U => (1, 7),
        }
    }

    /// Returns whether this profile targets a PDF/A conformance level.
    ///
    /// ```
    /// assert!(!quire::PdfProfile::Pdf.is_pdfa());
    /// ```
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

impl fmt::Display for PdfProfile {
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

impl FromStr for PdfProfile {
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
                "unsupported PDF profile {value:?}; expected one of pdf, pdf/a-1b, pdf/a-2b, pdf/a-3b, pdf/a-2u, pdf/a-3u"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PdfAIdentification {
    pub part: u8,
    pub conformance: &'static str,
}

/// Policy for serializing a rendered document as PDF.
///
/// These settings do not affect CSS parsing, cascade, layout, or paint. The
/// same [`Document`] can therefore be serialized repeatedly with distinct PDF
/// profiles, compression, font embedding, or producer values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfOptions {
    /// PDF profile and conformance-identification metadata to emit.
    pub profile: PdfProfile,
    /// Font-program embedding policy.
    pub font_embedding: FontEmbeddingMode,
    /// Generated-stream compression policy.
    pub compression: PdfCompression,
    /// Producer string written to PDF document information and XMP metadata.
    pub producer: String,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            profile: PdfProfile::default(),
            font_embedding: FontEmbeddingMode::default(),
            compression: PdfCompression::default(),
            producer: "quire 0.1.0".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Source metadata associated with a rendered document.
pub struct DocumentMetadata {
    pub(crate) title: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) creator: Option<String>,
}

impl DocumentMetadata {
    /// Returns the document title, if one was present in the source.
    ///
    /// ```no_run
    /// # fn inspect(metadata: &quire::DocumentMetadata) {
    /// assert!(metadata.title().is_none());
    /// # }
    /// ```
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the document author, if one was present in the source.
    ///
    /// ```no_run
    /// # fn inspect(metadata: &quire::DocumentMetadata) {
    /// let author = metadata.author();
    /// # let _ = author;
    /// # }
    /// ```
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// Returns the document creator, if one was present in the source.
    ///
    /// ```no_run
    /// # fn inspect(metadata: &quire::DocumentMetadata) {
    /// let creator = metadata.creator();
    /// # let _ = creator;
    /// # }
    /// ```
    pub fn creator(&self) -> Option<&str> {
        self.creator.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq)]
/// A document bookmark derived from the rendered source.
///
/// ```no_run
/// # fn inspect(bookmark: &quire::Bookmark) {
/// println!("{}", bookmark.label());
/// # }
/// ```
pub struct Bookmark {
    pub(crate) level: u32,
    pub(crate) label: String,
    pub(crate) page_index: usize,
    target: PaintPoint,
    pub(crate) state: BookmarkState,
}

impl Bookmark {
    pub(crate) fn new(
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

    /// Returns the bookmark destination's horizontal position in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(bookmark: &quire::Bookmark) {
    /// let x = bookmark.x();
    /// # let _ = x;
    /// # }
    /// ```
    pub fn x(&self) -> f32 {
        self.target.x
    }

    /// Returns the bookmark destination's vertical position in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(bookmark: &quire::Bookmark) {
    /// let y = bookmark.y();
    /// # let _ = y;
    /// # }
    /// ```
    pub fn y(&self) -> f32 {
        self.target.y
    }

    /// Returns the bookmark nesting level, starting at one.
    ///
    /// ```no_run
    /// # fn inspect(bookmark: &quire::Bookmark) {
    /// assert!(bookmark.level() >= 1);
    /// # }
    /// ```
    pub fn level(&self) -> u32 {
        self.level
    }

    /// Returns the bookmark's displayed label.
    ///
    /// ```no_run
    /// # fn inspect(bookmark: &quire::Bookmark) {
    /// let label = bookmark.label();
    /// # let _ = label;
    /// # }
    /// ```
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the zero-based index of the destination page.
    ///
    /// ```no_run
    /// # fn inspect(bookmark: &quire::Bookmark) {
    /// let page_index = bookmark.page_index();
    /// # let _ = page_index;
    /// # }
    /// ```
    pub fn page_index(&self) -> usize {
        self.page_index
    }

    /// Returns the bookmark's initial expansion state.
    ///
    /// ```no_run
    /// # fn inspect(bookmark: &quire::Bookmark) {
    /// let state = bookmark.state();
    /// # let _ = state;
    /// # }
    /// ```
    pub fn state(&self) -> BookmarkState {
        self.state
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
/// The initial expansion state of a bookmark in a PDF viewer.
///
/// ```
/// let state = quire::BookmarkState::Open;
/// assert_eq!(state, quire::BookmarkState::Open);
/// ```
pub enum BookmarkState {
    /// Display the bookmark's children initially.
    Open,
    /// Hide the bookmark's children initially.
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
/// A rendered page in document order.
///
/// ```no_run
/// # fn inspect(page: &quire::Page) {
/// let area = page.width() * page.height();
/// # let _ = area;
/// # }
/// ```
pub struct Page {
    size: PaintSize,
    pub(crate) rotation: i32,
    pub(crate) rects: Vec<RenderedRect>,
    pub(crate) rounded_rects: Vec<RenderedRoundedRect>,
    pub(crate) paths: Vec<RenderedPath>,
    pub(crate) strokes: Vec<RenderedStroke>,
    pub(crate) lines: Vec<RenderedLine>,
    pub(crate) links: Vec<RenderedLink>,
    pub(crate) images: Vec<RenderedImage>,
    pub(crate) image_patterns: Vec<RenderedImagePattern>,
    pub(crate) gradient_patterns: Vec<RenderedGradientPattern>,
    pub(crate) svg_patterns: Vec<RenderedSvgPattern>,
    pub(crate) opaque_text_coverages: Vec<OpaqueTextCoverage>,
    /// A committed CSS fragmentation slice that owns this page even when it
    /// has no visible paint primitives (for example, the trailing slice of a
    /// tall table row).
    pub(crate) has_fragmentation_content: bool,
    paint_tree: PagePaintTree,
}

#[allow(dead_code)]
impl Page {
    pub(crate) fn new(width: f32, height: f32) -> Self {
        Self {
            size: PaintSize::new(width.max(0.0), height.max(0.0)),
            rotation: 0,
            rects: Vec::new(),
            rounded_rects: Vec::new(),
            paths: Vec::new(),
            strokes: Vec::new(),
            lines: Vec::new(),
            links: Vec::new(),
            images: Vec::new(),
            image_patterns: Vec::new(),
            gradient_patterns: Vec::new(),
            svg_patterns: Vec::new(),
            opaque_text_coverages: Vec::new(),
            has_fragmentation_content: false,
            paint_tree: PagePaintTree::new(),
        }
    }

    /// Returns the page width in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(page: &quire::Page) {
    /// let width = page.width();
    /// # let _ = width;
    /// # }
    /// ```
    pub fn width(&self) -> f32 {
        self.size.width
    }

    /// Returns the page height in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(page: &quire::Page) {
    /// let height = page.height();
    /// # let _ = height;
    /// # }
    /// ```
    pub fn height(&self) -> f32 {
        self.size.height
    }

    /// Return the paint-operation projection of the canonical paint tree.
    ///
    /// The tree retains stacking contexts and effect scopes; this flattened
    /// view is for inspection and structural assertions only.
    pub(crate) fn operations(&self) -> Cow<'_, [PaintOperation]> {
        Cow::Owned(self.paint_tree.flattened_operations())
    }

    pub(crate) fn rects(&self) -> &[RenderedRect] {
        &self.rects
    }

    pub(crate) fn rounded_rects(&self) -> &[RenderedRoundedRect] {
        &self.rounded_rects
    }

    pub(crate) fn paths(&self) -> &[RenderedPath] {
        &self.paths
    }

    pub(crate) fn strokes(&self) -> &[RenderedStroke] {
        &self.strokes
    }

    pub(crate) fn lines(&self) -> &[RenderedLine] {
        &self.lines
    }

    /// Returns link annotations in page-local PDF-point coordinates.
    ///
    /// ```no_run
    /// # fn inspect(page: &quire::Page) {
    /// for link in page.links() {
    ///     println!("{}", link.target());
    /// }
    /// # }
    /// ```
    pub fn links(&self) -> &[LinkAnnotation] {
        &self.links
    }

    pub(crate) fn images(&self) -> &[RenderedImage] {
        &self.images
    }

    pub(crate) fn image_patterns(&self) -> &[RenderedImagePattern] {
        &self.image_patterns
    }

    /// Typed CSS gradient patterns painted on this page.
    ///
    /// Gradients remain vector PDF shadings rather than being flattened into
    /// raster images, while preserving their resolved CSS tile geometry.
    pub(crate) fn gradient_patterns(&self) -> &[RenderedGradientPattern] {
        &self.gradient_patterns
    }

    /// Returns the clockwise page rotation in degrees.
    ///
    /// ```no_run
    /// # fn inspect(page: &quire::Page) {
    /// assert_eq!(page.rotation() % 90, 0);
    /// # }
    /// ```
    pub fn rotation(&self) -> i32 {
        self.rotation
    }

    pub(crate) fn paint_size(&self) -> PaintSize {
        self.size
    }

    /// Return whether the page contains any visible PDF painting primitive.
    ///
    /// PDF content streams are ordered drawing operators (§8.2, ISO 32000-1),
    /// while CSS painting defines the source paint order for all visual
    /// primitives (CSS 2.2 Appendix E). The retained paint tree is therefore
    /// the source of truth; backing storage without a tree operation does not
    /// make a page renderable.
    pub(crate) fn has_paint_content(&self) -> bool {
        !self.paint_tree.flattened_operations().is_empty()
    }

    pub(crate) fn mark_fragmentation_content(&mut self) {
        self.has_fragmentation_content = true;
    }

    pub(crate) fn has_fragmentation_content(&self) -> bool {
        self.has_fragmentation_content
    }

    /// Return every concrete CSS color that can become vector PDF paint.
    ///
    /// PDF resource planning must follow the eventual output colors rather
    /// than these colors' retained CSS component spaces. In particular, a
    /// D50 PCS color may serialize as Display-P3 when it cannot fit in sRGB.
    pub(crate) fn vector_paint_colors(&self) -> Vec<CssColor> {
        let mut colors = Vec::new();
        for rect in &self.rects {
            collect_optional_color(rect.fill, &mut colors);
            collect_optional_color(rect.stroke, &mut colors);
        }
        for rect in &self.rounded_rects {
            collect_optional_color(rect.fill, &mut colors);
            collect_optional_color(rect.stroke, &mut colors);
        }
        for path in &self.paths {
            collect_path_colors(path, &mut colors);
        }
        for stroke in &self.strokes {
            colors.push(stroke.color);
        }
        for line in &self.lines {
            colors.push(line.color);
        }
        for pattern in &self.gradient_patterns {
            collect_gradient_colors(&pattern.gradient, &mut colors);
        }
        for pattern in &self.svg_patterns {
            for path in &pattern.paths {
                collect_path_colors(path, &mut colors);
            }
        }
        colors
    }
}

fn collect_optional_color(color: Option<CssColor>, colors: &mut Vec<CssColor>) {
    if let Some(color) = color {
        colors.push(color);
    }
}

fn collect_gradient_colors(gradient: &RenderedGradient, colors: &mut Vec<CssColor>) {
    colors.extend(gradient.stops.iter().map(|stop| stop.color));
    if let Some(periodic) = &gradient.periodic {
        colors.extend(periodic.stops.iter().map(|stop| stop.color));
    }
}

fn collect_path_colors(path: &RenderedPath, colors: &mut Vec<CssColor>) {
    collect_optional_color(path.fill, colors);
    collect_optional_color(path.stroke, colors);
    for paint in [path.fill_paint.as_ref(), path.stroke_paint.as_ref()]
        .into_iter()
        .flatten()
    {
        match paint {
            RenderedPathPaint::Solid(color) => {
                colors.push(*color);
            }
            RenderedPathPaint::Gradient(gradient) => collect_gradient_colors(gradient, colors),
            RenderedPathPaint::SvgPattern(pattern) => {
                for path in &pattern.paths {
                    collect_path_colors(path, colors);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FontProgramKind {
    TrueType,
    OpenTypeCff,
}

#[derive(Clone)]
pub(crate) struct DocumentFontData {
    blob: FontiqueBlob<u8>,
}

#[allow(dead_code)]
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

/// Native OpenType vertical metrics in font units.
///
/// These describe the coordinate system of the embedded glyph program. CSS
/// `@font-face` metric override descriptors must not alter them: glyph paths,
/// ink bounds, and font-table-backed decoration metrics remain in this space.
/// <https://drafts.csswg.org/css-fonts-5/#font-metric-override-desc>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenTypeVerticalMetrics {
    pub(crate) ascender: i16,
    pub(crate) descender: i16,
    pub(crate) line_gap: i16,
}

/// CSS inline-layout vertical metrics in font units.
///
/// An `@font-face` ascent, descent, or line-gap override changes this metric
/// set without changing the OpenType glyph coordinate system.
/// <https://drafts.csswg.org/css-fonts-5/#font-metric-override-desc>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CssFontVerticalMetrics {
    pub(crate) ascender: i16,
    pub(crate) descender: i16,
    pub(crate) line_gap: i16,
}

/// PDF-visible synthesis selected while matching a CSS font face.
///
/// CSS Fonts permits a user agent to synthesize a bold face only when font
/// matching selected that synthesis and `font-synthesis-weight` allows it.
/// This is deliberately independent from the embedded font program: a
/// regular and an emboldened use can share one subset while retaining their
/// distinct paint state.
/// <https://www.w3.org/TR/css-fonts-4/#font-synthesis-intro>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct DocumentFontSynthesis {
    pub(crate) embolden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocumentFont {
    pub(crate) id: usize,
    pub(crate) family: String,
    pub(crate) post_script_name: String,
    pub(crate) program_kind: FontProgramKind,
    pub(crate) data: DocumentFontData,
    pub(crate) face_index: u32,
    pub(crate) units_per_em: u16,
    pub(crate) program_metrics: OpenTypeVerticalMetrics,
    pub(crate) layout_metrics: CssFontVerticalMetrics,
    pub(crate) cap_height: i16,
    pub(crate) italic_angle: i16,
    pub(crate) bbox: [i16; 4],
    pub(crate) synthesis: DocumentFontSynthesis,
}
