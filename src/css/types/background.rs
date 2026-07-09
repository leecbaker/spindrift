use super::*;

/// Computed CSS value for one background-position axis.
///
/// CSS Backgrounds and Borders defines each axis as an origin keyword plus a
/// length-percentage offset:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-position>.
#[derive(Debug, Clone, PartialEq)]
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

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.offset.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        self.offset.resolve_em_relative_lengths(font_size);
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        self.offset
            .resolve_root_font_relative_lengths(root_font_size);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.offset.requires_ch_advance()
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        self.offset
            .resolve_line_height_relative_lengths(line_height);
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BackgroundPosition {
    pub x: BackgroundPositionAxis,
    pub y: BackgroundPositionAxis,
}

impl BackgroundPosition {
    pub(crate) const INITIAL: Self = Self {
        x: BackgroundPositionAxis::LEFT,
        y: BackgroundPositionAxis::TOP,
    };

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.x.resolve_font_metric_lengths(ch_advance);
        self.y.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        self.x.resolve_em_relative_lengths(font_size);
        self.y.resolve_em_relative_lengths(font_size);
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        self.x.resolve_root_font_relative_lengths(root_font_size);
        self.y.resolve_root_font_relative_lengths(root_font_size);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.x.requires_ch_advance() || self.y.requires_ch_advance()
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        self.x.resolve_line_height_relative_lengths(line_height);
        self.y.resolve_line_height_relative_lengths(line_height);
    }
}

/// Computed CSS value for one background-size axis.
///
/// CSS Backgrounds and Borders defines each explicit `background-size` axis as
/// `auto | <length-percentage>`:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BackgroundSizeAxis {
    Auto,
    LengthPercentage(ComputedLengthPercentage),
}

/// Computed CSS `background-size` value for a single background layer.
///
/// CSS Backgrounds and Borders defines `auto`, `cover`, `contain`, and
/// explicit one/two-axis sizing:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>.
#[derive(Debug, Clone, PartialEq)]
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

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        let Self::Explicit { width, height } = self else {
            return;
        };
        width.resolve_font_metric_lengths(ch_advance);
        height.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        let Self::Explicit { width, height } = self else {
            return;
        };
        width.resolve_em_relative_lengths(font_size);
        height.resolve_em_relative_lengths(font_size);
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        let Self::Explicit { width, height } = self else {
            return;
        };
        width.resolve_root_font_relative_lengths(root_font_size);
        height.resolve_root_font_relative_lengths(root_font_size);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        match self {
            Self::Explicit { width, height } => {
                width.requires_ch_advance() || height.requires_ch_advance()
            }
            Self::Auto | Self::Cover | Self::Contain => false,
        }
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        let Self::Explicit { width, height } = self else {
            return;
        };
        width.resolve_line_height_relative_lengths(line_height);
        height.resolve_line_height_relative_lengths(line_height);
    }
}

impl BackgroundSizeAxis {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_em_relative_lengths(font_size);
        }
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_relative_lengths(root_font_size);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_line_height_relative_lengths(line_height);
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
    /// The painted border ring, selected only by `background-clip`.
    ///
    /// CSS Backgrounds Level 4 defines `border-area` as the area occupied by
    /// the border, independent of the border color's transparency.
    /// <https://drafts.csswg.org/css-backgrounds-4/#background-clip>
    BorderArea,
    Padding,
    Content,
}

/// Coordinate system to which one background layer is attached.
///
/// CSS Backgrounds Level 3 resolves `scroll`, `fixed`, and `local` per layer;
/// the initial value is `scroll`.
/// <https://www.w3.org/TR/css-backgrounds-3/#background-attachment>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundAttachment {
    Scroll,
    Fixed,
    Local,
}

