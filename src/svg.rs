//! SVG parsing and the initial PDF vector adapter.
//!
//! SVG 2 defines an SVG element as a replaced element when embedded in HTML,
//! while SVG user units use CSS pixels at the default 96 DPI. The parser keeps
//! the normalized tree in SVG units; conversion to Quire paint points happens
//! only when a replaced SVG is painted.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use cssparser::{
    AtRuleParser, CowRcStr, Parser, ParserInput, ParserState, QualifiedRuleParser,
    StyleSheetParser, Token,
};

use crate::css::{
    self, BaselineMetric, ComputedStyle, CssColor, FontFamily, FontKerning, FontStyle,
    FontVariantCaps, FontVariationSetting, FontVariationSettings, FontWeight, FontWidth,
};
use crate::document::PaintStrokeWidth;
use crate::document::paint::effects::PaintBlendMode;
use crate::document::paint::geometry::{
    PaintClip, PaintPoint, PaintRect, PaintSize, PaintTransform, PaintTranslation,
};
use crate::document::paint::images::RenderedImage;
use crate::document::paint::paths::{
    RenderedGradient, RenderedGradientKind, RenderedGradientStop, RenderedPath, RenderedPathClip,
    RenderedPathClipPath, RenderedPathCommand, RenderedPathFillRule, RenderedPathLineCap,
    RenderedPathLineJoin, RenderedPathPaint, RenderedPathPaintOrder, RenderedPathStrokeStyle,
    RenderedSvgPathPattern, paint_rect_path_commands,
};
use crate::document::paint::patterns::RenderedImageSourceRect;
use crate::dom::{Element, ElementId, NodeKind};
use crate::resource::ExternalSvgUseResolver;
use crate::text::{FontSystem, TextShapingRequest};
use crate::units::{LayoutLength, LayoutSize, SemanticLengthExt, layout_pt};

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
const XLINK_NAMESPACE: &str = "http://www.w3.org/1999/xlink";
const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";
const SVG_IMAGE_ROOT_MARKER_ATTRIBUTE: &str = "data-quire-svg-root";
/// Private source marker retained by Quire's `usvg` adapter. It identifies
/// the host-CSS typography record for a normalized text span; it is never an
/// SVG author-visible identifier or a paint input.
const SVG_TEXT_TYPOGRAPHY_KEY_ATTRIBUTE: &str = "data-quire-text-typography-key";

/// Resource-processing policy for SVG image documents.
///
/// SVG images embedded by HTML/CSS are processed in SVG secure-static mode:
/// a self-contained `data:` payload is permitted, while resolving a string
/// URL would create an external fetch and is therefore rejected.  A future
/// static policy can use the same resolver boundary to consult an already
/// populated resource cache without letting `usvg` perform I/O.
/// <https://www.w3.org/TR/SVG2/conform.html#processing-modes>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SvgResourcePolicy {
    SecureStatic,
}

const MAX_SVG_DATA_IMAGE_COUNT: usize = 256;
const MAX_SVG_DATA_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_SVG_DATA_IMAGE_DEPTH: usize = 8;

#[derive(Default)]
struct SvgDataResourceBudget {
    count: usize,
    bytes: usize,
    svg_depth: usize,
}

/// An I/O-free adapter around `usvg`'s synchronous image callback.
struct SvgResourceResolver {
    policy: SvgResourcePolicy,
    budget: Arc<Mutex<SvgDataResourceBudget>>,
}

impl SvgResourceResolver {
    fn secure_static() -> Self {
        Self {
            policy: SvgResourcePolicy::SecureStatic,
            budget: Arc::new(Mutex::new(SvgDataResourceBudget::default())),
        }
    }

    fn image_href_resolver(self) -> usvg::ImageHrefResolver<'static> {
        let budget = self.budget;
        usvg::ImageHrefResolver {
            resolve_data: Box::new(move |mime, data, options| {
                let is_svg = mime == "image/svg+xml";
                let approved = matches!(
                    mime,
                    "image/jpg"
                        | "image/jpeg"
                        | "image/png"
                        | "image/gif"
                        | "image/webp"
                        | "image/svg+xml"
                );
                if !approved {
                    return None;
                }
                {
                    let mut budget = budget.lock().expect("SVG data resource budget lock");
                    if budget.count >= MAX_SVG_DATA_IMAGE_COUNT
                        || budget.bytes.saturating_add(data.len()) > MAX_SVG_DATA_IMAGE_BYTES
                        || (is_svg && budget.svg_depth >= MAX_SVG_DATA_IMAGE_DEPTH)
                    {
                        log::debug!("SVG secure-static data image limit exceeded");
                        return None;
                    }
                    budget.count += 1;
                    budget.bytes += data.len();
                    if is_svg {
                        budget.svg_depth += 1;
                    }
                }
                let result = usvg::ImageHrefResolver::default_data_resolver()(mime, data, options);
                if is_svg {
                    let mut budget = budget.lock().expect("SVG data resource budget lock");
                    budget.svg_depth = budget.svg_depth.saturating_sub(1);
                }
                result
            }),
            // Secure-static SVG images never turn an href into a filesystem
            // or network request.  This callback remains deliberately empty
            // even when an outer ResourceCache already holds the SVG bytes.
            resolve_string: Box::new(move |_, _| match self.policy {
                SvgResourcePolicy::SecureStatic => None,
            }),
        }
    }
}

/// CSS environment inherited by an SVG document embedded as an image.
///
/// Media Queries requires a secure animated SVG image to expose the used
/// color scheme of its embedding element, rather than the outer document's
/// raw user preference. Keeping this as an image-only context prevents a
/// parsed SVG variant for a light embedding from being reused in a dark one.
/// <https://www.w3.org/TR/mediaqueries-5/#prefers-color-scheme>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SvgImageContext {
    used_color_scheme: css::UsedColorScheme,
}

impl SvgImageContext {
    pub(crate) const fn from_used_color_scheme(used_color_scheme: css::UsedColorScheme) -> Self {
        Self { used_color_scheme }
    }

    fn media_environment(self) -> css::MediaEnvironment {
        let preference = match self.used_color_scheme {
            css::UsedColorScheme::Light => css::ColorSchemePreference::Light,
            css::UsedColorScheme::Dark => css::ColorSchemePreference::Dark,
        };
        css::MediaEnvironment::default().with_color_scheme_preference(preference)
    }
}

impl Default for SvgImageContext {
    fn default() -> Self {
        Self::from_used_color_scheme(css::UsedColorScheme::Light)
    }
}

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

/// One SVG element's local user coordinate system.
///
/// This is deliberately distinct from the root SVG source viewport and PDF
/// paint space. A CSS transform reference box and its matrix are evaluated in
/// this local coordinate system before the SVG scene maps them into the root
/// viewport:
/// <https://drafts.csswg.org/css-transforms/#transform-rendering>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SvgElementUserSpace {}

pub(crate) type SvgElementPoint = euclid::Point2D<f32, SvgElementUserSpace>;
pub(crate) type SvgElementSize = euclid::Size2D<f32, SvgElementUserSpace>;
pub(crate) type SvgElementRect = euclid::Rect<f32, SvgElementUserSpace>;
pub(crate) type SvgElementTransform =
    euclid::Transform2D<f32, SvgElementUserSpace, SvgElementUserSpace>;

/// An SVG text-positioning point in the element's current user coordinate
/// system. SVG's `x` and `y` attributes, and text-path samples, use this
/// top-left-origin space rather than PDF glyph coordinates.
/// <https://www.w3.org/TR/SVG2/text.html#TextLayoutAlgorithm>
type SvgTextPosition = SvgElementPoint;

/// An SVG text-positioning displacement in the element's current user
/// coordinate system. SVG's `dx` and `dy` lists must cross this explicit
/// boundary before becoming shaped-glyph offsets in `TextRunSpace`.
/// <https://www.w3.org/TR/SVG2/text.html#TextData>
type SvgTextUserDisplacement = euclid::Vector2D<f32, SvgElementUserSpace>;

/// The reference rectangles from which CSS `transform-box` selects one local
/// SVG coordinate system. All conversion into a selected reference rectangle
/// goes through [`Self::select`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct SvgTransformReferenceBoxes {
    fill: SvgElementRect,
    stroke: SvgElementRect,
    view: Option<SvgElementRect>,
}

/// The selected local rectangle for one CSS transform operation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SvgTransformReferenceBox(SvgElementRect);

/// A CSS transform origin after percentage resolution in local SVG units.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SvgTransformOrigin(SvgElementPoint);

impl SvgTransformReferenceBoxes {
    pub(crate) fn new(
        fill: SvgElementRect,
        stroke: SvgElementRect,
        view: Option<SvgElementRect>,
    ) -> Self {
        Self { fill, stroke, view }
    }

    /// CSS Transforms selects SVG fill-, stroke-, or view-box geometry here.
    /// `content-box` and `border-box` use the SVG fill box for graphical SVG
    /// elements:
    /// <https://drafts.csswg.org/css-transforms/#transform-box>.
    pub(crate) fn select(
        self,
        transform_box: css::TransformBox,
    ) -> Option<SvgTransformReferenceBox> {
        match transform_box {
            css::TransformBox::ContentBox
            | css::TransformBox::BorderBox
            | css::TransformBox::FillBox => Some(SvgTransformReferenceBox(self.fill)),
            css::TransformBox::StrokeBox => Some(SvgTransformReferenceBox(self.stroke)),
            css::TransformBox::ViewBox => self.view.map(SvgTransformReferenceBox),
        }
    }
}

impl SvgTransformReferenceBox {
    pub(crate) fn rect(self) -> SvgElementRect {
        self.0
    }

    pub(crate) fn origin(self, x: f32, y: f32) -> SvgTransformOrigin {
        SvgTransformOrigin(SvgElementPoint::new(self.0.min_x() + x, self.0.min_y() + y))
    }
}

impl SvgTransformOrigin {
    pub(crate) fn point(self) -> SvgElementPoint {
        self.0
    }
}
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

/// The geometric-mean scale used to shape SVG text before its residual affine
/// transform is retained in the PDF text matrix. Keeping it distinct from a
/// viewport or element scale prevents SVG user-coordinate lengths from being
/// passed to the font system unscaled.
#[derive(Debug, Clone, Copy)]
struct SvgFontScale(f32);

impl SvgFontScale {
    fn from_position_transform(transform: SvgTextUserToPaintTransform) -> Option<Self> {
        let transform = transform.0;
        let determinant = transform.a() * transform.d() - transform.b() * transform.c();
        let scale = determinant.abs().sqrt();
        (scale.is_finite() && scale > 0.0001).then_some(Self(scale))
    }

    fn points(self) -> f32 {
        self.0
    }

    fn scale_svg_length(self, length: f32) -> f32 {
        length * self.0
    }

    fn unscale_text_length(self, length: f32) -> f32 {
        length / self.0
    }
}

/// Maps SVG text positions in the current element user space to page paint
/// space. This is the same transform used for SVG geometry, so it retains the
/// root viewport's top-left-origin coordinate system and all authored SVG
/// transforms.
#[derive(Debug, Clone, Copy)]
struct SvgTextUserToPaintTransform(PaintTransform);

impl SvgTextUserToPaintTransform {
    fn from_usvg_transform(transform: usvg::Transform, viewport: ViewportTransform) -> Self {
        Self(svg_path_transform(transform, viewport))
    }

    fn map_position(self, position: SvgTextPosition) -> PaintPoint {
        self.0.apply_point(PaintPoint::new(position.x, position.y))
    }

    fn paint_transform(self) -> PaintTransform {
        self.0
    }

    /// Convert a shaped font's y-up glyph coordinates into SVG's y-down text
    /// coordinate convention before mapping them into the PDF page. SVG 2
    /// requires both a y-down viewport and upright ordinary text.
    /// <https://www.w3.org/TR/SVG2/coords.html#InitialCoordinateSystem>
    fn glyph_to_paint(self) -> SvgGlyphToPaintTransform {
        SvgGlyphToPaintTransform(PaintTransform::new(
            self.0.a(),
            self.0.b(),
            -self.0.c(),
            -self.0.d(),
            0.0,
            0.0,
        ))
    }
}

/// Maps shaped font glyph coordinates (`TextRunSpace`, whose y axis points
/// upward like PDF text space) to page paint space. It is deliberately not
/// interchangeable with [`SvgGeometryToPaintTransform`]: geometry consumes
/// SVG y-down coordinates, while glyphs first require the local reflection
/// that keeps normal SVG text upright.
#[derive(Debug, Clone, Copy)]
struct SvgGlyphToPaintTransform(PaintTransform);

impl SvgGlyphToPaintTransform {
    fn normalized_paint_transform(self, font_scale: SvgFontScale) -> PaintTransform {
        PaintTransform::new(
            self.0.a() / font_scale.points(),
            self.0.b() / font_scale.points(),
            self.0.c() / font_scale.points(),
            self.0.d() / font_scale.points(),
            0.0,
            0.0,
        )
    }

    fn text_matrix(
        self,
        font_scale: SvgFontScale,
    ) -> Option<crate::document::paint::text::RenderedTextMatrix> {
        let transform = self.normalized_paint_transform(font_scale);
        crate::document::paint::text::RenderedTextMatrix::from_pdf_linear_components([
            transform.a(),
            transform.b(),
            transform.c(),
            transform.d(),
        ])
    }

    fn map_text_run_point(
        self,
        font_scale: SvgFontScale,
        point: crate::document::paint::text::TextRunPoint,
    ) -> crate::document::paint::text::TextRunPoint {
        let mapped = self
            .normalized_paint_transform(font_scale)
            .apply_point(PaintPoint::new(point.x, point.y));
        crate::document::paint::text::TextRunPoint::new(mapped.x, mapped.y)
    }

    fn compose_text_matrix(
        self,
        font_scale: SvgFontScale,
        local: crate::document::paint::text::RenderedTextMatrix,
    ) -> crate::document::paint::text::RenderedTextMatrix {
        local.transformed_by(self.normalized_paint_transform(font_scale))
    }
}

/// The complete coordinate boundary for one normalized SVG text element.
///
/// SVG user-coordinate positions and shaped glyph coordinates intentionally
/// take different routes through this record. That makes it impossible for a
/// caller to accidentally install the root SVG y reflection as a PDF glyph
/// matrix.
#[derive(Debug, Clone, Copy)]
struct SvgTextCoordinateTransform {
    position_to_paint: SvgTextUserToPaintTransform,
    glyph_to_paint: SvgGlyphToPaintTransform,
    font_scale: SvgFontScale,
}

impl SvgTextCoordinateTransform {
    fn new(transform: usvg::Transform, viewport: ViewportTransform) -> Option<Self> {
        let position_to_paint =
            SvgTextUserToPaintTransform::from_usvg_transform(transform, viewport);
        let font_scale = SvgFontScale::from_position_transform(position_to_paint)?;
        let glyph_to_paint = position_to_paint.glyph_to_paint();
        Some(Self {
            position_to_paint,
            glyph_to_paint,
            font_scale,
        })
    }

    fn map_position(self, position: SvgTextPosition) -> PaintPoint {
        self.position_to_paint.map_position(position)
    }

    fn paint_transform(self) -> PaintTransform {
        self.position_to_paint.paint_transform()
    }

    fn font_scale(self) -> SvgFontScale {
        self.font_scale
    }

    fn text_matrix(self) -> Option<crate::document::paint::text::RenderedTextMatrix> {
        self.glyph_to_paint.text_matrix(self.font_scale)
    }

    /// SVG `dx`/`dy` values are y-down user-coordinate offsets. A shaped
    /// glyph run instead uses y-up text space, so only this conversion flips
    /// the local y component before the run's writing-mode matrix is applied.
    fn text_run_displacement(
        self,
        displacement: SvgTextUserDisplacement,
    ) -> crate::document::paint::text::TextRunDisplacement {
        crate::document::paint::text::TextRunDisplacement::new(
            self.font_scale.scale_svg_length(displacement.x),
            -self.font_scale.scale_svg_length(displacement.y),
        )
    }

    /// An SVG rotation is expressed in a y-down coordinate system. Conjugate
    /// it through the glyph-local y reflection before applying PDF's y-up
    /// text matrix.
    fn glyph_rotation_degrees(self, svg_degrees: f32) -> f32 {
        -svg_degrees
    }

    fn map_text_run_point(
        self,
        point: crate::document::paint::text::TextRunPoint,
    ) -> crate::document::paint::text::TextRunPoint {
        self.glyph_to_paint
            .map_text_run_point(self.font_scale, point)
    }

    fn compose_text_matrix(
        self,
        local: crate::document::paint::text::RenderedTextMatrix,
    ) -> crate::document::paint::text::RenderedTextMatrix {
        self.glyph_to_paint
            .compose_text_matrix(self.font_scale, local)
    }
}

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
    primary_clip_is_viewport: bool,
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
    fn materialize(mut self) -> Option<RenderedPath> {
        let commands = self
            .geometry
            .commands
            .into_iter()
            .map(|command| transform_path_command(command, self.geometry.to_paint.0))
            .collect::<Vec<_>>();
        self.paint.materialize_to_paint_space();
        let clip = remove_redundant_svg_clips(self.clip, &commands);
        let path = RenderedPath::new(
            commands,
            None,
            self.fill_rule,
            None,
            PaintStrokeWidth::ZERO,
            clip,
        )
        .with_paints(Some(self.paint.paint), None);
        if self.primary_clip_is_viewport {
            hard_crop_opaque_svg_rectangle(path)
        } else {
            Some(path)
        }
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
    pub(crate) transform: Option<SvgTransformOverride>,
    pub(crate) fill: Option<String>,
    pub(crate) stroke: Option<String>,
    pub(crate) stroke_width: Option<String>,
    /// Cascaded CSS text shadows serialized in SVG user units. This narrow
    /// bridge lets retained SVG text use the document CSS cascade even though
    /// `usvg` receives a standalone serialized subtree.
    pub(crate) text_shadow: Option<String>,
    pub(crate) font_family: Option<String>,
    pub(crate) font_size: Option<String>,
    pub(crate) font_weight: Option<String>,
    pub(crate) font_style: Option<String>,
    pub(crate) font_stretch: Option<String>,
    /// Cascaded `font-variation-settings` serialized as SVG/CSS axis pairs.
    /// The retained SVG adapter maps the normalized pairs directly to the
    /// shared document-font request.
    pub(crate) font_variation_settings: Option<String>,
    pub(crate) font_kerning: Option<String>,
    /// Resolved host-CSS spacing in SVG/CSS pixels. These are forwarded as
    /// used values so SVG and HTML submit the same shaping request.
    pub(crate) letter_spacing: Option<String>,
    pub(crate) word_spacing: Option<String>,
    pub(crate) writing_mode: Option<String>,
    pub(crate) text_orientation: Option<String>,
    pub(crate) direction: Option<String>,
    pub(crate) unicode_bidi: Option<String>,
    /// Key for the typed host-CSS typography side table. The serializer emits
    /// this as a private attribute and the retained-text parser copies it onto
    /// each normalized `TextSpan`.
    pub(crate) text_typography_key: Option<SvgTextTypographyKey>,
    pub(crate) flood_color: Option<SvgFilterColorOverride>,
    pub(crate) lighting_color: Option<SvgFilterColorOverride>,
    /// A forced solid color replaces this element's unsupported filter result.
    pub(crate) remove_filter: bool,
}

/// A concrete SVG parser color paired with the computed-value taint bit that
/// cannot be represented by `usvg`'s color parser.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SvgFilterColorOverride {
    color: CssColor,
    current_color_dependent: bool,
}

impl From<css::SvgFilterColor> for SvgFilterColorOverride {
    fn from(color: css::SvgFilterColor) -> Self {
        Self {
            color: color.color,
            current_color_dependent: color.current_color_dependent,
        }
    }
}

impl SvgPresentationOverride {
    /// Whether this bridge owns a declaration that would otherwise survive in
    /// an SVG element's inline `style` attribute with higher cascade
    /// specificity than the serialized presentation attribute.
    fn owns_style_property(&self, name: &str) -> bool {
        match name {
            "fill" => self.fill.is_some(),
            "stroke" => self.stroke.is_some(),
            "stroke-width" => self.stroke_width.is_some(),
            "text-shadow" => self.text_shadow.is_some(),
            "font-family" => self.font_family.is_some(),
            "font-size" => self.font_size.is_some(),
            "font-weight" => self.font_weight.is_some(),
            "font-style" => self.font_style.is_some(),
            "font-stretch" => self.font_stretch.is_some(),
            "font-variation-settings" => self.font_variation_settings.is_some(),
            "font-kerning" => self.font_kerning.is_some(),
            "letter-spacing" => self.letter_spacing.is_some(),
            "word-spacing" => self.word_spacing.is_some(),
            "writing-mode" => self.writing_mode.is_some(),
            "text-orientation" => self.text_orientation.is_some(),
            "direction" => self.direction.is_some(),
            "unicode-bidi" => self.unicode_bidi.is_some(),
            "flood-color" => self.flood_color.is_some(),
            "lighting-color" => self.lighting_color.is_some(),
            _ => false,
        }
    }
}

/// A transform selected by Quire's host-document cascade for an inline SVG
/// node. Scene-local values serialize into SVG user units; root SVG values
/// are applied only to the enclosing CSS layout box.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SvgTransformOverride {
    Scene(SvgUsedTransform),
    #[expect(
        dead_code,
        reason = "Root SVG transforms are consumed by the enclosing HTML box transform path."
    )]
    RootBox(SvgRootBoxTransform),
}

/// The complete static 2D transform result for one SVG scene node.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SvgUsedTransform {
    None,
    Affine(SvgElementTransform),
}

/// A root `<svg>` transform projected into Quire's page-paint coordinate
/// system. Keeping this distinct from [`SvgElementTransform`] prevents a CSS
/// box transform from being serialized into the SVG scene a second time.
#[derive(Debug, Clone, Copy)]
#[expect(
    dead_code,
    reason = "The root-box transform wrapper documents the scene/layout transform boundary."
)]
pub(crate) struct SvgRootBoxTransform(pub(crate) PaintTransform);

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

/// Opaque key joining a source inline-SVG element to the shaping style selected
/// by the host CSS cascade. The parser retains this only on normalized text
/// spans, so SVG geometry never observes a host-layout identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SvgTextTypographyKey(u64);

impl SvgTextTypographyKey {
    fn as_attribute_value(self) -> String {
        self.0.to_string()
    }

    fn from_usvg(value: u64) -> Self {
        Self(value)
    }
}

/// Font-system input selected by the host CSS cascade for an inline SVG text
/// content element. SVG geometry remains in `usvg`; this record contains only
/// values which affect Quire font selection, shaping, or glyph realization.
///
/// SVG 2 presentation attributes participate in the author cascade, while
/// inherited CSS typography crosses the inline-SVG resource boundary before
/// the shared document font system shapes the retained span:
/// <https://www.w3.org/TR/SVG2/styling.html#PresentationAttributes>
/// <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>
#[derive(Debug, Clone)]
pub(crate) struct SvgTextTypography {
    font_family: FontFamily,
    font_size_css_px: f32,
    font_size_adjust: css::FontSizeAdjust,
    font_weight: FontWeight,
    font_style: FontStyle,
    font_width: FontWidth,
    font_language_override: css::FontLanguageOverride,
    font_synthesis: css::FontSynthesis,
    font_feature_settings: css::FontFeatureSettings,
    font_variation_settings: FontVariationSettings,
    font_kerning: FontKerning,
    font_variant_ligatures: css::FontVariantLigatures,
    font_variant_position: css::FontVariantPosition,
    font_variant_caps: FontVariantCaps,
    font_variant_numeric: css::FontVariantNumeric,
    font_variant_alternates: css::FontVariantAlternates,
    font_variant_east_asian: css::FontVariantEastAsian,
    font_variant_emoji: css::FontVariantEmoji,
    font_palette: css::FontPalette,
    language: css::ContentLanguage,
    direction: css::Direction,
    unicode_bidi: css::UnicodeBidi,
    writing_mode: css::WritingMode,
    text_orientation: css::TextOrientation,
    letter_spacing_css_px: f32,
    word_spacing_css_px: f32,
    text_shadow: Vec<css::TextShadow>,
}

impl SvgTextTypography {
    pub(crate) fn from_computed_style(style: &ComputedStyle) -> Self {
        Self {
            font_family: style.font_family.clone(),
            font_size_css_px: style.font_size / css::CSS_PX_TO_PT,
            font_size_adjust: style.font_size_adjust,
            font_weight: style.font_weight,
            font_style: style.font_style,
            font_width: style.font_width,
            font_language_override: style.font_language_override,
            font_synthesis: style.font_synthesis,
            font_feature_settings: style.font_feature_settings.clone(),
            font_variation_settings: style.font_variation_settings.clone(),
            font_kerning: style.font_kerning,
            font_variant_ligatures: style.font_variant_ligatures,
            font_variant_position: style.font_variant_position,
            font_variant_caps: style.font_variant_caps,
            font_variant_numeric: style.font_variant_numeric.clone(),
            font_variant_alternates: style.font_variant_alternates.clone(),
            font_variant_east_asian: style.font_variant_east_asian.clone(),
            font_variant_emoji: style.font_variant_emoji,
            font_palette: style.font_palette.clone(),
            language: style.language.clone(),
            direction: style.direction,
            unicode_bidi: style.unicode_bidi,
            writing_mode: style.writing_mode,
            text_orientation: style.text_orientation,
            letter_spacing_css_px: style.used_letter_spacing().points() / css::CSS_PX_TO_PT,
            word_spacing_css_px: style.used_word_spacing().points() / css::CSS_PX_TO_PT,
            text_shadow: style.text_shadow.clone(),
        }
    }

    fn computed_style_at_font_scale(&self, font_scale: SvgFontScale) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.font_family = self.font_family.clone();
        style.font_size = self.font_size_css_px * font_scale.points();
        style.deferred_font_size = css::DeferredFontSize::Absolute(style.font_size);
        style.font_size_adjust = self.font_size_adjust;
        style.font_weight = self.font_weight;
        style.font_style = self.font_style;
        style.font_width = self.font_width;
        style.font_language_override = self.font_language_override;
        style.font_synthesis = self.font_synthesis;
        style.font_feature_settings = self.font_feature_settings.clone();
        style.font_variation_settings = self.font_variation_settings.clone();
        style.font_kerning = self.font_kerning;
        style.font_variant_ligatures = self.font_variant_ligatures;
        style.font_variant_position = self.font_variant_position;
        style.font_variant_caps = self.font_variant_caps;
        style.font_variant_numeric = self.font_variant_numeric.clone();
        style.font_variant_alternates = self.font_variant_alternates.clone();
        style.font_variant_east_asian = self.font_variant_east_asian.clone();
        style.font_variant_emoji = self.font_variant_emoji;
        style.font_palette = self.font_palette.clone();
        style.language = self.language.clone();
        style.direction = self.direction;
        style.unicode_bidi = self.unicode_bidi;
        style.writing_mode = self.writing_mode;
        style.text_orientation = self.text_orientation;
        style.letter_spacing = css::ComputedLengthPercentage::from_points(
            self.letter_spacing_css_px * font_scale.points(),
        );
        style.word_spacing = css::ComputedLengthPercentage::from_points(
            self.word_spacing_css_px * font_scale.points(),
        );
        style.text_shadow = self.text_shadow.clone();
        style.line_height = style.font_size * 1.2;
        style
    }
}

/// Host CSS values plus the serialized presentation overrides required by the
/// standalone SVG parser. The side table intentionally remains private to an
/// inline SVG asset and is absent for external SVG image documents.
#[derive(Debug, Clone, Default)]
pub(crate) struct SvgPresentationOverrides {
    presentation: HashMap<ElementId, SvgPresentationOverride>,
    typography: HashMap<SvgTextTypographyKey, SvgTextTypography>,
    next_typography_key: u64,
}

impl SvgPresentationOverrides {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert(&mut self, element: ElementId, override_values: SvgPresentationOverride) {
        self.presentation.insert(element, override_values);
    }

    pub(crate) fn get(&self, element: &ElementId) -> Option<&SvgPresentationOverride> {
        self.presentation.get(element)
    }

    pub(crate) fn record_typography(
        &mut self,
        typography: SvgTextTypography,
    ) -> SvgTextTypographyKey {
        let key = SvgTextTypographyKey(self.next_typography_key);
        self.next_typography_key += 1;
        self.typography.insert(key, typography);
        key
    }

    fn typography(&self) -> HashMap<SvgTextTypographyKey, SvgTextTypography> {
        self.typography.clone()
    }

    #[cfg(test)]
    pub(crate) fn typography_for_key(
        &self,
        key: SvgTextTypographyKey,
    ) -> Option<&SvgTextTypography> {
        self.typography.get(&key)
    }
}

/// A parsed inline SVG plus its intrinsic viewport size in Quire points.
#[derive(Debug, Clone)]
pub(crate) struct SvgAsset {
    tree: usvg::Tree,
    text_typography: HashMap<SvgTextTypographyKey, SvgTextTypography>,
    filter_taint: SvgFilterTaintCatalog,
    viewport_background: Option<SvgViewportBackground>,
    intrinsic_size: LayoutSize,
    intrinsic_dimensions: SvgIntrinsicDimensions,
    has_degenerate_view_box: bool,
    view_fragments: HashMap<String, SvgIntrinsicDimensions>,
    source: Rc<[u8]>,
}

