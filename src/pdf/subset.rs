#[cfg(has_harfbuzz_subset)]
mod harfbuzz {
    use std::ffi::{c_char, c_int, c_uint, c_void};

    enum HbBlob {}
    enum HbFace {}
    enum HbSet {}
    enum HbSubsetInput {}

    type HbTag = u32;

    const HB_MEMORY_MODE_READONLY: c_uint = 1;
    const HB_SUBSET_SETS_GLYPH_INDEX: c_uint = 0;
    const HB_SUBSET_SETS_DROP_TABLE_TAG: c_uint = 3;
    const HB_SUBSET_FLAGS_NO_HINTING: c_uint = 0x0000_0001;
    const HB_SUBSET_FLAGS_RETAIN_GIDS: c_uint = 0x0000_0002;
    const HB_SUBSET_FLAGS_DESUBROUTINIZE: c_uint = 0x0000_0004;
    const HB_SUBSET_FLAGS_PASSTHROUGH_UNRECOGNIZED: c_uint = 0x0000_0020;

    unsafe extern "C" {
        fn hb_blob_create(
            data: *const c_char,
            length: c_uint,
            mode: c_uint,
            user_data: *mut c_void,
            destroy: Option<unsafe extern "C" fn(*mut c_void)>,
        ) -> *mut HbBlob;
        fn hb_blob_destroy(blob: *mut HbBlob);
        fn hb_blob_get_data(blob: *mut HbBlob, length: *mut c_uint) -> *const c_char;
        fn hb_face_create(blob: *mut HbBlob, index: c_uint) -> *mut HbFace;
        fn hb_face_destroy(face: *mut HbFace);
        fn hb_face_reference_blob(face: *mut HbFace) -> *mut HbBlob;
        fn hb_subset_input_create_or_fail() -> *mut HbSubsetInput;
        fn hb_subset_input_destroy(input: *mut HbSubsetInput);
        fn hb_subset_input_glyph_set(input: *mut HbSubsetInput) -> *mut HbSet;
        fn hb_subset_input_set(input: *mut HbSubsetInput, set_type: c_uint) -> *mut HbSet;
        fn hb_subset_input_set_flags(input: *mut HbSubsetInput, flags: c_uint);
        fn hb_subset_or_fail(face: *mut HbFace, input: *const HbSubsetInput) -> *mut HbFace;
        fn hb_set_add_sorted_array(
            set: *mut HbSet,
            sorted_codepoints: *const u32,
            num_codepoints: c_int,
        );
        fn hb_tag_from_string(str: *const c_char, len: c_int) -> HbTag;
    }

    pub(super) fn subset_font(
        data: &[u8],
        face_index: u32,
        used_glyphs: &[u16],
    ) -> Option<Vec<u8>> {
        let max_glyph = used_glyphs.iter().copied().max()?;
        // WeasyPrint subsets with retained CIDs, then runs a no-hinting pass.
        // PDF text uses glyph IDs as CIDs through Identity-H, so retaining GIDs
        // keeps our already-shaped glyph stream valid after subsetting.
        let used = used_glyphs
            .iter()
            .copied()
            .map(u32::from)
            .collect::<Vec<_>>();
        let dense = (0..=u32::from(max_glyph)).collect::<Vec<_>>();
        unsafe {
            let blob = hb_blob_create(
                data.as_ptr().cast(),
                data.len().try_into().ok()?,
                HB_MEMORY_MODE_READONLY,
                std::ptr::null_mut(),
                None,
            );
            if blob.is_null() {
                return None;
            }
            let face = hb_face_create(blob, face_index);
            hb_blob_destroy(blob);
            if face.is_null() {
                return None;
            }

            let first = subset_face(
                face,
                &used,
                HB_SUBSET_FLAGS_RETAIN_GIDS
                    | HB_SUBSET_FLAGS_PASSTHROUGH_UNRECOGNIZED
                    | HB_SUBSET_FLAGS_DESUBROUTINIZE,
                true,
            );
            hb_face_destroy(face);
            let first = first?;
            let second = subset_face(
                first,
                &dense,
                HB_SUBSET_FLAGS_NO_HINTING
                    | HB_SUBSET_FLAGS_PASSTHROUGH_UNRECOGNIZED
                    | HB_SUBSET_FLAGS_DESUBROUTINIZE,
                false,
            );
            hb_face_destroy(first);
            let second = second?;
            let output = face_bytes(second);
            hb_face_destroy(second);
            output
        }
    }

    unsafe fn subset_face(
        face: *mut HbFace,
        glyphs: &[u32],
        flags: c_uint,
        drop_tables: bool,
    ) -> Option<*mut HbFace> {
        let input = unsafe { hb_subset_input_create_or_fail() };
        if input.is_null() {
            return None;
        }

        let glyph_set = unsafe { hb_subset_input_glyph_set(input) };
        unsafe {
            hb_set_add_sorted_array(glyph_set, glyphs.as_ptr(), glyphs.len().try_into().ok()?);
            hb_subset_input_set_flags(input, flags);
        }
        if drop_tables {
            let drop_set = unsafe { hb_subset_input_set(input, HB_SUBSET_SETS_DROP_TABLE_TAG) };
            let drop_tags = [
                tag(b"BASE"),
                tag(b"DSIG"),
                tag(b"EBDT"),
                tag(b"EBLC"),
                tag(b"EBSC"),
                tag(b"GPOS"),
                tag(b"GSUB"),
                tag(b"JSTF"),
                tag(b"LTSH"),
                tag(b"PCLT"),
                tag(b"SVG "),
            ];
            unsafe {
                hb_set_add_sorted_array(
                    drop_set,
                    drop_tags.as_ptr(),
                    drop_tags.len().try_into().ok()?,
                );
            }
        }

        // Keep the glyph-index set active; this documents that we are not using
        // Unicode closure for PDF CIDs.
        let _ = HB_SUBSET_SETS_GLYPH_INDEX;
        let subset = unsafe { hb_subset_or_fail(face, input) };
        unsafe {
            hb_subset_input_destroy(input);
        }
        (!subset.is_null()).then_some(subset)
    }

    fn tag(bytes: &[u8; 4]) -> u32 {
        unsafe { hb_tag_from_string(bytes.as_ptr().cast(), 4) }
    }

    unsafe fn face_bytes(face: *mut HbFace) -> Option<Vec<u8>> {
        let blob = unsafe { hb_face_reference_blob(face) };
        if blob.is_null() {
            return None;
        }
        let mut length = 0u32;
        let data = unsafe { hb_blob_get_data(blob, &mut length) };
        let output = if data.is_null() {
            None
        } else {
            let slice = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length as usize) };
            Some(slice.to_vec())
        };
        unsafe {
            hb_blob_destroy(blob);
        }
        output
    }
}

#[cfg(has_harfbuzz_subset)]
pub(super) fn subset_font(data: &[u8], face_index: u32, used_glyphs: &[u16]) -> Option<Vec<u8>> {
    harfbuzz::subset_font(data, face_index, used_glyphs)
}

#[cfg(not(has_harfbuzz_subset))]
pub(super) fn subset_font(_data: &[u8], _face_index: u32, _used_glyphs: &[u16]) -> Option<Vec<u8>> {
    None
}
