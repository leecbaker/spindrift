//! Small adapters at the `pdf_writer` boundary.

use pdf_writer::{Name, Ref};

pub(crate) fn pdf_ref(id: usize) -> Ref {
    Ref::new(i32_from_usize(id))
}

pub(crate) fn pdf_name(name: &str) -> Name<'_> {
    Name(name.as_bytes())
}

pub(crate) fn i32_from_usize(value: usize) -> i32 {
    i32::try_from(value).expect("PDF object value exceeds i32 range")
}

pub(crate) fn i32_from_u32(value: u32) -> i32 {
    i32::try_from(value).expect("PDF integer exceeds i32 range")
}