/// Root-SVG background paint retained outside the SVG user-coordinate scene.
///
/// SVG backgrounds cover the root viewport. For an external SVG used as a CSS
/// image, that viewport is the concrete object rectangle, while descendants
/// remain mapped through the root `viewBox`.
/// <https://www.w3.org/TR/SVG2/struct.html#SVGElement>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SvgViewportBackground {
    pub(crate) color: CssColor,
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
    /// Return the explicit root background that must paint in viewport space.
    pub(crate) fn viewport_background(&self) -> Option<SvgViewportBackground> {
        self.viewport_background
    }

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

    /// Reparse this SVG for a concrete CSS image viewport.
    ///
    /// SVG percentage geometry and `preserveAspectRatio` are relative to the
    /// root SVG viewport. CSS image consumers supply that viewport from their
    /// concrete image size, independently of the SVG's intrinsic dimensions
    /// used earlier by the CSS sizing algorithm. In particular, border-image
    /// establishes this viewport before resolving its source slice offsets.
    /// <https://www.w3.org/TR/SVG2/coords.html#ViewportSpace>
    /// <https://www.w3.org/TR/css-images-3/#default-sizing>
    /// <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>
    pub(crate) fn with_css_image_viewport(&self, viewport: PaintSize) -> Self {
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
        let transform = ViewportTransform::new(destination, source, true, true);
        let mut group = collect_svg_group(
            self.tree.root(),
            transform,
            &[],
            usvg::Transform::default(),
            &self.filter_taint,
        );
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
    #[cfg(test)]
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

    /// Materialize an inline SVG through the owning document's font system.
    ///
    /// SVG's text layout rules remain SVG-specific, but glyph selection,
    /// shaping, document-font registration, and PDF subsetting are shared
    /// with HTML.  Keeping this mutable dependency at the paint boundary is
    /// what prevents an SVG image from silently creating a second font path.
    pub(crate) fn paint_inline_group_with_font_system(
        &self,
        destination: PaintRect,
        clip_viewport: bool,
        font_system: &mut FontSystem,
    ) -> SvgPaintGroup {
        self.paint_group_for_source_rect_with_font_system(
            destination,
            SvgSourceRect::new(SvgSourcePoint::new(0.0, 0.0), self.source_viewport_size()),
            clip_viewport,
            font_system,
        )
    }

    /// Font-aware counterpart of [`Self::paint_group_for_source_rect_with_viewport_clip`].
    /// It retains CSS object-fit/view-box source selection while placing SVG
    /// text through the document font registry.
    pub(crate) fn paint_group_for_source_rect_with_font_system(
        &self,
        destination: PaintRect,
        source: SvgSourceRect,
        clip_viewport: bool,
        font_system: &mut FontSystem,
    ) -> SvgPaintGroup {
        self.paint_group_for_source_rect_with_viewport_clip_and_font_system(
            destination,
            source,
            clip_viewport,
            Some(font_system),
        )
    }

    pub(crate) fn paint_group_for_source_rect(
        &self,
        destination: PaintRect,
        source: SvgSourceRect,
    ) -> SvgPaintGroup {
        self.paint_group_for_source_rect_with_viewport_clip(destination, source, true)
    }

    pub(crate) fn paint_group_for_source_rect_with_viewport_clip(
        &self,
        destination: PaintRect,
        source: SvgSourceRect,
        clip_viewport: bool,
    ) -> SvgPaintGroup {
        self.paint_group_for_source_rect_with_viewport_clip_and_font_system(
            destination,
            source,
            clip_viewport,
            None,
        )
    }

    fn paint_group_for_source_rect_with_viewport_clip_and_font_system(
        &self,
        destination: PaintRect,
        source: SvgSourceRect,
        clip_viewport: bool,
        font_system: Option<&mut FontSystem>,
    ) -> SvgPaintGroup {
        if destination.size.width <= 0.0
            || destination.size.height <= 0.0
            || source.size.width <= 0.0
            || source.size.height <= 0.0
        {
            return SvgPaintGroup::empty();
        }
        let viewport = ViewportTransform::new(destination, source, clip_viewport, false);
        let mut font_system = font_system;
        let mut group = collect_svg_group_with_font_system(
            self.tree.root(),
            viewport,
            &[],
            usvg::Transform::default(),
            &self.filter_taint,
            &self.text_typography,
            &mut font_system,
        );
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SvgPaintGroup {
    pub(crate) items: Vec<SvgPaintItem>,
    pub(crate) opacity: f32,
    pub(crate) blend_mode: PaintBlendMode,
    pub(crate) isolation: bool,
    pub(crate) bounds: Option<PaintClip>,
}

/// SVG text lowered to outlines from Quire's already-shaped glyph stream.
///
/// The PDF writer wraps all paths in one `/ActualText` marked-content span so
/// a complex SVG paint remains extractable without adding an invisible native
/// text duplicate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SvgOutlinedText {
    pub(crate) paths: Vec<RenderedPath>,
    pub(crate) actual_text: Rc<str>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SvgPaintItem {
    Path(Box<RenderedPath>),
    RasterImage(Box<RenderedImage>),
    /// SVG text shaped and font-selected by the owning Quire document.
    Text(Box<crate::document::paint::text::RenderedLine>),
    OutlinedText(Box<SvgOutlinedText>),
    /// A separately normalized SVG document retained as a compositing scene.
    /// Keeping this distinct from an ordinary SVG group preserves the image
    /// resource boundary for future Static-policy cache resolution.
    NestedSvg(Box<SvgPaintGroup>),
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
                SvgPaintItem::NestedSvg(group) => {
                    let nested = std::mem::replace(group, Box::new(SvgPaintGroup::empty()));
                    **group = nested.with_clip(clip.clone());
                }
                SvgPaintItem::RasterImage(image) => {
                    **image = image.as_ref().clone().with_intersected_clip(clip.clone());
                }
                // PDF text clipping is represented by the surrounding SVG
                // group. A future text-path/outline fallback may attach a
                // tighter clip to the individual item.
                SvgPaintItem::Text(_) => {}
                SvgPaintItem::OutlinedText(outlined) => {
                    for path in &mut outlined.paths {
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
                SvgPaintItem::Group(group) | SvgPaintItem::NestedSvg(group) => {
                    paths.extend(group.into_paths())
                }
                SvgPaintItem::RasterImage(_) => {}
                SvgPaintItem::Text(_) => {}
                SvgPaintItem::OutlinedText(outlined) => paths.extend(outlined.paths),
            }
        }
        paths
    }

    fn transformed(mut self, transform: PaintTransform) -> Self {
        for item in &mut self.items {
            match item {
                SvgPaintItem::Path(path) => **path = path.clone().transformed(transform),
                SvgPaintItem::RasterImage(image) => {
                    **image = image.clone().transformed(transform);
                }
                SvgPaintItem::Group(group) | SvgPaintItem::NestedSvg(group) => {
                    let nested = std::mem::replace(group, Box::new(SvgPaintGroup::empty()));
                    **group = nested.transformed(transform);
                }
                SvgPaintItem::Text(line) => {
                    **line = line.as_ref().clone().transformed(transform);
                }
                SvgPaintItem::OutlinedText(outlined) => {
                    for path in &mut outlined.paths {
                        *path = path.clone().transformed(transform);
                    }
                }
            }
        }
        self
    }

    pub(crate) fn raster_images<'a>(&'a self, images: &mut Vec<&'a RenderedImage>) {
        for item in &self.items {
            match item {
                SvgPaintItem::RasterImage(image) => images.push(image),
                SvgPaintItem::Group(group) | SvgPaintItem::NestedSvg(group) => {
                    group.raster_images(images)
                }
                SvgPaintItem::Path(_) => {}
                SvgPaintItem::Text(_) => {}
                SvgPaintItem::OutlinedText(_) => {}
            }
        }
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
            SvgPaintItem::NestedSvg(group) => canonicalize_svg_paint_group(group, servers),
            SvgPaintItem::RasterImage(_) => {}
            SvgPaintItem::Text(_) => {}
            SvgPaintItem::OutlinedText(outlined) => {
                for path in &mut outlined.paths {
                    for paint in [&mut path.fill_paint, &mut path.stroke_paint]
                        .into_iter()
                        .flatten()
                    {
                        canonicalize_svg_paint_server(paint, servers);
                    }
                }
            }
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
            SvgPaintItem::NestedSvg(child) => {
                // A nested SVG can contain arbitrary alpha-bearing content;
                // never carry opaque vector coverage across its boundary.
                let mut nested_coverage = Vec::new();
                elide_redundant_svg_paints_in_group(child, &mut nested_coverage);
                coverage.clear();
                if !child.items.is_empty() {
                    retained.push(item);
                }
            }
            SvgPaintItem::RasterImage(_) => {
                coverage.clear();
                retained.push(item);
            }
            SvgPaintItem::Text(_) => {
                // Text can have arbitrary alpha and glyph coverage. Never
                // propagate an opaque path proof across it.
                coverage.clear();
                retained.push(item);
            }
            SvgPaintItem::OutlinedText(_) => {
                coverage.clear();
                retained.push(item);
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
        SvgPaintItem::NestedSvg(_)
        | SvgPaintItem::RasterImage(_)
        | SvgPaintItem::Text(_)
        | SvgPaintItem::OutlinedText(_) => None,
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
            SvgPaintItem::NestedSvg(_)
            | SvgPaintItem::RasterImage(_)
            | SvgPaintItem::Text(_)
            | SvgPaintItem::OutlinedText(_) => {
                return None;
            }
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

/// Materialize a root-viewport crop directly into a hard-edged rectangle when
/// doing so is exactly equivalent to the SVG paint.
///
/// A solid axis-aligned rectangle has no interior geometry that clipping can
/// reveal or hide. Intersecting it with the sole rectangular viewport clip is
/// therefore equivalent to PDF clipping, while avoiding the device-pixel
/// antialias seam that a separately clipped SVG tile can create next to an
/// adjacent tile. Paths with an SVG clip path, stroke, transparency, pattern,
/// gradient, curve, or non-rectangular geometry retain the general PDF clip
/// path unchanged.
/// <https://www.w3.org/TR/SVG2/coords.html#ViewportSpace>
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>
fn hard_crop_opaque_svg_rectangle(path: RenderedPath) -> Option<RenderedPath> {
    let Some(clip) = path.clip.as_ref() else {
        return Some(path);
    };
    if clip.fill_rule != RenderedPathFillRule::NonZero || !clip.additional_clips.is_empty() {
        return Some(path);
    }
    let Some(clip_bounds) = axis_aligned_rectangle_bounds(&clip.commands) else {
        return Some(path);
    };
    let Some((color, bounds)) = opaque_axis_aligned_rectangle(&path) else {
        return Some(path);
    };
    let left = bounds.0.max(clip_bounds.0);
    let bottom = bounds.1.max(clip_bounds.1);
    let right = bounds.2.min(clip_bounds.2);
    let top = bounds.3.min(clip_bounds.3);
    if right <= left || top <= bottom {
        return None;
    }
    Some(
        RenderedPath::new(
            rectangular_path_commands(left, bottom, right, top),
            None,
            RenderedPathFillRule::NonZero,
            None,
            PaintStrokeWidth::ZERO,
            None,
        )
        .with_paints(Some(RenderedPathPaint::Solid(color)), None),
    )
}

fn rectangular_path_commands(
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
) -> Vec<RenderedPathCommand> {
    vec![
        RenderedPathCommand::move_to(PaintPoint::new(left, bottom)),
        RenderedPathCommand::line_to(PaintPoint::new(right, bottom)),
        RenderedPathCommand::line_to(PaintPoint::new(right, top)),
        RenderedPathCommand::line_to(PaintPoint::new(left, top)),
        RenderedPathCommand::Close,
    ]
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
    hard_crop_viewport: bool,
}

impl ViewportTransform {
    /// Resolve the SVG source viewport into a bottom-left paint rectangle.
    ///
    /// The negative y scale is the sole coordinate-system conversion between
    /// SVG's top-left source space and PDF paint space.
    fn new(
        destination: PaintRect,
        source: SvgSourceRect,
        clip_viewport: bool,
        hard_crop_viewport: bool,
    ) -> Self {
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
            hard_crop_viewport,
        }
    }
}

fn collect_svg_group(
    group: &usvg::Group,
    viewport: ViewportTransform,
    inherited_clips: &[RenderedPathClipPath],
    image_transform: usvg::Transform,
    filter_taint: &SvgFilterTaintCatalog,
) -> SvgPaintGroup {
    let mut font_system = None;
    let text_typography = HashMap::new();
    collect_svg_group_with_font_system(
        group,
        viewport,
        inherited_clips,
        image_transform,
        filter_taint,
        &text_typography,
        &mut font_system,
    )
}

/// Convert an SVG group while threading the document-scoped font system only
/// through the paint entry points that own one.
fn collect_svg_group_with_font_system(
    group: &usvg::Group,
    viewport: ViewportTransform,
    inherited_clips: &[RenderedPathClipPath],
    image_transform: usvg::Transform,
    filter_taint: &SvgFilterTaintCatalog,
    text_typography: &HashMap<SvgTextTypographyKey, SvgTextTypography>,
    font_system: &mut Option<&mut FontSystem>,
) -> SvgPaintGroup {
    let text_options = SvgTextCollectionOptions {
        typography: text_typography,
        force_outline_text: false,
    };
    collect_svg_group_with_options(
        group,
        viewport,
        inherited_clips,
        image_transform,
        filter_taint,
        text_options,
        font_system,
    )
}

/// The private text resources threaded through SVG scene recursion. Group
/// geometry and effects are independent of this state; keeping it together
/// prevents a nested SVG from accidentally pairing one asset's typography
/// table with a different document font system.
#[derive(Clone, Copy)]
struct SvgTextCollectionOptions<'typography> {
    typography: &'typography HashMap<SvgTextTypographyKey, SvgTextTypography>,
    force_outline_text: bool,
}

impl SvgTextCollectionOptions<'_> {
    fn with_forced_outlines(self, force_outline_text: bool) -> Self {
        Self {
            force_outline_text: self.force_outline_text || force_outline_text,
            ..self
        }
    }
}

/// Collect one normalized group, optionally forcing text into the exact glyph
/// outlines selected by the owning document. Effects use that form because a
/// filtered subtree is emitted as one raster image with `/ActualText`, not as
/// a visual image plus a duplicate invisible text layer.
fn collect_svg_group_with_options(
    group: &usvg::Group,
    viewport: ViewportTransform,
    inherited_clips: &[RenderedPathClipPath],
    image_transform: usvg::Transform,
    filter_taint: &SvgFilterTaintCatalog,
    text_options: SvgTextCollectionOptions<'_>,
    font_system: &mut Option<&mut FontSystem>,
) -> SvgPaintGroup {
    let mask = group.mask();
    let image_transform = image_transform.post_concat(group.transform());
    let raster_filter = svg_raster_filter_plan(group.filters());
    let filter_clip = if raster_filter.is_some() {
        None
    } else {
        match analyze_svg_filters(group.filters(), filter_taint) {
            SvgFilterAnalysis::ExactSourceGraphic { filter_clip } => filter_clip,
            SvgFilterAnalysis::RequiresRasterBackend => return SvgPaintGroup::empty(),
        }
    };
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
                let child = collect_svg_group_with_options(
                    child,
                    viewport,
                    &clips,
                    image_transform,
                    filter_taint,
                    text_options.with_forced_outlines(raster_filter.is_some() || mask.is_some()),
                    font_system,
                );
                if !child.items.is_empty() {
                    rendered.items.push(SvgPaintItem::Group(Box::new(child)));
                }
            }
            usvg::Node::Path(path) => {
                for path in render_path_with_clips(path, viewport, &clips) {
                    rendered.items.push(SvgPaintItem::Path(Box::new(path)));
                }
            }
            usvg::Node::Image(image) => {
                if let Some(item) =
                    render_svg_image(image, image_transform, viewport, &clips, font_system)
                {
                    rendered.items.push(item);
                }
            }
            usvg::Node::Text(text) => {
                if let Some(font_system) = font_system.as_deref_mut() {
                    rendered.items.extend(render_svg_text(
                        text,
                        viewport,
                        text_options.typography,
                        font_system,
                        text_options.force_outline_text
                            || raster_filter.is_some()
                            || mask.is_some(),
                    ));
                }
            }
        }
    }
    if let Some(mask) = mask {
        let mut no_mask_fonts = None;
        let mask_scene = collect_svg_group_with_options(
            mask.root(),
            viewport,
            &clips,
            image_transform,
            filter_taint,
            text_options.with_forced_outlines(true),
            &mut no_mask_fonts,
        );
        return rasterize_svg_masked_group(rendered, mask_scene, mask.kind());
    }
    if let Some(filter) = raster_filter {
        let filter_transform = svg_path_transform(image_transform, viewport);
        return rasterize_svg_filtered_group(rendered, filter, filter_transform);
    }
    if let Some(filter_clip) = filter_clip {
        rendered = rendered.with_clip(svg_filter_clip_path(
            filter_clip,
            svg_path_transform(image_transform, viewport),
        ));
    }
    rendered.recompute_bounds();
    rendered
}

/// Shape a normalized SVG text element through Quire's document font system.
///
/// `usvg` supplies SVG inheritance, chunks, paints, and element transforms,
/// but its laid-out glyphs are intentionally ignored. This keeps SVG and HTML
/// on the same font-selection, shaping, subsetting, and ToUnicode path.
fn render_svg_text(
    text: &usvg::Text,
    viewport: ViewportTransform,
    text_typography: &HashMap<SvgTextTypographyKey, SvgTextTypography>,
    font_system: &mut FontSystem,
    force_outline: bool,
) -> Vec<SvgPaintItem> {
    let Some(coordinates) = SvgTextCoordinateTransform::new(text.abs_transform(), viewport) else {
        log::debug!("skipping non-invertible SVG text transform");
        return Vec::new();
    };
    let Some(_) = coordinates.text_matrix() else {
        return Vec::new();
    };
    let mut items = Vec::new();
    let mut text_character_offset = 0;
    for chunk in text.chunks() {
        let text_flow = chunk.text_flow();
        let chunk_character_count = chunk.text().chars().count();
        let (Some(x), Some(y)) = (chunk.x(), chunk.y()) else {
            text_character_offset += chunk_character_count;
            continue;
        };
        let origin = coordinates.map_position(SvgTextPosition::new(x, y));
        let mut lines = Vec::new();
        let mut advance = 0.0;
        for span in chunk.spans() {
            if !span.is_visible() || span.start() >= span.end() {
                continue;
            }
            let Some(source) = chunk.text().get(span.start()..span.end()) else {
                continue;
            };
            let Some(paint) = svg_text_paint(
                span,
                coordinates.paint_transform(),
                coordinates.font_scale(),
            ) else {
                continue;
            };
            let decorations = svg_text_decorations(
                span,
                coordinates.paint_transform(),
                coordinates.font_scale(),
            );
            let style = span
                .text_typography_key()
                .map(SvgTextTypographyKey::from_usvg)
                .and_then(|key| text_typography.get(&key))
                .map(|typography| typography.computed_style_at_font_scale(coordinates.font_scale()))
                .unwrap_or_else(|| {
                    svg_text_style(
                        span,
                        text.writing_mode(),
                        text.text_orientation(),
                        text.direction(),
                        text.unicode_bidi(),
                        coordinates.font_scale().points(),
                    )
                });
            let line_height = font_system.used_line_height(&style).points();
            let Some(shaped) = font_system.shape_text_request(TextShapingRequest::new(
                source,
                &style,
                line_height,
            )) else {
                continue;
            };
            let mut runs = crate::layout::text_paint::positioned_rendered_runs_for_writing_mode(
                &shaped, &style,
            );
            let baseline_shift =
                svg_text_baseline_shift(font_system, span, &style, coordinates.font_scale());
            apply_svg_relative_positioning(
                &mut runs,
                source,
                text.dx(),
                text.dy(),
                text_character_offset + chunk.text()[..span.start()].chars().count(),
                coordinates,
            );
            let text_length = svg_text_length_adjustment(
                span,
                &mut runs,
                shaped.width,
                coordinates.font_scale(),
                &style,
            );
            let vertical_inline_axis =
                crate::layout::text_paint::VerticalInlineAxis::for_style(&style);
            for run in &mut runs {
                // `advance` is the already-resolved SVG pen position from
                // preceding spans.  `lengthAdjust="spacingAndGlyphs"` only
                // scales this span's own text space, not the preceding pen.
                if let Some(axis) = vertical_inline_axis {
                    run.y_offset += axis.advance_sign() * advance;
                } else {
                    run.x_offset += advance;
                }
                run.text_matrix = if vertical_inline_axis.is_some() && run.text_matrix.is_identity()
                {
                    // Upright vertical units carry their inline placement in
                    // `y_offset`, not in a rotated text matrix. Scale both
                    // the glyph's vertical geometry and its logical inline
                    // origin for SVG `lengthAdjust="spacingAndGlyphs"`.
                    run.y_offset *= text_length.inline_scale;
                    run.text_matrix
                        .scaled_block(text_length.inline_scale)
                        .expect("validated SVG vertical text-length scale")
                } else {
                    run.text_matrix
                        .scaled_inline(text_length.inline_scale)
                        .expect("validated SVG text-length inline scale")
                };
                // SVG's text position denotes the selected baseline. The
                // shared font system gives us the selected baseline-table
                // metric in the same paint units as shaping. It is a
                // block-axis adjustment, so it must not be scaled by
                // `lengthAdjust="spacingAndGlyphs"`.
                let Some(local_baseline_shift) = run
                    .text_matrix
                    .inverse_transform_local_displacement(baseline_shift)
                else {
                    continue;
                };
                run.x_offset += local_baseline_shift.x;
                run.y_offset += local_baseline_shift.y;
                run.text_matrix = coordinates.compose_text_matrix(run.text_matrix);
                let positioned_offset = run.text_matrix.transform_local_point(
                    crate::document::paint::text::TextRunPoint::new(run.x_offset, run.y_offset),
                );
                run.x_offset = positioned_offset.x;
                run.y_offset = positioned_offset.y;
            }
            let color = match &paint {
                SvgTextPaint::Native(color) => *color,
                SvgTextPaint::Outline(SvgOutlinedTextPaint {
                    fill: Some(RenderedPathPaint::Solid(color)),
                    ..
                }) => *color,
                SvgTextPaint::Outline(_) => CssColor::BLACK,
            };
            lines.push((
                crate::document::paint::text::RenderedLine::from_paint_origin(
                    source.to_owned(),
                    origin,
                    style.font_size,
                    shaped.first_font_id(),
                    color,
                    runs,
                ),
                paint,
                text_character_offset + chunk.text()[..span.start()].chars().count(),
                style,
                decorations,
                text_length.advance / text_length.inline_scale,
            ));
            advance += text_length.advance;
        }
        let anchor_offset = match chunk.anchor() {
            usvg::TextAnchor::Start => 0.0,
            usvg::TextAnchor::Middle => -advance * 0.5,
            usvg::TextAnchor::End => -advance,
        };
        let anchor_translation = if let Some((_, _, _, style, _, _)) = lines.first() {
            if let Some(axis) = crate::layout::text_paint::VerticalInlineAxis::for_style(style) {
                coordinates.map_text_run_point(crate::document::paint::text::TextRunPoint::new(
                    0.0,
                    axis.advance_sign() * anchor_offset,
                ))
            } else {
                coordinates.map_text_run_point(crate::document::paint::text::TextRunPoint::new(
                    anchor_offset,
                    0.0,
                ))
            }
        } else {
            coordinates.map_text_run_point(crate::document::paint::text::TextRunPoint::new(
                anchor_offset,
                0.0,
            ))
        };
        for (mut line, paint, source_character_offset, style, decorations, local_advance) in lines {
            if let usvg::TextFlow::Path(path) = &text_flow {
                let paths = svg_text_path_outline_paths(
                    font_system,
                    &line,
                    path,
                    coordinates,
                    x + path.start_offset()
                        + coordinates.font_scale().unscale_text_length(anchor_offset),
                    &paint,
                );
                if !paths.is_empty() {
                    items.push(SvgPaintItem::OutlinedText(Box::new(SvgOutlinedText {
                        paths,
                        actual_text: Rc::from(line.text),
                    })));
                }
                continue;
            }
            for run in &mut line.runs {
                run.x_offset += anchor_translation.x;
                run.y_offset += anchor_translation.y;
            }
            let rotate = svg_text_rotation_paths(
                font_system,
                &line,
                line.text.as_ref(),
                text.rotate(),
                source_character_offset,
                coordinates,
                &paint,
            );
            let has_rotation = rotate.is_some();
            let after_text_decorations = if has_rotation {
                Vec::new()
            } else {
                items.extend(svg_text_decoration_paths(
                    font_system,
                    &line,
                    &style,
                    local_advance,
                    &decorations,
                    SvgTextDecorationPhase::BeforeText,
                ));
                items.extend(svg_text_shadow_paths(font_system, &line, &style, &paint));
                svg_text_decoration_paths(
                    font_system,
                    &line,
                    &style,
                    local_advance,
                    &decorations,
                    SvgTextDecorationPhase::AfterText,
                )
            };
            let forced_outline_paint = force_outline.then(|| paint.outline_paint());
            match (paint, rotate) {
                (_, Some(paths)) if !paths.is_empty() => {
                    items.push(SvgPaintItem::OutlinedText(Box::new(SvgOutlinedText {
                        paths,
                        actual_text: Rc::from(line.text),
                    })));
                }
                (SvgTextPaint::Native(_), _) if !force_outline => {
                    items.push(SvgPaintItem::Text(Box::new(line)))
                }
                (SvgTextPaint::Native(_), _) => {
                    let paths = svg_text_outline_paths(
                        font_system,
                        line.origin(),
                        &line.runs,
                        forced_outline_paint
                            .as_ref()
                            .expect("forced SVG effect text has outline paint"),
                    );
                    if !paths.is_empty() {
                        items.push(SvgPaintItem::OutlinedText(Box::new(SvgOutlinedText {
                            paths,
                            actual_text: Rc::from(line.text),
                        })));
                    }
                }
                (SvgTextPaint::Outline(paint), _) => {
                    let paths =
                        svg_text_outline_paths(font_system, line.origin(), &line.runs, &paint);
                    if !paths.is_empty() {
                        items.push(SvgPaintItem::OutlinedText(Box::new(SvgOutlinedText {
                            paths,
                            actual_text: Rc::from(line.text),
                        })));
                    }
                }
            }
            items.extend(after_text_decorations);
        }
        text_character_offset += chunk_character_count;
    }
    items
}

/// SVG decorations retain their own fill/stroke styles, independently from
/// the decorated glyphs. Keeping them as path paint avoids another text run
/// and lets gradients/strokes follow the same SVG paint-server lowering as
/// complex text.
#[derive(Debug, Clone, Default)]
struct SvgTextDecorations {
    underline: Option<SvgTextPaint>,
    overline: Option<SvgTextPaint>,
    line_through: Option<SvgTextPaint>,
}

#[derive(Clone, Copy)]
enum SvgTextDecorationPhase {
    BeforeText,
    AfterText,
}

fn svg_text_decorations(
    span: &usvg::TextSpan,
    transform: PaintTransform,
    font_scale: SvgFontScale,
) -> SvgTextDecorations {
    let paint = |decoration: Option<&usvg::TextDecorationStyle>| {
        decoration.and_then(|decoration| {
            svg_text_paint_from_sources(
                decoration.fill(),
                decoration.stroke(),
                usvg::PaintOrder::FillAndStroke,
                transform,
                font_scale,
            )
        })
    };
    SvgTextDecorations {
        underline: paint(span.decoration().underline()),
        overline: paint(span.decoration().overline()),
        line_through: paint(span.decoration().line_through()),
    }
}

/// Realize SVG text decorations after text shaping, so their lengths,
/// transforms, and font metrics all match the chosen document font. Upright
/// vertical text uses the shared vertical inline axis: its decorations extend
/// along the SVG user-space Y axis rather than accidentally inheriting the
/// horizontal glyph rectangle. SVG's underline/overline paint before glyph
/// ink; the line-through paints afterward.
/// <https://www.w3.org/TR/SVG2/painting.html#TextDecorationProperties>
fn svg_text_decoration_paths(
    font_system: &mut FontSystem,
    line: &crate::document::paint::text::RenderedLine,
    style: &ComputedStyle,
    local_advance: f32,
    decorations: &SvgTextDecorations,
    phase: SvgTextDecorationPhase,
) -> Vec<SvgPaintItem> {
    if !local_advance.is_finite() || local_advance <= 0.0 {
        return Vec::new();
    }
    let Some(run) = line.runs.first() else {
        return Vec::new();
    };
    let metrics = font_system.text_decoration_metrics(run.font_id, style);
    let ascent = font_system
        .baseline_offset_for_style(style, BaselineMetric::Alphabetic)
        .points()
        - font_system
            .baseline_offset_for_style(style, BaselineMetric::TextTop)
            .points();
    let entries = match phase {
        SvgTextDecorationPhase::BeforeText => [
            (
                decorations.underline.as_ref(),
                metrics.underline_position,
                metrics.underline_thickness,
            ),
            (
                decorations.overline.as_ref(),
                ascent - metrics.underline_thickness * 0.5,
                metrics.underline_thickness,
            ),
            (None, 0.0, 0.0),
        ],
        SvgTextDecorationPhase::AfterText => [
            (
                decorations.line_through.as_ref(),
                metrics.strikeout_position,
                metrics.strikeout_thickness,
            ),
            (None, 0.0, 0.0),
            (None, 0.0, 0.0),
        ],
    };
    let [a, b, c, d] = run.text_matrix.pdf_components();
    let transform = PaintTransform::new(
        a,
        b,
        c,
        d,
        line.origin().x + run.x_offset,
        line.origin().y + run.y_offset,
    );
    let upright_vertical = crate::layout::text_paint::VerticalInlineAxis::for_style(style)
        .is_some()
        && style.text_orientation == css::TextOrientation::Upright;
    entries
        .into_iter()
        .filter_map(|(paint, center, thickness)| {
            let paint = paint?.outline_paint();
            if !center.is_finite() || !thickness.is_finite() || thickness <= 0.0 {
                return None;
            }
            let rect = if upright_vertical {
                // The horizontal font underline metric becomes a block-axis
                // offset in upright vertical flow; the decoration's extent
                // follows the logical inline (SVG Y) axis.
                PaintRect::new(
                    PaintPoint::new(center - thickness * 0.5, 0.0),
                    PaintSize::new(thickness, local_advance),
                )
            } else {
                PaintRect::new(
                    PaintPoint::new(0.0, center - thickness * 0.5),
                    PaintSize::new(local_advance, thickness),
                )
            };
            paint.fill.as_ref().or(paint.stroke.as_ref())?;
            Some(
                RenderedPath::new(
                    paint_rect_path_commands(rect),
                    None,
                    RenderedPathFillRule::NonZero,
                    None,
                    paint.stroke_width,
                    None,
                )
                .with_paints(paint.fill, paint.stroke)
                .with_stroke_style(paint.stroke_style)
                .with_paint_order(paint.paint_order)
                .with_transform(transform),
            )
        })
        .map(|path| SvgPaintItem::Path(Box::new(path)))
        .collect()
}

/// Paint SVG `text-shadow` using the existing CSS shadow sampling policy, but
/// lower every replay to the already-shaped glyph outlines. A shadow must not
/// introduce another selectable PDF text run: the source text item alone owns
/// extraction, while the decorative shadow stays ordinary vector paint.
/// <https://www.w3.org/TR/css-text-decor-4/#text-shadow-property>
fn svg_text_shadow_paths(
    font_system: &FontSystem,
    line: &crate::document::paint::text::RenderedLine,
    style: &ComputedStyle,
    paint: &SvgTextPaint,
) -> Vec<SvgPaintItem> {
    let Some(reference_run) = line.runs.first() else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for shadow in style.text_shadow.iter().rev() {
        let color = shadow.color.resolve(style.color);
        if shadow.inset || !color.is_visible() {
            continue;
        }
        // PDF has no portable text-shadow/blur operator.  Keep a zero-blur
        // shadow as vector ink, but rasterize a blurred replay of the same
        // Quire-shaped outlines.  In particular, do not emit a second PDF
        // text run here: the unshadowed source below remains the one
        // selectable/tagged representation of the SVG character data.
        if shadow.blur_radius.length_points() > 0.0 {
            let local_offset = reference_run.text_matrix.transform_local_point(
                crate::document::paint::text::TextRunPoint::new(
                    shadow.offset_x.length_points(),
                    -shadow.offset_y.length_points(),
                ),
            );
            let mut shadow_line = line.clone();
            shadow_line.translate_origin(PaintTranslation::new(local_offset.x, local_offset.y));
            let mut shadow_paint = paint.outline_paint();
            if shadow_paint.fill.is_some() {
                shadow_paint.fill = Some(RenderedPathPaint::Solid(color));
            }
            if shadow_paint.stroke.is_some() {
                shadow_paint.stroke = Some(RenderedPathPaint::Solid(color));
            }
            let paths = svg_text_outline_paths(
                font_system,
                shadow_line.origin(),
                &shadow_line.runs,
                &shadow_paint,
            );
            if let Some(image) = rasterize_svg_solid_paths(
                &paths,
                // Match the established CSS shadow replay footprint: its
                // outer samples reach 0.45 radii, while three Gaussian
                // standard deviations reach the same visual extent. SVG
                // filter `stdDeviation` remains a true standard deviation
                // at the common rasterizer boundary below.
                shadow.blur_radius.length_max_zero().points() * 0.15,
            ) {
                items.push(SvgPaintItem::RasterImage(Box::new(image)));
            }
            continue;
        }
        for pass in crate::layout::text_paint::text_shadow_paint_passes(shadow.clone(), color) {
            let local_offset = reference_run.text_matrix.transform_local_point(
                crate::document::paint::text::TextRunPoint::new(
                    shadow.offset_x.length_points() + pass.offset.x,
                    -shadow.offset_y.length_points() + pass.offset.y,
                ),
            );
            let mut shadow_line = line.clone();
            shadow_line.translate_origin(PaintTranslation::new(local_offset.x, local_offset.y));
            let mut shadow_paint = paint.outline_paint();
            if shadow_paint.fill.is_some() {
                shadow_paint.fill = Some(RenderedPathPaint::Solid(pass.color));
            }
            if shadow_paint.stroke.is_some() {
                shadow_paint.stroke = Some(RenderedPathPaint::Solid(pass.color));
            }
            let paths = svg_text_outline_paths(
                font_system,
                shadow_line.origin(),
                &shadow_line.runs,
                &shadow_paint,
            );
            items.extend(
                paths
                    .into_iter()
                    .map(|path| SvgPaintItem::Path(Box::new(path))),
            );
        }
    }
    items
}

/// Maximum pixels allocated for one SVG paint effect surface.
///
/// Effects are intentionally a bounded fallback.  A pathological filter or
/// blur must not turn a small SVG source into an unbounded PDF-generation
/// allocation.  The eventual filter compositor shares this limit.
const MAX_SVG_EFFECT_PIXELS: u64 = 16 * 1024 * 1024;
/// Upper bounds work for one `feConvolveMatrix` primitive after the bounded
/// surface allocation check above. A kernel is authored data, so its cost is
/// not implied by the SVG viewport alone.
const MAX_SVG_CONVOLVE_SAMPLES: u64 = 128 * 1024 * 1024;
/// The retained filter graph may hold the two standard inputs plus this many
/// authored `result` surfaces. Refuse graphs that exceed the bound instead of
/// allowing SVG result names to allocate unbounded RGBA buffers.
const MAX_SVG_NAMED_EFFECT_SURFACES: usize = 8;
const SVG_EFFECT_RASTER_SCALE: f32 = 2.0;

/// Rasterize solid SVG paths into an sRGB image, applying a separable Gaussian
/// blur in premultiplied-alpha space when requested.
///
/// This is the common source-ink boundary for SVG effects.  It deliberately
/// accepts retained Quire paths instead of SVG text: callers shape once with
/// the document [`FontSystem`], convert the selected glyph IDs to outlines,
/// and this compositor never consults a font database or reshapes Unicode.
/// Gradients/patterns are left for the general paint-server compositor rather
/// than being silently approximated as solid colors.
fn rasterize_svg_solid_paths(paths: &[RenderedPath], blur_radius: f32) -> Option<RenderedImage> {
    rasterize_svg_solid_paths_with_effect(paths, blur_radius, &[])
}

