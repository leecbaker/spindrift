use super::*;

/// Computed CSS value for one background-position axis.
///
/// CSS Backgrounds and Borders defines each axis as an origin keyword plus a
/// length-percentage offset:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-position>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BackgroundPositionAxis {
    pub origin: BackgroundPositionOrigin,
    pub offset: ComputedLengthPercentage,
}

impl BackgroundPositionAxis {
    pub(crate) const LEFT: Self = Self {
        origin: BackgroundPositionOrigin::Start,
        offset: ComputedLengthPercentage::ZERO,
    };
    pub(crate) const TOP: Self = Self {
        origin: BackgroundPositionOrigin::Start,
        offset: ComputedLengthPercentage::ZERO,
    };

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.offset.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.offset.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }
}

/// Origin side for a computed background-position axis.
///
/// CSS Backgrounds and Borders resolves offsets from the start side, center,
/// or end side of the positioning area:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-position>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundPositionOrigin {
    Start,
    Center,
    End,
}

/// Computed CSS `background-position` value for a single background layer.
///
/// CSS Backgrounds and Borders permits a list of layers; this renderer
/// currently stores one layer because painting supports one background image:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-position>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BackgroundPosition {
    pub x: BackgroundPositionAxis,
    pub y: BackgroundPositionAxis,
}

impl BackgroundPosition {
    pub(crate) const INITIAL: Self = Self {
        x: BackgroundPositionAxis::LEFT,
        y: BackgroundPositionAxis::TOP,
    };

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.x.resolve_font_metric_lengths(ch_advance);
        self.y.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.x.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.y.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }
}

/// Computed CSS value for one background-size axis.
///
/// CSS Backgrounds and Borders defines each explicit `background-size` axis as
/// `auto | <length-percentage>`:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BackgroundSizeAxis {
    Auto,
    LengthPercentage(ComputedLengthPercentage),
}

/// Computed CSS `background-size` value for a single background layer.
///
/// CSS Backgrounds and Borders defines `auto`, `cover`, `contain`, and
/// explicit one/two-axis sizing:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BackgroundSize {
    Auto,
    Cover,
    Contain,
    Explicit {
        width: BackgroundSizeAxis,
        height: BackgroundSizeAxis,
    },
}

impl BackgroundSize {
    pub(crate) const AUTO: Self = Self::Auto;

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        let Self::Explicit { width, height } = self else {
            return;
        };
        width.resolve_font_metric_lengths(ch_advance);
        height.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        let Self::Explicit { width, height } = self else {
            return;
        };
        width.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        height.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }
}

impl BackgroundSizeAxis {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
    }
}

/// Box edge used by background-origin and background-clip.
///
/// CSS Backgrounds and Borders defines these keywords as selecting the
/// border, padding, or content box for background positioning and painting:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin> and
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-clip>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundBox {
    Border,
    Padding,
    Content,
}

/// Computed single-layer CSS background image.
///
/// CSS Images defines gradients as generated images. The renderer supports URL
/// images and CSS Images Level 3 linear and radial gradients:
/// <https://www.w3.org/TR/css-images-3/#gradients>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BackgroundImage {
    Url {
        src: String,
        base_url: Option<PathBuf>,
        root_url: Option<PathBuf>,
    },
    LinearGradient(LinearGradient),
    RadialGradient(RadialGradient),
}

impl BackgroundImage {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        match self {
            Self::LinearGradient(gradient) => gradient.resolve_font_metric_lengths(ch_advance),
            Self::RadialGradient(gradient) => gradient.resolve_font_metric_lengths(ch_advance),
            Self::Url { .. } => {}
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        match self {
            Self::LinearGradient(gradient) => gradient.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            ),
            Self::RadialGradient(gradient) => gradient.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            ),
            Self::Url { .. } => {}
        }
    }
}

/// Computed values for one CSS background layer.
///
/// CSS Backgrounds and Borders defines every `background-*` longhand except
/// `background-color` as a comma-separated layer list. Shorter lists repeat to
/// match the image layer count:
/// <https://www.w3.org/TR/css-backgrounds-3/#layering>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BackgroundLayer {
    pub image: Option<BackgroundImage>,
    pub position: BackgroundPosition,
    pub size: BackgroundSize,
    pub repeat: BackgroundRepeat,
    pub origin: BackgroundBox,
    pub clip: BackgroundBox,
}

