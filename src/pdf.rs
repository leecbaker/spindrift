use crate::document::FontProgramKind;
use crate::{
    Bookmark, BookmarkState, Color, Document, DocumentFont, Page, RenderedImage, RenderedLink,
    RenderedPath, RenderedPathCommand, RenderedPathFillRule, RenderedRoundedRect,
};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ImageKey {
    pixel_width: u32,
    pixel_height: u32,
    interpolate: bool,
    rgb: Vec<u8>,
    alpha: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImageObjectIds {
    image_id: usize,
    alpha_mask_id: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
struct PageContentRender {
    stream: Vec<u8>,
    form_xobjects: Vec<FormXObjectRender>,
}

#[derive(Debug, Clone, PartialEq)]
struct FormXObjectRender {
    id: usize,
    name: String,
    bbox: crate::document::PaintClip,
    stream: Vec<u8>,
}

const EMBEDDED_FONT_OBJECTS: usize = 5;

#[derive(Debug, Clone, PartialEq)]
struct ShapedDocument {
    pages: Vec<Vec<Option<ShapedLine>>>,
}

#[derive(Debug, Clone, PartialEq)]
struct ShapedLine {
    runs: Vec<ShapedRun>,
}

#[derive(Debug, Clone, PartialEq)]
struct ShapedRun {
    document_font_id: usize,
    x_offset: f32,
    font_size: f32,
    glyphs: Vec<ShapedGlyph>,
}

#[derive(Debug, Clone, PartialEq)]
struct ShapedGlyph {
    id: u16,
    x_advance: f32,
    nominal_x_advance: f32,
    x_offset: f32,
    y_offset: f32,
    unicode: String,
}

#[derive(Debug, Clone, PartialEq)]
struct EmbeddedFontPlan<'a> {
    font: &'a DocumentFont,
    resource_name: String,
    base_name: String,
    type0_id: usize,
    cid_font_id: usize,
    descriptor_id: usize,
    file_id: usize,
    to_unicode_id: usize,
    used_glyphs: BTreeMap<u16, String>,
}

#[derive(Debug, Clone, PartialEq)]
struct EmbeddedFontPlans<'a> {
    fonts: Vec<EmbeddedFontPlan<'a>>,
    document_font_to_embedded_font: Vec<Option<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EmbeddedFontKey {
    blob_id: u64,
    face_index: u32,
    program_kind: FontProgramKind,
    post_script_name: String,
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    cap_height: i16,
    italic_angle: i16,
    bbox: [i16; 4],
}

#[derive(Debug, Clone, PartialEq)]
struct BookmarkTreeNode {
    bookmark: Bookmark,
    children: Vec<BookmarkTreeNode>,
}

#[derive(Debug, Clone, PartialEq)]
struct OutlinePlan {
    root_id: usize,
    nodes: Vec<OutlineNodePlan>,
    visible_count: i32,
}

#[derive(Debug, Clone, PartialEq)]
struct OutlineNodePlan {
    id: usize,
    bookmark: Bookmark,
    parent_id: usize,
    prev_id: Option<usize>,
    next_id: Option<usize>,
    first_child_id: Option<usize>,
    last_child_id: Option<usize>,
    child_count: i32,
}

mod content;
mod fonts;
mod outlines;
mod resources;
mod shaping;
mod subset;
mod writer;

use content::*;
use fonts::*;
use outlines::*;
use resources::*;
use shaping::*;
pub(crate) use writer::write_document;

#[cfg(test)]
mod tests;
