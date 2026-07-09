use super::*;
use crate::LayoutSize;
use crate::layout::assets::rasterize_generated_css_image;
use std::rc::Rc;

/// A URL-backed image that remains raster or vector through layout.
#[derive(Debug, Clone)]
pub(super) enum ResolvedImageAsset {
    Raster(DecodedPngImage),
    Svg(SharedSvgAsset),
}

impl ResolvedImageAsset {
    pub(super) fn intrinsic_size(&self) -> LayoutSize {
        match self {
            Self::Raster(image) => image.natural_layout_size(),
            Self::Svg(asset) => asset.intrinsic_size(),
        }
    }
}

pub(super) fn load_resolved_image_source(
    src: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    apply_orientation: bool,
) -> Option<ResolvedImageAsset> {
    load_resolved_image_source_with_request(
        src,
        base_url,
        root_url,
        resource_cache,
        apply_orientation,
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
    apply_orientation: bool,
    request_modifiers: &css::RequestUrlModifiers,
) -> Option<ResolvedImageAsset> {
    let (asset, svg_fragment) = if src.starts_with("data:") {
        (
            resource_cache.data_image_asset_with_orientation(src, apply_orientation)?,
            None,
        )
    } else {
        let url = crate::resource::resolve_url(src, base_url, root_url)?;
        if !resource_cache.allows_css_image_request(&url, root_url.or(base_url), request_modifiers)
        {
            return None;
        }
        let svg_fragment = url.fragment().map(str::to_owned);
        (
            resource_cache.image_asset_url_with_orientation(&url, apply_orientation)?,
            svg_fragment,
        )
    };
    Some(match asset {
        crate::resource::ResourceImageAsset::Raster { image_id, metadata } => {
            ResolvedImageAsset::Raster(DecodedPngImage {
                image_id: Some(image_id),
                pixel_width: metadata.pixel_width,
                pixel_height: metadata.pixel_height,
                rgb: resource_cache.image_placeholder_rgb(),
                alpha: None,
                color_space: crate::color::RasterColorSpace::SRGB,
            })
        }
        crate::resource::ResourceImageAsset::Svg(asset) => {
            ResolvedImageAsset::Svg(Rc::new(asset.with_view_fragment(svg_fragment.as_deref())))
        }
    })
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
        apply_orientation,
        &css::RequestUrlModifiers::default(),
    )
}

pub(super) fn load_image_source_with_request(
    src: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
    apply_orientation: bool,
    request_modifiers: &css::RequestUrlModifiers,
) -> Option<DecodedPngImage> {
    match load_resolved_image_source_with_request(
        src,
        base_url,
        root_url,
        resource_cache,
        apply_orientation,
        request_modifiers,
    )? {
        ResolvedImageAsset::Raster(image) => Some(image),
        ResolvedImageAsset::Svg(_) => None,
    }
}

/// Used CSS size and decoded pixels for an HTML image replaced element.
///
/// CSS Images defines raster images as replaced elements with intrinsic
/// dimensions, while CSS Sizing/Box Sizing define how `width`, `height`,
/// padding, and borders resolve to content-box and border-box sizes:
/// <https://www.w3.org/TR/css-images-3/#sizing> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(super) struct UsedImage {
    pub(super) decoded: DecodedPngImage,
    pub(super) svg: Option<SharedSvgAsset>,
    pub(super) content_size: ContentBoxSize,
    pub(super) border_box_size: BorderBoxSize,
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
    pub(super) width: f32,
    pub(super) height: f32,
    /// The source's preferred aspect ratio, when it has one. Default iframe
    /// dimensions deliberately do not establish this relationship.
    pub(super) preferred_aspect_ratio: Option<f32>,
    /// Whether the resource supplies either intrinsic dimension rather than
    /// only a preferred aspect ratio and CSS's default object size.
    pub(super) has_intrinsic_size: bool,
    pub(super) attr_width: Option<f32>,
    pub(super) attr_height: Option<f32>,
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
            .filter(|(width, height)| *width > 0.0 && *height > 0.0)
            .map(|(width, height)| width / height)
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
            content_size,
            border_box_size,
        }
    }

    fn from_geometry(decoded: DecodedPngImage, geometry: UsedReplacedBox) -> Self {
        Self {
            decoded,
            svg: None,
            content_size: geometry.content_size,
            border_box_size: geometry.border_box_size,
        }
    }

    fn with_svg(mut self, svg: Option<SharedSvgAsset>) -> Self {
        self.svg = svg;
        self
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
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) attr_width: Option<f32>,
    pub(super) attr_height: Option<f32>,
}

