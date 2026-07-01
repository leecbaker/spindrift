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
/// CSS Images defines gradients as generated images. The renderer currently
/// supports URL images and an axis-aligned `linear-gradient()` subset:
/// <https://www.w3.org/TR/css-images-3/#gradients>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BackgroundImage {
    Url {
        src: String,
        base_url: Option<PathBuf>,
        root_url: Option<PathBuf>,
    },
    LinearGradient(LinearGradient),
}

impl BackgroundImage {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Self::LinearGradient(gradient) = self {
            gradient.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Self::LinearGradient(gradient) = self {
            gradient.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
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

/// Computed `linear-gradient()` image for the supported axis-aligned subset.
///
/// CSS Images Level 3 defines gradient lines and color stops. This stores stop
/// positions as typed CSS length-percentages because percentages resolve
/// against the concrete gradient line available only when painting:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinearGradient {
    pub direction: LinearGradientDirection,
    pub stops: Vec<GradientColorStop>,
}

impl LinearGradient {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        for stop in &mut self.stops {
            stop.resolve_font_metric_lengths(ch_advance);
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
    }
}

/// Axis direction for a supported linear gradient.
///
/// CSS Images uses `to <side-or-corner>` syntax for gradient direction:
/// <https://www.w3.org/TR/css-images-3/#typedef-side-or-corner>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinearGradientDirection {
    Bottom,
    Top,
    Right,
    Left,
}

/// One parsed gradient color stop.
///
/// CSS Images allows omitted and repeated stop positions; the current subset
/// requires explicit length-percentage positions, enough for hard-stop stripe
/// tests:
/// <https://www.w3.org/TR/css-images-3/#color-stop-syntax>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GradientColorStop {
    pub color: Color,
    pub position: ComputedLengthPercentage,
}

impl GradientColorStop {
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
