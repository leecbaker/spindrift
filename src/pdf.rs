use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use pdf_writer::types::BlendMode;

use crate::document::paint::geometry::PdfSize;
use crate::document::paint::images::RenderedImage;
use crate::document::paint::paths::{RenderedPath, RenderedPathCommand, RenderedPathFillRule};
use crate::document::paint::patterns::{RenderedImagePattern, RenderedImageSourceRect};
use crate::document::paint::shapes::RenderedRoundedRect;
use crate::document::paint::text::{RenderedGlyph, RenderedTextMatrix};
use crate::document::{DocumentFont, FontProgramKind};
use crate::{Bookmark, BookmarkState, CssColor, Document, DocumentMetadata, Page};

/// Typed symbolic identities for resources materialized outside page-content
/// lowering.  Their numeric payload is an index in the corresponding
/// semantic plan, never an indirect PDF object number.
macro_rules! pdf_static_handle {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        struct $name(usize);
    };
}

pdf_static_handle!(PdfFontHandle);
pdf_static_handle!(PdfImageHandle);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PdfImagePatternHandle {
    page_index: usize,
    pattern_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PdfPageExtGStateHandle {
    page_index: usize,
    resource_index: usize,
}

/// A semantic calibrated colour-space dependency.  The embedded profile is
/// retained as data rather than a provisional ICC object number, so lowering
/// remains independent of indirect-reference allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PdfColorSpaceHandle {
    BuiltIn(crate::css::CssColorSpace),
    EmbeddedRgb(Rc<[u8]>),
}

/// Lowered PDF content program. Its streams contain PDF operators and stable
/// resource names, but no dynamically allocated PDF object IDs.
#[derive(Debug, Clone)]
struct PdfLoweredDocumentProgram {
    pages: Vec<PageContentRender>,
    dynamic_resources: PdfResourceRegistry,
}

impl PdfLoweredDocumentProgram {
    /// Assert that every named Form dependency is local to the stream that
    /// invokes it and resolves to one Form in this document program.
    fn debug_assert_well_formed(&self) {
        let forms = self
            .pages
            .iter()
            .flat_map(|page| &page.form_xobjects)
            .map(|form| form.id)
            .collect::<std::collections::BTreeSet<_>>();
        for stream in self.pages.iter().map(|page| &page.stream).chain(
            self.pages
                .iter()
                .flat_map(|page| page.form_xobjects.iter().map(|form| &form.stream)),
        ) {
            for dependency in stream.resource_uses.xobjects.values() {
                if let PdfXObjectHandle::Form(handle) = dependency {
                    debug_assert!(forms.contains(handle));
                }
            }
        }
    }
}

/// The complete private PDF document program after late resource resolution.
///
/// Semantic lowering creates [`PdfLoweredDocumentProgram`] without object
/// references. This record then joins it to the resolved static entries that
/// serialization needs. Keeping images, fonts, ExtGStates, annotations,
/// metadata, outlines, and page objects here prevents a writer helper from
/// consulting unrelated document-global state to discover a resource.
struct PdfDocumentProgram<'a> {
    catalog_id: usize,
    pages_id: usize,
    pages: Vec<PdfPageProgram>,
    dynamic_resources: PdfResourcePlanner,
    color_plan: colors::PdfColorPlan,
    fonts: Vec<EmbeddedFontPlan<'a>>,
    images: PdfImageProgram,
    page_ext_gstates: Vec<Vec<ExtGStateObjectPlan>>,
    metadata: PdfMetadataProgram,
    outline: Option<OutlinePlan>,
    file_id: (Vec<u8>, Vec<u8>),
}

/// One fully resolved page entry.  It contains the page dictionary values
/// and content stream together so page serialization never has to consult a
/// source [`Document`].
struct PdfPageProgram {
    id: usize,
    content_id: Option<usize>,
    size: crate::document::paint::geometry::PaintSize,
    rotation: i32,
    annotations: Vec<PdfAnnotationProgram>,
    render: PageContentRender,
}