impl IntrinsicImageSize {
    pub(super) fn replaced_size(&self) -> IntrinsicReplacedSize {
        IntrinsicReplacedSize {
            width: self.width,
            height: self.height,
            preferred_aspect_ratio: (self.width > 0.0 && self.height > 0.0)
                .then_some(self.width / self.height),
            has_intrinsic_size: self.svg.as_ref().is_none_or(|asset| {
                let dimensions = asset.intrinsic_dimensions();
                dimensions.width.is_some() || dimensions.height.is_some()
            }),
            attr_width: self.attr_width,
            attr_height: self.attr_height,
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
        width: attr_width.unwrap_or(300.0 * css::CSS_PX_TO_PT),
        height: attr_height.unwrap_or(150.0 * css::CSS_PX_TO_PT),
        preferred_aspect_ratio: Some(
            attr_width.unwrap_or(300.0 * css::CSS_PX_TO_PT)
                / attr_height.unwrap_or(150.0 * css::CSS_PX_TO_PT),
        ),
        has_intrinsic_size: true,
        attr_width,
        attr_height,
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
    let mut size = intrinsic_default_replaced_size(element);
    size.preferred_aspect_ratio = None;
    size
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
        width,
        height,
        preferred_aspect_ratio: (width > 0.0 && height > 0.0).then_some(width / height),
        has_intrinsic_size: dimensions.width.is_some() || dimensions.height.is_some(),
        // Root SVG width/height attributes are presentation attributes for
        // the replaced box. Keep their authored absolute values separate
        // from the SVG document's natural dimensions so size containment can
        // suppress the latter without discarding an explicit axis.
        // <https://www.w3.org/TR/SVG2/embedded.html#placement>
        attr_width: element.attrs.get("width").and_then(|value| {
            parse_html_length(value).filter(|width| *width > 0.0 && !value.contains('%'))
        }),
        attr_height: element.attrs.get("height").and_then(|value| {
            parse_html_length(value).filter(|height| *height > 0.0 && !value.contains('%'))
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
            .or_else(|| element.attrs.get("src"))
            .map(|src| (src.as_str(), 1.0))?,
        "video" => element.attrs.get("poster").map(|src| (src.as_str(), 1.0))?,
        "img" => element
            .attrs
            .get("src")
            .map(|src| (src.as_str(), 1.0))
            .or_else(|| {
                element
                    .attrs
                    .get("srcset")
                    .and_then(|srcset| selected_srcset_candidate(srcset))
            })?,
        _ => element.attrs.get("src").map(|src| (src.as_str(), 1.0))?,
    };
    let asset = load_resolved_image_source(
        src,
        base_url,
        root_url,
        resource_cache,
        style.image_orientation == css::ImageOrientation::FromImage,
    )?;
    let intrinsic_size = match &asset {
        ResolvedImageAsset::Raster(image) => image.natural_layout_size(),
        ResolvedImageAsset::Svg(asset) => asset.replaced_intrinsic_size(),
    };
    let intrinsic_size =
        object_view_box_intrinsic_size(style.object_view_box.clone(), intrinsic_size)?;
    let width = intrinsic_size.width / intrinsic_resolution;
    let height = intrinsic_size.height / intrinsic_resolution;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let attr_width = element.attrs.get("width").and_then(|value| {
        parse_html_length(value).filter(|width| *width > 0.0 && !value.contains('%'))
    });
    let attr_height = element.attrs.get("height").and_then(|value| {
        parse_html_length(value).filter(|height| *height > 0.0 && !value.contains('%'))
    });
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
        width,
        height,
        attr_width,
        attr_height,
    })
}

/// Select the 1dppx `srcset` candidate used by this fixed-resolution renderer.
///
/// This accepts density descriptors and preserves source order for equal
/// densities, matching CSS Images candidate selection's tie-break rule.
fn selected_srcset_candidate(srcset: &str) -> Option<(&str, f32)> {
    let mut selected = None;
    for candidate in srcset.split(',') {
        let mut parts = candidate.split_ascii_whitespace();
        let src = parts.next()?;
        let density = parts
            .next()
            .and_then(|value| value.strip_suffix('x'))
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value > 0.0)
            .unwrap_or(1.0);
        selected = match selected {
            Some((_, best)) if best >= 1.0 && density >= 1.0 => selected,
            Some((_, best)) if density < 1.0 && density <= best => selected,
            _ => Some((src, density)),
        };
    }
    selected
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
        None if matches!(element.tag.as_str(), "img" | "video") => (
            transparent_replaced_fallback_pixel(),
            None,
            intrinsic_default_replaced_size(element),
        ),
        None => return None,
    };
    let geometry = used_replaced_box(intrinsic, style, available_width, height_basis);
    Some(UsedImage::from_geometry(decoded, geometry).with_svg(svg))
}

/// Geometry inputs for image sizing during intrinsic inline collection.
///
/// The physical available width constrains paint geometry independently from
/// the logical percentage basis, which may be indefinite when that width is
/// cyclic.
#[derive(Debug, Clone, Copy)]
pub(super) struct IntrinsicInlineImageSizingContext {
    pub(super) available_width: f32,
    pub(super) inline_percentage_basis: IntrinsicInlinePercentageBasis,
    pub(super) height_basis: BlockSizePercentageBasis,
}