/// Computed single-layer CSS background image.
///
/// CSS Images defines gradients as generated images. The renderer supports URL
/// images and CSS Images Level 3 linear and radial gradients:
/// <https://www.w3.org/TR/css-images-3/#gradients>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BackgroundImage {
    /// The image selected from an `image-set()` together with the candidate's
    /// resolution. CSS Images uses that resolution to scale the selected
    /// image's intrinsic dimensions.
    /// <https://drafts.csswg.org/css-images-4/#image-set-notation>
    ImageSet {
        image: Box<BackgroundImage>,
        resolution: f32,
    },
    Url {
        src: String,
        base_url: Option<url::Url>,
        root_url: Option<url::Url>,
        request_modifiers: crate::css::RequestUrlModifiers,
    },
    LinearGradient(LinearGradient),
    RadialGradient(RadialGradient),
    ConicGradient(ConicGradient),
    Color(ColorImageColor),
}

/// The color argument to CSS Images Level 4's `image()` function.
///
/// `currentcolor` remains symbolic until the generated image is used, because
/// its value is the element's computed `color`, not a parser-global default.
/// <https://drafts.csswg.org/css-color-4/#currentcolor-color>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ColorImageColor {
    Color(Color),
    CurrentColor,
}

impl ColorImageColor {
    pub(crate) fn resolve(self, current_color: Color) -> Color {
        match self {
            Self::Color(color) => color,
            Self::CurrentColor => current_color,
        }
    }
}

impl BackgroundImage {
    /// Return the selected image after unwrapping any nested `image-set()`
    /// candidates.
    pub(crate) fn selected_image(&self) -> &Self {
        match self {
            Self::ImageSet { image, .. } => image.selected_image(),
            image => image,
        }
    }

