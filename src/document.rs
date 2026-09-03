pub(crate) mod paint;

use std::borrow::Cow;
use std::fmt;
use std::io::Write;
use std::ops::Deref;
use std::str::FromStr;

use fontique::Blob as FontiqueBlob;
use jiff::civil::Date;
use jiff::fmt::strtime::BrokenDownTime;
pub(crate) use paint::annotations::LinkAnnotation;
use paint::annotations::RenderedLink;
use paint::display_list::PagePaintTree;
pub(crate) use paint::geometry::PaintStrokeWidth;
use paint::geometry::{PaintPoint, PaintSize};
use paint::images::RenderedImage;
use paint::page::{OpaqueTextCoverage, PaintOperation, SvgTextOutline};
use paint::paths::{RenderedGradient, RenderedPath, RenderedPathPaint};
use paint::patterns::{RenderedGradientPattern, RenderedImagePattern, RenderedSvgPattern};
use paint::shapes::{RenderedRect, RenderedRoundedRect, RenderedStroke};
use paint::text::RenderedLine;

use crate::image_store::DocumentImageStore;
use crate::timing::DebugTimer;
use crate::{CssColor, Error, Result, pdf};

#[derive(Debug, Clone, PartialEq)]
/// The fully rendered, inspectable document before PDF serialization.
///
/// ```no_run
/// use quire::{Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let document = Html::from_file("document.html")
///     .await?
///     .render(&RenderOptions::default())
///     .await?;
/// let mut output = File::create("document.pdf")?;
/// document.write_pdf(&mut output, &PdfOptions::default())?;
/// # Ok(())
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

    /// Serializes this document as a PDF into `writer` using the supplied PDF
    /// options.
    ///
    /// ```no_run
    /// # fn write(document: &quire::Document, output: &mut Vec<u8>) -> quire::Result<()> {
    /// document.write_pdf(output, &quire::PdfOptions::default())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn write_pdf<W: Write>(&self, writer: &mut W, options: &PdfOptions) -> Result<()> {
        let _timer = DebugTimer::start(format!("writing {} page(s) to PDF", self.pages.len()));
        if self.pages.is_empty() {
            return Err(Error::InvalidInput("document has no pages".to_string()));
        }
        {
            let _timer = DebugTimer::start("validating paint operations");
            self.validate_paint_operations()?;
        }
        pdf::write_document(self, options, writer)
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
/// ```no_run
/// use quire::{Html, PdfCompression, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let pdf_options = PdfOptions {
///     compression: PdfCompression::Uncompressed,
///     ..PdfOptions::default()
/// };
/// let document = Html::from_file("document.html")
///     .await?
///     .render(&RenderOptions::default())
///     .await?;
/// let mut output = File::create("document.pdf")?;
/// document.write_pdf(&mut output, &pdf_options)?;
/// # Ok(())
/// # }
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
///
/// ```no_run
/// use quire::{FontEmbeddingMode, Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let pdf_options = PdfOptions {
///     font_embedding: FontEmbeddingMode::Full,
///     ..PdfOptions::default()
/// };
/// let document = Html::from_file("document.html")
///     .await?
///     .render(&RenderOptions::default())
///     .await?;
/// let mut output = File::create("document.pdf")?;
/// document.write_pdf(&mut output, &pdf_options)?;
/// # Ok(())
/// # }
/// ```
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
/// ```no_run
/// use quire::{Html, PdfOptions, PdfProfile, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let pdf_options = PdfOptions {
///     profile: PdfProfile::PdfA1B,
///     ..PdfOptions::default()
/// };
/// let document = Html::from_file("document.html")
///     .await?
///     .render(&RenderOptions::default())
///     .await?;
/// let mut output = File::create("document.pdf")?;
/// document.write_pdf(&mut output, &pdf_options)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PdfProfile {
    /// A regular PDF document without a PDF/A conformance target.
    #[default]
    Pdf,
    /// A PDF/A-1b document.
    PdfA1B,
}

impl PdfProfile {
    /// Returns the PDF version required by this profile.
    ///
    /// ```
    /// assert_eq!(quire::PdfProfile::PdfA1B.pdf_version(), (1, 4));
    /// ```
    pub const fn pdf_version(self) -> (u8, u8) {
        match self {
            Self::Pdf | Self::PdfA1B => (1, 4),
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
        }
    }
}

