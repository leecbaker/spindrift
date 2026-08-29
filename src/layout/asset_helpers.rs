use std::rc::Rc;

use super::*;
use crate::image_store::ImageId;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct DecodedPngImage {
    pub(in crate::layout) image_id: Option<ImageId>,
    /// Source samples used for image paint and PDF resource emission.
    pub(in crate::layout) pixel_size: RasterPixelSize,
    /// Optional source crop in the image's original pixel grid. The cached
    /// resource stays whole; PDF emission applies this crop at draw time.
    pub(in crate::layout) source_rect: Option<RenderedImageSourceRect>,
    /// Preferred natural dimensions used by CSS sizing algorithms.
    pub(in crate::layout) natural_size: crate::units::CssPixelSize,
    /// The depth shared by the RGB and optional alpha sample planes.
    pub(in crate::layout) sample_depth: crate::image_store::RasterSampleDepth,
    /// Byte-encoded raster samples, never CSS coordinates. Generated CSS
    /// images construct this only after their explicit output-space encoding.
    pub(in crate::layout) rgb: EncodedRasterRgbSamples,
    pub(in crate::layout) alpha: Option<Rc<[u8]>>,
    pub(in crate::layout) color_space: crate::color::RasterColorSpace,
}

/// Encoded RGB samples paired with `DecodedPngImage::color_space`.
///
/// The wrapper prevents image payloads from being confused with CSS component
/// triples, which may be wide-gamut, unbounded, or D50 PCS coordinates.

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct EncodedRasterRgbSamples(Rc<[u8]>);

impl EncodedRasterRgbSamples {
    pub(in crate::layout) fn new(samples: Vec<u8>) -> Self {
        Self(Rc::from(samples.into_boxed_slice()))
    }

    pub(in crate::layout) fn from_shared(samples: Rc<[u8]>) -> Self {
        Self(samples)
    }

    pub(in crate::layout) fn shared(&self) -> Rc<[u8]> {
        Rc::clone(&self.0)
    }
}

impl AsRef<[u8]> for EncodedRasterRgbSamples {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl std::ops::Deref for EncodedRasterRgbSamples {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq<Vec<u8>> for EncodedRasterRgbSamples {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.as_ref() == other.as_slice()
    }
}

impl DecodedPngImage {
    pub(in crate::layout) fn new(
        pixel_width: u32,
        pixel_height: u32,
        rgb: Vec<u8>,
        alpha: Option<Vec<u8>>,
    ) -> Self {
        Self {
            image_id: None,
            pixel_size: RasterPixelSize::new(pixel_width, pixel_height),
            source_rect: None,
            natural_size: crate::units::CssPixelSize::new(pixel_width, pixel_height),
            sample_depth: crate::image_store::RasterSampleDepth::Eight,
            rgb: EncodedRasterRgbSamples::new(rgb),
            alpha: alpha.map(|alpha| Rc::from(alpha.into_boxed_slice())),
            color_space: crate::color::RasterColorSpace::SRGB,
        }
    }

    pub(in crate::layout) fn in_color_space(
        mut self,
        color_space: crate::css::CssColorSpace,
    ) -> Self {
        self.color_space = crate::color::RasterColorSpace::BuiltIn(color_space);
        self
    }

    pub(in crate::layout) fn natural_layout_size(&self) -> crate::units::LayoutSize {
        crate::units::css_pixel_natural_layout_size(self.natural_size)
    }

    pub(in crate::layout) fn with_source_rect(
        mut self,
        source_rect: RenderedImageSourceRect,
    ) -> Self {
        self.natural_size = crate::units::CssPixelSize::new(source_rect.width, source_rect.height);
        self.source_rect = Some(source_rect);
        self
    }
}
use crate::LayoutSize;
use crate::document::paint::patterns::RenderedImageSourceRect;
use crate::image_store::RasterOrientationPolicy;
use crate::layout::assets::rasterize_generated_css_image;
use crate::svg::SvgImageContext;
use crate::units::{IntoLayoutLength, LayoutLength, layout_px};

/// High-precision displacement of a background tile in page-local paint space.
///
/// Background positioning keeps this separate from generic primitive
/// translations because percentages may be resolved against extremely large
/// free space before the tile is clipped to a page.
pub(in crate::layout) type PaintBackgroundOffset = euclid::Vector2D<f64, PaintSpace>;

/// A URL-backed image that remains raster or vector through layout.
#[derive(Debug, Clone)]
pub(super) enum ResolvedImageAsset {
    Raster(DecodedPngImage),
    Svg(SharedSvgAsset),
}

/// The used result of a CSS URL-bearing image form.
///
/// This is the boundary at which CSS Images' invalid-source fallback becomes
/// observable. Consumers retain their own handling for generated gradients,
/// but all URL and `image()` users share this source/fallback selection.
#[derive(Debug, Clone)]
pub(super) enum ResolvedCssImage {
    External(ResolvedImageAsset),
    SolidColor(CssColor),
    Invalid,
}

/// A parsed Media Fragments source rectangle. This deliberately stays in
/// source-pixel coordinates rather than leaking into CSS layout or PDF paint
/// coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ImagePixelRect {
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ParsedImageFragment {
    None,
    Xywh(ImagePixelRect),
    Other(String),
}

/// Inputs common to every CSS `image()` source resolution.
pub(super) struct ImageResolutionContext<'a> {
    pub(super) base_url: Option<&'a url::Url>,
    pub(super) root_url: Option<&'a url::Url>,
    pub(super) current_color: CssColor,
    pub(super) orientation: RasterOrientationPolicy,
    pub(super) svg_context: SvgImageContext,
    pub(super) resource_cache: &'a ResourceCache,
}

/// Resolve a direct URL or CSS Images 5 `image()` value after image-set and
/// light-dark selection. A fallback color is used only when its source cannot
/// be loaded as an image; it never paints beneath a successfully loaded
/// source.
pub(super) fn resolve_css_image_source(
    image: &css::BackgroundImage,
    context: ImageResolutionContext<'_>,
) -> ResolvedCssImage {
    let image = image.selected_image();
    let resolve_url = |url: &css::ImageUrl| {
        load_resolved_image_source_with_request(
            &url.href,
            url.base_url.as_ref().or(context.base_url),
            url.root_url.as_ref().or(context.root_url),
            context.resource_cache,
            context.orientation,
            context.svg_context,
            &url.request_modifiers,
        )
    };
    match image {
        css::BackgroundImage::Url(url) => resolve_url(url)
            .map(ResolvedCssImage::External)
            .unwrap_or(ResolvedCssImage::Invalid),
        css::BackgroundImage::ImageFunction(function) => match function.source.as_ref() {
            Some(source) => resolve_image_function_source(source, &context)
                .map(ResolvedCssImage::External)
                .or_else(|| {
                    function.fallback_color.map(|color| {
                        ResolvedCssImage::SolidColor(color.resolve(context.current_color))
                    })
                })
                .unwrap_or(ResolvedCssImage::Invalid),
            None => function
                .fallback_color
                .map(|color| ResolvedCssImage::SolidColor(color.resolve(context.current_color)))
                .unwrap_or(ResolvedCssImage::Invalid),
        },
        _ => ResolvedCssImage::Invalid,
    }
}

fn resolve_image_function_source(
    source: &css::ImageUrl,
    context: &ImageResolutionContext<'_>,
) -> Option<ResolvedImageAsset> {
    let fragment = parse_image_fragment(&source.href)?;
    match fragment {
        ParsedImageFragment::None => load_resolved_image_source_with_request(
            &source.href,
            source.base_url.as_ref().or(context.base_url),
            source.root_url.as_ref().or(context.root_url),
            context.resource_cache,
            context.orientation,
            context.svg_context,
            &source.request_modifiers,
        ),
        ParsedImageFragment::Other(_) => {
            // Existing SVG view-fragment support remains available. Other
            // raster fragments have no CSS Images 5 meaning and therefore
            // fail this `image()` source (allowing its fallback color).
            match load_resolved_image_source_with_request(
                &source.href,
                source.base_url.as_ref().or(context.base_url),
                source.root_url.as_ref().or(context.root_url),
                context.resource_cache,
                context.orientation,
                context.svg_context,
                &source.request_modifiers,
            )? {
                asset @ ResolvedImageAsset::Svg(_) => Some(asset),
                ResolvedImageAsset::Raster(_) => None,
            }
        }
        ParsedImageFragment::Xywh(rect) => {
            let href = source
                .href
                .split_once('#')
                .map_or(source.href.as_str(), |(href, _)| href);
            let asset = load_resolved_image_source_with_request(
                href,
                source.base_url.as_ref().or(context.base_url),
                source.root_url.as_ref().or(context.root_url),
                context.resource_cache,
                context.orientation,
                context.svg_context,
                &source.request_modifiers,
            )?;
            match asset {
                ResolvedImageAsset::Raster(image) => {
                    clamp_raster_fragment(image, rect).map(ResolvedImageAsset::Raster)
                }
                // SVG crop paint uses a source viewport rather than bitmap
                // pixels. It is not safe to silently ignore `xywh`: retain
                // invalid-source behavior until that adapter is selected.
                ResolvedImageAsset::Svg(_) => None,
            }
        }
    }
}

fn parse_image_fragment(href: &str) -> Option<ParsedImageFragment> {
    let Some((_, fragment)) = href.split_once('#') else {
        return Some(ParsedImageFragment::None);
    };
    let Some(value) = fragment.strip_prefix("xywh=") else {
        return Some(ParsedImageFragment::Other(fragment.to_string()));
    };
    // CSS Images 5 requires the unqualified integer form. Units, percent
    // coordinates, and extra media-fragment dimensions are invalid here.
    let mut values = value.split(',').map(str::trim);
    let x = values.next()?.parse::<u32>().ok()?;
    let y = values.next()?.parse::<u32>().ok()?;
    let width = values.next()?.parse::<u32>().ok()?;
    let height = values.next()?.parse::<u32>().ok()?;
    if values.next().is_some() || width == 0 || height == 0 {
        return None;
    }
    Some(ParsedImageFragment::Xywh(ImagePixelRect {
        x,
        y,
        width,
        height,
    }))
}

fn clamp_raster_fragment(
    image: DecodedPngImage,
    fragment: ImagePixelRect,
) -> Option<DecodedPngImage> {
    let width = image.pixel_size.width;
    let height = image.pixel_size.height;
    if fragment.x >= width || fragment.y >= height {
        return None;
    }
    let right = fragment.x.saturating_add(fragment.width).min(width);
    let bottom = fragment.y.saturating_add(fragment.height).min(height);
    let source_rect = RenderedImageSourceRect {
        x: fragment.x,
        y: fragment.y,
        width: right.checked_sub(fragment.x)?,
        height: bottom.checked_sub(fragment.y)?,
    };
    (source_rect.width > 0 && source_rect.height > 0).then(|| image.with_source_rect(source_rect))
}

/// Select the requested image representation from computed CSS.  A URL
/// loader may strengthen `Encoded` to `FromImage` for an opaque response.
pub(super) const fn raster_orientation_policy(
    orientation: css::ImageOrientation,
) -> RasterOrientationPolicy {
    match orientation {
        css::ImageOrientation::FromImage => RasterOrientationPolicy::FromImage,
        css::ImageOrientation::None => RasterOrientationPolicy::Encoded,
    }
}

impl ResolvedImageAsset {
    pub(super) fn intrinsic_size(&self) -> LayoutSize {
        match self {
            Self::Raster(image) => image.natural_layout_size(),
            Self::Svg(asset) => asset.intrinsic_size(),
        }
    }
}

#[cfg(test)]
pub(super) fn load_resolved_image_source(
    src: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    orientation_policy: RasterOrientationPolicy,
    image_context: SvgImageContext,
) -> Option<ResolvedImageAsset> {
    load_resolved_image_source_with_request(
        src,
        base_url,
        root_url,
        resource_cache,
        orientation_policy,
        image_context,
        &css::RequestUrlModifiers::default(),
    )
}

/// Resolve a CSS URL image after enforcing request URL modifiers.
/// <https://drafts.csswg.org/css-values-5/#request-url-modifiers>
pub(super) fn load_resolved_image_source_with_request(
    src: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    orientation_policy: RasterOrientationPolicy,
    image_context: SvgImageContext,
    request_modifiers: &css::RequestUrlModifiers,
) -> Option<ResolvedImageAsset> {
    let (asset, svg_fragment) = if src.starts_with("data:") {
        (
            resource_cache.data_image_asset_with_orientation(
                src,
                orientation_policy,
                image_context,
            )?,
            None,
        )
    } else {
        let url = crate::resource::resolve_url(src, base_url, root_url)?;
        let taint =
            resource_cache.css_image_fetch_taint(&url, root_url.or(base_url), request_modifiers)?;
        let orientation_policy = orientation_policy_for_taint(orientation_policy, taint);
        let svg_fragment = url.fragment().map(str::to_owned);
        (
            resource_cache.image_asset_url_with_orientation(
                &url,
                orientation_policy,
                image_context,
            )?,
            svg_fragment,
        )
    };
    Some(match asset {
        crate::resource::ResourceImageAsset::Raster { image_id, metadata } => {
            ResolvedImageAsset::Raster(DecodedPngImage {
                image_id: Some(image_id),
                pixel_size: metadata.pixel_size,
                source_rect: None,
                natural_size: metadata.natural_size,
                sample_depth: crate::image_store::RasterSampleDepth::Eight,
                rgb: EncodedRasterRgbSamples::from_shared(resource_cache.image_placeholder_rgb()),
                alpha: None,
                color_space: crate::color::RasterColorSpace::SRGB,
            })
        }
        crate::resource::ResourceImageAsset::Svg(asset) => {
            ResolvedImageAsset::Svg(Rc::new(asset.with_view_fragment(svg_fragment.as_deref())))
        }
    })
}

/// Resolve the requested CSS representation against the fetch taint before an
/// image can enter either raster cache.
const fn orientation_policy_for_taint(
    requested: RasterOrientationPolicy,
    taint: crate::resource::ImageFetchTaint,
) -> RasterOrientationPolicy {
    match (requested, taint) {
        (RasterOrientationPolicy::Encoded, crate::resource::ImageFetchTaint::Opaque) => {
            RasterOrientationPolicy::FromImage
        }
        (policy, _) => policy,
    }
}

#[cfg(test)]
pub(super) fn load_image_source(
    src: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    apply_orientation: bool,
) -> Option<DecodedPngImage> {
    load_image_source_with_request(
        src,
        base_url,
        root_url,
        resource_cache,
        if apply_orientation {
            RasterOrientationPolicy::FromImage
        } else {
            RasterOrientationPolicy::Encoded
        },
        &css::RequestUrlModifiers::default(),
    )
}

pub(super) fn load_image_source_with_request(
    src: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    orientation_policy: RasterOrientationPolicy,
    request_modifiers: &css::RequestUrlModifiers,
) -> Option<DecodedPngImage> {
    match load_resolved_image_source_with_request(
        src,
        base_url,
        root_url,
        resource_cache,
        orientation_policy,
        SvgImageContext::default(),
        request_modifiers,
    )? {
        ResolvedImageAsset::Raster(image) => Some(image),
        ResolvedImageAsset::Svg(_) => None,
    }
}

/// Source identity retained by a used replacement box.
///
/// A CSS `content` image can be a raster, SVG, or generated gradient.  Keep
/// this distinction after sizing so painting can use the same concrete object
/// geometry without forcing a vector-capable gradient through a raster image.
/// The raster member of [`Self::Gradient`] is a bounded fallback for paint
/// backends that cannot represent the CSS interpolation exactly.
/// <https://drafts.csswg.org/css-content-3/#content-property>
/// <https://drafts.csswg.org/css-images-3/#sizing>
#[derive(Debug, Clone)]
pub(super) enum UsedReplacementImageSource {
    Raster,
    Svg,
    Gradient { image: Box<BackgroundImage> },
}

