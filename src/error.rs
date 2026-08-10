/// Quire's standard result type.
///
/// ```no_run
/// use quire::{Html, PdfOptions, RenderOptions, Result};
/// use std::fs::File;
///
/// async fn render() -> Result<()> {
///     let html = Html::from_file("document.html").await?;
///     let mut output = File::create("document.pdf")?;
///     html.write_pdf(&mut output, &RenderOptions::default(), &PdfOptions::default())
///         .await
/// }
/// ```
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
/// An error returned while loading, rendering, or serializing a document.
///
/// ```no_run
/// use quire::{Error, Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> Result<(), Error> {
/// let html = Html::from_file("document.html").await?;
/// let mut output = File::create("document.pdf")?;
/// if let Err(error) = html
///     .write_pdf(&mut output, &RenderOptions::default(), &PdfOptions::default())
///     .await
/// {
///     eprintln!("could not create PDF: {error}");
///     return Err(error);
/// }
/// # Ok(())
/// # }
/// ```
pub enum Error {
    #[error(transparent)]
    /// An underlying filesystem error.
    Io(#[from] std::io::Error),

    #[error("{0}")]
    /// Input that Quire cannot process.
    InvalidInput(String),

    /// A painted font cannot be represented by the selected PDF font policy.
    ///
    /// PDF text operators must reference an embedded font program with a
    /// matching CID mapping.  Returning an error prevents emitting a PDF that
    /// contains a known-invalid or empty font resource.
    #[error("cannot embed painted font {font:?}: {reason}")]
    FontEmbedding {
        /// The font's PostScript name.
        font: String,
        /// Why embedding this font failed.
        reason: String,
    },
}