impl fmt::Display for PdfProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pdf => "pdf",
            Self::PdfA1B => "pdf/a-1b",
        })
    }
}

impl FromStr for PdfProfile {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "pdf" => Ok(Self::Pdf),
            "pdf/a-1b" => Ok(Self::PdfA1B),
            _ => Err(format!(
                "unsupported PDF profile {value:?}; expected one of pdf, pdf/a-1b"
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
///
/// ```no_run
/// use quire::{Html, PdfOptions, PdfProfile, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let pdf_options = PdfOptions {
///     profile: PdfProfile::Pdf,
///     producer: "Example report service".to_string(),
///     ..PdfOptions::default()
/// };
/// let document = Html::from_file("document.html")
///     .await?
///     .render(&RenderOptions::default())
///     .await?;
/// let mut output = File::create("document.pdf")?;
/// document.write_pdf(&mut output, &pdf_options)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfOptions {
    /// PDF profile and conformance-identification metadata to emit.
    pub profile: PdfProfile,
    /// Font-program embedding policy.
    pub font_embedding: FontEmbeddingMode,
    /// Generated-stream compression policy.
    pub compression: PdfCompression,
    /// Producer string written to PDF document information and, when emitted,
    /// XMP metadata.
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
///
/// ```no_run
/// use quire::{Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let document = Html::from_string(
///     "<title>Quarterly report</title><meta name=author content=Quire>",
/// )
/// .render(&RenderOptions::default())
/// .await?;
/// if let Some(title) = document.metadata().title() {
///     println!("Rendering {title}");
/// }
/// let mut output = File::create("document.pdf")?;
/// document.write_pdf(&mut output, &PdfOptions::default())?;
/// # Ok(())
/// # }
/// ```
pub struct DocumentMetadata {
    pub(crate) title: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) creator: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) keywords: Vec<String>,
    pub(crate) created: Option<DocumentDate>,
    pub(crate) modified: Option<DocumentDate>,
}

impl DocumentMetadata {
    /// Returns whether the source document supplied any metadata property.
    ///
    /// A PDF producer is writer metadata rather than source metadata and is
    /// therefore deliberately excluded. Ordinary PDFs may omit their XMP
    /// packet when this is false, while PDF/A always requires XMP metadata.
    pub(crate) fn has_source_metadata(&self) -> bool {
        self.title.is_some()
            || self.author.is_some()
            || self.creator.is_some()
            || self.language.is_some()
            || self.description.is_some()
            || !self.keywords.is_empty()
            || self.created.is_some()
            || self.modified.is_some()
    }

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

    /// Returns the document language from the root HTML element, if specified.
    ///
    /// ```no_run
    /// # fn inspect(metadata: &quire::DocumentMetadata) {
    /// let language = metadata.language();
    /// # let _ = language;
    /// # }
    /// ```
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    /// Returns the document description, if one was present in the source.
    ///
    /// ```no_run
    /// # fn inspect(metadata: &quire::DocumentMetadata) {
    /// let description = metadata.description();
    /// # let _ = description;
    /// # }
    /// ```
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns the document keywords in source order.
    ///
    /// ```no_run
    /// # fn inspect(metadata: &quire::DocumentMetadata) {
    /// for keyword in metadata.keywords() {
    ///     println!("{keyword}");
    /// }
    /// # }
    /// ```
    pub fn keywords(&self) -> &[String] {
        &self.keywords
    }

    /// Returns the document creation date, if one was present in the source.
    ///
    /// ```no_run
    /// # fn inspect(metadata: &quire::DocumentMetadata) {
    /// let created = metadata.created();
    /// # let _ = created;
    /// # }
    /// ```
    pub fn created(&self) -> Option<&DocumentDate> {
        self.created.as_ref()
    }