/// Resolve an image while preserving whether the inline percentage basis is
/// definite. Intrinsic inline collection uses this alongside canvas so every
/// raster/SVG replaced image shares the same cyclic-percentage behavior.
/// <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
pub(super) fn used_image_with_inline_percentage_basis(
    element: &Element,
    style: &ComputedStyle,
    sizing: IntrinsicInlineImageSizingContext,
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
        None if matches!(element.tag.as_str(), "img" | "video") => (
            transparent_replaced_fallback_pixel(),
            None,
            intrinsic_default_replaced_size(element),
        ),
        None => return None,
    };
    let geometry = used_replaced_box_with_inline_percentage_basis(
        intrinsic,
        style,
        sizing.available_width,
        sizing.inline_percentage_basis,
        sizing.height_basis,
    );
    Some(UsedImage::from_geometry(decoded, geometry).with_svg(svg))
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
    available_width: f32,
    height_basis: BlockSizePercentageBasis,
) -> UsedReplacedBox {
    used_replaced_box_with_inline_percentage_basis(
        intrinsic,
        style,
        available_width,
        PercentageBasis::definite_from(
            content_box_pt(available_width.max(0.0)),
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
    available_width: f32,
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
                    PercentageBasis::definite(layout_pt(available_width.max(0.0))),
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
                    PercentageBasis::definite(layout_pt(available_width.max(0.0))),
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
    let available_content_width = (available_width - horizontal_non_content).max(0.0);
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
        (intrinsic.attr_width, intrinsic.attr_height)
    };
    // HTML dimension attributes contribute intrinsic dimensions, not definite
    // CSS preferred sizes. Constraints may therefore transfer through their
    // intrinsic aspect ratio when the corresponding CSS axis is automatic.
    // <https://html.spec.whatwg.org/multipage/rendering.html#attributes-for-embedded-content-and-images>
    // <https://www.w3.org/TR/css-sizing-3/#aspect-ratio>
    let width_is_auto = css_width.is_none();
    let height_is_auto = css_height.is_none();
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
        (Some(width_value), None, None) => (width_value, 0.0),
        (None, Some(height_value), None) => (0.0, height_value),
        (None, None, _) if style.contain.size => {
            (contained_intrinsic_width, contained_intrinsic_height)
        }
        (None, None, _) => (intrinsic.width, intrinsic.height),
        (Some(width_value), Some(height_value), _) => (width_value, height_value),
    };
    if let Some(aspect_ratio) = aspect_ratio {
        constrain_replaced_size_with_aspect_ratio(
            &mut content_width,
            &mut content_height,
            aspect_ratio,
            ReplacedAutoAxes {
                width: width_is_auto,
                height: height_is_auto,
            },
            ReplacedSizeConstraints {
                min_width: used_min_width(
                    style,
                    PercentageBasis::definite(layout_pt(available_width)),
                )
                .map(SemanticLengthExt::points),
                max_width: used_max_width(
                    style,
                    PercentageBasis::definite(layout_pt(available_width)),
                )
                .map(|width| width.points().min(available_content_width)),
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
                .map(|height| height.points().max(0.0)),
                max_height: used_length_percentage_or_auto_with_basis(
                    style.box_values.max_height.clone(),
                    height_basis,
                )
                .map(|height| height.points().max(0.0)),
            },
        );
    } else {
        content_width = constrain_content_width(
            style,
            content_box_pt(content_width),
            PercentageBasis::definite(layout_pt(available_width)),
        )
        .points();
        content_height = constrain_content_height(
            style,
            content_box_pt(content_height),
            PercentageBasis::definite(layout_pt(available_width)),
        )
        .points();
    }
    // A specified inline size on a replaced element is not shrink-to-fit.
    // In particular, an inline canvas with `width: 100px` in a narrower
    // multicolumn column keeps its used width and overflows the column just
    // like any other in-flow box.  The available measure only constrains the
    // automatic sizing path used while resolving an intrinsic contribution.
    // <https://www.w3.org/TR/CSS22/visudet.html#inline-replaced-width>
    if width_is_auto {
        content_width = content_width.min(available_content_width);
    }
    if width_is_auto
        && !height_is_auto
        && let Some(aspect_ratio) = aspect_ratio
    {
        content_height = content_width / aspect_ratio;
    }
    UsedReplacedBox::new(
        content_box_size_pt(content_width, content_height),
        non_content_pt(horizontal_non_content),
        non_content_pt(vertical_non_content),
    )
}

