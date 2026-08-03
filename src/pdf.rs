use crate::document::DocumentFont;
use crate::document::FontProgramKind;
use crate::document::paint::geometry::PdfSize;
use crate::document::paint::images::RenderedImage;
use crate::document::paint::paths::{RenderedPath, RenderedPathCommand, RenderedPathFillRule};
use crate::document::paint::patterns::RenderedImagePattern;
use crate::document::paint::patterns::RenderedImageSourceRect;
use crate::document::paint::shapes::RenderedRoundedRect;
use crate::document::paint::text::{RenderedGlyph, RenderedTextMatrix};
use crate::{Bookmark, BookmarkState, CssColor, Document, DocumentMetadata, Page};
use pdf_writer::types::BlendMode;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// Whether fully opaque uniform decoded raster images may serialize as direct
/// PDF vector fills instead of `/Image` XObjects.
///
/// This is a deliberately compile-time rollout switch: set it to `false` to
/// retain the conventional raster-image representation while investigating a
/// PDF consumer or rasterizer. It affects only PDF serialization after image
/// materialization and color conversion; CSS image layout is unchanged.
pub(super) const PROMOTE_SOLID_RASTER_IMAGES_TO_VECTOR_FILLS: bool = true;

/// Bytes prepared for one generated PDF stream.
///
/// Keeping the bytes and the required filter together prevents one PDF stream
/// producer from applying `/FlateDecode` without first encoding its payload.
pub(super) enum PdfStreamData<'a> {
    Flate(Vec<u8>),
    Raw(&'a [u8]),
}

impl PdfStreamData<'_> {
    pub(super) fn bytes(&self) -> &[u8] {
        match self {
            Self::Flate(data) => data,
            Self::Raw(data) => data,
        }
    }

    pub(super) const fn uses_flate(&self) -> bool {
        matches!(self, Self::Flate(_))
    }
}

/// Select the serialized bytes and PDF filter for a generated stream.
pub(super) fn encode_pdf_stream(
    compression: crate::PdfCompression,
    data: &[u8],
) -> PdfStreamData<'_> {
    match compression {
        crate::PdfCompression::Compressed => PdfStreamData::Flate(flate_compress(data)),
        crate::PdfCompression::Uncompressed => PdfStreamData::Raw(data),
    }
}

