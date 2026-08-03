//! SVG parsing and the initial PDF vector adapter.
//!
//! SVG 2 defines an SVG element as a replaced element when embedded in HTML,
//! while SVG user units use CSS pixels at the default 96 DPI. The parser keeps
//! the normalized tree in SVG units; conversion to Quire paint points happens
//! only when a replaced SVG is painted.

use crate::css::{self, CssColor};
use crate::document::PaintStrokeWidth;
use crate::document::paint::effects::PaintBlendMode;
use crate::document::paint::geometry::{
    PaintClip, PaintPoint, PaintRect, PaintSize, PaintTransform,
};
use crate::document::paint::paths::{
    RenderedGradient, RenderedGradientKind, RenderedGradientStop, RenderedPath, RenderedPathClip,
    RenderedPathClipPath, RenderedPathCommand, RenderedPathFillRule, RenderedPathLineCap,
    RenderedPathLineJoin, RenderedPathPaint, RenderedPathPaintOrder, RenderedPathStrokeStyle,
    RenderedSvgPathPattern,
};
use crate::dom::{Element, ElementId, NodeKind};
use crate::units::{LayoutLength, LayoutSize, SemanticLengthExt, layout_pt};
use std::collections::HashMap;
use std::rc::Rc;

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
const XLINK_NAMESPACE: &str = "http://www.w3.org/1999/xlink";
const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// SVG-root viewport coordinates in top-left-origin SVG user units.
///
/// These coordinates describe the source image, not page-local paint output.
/// SVG source cropping must therefore cross an explicit transform boundary
/// before it becomes [`PaintPoint`] geometry:
/// <https://www.w3.org/TR/SVG2/coords.html#ViewportSpace>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SvgSourceSpace {}

pub(crate) type SvgSourcePoint = euclid::Point2D<f32, SvgSourceSpace>;
pub(crate) type SvgSourceSize = euclid::Size2D<f32, SvgSourceSpace>;
pub(crate) type SvgSourceRect = euclid::Rect<f32, SvgSourceSpace>;
type SvgSourceToPaintTransform =
    euclid::ScaleOffset2D<f32, SvgSourceSpace, crate::document::paint::geometry::PaintSpace>;

/// Maps one normalized SVG shape's local geometry into page paint space.
///
/// Keeping this distinct from [`SvgPaintServerToPaintTransform`] prevents a
/// marker's placement transform from being mistaken for the coordinate system
/// of a `context-fill` or `context-stroke` paint server. SVG requires those
/// paint servers to use the context element's coordinate system and bounding
/// box, not the marker child's:
/// <https://www.w3.org/TR/SVG2/painting.html#SpecifyingPaint>.
#[derive(Debug, Clone, Copy)]
struct SvgGeometryToPaintTransform(PaintTransform);

/// Maps a normalized SVG paint server into page paint space.
///
/// A `usvg` context paint server retains the inverse marker placement needed
/// to return to its context element. This transform composes that server-local
/// mapping with the current SVG viewport exactly once, independently of the
/// geometry transform used to place the marker child.
#[derive(Debug, Clone, Copy)]
struct SvgPaintServerToPaintTransform(PaintTransform);

/// Geometry and paint for one SVG fill-like operation before it crosses the
/// SVG/PDF coordinate-system boundary.
///
/// SVG paths retain local commands until this boundary so a marker can have a
/// different geometry placement from its context paint server. Materializing
/// both sides in page paint space makes the PDF pattern matrix independent of
/// the path's transient CTM, as required for a pattern resource on a page.
struct SvgPaintOperation {
    geometry: SvgPathGeometry,
    paint: SvgPaintServer,
    fill_rule: RenderedPathFillRule,
    clip: Option<RenderedPathClip>,
}

struct SvgPathGeometry {
    commands: Vec<RenderedPathCommand>,
    to_paint: SvgGeometryToPaintTransform,
}

struct SvgPaintServer {
    paint: RenderedPathPaint,
    to_paint: SvgPaintServerToPaintTransform,
}

impl SvgPaintOperation {
    /// Materialize an SVG paint operation into canonical page paint space.
    ///
    /// SVG 2 defines a complex stroke as the equivalent stroked outline filled
    /// by its stroke paint. At this point both ordinary fills and those stroke
    /// outlines therefore share the same page-space path representation:
    /// <https://www.w3.org/TR/SVG2/painting.html#StrokeShape>.
    fn materialize(mut self) -> RenderedPath {
        let commands = self
            .geometry
            .commands
            .into_iter()
            .map(|command| transform_path_command(command, self.geometry.to_paint.0))
            .collect::<Vec<_>>();
        self.paint.materialize_to_paint_space();
        let clip = remove_redundant_svg_clips(self.clip, &commands);
        RenderedPath::new(
            commands,
            None,
            self.fill_rule,
            None,
            PaintStrokeWidth::ZERO,
            clip,
        )
        .with_paints(Some(self.paint.paint), None)
    }
}

impl SvgPaintServer {
    fn materialize_to_paint_space(&mut self) {
        match &mut self.paint {
            RenderedPathPaint::Solid(_) => {}
            RenderedPathPaint::Gradient(gradient) => {
                gradient.transform = self.to_paint.0.multiply(gradient.transform);
            }
            RenderedPathPaint::SvgPattern(pattern) => {
                pattern.transform = self.to_paint.0.multiply(pattern.transform);
            }
        }
    }
}

/// Host-document CSS presentation values selected for one inline SVG node.
///
/// The SVG adapter consumes a standalone payload, so CSS that matched its DOM
/// descendants must cross this boundary as SVG presentation attributes. The
/// optional fields preserve the distinction between no host-CSS declaration
/// and an explicit CSS `none` value:
/// <https://www.w3.org/TR/SVG2/painting.html#SpecifyingPaint>.
#[derive(Debug, Clone, Default)]
pub(crate) struct SvgPresentationOverride {
    pub(crate) display: Option<SvgDisplayOverride>,
    pub(crate) transform: Option<String>,
    pub(crate) fill: Option<String>,
    pub(crate) stroke: Option<String>,
    pub(crate) stroke_width: Option<String>,
    /// A forced solid color replaces this element's unsupported filter result.
    pub(crate) remove_filter: bool,
}

/// The SVG-scene equivalent of CSS box suppression.
///
/// Inline SVG is parsed as a standalone scene, so CSS `display` selected on
/// SVG descendants has to cross that serialization boundary explicitly.
/// <https://drafts.csswg.org/css-display-3/#unbox-svg>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SvgDisplayOverride {
    None,
    Contents,
    UseContents,
}

pub(crate) type SvgPresentationOverrides = HashMap<ElementId, SvgPresentationOverride>;

/// A parsed inline SVG plus its intrinsic viewport size in Quire points.
#[derive(Debug, Clone)]
pub(crate) struct SvgAsset {
    tree: usvg::Tree,
    intrinsic_size: LayoutSize,
    intrinsic_dimensions: SvgIntrinsicDimensions,
    has_degenerate_view_box: bool,
    view_fragments: HashMap<String, SvgIntrinsicDimensions>,
    source: Rc<[u8]>,
}

/// The intrinsic dimensions and preferred aspect ratio exposed by an SVG
/// image to CSS image consumers.
///
/// SVG's concrete viewport falls back to 300 by 150 CSS pixels when its root
/// dimensions are omitted. That fallback is necessary to interpret SVG user
/// units, but it must not become a CSS intrinsic image size: CSS Backgrounds
/// gives SVG images with omitted or percentage root dimensions special
/// `background-size` behavior.
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
/// <https://www.w3.org/TR/SVG2/coords.html#IntrinsicSizing>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SvgIntrinsicDimensions {
    pub(crate) width: Option<LayoutLength>,
    pub(crate) height: Option<LayoutLength>,
    pub(crate) aspect_ratio: Option<f32>,
}

impl SvgAsset {
    pub(crate) fn intrinsic_size(&self) -> LayoutSize {
        self.intrinsic_size
    }

    /// Return the CSS intrinsic dimensions without SVG's concrete viewport
    /// fallback dimensions.
    pub(crate) fn intrinsic_dimensions(&self) -> SvgIntrinsicDimensions {
        self.intrinsic_dimensions
    }

    /// Return the CSS intrinsic size of this SVG when used as a replaced
    /// image.
    ///
    /// SVG parsing requires a concrete viewport even when one root dimension
    /// is omitted. That parser fallback must not replace CSS Images' rule
    /// that derives the missing intrinsic dimension from the preferred aspect
    /// ratio.
    /// <https://www.w3.org/TR/css-images-3/#default-sizing>
    /// <https://www.w3.org/TR/SVG2/coords.html#IntrinsicSizing>
    pub(crate) fn replaced_intrinsic_size(&self) -> LayoutSize {
        let dimensions = self.intrinsic_dimensions;
        // `usvg` exposes a concrete viewport based on the SVG `viewBox` when
        // root dimensions are omitted.  That is necessary for SVG painting,
        // but it is not CSS intrinsic geometry: the default object size is
        // 300 by 150 CSS pixels.  In particular, a 1 by 1 viewBox must not
        // become a one-CSS-pixel replaced element.
        // <https://www.w3.org/TR/CSS22/visudet.html#inline-replaced-width>
        let fallback = LayoutSize::new(300.0 * css::CSS_PX_TO_PT, 150.0 * css::CSS_PX_TO_PT);
        let ratio = dimensions
            .aspect_ratio
            .filter(|ratio| ratio.is_finite() && *ratio > 0.0);
        match (dimensions.width, dimensions.height, ratio) {
            (Some(width), Some(height), _) => LayoutSize::new(width.points(), height.points()),
            (Some(width), None, Some(ratio)) => {
                LayoutSize::new(width.points(), width.points() / ratio)
            }
            (None, Some(height), Some(ratio)) => {
                LayoutSize::new(height.points() * ratio, height.points())
            }
            // Without a preferred aspect ratio, the supplied intrinsic axis
            // combines with the corresponding default object-size axis.
            // CSS Images does not discard a declared SVG width or height just
            // because the other axis is absent.
            // <https://www.w3.org/TR/css-images-3/#default-sizing>
            (Some(width), None, None) => LayoutSize::new(width.points(), fallback.height),
            (None, Some(height), None) => LayoutSize::new(fallback.width, height.points()),
            // The default object size supplies the fallback inline size;
            // a ratio-only SVG derives the block size from it rather than
            // retaining the SVG parser's unrelated 300 by 150 viewport.
            (None, None, Some(ratio)) => LayoutSize::new(fallback.width, fallback.width / ratio),
            _ => fallback,
        }
    }