    /// Return the product of selected `image-set()` resolutions.
    pub(crate) fn intrinsic_resolution(&self) -> f32 {
        match self {
            Self::ImageSet { image, resolution } => resolution * image.intrinsic_resolution(),
            _ => 1.0,
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        match self {
            Self::ImageSet { image, .. } => image.resolve_font_metric_lengths(ch_advance),
            Self::LinearGradient(gradient) => gradient.resolve_font_metric_lengths(ch_advance),
            Self::RadialGradient(gradient) => gradient.resolve_font_metric_lengths(ch_advance),
            Self::ConicGradient(gradient) => gradient.resolve_font_metric_lengths(ch_advance),
            Self::Color(_) => {}
            Self::Url { .. } => {}
        }
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        match self {
            Self::ImageSet { image, .. } => image.resolve_em_relative_lengths(font_size),
            Self::LinearGradient(gradient) => gradient.resolve_em_relative_lengths(font_size),
            Self::RadialGradient(gradient) => gradient.resolve_em_relative_lengths(font_size),
            Self::ConicGradient(gradient) => gradient.resolve_em_relative_lengths(font_size),
            Self::Color(_) | Self::Url { .. } => {}
        }
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        match self {
            Self::ImageSet { image, .. } => {
                image.resolve_root_font_relative_lengths(root_font_size)
            }
            Self::LinearGradient(gradient) => {
                gradient.resolve_root_font_relative_lengths(root_font_size)
            }
            Self::RadialGradient(gradient) => {
                gradient.resolve_root_font_relative_lengths(root_font_size)
            }
            Self::ConicGradient(gradient) => {
                gradient.resolve_root_font_relative_lengths(root_font_size)
            }
            Self::Color(_) | Self::Url { .. } => {}
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        match self {
            Self::ImageSet { image, .. } => image.requires_ch_advance(),
            Self::LinearGradient(gradient) => gradient.requires_ch_advance(),
            Self::RadialGradient(gradient) => gradient.requires_ch_advance(),
            Self::ConicGradient(gradient) => gradient.requires_ch_advance(),
            Self::Color(_) => false,
            Self::Url { .. } => false,
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
    pub attachment: BackgroundAttachment,
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
            attachment: BackgroundAttachment::Scroll,
            origin: BackgroundBox::Padding,
            clip: BackgroundBox::Border,
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Some(image) = &mut self.image {
            image.resolve_font_metric_lengths(ch_advance);
        }
        self.size.resolve_font_metric_lengths(ch_advance);
        self.position.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        if let Some(image) = &mut self.image {
            image.resolve_em_relative_lengths(font_size);
        }
        self.size.resolve_em_relative_lengths(font_size);
        self.position.resolve_em_relative_lengths(font_size);
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        if let Some(image) = &mut self.image {
            image.resolve_root_font_relative_lengths(root_font_size);
        }
        self.size.resolve_root_font_relative_lengths(root_font_size);
        self.position
            .resolve_root_font_relative_lengths(root_font_size);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.image
            .as_ref()
            .is_some_and(BackgroundImage::requires_ch_advance)
            || self.size.requires_ch_advance()
            || self.position.requires_ch_advance()
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        self.size.resolve_line_height_relative_lengths(line_height);
        self.position
            .resolve_line_height_relative_lengths(line_height);
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

/// Computed `conic-gradient()` or `repeating-conic-gradient()` image.
///
/// Conic color stops use degrees in the generated image's clockwise angular
/// coordinate system; percentage positions are normalized to a full turn.
/// <https://drafts.csswg.org/css-images-4/#conic-gradients>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConicGradient {
    pub start_angle: f32,
    pub position: BackgroundPosition,
    pub repeating: bool,
    pub stops: Vec<ConicGradientStop>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ConicGradientStop {
    pub color: Color,
    pub position: Option<f32>,
}

impl ConicGradient {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.position.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        self.position.resolve_em_relative_lengths(font_size);
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        self.position
            .resolve_root_font_relative_lengths(root_font_size);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.position.requires_ch_advance()
    }
}

impl RadialGradient {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.size.resolve_font_metric_lengths(ch_advance);
        self.position.resolve_font_metric_lengths(ch_advance);
        for stop in &mut self.stops {
            stop.resolve_font_metric_lengths(ch_advance);
        }
        for hint in &mut self.hints {
            hint.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        self.size.resolve_em_relative_lengths(font_size);
        self.position.resolve_em_relative_lengths(font_size);
        for stop in &mut self.stops {
            stop.resolve_em_relative_lengths(font_size);
        }
        for hint in &mut self.hints {
            hint.resolve_em_relative_lengths(font_size);
        }
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        self.size.resolve_root_font_relative_lengths(root_font_size);
        self.position
            .resolve_root_font_relative_lengths(root_font_size);
        for stop in &mut self.stops {
            stop.resolve_root_font_relative_lengths(root_font_size);
        }
        for hint in &mut self.hints {
            hint.resolve_root_font_relative_lengths(root_font_size);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.size.requires_ch_advance()
            || self.position.requires_ch_advance()
            || self
                .stops
                .iter()
                .any(GradientColorStop::requires_ch_advance)
            || self
                .hints
                .iter()
                .any(GradientColorHint::requires_ch_advance)
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RadialGradientSize {
    Extent(RadialGradientExtent),
    CircleRadius(ComputedLengthPercentage),
    EllipseRadii {
        x: ComputedLengthPercentage,
        y: ComputedLengthPercentage,
    },
}

impl RadialGradientSize {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        match self {
            Self::CircleRadius(radius) => radius.resolve_font_metric_lengths(ch_advance),
            Self::EllipseRadii { x, y } => {
                x.resolve_font_metric_lengths(ch_advance);
                y.resolve_font_metric_lengths(ch_advance);
            }
            Self::Extent(_) => {}
        }
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        match self {
            Self::CircleRadius(radius) => radius.resolve_em_relative_lengths(font_size),
            Self::EllipseRadii { x, y } => {
                x.resolve_em_relative_lengths(font_size);
                y.resolve_em_relative_lengths(font_size);
            }
            Self::Extent(_) => {}
        }
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        match self {
            Self::CircleRadius(radius) => radius.resolve_root_font_relative_lengths(root_font_size),
            Self::EllipseRadii { x, y } => {
                x.resolve_root_font_relative_lengths(root_font_size);
                y.resolve_root_font_relative_lengths(root_font_size);
            }
            Self::Extent(_) => {}
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        match self {
            Self::CircleRadius(radius) => radius.requires_ch_advance(),
            Self::EllipseRadii { x, y } => x.requires_ch_advance() || y.requires_ch_advance(),
            Self::Extent(_) => false,
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
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        for stop in &mut self.stops {
            stop.resolve_font_metric_lengths(ch_advance);
        }
        for hint in &mut self.hints {
            hint.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        for stop in &mut self.stops {
            stop.resolve_em_relative_lengths(font_size);
        }
        for hint in &mut self.hints {
            hint.resolve_em_relative_lengths(font_size);
        }
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        for stop in &mut self.stops {
            stop.resolve_root_font_relative_lengths(root_font_size);
        }
        for hint in &mut self.hints {
            hint.resolve_root_font_relative_lengths(root_font_size);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.stops
            .iter()
            .any(GradientColorStop::requires_ch_advance)
            || self
                .hints
                .iter()
                .any(GradientColorHint::requires_ch_advance)
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GradientColorStop {
    pub color: Color,
    pub position: Option<ComputedLengthPercentage>,
}

impl GradientColorStop {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Some(position) = &mut self.position {
            position.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        if let Some(position) = &mut self.position {
            position.resolve_em_relative_lengths(font_size);
        }
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        if let Some(position) = &mut self.position {
            position.resolve_root_font_relative_lengths(root_font_size);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.position
            .as_ref()
            .is_some_and(ComputedLengthPercentage::requires_ch_advance)
    }
}

/// One color interpolation hint between two adjacent color stops.
///
/// CSS Images Level 3 permits an unlabeled `<linear-color-hint>` between two
/// color stops to move the midpoint of interpolation:
/// <https://www.w3.org/TR/css-images-3/#color-stop-syntax>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GradientColorHint {
    pub after_stop: usize,
    pub position: ComputedLengthPercentage,
}

impl GradientColorHint {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.position.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        self.position.resolve_em_relative_lengths(font_size);
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        self.position
            .resolve_root_font_relative_lengths(root_font_size);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.position.requires_ch_advance()
    }
}

impl ResolveViewportLengths for BackgroundPositionAxis {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.offset.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for BackgroundPosition {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.x.resolve_viewport_lengths(basis);
        self.y.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for BackgroundSize {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        let Self::Explicit { width, height } = self else {
            return;
        };
        width.resolve_viewport_lengths(basis);
        height.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for BackgroundSizeAxis {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for BackgroundImage {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        match self {
            Self::ImageSet { image, .. } => image.resolve_viewport_lengths(basis),
            Self::LinearGradient(gradient) => gradient.resolve_viewport_lengths(basis),
            Self::RadialGradient(gradient) => gradient.resolve_viewport_lengths(basis),
            Self::ConicGradient(gradient) => gradient.resolve_viewport_lengths(basis),
            Self::Color(_) => {}
            Self::Url { .. } => {}
        }
    }
}

impl ResolveViewportLengths for BackgroundLayer {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Some(image) = &mut self.image {
            image.resolve_viewport_lengths(basis);
        }
        self.size.resolve_viewport_lengths(basis);
        self.position.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for ConicGradient {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.position.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for RadialGradient {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.size.resolve_viewport_lengths(basis);
        self.position.resolve_viewport_lengths(basis);
        for stop in &mut self.stops {
            stop.resolve_viewport_lengths(basis);
        }
        for hint in &mut self.hints {
            hint.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for RadialGradientSize {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        match self {
            Self::CircleRadius(radius) => radius.resolve_viewport_lengths(basis),
            Self::EllipseRadii { x, y } => {
                x.resolve_viewport_lengths(basis);
                y.resolve_viewport_lengths(basis);
            }
            Self::Extent(_) => {}
        }
    }
}

impl ResolveViewportLengths for LinearGradient {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        for stop in &mut self.stops {
            stop.resolve_viewport_lengths(basis);
        }
        for hint in &mut self.hints {
            hint.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for GradientColorStop {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Some(position) = &mut self.position {
            position.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for GradientColorHint {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.position.resolve_viewport_lengths(basis);
    }
}