fn rasterize_svg_solid_paths_with_effect(
    paths: &[RenderedPath],
    blur_radius: f32,
    pixel_effects: &[SvgRasterPixelEffect],
) -> Option<RenderedImage> {
    let mut bounds = svg_paths_bounds(paths)?;
    let blur_radius = blur_radius.max(0.0);
    let chained_blur_radius = pixel_effects
        .iter()
        .filter_map(|effect| match effect {
            SvgRasterPixelEffect::GaussianBlur { std_deviation } => Some(*std_deviation),
            SvgRasterPixelEffect::DropShadow { std_deviation, .. } => Some(*std_deviation),
            SvgRasterPixelEffect::Offset { .. }
            | SvgRasterPixelEffect::FloodInSourceAlpha { .. }
            | SvgRasterPixelEffect::ColorMatrix { .. }
            | SvgRasterPixelEffect::ComponentTransfer { .. }
            | SvgRasterPixelEffect::Morphology { .. }
            | SvgRasterPixelEffect::ConvolveMatrix { .. }
            | SvgRasterPixelEffect::CompositeWithSourceGraphic { .. }
            | SvgRasterPixelEffect::CompositeWithSourceAlpha { .. } => None,
        })
        .sum::<f32>()
        .max(0.0);
    let chained_offset_padding = pixel_effects
        .iter()
        .filter_map(|effect| match effect {
            SvgRasterPixelEffect::Offset { dx, dy } => Some(dx.abs().max(dy.abs())),
            SvgRasterPixelEffect::GaussianBlur { .. }
            | SvgRasterPixelEffect::DropShadow { .. }
            | SvgRasterPixelEffect::FloodInSourceAlpha { .. }
            | SvgRasterPixelEffect::ColorMatrix { .. }
            | SvgRasterPixelEffect::ComponentTransfer { .. }
            | SvgRasterPixelEffect::Morphology { .. }
            | SvgRasterPixelEffect::ConvolveMatrix { .. }
            | SvgRasterPixelEffect::CompositeWithSourceGraphic { .. }
            | SvgRasterPixelEffect::CompositeWithSourceAlpha { .. } => None,
        })
        .sum::<f32>();
    let max_stroke = paths
        .iter()
        .map(|path| path.stroke_width.points())
        .filter(|width| width.is_finite())
        .fold(0.0_f32, f32::max);
    let padding =
        max_stroke * 0.5 + (blur_radius + chained_blur_radius) * 3.0 + chained_offset_padding + 1.0;
    bounds = PaintRect::new(
        PaintPoint::new(bounds.origin.x - padding, bounds.origin.y - padding),
        PaintSize::new(
            bounds.size.width + padding * 2.0,
            bounds.size.height + padding * 2.0,
        ),
    );
    if !bounds.size.width.is_finite()
        || !bounds.size.height.is_finite()
        || bounds.size.width <= 0.0
        || bounds.size.height <= 0.0
    {
        return None;
    }
    let width = (bounds.size.width * SVG_EFFECT_RASTER_SCALE).ceil() as u64;
    let height = (bounds.size.height * SVG_EFFECT_RASTER_SCALE).ceil() as u64;
    if width == 0
        || height == 0
        || width > u32::MAX as u64
        || height > u32::MAX as u64
        || width.saturating_mul(height) > MAX_SVG_EFFECT_PIXELS
    {
        log::warn!(
            "skipping SVG effect surface of {}x{} pixels; limit is {} pixels",
            width,
            height,
            MAX_SVG_EFFECT_PIXELS
        );
        return None;
    }
    let mut pixmap =
        rasterize_svg_paths_to_effect_pixmap(paths, bounds, width as u32, height as u32)?;
    // Keep SourceGraphic available for binary named-input primitives. The
    // working pixmap below becomes each primitive's result; this immutable
    // copy is never reshaped or repainted.
    let source_graphic = pixmap.data().to_vec();
    let source_alpha = svg_source_alpha_surface(&source_graphic);
    for pixel_effect in pixel_effects {
        match pixel_effect {
            SvgRasterPixelEffect::GaussianBlur { std_deviation } => gaussian_blur_rgba(
                pixmap.data_mut(),
                width as usize,
                height as usize,
                *std_deviation * SVG_EFFECT_RASTER_SCALE,
            ),
            SvgRasterPixelEffect::DropShadow {
                std_deviation,
                dx,
                dy,
                color,
            } => apply_svg_drop_shadow(
                pixmap.data_mut(),
                width as usize,
                height as usize,
                *std_deviation * SVG_EFFECT_RASTER_SCALE,
                (*dx * SVG_EFFECT_RASTER_SCALE).round() as i32,
                (-*dy * SVG_EFFECT_RASTER_SCALE).round() as i32,
                *color,
            ),
            SvgRasterPixelEffect::Offset { dx, dy } => apply_svg_offset(
                pixmap.data_mut(),
                width as usize,
                height as usize,
                (*dx * SVG_EFFECT_RASTER_SCALE).round() as i32,
                (-*dy * SVG_EFFECT_RASTER_SCALE).round() as i32,
            ),
            SvgRasterPixelEffect::FloodInSourceAlpha { color } => {
                apply_svg_flood_in_source_alpha(pixmap.data_mut(), *color)
            }
            SvgRasterPixelEffect::ColorMatrix { matrix, linear_rgb } => {
                apply_svg_color_matrix(pixmap.data_mut(), *matrix, *linear_rgb);
            }
            SvgRasterPixelEffect::ComponentTransfer {
                functions,
                linear_rgb,
            } => apply_svg_component_transfer(pixmap.data_mut(), functions, *linear_rgb),
            SvgRasterPixelEffect::Morphology {
                radius_x,
                radius_y,
                dilate,
            } => {
                let radius_x = (*radius_x * SVG_EFFECT_RASTER_SCALE).round();
                let radius_y = (*radius_y * SVG_EFFECT_RASTER_SCALE).round();
                if !(0.0..=256.0).contains(&radius_x) || !(0.0..=256.0).contains(&radius_y) {
                    log::warn!("skipping SVG morphology with an effect radius over 256 pixels");
                    return None;
                }
                apply_svg_morphology(
                    pixmap.data_mut(),
                    width as usize,
                    height as usize,
                    radius_x as usize,
                    radius_y as usize,
                    *dilate,
                )
            }
            SvgRasterPixelEffect::ConvolveMatrix {
                matrix,
                columns,
                rows,
                target_x,
                target_y,
                divisor,
                bias,
                edge_mode,
                preserve_alpha,
                linear_rgb,
            } => {
                if !apply_svg_convolve_matrix(
                    pixmap.data_mut(),
                    width as usize,
                    height as usize,
                    matrix,
                    *columns,
                    *rows,
                    *target_x,
                    *target_y,
                    *divisor,
                    *bias,
                    *edge_mode,
                    *preserve_alpha,
                    *linear_rgb,
                ) {
                    log::warn!("skipping SVG feConvolveMatrix exceeding compositor limits");
                    return None;
                }
            }
            SvgRasterPixelEffect::CompositeWithSourceGraphic {
                operator,
                source_as_second,
            } => {
                let current = pixmap.data().to_vec();
                let composited = if *source_as_second {
                    apply_svg_composite(&current, &source_graphic, pixmap.data_mut(), *operator)
                } else {
                    apply_svg_composite(&source_graphic, &current, pixmap.data_mut(), *operator)
                };
                debug_assert!(composited, "same-sized SVG graph surfaces composite");
                if !composited {
                    return None;
                }
            }
            SvgRasterPixelEffect::CompositeWithSourceAlpha {
                operator,
                source_as_second,
            } => {
                let current = pixmap.data().to_vec();
                let composited = if *source_as_second {
                    apply_svg_composite(&current, &source_alpha, pixmap.data_mut(), *operator)
                } else {
                    apply_svg_composite(&source_alpha, &current, pixmap.data_mut(), *operator)
                };
                debug_assert!(composited, "same-sized SVG graph surfaces composite");
                if !composited {
                    return None;
                }
            }
        }
    }
    if blur_radius > 0.0 {
        // This receives an SVG-filter standard deviation in paint units.
        // CSS text-shadow converts its implementation-defined blur radius at
        // the caller, keeping CSS blur behavior out of SVG filter math.
        gaussian_blur_rgba(
            pixmap.data_mut(),
            width as usize,
            height as usize,
            blur_radius * SVG_EFFECT_RASTER_SCALE,
        );
    }
    svg_effect_pixmap_to_rendered_image(bounds, pixmap)
}

/// Encode one completed bounded SVG effect surface as the existing PDF image
/// representation. Keeping this conversion separate from path rasterization
/// is the graph-compositor boundary: named filter intermediates remain
/// premultiplied tiny-skia surfaces until the final result alone is encoded.
fn svg_effect_pixmap_to_rendered_image(
    bounds: PaintRect,
    pixmap: tiny_skia::Pixmap,
) -> Option<RenderedImage> {
    let width = pixmap.width();
    let height = pixmap.height();
    let rgba = pixmap.take_demultiplied();
    let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
    let mut alpha = Vec::with_capacity((width as usize) * (height as usize));
    let (pixels, trailing) = rgba.as_chunks::<4>();
    debug_assert!(trailing.is_empty(), "RGBA pixmap has whole pixels");
    for &[red, green, blue, opacity] in pixels {
        rgb.extend_from_slice(&[red, green, blue]);
        alpha.push(opacity);
    }
    Some(RenderedImage::from_paint_rect(
        bounds,
        false,
        width,
        height,
        None,
        true,
        Rc::from(rgb),
        Some(Rc::from(alpha)),
        None,
    ))
}

/// Materialize retained solid SVG paths into one bounded premultiplied effect
/// surface. The graph executor consumes this same source surface for
/// `SourceGraphic`, derives `SourceAlpha` from it, and retains named results
/// as surfaces until [`svg_effect_pixmap_to_rendered_image`] is called.
fn rasterize_svg_paths_to_effect_pixmap(
    paths: &[RenderedPath],
    bounds: PaintRect,
    width: u32,
    height: u32,
) -> Option<tiny_skia::Pixmap> {
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    for path in paths {
        rasterize_svg_path(&mut pixmap, path, bounds)?;
    }
    Some(pixmap)
}

/// Derive the SVG standard input `SourceAlpha` from premultiplied
/// `SourceGraphic`. RGB is transparent black while alpha is retained exactly.
fn svg_source_alpha_surface(source_graphic: &[u8]) -> Vec<u8> {
    let mut alpha = vec![0; source_graphic.len()];
    for (source, alpha) in source_graphic
        .as_chunks::<4>()
        .0
        .iter()
        .zip(alpha.as_chunks_mut::<4>().0)
    {
        alpha[3] = source[3];
    }
    alpha
}

fn svg_paths_bounds(paths: &[RenderedPath]) -> Option<PaintRect> {
    let mut left = f32::INFINITY;
    let mut bottom = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let mut top = f32::NEG_INFINITY;
    for path in paths {
        let bounds = path.bounds()?;
        left = left.min(bounds.origin.x);
        bottom = bottom.min(bounds.origin.y);
        right = right.max(bounds.max_x());
        top = top.max(bounds.max_y());
    }
    (left.is_finite() && bottom.is_finite() && right.is_finite() && top.is_finite()).then(|| {
        PaintRect::new(
            PaintPoint::new(left, bottom),
            PaintSize::new((right - left).max(0.0), (top - bottom).max(0.0)),
        )
    })
}

fn rasterize_svg_path(
    pixmap: &mut tiny_skia::Pixmap,
    source: &RenderedPath,
    bounds: PaintRect,
) -> Option<()> {
    let path = tiny_skia_path_from_rendered(source)?;
    let transform = source.transform;
    let matrix = tiny_skia::Transform::from_row(
        SVG_EFFECT_RASTER_SCALE * transform.a(),
        -SVG_EFFECT_RASTER_SCALE * transform.b(),
        SVG_EFFECT_RASTER_SCALE * transform.c(),
        -SVG_EFFECT_RASTER_SCALE * transform.d(),
        SVG_EFFECT_RASTER_SCALE * (transform.e() - bounds.origin.x),
        SVG_EFFECT_RASTER_SCALE * (bounds.max_y() - transform.f()),
    );
    let fill_rule = match source.fill_rule {
        RenderedPathFillRule::NonZero => tiny_skia::FillRule::Winding,
        RenderedPathFillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
    };
    let mut paint_path = |paint: &RenderedPathPaint, stroke: bool| {
        let RenderedPathPaint::Solid(color) = paint else {
            return false;
        };
        let mut paint = tiny_skia::Paint::default();
        let color = color.to_rgb_space(css::RgbColorSpace::Srgb);
        let [red, green, blue] = color.components();
        paint.set_color_rgba8(
            (red.clamp(0.0, 1.0) * 255.0).round() as u8,
            (green.clamp(0.0, 1.0) * 255.0).round() as u8,
            (blue.clamp(0.0, 1.0) * 255.0).round() as u8,
            (color.alpha() * 255.0).round() as u8,
        );
        if stroke {
            let stroke = tiny_skia::Stroke {
                width: source.stroke_width.points(),
                miter_limit: source.stroke_style.miter_limit,
                line_cap: match source.stroke_style.line_cap {
                    RenderedPathLineCap::Butt => tiny_skia::LineCap::Butt,
                    RenderedPathLineCap::Round => tiny_skia::LineCap::Round,
                    RenderedPathLineCap::Square => tiny_skia::LineCap::Square,
                },
                line_join: match source.stroke_style.line_join {
                    RenderedPathLineJoin::Miter => tiny_skia::LineJoin::Miter,
                    RenderedPathLineJoin::Round => tiny_skia::LineJoin::Round,
                    RenderedPathLineJoin::Bevel => tiny_skia::LineJoin::Bevel,
                },
                dash: tiny_skia::StrokeDash::new(
                    source.stroke_style.dash_array.clone(),
                    source.stroke_style.dash_offset,
                ),
            };
            pixmap.stroke_path(&path, &paint, &stroke, matrix, None);
        } else {
            pixmap.fill_path(&path, &paint, fill_rule, matrix, None);
        }
        true
    };
    let mut painted = false;
    match source.paint_order {
        RenderedPathPaintOrder::FillThenStroke => {
            if let Some(fill) = &source.fill_paint {
                painted |= paint_path(fill, false);
            }
            if let Some(stroke) = &source.stroke_paint {
                painted |= paint_path(stroke, true);
            }
        }
        RenderedPathPaintOrder::StrokeThenFill => {
            if let Some(stroke) = &source.stroke_paint {
                painted |= paint_path(stroke, true);
            }
            if let Some(fill) = &source.fill_paint {
                painted |= paint_path(fill, false);
            }
        }
    }
    painted.then_some(())
}

fn tiny_skia_path_from_rendered(source: &RenderedPath) -> Option<tiny_skia::Path> {
    let mut builder = tiny_skia::PathBuilder::new();
    for command in &source.commands {
        match command {
            RenderedPathCommand::MoveTo(point) => builder.move_to(point.x, point.y),
            RenderedPathCommand::LineTo(point) => builder.line_to(point.x, point.y),
            RenderedPathCommand::CurveTo {
                control_1,
                control_2,
                end,
            } => builder.cubic_to(
                control_1.x,
                control_1.y,
                control_2.x,
                control_2.y,
                end.x,
                end.y,
            ),
            RenderedPathCommand::Close => builder.close(),
        }
    }
    builder.finish()
}

/// Translate a premultiplied RGBA filter surface, exposing transparent black
/// outside the previous primitive subregion.
///
/// Surface rows are top-to-bottom while paint coordinates are bottom-to-top,
/// hence the caller reverses the paint-space `dy` before this raster-space
/// copy.
fn apply_svg_offset(data: &mut [u8], width: usize, height: usize, dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }
    let source = data.to_vec();
    data.fill(0);
    for source_y in 0..height {
        let destination_y = source_y as i32 + dy;
        if !(0..height as i32).contains(&destination_y) {
            continue;
        }
        for source_x in 0..width {
            let destination_x = source_x as i32 + dx;
            if !(0..width as i32).contains(&destination_x) {
                continue;
            }
            let source_offset = (source_y * width + source_x) * 4;
            let destination_offset = (destination_y as usize * width + destination_x as usize) * 4;
            data[destination_offset..destination_offset + 4]
                .copy_from_slice(&source[source_offset..source_offset + 4]);
        }
    }
}

/// Evaluate `feFlood` composited `in` the current `SourceAlpha` surface.
///
/// Filter surfaces are premultiplied RGBA, so the flood RGB and alpha are
/// each multiplied by the retained source alpha exactly once.
fn apply_svg_flood_in_source_alpha(data: &mut [u8], color: CssColor) {
    let color = color.to_rgb_space(css::RgbColorSpace::Srgb);
    let [red, green, blue] = color.components();
    let flood_alpha = color.alpha().clamp(0.0, 1.0);
    let flood = [
        (red.clamp(0.0, 1.0) * flood_alpha * 255.0).round() as u8,
        (green.clamp(0.0, 1.0) * flood_alpha * 255.0).round() as u8,
        (blue.clamp(0.0, 1.0) * flood_alpha * 255.0).round() as u8,
        (flood_alpha * 255.0).round() as u8,
    ];
    let mut flood_surface = Vec::with_capacity(data.len());
    for _ in 0..data.len() / 4 {
        flood_surface.extend_from_slice(&flood);
    }
    let mut output = vec![0; data.len()];
    let composited = apply_svg_composite(
        &flood_surface,
        data,
        &mut output,
        usvg::filter::CompositeOperator::In,
    );
    debug_assert!(composited, "same-sized RGBA filter surfaces must composite");
    data.copy_from_slice(&output);
}