    /// Reparse this SVG for a concrete CSS replaced-object viewport.
    ///
    /// The root SVG viewport is established by the used concrete object, not
    /// by the parser's default for an omitted root dimension. Replacing both
    /// root dimensions before SVG viewport processing also preserves the SVG
    /// `preserveAspectRatio` behavior after CSS `object-fit` has selected its
    /// concrete object size.
    /// <https://www.w3.org/TR/SVG2/coords.html#ViewportSpace>
    /// <https://www.w3.org/TR/css-images-3/#the-object-fit>
    pub(crate) fn with_replaced_viewport(&self, viewport: crate::units::ContentBoxSize) -> Self {
        let width = viewport.width;
        let height = viewport.height;
        if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
            return self.clone();
        }
        self.with_css_image_viewport_px(width / css::CSS_PX_TO_PT, height / css::CSS_PX_TO_PT)
    }

    /// Whether the root SVG has a zero or negative `viewBox` extent.
    ///
    /// A non-positive `viewBox` disables rendering rather than becoming an
    /// arbitrary fallback viewport. SVG's parser still needs a concrete
    /// viewport to build its tree, so CSS image consumers must retain this
    /// source-level fact separately.
    /// <https://www.w3.org/TR/SVG2/coords.html#ViewBoxAttribute>
    pub(crate) fn has_degenerate_view_box(&self) -> bool {
        self.has_degenerate_view_box
    }

    /// Specialize this SVG image for an SVG `<view>` fragment identifier.
    ///
    /// A view fragment establishes the viewport viewBox used by an external
    /// SVG image. Its viewBox supplies the preferred aspect ratio for CSS
    /// image sizing even when the root SVG itself has no intrinsic ratio.
    /// <https://www.w3.org/TR/SVG2/linking.html#LinksIntoSVG>
    /// <https://www.w3.org/TR/css-images-4/#image-fragments>
    pub(crate) fn with_view_fragment(&self, fragment: Option<&str>) -> Self {
        let mut asset = self.clone();
        if let Some(fragment) = fragment
            && let Some(view) = self.view_fragments.get(fragment)
        {
            asset.intrinsic_dimensions.aspect_ratio = view.aspect_ratio;
            if let Some(source) = svg_with_view_fragment_view_box(&self.source, fragment)
                && let Ok(tree) = parse_svg_tree(
                    &source,
                    usvg::Size::from_wh(300.0, 150.0).expect("default SVG viewport is valid"),
                )
            {
                asset.tree = tree;
            }
        }
        asset
    }

    /// Reparse this SVG for a concrete CSS background-image viewport.
    ///
    /// SVG percentage geometry and `preserveAspectRatio` are relative to the
    /// root SVG viewport. CSS Backgrounds supplies that viewport from the
    /// concrete background image size, independently of the SVG's intrinsic
    /// dimensions used earlier by `background-size`.
    /// <https://www.w3.org/TR/SVG2/coords.html#ViewportSpace>
    /// <https://www.w3.org/TR/css-images-3/#default-sizing>
    pub(crate) fn with_background_viewport(&self, viewport: PaintSize) -> Self {
        let width = viewport.width;
        let height = viewport.height;
        if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
            return self.clone();
        }
        self.with_css_image_viewport_px(width / css::CSS_PX_TO_PT, height / css::CSS_PX_TO_PT)
    }

    /// Reparse the root SVG against a used CSS image viewport in CSS pixels.
    ///
    /// The cloned asset deliberately retains its original intrinsic-dimension
    /// metadata: CSS image sizing has already resolved before this paint-only
    /// specialization, and must not observe the rewritten root dimensions.
    fn with_css_image_viewport_px(&self, width: f32, height: f32) -> Self {
        if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
            return self.clone();
        }
        let Some(source) = svg_with_css_image_viewport(&self.source, width, height) else {
            return self.clone();
        };
        let Some(viewport) = usvg::Size::from_wh(width, height) else {
            return self.clone();
        };
        let Ok(tree) = parse_svg_tree(&source, viewport) else {
            return self.clone();
        };
        let mut asset = self.clone();
        asset.tree = tree;
        asset
    }

    /// The root SVG viewport in SVG user units.
    ///
    /// CSS image slicing operates on the concrete source viewport, whereas
    /// [`Self::intrinsic_size`] is expressed in Quire points for layout.
    pub(crate) fn source_viewport_size(&self) -> SvgSourceSize {
        let size = self.tree.size();
        SvgSourceSize::new(size.width(), size.height())
    }

    /// Convert the supported vector subset into paint paths for one CSS content box.
    pub(crate) fn paint_paths(&self, destination: PaintRect) -> Vec<RenderedPath> {
        let source_size = self.source_viewport_size();
        self.paint_paths_for_source_rect(
            destination,
            SvgSourceRect::new(SvgSourcePoint::new(0.0, 0.0), source_size),
        )
    }

    /// Convert a source viewport rectangle into paths in a destination box.
    ///
    /// The source rectangle uses SVG's top-left-origin root viewport. This
    /// lets CSS image consumers crop, tile, and slice an SVG without a raster
    /// intermediary; the PDF coordinate inversion is composed into each
    /// path's local transform.
    pub(crate) fn paint_paths_for_source_rect(
        &self,
        destination: PaintRect,
        source: SvgSourceRect,
    ) -> Vec<RenderedPath> {
        if destination.size.width <= 0.0 || destination.size.height <= 0.0 {
            return Vec::new();
        }
        if source.size.width <= 0.0 || source.size.height <= 0.0 {
            return Vec::new();
        }
        let transform = ViewportTransform::new(destination, source, true);
        let mut group = collect_svg_group(self.tree.root(), transform, &[]);
        canonicalize_svg_paint_servers(&mut group);
        elide_redundant_svg_paints(&mut group);
        group.into_paths()
    }

    /// Materialize the SVG as an ordered vector paint group.
    ///
    /// Unlike [`Self::paint_paths_for_source_rect`], this retains SVG group
    /// opacity, isolation, and blend-mode boundaries for PDF compositing.
    pub(crate) fn paint_group(&self, destination: PaintRect) -> SvgPaintGroup {
        let source_size = self.source_viewport_size();
        self.paint_group_for_source_rect_with_viewport_clip(
            destination,
            SvgSourceRect::new(SvgSourcePoint::new(0.0, 0.0), source_size),
            true,
        )
    }

    /// Paint an inline SVG root with the host element's overflow policy.
    ///
    /// Inline SVG geometry is still projected through the root `viewBox`; the
    /// boolean only controls whether the root viewport clips overflowing SVG
    /// strokes and paths.  CSS `overflow: visible` must not silently regain a
    /// replaced-image viewport clip.
    pub(crate) fn paint_inline_group(
        &self,
        destination: PaintRect,
        clip_viewport: bool,
    ) -> SvgPaintGroup {
        let source_size = self.source_viewport_size();
        self.paint_group_for_source_rect_with_viewport_clip(
            destination,
            SvgSourceRect::new(SvgSourcePoint::new(0.0, 0.0), source_size),
            clip_viewport,
        )
    }

    pub(crate) fn paint_group_for_source_rect(
        &self,
        destination: PaintRect,
        source: SvgSourceRect,
    ) -> SvgPaintGroup {
        self.paint_group_for_source_rect_with_viewport_clip(destination, source, true)
    }

    fn paint_group_for_source_rect_with_viewport_clip(
        &self,
        destination: PaintRect,
        source: SvgSourceRect,
        clip_viewport: bool,
    ) -> SvgPaintGroup {
        if destination.size.width <= 0.0
            || destination.size.height <= 0.0
            || source.size.width <= 0.0
            || source.size.height <= 0.0
        {
            return SvgPaintGroup::empty();
        }
        let viewport = ViewportTransform::new(destination, source, clip_viewport);
        let mut group = collect_svg_group(self.tree.root(), viewport, &[]);
        canonicalize_svg_paint_servers(&mut group);
        elide_redundant_svg_paints(&mut group);
        group
    }

    /// Return the color when the SVG is exactly one opaque rectangle spanning
    /// its root viewport.
    ///
    /// This is an equivalence-preserving vector simplification, not an image
    /// fallback: a spatially uniform opaque SVG has the same result as a CSS
    /// color image after background tiling and clipping.  Keeping that fact
    /// explicit also prevents PDF consumers from receiving coordinates that
    /// are needlessly enormous when a `viewBox` has an extreme aspect ratio.
    /// SVG 2 clips root content to the viewport; CSS Backgrounds subsequently
    /// clips each background tile to its background painting area.
    /// <https://www.w3.org/TR/SVG2/struct.html#SVGElement>
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-image>
    pub(crate) fn opaque_viewport_fill(&self) -> Option<CssColor> {
        let group = self.paint_group(unit_paint_rect());
        let path = single_opaque_path(&group)?;
        opaque_unit_rectangle_fill(path)
    }

    /// Return the opaque color covering a given source rectangle when it is
    /// provably spatially uniform.
    ///
    /// Background painting may expose only part of a multi-color SVG after
    /// `cover` or `contain` positioning.  If that exposed source region is
    /// covered by exactly one opaque solid rectangle, it can be emitted as a
    /// finite CSS paint rectangle instead of retaining a PDF clip around a
    /// huge transformed SVG viewport.
    pub(crate) fn opaque_source_rect_fill(&self, source: SvgSourceRect) -> Option<CssColor> {
        let group = self.paint_group_for_source_rect(unit_paint_rect(), source);
        let paths = simple_group_paths(&group)?;
        let mut color = None;
        for path in paths {
            let (path_color, bounds) = opaque_axis_aligned_rectangle(path)?;
            if bounds.2 <= 0.0 || bounds.3 <= 0.0 || bounds.0 >= 1.0 || bounds.1 >= 1.0 {
                continue;
            }
            if bounds.0 > 0.0001
                || bounds.1 > 0.0001
                || bounds.2 < 1.0 - 0.0001
                || bounds.3 < 1.0 - 0.0001
                || color.replace(path_color).is_some()
            {
                return None;
            }
        }
        color
    }
}

/// An ordered SVG compositing group in page-local paint coordinates.
#[derive(Debug, Clone)]
pub(crate) struct SvgPaintGroup {
    pub(crate) items: Vec<SvgPaintItem>,
    pub(crate) opacity: f32,
    pub(crate) blend_mode: PaintBlendMode,
    pub(crate) isolation: bool,
    pub(crate) bounds: Option<PaintClip>,
}

#[derive(Debug, Clone)]
pub(crate) enum SvgPaintItem {
    Path(Box<RenderedPath>),
    Group(Box<SvgPaintGroup>),
}

impl SvgPaintGroup {
    pub(crate) fn empty() -> Self {
        Self {
            items: Vec::new(),
            opacity: 1.0,
            blend_mode: PaintBlendMode::Normal,
            isolation: false,
            bounds: None,
        }
    }

    /// Apply an additional destination-space clip without flattening SVG
    /// compositing groups. CSS replaced-image effects such as
    /// `object-view-box: ... round ...` must constrain every descendant path
    /// while preserving group opacity and blend boundaries.
    pub(crate) fn with_clip(mut self, clip: RenderedPathClip) -> Self {
        for item in &mut self.items {
            match item {
                SvgPaintItem::Path(path) => {
                    let path = path.as_mut();
                    if let Some(existing) = path.clip.take() {
                        let mut combined = clip.clone();
                        combined.additional_clips.push(RenderedPathClipPath::new(
                            existing.commands,
                            existing.fill_rule,
                        ));
                        combined.additional_clips.extend(existing.additional_clips);
                        path.clip = Some(combined);
                    } else {
                        path.clip = Some(clip.clone());
                    }
                }
                SvgPaintItem::Group(group) => {
                    let nested = std::mem::replace(group, Box::new(SvgPaintGroup::empty()));
                    **group = nested.with_clip(clip.clone());
                }
            }
        }
        self
    }

    fn recompute_bounds(&mut self) {
        // The document paint tree owns clip/bounds union operations. Retaining
        // no SVG-local bound is conservative and lets the PDF form use the
        // page box until the group is recorded there.
        self.bounds = None;
    }

    fn into_paths(self) -> Vec<RenderedPath> {
        let mut paths = Vec::new();
        for item in self.items {
            match item {
                SvgPaintItem::Path(path) => paths.push(*path),
                SvgPaintItem::Group(group) => paths.extend(group.into_paths()),
            }
        }
        paths
    }
}

/// Maximum transform drift introduced by `usvg`'s f32 context-paint
/// round-trip through a marker placement and its inverse.
///
/// This is not a general paint-server snapping tolerance. It is used only
/// after matching the complete non-transform server definition, then shares
/// the first equivalent page-space server matrix so PDF emits one continuous
/// shading for an owner shape and its marker descendants.
const SVG_CONTEXT_PAINT_TRANSFORM_EPSILON: f32 = 0.002;

fn canonicalize_svg_paint_servers(group: &mut SvgPaintGroup) {
    let mut servers = HashMap::<String, Vec<PaintTransform>>::new();
    canonicalize_svg_paint_group(group, &mut servers);
}

fn canonicalize_svg_paint_group(
    group: &mut SvgPaintGroup,
    servers: &mut HashMap<String, Vec<PaintTransform>>,
) {
    for item in &mut group.items {
        match item {
            SvgPaintItem::Path(path) => {
                for paint in [&mut path.fill_paint, &mut path.stroke_paint]
                    .into_iter()
                    .flatten()
                {
                    canonicalize_svg_paint_server(paint, servers);
                }
            }
            SvgPaintItem::Group(group) => canonicalize_svg_paint_group(group, servers),
        }
    }
}

fn canonicalize_svg_paint_server(
    paint: &mut RenderedPathPaint,
    servers: &mut HashMap<String, Vec<PaintTransform>>,
) {
    let (signature, transform) = match paint {
        RenderedPathPaint::Solid(_) => return,
        RenderedPathPaint::Gradient(gradient) => (
            format!(
                "gradient:{:?}:{:?}:{:?}:{:?}",
                gradient.kind, gradient.color_space, gradient.stops, gradient.periodic
            ),
            &mut gradient.transform,
        ),
        RenderedPathPaint::SvgPattern(pattern) => {
            let mut signature = pattern.clone();
            signature.transform = PaintTransform::identity();
            (format!("pattern:{signature:?}"), &mut pattern.transform)
        }
    };
    let candidates = servers.entry(signature).or_default();
    if let Some(canonical) = candidates
        .iter()
        .copied()
        .find(|candidate| svg_paint_transforms_match(*candidate, *transform))
    {
        *transform = canonical;
    } else {
        candidates.push(*transform);
    }
}

fn svg_paint_transforms_match(left: PaintTransform, right: PaintTransform) -> bool {
    [
        left.a() - right.a(),
        left.b() - right.b(),
        left.c() - right.c(),
        left.d() - right.d(),
        left.e() - right.e(),
        left.f() - right.f(),
    ]
    .into_iter()
    .all(|difference| {
        difference.is_finite() && difference.abs() <= SVG_CONTEXT_PAINT_TRANSFORM_EPSILON
    })
}

/// Opaque coverage from an earlier SVG fill in one compositing context.
///
/// This is intentionally limited to convex, line-only paths and completely
/// opaque solid/gradient paints. Within that subset, painting an equal paint
/// over geometry already covered by it is exactly redundant. Eliding it avoids
/// the second antialiased source-over operation that PDF rasterizers can round
/// differently at marker viewport edges.
#[derive(Clone)]
struct OpaqueSvgPaintCoverage {
    paint: RenderedPathPaint,
    polygon: Vec<PaintPoint>,
}

fn elide_redundant_svg_paints(group: &mut SvgPaintGroup) {
    let mut coverage = Vec::new();
    elide_redundant_svg_paints_in_group(group, &mut coverage);
}

fn elide_redundant_svg_paints_in_group(
    group: &mut SvgPaintGroup,
    coverage: &mut Vec<OpaqueSvgPaintCoverage>,
) {
    let mut retained = Vec::with_capacity(group.items.len());
    for mut item in std::mem::take(&mut group.items) {
        match &mut item {
            SvgPaintItem::Path(path) => {
                if !svg_path_has_redundant_opaque_paint(path, coverage) {
                    if let Some(path_coverage) = opaque_svg_paint_coverage(path) {
                        coverage.push(path_coverage);
                    }
                    retained.push(item);
                }
            }
            SvgPaintItem::Group(child) => {
                if svg_group_is_compositing_neutral(child) {
                    elide_redundant_svg_paints_in_group(child, coverage);
                } else {
                    let mut nested_coverage = Vec::new();
                    elide_redundant_svg_paints_in_group(child, &mut nested_coverage);
                }
                if !child.items.is_empty() {
                    retained.push(item);
                }
            }
        }
    }
    group.items = retained;
}

fn svg_group_is_compositing_neutral(group: &SvgPaintGroup) -> bool {
    (group.opacity - 1.0).abs() <= f32::EPSILON
        && group.blend_mode == PaintBlendMode::Normal
        && !group.isolation
}

fn svg_path_has_redundant_opaque_paint(
    path: &RenderedPath,
    coverage: &[OpaqueSvgPaintCoverage],
) -> bool {
    let Some(paint) = path
        .fill_paint
        .as_ref()
        .filter(|paint| svg_paint_is_opaque(paint))
    else {
        return false;
    };
    let Some(polygon) = svg_convex_polygon(&path.commands) else {
        return false;
    };
    coverage.iter().any(|covered| {
        covered.paint == *paint
            && polygon
                .iter()
                .all(|point| convex_polygon_contains(&covered.polygon, *point))
    })
}

fn opaque_svg_paint_coverage(path: &RenderedPath) -> Option<OpaqueSvgPaintCoverage> {
    let paint = path.fill_paint.as_ref()?.clone();
    if !svg_paint_is_opaque(&paint) {
        return None;
    }
    Some(OpaqueSvgPaintCoverage {
        paint,
        polygon: svg_convex_polygon(&path.commands)?,
    })
}

fn svg_paint_is_opaque(paint: &RenderedPathPaint) -> bool {
    match paint {
        RenderedPathPaint::Solid(color) => color.is_opaque(),
        RenderedPathPaint::Gradient(gradient) => !gradient.has_transparent_stop(),
        // Pattern cells can have transparent gaps even if each child path is
        // opaque, so they cannot prove full coverage without tile analysis.
        RenderedPathPaint::SvgPattern(_) => false,
    }
}

fn svg_convex_polygon(commands: &[RenderedPathCommand]) -> Option<Vec<PaintPoint>> {
    let mut points = Vec::new();
    for command in commands {
        match command {
            RenderedPathCommand::MoveTo(point) | RenderedPathCommand::LineTo(point) => {
                points.push(*point)
            }
            RenderedPathCommand::Close => {}
            RenderedPathCommand::CurveTo { .. } => return None,
        }
    }
    if points.len() < 3 || !matches!(commands.last(), Some(RenderedPathCommand::Close)) {
        return None;
    }
    let mut sign = 0.0_f32;
    for index in 0..points.len() {
        let first = points[index];
        let second = points[(index + 1) % points.len()];
        let third = points[(index + 2) % points.len()];
        let cross = (second.x - first.x) * (third.y - second.y)
            - (second.y - first.y) * (third.x - second.x);
        if cross.abs() <= 0.000_001 {
            continue;
        }
        if sign != 0.0 && cross.signum() != sign {
            return None;
        }
        sign = cross.signum();
    }
    (sign != 0.0).then_some(points)
}