impl BackgroundLayer {
    pub(crate) const fn initial() -> Self {
        Self {
            image: None,
            position: BackgroundPosition::INITIAL,
            size: BackgroundSize::AUTO,
            repeat: BackgroundRepeat::Repeat,
            origin: BackgroundBox::Padding,
            clip: BackgroundBox::Border,
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Some(image) = &mut self.image {
            image.resolve_font_metric_lengths(ch_advance);
        }
        self.size.resolve_font_metric_lengths(ch_advance);
        self.position.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Some(image) = &mut self.image {
            image.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
        self.size.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.position.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }
}

/// Computed `linear-gradient()` or `repeating-linear-gradient()` image.
///
/// CSS Images Level 3 defines the gradient direction, color-stop list, and
/// repeating behavior. Stop positions remain typed length-percentages because
/// percentages resolve against the concrete gradient line only when painting:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinearGradient {
    pub direction: LinearGradientDirection,
    pub repeating: bool,
    pub stops: Vec<GradientColorStop>,
    pub hints: Vec<GradientColorHint>,
}

/// Computed `radial-gradient()` or `repeating-radial-gradient()` image.
///
/// CSS Images Level 3 defines radial gradients by a shape, ending size,
/// center position, color stops, and optional repeating behavior:
/// <https://www.w3.org/TR/css-images-3/#radial-gradients>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RadialGradient {
    pub shape: RadialGradientShape,
    pub size: RadialGradientSize,
    pub position: BackgroundPosition,
    pub repeating: bool,
    pub stops: Vec<GradientColorStop>,
    pub hints: Vec<GradientColorHint>,
}

impl RadialGradient {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.size.resolve_font_metric_lengths(ch_advance);
        self.position.resolve_font_metric_lengths(ch_advance);
        for stop in &mut self.stops {
            stop.resolve_font_metric_lengths(ch_advance);
        }
        for hint in &mut self.hints {
            hint.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.size.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.position.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        for stop in &mut self.stops {
            stop.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
        for hint in &mut self.hints {
            hint.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RadialGradientShape {
    Circle,
    Ellipse,
}

/// Computed radial-gradient ending size.
///
/// Extent keywords are resolved only when the concrete gradient box and center
/// point are known. Explicit radii stay as length-percentages so percentages
/// can resolve against the concrete gradient box at paint time:
/// <https://www.w3.org/TR/css-images-3/#typedef-radial-size>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RadialGradientSize {
    Extent(RadialGradientExtent),
    CircleRadius(ComputedLengthPercentage),
    EllipseRadii {
        x: ComputedLengthPercentage,
        y: ComputedLengthPercentage,
    },
}

impl RadialGradientSize {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        match self {
            Self::CircleRadius(radius) => radius.resolve_font_metric_lengths(ch_advance),
            Self::EllipseRadii { x, y } => {
                x.resolve_font_metric_lengths(ch_advance);
                y.resolve_font_metric_lengths(ch_advance);
            }
            Self::Extent(_) => {}
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        match self {
            Self::CircleRadius(radius) => radius.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            ),
            Self::EllipseRadii { x, y } => {
                x.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
                y.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
            }
            Self::Extent(_) => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RadialGradientExtent {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
}

impl LinearGradient {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        for stop in &mut self.stops {
            stop.resolve_font_metric_lengths(ch_advance);
        }
        for hint in &mut self.hints {
            hint.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        for stop in &mut self.stops {
            stop.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
        for hint in &mut self.hints {
            hint.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
    }
}

/// Normalized linear-gradient direction.
///
/// CSS Images Level 3 maps side keywords to angles, but corner directions
/// depend on the concrete gradient box dimensions and must be kept distinct
/// until paint-time gradient-line construction:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LinearGradientDirection {
    Angle(f32),
    Corner {
        horizontal: GradientHorizontalDirection,
        vertical: GradientVerticalDirection,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GradientHorizontalDirection {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GradientVerticalDirection {
    Top,
    Bottom,
}

/// One parsed gradient color stop.
///
/// CSS Images allows omitted stop positions and two-position stops. The parser
/// expands two-position stops into two adjacent stops and keeps omitted
/// positions as `None` until the CSS Images fixup algorithm runs with the
/// concrete gradient line:
/// <https://www.w3.org/TR/css-images-3/#color-stop-syntax>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GradientColorStop {
    pub color: Color,
    pub position: Option<ComputedLengthPercentage>,
}

impl GradientColorStop {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Some(position) = &mut self.position {
            position.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Some(position) = &mut self.position {
            position.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
    }
}

/// One color interpolation hint between two adjacent color stops.
///
/// CSS Images Level 3 permits an unlabeled `<linear-color-hint>` between two
/// color stops to move the midpoint of interpolation:
/// <https://www.w3.org/TR/css-images-3/#color-stop-syntax>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GradientColorHint {
    pub after_stop: usize,
    pub position: ComputedLengthPercentage,
}

impl GradientColorHint {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.position.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.position.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }
}