/// Composite `input1` over/with `input2` using SVG Filter Effects' premultiplied
/// pixel equations.
///
/// Both inputs must be same-sized premultiplied RGBA surfaces. Keeping this
/// operation independent from SVG paths and text lets the future named-surface
/// graph executor compose retained Quire-shaped text without another renderer.
fn apply_svg_composite(
    input1: &[u8],
    input2: &[u8],
    output: &mut [u8],
    operator: usvg::filter::CompositeOperator,
) -> bool {
    if input1.len() != input2.len()
        || input1.len() != output.len()
        || !input1.len().is_multiple_of(4)
    {
        return false;
    }
    let (input1, remainder1) = input1.as_chunks::<4>();
    let (input2, remainder2) = input2.as_chunks::<4>();
    let (output, remainder_output) = output.as_chunks_mut::<4>();
    debug_assert!(remainder1.is_empty() && remainder2.is_empty() && remainder_output.is_empty());
    for ((first, second), destination) in input1.iter().zip(input2).zip(output) {
        let first = first.map(|component| component as f32 / 255.0);
        let second = second.map(|component| component as f32 / 255.0);
        let first_alpha = first[3];
        let second_alpha = second[3];
        let result: [f32; 4] = match operator {
            usvg::filter::CompositeOperator::Over => {
                std::array::from_fn(|index| first[index] + second[index] * (1.0 - first_alpha))
            }
            usvg::filter::CompositeOperator::In => {
                std::array::from_fn(|index| first[index] * second_alpha)
            }
            usvg::filter::CompositeOperator::Out => {
                std::array::from_fn(|index| first[index] * (1.0 - second_alpha))
            }
            usvg::filter::CompositeOperator::Atop => std::array::from_fn(|index| {
                first[index] * second_alpha + second[index] * (1.0 - first_alpha)
            }),
            usvg::filter::CompositeOperator::Xor => std::array::from_fn(|index| {
                first[index] * (1.0 - second_alpha) + second[index] * (1.0 - first_alpha)
            }),
            usvg::filter::CompositeOperator::Arithmetic { k1, k2, k3, k4 } => {
                std::array::from_fn(|index| {
                    (k1 * first[index] * second[index]
                        + k2 * first[index]
                        + k3 * second[index]
                        + k4)
                        .clamp(0.0, 1.0)
                })
            }
        };
        for (destination, component) in destination.iter_mut().zip(result) {
            *destination = (component.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    true
}

/// Apply SVG's 4x5 color matrix to premultiplied sRGB samples.
///
/// Filter primitives operate on unpremultiplied components.  The retained
/// offscreen buffer is premultiplied, so convert at this one backend boundary
/// and re-premultiply before the image is handed to PDF resource planning.
/// SVG Filter Effects defaults `color-interpolation-filters` to linearRGB.
fn apply_svg_color_matrix(data: &mut [u8], matrix: [f32; 20], linear_rgb: bool) {
    let (pixels, remainder) = data.as_chunks_mut::<4>();
    debug_assert!(remainder.is_empty(), "SVG pixels have four channels");
    for pixel in pixels {
        let alpha = pixel[3] as f32 / 255.0;
        if alpha <= 0.0 {
            pixel.fill(0);
            continue;
        }
        let mut red = (pixel[0] as f32 / 255.0 / alpha).clamp(0.0, 1.0);
        let mut green = (pixel[1] as f32 / 255.0 / alpha).clamp(0.0, 1.0);
        let mut blue = (pixel[2] as f32 / 255.0 / alpha).clamp(0.0, 1.0);
        if linear_rgb {
            red = srgb_to_linear(red);
            green = srgb_to_linear(green);
            blue = srgb_to_linear(blue);
        }
        let output = [
            matrix[0] * red + matrix[1] * green + matrix[2] * blue + matrix[3] * alpha + matrix[4],
            matrix[5] * red + matrix[6] * green + matrix[7] * blue + matrix[8] * alpha + matrix[9],
            matrix[10] * red
                + matrix[11] * green
                + matrix[12] * blue
                + matrix[13] * alpha
                + matrix[14],
            matrix[15] * red
                + matrix[16] * green
                + matrix[17] * blue
                + matrix[18] * alpha
                + matrix[19],
        ];
        let alpha = output[3].clamp(0.0, 1.0);
        let encode = |component: f32| {
            let component = component.clamp(0.0, 1.0);
            let component = if linear_rgb {
                linear_to_srgb(component)
            } else {
                component
            };
            (component * alpha * 255.0).round() as u8
        };
        pixel[0] = encode(output[0]);
        pixel[1] = encode(output[1]);
        pixel[2] = encode(output[2]);
        pixel[3] = (alpha * 255.0).round() as u8;
    }
}

/// Apply SVG `feComponentTransfer` channel functions to premultiplied samples.
fn apply_svg_component_transfer(
    data: &mut [u8],
    functions: &[SvgTransferFunction; 4],
    linear_rgb: bool,
) {
    let (pixels, remainder) = data.as_chunks_mut::<4>();
    debug_assert!(remainder.is_empty(), "SVG pixels have four channels");
    for pixel in pixels {
        let alpha = pixel[3] as f32 / 255.0;
        if alpha <= 0.0 {
            pixel.fill(0);
            continue;
        }
        let mut channels = [
            (pixel[0] as f32 / 255.0 / alpha).clamp(0.0, 1.0),
            (pixel[1] as f32 / 255.0 / alpha).clamp(0.0, 1.0),
            (pixel[2] as f32 / 255.0 / alpha).clamp(0.0, 1.0),
            alpha,
        ];
        if linear_rgb {
            for channel in &mut channels[..3] {
                *channel = srgb_to_linear(*channel);
            }
        }
        for (channel, function) in channels.iter_mut().zip(functions) {
            *channel = apply_svg_transfer_function(*channel, function).clamp(0.0, 1.0);
        }
        let alpha = channels[3];
        for (index, channel) in channels[..3].iter().enumerate() {
            let channel = if linear_rgb {
                linear_to_srgb(*channel)
            } else {
                *channel
            };
            pixel[index] = (channel * alpha * 255.0).round() as u8;
        }
        pixel[3] = (alpha * 255.0).round() as u8;
    }
}

fn apply_svg_transfer_function(value: f32, function: &SvgTransferFunction) -> f32 {
    match function {
        SvgTransferFunction::Identity => value,
        SvgTransferFunction::Table(values) => {
            if values.is_empty() {
                return value;
            }
            if values.len() == 1 {
                return values[0];
            }
            let position = value.clamp(0.0, 1.0) * (values.len() - 1) as f32;
            let index = position.floor() as usize;
            let next = (index + 1).min(values.len() - 1);
            values[index] + (values[next] - values[index]) * (position - index as f32)
        }
        SvgTransferFunction::Discrete(values) => {
            if values.is_empty() {
                return value;
            }
            let index = (value.clamp(0.0, 1.0) * values.len() as f32).floor() as usize;
            values[index.min(values.len() - 1)]
        }
        SvgTransferFunction::Linear { slope, intercept } => value * slope + intercept,
        SvgTransferFunction::Gamma {
            amplitude,
            exponent,
            offset,
        } => amplitude * value.clamp(0.0, 1.0).powf(*exponent) + offset,
    }
}

/// Apply `feMorphology` in the bounded filter surface.  Filter input outside
/// the primitive subregion is transparent black, so erosion samples that
/// boundary as zero while dilation leaves it without additional ink.
fn apply_svg_morphology(
    data: &mut [u8],
    width: usize,
    height: usize,
    radius_x: usize,
    radius_y: usize,
    dilate: bool,
) {
    if radius_x == 0 && radius_y == 0 {
        return;
    }
    let source = data.to_vec();
    for y in 0..height {
        for x in 0..width {
            let destination = (y * width + x) * 4;
            for channel in 0..4 {
                let mut value = if dilate { 0 } else { u8::MAX };
                for sample_y in y.saturating_sub(radius_y)..=(y + radius_y).min(height - 1) {
                    for sample_x in x.saturating_sub(radius_x)..=(x + radius_x).min(width - 1) {
                        let sample = source[(sample_y * width + sample_x) * 4 + channel];
                        if dilate {
                            value = value.max(sample);
                        } else {
                            value = value.min(sample);
                        }
                    }
                }
                // Erosion's transparent-black exterior is observable at the
                // finite source surface boundary.
                if !dilate
                    && (x < radius_x
                        || y < radius_y
                        || x.saturating_add(radius_x) >= width
                        || y.saturating_add(radius_y) >= height)
                {
                    value = 0;
                }
                data[destination + channel] = value;
            }
        }
    }
}

/// Apply SVG `feDropShadow` from the current source surface.
///
/// The primitive's result is the untouched input composited over its colored,
/// blurred, translated alpha shadow. The source copy here is an effect
/// intermediate, not a second text layer: SVG text was already shaped once
/// before entering this raster compositor.
fn apply_svg_drop_shadow(
    data: &mut [u8],
    width: usize,
    height: usize,
    std_deviation: f32,
    dx: i32,
    dy: i32,
    color: CssColor,
) {
    let source = data.to_vec();
    let mut shadow = source.clone();
    apply_svg_flood_in_source_alpha(&mut shadow, color);
    gaussian_blur_rgba(&mut shadow, width, height, std_deviation);
    apply_svg_offset(&mut shadow, width, height, dx, dy);
    let composited = apply_svg_composite(
        &source,
        &shadow,
        data,
        usvg::filter::CompositeOperator::Over,
    );
    debug_assert!(composited, "same-sized SVG drop-shadow surfaces composite");
}

/// Apply SVG `feConvolveMatrix` to a premultiplied filter surface.
///
/// The filter specification defines the kernel over unpremultiplied color
/// components. This backend boundary therefore decodes each sample, applies
/// the matrix in the primitive's declared color space, then premultiplies the
/// clamped result for the PDF image. `edgeMode=none` samples transparent
/// black; the other modes resolve coordinates before sampling.
/// <https://www.w3.org/TR/filter-effects/#element-attrdef-feconvolvematrix-kernelmatrix>
#[allow(clippy::too_many_arguments)]
fn apply_svg_convolve_matrix(
    data: &mut [u8],
    width: usize,
    height: usize,
    matrix: &[f32],
    columns: u32,
    rows: u32,
    target_x: u32,
    target_y: u32,
    divisor: f32,
    bias: f32,
    edge_mode: usvg::filter::EdgeMode,
    preserve_alpha: bool,
    linear_rgb: bool,
) -> bool {
    let columns = columns as usize;
    let rows = rows as usize;
    if width == 0
        || height == 0
        || columns == 0
        || rows == 0
        || target_x as usize >= columns
        || target_y as usize >= rows
        || matrix.len() != columns.saturating_mul(rows)
        || !divisor.is_finite()
        || divisor == 0.0
        || !bias.is_finite()
        || matrix.iter().any(|coefficient| !coefficient.is_finite())
    {
        return false;
    }
    let Some(work) = (width as u64)
        .checked_mul(height as u64)
        .and_then(|pixels| pixels.checked_mul(matrix.len() as u64))
    else {
        return false;
    };
    if work > MAX_SVG_CONVOLVE_SAMPLES {
        return false;
    }

    let source = data.to_vec();
    let sample = |x: isize, y: isize| -> [f32; 4] {
        let (x, y) = match edge_mode {
            usvg::filter::EdgeMode::None
                if x < 0 || y < 0 || x >= width as isize || y >= height as isize =>
            {
                return [0.0; 4];
            }
            usvg::filter::EdgeMode::None => (x as usize, y as usize),
            usvg::filter::EdgeMode::Duplicate => (
                x.clamp(0, width as isize - 1) as usize,
                y.clamp(0, height as isize - 1) as usize,
            ),
            usvg::filter::EdgeMode::Wrap => (
                x.rem_euclid(width as isize) as usize,
                y.rem_euclid(height as isize) as usize,
            ),
        };
        let pixel = &source[(y * width + x) * 4..][..4];
        let alpha = pixel[3] as f32 / 255.0;
        if alpha <= 0.0 {
            return [0.0; 4];
        }
        let mut result = [
            (pixel[0] as f32 / 255.0 / alpha).clamp(0.0, 1.0),
            (pixel[1] as f32 / 255.0 / alpha).clamp(0.0, 1.0),
            (pixel[2] as f32 / 255.0 / alpha).clamp(0.0, 1.0),
            alpha,
        ];
        if linear_rgb {
            for channel in &mut result[..3] {
                *channel = srgb_to_linear(*channel);
            }
        }
        result
    };
    for y in 0..height {
        for x in 0..width {
            let mut output = [0.0; 4];
            for kernel_y in 0..rows {
                for kernel_x in 0..columns {
                    // SVG's target identifies the kernel element aligned to
                    // the destination pixel. The other matrix coordinates
                    // are sampled relative to that point.
                    let sample = sample(
                        x as isize - target_x as isize + kernel_x as isize,
                        y as isize - target_y as isize + kernel_y as isize,
                    );
                    let coefficient = matrix[kernel_y * columns + kernel_x];
                    for (component, sample) in output.iter_mut().zip(sample) {
                        *component += sample * coefficient;
                    }
                }
            }
            let center_alpha = source[(y * width + x) * 4 + 3] as f32 / 255.0;
            let alpha = if preserve_alpha {
                center_alpha
            } else {
                (output[3] / divisor + bias).clamp(0.0, 1.0)
            };
            let destination = &mut data[(y * width + x) * 4..][..4];
            for (destination, component) in destination[..3].iter_mut().zip(output[..3].iter()) {
                let component = (component / divisor + bias).clamp(0.0, 1.0);
                let component = if linear_rgb {
                    linear_to_srgb(component)
                } else {
                    component
                };
                *destination = (component * alpha * 255.0).round() as u8;
            }
            destination[3] = (alpha * 255.0).round() as u8;
        }
    }
    true
}

fn srgb_to_linear(component: f32) -> f32 {
    if component <= 0.04045 {
        component / 12.92
    } else {
        ((component + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(component: f32) -> f32 {
    if component <= 0.003_130_8 {
        component * 12.92
    } else {
        1.055 * component.powf(1.0 / 2.4) - 0.055
    }
}

/// Blur a premultiplied RGBA buffer with a normalized separable Gaussian.
///
/// The bounded surface check in [`rasterize_svg_solid_paths`] means the
/// straightforward deterministic convolution is appropriate here and avoids
/// depending on a second graphics stack for SVG filter math.
fn gaussian_blur_rgba(data: &mut [u8], width: usize, height: usize, sigma: f32) {
    if !sigma.is_finite() || sigma <= 0.01 || width == 0 || height == 0 {
        return;
    }
    let radius = (sigma * 3.0).ceil().clamp(1.0, 256.0) as isize;
    let weights: Vec<f32> = (-radius..=radius)
        .map(|offset| (-(offset * offset) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let normalization: f32 = weights.iter().sum();
    let mut horizontal = vec![0_u8; data.len()];
    for y in 0..height {
        for x in 0..width {
            let destination = (y * width + x) * 4;
            for channel in 0..4 {
                let mut value = 0.0;
                for (weight_index, weight) in weights.iter().enumerate() {
                    let sample_x = (x as isize + weight_index as isize - radius)
                        .clamp(0, width as isize - 1) as usize;
                    value += data[(y * width + sample_x) * 4 + channel] as f32 * *weight;
                }
                horizontal[destination + channel] = (value / normalization).round() as u8;
            }
        }
    }
    for y in 0..height {
        for x in 0..width {
            let destination = (y * width + x) * 4;
            for channel in 0..4 {
                let mut value = 0.0;
                for (weight_index, weight) in weights.iter().enumerate() {
                    let sample_y = (y as isize + weight_index as isize - radius)
                        .clamp(0, height as isize - 1) as usize;
                    value += horizontal[(sample_y * width + x) * 4 + channel] as f32 * *weight;
                }
                data[destination + channel] = (value / normalization).round() as u8;
            }
        }
    }
}

/// Resolve SVG's selected baseline and inherited `baseline-shift` list with
/// Quire's document font metrics.
///
/// SVG 2 defines the text positioning point in terms of a selected baseline;
/// an alphabetic baseline is the default. Quire's `FontSystem` already
/// resolves OpenType BASE coordinates, variable-font instances, and
/// synthesized fallback metrics for CSS inline layout, so SVG adapts the
/// normalized `usvg` values to that concrete API instead of duplicating a
/// font-metric path here.
/// <https://www.w3.org/TR/SVG2/text.html#TextLayoutAlgorithm>
/// <https://drafts.csswg.org/css-inline-3/#baseline-alignment>
fn svg_text_baseline_shift(
    font_system: &mut FontSystem,
    span: &usvg::TextSpan,
    style: &ComputedStyle,
    font_scale: SvgFontScale,
) -> crate::document::paint::text::TextRunDisplacement {
    let baseline_metric = svg_alignment_baseline_metric(span);
    let alphabetic = font_system
        .baseline_offset_for_style(style, BaselineMetric::Alphabetic)
        .points();
    let selected = font_system
        .baseline_offset_for_style(style, baseline_metric)
        .points();
    // Baseline coordinates are measured downward from the text content-area
    // start. Moving an alternative selected baseline to SVG's positioning
    // point therefore moves the alphabetic glyph origin by their difference.
    let mut shift = alphabetic - selected;

    // `usvg` retains the inherited property values from `<text>` down to the
    // leaf tspan. SVG applies the innermost shift first; additions commute
    // for the numeric values modelled here, but preserving that order makes
    // the reset baseline explicit and matches its normalized representation.
    for baseline_shift in span.baseline_shift().iter().rev() {
        shift += match baseline_shift {
            usvg::BaselineShift::Baseline => 0.0,
            usvg::BaselineShift::Number(value) if value.is_finite() => {
                -font_scale.scale_svg_length(*value)
            }
            usvg::BaselineShift::Superscript => -font_system
                .script_vertical_align_shift(style, css::BaselineShift::Super)
                .unwrap_or(style.font_size * 0.45),
            usvg::BaselineShift::Subscript => -font_system
                .script_vertical_align_shift(style, css::BaselineShift::Sub)
                .unwrap_or(-style.font_size * 0.4),
            usvg::BaselineShift::Number(_) => 0.0,
        };
    }
    // Baseline coordinates are initially measured in SVG's y-down user
    // space, while shaped runs use PDF's y-up glyph space.
    crate::document::paint::text::TextRunDisplacement::new(0.0, -shift)
}

fn svg_alignment_baseline_metric(span: &usvg::TextSpan) -> BaselineMetric {
    let alignment = match span.alignment_baseline() {
        usvg::AlignmentBaseline::Auto | usvg::AlignmentBaseline::Baseline => {
            match span.dominant_baseline() {
                usvg::DominantBaseline::Ideographic => usvg::AlignmentBaseline::Ideographic,
                usvg::DominantBaseline::Hanging => usvg::AlignmentBaseline::Hanging,
                usvg::DominantBaseline::Mathematical => usvg::AlignmentBaseline::Mathematical,
                usvg::DominantBaseline::Central => usvg::AlignmentBaseline::Central,
                usvg::DominantBaseline::Middle => usvg::AlignmentBaseline::Middle,
                usvg::DominantBaseline::TextAfterEdge => usvg::AlignmentBaseline::TextAfterEdge,
                usvg::DominantBaseline::TextBeforeEdge => usvg::AlignmentBaseline::TextBeforeEdge,
                usvg::DominantBaseline::Auto
                | usvg::DominantBaseline::UseScript
                | usvg::DominantBaseline::NoChange
                | usvg::DominantBaseline::ResetSize
                | usvg::DominantBaseline::Alphabetic => usvg::AlignmentBaseline::Alphabetic,
            }
        }
        alignment => alignment,
    };
    match alignment {
        usvg::AlignmentBaseline::BeforeEdge | usvg::AlignmentBaseline::TextBeforeEdge => {
            BaselineMetric::TextTop
        }
        usvg::AlignmentBaseline::Middle => BaselineMetric::Middle,
        usvg::AlignmentBaseline::Central => BaselineMetric::Central,
        usvg::AlignmentBaseline::AfterEdge | usvg::AlignmentBaseline::TextAfterEdge => {
            BaselineMetric::TextBottom
        }
        usvg::AlignmentBaseline::Ideographic => BaselineMetric::Ideographic,
        usvg::AlignmentBaseline::Hanging => BaselineMetric::Hanging,
        usvg::AlignmentBaseline::Mathematical => BaselineMetric::Mathematical,
        usvg::AlignmentBaseline::Auto
        | usvg::AlignmentBaseline::Baseline
        | usvg::AlignmentBaseline::Alphabetic => BaselineMetric::Alphabetic,
    }
}

/// Place Quire-shaped SVG glyphs along a normalized `<textPath>` contour.
///
/// Text-on-a-path needs an independently rotated origin for every glyph, so
/// it is intentionally realized as semantic outline fallback. The path
/// sampler is geometry-only; no upstream font selection or shaping occurs.
fn svg_text_path_outline_paths(
    font_system: &FontSystem,
    line: &crate::document::paint::text::RenderedLine,
    path: &usvg::TextPath,
    coordinates: SvgTextCoordinateTransform,
    start_offset: f32,
    paint: &SvgTextPaint,
) -> Vec<RenderedPath> {
    if !start_offset.is_finite() {
        return Vec::new();
    }
    let paint = paint.outline_paint();
    let mut paths = Vec::new();
    let mut cursor = start_offset;
    for run in &line.runs {
        let Some(glyphs) = run.glyphs.as_ref() else {
            continue;
        };
        for glyph in glyphs {
            let advance = coordinates
                .font_scale()
                .unscale_text_length(glyph.x_advance);
            let glyph_offset = coordinates.font_scale().unscale_text_length(glyph.x_offset);
            let center = cursor + glyph_offset + advance * 0.5;
            let Some(position) = path.position_at_distance(center) else {
                cursor += advance;
                continue;
            };
            let page_position =
                coordinates.map_position(SvgTextPosition::new(position.x, position.y));
            let Some(glyph_matrix) = run.text_matrix.rotated_in_text_space(
                coordinates.glyph_rotation_degrees(position.tangent_degrees),
            ) else {
                cursor += advance;
                continue;
            };
            let center_offset = glyph_matrix.transform_local_point(
                crate::document::paint::text::TextRunPoint::new(glyph.x_advance * 0.5, 0.0),
            );
            let mut glyph_run = run.clone();
            glyph_run.x_offset = page_position.x - line.origin().x - center_offset.x;
            glyph_run.y_offset = page_position.y - line.origin().y - center_offset.y;
            let mut glyph = glyph.clone();
            // The text-path distance has already consumed SVG's horizontal
            // character offset. Avoid applying that same source position a
            // second time inside the glyph-local outline transform.
            glyph.x_offset = 0.0;
            glyph_run.glyphs = Some(vec![glyph].into());
            glyph_run.glyph_source_ranges = None;
            glyph_run.text_matrix = glyph_matrix;
            paths.extend(svg_text_outline_paths(
                font_system,
                line.origin(),
                &[glyph_run],
                &paint,
            ));
            cursor += advance;
        }
    }
    paths
}

/// Lower a span with non-zero SVG character rotation to outlines while
/// retaining the source text as one semantic unit. PDF text operators have a
/// run-wide text matrix; emitting one native run would rotate later advances
/// as well as the individual glyphs, which is not SVG's `rotate` behavior.
/// The outlines still come from the same Quire-shaped glyph IDs and selected
/// document font, and `SvgOutlinedText` carries the corresponding ActualText.
fn svg_text_rotation_paths(
    font_system: &FontSystem,
    line: &crate::document::paint::text::RenderedLine,
    source: &str,
    rotate: &[f32],
    source_character_offset: usize,
    coordinates: SvgTextCoordinateTransform,
    paint: &SvgTextPaint,
) -> Option<Vec<RenderedPath>> {
    let paint = paint.outline_paint();
    let mut paths = Vec::new();
    let mut saw_rotation = false;
    for run in &line.runs {
        let (Some(glyphs), Some(source_ranges)) =
            (run.glyphs.as_ref(), run.glyph_source_ranges.as_ref())
        else {
            continue;
        };
        let mut cursor = 0.0;
        let mut previous_cluster = None;
        let mut cluster_angle = 0.0;
        for (glyph, source_range) in glyphs.iter().zip(source_ranges.iter()) {
            if let Some(source_range) = source_range
                && previous_cluster != Some(source_range.start)
            {
                let character = source[..source_range.start].chars().count();
                let absolute_character = source_character_offset + character;
                cluster_angle = rotate
                    .get(absolute_character)
                    .copied()
                    .or_else(|| rotate.last().copied())
                    .filter(|angle| angle.is_finite())
                    .unwrap_or(0.0);
                previous_cluster = Some(source_range.start);
            }
            let pen = run.text_matrix.transform_local_point(
                crate::document::paint::text::TextRunPoint::new(cursor, 0.0),
            );
            let mut glyph_run = run.clone();
            glyph_run.x_offset += pen.x;
            glyph_run.y_offset += pen.y;
            if cluster_angle != 0.0 {
                saw_rotation = true;
                glyph_run.text_matrix = run
                    .text_matrix
                    .rotated_in_text_space(coordinates.glyph_rotation_degrees(cluster_angle))
                    .expect("finite SVG rotation produces a finite text matrix");
            }
            glyph_run.glyphs = Some(vec![glyph.clone()].into());
            glyph_run.glyph_source_ranges = None;
            paths.extend(svg_text_outline_paths(
                font_system,
                line.origin(),
                &[glyph_run],
                &paint,
            ));
            cursor += glyph.x_advance;
        }
    }
    saw_rotation.then_some(paths)
}

/// Apply SVG's character-indexed relative position lists to an already shaped
/// glyph stream.  The lists belong to the outer `<text>` element, while spans
/// and font fallback divide the stream into independently shaped runs; source
/// ranges retained by Quire's shaping API reconnect those two coordinate
/// systems without reshaping through `usvg`.
///
/// SVG 2 text positioning applies `dx`/`dy` before the affected character.
/// A cluster can have several glyphs, so every glyph in a cluster receives
/// the same relative origin while only the first glyph consumes the list
/// entry.  This preserves ligatures and combining marks selected by Quire's
/// shared OpenType shaping path.
/// <https://www.w3.org/TR/SVG2/text.html#TextData>
fn apply_svg_relative_positioning(
    runs: &mut [crate::document::paint::text::RenderedTextRun],
    source: &str,
    dx: &[f32],
    dy: &[f32],
    source_character_offset: usize,
    coordinates: SvgTextCoordinateTransform,
) {
    if dx.is_empty() && dy.is_empty() {
        return;
    }
    let mut accumulated = SvgTextUserDisplacement::zero();
    let mut next_character = 0;
    let mut previous_cluster = None;
    for run in runs {
        let (Some(glyphs), Some(source_ranges)) =
            (run.glyphs.as_ref(), run.glyph_source_ranges.as_ref())
        else {
            continue;
        };
        let mut adjusted = glyphs.as_ref().to_vec();
        for (glyph, source_range) in adjusted.iter_mut().zip(source_ranges.iter()) {
            let Some(source_range) = source_range else {
                continue;
            };
            let character = source[..source_range.start].chars().count();
            if previous_cluster != Some(source_range.start) {
                for index in next_character..=character {
                    let absolute_index = source_character_offset + index;
                    accumulated.x += dx.get(absolute_index).copied().unwrap_or(0.0);
                    accumulated.y += dy.get(absolute_index).copied().unwrap_or(0.0);
                }
                next_character = character.saturating_add(1);
                previous_cluster = Some(source_range.start);
            }
            let Some(local_displacement) = run.text_matrix.inverse_transform_local_displacement(
                coordinates.text_run_displacement(accumulated),
            ) else {
                // A non-invertible text matrix cannot preserve SVG's
                // character-position semantics. `render_svg_text` rejects
                // non-invertible outer transforms before this point, so this
                // only protects a future per-run matrix extension.
                continue;
            };
            glyph.x_offset += local_displacement.x;
            glyph.y_offset += local_displacement.y;
        }
        run.glyphs = Some(adjusted.into());
    }
}

#[derive(Debug, Clone, Copy)]
struct SvgTextLengthAdjustment {
    advance: f32,
    inline_scale: f32,
}

fn svg_text_length_adjustment(
    span: &usvg::TextSpan,
    runs: &mut [crate::document::paint::text::RenderedTextRun],
    measured_advance: f32,
    font_scale: SvgFontScale,
    style: &ComputedStyle,
) -> SvgTextLengthAdjustment {
    let Some(requested) = span.text_length() else {
        return SvgTextLengthAdjustment {
            advance: measured_advance,
            inline_scale: 1.0,
        };
    };
    let requested = font_scale.scale_svg_length(requested);
    if !requested.is_finite() || requested < 0.0 || measured_advance <= 0.0 {
        return SvgTextLengthAdjustment {
            advance: measured_advance,
            inline_scale: 1.0,
        };
    }
    match span.length_adjust() {
        usvg::LengthAdjust::SpacingAndGlyphs => SvgTextLengthAdjustment {
            advance: requested,
            inline_scale: requested / measured_advance,
        },
        usvg::LengthAdjust::Spacing => {
            let glyph_count = runs
                .iter()
                .filter_map(|run| run.glyphs.as_ref())
                .map(|glyphs| glyphs.len())
                .sum::<usize>();
            if glyph_count < 2 {
                return SvgTextLengthAdjustment {
                    advance: measured_advance,
                    inline_scale: 1.0,
                };
            }
            let extra_spacing = (requested - measured_advance) / (glyph_count - 1) as f32;
            let vertical_inline_axis =
                crate::layout::text_paint::VerticalInlineAxis::for_style(style);
            let mut remaining_glyphs = glyph_count;
            let mut accumulated_spacing = 0.0;
            for run in runs {
                if let Some(axis) = vertical_inline_axis {
                    run.y_offset += axis.advance_sign() * accumulated_spacing;
                } else {
                    run.x_offset += accumulated_spacing;
                }
                let Some(glyphs) = run.glyphs.as_ref() else {
                    continue;
                };
                let mut adjusted = glyphs.as_ref().to_vec();
                for glyph in &mut adjusted {
                    remaining_glyphs -= 1;
                    if remaining_glyphs != 0 {
                        glyph.x_advance += extra_spacing;
                        glyph.nominal_x_advance += extra_spacing;
                        accumulated_spacing += extra_spacing;
                    }
                }
                run.glyphs = Some(adjusted.into());
            }
            SvgTextLengthAdjustment {
                advance: requested,
                inline_scale: 1.0,
            }
        }
    }
}

#[derive(Debug, Clone)]
enum SvgTextPaint {
    Native(CssColor),
    Outline(SvgOutlinedTextPaint),
}

#[derive(Debug, Clone)]
struct SvgOutlinedTextPaint {
    fill: Option<RenderedPathPaint>,
    stroke: Option<RenderedPathPaint>,
    stroke_width: PaintStrokeWidth,
    stroke_style: RenderedPathStrokeStyle,
    paint_order: RenderedPathPaintOrder,
}

impl SvgTextPaint {
    fn outline_paint(&self) -> SvgOutlinedTextPaint {
        match self {
            Self::Native(color) => SvgOutlinedTextPaint {
                fill: Some(RenderedPathPaint::Solid(*color)),
                stroke: None,
                stroke_width: PaintStrokeWidth::ZERO,
                stroke_style: RenderedPathStrokeStyle::default(),
                paint_order: RenderedPathPaintOrder::FillThenStroke,
            },
            Self::Outline(paint) => paint.clone(),
        }
    }
}

fn svg_text_outline_paths(
    font_system: &FontSystem,
    origin: PaintPoint,
    runs: &[crate::document::paint::text::RenderedTextRun],
    paint: &SvgOutlinedTextPaint,
) -> Vec<RenderedPath> {
    let Some(seed_paint) = paint.fill.as_ref().or(paint.stroke.as_ref()).cloned() else {
        return Vec::new();
    };
    font_system
        .glyph_outline_paths(origin, runs, seed_paint)
        .into_iter()
        .map(|mut path| {
            path.stroke_width = paint.stroke_width;
            path.with_paints(paint.fill.clone(), paint.stroke.clone())
                .with_stroke_style(paint.stroke_style.clone())
                .with_paint_order(paint.paint_order)
        })
        .collect()
}

fn svg_text_paint(
    span: &usvg::TextSpan,
    transform: PaintTransform,
    font_scale: SvgFontScale,
) -> Option<SvgTextPaint> {
    svg_text_paint_from_sources(
        span.fill(),
        span.stroke(),
        span.paint_order(),
        transform,
        font_scale,
    )
}

fn svg_text_paint_from_sources(
    fill_source: Option<&usvg::Fill>,
    stroke_source: Option<&usvg::Stroke>,
    paint_order: usvg::PaintOrder,
    transform: PaintTransform,
    font_scale: SvgFontScale,
) -> Option<SvgTextPaint> {
    let mut fill = fill_source.and_then(|fill| svg_paint(fill.paint(), fill.opacity().get()));
    let mut stroke =
        stroke_source.and_then(|stroke| svg_paint(stroke.paint(), stroke.opacity().get()));
    for paint in [&mut fill, &mut stroke].into_iter().flatten() {
        if let RenderedPathPaint::Gradient(gradient) = paint {
            gradient.transform = transform.multiply(gradient.transform);
        }
    }
    if fill.is_none() && stroke.is_none() {
        return None;
    }
    if let (Some(RenderedPathPaint::Solid(color)), None) = (&fill, &stroke) {
        return Some(SvgTextPaint::Native(*color));
    }
    let (stroke_width, stroke_style) = stroke_source.map_or_else(
        || (PaintStrokeWidth::ZERO, RenderedPathStrokeStyle::default()),
        |stroke| {
            (
                PaintStrokeWidth::new(stroke.width().get() * font_scale.points()),
                RenderedPathStrokeStyle {
                    line_cap: match stroke.linecap() {
                        usvg::LineCap::Butt => RenderedPathLineCap::Butt,
                        usvg::LineCap::Round => RenderedPathLineCap::Round,
                        usvg::LineCap::Square => RenderedPathLineCap::Square,
                    },
                    line_join: match stroke.linejoin() {
                        usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => {
                            RenderedPathLineJoin::Miter
                        }
                        usvg::LineJoin::Round => RenderedPathLineJoin::Round,
                        usvg::LineJoin::Bevel => RenderedPathLineJoin::Bevel,
                    },
                    miter_limit: stroke.miterlimit().get(),
                    dash_array: stroke.dasharray().map_or_else(Vec::new, ToOwned::to_owned),
                    dash_offset: stroke.dashoffset(),
                },
            )
        },
    );
    Some(SvgTextPaint::Outline(SvgOutlinedTextPaint {
        fill: fill.take(),
        stroke: stroke.take(),
        stroke_width,
        stroke_style,
        paint_order: match paint_order {
            usvg::PaintOrder::FillAndStroke => RenderedPathPaintOrder::FillThenStroke,
            usvg::PaintOrder::StrokeAndFill => RenderedPathPaintOrder::StrokeThenFill,
        },
    }))
}

fn svg_text_style(
    span: &usvg::TextSpan,
    writing_mode: usvg::WritingMode,
    text_orientation: usvg::TextOrientation,
    direction: usvg::TextDirection,
    unicode_bidi: usvg::TextUnicodeBidi,
    scale: f32,
) -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    style.font_family = svg_font_family(span.font().families());
    style.font_weight = FontWeight(span.font().weight());
    style.font_style = match span.font().style() {
        usvg::FontStyle::Normal => FontStyle::Normal,
        usvg::FontStyle::Italic => FontStyle::Italic,
        usvg::FontStyle::Oblique => FontStyle::DEFAULT_OBLIQUE,
    };
    style.font_width = match span.font().stretch() {
        usvg::FontStretch::UltraCondensed => FontWidth::ULTRA_CONDENSED,
        usvg::FontStretch::ExtraCondensed => FontWidth::EXTRA_CONDENSED,
        usvg::FontStretch::Condensed => FontWidth::CONDENSED,
        usvg::FontStretch::SemiCondensed => FontWidth::SEMI_CONDENSED,
        usvg::FontStretch::Normal => FontWidth::NORMAL,
        usvg::FontStretch::SemiExpanded => FontWidth::SEMI_EXPANDED,
        usvg::FontStretch::Expanded => FontWidth::EXPANDED,
        usvg::FontStretch::ExtraExpanded => FontWidth::EXTRA_EXPANDED,
        usvg::FontStretch::UltraExpanded => FontWidth::ULTRA_EXPANDED,
    };
    style.font_size = span.font_size().get() * scale;
    style.writing_mode = match writing_mode {
        usvg::WritingMode::LeftToRight => css::WritingMode::HorizontalTb,
        usvg::WritingMode::VerticalRl => css::WritingMode::VerticalRl,
        usvg::WritingMode::VerticalLr => css::WritingMode::VerticalLr,
        usvg::WritingMode::SidewaysRl => css::WritingMode::SidewaysRl,
        usvg::WritingMode::SidewaysLr => css::WritingMode::SidewaysLr,
    };
    style.text_orientation = match text_orientation {
        usvg::TextOrientation::Mixed => css::TextOrientation::Mixed,
        usvg::TextOrientation::Upright => css::TextOrientation::Upright,
        usvg::TextOrientation::Sideways => css::TextOrientation::Sideways,
    };
    style.direction = match direction {
        usvg::TextDirection::LeftToRight => css::Direction::Ltr,
        usvg::TextDirection::RightToLeft => css::Direction::Rtl,
    };
    style.unicode_bidi = match unicode_bidi {
        usvg::TextUnicodeBidi::Normal => css::UnicodeBidi::Normal,
        usvg::TextUnicodeBidi::Embed => css::UnicodeBidi::Embed,
        usvg::TextUnicodeBidi::Isolate => css::UnicodeBidi::Isolate,
        usvg::TextUnicodeBidi::BidiOverride => css::UnicodeBidi::BidiOverride,
        usvg::TextUnicodeBidi::IsolateOverride => css::UnicodeBidi::IsolateOverride,
        usvg::TextUnicodeBidi::Plaintext => css::UnicodeBidi::Plaintext,
    };
    style.line_height = style.font_size * 1.2;
    style.letter_spacing =
        css::ComputedLengthPercentage::from_points(span.letter_spacing() * scale);
    style.word_spacing = css::ComputedLengthPercentage::from_points(span.word_spacing() * scale);
    style.font_kerning = if span.apply_kerning() {
        FontKerning::Normal
    } else {
        FontKerning::None
    };
    style.font_variant_caps = if span.small_caps() {
        FontVariantCaps::SmallCaps
    } else {
        FontVariantCaps::Normal
    };
    style.font_variation_settings = FontVariationSettings(
        span.font()
            .variations()
            .iter()
            .filter(|variation| variation.value.is_finite())
            .map(|variation| FontVariationSetting {
                tag: variation.tag,
                value: variation.value.to_bits(),
            })
            .collect(),
    );
    style.text_shadow = span
        .text_shadow()
        .and_then(|shadow| css::parse_text_shadow(shadow, style.font_size))
        .unwrap_or_default();
    style
}

fn svg_font_family(families: &[usvg::FontFamily]) -> FontFamily {
    let mut families = families.iter().map(|family| match family {
        usvg::FontFamily::SansSerif => FontFamily::SansSerif,
        usvg::FontFamily::Serif => FontFamily::Serif,
        usvg::FontFamily::Monospace => FontFamily::Monospace,
        usvg::FontFamily::Named(name) => FontFamily::named(name.clone()),
        // Quire does not yet expose distinct cursive/fantasy generic family
        // values. Preserve the authored generic as a shared fallback name.
        usvg::FontFamily::Cursive => FontFamily::named("cursive"),
        usvg::FontFamily::Fantasy => FontFamily::named("fantasy"),
    });
    let first = families.next().unwrap_or(FontFamily::SansSerif);
    match families.next() {
        Some(second) => FontFamily::List(
            std::iter::once(first)
                .chain(std::iter::once(second))
                .chain(families)
                .collect(),
        ),
        None => first,
    }
}

/// The vector result of proving that an SVG filter does not alter
/// `SourceGraphic`.  Filter primitive regions remain observable clipping, so
/// they are retained even when the pixel operation is a mandated pass-through.
/// <https://drafts.csswg.org/filter-effects/#tainted-filter-primitives>
#[derive(Debug, Clone, Copy)]
enum SvgFilterAnalysis {
    ExactSourceGraphic {
        filter_clip: Option<usvg::NonZeroRect>,
    },
    RequiresRasterBackend,
}

/// A normalized filter sequence that can currently be executed from one
/// rasterized `SourceGraphic` without fabricating unsupported intermediate
/// inputs.  This stays deliberately concrete: future primitives add explicit
/// variants rather than treating every SVG filter as an opaque backend blob.
#[derive(Debug, Clone)]
enum SvgRasterFilter {
    /// An ordered linear chain of in-place pixel transforms. Each primitive
    /// consumes the previous primitive's result, so one bounded surface is
    /// sufficient and no filter-graph input is approximated.
    PixelEffects {
        effects: Vec<SvgRasterPixelEffect>,
        region: usvg::NonZeroRect,
    },
    /// `feFlood` composited `in` `SourceAlpha`. This is an exact two-input
    /// filter pattern, but can be evaluated on the retained source-alpha
    /// surface without fabricating a general graph cache.
    FloodInSourceAlpha {
        color: CssColor,
        region: usvg::NonZeroRect,
    },
    GaussianBlur {
        std_deviation: f32,
        region: usvg::NonZeroRect,
    },
    Offset {
        dx: f32,
        dy: f32,
        region: usvg::NonZeroRect,
    },
    ColorMatrix {
        matrix: [f32; 20],
        linear_rgb: bool,
        region: usvg::NonZeroRect,
    },
    ComponentTransfer {
        functions: [SvgTransferFunction; 4],
        linear_rgb: bool,
        region: usvg::NonZeroRect,
    },
    Morphology {
        radius_x: f32,
        radius_y: f32,
        dilate: bool,
        region: usvg::NonZeroRect,
    },
}

#[derive(Debug, Clone)]
enum SvgRasterPixelEffect {
    /// SVG-filter standard deviation in paint units. The raster boundary
    /// converts it to surface pixels exactly once.
    GaussianBlur {
        std_deviation: f32,
    },
    /// SVG `feDropShadow`, whose result combines a generated shadow with the
    /// current source surface instead of replacing it.
    DropShadow {
        std_deviation: f32,
        dx: f32,
        dy: f32,
        color: CssColor,
    },
    /// A paint-space translation of the current filter surface.
    Offset {
        dx: f32,
        dy: f32,
    },
    /// Replace current alpha coverage with a solid premultiplied flood.
    FloodInSourceAlpha {
        color: CssColor,
    },
    ColorMatrix {
        matrix: [f32; 20],
        linear_rgb: bool,
    },
    ComponentTransfer {
        functions: [SvgTransferFunction; 4],
        linear_rgb: bool,
    },
    Morphology {
        /// Paint-space radii converted to surface pixels at the raster
        /// boundary alongside every other spatial primitive.
        radius_x: f32,
        radius_y: f32,
        dilate: bool,
    },
    ConvolveMatrix {
        matrix: Vec<f32>,
        columns: u32,
        rows: u32,
        target_x: u32,
        target_y: u32,
        divisor: f32,
        bias: f32,
        edge_mode: usvg::filter::EdgeMode,
        preserve_alpha: bool,
        linear_rgb: bool,
    },
    /// A named working result composited with the retained `SourceGraphic`.
    /// The graph compiler emits this only when the preceding result name is
    /// explicit, so it cannot silently substitute a different SVG input.
    CompositeWithSourceGraphic {
        operator: usvg::filter::CompositeOperator,
        source_as_second: bool,
    },
    /// As above, with the derived SVG standard input `SourceAlpha`.
    CompositeWithSourceAlpha {
        operator: usvg::filter::CompositeOperator,
        source_as_second: bool,
    },
}

#[derive(Debug, Clone)]
enum SvgTransferFunction {
    Identity,
    Table(Vec<f32>),
    Discrete(Vec<f32>),
    Linear {
        slope: f32,
        intercept: f32,
    },
    Gamma {
        amplitude: f32,
        exponent: f32,
        offset: f32,
    },
}

/// Recognize the first fully retained-scene filter operation.
///
/// SVG's filter graph has named intermediate results and multiple standard
/// inputs.  A lone `feGaussianBlur` of `SourceGraphic` has neither ambiguity:
/// its filter region, source pixels, and premultiplied-alpha blur are all
/// available at this boundary.  Other graphs remain on the explicit
/// `RequiresRasterBackend` path until their input/output rules are modeled.
fn svg_raster_filter_plan(filters: &[Arc<usvg::filter::Filter>]) -> Option<SvgRasterFilter> {
    if filters.len() != 1 {
        return None;
    }
    let filter = &filters[0];
    if let Some(color) = svg_flood_in_source_alpha(filter.primitives()) {
        return Some(SvgRasterFilter::FloodInSourceAlpha {
            color,
            region: filter.rect(),
        });
    }
    if let Some(shadow) = svg_blurred_flood_shadow_merge(filter.primitives()) {
        return Some(SvgRasterFilter::PixelEffects {
            effects: vec![shadow],
            region: filter.rect(),
        });
    }
    if let Some(effects) = svg_source_graphic_composite_effects(filter.primitives()) {
        return Some(SvgRasterFilter::PixelEffects {
            effects,
            region: filter.rect(),
        });
    }
    if let Some(effects) = svg_source_alpha_composite_effects(filter.primitives()) {
        return Some(SvgRasterFilter::PixelEffects {
            effects,
            region: filter.rect(),
        });
    }
    if let Some(effects) = svg_linear_pixel_effects(filter.primitives()) {
        return Some(SvgRasterFilter::PixelEffects {
            effects,
            region: filter.rect(),
        });
    }
    let primitive = filter.primitives().first()?;
    if filter.primitives().len() != 1 {
        return None;
    }
    match primitive.kind() {
        usvg::filter::Kind::GaussianBlur(blur)
            if matches!(blur.input(), usvg::filter::Input::SourceGraphic) =>
        {
            let std_deviation = (blur.std_dev_x().get() + blur.std_dev_y().get()) * 0.5;
            (std_deviation.is_finite() && std_deviation > 0.0).then_some(
                SvgRasterFilter::GaussianBlur {
                    std_deviation,
                    region: filter.rect(),
                },
            )
        }
        usvg::filter::Kind::Offset(offset)
            if matches!(offset.input(), usvg::filter::Input::SourceGraphic)
                && offset.dx().is_finite()
                && offset.dy().is_finite() =>
        {
            Some(SvgRasterFilter::Offset {
                dx: offset.dx(),
                dy: offset.dy(),
                region: filter.rect(),
            })
        }
        usvg::filter::Kind::ColorMatrix(color_matrix)
            if matches!(color_matrix.input(), usvg::filter::Input::SourceGraphic) =>
        {
            svg_color_matrix_values(color_matrix.kind()).map(|matrix| {
                SvgRasterFilter::ColorMatrix {
                    matrix,
                    linear_rgb: primitive.color_interpolation()
                        == usvg::filter::ColorInterpolation::LinearRGB,
                    region: filter.rect(),
                }
            })
        }
        usvg::filter::Kind::ComponentTransfer(transfer)
            if matches!(transfer.input(), usvg::filter::Input::SourceGraphic) =>
        {
            let functions = [
                svg_transfer_function(transfer.func_r())?,
                svg_transfer_function(transfer.func_g())?,
                svg_transfer_function(transfer.func_b())?,
                svg_transfer_function(transfer.func_a())?,
            ];
            Some(SvgRasterFilter::ComponentTransfer {
                functions,
                linear_rgb: primitive.color_interpolation()
                    == usvg::filter::ColorInterpolation::LinearRGB,
                region: filter.rect(),
            })
        }
        usvg::filter::Kind::Morphology(morphology)
            if matches!(morphology.input(), usvg::filter::Input::SourceGraphic) =>
        {
            let radius_x = morphology.radius_x().get();
            let radius_y = morphology.radius_y().get();
            (radius_x.is_finite() && radius_y.is_finite()).then_some(SvgRasterFilter::Morphology {
                radius_x,
                radius_y,
                dilate: morphology.operator() == usvg::filter::MorphologyOperator::Dilate,
                region: filter.rect(),
            })
        }
        _ => None,
    }
}

/// Recognize `feFlood` followed by `feComposite operator="in"` with the
/// flood as `in` and `SourceAlpha` as `in2`.
///
/// This is the common SVG filter idiom for coloring an alpha mask. It is a
/// real binary primitive, yet does not require retaining two independent
/// images: source-alpha coverage is already present in the glyph surface.
fn svg_flood_in_source_alpha(primitives: &[usvg::filter::Primitive]) -> Option<CssColor> {
    let [flood_primitive, composite_primitive] = primitives else {
        return None;
    };
    let usvg::filter::Kind::Flood(flood) = flood_primitive.kind() else {
        return None;
    };
    let usvg::filter::Kind::Composite(composite) = composite_primitive.kind() else {
        return None;
    };
    (composite.operator() == usvg::filter::CompositeOperator::In
        && matches!(composite.input1(), usvg::filter::Input::Reference(name) if name == flood_primitive.result())
        && matches!(composite.input2(), usvg::filter::Input::SourceAlpha))
        .then(|| svg_color(flood.color(), flood.opacity().get()))
}

/// Recognize the canonical SVG drop-shadow graph:
/// `SourceGraphic` → blur → offset, flood `in` that alpha, then merge the
/// shadow below `SourceGraphic`.
///
/// This is an exact named-input graph, not a heuristic for arbitrary merge or
/// composite nodes. Its output is identical to `feDropShadow`, so it reuses
/// the same bounded Quire-shaped source surface and compositing operation.
/// <https://www.w3.org/TR/filter-effects/#element-attrdef-fedropshadow-in>
fn svg_blurred_flood_shadow_merge(
    primitives: &[usvg::filter::Primitive],
) -> Option<SvgRasterPixelEffect> {
    let [
        blur_primitive,
        offset_primitive,
        flood_primitive,
        composite_primitive,
        merge_primitive,
    ] = primitives
    else {
        return None;
    };
    let usvg::filter::Kind::GaussianBlur(blur) = blur_primitive.kind() else {
        return None;
    };
    let usvg::filter::Kind::Offset(offset) = offset_primitive.kind() else {
        return None;
    };
    let usvg::filter::Kind::Flood(flood) = flood_primitive.kind() else {
        return None;
    };
    let usvg::filter::Kind::Composite(composite) = composite_primitive.kind() else {
        return None;
    };
    let usvg::filter::Kind::Merge(merge) = merge_primitive.kind() else {
        return None;
    };
    let std_deviation = (blur.std_dev_x().get() + blur.std_dev_y().get()) * 0.5;
    (std_deviation.is_finite()
        && std_deviation >= 0.0
        && offset.dx().is_finite()
        && offset.dy().is_finite()
        && matches!(blur.input(), usvg::filter::Input::SourceGraphic)
        && matches!(offset.input(), usvg::filter::Input::Reference(name) if name == blur_primitive.result())
        && composite.operator() == usvg::filter::CompositeOperator::In
        && matches!(composite.input1(), usvg::filter::Input::Reference(name) if name == flood_primitive.result())
        && matches!(composite.input2(), usvg::filter::Input::Reference(name) if name == offset_primitive.result())
        && matches!(merge.inputs(), [usvg::filter::Input::Reference(name), usvg::filter::Input::SourceGraphic] if name == composite_primitive.result()))
    .then(|| SvgRasterPixelEffect::DropShadow {
        std_deviation,
        dx: offset.dx(),
        dy: offset.dy(),
        color: svg_color(flood.color(), flood.opacity().get()),
    })
}

/// Compile a named linear result followed by `feComposite` with
/// `SourceGraphic` as its second input. This is the first general retained
/// graph edge: the working result and immutable source surface are explicit,
/// rather than inferred from primitive order.
fn svg_source_graphic_composite_effects(
    primitives: &[usvg::filter::Primitive],
) -> Option<Vec<SvgRasterPixelEffect>> {
    let (prefix, [composite_primitive]) =
        primitives.split_at_checked(primitives.len().checked_sub(1)?)?
    else {
        return None;
    };
    let usvg::filter::Kind::Composite(composite) = composite_primitive.kind() else {
        return None;
    };
    let previous = prefix.last()?;
    let source_as_second = match (composite.input1(), composite.input2()) {
        (usvg::filter::Input::Reference(name), usvg::filter::Input::SourceGraphic)
            if name == previous.result() =>
        {
            true
        }
        (usvg::filter::Input::SourceGraphic, usvg::filter::Input::Reference(name))
            if name == previous.result() =>
        {
            false
        }
        _ => return None,
    };
    Some({
        let mut effects = svg_linear_pixel_effects(prefix)?;
        effects.push(SvgRasterPixelEffect::CompositeWithSourceGraphic {
            operator: composite.operator(),
            source_as_second,
        });
        effects
    })
}

/// Compile a named linear result followed by `feComposite` with `SourceAlpha`
/// as its second input.
fn svg_source_alpha_composite_effects(
    primitives: &[usvg::filter::Primitive],
) -> Option<Vec<SvgRasterPixelEffect>> {
    let (prefix, [composite_primitive]) =
        primitives.split_at_checked(primitives.len().checked_sub(1)?)?
    else {
        return None;
    };
    let usvg::filter::Kind::Composite(composite) = composite_primitive.kind() else {
        return None;
    };
    let previous = prefix.last()?;
    let source_as_second = match (composite.input1(), composite.input2()) {
        (usvg::filter::Input::Reference(name), usvg::filter::Input::SourceAlpha)
            if name == previous.result() =>
        {
            true
        }
        (usvg::filter::Input::SourceAlpha, usvg::filter::Input::Reference(name))
            if name == previous.result() =>
        {
            false
        }
        _ => return None,
    };
    Some({
        let mut effects = svg_linear_pixel_effects(prefix)?;
        effects.push(SvgRasterPixelEffect::CompositeWithSourceAlpha {
            operator: composite.operator(),
            source_as_second,
        });
        effects
    })
}

/// Recognize a strictly linear sequence of in-place color transforms.
///
/// `usvg` has already normalized an omitted `in` to the previous primitive's
/// `result`. Accepting only that exact dependency means every step can mutate
/// the one retained `SourceGraphic` surface in order. A later explicit
/// `SourceGraphic`, `SourceAlpha`, or any branch is intentionally rejected:
/// it needs a graph compositor with more than one input surface.
fn svg_linear_pixel_effects(
    primitives: &[usvg::filter::Primitive],
) -> Option<Vec<SvgRasterPixelEffect>> {
    if primitives.is_empty() {
        return None;
    }
    let mut effects = Vec::with_capacity(primitives.len());
    let mut previous_result: Option<&str> = None;
    for primitive in primitives {
        let input = match primitive.kind() {
            usvg::filter::Kind::GaussianBlur(blur) => blur.input(),
            usvg::filter::Kind::DropShadow(shadow) => shadow.input(),
            usvg::filter::Kind::Offset(offset) => offset.input(),
            usvg::filter::Kind::Morphology(morphology) => morphology.input(),
            usvg::filter::Kind::ConvolveMatrix(matrix) => matrix.input(),
            usvg::filter::Kind::ColorMatrix(color_matrix) => color_matrix.input(),
            usvg::filter::Kind::ComponentTransfer(transfer) => transfer.input(),
            _ => return None,
        };
        let is_expected_input = match previous_result {
            None => matches!(input, usvg::filter::Input::SourceGraphic),
            Some(previous) => {
                matches!(input, usvg::filter::Input::Reference(name) if name == previous)
            }
        };
        if !is_expected_input {
            return None;
        }
        let linear_rgb =
            primitive.color_interpolation() == usvg::filter::ColorInterpolation::LinearRGB;
        let effect = match primitive.kind() {
            usvg::filter::Kind::GaussianBlur(blur) => {
                let std_deviation = (blur.std_dev_x().get() + blur.std_dev_y().get()) * 0.5;
                (std_deviation.is_finite() && std_deviation >= 0.0)
                    .then_some(SvgRasterPixelEffect::GaussianBlur { std_deviation })?
            }
            usvg::filter::Kind::DropShadow(shadow)
                if shadow.dx().is_finite() && shadow.dy().is_finite() =>
            {
                let std_deviation = (shadow.std_dev_x().get() + shadow.std_dev_y().get()) * 0.5;
                (std_deviation.is_finite() && std_deviation >= 0.0).then(|| {
                    SvgRasterPixelEffect::DropShadow {
                        std_deviation,
                        dx: shadow.dx(),
                        dy: shadow.dy(),
                        color: svg_color(shadow.color(), shadow.opacity().get()),
                    }
                })?
            }
            usvg::filter::Kind::Offset(offset)
                if offset.dx().is_finite() && offset.dy().is_finite() =>
            {
                SvgRasterPixelEffect::Offset {
                    dx: offset.dx(),
                    dy: offset.dy(),
                }
            }
            usvg::filter::Kind::Morphology(morphology) => {
                let radius_x = morphology.radius_x().get();
                let radius_y = morphology.radius_y().get();
                (radius_x.is_finite() && radius_y.is_finite()).then_some(
                    SvgRasterPixelEffect::Morphology {
                        radius_x,
                        radius_y,
                        dilate: morphology.operator() == usvg::filter::MorphologyOperator::Dilate,
                    },
                )?
            }
            usvg::filter::Kind::ColorMatrix(color_matrix) => SvgRasterPixelEffect::ColorMatrix {
                matrix: svg_color_matrix_values(color_matrix.kind())?,
                linear_rgb,
            },
            usvg::filter::Kind::ComponentTransfer(transfer) => {
                SvgRasterPixelEffect::ComponentTransfer {
                    functions: [
                        svg_transfer_function(transfer.func_r())?,
                        svg_transfer_function(transfer.func_g())?,
                        svg_transfer_function(transfer.func_b())?,
                        svg_transfer_function(transfer.func_a())?,
                    ],
                    linear_rgb,
                }
            }
            usvg::filter::Kind::ConvolveMatrix(convolve) => {
                let matrix = convolve.matrix();
                let values = matrix.data();
                let divisor = convolve.divisor().get();
                (values.len() <= 4096
                    && values.iter().all(|value| value.is_finite())
                    && divisor.is_finite()
                    && convolve.bias().is_finite())
                .then(|| SvgRasterPixelEffect::ConvolveMatrix {
                    matrix: values.to_vec(),
                    columns: matrix.columns(),
                    rows: matrix.rows(),
                    target_x: matrix.target_x(),
                    target_y: matrix.target_y(),
                    divisor,
                    bias: convolve.bias(),
                    edge_mode: convolve.edge_mode(),
                    preserve_alpha: convolve.preserve_alpha(),
                    linear_rgb,
                })?
            }
            _ => unreachable!("input kind was filtered above"),
        };
        effects.push(effect);
        previous_result = Some(primitive.result());
    }
    Some(effects)
}

fn svg_transfer_function(function: &usvg::filter::TransferFunction) -> Option<SvgTransferFunction> {
    match function {
        usvg::filter::TransferFunction::Identity => Some(SvgTransferFunction::Identity),
        usvg::filter::TransferFunction::Table(values) => values
            .iter()
            .all(|value| value.is_finite())
            .then(|| SvgTransferFunction::Table(values.clone())),
        usvg::filter::TransferFunction::Discrete(values) => values
            .iter()
            .all(|value| value.is_finite())
            .then(|| SvgTransferFunction::Discrete(values.clone())),
        usvg::filter::TransferFunction::Linear { slope, intercept }
            if slope.is_finite() && intercept.is_finite() =>
        {
            Some(SvgTransferFunction::Linear {
                slope: *slope,
                intercept: *intercept,
            })
        }
        usvg::filter::TransferFunction::Gamma {
            amplitude,
            exponent,
            offset,
        } if amplitude.is_finite() && exponent.is_finite() && offset.is_finite() => {
            Some(SvgTransferFunction::Gamma {
                amplitude: *amplitude,
                exponent: *exponent,
                offset: *offset,
            })
        }
        _ => None,
    }
}

fn svg_color_matrix_values(kind: &usvg::filter::ColorMatrixKind) -> Option<[f32; 20]> {
    match kind {
        usvg::filter::ColorMatrixKind::Matrix(values) => values
            .as_slice()
            .try_into()
            .ok()
            .filter(|matrix: &[f32; 20]| matrix.iter().all(|value| value.is_finite())),
        usvg::filter::ColorMatrixKind::Saturate(amount) => {
            let amount = amount.get();
            amount.is_finite().then_some({
                [
                    0.213 + 0.787 * amount,
                    0.715 - 0.715 * amount,
                    0.072 - 0.072 * amount,
                    0.0,
                    0.0,
                    0.213 - 0.213 * amount,
                    0.715 + 0.285 * amount,
                    0.072 - 0.072 * amount,
                    0.0,
                    0.0,
                    0.213 - 0.213 * amount,
                    0.715 - 0.715 * amount,
                    0.072 + 0.928 * amount,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                ]
            })
        }
        usvg::filter::ColorMatrixKind::HueRotate(angle) => {
            if !angle.is_finite() {
                return None;
            }
            let cosine = angle.to_radians().cos();
            let sine = angle.to_radians().sin();
            Some([
                0.213 + cosine * 0.787 - sine * 0.213,
                0.715 - cosine * 0.715 - sine * 0.715,
                0.072 - cosine * 0.072 + sine * 0.928,
                0.0,
                0.0,
                0.213 - cosine * 0.213 + sine * 0.143,
                0.715 + cosine * 0.285 + sine * 0.140,
                0.072 - cosine * 0.072 - sine * 0.283,
                0.0,
                0.0,
                0.213 - cosine * 0.213 - sine * 0.787,
                0.715 - cosine * 0.715 + sine * 0.715,
                0.072 + cosine * 0.928 + sine * 0.072,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ])
        }
        usvg::filter::ColorMatrixKind::LuminanceToAlpha => Some([
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.2125,
            0.7154, 0.0721, 0.0, 0.0,
        ]),
    }
}

/// Rasterize a filterable SVG group and preserve its ordered source text as
/// the image's semantic replacement.  A filtered SVG subtree cannot retain a
/// native PDF text operation without duplicating its unfiltered ink, so the
/// image's `/ActualText` is the only accessible representation.
fn rasterize_svg_filtered_group(
    group: SvgPaintGroup,
    filter: SvgRasterFilter,
    filter_transform: PaintTransform,
) -> SvgPaintGroup {
    let Some((mut paths, actual_text)) = svg_effect_paths_and_text(group) else {
        return SvgPaintGroup::empty();
    };
    let scale = (filter_transform.a() * filter_transform.d()
        - filter_transform.b() * filter_transform.c())
    .abs()
    .sqrt();
    if !scale.is_finite() || scale <= 0.0 {
        return SvgPaintGroup::empty();
    }
    let (blur_radius, pixel_effects, region) = match filter {
        SvgRasterFilter::PixelEffects { effects, region } => (
            0.0,
            svg_pixel_effects_in_paint_space(effects, filter_transform),
            region,
        ),
        SvgRasterFilter::FloodInSourceAlpha { color, region } => (
            0.0,
            vec![SvgRasterPixelEffect::FloodInSourceAlpha { color }],
            region,
        ),
        SvgRasterFilter::GaussianBlur {
            std_deviation,
            region,
        } => (std_deviation * scale, Vec::new(), region),
        SvgRasterFilter::Offset { dx, dy, region } => {
            let offset = PaintTranslation::new(
                filter_transform.a() * dx + filter_transform.c() * dy,
                filter_transform.b() * dx + filter_transform.d() * dy,
            );
            paths = paths
                .into_iter()
                .map(|path| path.transformed(PaintTransform::translate(offset)))
                .collect();
            (0.0, Vec::new(), region)
        }
        SvgRasterFilter::ColorMatrix {
            matrix,
            linear_rgb,
            region,
        } => (
            0.0,
            vec![SvgRasterPixelEffect::ColorMatrix { matrix, linear_rgb }],
            region,
        ),
        SvgRasterFilter::ComponentTransfer {
            functions,
            linear_rgb,
            region,
        } => (
            0.0,
            vec![SvgRasterPixelEffect::ComponentTransfer {
                functions,
                linear_rgb,
            }],
            region,
        ),
        SvgRasterFilter::Morphology {
            radius_x,
            radius_y,
            dilate,
            region,
        } => {
            let radius_x = radius_x * scale;
            let radius_y = radius_y * scale;
            if !(0.0..=256.0).contains(&(radius_x * SVG_EFFECT_RASTER_SCALE).round())
                || !(0.0..=256.0).contains(&(radius_y * SVG_EFFECT_RASTER_SCALE).round())
            {
                log::warn!("skipping SVG morphology with an effect radius over 256 pixels");
                return SvgPaintGroup::empty();
            }
            (
                0.0,
                vec![SvgRasterPixelEffect::Morphology {
                    radius_x,
                    radius_y,
                    dilate,
                }],
                region,
            )
        }
    };
    let Some(image) = rasterize_svg_solid_paths_with_effect(&paths, blur_radius, &pixel_effects)
    else {
        return SvgPaintGroup::empty();
    };
    let mut result = SvgPaintGroup::empty();
    result.items.push(SvgPaintItem::RasterImage(Box::new(
        image
            .with_intersected_clip(svg_filter_clip_path(region, filter_transform))
            .with_actual_text(Rc::from(actual_text)),
    )));
    result
}

/// Convert linear filter effects from normalized SVG user units to the paint
/// units of their bounded raster surface. Color transforms are unitless;
/// Gaussian standard deviations use the same geometric-mean affine scale as
/// the single-primitive filter path.
fn svg_pixel_effects_in_paint_space(
    effects: Vec<SvgRasterPixelEffect>,
    filter_transform: PaintTransform,
) -> Vec<SvgRasterPixelEffect> {
    let scale = (filter_transform.a() * filter_transform.d()
        - filter_transform.b() * filter_transform.c())
    .abs()
    .sqrt();
    effects
        .into_iter()
        .map(|effect| match effect {
            SvgRasterPixelEffect::GaussianBlur { std_deviation } => {
                SvgRasterPixelEffect::GaussianBlur {
                    std_deviation: std_deviation * scale,
                }
            }
            SvgRasterPixelEffect::DropShadow {
                std_deviation,
                dx,
                dy,
                color,
            } => SvgRasterPixelEffect::DropShadow {
                std_deviation: std_deviation * scale,
                dx: filter_transform.a() * dx + filter_transform.c() * dy,
                dy: filter_transform.b() * dx + filter_transform.d() * dy,
                color,
            },
            SvgRasterPixelEffect::Offset { dx, dy } => SvgRasterPixelEffect::Offset {
                dx: filter_transform.a() * dx + filter_transform.c() * dy,
                dy: filter_transform.b() * dx + filter_transform.d() * dy,
            },
            SvgRasterPixelEffect::Morphology {
                radius_x,
                radius_y,
                dilate,
            } => SvgRasterPixelEffect::Morphology {
                radius_x: radius_x * scale,
                radius_y: radius_y * scale,
                dilate,
            },
            effect @ SvgRasterPixelEffect::ConvolveMatrix { .. } => effect,
            effect @ SvgRasterPixelEffect::CompositeWithSourceGraphic { .. } => effect,
            effect @ SvgRasterPixelEffect::CompositeWithSourceAlpha { .. } => effect,
            effect => effect,
        })
        .collect()
}

/// Rasterize the bounded retained solid-path subset of an SVG mask.
///
/// SVG masks affect the composited subtree, so native PDF text cannot remain
/// alongside an image of its masked ink. Text is forced to the document-shaped
/// outline stream before entering this function and the resulting image owns
/// the source-order `/ActualText` replacement. This first compositor slice
/// intentionally accepts only flat solid paths; gradients, images, nested
/// masks, and filter-bearing mask content stay unsupported rather than being
/// painted as an unmasked approximation.
/// <https://www.w3.org/TR/SVG2/masking.html#MaskElement>
fn rasterize_svg_masked_group(
    source: SvgPaintGroup,
    mask: SvgPaintGroup,
    mask_type: usvg::MaskType,
) -> SvgPaintGroup {
    let Some((source_paths, actual_text)) = svg_effect_paths_and_text(source) else {
        return SvgPaintGroup::empty();
    };
    let Some((mask_paths, _)) = svg_effect_paths_and_text(mask) else {
        return SvgPaintGroup::empty();
    };
    let Some(image) = rasterize_svg_solid_paths_with_mask(&source_paths, &mask_paths, mask_type)
    else {
        return SvgPaintGroup::empty();
    };
    let image = if actual_text.is_empty() {
        image
    } else {
        image.with_actual_text(Rc::from(actual_text))
    };
    SvgPaintGroup {
        items: vec![SvgPaintItem::RasterImage(Box::new(image))],
        ..SvgPaintGroup::empty()
    }
}

/// Composite a solid retained SVG source against a solid retained mask into
/// one alpha-bearing PDF image. The source bounds are the observable image
/// extent, while mask samples outside those bounds are transparent black.
fn rasterize_svg_solid_paths_with_mask(
    source_paths: &[RenderedPath],
    mask_paths: &[RenderedPath],
    mask_type: usvg::MaskType,
) -> Option<RenderedImage> {
    let mut bounds = svg_paths_bounds(source_paths)?;
    let max_stroke = source_paths
        .iter()
        .chain(mask_paths)
        .map(|path| path.stroke_width.points())
        .filter(|width| width.is_finite())
        .fold(0.0_f32, f32::max);
    let padding = max_stroke * 0.5 + 1.0;
    bounds = PaintRect::new(
        PaintPoint::new(bounds.origin.x - padding, bounds.origin.y - padding),
        PaintSize::new(
            bounds.size.width + padding * 2.0,
            bounds.size.height + padding * 2.0,
        ),
    );
    let width = (bounds.size.width * SVG_EFFECT_RASTER_SCALE).ceil() as u64;
    let height = (bounds.size.height * SVG_EFFECT_RASTER_SCALE).ceil() as u64;
    if width == 0
        || height == 0
        || width > u32::MAX as u64
        || height > u32::MAX as u64
        || width.saturating_mul(height) > MAX_SVG_EFFECT_PIXELS
    {
        return None;
    }
    let mut source = tiny_skia::Pixmap::new(width as u32, height as u32)?;
    let mut mask = tiny_skia::Pixmap::new(width as u32, height as u32)?;
    for path in source_paths {
        rasterize_svg_path(&mut source, path, bounds)?;
    }
    for path in mask_paths {
        rasterize_svg_path(&mut mask, path, bounds)?;
    }
    let (source_pixels, source_remainder) = source.data_mut().as_chunks_mut::<4>();
    let (mask_pixels, mask_remainder) = mask.data().as_chunks::<4>();
    debug_assert!(source_remainder.is_empty(), "SVG pixels have four channels");
    debug_assert!(mask_remainder.is_empty(), "SVG pixels have four channels");
    for (source_pixel, mask_pixel) in source_pixels.iter_mut().zip(mask_pixels) {
        let alpha = match mask_type {
            usvg::MaskType::Alpha => mask_pixel[3] as f32 / 255.0,
            usvg::MaskType::Luminance => {
                let alpha = mask_pixel[3] as f32 / 255.0;
                if alpha <= 0.0 {
                    0.0
                } else {
                    let red = mask_pixel[0] as f32 / 255.0 / alpha;
                    let green = mask_pixel[1] as f32 / 255.0 / alpha;
                    let blue = mask_pixel[2] as f32 / 255.0 / alpha;
                    (0.2126 * red + 0.7152 * green + 0.0722 * blue).clamp(0.0, 1.0) * alpha
                }
            }
        };
        for channel in source_pixel {
            *channel = (*channel as f32 * alpha).round() as u8;
        }
    }
    let rgba = source.take_demultiplied();
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    let mut alpha = Vec::with_capacity((width * height) as usize);
    let (pixels, trailing) = rgba.as_chunks::<4>();
    debug_assert!(trailing.is_empty(), "RGBA pixmap has whole pixels");
    for &[red, green, blue, opacity] in pixels {
        rgb.extend_from_slice(&[red, green, blue]);
        alpha.push(opacity);
    }
    Some(RenderedImage::from_paint_rect(
        bounds,
        false,
        width as u32,
        height as u32,
        None,
        true,
        Rc::from(rgb),
        Some(Rc::from(alpha)),
        None,
    ))
}

/// Flatten a self-contained effect source only when every item can be
/// rasterized from the retained scene without losing a non-text resource.
fn svg_effect_paths_and_text(group: SvgPaintGroup) -> Option<(Vec<RenderedPath>, String)> {
    if group.opacity != 1.0
        || group.blend_mode != PaintBlendMode::Normal
        || group.isolation
        || group.bounds.is_some()
    {
        return None;
    }
    let mut paths = Vec::new();
    let mut actual_text = String::new();
    for item in group.items {
        match item {
            SvgPaintItem::Path(path) => paths.push(*path),
            SvgPaintItem::OutlinedText(outlined) => {
                paths.extend(outlined.paths);
                actual_text.push_str(&outlined.actual_text);
            }
            SvgPaintItem::Group(group) | SvgPaintItem::NestedSvg(group) => {
                let (child_paths, child_text) = svg_effect_paths_and_text(*group)?;
                paths.extend(child_paths);
                actual_text.push_str(&child_text);
            }
            // Text must have been forced to document-shaped outlines before
            // effect collection; raster images require their own sampler.
            SvgPaintItem::Text(_) | SvgPaintItem::RasterImage(_) => return None,
        }
    }
    (!paths.is_empty()).then_some((paths, actual_text))
}

#[derive(Debug, Clone, Default)]
struct SvgFilterTaintCatalog {
    by_filter_id: HashMap<String, Vec<SvgFilterPrimitiveTaint>>,
}

#[derive(Debug, Clone)]
struct SvgFilterPrimitiveTaint {
    tag: String,
    color_tainted: Option<bool>,
    has_unsupported_standard_input: bool,
    declared_inputs: Vec<String>,
}

fn filter_taint_catalog(
    root: &Element,
    overrides: &SvgPresentationOverrides,
) -> SvgFilterTaintCatalog {
    fn visit(
        element: &Element,
        overrides: &SvgPresentationOverrides,
        catalog: &mut SvgFilterTaintCatalog,
    ) {
        if element.namespace_url == SVG_NAMESPACE
            && element.tag == "filter"
            && let Some(id) = inline_svg_unprefixed_attribute(element, "id")
        {
            let primitives = element
                .children
                .iter()
                .filter_map(|child| match &child.kind {
                    NodeKind::Element(child) if child.namespace_url == SVG_NAMESPACE => Some(child),
                    _ => None,
                })
                .map(|primitive| {
                    let override_values = overrides.get(&primitive.id);
                    let color_tainted = match primitive.tag.as_str() {
                        "feFlood" | "feDropShadow" => override_values
                            .and_then(|values| values.flood_color)
                            .map(|color| color.current_color_dependent),
                        "feDiffuseLighting" | "feSpecularLighting" => override_values
                            .and_then(|values| values.lighting_color)
                            .map(|color| color.current_color_dependent),
                        _ => None,
                    };
                    let has_unsupported_standard_input = ["in", "in2"].into_iter().any(|name| {
                        inline_svg_unprefixed_attribute(primitive, name).is_some_and(|input| {
                            matches!(
                                input,
                                "BackgroundImage" | "BackgroundAlpha" | "FillPaint" | "StrokePaint"
                            )
                        })
                    });
                    let declared_inputs = ["in", "in2"]
                        .into_iter()
                        .filter_map(|name| {
                            inline_svg_unprefixed_attribute(primitive, name).map(str::to_owned)
                        })
                        .collect();
                    SvgFilterPrimitiveTaint {
                        tag: primitive.tag.clone(),
                        color_tainted,
                        has_unsupported_standard_input,
                        declared_inputs,
                    }
                })
                .collect();
            catalog.by_filter_id.insert(id.to_owned(), primitives);
        }
        for child in &element.children {
            if let NodeKind::Element(child) = &child.kind {
                visit(child, overrides, catalog);
            }
        }
    }

    let mut catalog = SvgFilterTaintCatalog::default();
    visit(root, overrides, &mut catalog);
    catalog
}

fn analyze_svg_filters(
    filters: &[Arc<usvg::filter::Filter>],
    catalog: &SvgFilterTaintCatalog,
) -> SvgFilterAnalysis {
    if filters.is_empty() {
        return SvgFilterAnalysis::ExactSourceGraphic { filter_clip: None };
    }

    let mut clip = None;
    for filter in filters {
        let Some(metadata) = catalog.by_filter_id.get(filter.id()) else {
            return SvgFilterAnalysis::RequiresRasterBackend;
        };
        let primitives = filter.primitives();
        if primitives.len() != metadata.len() {
            return SvgFilterAnalysis::RequiresRasterBackend;
        }
        let mut results: Vec<(String, bool, Option<usvg::NonZeroRect>)> = Vec::new();
        for (primitive, metadata) in primitives.iter().zip(metadata) {
            if metadata.has_unsupported_standard_input
                || !filter_kind_matches_tag(primitive.kind(), &metadata.tag)
                || matches!(primitive.kind(), usvg::filter::Kind::Merge(_))
                || !declared_filter_inputs_are_resolved(&metadata.declared_inputs, &results)
            {
                return SvgFilterAnalysis::RequiresRasterBackend;
            }
            let input_tainted = primitive_input_tainted(primitive.kind(), &results);
            let color_tainted = match primitive.kind() {
                usvg::filter::Kind::Flood(_) | usvg::filter::Kind::DropShadow(_) => {
                    let Some(tainted) = metadata.color_tainted else {
                        return SvgFilterAnalysis::RequiresRasterBackend;
                    };
                    tainted
                }
                usvg::filter::Kind::DiffuseLighting(_)
                | usvg::filter::Kind::SpecularLighting(_) => {
                    let Some(tainted) = metadata.color_tainted else {
                        return SvgFilterAnalysis::RequiresRasterBackend;
                    };
                    tainted
                }
                // `<feImage>` depends on CORS mode and resource provenance,
                // neither of which is represented in the static SVG adapter.
                usvg::filter::Kind::Image(_) => return SvgFilterAnalysis::RequiresRasterBackend,
                _ => false,
            };
            let tainted = input_tainted || color_tainted;
            let exact_clip = if let usvg::filter::Kind::DisplacementMap(map) = primitive.kind() {
                let input = exact_source_graphic_clip(map.input1(), &results, filter.rect());
                let tainted_input = filter_input_tainted(map.input2(), &results);
                if tainted_input {
                    input.and_then(|clip| intersect_filter_rects(clip, primitive.rect()))
                } else {
                    None
                }
            } else {
                None
            };
            results.push((primitive.result().to_owned(), tainted, exact_clip));
        }
        let Some((_, _, Some(filter_clip))) = results.last() else {
            return SvgFilterAnalysis::RequiresRasterBackend;
        };
        clip = Some(match clip {
            Some(existing) => match intersect_filter_rects(existing, *filter_clip) {
                Some(intersection) => intersection,
                None => return SvgFilterAnalysis::RequiresRasterBackend,
            },
            None => *filter_clip,
        });
    }
    SvgFilterAnalysis::ExactSourceGraphic { filter_clip: clip }
}

fn declared_filter_inputs_are_resolved(
    declared_inputs: &[String],
    results: &[(String, bool, Option<usvg::NonZeroRect>)],
) -> bool {
    declared_inputs.iter().all(|input| {
        matches!(input.as_str(), "SourceGraphic" | "SourceAlpha")
            || results.iter().any(|(result, _, _)| result == input)
    })
}

fn filter_kind_matches_tag(kind: &usvg::filter::Kind, tag: &str) -> bool {
    matches!(
        (kind, tag),
        (usvg::filter::Kind::Blend(_), "feBlend")
            | (usvg::filter::Kind::ColorMatrix(_), "feColorMatrix")
            | (
                usvg::filter::Kind::ComponentTransfer(_),
                "feComponentTransfer"
            )
            | (usvg::filter::Kind::Composite(_), "feComposite")
            | (usvg::filter::Kind::ConvolveMatrix(_), "feConvolveMatrix")
            | (usvg::filter::Kind::DiffuseLighting(_), "feDiffuseLighting")
            | (usvg::filter::Kind::DisplacementMap(_), "feDisplacementMap")
            | (usvg::filter::Kind::DropShadow(_), "feDropShadow")
            | (usvg::filter::Kind::Flood(_), "feFlood")
            | (usvg::filter::Kind::GaussianBlur(_), "feGaussianBlur")
            | (usvg::filter::Kind::Image(_), "feImage")
            | (usvg::filter::Kind::Merge(_), "feMerge")
            | (usvg::filter::Kind::Morphology(_), "feMorphology")
            | (usvg::filter::Kind::Offset(_), "feOffset")
            | (
                usvg::filter::Kind::SpecularLighting(_),
                "feSpecularLighting"
            )
            | (usvg::filter::Kind::Tile(_), "feTile")
            | (usvg::filter::Kind::Turbulence(_), "feTurbulence")
    )
}

fn primitive_input_tainted(
    kind: &usvg::filter::Kind,
    results: &[(String, bool, Option<usvg::NonZeroRect>)],
) -> bool {
    kind.has_input(&usvg::filter::Input::SourceGraphic)
        || kind.has_input(&usvg::filter::Input::SourceAlpha)
        || results.iter().any(|(result, tainted, _)| {
            *tainted && kind.has_input(&usvg::filter::Input::Reference(result.clone()))
        })
}

fn filter_input_tainted(
    input: &usvg::filter::Input,
    results: &[(String, bool, Option<usvg::NonZeroRect>)],
) -> bool {
    match input {
        usvg::filter::Input::SourceGraphic | usvg::filter::Input::SourceAlpha => true,
        usvg::filter::Input::Reference(reference) => results
            .iter()
            .rev()
            .find(|(result, _, _)| result == reference)
            .is_some_and(|(_, tainted, _)| *tainted),
    }
}

fn exact_source_graphic_clip(
    input: &usvg::filter::Input,
    results: &[(String, bool, Option<usvg::NonZeroRect>)],
    filter_rect: usvg::NonZeroRect,
) -> Option<usvg::NonZeroRect> {
    match input {
        usvg::filter::Input::SourceGraphic => Some(filter_rect),
        usvg::filter::Input::SourceAlpha => None,
        usvg::filter::Input::Reference(reference) => results
            .iter()
            .rev()
            .find(|(result, _, _)| result == reference)
            .and_then(|(_, _, clip)| *clip),
    }
}

fn intersect_filter_rects(
    left: usvg::NonZeroRect,
    right: usvg::NonZeroRect,
) -> Option<usvg::NonZeroRect> {
    let x1 = left.x().max(right.x());
    let y1 = left.y().max(right.y());
    let x2 = (left.x() + left.width()).min(right.x() + right.width());
    let y2 = (left.y() + left.height()).min(right.y() + right.height());
    usvg::NonZeroRect::from_xywh(x1, y1, x2 - x1, y2 - y1)
}

fn svg_filter_clip_path(rect: usvg::NonZeroRect, transform: PaintTransform) -> RenderedPathClip {
    let points = [
        PaintPoint::new(rect.x(), rect.y()),
        PaintPoint::new(rect.x() + rect.width(), rect.y()),
        PaintPoint::new(rect.x() + rect.width(), rect.y() + rect.height()),
        PaintPoint::new(rect.x(), rect.y() + rect.height()),
    ];
    let commands = points
        .into_iter()
        .enumerate()
        .map(|(index, point)| {
            if index == 0 {
                RenderedPathCommand::MoveTo(transform.apply_point(point))
            } else {
                RenderedPathCommand::LineTo(transform.apply_point(point))
            }
        })
        .chain(std::iter::once(RenderedPathCommand::Close))
        .collect();
    RenderedPathClip::new(commands, RenderedPathFillRule::NonZero, Vec::new())
}

/// Lower a normalized SVG `<image>` without flattening its parent scene.
///
/// `usvg::Image::abs_transform` already contains SVG's concrete object size
/// and `preserveAspectRatio` placement.  The extra local vertical flip bridges
/// an image XObject's bottom-left sample coordinates to SVG's top-left image
/// coordinates before the root SVG viewport crosses into PDF paint space.
/// <https://www.w3.org/TR/SVG2/embedded.html#ImageElement>
fn render_svg_image(
    image: &usvg::Image,
    image_transform: usvg::Transform,
    viewport: ViewportTransform,
    additional_clips: &[RenderedPathClipPath],
    font_system: &mut Option<&mut FontSystem>,
) -> Option<SvgPaintItem> {
    if !image.is_visible() {
        return None;
    }
    let size = image.size();
    let width = size.width();
    let height = size.height();
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let image_to_paint = svg_path_transform(image_transform, viewport)
        .multiply(PaintTransform::new(1.0, 0.0, 0.0, -1.0, 0.0, height));
    match image.kind() {
        usvg::ImageKind::JPEG(bytes)
        | usvg::ImageKind::PNG(bytes)
        | usvg::ImageKind::GIF(bytes)
        | usvg::ImageKind::WEBP(bytes) => {
            let raster = crate::image_store::decode_embedded_raster(Rc::from(bytes.as_slice()))?;
            let mut rendered = RenderedImage::from_paint_rect(
                PaintRect::new(PaintPoint::new(0.0, 0.0), PaintSize::new(width, height)),
                false,
                raster.metadata.pixel_size.width,
                raster.metadata.pixel_size.height,
                Some(RenderedImageSourceRect {
                    x: 0,
                    y: 0,
                    width: raster.metadata.pixel_size.width,
                    height: raster.metadata.pixel_size.height,
                }),
                !matches!(
                    image.rendering_mode(),
                    usvg::ImageRendering::CrispEdges | usvg::ImageRendering::Pixelated
                ),
                Rc::from(raster.rgb),
                raster.alpha.map(Rc::from),
                None,
            )
            .with_raster_sample_depth(raster.sample_depth)
            .with_transform(image_to_paint);
            if viewport.clip_viewport {
                rendered = rendered.with_intersected_clip(viewport_clip(viewport));
            }
            for clip in additional_clips {
                rendered = rendered.with_intersected_clip(RenderedPathClip::new(
                    clip.commands.clone(),
                    clip.fill_rule,
                    Vec::new(),
                ));
            }
            Some(SvgPaintItem::RasterImage(Box::new(rendered)))
        }
        usvg::ImageKind::SVG(tree) => {
            let source = SvgSourceRect::new(
                SvgSourcePoint::new(0.0, 0.0),
                SvgSourceSize::new(width, height),
            );
            let local_viewport = ViewportTransform::new(
                PaintRect::new(PaintPoint::new(0.0, 0.0), PaintSize::new(width, height)),
                source,
                true,
                false,
            );
            let nested_text_typography = HashMap::new();
            let mut scene = collect_svg_group_with_font_system(
                tree.root(),
                local_viewport,
                &[],
                usvg::Transform::default(),
                &SvgFilterTaintCatalog::default(),
                &nested_text_typography,
                font_system,
            )
            .transformed(image_to_paint);
            if viewport.clip_viewport {
                scene = scene.with_clip(viewport_clip(viewport));
            }
            for clip in additional_clips {
                scene = scene.with_clip(RenderedPathClip::new(
                    clip.commands.clone(),
                    clip.fill_rule,
                    Vec::new(),
                ));
            }
            Some(SvgPaintItem::NestedSvg(Box::new(scene)))
        }
    }
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
        primary_clip_is_viewport: viewport.hard_crop_viewport
            && viewport.clip_viewport
            && additional_clips.is_empty(),
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
            primary_clip_is_viewport: viewport.hard_crop_viewport
                && viewport.clip_viewport
                && additional_clips.is_empty(),
        })
    });

    let mut rendered =
        Vec::with_capacity(usize::from(fill.is_some()) + usize::from(stroke.is_some()));
    match path.paint_order() {
        usvg::PaintOrder::FillAndStroke => {
            rendered.extend(fill.and_then(SvgPaintOperation::materialize));
            rendered.extend(stroke.and_then(SvgPaintOperation::materialize));
        }
        usvg::PaintOrder::StrokeAndFill => {
            rendered.extend(stroke.and_then(SvgPaintOperation::materialize));
            rendered.extend(fill.and_then(SvgPaintOperation::materialize));
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
    // A pattern cell has its own user-coordinate system.  It deliberately
    // does not cross the SVG-root y-axis boundary here: the target path's
    // paint-server transform applies that mapping exactly once.
    let cell_viewport = ViewportTransform {
        destination: PaintRect::new(
            PaintPoint::new(0.0, 0.0),
            PaintSize::new(tile_width, tile_height),
        ),
        source_to_paint: SvgSourceToPaintTransform::new(1.0, 1.0, 0.0, 0.0),
        clip_viewport: false,
        hard_crop_viewport: false,
    };
    let scene = collect_svg_group(
        pattern.root(),
        cell_viewport,
        &[],
        usvg::Transform::default(),
        &SvgFilterTaintCatalog::default(),
    );
    Some(RenderedSvgPathPattern {
        tile_size: PaintSize::new(tile_width, tile_height),
        origin: PaintPoint::new(rect.x(), rect.y()),
        transform: svg_gradient_transform(pattern.transform()),
        scene: Box::new(scene),
        opacity,
    })
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
    external_uses: &ExternalSvgUseResolver,
) -> Result<SvgAsset, String> {
    let xml = external_uses.expand_inline_svg(serialize_inline_svg_with_presentation_overrides(
        element, overrides,
    ));
    parse_svg_bytes_with_filter_taint_and_typography(
        xml.as_bytes(),
        filter_taint_catalog(element, overrides),
        overrides.typography(),
    )
}

pub(crate) fn parse_svg_bytes(bytes: &[u8]) -> Result<SvgAsset, String> {
    parse_svg_bytes_with_filter_taint(bytes, SvgFilterTaintCatalog::default())
}

fn parse_svg_bytes_with_filter_taint(
    bytes: &[u8],
    filter_taint: SvgFilterTaintCatalog,
) -> Result<SvgAsset, String> {
    parse_svg_bytes_with_filter_taint_and_typography(bytes, filter_taint, HashMap::new())
}

fn parse_svg_bytes_with_filter_taint_and_typography(
    bytes: &[u8],
    filter_taint: SvgFilterTaintCatalog,
    text_typography: HashMap<SvgTextTypographyKey, SvgTextTypography>,
) -> Result<SvgAsset, String> {
    parse_svg_bytes_with_optional_image_context_and_filter_taint(
        bytes,
        None,
        filter_taint,
        text_typography,
    )
}

/// Parse an external SVG image in the color-scheme environment of its
/// embedding element.
pub(crate) fn parse_svg_bytes_with_image_context(
    bytes: &[u8],
    image_context: SvgImageContext,
) -> Result<SvgAsset, String> {
    parse_svg_bytes_with_optional_image_context_and_filter_taint(
        bytes,
        Some(image_context),
        SvgFilterTaintCatalog::default(),
        HashMap::new(),
    )
}

fn parse_svg_bytes_with_optional_image_context_and_filter_taint(
    bytes: &[u8],
    image_context: Option<SvgImageContext>,
    filter_taint: SvgFilterTaintCatalog,
    text_typography: HashMap<SvgTextTypographyKey, SvgTextTypography>,
) -> Result<SvgAsset, String> {
    let normalized_source = image_context
        .and_then(|context| normalize_svg_image_stylesheet(bytes, context))
        .unwrap_or_else(|| bytes.to_vec());
    let (normalized_source, viewport_background) =
        extract_svg_viewport_background(&normalized_source);
    let tree = parse_svg_tree(
        &normalized_source,
        usvg::Size::from_wh(300.0, 150.0).expect("default SVG viewport is valid"),
    )?;
    if svg_tree_has_unsupported_content(tree.root()) {
        log::debug!(
            "SVG contains unsupported paints or compositing; affected nodes will not be painted"
        );
    }
    let size = tree.size();
    let intrinsic_dimensions = svg_intrinsic_dimensions(&normalized_source, size);
    let has_degenerate_view_box = svg_has_degenerate_view_box(&normalized_source);
    let view_fragments = svg_view_fragments(&normalized_source);
    Ok(SvgAsset {
        tree,
        text_typography,
        filter_taint,
        viewport_background,
        intrinsic_size: LayoutSize::new(
            size.width() * css::CSS_PX_TO_PT,
            size.height() * css::CSS_PX_TO_PT,
        ),
        intrinsic_dimensions,
        has_degenerate_view_box,
        view_fragments,
        source: Rc::from(normalized_source),
    })
}

/// Move an explicit root `background-color` out of the source handed to
/// `usvg` so its viewport path is not transformed through the root `viewBox`.
///
/// SVG's root presentation attribute and inline style both establish a root
/// viewport background. We retain only colors that Quire can resolve without
/// a scene-local cascade (for example, not `currentColor`), leaving all other
/// declarations untouched for `usvg`'s existing handling.
/// <https://www.w3.org/TR/SVG2/styling.html#PresentationAttributes>
fn extract_svg_viewport_background(bytes: &[u8]) -> (Vec<u8>, Option<SvgViewportBackground>) {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return (bytes.to_vec(), None);
    };
    let Ok(document) = usvg::roxmltree::Document::parse(source) else {
        return (bytes.to_vec(), None);
    };
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return (bytes.to_vec(), None);
    }

    let mut replacements = Vec::new();
    let background = if let Some(style) = root.attribute("style") {
        let declarations = css::parse_declarations(style);
        if let Some(value) = declarations.get("background-color") {
            let Some(color) = css::parse_color(value) else {
                return (bytes.to_vec(), None);
            };
            let retained_style = declarations
                .iter()
                .filter(|(name, _)| name != "background-color")
                .map(|(name, value)| format!("{name}: {value};"))
                .collect::<String>();
            let attribute = root
                .attributes()
                .find(|attribute| attribute.name() == "style")
                .expect("root style attribute exists");
            replacements.push((
                attribute.range(),
                if retained_style.is_empty() {
                    String::new()
                } else {
                    svg_attribute("style", &retained_style)
                },
            ));
            if let Some(attribute) = root
                .attributes()
                .find(|attribute| attribute.name() == "background-color")
            {
                replacements.push((attribute.range(), String::new()));
            }
            Some(SvgViewportBackground { color })
        } else {
            root.attribute("background-color")
                .and_then(css::parse_color)
                .map(|color| {
                    let attribute = root
                        .attributes()
                        .find(|attribute| attribute.name() == "background-color")
                        .expect("root background-color attribute exists");
                    replacements.push((attribute.range(), String::new()));
                    SvgViewportBackground { color }
                })
        }
    } else {
        root.attribute("background-color")
            .and_then(css::parse_color)
            .map(|color| {
                let attribute = root
                    .attributes()
                    .find(|attribute| attribute.name() == "background-color")
                    .expect("root background-color attribute exists");
                replacements.push((attribute.range(), String::new()));
                SvgViewportBackground { color }
            })
    };

    let Some(background) = background else {
        return (bytes.to_vec(), None);
    };
    let mut rewritten = source.to_owned();
    replacements.sort_by_key(|(range, _)| range.start);
    for (range, replacement) in replacements.into_iter().rev() {
        rewritten.replace_range(range, &replacement);
    }
    (rewritten.into_bytes(), Some(background))
}

fn svg_attribute(name: &str, value: &str) -> String {
    let mut output = String::new();
    push_attribute(&mut output, name, value);
    output.trim_start().to_string()
}

/// Normalize the small CSS boundary between an image SVG and `usvg`.
///
/// `usvg` deliberately uses SimpleCSS, which skips conditional rules and does
/// not implement `:root`. Quire evaluates the image document's media queries
/// before that handoff, then gives SimpleCSS an equivalent static stylesheet.
/// The upstream limitation, including both `:root` and `@media`, is tracked
/// at <https://github.com/linebender/resvg/issues/960>.
/// `cssparser` tokenizes the stylesheet boundary, so strings, comments, escaped
/// identifiers, and nested component-value blocks cannot be mistaken for CSS
/// syntax by this normalization step.
fn normalize_svg_image_stylesheet(bytes: &[u8], context: SvgImageContext) -> Option<Vec<u8>> {
    let source = std::str::from_utf8(bytes).ok()?;
    let document = usvg::roxmltree::Document::parse(source).ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return None;
    }

    let environment = context.media_environment();
    let mut replacements = Vec::new();
    let mut needs_root_marker = false;
    for style in root.descendants().filter(|node| node.has_tag_name("style")) {
        if !matches!(style.attribute("type"), None | Some("text/css")) {
            continue;
        }
        for text in style.children().filter(|node| node.is_text()) {
            let range = text.range();
            let stylesheet = source.get(range.clone())?;
            let (normalized, rewrote_root) = flatten_svg_image_css(stylesheet, &environment);
            needs_root_marker |= rewrote_root;
            if normalized != stylesheet {
                replacements.push((range, normalized));
            }
        }
    }

    if replacements.is_empty() && !needs_root_marker {
        return Some(bytes.to_vec());
    }
    let mut normalized = source.to_owned();
    for (range, replacement) in replacements.into_iter().rev() {
        normalized.replace_range(range, &replacement);
    }
    if needs_root_marker {
        let root_start = root.range().start;
        let close = svg_start_tag_close(normalized.get(root_start..)?)? + root_start;
        let insert_at = normalized[..close]
            .trim_end_matches(char::is_whitespace)
            .strip_suffix('/')
            .map_or(close, str::len);
        normalized.insert_str(
            insert_at,
            &format!(r#" {SVG_IMAGE_ROOT_MARKER_ATTRIBUTE}="""#),
        );
    }
    Some(normalized.into_bytes())
}

/// Evaluate `@media` rules in an SVG stylesheet while retaining all
/// non-conditional rule text for `usvg`'s existing SVG CSS cascade.
fn flatten_svg_image_css(source: &str, environment: &css::MediaEnvironment) -> (String, bool) {
    let mut input = ParserInput::new(source);
    let mut input = Parser::new(&mut input);
    let mut parser = SvgImageCssParser {
        environment,
        rewrote_root: false,
    };
    let mut output = String::with_capacity(source.len());
    for rule in StyleSheetParser::new(&mut input, &mut parser) {
        match rule {
            Ok(rule) => output.push_str(&rule),
            // Keep malformed and unsupported rules in the source handed to
            // SimpleCSS. This preserves its existing recovery behavior.
            Err((_, source)) => output.push_str(source),
        }
    }
    (output, parser.rewrote_root)
}

struct SvgImageCssParser<'a> {
    environment: &'a css::MediaEnvironment,
    rewrote_root: bool,
}

enum SvgImageCssAtRulePrelude {
    Media { applies: bool },
    Other { name: String, prelude: String },
}

impl<'i> QualifiedRuleParser<'i> for SvgImageCssParser<'_> {
    type Prelude = String;
    type QualifiedRule = String;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        let selector = consume_svg_css_input(input);
        let (selector, rewrote_root) = rewrite_svg_root_selector(&selector);
        self.rewrote_root |= rewrote_root;
        Ok(selector)
    }

    fn parse_block<'t>(
        &mut self,
        selector: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, cssparser::ParseError<'i, Self::Error>> {
        Ok(format!("{selector}{{{}}}", consume_svg_css_input(input)))
    }
}