/// Resolved image entries belonging to one [`PdfDocumentProgram`].
struct PdfImageProgram {
    prepared: Vec<PreparedImageResource>,
    unique_object_ids: Vec<Option<ImageObjectIds>>,
    page_patterns: Vec<Vec<PageImagePatternPlan>>,
}

/// Static PDF document metadata entries. The information dictionary exists
/// for every PDF; the XMP stream is conditional on the selected profile and
/// source metadata.
struct PdfMetadataProgram {
    info_id: usize,
    xmp_id: Option<usize>,
    source: DocumentMetadata,
    producer: String,
}

/// A fully lowered link annotation, independent of its source page object.
///
/// ISO 32000-2:2020, 12.5.2 and 12.6.4.7 define link annotations and URI
/// actions respectively.
struct PdfAnnotationProgram {
    id: usize,
    rect: crate::document::paint::geometry::PaintRect,
    target: String,
}

impl PdfDocumentProgram<'_> {
    fn debug_assert_resolved_references_are_unique(&self) {
        let mut ids = std::collections::BTreeSet::new();
        let mut insert = |id| {
            debug_assert!(
                ids.insert(id),
                "resolved PDF object references must be unique"
            );
        };
        insert(self.catalog_id);
        insert(self.pages_id);
        for page in &self.pages {
            insert(page.id);
            if let Some(content_id) = page.content_id {
                insert(content_id);
            }
        }
        for id in self.dynamic_resources.object_ids() {
            insert(id);
        }
        for id in self.color_plan.object_ids() {
            insert(id);
        }
        for font in &self.fonts {
            insert(font.type0_id);
            insert(font.cid_font_id);
            insert(font.descriptor_id);
            insert(font.file_id);
            insert(font.to_unicode_id);
            if let Some(id) = font.cid_set_id {
                insert(id);
            }
        }
        for ids_for_image in self.images.unique_object_ids.iter().flatten() {
            insert(ids_for_image.image_id.0);
            if let Some(alpha) = ids_for_image.alpha_mask_id {
                insert(alpha.0);
            }
        }
        for pattern in self.images.page_patterns.iter().flatten() {
            insert(pattern.id);
        }
        for plan in self.page_ext_gstates.iter().flatten() {
            insert(plan.id);
        }
        insert(self.metadata.info_id);
        if let Some(id) = self.metadata.xmp_id {
            insert(id);
        }
        for annotation in self.pages.iter().flat_map(|page| &page.annotations) {
            insert(annotation.id);
        }
        if let Some(outline) = &self.outline {
            insert(outline.root_id);
            for node in &outline.nodes {
                insert(node.id);
            }
        }
    }
}

