const WOFF_SIGNATURE: &[u8; 4] = b"wOFF";
const WOFF2_SIGNATURE: &[u8; 4] = b"wOF2";
const TOTAL_SFNT_SIZE_OFFSET: usize = 16;
const TOTAL_SFNT_SIZE_END: usize = TOTAL_SFNT_SIZE_OFFSET + size_of::<u32>();
/// WOFF data is untrusted font input. Bound the reconstructed program before
/// decoding so a small compressed resource cannot request an impractically
/// large allocation.
const MAX_RECONSTRUCTED_SFNT_LEN: usize = 256 * 1024 * 1024;

/// Decode WOFF 1.0 or WOFF2 font data into an sfnt font program.
///
/// WOFF 1.0 and WOFF2 store OpenType/TrueType sfnt tables in compressed
/// containers; font matching and PDF embedding consume the reconstructed sfnt
/// bytes:
/// <https://www.w3.org/TR/WOFF/#Conform-mustReconstruct> and
/// <https://www.w3.org/TR/WOFF2/#conform-mustReconstruct> require decoders to
/// reconstruct equivalent input font data for downstream font consumers.
pub(super) fn decode_if_woff(data: Vec<u8>) -> Vec<u8> {
    let (format, decode) = if data.starts_with(WOFF_SIGNATURE) {
        ("WOFF", wuff::decompress_woff1 as fn(&[u8]) -> _)
    } else if data.starts_with(WOFF2_SIGNATURE) {
        ("WOFF2", wuff::decompress_woff2 as fn(&[u8]) -> _)
    } else {
        return data;
    };
    if let Some(size) = declared_reconstructed_size(&data)
        && size > MAX_RECONSTRUCTED_SFNT_LEN
    {
        log::warn!(
            "refusing to decode {format} font with declared {size}-byte sfnt program; limit is {MAX_RECONSTRUCTED_SFNT_LEN} bytes"
        );
        return data;
    }
    match decode(&data) {
        Ok(decoded) => decoded,
        Err(error) => {
            log::warn!("failed to decode {format} font: {error}");
            data
        }
    }
}

fn declared_reconstructed_size(data: &[u8]) -> Option<usize> {
    let size = data.get(TOTAL_SFNT_SIZE_OFFSET..TOTAL_SFNT_SIZE_END)?;
    Some(u32::from_be_bytes(size.try_into().expect("fixed-size slice")) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_fixture_decodes(data: &[u8], signature: &[u8; 4]) {
        assert!(data.starts_with(signature));
        let decoded = decode_if_woff(data.to_vec());
        assert!(!decoded.starts_with(signature));
        ttf_parser::Face::parse(&decoded, 0).expect("decoded font fixture must be valid sfnt");
    }

    fn assert_reconstructed_size_limit(data: &[u8]) {
        let mut oversized = data.to_vec();
        oversized[TOTAL_SFNT_SIZE_OFFSET..TOTAL_SFNT_SIZE_END]
            .copy_from_slice(&((MAX_RECONSTRUCTED_SFNT_LEN + 1) as u32).to_be_bytes());
        assert_eq!(decode_if_woff(oversized.clone()), oversized);
    }

    #[test]
    fn leaves_non_woff_font_data_unchanged() {
        let data = b"\0\x01\0\0font".to_vec();
        assert_eq!(decode_if_woff(data.clone()), data);
    }

    #[test]
    fn malformed_woff_returns_original_data() {
        let data = b"wOFFbad".to_vec();
        assert_eq!(decode_if_woff(data.clone()), data);
    }

    #[test]
    fn malformed_woff2_returns_original_data() {
        let data = b"wOF2bad".to_vec();
        assert_eq!(decode_if_woff(data.clone()), data);
    }

    #[test]
    fn decodes_woff1_fixture() {
        assert_fixture_decodes(
            include_bytes!("../../../tests/resources/fonts/noto-sans-v8-latin-regular.woff"),
            WOFF_SIGNATURE,
        );
    }

    #[test]
    fn decodes_woff2_fixture() {
        assert_fixture_decodes(
            include_bytes!("../../../tests/resources/fonts/NotoNaskhArabic-regular.woff2"),
            WOFF2_SIGNATURE,
        );
    }

    #[test]
    fn reconstructed_size_limit_applies_to_woff1() {
        assert_reconstructed_size_limit(include_bytes!(
            "../../../tests/resources/fonts/noto-sans-v8-latin-regular.woff"
        ));
    }

    #[test]
    fn reconstructed_size_limit_applies_to_woff2() {
        assert_reconstructed_size_limit(include_bytes!(
            "../../../tests/resources/fonts/NotoNaskhArabic-regular.woff2"
        ));
    }
}