fn convex_polygon_contains(polygon: &[PaintPoint], point: PaintPoint) -> bool {
    let mut sign = 0.0_f32;
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let cross =
            (second.x - first.x) * (point.y - first.y) - (second.y - first.y) * (point.x - first.x);
        if cross.abs() <= 0.000_001 {
            continue;
        }
        if sign != 0.0 && cross.signum() != sign {
            return false;
        }
        sign = cross.signum();
    }
    true
}

/// Return the sole path of an SVG group only when no group-level compositing
/// can affect its color or coverage.
fn single_opaque_path(group: &SvgPaintGroup) -> Option<&RenderedPath> {
    if (group.opacity - 1.0).abs() > f32::EPSILON
        || group.blend_mode != PaintBlendMode::Normal
        || group.isolation
    {
        return None;
    }
    let [item] = group.items.as_slice() else {
        return None;
    };
    match item {
        SvgPaintItem::Path(path) => Some(path),
        SvgPaintItem::Group(group) => single_opaque_path(group),
    }
}

/// Flatten a group only when every boundary is compositing-neutral.
fn simple_group_paths(group: &SvgPaintGroup) -> Option<Vec<&RenderedPath>> {
    if (group.opacity - 1.0).abs() > f32::EPSILON
        || group.blend_mode != PaintBlendMode::Normal
        || group.isolation
    {
        return None;
    }
    let mut paths = Vec::new();
    for item in &group.items {
        match item {
            SvgPaintItem::Path(path) => paths.push(path.as_ref()),
            SvgPaintItem::Group(group) => paths.extend(simple_group_paths(group)?),
        }
    }
    Some(paths)
}

/// Drop rectangular SVG clips that cannot affect the already materialized
/// path.  In particular, a marker's viewport clip frequently coincides with a
/// marker child edge. Applying that redundant clip causes PDF rasterizers to
/// antialias an otherwise identical `context-fill` over its owner a second
/// time, creating a visible seam.
///
/// Only convex axis-aligned rectangles are removed, and only when every path
/// control point lies inside them. Bézier curves lie in the convex hull of
/// their control points, so this is a conservative proof for the supported
/// path commands.
fn remove_redundant_svg_clips(
    clip: Option<RenderedPathClip>,
    commands: &[RenderedPathCommand],
) -> Option<RenderedPathClip> {
    let clip = clip?;
    let mut candidates = Vec::with_capacity(1 + clip.additional_clips.len());
    candidates.push(RenderedPathClipPath::new(clip.commands, clip.fill_rule));
    candidates.extend(clip.additional_clips);
    let mut retained = candidates
        .into_iter()
        .filter(|candidate| !svg_rectangular_clip_contains_path(candidate, commands));
    let primary = retained.next()?;
    Some(RenderedPathClip::new(
        primary.commands,
        primary.fill_rule,
        retained.collect(),
    ))
}

fn svg_rectangular_clip_contains_path(
    clip: &RenderedPathClipPath,
    commands: &[RenderedPathCommand],
) -> bool {
    if clip.fill_rule != RenderedPathFillRule::NonZero {
        return false;
    }
    let Some(bounds) = axis_aligned_rectangle_bounds(&clip.commands) else {
        return false;
    };
    path_command_points(commands)
        .all(|point| rectangle_contains(bounds, (point.x, point.y, point.x, point.y)))
}

fn path_command_points(commands: &[RenderedPathCommand]) -> impl Iterator<Item = PaintPoint> + '_ {
    commands
        .iter()
        .flat_map(|command| match command {
            RenderedPathCommand::MoveTo(point) | RenderedPathCommand::LineTo(point) => {
                [Some(*point), None, None]
            }
            RenderedPathCommand::CurveTo {
                control_1,
                control_2,
                end,
            } => [Some(*control_1), Some(*control_2), Some(*end)],
            RenderedPathCommand::Close => [None, None, None],
        })
        .flatten()
}

/// Recognize a solid path which exactly covers a unit SVG viewport.
fn opaque_unit_rectangle_fill(path: &RenderedPath) -> Option<CssColor> {
    let (color, bounds) = opaque_axis_aligned_rectangle(path)?;
    if rectangle_matches_unit_viewport(bounds) {
        return Some(color);
    }

    // SVG painting installs the root viewport as its primary clip. A solid
    // path can cover that viewport while its source geometry overflows it,
    // such as an SVG without a `viewBox` used in a smaller CSS tile. That is
    // still equivalent to a uniform color image, provided no SVG clip path
    // further restricts the painted result.
    let clip = path.clip.as_ref()?;
    if clip.fill_rule != RenderedPathFillRule::NonZero || !clip.additional_clips.is_empty() {
        return None;
    }
    let clip_bounds = axis_aligned_rectangle_bounds(&clip.commands)?;
    (rectangle_matches_unit_viewport(clip_bounds) && rectangle_contains(bounds, clip_bounds))
        .then_some(color)
}

fn rectangle_matches_unit_viewport(bounds: (f32, f32, f32, f32)) -> bool {
    (bounds.0 - 0.0).abs() <= 0.0001
        && (bounds.1 - 0.0).abs() <= 0.0001
        && (bounds.2 - 1.0).abs() <= 0.0001
        && (bounds.3 - 1.0).abs() <= 0.0001
}

fn rectangle_contains(outer: (f32, f32, f32, f32), inner: (f32, f32, f32, f32)) -> bool {
    const EPSILON: f32 = 0.0001;
    outer.0 <= inner.0 + EPSILON
        && outer.1 <= inner.1 + EPSILON
        && outer.2 >= inner.2 - EPSILON
        && outer.3 >= inner.3 - EPSILON
}

fn axis_aligned_rectangle_bounds(commands: &[RenderedPathCommand]) -> Option<(f32, f32, f32, f32)> {
    let [
        RenderedPathCommand::MoveTo(first),
        RenderedPathCommand::LineTo(second),
        RenderedPathCommand::LineTo(third),
        RenderedPathCommand::LineTo(fourth),
        RenderedPathCommand::Close,
    ] = commands
    else {
        return None;
    };
    let points = [*first, *second, *third, *fourth];
    let min_x = points.iter().map(|point| point.x).reduce(f32::min)?;
    let min_y = points.iter().map(|point| point.y).reduce(f32::min)?;
    let max_x = points.iter().map(|point| point.x).reduce(f32::max)?;
    let max_y = points.iter().map(|point| point.y).reduce(f32::max)?;
    let expected = [
        PaintPoint::new(min_x, min_y),
        PaintPoint::new(max_x, min_y),
        PaintPoint::new(max_x, max_y),
        PaintPoint::new(min_x, max_y),
    ];
    let mut corners = [false; 4];
    for point in points {
        let index = expected.iter().position(|corner| {
            (point.x - corner.x).abs() <= 0.0001 && (point.y - corner.y).abs() <= 0.0001
        })?;
        if std::mem::replace(&mut corners[index], true) {
            return None;
        }
    }
    corners
        .into_iter()
        .all(|corner| corner)
        .then_some((min_x, min_y, max_x, max_y))
}

/// Return a solid opaque rectangular path's color and transformed bounds.
fn opaque_axis_aligned_rectangle(path: &RenderedPath) -> Option<(CssColor, (f32, f32, f32, f32))> {
    let RenderedPathPaint::Solid(color) = path.fill_paint.as_ref()? else {
        return None;
    };
    if !color.is_opaque()
        || path.stroke_paint.is_some()
        || path.fill_rule != RenderedPathFillRule::NonZero
        || path
            .clip
            .as_ref()
            .is_some_and(|clip| !clip.additional_clips.is_empty())
    {
        return None;
    }
    let [
        RenderedPathCommand::MoveTo(first),
        RenderedPathCommand::LineTo(second),
        RenderedPathCommand::LineTo(third),
        RenderedPathCommand::LineTo(fourth),
        RenderedPathCommand::Close,
    ] = path.commands.as_slice()
    else {
        return None;
    };
    let commands = [
        RenderedPathCommand::MoveTo(path.transform.apply_point(*first)),
        RenderedPathCommand::LineTo(path.transform.apply_point(*second)),
        RenderedPathCommand::LineTo(path.transform.apply_point(*third)),
        RenderedPathCommand::LineTo(path.transform.apply_point(*fourth)),
        RenderedPathCommand::Close,
    ];
    axis_aligned_rectangle_bounds(&commands).map(|bounds| (*color, bounds))
}

#[derive(Clone, Copy)]
struct ViewportTransform {
    destination: PaintRect,
    source_to_paint: SvgSourceToPaintTransform,
    clip_viewport: bool,
}

impl ViewportTransform {
    /// Resolve the SVG source viewport into a bottom-left paint rectangle.
    ///
    /// The negative y scale is the sole coordinate-system conversion between
    /// SVG's top-left source space and PDF paint space.
    fn new(destination: PaintRect, source: SvgSourceRect, clip_viewport: bool) -> Self {
        let scale_x = destination.size.width / source.size.width;
        let scale_y = destination.size.height / source.size.height;
        Self {
            destination,
            source_to_paint: SvgSourceToPaintTransform::new(
                scale_x,
                -scale_y,
                destination.origin.x - scale_x * source.origin.x,
                destination.max_y() + scale_y * source.origin.y,
            ),
            clip_viewport,
        }
    }
}

fn collect_svg_group(
    group: &usvg::Group,
    viewport: ViewportTransform,
    inherited_clips: &[RenderedPathClipPath],
) -> SvgPaintGroup {
    // SVG masks and filters alter the alpha/color result of every descendant.
    // Until a PDF soft-mask/filter compositor exists, painting the unmodified
    // children would be an incorrect substitute.
    if group.mask().is_some() || !group.filters().is_empty() {
        return SvgPaintGroup::empty();
    }
    let mut clips = inherited_clips.to_vec();
    if let Some(clip_path) = group.clip_path() {
        clips.extend(render_svg_clip_path(
            clip_path,
            group.abs_transform(),
            viewport,
        ));
    }
    let mut rendered = SvgPaintGroup {
        items: Vec::new(),
        opacity: group.opacity().get(),
        blend_mode: svg_blend_mode(group.blend_mode()),
        isolation: group.isolate(),
        bounds: None,
    };
    for node in group.children() {
        match node {
            usvg::Node::Group(child) => {
                let child = collect_svg_group(child, viewport, &clips);
                if !child.items.is_empty() {
                    rendered.items.push(SvgPaintItem::Group(Box::new(child)));
                }
            }
            usvg::Node::Path(path) => {
                for path in render_path_with_clips(path, viewport, &clips) {
                    rendered.items.push(SvgPaintItem::Path(Box::new(path)));
                }
            }
            usvg::Node::Image(_) | usvg::Node::Text(_) => {}
        }
    }
    rendered.recompute_bounds();
    rendered
}

fn render_svg_clip_path(
    clip_path: &usvg::ClipPath,
    target_transform: usvg::Transform,
    viewport: ViewportTransform,
) -> Vec<RenderedPathClipPath> {
    let transform = target_transform.pre_concat(clip_path.transform());
    let mut commands = Vec::new();
    let mut fill_rule = RenderedPathFillRule::NonZero;
    collect_svg_clip_commands(
        clip_path.root(),
        transform,
        viewport,
        &mut commands,
        &mut fill_rule,
    );
    let mut clips = (!commands.is_empty())
        .then(|| RenderedPathClipPath::new(commands, fill_rule))
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(nested) = clip_path.clip_path() {
        clips.extend(render_svg_clip_path(nested, target_transform, viewport));
    }
    clips
}

fn collect_svg_clip_commands(
    group: &usvg::Group,
    transform: usvg::Transform,
    viewport: ViewportTransform,
    commands: &mut Vec<RenderedPathCommand>,
    fill_rule: &mut RenderedPathFillRule,
) {
    let transform = transform.pre_concat(group.transform());
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => {
                collect_svg_clip_commands(group, transform, viewport, commands, fill_rule);
            }
            usvg::Node::Path(path) if path.is_visible() => {
                let path_transform = transform.pre_concat(path.abs_transform());
                let paint_transform = svg_path_transform(path_transform, viewport);
                commands.extend(
                    path_commands(path.data())
                        .into_iter()
                        .map(|command| transform_path_command(command, paint_transform)),
                );
                if matches!(
                    path.fill().map(usvg::Fill::rule),
                    Some(usvg::FillRule::EvenOdd)
                ) {
                    *fill_rule = RenderedPathFillRule::EvenOdd;
                }
            }
            usvg::Node::Image(_) | usvg::Node::Text(_) | usvg::Node::Path(_) => {}
        }
    }
}

fn svg_blend_mode(mode: usvg::BlendMode) -> PaintBlendMode {
    match mode {
        usvg::BlendMode::Normal => PaintBlendMode::Normal,
        usvg::BlendMode::Multiply => PaintBlendMode::Multiply,
        usvg::BlendMode::Screen => PaintBlendMode::Screen,
        usvg::BlendMode::Overlay => PaintBlendMode::Overlay,
        usvg::BlendMode::Darken => PaintBlendMode::Darken,
        usvg::BlendMode::Lighten => PaintBlendMode::Lighten,
        usvg::BlendMode::ColorDodge => PaintBlendMode::ColorDodge,
        usvg::BlendMode::ColorBurn => PaintBlendMode::ColorBurn,
        usvg::BlendMode::HardLight => PaintBlendMode::HardLight,
        usvg::BlendMode::SoftLight => PaintBlendMode::SoftLight,
        usvg::BlendMode::Difference => PaintBlendMode::Difference,
        usvg::BlendMode::Exclusion => PaintBlendMode::Exclusion,
        usvg::BlendMode::Hue => PaintBlendMode::Hue,
        usvg::BlendMode::Saturation => PaintBlendMode::Saturation,
        usvg::BlendMode::Color => PaintBlendMode::Color,
        usvg::BlendMode::Luminosity => PaintBlendMode::Luminosity,
    }
}