/// Named resource dependencies of one PDF content stream.
///
/// A PDF name is scoped by the resource dictionary of the stream that uses
/// it.  In particular, Form XObject dependencies are never inherited from a
/// page resource dictionary (ISO 32000-2:2020, 8.10.2).
#[derive(Debug, Default, Clone, PartialEq)]
struct PdfStreamResourceUses {
    fonts: BTreeMap<String, PdfFontHandle>,
    xobjects: BTreeMap<String, PdfXObjectHandle>,
    patterns: BTreeMap<String, PdfPatternResourceHandle>,
    ext_gstates: BTreeMap<String, PdfExtGStateResourceHandle>,
    color_spaces: BTreeMap<String, PdfColorSpaceHandle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PdfXObjectHandle {
    Form(PdfFormHandle),
    Image(PdfImageHandle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PdfPatternResourceHandle {
    Dynamic(PdfPatternHandle),
    Image(PdfImagePatternHandle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PdfExtGStateResourceHandle {
    Dynamic(PdfExtGStateHandle),
    Page(PdfPageExtGStateHandle),
}

/// Lowered PDF operators plus the resource names those operators select.
#[derive(Debug, Clone, PartialEq)]
struct PdfStreamProgram {
    bytes: Vec<u8>,
    resource_uses: PdfStreamResourceUses,
    /// Filled exactly once by the late planner. Serialization only reads this
    /// direct binding table and never searches document-global resources.
    resolved_resources: Option<PdfResolvedStreamResources>,
}

/// Direct object bindings for one fully resolved content stream.
///
/// Keeping the PDF names next to their resolved references makes a missing
/// resource impossible to silently omit during serialization.
#[derive(Debug, Default, Clone, PartialEq)]
struct PdfResolvedStreamResources {
    fonts: BTreeMap<String, PdfResolvedReference>,
    xobjects: BTreeMap<String, PdfResolvedReference>,
    patterns: BTreeMap<String, PdfResolvedReference>,
    ext_gstates: BTreeMap<String, PdfResolvedReference>,
    color_spaces: BTreeMap<String, PdfResolvedReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PdfResolvedReference(usize);

impl PdfResolvedStreamResources {
    fn is_empty(&self) -> bool {
        self.fonts.is_empty()
            && self.xobjects.is_empty()
            && self.patterns.is_empty()
            && self.ext_gstates.is_empty()
            && self.color_spaces.is_empty()
    }
}

/// Typed local bindings available while auditing one page's final operators.
///
/// This is deliberately not a document-global name inventory: each entry
/// already identifies the one symbolic resource named by the stream. The
/// audit only proves that the emitted operator and declared local binding
/// agree before planning assigns indirect references.
struct PdfPageResourceBindings {
    xobjects: BTreeMap<String, PdfXObjectHandle>,
    fonts: BTreeMap<String, PdfFontHandle>,
    patterns: BTreeMap<String, PdfPatternResourceHandle>,
    ext_gstates: BTreeMap<String, PdfExtGStateResourceHandle>,
    color_spaces: BTreeMap<String, PdfColorSpaceHandle>,
}

impl PdfPageResourceBindings {
    fn record_uses(&self, stream: &mut PdfStreamProgram) {
        let bytes = &stream.bytes;
        let audited = PdfStreamResourceUses {
            xobjects: self
                .xobjects
                .iter()
                .filter(|(name, _)| stream_uses_named_operator(bytes, name, b"Do"))
                .map(|(name, handle)| (name.clone(), *handle))
                .collect(),
            fonts: self
                .fonts
                .iter()
                .filter(|(name, _)| stream_uses_named_operator(bytes, name, b"Tf"))
                .map(|(name, handle)| (name.clone(), *handle))
                .collect(),
            patterns: self
                .patterns
                .iter()
                .filter(|(name, _)| {
                    stream_uses_named_operator(bytes, name, b"scn")
                        || stream_uses_named_operator(bytes, name, b"SCN")
                })
                .map(|(name, handle)| (name.clone(), *handle))
                .collect(),
            ext_gstates: self
                .ext_gstates
                .iter()
                .filter(|(name, _)| stream_uses_named_operator(bytes, name, b"gs"))
                .map(|(name, handle)| (name.clone(), *handle))
                .collect(),
            color_spaces: self
                .color_spaces
                .iter()
                .filter(|(name, _)| {
                    stream_uses_named_operator(bytes, name, b"cs")
                        || stream_uses_named_operator(bytes, name, b"CS")
                })
                .map(|(name, handle)| (name.clone(), handle.clone()))
                .collect(),
        };
        debug_assert!(
            stream
                .resource_uses
                .fonts
                .iter()
                .all(|entry| audited.fonts.get(entry.0) == Some(entry.1))
                && stream
                    .resource_uses
                    .xobjects
                    .iter()
                    .all(|entry| audited.xobjects.get(entry.0) == Some(entry.1))
                && stream
                    .resource_uses
                    .patterns
                    .iter()
                    .all(|entry| audited.patterns.get(entry.0) == Some(entry.1))
                && stream
                    .resource_uses
                    .ext_gstates
                    .iter()
                    .all(|entry| audited.ext_gstates.get(entry.0) == Some(entry.1))
                && stream
                    .resource_uses
                    .color_spaces
                    .iter()
                    .all(|entry| audited.color_spaces.get(entry.0) == Some(entry.1)),
            "every declared lowered PDF resource binding must select its named resource operator"
        );
        stream.resource_uses = audited;
    }
}

/// Match one generated PDF name and graphics operator in one emitted content
/// operation.  Some operators (notably `Tf`) have numeric operands between
/// the name and the operator, so auditing uses the final operator line rather
/// than assuming adjacency.
fn stream_uses_named_operator(bytes: &[u8], name: &str, operator: &[u8]) -> bool {
    let mut name_token = Vec::with_capacity(name.len() + 1);
    name_token.push(b'/');
    name_token.extend_from_slice(name.as_bytes());
    bytes
        .split(|byte| *byte == b'\n' || *byte == b'\r')
        .any(|line| contains_pdf_token(line, &name_token) && contains_pdf_token(line, operator))
}

fn contains_pdf_token(line: &[u8], token: &[u8]) -> bool {
    line.windows(token.len())
        .enumerate()
        .any(|(index, window)| {
            window == token
                && line
                    .get(index.wrapping_sub(1))
                    .is_none_or(u8::is_ascii_whitespace)
                && line
                    .get(index + token.len())
                    .is_none_or(u8::is_ascii_whitespace)
        })
}

/// Whether fully opaque uniform decoded raster images may serialize as direct
/// PDF vector fills instead of `/Image` XObjects.
///
/// This is a deliberately compile-time rollout switch: set it to `false` to
/// retain the conventional raster-image representation while investigating a
/// PDF consumer or rasterizer. It affects only PDF serialization after image
/// materialization and color conversion; CSS image layout is unchanged.
pub(super) const PROMOTE_SOLID_RASTER_IMAGES_TO_VECTOR_FILLS: bool = true;

/// Converts Spindrift paint blend modes to the PDF `/BM` blend-mode values.
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
    sample_depth: crate::image_store::RasterSampleDepth,
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
        sampling: crate::document::paint::images::RasterSampling,
        target_size: crate::units::RasterPixelSize,
    },
    Inline {
        pixel_width: u32,
        pixel_height: u32,
        natural_size: crate::units::CssPixelSize,
        source_rect: Option<RenderedImageSourceRect>,
        sampling: crate::document::paint::images::RasterSampling,
        target_size: crate::units::RasterPixelSize,
        color_space: crate::color::RasterColorSpace,
        sample_depth: crate::image_store::RasterSampleDepth,
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
    page_svg_pattern_image_unique_indexes: Vec<Vec<PlannedImageIndex>>,
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
        for indexes in &self.page_svg_pattern_image_unique_indexes {
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
    handle: PdfImagePatternHandle,
    id: usize,
    name: String,
    tile_image_id: PdfImageObjectId,
    pattern: RenderedImagePattern,
    stream: PdfStreamProgram,
}

#[derive(Debug, Clone, PartialEq)]
struct ExtGStateObjectPlan {
    id: usize,
    resource: ExtGStateResource,
}

#[derive(Debug, Clone, PartialEq)]
struct PageContentRender {
    stream: PdfStreamProgram,
    form_xobjects: Vec<FormXObjectRender>,
    gradient_patterns: Vec<GradientPatternPlan>,
    gradient_tiling_patterns: Vec<GradientTilingPatternPlan>,
    svg_tiling_patterns: Vec<SvgTilingPatternPlan>,
    svg_path_tiling_patterns: Vec<SvgPathTilingPatternPlan>,
}

#[derive(Debug, Clone, PartialEq)]
struct SvgTilingPatternPlan {
    id: PdfPatternHandle,
    name: String,
    form_id: PdfFormHandle,
    form_name: String,
    pattern: crate::document::paint::patterns::RenderedSvgPattern,
    transform: crate::document::paint::geometry::PaintTransform,
    stream: PdfStreamProgram,
}

/// A Type 1 tiling pattern used as the fill or stroke paint of one SVG path.
/// Its stream stays in SVG user space and is transformed by the target path's
/// active CTM when selected from the page or effect-form resources.
#[derive(Debug, Clone, PartialEq)]
struct SvgPathTilingPatternPlan {
    id: PdfPatternHandle,
    name: String,
    pattern: crate::document::paint::paths::RenderedSvgPathPattern,
    stream: PdfStreamProgram,
}

#[derive(Debug, Clone, PartialEq)]
struct GradientTilingPatternPlan {
    id: PdfPatternHandle,
    name: String,
    shading_pattern_name: String,
    alpha_gstate_name: Option<String>,
    pattern: crate::document::paint::patterns::RenderedGradientPattern,
    stream: PdfStreamProgram,
}

/// A page-local PDF shading-pattern resource for one normalized gradient paint.
///
/// ISO 32000-2:2020, 8.7.4 represents axial and radial SVG/CSS gradients as
/// `/PatternType 2` resources plus Type 2/3 shading functions.
#[derive(Debug, Clone, PartialEq)]
struct GradientPatternPlan {
    id: PdfPatternHandle,
    name: String,
    function_ids: Vec<PdfFunctionHandle>,
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
    pattern_id: PdfPatternHandle,
    pattern_name: String,
    function_ids: Vec<PdfFunctionHandle>,
    form_id: PdfFormHandle,
    ext_gstate_id: PdfExtGStateHandle,
    ext_gstate_name: String,
    page_size: PdfSize,
    stream: PdfStreamProgram,
}

#[derive(Debug, Clone, PartialEq)]
struct FormXObjectRender {
    id: PdfFormHandle,
    name: String,
    /// Form XObjects directly invoked by this form's content stream.
    ///
    /// PDF Form resource dictionaries must define every nested `Do` name, but
    /// including the page's complete form set introduces recursive resources
    /// and duplicate dictionary keys. ISO 32000-1:2008, 8.10 Form XObjects.
    bbox: crate::document::paint::geometry::PaintClip,
    stream: PdfStreamProgram,
    /// Effect-scope forms need an isolated transparency group; a simple SVG
    /// tile is an opaque reusable drawing and must retain ordinary form
    /// painting semantics inside a tiling pattern.
    kind: PdfFormKind,
}

/// The semantic kind of a Form XObject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PdfFormKind {
    Ordinary,
    TransparencyGroup {
        blending_space: colors::PdfBlendColorSpace,
    },
}

/// A named Form XObject that may be used by a page or another Form stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FormXObjectReference {
    id: PdfFormHandle,
    name: String,
}

#[cfg(test)]
mod resource_program_tests {
    use super::*;

    #[test]
    fn symbolic_resources_resolve_to_unique_deterministic_object_references() {
        let mut registry = PdfResourceRegistry::default();
        let form = registry.form();
        let pattern = registry.pattern();
        let function = registry.function();
        let gstate = registry.ext_gstate();

        let planner = PdfResourcePlanner::from_object_ids(&registry, vec![41, 42, 43, 44]);
        assert_eq!(planner.form(form), 41);
        assert_eq!(planner.pattern(pattern), 42);
        assert_eq!(planner.function(function), 43);
        assert_eq!(planner.ext_gstate(gstate), 44);
    }

    #[test]
    fn nested_form_dependencies_are_scoped_to_the_calling_stream() {
        let mut registry = PdfResourceRegistry::default();
        let parent = registry.form();
        let child = registry.form();
        let parent_form = FormXObjectRender {
            id: parent,
            name: "Fm1".into(),
            bbox: crate::document::paint::geometry::PaintClip::new(0.0, 0.0, 1.0, 1.0),
            stream: PdfStreamProgram {
                bytes: Vec::new(),
                resource_uses: PdfStreamResourceUses {
                    xobjects: [("Fm2".into(), PdfXObjectHandle::Form(child))].into(),
                    ..PdfStreamResourceUses::default()
                },
                resolved_resources: None,
            },
            kind: PdfFormKind::TransparencyGroup {
                blending_space: colors::PdfBlendColorSpace::Srgb,
            },
        };
        let child_form = FormXObjectRender {
            id: child,
            name: "Fm2".into(),
            bbox: crate::document::paint::geometry::PaintClip::new(0.0, 0.0, 1.0, 1.0),
            stream: PdfStreamProgram {
                bytes: Vec::new(),
                resource_uses: PdfStreamResourceUses::default(),
                resolved_resources: None,
            },
            kind: PdfFormKind::Ordinary,
        };
        let program = PdfLoweredDocumentProgram {
            pages: vec![PageContentRender {
                stream: PdfStreamProgram {
                    bytes: Vec::new(),
                    resource_uses: PdfStreamResourceUses {
                        xobjects: [("Fm1".into(), PdfXObjectHandle::Form(parent))].into(),
                        ..PdfStreamResourceUses::default()
                    },
                    resolved_resources: None,
                },
                form_xobjects: vec![parent_form, child_form],
                gradient_patterns: Vec::new(),
                gradient_tiling_patterns: Vec::new(),
                svg_tiling_patterns: Vec::new(),
                svg_path_tiling_patterns: Vec::new(),
            }],
            dynamic_resources: registry,
        };
        program.debug_assert_well_formed();
        assert_eq!(
            program.pages[0].form_xobjects[0]
                .stream
                .resource_uses
                .xobjects
                .len(),
            1
        );
    }

    #[test]
    fn final_operator_audit_scopes_each_named_resource_category() {
        let mut registry = PdfResourceRegistry::default();
        let form = registry.form();
        let bindings = PdfPageResourceBindings {
            xobjects: [
                ("Fm1".into(), PdfXObjectHandle::Form(form)),
                ("Im1".into(), PdfXObjectHandle::Image(PdfImageHandle(0))),
                ("Im2".into(), PdfXObjectHandle::Image(PdfImageHandle(1))),
            ]
            .into(),
            fonts: [
                ("F1".into(), PdfFontHandle(0)),
                ("F2".into(), PdfFontHandle(1)),
            ]
            .into(),
            patterns: [
                (
                    "SG1".into(),
                    PdfPatternResourceHandle::Dynamic(registry.pattern()),
                ),
                (
                    "P1".into(),
                    PdfPatternResourceHandle::Image(PdfImagePatternHandle {
                        page_index: 0,
                        pattern_index: 0,
                    }),
                ),
            ]
            .into(),
            ext_gstates: [
                (
                    "GSalpha500".into(),
                    PdfExtGStateResourceHandle::Dynamic(registry.ext_gstate()),
                ),
                (
                    "GSblendMultiply".into(),
                    PdfExtGStateResourceHandle::Page(PdfPageExtGStateHandle {
                        page_index: 0,
                        resource_index: 0,
                    }),
                ),
            ]
            .into(),
            color_spaces: [
                (
                    "CSsRGB".into(),
                    PdfColorSpaceHandle::BuiltIn(crate::css::CssColorSpace::Srgb),
                ),
                (
                    "CSDisplayP3".into(),
                    PdfColorSpaceHandle::BuiltIn(crate::css::CssColorSpace::DisplayP3),
                ),
            ]
            .into(),
        };
        let mut stream = PdfStreamProgram {
            bytes: b"/CSsRGB cs\n/F1 12 Tf\n/Im2 Do\n/SG1 scn\n/GSalpha500 gs\n/Fm1 Do\n".to_vec(),
            resource_uses: PdfStreamResourceUses::default(),
            resolved_resources: None,
        };

        bindings.record_uses(&mut stream);

        assert_eq!(
            stream
                .resource_uses
                .fonts
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["F1"]
        );
        assert_eq!(
            stream
                .resource_uses
                .xobjects
                .keys()
                .filter(|name| name.as_str() == "Im2")
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["Im2"]
        );
        assert_eq!(
            stream
                .resource_uses
                .patterns
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["SG1"]
        );
        assert_eq!(
            stream
                .resource_uses
                .ext_gstates
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["GSalpha500"]
        );
        assert_eq!(
            stream
                .resource_uses
                .color_spaces
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["CSsRGB"]
        );
        assert_eq!(
            stream.resource_uses.xobjects.get("Fm1").copied(),
            Some(PdfXObjectHandle::Form(form))
        );
    }
}

const EMBEDDED_FONT_OBJECTS: usize = 5;
const EMBEDDED_FONT_OBJECTS_WITH_CID_SET: usize = 6;

/// A glyph width written to a PDF CID font's `/W` array.
///
/// PDF text-space widths are expressed in thousandths of an em. Keeping this
/// distinct from CSS layout advances ensures that `TJ` positioning compares
/// against the metric a PDF reader actually uses for `Tj` text progression.
/// See ISO 32000-2:2020, 9.7.4.3 "CIDFonts".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PdfTextSpaceWidth(i32);

impl PdfTextSpaceWidth {
    const SCALE: i32 = 1000;

    fn from_font_units(advance: u16, units_per_em: u16) -> Self {
        let units_per_em = i64::from(units_per_em.max(1));
        let numerator = i64::from(advance) * i64::from(Self::SCALE);
        Self(((numerator as f64 / units_per_em as f64).round()) as i32)
    }

    fn points_at(self, font_size: f32) -> f32 {
        self.0 as f32 * font_size / Self::SCALE as f32
    }

    fn as_pdf_number(self) -> f32 {
        self.0 as f32
    }
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
    font_program_kind: FontProgramKind,
    /// Maps shaped source glyph IDs to the compact CIDs emitted in PDF text.
    ///
    /// `subsetter` makes remapped GIDs and CIDs identical, so this map keeps
    /// content streams, the embedded font program, and PDF CMaps in lockstep.
    source_gid_to_cid: BTreeMap<u16, u16>,
    /// The exact integer text-space widths written to the corresponding `/W`
    /// entries. PDF content emission uses this same map to position text.
    source_gid_to_width: BTreeMap<u16, PdfTextSpaceWidth>,
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
    /// A variable-font instance materialized with every source glyph retained.
    InstantiatedFullCoverage,
    FullStandaloneFont,
    ExtractedCollectionFace,
    Rejected {
        reason: String,
    },
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

impl EmbeddedFontPlans<'_> {
    /// Resolve provisional font-plan object offsets after resource lowering.
    ///
    /// Font subsetting and resource names are independent from indirect PDF
    /// object numbers, so lowering can use a zero-based provisional plan and
    /// this adapter is the single later object-ID boundary.
    fn resolve_object_ids(&mut self, first_id: usize, profile: PdfFontValidationProfile) {
        let object_count = profile.embedded_font_object_count();
        for (index, font) in self.fonts.iter_mut().enumerate() {
            let base_id = first_id + index * object_count;
            font.type0_id = base_id;
            font.cid_font_id = base_id + 1;
            font.descriptor_id = base_id + 2;
            font.file_id = base_id + 3;
            font.to_unicode_id = base_id + 4;
            font.cid_set_id = profile.emits_cid_set().then_some(base_id + 5);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EmbeddedFontCandidateKey {
    program_len: usize,
    face_index: u32,
    program_kind: FontProgramKind,
    variation_coordinates: crate::document::DocumentFontVariationCoordinates,
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
    label: String,
    page_index: usize,
    target: crate::document::paint::geometry::PaintPoint,
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
mod planner;
mod resources;
mod serialize;
mod text;
mod writer;

use content::*;
use font_subset::*;
use fonts::*;
use metadata::*;
use outlines::*;
use planner::*;
use resources::*;
use serialize::*;
use text::*;
pub(crate) use writer::write_document;

#[cfg(test)]
mod tests;