    /// Returns the document modification date, if one was present in the source.
    ///
    /// ```no_run
    /// # fn inspect(metadata: &quire::DocumentMetadata) {
    /// let modified = metadata.modified();
    /// # let _ = modified;
    /// # }
    /// ```
    pub fn modified(&self) -> Option<&DocumentDate> {
        self.modified.as_ref()
    }
}

/// A date in [W3C's ISO 8601 profile] extracted from document metadata.
///
/// It retains the source representation while storing validated date
/// components for PDF serialization.
///
/// [W3C's ISO 8601 profile]: <https://www.w3.org/TR/NOTE-datetime>
///
/// ```no_run
/// use quire::{Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let document = Html::from_string(
///     r#"<meta name="dcterms.created" content="2026-08-08">"#,
/// )
/// .render(&RenderOptions::default())
/// .await?;
/// if let Some(created) = document.metadata().created() {
///     println!("Created on {created}");
/// }
/// let mut output = File::create("document.pdf")?;
/// document.write_pdf(&mut output, &PdfOptions::default())?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentDate {
    source: String,
    components: DocumentDateComponents,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DocumentDateComponents {
    Year {
        year: u16,
    },
    YearMonth {
        year: u16,
        month: u8,
    },
    Date {
        year: u16,
        month: u8,
        day: u8,
    },
    DateTime {
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        offset_seconds: i32,
        offset_is_negative: bool,
    },
}

impl DocumentDate {
    /// Returns the validated source date in W3C ISO 8601 profile form.
    ///
    /// ```no_run
    /// # fn inspect(date: &quire::DocumentDate) {
    /// println!("{}", date.as_str());
    /// # }
    /// ```
    pub fn as_str(&self) -> &str {
        &self.source
    }

    pub(crate) fn parse(source: String) -> Option<Self> {
        let value = source.trim_matches(is_html_space);
        is_w3c_iso_8601_profile_form(value)?;
        let datetime_value = value
            .strip_suffix('Z')
            .map(|prefix| format!("{prefix}+00:00"))
            .unwrap_or_else(|| value.to_string());
        let offset_is_negative = datetime_value
            .as_bytes()
            .get(datetime_value.len().saturating_sub(6))
            .is_some_and(|sign| *sign == b'-');

        if let Some(components) = Self::parse_datetime(
            &datetime_value,
            "%Y-%m-%dT%H:%M:%S%.f%:z",
            offset_is_negative,
        ) {
            return Some(Self { source, components });
        }
        if let Some(components) =
            Self::parse_datetime(&datetime_value, "%Y-%m-%dT%H:%M%:z", offset_is_negative)
        {
            return Some(Self { source, components });
        }
        if let Some(components) = Self::parse_date(value, "%F") {
            return Some(Self { source, components });
        }
        if let Some(components) = Self::parse_year_month(value) {
            return Some(Self { source, components });
        }
        Self::parse_year(value).map(|components| Self { source, components })
    }

    pub(crate) fn pdf_info_value(&self) -> String {
        match self.components {
            DocumentDateComponents::Year { year } => format!("D:{year:04}"),
            DocumentDateComponents::YearMonth { year, month } => {
                format!("D:{year:04}{month:02}")
            }
            DocumentDateComponents::Date { year, month, day } => {
                format!("D:{year:04}{month:02}{day:02}")
            }
            DocumentDateComponents::DateTime {
                year,
                month,
                day,
                hour,
                minute,
                second,
                offset_seconds,
                offset_is_negative,
            } => {
                let offset = if offset_seconds == 0 && !offset_is_negative {
                    "Z".to_string()
                } else {
                    let sign = if offset_seconds < 0 || offset_is_negative {
                        '-'
                    } else {
                        '+'
                    };
                    let offset_minutes = offset_seconds.unsigned_abs() / 60;
                    format!(
                        "{sign}{:02}'{:02}",
                        offset_minutes / 60,
                        offset_minutes % 60
                    )
                };
                format!("D:{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}{offset}")
            }
        }
    }

    fn parse_datetime(
        value: &str,
        format: &str,
        offset_is_negative: bool,
    ) -> Option<DocumentDateComponents> {
        let parsed = BrokenDownTime::parse(format, value).ok()?;
        let (year, month, day) = parsed_date_parts(&parsed)?;
        Date::new(
            i16::try_from(year).ok()?,
            i8::try_from(month).ok()?,
            i8::try_from(day).ok()?,
        )
        .ok()?;
        Some(DocumentDateComponents::DateTime {
            year,
            month,
            day,
            hour: u8::try_from(parsed.hour()?).ok()?,
            minute: u8::try_from(parsed.minute()?).ok()?,
            second: u8::try_from(parsed.second().unwrap_or(0)).ok()?,
            offset_seconds: parsed.offset()?.seconds(),
            offset_is_negative,
        })
    }