fn render_path_with_clips(
    path: &usvg::Path,
    viewport: ViewportTransform,
    additional_clips: &[RenderedPathClipPath],
) -> Vec<RenderedPath> {
    if !path.is_visible() {
        return Vec::new();
    }
    let geometry_to_paint =
        SvgGeometryToPaintTransform(svg_path_transform(path.abs_transform(), viewport));
    let fill = path.fill().map(|fill| {
        svg_paint_server(
            fill.paint(),
            fill.opacity().get(),
            SvgPaintServerToPaintTransform(geometry_to_paint.0),
        )
    });
    let stroke = path.stroke().map(|stroke| {
        svg_paint_server(
            stroke.paint(),
            stroke.opacity().get(),
            SvgPaintServerToPaintTransform(geometry_to_paint.0),
        )
    });
    // A path's fill and stroke share its geometry. Drawing only the supported
    // half of a gradient/pattern path would be a visually plausible but
    // incorrect substitute, so omit the affected path as a whole.
    if fill.as_ref().is_some_and(|paint| paint.is_none())
        || stroke.as_ref().is_some_and(|paint| paint.is_none())
    {
        return Vec::new();
    }
    let fill = fill.flatten();
    let stroke = stroke.flatten();
    if fill.is_none() && stroke.is_none() {
        return Vec::new();
    }
    let fill_commands = path_commands(path.data());
    if fill_commands.is_empty() {
        return Vec::new();
    }
    let fill_rule = match path.fill().map(usvg::Fill::rule) {
        Some(usvg::FillRule::EvenOdd) => RenderedPathFillRule::EvenOdd,
        _ => RenderedPathFillRule::NonZero,
    };
    let clip = if viewport.clip_viewport {
        let mut clip = viewport_clip(viewport);
        clip.additional_clips.extend_from_slice(additional_clips);
        Some(clip)
    } else {
        let mut clips = additional_clips.iter().cloned();
        clips.next().map(|primary| {
            let mut clip = RenderedPathClip::new(primary.commands, primary.fill_rule, Vec::new());
            clip.additional_clips.extend(clips);
            clip
        })
    };

    let fill = fill.map(|paint| SvgPaintOperation {
        geometry: SvgPathGeometry {
            commands: fill_commands.clone(),
            to_paint: geometry_to_paint,
        },
        paint,
        fill_rule,
        clip: clip.clone(),
    });
    let stroke = path.stroke().and_then(|stroke_style| {
        let paint = stroke?;
        let outline = path.data().stroke(
            &stroke_style.to_tiny_skia(),
            svg_stroke_resolution_scale(geometry_to_paint),
        )?;
        Some(SvgPaintOperation {
            geometry: SvgPathGeometry {
                commands: path_commands(&outline),
                to_paint: geometry_to_paint,
            },
            paint,
            fill_rule: RenderedPathFillRule::NonZero,
            clip: clip.clone(),
        })
    });

    let mut rendered =
        Vec::with_capacity(usize::from(fill.is_some()) + usize::from(stroke.is_some()));
    match path.paint_order() {
        usvg::PaintOrder::FillAndStroke => {
            rendered.extend(fill.map(SvgPaintOperation::materialize));
            rendered.extend(stroke.map(SvgPaintOperation::materialize));
        }
        usvg::PaintOrder::StrokeAndFill => {
            rendered.extend(stroke.map(SvgPaintOperation::materialize));
            rendered.extend(fill.map(SvgPaintOperation::materialize));
        }
    }
    rendered
}

/// Convert a normalized `usvg` paint into the vector paint tree.
///
/// PDF carries color shadings and a separate alpha soft mask for each
/// normalized SVG gradient, preserving independent stop color and opacity.
fn svg_paint(paint: &usvg::Paint, opacity: f32) -> Option<RenderedPathPaint> {
    match paint {
        usvg::Paint::Color(color) => Some(RenderedPathPaint::Solid(svg_color(*color, opacity))),
        usvg::Paint::LinearGradient(gradient) => {
            svg_linear_gradient(gradient, opacity).map(RenderedPathPaint::Gradient)
        }
        usvg::Paint::RadialGradient(gradient) => {
            svg_radial_gradient(gradient, opacity).map(RenderedPathPaint::Gradient)
        }
        usvg::Paint::Pattern(_) => None,
    }
}

/// Resolve an SVG paint server separately from its target geometry.
///
/// The transform records how the paint server's context element maps into the
/// page. Its eventual path can be a marker child with a different geometry
/// transform, so this function deliberately does not mutate the generic paint
/// representation yet.
fn svg_paint_server(
    paint: &usvg::Paint,
    opacity: f32,
    to_paint: SvgPaintServerToPaintTransform,
) -> Option<SvgPaintServer> {
    if let usvg::Paint::Pattern(pattern) = paint {
        return svg_pattern(pattern, opacity)
            .map(RenderedPathPaint::SvgPattern)
            .map(|paint| SvgPaintServer { paint, to_paint });
    }
    svg_paint(paint, opacity).map(|paint| SvgPaintServer { paint, to_paint })
}

/// Convert the supported vector subset of an SVG pattern into a PDF tiling
/// cell.  The cell remains in the target path's SVG user space: PDF emission
/// installs it while the path CTM is active, which applies element transforms
/// to the geometry and paint server exactly once.
///
/// SVG 2, 13.4 defines pattern content and `patternTransform` in this user
/// coordinate system: <https://www.w3.org/TR/SVG2/pservers.html#Patterns>.
fn svg_pattern(pattern: &usvg::Pattern, opacity: f32) -> Option<RenderedSvgPathPattern> {
    if !opacity.is_finite() || opacity <= 0.0 || opacity > 1.0 {
        return None;
    }
    let rect = pattern.rect();
    let tile_width = rect.width();
    let tile_height = rect.height();
    if !(tile_width.is_finite() && tile_height.is_finite())
        || tile_width <= 0.0
        || tile_height <= 0.0
    {
        return None;
    }
    let paths = svg_pattern_group_paths(pattern.root())?;
    Some(RenderedSvgPathPattern {
        tile_size: PaintSize::new(tile_width, tile_height),
        origin: PaintPoint::new(rect.x(), rect.y()),
        transform: svg_gradient_transform(pattern.transform()),
        paths,
        opacity,
    })
}

/// Pattern cells intentionally support only ordinary opaque vector paths for
/// now.  This keeps an unsupported nested paint server or effect from being
/// silently approximated by the PDF tiling Form.
fn svg_pattern_group_paths(group: &usvg::Group) -> Option<Vec<RenderedPath>> {
    if group.mask().is_some()
        || !group.filters().is_empty()
        || group.clip_path().is_some()
        || group.opacity().get() != 1.0
        || group.blend_mode() != usvg::BlendMode::Normal
        || group.isolate()
    {
        return None;
    }
    let mut paths = Vec::new();
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => paths.extend(svg_pattern_group_paths(group)?),
            usvg::Node::Path(path) => {
                let path = svg_pattern_path(path)?;
                paths.push(path);
            }
            usvg::Node::Image(_) | usvg::Node::Text(_) => return None,
        }
    }
    Some(paths)
}

fn svg_pattern_path(path: &usvg::Path) -> Option<RenderedPath> {
    if !path.is_visible() {
        return None;
    }
    let fill = match path.fill() {
        Some(fill) => Some(svg_pattern_solid_paint(fill)?),
        None => None,
    };
    let stroke = match path.stroke() {
        Some(stroke) => Some(svg_pattern_stroke_paint(stroke)?),
        None => None,
    };
    if fill.is_none() && stroke.is_none() {
        return None;
    }
    let commands = path_commands(path.data());
    if commands.is_empty() {
        return None;
    }
    let fill_rule = match path.fill().map(usvg::Fill::rule) {
        Some(usvg::FillRule::EvenOdd) => RenderedPathFillRule::EvenOdd,
        _ => RenderedPathFillRule::NonZero,
    };
    let stroke_width = PaintStrokeWidth::new(
        path.stroke()
            .map(|stroke| stroke.width().get())
            .unwrap_or(0.0),
    );
    let transform = path.abs_transform();
    let rendered = RenderedPath::new(commands, fill, fill_rule, stroke, stroke_width, None)
        .with_transform(PaintTransform::new(
            transform.sx,
            transform.ky,
            transform.kx,
            transform.sy,
            transform.tx,
            transform.ty,
        ));
    let rendered = if let Some(stroke) = path.stroke() {
        rendered.with_stroke_style(RenderedPathStrokeStyle {
            line_cap: match stroke.linecap() {
                usvg::LineCap::Butt => RenderedPathLineCap::Butt,
                usvg::LineCap::Round => RenderedPathLineCap::Round,
                usvg::LineCap::Square => RenderedPathLineCap::Square,
            },
            line_join: match stroke.linejoin() {
                usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => RenderedPathLineJoin::Miter,
                usvg::LineJoin::Round => RenderedPathLineJoin::Round,
                usvg::LineJoin::Bevel => RenderedPathLineJoin::Bevel,
            },
            miter_limit: stroke.miterlimit().get(),
            dash_array: stroke.dasharray().unwrap_or_default().to_vec(),
            dash_offset: stroke.dashoffset(),
        })
    } else {
        rendered
    };
    Some(rendered.with_paint_order(match path.paint_order() {
        usvg::PaintOrder::FillAndStroke => RenderedPathPaintOrder::FillThenStroke,
        usvg::PaintOrder::StrokeAndFill => RenderedPathPaintOrder::StrokeThenFill,
    }))
}

fn svg_pattern_solid_paint(fill: &usvg::Fill) -> Option<CssColor> {
    let usvg::Paint::Color(color) = fill.paint() else {
        return None;
    };
    let color = svg_color(*color, fill.opacity().get());
    color.is_opaque().then_some(color)
}

fn svg_pattern_stroke_paint(stroke: &usvg::Stroke) -> Option<CssColor> {
    let usvg::Paint::Color(color) = stroke.paint() else {
        return None;
    };
    let color = svg_color(*color, stroke.opacity().get());
    color.is_opaque().then_some(color)
}

fn svg_linear_gradient(gradient: &usvg::LinearGradient, opacity: f32) -> Option<RenderedGradient> {
    let start = PaintPoint::new(gradient.x1(), gradient.y1());
    let end = PaintPoint::new(gradient.x2(), gradient.y2());
    let stops = svg_gradient_stops(gradient.stops(), opacity)?;
    svg_gradient_spread(gradient.spread_method())?;
    Some(RenderedGradient {
        kind: RenderedGradientKind::Linear { start, end },
        color_space: crate::css::CssColorSpace::Srgb,
        stops,
        periodic: None,
        transform: svg_gradient_transform(gradient.transform()),
    })
}

fn svg_radial_gradient(gradient: &usvg::RadialGradient, opacity: f32) -> Option<RenderedGradient> {
    // A PDF shading-pattern matrix applies SVG's full affine transform before
    // the path CTM, so non-uniform transforms keep radial circles as SVG
    // ellipses rather than reducing their radius to an approximation.
    let start_center = PaintPoint::new(gradient.fx(), gradient.fy());
    let end_center = PaintPoint::new(gradient.cx(), gradient.cy());
    let stops = svg_gradient_stops(gradient.stops(), opacity)?;
    svg_gradient_spread(gradient.spread_method())?;
    Some(RenderedGradient {
        kind: RenderedGradientKind::Radial {
            start_center,
            start_radius: gradient.fr().get(),
            end_center,
            end_radius: gradient.r().get(),
        },
        color_space: crate::css::CssColorSpace::Srgb,
        stops,
        periodic: None,
        transform: svg_gradient_transform(gradient.transform()),
    })
}

/// Normalize SVG gradient stops to the PDF shading function domain.
///
/// SVG gradients pad the first and last stop colors to offsets zero and one,
/// respectively.  Keeping coincident stops is equally important: SVG uses
/// them to represent a discontinuous color transition, which a PDF stitching
/// function models with adjacent intervals sharing a boundary.
/// <https://www.w3.org/TR/SVG2/pservers.html#GradientStops>
fn svg_gradient_stops(stops: &[usvg::Stop], opacity: f32) -> Option<Vec<RenderedGradientStop>> {
    let first = stops.first()?;
    let last = stops.last()?;
    let rendered = (first.offset().get() > 0.0)
        .then_some(RenderedGradientStop {
            offset: 0.0,
            color: svg_color(first.color(), first.opacity().get() * opacity),
            interpolation_exponent: 1.0,
        })
        .into_iter()
        .chain(stops.iter().map(|stop| RenderedGradientStop {
            offset: stop.offset().get(),
            color: svg_color(stop.color(), stop.opacity().get() * opacity),
            interpolation_exponent: 1.0,
        }))
        .chain((last.offset().get() < 1.0).then_some(RenderedGradientStop {
            offset: 1.0,
            color: svg_color(last.color(), last.opacity().get() * opacity),
            interpolation_exponent: 1.0,
        }))
        .collect::<Vec<_>>();
    (rendered.len() >= 2).then_some(rendered)
}

fn svg_gradient_spread(method: usvg::SpreadMethod) -> Option<()> {
    match method {
        usvg::SpreadMethod::Pad => Some(()),
        // PDF's axial/radial `Extend` covers pad. Repeating and reflecting
        // require a separately tiled vector pattern and remain unsupported.
        usvg::SpreadMethod::Repeat | usvg::SpreadMethod::Reflect => None,
    }
}

fn svg_gradient_transform(transform: usvg::Transform) -> PaintTransform {
    PaintTransform::new(
        transform.sx,
        transform.ky,
        transform.kx,
        transform.sy,
        transform.tx,
        transform.ty,
    )
}

/// Return the target-resolution scale used while expanding an SVG stroke.
///
/// The path stroker operates in SVG user space, while the resulting outline
/// is later projected into page paint space. Supplying the maximum affine
/// scale keeps curved joins accurate after non-uniform SVG and viewport
/// transforms without introducing a raster fallback.
fn svg_stroke_resolution_scale(transform: SvgGeometryToPaintTransform) -> f32 {
    let transform = transform.0;
    tiny_skia_path::PathStroker::compute_resolution_scale(&tiny_skia_path::Transform::from_row(
        transform.a(),
        transform.b(),
        transform.c(),
        transform.d(),
        transform.e(),
        transform.f(),
    ))
}

fn transform_path_command(
    command: RenderedPathCommand,
    transform: PaintTransform,
) -> RenderedPathCommand {
    match command {
        RenderedPathCommand::MoveTo(point) => {
            RenderedPathCommand::move_to(transform.apply_point(point))
        }
        RenderedPathCommand::LineTo(point) => {
            RenderedPathCommand::line_to(transform.apply_point(point))
        }
        RenderedPathCommand::CurveTo {
            control_1,
            control_2,
            end,
        } => RenderedPathCommand::curve_to(
            transform.apply_point(control_1),
            transform.apply_point(control_2),
            transform.apply_point(end),
        ),
        RenderedPathCommand::Close => RenderedPathCommand::Close,
    }
}