impl<'i> AtRuleParser<'i> for SvgImageCssParser<'_> {
    type Prelude = SvgImageCssAtRulePrelude;
    type AtRule = String;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        let prelude = consume_svg_css_input(input);
        if name.eq_ignore_ascii_case("media") {
            Ok(SvgImageCssAtRulePrelude::Media {
                applies: css::media_rule_applies_in_environment(prelude.trim(), self.environment),
            })
        } else {
            Ok(SvgImageCssAtRulePrelude::Other {
                name: name.to_string(),
                prelude,
            })
        }
    }

    fn rule_without_block(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
    ) -> Result<Self::AtRule, ()> {
        Ok(match prelude {
            SvgImageCssAtRulePrelude::Media { .. } => String::new(),
            SvgImageCssAtRulePrelude::Other { name, prelude } => format!("@{name}{prelude};"),
        })
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, cssparser::ParseError<'i, Self::Error>> {
        match prelude {
            SvgImageCssAtRulePrelude::Media { applies: true } => {
                let (flattened, rewrote_root) =
                    flatten_svg_image_css(&consume_svg_css_input(input), self.environment);
                self.rewrote_root |= rewrote_root;
                Ok(flattened)
            }
            SvgImageCssAtRulePrelude::Media { applies: false } => {
                consume_svg_css_input(input);
                Ok(String::new())
            }
            SvgImageCssAtRulePrelude::Other { name, prelude } => Ok(format!(
                "@{name}{prelude}{{{}}}",
                consume_svg_css_input(input)
            )),
        }
    }
}

