const WOFF_SIGNATURE: &[u8; 4] = b"wOFF";
const WOFF2_SIGNATURE: &[u8; 4] = b"wOF2";
const WOFF_HEADER_LEN: usize = 44;
const WOFF_DIRECTORY_ENTRY_LEN: usize = 20;
const SFNT_HEADER_LEN: usize = 12;
const SFNT_DIRECTORY_ENTRY_LEN: usize = 16;

#[derive(Debug, thiserror::Error)]
pub(super) enum WoffError {
    #[error("WOFF data is shorter than the header")]
    TruncatedHeader,
    #[error("WOFF signature is missing")]
    InvalidSignature,
    #[error("WOFF reserved header field is nonzero")]
    InvalidReservedField,
    #[error("WOFF length field exceeds available data")]
    InvalidLength,
    #[error("WOFF table directory is truncated")]
    TruncatedDirectory,
    #[error("WOFF table data is outside the declared file length")]
    TableOutOfBounds,
    #[error("WOFF table decompressed to {actual} bytes, expected {expected}")]
    InvalidDecompressedLength { actual: usize, expected: usize },
    #[error("WOFF table decompression failed")]
    DecompressionFailed,
}

#[derive(Debug, Clone)]
struct WoffTable {
    tag: [u8; 4],
    compressed_offset: usize,
    compressed_len: usize,
    original_len: usize,
    checksum: u32,
    data: Vec<u8>,
    sfnt_offset: usize,
}

/// Decode WOFF 1.0 or WOFF2 font data into an sfnt font program.
///
/// WOFF 1.0 and WOFF2 store OpenType/TrueType sfnt tables in compressed
/// containers; font matching and PDF embedding consume the reconstructed sfnt
/// bytes:
/// <https://www.w3.org/TR/WOFF/#Conform-mustReconstruct> and
/// <https://www.w3.org/TR/WOFF2/#conform-mustReconstruct> require decoders to
/// reconstruct equivalent input font data for downstream font consumers.
pub(super) fn decode_if_woff(data: Vec<u8>) -> Vec<u8> {
    if data.starts_with(WOFF2_SIGNATURE) {
        return match woff2_patched::convert_woff2_to_ttf(&mut std::io::Cursor::new(&data)) {
            Ok(decoded) => decoded,
            Err(error) => {
                log::warn!("failed to decode WOFF2 font: {error}");
                data
            }
        };
    }
    if data.starts_with(WOFF_SIGNATURE) {
        match decode_woff(&data) {
            Ok(decoded) => decoded,
            Err(error) => {
                log::warn!("failed to decode WOFF font: {error}");
                data
            }
        }
    } else {
        data
    }
}

fn decode_woff(data: &[u8]) -> Result<Vec<u8>, WoffError> {
    if data.len() < WOFF_HEADER_LEN {
        return Err(WoffError::TruncatedHeader);
    }
    if &data[..4] != WOFF_SIGNATURE {
        return Err(WoffError::InvalidSignature);
    }

    let flavor = read_u32(data, 4);
    let declared_len = read_u32(data, 8) as usize;
    let table_count = read_u16(data, 12) as usize;
    let reserved = read_u16(data, 14);
    if reserved != 0 {
        return Err(WoffError::InvalidReservedField);
    }
    if declared_len > data.len() || declared_len < WOFF_HEADER_LEN {
        return Err(WoffError::InvalidLength);
    }

    let directory_len = table_count
        .checked_mul(WOFF_DIRECTORY_ENTRY_LEN)
        .and_then(|len| WOFF_HEADER_LEN.checked_add(len))
        .ok_or(WoffError::TruncatedDirectory)?;
    if directory_len > declared_len {
        return Err(WoffError::TruncatedDirectory);
    }

    let mut tables = Vec::with_capacity(table_count);
    for index in 0..table_count {
        let offset = WOFF_HEADER_LEN + index * WOFF_DIRECTORY_ENTRY_LEN;
        let tag = data[offset..offset + 4].try_into().unwrap();
        let compressed_offset = read_u32(data, offset + 4) as usize;
        let compressed_len = read_u32(data, offset + 8) as usize;
        let original_len = read_u32(data, offset + 12) as usize;
        let checksum = read_u32(data, offset + 16);
        let compressed_end = compressed_offset
            .checked_add(compressed_len)
            .ok_or(WoffError::TableOutOfBounds)?;
        if compressed_end > declared_len {
            return Err(WoffError::TableOutOfBounds);
        }
        let compressed = &data[compressed_offset..compressed_end];
        let table_data = if compressed_len == original_len {
            compressed.to_vec()
        } else {
            let decompressed =
                miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(compressed, original_len)
                    .map_err(|_| WoffError::DecompressionFailed)?;
            if decompressed.len() != original_len {
                return Err(WoffError::InvalidDecompressedLength {
                    actual: decompressed.len(),
                    expected: original_len,
                });
            }
            decompressed
        };
        if table_data.len() != original_len {
            return Err(WoffError::InvalidDecompressedLength {
                actual: table_data.len(),
                expected: original_len,
            });
        }
        tables.push(WoffTable {
            tag,
            compressed_offset,
            compressed_len,
            original_len,
            checksum,
            data: table_data,
            sfnt_offset: 0,
        });
    }

    tables.sort_by_key(|table| table.tag);
    let mut next_table_offset = SFNT_HEADER_LEN + tables.len() * SFNT_DIRECTORY_ENTRY_LEN;
    for table in &mut tables {
        next_table_offset = align_to_four(next_table_offset);
        table.sfnt_offset = next_table_offset;
        next_table_offset = next_table_offset
            .checked_add(table.original_len)
            .ok_or(WoffError::InvalidLength)?;
    }

    let mut output = vec![0; align_to_four(next_table_offset)];
    write_u32(&mut output, 0, flavor);
    write_u16(&mut output, 4, table_count as u16);
    let (search_range, entry_selector, range_shift) = sfnt_search_parameters(table_count);
    write_u16(&mut output, 6, search_range);
    write_u16(&mut output, 8, entry_selector);
    write_u16(&mut output, 10, range_shift);

    for (index, table) in tables.iter().enumerate() {
        let directory_offset = SFNT_HEADER_LEN + index * SFNT_DIRECTORY_ENTRY_LEN;
        output[directory_offset..directory_offset + 4].copy_from_slice(&table.tag);
        write_u32(&mut output, directory_offset + 4, table.checksum);
        write_u32(&mut output, directory_offset + 8, table.sfnt_offset as u32);
        write_u32(
            &mut output,
            directory_offset + 12,
            table.original_len as u32,
        );
        output[table.sfnt_offset..table.sfnt_offset + table.original_len]
            .copy_from_slice(&table.data);
        let _ = (table.compressed_offset, table.compressed_len);
    }

    Ok(output)
}

fn sfnt_search_parameters(table_count: usize) -> (u16, u16, u16) {
    let max_power = if table_count == 0 {
        0
    } else {
        1usize << (usize::BITS - 1 - table_count.leading_zeros())
    };
    let search_range = (max_power * SFNT_DIRECTORY_ENTRY_LEN).min(u16::MAX as usize) as u16;
    let entry_selector = if max_power == 0 {
        0
    } else {
        max_power.trailing_zeros() as u16
    };
    let range_shift = (table_count * SFNT_DIRECTORY_ENTRY_LEN)
        .saturating_sub(search_range as usize)
        .min(u16::MAX as usize) as u16;
    (search_range, entry_selector, range_shift)
}

fn align_to_four(value: usize) -> usize {
    (value + 3) & !3
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn write_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