fn viewport_clip(viewport: ViewportTransform) -> RenderedPathClip {
    let destination = viewport.destination;
    let left = destination.min_x();
    let right = destination.max_x();
    let bottom = destination.min_y();
    let top = destination.max_y();
    RenderedPathClip::new(
        vec![
            RenderedPathCommand::move_to(PaintPoint::new(left, bottom)),
            RenderedPathCommand::line_to(PaintPoint::new(right, bottom)),
            RenderedPathCommand::line_to(PaintPoint::new(right, top)),
            RenderedPathCommand::line_to(PaintPoint::new(left, top)),
            RenderedPathCommand::Close,
        ],
        RenderedPathFillRule::NonZero,
        Vec::new(),
    )
}

fn path_commands(path: &tiny_skia_path::Path) -> Vec<RenderedPathCommand> {
    let mut commands = Vec::new();
    let mut current = tiny_skia_path::Point::zero();
    for segment in path.segments() {
        match segment {
            tiny_skia_path::PathSegment::MoveTo(point) => {
                current = point;
                commands.push(RenderedPathCommand::move_to(PaintPoint::new(
                    point.x, point.y,
                )));
            }
            tiny_skia_path::PathSegment::LineTo(point) => {
                current = point;
                commands.push(RenderedPathCommand::line_to(PaintPoint::new(
                    point.x, point.y,
                )));
            }
            tiny_skia_path::PathSegment::QuadTo(control, end) => {
                let control_1 = tiny_skia_path::Point::from_xy(
                    current.x + (control.x - current.x) * (2.0 / 3.0),
                    current.y + (control.y - current.y) * (2.0 / 3.0),
                );
                let control_2 = tiny_skia_path::Point::from_xy(
                    end.x + (control.x - end.x) * (2.0 / 3.0),
                    end.y + (control.y - end.y) * (2.0 / 3.0),
                );
                current = end;
                commands.push(RenderedPathCommand::curve_to(
                    PaintPoint::new(control_1.x, control_1.y),
                    PaintPoint::new(control_2.x, control_2.y),
                    PaintPoint::new(end.x, end.y),
                ));
            }
            tiny_skia_path::PathSegment::CubicTo(control_1, control_2, end) => {
                current = end;
                commands.push(RenderedPathCommand::curve_to(
                    PaintPoint::new(control_1.x, control_1.y),
                    PaintPoint::new(control_2.x, control_2.y),
                    PaintPoint::new(end.x, end.y),
                ));
            }
            tiny_skia_path::PathSegment::Close => commands.push(RenderedPathCommand::Close),
        }
    }
    commands
}

fn svg_path_transform(transform: usvg::Transform, viewport: ViewportTransform) -> PaintTransform {
    let mapping = viewport.source_to_paint;
    PaintTransform::new(
        mapping.sx * transform.sx,
        mapping.sy * transform.ky,
        mapping.sx * transform.kx,
        mapping.sy * transform.sy,
        mapping.sx * transform.tx + mapping.tx,
        mapping.sy * transform.ty + mapping.ty,
    )
}

fn unit_paint_rect() -> PaintRect {
    PaintRect::new(PaintPoint::new(0.0, 0.0), PaintSize::new(1.0, 1.0))
}

fn svg_color(color: usvg::Color, opacity: f32) -> CssColor {
    CssColor::rgba(color.red, color.green, color.blue, opacity)
}

pub(crate) fn parse_inline_svg(element: &Element) -> Result<SvgAsset, String> {
    let xml = serialize_inline_svg(element);
    parse_svg_bytes(xml.as_bytes())
}

/// Parses an inline SVG after applying cascaded CSS transform overrides.
///
/// The host document's stylesheet is not part of the standalone SVG payload,
/// so the layout phase supplies only the transform declarations that apply to
/// SVG descendants. They are serialized as presentation attributes with the
/// CSS cascade's higher-priority result already selected.
pub(crate) fn parse_inline_svg_with_presentation_overrides(
    element: &Element,
    overrides: &SvgPresentationOverrides,
) -> Result<SvgAsset, String> {
    let xml = serialize_inline_svg_with_presentation_overrides(element, overrides);
    parse_svg_bytes(xml.as_bytes())
}

pub(crate) fn parse_svg_bytes(bytes: &[u8]) -> Result<SvgAsset, String> {
    let tree = parse_svg_tree(
        bytes,
        usvg::Size::from_wh(300.0, 150.0).expect("default SVG viewport is valid"),
    )?;
    if svg_tree_has_unsupported_content(tree.root()) {
        log::debug!(
            "SVG contains unsupported paints or compositing; affected nodes will not be painted"
        );
    }
    let size = tree.size();
    let intrinsic_dimensions = svg_intrinsic_dimensions(bytes, size);
    let has_degenerate_view_box = svg_has_degenerate_view_box(bytes);
    let view_fragments = svg_view_fragments(bytes);
    Ok(SvgAsset {
        tree,
        intrinsic_size: LayoutSize::new(
            size.width() * css::CSS_PX_TO_PT,
            size.height() * css::CSS_PX_TO_PT,
        ),
        intrinsic_dimensions,
        has_degenerate_view_box,
        view_fragments,
        source: Rc::from(bytes),
    })
}

