//! Shared generated-stream encoding.

/// Bytes prepared for one generated PDF stream.
///
/// Keeping the bytes and the required filter together prevents one PDF stream
/// producer from applying `/FlateDecode` without first encoding its payload.
pub(crate) enum PdfStreamData<'a> {
    Flate(Vec<u8>),
    Raw(&'a [u8]),
}

impl PdfStreamData<'_> {
    pub(crate) fn bytes(&self) -> &[u8] {
        match self {
            Self::Flate(data) => data,
            Self::Raw(data) => data,
        }
    }

    pub(crate) const fn uses_flate(&self) -> bool {
        matches!(self, Self::Flate(_))
    }
}

/// Select the serialized bytes and PDF filter for a generated stream.
pub(crate) fn encode_pdf_stream(
    compression: crate::PdfCompression,
    data: &[u8],
) -> PdfStreamData<'_> {
    match compression {
        crate::PdfCompression::Compressed => PdfStreamData::Flate(flate_compress(data)),
        crate::PdfCompression::Uncompressed => PdfStreamData::Raw(data),
    }
}

/// Compress a PDF stream with the zlib wrapper required by `/FlateDecode`.
/// ISO 32000-1:2008, 7.4.4 defines the FlateDecode filter.
pub(crate) fn flate_compress(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}
