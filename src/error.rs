/// Quire's standard result type.
///
/// ```
/// fn render() -> quire::Result<()> {
///     Ok(())
/// }
/// ```
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
/// An error returned while loading, rendering, or serializing a document.
///
/// ```
/// let error = quire::Error::InvalidInput("missing source".to_string());
/// assert!(error.to_string().contains("missing source"));
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