/// Consume a delimited CSS parser while preserving its source text. Component
/// values are tokenized by `cssparser`, so nested strings and blocks are safe.
fn consume_svg_css_input(input: &mut Parser<'_, '_>) -> String {
    let start = input.position();
    while input.next_including_whitespace_and_comments().is_ok() {}
    input.slice_from(start).to_string()
}

/// Replace the exact `:root` pseudo-class in a selector prelude. The private
/// attribute carries the same class specificity and cannot match descendants.
/// This compensates for the upstream SimpleCSS `:root` gap:
/// <https://github.com/linebender/resvg/issues/960>.
fn rewrite_svg_root_selector(source: &str) -> (String, bool) {
    let mut input = ParserInput::new(source);
    let mut input = Parser::new(&mut input);
    let mut output = String::with_capacity(source.len());
    let mut cursor = input.position();
    let mut changed = false;
    while !input.is_exhausted() {
        let token_start = input.position();
        let Ok(token) = input.next_including_whitespace_and_comments() else {
            break;
        };
        if matches!(token, Token::Colon)
            && matches!(input.next_including_whitespace_and_comments(), Ok(Token::Ident(name)) if name.eq_ignore_ascii_case("root"))
        {
            output.push_str(input.slice(cursor..token_start));
            output.push('[');
            output.push_str(SVG_IMAGE_ROOT_MARKER_ATTRIBUTE);
            output.push(']');
            cursor = input.position();
            changed = true;
        }
    }
    if changed {
        output.push_str(input.slice_from(cursor));
        (output, true)
    } else {
        (source.to_owned(), false)
    }
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
        image_href_resolver: SvgResourceResolver::secure_static().image_href_resolver(),
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

/// Return an inline SVG attribute in the null namespace.
///
/// HTML's foreign-content parser preserves a legacy `xlink:href` separately
/// from SVG 2's null-namespace `href`. The general-purpose attribute map is
/// keyed only by local name, so inline SVG must read the namespace-aware DOM
/// representation to avoid conflating them.
fn inline_svg_unprefixed_attribute<'a>(element: &'a Element, name: &str) -> Option<&'a str> {
    if element.namespace_attrs.is_empty() {
        return element.attrs.get(name).map(String::as_str);
    }
    element
        .namespace_attrs
        .iter()
        .find(|attribute| attribute.namespace_url.is_empty() && attribute.local_name == name)
        .map(|attribute| attribute.value.as_str())
}

