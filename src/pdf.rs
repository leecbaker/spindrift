use crate::document::FontProgramKind;
use crate::{
    Bookmark, BookmarkState, Color, Document, DocumentFont, DocumentMetadata, Page, PdfVariant,
    RenderedImage, RenderedPath, RenderedPathCommand, RenderedPathFillRule, RenderedRoundedRect,
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
struct ExtGStateObjectPlan {
    id: usize,
    resource: ExtGStateResource,
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
const EMBEDDED_FONT_OBJECTS_WITH_CID_SET: usize = 6;

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
    y_offset: f32,
    text_matrix: crate::RenderedTextMatrix,
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
    cid_set_id: Option<usize>,
    used_glyphs: BTreeMap<u16, String>,
    font_file_data: Vec<u8>,
    embedding_kind: FontEmbeddingKind,
    descriptor_metrics: FontDescriptorMetrics,
    default_width: f32,
    cid_set_data: Option<Vec<u8>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum FontEmbeddingKind {
    SubsetRetainedGids,
    FullStandaloneFont,
    ExtractedCollectionFace,
    Rejected { reason: String },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PdfFontValidationProfile {
    Default,
    StrictPdf,
    PdfA,
    PdfUa,
}

impl PdfFontValidationProfile {
    fn emits_cid_set(self) -> bool {
        matches!(self, Self::PdfA | Self::PdfUa)
    }

    fn embedded_font_object_count(self) -> usize {
        if self.emits_cid_set() {
            EMBEDDED_FONT_OBJECTS_WITH_CID_SET
        } else {
            EMBEDDED_FONT_OBJECTS
        }
    }

    fn allows_full_font_fallback(self) -> bool {
        matches!(self, Self::Default)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct FontDescriptorMetrics {
    flags: u32,
    bbox: [i32; 4],
    italic_angle: f32,
    ascent: f32,
    descent: f32,
    cap_height: f32,
    x_height: Option<f32>,
    stem_v: f32,
    avg_width: Option<f32>,
    max_width: Option<f32>,
    missing_width: Option<f32>,
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
mod font_subset;
mod fonts;
mod metadata;
mod outlines;
mod resources;
mod shaping;
mod writer;

use content::*;
use font_subset::*;
use fonts::*;
use metadata::*;
use outlines::*;
use resources::*;
use shaping::*;
pub(crate) use writer::write_document;

#[cfg(test)]
mod tests;