/// Used CSS size and paint payload for an HTML image replaced element.
///
/// CSS Images defines raster images as replaced elements with intrinsic
/// dimensions, while CSS Sizing/Box Sizing define how `width`, `height`,
/// padding, and borders resolve to content-box and border-box sizes:
/// <https://www.w3.org/TR/css-images-3/#sizing> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(super) struct UsedImage {
    pub(super) decoded: DecodedPngImage,
    pub(super) svg: Option<SharedSvgAsset>,
    pub(super) source: UsedReplacementImageSource,
    pub(super) content_size: ContentBoxSize,
    pub(super) border_box_size: BorderBoxSize,
}

/// Source-local coordinate space for a resolved CSS `object-view-box`.
///
/// The rectangle is normalized to the image's natural width and height, but
/// deliberately permits origins outside the source image. CSS Images allows a
/// view box to pan beyond the source; only a non-positive used width or height
/// makes the view box ineffective.
/// <https://drafts.csswg.org/css-images-5/#the-object-view-box-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum ObjectViewBoxSourceSpace {}

pub(in crate::layout) type NormalizedObjectSourcePoint =
    euclid::Point2D<f32, ObjectViewBoxSourceSpace>;
pub(in crate::layout) type NormalizedObjectSourceSize =
    euclid::Size2D<f32, ObjectViewBoxSourceSpace>;
pub(in crate::layout) type NormalizedObjectSourceRect = euclid::Rect<f32, ObjectViewBoxSourceSpace>;

/// The effective CSS Images source selection after resolving `object-view-box`.
///
/// `NoEffect` covers `none`, source images without both natural axes, and
/// basic rectangles with a non-positive or non-finite used extent. In each
/// case the image retains its full source and receives no view-box-specific
/// clip.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) enum ResolvedObjectViewBox {
    NoEffect,
    Source {
        rect: NormalizedObjectSourceRect,
        radii: Option<css::BorderRadius>,
    },
}

impl ResolvedObjectViewBox {
    pub(in crate::layout) fn source_rect(&self) -> NormalizedObjectSourceRect {
        match self {
            Self::NoEffect => NormalizedObjectSourceRect::new(
                NormalizedObjectSourcePoint::new(0.0, 0.0),
                NormalizedObjectSourceSize::new(1.0, 1.0),
            ),
            Self::Source { rect, .. } => *rect,
        }
    }

    pub(in crate::layout) fn effective_natural_size(&self, natural_size: LayoutSize) -> LayoutSize {
        let source = self.source_rect();
        LayoutSize::new(
            natural_size.width * source.size.width,
            natural_size.height * source.size.height,
        )
    }

    pub(in crate::layout) fn radii(&self) -> Option<&css::BorderRadius> {
        match self {
            Self::NoEffect => None,
            Self::Source { radii, .. } => radii.as_ref(),
        }
    }

    pub(in crate::layout) fn applies(&self) -> bool {
        matches!(self, Self::Source { .. })
    }
}

/// Used CSS box geometry shared by replaced elements without a painted image
/// resource, such as HTML `<canvas>`.
///
/// The content and border boxes stay distinct so each formatting context can
/// supply its own margins without accidentally adding padding or borders a
/// second time. CSS sizing resolves replaced-element dimensions in the content
/// box, while CSS Box Sizing converts that result to the border box:
/// <https://www.w3.org/TR/css-images-3/#default-sizing> and
/// <https://www.w3.org/TR/css-sizing-3/#box-models>.
#[derive(Debug, Clone, Copy)]
pub(super) struct UsedReplacedBox {
    pub(super) content_size: ContentBoxSize,
    pub(super) border_box_size: BorderBoxSize,
}

impl UsedReplacedBox {
    fn new(
        content_size: ContentBoxSize,
        horizontal_non_content: NonContentLength,
        vertical_non_content: NonContentLength,
    ) -> Self {
        Self {
            border_box_size: content_box_to_border_box_size(
                content_size,
                horizontal_non_content,
                vertical_non_content,
            ),
            content_size,
        }
    }
}

/// Intrinsic dimensions and HTML sizing hints used by all replaced flex-item
/// sizing paths.
///
/// Image resources, canvas elements, and inline SVG all provide this same
/// geometry to Flexbox. Their paint resources remain format-specific, but the
/// flex base-size and automatic-minimum algorithms only need these dimensions.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
#[derive(Debug, Clone, Copy)]
pub(super) struct IntrinsicReplacedSize {
    pub(super) width: ContentBoxLength,
    pub(super) height: ContentBoxLength,
    /// The source's preferred aspect ratio, when it has one. Default iframe
    /// dimensions deliberately do not establish this relationship.
    pub(super) preferred_aspect_ratio: Option<f32>,
    /// Whether the resource supplies either intrinsic dimension rather than
    /// only a preferred aspect ratio and CSS's default object size.
    pub(super) has_intrinsic_size: bool,
    pub(super) attr_width: Option<ContentBoxLength>,
    pub(super) attr_height: Option<ContentBoxLength>,
}

impl IntrinsicReplacedSize {
    /// Applies CSS `zoom` to natural and presentation-attribute dimensions
    /// while preserving an intrinsic aspect ratio.
    pub(super) fn scaled_by_effective_zoom(mut self, factor: f32) -> Self {
        self.width *= factor;
        self.height *= factor;
        self.attr_width = self.attr_width.map(|width| width * factor);
        self.attr_height = self.attr_height.map(|height| height * factor);
        self
    }

    pub(super) fn natural_aspect_ratio(self) -> Option<f32> {
        self.preferred_aspect_ratio
    }

    pub(super) fn attribute_aspect_ratio(self) -> Option<f32> {
        self.attr_width
            .zip(self.attr_height)
            .filter(|(width, height)| *width > content_box_pt(0.0) && *height > content_box_pt(0.0))
            .map(|(width, height)| width.points() / height.points())
    }
}

impl UsedImage {
    fn new(
        decoded: DecodedPngImage,
        content_size: ContentBoxSize,
        horizontal_non_content: NonContentLength,
        vertical_non_content: NonContentLength,
    ) -> Self {
        let border_box_size = content_box_to_border_box_size(
            content_size,
            horizontal_non_content,
            vertical_non_content,
        );
        Self {
            decoded,
            svg: None,
            source: UsedReplacementImageSource::Raster,
            content_size,
            border_box_size,
        }
    }

    fn from_geometry(decoded: DecodedPngImage, geometry: UsedReplacedBox) -> Self {
        Self {
            decoded,
            svg: None,
            source: UsedReplacementImageSource::Raster,
            content_size: geometry.content_size,
            border_box_size: geometry.border_box_size,
        }
    }

    fn with_svg(mut self, svg: Option<SharedSvgAsset>) -> Self {
        if svg.is_some() {
            self.source = UsedReplacementImageSource::Svg;
        }
        self.svg = svg;
        self
    }

    fn with_generated_gradient(mut self, image: BackgroundImage) -> Self {
        self.source = UsedReplacementImageSource::Gradient {
            image: Box::new(image),
        };
        self
    }

    pub(super) fn into_inline_atom_content(self) -> InlineAtomContent {
        match self.source {
            UsedReplacementImageSource::Raster => InlineAtomContent::Image(self.decoded),
            UsedReplacementImageSource::Svg => InlineAtomContent::Svg { asset: self.svg },
            UsedReplacementImageSource::Gradient { image } => InlineAtomContent::Gradient {
                image: *image,
                fallback: self.decoded,
            },
        }
    }
}

/// Intrinsic dimensions and HTML presentational hints for an image element.
///
/// CSS Images treats raster images as replaced elements with intrinsic
/// dimensions and ratio. HTML `width`/`height` attributes participate as
/// presentational hints when present:
/// <https://www.w3.org/TR/css-images-3/#sizing> and
/// <https://html.spec.whatwg.org/multipage/rendering.html#attributes-for-embedded-content-and-images>.
pub(super) struct IntrinsicImageSize {
    pub(super) decoded: DecodedPngImage,
    pub(super) svg: Option<SharedSvgAsset>,
    pub(super) width: ContentBoxLength,
    pub(super) height: ContentBoxLength,
}

impl IntrinsicImageSize {
    pub(super) fn replaced_size(&self) -> IntrinsicReplacedSize {
        IntrinsicReplacedSize {
            width: self.width,
            height: self.height,
            // An SVG's concrete 300 by 150 viewport is the CSS default
            // object size, not a preferred aspect ratio.  Keep the SVG
            // document's ratio separate so an omitted `viewBox` does not
            // accidentally turn the fallback size into a 2:1 constraint.
            // <https://www.w3.org/TR/css-images-3/#default-sizing>
            preferred_aspect_ratio: self.svg.as_ref().map_or_else(
                || {
                    (self.width > content_box_pt(0.0) && self.height > content_box_pt(0.0))
                        .then_some(self.width.points() / self.height.points())
                },
                |asset| asset.intrinsic_dimensions().aspect_ratio,
            ),
            has_intrinsic_size: self.svg.as_ref().is_none_or(|asset| {
                let dimensions = asset.intrinsic_dimensions();
                dimensions.width.is_some() || dimensions.height.is_some()
            }),
            // HTML image dimensions are CSS presentational hints, emitted by
            // the cascade. Keeping a second layout-time copy would make an
            // author `width: auto` revive the attribute after it had won the
            // cascade.
            // <https://html.spec.whatwg.org/multipage/rendering.html#attributes-for-embedded-content-and-images>
            attr_width: None,
            attr_height: None,
        }
    }
}

/// Return the default intrinsic dimensions supplied by HTML replaced elements.
///
/// `canvas`, `video`, and similar embedded objects use `width` and `height`
/// attributes as CSS-pixel dimensions; absent attributes fall back to the
/// HTML/CSS default object size. A media resource can replace these dimensions
/// when it supplies its own intrinsic metadata.
/// <https://html.spec.whatwg.org/multipage/canvas.html#attr-canvas-width>
/// <https://www.w3.org/TR/CSS22/visudet.html#inline-replaced-width>
pub(super) fn intrinsic_default_replaced_size(element: &Element) -> IntrinsicReplacedSize {
    let attr_width = element
        .attrs
        .get("width")
        .and_then(|value| parse_html_length(value))
        .filter(|value| *value > 0.0);
    let attr_height = element
        .attrs
        .get("height")
        .and_then(|value| parse_html_length(value))
        .filter(|value| *value > 0.0);
    IntrinsicReplacedSize {
        width: content_box_pt(attr_width.unwrap_or(300.0 * css::CSS_PX_TO_PT)),
        height: content_box_pt(attr_height.unwrap_or(150.0 * css::CSS_PX_TO_PT)),
        preferred_aspect_ratio: Some(
            attr_width.unwrap_or(300.0 * css::CSS_PX_TO_PT)
                / attr_height.unwrap_or(150.0 * css::CSS_PX_TO_PT),
        ),
        has_intrinsic_size: true,
        attr_width: attr_width.map(content_box_pt),
        attr_height: attr_height.map(content_box_pt),
    }
}

/// CSS default-object geometry for an unavailable image-like resource whose
/// HTML dimension attributes have already entered the CSS cascade.
///
/// Do not read `width`/`height` here: the cascade must remain the only source
/// of these presentational hints so an author declaration of `auto` can
/// override an attribute instead of silently reintroducing it during layout.
/// <https://html.spec.whatwg.org/multipage/rendering.html#attributes-for-embedded-content-and-images>
/// <https://drafts.csswg.org/css-images-3/#default-sizing>
pub(super) fn intrinsic_unavailable_image_size() -> IntrinsicReplacedSize {
    IntrinsicReplacedSize {
        width: content_box_pt(300.0 * css::CSS_PX_TO_PT),
        height: content_box_pt(150.0 * css::CSS_PX_TO_PT),
        preferred_aspect_ratio: None,
        has_intrinsic_size: false,
        attr_width: None,
        attr_height: None,
    }
}

/// Return default iframe geometry without inventing a preferred ratio.
///
/// HTML's 300 by 150 fallback dimensions for an embedded browsing context
/// are independent intrinsic sizes, unlike an image's natural dimensions.
/// Flexbox therefore must not transfer one through the other while resolving
/// an automatic flex basis or stretched cross size.
/// <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#attr-iframe-width>
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
pub(super) fn intrinsic_iframe_size(element: &Element) -> IntrinsicReplacedSize {
    let _ = element;
    intrinsic_unavailable_image_size()
}

/// Return the intrinsic dimensions supplied by an HTML `<canvas>` element.
pub(super) fn intrinsic_canvas_size(element: &Element) -> IntrinsicReplacedSize {
    intrinsic_default_replaced_size(element)
}

/// Return the intrinsic dimensions exposed by Quire's inline SVG support.
pub(super) fn intrinsic_svg_size(element: &Element) -> Option<IntrinsicReplacedSize> {
    let (size, dimensions) = crate::svg::svg_replaced_size(element)?;
    let width = size.width;
    let height = size.height;
    Some(IntrinsicReplacedSize {
        width: content_box_pt(width),
        height: content_box_pt(height),
        preferred_aspect_ratio: (width > 0.0 && height > 0.0).then_some(width / height),
        has_intrinsic_size: dimensions.width.is_some() || dimensions.height.is_some(),
        // Root SVG width/height attributes are presentation attributes for
        // the replaced box. Keep their authored absolute values separate
        // from the SVG document's natural dimensions so size containment can
        // suppress the latter without discarding an explicit axis.
        // <https://www.w3.org/TR/SVG2/embedded.html#placement>
        attr_width: element.attrs.get("width").and_then(|value| {
            parse_html_length(value)
                .filter(|width| *width > 0.0 && !value.contains('%'))
                .map(content_box_pt)
        }),
        attr_height: element.attrs.get("height").and_then(|value| {
            parse_html_length(value)
                .filter(|height| *height > 0.0 && !value.contains('%'))
                .map(content_box_pt)
        }),
    })
}