/// Compress a PDF stream with the zlib wrapper required by `/FlateDecode`.
///
/// ISO 32000-1:2008, 7.4.4 defines the FlateDecode filter. Keeping compression
/// here makes every PDF stream producer use the same deterministic level.
pub(super) fn flate_compress(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

/// Converts Quire paint blend modes to the PDF `/BM` blend-mode values.
///
/// PDF 1.4 transparency defines `/BM` in an ExtGState dictionary: ISO
/// 32000-1:2008, 11.3.5 "Blend Mode".
impl From<crate::document::paint::effects::PaintBlendMode> for BlendMode {
    fn from(mode: crate::document::paint::effects::PaintBlendMode) -> Self {
        match mode {
            crate::document::paint::effects::PaintBlendMode::Normal => Self::Normal,
            crate::document::paint::effects::PaintBlendMode::Multiply => Self::Multiply,
            crate::document::paint::effects::PaintBlendMode::Screen => Self::Screen,
            crate::document::paint::effects::PaintBlendMode::Overlay => Self::Overlay,
            crate::document::paint::effects::PaintBlendMode::Darken => Self::Darken,
            crate::document::paint::effects::PaintBlendMode::Lighten => Self::Lighten,
            crate::document::paint::effects::PaintBlendMode::ColorDodge => Self::ColorDodge,
            crate::document::paint::effects::PaintBlendMode::ColorBurn => Self::ColorBurn,
            crate::document::paint::effects::PaintBlendMode::HardLight => Self::HardLight,
            crate::document::paint::effects::PaintBlendMode::SoftLight => Self::SoftLight,
            crate::document::paint::effects::PaintBlendMode::Difference => Self::Difference,
            crate::document::paint::effects::PaintBlendMode::Exclusion => Self::Exclusion,
            crate::document::paint::effects::PaintBlendMode::Hue => Self::Hue,
            crate::document::paint::effects::PaintBlendMode::Saturation => Self::Saturation,
            crate::document::paint::effects::PaintBlendMode::Color => Self::Color,
            crate::document::paint::effects::PaintBlendMode::Luminosity => Self::Luminosity,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ImageResource {
    pixel_width: u32,
    pixel_height: u32,
    interpolate: bool,
    color_space: crate::color::RasterColorSpace,
    payload: ImagePayload,
}

/// One resolved PDF paint representation for a raster source.
///
/// A fully transparent decoded image has no PDF paint operation, while a
/// uniform, fully opaque image can be painted as an ICC-tagged vector fill
/// without changing its resolved CSS destination geometry. All other sources
/// retain their PDF image XObject representation.
/// ISO 32000-2:2020, 8.9.5 defines image XObjects and 8.6.5 defines
/// calibrated color spaces for direct graphics paint.
#[derive(Debug, Clone, PartialEq)]
enum PreparedImageResource {
    Transparent,
    Raster(ImageResource),
    SolidFill(SolidImageFill),
}

/// The final calibrated samples for an opaque uniform raster image.
///
/// `color_space` and `components` are retained after image color conversion,
/// so a vector replacement selects the same ICC resource and component values
/// that the ordinary image XObject would have used.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SolidImageFill {
    color_space: crate::color::RasterColorSpace,
    components: [u8; 3],
}

/// The PDF encoding selected for one resolved raster-image resource.
///
/// Decoded samples need a PDF stream filter, while an eligible JPEG is already
/// a complete DCT-coded stream and must be passed through unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ImagePayload {
    Samples {
        rgb: Vec<u8>,
        alpha: Option<Vec<u8>>,
    },
    Jpeg(Rc<[u8]>),
}

/// A PDF image resource before its source has been expanded into samples.
///
/// Document-backed images deliberately keep only the stable store handle here.
/// This lets PDF emission decode one source at a time instead of retaining a
/// decoded copy of every distinct image while pages and resources are planned.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ImageResourceSource {
    Stored {
        image_id: crate::image_store::ImageId,
        source_rect: RenderedImageSourceRect,
        interpolate: bool,
    },
    Inline {
        pixel_width: u32,
        pixel_height: u32,
        interpolate: bool,
        color_space: crate::color::RasterColorSpace,
        rgb: Rc<[u8]>,
        alpha: Option<Rc<[u8]>>,
    },
}

impl ImageResourceSource {
    fn raster_color_space(
        &self,
        image_store: &crate::image_store::DocumentImageStore,
    ) -> crate::color::RasterColorSpace {
        match self {
            Self::Stored { image_id, .. } => image_store
                .color_space(*image_id)
                .unwrap_or(crate::color::RasterColorSpace::SRGB),
            Self::Inline { color_space, .. } => color_space.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImageObjectIds {
    image_id: PdfImageObjectId,
    alpha_mask_id: Option<PdfImageObjectId>,
}

/// Index into the deduplicated image-source plan, distinct from a PDF object
/// reference so planning code cannot accidentally mix the two domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PlannedImageIndex(usize);

/// PDF indirect object allocated for an image or soft mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PdfImageObjectId(usize);

#[derive(Debug, Clone, PartialEq)]
struct ImageResourcePlan {
    unique_images: Vec<ImageResourceSource>,
    page_image_unique_indexes: Vec<Vec<PlannedImageIndex>>,
    page_pattern_tile_unique_indexes: Vec<Vec<PlannedImageIndex>>,
}

impl ImageResourcePlan {
    fn built_in_color_spaces(
        &self,
        image_store: &crate::image_store::DocumentImageStore,
    ) -> Vec<crate::css::CssColorSpace> {
        let mut spaces = Vec::new();
        for source in &self.unique_images {
            let color_space = source.raster_color_space(image_store);
            let crate::color::RasterColorSpace::BuiltIn(space) = color_space else {
                continue;
            };
            if !spaces.contains(&space) {
                spaces.push(space);
            }
        }
        spaces
    }

    fn embedded_rgb_profiles(
        &self,
        image_store: &crate::image_store::DocumentImageStore,
    ) -> Vec<Rc<[u8]>> {
        let mut profiles: Vec<Rc<[u8]>> = Vec::new();
        for source in &self.unique_images {
            let crate::color::RasterColorSpace::EmbeddedRgb(profile) =
                source.raster_color_space(image_store)
            else {
                continue;
            };
            if !profiles.iter().any(|existing| {
                let existing: &[u8] = existing.as_ref();
                existing == profile.as_ref()
            }) {
                profiles.push(profile);
            }
        }
        profiles
    }

    /// Returns source indexes which cannot be emitted as a direct solid fill.
    ///
    /// Image patterns retain an image XObject by construction, and local
    /// image transforms use a source-space image matrix rather than a
    /// page-space rectangle. Keeping those sources raster also means one
    /// deduplicated resource has one stable paint representation everywhere.
    fn solid_fill_eligibility(&self, document: &Document) -> Vec<bool> {
        let mut eligible = vec![true; self.unique_images.len()];
        for indexes in &self.page_pattern_tile_unique_indexes {
            for index in indexes {
                eligible[index.0] = false;
            }
        }
        for (page, indexes) in document.pages.iter().zip(&self.page_image_unique_indexes) {
            for (image, index) in page.images.iter().zip(indexes) {
                if image.transform.is_some() {
                    eligible[index.0] = false;
                }
            }
        }
        eligible
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PageImagePatternPlan {
    id: usize,
    name: String,
    tile_image_id: PdfImageObjectId,
    pattern: RenderedImagePattern,
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
    gradient_patterns: Vec<GradientPatternPlan>,
    gradient_tiling_patterns: Vec<GradientTilingPatternPlan>,
    svg_tiling_patterns: Vec<SvgTilingPatternPlan>,
    svg_path_tiling_patterns: Vec<SvgPathTilingPatternPlan>,
}

#[derive(Debug, Clone, PartialEq)]
struct SvgTilingPatternPlan {
    id: usize,
    name: String,
    form_id: usize,
    form_name: String,
    pattern: crate::document::paint::patterns::RenderedSvgPattern,
    transform: crate::document::paint::geometry::PaintTransform,
}

/// A Type 1 tiling pattern used as the fill or stroke paint of one SVG path.
/// Its stream stays in SVG user space and is transformed by the target path's
/// active CTM when selected from the page or effect-form resources.
#[derive(Debug, Clone, PartialEq)]
struct SvgPathTilingPatternPlan {
    id: usize,
    name: String,
    pattern: crate::document::paint::paths::RenderedSvgPathPattern,
}

#[derive(Debug, Clone, PartialEq)]
struct GradientTilingPatternPlan {
    id: usize,
    name: String,
    shading_pattern_name: String,
    alpha_gstate_name: Option<String>,
    pattern: crate::document::paint::patterns::RenderedGradientPattern,
}

/// A page-local PDF shading-pattern resource for one normalized gradient paint.
///
/// ISO 32000-2:2020, 8.7.4 represents axial and radial SVG/CSS gradients as
/// `/PatternType 2` resources plus Type 2/3 shading functions.
#[derive(Debug, Clone, PartialEq)]
struct GradientPatternPlan {
    id: usize,
    name: String,
    function_ids: Vec<usize>,
    gradient: crate::document::paint::paths::RenderedGradient,
    alpha: Option<GradientAlphaPlan>,
}

/// The PDF resources that interpolate a gradient's stop alpha values.
///
/// A PDF shading has no alpha output channel, so SVG stop alpha is emitted as
/// an `/SMask` transparency group containing an equivalent DeviceGray shading:
/// ISO 32000-2:2020, 11.7.4.3 and SVG 2, 13.2.4.
#[derive(Debug, Clone, PartialEq)]
struct GradientAlphaPlan {
    pattern_id: usize,
    pattern_name: String,
    function_ids: Vec<usize>,
    form_id: usize,
    ext_gstate_id: usize,
    ext_gstate_name: String,
    page_size: PdfSize,
}

#[derive(Debug, Clone, PartialEq)]
struct FormXObjectRender {
    id: usize,
    name: String,
    /// Form XObjects directly invoked by this form's content stream.
    ///
    /// PDF Form resource dictionaries must define every nested `Do` name, but
    /// including the page's complete form set introduces recursive resources
    /// and duplicate dictionary keys. ISO 32000-1:2008, 8.10 Form XObjects.
    form_dependencies: Vec<FormXObjectReference>,
    bbox: crate::document::paint::geometry::PaintClip,
    stream: Vec<u8>,
    /// Effect-scope forms need an isolated transparency group; a simple SVG
    /// tile is an opaque reusable drawing and must retain ordinary form
    /// painting semantics inside a tiling pattern.
    transparency_group: bool,
}

/// A named Form XObject that may be used by a page or another Form stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FormXObjectReference {
    id: usize,
    name: String,
}

const EMBEDDED_FONT_OBJECTS: usize = 5;
const EMBEDDED_FONT_OBJECTS_WITH_CID_SET: usize = 6;

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
    font_program_kind: FontProgramKind,
    /// Maps shaped source glyph IDs to the compact CIDs emitted in PDF text.
    ///
    /// `subsetter` makes remapped GIDs and CIDs identical, so this map keeps
    /// content streams, the embedded font program, and PDF CMaps in lockstep.
    source_gid_to_cid: BTreeMap<u16, u16>,
    used_cids: BTreeMap<u16, String>,
    font_file_data: Vec<u8>,
    embedding_kind: FontEmbeddingKind,
    descriptor_metrics: FontDescriptorMetrics,
    default_width: f32,
    cid_set_data: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FontEmbeddingKind {
    SubsetCompactGids,
    FullStandaloneFont,
    ExtractedCollectionFace,
    Rejected { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PdfFontValidationProfile {
    Default,
    PdfA,
}

impl PdfFontValidationProfile {
    fn emits_cid_set(self) -> bool {
        matches!(self, Self::PdfA)
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
    /// Paint-only synthesis remains keyed by document font rather than by
    /// embedded resource, because multiple document uses can share a subset.
    document_font_synthesis: Vec<crate::document::DocumentFontSynthesis>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EmbeddedFontCandidateKey {
    program_len: usize,
    face_index: u32,
    program_kind: FontProgramKind,
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

mod colors;
mod content;
mod font_subset;
mod fonts;
mod metadata;
mod outlines;
mod resources;
mod text;
mod writer;

use content::*;
use font_subset::*;
use fonts::*;
use metadata::*;
use outlines::*;
use resources::*;
use text::*;
pub(crate) use writer::write_document;

#[cfg(test)]
mod tests;