    fn parse_date(value: &str, format: &str) -> Option<DocumentDateComponents> {
        let parsed = BrokenDownTime::parse(format, value).ok()?;
        let (year, month, day) = parsed_date_parts(&parsed)?;
        Date::new(
            i16::try_from(year).ok()?,
            i8::try_from(month).ok()?,
            i8::try_from(day).ok()?,
        )
        .ok()?;
        Some(DocumentDateComponents::Date { year, month, day })
    }

    fn parse_year_month(value: &str) -> Option<DocumentDateComponents> {
        let parsed = BrokenDownTime::parse("%Y-%m", value).ok()?;
        Some(DocumentDateComponents::YearMonth {
            year: u16::try_from(parsed.year()?).ok()?,
            month: u8::try_from(parsed.month()?).ok()?,
        })
    }

    fn parse_year(value: &str) -> Option<DocumentDateComponents> {
        let parsed = BrokenDownTime::parse("%Y", value).ok()?;
        Some(DocumentDateComponents::Year {
            year: u16::try_from(parsed.year()?).ok()?,
        })
    }
}

impl fmt::Display for DocumentDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn parsed_date_parts(parsed: &BrokenDownTime) -> Option<(u16, u8, u8)> {
    Some((
        u16::try_from(parsed.year()?).ok()?,
        u8::try_from(parsed.month()?).ok()?,
        u8::try_from(parsed.day()?).ok()?,
    ))
}

fn is_html_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\u{000C}' | '\r')
}

/// Checks the lexical forms admitted by the W3C ISO 8601 profile before the
/// time parser validates their calendar values.
/// <https://www.w3.org/TR/NOTE-datetime>
fn is_w3c_iso_8601_profile_form(value: &str) -> Option<()> {
    fn digits(value: &[u8]) -> bool {
        value.iter().all(u8::is_ascii_digit)
    }

    let bytes = value.as_bytes();
    match bytes.len() {
        4 if digits(bytes) => Some(()),
        7 if digits(&bytes[..4]) && bytes[4] == b'-' && digits(&bytes[5..]) => Some(()),
        10 if digits(&bytes[..4])
            && bytes[4] == b'-'
            && digits(&bytes[5..7])
            && bytes[7] == b'-'
            && digits(&bytes[8..]) =>
        {
            Some(())
        }
        _ => is_w3c_iso_8601_profile_datetime_form(bytes),
    }
}