/// Collect null-namespace inline SVG attributes for XML serialization.
///
/// Synthetic unit-test elements predate `namespace_attrs`; retaining the
/// empty-vector fallback keeps those fixtures valid while parsed documents use
/// the namespace-preserving source of truth.
fn inline_svg_unprefixed_attributes(element: &Element) -> Vec<(&str, &str)> {
    if element.namespace_attrs.is_empty() {
        return element
            .attrs
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
    }
    element
        .namespace_attrs
        .iter()
        .filter(|attribute| attribute.namespace_url.is_empty())
        .map(|attribute| (attribute.local_name.as_str(), attribute.value.as_str()))
        .collect()
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
    let Some(origin) = inline_svg_unprefixed_attribute(element, "transform-origin") else {
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
        inline_svg_unprefixed_attribute(element, name).map_or(Some(default), svg_user_length)
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

/// Serialize a cascaded CSS `text-shadow` list into SVG user units.
///
/// Inline SVG is parsed as an isolated normalized resource, while its host
/// HTML stylesheet is evaluated by Quire before that boundary. CSS pixels are
/// stored as points in `ComputedStyle`; SVG presentation values use CSS-pixel
/// user units, so this is the one explicit conversion between those systems.
pub(crate) fn svg_text_shadow_presentation_attribute(style: &ComputedStyle) -> String {
    style
        .text_shadow
        .iter()
        .map(|shadow| {
            let color = shadow.color.resolve(style.color);
            let srgb = color.to_rgb_space(css::RgbColorSpace::Srgb);
            let [red, green, blue] = srgb.components();
            let rgba = format!(
                "rgba({} {} {} / {})",
                red * 255.0,
                green * 255.0,
                blue * 255.0,
                srgb.alpha()
            );
            let unit = |length: &css::ComputedLengthPercentage| {
                format!("{}px", length.length_points() / css::CSS_PX_TO_PT)
            };
            let mut value = format!(
                "{} {} {} {} {}",
                unit(&shadow.offset_x),
                unit(&shadow.offset_y),
                unit(&shadow.blur_radius),
                unit(&shadow.spread),
                rgba,
            );
            if shadow.inset {
                value.push_str(" inset");
            }
            value
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Serialize a computed CSS `font-family` list for the retained SVG parser.
/// The document `FontSystem` still performs resolution; this only transfers
/// the cascade result across the inline-SVG serialization boundary.
pub(crate) fn svg_font_family_presentation_attribute(family: &FontFamily) -> String {
    fn one(family: &FontFamily) -> String {
        match family {
            FontFamily::SansSerif => "sans-serif".to_owned(),
            FontFamily::Serif => "serif".to_owned(),
            FontFamily::Monospace => "monospace".to_owned(),
            FontFamily::SystemUi => "system-ui".to_owned(),
            FontFamily::UiSerif => "ui-serif".to_owned(),
            FontFamily::UiSansSerif => "ui-sans-serif".to_owned(),
            FontFamily::UiMonospace => "ui-monospace".to_owned(),
            FontFamily::UiRounded => "ui-rounded".to_owned(),
            FontFamily::Named(name) => format!("\"{}\"", name.as_str().replace('"', "\\\"")),
            FontFamily::List(_) => unreachable!("font-family list is flattened below"),
        }
    }
    match family {
        FontFamily::List(families) => families.iter().map(one).collect::<Vec<_>>().join(", "),
        family => one(family),
    }
}

/// Serialize computed OpenType variation coordinates for the standalone SVG
/// normalization payload. Values remain CSS numbers; the parser retains them
/// as axis/value pairs and `svg_text_style` passes their exact IEEE-754 bits
/// into the shared document-font request.
pub(crate) fn svg_font_variation_settings_presentation_attribute(
    settings: &FontVariationSettings,
) -> String {
    if settings.0.is_empty() {
        return "normal".to_owned();
    }
    settings
        .0
        .iter()
        .filter_map(|setting| {
            let value = f32::from_bits(setting.value);
            value.is_finite().then(|| {
                let tag = setting
                    .tag
                    .iter()
                    .map(|byte| match byte {
                        b'\\' => "\\\\".to_owned(),
                        b'\"' => "\\\"".to_owned(),
                        0x20..=0x7e => char::from(*byte).to_string(),
                        byte => format!("\\{:x} ", byte),
                    })
                    .collect::<String>();
                format!("\"{tag}\" {value}")
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
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
    let mut attrs = inline_svg_unprefixed_attributes(element);
    attrs.sort_unstable_by_key(|(name, _)| *name);
    let selected_scene_transform = override_values.and_then(|values| match values.transform {
        Some(SvgTransformOverride::Scene(transform)) => Some(transform),
        Some(SvgTransformOverride::RootBox(_)) | None => None,
    });
    let transform_is_owned = selected_scene_transform.is_some();
    let source_transform = inline_svg_unprefixed_attribute(element, "transform").map(str::to_owned);
    let resolved_transform = match selected_scene_transform {
        Some(SvgUsedTransform::None) => None,
        Some(SvgUsedTransform::Affine(transform)) => {
            Some(svg_element_transform_attribute(transform))
        }
        None => source_transform
            .as_deref()
            .map(|transform| svg_presentation_transform_with_origin(element, transform)),
    };
    let mut emitted_transform = false;
    let mut emitted_style = false;
    for (name, value) in attrs {
        if name == "xmlns" || name.starts_with("xmlns:") {
            continue;
        }
        if name == "transform-origin" && (transform_is_owned || resolved_transform.is_some()) {
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
        if name == "style"
            && (transform_is_owned
                || override_values.is_some_and(|values| {
                    values.flood_color.is_some()
                        || values.lighting_color.is_some()
                        || [
                            "fill",
                            "stroke",
                            "stroke-width",
                            "text-shadow",
                            "font-family",
                            "font-size",
                            "font-weight",
                            "font-style",
                            "font-stretch",
                            "font-variation-settings",
                            "font-kerning",
                            "writing-mode",
                            "text-orientation",
                            "direction",
                            "unicode-bidi",
                        ]
                        .into_iter()
                        .any(|name| values.owns_style_property(name))
                }))
        {
            if let Some(style) =
                sanitize_inline_svg_presentation_style(value, transform_is_owned, override_values)
            {
                push_attribute(output, name, &style);
                emitted_style = true;
            }
            continue;
        }
        if matches!(
            name,
            "fill"
                | "stroke"
                | "stroke-width"
                | "text-shadow"
                | "font-family"
                | "font-size"
                | "font-weight"
                | "font-style"
                | "font-stretch"
                | "font-variation-settings"
                | "writing-mode"
                | "text-orientation"
                | "direction"
                | "unicode-bidi"
                | "flood-color"
                | "lighting-color"
        ) && ((name == "fill"
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
                    .is_some())
            || (name == "text-shadow"
                && override_values
                    .and_then(|values| values.text_shadow.as_ref())
                    .is_some())
            || (name == "font-family"
                && override_values
                    .and_then(|values| values.font_family.as_ref())
                    .is_some())
            || (name == "font-size"
                && override_values
                    .and_then(|values| values.font_size.as_ref())
                    .is_some())
            || (name == "font-weight"
                && override_values
                    .and_then(|values| values.font_weight.as_ref())
                    .is_some())
            || (name == "font-style"
                && override_values
                    .and_then(|values| values.font_style.as_ref())
                    .is_some())
            || (name == "font-stretch"
                && override_values
                    .and_then(|values| values.font_stretch.as_ref())
                    .is_some())
            || (name == "font-variation-settings"
                && override_values
                    .and_then(|values| values.font_variation_settings.as_ref())
                    .is_some())
            || (name == "letter-spacing"
                && override_values
                    .and_then(|values| values.letter_spacing.as_ref())
                    .is_some())
            || (name == "word-spacing"
                && override_values
                    .and_then(|values| values.word_spacing.as_ref())
                    .is_some())
            || (name == "writing-mode"
                && override_values
                    .and_then(|values| values.writing_mode.as_ref())
                    .is_some())
            || (name == "text-orientation"
                && override_values
                    .and_then(|values| values.text_orientation.as_ref())
                    .is_some())
            || (name == "direction"
                && override_values
                    .and_then(|values| values.direction.as_ref())
                    .is_some())
            || (name == "unicode-bidi"
                && override_values
                    .and_then(|values| values.unicode_bidi.as_ref())
                    .is_some())
            || (name == "flood-color"
                && override_values
                    .and_then(|values| values.flood_color)
                    .is_some())
            || (name == "lighting-color"
                && override_values
                    .and_then(|values| values.lighting_color)
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
        } else if name == "transform" && transform_is_owned {
            // Quire owns the selected cascade result. The source presentation
            // attribute must not enter `usvg`'s second cascade.
            emitted_transform = true;
            if let Some(transform) = resolved_transform.as_deref() {
                push_attribute(output, name, transform);
            }
        } else if name == "transform" {
            emitted_transform = true;
            push_attribute(output, name, resolved_transform.as_deref().unwrap_or(value));
        } else {
            if name == "style" {
                emitted_style = true;
            }
            push_attribute(output, name, value);
        }
    }
    if !emitted_transform && let Some(transform) = resolved_transform.as_deref() {
        push_attribute(output, "transform", transform);
    }
    if !emitted_style
        && let Some(font_kerning) =
            override_values.and_then(|values| values.font_kerning.as_deref())
    {
        push_attribute(output, "style", &format!("font-kerning: {font_kerning};"));
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
        (
            "text-shadow",
            override_values.and_then(|values| values.text_shadow.as_deref()),
        ),
        (
            "font-family",
            override_values.and_then(|values| values.font_family.as_deref()),
        ),
        (
            "font-size",
            override_values.and_then(|values| values.font_size.as_deref()),
        ),
        (
            "font-weight",
            override_values.and_then(|values| values.font_weight.as_deref()),
        ),
        (
            "font-style",
            override_values.and_then(|values| values.font_style.as_deref()),
        ),
        (
            "font-stretch",
            override_values.and_then(|values| values.font_stretch.as_deref()),
        ),
        (
            "font-variation-settings",
            override_values.and_then(|values| values.font_variation_settings.as_deref()),
        ),
        (
            "letter-spacing",
            override_values.and_then(|values| values.letter_spacing.as_deref()),
        ),
        (
            "word-spacing",
            override_values.and_then(|values| values.word_spacing.as_deref()),
        ),
        (
            "writing-mode",
            override_values.and_then(|values| values.writing_mode.as_deref()),
        ),
        (
            "text-orientation",
            override_values.and_then(|values| values.text_orientation.as_deref()),
        ),
        (
            "direction",
            override_values.and_then(|values| values.direction.as_deref()),
        ),
        (
            "unicode-bidi",
            override_values.and_then(|values| values.unicode_bidi.as_deref()),
        ),
    ] {
        if let Some(value) = value {
            push_attribute(output, name, value);
        }
    }
    if let Some(key) = override_values.and_then(|values| values.text_typography_key) {
        push_attribute(
            output,
            SVG_TEXT_TYPOGRAPHY_KEY_ATTRIBUTE,
            &key.as_attribute_value(),
        );
    }
    for (name, value) in [
        (
            "flood-color",
            override_values
                .and_then(|values| values.flood_color)
                .map(|color| svg_filter_color_attribute(color.color)),
        ),
        (
            "lighting-color",
            override_values
                .and_then(|values| values.lighting_color)
                .map(|color| svg_filter_color_attribute(color.color)),
        ),
    ] {
        if let Some(value) = value.as_deref() {
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

fn svg_filter_color_attribute(color: CssColor) -> String {
    format!(
        "rgba({}, {}, {}, {})",
        (color.components()[0] * 255.0).round().clamp(0.0, 255.0),
        (color.components()[1] * 255.0).round().clamp(0.0, 255.0),
        (color.components()[2] * 255.0).round().clamp(0.0, 255.0),
        color.alpha()
    )
}

/// Serialize a typed SVG-element matrix at the only string boundary of the
/// host-CSS-to-SVG bridge.
fn svg_element_transform_attribute(transform: SvgElementTransform) -> String {
    format!(
        "matrix({} {} {} {} {} {})",
        transform.m11, transform.m12, transform.m21, transform.m22, transform.m31, transform.m32
    )
}

/// Remove declarations whose used values Quire has already applied to the
/// serialized SVG transform attribute. Rebuilding only the declaration list
/// is safe here: this is a private payload for `usvg`, not the source DOM.
fn sanitize_inline_svg_presentation_style(
    value: &str,
    remove_transform: bool,
    overrides: Option<&SvgPresentationOverride>,
) -> Option<String> {
    let declarations = css::parse_declarations(value);
    let mut style = declarations
        .iter()
        .filter(|(name, _)| {
            !((remove_transform
                && matches!(
                    name.as_str(),
                    "transform"
                        | "transform-origin"
                        | "transform-box"
                        | "translate"
                        | "rotate"
                        | "scale"
                ))
                || overrides.is_some_and(|overrides| overrides.owns_style_property(name)))
        })
        .map(|(name, value)| format!("{name}: {value};"))
        .collect::<String>();
    if let Some(font_kerning) = overrides.and_then(|overrides| overrides.font_kerning.as_deref()) {
        style.push_str("font-kerning: ");
        style.push_str(font_kerning);
        style.push(';');
    }
    (!style.is_empty()).then_some(style)
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
    use base64::Engine;

    use super::*;
    use crate::dom::Node;
    use crate::units::content_box_size_pt;

    #[test]
    fn svg_transform_reference_boxes_select_only_the_requested_coordinate_space() {
        let fill = SvgElementRect::new(
            SvgElementPoint::new(1.0, 2.0),
            SvgElementSize::new(30.0, 40.0),
        );
        let stroke = SvgElementRect::new(
            SvgElementPoint::new(0.0, 1.0),
            SvgElementSize::new(32.0, 42.0),
        );
        let view = SvgElementRect::new(
            SvgElementPoint::new(-10.0, -20.0),
            SvgElementSize::new(100.0, 200.0),
        );
        let boxes = SvgTransformReferenceBoxes::new(fill, stroke, Some(view));

        assert_eq!(
            boxes.select(css::TransformBox::FillBox).unwrap().rect(),
            fill
        );
        assert_eq!(
            boxes.select(css::TransformBox::StrokeBox).unwrap().rect(),
            stroke
        );
        assert_eq!(
            boxes.select(css::TransformBox::ViewBox).unwrap().rect(),
            view
        );
    }

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
            document_compatibility_mode: crate::dom::DocumentCompatibilityMode::NoQuirks,
            attrs: HashMap::from([
                ("width".to_owned(), "150".to_owned()),
                ("height".to_owned(), "150".to_owned()),
                ("transform".to_owned(), "rotate(90)".to_owned()),
                ("transform-origin".to_owned(), "75".to_owned()),
            ]),
            namespace_attrs: Vec::new(),
            children: Vec::new(),
            is_target: false,
            selector_snapshot: std::cell::OnceCell::new(),
            object_rendering: crate::dom::ObjectRendering::Fallback,
            selected_image_source: None,
            image_rendering: crate::dom::ImageRendering::Image,
        };
        let mut overrides = SvgPresentationOverrides::new();
        overrides.insert(
            rect.id,
            SvgPresentationOverride {
                transform: Some(SvgTransformOverride::Scene(SvgUsedTransform::Affine(
                    // The bridge has already folded the fill-box origin into
                    // this scene-local matrix.
                    SvgElementTransform::new(0.0, 1.0, -1.0, 0.0, 150.0, 0.0),
                ))),
                ..SvgPresentationOverride::default()
            },
        );
        let xml = serialize_inline_svg_with_presentation_overrides(&rect, &overrides);

        assert!(xml.contains("transform=\"matrix(0 1 -1 0 150 0)\""));
        assert!(!xml.contains("transform-origin="));
    }

    #[test]
    fn host_css_presentation_overrides_replace_svg_paint_attributes() {
        let circle = Element {
            id: ElementId::next(),
            tag: "circle".to_owned(),
            namespace_url: "http://www.w3.org/2000/svg".to_owned(),
            document_syntax: crate::dom::DocumentSyntax::Html,
            document_compatibility_mode: crate::dom::DocumentCompatibilityMode::NoQuirks,
            attrs: HashMap::from([
                ("fill".to_owned(), "red".to_owned()),
                ("stroke".to_owned(), "black".to_owned()),
            ]),
            namespace_attrs: Vec::new(),
            children: Vec::new(),
            is_target: false,
            selector_snapshot: std::cell::OnceCell::new(),
            object_rendering: crate::dom::ObjectRendering::Fallback,
            selected_image_source: None,
            image_rendering: crate::dom::ImageRendering::Image,
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
            document_compatibility_mode: crate::dom::DocumentCompatibilityMode::NoQuirks,
            attrs: HashMap::from([("style".to_owned(), "opacity: 0".to_owned())]),
            namespace_attrs: Vec::new(),
            children: vec![Node::text("P")],
            is_target: false,
            selector_snapshot: std::cell::OnceCell::new(),
            object_rendering: crate::dom::ObjectRendering::Fallback,
            selected_image_source: None,
            image_rendering: crate::dom::ImageRendering::Image,
        };
        let group = Element {
            id: ElementId::next(),
            tag: "g".to_owned(),
            namespace_url: SVG_NAMESPACE.to_owned(),
            document_syntax: crate::dom::DocumentSyntax::Html,
            document_compatibility_mode: crate::dom::DocumentCompatibilityMode::NoQuirks,
            attrs: HashMap::from([("style".to_owned(), "opacity: 0".to_owned())]),
            namespace_attrs: Vec::new(),
            children: vec![Node {
                kind: NodeKind::Element(text),
            }],
            is_target: false,
            selector_snapshot: std::cell::OnceCell::new(),
            object_rendering: crate::dom::ObjectRendering::Fallback,
            selected_image_source: None,
            image_rendering: crate::dom::ImageRendering::Image,
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

    fn inline_svg_element(source: &str) -> Element {
        fn find_svg(node: &Node) -> Option<&Element> {
            let NodeKind::Element(element) = &node.kind else {
                return None;
            };
            if element.namespace_url == SVG_NAMESPACE && element.tag == "svg" {
                return Some(element);
            }
            element.children.iter().find_map(find_svg)
        }

        let document = crate::dom::parse(source);
        find_svg(&document)
            .expect("expected inline SVG root")
            .clone()
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
    fn normalizes_omitted_svg_viewports_to_the_css_image_size() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="100%" height="100%" fill="orange"/></svg>"#,
        )
        .unwrap();

        assert_eq!(
            asset
                .with_css_image_viewport(PaintSize::new(150.0, 300.0))
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

    #[test]
    fn css_image_viewport_uses_border_image_area_for_view_box_only_svg() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect width="100" height="100" fill="orange"/></svg>"#,
        )
        .unwrap();

        // A 150pt border-image area is 200 CSS pixels.  Number-valued
        // border-image slices therefore resolve against this viewport rather
        // than the SVG parser's viewBox-sized fallback.
        // <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>
        let viewport = asset.with_css_image_viewport(PaintSize::new(150.0, 150.0));
        assert_eq!(
            viewport.source_viewport_size(),
            SvgSourceSize::new(200.0, 200.0)
        );
        assert_opaque_rect_bounds(&viewport, (0.0, 0.0, 12.0, 12.0));
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
    fn css_image_viewport_honors_default_preserve_aspect_ratio_for_view_box_only_svg() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 8"><rect width="16" height="8" fill="orange"/></svg>"#,
        )
        .unwrap();
        let intrinsic_dimensions = asset.intrinsic_dimensions();
        let viewport = asset.with_css_image_viewport(PaintSize::new(12.0, 12.0));

        assert_eq!(
            viewport.source_viewport_size(),
            SvgSourceSize::new(16.0, 16.0)
        );
        assert_eq!(viewport.intrinsic_dimensions(), intrinsic_dimensions);
        assert_opaque_rect_bounds(&viewport, (0.0, 3.0, 12.0, 9.0));
    }

    #[test]
    fn css_image_viewport_honors_default_preserve_aspect_ratio_for_sized_svg() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8" viewBox="0 0 16 8"><rect width="16" height="8" fill="orange"/></svg>"#,
        )
        .unwrap();
        let intrinsic_dimensions = asset.intrinsic_dimensions();
        let viewport = asset.with_css_image_viewport(PaintSize::new(12.0, 12.0));

        assert_eq!(
            viewport.source_viewport_size(),
            SvgSourceSize::new(16.0, 16.0)
        );
        assert_eq!(viewport.intrinsic_dimensions(), intrinsic_dimensions);
        assert_opaque_rect_bounds(&viewport, (0.0, 3.0, 12.0, 9.0));
    }

    #[test]
    fn css_image_viewport_preserves_none_aspect_ratio_stretching() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8" viewBox="0 0 16 8" preserveAspectRatio="none"><rect width="16" height="8" fill="orange"/></svg>"#,
        )
        .unwrap();
        let viewport = asset.with_css_image_viewport(PaintSize::new(12.0, 12.0));

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
    fn svg_image_stylesheet_uses_embedding_color_scheme_for_root_media_rules() {
        let source = br#"
            <svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">
                <style>
                    :root { color: blue }
                    @media (prefers-color-scheme: dark) {
                        :root { color: purple }
                    }
                </style>
                <rect width="32" height="32" fill="currentColor"/>
            </svg>
        "#;
        let light = parse_svg_bytes_with_image_context(
            source,
            SvgImageContext::from_used_color_scheme(css::UsedColorScheme::Light),
        )
        .unwrap();
        let dark = parse_svg_bytes_with_image_context(
            source,
            SvgImageContext::from_used_color_scheme(css::UsedColorScheme::Dark),
        )
        .unwrap();

        assert_eq!(light.opaque_viewport_fill(), Some(CssColor::new(0, 0, 255)));
        assert_eq!(
            dark.opaque_viewport_fill(),
            Some(CssColor::new(128, 0, 128))
        );
    }

    #[test]
    fn svg_image_stylesheet_media_normalization_preserves_nonmatching_branches() {
        let source = br#"
            <svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">
                <style>
                    :root { color: blue }
                    @media (prefers-color-scheme: dark) { :root { color: purple } }
                    @media (prefers-color-scheme: light) { :root { color: lime } }
                </style>
                <rect width="32" height="32" fill="currentColor"/>
            </svg>
        "#;
        let light = parse_svg_bytes_with_image_context(
            source,
            SvgImageContext::from_used_color_scheme(css::UsedColorScheme::Light),
        )
        .unwrap();

        assert_eq!(light.opaque_viewport_fill(), Some(CssColor::new(0, 255, 0)));
    }

    #[test]
    fn svg_image_stylesheet_normalization_uses_css_tokens_for_root_and_media() {
        let environment =
            SvgImageContext::from_used_color_scheme(css::UsedColorScheme::Dark).media_environment();
        let (normalized, rewrote_root) = flatten_svg_image_css(
            r#"
                /* :root is only a comment. */
                :r\6f ot { --label: ":root"; color: blue }
                @media (prefers-color-scheme: dark) {
                    @media (prefers-color-scheme: dark) { :root { color: purple } }
                }
                @media (prefers-color-scheme: light) { :root { color: lime } }
            "#,
            &environment,
        );

        assert!(rewrote_root);
        assert!(normalized.contains(&format!("[{SVG_IMAGE_ROOT_MARKER_ATTRIBUTE}]")));
        assert!(normalized.contains(r#"--label: ":root""#));
        assert!(normalized.contains("color: purple"));
        assert!(!normalized.contains("color: lime"));
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
    fn inline_svg_keeps_modern_href_separate_from_xlink_href() {
        let svg = inline_svg_element(
            r##"<svg><pattern id="Copied" href="#Modern" xlink:href="#Legacy"/></svg>"##,
        );
        let NodeKind::Element(pattern) = &svg.children[0].kind else {
            panic!("expected pattern child");
        };

        assert_eq!(
            inline_svg_unprefixed_attribute(pattern, "href"),
            Some("#Modern")
        );
        assert!(pattern.namespace_attrs.iter().any(|attribute| {
            attribute.namespace_url == XLINK_NAMESPACE
                && attribute.local_name == "href"
                && attribute.value == "#Legacy"
        }));

        let serialized = serialize_inline_svg(&svg);
        assert!(serialized.contains("href=\"#Modern\""));
        assert!(serialized.contains("xlink:href=\"#Legacy\""));
        assert!(serialized.contains(&format!("xmlns:xlink=\"{XLINK_NAMESPACE}\"")));
    }

    #[test]
    fn inline_svg_pattern_prefers_modern_href_over_xlink_href() {
        let svg = inline_svg_element(
            r##"<svg width="100" height="100"><pattern id="Modern" patternUnits="userSpaceOnUse" width="25" height="25"><rect width="25" height="25" fill="green"/></pattern><pattern id="Legacy" patternUnits="userSpaceOnUse" width="25" height="25"><rect width="25" height="25" fill="red"/></pattern><pattern id="Copied" href="#Modern" xlink:href="#Legacy"/><rect width="100" height="100" fill="url(#Copied)"/></svg>"##,
        );
        let asset = parse_inline_svg(&svg).expect("inline SVG parses");
        let paths = asset.paint_paths(paint_rect(0.0, 0.0, 75.0, 75.0));
        let [path] = paths.as_slice() else {
            panic!("expected patterned SVG path");
        };
        let Some(RenderedPathPaint::SvgPattern(pattern)) = path.fill_paint.as_ref() else {
            panic!("expected SVG tiling paint");
        };
        let paths = simple_group_paths(pattern.scene.as_ref())
            .expect("the pattern cell should contain a simple vector path");

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].fill, Some(CssColor::new(0, 128, 0)));
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
        let paths = simple_group_paths(pattern.scene.as_ref())
            .expect("the pattern cell should contain only simple vector paths");
        assert_eq!(paths.len(), 4);
        assert_eq!(path.transform, PaintTransform::identity());
        assert_ne!(pattern.transform, PaintTransform::identity());
        assert_eq!(paths[0].fill, Some(CssColor::new(0, 128, 0)));
        assert_eq!(paths[3].fill, Some(CssColor::new(0, 0, 255)));
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
        assert_eq!(paths[0].clip, None);
        assert_eq!(paths[0].bounds(), Some(paint_rect(100.0, 200.0, 20.0, 8.0)));
    }

    #[test]
    fn border_image_viewport_crop_hardens_a_view_box_only_solid_rectangle() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect width="100" height="100" fill="aqua"/></svg>"#,
        )
        .unwrap()
        // The 150pt border-image area establishes a 200 CSS-pixel viewport.
        .with_css_image_viewport(PaintSize::new(150.0, 150.0));
        let paths = asset.paint_paths_for_source_rect(
            paint_rect(10.0, 20.0, 30.0, 40.0),
            SvgSourceRect::new(
                SvgSourcePoint::new(10.0, 20.0),
                SvgSourceSize::new(40.0, 20.0),
            ),
        );

        let [path] = paths.as_slice() else {
            panic!("the cropped rectangle should remain one vector path");
        };
        assert_eq!(path.fill, Some(CssColor::new(0, 255, 255)));
        assert_eq!(path.clip, None);
        assert_eq!(path.bounds(), Some(paint_rect(10.0, 20.0, 30.0, 40.0)));
    }

    #[test]
    fn border_image_viewport_crop_hardens_each_solid_cell_in_an_edge_slice() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
                <rect x="0" y="0" width="10" height="40" fill="yellow"/>
                <rect x="10" y="0" width="60" height="40" fill="blue"/>
                <rect x="70" y="0" width="30" height="40" fill="aqua"/>
            </svg>"#,
        )
        .unwrap()
        // The 150pt border-image area establishes a 200 CSS-pixel viewport.
        .with_css_image_viewport(PaintSize::new(150.0, 150.0));
        let paths = asset.paint_paths_for_source_rect(
            paint_rect(10.0, 20.0, 120.0, 30.0),
            // A numeric `border-image-slice: 40 30 20 10` top edge: crop the
            // 200px root viewport after the concrete SVG sizing pass.
            SvgSourceRect::new(
                SvgSourcePoint::new(10.0, 0.0),
                SvgSourceSize::new(160.0, 40.0),
            ),
        );

        assert_eq!(paths.len(), 3);
        assert!(paths.iter().all(|path| path.clip.is_none()));
        assert_eq!(
            paths.iter().map(|path| path.fill).collect::<Vec<_>>(),
            vec![
                Some(CssColor::new(255, 255, 0)),
                Some(CssColor::new(0, 0, 255)),
                Some(CssColor::new(0, 255, 255)),
            ]
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
    fn solid_svg_masks_rasterize_the_retained_subtree_instead_of_painting_it_unmasked() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><mask id="fade"><rect width="20" height="10" fill="white"/></mask><g mask="url(#fade)"><rect width="20" height="10" fill="red"/></g></svg>"#,
        );
        let asset = parse_inline_svg(&element).unwrap();

        let mut fonts = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 15.0, 7.5),
            true,
            &mut fonts,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "scene={scene:#?}");
        assert!(images[0].actual_text.is_none());
    }

    #[test]
    fn solid_inline_svg_masks_outline_shared_text_once_with_actual_text() {
        let element = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="30"><mask id="ink" maskUnits="userSpaceOnUse"><rect width="80" height="30" fill="white"/></mask><text mask="url(#ink)" x="4" y="20" font-size="16">Masked text</text></svg>"#,
        );
        let asset = parse_inline_svg(&element).unwrap();
        let mut fonts = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 60.0, 22.5),
            true,
            &mut fonts,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "scene={scene:#?}");
        assert_eq!(images[0].actual_text.as_deref(), Some("Masked text"));
        assert!(
            !scene
                .items
                .iter()
                .any(|item| matches!(item, SvgPaintItem::Text(_))),
            "masked text must not leave an invisible native-PDF duplicate"
        );
    }

    #[test]
    fn tainted_displacement_map_lowers_only_its_exact_source_graphic_input() {
        let tree = parse_svg_tree(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><defs><filter id="tainted"><feFlood result="flood" flood-color="red"/><feDisplacementMap in="SourceGraphic" in2="flood" x="2" y="3" width="10" height="4"/></filter></defs><g filter="url(#tainted)"><rect width="20" height="10" fill="green"/></g></svg>"#,
            usvg::Size::from_wh(300.0, 150.0).unwrap(),
        )
        .unwrap();
        let group = tree
            .root()
            .children()
            .iter()
            .find_map(|node| match node {
                usvg::Node::Group(group) if !group.filters().is_empty() => Some(group),
                _ => None,
            })
            .unwrap();
        let catalog = SvgFilterTaintCatalog {
            by_filter_id: HashMap::from([(
                "tainted".to_owned(),
                vec![
                    SvgFilterPrimitiveTaint {
                        tag: "feFlood".to_owned(),
                        color_tainted: Some(true),
                        has_unsupported_standard_input: false,
                        declared_inputs: Vec::new(),
                    },
                    SvgFilterPrimitiveTaint {
                        tag: "feDisplacementMap".to_owned(),
                        color_tainted: None,
                        has_unsupported_standard_input: false,
                        declared_inputs: vec!["SourceGraphic".to_owned(), "flood".to_owned()],
                    },
                ],
            )]),
        };
        assert!(matches!(
            analyze_svg_filters(group.filters(), &catalog),
            SvgFilterAnalysis::ExactSourceGraphic {
                filter_clip: Some(_)
            }
        ));

        let mut untainted = catalog;
        untainted.by_filter_id.get_mut("tainted").unwrap()[0].color_tainted = Some(false);
        assert!(matches!(
            analyze_svg_filters(group.filters(), &untainted),
            SvgFilterAnalysis::RequiresRasterBackend
        ));
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

    #[test]
    fn inline_svg_text_uses_the_document_font_system() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><text x="10" y="15" font-family="sans-serif" font-size="12" fill="red">Shared font</text></svg>"#,
        )
        .expect("test SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 15.0),
            true,
            &mut font_system,
        );
        let [SvgPaintItem::Text(line)] = scene.items.as_slice() else {
            panic!("expected one native SVG text item, got {:?}", scene.items);
        };
        assert_eq!(line.text, "Shared font");
        assert!(line.runs.iter().all(|run| run.font_id.is_some()));
        assert!(line.runs.iter().any(|run| run.glyphs.is_some()));
    }

    #[test]
    fn inline_svg_text_keeps_normal_glyphs_upright_in_a_y_down_viewport() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="10" y="20" font-size="12">Upright</text></svg>"#,
        )
        .expect("test SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let line = first_svg_text(&scene).expect("solid SVG text stays native");
        let [a, b, c, d] = line.runs[0].text_matrix.pdf_components();

        assert_eq!([a, b, c, d], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(line.origin(), PaintPoint::new(7.5, 7.5));
    }

    #[test]
    fn inline_svg_text_converts_positive_dy_to_a_downward_glyph_offset() {
        let plain = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="10" y="20" font-size="12">A</text></svg>"#,
        )
        .unwrap();
        let shifted = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="10" y="20" dy="4" font-size="12">A</text></svg>"#,
        )
        .unwrap();
        let destination = paint_rect(0.0, 0.0, 75.0, 22.5);
        let mut plain_fonts = FontSystem::new();
        let mut shifted_fonts = FontSystem::new();
        let plain = plain.paint_inline_group_with_font_system(destination, true, &mut plain_fonts);
        let shifted =
            shifted.paint_inline_group_with_font_system(destination, true, &mut shifted_fonts);
        let plain = first_svg_text(&plain).unwrap();
        let shifted = first_svg_text(&shifted).unwrap();

        let plain_glyph = plain.runs[0].glyphs.as_ref().unwrap()[0].y_offset;
        let shifted_glyph = shifted.runs[0].glyphs.as_ref().unwrap()[0].y_offset;
        assert!(shifted_glyph < plain_glyph - 2.5);
        assert_eq!(shifted.runs[0].text_matrix.pdf_components()[3], 1.0);
    }

    #[test]
    fn inline_svg_text_preserves_an_authored_y_reflection() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><g transform="scale(1 -1)"><text x="10" y="-20" font-size="12">Reflected</text></g></svg>"#,
        )
        .expect("test SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let line = first_svg_text(&scene).expect("solid SVG text stays native");

        assert_eq!(
            line.runs[0].text_matrix.pdf_components(),
            [1.0, 0.0, 0.0, -1.0]
        );
    }

    #[test]
    fn inline_svg_text_maps_view_box_translation_and_non_uniform_scale() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="10 20 100 50" preserveAspectRatio="none"><text x="10" y="20" font-size="12">Mapped</text></svg>"#,
        )
        .expect("test SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 150.0, 100.0),
            true,
            &mut font_system,
        );
        let line = first_svg_text(&scene).expect("solid SVG text stays native");
        let [a, b, c, d] = line.runs[0].text_matrix.pdf_components();

        assert!(line.origin().x.abs() < 0.001);
        assert!((line.origin().y - 100.0).abs() < 0.001);
        assert!(a > 0.0 && d > 0.0 && a < d);
        assert!(b.abs() < 0.001 && c.abs() < 0.001);
    }

    /// The SVG adapter and HTML computed-style adapter submit equivalent
    /// requests to one document font registry. Native SVG PDF text therefore
    /// reuses HTML's selected variation instance, glyph mapping, subset, and
    /// ToUnicode source (the PDF-level native-text assertion lives in the
    /// smoke test alongside this unit-level identity check).
    #[test]
    fn equivalent_html_and_svg_requests_share_document_font_and_glyph_mapping() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><text x="4" y="16" font-family="serif" font-size="16" letter-spacing="2">Shared glyph mapping</text></svg>"#,
        )
        .expect("test SVG parses");
        let (source, svg_style) = {
            let usvg::Node::Text(text) = &asset.tree.root().children()[0] else {
                panic!("parser retains text");
            };
            let chunk = &text.chunks()[0];
            let span = &chunk.spans()[0];
            (
                chunk.text()[span.start()..span.end()].to_owned(),
                svg_text_style(
                    span,
                    text.writing_mode(),
                    text.text_orientation(),
                    text.direction(),
                    text.unicode_bidi(),
                    0.75,
                ),
            )
        };
        let mut font_system = FontSystem::new();
        let html_shaped = font_system
            .shape_text_request(TextShapingRequest::from_html_computed_style(
                &source,
                &svg_style,
                svg_style.line_height,
            ))
            .expect("equivalent HTML request shapes");
        let html_runs = html_shaped.rendered_runs();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 15.0),
            true,
            &mut font_system,
        );
        let svg_line = first_svg_text(&scene).expect("solid SVG text stays native");

        assert_eq!(html_runs.len(), svg_line.runs.len());
        for (html, svg) in html_runs.iter().zip(&svg_line.runs) {
            assert_eq!(html.font_id, svg.font_id);
            assert_eq!(html.glyphs, svg.glyphs);
            assert_eq!(html.glyph_source_ranges, svg.glyph_source_ranges);
        }
    }

    #[test]
    fn retained_svg_text_does_not_require_usvg_font_lookup() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><text x="10" y="15" font-family="quire-font-that-does-not-exist" font-size="12">Retained text</text></svg>"#,
        )
        .expect("the parser-only usvg fork retains unresolved font families");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 15.0),
            true,
            &mut font_system,
        );
        assert_eq!(first_svg_text(&scene).unwrap().text, "Retained text");
    }

    #[test]
    fn inline_svg_text_preserves_its_affine_transform_in_the_pdf_text_matrix() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><g transform="translate(20 30) rotate(30) skewX(10)"><text x="4" y="20" font-size="12">Affine</text></g></svg>"#,
        )
        .expect("test SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 75.0),
            true,
            &mut font_system,
        );
        let line = first_svg_text(&scene).expect("one transformed text item");
        assert!(line.runs.iter().all(|run| !run.text_matrix.is_identity()));
        let [a, b, c, d] = line.runs[0].text_matrix.pdf_components();
        assert!([a, b, c, d].into_iter().all(f32::is_finite));
        assert!(b.abs() > 0.01 || c.abs() > 0.01);
    }

    #[test]
    fn inline_svg_text_anchor_offsets_the_text_space_pen() {
        let start = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><text x="50" y="15" text-anchor="start" font-size="12">Anchor</text></svg>"#,
        )
        .unwrap();
        let middle = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><text x="50" y="15" text-anchor="middle" font-size="12">Anchor</text></svg>"#,
        )
        .unwrap();
        let mut start_fonts = FontSystem::new();
        let mut middle_fonts = FontSystem::new();
        let destination = paint_rect(0.0, 0.0, 75.0, 15.0);
        let start_scene =
            start.paint_inline_group_with_font_system(destination, true, &mut start_fonts);
        let middle_scene =
            middle.paint_inline_group_with_font_system(destination, true, &mut middle_fonts);
        let start = first_svg_text(&start_scene).unwrap();
        let middle = first_svg_text(&middle_scene).unwrap();
        assert_eq!(start.origin(), middle.origin());
        assert!(middle.runs[0].x_offset < start.runs[0].x_offset);
    }

    #[test]
    fn inline_svg_text_applies_character_indexed_dx_and_dy_without_reshaping() {
        let plain = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><text x="0" y="15" font-size="12">AB</text></svg>"#,
        )
        .unwrap();
        let positioned = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><text x="0" y="15" dx="5 3" dy="2 4" font-size="12">AB</text></svg>"#,
        )
        .unwrap();
        let destination = paint_rect(0.0, 0.0, 75.0, 15.0);
        let mut plain_fonts = FontSystem::new();
        let mut positioned_fonts = FontSystem::new();
        let plain_scene =
            plain.paint_inline_group_with_font_system(destination, true, &mut plain_fonts);
        let positioned_scene = positioned.paint_inline_group_with_font_system(
            destination,
            true,
            &mut positioned_fonts,
        );
        let plain = first_svg_text(&plain_scene).unwrap();
        let positioned = first_svg_text(&positioned_scene).unwrap();
        let plain_glyphs = plain.runs[0].glyphs.as_ref().unwrap();
        let positioned_glyphs = positioned.runs[0].glyphs.as_ref().unwrap();
        assert_eq!(plain_glyphs.len(), positioned_glyphs.len());
        assert!(positioned_glyphs[0].x_offset > plain_glyphs[0].x_offset);
        assert!(positioned_glyphs[0].y_offset < plain_glyphs[0].y_offset);
        assert!(positioned_glyphs[1].x_offset > plain_glyphs[1].x_offset);
        assert!(positioned_glyphs[1].y_offset < plain_glyphs[1].y_offset);
    }

    #[test]
    fn inline_svg_text_uses_absolute_character_x_and_y_chunks() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="4 40" y="20 8" font-size="16">AB</text></svg>"#,
        )
        .unwrap();
        let mut fonts = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut fonts,
        );
        let lines = scene
            .items
            .iter()
            .filter_map(|item| match item {
                SvgPaintItem::Text(line) => Some(line),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert!(lines[1].origin().x > lines[0].origin().x + 20.0);
        assert!(lines[1].origin().y > lines[0].origin().y + 5.0);
    }

    #[test]
    fn inline_svg_text_uses_document_baselines_and_inherited_baseline_shift() {
        let plain = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="4" y="20" font-size="16">Base</text></svg>"#,
        )
        .unwrap();
        let shifted = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="4" y="20" font-size="16" dominant-baseline="hanging" baseline-shift="2"><tspan baseline-shift="3">Base</tspan></text></svg>"#,
        )
        .unwrap();
        let destination = paint_rect(0.0, 0.0, 75.0, 22.5);
        let mut plain_fonts = FontSystem::new();
        let mut shifted_fonts = FontSystem::new();
        let plain_scene =
            plain.paint_inline_group_with_font_system(destination, true, &mut plain_fonts);
        let shifted_scene =
            shifted.paint_inline_group_with_font_system(destination, true, &mut shifted_fonts);
        let plain = first_svg_text(&plain_scene).unwrap();
        let shifted = first_svg_text(&shifted_scene).unwrap();

        // Positive SVG baseline-shift raises text, while hanging changes the
        // positioning baseline via the selected document-font baseline set.
        assert!(shifted.runs[0].y_offset < plain.runs[0].y_offset - 3.0);
        assert_eq!(plain.runs[0].font_id, shifted.runs[0].font_id);
    }

    #[test]
    fn inline_svg_vertical_text_uses_quire_vertical_glyph_positioning() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="80"><text writing-mode="vertical-rl" x="10" y="5" font-size="16">AB</text></svg>"#,
        )
        .expect("vertical SVG parses");
        let usvg::Node::Text(text) = &asset.tree.root().children()[0] else {
            panic!("parser retains vertical SVG text");
        };
        assert_eq!(text.writing_mode(), usvg::WritingMode::VerticalRl);
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 30.0, 60.0),
            true,
            &mut font_system,
        );
        let line = first_svg_text(&scene).expect("vertical SVG remains native PDF text");
        assert!(
            line.runs.iter().any(|run| !run.text_matrix.is_identity()),
            "sideways vertical glyphs carry Quire's vertical inline axis in their PDF text matrix"
        );
    }

    #[test]
    fn inline_svg_vertical_text_keeps_dx_and_dy_in_svg_user_axes() {
        let plain = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="80"><text writing-mode="vertical-rl" x="10" y="5" font-size="16">A</text></svg>"#,
        )
        .unwrap();
        let positioned = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="80"><text writing-mode="vertical-rl" x="10" y="5" dx="5" dy="3" font-size="16">A</text></svg>"#,
        )
        .unwrap();
        let destination = paint_rect(0.0, 0.0, 30.0, 60.0);
        let mut plain_fonts = FontSystem::new();
        let mut positioned_fonts = FontSystem::new();
        let plain_scene =
            plain.paint_inline_group_with_font_system(destination, true, &mut plain_fonts);
        let positioned_scene = positioned.paint_inline_group_with_font_system(
            destination,
            true,
            &mut positioned_fonts,
        );
        let plain = first_svg_text(&plain_scene).expect("vertical SVG remains native PDF text");
        let positioned =
            first_svg_text(&positioned_scene).expect("vertical SVG remains native PDF text");
        let plain_glyph = &plain.runs[0].glyphs.as_ref().unwrap()[0];
        let positioned_glyph = &positioned.runs[0].glyphs.as_ref().unwrap()[0];
        let plain_offset = plain.runs[0].text_matrix.transform_local_point(
            crate::document::paint::text::TextRunPoint::new(
                plain_glyph.x_offset,
                plain_glyph.y_offset,
            ),
        );
        let positioned_offset = positioned.runs[0].text_matrix.transform_local_point(
            crate::document::paint::text::TextRunPoint::new(
                positioned_glyph.x_offset,
                positioned_glyph.y_offset,
            ),
        );

        // SVG `dx` moves right and `dy` moves down in the source user space,
        // even though Quire's sideways glyph run has a rotated inline axis.
        assert!(positioned_offset.x > plain_offset.x + 0.1);
        assert!(positioned_offset.y < plain_offset.y - 0.1);
    }

    #[test]
    fn inline_svg_upright_vertical_text_length_scales_the_vertical_inline_axis() {
        let plain = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="120"><text writing-mode="vertical-rl" text-orientation="upright" x="10" y="5" font-size="16">AB</text></svg>"#,
        )
        .unwrap();
        let adjusted = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="120"><text writing-mode="vertical-rl" text-orientation="upright" x="10" y="5" font-size="16" textLength="48" lengthAdjust="spacingAndGlyphs">AB</text></svg>"#,
        )
        .unwrap();
        let destination = paint_rect(0.0, 0.0, 30.0, 90.0);
        let mut plain_fonts = FontSystem::new();
        let mut adjusted_fonts = FontSystem::new();
        let plain_scene =
            plain.paint_inline_group_with_font_system(destination, true, &mut plain_fonts);
        let adjusted_scene =
            adjusted.paint_inline_group_with_font_system(destination, true, &mut adjusted_fonts);
        let plain = first_svg_text(&plain_scene).expect("plain upright vertical SVG is native");
        let adjusted =
            first_svg_text(&adjusted_scene).expect("adjusted upright vertical SVG is native");
        assert_eq!(
            plain.runs.len(),
            2,
            "upright units retain individual origins"
        );
        assert_eq!(adjusted.runs.len(), 2);

        let plain_distance = (plain.runs[1].y_offset - plain.runs[0].y_offset).abs();
        let adjusted_distance = (adjusted.runs[1].y_offset - adjusted.runs[0].y_offset).abs();
        assert!(
            adjusted_distance > plain_distance * 1.5,
            "the second upright unit moves along the vertical SVG inline axis"
        );
        let [_, plain_b, _, plain_d] = plain.runs[0].text_matrix.pdf_components();
        let [_, adjusted_b, _, adjusted_d] = adjusted.runs[0].text_matrix.pdf_components();
        assert!(
            adjusted_b.abs() > plain_b.abs() * 1.1 || adjusted_d.abs() > plain_d.abs() * 1.1,
            "upright glyph geometry scales on the vertical text-space axis"
        );
    }

    #[test]
    fn inline_svg_upright_vertical_text_anchor_moves_along_the_inline_axis() {
        let start = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="120"><text writing-mode="vertical-rl" text-orientation="upright" x="10" y="60" font-size="16">AB</text></svg>"#,
        )
        .unwrap();
        let middle = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="120"><text writing-mode="vertical-rl" text-orientation="upright" text-anchor="middle" x="10" y="60" font-size="16">AB</text></svg>"#,
        )
        .unwrap();
        let destination = paint_rect(0.0, 0.0, 30.0, 90.0);
        let mut start_fonts = FontSystem::new();
        let mut middle_fonts = FontSystem::new();
        let start = start.paint_inline_group_with_font_system(destination, true, &mut start_fonts);
        let middle =
            middle.paint_inline_group_with_font_system(destination, true, &mut middle_fonts);
        let start = first_svg_text(&start).expect("start-aligned vertical SVG is native");
        let middle = first_svg_text(&middle).expect("middle-aligned vertical SVG is native");
        assert!(
            middle.runs[0].y_offset > start.runs[0].y_offset + 0.1,
            "vertical text-anchor moves the text origin upward along SVG's inline axis"
        );
        assert!(
            (middle.runs[0].x_offset - start.runs[0].x_offset).abs() < 0.1,
            "vertical text-anchor does not become a horizontal translation"
        );
    }

    #[test]
    fn parser_retains_vertical_lr_distinct_from_vertical_rl() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="80"><text writing-mode="vertical-lr" x="10" y="5" font-size="16">A</text></svg>"#,
        )
        .expect("vertical SVG parses");
        let usvg::Node::Text(text) = &asset.tree.root().children()[0] else {
            panic!("parser retains vertical SVG text");
        };
        assert_eq!(text.writing_mode(), usvg::WritingMode::VerticalLr);
    }

    #[test]
    fn parser_retains_sideways_writing_modes_for_shared_text_layout() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="80"><text writing-mode="sideways-lr" x="10" y="5" font-size="16">A</text></svg>"#,
        )
        .expect("sideways SVG parses");
        let usvg::Node::Text(text) = &asset.tree.root().children()[0] else {
            panic!("parser retains sideways SVG text");
        };
        assert_eq!(text.writing_mode(), usvg::WritingMode::SidewaysLr);
        let style = svg_text_style(
            &text.chunks()[0].spans()[0],
            text.writing_mode(),
            text.text_orientation(),
            text.direction(),
            text.unicode_bidi(),
            0.75,
        );
        assert_eq!(style.writing_mode, css::WritingMode::SidewaysLr);

        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 30.0, 60.0),
            true,
            &mut font_system,
        );
        assert!(
            first_svg_text(&scene).is_some(),
            "sideways SVG uses the shared native-PDF text route when its paint is representable"
        );
    }

    #[test]
    fn parser_retains_text_orientation_for_shared_vertical_shaping() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="80"><text writing-mode="vertical-rl" text-orientation="upright" x="10" y="5" font-size="16">A</text></svg>"#,
        )
        .expect("vertical SVG parses");
        let usvg::Node::Text(text) = &asset.tree.root().children()[0] else {
            panic!("parser retains vertical SVG text");
        };
        assert_eq!(text.text_orientation(), usvg::TextOrientation::Upright);

        let span = &text.chunks()[0].spans()[0];
        let style = svg_text_style(
            span,
            text.writing_mode(),
            text.text_orientation(),
            text.direction(),
            text.unicode_bidi(),
            0.75,
        );
        assert_eq!(style.text_orientation, css::TextOrientation::Upright);
    }

    #[test]
    fn parser_cascades_text_orientation_from_svg_style() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="80"><g style="text-orientation: sideways"><text writing-mode="vertical-rl" x="10" y="5" font-size="16">A</text></g></svg>"#,
        )
        .expect("styled vertical SVG parses");
        let usvg::Node::Group(group) = &asset.tree.root().children()[0] else {
            panic!("parser retains SVG group");
        };
        let usvg::Node::Text(text) = &group.children()[0] else {
            panic!("parser retains styled vertical SVG text");
        };
        assert_eq!(text.text_orientation(), usvg::TextOrientation::Sideways);
    }

    #[test]
    fn inline_svg_text_shadow_rasterizes_shaped_outlines_without_duplicate_text() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="4" y="20" font-size="16" style="text-shadow: 0 0 4px green" fill="darkgreen">Shadow</text></svg>"#,
        )
        .unwrap();
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let native_text = scene
            .items
            .iter()
            .filter(|item| matches!(item, SvgPaintItem::Text(_)))
            .count();
        let shadow_images = scene
            .items
            .iter()
            .filter_map(|item| match item {
                SvgPaintItem::RasterImage(image) => Some(image),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(native_text, 1, "only source SVG text is selectable");
        assert_eq!(
            shadow_images.len(),
            1,
            "blur uses one bounded effect surface"
        );
        assert!(
            shadow_images[0].actual_text.is_none(),
            "the decorative blur must not introduce semantic duplicate text"
        );
    }

    #[test]
    fn filtered_svg_text_uses_one_actual_text_effect_image() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="blur" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feGaussianBlur stdDeviation="2"/></filter></defs><text filter="url(#blur)" x="4" y="20" font-size="16" fill="darkgreen">Filtered</text></svg>"#,
        )
        .expect("filtered SVG parses");
        fn has_raster_filter(group: &usvg::Group) -> bool {
            svg_raster_filter_plan(group.filters()).is_some()
                || group.children().iter().any(|node| match node {
                    usvg::Node::Group(group) => has_raster_filter(group),
                    _ => false,
                })
        }
        assert!(
            has_raster_filter(asset.tree.root()),
            "{:#?}",
            asset.tree.root()
        );
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        assert!(
            !scene
                .items
                .iter()
                .any(|item| matches!(item, SvgPaintItem::Text(_))),
            "a filtered subtree must not also emit unfiltered native text"
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one source-graphic filter image");
        assert_eq!(images[0].actual_text.as_deref(), Some("Filtered"));
    }

    #[test]
    fn offset_filtered_svg_text_uses_the_same_actual_text_effect_image() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="offset" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feOffset dx="8" dy="3"/></filter></defs><text filter="url(#offset)" x="4" y="20" font-size="16" fill="darkgreen">Offset</text></svg>"#,
        )
        .expect("filtered SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one source-graphic filter image");
        assert_eq!(images[0].actual_text.as_deref(), Some("Offset"));
    }

    #[test]
    fn color_matrix_filtered_svg_text_uses_the_same_actual_text_effect_image() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="matrix" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feColorMatrix type="saturate" values="0"/></filter></defs><text filter="url(#matrix)" x="4" y="20" font-size="16" fill="darkgreen">Matrix</text></svg>"#,
        )
        .expect("filtered SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one source-graphic filter image");
        assert_eq!(images[0].actual_text.as_deref(), Some("Matrix"));
    }

    #[test]
    fn svg_color_matrix_unpremultiplies_and_repremultiplies_alpha() {
        let mut pixel = [64, 32, 16, 128];
        apply_svg_color_matrix(
            &mut pixel,
            [
                0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
            ],
            false,
        );
        assert_eq!(pixel, [128, 0, 0, 128]);
    }

    #[test]
    fn component_transfer_filtered_svg_text_uses_the_same_actual_text_effect_image() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="transfer" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feComponentTransfer><feFuncR type="linear" slope="0" intercept="1"/><feFuncG type="table" tableValues="0 1"/><feFuncB type="discrete" tableValues="0 1"/></feComponentTransfer></filter></defs><text filter="url(#transfer)" x="4" y="20" font-size="16" fill="darkgreen">Transfer</text></svg>"#,
        )
        .expect("filtered SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one source-graphic filter image");
        assert_eq!(images[0].actual_text.as_deref(), Some("Transfer"));
    }

    #[test]
    fn linear_color_filter_chain_reuses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="chain" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feColorMatrix result="gray" type="saturate" values="0"/><feComponentTransfer in="gray"><feFuncR type="linear" slope="0" intercept="1"/></feComponentTransfer></filter></defs><text filter="url(#chain)" x="4" y="20" font-size="16" fill="darkgreen">Chained</text></svg>"#,
        )
        .expect("linear filtered SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        assert!(
            !scene
                .items
                .iter()
                .any(|item| matches!(item, SvgPaintItem::Text(_))),
            "a filtered chain owns the text as one semantic image"
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one bounded surface for a linear chain");
        assert_eq!(images[0].actual_text.as_deref(), Some("Chained"));
    }

    #[test]
    fn linear_blur_and_color_filter_chain_reuses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="chain" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feGaussianBlur result="soft" stdDeviation="2"/><feColorMatrix in="soft" type="saturate" values="0"/></filter></defs><text filter="url(#chain)" x="4" y="20" font-size="16" fill="darkgreen">Blurred chain</text></svg>"#,
        )
        .expect("linear blur and color filtered SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one bounded surface for a linear chain");
        assert_eq!(images[0].actual_text.as_deref(), Some("Blurred chain"));
    }

    #[test]
    fn named_blur_composited_with_source_graphic_uses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="30"><defs><filter id="composite" filterUnits="userSpaceOnUse" x="0" y="0" width="120" height="30"><feGaussianBlur stdDeviation="1" result="blur"/><feComposite in="blur" in2="SourceGraphic" operator="over"/></filter></defs><text filter="url(#composite)" x="4" y="20" font-size="16" fill="darkgreen">Binary graph</text></svg>"#,
        )
        .expect("named composite SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 90.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].actual_text.as_deref(), Some("Binary graph"));
    }

    #[test]
    fn named_blur_composited_with_source_alpha_uses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="30"><defs><filter id="composite" filterUnits="userSpaceOnUse" x="0" y="0" width="120" height="30"><feGaussianBlur stdDeviation="1" result="blur"/><feComposite in="blur" in2="SourceAlpha" operator="in"/></filter></defs><text filter="url(#composite)" x="4" y="20" font-size="16" fill="darkgreen">Alpha graph</text></svg>"#,
        )
        .expect("named source-alpha composite SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 90.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].actual_text.as_deref(), Some("Alpha graph"));
    }

    #[test]
    fn source_graphic_composited_over_a_named_blur_uses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="30"><defs><filter id="composite" filterUnits="userSpaceOnUse" x="0" y="0" width="120" height="30"><feGaussianBlur stdDeviation="1" result="blur"/><feComposite in="SourceGraphic" in2="blur" operator="over"/></filter></defs><text filter="url(#composite)" x="4" y="20" font-size="16" fill="darkgreen">Reverse graph</text></svg>"#,
        )
        .expect("reverse named composite SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 90.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].actual_text.as_deref(), Some("Reverse graph"));
    }

    #[test]
    fn linear_offset_and_color_filter_chain_reuses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="chain" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feOffset result="moved" dx="5" dy="2"/><feColorMatrix in="moved" type="saturate" values="0"/></filter></defs><text filter="url(#chain)" x="4" y="20" font-size="16" fill="darkgreen">Offset chain</text></svg>"#,
        )
        .expect("linear offset and color filtered SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one bounded surface for a linear chain");
        assert_eq!(images[0].actual_text.as_deref(), Some("Offset chain"));
    }

    #[test]
    fn linear_morphology_and_color_filter_chain_reuses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="chain" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feMorphology result="grown" operator="dilate" radius="1"/><feColorMatrix in="grown" type="saturate" values="0"/></filter></defs><text filter="url(#chain)" x="4" y="20" font-size="16" fill="darkgreen">Morphology chain</text></svg>"#,
        )
        .expect("linear morphology and color filtered SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one bounded surface for a linear chain");
        assert_eq!(images[0].actual_text.as_deref(), Some("Morphology chain"));
    }

    #[test]
    fn linear_convolution_and_color_filter_chain_reuses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="chain" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feConvolveMatrix order="1" kernelMatrix="1" preserveAlpha="true" result="sharp"/><feColorMatrix in="sharp" type="saturate" values="0"/></filter></defs><text filter="url(#chain)" x="4" y="20" font-size="16" fill="darkgreen">Convolution chain</text></svg>"#,
        )
        .expect("linear convolution and color filtered SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one bounded surface for a linear chain");
        assert_eq!(images[0].actual_text.as_deref(), Some("Convolution chain"));
    }

    #[test]
    fn drop_shadow_filtered_svg_text_reuses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="shadow" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feDropShadow dx="2" dy="1" stdDeviation="1" flood-color="red"/></filter></defs><text filter="url(#shadow)" x="4" y="20" font-size="16" fill="darkgreen">Filter shadow</text></svg>"#,
        )
        .expect("drop shadow filtered SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one bounded surface for feDropShadow");
        assert_eq!(images[0].actual_text.as_deref(), Some("Filter shadow"));
    }

    #[test]
    fn canonical_blur_flood_composite_merge_shadow_reuses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="30"><defs><filter id="shadow" filterUnits="userSpaceOnUse" x="0" y="0" width="120" height="30"><feGaussianBlur in="SourceGraphic" stdDeviation="1" result="blur"/><feOffset in="blur" dx="2" dy="1" result="offset"/><feFlood flood-color="red" result="flood"/><feComposite in="flood" in2="offset" operator="in" result="shadow"/><feMerge><feMergeNode in="shadow"/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs><text filter="url(#shadow)" x="4" y="20" font-size="16" fill="darkgreen">Merged shadow</text></svg>"#,
        )
        .expect("canonical merged shadow SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 90.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one bounded source surface for the graph");
        assert_eq!(images[0].actual_text.as_deref(), Some("Merged shadow"));
    }

    #[test]
    fn svg_drop_shadow_composites_source_over_its_offset_alpha_shadow() {
        let mut pixels = [255, 0, 0, 255, 0, 0, 0, 0];
        apply_svg_drop_shadow(&mut pixels, 2, 1, 0.0, 1, 0, CssColor::new(0, 255, 0));
        assert_eq!(pixels, [255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn svg_convolution_honors_target_edge_mode_and_preserved_alpha() {
        let mut pixels = [
            255, 0, 0, 255, // opaque red
            0, 64, 0, 128, // half-transparent green
        ];
        // The second coefficient is aligned to targetX=0. The first output
        // pixel therefore samples its right neighbor; the last pixel wraps
        // to the first. `preserveAlpha` keeps each destination coverage.
        assert!(apply_svg_convolve_matrix(
            &mut pixels,
            2,
            1,
            &[0.0, 1.0],
            2,
            1,
            0,
            0,
            1.0,
            0.0,
            usvg::filter::EdgeMode::Wrap,
            true,
            false,
        ));
        assert_eq!(pixels, [0, 128, 0, 255, 128, 0, 0, 128]);
    }

    #[test]
    fn svg_offset_exposes_transparent_black_at_the_surface_edge() {
        let mut pixels = [255, 0, 0, 255, 0, 255, 0, 255];
        apply_svg_offset(&mut pixels, 2, 1, 1, 0);
        assert_eq!(pixels, [0, 0, 0, 0, 255, 0, 0, 255]);
    }

    #[test]
    fn svg_flood_in_source_alpha_recolors_premultiplied_coverage() {
        let mut pixels = [64, 32, 16, 128];
        apply_svg_flood_in_source_alpha(&mut pixels, CssColor::new(0, 255, 0));
        assert_eq!(pixels, [0, 128, 0, 128]);
    }

    #[test]
    fn svg_composite_uses_premultiplied_source_over_and_in_equations() {
        let first = [128, 0, 0, 128];
        let second = [0, 64, 0, 64];
        let mut over = [0; 4];
        assert!(apply_svg_composite(
            &first,
            &second,
            &mut over,
            usvg::filter::CompositeOperator::Over,
        ));
        assert_eq!(over, [128, 32, 0, 160]);

        let mut inside = [0; 4];
        assert!(apply_svg_composite(
            &first,
            &second,
            &mut inside,
            usvg::filter::CompositeOperator::In,
        ));
        assert_eq!(inside, [32, 0, 0, 32]);
    }

    #[test]
    fn svg_source_alpha_surface_retains_only_coverage() {
        assert_eq!(
            svg_source_alpha_surface(&[64, 32, 16, 128, 255, 0, 0, 255]),
            [0, 0, 0, 128, 0, 0, 0, 255]
        );
    }

    #[test]
    fn flood_in_source_alpha_filter_uses_one_shaped_text_surface() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="tint" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feFlood result="flood" flood-color="blue"/><feComposite in="flood" in2="SourceAlpha" operator="in"/></filter></defs><text filter="url(#tint)" x="4" y="20" font-size="16" fill="darkgreen">Tinted</text></svg>"#,
        )
        .expect("flood and source-alpha SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].actual_text.as_deref(), Some("Tinted"));
    }

    #[test]
    fn svg_component_transfer_table_and_discrete_functions_follow_channel_ranges() {
        assert_eq!(
            apply_svg_transfer_function(0.25, &SvgTransferFunction::Table(vec![0.0, 1.0])),
            0.25
        );
        assert_eq!(
            apply_svg_transfer_function(0.25, &SvgTransferFunction::Discrete(vec![0.0, 1.0])),
            0.0
        );
        assert_eq!(
            apply_svg_transfer_function(0.75, &SvgTransferFunction::Discrete(vec![0.0, 1.0])),
            1.0
        );
    }

    #[test]
    fn morphology_filtered_svg_text_uses_the_same_actual_text_effect_image() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><defs><filter id="morphology" filterUnits="userSpaceOnUse" x="0" y="0" width="100" height="30"><feMorphology operator="dilate" radius="1"/></filter></defs><text filter="url(#morphology)" x="4" y="20" font-size="16" fill="darkgreen">Morphology</text></svg>"#,
        )
        .expect("filtered SVG parses");
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let mut images = Vec::new();
        scene.raster_images(&mut images);
        assert_eq!(images.len(), 1, "one source-graphic filter image");
        assert_eq!(images[0].actual_text.as_deref(), Some("Morphology"));
    }

    #[test]
    fn svg_morphology_dilate_and_erode_handle_transparent_surface_edges() {
        let mut dilated = [0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 0, 0];
        apply_svg_morphology(&mut dilated, 3, 1, 1, 0, true);
        assert_eq!(dilated, [255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255]);

        let mut eroded = dilated;
        apply_svg_morphology(&mut eroded, 3, 1, 1, 0, false);
        assert_eq!(eroded, [0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn inline_svg_text_decorations_use_document_metrics_and_svg_paint_order() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="4" y="20" font-size="16" fill="red" text-decoration="underline overline line-through">Decorate</text></svg>"#,
        )
        .unwrap();
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let text_index = scene
            .items
            .iter()
            .position(|item| matches!(item, SvgPaintItem::Text(_)))
            .expect("the solid source text remains native PDF text");
        let path_indices = scene
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| matches!(item, SvgPaintItem::Path(_)).then_some(index))
            .collect::<Vec<_>>();
        assert_eq!(path_indices.len(), 3);
        assert!(path_indices.iter().any(|index| *index < text_index));
        assert!(path_indices.iter().any(|index| *index > text_index));
        assert!(
            scene
                .items
                .iter()
                .filter_map(|item| match item {
                    SvgPaintItem::Path(path) => Some(path.fill),
                    _ => None,
                })
                .all(|fill| fill == Some(CssColor::new(255, 0, 0)))
        );
    }

    #[test]
    fn upright_vertical_svg_text_decorations_follow_the_vertical_inline_axis() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><text x="20" y="4" font-size="16" writing-mode="vertical-rl" text-orientation="upright" text-decoration="underline overline line-through">Vertical</text></svg>"#,
        )
        .unwrap();
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 60.0),
            true,
            &mut font_system,
        );
        let decoration_bounds = scene
            .items
            .iter()
            .filter_map(|item| match item {
                SvgPaintItem::Path(path) => path.bounds(),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(decoration_bounds.len(), 3);
        assert!(
            decoration_bounds
                .iter()
                .all(|bounds| { bounds.size.height > bounds.size.width * 4.0 })
        );
    }

    #[test]
    fn host_css_text_overrides_cross_the_inline_svg_serialization_boundary() {
        let svg = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text x="4" y="20" style="font-size: 4px; font-variation-settings: 'wght' 100; letter-spacing: 1px; word-spacing: 2px; writing-mode: horizontal-tb; text-orientation: mixed">Shadow</text></svg>"#,
        );
        let NodeKind::Element(text) = &svg.children[0].kind else {
            panic!("expected SVG text child");
        };
        let mut overrides = SvgPresentationOverrides::new();
        overrides.insert(
            text.id,
            SvgPresentationOverride {
                font_family: Some("\"Ahem\"".to_owned()),
                font_size: Some("16px".to_owned()),
                font_weight: Some("700".to_owned()),
                font_style: Some("italic".to_owned()),
                font_stretch: Some("75%".to_owned()),
                font_variation_settings: Some("\"wght\" 650".to_owned()),
                font_kerning: Some("none".to_owned()),
                letter_spacing: Some("3px".to_owned()),
                word_spacing: Some("4px".to_owned()),
                writing_mode: Some("vertical-lr".to_owned()),
                text_orientation: Some("upright".to_owned()),
                direction: Some("rtl".to_owned()),
                unicode_bidi: Some("isolate-override".to_owned()),
                text_shadow: Some("0px 0px 5px 0px rgba(0 128 0 / 1)".to_owned()),
                ..SvgPresentationOverride::default()
            },
        );
        let xml = serialize_inline_svg_with_presentation_overrides(&svg, &overrides);
        assert!(xml.contains("font-family=\"&quot;Ahem&quot;\""));
        assert!(xml.contains("font-size=\"16px\""));
        assert!(xml.contains("font-weight=\"700\""));
        assert!(xml.contains("font-style=\"italic\""));
        assert!(xml.contains("font-stretch=\"75%\""));
        assert!(xml.contains("font-variation-settings=\"&quot;wght&quot; 650\""));
        assert!(xml.contains("letter-spacing=\"3px\""));
        assert!(xml.contains("word-spacing=\"4px\""));
        assert!(xml.contains("style=\"font-kerning: none;\""));
        assert!(xml.contains("writing-mode=\"vertical-lr\""));
        assert!(xml.contains("text-orientation=\"upright\""));
        assert!(!xml.contains("font-size: 4px"));
        assert!(!xml.contains("font-variation-settings: 'wght' 100"));
        assert!(!xml.contains("letter-spacing: 1px"));
        assert!(!xml.contains("word-spacing: 2px"));
        assert!(!xml.contains("writing-mode: horizontal-tb"));
        assert!(!xml.contains("text-orientation: mixed"));
        assert!(xml.contains("direction=\"rtl\""));
        assert!(xml.contains("unicode-bidi=\"isolate-override\""));
        assert!(xml.contains("text-shadow=\"0px 0px 5px 0px rgba(0 128 0 / 1)\""));

        let asset = parse_svg_bytes(xml.as_bytes()).expect("serialized SVG parses");
        let usvg::Node::Text(text) = &asset.tree.root().children()[0] else {
            panic!("serialized text remains retained");
        };
        assert!(
            text.chunks()[0].spans()[0]
                .font()
                .variations()
                .contains(&usvg::FontVariation::new(*b"wght", 650.0)),
            "the bridged axis survives alongside style-derived font axes"
        );
        assert!(!text.chunks()[0].spans()[0].apply_kerning());
        assert_eq!(text.writing_mode(), usvg::WritingMode::VerticalLr);
        assert_eq!(text.text_orientation(), usvg::TextOrientation::Upright);
        let mut fonts = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut fonts,
        );
        assert!(
            scene
                .items
                .iter()
                .any(|item| matches!(item, SvgPaintItem::RasterImage(_))),
            "a bridged blurred shadow crosses the boundary as one effect image"
        );
        assert_eq!(
            scene
                .items
                .iter()
                .filter(|item| matches!(item, SvgPaintItem::Text(_)))
                .count(),
            1
        );
    }

    #[test]
    fn inline_svg_text_span_retains_its_typed_host_typography_key() {
        let svg = svg_element(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="4" y="20">Keyed text</text></svg>"#,
        );
        let NodeKind::Element(text) = &svg.children[0].kind else {
            panic!("expected SVG text child");
        };
        let mut host_style = ComputedStyle::initial();
        host_style.font_weight = FontWeight(650);
        host_style.font_feature_settings =
            css::FontFeatureSettings(vec![css::FontFeatureSetting::new(*b"liga", 0)]);
        host_style.font_synthesis = css::FontSynthesis::NONE;
        host_style.font_language_override = css::FontLanguageOverride::OpenType(*b"TRK ");
        host_style.font_palette = css::FontPalette::Index(2);

        let mut overrides = SvgPresentationOverrides::new();
        let key = overrides.record_typography(SvgTextTypography::from_computed_style(&host_style));
        overrides.insert(
            text.id,
            SvgPresentationOverride {
                text_typography_key: Some(key),
                ..SvgPresentationOverride::default()
            },
        );
        let asset = parse_inline_svg_with_presentation_overrides(
            &svg,
            &overrides,
            &ExternalSvgUseResolver::default(),
        )
        .expect("keyed inline SVG parses");
        let usvg::Node::Text(text) = &asset.tree.root().children()[0] else {
            panic!("expected retained SVG text");
        };
        assert_eq!(
            text.chunks()[0].spans()[0].text_typography_key(),
            Some(key.0)
        );

        let restored = asset
            .text_typography
            .get(&key)
            .expect("asset retains host typography")
            .computed_style_at_font_scale(SvgFontScale(1.0));
        assert_eq!(restored.font_weight, FontWeight(650));
        assert_eq!(
            restored.font_feature_settings,
            host_style.font_feature_settings
        );
        assert_eq!(restored.font_synthesis, css::FontSynthesis::NONE);
        assert_eq!(
            restored.font_language_override,
            css::FontLanguageOverride::OpenType(*b"TRK ")
        );
        assert_eq!(restored.font_palette, css::FontPalette::Index(2));
    }

    #[test]
    fn inline_svg_text_rotation_uses_quire_glyph_outlines_with_actual_text() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="4" y="20" rotate="30" font-size="16">Rotate</text></svg>"#,
        )
        .unwrap();
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let [SvgPaintItem::OutlinedText(outlined)] = scene.items.as_slice() else {
            panic!(
                "expected one outlined rotated SVG text item, got {:?}",
                scene.items
            );
        };
        assert_eq!(outlined.actual_text.as_ref(), "Rotate");
        assert!(!outlined.paths.is_empty());
    }

    #[test]
    fn inline_svg_text_stroke_uses_the_same_shaped_glyph_outlines() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30"><text x="4" y="20" font-size="16" fill="red" stroke="blue" stroke-width="2" paint-order="stroke fill">Stroke</text></svg>"#,
        )
        .unwrap();
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let [SvgPaintItem::OutlinedText(outlined)] = scene.items.as_slice() else {
            panic!(
                "expected one outlined stroked SVG text item, got {:?}",
                scene.items
            );
        };
        assert!(
            outlined
                .paths
                .iter()
                .all(|path| path.fill == Some(CssColor::new(255, 0, 0)))
        );
        assert!(
            outlined
                .paths
                .iter()
                .all(|path| path.stroke == Some(CssColor::new(0, 0, 255)))
        );
        assert!(outlined.paths.iter().all(|path| {
            path.stroke_width != PaintStrokeWidth::ZERO
                && path.paint_order == RenderedPathPaintOrder::StrokeThenFill
        }));
        assert!(
            outlined.paths.iter().all(|path| path.transform.d() > 0.0),
            "outline fallback must use the same upright glyph basis as native SVG text"
        );
    }

    #[test]
    fn inline_svg_text_path_places_quire_shaped_glyphs_on_the_retained_path() {
        let asset = parse_svg_bytes(
            br##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="30"><path id="guide" d="M 0 20 H 100"/><text x="0" y="0" font-size="12"><textPath xlink:href="#guide">Path</textPath></text></svg>"##,
        )
        .unwrap();
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 22.5),
            true,
            &mut font_system,
        );
        let outlined = scene.items.iter().find_map(|item| match item {
            SvgPaintItem::OutlinedText(outlined) => Some(outlined),
            _ => None,
        });
        let outlined = outlined.expect("textPath must retain Quire-shaped outline glyphs");
        assert_eq!(outlined.actual_text.as_ref(), "Path");
        assert!(!outlined.paths.is_empty());
        assert!(outlined.paths.iter().all(|path| path.bounds().is_some()));
    }

    #[test]
    fn inline_svg_text_length_scales_the_pdf_inline_axis() {
        let plain = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><text x="0" y="15" font-size="12">Length</text></svg>"#,
        )
        .unwrap();
        let adjusted = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><text x="0" y="15" font-size="12" textLength="60" lengthAdjust="spacingAndGlyphs">Length</text></svg>"#,
        )
        .unwrap();
        let mut plain_fonts = FontSystem::new();
        let mut adjusted_fonts = FontSystem::new();
        let destination = paint_rect(0.0, 0.0, 75.0, 15.0);
        let plain_scene =
            plain.paint_inline_group_with_font_system(destination, true, &mut plain_fonts);
        let adjusted_scene =
            adjusted.paint_inline_group_with_font_system(destination, true, &mut adjusted_fonts);
        let plain = first_svg_text(&plain_scene).unwrap();
        let adjusted = first_svg_text(&adjusted_scene).unwrap();
        let plain_inline = plain.runs[0].text_matrix.pdf_components()[0].abs();
        let adjusted_inline = adjusted.runs[0].text_matrix.pdf_components()[0].abs();
        assert!(adjusted_inline > plain_inline * 1.5);
    }

    #[test]
    fn nested_svg_image_text_uses_the_owning_document_font_system() {
        let nested = base64::engine::general_purpose::STANDARD.encode(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="20"><text x="2" y="15" font-size="12">Nested</text></svg>"#,
        );
        let outer = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><image href="data:image/svg+xml;base64,{nested}" width="30" height="20"/></svg>"#
        );
        let asset = parse_svg_bytes(outer.as_bytes()).unwrap();
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 30.0, 22.5),
            true,
            &mut font_system,
        );
        let line = first_svg_text(&scene).expect("nested text run");
        assert_eq!(line.text, "Nested");
        assert!(line.runs.iter().all(|run| run.font_id.is_some()));
    }

    #[test]
    fn gradient_svg_text_lowers_quire_shaped_glyphs_to_outlines() {
        let asset = parse_svg_bytes(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs><text x="2" y="15" font-size="12" fill="url(#g)">Gradient</text></svg>"#,
        )
        .unwrap();
        let mut font_system = FontSystem::new();
        let scene = asset.paint_inline_group_with_font_system(
            paint_rect(0.0, 0.0, 75.0, 15.0),
            true,
            &mut font_system,
        );
        let outlined = scene.items.iter().find_map(|item| match item {
            SvgPaintItem::OutlinedText(outlined) => Some(outlined),
            _ => None,
        });
        let outlined = outlined.expect("gradient text outline item");
        assert_eq!(outlined.actual_text.as_ref(), "Gradient");
        assert!(!outlined.paths.is_empty());
        assert!(
            outlined
                .paths
                .iter()
                .all(|path| matches!(path.fill_paint, Some(RenderedPathPaint::Gradient(_))))
        );
    }

    fn first_svg_text(
        group: &SvgPaintGroup,
    ) -> Option<&crate::document::paint::text::RenderedLine> {
        group.items.iter().find_map(|item| match item {
            SvgPaintItem::Text(line) => Some(line.as_ref()),
            SvgPaintItem::Group(group) | SvgPaintItem::NestedSvg(group) => first_svg_text(group),
            SvgPaintItem::Path(_)
            | SvgPaintItem::RasterImage(_)
            | SvgPaintItem::OutlinedText(_) => None,
        })
    }
}