pub(super) fn used_generated_image(
    src: &str,
    style: &ComputedStyle,
    available_width: f32,
    intrinsic_resolution: f32,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
    resource_cache: &ResourceCache,
) -> Option<UsedImage> {
    let asset = load_resolved_image_source(src, base_url, root_url, resource_cache, true)?;
    let intrinsic_resolution = intrinsic_resolution.max(f32::MIN_POSITIVE);
    let raw_intrinsic_size = match &asset {
        ResolvedImageAsset::Raster(image) => image.natural_layout_size(),
        // Generated `content: url(...)` SVGs are still CSS replaced images.
        // Their layout fallback must therefore use SVG's replaced intrinsic
        // size, not the parser's concrete root viewport fallback.
        ResolvedImageAsset::Svg(asset) => asset.replaced_intrinsic_size(),
    };
    let raw_intrinsic_size =
        object_view_box_intrinsic_size(style.object_view_box.clone(), raw_intrinsic_size)?;
    let intrinsic_width = raw_intrinsic_size.width / intrinsic_resolution;
    let intrinsic_height = raw_intrinsic_size.height / intrinsic_resolution;
    if intrinsic_width <= 0.0 || intrinsic_height <= 0.0 {
        return None;
    }
    let natural_aspect_ratio = intrinsic_width / intrinsic_height;
    let aspect_ratio = style
        .aspect_ratio
        .preferred_ratio(true, Some(natural_aspect_ratio))?;
    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    let vertical_non_content =
        borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    let available_content_width = (available_width - horizontal_non_content).max(0.0);
    let width = used_content_box_width_or_auto(
        style,
        layout_pt(available_width),
        non_content_pt(horizontal_non_content),
    )
    .map(SemanticLengthExt::points);
    let height = definite_image_content_height_without_percent(style, vertical_non_content);
    let width_is_auto = width.is_none();
    let height_is_auto = height.is_none();
    let (mut content_width, mut content_height) = match (width, height) {
        (Some(width_value), None) => (width_value, width_value / aspect_ratio),
        (None, Some(height_value)) => (height_value * aspect_ratio, height_value),
        (None, None) => (intrinsic_width, intrinsic_height),
        (Some(width_value), Some(height_value)) => (width_value, height_value),
    };
    constrain_replaced_size_with_aspect_ratio(
        &mut content_width,
        &mut content_height,
        aspect_ratio,
        ReplacedAutoAxes {
            width: width_is_auto,
            height: height_is_auto,
        },
        ReplacedSizeConstraints {
            min_width: used_min_width(style, PercentageBasis::definite(layout_pt(available_width)))
                .map(SemanticLengthExt::points),
            max_width: used_max_width(style, PercentageBasis::definite(layout_pt(available_width)))
                .map(|width| width.points().min(available_content_width)),
            min_height: used_min_height(
                style,
                PercentageBasis::definite(layout_pt(available_width)),
            )
            .map(SemanticLengthExt::points),
            max_height: used_max_height(
                style,
                PercentageBasis::definite(layout_pt(available_width)),
            )
            .map(SemanticLengthExt::points),
        },
    );
    content_width = content_width.min(available_content_width);
    if width_is_auto && !height_is_auto {
        content_height = content_width / aspect_ratio;
    }
    let (decoded, svg) = match asset {
        ResolvedImageAsset::Raster(decoded) => (decoded, None),
        ResolvedImageAsset::Svg(svg) => (
            DecodedPngImage::new(1, 1, vec![0, 0, 0], Some(vec![0])),
            Some(svg),
        ),
    };
    Some(
        UsedImage::new(
            decoded,
            content_box_size_pt(content_width, content_height),
            non_content_pt(horizontal_non_content),
            non_content_pt(vertical_non_content),
        )
        .with_svg(svg),
    )
}