fn is_w3c_iso_8601_profile_datetime_form(bytes: &[u8]) -> Option<()> {
    // The profile requires a full calendar date, a minute-precision time, and
    // a timezone. Seconds and a fractional seconds component are optional.
    if bytes.len() < 17
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || bytes.get(4) != Some(&b'-')
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || bytes.get(7) != Some(&b'-')
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
        || bytes.get(10) != Some(&b'T')
        || !bytes[11..13].iter().all(u8::is_ascii_digit)
        || bytes.get(13) != Some(&b':')
        || !bytes[14..16].iter().all(u8::is_ascii_digit)
    {
        return None;
    }

    let time_end = match bytes.get(16) {
        Some(b'Z') if bytes.len() == 17 => return Some(()),
        Some(b'+') | Some(b'-') => 16,
        Some(b':') if bytes.len() >= 19 && bytes[17..19].iter().all(u8::is_ascii_digit) => {
            let mut end = 19;
            if bytes.get(end) == Some(&b'.') {
                end += 1;
                let fraction_start = end;
                while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                    end += 1;
                }
                if end == fraction_start {
                    return None;
                }
            }
            end
        }
        _ => return None,
    };

    match bytes.get(time_end..) {
        Some([b'Z']) => Some(()),
        Some(
            [
                sign @ (b'+' | b'-'),
                hour_tens,
                hour_ones,
                b':',
                minute_tens,
                minute_ones,
            ],
        ) if sign.is_ascii()
            && hour_tens.is_ascii_digit()
            && hour_ones.is_ascii_digit()
            && minute_tens.is_ascii_digit()
            && minute_ones.is_ascii_digit() =>
        {
            Some(())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::DocumentDate;

    #[test]
    fn metadata_dates_accept_w3c_iso_8601_profile_forms() {
        let cases = [
            ("1997", "D:1997"),
            ("1997-07", "D:199707"),
            ("1997-07-16", "D:19970716"),
            ("1997-07-16T19:20+01:00", "D:19970716192000+01'00"),
            ("1997-07-16T19:20:30Z", "D:19970716192030Z"),
            ("1997-07-16T19:20:30.45-00:30", "D:19970716192030-00'30"),
        ];

        for (source, expected_pdf_date) in cases {
            let date = DocumentDate::parse(source.to_string()).expect("valid W3C profile date");
            assert_eq!(date.as_str(), source);
            assert_eq!(date.pdf_info_value(), expected_pdf_date);
        }
    }

    #[test]
    fn metadata_dates_reject_non_profile_values() {
        for source in [
            "1997-7-16",
            "1997-07-16T19:20",
            "1997-07-16T19:20:30+0100",
            "1997-02-30",
        ] {
            assert!(
                DocumentDate::parse(source.to_string()).is_none(),
                "{source:?} must not be accepted"
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// A document bookmark derived from the rendered source.
///
/// ```no_run
/// use quire::{Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let document = Html::from_string("<h1>Introduction</h1>")
///     .render(&RenderOptions::default())
///     .await?;
/// for bookmark in document.bookmarks() {
///     println!("{} on page {}", bookmark.label(), bookmark.page_index() + 1);
/// }
/// let mut output = File::create("document.pdf")?;
/// document.write_pdf(&mut output, &PdfOptions::default())?;
/// # Ok(())
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

    /// Internal replay boundary for projecting a captured source bookmark to
    /// its committed fragment without exposing mutable destinations publicly.
    pub(crate) fn replay_target(&self) -> PaintPoint {
        self.target
    }

    /// Internal replay boundary for assigning the final page and paint-space
    /// destination of a captured bookmark.
    pub(crate) fn set_replay_destination(&mut self, page_index: usize, target: PaintPoint) {
        self.page_index = page_index;
        self.target = target;
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
/// ```no_run
/// use quire::{BookmarkState, Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let document = Html::from_string(
///     "<style>h1 { bookmark-state: closed }</style><h1>Introduction</h1>",
/// )
/// .render(&RenderOptions::default())
/// .await?;
/// assert_eq!(document.bookmarks()[0].state(), BookmarkState::Closed);
/// let mut output = File::create("document.pdf")?;
/// document.write_pdf(&mut output, &PdfOptions::default())?;
/// # Ok(())
/// # }
/// ```
pub enum BookmarkState {
    /// Display the bookmark's children initially.
    Open,
    /// Hide the bookmark's children initially.
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
/// A renderer-owned page in document order.
pub(crate) struct Page {
    size: PaintSize,
    pub(crate) rotation: i32,
    pub(crate) rects: Vec<RenderedRect>,
    pub(crate) rounded_rects: Vec<RenderedRoundedRect>,
    pub(crate) paths: Vec<RenderedPath>,
    pub(crate) strokes: Vec<RenderedStroke>,
    pub(crate) lines: Vec<RenderedLine>,
    pub(crate) links: Vec<RenderedLink>,
    pub(crate) images: Vec<RenderedImage>,
    /// Image sources owned by retained SVG paint-server scenes. They are
    /// resource inventory, not page-level paint operations.
    pub(crate) svg_pattern_images: Vec<RenderedImage>,
    pub(crate) image_patterns: Vec<RenderedImagePattern>,
    pub(crate) gradient_patterns: Vec<RenderedGradientPattern>,
    pub(crate) svg_patterns: Vec<RenderedSvgPattern>,
    pub(crate) opaque_text_coverages: Vec<OpaqueTextCoverage>,
    pub(crate) svg_text_outlines: Vec<SvgTextOutline>,
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
            svg_pattern_images: Vec::new(),
            image_patterns: Vec::new(),
            gradient_patterns: Vec::new(),
            svg_patterns: Vec::new(),
            opaque_text_coverages: Vec::new(),
            svg_text_outlines: Vec::new(),
            has_fragmentation_content: false,
            paint_tree: PagePaintTree::new(),
        }
    }

    /// Returns the page width in PDF points.
    pub(crate) fn width(&self) -> f32 {
        self.size.width
    }

    /// Returns the page height in PDF points.
    pub(crate) fn height(&self) -> f32 {
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

    pub(crate) fn links(&self) -> &[LinkAnnotation] {
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
    pub(crate) fn rotation(&self) -> i32 {
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

    /// Whether final PDF lowering needs an isolated transparency Form for
    /// this page.  The answer is taken from the retained paint tree so a
    /// colourless CSS group is still visible to PDF resource planning.
    pub(crate) fn has_transparency_group(&self) -> bool {
        self.paint_tree.has_transparency_group()
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
                collect_svg_scene_colors(&pattern.scene, colors);
            }
        }
    }
}

fn collect_svg_scene_colors(scene: &crate::svg::SvgPaintGroup, colors: &mut Vec<CssColor>) {
    for item in &scene.items {
        match item {
            crate::svg::SvgPaintItem::Path(path) => collect_path_colors(path, colors),
            crate::svg::SvgPaintItem::Group(group) | crate::svg::SvgPaintItem::NestedSvg(group) => {
                collect_svg_scene_colors(group, colors)
            }
            crate::svg::SvgPaintItem::OutlinedText(outlined) => {
                collect_svg_scene_colors(&outlined.content, colors)
            }
            crate::svg::SvgPaintItem::RasterImage(_) => {}
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

/// PDF-visible faux-oblique angle selected while matching a CSS font face.
///
/// Fontique reports faux oblique synthesis in whole degrees. Retaining that
/// representation avoids conflating the requested CSS `font-style` with the
/// paint-only transform required when no matching face exists.
/// <https://www.w3.org/TR/css-fonts-4/#font-style-prop>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SyntheticObliqueAngle(i8);

impl SyntheticObliqueAngle {
    pub(crate) const fn from_fontique_degrees(degrees: i8) -> Option<Self> {
        if degrees == 0 {
            None
        } else {
            Some(Self(degrees))
        }
    }

    pub(crate) const fn degrees(self) -> i8 {
        self.0
    }
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
    pub(crate) oblique: Option<SyntheticObliqueAngle>,
}

/// The effective OpenType variation coordinates used to shape one document font.
///
/// PDF Type 0 fonts do not retain a portable per-text-run variable-font
/// location. The PDF writer therefore materializes this exact instance before
/// embedding it. Values retain their IEEE representation so document-font
/// identity and the shaping backend cannot merge distinct authored axis
/// settings.
/// <https://www.w3.org/TR/css-fonts-4/#font-variation-settings-def>
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct DocumentFontVariationCoordinates(pub(crate) Vec<([u8; 4], u32)>);

impl DocumentFontVariationCoordinates {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// An OpenType `BASE` coordinate retained in font design units.
///
/// The optional delta-set index belongs to a `BaseCoord` format 3 record and
/// is resolved against the font's selected normalized variation coordinates at
/// layout time. Quire lays out PDF text without device hinting, so format 2's
/// nominal design coordinate is retained without its ppem-specific point
/// adjustment.
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/base#basecoord-tables>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenTypeBaselineCoordinate {
    pub(crate) design_units: i16,
    pub(crate) variation_index: Option<OpenTypeVariationIndex>,
}

/// An OpenType ItemVariationStore delta-set index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenTypeVariationIndex {
    pub(crate) outer: u16,
    pub(crate) inner: u16,
}

/// The `BASE` values for one OpenType script in one typographic axis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenTypeBaselineScript {
    pub(crate) script: [u8; 4],
    pub(crate) default_baseline: Option<[u8; 4]>,
    /// Coordinates retain unrecognized tags as well as CSS-used metrics so
    /// script-specific selection never depends on a lossy parse boundary.
    pub(crate) coordinates: Vec<([u8; 4], OpenTypeBaselineCoordinate)>,
}

/// One directional OpenType `BASE` axis.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OpenTypeBaselineAxis {
    pub(crate) scripts: Vec<OpenTypeBaselineScript>,
}

/// Horizontal and vertical OpenType `BASE` axes retained for layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OpenTypeBaselineTable {
    pub(crate) horizontal: OpenTypeBaselineAxis,
    pub(crate) vertical: OpenTypeBaselineAxis,
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
    pub(crate) baselines: OpenTypeBaselineTable,
    pub(crate) variation_coordinates: DocumentFontVariationCoordinates,
    pub(crate) synthesis: DocumentFontSynthesis,
}