/// Substitute the used CSS image viewport for an SVG root's own viewport
/// dimensions. The source stays otherwise byte-for-byte intact, so parsing
/// continues to handle namespaces, style, and child geometry.
fn svg_with_css_image_viewport(bytes: &[u8], width: f32, height: f32) -> Option<Vec<u8>> {
    let source = std::str::from_utf8(bytes).ok()?;
    let document = usvg::roxmltree::Document::parse(source).ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" || !width.is_finite() || !height.is_finite() {
        return None;
    }

    let root_start = root.range().start;
    let mut rewritten = source.to_owned();
    let replacements = root
        .attributes()
        .filter_map(|attribute| match attribute.name() {
            "width" => Some((attribute.range(), format!(r#"width="{width}px""#))),
            "height" => Some((attribute.range(), format!(r#"height="{height}px""#))),
            _ => None,
        })
        .collect::<Vec<_>>();
    let has_width = replacements
        .iter()
        .any(|(_, replacement)| replacement.starts_with("width="));
    let has_height = replacements
        .iter()
        .any(|(_, replacement)| replacement.starts_with("height="));
    for (range, replacement) in replacements.into_iter().rev() {
        rewritten.replace_range(range, &replacement);
    }

    let close = svg_start_tag_close(rewritten.get(root_start..)?)? + root_start;
    let insert_at = rewritten[..close]
        .trim_end_matches(char::is_whitespace)
        .strip_suffix('/')
        .map_or(close, str::len);
    if !has_width {
        rewritten.insert_str(insert_at, &format!(r#" width="{width}px""#));
    }
    if !has_height {
        let close = svg_start_tag_close(rewritten.get(root_start..)?)? + root_start;
        let insert_at = rewritten[..close]
            .trim_end_matches(char::is_whitespace)
            .strip_suffix('/')
            .map_or(close, str::len);
        rewritten.insert_str(insert_at, &format!(r#" height="{height}px""#));
    }
    Some(rewritten.into_bytes())
}

/// Apply an external SVG `<view>` fragment by installing its viewBox on an
/// otherwise viewBox-less root viewport before `usvg` normalizes the tree.
///
/// The `<view>` target replaces the initial view when an SVG is referenced as
/// an image fragment. Keeping this source-level operation before parsing is
/// essential: it makes both paths and percentage geometry use the selected
/// view's user coordinate system.
/// <https://www.w3.org/TR/SVG2/linking.html#LinksIntoSVG>
fn svg_with_view_fragment_view_box(bytes: &[u8], fragment: &str) -> Option<Vec<u8>> {
    let source = std::str::from_utf8(bytes).ok()?;
    let document = usvg::roxmltree::Document::parse(source).ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" || root.attribute("viewBox").is_some() {
        return None;
    }
    let view_box = document
        .descendants()
        .find(|node| node.tag_name().name() == "view" && node.attribute("id") == Some(fragment))?
        .attribute("viewBox")?;
    let [x, y, width, height] = svg_view_box_values(Some(view_box))?;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let start = root.range().start;
    let close = svg_start_tag_close(source.get(start..)?)? + start;
    let insert_at = source[..close]
        .trim_end_matches(char::is_whitespace)
        .strip_suffix('/')
        .map_or(close, |prefix| prefix.len());
    let mut normalized = source.as_bytes().to_vec();
    normalized.splice(
        insert_at..insert_at,
        format!(" viewBox=\"{x} {y} {width} {height}\"").bytes(),
    );
    Some(normalized)
}

/// Locate the closing `>` of an XML start tag without treating a quoted `>`
/// as markup.
fn svg_start_tag_close(source: &str) -> Option<usize> {
    let mut quote = None;
    for (index, character) in source.char_indices() {
        match (quote, character) {
            (Some(delimiter), character) if character == delimiter => quote = None,
            (None, '\'' | '\"') => quote = Some(character),
            (None, '>') => return Some(index),
            _ => {}
        }
    }
    None
}

fn parse_svg_tree(bytes: &[u8], default_size: usvg::Size) -> Result<usvg::Tree, String> {
    let options = usvg::Options {
        default_size,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    usvg::Tree::from_data(bytes, &options).map_err(|error| error.to_string())
}

/// Extract SVG `<view>` fragment aspect ratios used by external SVG images.
fn svg_view_fragments(bytes: &[u8]) -> HashMap<String, SvgIntrinsicDimensions> {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return HashMap::new();
    };
    let Ok(document) = usvg::roxmltree::Document::parse(source) else {
        return HashMap::new();
    };
    document
        .descendants()
        .filter(|node| node.tag_name().name() == "view")
        .filter_map(|node| {
            let id = node.attribute("id")?.to_string();
            let aspect_ratio = svg_view_box_aspect_ratio(node.attribute("viewBox"))?;
            Some((
                id,
                SvgIntrinsicDimensions {
                    width: None,
                    height: None,
                    aspect_ratio: Some(aspect_ratio),
                },
            ))
        })
        .collect()
}

/// Extract the CSS-facing intrinsic dimensions from an SVG root element.
///
/// `usvg::Tree::size` is the concrete SVG viewport and therefore includes the
/// SVG default viewport fallback. The root attributes determine whether those
/// dimensions are intrinsic to the image. Percentages intentionally do not
/// provide an intrinsic dimension because their containing-block basis is not
/// available while sizing a CSS image.
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
/// <https://www.w3.org/TR/SVG2/coords.html#IntrinsicSizing>
fn svg_intrinsic_dimensions(bytes: &[u8], viewport: usvg::Size) -> SvgIntrinsicDimensions {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return SvgIntrinsicDimensions {
            width: None,
            height: None,
            aspect_ratio: None,
        };
    };
    // `usvg` accepts SVG documents with an external DTD, but `roxmltree`
    // intentionally does not resolve external subsets. The DTD has no role
    // in the root geometry we read here, so remove its declaration before
    // extracting the root dimensions and `viewBox` ratio.
    // <https://www.w3.org/TR/SVG2/coords.html#IntrinsicSizing>
    let source = svg_source_without_doctype(source);
    let Ok(document) = usvg::roxmltree::Document::parse(&source) else {
        return SvgIntrinsicDimensions {
            width: None,
            height: None,
            aspect_ratio: None,
        };
    };
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return SvgIntrinsicDimensions {
            width: None,
            height: None,
            aspect_ratio: None,
        };
    }

    let width = root
        .attribute("width")
        .filter(|value| svg_length_is_intrinsic(value))
        .map(|_| layout_pt(viewport.width() * css::CSS_PX_TO_PT));
    let height = root
        .attribute("height")
        .filter(|value| svg_length_is_intrinsic(value))
        .map(|_| layout_pt(viewport.height() * css::CSS_PX_TO_PT));
    // Explicit intrinsic dimensions establish the image's intrinsic ratio.
    // A `viewBox` only supplies the ratio when those dimensions do not both
    // exist; with `preserveAspectRatio="none"`, the viewBox can be stretched
    // independently to the intrinsic viewport.
    // <https://www.w3.org/TR/SVG2/coords.html#IntrinsicSizing>
    let aspect_ratio = width
        .zip(height)
        .and_then(|(width, height)| {
            (width > layout_pt(0.0) && height > layout_pt(0.0))
                .then_some(width.points() / height.points())
        })
        .or_else(|| svg_view_box_aspect_ratio(root.attribute("viewBox")));

    SvgIntrinsicDimensions {
        width,
        height,
        aspect_ratio,
    }
}

/// Remove one XML doctype declaration without interpreting its external or
/// internal subset. XML declarations can contain quoted `>` characters and an
/// internal subset, so the terminator is the first unquoted `>` outside `[]`.
fn svg_source_without_doctype(source: &str) -> std::borrow::Cow<'_, str> {
    let Some(start) = source.find("<!DOCTYPE") else {
        return std::borrow::Cow::Borrowed(source);
    };
    let mut quote = None;
    let mut subset_depth = 0usize;
    for (relative_end, character) in source[start..].char_indices() {
        match (quote, character) {
            (Some(delimiter), character) if character == delimiter => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '\"') => quote = Some(character),
            (None, '[') => subset_depth += 1,
            (None, ']') => subset_depth = subset_depth.saturating_sub(1),
            (None, '>') if subset_depth == 0 => {
                let end = start + relative_end + character.len_utf8();
                let mut stripped = String::with_capacity(source.len() - (end - start));
                stripped.push_str(&source[..start]);
                stripped.push_str(&source[end..]);
                return std::borrow::Cow::Owned(stripped);
            }
            (None, _) => {}
        }
    }
    std::borrow::Cow::Borrowed(source)
}

/// Whether an SVG root `width` or `height` attribute supplies an intrinsic
/// CSS image dimension.
///
/// SVG percentage dimensions depend on the embedding viewport and are thus
/// not intrinsic. All non-percentage values are already resolved by `usvg`
/// into the concrete viewport passed to [`svg_intrinsic_dimensions`].
/// <https://www.w3.org/TR/SVG2/coords.html#IntrinsicSizing>
fn svg_length_is_intrinsic(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains('%')
}

/// Parse the positive width and height of an SVG `viewBox` for its intrinsic
/// aspect ratio. The `viewBox` grammar permits comma and/or whitespace
/// separation.
/// <https://www.w3.org/TR/SVG2/coords.html#ViewBoxAttribute>
fn svg_view_box_aspect_ratio(value: Option<&str>) -> Option<f32> {
    let [_, _, width, height] = svg_view_box_values(value)?;
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then_some(width / height)
}

/// Whether a root `viewBox` is present with a non-positive width or height.
/// Such a `viewBox` is invalid and causes the SVG to render nothing.
/// <https://www.w3.org/TR/SVG2/coords.html#ViewBoxAttribute>
fn svg_has_degenerate_view_box(bytes: &[u8]) -> bool {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return false;
    };
    let Ok(document) = usvg::roxmltree::Document::parse(source) else {
        return false;
    };
    let root = document.root_element();
    root.tag_name().name() == "svg"
        && svg_view_box_values(root.attribute("viewBox"))
            .is_some_and(|[_, _, width, height]| width <= 0.0 || height <= 0.0)
}

/// Parse the four finite numbers in an SVG `viewBox` attribute.
/// <https://www.w3.org/TR/SVG2/coords.html#ViewBoxAttribute>
fn svg_view_box_values(value: Option<&str>) -> Option<[f32; 4]> {
    let value = value?;
    let values = value
        .split(|character: char| character == ',' || character.is_ascii_whitespace())
        .filter(|part| !part.is_empty())
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let [x, y, width, height] = values.as_slice() else {
        return None;
    };
    [*x, *y, *width, *height]
        .into_iter()
        .all(f32::is_finite)
        .then_some([*x, *y, *width, *height])
}

fn svg_tree_has_unsupported_content(group: &usvg::Group) -> bool {
    group.mask().is_some()
        || !group.filters().is_empty()
        || group.children().iter().any(|node| match node {
            usvg::Node::Group(group) => svg_tree_has_unsupported_content(group),
            usvg::Node::Image(_) | usvg::Node::Text(_) => true,
            usvg::Node::Path(path) => {
                path.fill()
                    .is_some_and(|fill| !svg_paint_is_supported(fill.paint(), fill.opacity().get()))
                    || path.stroke().is_some_and(|stroke| {
                        !svg_paint_is_supported(stroke.paint(), stroke.opacity().get())
                    })
            }
        })
}

fn svg_paint_is_supported(paint: &usvg::Paint, opacity: f32) -> bool {
    match paint {
        usvg::Paint::Pattern(pattern) => svg_pattern(pattern, opacity).is_some(),
        _ => svg_paint(paint, opacity).is_some(),
    }
}

pub(crate) fn serialize_inline_svg(element: &Element) -> String {
    serialize_inline_svg_with_presentation_overrides(element, &SvgPresentationOverrides::new())
}

/// Compose an SVG presentation `transform-origin` around an existing SVG
/// transform list.
///
/// `usvg` consumes the SVG transform attribute but does not retain CSS
/// Transforms' origin property.  For basic graphics we can resolve the
/// presentation attribute against the element's fill box before handing the
/// normalized payload to `usvg`.  More complex geometry retains its original
/// transform until the scene adapter has a general fill/stroke-box resolver.
/// <https://drafts.csswg.org/css-transforms-1/#svg-transform>
fn svg_presentation_transform_with_origin(element: &Element, transform: &str) -> String {
    let Some(origin) = element.attrs.get("transform-origin") else {
        return transform.to_owned();
    };
    let Some((x, y, width, height)) = svg_rect_fill_box(element) else {
        return transform.to_owned();
    };
    let Some((origin_x, origin_y)) = svg_transform_origin_in_fill_box(origin, x, y, width, height)
    else {
        // An invalid presentation attribute is ignored, leaving SVG's
        // transform-origin initial value (0 0) in effect.
        return transform.to_owned();
    };
    format!(
        "translate({origin_x} {origin_y}) {transform} translate({} {})",
        -origin_x, -origin_y
    )
}

fn svg_rect_fill_box(element: &Element) -> Option<(f32, f32, f32, f32)> {
    if element.tag != "rect" {
        return None;
    }
    let number = |name: &str, default: f32| {
        element
            .attrs
            .get(name)
            .map_or(Some(default), |value| svg_user_length(value))
    };
    let x = number("x", 0.0)?;
    let y = number("y", 0.0)?;
    let width = number("width", 0.0)?;
    let height = number("height", 0.0)?;
    (width >= 0.0 && height >= 0.0).then_some((x, y, width, height))
}

/// Resolve the SVG subset of the CSS `<position>` grammar used by
/// `transform-origin`. Percentages use the selected fill box dimensions and
/// unitless SVG lengths remain in the current user coordinate system.
fn svg_transform_origin_in_fill_box(
    value: &str,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Option<(f32, f32)> {
    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    let (x_value, y_value) = match parts.as_slice() {
        [single] if svg_origin_is_vertical_keyword(single) => ("center", *single),
        [single] => (*single, "center"),
        [first, second]
            if svg_origin_is_vertical_keyword(first)
                && svg_origin_is_horizontal_keyword(second) =>
        {
            (*second, *first)
        }
        // `center` is axis-ambiguous. When it precedes a horizontal edge it
        // supplies the vertical component, as in `center left`.
        [first, second]
            if first.eq_ignore_ascii_case("center")
                && matches!(second.to_ascii_lowercase().as_str(), "left" | "right") =>
        {
            (*second, *first)
        }
        // A vertical keyword cannot precede a numeric or percentage value;
        // CSS Transforms treats that declaration as invalid.
        [first, _] if svg_origin_is_vertical_keyword(first) => return None,
        [first, second] => (*first, *second),
        _ => return None,
    };
    Some((
        svg_origin_coordinate(x_value, x, width, false)?,
        svg_origin_coordinate(y_value, y, height, true)?,
    ))
}

fn svg_origin_is_vertical_keyword(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "top" | "bottom")
}

fn svg_origin_is_horizontal_keyword(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "left" | "right" | "center"
    )
}

fn svg_origin_coordinate(value: &str, start: f32, extent: f32, vertical: bool) -> Option<f32> {
    match value.to_ascii_lowercase().as_str() {
        "center" => Some(start + extent * 0.5),
        "left" if !vertical => Some(start),
        "right" if !vertical => Some(start + extent),
        "top" if vertical => Some(start),
        "bottom" if vertical => Some(start + extent),
        value if value.ends_with('%') => value[..value.len() - 1]
            .trim()
            .parse::<f32>()
            .ok()
            .map(|percent| start + extent * percent / 100.0),
        // SVG presentation-attribute lengths are current-user-coordinate
        // values. Unlike CSS percentages and position keywords, they do not
        // become an offset from the selected fill/stroke box.
        value => svg_user_length(value),
    }
}

/// Resolve absolute SVG/CSS lengths into SVG user units (CSS pixels).
///
/// This is deliberately kept at the SVG serialization boundary: CSS layout
/// stores absolute lengths in points, whereas SVG presentation attributes are
/// parsed in the current SVG user coordinate system.
fn svg_user_length(value: &str) -> Option<f32> {
    let value = value.trim();
    let units = [
        ("px", 1.0),
        ("in", 96.0),
        ("cm", 96.0 / 2.54),
        ("mm", 96.0 / 25.4),
        ("pt", 96.0 / 72.0),
        ("pc", 16.0),
    ];
    for (suffix, scale) in units {
        if let Some(number) = value.strip_suffix(suffix) {
            return number
                .trim()
                .parse::<f32>()
                .ok()
                .map(|number| number * scale);
        }
    }
    value.parse::<f32>().ok()
}

fn serialize_inline_svg_with_presentation_overrides(
    element: &Element,
    overrides: &SvgPresentationOverrides,
) -> String {
    let mut output = String::new();
    serialize_element(
        element,
        true,
        &mut NamespacePrefixes::default(),
        overrides,
        &mut output,
    );
    output
}

#[derive(Default)]
struct NamespacePrefixes {
    prefixes: HashMap<String, String>,
}

impl NamespacePrefixes {
    fn prefix_for(&mut self, namespace_url: &str) -> String {
        if let Some(prefix) = self.prefixes.get(namespace_url) {
            return prefix.clone();
        }
        let prefix = format!("ns{}", self.prefixes.len());
        self.prefixes
            .insert(namespace_url.to_owned(), prefix.clone());
        prefix
    }
}

fn serialize_element(
    element: &Element,
    root: bool,
    prefixes: &mut NamespacePrefixes,
    overrides: &SvgPresentationOverrides,
    output: &mut String,
) {
    let override_values = overrides.get(&element.id);
    match override_values.and_then(|values| values.display) {
        Some(SvgDisplayOverride::None) => return,
        Some(SvgDisplayOverride::Contents) => {
            for child in &element.children {
                match &child.kind {
                    NodeKind::Text(text) => push_escaped_text(output, text),
                    NodeKind::Element(child) => {
                        serialize_element(child, false, prefixes, overrides, output)
                    }
                }
            }
            return;
        }
        Some(SvgDisplayOverride::UseContents) | None => {}
    }
    let mut namespace_declarations = Vec::new();
    let tag = if root || element.namespace_url.is_empty() || element.namespace_url == SVG_NAMESPACE
    {
        element.tag.clone()
    } else {
        let prefix = prefixes.prefix_for(&element.namespace_url);
        namespace_declarations.push((prefix.clone(), element.namespace_url.clone()));
        format!("{prefix}:{}", element.tag)
    };
    output.push('<');
    output.push_str(&tag);
    if root {
        output.push_str(" xmlns=\"");
        output.push_str(SVG_NAMESPACE);
        output.push('"');
    }
    let mut attrs: Vec<_> = element.attrs.iter().collect();
    attrs.sort_unstable_by_key(|(name, _)| *name);
    let source_transform = override_values
        .and_then(|values| values.transform.clone())
        .or_else(|| element.attrs.get("transform").cloned());
    let resolved_transform = source_transform
        .as_deref()
        .map(|transform| svg_presentation_transform_with_origin(element, transform));
    let mut emitted_transform = false;
    for (name, value) in attrs {
        if name == "xmlns" || name.starts_with("xmlns:") {
            continue;
        }
        if name == "transform-origin" && resolved_transform.is_some() {
            // `usvg` would resolve this presentation attribute against its
            // viewport after the normalized transform has already been
            // wrapped around the selected CSS reference box.
            continue;
        }
        if name == "filter" && override_values.is_some_and(|values| values.remove_filter) {
            continue;
        }
        if name == "style"
            && override_values.is_some_and(|values| {
                matches!(values.display, Some(SvgDisplayOverride::UseContents))
            })
        {
            // A stripped `<use>` retains its referenced shadow content but
            // not its own non-inherited layout/visual style. The host cascade
            // separately serializes computed inherited presentation values.
            continue;
        }
        if matches!(name.as_str(), "fill" | "stroke" | "stroke-width")
            && ((name == "fill"
                && override_values
                    .and_then(|values| values.fill.as_ref())
                    .is_some())
                || (name == "stroke"
                    && override_values
                        .and_then(|values| values.stroke.as_ref())
                        .is_some())
                || (name == "stroke-width"
                    && override_values
                        .and_then(|values| values.stroke_width.as_ref())
                        .is_some()))
        {
            // The host CSS declaration has author origin and therefore
            // overrides this SVG presentation attribute.
            continue;
        }
        if name == "style" && override_values.is_some_and(|values| values.remove_filter) {
            // A CSS parser in usvg retains the source filter even when a
            // later declaration resets it. These forced-solid cases have no
            // other presentation declaration, so omit the source style.
            continue;
        } else if name == "transform" {
            emitted_transform = true;
            push_attribute(output, name, resolved_transform.as_deref().unwrap_or(value));
        } else {
            push_attribute(output, name, value);
        }
    }
    if !emitted_transform && let Some(transform) = resolved_transform.as_deref() {
        push_attribute(output, "transform", transform);
    }
    for (name, value) in [
        (
            "fill",
            override_values.and_then(|values| values.fill.as_deref()),
        ),
        (
            "stroke",
            override_values.and_then(|values| values.stroke.as_deref()),
        ),
        (
            "stroke-width",
            override_values.and_then(|values| values.stroke_width.as_deref()),
        ),
    ] {
        if let Some(value) = value {
            push_attribute(output, name, value);
        }
    }
    let mut namespaced = element.namespace_attrs.clone();
    namespaced.sort_unstable_by(|left, right| {
        (&left.namespace_url, &left.local_name).cmp(&(&right.namespace_url, &right.local_name))
    });
    for attribute in namespaced {
        if attribute.namespace_url.is_empty() || attribute.namespace_url == XMLNS_NAMESPACE {
            continue;
        }
        let name = match attribute.namespace_url.as_str() {
            XML_NAMESPACE => format!("xml:{}", attribute.local_name),
            XLINK_NAMESPACE => {
                namespace_declarations.push(("xlink".to_owned(), XLINK_NAMESPACE.to_owned()));
                format!("xlink:{}", attribute.local_name)
            }
            _ => {
                let prefix = prefixes.prefix_for(&attribute.namespace_url);
                namespace_declarations.push((prefix.clone(), attribute.namespace_url.clone()));
                format!("{prefix}:{}", attribute.local_name)
            }
        };
        push_attribute(output, &name, &attribute.value);
    }
    namespace_declarations.sort_unstable();
    namespace_declarations.dedup();
    for (prefix, namespace_url) in namespace_declarations {
        output.push_str(" xmlns:");
        output.push_str(&prefix);
        output.push_str("=\"");
        output.push_str(&namespace_url);
        output.push('"');
    }
    output.push('>');
    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => push_escaped_text(output, text),
            NodeKind::Element(child) => {
                serialize_element(child, false, prefixes, overrides, output)
            }
        }
    }
    output.push_str("</");
    output.push_str(&tag);
    output.push('>');
}

fn push_attribute(output: &mut String, name: &str, value: &str) {
    output.push(' ');
    output.push_str(name);
    output.push_str("=\"");
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '"' => output.push_str("&quot;"),
            _ => output.push(character),
        }
    }
    output.push('"');
}

fn push_escaped_text(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
}

pub(crate) fn svg_intrinsic_size(element: &Element) -> Option<LayoutSize> {
    svg_replaced_size(element).map(|(size, _)| size)
}

/// Return an inline SVG's CSS replaced-object size together with the source
/// dimensions that distinguish a ratio-only image from one with an intrinsic
/// size. Flexbox's automatic minimum size needs that distinction even though
/// both use the default object size while establishing a flex base size.
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
pub(crate) fn svg_replaced_size(element: &Element) -> Option<(LayoutSize, SvgIntrinsicDimensions)> {
    parse_inline_svg(element).ok().map(|asset| {
        let dimensions = asset.intrinsic_dimensions();
        (asset.replaced_intrinsic_size(), dimensions)
    })
}

