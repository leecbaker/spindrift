//! Serialization of an already-resolved private PDF program.
//!
//! These modules never inspect `Document`; lowering and planning have already
//! completed before they are invoked.

pub(super) mod fonts;
pub(super) mod images;
pub(super) mod pages;
pub(super) mod primitives;
pub(super) mod resources;
pub(super) mod stream;

pub(super) use fonts::write_embedded_fonts;
pub(super) use images::{write_image_patterns, write_images};
pub(super) use pages::{
    write_annotations, write_catalog, write_outlines, write_pages, write_pages_and_content,
};
pub(super) use primitives::*;
pub(super) use resources::write_resource_dictionary;
pub(super) use stream::*;