/// Resolve the effective natural size established by CSS Images Level 5's
/// `object-view-box`. The source crop is resolved before CSS replaced sizing;
/// paint later maps the same source rectangle into that effective object.
/// <https://drafts.csswg.org/css-images-5/#the-object-view-box-property>
fn object_view_box_intrinsic_size(
    view_box: css::ObjectViewBox,
    natural_size: LayoutSize,
) -> Option<LayoutSize> {
    if natural_size.width <= 0.0 || natural_size.height <= 0.0 {
        return None;
    }
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
    let size = match view_box {
        css::ObjectViewBox::None => natural_size,
        css::ObjectViewBox::Inset {
            top,
            right,
            bottom,
            left,
            ..
        } => LayoutSize::new(
            natural_size.width - resolve_x(left) - resolve_x(right),
            natural_size.height - resolve_y(top) - resolve_y(bottom),
        ),
        css::ObjectViewBox::Xywh { width, height, .. } => {
            LayoutSize::new(resolve_x(width), resolve_y(height))
        }
        css::ObjectViewBox::Rect {
            top,
            right,
            bottom,
            left,
        } => LayoutSize::new(
            resolve_x(right) - resolve_x(left),
            resolve_y(bottom) - resolve_y(top),
        ),
    };
    (size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0)
        .then_some(size)
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
    if let BackgroundImage::Url {
        src,
        base_url,
        root_url,
        ..
    } = image.selected_image()
    {
        return used_generated_image(
            src,
            style,
            available_width,
            intrinsic_resolution,
            base_url.as_ref().or(fallback_base_url),
            root_url.as_ref().or(fallback_root_url),
            resource_cache,
        );
    }

    let borders = used_border_widths(style);
    let horizontal_non_content =
        borders.left + borders.right + style.padding.left + style.padding.right;
    let vertical_non_content =
        borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    let available_content_width = (available_width - horizontal_non_content).max(0.0);
    let width = used_content_box_width_or_auto(
        style,
        layout_pt(available_width),
        non_content_pt(horizontal_non_content),
    )
    .map(SemanticLengthExt::points);
    let height = definite_image_content_height_without_percent(style, vertical_non_content);
    let width_is_auto = width.is_none();
    let height_is_auto = height.is_none();
    let default_size = style.font_size.max(1.0);
    let (mut content_width, mut content_height) = match (width, height) {
        (Some(width_value), None) => (width_value, width_value),
        (None, Some(height_value)) => (height_value, height_value),
        (None, None) => (default_size, default_size),
        (Some(width_value), Some(height_value)) => (width_value, height_value),
    };
    constrain_replaced_size_with_aspect_ratio(
        &mut content_width,
        &mut content_height,
        1.0,
        ReplacedAutoAxes {
            width: width_is_auto,
            height: height_is_auto,
        },
        ReplacedSizeConstraints {
            min_width: used_min_width(style, PercentageBasis::definite(layout_pt(available_width)))
                .map(SemanticLengthExt::points),
            max_width: used_max_width(style, PercentageBasis::definite(layout_pt(available_width)))
                .map(|width| width.points().min(available_content_width)),
            min_height: used_min_height(
                style,
                PercentageBasis::definite(layout_pt(available_width)),
            )
            .map(SemanticLengthExt::points),
            max_height: used_max_height(
                style,
                PercentageBasis::definite(layout_pt(available_width)),
            )
            .map(SemanticLengthExt::points),
        },
    );
    content_width = content_width.min(available_content_width);
    if width_is_auto && !height_is_auto {
        content_height = content_width;
    }
    let decoded = rasterize_generated_css_image(
        image,
        PaintSize::new(content_width, content_height),
        style.color,
        fallback_base_url,
        fallback_root_url,
        resource_cache,
    )?;
    Some(UsedImage::new(
        decoded,
        content_box_size_pt(content_width, content_height),
        non_content_pt(horizontal_non_content),
        non_content_pt(vertical_non_content),
    ))
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

#[derive(Debug, Clone, Copy)]
pub(super) struct ReplacedAutoAxes {
    pub(super) width: bool,
    pub(super) height: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ReplacedSizeConstraints {
    pub(super) min_width: Option<f32>,
    pub(super) max_width: Option<f32>,
    pub(super) min_height: Option<f32>,
    pub(super) max_height: Option<f32>,
}

/// Applies min/max constraints to a replaced element while preserving ratio when possible.
///
/// CSS Sizing defines preferred aspect ratio transfer for boxes with an
/// intrinsic ratio. When one axis is automatic, constraints in the other axis
/// can transfer through that ratio instead of distorting the replaced content:
/// <https://www.w3.org/TR/css-sizing-3/#aspect-ratio> and
/// <https://www.w3.org/TR/css-sizing-3/#min-size-properties>.
pub(super) fn constrain_replaced_size_with_aspect_ratio(
    width: &mut f32,
    height: &mut f32,
    aspect_ratio: f32,
    auto_axes: ReplacedAutoAxes,
    constraints: ReplacedSizeConstraints,
) {
    if aspect_ratio <= 0.0 {
        return;
    }
    if let Some(min_width) = constraints.min_width
        && *width < min_width
    {
        *width = min_width;
        if auto_axes.height {
            *height = *width / aspect_ratio;
        }
    }
    if let Some(max_width) = constraints.max_width
        && *width > max_width
    {
        *width = max_width;
        if auto_axes.height {
            *height = *width / aspect_ratio;
        }
    }
    if let Some(min_height) = constraints.min_height
        && *height < min_height
    {
        *height = min_height;
        if auto_axes.width {
            *width = *height * aspect_ratio;
        }
    }
    if let Some(max_height) = constraints.max_height
        && *height > max_height
    {
        *height = max_height;
        if auto_axes.width {
            *width = *height * aspect_ratio;
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
        available_width,
        height_basis,
    )
}

pub(super) fn used_canvas_with_inline_percentage_basis(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
    inline_percentage_basis: IntrinsicInlinePercentageBasis,
    height_basis: BlockSizePercentageBasis,
) -> UsedReplacedBox {
    used_replaced_box_with_inline_percentage_basis(
        intrinsic_canvas_size(element),
        style,
        available_width,
        inline_percentage_basis,
        height_basis,
    )
}

/// Resolve an inline SVG element through the same replaced-element geometry as
/// canvas and image resources.
pub(super) fn used_svg(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
    height_basis: BlockSizePercentageBasis,
) -> Option<UsedReplacedBox> {
    intrinsic_svg_size(element)
        .map(|intrinsic| used_replaced_box(intrinsic, style, available_width, height_basis))
}

pub(super) fn used_background_size(
    image: &DecodedPngImage,
    area_width: f32,
    area_height: f32,
    value: css::BackgroundSize,
    intrinsic_resolution: f32,
) -> PaintSize {
    let natural_size = image.natural_layout_size();
    let intrinsic_resolution = intrinsic_resolution.max(f32::MIN_POSITIVE);
    let intrinsic_width = natural_size.width / intrinsic_resolution;
    let intrinsic_height = natural_size.height / intrinsic_resolution;
    used_background_size_from_intrinsic_dimensions(
        area_width,
        area_height,
        value,
        BackgroundIntrinsicDimensions {
            width: intrinsic_width
                .is_finite()
                .then_some(intrinsic_width.max(0.0)),
            height: intrinsic_height
                .is_finite()
                .then_some(intrinsic_height.max(0.0)),
            aspect_ratio: (intrinsic_width > 0.0 && intrinsic_height > 0.0)
                .then_some(intrinsic_width / intrinsic_height),
        },
    )
}

/// CSS-facing intrinsic dimensions used by the background image sizing
/// algorithm.
///
/// Concrete image decoders may need fallback dimensions to render source
/// content, but `background-size` must distinguish those from actual
/// intrinsic dimensions. This is particularly important for SVG images with
/// omitted or percentage root dimensions.
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct BackgroundIntrinsicDimensions {
    pub(super) width: Option<f32>,
    pub(super) height: Option<f32>,
    pub(super) aspect_ratio: Option<f32>,
}

/// Resolve a background image's used size from its intrinsic dimensions.
///
/// This implements the CSS Backgrounds used-size algorithm, including the
/// special cases for images with one or no intrinsic dimensions and for images
/// that have an intrinsic ratio but no intrinsic size.
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>
/// <https://www.w3.org/TR/css-images-3/#default-sizing>
pub(super) fn used_background_size_from_intrinsic_dimensions(
    area_width: f32,
    area_height: f32,
    value: css::BackgroundSize,
    intrinsic: BackgroundIntrinsicDimensions,
) -> PaintSize {
    let area_width = area_width.max(0.0);
    let area_height = area_height.max(0.0);
    let ratio = intrinsic
        .aspect_ratio
        .filter(|ratio| ratio.is_finite() && *ratio > 0.0);
    let auto_size = || match (intrinsic.width, intrinsic.height, ratio) {
        (Some(width), Some(height), _) => PaintSize::new(width, height),
        (Some(width), None, Some(ratio)) => PaintSize::new(width, width / ratio),
        (None, Some(height), Some(ratio)) => PaintSize::new(height * ratio, height),
        (Some(width), None, None) => PaintSize::new(width, area_height),
        (None, Some(height), None) => PaintSize::new(area_width, height),
        (None, None, Some(ratio)) => background_size_contain(area_width, area_height, ratio),
        (None, None, None) => PaintSize::new(area_width, area_height),
    };

    match value {
        css::BackgroundSize::Auto => auto_size(),
        css::BackgroundSize::Cover => ratio
            .map(|ratio| background_size_cover(area_width, area_height, ratio))
            .unwrap_or_else(|| PaintSize::new(area_width, area_height)),
        css::BackgroundSize::Contain => ratio
            .map(|ratio| background_size_contain(area_width, area_height, ratio))
            .unwrap_or_else(|| PaintSize::new(area_width, area_height)),
        css::BackgroundSize::Explicit { width, height } => match (
            used_background_size_axis(width, area_width),
            used_background_size_axis(height, area_height),
        ) {
            (Some(width), Some(height)) => PaintSize::new(width, height),
            (Some(width), None) => ratio
                .map(|ratio| PaintSize::new(width, width / ratio))
                .unwrap_or_else(|| PaintSize::new(width, intrinsic.height.unwrap_or(area_height))),
            (None, Some(height)) => ratio
                .map(|ratio| PaintSize::new(height * ratio, height))
                .unwrap_or_else(|| PaintSize::new(intrinsic.width.unwrap_or(area_width), height)),
            (None, None) => auto_size(),
        },
    }
}

fn background_size_cover(area_width: f32, area_height: f32, ratio: f32) -> PaintSize {
    if area_width <= 0.0 && area_height <= 0.0 {
        return PaintSize::new(0.0, 0.0);
    }
    let scale = (area_width / ratio).max(area_height);
    PaintSize::new(scale * ratio, scale)
}

fn background_size_contain(area_width: f32, area_height: f32, ratio: f32) -> PaintSize {
    if area_width <= 0.0 || area_height <= 0.0 {
        return PaintSize::new(0.0, 0.0);
    }
    let scale = (area_width / ratio).min(area_height);
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
    area_width: f32,
    area_height: f32,
    image_width: f32,
    image_height: f32,
) -> (f64, f64) {
    // Keep the free space and its position calculation in f64. A valid SVG
    // `cover` size can be far larger than its background positioning area;
    // doing `area - image` in f32 would discard the area entirely and make a
    // tile that should cover the box appear not to intersect it.
    let free_x = f64::from(area_width) - f64::from(image_width);
    let free_y = f64::from(area_height) - f64::from(image_height);
    (
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

#[derive(Debug, Clone, Copy)]
pub(super) struct UsedBorderImageSlices {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

/// Resolve `border-image-slice` against the source image dimensions.
///
/// CSS Backgrounds and Borders resolves percentages against image dimensions
/// and proportionally reduces opposing slices when their sum exceeds the image
/// size:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>.
pub(super) fn used_border_image_slices(
    values: css::BorderImageSliceOffsets,
    image_width: u32,
    image_height: u32,
) -> UsedBorderImageSlices {
    let mut top = used_border_image_slice_value(values.top, image_height);
    let mut right = used_border_image_slice_value(values.right, image_width);
    let mut bottom = used_border_image_slice_value(values.bottom, image_height);
    let mut left = used_border_image_slice_value(values.left, image_width);
    reduce_opposing_slices(&mut top, &mut bottom, image_height);
    reduce_opposing_slices(&mut left, &mut right, image_width);
    UsedBorderImageSlices {
        top,
        right,
        bottom,
        left,
    }
}

fn used_border_image_slice_value(value: css::BorderImageSliceValue, reference: u32) -> u32 {
    let resolved = match value {
        css::BorderImageSliceValue::Number(value) => value,
        css::BorderImageSliceValue::Percent(value) => value * reference as f32,
    };
    resolved.max(0.0).round() as u32
}

fn reduce_opposing_slices(first: &mut u32, second: &mut u32, reference: u32) {
    let sum = first.saturating_add(*second);
    if sum <= reference || sum == 0 {
        return;
    }
    // A one-pixel raster source cannot represent the two half-pixel source
    // slices produced by the border-image process. Both slices nevertheless
    // sample that pixel; reducing either one to zero would incorrectly erase
    // three of the four corner images. Preserve the overlapping source sample
    // here, while destination geometry still uses the independently resolved
    // border-image widths.
    // <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>
    if reference == 1 && *first > 0 && *second > 0 {
        return;
    }
    let scale = reference as f32 / sum as f32;
    *first = ((*first as f32) * scale).round() as u32;
    *second = reference.saturating_sub(*first);
}

/// Resolve `border-image-width` to destination border-image side widths.
///
/// Numeric values multiply the corresponding used border width; lengths and
/// percentages resolve against the border image area dimensions. `auto` uses
/// the intrinsic size of the corresponding image slice, falling back to the
/// used border width only when that slice dimension is unavailable:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-width>.
pub(super) fn used_border_image_widths(
    style: &ComputedStyle,
    border_widths: css::Edges,
    border_box_width: f32,
    border_box_height: f32,
    slices: UsedBorderImageSlices,
) -> css::Edges {
    css::Edges {
        top: used_border_image_width_value(
            style.border_image.width.top.clone(),
            border_widths.top,
            border_box_height,
            slices.top,
        ),
        right: used_border_image_width_value(
            style.border_image.width.right.clone(),
            border_widths.right,
            border_box_width,
            slices.right,
        ),
        bottom: used_border_image_width_value(
            style.border_image.width.bottom.clone(),
            border_widths.bottom,
            border_box_height,
            slices.bottom,
        ),
        left: used_border_image_width_value(
            style.border_image.width.left.clone(),
            border_widths.left,
            border_box_width,
            slices.left,
        ),
    }
}

fn used_border_image_width_value(
    value: css::BorderImageWidthValue,
    border_width: f32,
    reference: f32,
    slice_width: u32,
) -> f32 {
    match value {
        css::BorderImageWidthValue::Auto => {
            if slice_width > 0 {
                // Border-image slice numbers are source CSS pixels. Convert
                // the intrinsic slice extent into Quire's PDF-point layout
                // space before using it as an `auto` border-image width.
                // <https://www.w3.org/TR/css-backgrounds-3/#border-image-width>
                slice_width as f32 * css::CSS_PX_TO_PT
            } else {
                border_width
            }
        }
        css::BorderImageWidthValue::Number(value) => border_width * value,
        css::BorderImageWidthValue::LengthPercentage(value) => {
            used_length_percentage(value, PercentageBasis::definite(layout_pt(reference))).points()
        }
    }
    .max(0.0)
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
/// Numeric values multiply the corresponding used border width; length values
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

pub(super) fn inline_replaced_descent(style: &ComputedStyle) -> LayoutLength {
    // CSS inline replaced elements align to the text baseline by default.
    // Reserve a conservative descender area below the image until line layout
    // has real font ascent/descent metrics.
    layout_pt((style.line_height * 0.25).max(0.0))
}

pub(super) fn svg_rect(element: &Element) -> Option<(f32, f32, Color)> {
    crate::svg::svg_intrinsic_size(element)
        .map(|size| (size.width, size.height, Color::TRANSPARENT))
}

pub(super) fn estimate_svg_height(
    element: &Element,
    style: &ComputedStyle,
    available_width: f32,
) -> f32 {
    let height = used_svg(
        element,
        style,
        available_width,
        BlockSizePercentageBasis::indefinite(),
    )
    .map(|svg| svg.border_box_size.height)
    .unwrap_or(style.line_height);
    style.margin.top + height + style.margin.bottom
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
    use crate::units::{border_box_size_pt, border_box_to_content_box_size};
    use std::rc::Rc;

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

    #[test]
    fn percent_encoded_svg_data_url_resolves_as_a_vector_asset() {
        let cache = ResourceCache::default();
        let source = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2220%22%20height%3D%2210%22%3E%3Cpath%20d%3D%22M0%200H20V10H0Z%22%20fill%3D%22red%22%2F%3E%3C%2Fsvg%3E";
        let asset = load_resolved_image_source(source, None, None, &cache, true).unwrap();
        let ResolvedImageAsset::Svg(asset) = asset else {
            panic!("expected SVG asset");
        };
        assert_eq!(asset.intrinsic_size(), LayoutSize::new(15.0, 7.5));
        assert_eq!(
            asset
                .paint_paths(crate::PaintRect::new(
                    crate::PaintPoint::new(0.0, 0.0),
                    crate::PaintSize::new(15.0, 7.5),
                ))
                .len(),
            1
        );
    }

    #[test]
    fn base64_svg_data_url_resolves_as_a_vector_asset() {
        let cache = ResourceCache::default();
        let source = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyMCIgaGVpZ2h0PSIxMCI+PHBhdGggZD0iTTAgMEgyMFYxMEgwWiIgZmlsbD0icmVkIi8+PC9zdmc+";
        let asset = load_resolved_image_source(source, None, None, &cache, true).unwrap();

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
            cache.image_asset_url_with_orientation(&url, true),
            Some(crate::resource::ResourceImageAsset::Svg(_))
        ));
    }

    #[test]
    fn repeated_data_url_images_share_layout_placeholder_storage() {
        let cache = ResourceCache::default();
        let source = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

        let first = load_image_source(source, None, None, &cache, true).unwrap();
        let second = load_image_source(source, None, None, &cache, true).unwrap();

        assert!(Rc::ptr_eq(&first.rgb, &second.rgb));
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
        assert_eq!(default_canvas.width, 225.0);
        assert_eq!(default_canvas.height, 112.5);
        assert_eq!(default_canvas.attr_width, None);
        assert_eq!(default_canvas.attr_height, None);

        let attributed_canvas =
            intrinsic_canvas_size(&canvas_element(&[("width", "96"), ("height", "48px")]));
        assert_eq!(attributed_canvas.width, 72.0);
        assert_eq!(attributed_canvas.height, 36.0);
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
                width: 150.0,
                height: 75.0,
                preferred_aspect_ratio: Some(2.0),
                has_intrinsic_size: true,
                attr_width: Some(150.0),
                attr_height: Some(75.0),
            },
            &style,
            500.0,
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
                width: 150.0,
                height: 75.0,
                preferred_aspect_ratio: Some(2.0),
                has_intrinsic_size: true,
                attr_width: None,
                attr_height: None,
            },
            &style,
            500.0,
            BlockSizePercentageBasis::indefinite(),
        );

        assert_eq!(geometry.content_size.width, 180.0);
        assert_eq!(geometry.content_size.height, 90.0);
    }

    #[test]
    fn effective_zoom_scales_replaced_dimensions_but_not_aspect_ratio() {
        let intrinsic = IntrinsicReplacedSize {
            width: 100.0,
            height: 50.0,
            preferred_aspect_ratio: Some(2.0),
            has_intrinsic_size: true,
            attr_width: Some(100.0),
            attr_height: Some(50.0),
        }
        .scaled_by_effective_zoom(2.0);

        assert_eq!(intrinsic.width, 200.0);
        assert_eq!(intrinsic.height, 100.0);
        assert_eq!(intrinsic.attr_width, Some(200.0));
        assert_eq!(intrinsic.attr_height, Some(100.0));
        assert_eq!(intrinsic.natural_aspect_ratio(), Some(2.0));
    }

    #[test]
    fn replaced_percentage_max_height_uses_the_block_size_basis() {
        let mut style = ComputedStyle::initial();
        style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(1.0),
        );
        let intrinsic = IntrinsicReplacedSize {
            width: 200.0,
            height: 200.0,
            preferred_aspect_ratio: Some(1.0),
            has_intrinsic_size: true,
            attr_width: None,
            attr_height: None,
        };

        let definite = used_replaced_box(
            intrinsic,
            &style,
            500.0,
            PercentageBasis::definite_from(
                content_box_pt(100.0),
                BlockSizeBasisSource::ContainingBlock,
            ),
        );
        let indefinite = used_replaced_box(intrinsic, &style, 500.0, PercentageBasis::indefinite());

        assert_eq!(definite.content_size.width, 100.0);
        assert_eq!(definite.content_size.height, 100.0);
        assert_eq!(indefinite.content_size.width, 200.0);
        assert_eq!(indefinite.content_size.height, 200.0);
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

        assert_eq!(image.decoded.pixel_width, 1);
        assert_eq!(image.decoded.pixel_height, 1);
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
            300.0,
            150.0,
            css::BackgroundSize::Auto,
            BackgroundIntrinsicDimensions {
                width: None,
                height: None,
                aspect_ratio: Some(1.0 / 4.0),
            },
        );

        assert_eq!(size, PaintSize::new(37.5, 150.0));
    }

    #[test]
    fn background_size_fills_area_for_images_without_intrinsic_geometry() {
        let intrinsic = BackgroundIntrinsicDimensions {
            width: None,
            height: None,
            aspect_ratio: None,
        };

        assert_eq!(
            used_background_size_from_intrinsic_dimensions(
                300.0,
                150.0,
                css::BackgroundSize::Auto,
                intrinsic,
            ),
            PaintSize::new(300.0, 150.0)
        );
        assert_eq!(
            used_background_size_from_intrinsic_dimensions(
                300.0,
                150.0,
                css::BackgroundSize::Contain,
                intrinsic,
            ),
            PaintSize::new(300.0, 150.0)
        );
    }

    #[test]
    fn background_size_uses_positioning_area_for_auto_axis_without_ratio() {
        let size = used_background_size_from_intrinsic_dimensions(
            300.0,
            150.0,
            css::BackgroundSize::Explicit {
                width: css::BackgroundSizeAxis::LengthPercentage(
                    css::ComputedLengthPercentage::from_points(50.0),
                ),
                height: css::BackgroundSizeAxis::Auto,
            },
            BackgroundIntrinsicDimensions {
                width: None,
                height: None,
                aspect_ratio: None,
            },
        );

        assert_eq!(size, PaintSize::new(50.0, 150.0));
    }

    #[test]
    fn background_size_cover_uses_the_nonzero_positioning_axis() {
        assert_eq!(
            background_size_cover(100.0, 0.0, 2.0),
            PaintSize::new(100.0, 50.0)
        );
        assert_eq!(
            background_size_cover(0.0, 100.0, 2.0),
            PaintSize::new(200.0, 100.0)
        );
        assert_eq!(
            background_size_cover(0.0, 0.0, 2.0),
            PaintSize::new(0.0, 0.0)
        );
    }
}