pub(super) fn intrinsic_image_size(
    element: &Element,
    style: &ComputedStyle,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Option<IntrinsicImageSize> {
    let (src, intrinsic_resolution) = match element.tag.as_str() {
        "object" => element
            .attrs
            .get("data")
            .map(|src| (src.as_str(), crate::dom::ImageDensity::one()))?,
        "video" => element
            .attrs
            .get("poster")
            .map(|src| (src.as_str(), crate::dom::ImageDensity::one()))?,
        "img" => crate::dom::selected_img_source(element)?,
        _ => element
            .attrs
            .get("src")
            .map(|src| (src.as_str(), crate::dom::ImageDensity::one()))?,
    };
    let request_modifiers = html_image_request_modifiers(element);
    let asset = load_resolved_image_source_with_request(
        src,
        base_url,
        root_url,
        resource_cache,
        raster_orientation_policy(style.image_orientation),
        SvgImageContext::from_used_color_scheme(style.used_color_scheme),
        &request_modifiers,
    )?;
    let intrinsic_size = match &asset {
        ResolvedImageAsset::Raster(image) => image.natural_layout_size(),
        ResolvedImageAsset::Svg(asset) => asset.replaced_intrinsic_size(),
    };
    let view_box = resolved_object_view_box_for_asset(style.object_view_box.clone(), &asset);
    let intrinsic_size = view_box.effective_natural_size(intrinsic_size);
    let (width, height) = match intrinsic_resolution {
        crate::dom::ImageDensity::Finite(density) => (
            intrinsic_size.width / density.value(),
            intrinsic_size.height / density.value(),
        ),
        crate::dom::ImageDensity::Infinite => (0.0, 0.0),
    };
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let (decoded, svg) = match asset {
        ResolvedImageAsset::Raster(decoded) => (decoded, None),
        ResolvedImageAsset::Svg(svg) => (
            DecodedPngImage::new(1, 1, vec![0, 0, 0], Some(vec![0])),
            Some(svg),
        ),
    };
    Some(IntrinsicImageSize {
        decoded,
        svg,
        width: content_box_pt(width),
        height: content_box_pt(height),
    })
}

/// Return whether the preloaded static renderer can decode an HTML image.
///
/// This shares image candidate selection and request modifiers with
/// [`intrinsic_image_size`], so DOM rendering-state selection cannot disagree
/// with the later replaced-image layout path.
/// <https://html.spec.whatwg.org/multipage/rendering.html>
pub(crate) fn static_html_img_is_available(
    element: &Element,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> bool {
    debug_assert_eq!(element.tag, "img");
    let Some((src, _density)) = crate::dom::selected_img_source(element) else {
        return false;
    };
    load_resolved_image_source_with_request(
        src,
        base_url,
        root_url,
        resource_cache,
        RasterOrientationPolicy::Encoded,
        SvgImageContext::default(),
        &html_image_request_modifiers(element),
    )
    .is_some()
}

/// Translate HTML's CORS settings attribute into the common image-request
/// representation used by CSS URL sources. Missing means no-CORS; an empty or
/// invalid `crossorigin` value selects anonymous CORS.
/// <https://html.spec.whatwg.org/multipage/urls-and-fetching.html#cors-settings-attributes>
pub(super) fn html_image_request_modifiers(element: &Element) -> css::RequestUrlModifiers {
    let cross_origin = match element.attrs.get("crossorigin") {
        None => None,
        Some(value) if value.eq_ignore_ascii_case("use-credentials") => {
            Some(css::CrossOriginRequestMode::UseCredentials)
        }
        Some(_) => Some(css::CrossOriginRequestMode::Anonymous),
    };
    css::RequestUrlModifiers {
        cross_origin,
        integrity: None,
        referrer_policy: None,
    }
}

pub(super) fn used_image(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
    height_basis: BlockSizePercentageBasis,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Option<UsedImage> {
    let intrinsic = intrinsic_image_size(element, style, base_url, root_url, resource_cache);
    let (decoded, svg, intrinsic) = match intrinsic {
        Some(intrinsic) => {
            let replaced_size = intrinsic.replaced_size();
            (intrinsic.decoded, intrinsic.svg, replaced_size)
        }
        // A video without a poster still establishes a replaced box.  Quire
        // has no media-frame decoder, so represent its unavailable frame with
        // one transparent pixel while retaining its CSS object geometry.
        // <https://html.spec.whatwg.org/multipage/media.html#the-video-element>
        None if element.image_rendering == crate::dom::ImageRendering::Empty => (
            transparent_replaced_fallback_pixel(),
            None,
            intrinsic_empty_image_size(),
        ),
        None if unavailable_image_establishes_replaced_box(element, style) => (
            transparent_replaced_fallback_pixel(),
            None,
            intrinsic_unavailable_image_size(),
        ),
        None => return None,
    };
    let geometry = used_replaced_box(
        intrinsic,
        style,
        content_box_pt(available_width),
        height_basis,
    );
    Some(UsedImage::from_geometry(decoded, geometry).with_svg(svg))
}

/// The zero natural dimensions of an unavailable image without alternative
/// text. Explicit CSS dimensions still apply through ordinary replaced sizing.
/// <https://html.spec.whatwg.org/multipage/rendering.html>
fn intrinsic_empty_image_size() -> IntrinsicReplacedSize {
    IntrinsicReplacedSize {
        width: content_box_pt(0.0),
        height: content_box_pt(0.0),
        preferred_aspect_ratio: None,
        has_intrinsic_size: false,
        attr_width: None,
        attr_height: None,
    }
}

/// The Flexbox operation that established an intrinsic descendant block
/// basis.  This is deliberately narrower than a generic available-size
/// source: each variant represents a Flexbox operation that CSS allows to
/// make percentages definite.
/// <https://drafts.csswg.org/css-flexbox/#definite-sizes>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FlexIntrinsicBlockBasisSource {
    ExistingFlexItem,
    DefiniteFlexBase,
    PostFlexingMainSize,
    DefinitePreferredSize,
    BalancedLineSlot,
    DefiniteSingleLineStretch,
    AspectRatioTransfer,
}

/// The explicit bound that won while constraining an otherwise automatic
/// intrinsic block contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WinningBlockConstraintKind {
    Minimum,
    Maximum,
}

/// A block-size result that may participate in intrinsic replaced sizing.
///
/// CSS Sizing keeps content-derived intrinsic measurements indefinite. The
/// definite variants are therefore produced only by the formatting context
/// that resolved the item's own block size.
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
/// <https://drafts.csswg.org/css-flexbox-1/#definite-sizes>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum IntrinsicBlockBasis {
    Indefinite,
    DefiniteFromContainingBlock(ContentBoxLength),
    DefiniteFromFlexLayout {
        value: ContentBoxLength,
        source: FlexIntrinsicBlockBasisSource,
    },
    DefiniteWinningConstraint {
        value: ContentBoxLength,
        kind: WinningBlockConstraintKind,
    },
}

impl IntrinsicBlockBasis {
    /// Adapt the established percentage basis of a general formatting context.
    /// Flex intrinsic sizing constructs the enum variants directly, which
    /// prevents an arbitrary available intrinsic size from becoming definite.
    pub(super) fn from_layout_percentage_basis(basis: BlockSizePercentageBasis) -> Self {
        match basis {
            PercentageBasis::Indefinite => Self::Indefinite,
            PercentageBasis::Definite {
                value,
                source: BlockSizeBasisSource::FlexItem,
            } => Self::from_flex_layout(value, FlexIntrinsicBlockBasisSource::ExistingFlexItem),
            PercentageBasis::Definite { value, .. } => Self::DefiniteFromContainingBlock(value),
        }
    }

    pub(super) fn from_flex_layout(
        value: ContentBoxLength,
        source: FlexIntrinsicBlockBasisSource,
    ) -> Self {
        Self::DefiniteFromFlexLayout { value, source }
    }

    pub(super) fn from_winning_constraint(
        value: ContentBoxLength,
        kind: WinningBlockConstraintKind,
    ) -> Self {
        Self::DefiniteWinningConstraint { value, kind }
    }

    pub(super) fn descendant_percentage_basis(self) -> BlockSizePercentageBasis {
        let (value, source) = match self {
            Self::Indefinite => return PercentageBasis::indefinite(),
            Self::DefiniteFromContainingBlock(value) => {
                (value, BlockSizeBasisSource::ContainingBlock)
            }
            Self::DefiniteFromFlexLayout { value, .. }
            | Self::DefiniteWinningConstraint { value, .. } => {
                (value, BlockSizeBasisSource::FlexItem)
            }
        };
        PercentageBasis::definite_from(value, source)
    }
}

/// Geometry inputs for one CSS replaced-element sizing operation.
///
/// The physical available width constrains paint geometry independently from
/// the logical percentage basis, which may be indefinite when that width is
/// cyclic. Image, canvas, and SVG share this context so their percentage
/// height behavior cannot diverge.
#[derive(Debug, Clone, Copy)]
pub(super) struct ReplacedBoxSizingContext {
    pub(super) available_width: ContentBoxLength,
    pub(super) inline_percentage_basis: IntrinsicInlinePercentageBasis,
    pub(super) block_basis: IntrinsicBlockBasis,
}

/// A fully sized replaced element, retaining its format-specific paint
/// resource while exposing one shared CSS box geometry to every layout
/// consumer.
///
/// CSS replaced-element sizing is independent from the later painting format.
/// Keeping the dispatch here prevents Flexbox, tables, and positioned layout
/// from accidentally treating an SVG or canvas as an empty ordinary block.
/// <https://www.w3.org/TR/css-images-3/#sizing>
pub(super) enum ResolvedReplacedElement {
    Canvas(UsedReplacedBox),
    Image(UsedImage),
    Svg(UsedReplacedBox),
}

impl ResolvedReplacedElement {
    pub(super) fn geometry(&self) -> UsedReplacedBox {
        match self {
            Self::Canvas(geometry) | Self::Svg(geometry) => *geometry,
            Self::Image(image) => UsedReplacedBox {
                content_size: image.content_size,
                border_box_size: image.border_box_size,
            },
        }
    }

    pub(super) fn into_image(self) -> Option<UsedImage> {
        match self {
            Self::Image(image) => Some(image),
            Self::Canvas(_) | Self::Svg(_) => None,
        }
    }
}