pub(crate) type SharedSvgAsset = Rc<SvgAsset>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::Node;
    use crate::units::content_box_size_pt;

    #[test]
    fn presentation_origin_rotates_svg_rect_about_its_fill_box_center() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"><rect width="150" height="150" transform="translate(75 75) rotate(90) translate(-75 -75)"/></svg>"#,
        )
        .expect("test SVG parses");
        let paths = asset.paint_paths(paint_rect(0.0, 0.0, 150.0, 150.0));
        let [path] = paths.as_slice() else {
            panic!("expected one path");
        };
        assert_eq!(path.transform, PaintTransform::identity());
        assert_eq!(
            path.commands.first(),
            Some(&RenderedPathCommand::move_to(PaintPoint::new(112.5, 150.0)))
        );
    }

    #[test]
    fn svg_presentation_origin_wraps_a_cascaded_transform_once() {
        let rect = Element {
            id: ElementId::next(),
            tag: "rect".to_owned(),
            namespace_url: "http://www.w3.org/2000/svg".to_owned(),
            document_syntax: crate::dom::DocumentSyntax::Html,
            attrs: HashMap::from([
                ("width".to_owned(), "150".to_owned()),
                ("height".to_owned(), "150".to_owned()),
                ("transform".to_owned(), "rotate(90)".to_owned()),
                ("transform-origin".to_owned(), "75".to_owned()),
            ]),
            namespace_attrs: Vec::new(),
            children: Vec::new(),
            is_target: false,
            object_rendering: crate::dom::ObjectRendering::Fallback,
        };
        let mut overrides = SvgPresentationOverrides::new();
        overrides.insert(
            rect.id,
            SvgPresentationOverride {
                transform: Some("matrix(0 1 -1 0 0 0)".to_owned()),
                ..SvgPresentationOverride::default()
            },
        );
        let xml = serialize_inline_svg_with_presentation_overrides(&rect, &overrides);

        assert!(
            xml.contains("transform=\"translate(75 75) matrix(0 1 -1 0 0 0) translate(-75 -75)\"")
        );
        assert!(!xml.contains("transform-origin="));
    }

    #[test]
    fn host_css_presentation_overrides_replace_svg_paint_attributes() {
        let circle = Element {
            id: ElementId::next(),
            tag: "circle".to_owned(),
            namespace_url: "http://www.w3.org/2000/svg".to_owned(),
            document_syntax: crate::dom::DocumentSyntax::Html,
            attrs: HashMap::from([
                ("fill".to_owned(), "red".to_owned()),
                ("stroke".to_owned(), "black".to_owned()),
            ]),
            namespace_attrs: Vec::new(),
            children: Vec::new(),
            is_target: false,
            object_rendering: crate::dom::ObjectRendering::Fallback,
        };
        let mut overrides = SvgPresentationOverrides::new();
        overrides.insert(
            circle.id,
            SvgPresentationOverride {
                fill: Some("green".to_owned()),
                stroke: Some("purple".to_owned()),
                stroke_width: Some("10px".to_owned()),
                ..SvgPresentationOverride::default()
            },
        );
        let xml = serialize_inline_svg_with_presentation_overrides(&circle, &overrides);

        assert_eq!(xml.matches("fill=").count(), 1);
        assert_eq!(xml.matches("stroke=").count(), 1);
        assert!(xml.contains("fill=\"green\""));
        assert!(xml.contains("stroke=\"purple\""));
        assert!(xml.contains("stroke-width=\"10px\""));
    }

    #[test]
    fn display_contents_svg_override_hoists_children_without_wrapper_style() {
        let text = Element {
            id: ElementId::next(),
            tag: "text".to_owned(),
            namespace_url: SVG_NAMESPACE.to_owned(),
            document_syntax: crate::dom::DocumentSyntax::Html,
            attrs: HashMap::from([("style".to_owned(), "opacity: 0".to_owned())]),
            namespace_attrs: Vec::new(),
            children: vec![Node::text("P")],
            is_target: false,
            object_rendering: crate::dom::ObjectRendering::Fallback,
        };
        let group = Element {
            id: ElementId::next(),
            tag: "g".to_owned(),
            namespace_url: SVG_NAMESPACE.to_owned(),
            document_syntax: crate::dom::DocumentSyntax::Html,
            attrs: HashMap::from([("style".to_owned(), "opacity: 0".to_owned())]),
            namespace_attrs: Vec::new(),
            children: vec![Node {
                kind: NodeKind::Element(text),
            }],
            is_target: false,
            object_rendering: crate::dom::ObjectRendering::Fallback,
        };
        let mut overrides = SvgPresentationOverrides::new();
        overrides.insert(
            group.id,
            SvgPresentationOverride {
                display: Some(SvgDisplayOverride::Contents),
                ..SvgPresentationOverride::default()
            },
        );

        let xml = serialize_inline_svg_with_presentation_overrides(&group, &overrides);
        assert!(!xml.contains("<g"));
        assert!(xml.contains("<text style=\"opacity: 0\">P</text>"));
    }

    #[test]
    fn svg_origin_position_uses_css_absolute_units_and_ambiguous_center_order() {
        assert_eq!(svg_user_length("1in"), Some(96.0));
        assert_eq!(svg_user_length("72pt"), Some(96.0));
        assert_eq!(
            svg_transform_origin_in_fill_box("center right", 75.0, 75.0, 150.0, 150.0),
            Some((225.0, 150.0))
        );
        assert_eq!(
            svg_transform_origin_in_fill_box("100px 0", 0.0, 0.0, 100.0, 100.0),
            Some((100.0, 0.0))
        );
    }

    fn paint_rect(x: f32, y: f32, width: f32, height: f32) -> PaintRect {
        PaintRect::new(PaintPoint::new(x, y), PaintSize::new(width, height))
    }

    fn svg_element(source: &str) -> Element {
        let document = crate::dom::parse_with_syntax(source, crate::dom::DocumentSyntax::Xml)
            .expect("valid SVG XML");
        let NodeKind::Element(document) = document.kind else {
            panic!("expected XML document element");
        };
        let NodeKind::Element(svg) = document.children.into_iter().next().unwrap().kind else {
            panic!("expected SVG root");
        };
        svg
    }

    #[test]
    fn parses_svg_intrinsic_dimensions_in_css_pixels() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="48"><rect width="96" height="48"/></svg>"#,
        );
        let asset = parse_inline_svg(&element).unwrap();
        assert_eq!(asset.intrinsic_size(), LayoutSize::new(72.0, 36.0));
        assert_eq!(
            asset.intrinsic_dimensions(),
            SvgIntrinsicDimensions {
                width: Some(layout_pt(72.0)),
                height: Some(layout_pt(36.0)),
                aspect_ratio: Some(2.0),
            }
        );
    }

    #[test]
    fn external_doctype_does_not_hide_an_svg_view_box_ratio() {
        let asset = parse_svg_bytes(
            br#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.0//EN" "http://www.w3.org/TR/2001/REC-SVG-20010904/DTD/svg10.dtd">
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><rect width="200" height="100"/></svg>"#,
        )
        .expect("test SVG parses");

        assert_eq!(asset.intrinsic_dimensions().width, None);
        assert_eq!(asset.intrinsic_dimensions().height, None);
        assert_eq!(asset.intrinsic_dimensions().aspect_ratio, Some(2.0));
    }

    #[test]
    fn inline_svg_viewport_clip_follows_host_overflow() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="8" fill="green"/></svg>"#,
        )
        .expect("test SVG parses");
        let destination = paint_rect(0.0, 0.0, 10.0, 10.0);
        let clipped = asset.paint_inline_group(destination, true);
        let visible = asset.paint_inline_group(destination, false);
        let [SvgPaintItem::Path(clipped_path)] = clipped.items.as_slice() else {
            panic!("expected one clipped SVG path");
        };
        let [SvgPaintItem::Path(visible_path)] = visible.items.as_slice() else {
            panic!("expected one visible SVG path");
        };
        assert!(clipped_path.clip.is_some());
        assert!(visible_path.clip.is_none());
    }

    #[test]
    fn distinguishes_svg_viewport_fallbacks_from_intrinsic_dimensions() {
        let no_dimensions = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="100%" height="100%"/></svg>"#,
        )
        .unwrap();
        assert_eq!(
            no_dimensions.intrinsic_dimensions(),
            SvgIntrinsicDimensions {
                width: None,
                height: None,
                aspect_ratio: None,
            }
        );

        let view_box_only = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 64"><rect width="100%" height="100%"/></svg>"#,
        )
        .unwrap();
        assert_eq!(
            view_box_only.intrinsic_dimensions(),
            SvgIntrinsicDimensions {
                width: None,
                height: None,
                aspect_ratio: Some(1.0 / 16.0),
            }
        );

        let percentage_height = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="8px" height="50%"><rect width="100%" height="100%"/></svg>"#,
        )
        .unwrap();
        assert_eq!(
            percentage_height.intrinsic_dimensions(),
            SvgIntrinsicDimensions {
                width: Some(layout_pt(8.0 * css::CSS_PX_TO_PT)),
                height: None,
                aspect_ratio: None,
            }
        );
    }

    #[test]
    fn replaced_svg_uses_view_box_ratio_for_a_missing_root_dimension() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="50px" viewBox="0 0 200 400"><rect width="100%" height="100%"/></svg>"#,
        )
        .unwrap();

        assert_eq!(
            asset.replaced_intrinsic_size(),
            LayoutSize::new(50.0 * css::CSS_PX_TO_PT, 100.0 * css::CSS_PX_TO_PT)
        );

        let viewport = asset.with_replaced_viewport(content_box_size_pt(
            50.0 * css::CSS_PX_TO_PT,
            100.0 * css::CSS_PX_TO_PT,
        ));
        assert_eq!(
            viewport.source_viewport_size(),
            SvgSourceSize::new(50.0, 100.0)
        );
    }

    #[test]
    fn ratio_only_replaced_svg_derives_its_default_block_size() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="100%" height="100%"/></svg>"#,
        )
        .unwrap();

        assert_eq!(
            asset.replaced_intrinsic_size(),
            LayoutSize::new(300.0 * css::CSS_PX_TO_PT, 300.0 * css::CSS_PX_TO_PT)
        );
    }

    #[test]
    fn one_axis_svg_intrinsic_size_preserves_that_axis_without_a_ratio() {
        let height_only = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" height="25"><rect width="100%" height="100%"/></svg>"#,
        )
        .unwrap();
        let width_only = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="50"><rect width="100%" height="100%"/></svg>"#,
        )
        .unwrap();

        assert_eq!(
            height_only.replaced_intrinsic_size(),
            LayoutSize::new(300.0 * css::CSS_PX_TO_PT, 25.0 * css::CSS_PX_TO_PT)
        );
        assert_eq!(
            width_only.replaced_intrinsic_size(),
            LayoutSize::new(50.0 * css::CSS_PX_TO_PT, 150.0 * css::CSS_PX_TO_PT)
        );
    }

    #[test]
    fn view_fragment_contributes_its_view_box_ratio_to_css_image_sizing() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><view id="wide" viewBox="0 0 16 8"/></svg>"#,
        )
        .unwrap();

        assert_eq!(asset.intrinsic_dimensions().aspect_ratio, None);
        assert_eq!(
            asset
                .with_view_fragment(Some("wide"))
                .intrinsic_dimensions()
                .aspect_ratio,
            Some(2.0)
        );
    }

    #[test]
    fn normalizes_omitted_svg_viewports_to_the_background_image_size() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="100%" height="100%" fill="orange"/></svg>"#,
        )
        .unwrap();

        assert_eq!(
            asset
                .with_background_viewport(PaintSize::new(150.0, 300.0))
                .source_viewport_size(),
            SvgSourceSize::new(200.0, 400.0)
        );

        let self_closing = svg_with_css_image_viewport(
            br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#,
            200.0,
            400.0,
        )
        .unwrap();
        assert!(
            std::str::from_utf8(&self_closing)
                .unwrap()
                .contains("width=\"200px\" height=\"400px\"/")
        );
    }

    fn assert_opaque_rect_bounds(asset: &SvgAsset, expected: (f32, f32, f32, f32)) {
        let paths = asset.paint_paths(paint_rect(0.0, 0.0, 12.0, 12.0));
        let [path] = paths.as_slice() else {
            panic!("expected one opaque SVG rectangle");
        };
        let (_, actual) = opaque_axis_aligned_rectangle(path)
            .expect("expected an opaque axis-aligned SVG rectangle");
        for (actual, expected) in [actual.0, actual.1, actual.2, actual.3]
            .into_iter()
            .zip([expected.0, expected.1, expected.2, expected.3])
        {
            assert!(
                (actual - expected).abs() < 0.0001,
                "expected {expected}, got {actual}"
            );
        }
    }

    #[test]
    fn background_viewport_honors_default_preserve_aspect_ratio_for_view_box_only_svg() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 8"><rect width="16" height="8" fill="orange"/></svg>"#,
        )
        .unwrap();
        let intrinsic_dimensions = asset.intrinsic_dimensions();
        let viewport = asset.with_background_viewport(PaintSize::new(12.0, 12.0));

        assert_eq!(
            viewport.source_viewport_size(),
            SvgSourceSize::new(16.0, 16.0)
        );
        assert_eq!(viewport.intrinsic_dimensions(), intrinsic_dimensions);
        assert_opaque_rect_bounds(&viewport, (0.0, 3.0, 12.0, 9.0));
    }

    #[test]
    fn background_viewport_honors_default_preserve_aspect_ratio_for_sized_svg() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8" viewBox="0 0 16 8"><rect width="16" height="8" fill="orange"/></svg>"#,
        )
        .unwrap();
        let intrinsic_dimensions = asset.intrinsic_dimensions();
        let viewport = asset.with_background_viewport(PaintSize::new(12.0, 12.0));

        assert_eq!(
            viewport.source_viewport_size(),
            SvgSourceSize::new(16.0, 16.0)
        );
        assert_eq!(viewport.intrinsic_dimensions(), intrinsic_dimensions);
        assert_opaque_rect_bounds(&viewport, (0.0, 3.0, 12.0, 9.0));
    }

    #[test]
    fn background_viewport_preserves_none_aspect_ratio_stretching() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8" viewBox="0 0 16 8" preserveAspectRatio="none"><rect width="16" height="8" fill="orange"/></svg>"#,
        )
        .unwrap();
        let viewport = asset.with_background_viewport(PaintSize::new(12.0, 12.0));

        assert_eq!(
            viewport.source_viewport_size(),
            SvgSourceSize::new(16.0, 16.0)
        );
        assert_opaque_rect_bounds(&viewport, (0.0, 0.0, 12.0, 12.0));
    }

    #[test]
    fn records_degenerate_svg_view_boxes_for_css_image_painting() {
        let degenerate = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 0"><rect width="100%" height="100%"/></svg>"#,
        )
        .unwrap();
        let valid = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 1"><rect width="100%" height="100%"/></svg>"#,
        )
        .unwrap();

        assert!(degenerate.has_degenerate_view_box());
        assert!(!valid.has_degenerate_view_box());
    }

    #[test]
    fn recognizes_an_opaque_svg_viewport_fill_without_relaxing_compositing() {
        let fill = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2147483647 1" preserveAspectRatio="none"><rect width="100%" height="100%" fill="lime"/></svg>"#,
        )
        .unwrap();
        assert_eq!(fill.opaque_viewport_fill(), Some(CssColor::new(0, 255, 0)));

        let tall_fill = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" height="8px" viewBox="0 0 1 2147483647" preserveAspectRatio="none"><rect width="100%" height="100%" fill="lime"/></svg>"#,
        )
        .unwrap();
        assert_eq!(
            tall_fill.opaque_viewport_fill(),
            Some(CssColor::new(0, 255, 0))
        );

        let translucent = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="100%" height="100%" fill="lime" opacity="0.5"/></svg>"#,
        )
        .unwrap();
        assert_eq!(translucent.opaque_viewport_fill(), None);
    }

    #[test]
    fn concrete_replaced_viewport_keeps_a_partial_root_rect_partial() {
        let partial = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="100" height="100" fill="lime"/></svg>"#,
        )
        .unwrap()
        .with_replaced_viewport(content_box_size_pt(150.0, 150.0));
        assert_eq!(partial.opaque_viewport_fill(), None);
        assert_opaque_rect_bounds(&partial, (0.0, 6.0, 6.0, 12.0));

        let viewport_fill = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="100%" height="100%" fill="lime"/></svg>"#,
        )
        .unwrap()
        .with_replaced_viewport(content_box_size_pt(150.0, 150.0));
        assert_eq!(
            viewport_fill.opaque_viewport_fill(),
            Some(CssColor::new(0, 255, 0))
        );
    }

    #[test]
    fn serializes_text_and_attribute_values_as_xml() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text data-label="a &amp; b">x &amp; y</text></svg>"#,
        );
        let serialized = serialize_inline_svg(&element);
        assert!(serialized.contains("data-label=\"a &amp; b\""));
        assert!(serialized.contains(">x &amp; y</text>"));
    }

    #[test]
    fn reconstructs_arbitrary_namespaced_attributes() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:vendor="urn:vendor"><g vendor:role="plot"/></svg>"#,
        );
        let serialized = serialize_inline_svg(&element);
        assert!(serialized.contains("xmlns:ns0=\"urn:vendor\""));
        assert!(serialized.contains("ns0:role=\"plot\""));
    }

    #[test]
    fn normalizes_shapes_to_pdf_compatible_paths() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><circle cx="5" cy="5" r="5" fill="red"/></svg>"#,
        );
        let asset = parse_inline_svg(&element).unwrap();
        let paths = asset.paint_paths(paint_rect(10.0, 20.0, 15.0, 7.5));
        assert_eq!(paths.len(), 1);
        assert!(
            paths[0]
                .commands
                .iter()
                .any(|command| matches!(command, RenderedPathCommand::CurveTo { .. }))
        );
        assert_eq!(paths[0].transform, PaintTransform::identity());
        assert!(paths[0].clip.is_none());
    }

    #[test]
    fn inline_svg_without_dimensions_uses_its_css_replaced_viewport() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="200" height="200"/></svg>"#,
        )
        .unwrap()
        .with_replaced_viewport(content_box_size_pt(300.0, 300.0));

        assert_eq!(
            asset.source_viewport_size(),
            SvgSourceSize::new(400.0, 400.0)
        );
    }

    #[test]
    fn preserves_solid_vector_svg_pattern_as_path_paint() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><pattern id="boxes" patternUnits="userSpaceOnUse" width="50" height="100"><rect width="25" height="50" fill="green"/><rect x="25" width="25" height="50" fill="fuchsia"/><rect y="50" width="25" height="50" fill="yellow"/><rect x="25" y="50" width="25" height="50" fill="blue"/></pattern></defs><rect width="50" height="100" fill="url(#boxes)" transform="matrix(2 0 0 1 0 0)"/></svg>"#,
        )
        .unwrap();
        let paths = asset.paint_paths(paint_rect(0.0, 0.0, 75.0, 75.0));
        let [path] = paths.as_slice() else {
            panic!("expected one patterned SVG path");
        };
        let Some(RenderedPathPaint::SvgPattern(pattern)) = path.fill_paint.as_ref() else {
            panic!("expected an SVG tiling paint");
        };

        assert_eq!(pattern.tile_size, PaintSize::new(50.0, 100.0));
        assert_eq!(pattern.paths.len(), 4);
        assert_eq!(path.transform, PaintTransform::identity());
        assert_ne!(pattern.transform, PaintTransform::identity());
        assert_eq!(pattern.paths[0].fill, Some(CssColor::new(0, 128, 0)));
        assert_eq!(pattern.paths[3].fill, Some(CssColor::new(0, 0, 255)));
    }

    #[test]
    fn invalid_svg_matrix_keeps_patterned_path_untransformed() {
        let source = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><pattern id="p" patternUnits="userSpaceOnUse" width="100" height="100"><rect width="100" height="100" fill="green"/></pattern></defs><rect width="100" height="100" fill="url(#p)"/></svg>"#;
        let plain = parse_svg_bytes(source)
            .unwrap()
            .paint_paths(paint_rect(0.0, 0.0, 75.0, 75.0));
        let invalid = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><pattern id="p" patternUnits="userSpaceOnUse" width="100" height="100"><rect width="100" height="100" fill="green"/></pattern></defs><rect width="100" height="100" fill="url(#p)" transform="matrix(50% 0 0 1 0 0)"/></svg>"#,
        )
        .unwrap()
        .paint_paths(paint_rect(0.0, 0.0, 75.0, 75.0));

        assert_eq!(invalid.len(), 1);
        assert_eq!(invalid[0].transform, plain[0].transform);
        assert!(matches!(
            invalid[0].fill_paint,
            Some(RenderedPathPaint::SvgPattern(_))
        ));
    }

    #[test]
    fn preserves_svg_gradient_hard_stop_and_padded_endpoints() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><linearGradient id="gradient"><stop offset="50%" stop-color="green"/><stop offset="50%" stop-color="yellow"/></linearGradient></defs><rect width="100" height="100" fill="url(#gradient)"/></svg>"#,
        )
        .unwrap();
        let paths = asset.paint_paths(paint_rect(0.0, 0.0, 75.0, 75.0));
        let [path] = paths.as_slice() else {
            panic!("expected one gradient-filled path");
        };
        let Some(RenderedPathPaint::Gradient(gradient)) = path.fill_paint.as_ref() else {
            panic!("expected an SVG gradient fill");
        };

        assert_eq!(gradient.stops.len(), 4);
        assert_eq!(gradient.stops[0].offset, 0.0);
        assert!((gradient.stops[1].offset - 0.5).abs() < 1e-5);
        assert!((gradient.stops[2].offset - 0.5).abs() < 1e-5);
        assert_eq!(gradient.stops[3].offset, 1.0);
        assert_eq!(gradient.stops[0].color, CssColor::new(0, 128, 0));
        assert_eq!(gradient.stops[3].color, CssColor::new(255, 255, 0));
    }

    #[test]
    fn source_viewport_rect_maps_svg_coordinates_into_its_destination() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><path d="M 0 0 H 20 V 10 H 0 Z"/></svg>"#,
        );
        let asset = parse_inline_svg(&element).unwrap();
        let paths = asset.paint_paths_for_source_rect(
            paint_rect(100.0, 200.0, 20.0, 8.0),
            SvgSourceRect::new(SvgSourcePoint::new(5.0, 2.0), SvgSourceSize::new(10.0, 4.0)),
        );

        assert_eq!(paths.len(), 1);
        assert_eq!(
            paths[0].commands.first(),
            Some(&RenderedPathCommand::move_to(PaintPoint::new(90.0, 212.0)))
        );
        assert_eq!(
            paths[0].clip.as_ref().map(|clip| &clip.commands),
            Some(&vec![
                RenderedPathCommand::move_to(PaintPoint::new(100.0, 200.0)),
                RenderedPathCommand::line_to(PaintPoint::new(120.0, 200.0)),
                RenderedPathCommand::line_to(PaintPoint::new(120.0, 208.0)),
                RenderedPathCommand::line_to(PaintPoint::new(100.0, 208.0)),
                RenderedPathCommand::Close,
            ])
        );
    }

    #[test]
    fn nested_svg_image_resources_do_not_emit_substitute_paths() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><image href="https://example.com/nested.svg" width="20" height="10"/></svg>"#,
        )
        .expect("the safe resolver should leave the parent SVG parseable");

        assert!(
            asset
                .paint_paths(paint_rect(0.0, 0.0, 15.0, 7.5))
                .is_empty()
        );
    }

    #[test]
    fn preserves_svg_group_opacity_for_pdf_compositing() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><g opacity="0.5"><path d="M 0 0 H 20 V 10 H 0 Z" fill="red"/></g></svg>"#,
        );
        let asset = parse_inline_svg(&element).unwrap();
        let group = asset.paint_group(paint_rect(0.0, 0.0, 15.0, 7.5));

        let [SvgPaintItem::Group(child)] = group.items.as_slice() else {
            panic!("expected an isolated SVG opacity group");
        };
        assert!((child.opacity - 0.5).abs() < f32::EPSILON);
        assert!(matches!(child.items.as_slice(), [SvgPaintItem::Path(_)]));
    }

    #[test]
    fn propagates_svg_clip_paths_to_group_leaf_paths() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><defs><clipPath id="left"><rect width="5" height="10"/></clipPath></defs><g clip-path="url(#left)"><rect width="20" height="10" fill="red"/></g></svg>"#,
        );
        let asset = parse_inline_svg(&element).unwrap();
        let group = asset.paint_group(paint_rect(0.0, 0.0, 15.0, 7.5));

        let [SvgPaintItem::Group(child)] = group.items.as_slice() else {
            panic!("expected SVG group with a clip path");
        };
        let [SvgPaintItem::Path(path)] = child.items.as_slice() else {
            panic!("expected clipped path");
        };
        assert!(path.clip.is_some());
        assert!(path.clip.as_ref().unwrap().additional_clips.is_empty());
    }

    #[test]
    fn expands_svg_markers_into_supported_vector_paths() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><defs><marker id="arrow" markerWidth="4" markerHeight="4" refX="4" refY="2" orient="auto"><path d="M 0 0 L 4 2 L 0 4 Z" fill="red"/></marker></defs><path d="M 1 5 H 19" stroke="blue" marker-end="url(#arrow)"/></svg>"#,
        );
        let asset = parse_inline_svg(&element).unwrap();
        let paths = asset.paint_paths(paint_rect(0.0, 0.0, 15.0, 7.5));

        assert!(
            paths
                .iter()
                .any(|path| path.fill == Some(CssColor::new(0, 0, 255)))
        );
        assert!(
            paths
                .iter()
                .any(|path| path.fill == Some(CssColor::new(255, 0, 0)))
        );
    }

    #[test]
    fn context_fill_markers_share_the_owner_gradient_in_page_space() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 1600 900"><defs><linearGradient id="gradient"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><marker id="marker" refX="-10" refY="-10" markerWidth="600" markerHeight="400"><rect width="200" height="100" fill="context-fill"/><rect x="250" y="270" width="300" height="100" fill="context-fill"/></marker></defs><path fill="url(#gradient)" d="M 10 10 h 600 v 400 h -600 Z" marker-start="url(#marker)"/><g transform="translate(300 450) scale(.75 .5) rotate(60)"><path fill="url(#gradient)" d="M 10 10 h 600 v 400 h -600 Z" marker-start="url(#marker)"/></g><g transform="translate(600 450) scale(1.5 .5) rotate(-30)"><path fill="url(#gradient)" d="M 10 10 h 600 v 400 h -600 Z" marker-start="url(#marker)"/></g></svg>"#,
        )
        .unwrap();
        let paths = asset.paint_paths(paint_rect(0.0, 0.0, 225.0, 112.5));
        let gradients = paths
            .iter()
            .map(|path| {
                assert_eq!(path.transform, PaintTransform::identity());
                let Some(RenderedPathPaint::Gradient(gradient)) = path.fill_paint.as_ref() else {
                    panic!("expected context-fill to retain its gradient");
                };
                gradient.transform
            })
            .collect::<Vec<_>>();

        // Each marker rectangle is already completely covered by its owner
        // path with the same opaque context paint, so it is omitted as an
        // equivalence-preserving paint operation.
        assert_eq!(gradients.len(), 3);
        assert_ne!(gradients[0], gradients[1]);
        assert_ne!(gradients[1], gradients[2]);
    }

    #[test]
    fn context_stroke_marker_uses_an_outlined_page_space_stroke() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><defs><linearGradient id="gradient"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><marker id="marker" markerWidth="30" markerHeight="30" refX="15" refY="15"><rect x="5" y="5" width="20" height="20" fill="context-stroke"/></marker></defs><g transform="scale(1.5 .5) rotate(30)"><path d="M 20 20 H 100 V 60 H 20 Z" fill="none" stroke="url(#gradient)" stroke-width="12" marker-end="url(#marker)"/></g></svg>"#,
        )
        .unwrap();
        let paths = asset.paint_paths(paint_rect(0.0, 0.0, 150.0, 75.0));
        let gradients = paths
            .iter()
            .map(|path| {
                assert_eq!(path.transform, PaintTransform::identity());
                let Some(RenderedPathPaint::Gradient(gradient)) = path.fill_paint.as_ref() else {
                    panic!("expected outlined context-stroke gradient");
                };
                gradient.transform
            })
            .collect::<Vec<_>>();
        assert_eq!(gradients.len(), 2);
        assert_eq!(gradients[0], gradients[1]);
        assert!(paths.iter().all(|path| path.stroke.is_none()));
    }

    #[test]
    fn svg_stroke_outline_respects_paint_order() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect x="5" y="5" width="30" height="30" fill="red" stroke="blue" stroke-width="6" paint-order="stroke fill"/></svg>"#,
        )
        .unwrap();
        let paths = asset.paint_paths(paint_rect(0.0, 0.0, 30.0, 30.0));
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].fill, Some(CssColor::new(0, 0, 255)));
        assert_eq!(paths[1].fill, Some(CssColor::new(255, 0, 0)));
        assert!(paths.iter().all(|path| path.stroke.is_none()));
    }

    #[test]
    fn omits_masked_svg_subtrees_instead_of_painting_unmasked_children() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><mask id="fade"><rect width="20" height="10" fill="white"/></mask><g mask="url(#fade)"><rect width="20" height="10" fill="red"/></g></svg>"#,
        );
        let asset = parse_inline_svg(&element).unwrap();

        assert!(
            asset
                .paint_paths(paint_rect(0.0, 0.0, 15.0, 7.5))
                .is_empty()
        );
    }

    #[test]
    fn resource_cache_reuses_the_parsed_inline_asset() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><path d="M 0 0 H 1"/></svg>"#,
        );
        let cache = crate::resource::ResourceCache::default();
        let first = cache.inline_svg_asset(&element).unwrap();
        let second = cache.inline_svg_asset(&element).unwrap();
        assert!(Rc::ptr_eq(&first, &second));
    }
}
