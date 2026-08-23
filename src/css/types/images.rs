use super::*;

/// Computed CSS Images view box for a replaced object. The rectangle remains
/// source-relative until it is resolved against an image's natural size.
/// <https://drafts.csswg.org/css-images-5/#the-object-view-box-property>
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ObjectViewBox {
    None,
    Inset {
        top: ComputedLengthPercentage,
        right: ComputedLengthPercentage,
        bottom: ComputedLengthPercentage,
        left: ComputedLengthPercentage,
        radii: Option<BorderRadius>,
    },
    Xywh {
        x: ComputedLengthPercentage,
        y: ComputedLengthPercentage,
        width: ComputedLengthPercentage,
        height: ComputedLengthPercentage,
        radii: Option<BorderRadius>,
    },
    Rect {
        top: ComputedLengthPercentage,
        right: ComputedLengthPercentage,
        bottom: ComputedLengthPercentage,
        left: ComputedLengthPercentage,
    },
}

impl ObjectViewBox {
    pub(crate) const NONE: Self = Self::None;

    pub(crate) fn requires_ch_advance(&self) -> bool {
        let requires = |value: &ComputedLengthPercentage| value.requires_ch_advance();
        match self {
            Self::None => false,
            Self::Inset {
                top,
                right,
                bottom,
                left,
                radii,
            } => {
                requires(top)
                    || requires(right)
                    || requires(bottom)
                    || requires(left)
                    || radii
                        .as_ref()
                        .is_some_and(BorderRadius::requires_ch_advance)
            }
            Self::Rect {
                top,
                right,
                bottom,
                left,
            } => requires(top) || requires(right) || requires(bottom) || requires(left),
            Self::Xywh {
                x,
                y,
                width,
                height,
                radii,
            } => {
                requires(x)
                    || requires(y)
                    || requires(width)
                    || requires(height)
                    || radii
                        .as_ref()
                        .is_some_and(BorderRadius::requires_ch_advance)
            }
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        let requires = |value: &ComputedLengthPercentage| value.requires_root_font_metrics();
        match self {
            Self::None => false,
            Self::Inset {
                top,
                right,
                bottom,
                left,
                radii,
            } => {
                requires(top)
                    || requires(right)
                    || requires(bottom)
                    || requires(left)
                    || radii
                        .as_ref()
                        .is_some_and(BorderRadius::requires_root_font_metrics)
            }
            Self::Rect {
                top,
                right,
                bottom,
                left,
            } => requires(top) || requires(right) || requires(bottom) || requires(left),
            Self::Xywh {
                x,
                y,
                width,
                height,
                radii,
            } => {
                requires(x)
                    || requires(y)
                    || requires(width)
                    || requires(height)
                    || radii
                        .as_ref()
                        .is_some_and(BorderRadius::requires_root_font_metrics)
            }
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        let resolve =
            |value: &mut ComputedLengthPercentage| value.resolve_font_metric_lengths(ch_advance);
        match self {
            Self::None => {}
            Self::Inset {
                top,
                right,
                bottom,
                left,
                radii,
            } => {
                resolve(top);
                resolve(right);
                resolve(bottom);
                resolve(left);
                if let Some(radii) = radii {
                    radii.resolve_font_metric_lengths(ch_advance);
                }
            }
            Self::Rect {
                top,
                right,
                bottom,
                left,
            } => {
                resolve(top);
                resolve(right);
                resolve(bottom);
                resolve(left);
            }
            Self::Xywh {
                x,
                y,
                width,
                height,
                radii,
            } => {
                resolve(x);
                resolve(y);
                resolve(width);
                resolve(height);
                if let Some(radii) = radii {
                    radii.resolve_font_metric_lengths(ch_advance);
                }
            }
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        let resolve =
            |value: &mut ComputedLengthPercentage| value.resolve_root_font_metric_lengths(basis);
        match self {
            Self::None => {}
            Self::Inset {
                top,
                right,
                bottom,
                left,
                radii,
            } => {
                resolve(top);
                resolve(right);
                resolve(bottom);
                resolve(left);
                if let Some(radii) = radii {
                    radii.resolve_root_font_metric_lengths(basis);
                }
            }
            Self::Rect {
                top,
                right,
                bottom,
                left,
            } => {
                resolve(top);
                resolve(right);
                resolve(bottom);
                resolve(left);
            }
            Self::Xywh {
                x,
                y,
                width,
                height,
                radii,
            } => {
                resolve(x);
                resolve(y);
                resolve(width);
                resolve(height);
                if let Some(radii) = radii {
                    radii.resolve_root_font_metric_lengths(basis);
                }
            }
        }
    }
}

impl ResolveViewportLengths for ObjectViewBox {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        let resolve = |value: &mut ComputedLengthPercentage| value.resolve_viewport_lengths(basis);
        match self {
            Self::None => {}
            Self::Inset {
                top,
                right,
                bottom,
                left,
                radii,
            } => {
                resolve(top);
                resolve(right);
                resolve(bottom);
                resolve(left);
                if let Some(radii) = radii {
                    radii.resolve_viewport_lengths(basis);
                }
            }
            Self::Rect {
                top,
                right,
                bottom,
                left,
            } => {
                resolve(top);
                resolve(right);
                resolve(bottom);
                resolve(left);
            }
            Self::Xywh {
                x,
                y,
                width,
                height,
                radii,
            } => {
                resolve(x);
                resolve(y);
                resolve(width);
                resolve(height);
                if let Some(radii) = radii {
                    radii.resolve_viewport_lengths(basis);
                }
            }
        }
    }
}

/// Computed [`object-fit`](https://www.w3.org/TR/css-images-3/#the-object-fit)
/// value for replaced elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ObjectFit {
    #[default]
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

pub(crate) fn parse_object_fit(value: &str) -> Option<ObjectFit> {
    match value.trim().to_ascii_lowercase().as_str() {
        "fill" => Some(ObjectFit::Fill),
        "contain" => Some(ObjectFit::Contain),
        "cover" => Some(ObjectFit::Cover),
        "none" => Some(ObjectFit::None),
        "scale-down" => Some(ObjectFit::ScaleDown),
        _ => None,
    }
}

/// Computed CSS Images metadata-orientation policy for raster image sources.
/// <https://drafts.csswg.org/css-images-3/#propdef-image-orientation>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ImageOrientation {
    #[default]
    FromImage,
    None,
}

pub(crate) fn parse_image_orientation(value: &str) -> Option<ImageOrientation> {
    match value.trim().to_ascii_lowercase().as_str() {
        "from-image" => Some(ImageOrientation::FromImage),
        "none" => Some(ImageOrientation::None),
        _ => None,
    }
}

/// Sampling policy for raster CSS images.
/// <https://drafts.csswg.org/css-images-4/#propdef-image-rendering>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ImageRendering {
    #[default]
    Auto,
    Smooth,
    HighQuality,
    Pixelated,
    CrispEdges,
}

pub(crate) fn parse_image_rendering(value: &str) -> Option<ImageRendering> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(ImageRendering::Auto),
        "smooth" => Some(ImageRendering::Smooth),
        "high-quality" => Some(ImageRendering::HighQuality),
        "pixelated" => Some(ImageRendering::Pixelated),
        "crisp-edges" => Some(ImageRendering::CrispEdges),
        // CSS Images retains these legacy spellings as required aliases.
        // <https://drafts.csswg.org/css-images-3/#the-image-rendering>
        "optimizespeed" => Some(ImageRendering::CrispEdges),
        "optimizequality" => Some(ImageRendering::Smooth),
        _ => None,
    }
}