/// Resolve any replaced element through the one typed CSS sizing boundary.
///
/// The caller supplies percentage-basis provenance; the resolver owns the
/// exhaustive format dispatch and returns the resulting content- and
/// border-box geometry. Paint callers may retain the returned image resource,
/// while sizing callers use [`ResolvedReplacedElement::geometry`].
pub(super) fn resolve_replaced_element(
    element: &Element,
    style: &ComputedStyle,
    sizing: ReplacedBoxSizingContext,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Option<ResolvedReplacedElement> {
    match replaced_element_kind(element)? {
        ReplacedElementKind::Canvas => Some(ResolvedReplacedElement::Canvas(
            used_canvas_with_intrinsic_sizing_context(element, style, sizing),
        )),
        ReplacedElementKind::Image => used_image_with_intrinsic_sizing_context(
            element,
            style,
            sizing,
            base_url,
            root_url,
            resource_cache,
        )
        .map(ResolvedReplacedElement::Image),
        ReplacedElementKind::Svg => used_svg_with_intrinsic_sizing_context(element, style, sizing)
            .map(ResolvedReplacedElement::Svg),
    }
}

/// Resolve an image while preserving whether the inline percentage basis is
/// definite. Intrinsic inline collection uses this alongside canvas so every
/// raster/SVG replaced image shares the same cyclic-percentage behavior.
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
pub(super) fn used_image_with_intrinsic_sizing_context(
    element: &Element,
    style: &ComputedStyle,
    sizing: ReplacedBoxSizingContext,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Option<UsedImage> {
    let intrinsic = intrinsic_image_size(element, style, base_url, root_url, resource_cache);
    let (decoded, svg, intrinsic) = match intrinsic {
        Some(intrinsic) => {
            let replaced_size = intrinsic.replaced_size();
            (intrinsic.decoded, intrinsic.svg, replaced_size)
        }
        // Intrinsic probes run while building table column measures, before
        // the document-wide static image-state pass necessarily records an
        // empty image. A missing selected source is already sufficient to
        // establish the HTML zero-natural-dimension state.
        // <https://html.spec.whatwg.org/multipage/images.html#the-img-element>
        None if element.tag == "img" && crate::dom::selected_img_source(element).is_none() => (
            transparent_replaced_fallback_pixel(),
            None,
            intrinsic_empty_image_size(),
        ),
        None if unavailable_image_establishes_replaced_box(element, style) => (
            transparent_replaced_fallback_pixel(),
            None,
            unavailable_image_intrinsic_size_for_inline_basis(
                element,
                style,
                sizing.inline_percentage_basis,
            ),
        ),
        None => return None,
    };
    let geometry = used_replaced_box_with_inline_percentage_basis(
        intrinsic,
        style,
        sizing.available_width,
        sizing.inline_percentage_basis,
        sizing.block_basis.descendant_percentage_basis(),
    );
    Some(UsedImage::from_geometry(decoded, geometry).with_svg(svg))
}

/// Return fallback image geometry for an unavailable resource during intrinsic
/// inline measurement.
///
/// A cyclic percentage width has no intrinsic table-column contribution: using
/// HTML's 300×150 default object size here would turn `width: 100%` into a
/// spurious table minimum. Once the table commits a cell width, normal image
/// layout calls [`used_image`] with a definite basis and restores the ordinary
/// broken-image fallback geometry.
/// <https://drafts.csswg.org/css-tables-3/#computing-cell-measures>
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
fn unavailable_image_intrinsic_size_for_inline_basis(
    element: &Element,
    style: &ComputedStyle,
    inline_percentage_basis: IntrinsicInlinePercentageBasis,
) -> IntrinsicReplacedSize {
    let cyclic_inline_percentage = !inline_percentage_basis.is_definite()
        && element.tag == "img"
        && element
            .attrs
            .get("src")
            .is_none_or(|source| source.trim().is_empty())
        && matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(ref value)
                if value.needs_percentage_basis()
        );
    if !cyclic_inline_percentage {
        return intrinsic_unavailable_image_size();
    }
    IntrinsicReplacedSize {
        width: content_box_pt(0.0),
        height: content_box_pt(0.0),
        preferred_aspect_ratio: None,
        has_intrinsic_size: false,
        attr_width: None,
        attr_height: None,
    }
}

/// Return an invisible stand-in for a replaced element whose visual resource
/// is unavailable to the static renderer.
///
/// The resource only permits the normal image paint path to retain a replaced
/// box's CSS geometry.  Its alpha channel keeps the unsupported media frame
/// invisible rather than inventing a poster image.
fn transparent_replaced_fallback_pixel() -> DecodedPngImage {
    DecodedPngImage::new(1, 1, vec![0, 0, 0], Some(vec![0]))
}

/// Return whether an unavailable HTML image-like element still establishes a
/// replaced box.
///
/// A media element has an HTML default object size without decoded media, and
/// an `<img>` with a source, HTML dimensions, or a definite CSS size retains a
/// broken-image box. A bare auto-sized `<img>` has no image request or
/// intrinsic geometry; treating it as the 300×150 default object would
/// incorrectly contribute to intrinsic sizing.
/// <https://html.spec.whatwg.org/multipage/images.html#the-img-element>
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
fn unavailable_image_establishes_replaced_box(element: &Element, style: &ComputedStyle) -> bool {
    if element.tag == "video" {
        return true;
    }
    element.tag == "img"
        && (element
            .attrs
            .get("src")
            .is_some_and(|source| !source.trim().is_empty())
            || element.attrs.contains_key("width")
            || element.attrs.contains_key("height")
            || !style.box_values.width.is_auto()
            || !style.box_values.height.is_auto())
}

/// Resolve a replaced element's intrinsic dimensions into CSS content and
/// border boxes.
///
/// The resource adapter supplies only intrinsic dimensions and HTML dimension
/// attributes. This common sizing step owns CSS `width`/`height`, preferred
/// aspect-ratio transfer, `box-sizing`, and min/max constraints, so formatting
/// contexts never need to reconstruct one box from another.
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
/// <https://www.w3.org/TR/css-sizing-3/#aspect-ratio>
pub(super) fn used_replaced_box(
    intrinsic: IntrinsicReplacedSize,
    style: &ComputedStyle,
    available_width: ContentBoxLength,
    height_basis: BlockSizePercentageBasis,
) -> UsedReplacedBox {
    used_replaced_box_with_inline_percentage_basis(
        intrinsic,
        style,
        available_width,
        PercentageBasis::definite_from(
            ContentBoxLength::new(available_width.points().max(0.0)),
            IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
        ),
        height_basis,
    )
}

/// Resolve a replaced box while preserving the definiteness of its inline
/// percentage basis.
///
/// Intrinsic inline sizing can carry a geometric line-width constraint without
/// making that width a percentage basis. Keeping the two inputs separate lets
/// cyclic percentage widths behave as `auto` while a definite block size still
/// transfers through the preferred aspect ratio.
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
pub(super) fn used_replaced_box_with_inline_percentage_basis(
    intrinsic: IntrinsicReplacedSize,
    style: &ComputedStyle,
    available_width: ContentBoxLength,
    inline_percentage_basis: IntrinsicInlinePercentageBasis,
    height_basis: BlockSizePercentageBasis,
) -> UsedReplacedBox {
    let intrinsic = intrinsic.scaled_by_effective_zoom(style.effective_zoom.factor());
    // Size containment replaces decoded-image natural dimensions with the
    // author-provided contain-intrinsic fallback. HTML width/height dimension
    // attributes additionally establish a presentational aspect ratio; that
    // authored ratio remains available even when the replaced content itself
    // is size-contained.
    // <https://www.w3.org/TR/css-contain-1/#containment-size>
    // <https://html.spec.whatwg.org/multipage/embedded-content-other.html#dimension-attributes>
    let attribute_aspect_ratio = intrinsic.attribute_aspect_ratio();
    let contained_intrinsic_width = style
        .contain
        .size
        .then(|| {
            style.contain_intrinsic_size.width.clone().map(|width| {
                used_length_percentage(
                    width,
                    PercentageBasis::definite(available_width.into_layout_length()),
                )
                .points()
            })
        })
        .flatten()
        .unwrap_or(0.0);
    let contained_intrinsic_height = style
        .contain
        .size
        .then(|| {
            style.contain_intrinsic_size.height.clone().map(|height| {
                used_length_percentage(
                    height,
                    PercentageBasis::definite(available_width.into_layout_length()),
                )
                .points()
            })
        })
        .flatten()
        .unwrap_or(0.0);
    let natural_aspect_ratio = if style.contain.size {
        // Contain-intrinsic dimensions are independent fallback sizes, not a
        // natural aspect ratio. An HTML width/height attribute remains an
        // authored presentational ratio, however.
        // <https://drafts.csswg.org/css-sizing-4/#intrinsic-size-override>
        attribute_aspect_ratio
    } else {
        intrinsic.natural_aspect_ratio()
    };
    let aspect_ratio = style
        .aspect_ratio
        .preferred_ratio(true, natural_aspect_ratio);
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    let vertical_non_content =
        borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    let available_content_width = (available_width.points() - horizontal_non_content).max(0.0);
    let css_width = used_content_box_width_or_auto_with_basis(
        style,
        inline_percentage_basis,
        non_content_pt(horizontal_non_content),
    )
    .map(SemanticLengthExt::points);
    // CSS percentage heights on replaced elements resolve against a definite
    // containing-block height. Flex and grid establish such bases in cases
    // where their final item size is definite, so this cannot be reduced to a
    // non-percentage-only shortcut.
    // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
    // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
    let css_height = used_content_box_height_or_auto_with_basis(
        style,
        height_basis,
        non_content_pt(vertical_non_content),
    )
    .map(SemanticLengthExt::points);
    let (width, height) = if css_width.is_some() || css_height.is_some() {
        // A resolved CSS dimension wins over lower-priority HTML
        // presentational hints; the remaining auto axis uses the preferred
        // aspect ratio rather than reviving the overridden attribute length.
        (css_width, css_height)
    } else {
        (
            intrinsic.attr_width.map(SemanticLengthExt::points),
            intrinsic.attr_height.map(SemanticLengthExt::points),
        )
    };
    // HTML dimension attributes contribute intrinsic dimensions, not definite
    // CSS preferred sizes. Constraints may therefore transfer through their
    // intrinsic aspect ratio when the corresponding CSS axis is automatic.
    // <https://html.spec.whatwg.org/multipage/rendering.html#attributes-for-embedded-content-and-images>
    // <https://www.w3.org/TR/css-sizing-3/#aspect-ratio>
    let width_is_auto = css_width.is_none();
    let height_is_auto = css_height.is_none();
    let ratio_only_automatic_size =
        width_is_auto && height_is_auto && !intrinsic.has_intrinsic_size && aspect_ratio.is_some();
    let (mut content_width, mut content_height) = match (width, height, aspect_ratio) {
        (Some(width_value), None, Some(ratio)) => (width_value, width_value / ratio),
        (None, Some(height_value), Some(ratio)) => (height_value * ratio, height_value),
        // A `contain-intrinsic-size` fallback supplies intrinsic dimensions,
        // but does not synthesize a natural aspect ratio.  Consequently an
        // explicitly-sized axis does not scale the fallback on the auto axis:
        // that axis retains its own fallback intrinsic dimension.
        // <https://drafts.csswg.org/css-sizing-4/#intrinsic-size-override>
        (Some(width_value), None, None) if style.contain.size => {
            (width_value, contained_intrinsic_height)
        }
        (None, Some(height_value), None) if style.contain.size => {
            (contained_intrinsic_width, height_value)
        }
        // With one CSS axis specified but no preferred ratio, the automatic
        // axis retains its independent intrinsic dimension. This is distinct
        // from transferring a size through an aspect ratio: an SVG can supply
        // an intrinsic width or height without supplying a ratio at all.
        // <https://www.w3.org/TR/CSS22/visudet.html#inline-replaced-width>
        // <https://www.w3.org/TR/CSS22/visudet.html#inline-replaced-height>
        (Some(width_value), None, None) => (width_value, intrinsic.height.points()),
        (None, Some(height_value), None) => (intrinsic.width.points(), height_value),
        (None, None, _) if style.contain.size => {
            (contained_intrinsic_width, contained_intrinsic_height)
        }
        (None, None, _) => (intrinsic.width.points(), intrinsic.height.points()),
        (Some(width_value), Some(height_value), _) => (width_value, height_value),
    };
    // CSS 2.2 suggests resolving a ratio-only automatic replaced element from
    // the normal-flow width equation when its containing block does not depend
    // on that width.  Establish that tentative size before min/max processing:
    // the CSS 2.2 replaced-element constraint table operates on this `w`/`h`.
    // <https://www.w3.org/TR/CSS22/visudet.html#inline-replaced-width>
    // <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>
    if ratio_only_automatic_size {
        content_width = content_width.min(available_content_width);
        content_height = content_width / aspect_ratio.expect("ratio-only size has a ratio");
    }
    if let Some(aspect_ratio) = aspect_ratio {
        let constrained_size = resolve_replaced_size_with_aspect_ratio(
            content_box_size_pt(content_width, content_height),
            aspect_ratio,
            ReplacedPreferredSizeAxes {
                width: ReplacedPreferredSize::from_is_automatic(width_is_auto),
                height: ReplacedPreferredSize::from_is_automatic(height_is_auto),
            },
            ReplacedSizeConstraints {
                min_width: used_min_width(
                    style,
                    PercentageBasis::definite(available_width.into_layout_length()),
                )
                .map(|width| width.max(content_box_pt(0.0))),
                max_width: used_max_width(
                    style,
                    PercentageBasis::definite(available_width.into_layout_length()),
                )
                .map(|width| width.max(content_box_pt(0.0))),
                // CSS block-axis constraints resolve percentage values from
                // the containing block's block-size basis. An indefinite
                // basis leaves a percentage min/max-height unresolved rather
                // than accidentally resolving it against available inline
                // width.
                // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
                min_height: used_length_percentage_or_auto_with_basis(
                    style.box_values.min_height.clone(),
                    height_basis,
                )
                .map(|height| content_box_pt(height.points().max(0.0))),
                max_height: used_length_percentage_or_auto_with_basis(
                    style.box_values.max_height.clone(),
                    height_basis,
                )
                .map(|height| content_box_pt(height.points().max(0.0))),
            },
        );
        content_width = constrained_size.width;
        content_height = constrained_size.height;
    } else {
        content_width = constrain_content_width(
            style,
            content_box_pt(content_width),
            PercentageBasis::definite(available_width.into_layout_length()),
        )
        .points();
        content_height = constrain_content_height(
            style,
            content_box_pt(content_height),
            PercentageBasis::definite(available_width.into_layout_length()),
        )
        .points();
    }
    UsedReplacedBox::new(
        content_box_size_pt(content_width, content_height),
        non_content_pt(horizontal_non_content),
        non_content_pt(vertical_non_content),
    )
}

/// Size a concrete external asset for a CSS generated-content replacement.
/// This intentionally receives the already-selected asset so CSS Images 5
/// `image()` values share the same sizing path as a direct `url()`.
fn used_generated_image_asset(
    asset: ResolvedImageAsset,
    style: &ComputedStyle,
    available_width: f32,
    intrinsic_resolution: f32,
) -> Option<UsedImage> {
    // CSS Images applies an image-set resolution descriptor to the natural
    // resolution of raster candidates only. Vector SVG candidates retain
    // their own intrinsic CSS dimensions.
    // <https://drafts.csswg.org/css-images-4/#image-set-notation>
    let intrinsic_resolution = match &asset {
        ResolvedImageAsset::Raster(_) => intrinsic_resolution,
        ResolvedImageAsset::Svg(_) => 1.0,
    }
    .max(f32::MIN_POSITIVE);
    let raw_intrinsic_size = match &asset {
        ResolvedImageAsset::Raster(image) => image.natural_layout_size(),
        // Generated `content: url(...)` SVGs are still CSS replaced images.
        // Their layout fallback must therefore use SVG's replaced intrinsic
        // size, not the parser's concrete root viewport fallback.
        ResolvedImageAsset::Svg(asset) => asset.replaced_intrinsic_size(),
    };
    let view_box = resolved_object_view_box_for_asset(style.object_view_box.clone(), &asset);
    let raw_intrinsic_size = view_box.effective_natural_size(raw_intrinsic_size);
    let intrinsic_width = raw_intrinsic_size.width / intrinsic_resolution;
    let intrinsic_height = raw_intrinsic_size.height / intrinsic_resolution;
    if intrinsic_width <= 0.0 || intrinsic_height <= 0.0 {
        return None;
    }
    let geometry = used_replaced_box(
        IntrinsicReplacedSize {
            width: content_box_pt(intrinsic_width),
            height: content_box_pt(intrinsic_height),
            preferred_aspect_ratio: Some(intrinsic_width / intrinsic_height),
            has_intrinsic_size: true,
            attr_width: None,
            attr_height: None,
        },
        style,
        content_box_pt(available_width),
        PercentageBasis::indefinite(),
    );
    let (decoded, svg) = match asset {
        ResolvedImageAsset::Raster(decoded) => (decoded, None),
        ResolvedImageAsset::Svg(svg) => (
            DecodedPngImage::new(1, 1, vec![0, 0, 0], Some(vec![0])),
            Some(svg),
        ),
    };
    Some(UsedImage::from_geometry(decoded, geometry).with_svg(svg))
}

/// Resolve the effective natural size established by CSS Images Level 5's
/// `object-view-box`. The source crop is resolved before CSS replaced sizing;
/// paint later maps the same source rectangle into that effective object.
/// <https://drafts.csswg.org/css-images-5/#the-object-view-box-property>
/// Resolve a view box for an image asset before its CSS replaced sizing.
///
/// SVG's 300 by 150 parser viewport fallback is not a natural image size, so
/// an SVG only participates when it supplies both natural axes. Raster images
/// always expose both axes through their decoded dimensions.
pub(in crate::layout) fn resolved_object_view_box_for_asset(
    view_box: css::ObjectViewBox,
    asset: &ResolvedImageAsset,
) -> ResolvedObjectViewBox {
    let natural_size = match asset {
        ResolvedImageAsset::Raster(image) => Some(image.natural_layout_size()),
        ResolvedImageAsset::Svg(asset) => asset
            .intrinsic_dimensions()
            .width
            .zip(asset.intrinsic_dimensions().height)
            .map(|(width, height)| LayoutSize::new(width.points(), height.points())),
    };
    resolved_object_view_box(view_box, natural_size)
}

/// Resolve an SVG view box without promoting its parser viewport fallback to
/// CSS natural dimensions.
pub(in crate::layout) fn resolved_object_view_box_for_svg(
    view_box: css::ObjectViewBox,
    asset: &SharedSvgAsset,
) -> ResolvedObjectViewBox {
    let dimensions = asset.intrinsic_dimensions();
    let natural_size = dimensions
        .width
        .zip(dimensions.height)
        .map(|(width, height)| LayoutSize::new(width.points(), height.points()));
    resolved_object_view_box(view_box, natural_size)
}

/// Resolve CSS `object-view-box` into an effective normalized source rectangle.
///
/// A missing natural axis or a non-positive used rectangle makes the property
/// ineffective. This follows the WPT-defined behavior for empty and negative
/// basic shapes while retaining valid out-of-bounds source origins.
/// <https://drafts.csswg.org/css-images-5/#the-object-view-box-property>
pub(in crate::layout) fn resolved_object_view_box(
    view_box: css::ObjectViewBox,
    natural_size: Option<LayoutSize>,
) -> ResolvedObjectViewBox {
    let Some(natural_size) = natural_size.filter(|size| {
        size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0
    }) else {
        return ResolvedObjectViewBox::NoEffect;
    };
    let resolve_x = |value| {
        used_length_percentage(
            value,
            PercentageBasis::definite(layout_pt(natural_size.width)),
        )
        .points()
    };
    let resolve_y = |value| {
        used_length_percentage(
            value,
            PercentageBasis::definite(layout_pt(natural_size.height)),
        )
        .points()
    };
    let (rect, radii) = match view_box {
        css::ObjectViewBox::None => return ResolvedObjectViewBox::NoEffect,
        css::ObjectViewBox::Inset {
            top,
            right,
            bottom,
            left,
            radii,
        } => {
            let left = resolve_x(left) / natural_size.width;
            let right = resolve_x(right) / natural_size.width;
            let top = resolve_y(top) / natural_size.height;
            let bottom = resolve_y(bottom) / natural_size.height;
            (
                NormalizedObjectSourceRect::new(
                    NormalizedObjectSourcePoint::new(left, top),
                    NormalizedObjectSourceSize::new(1.0 - left - right, 1.0 - top - bottom),
                ),
                radii,
            )
        }
        css::ObjectViewBox::Xywh {
            x,
            y,
            width,
            height,
            radii,
        } => (
            NormalizedObjectSourceRect::new(
                NormalizedObjectSourcePoint::new(
                    resolve_x(x) / natural_size.width,
                    resolve_y(y) / natural_size.height,
                ),
                NormalizedObjectSourceSize::new(
                    resolve_x(width) / natural_size.width,
                    resolve_y(height) / natural_size.height,
                ),
            ),
            radii,
        ),
        css::ObjectViewBox::Rect {
            top,
            right,
            bottom,
            left,
        } => {
            let left = resolve_x(left) / natural_size.width;
            let right = resolve_x(right) / natural_size.width;
            let top = resolve_y(top) / natural_size.height;
            let bottom = resolve_y(bottom) / natural_size.height;
            (
                NormalizedObjectSourceRect::new(
                    NormalizedObjectSourcePoint::new(left, top),
                    NormalizedObjectSourceSize::new(right - left, bottom - top),
                ),
                None,
            )
        }
    };
    if rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && rect.size.width.is_finite()
        && rect.size.height.is_finite()
        && rect.size.width > 0.0
        && rect.size.height > 0.0
    {
        ResolvedObjectViewBox::Source { rect, radii }
    } else {
        ResolvedObjectViewBox::NoEffect
    }
}

pub(super) fn used_generated_image_value(
    image: &BackgroundImage,
    style: &ComputedStyle,
    available_width: f32,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Option<UsedImage> {
    let intrinsic_resolution = image.intrinsic_resolution();
    if matches!(
        image.selected_image(),
        BackgroundImage::Url(_) | BackgroundImage::ImageFunction(_)
    ) {
        return match resolve_css_image_source(
            image,
            ImageResolutionContext {
                base_url: fallback_base_url,
                root_url: fallback_root_url,
                current_color: style.color,
                orientation: raster_orientation_policy(style.image_orientation),
                svg_context: SvgImageContext::from_used_color_scheme(style.used_color_scheme),
                resource_cache,
            },
        ) {
            ResolvedCssImage::External(asset) => {
                used_generated_image_asset(asset, style, available_width, intrinsic_resolution)
            }
            // A color fallback is a dimensionless generated image, sharing
            // gradients' default-object sizing rather than acquiring a 1px
            // natural size from its raster paint representation.
            ResolvedCssImage::SolidColor(_) => used_dimensionless_generated_image(
                image,
                style,
                available_width,
                fallback_base_url,
                fallback_root_url,
                resource_cache,
            ),
            ResolvedCssImage::Invalid => None,
        };
    }

    used_dimensionless_generated_image(
        image,
        style,
        available_width,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
    )
}

fn used_dimensionless_generated_image(
    image: &BackgroundImage,
    style: &ComputedStyle,
    available_width: f32,
    fallback_base_url: Option<&url::Url>,
    fallback_root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Option<UsedImage> {
    // Gradients have no natural dimensions or preferred aspect ratio.  They
    // therefore negotiate CSS's 300 by 150px default object size rather than
    // inheriting an arbitrary font-sized square.
    // <https://drafts.csswg.org/css-images-3/#default-sizing>
    let geometry = used_replaced_box(
        IntrinsicReplacedSize {
            width: content_box_pt(300.0 * css::CSS_PX_TO_PT),
            height: content_box_pt(150.0 * css::CSS_PX_TO_PT),
            preferred_aspect_ratio: None,
            has_intrinsic_size: false,
            attr_width: None,
            attr_height: None,
        },
        style,
        content_box_pt(available_width),
        PercentageBasis::indefinite(),
    );
    let content_width = geometry.content_size.width;
    let content_height = geometry.content_size.height;
    let decoded = rasterize_generated_css_image(
        image,
        PaintSize::new(content_width, content_height),
        style.color,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
    )?;
    Some(UsedImage::from_geometry(decoded, geometry).with_generated_gradient(image.clone()))
}

/// Used size for an invalid CSS `content: url(...)` replacement image.
///
/// CSS Content Level 3 requires invalid replacement images to render as a
/// transparent image with no natural dimensions rather than suppressing the
/// replacement box:
/// <https://www.w3.org/TR/css-content-3/#content-property>.
pub(super) fn used_invalid_replacement_image(
    style: &ComputedStyle,
    available_width: f32,
) -> UsedImage {
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    let vertical_non_content =
        borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    let available_content_width = (available_width - horizontal_non_content).max(0.0);
    let mut content_width = used_content_box_width_or_auto(
        style,
        layout_pt(available_width),
        non_content_pt(horizontal_non_content),
    )
    .map(SemanticLengthExt::points)
    .unwrap_or(0.0);
    let content_height =
        definite_image_content_height_without_percent(style, vertical_non_content).unwrap_or(0.0);
    content_width = content_width.min(available_content_width);
    UsedImage::new(
        DecodedPngImage::new(1, 1, vec![0, 0, 0], Some(vec![0])),
        content_box_size_pt(content_width, content_height),
        non_content_pt(horizontal_non_content),
        non_content_pt(vertical_non_content),
    )
}

/// Whether a replaced element's preferred size is automatic in one axis.
///
/// CSS Sizing transfers constraints only into an automatic destination axis;
/// a definite preferred size is unaffected by a transferred constraint.
/// <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplacedPreferredSize {
    Automatic,
    Definite,
}

impl ReplacedPreferredSize {
    pub(super) const fn from_is_automatic(is_automatic: bool) -> Self {
        if is_automatic {
            Self::Automatic
        } else {
            Self::Definite
        }
    }

    const fn is_automatic(self) -> bool {
        matches!(self, Self::Automatic)
    }
}

/// Preferred-size definiteness for each physical content-box axis.
#[derive(Debug, Clone, Copy)]
pub(super) struct ReplacedPreferredSizeAxes {
    pub(super) width: ReplacedPreferredSize,
    pub(super) height: ReplacedPreferredSize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ReplacedSizeConstraints {
    pub(super) min_width: Option<ContentBoxLength>,
    pub(super) max_width: Option<ContentBoxLength>,
    pub(super) min_height: Option<ContentBoxLength>,
    pub(super) max_height: Option<ContentBoxLength>,
}

/// Resolve a replaced content box against min/max constraints and its preferred ratio.
///
/// When both preferred axes are automatic, CSS 2.2 gives an explicit
/// two-dimensional constraint table for intrinsic-ratio replaced elements.
/// Otherwise CSS Sizing transfers constraints only into an automatic axis.
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>
/// <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
pub(super) fn resolve_replaced_size_with_aspect_ratio(
    size: ContentBoxSize,
    aspect_ratio: f32,
    preferred_sizes: ReplacedPreferredSizeAxes,
    constraints: ReplacedSizeConstraints,
) -> ContentBoxSize {
    if !aspect_ratio.is_finite() || aspect_ratio <= 0.0 {
        return size;
    }
    let width_bounds = ReplacedAxisBounds::new(constraints.min_width, constraints.max_width);
    let height_bounds = ReplacedAxisBounds::new(constraints.min_height, constraints.max_height);
    if preferred_sizes.width.is_automatic() && preferred_sizes.height.is_automatic() {
        return resolve_automatic_replaced_size_constraint_table(
            size,
            aspect_ratio,
            width_bounds,
            height_bounds,
        );
    }

    let width_bounds = if preferred_sizes.width.is_automatic() {
        width_bounds.with_transferred_from_height(height_bounds, aspect_ratio)
    } else {
        width_bounds
    };
    let height_bounds = if preferred_sizes.height.is_automatic() {
        height_bounds.with_transferred_from_width(width_bounds, aspect_ratio)
    } else {
        height_bounds
    };
    content_box_size_pt(
        width_bounds.clamp(size.width),
        height_bounds.clamp(size.height),
    )
}

/// Resolved content-box bounds for one physical axis.
#[derive(Debug, Clone, Copy)]
struct ReplacedAxisBounds {
    min: ContentBoxLength,
    max: ContentBoxLength,
}

impl ReplacedAxisBounds {
    fn new(min: Option<ContentBoxLength>, max: Option<ContentBoxLength>) -> Self {
        let min = min.unwrap_or_else(|| content_box_pt(0.0));
        let max = max
            .unwrap_or_else(|| content_box_pt(f32::INFINITY))
            .max(min);
        Self { min, max }
    }

    fn clamp(self, value: f32) -> f32 {
        value.max(self.min.points()).min(self.max.points())
    }

    fn with_transferred_from_width(self, width: Self, aspect_ratio: f32) -> Self {
        self.with_transferred_bounds(
            content_box_pt(width.min.points() / aspect_ratio),
            content_box_pt(width.max.points() / aspect_ratio),
        )
    }

    fn with_transferred_from_height(self, height: Self, aspect_ratio: f32) -> Self {
        self.with_transferred_bounds(
            content_box_pt(height.min.points() * aspect_ratio),
            content_box_pt(height.max.points() * aspect_ratio),
        )
    }

    fn with_transferred_bounds(
        self,
        transferred_min: ContentBoxLength,
        transferred_max: ContentBoxLength,
    ) -> Self {
        let min = self.min.max(transferred_min.min(self.max));
        let max = self.max.min(transferred_max.max(min));
        Self { min, max }
    }
}

/// Apply CSS 2.2's replaced-element min/max constraint table.
fn resolve_automatic_replaced_size_constraint_table(
    size: ContentBoxSize,
    aspect_ratio: f32,
    width_bounds: ReplacedAxisBounds,
    height_bounds: ReplacedAxisBounds,
) -> ContentBoxSize {
    let width = size.width.max(0.0);
    let height = size.height.max(0.0);
    let width_scale = |new_width: f32| {
        if width > 0.0 {
            new_width * height / width
        } else {
            new_width / aspect_ratio
        }
    };
    let height_scale = |new_height: f32| {
        if height > 0.0 {
            new_height * width / height
        } else {
            new_height * aspect_ratio
        }
    };
    let width_constraint_scale = |constraint: f32| {
        if width > 0.0 {
            constraint / width
        } else if constraint > 0.0 {
            f32::INFINITY
        } else {
            0.0
        }
    };
    let height_constraint_scale = |constraint: f32| {
        if height > 0.0 {
            constraint / height
        } else if constraint > 0.0 {
            f32::INFINITY
        } else {
            0.0
        }
    };
    let width_violation = ReplacedConstraintViolation::classify(width, width_bounds);
    let height_violation = ReplacedConstraintViolation::classify(height, height_bounds);
    let (width, height) = match (width_violation, height_violation) {
        (ReplacedConstraintViolation::None, ReplacedConstraintViolation::None) => (width, height),
        (ReplacedConstraintViolation::Maximum, ReplacedConstraintViolation::None) => {
            let width = width_bounds.max.points();
            (width, width_scale(width).max(height_bounds.min.points()))
        }
        (ReplacedConstraintViolation::Minimum, ReplacedConstraintViolation::None) => {
            let width = width_bounds.min.points();
            (width, width_scale(width).min(height_bounds.max.points()))
        }
        (ReplacedConstraintViolation::None, ReplacedConstraintViolation::Maximum) => {
            let height = height_bounds.max.points();
            (height_scale(height).max(width_bounds.min.points()), height)
        }
        (ReplacedConstraintViolation::None, ReplacedConstraintViolation::Minimum) => {
            let height = height_bounds.min.points();
            (height_scale(height).min(width_bounds.max.points()), height)
        }
        (ReplacedConstraintViolation::Maximum, ReplacedConstraintViolation::Maximum) => {
            if width_constraint_scale(width_bounds.max.points())
                <= height_constraint_scale(height_bounds.max.points())
            {
                let width = width_bounds.max.points();
                (width, width_scale(width).max(height_bounds.min.points()))
            } else {
                let height = height_bounds.max.points();
                (height_scale(height).max(width_bounds.min.points()), height)
            }
        }
        (ReplacedConstraintViolation::Minimum, ReplacedConstraintViolation::Minimum) => {
            if width_constraint_scale(width_bounds.min.points())
                <= height_constraint_scale(height_bounds.min.points())
            {
                let height = height_bounds.min.points();
                (height_scale(height).min(width_bounds.max.points()), height)
            } else {
                let width = width_bounds.min.points();
                (width, width_scale(width).min(height_bounds.max.points()))
            }
        }
        (ReplacedConstraintViolation::Minimum, ReplacedConstraintViolation::Maximum) => {
            (width_bounds.min.points(), height_bounds.max.points())
        }
        (ReplacedConstraintViolation::Maximum, ReplacedConstraintViolation::Minimum) => {
            (width_bounds.max.points(), height_bounds.min.points())
        }
    };
    content_box_size_pt(width, height)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplacedConstraintViolation {
    None,
    Minimum,
    Maximum,
}

impl ReplacedConstraintViolation {
    fn classify(value: f32, bounds: ReplacedAxisBounds) -> Self {
        if value < bounds.min.points() {
            Self::Minimum
        } else if value > bounds.max.points() {
            Self::Maximum
        } else {
            Self::None
        }
    }
}

/// Resolve a definite image content height while preserving indefinite percentages as auto.
///
/// CSS 2.2 treats percentage heights as auto when the containing block block
/// size is not definite. Image layout currently reaches this helper only in
/// those indefinite block-axis contexts, so percentage heights remain unresolved
/// here while absolute lengths still honor `box-sizing`:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-height-property> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(super) fn definite_image_content_height_without_percent(
    style: &ComputedStyle,
    vertical_non_content: f32,
) -> Option<f32> {
    let height = style.box_values.height.length_if_no_percent()?;
    Some(match style.box_sizing {
        BoxSizing::BorderBox => (height - vertical_non_content).max(0.0),
        BoxSizing::ContentBox => height.max(0.0),
    })
}

/// Resolve the used content size of an HTML `<canvas>` with an optional
/// containing-block height for percentage resolution.
///
/// CSS 2.2 treats percentage heights as auto unless the containing block has a
/// definite block size. Table-cell content relayout can provide that definite
/// basis after row height distribution:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-height-property> and
/// <https://drafts.csswg.org/css-tables-3/#table-cell-content-relayout>.
pub(super) fn used_canvas_size_with_height_basis(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
    height_basis: BlockSizePercentageBasis,
) -> (f32, f32) {
    let size = used_canvas(element, style, available_width, height_basis);
    (size.content_size.width, size.content_size.height)
}

/// Resolve an HTML canvas into its content and border boxes.
///
/// This is the canonical canvas geometry entry point for block, inline, table,
/// grid, and flex layout. Callers add margins only after selecting the box they
/// need, preventing canvas padding and borders from disappearing from inline
/// atoms or being counted twice during flex replay.
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
pub(super) fn used_canvas(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
    height_basis: BlockSizePercentageBasis,
) -> UsedReplacedBox {
    used_replaced_box(
        intrinsic_canvas_size(element),
        style,
        content_box_pt(available_width),
        height_basis,
    )
}

pub(super) fn used_canvas_with_intrinsic_sizing_context(
    element: &Element,
    style: &ComputedStyle,
    sizing: ReplacedBoxSizingContext,
) -> UsedReplacedBox {
    used_replaced_box_with_inline_percentage_basis(
        intrinsic_canvas_size(element),
        style,
        sizing.available_width,
        sizing.inline_percentage_basis,
        sizing.block_basis.descendant_percentage_basis(),
    )
}

/// Resolve an inline SVG element through the same typed intrinsic context as
/// raster images and canvas.
pub(super) fn used_svg_with_intrinsic_sizing_context(
    element: &Element,
    style: &ComputedStyle,
    sizing: ReplacedBoxSizingContext,
) -> Option<UsedReplacedBox> {
    intrinsic_svg_size(element).map(|intrinsic| {
        used_replaced_box_with_inline_percentage_basis(
            intrinsic,
            style,
            sizing.available_width,
            sizing.inline_percentage_basis,
            sizing.block_basis.descendant_percentage_basis(),
        )
    })
}

/// Resolve an inline SVG element through the same replaced-element geometry as
/// canvas and image resources.
pub(super) fn used_svg(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
    height_basis: BlockSizePercentageBasis,
) -> Option<UsedReplacedBox> {
    intrinsic_svg_size(element).map(|intrinsic| {
        used_replaced_box(
            intrinsic,
            style,
            content_box_pt(available_width),
            height_basis,
        )
    })
}

pub(super) fn used_background_size(
    image: &DecodedPngImage,
    positioning_area: PaintSize,
    value: css::BackgroundSize,
    intrinsic_resolution: f32,
) -> PaintSize {
    let natural_size = image.natural_layout_size();
    let intrinsic_resolution = intrinsic_resolution.max(f32::MIN_POSITIVE);
    used_background_size_from_intrinsic_dimensions(
        positioning_area,
        value,
        CssImageNaturalDimensions::from_raster_layout_size(natural_size, intrinsic_resolution),
    )
}

/// CSS-facing natural dimensions used by image sizing algorithms.
///
/// Concrete image decoders may need fallback dimensions to render source
/// content, but `background-size` must distinguish those from actual
/// intrinsic dimensions. This is particularly important for SVG images with
/// omitted or percentage root dimensions.
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct CssImageNaturalDimensions {
    axes: ImageNaturalAxes,
    pub(super) aspect_ratio: Option<f32>,
}

/// The intrinsic axes that CSS Backgrounds can learn from an image source.
///
/// A source may provide neither axis, exactly one axis, or a complete size.
/// These cases have different `background-size: auto` behavior, so retain the
/// state rather than reducing it to optional untyped scalar fields.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ImageNaturalAxes {
    None,
    Width(euclid::Length<f32, PaintSpace>),
    Height(euclid::Length<f32, PaintSpace>),
    Size(PaintSize),
}

impl CssImageNaturalDimensions {
    #[cfg(test)]
    pub(super) fn without_size(aspect_ratio: Option<f32>) -> Self {
        Self {
            axes: ImageNaturalAxes::None,
            aspect_ratio,
        }
    }

    pub(in crate::layout) fn from_layout_axes(
        width: Option<LayoutLength>,
        height: Option<LayoutLength>,
        aspect_ratio: Option<f32>,
    ) -> Self {
        let width = width.and_then(Self::paint_axis);
        let height = height.and_then(Self::paint_axis);
        let axes = match (width, height) {
            (Some(width), Some(height)) => {
                ImageNaturalAxes::Size(PaintSize::new(width.get(), height.get()))
            }
            (Some(width), None) => ImageNaturalAxes::Width(width),
            (None, Some(height)) => ImageNaturalAxes::Height(height),
            (None, None) => ImageNaturalAxes::None,
        };
        Self { axes, aspect_ratio }
    }

    /// Build fully definite natural dimensions from a raster source or a
    /// source rectangle already resolved into CSS layout space.
    pub(in crate::layout) fn from_layout_size(size: LayoutSize) -> Self {
        let width = layout_pt(size.width);
        let height = layout_pt(size.height);
        Self::from_layout_axes(
            Some(width),
            Some(height),
            (width > layout_pt(0.0) && height > layout_pt(0.0))
                .then_some(width.points() / height.points()),
        )
    }

    fn from_raster_layout_size(size: LayoutSize, intrinsic_resolution: f32) -> Self {
        let width = layout_pt(size.width / intrinsic_resolution);
        let height = layout_pt(size.height / intrinsic_resolution);
        Self::from_layout_axes(
            Some(width),
            Some(height),
            (width.get() > 0.0 && height.get() > 0.0).then_some(width.get() / height.get()),
        )
    }

    fn paint_axis(value: LayoutLength) -> Option<euclid::Length<f32, PaintSpace>> {
        let value = value.max(layout_pt(0.0));
        value.get().is_finite().then_some(value.cast_unit())
    }

    fn width(self) -> Option<euclid::Length<f32, PaintSpace>> {
        match self.axes {
            ImageNaturalAxes::Width(width) => Some(width),
            ImageNaturalAxes::Size(size) => Some(euclid::Length::new(size.width)),
            ImageNaturalAxes::None | ImageNaturalAxes::Height(_) => None,
        }
    }

    fn height(self) -> Option<euclid::Length<f32, PaintSpace>> {
        match self.axes {
            ImageNaturalAxes::Height(height) => Some(height),
            ImageNaturalAxes::Size(size) => Some(euclid::Length::new(size.height)),
            ImageNaturalAxes::None | ImageNaturalAxes::Width(_) => None,
        }
    }

    /// Restrict natural dimensions to a normalized source rectangle.
    ///
    /// `object-view-box` selects this source before CSS resolves the concrete
    /// object size, so any present natural axes and preferred ratio must be
    /// scaled together rather than reconstructed from a fallback size.
    pub(in crate::layout) fn scaled(self, width_scale: f32, height_scale: f32) -> Self {
        debug_assert!(width_scale.is_finite() && width_scale > 0.0);
        debug_assert!(height_scale.is_finite() && height_scale > 0.0);
        let scale_axis =
            |axis: euclid::Length<f32, PaintSpace>, scale| euclid::Length::new(axis.get() * scale);
        let axes = match self.axes {
            ImageNaturalAxes::None => ImageNaturalAxes::None,
            ImageNaturalAxes::Width(width) => {
                ImageNaturalAxes::Width(scale_axis(width, width_scale))
            }
            ImageNaturalAxes::Height(height) => {
                ImageNaturalAxes::Height(scale_axis(height, height_scale))
            }
            ImageNaturalAxes::Size(size) => ImageNaturalAxes::Size(PaintSize::new(
                size.width * width_scale,
                size.height * height_scale,
            )),
        };
        let aspect_ratio = self
            .aspect_ratio
            .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
            .and_then(|ratio| {
                let scaled = ratio * width_scale / height_scale;
                (scaled.is_finite() && scaled > 0.0).then_some(scaled)
            });
        Self { axes, aspect_ratio }
    }

    /// Resolve default object sizing with no specified dimensions.
    ///
    /// This is shared by `background-size: auto auto` and `object-fit: none`.
    /// <https://www.w3.org/TR/css-images-3/#default-sizing>
    pub(in crate::layout) fn default_size(self, default_object_size: PaintSize) -> PaintSize {
        let default_object_size = PaintSize::new(
            default_object_size.width.max(0.0),
            default_object_size.height.max(0.0),
        );
        let ratio = self
            .aspect_ratio
            .filter(|ratio| ratio.is_finite() && *ratio > 0.0);
        match (self.width(), self.height(), ratio) {
            (Some(width), Some(height), _) => PaintSize::new(width.get(), height.get()),
            (Some(width), None, Some(ratio)) => PaintSize::new(width.get(), width.get() / ratio),
            (None, Some(height), Some(ratio)) => PaintSize::new(height.get() * ratio, height.get()),
            (Some(width), None, None) => PaintSize::new(width.get(), default_object_size.height),
            (None, Some(height), None) => PaintSize::new(default_object_size.width, height.get()),
            (None, None, Some(ratio)) => background_size_contain(default_object_size, ratio),
            (None, None, None) => default_object_size,
        }
    }

    /// Resolve a contain constraint against the image's preferred ratio.
    pub(in crate::layout) fn contain_size(self, constraint: PaintSize) -> PaintSize {
        self.aspect_ratio
            .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
            .map(|ratio| background_size_contain(constraint, ratio))
            .unwrap_or(constraint)
    }

    /// Resolve a cover constraint against the image's preferred ratio.
    pub(in crate::layout) fn cover_size(self, constraint: PaintSize) -> PaintSize {
        self.aspect_ratio
            .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
            .map(|ratio| background_size_cover(constraint, ratio))
            .unwrap_or(constraint)
    }
}

/// Resolve a background image's used size from its intrinsic dimensions.
///
/// This implements the CSS Backgrounds used-size algorithm, including the
/// special cases for images with one or no intrinsic dimensions and for images
/// that have an intrinsic ratio but no intrinsic size.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
pub(super) fn used_background_size_from_intrinsic_dimensions(
    positioning_area: PaintSize,
    value: css::BackgroundSize,
    intrinsic: CssImageNaturalDimensions,
) -> PaintSize {
    let positioning_area = PaintSize::new(
        positioning_area.width.max(0.0),
        positioning_area.height.max(0.0),
    );
    let ratio = intrinsic
        .aspect_ratio
        .filter(|ratio| ratio.is_finite() && *ratio > 0.0);
    let auto_size = || intrinsic.default_size(positioning_area);

    match value {
        css::BackgroundSize::Auto => auto_size(),
        css::BackgroundSize::Cover => intrinsic.cover_size(positioning_area),
        css::BackgroundSize::Contain => intrinsic.contain_size(positioning_area),
        css::BackgroundSize::Explicit { width, height } => match (
            used_background_size_axis(width, positioning_area.width),
            used_background_size_axis(height, positioning_area.height),
        ) {
            (Some(width), Some(height)) => PaintSize::new(width, height),
            (Some(width), None) => ratio
                .map(|ratio| PaintSize::new(width, width / ratio))
                .unwrap_or_else(|| {
                    PaintSize::new(
                        width,
                        intrinsic
                            .height()
                            .map_or(positioning_area.height, |height| height.get()),
                    )
                }),
            (None, Some(height)) => ratio
                .map(|ratio| PaintSize::new(height * ratio, height))
                .unwrap_or_else(|| {
                    PaintSize::new(
                        intrinsic
                            .width()
                            .map_or(positioning_area.width, |width| width.get()),
                        height,
                    )
                }),
            (None, None) => auto_size(),
        },
    }
}

fn background_size_cover(positioning_area: PaintSize, ratio: f32) -> PaintSize {
    if positioning_area.width <= 0.0 && positioning_area.height <= 0.0 {
        return PaintSize::new(0.0, 0.0);
    }
    let scale = (positioning_area.width / ratio).max(positioning_area.height);
    PaintSize::new(scale * ratio, scale)
}

fn background_size_contain(positioning_area: PaintSize, ratio: f32) -> PaintSize {
    if positioning_area.width <= 0.0 || positioning_area.height <= 0.0 {
        return PaintSize::new(0.0, 0.0);
    }
    let scale = (positioning_area.width / ratio).min(positioning_area.height);
    PaintSize::new(scale * ratio, scale)
}

/// Resolves one computed `background-size` axis.
///
/// CSS Backgrounds and Borders resolves explicit size percentages against the
/// background positioning area:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>.
pub(super) fn used_background_size_axis(
    value: css::BackgroundSizeAxis,
    reference: f32,
) -> Option<f32> {
    match value {
        css::BackgroundSizeAxis::Auto => None,
        css::BackgroundSizeAxis::LengthPercentage(value) => Some(
            used_length_percentage(
                value,
                PercentageBasis::definite(layout_pt(reference.max(0.0))),
            )
            .points(),
        ),
    }
}

pub(super) fn background_position(
    value: css::BackgroundPosition,
    positioning_area: PaintSize,
    image_size: PaintSize,
) -> PaintBackgroundOffset {
    // Keep the free space and its position calculation in f64. A valid SVG
    // `cover` size can be far larger than its background positioning area;
    // doing `area - image` in f32 would discard the area entirely and make a
    // tile that should cover the box appear not to intersect it.
    let free_x = f64::from(positioning_area.width) - f64::from(image_size.width);
    let free_y = f64::from(positioning_area.height) - f64::from(image_size.height);
    PaintBackgroundOffset::new(
        used_background_position_axis_precise(value.x, free_x, false),
        used_background_position_axis_precise(value.y, free_y, true),
    )
}

/// Resolve a background-position axis without discarding a finite percentage
/// basis merely because the image is much larger than its positioning area.
///
/// CSS Backgrounds positions an image by applying the offset to the remaining
/// space. Components with unresolved metric-relative terms continue through
/// the established f32 used-value resolver; ordinary computed components are
/// already absolute apart from their percentage and can retain f64 precision.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-position>
fn used_background_position_axis_precise(
    axis: css::BackgroundPositionAxis,
    free_space: f64,
    invert_start_end: bool,
) -> f64 {
    let offset = axis
        .offset
        .percentage_coefficient()
        .map(|percentage| {
            f64::from(axis.offset.fixed_component().points()) + f64::from(percentage) * free_space
        })
        .unwrap_or_else(|| {
            f64::from(
                used_length_percentage(
                    axis.offset,
                    PercentageBasis::definite(layout_pt(free_space as f32)),
                )
                .points(),
            )
        });
    match (axis.origin, invert_start_end) {
        (css::BackgroundPositionOrigin::Start, false) => offset,
        (css::BackgroundPositionOrigin::Start, true) => free_space - offset,
        (css::BackgroundPositionOrigin::Center, _) => free_space / 2.0 + offset,
        (css::BackgroundPositionOrigin::End, false) => free_space - offset,
        (css::BackgroundPositionOrigin::End, true) => offset,
    }
}

/// Source-coordinate offsets for the four `border-image-slice` edges.
///
/// These are deliberately floating point: SVG slices use the SVG viewport's
/// coordinate system and CSS permits percentage-derived fractional source
/// coordinates. Raster sources are quantized only by the PDF image adapter.
/// <https://drafts.csswg.org/css-backgrounds-3/#border-image-slice>
#[derive(Debug, Clone, Copy)]
pub(super) struct UsedBorderImageSlices {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Resolve `border-image-slice` against the source image dimensions.
///
/// CSS Backgrounds and Borders resolves percentages against image dimensions
/// and clamps each edge to the corresponding image dimension. Opposing slices
/// are intentionally allowed to overlap; in that case the relevant middle
/// image is empty rather than the source slices being rescaled:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>.
pub(super) fn used_border_image_slices(
    values: css::BorderImageSliceOffsets,
    image_width: f32,
    image_height: f32,
) -> UsedBorderImageSlices {
    UsedBorderImageSlices {
        top: used_border_image_slice_value(values.top, image_height),
        right: used_border_image_slice_value(values.right, image_width),
        bottom: used_border_image_slice_value(values.bottom, image_height),
        left: used_border_image_slice_value(values.left, image_width),
    }
}

fn used_border_image_slice_value(value: css::BorderImageSliceValue, reference: f32) -> f32 {
    let resolved = match value {
        css::BorderImageSliceValue::Number(value) => value,
        css::BorderImageSliceValue::Percent(value) => value * reference,
    };
    resolved.clamp(0.0, reference.max(0.0))
}

/// Resolve `border-image-width` to destination border-image side widths.
///
/// Numeric values multiply the corresponding computed border width; lengths and
/// percentages resolve against the border image area dimensions. `auto` uses
/// the intrinsic size of the corresponding image slice, falling back to the
/// used border width only when that slice dimension is unavailable:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-width>.
pub(super) fn used_border_image_widths(
    style: &ComputedStyle,
    computed_border_widths: css::Edges,
    border_image_area_width: BorderBoxLength,
    border_image_area_height: BorderBoxLength,
    slices: UsedBorderImageSlices,
) -> css::Edges {
    css::Edges {
        top: used_border_image_width_value(
            style.border_image.width.top.clone(),
            layout_pt(computed_border_widths.top),
            border_image_area_height,
            slices.top,
        )
        .points(),
        right: used_border_image_width_value(
            style.border_image.width.right.clone(),
            layout_pt(computed_border_widths.right),
            border_image_area_width,
            slices.right,
        )
        .points(),
        bottom: used_border_image_width_value(
            style.border_image.width.bottom.clone(),
            layout_pt(computed_border_widths.bottom),
            border_image_area_height,
            slices.bottom,
        )
        .points(),
        left: used_border_image_width_value(
            style.border_image.width.left.clone(),
            layout_pt(computed_border_widths.left),
            border_image_area_width,
            slices.left,
        )
        .points(),
    }
}

fn used_border_image_width_value(
    value: css::BorderImageWidthValue,
    border_width: LayoutLength,
    reference: BorderBoxLength,
    slice_width: f32,
) -> LayoutLength {
    match value {
        css::BorderImageWidthValue::Auto => {
            if slice_width > 0.0 {
                // Border-image slice numbers are source CSS pixels. Convert
                // the intrinsic slice extent into Quire's PDF-point layout
                // space before using it as an `auto` border-image width.
                // <https://www.w3.org/TR/css-backgrounds-3/#border-image-width>
                layout_px(slice_width)
            } else {
                border_width
            }
        }
        css::BorderImageWidthValue::Number(value) => border_width * value,
        css::BorderImageWidthValue::LengthPercentage(value) => used_length_percentage(
            value,
            PercentageBasis::definite(reference.into_layout_length()),
        ),
    }
    .max(layout_pt(0.0))
}

/// Proportionally fit border-image widths inside the border-image area.
///
/// CSS Backgrounds and Borders scales all four used `border-image-width`
/// values down by a common factor when opposite sides would overlap:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>.
pub(super) fn fit_border_image_widths_to_area(
    widths: css::Edges,
    area_width: f32,
    area_height: f32,
) -> css::Edges {
    let horizontal_sum = widths.left + widths.right;
    let vertical_sum = widths.top + widths.bottom;
    let mut factor = 1.0_f32;
    if horizontal_sum > area_width && horizontal_sum > 0.0 {
        factor = factor.min(area_width / horizontal_sum);
    }
    if vertical_sum > area_height && vertical_sum > 0.0 {
        factor = factor.min(area_height / vertical_sum);
    }
    if factor >= 1.0 {
        widths
    } else {
        css::Edges {
            top: widths.top * factor,
            right: widths.right * factor,
            bottom: widths.bottom * factor,
            left: widths.left * factor,
        }
    }
}

/// Resolve `border-image-outset` to physical outsets.
///
/// Numeric values multiply the corresponding computed border width; length values
/// are absolute:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-outset>.
pub(super) fn used_border_image_outsets(
    style: &ComputedStyle,
    border_widths: css::Edges,
) -> css::Edges {
    css::Edges {
        top: used_border_image_outset_value(
            style.border_image.outset.top.clone(),
            border_widths.top,
        ),
        right: used_border_image_outset_value(
            style.border_image.outset.right.clone(),
            border_widths.right,
        ),
        bottom: used_border_image_outset_value(
            style.border_image.outset.bottom.clone(),
            border_widths.bottom,
        ),
        left: used_border_image_outset_value(
            style.border_image.outset.left.clone(),
            border_widths.left,
        ),
    }
}

fn used_border_image_outset_value(value: css::BorderImageOutsetValue, border_width: f32) -> f32 {
    match value {
        css::BorderImageOutsetValue::Number(value) => value * border_width,
        css::BorderImageOutsetValue::Length(value) => value.length_points(),
    }
    .max(0.0)
}

/// Resolves one computed `background-position` axis to the renderer's PDF
/// coordinate space.
///
/// CSS Backgrounds and Borders positions images in the positioning area; the
/// vertical result is inverted here because PDF page coordinates in this
/// renderer are top-origin for rectangles but image placement uses bottom
/// offsets:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-position>.
pub(super) fn used_background_position_axis(
    axis: css::BackgroundPositionAxis,
    free_space: f32,
    invert_start_end: bool,
) -> f32 {
    let offset = used_length_percentage(
        axis.offset,
        PercentageBasis::definite(layout_pt(free_space)),
    )
    .points();
    match (axis.origin, invert_start_end) {
        (css::BackgroundPositionOrigin::Start, false) => offset,
        (css::BackgroundPositionOrigin::Start, true) => free_space - offset,
        (css::BackgroundPositionOrigin::Center, _) => free_space / 2.0 + offset,
        (css::BackgroundPositionOrigin::End, false) => free_space - offset,
        (css::BackgroundPositionOrigin::End, true) => offset,
    }
}

pub(super) fn svg_rect(element: &Element) -> Option<(f32, f32, CssColor)> {
    crate::svg::svg_intrinsic_size(element)
        .map(|size| (size.width, size.height, CssColor::TRANSPARENT))
}

pub(super) fn parse_html_length(value: &str) -> Option<f32> {
    let value = value.trim();
    if let Some(number) = value.strip_suffix("px") {
        return number
            .trim()
            .parse::<f32>()
            .ok()
            .map(|value| value * css::CSS_PX_TO_PT);
    }
    if let Some(number) = value.strip_suffix("pt") {
        return number.trim().parse().ok();
    }
    value
        .parse::<f32>()
        .ok()
        .map(|value| value * css::CSS_PX_TO_PT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_images_cannot_select_the_encoded_orientation_policy() {
        assert_eq!(
            orientation_policy_for_taint(
                RasterOrientationPolicy::Encoded,
                crate::resource::ImageFetchTaint::Opaque,
            ),
            RasterOrientationPolicy::FromImage
        );
        assert_eq!(
            orientation_policy_for_taint(
                RasterOrientationPolicy::Encoded,
                crate::resource::ImageFetchTaint::OriginClean,
            ),
            RasterOrientationPolicy::Encoded
        );
    }
    use std::rc::Rc;

    use crate::units::{border_box_size_pt, border_box_to_content_box_size};

    fn transparent_decoded_image() -> DecodedPngImage {
        DecodedPngImage::new(1, 1, vec![0, 0, 0], Some(vec![0]))
    }

    fn canvas_element(attrs: &[(&str, &str)]) -> Element {
        element_with_attrs("canvas", attrs)
    }

    fn element_with_attrs(tag: &str, attrs: &[(&str, &str)]) -> Element {
        let NodeKind::Element(mut element) = Node::element(tag).kind else {
            unreachable!("element constructor must produce an element")
        };
        element.attrs.extend(
            attrs
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string())),
        );
        element
    }

    fn automatic_replaced_axes() -> ReplacedPreferredSizeAxes {
        ReplacedPreferredSizeAxes {
            width: ReplacedPreferredSize::Automatic,
            height: ReplacedPreferredSize::Automatic,
        }
    }

    #[test]
    fn automatic_replaced_size_uses_the_css22_min_max_constraint_table() {
        struct Case {
            name: &'static str,
            size: ContentBoxSize,
            constraints: ReplacedSizeConstraints,
            expected: ContentBoxSize,
        }

        let cases = [
            Case {
                name: "no violation",
                size: content_box_size_pt(100.0, 50.0),
                constraints: ReplacedSizeConstraints {
                    min_width: None,
                    max_width: None,
                    min_height: None,
                    max_height: None,
                },
                expected: content_box_size_pt(100.0, 50.0),
            },
            Case {
                name: "maximum width",
                size: content_box_size_pt(200.0, 100.0),
                constraints: ReplacedSizeConstraints {
                    min_width: None,
                    max_width: Some(content_box_pt(150.0)),
                    min_height: None,
                    max_height: None,
                },
                expected: content_box_size_pt(150.0, 75.0),
            },
            Case {
                name: "minimum width",
                size: content_box_size_pt(100.0, 50.0),
                constraints: ReplacedSizeConstraints {
                    min_width: Some(content_box_pt(150.0)),
                    max_width: None,
                    min_height: None,
                    max_height: None,
                },
                expected: content_box_size_pt(150.0, 75.0),
            },
            Case {
                name: "maximum height",
                size: content_box_size_pt(200.0, 100.0),
                constraints: ReplacedSizeConstraints {
                    min_width: None,
                    max_width: None,
                    min_height: None,
                    max_height: Some(content_box_pt(75.0)),
                },
                expected: content_box_size_pt(150.0, 75.0),
            },
            Case {
                name: "minimum height",
                size: content_box_size_pt(100.0, 50.0),
                constraints: ReplacedSizeConstraints {
                    min_width: None,
                    max_width: None,
                    min_height: Some(content_box_pt(75.0)),
                    max_height: None,
                },
                expected: content_box_size_pt(150.0, 75.0),
            },
            Case {
                name: "both maxima",
                size: content_box_size_pt(200.0, 100.0),
                constraints: ReplacedSizeConstraints {
                    min_width: None,
                    max_width: Some(content_box_pt(150.0)),
                    min_height: None,
                    max_height: Some(content_box_pt(60.0)),
                },
                expected: content_box_size_pt(120.0, 60.0),
            },
            Case {
                name: "both minima",
                size: content_box_size_pt(100.0, 50.0),
                constraints: ReplacedSizeConstraints {
                    min_width: Some(content_box_pt(150.0)),
                    max_width: None,
                    min_height: Some(content_box_pt(100.0)),
                    max_height: None,
                },
                expected: content_box_size_pt(200.0, 100.0),
            },
            Case {
                name: "minimum width and maximum height",
                size: content_box_size_pt(100.0, 50.0),
                constraints: ReplacedSizeConstraints {
                    min_width: Some(content_box_pt(150.0)),
                    max_width: None,
                    min_height: None,
                    max_height: Some(content_box_pt(25.0)),
                },
                expected: content_box_size_pt(150.0, 25.0),
            },
            Case {
                name: "maximum width and minimum height",
                size: content_box_size_pt(200.0, 100.0),
                constraints: ReplacedSizeConstraints {
                    min_width: None,
                    max_width: Some(content_box_pt(150.0)),
                    min_height: Some(content_box_pt(125.0)),
                    max_height: None,
                },
                expected: content_box_size_pt(150.0, 125.0),
            },
            Case {
                name: "maximum is normalized by minimum",
                size: content_box_size_pt(200.0, 100.0),
                constraints: ReplacedSizeConstraints {
                    min_width: Some(content_box_pt(150.0)),
                    max_width: Some(content_box_pt(100.0)),
                    min_height: None,
                    max_height: None,
                },
                expected: content_box_size_pt(150.0, 75.0),
            },
        ];

        for case in cases {
            assert_eq!(
                resolve_replaced_size_with_aspect_ratio(
                    case.size,
                    2.0,
                    automatic_replaced_axes(),
                    case.constraints,
                ),
                case.expected,
                "{}",
                case.name,
            );
        }
    }

    #[test]
    fn ratio_only_replaced_size_resolves_before_min_max_constraints() {
        let intrinsic = IntrinsicReplacedSize {
            width: content_box_pt(225.0),
            height: content_box_pt(112.5),
            preferred_aspect_ratio: Some(2.0),
            has_intrinsic_size: false,
            attr_width: None,
            attr_height: None,
        };
        let mut style = ComputedStyle::initial();
        style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(75.0),
        );
        style.box_values.max_width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(75.0),
        );

        let geometry = used_replaced_box(
            intrinsic,
            &style,
            content_box_pt(37.5),
            PercentageBasis::indefinite(),
        );

        assert_eq!(geometry.content_size, content_box_size_pt(75.0, 75.0));
    }

    #[test]
    fn square_ratio_only_replaced_size_transfers_min_height_after_stretch_fit() {
        let intrinsic = IntrinsicReplacedSize {
            width: content_box_pt(225.0),
            height: content_box_pt(112.5),
            preferred_aspect_ratio: Some(1.0),
            has_intrinsic_size: false,
            attr_width: None,
            attr_height: None,
        };
        let mut style = ComputedStyle::initial();
        style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(75.0),
        );

        let geometry = used_replaced_box(
            intrinsic,
            &style,
            content_box_pt(37.5),
            PercentageBasis::indefinite(),
        );

        assert_eq!(geometry.content_size, content_box_size_pt(75.0, 75.0));
    }

    #[test]
    fn transferred_constraint_does_not_modify_a_definite_preferred_axis() {
        let size = resolve_replaced_size_with_aspect_ratio(
            content_box_size_pt(75.0, 37.5),
            2.0,
            ReplacedPreferredSizeAxes {
                width: ReplacedPreferredSize::Definite,
                height: ReplacedPreferredSize::Automatic,
            },
            ReplacedSizeConstraints {
                min_width: None,
                max_width: None,
                min_height: Some(content_box_pt(75.0)),
                max_height: None,
            },
        );

        assert_eq!(size, content_box_size_pt(75.0, 75.0));
    }

    #[test]
    fn percent_encoded_svg_data_url_resolves_as_a_vector_asset() {
        let cache = ResourceCache::default();
        let source = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2220%22%20height%3D%2210%22%3E%3Cpath%20d%3D%22M0%200H20V10H0Z%22%20fill%3D%22red%22%2F%3E%3C%2Fsvg%3E";
        let asset = load_resolved_image_source(
            source,
            None,
            None,
            &cache,
            RasterOrientationPolicy::FromImage,
            SvgImageContext::default(),
        )
        .unwrap();
        let ResolvedImageAsset::Svg(asset) = asset else {
            panic!("expected SVG asset");
        };
        assert_eq!(asset.intrinsic_size(), LayoutSize::new(15.0, 7.5));
        assert_eq!(
            asset
                .paint_paths(crate::document::paint::geometry::PaintRect::new(
                    crate::document::paint::geometry::PaintPoint::new(0.0, 0.0),
                    crate::document::paint::geometry::PaintSize::new(15.0, 7.5),
                ))
                .len(),
            1
        );
    }

    #[test]
    fn base64_svg_data_url_resolves_as_a_vector_asset() {
        let cache = ResourceCache::default();
        let source = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyMCIgaGVpZ2h0PSIxMCI+PHBhdGggZD0iTTAgMEgyMFYxMEgwWiIgZmlsbD0icmVkIi8+PC9zdmc+";
        let asset = load_resolved_image_source(
            source,
            None,
            None,
            &cache,
            RasterOrientationPolicy::FromImage,
            SvgImageContext::default(),
        )
        .unwrap();

        assert!(matches!(asset, ResolvedImageAsset::Svg(_)));
    }

    #[tokio::test]
    async fn preloaded_svg_file_resolves_as_a_vector_asset() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/external-vector.svg");
        let url = url::Url::from_file_path(path).unwrap();
        let fetcher = crate::resource::ResourceFetcher::new(crate::ResourcePolicy::default())
            .expect("default resource policy must create an HTTP client");
        let cache = ResourceCache::preload(&fetcher, [url.clone()])
            .await
            .expect("local image fixture must preload");
        assert!(matches!(
            cache.image_asset_url_with_orientation(
                &url,
                RasterOrientationPolicy::FromImage,
                SvgImageContext::default(),
            ),
            Some(crate::resource::ResourceImageAsset::Svg(_))
        ));
    }

    #[test]
    fn repeated_data_url_images_share_layout_placeholder_storage() {
        let cache = ResourceCache::default();
        let source = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

        let first = load_image_source(source, None, None, &cache, true).unwrap();
        let second = load_image_source(source, None, None, &cache, true).unwrap();

        assert!(Rc::ptr_eq(&first.rgb.shared(), &second.rgb.shared()));
        assert!(match (&first.alpha, &second.alpha) {
            (Some(first), Some(second)) => Rc::ptr_eq(first, second),
            (None, None) => true,
            _ => false,
        });
    }

    #[test]
    fn used_image_new_expands_content_size_to_border_box_size() {
        let image = UsedImage::new(
            transparent_decoded_image(),
            content_box_size_pt(150.0, 100.0),
            non_content_pt(20.0),
            non_content_pt(30.0),
        );

        assert_eq!(image.content_size.width, 150.0);
        assert_eq!(image.content_size.height, 100.0);
        assert_eq!(image.border_box_size.width, 170.0);
        assert_eq!(image.border_box_size.height, 130.0);
    }

    #[test]
    fn used_replaced_box_expands_content_size_to_border_box_size() {
        let size = UsedReplacedBox::new(
            content_box_size_pt(150.0, 100.0),
            non_content_pt(20.0),
            non_content_pt(30.0),
        );

        assert_eq!(size.content_size.width, 150.0);
        assert_eq!(size.content_size.height, 100.0);
        assert_eq!(size.border_box_size.width, 170.0);
        assert_eq!(size.border_box_size.height, 130.0);
    }

    #[test]
    fn intrinsic_canvas_size_uses_html_defaults_and_attributes() {
        let default_canvas = intrinsic_canvas_size(&canvas_element(&[]));
        assert_eq!(default_canvas.width, content_box_pt(225.0));
        assert_eq!(default_canvas.height, content_box_pt(112.5));
        assert_eq!(default_canvas.attr_width, None);
        assert_eq!(default_canvas.attr_height, None);

        let attributed_canvas =
            intrinsic_canvas_size(&canvas_element(&[("width", "96"), ("height", "48px")]));
        assert_eq!(attributed_canvas.width, content_box_pt(72.0));
        assert_eq!(attributed_canvas.height, content_box_pt(36.0));
        assert_eq!(attributed_canvas.attribute_aspect_ratio(), Some(2.0));
    }

    #[test]
    fn shared_replaced_geometry_applies_box_sizing_once_for_canvas_image_and_svg() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(120.0),
        );
        style.padding.left = 10.0;
        style.padding.right = 10.0;

        let canvas = used_canvas(
            &canvas_element(&[("width", "200"), ("height", "100")]),
            &style,
            500.0,
            BlockSizePercentageBasis::indefinite(),
        );
        let image = used_replaced_box(
            IntrinsicReplacedSize {
                width: content_box_pt(150.0),
                height: content_box_pt(75.0),
                preferred_aspect_ratio: Some(2.0),
                has_intrinsic_size: true,
                attr_width: Some(content_box_pt(150.0)),
                attr_height: Some(content_box_pt(75.0)),
            },
            &style,
            content_box_pt(500.0),
            BlockSizePercentageBasis::indefinite(),
        );
        let svg = used_svg(
            &element_with_attrs("svg", &[("width", "200"), ("height", "100")]),
            &style,
            500.0,
            BlockSizePercentageBasis::indefinite(),
        )
        .unwrap();

        for geometry in [canvas, image, svg] {
            assert_eq!(geometry.content_size.width, 100.0);
            assert_eq!(geometry.content_size.height, 50.0);
            assert_eq!(geometry.border_box_size.width, 120.0);
            assert_eq!(geometry.border_box_size.height, 50.0);
        }
    }

    #[test]
    fn shared_replaced_geometry_transfers_auto_axis_through_min_width() {
        let mut style = ComputedStyle::initial();
        style.box_values.min_width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(180.0),
        );

        let geometry = used_replaced_box(
            IntrinsicReplacedSize {
                width: content_box_pt(150.0),
                height: content_box_pt(75.0),
                preferred_aspect_ratio: Some(2.0),
                has_intrinsic_size: true,
                attr_width: None,
                attr_height: None,
            },
            &style,
            content_box_pt(500.0),
            BlockSizePercentageBasis::indefinite(),
        );

        assert_eq!(geometry.content_size.width, 180.0);
        assert_eq!(geometry.content_size.height, 90.0);
    }

    #[test]
    fn no_ratio_replaced_element_keeps_the_independent_intrinsic_auto_axis() {
        let intrinsic = IntrinsicReplacedSize {
            width: content_box_pt(225.0),
            height: content_box_pt(112.5),
            preferred_aspect_ratio: None,
            has_intrinsic_size: false,
            attr_width: None,
            attr_height: None,
        };

        let mut explicit_width = ComputedStyle::initial();
        explicit_width.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(30.0),
        );
        let width_geometry = used_replaced_box(
            intrinsic,
            &explicit_width,
            content_box_pt(500.0),
            BlockSizePercentageBasis::indefinite(),
        );

        let mut explicit_height = ComputedStyle::initial();
        explicit_height.box_values.height.replace_with_used(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(15.0),
            ),
        );
        let height_geometry = used_replaced_box(
            intrinsic,
            &explicit_height,
            content_box_pt(500.0),
            BlockSizePercentageBasis::indefinite(),
        );

        assert_eq!(
            width_geometry.content_size,
            content_box_size_pt(30.0, 112.5)
        );
        assert_eq!(
            height_geometry.content_size,
            content_box_size_pt(225.0, 15.0)
        );
    }

    #[test]
    fn effective_zoom_scales_replaced_dimensions_but_not_aspect_ratio() {
        let intrinsic = IntrinsicReplacedSize {
            width: content_box_pt(100.0),
            height: content_box_pt(50.0),
            preferred_aspect_ratio: Some(2.0),
            has_intrinsic_size: true,
            attr_width: Some(content_box_pt(100.0)),
            attr_height: Some(content_box_pt(50.0)),
        }
        .scaled_by_effective_zoom(2.0);

        assert_eq!(intrinsic.width, content_box_pt(200.0));
        assert_eq!(intrinsic.height, content_box_pt(100.0));
        assert_eq!(intrinsic.attr_width, Some(content_box_pt(200.0)));
        assert_eq!(intrinsic.attr_height, Some(content_box_pt(100.0)));
        assert_eq!(intrinsic.natural_aspect_ratio(), Some(2.0));
    }

    #[test]
    fn replaced_percentage_max_height_uses_the_block_size_basis() {
        let mut style = ComputedStyle::initial();
        style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(1.0),
        );
        let intrinsic = IntrinsicReplacedSize {
            width: content_box_pt(200.0),
            height: content_box_pt(200.0),
            preferred_aspect_ratio: Some(1.0),
            has_intrinsic_size: true,
            attr_width: None,
            attr_height: None,
        };

        let definite = used_replaced_box(
            intrinsic,
            &style,
            content_box_pt(500.0),
            PercentageBasis::definite_from(
                content_box_pt(100.0),
                BlockSizeBasisSource::ContainingBlock,
            ),
        );
        let indefinite = used_replaced_box(
            intrinsic,
            &style,
            content_box_pt(500.0),
            PercentageBasis::indefinite(),
        );

        assert_eq!(definite.content_size.width, 100.0);
        assert_eq!(definite.content_size.height, 100.0);
        assert_eq!(indefinite.content_size.width, 200.0);
        assert_eq!(indefinite.content_size.height, 200.0);
    }

    #[test]
    fn only_closed_intrinsic_block_bases_create_replaced_percentage_bases() {
        assert!(
            !IntrinsicBlockBasis::Indefinite
                .descendant_percentage_basis()
                .is_definite()
        );

        for basis in [
            IntrinsicBlockBasis::DefiniteFromContainingBlock(content_box_pt(100.0)),
            IntrinsicBlockBasis::from_flex_layout(
                content_box_pt(100.0),
                FlexIntrinsicBlockBasisSource::ExistingFlexItem,
            ),
            IntrinsicBlockBasis::from_winning_constraint(
                content_box_pt(100.0),
                WinningBlockConstraintKind::Minimum,
            ),
        ] {
            assert_eq!(
                basis.descendant_percentage_basis().value(),
                Some(content_box_pt(100.0))
            );
        }
    }

    #[test]
    fn shared_intrinsic_replaced_context_transfers_definite_height_for_canvas_geometry() {
        let mut style = ComputedStyle::initial();
        style.box_values.height.replace_with_used(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(1.0),
            ),
        );
        let context = ReplacedBoxSizingContext {
            available_width: content_box_pt(500.0),
            inline_percentage_basis: PercentageBasis::indefinite(),
            block_basis: IntrinsicBlockBasis::from_flex_layout(
                content_box_pt(100.0),
                FlexIntrinsicBlockBasisSource::ExistingFlexItem,
            ),
        };
        let geometry = used_canvas_with_intrinsic_sizing_context(
            &canvas_element(&[("width", "10"), ("height", "10")]),
            &style,
            context,
        );

        assert_eq!(geometry.content_size, content_box_size_pt(100.0, 100.0));
    }

    #[test]
    fn image_intrinsic_context_matches_normal_flow_replaced_sizing() {
        let mut style = ComputedStyle::initial();
        style.box_values.height.replace_with_used(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(1.0),
            ),
        );
        let context = ReplacedBoxSizingContext {
            available_width: content_box_pt(500.0),
            inline_percentage_basis: PercentageBasis::indefinite(),
            block_basis: IntrinsicBlockBasis::from_flex_layout(
                content_box_pt(100.0),
                FlexIntrinsicBlockBasisSource::ExistingFlexItem,
            ),
        };
        let image = element_with_attrs(
            "img",
            &[(
                "src",
                "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2210%22%20height%3D%2210%22%2F%3E",
            )],
        );
        let resource_cache = ResourceCache::default();

        let intrinsic = used_image_with_intrinsic_sizing_context(
            &image,
            &style,
            context,
            None,
            None,
            &resource_cache,
        )
        .expect("data SVG image must establish a replaced box");
        let normal_flow = used_image(
            &image,
            &style,
            context.available_width.points(),
            context.block_basis.descendant_percentage_basis(),
            None,
            None,
            &resource_cache,
        )
        .expect("data SVG image must establish a replaced box");

        assert_eq!(intrinsic.content_size, content_box_size_pt(100.0, 100.0));
        assert_eq!(intrinsic.content_size, normal_flow.content_size);
        assert_eq!(intrinsic.border_box_size, normal_flow.border_box_size);
    }

    #[test]
    fn used_image_shared_border_to_content_conversion_clamps_at_zero() {
        let content = border_box_to_content_box_size(
            border_box_size_pt(100.0, 100.0),
            non_content_pt(150.0),
            non_content_pt(150.0),
        );

        assert_eq!(content.width, 0.0);
        assert_eq!(content.height, 0.0);
    }

    #[test]
    fn used_image_invalid_replacement_keeps_transparent_pixel_and_zero_content_size() {
        let style = ComputedStyle::initial();
        let image = used_invalid_replacement_image(&style, 100.0);

        assert_eq!(image.decoded.pixel_size.width, 1);
        assert_eq!(image.decoded.pixel_size.height, 1);
        assert_eq!(image.decoded.rgb.as_ref(), &[0, 0, 0]);
        assert_eq!(image.decoded.alpha.as_deref(), Some([0].as_slice()));
        assert_eq!(image.content_size.width, 0.0);
        assert_eq!(image.content_size.height, 0.0);
        assert_eq!(image.border_box_size.width, 0.0);
        assert_eq!(image.border_box_size.height, 0.0);
    }

    #[test]
    fn background_size_uses_contain_for_ratio_only_images() {
        let size = used_background_size_from_intrinsic_dimensions(
            PaintSize::new(300.0, 150.0),
            css::BackgroundSize::Auto,
            CssImageNaturalDimensions::without_size(Some(1.0 / 4.0)),
        );

        assert_eq!(size, PaintSize::new(37.5, 150.0));
    }

    #[test]
    fn background_size_fills_area_for_images_without_intrinsic_geometry() {
        let intrinsic = CssImageNaturalDimensions::without_size(None);

        assert_eq!(
            used_background_size_from_intrinsic_dimensions(
                PaintSize::new(300.0, 150.0),
                css::BackgroundSize::Auto,
                intrinsic,
            ),
            PaintSize::new(300.0, 150.0)
        );
        assert_eq!(
            used_background_size_from_intrinsic_dimensions(
                PaintSize::new(300.0, 150.0),
                css::BackgroundSize::Contain,
                intrinsic,
            ),
            PaintSize::new(300.0, 150.0)
        );
    }

    #[test]
    fn background_size_uses_positioning_area_for_auto_axis_without_ratio() {
        let size = used_background_size_from_intrinsic_dimensions(
            PaintSize::new(300.0, 150.0),
            css::BackgroundSize::Explicit {
                width: css::BackgroundSizeAxis::LengthPercentage(
                    css::ComputedLengthPercentage::from_points(50.0),
                ),
                height: css::BackgroundSizeAxis::Auto,
            },
            CssImageNaturalDimensions::without_size(None),
        );

        assert_eq!(size, PaintSize::new(50.0, 150.0));
    }

    #[test]
    fn background_size_cover_uses_the_nonzero_positioning_axis() {
        assert_eq!(
            background_size_cover(PaintSize::new(100.0, 0.0), 2.0),
            PaintSize::new(100.0, 50.0)
        );
        assert_eq!(
            background_size_cover(PaintSize::new(0.0, 100.0), 2.0),
            PaintSize::new(200.0, 100.0)
        );
        assert_eq!(
            background_size_cover(PaintSize::new(0.0, 0.0), 2.0),
            PaintSize::new(0.0, 0.0)
        );
    }

    #[test]
    fn background_position_returns_a_typed_high_precision_offset() {
        let offset: PaintBackgroundOffset = background_position(
            css::BackgroundPosition::INITIAL,
            PaintSize::new(100.0, 80.0),
            PaintSize::new(30.0, 20.0),
        );

        assert_eq!(offset, PaintBackgroundOffset::new(0.0, 60.0));
    }

    #[test]
    fn object_view_box_resolution_preserves_valid_source_geometry() {
        let view_box = css::ObjectViewBox::Xywh {
            x: css::ComputedLengthPercentage::from_points(25.0),
            y: css::ComputedLengthPercentage::from_points(50.0),
            width: css::ComputedLengthPercentage::from_points(25.0),
            height: css::ComputedLengthPercentage::from_points(50.0),
            radii: None,
        };

        let resolved = resolved_object_view_box(view_box, Some(LayoutSize::new(50.0, 100.0)));

        assert!(resolved.applies());
        assert_eq!(
            resolved.source_rect(),
            NormalizedObjectSourceRect::new(
                NormalizedObjectSourcePoint::new(0.5, 0.5),
                NormalizedObjectSourceSize::new(0.5, 0.5),
            )
        );
        assert_eq!(
            resolved.effective_natural_size(LayoutSize::new(50.0, 100.0)),
            LayoutSize::new(25.0, 50.0)
        );
    }

    #[test]
    fn empty_or_inapplicable_object_view_box_has_no_effect() {
        let empty = css::ObjectViewBox::Inset {
            top: css::ComputedLengthPercentage::from_points(50.0),
            right: css::ComputedLengthPercentage::ZERO,
            bottom: css::ComputedLengthPercentage::from_points(50.0),
            left: css::ComputedLengthPercentage::ZERO,
            radii: None,
        };
        let natural = LayoutSize::new(100.0, 100.0);

        for resolved in [
            resolved_object_view_box(empty.clone(), Some(natural)),
            resolved_object_view_box(empty, None),
        ] {
            assert!(!resolved.applies());
            assert_eq!(
                resolved.source_rect().size,
                NormalizedObjectSourceSize::new(1.0, 1.0)
            );
            assert_eq!(resolved.effective_natural_size(natural), natural);
        }
    }

    #[test]
    fn svg_without_both_natural_axes_ignores_object_view_box() {
        let asset = Rc::new(
            crate::svg::parse_svg_bytes(
                br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="100%" height="100%"/></svg>"#,
            )
            .expect("dimensionless SVG parses"),
        );
        let view_box = css::ObjectViewBox::Inset {
            top: css::ComputedLengthPercentage::from_points(50.0),
            right: css::ComputedLengthPercentage::ZERO,
            bottom: css::ComputedLengthPercentage::ZERO,
            left: css::ComputedLengthPercentage::ZERO,
            radii: None,
        };

        let resolved = resolved_object_view_box_for_svg(view_box, &asset);

        assert!(!resolved.applies());
        assert_eq!(
            resolved.source_rect().size,
            NormalizedObjectSourceSize::new(1.0, 1.0)
        );
    }

    #[test]
    fn border_image_percentage_slices_use_the_concrete_svg_viewport() {
        // A 150pt square border-image area establishes a 200 by 200 CSS-pixel
        // SVG viewport. Percentage slices must remain proportions of that
        // viewport, independently of the SVG's viewBox coordinates.
        // <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>
        let slices = used_border_image_slices(
            css::BorderImageSliceOffsets {
                top: css::BorderImageSliceValue::Percent(0.40),
                right: css::BorderImageSliceValue::Percent(0.30),
                bottom: css::BorderImageSliceValue::Percent(0.20),
                left: css::BorderImageSliceValue::Percent(0.10),
            },
            200.0,
            200.0,
        );

        assert!((slices.top - 80.0).abs() < 0.001);
        assert!((slices.right - 60.0).abs() < 0.001);
        assert!((slices.bottom - 40.0).abs() < 0.001);
        assert!((slices.left - 20.0).abs() < 0.001);
    }

    #[test]
    fn border_image_numeric_slices_keep_concrete_svg_coordinates() {
        let slices = used_border_image_slices(
            css::BorderImageSliceOffsets {
                top: css::BorderImageSliceValue::Number(40.0),
                right: css::BorderImageSliceValue::Number(30.0),
                bottom: css::BorderImageSliceValue::Number(20.0),
                left: css::BorderImageSliceValue::Number(10.0),
            },
            200.0,
            200.0,
        );

        assert_eq!(slices.top, 40.0);
        assert_eq!(slices.right, 30.0);
        assert_eq!(slices.bottom, 20.0);
        assert_eq!(slices.left, 10.0);
    }

    #[test]
    fn border_image_opposing_slices_overlap_without_rescaling() {
        let slices = used_border_image_slices(
            css::BorderImageSliceOffsets {
                top: css::BorderImageSliceValue::Number(80.0),
                right: css::BorderImageSliceValue::Number(90.0),
                bottom: css::BorderImageSliceValue::Number(70.0),
                left: css::BorderImageSliceValue::Number(60.0),
            },
            100.0,
            100.0,
        );

        // CSS makes the middle regions empty here; it does not normalize the
        // source edge slices to fit within the source image.
        // <https://drafts.csswg.org/css-backgrounds-3/#border-image-slice>
        assert_eq!(slices.top, 80.0);
        assert_eq!(slices.bottom, 70.0);
        assert_eq!(slices.left, 60.0);
        assert_eq!(slices.right, 90.0);
    }

    #[test]
    fn border_image_width_percentages_use_the_outset_area() {
        let mut style = ComputedStyle::initial();
        style.border_image.width = css::BorderImageWidth {
            top: css::BorderImageWidthValue::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(0.5),
            ),
            right: css::BorderImageWidthValue::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(0.5),
            ),
            bottom: css::BorderImageWidthValue::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(0.5),
            ),
            left: css::BorderImageWidthValue::LengthPercentage(
                css::ComputedLengthPercentage::from_percent(0.5),
            ),
        };

        let widths = used_border_image_widths(
            &style,
            css::Edges {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            },
            border_box_pt(75.0),
            border_box_pt(75.0),
            UsedBorderImageSlices {
                top: 1.0,
                right: 1.0,
                bottom: 1.0,
                left: 1.0,
            },
        );

        assert_eq!(
            widths,
            css::Edges {
                top: 37.5,
                right: 37.5,
                bottom: 37.5,
                left: 37.5,
            }
        );
    }

    #[test]
    fn numeric_border_image_outset_uses_computed_width_input() {
        let mut style = ComputedStyle::initial();
        style.border_image.outset = css::BorderImageOutset {
            top: css::BorderImageOutsetValue::Number(2.0),
            right: css::BorderImageOutsetValue::Number(2.0),
            bottom: css::BorderImageOutsetValue::Number(2.0),
            left: css::BorderImageOutsetValue::Number(2.0),
        };

        let outsets = used_border_image_outsets(
            &style,
            css::Edges {
                top: 5.0,
                right: 5.0,
                bottom: 5.0,
                left: 5.0,
            },
        );

        assert_eq!(
            outsets,
            css::Edges {
                top: 10.0,
                right: 10.0,
                bottom: 10.0,
                left: 10.0,
            }
        );
    }

    #[test]
    fn image_function_xywh_fragment_requires_nonzero_integer_geometry() {
        assert_eq!(
            parse_image_fragment("image.png#xywh=2,3,4,5"),
            Some(ParsedImageFragment::Xywh(ImagePixelRect {
                x: 2,
                y: 3,
                width: 4,
                height: 5,
            }))
        );
        for href in [
            "image.png#xywh=2,3,0,5",
            "image.png#xywh=2,3,4.0,5",
            "image.png#xywh=pixel:2,3,4,5",
            "image.png#xywh=2,3,4,5&foo=bar",
        ] {
            assert_eq!(parse_image_fragment(href), None, "{href}");
        }
    }

    #[test]
    fn image_function_xywh_crop_clamps_to_the_source_grid() {
        let image = DecodedPngImage::new(10, 8, vec![0; 10 * 8 * 3], None);
        let image = clamp_raster_fragment(
            image,
            ImagePixelRect {
                x: 8,
                y: 6,
                width: 8,
                height: 8,
            },
        )
        .unwrap();
        assert_eq!(
            image.source_rect,
            Some(RenderedImageSourceRect {
                x: 8,
                y: 6,
                width: 2,
                height: 2,
            })
        );
        assert_eq!(image.natural_size, crate::units::CssPixelSize::new(2, 2));
    }
}
