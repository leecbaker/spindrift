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
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        self.offset.requires_selected_font_metrics()
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        self.offset.resolve_selected_font_metric_lengths(basis);
    }

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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.offset.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.offset.requires_root_font_metrics()
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
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        self.x.requires_selected_font_metrics() || self.y.requires_selected_font_metrics()
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        self.x.resolve_selected_font_metric_lengths(basis);
        self.y.resolve_selected_font_metric_lengths(basis);
    }

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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.x.resolve_root_font_metric_lengths(basis);
        self.y.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.x.requires_root_font_metrics() || self.y.requires_root_font_metrics()
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
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        matches!(self, Self::Explicit { width, height }
            if width.requires_selected_font_metrics() || height.requires_selected_font_metrics())
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        if let Self::Explicit { width, height } = self {
            width.resolve_selected_font_metric_lengths(basis);
            height.resolve_selected_font_metric_lengths(basis);
        }
    }

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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::Explicit { width, height } = self {
            width.resolve_root_font_metric_lengths(basis);
            height.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::Explicit { width, height } if width.requires_root_font_metrics() || height.requires_root_font_metrics())
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
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_selected_font_metrics())
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_selected_font_metric_lengths(basis);
        }
    }

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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_root_font_metrics())
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

/// A computed CSS image-property value.
///
/// CSS image-consuming properties distinguish the `none` keyword from a
/// syntactically valid image that has become invalid (for example, an
/// `image-set()` whose candidates were all removed by MIME negotiation).
/// Keeping the distinction in computed style prevents property parsers and
/// paint consumers from accidentally treating an invalid image as an invalid
/// declaration or as the `none` keyword.
/// <https://drafts.csswg.org/css-images-4/#invalid-image>
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComputedImage {
    /// The CSS `none` keyword or the property's initial value.
    None,
    /// A valid CSS image value that represents an invalid image.
    Invalid,
    /// A syntactically valid image value. Its external resource can still
    /// fail to load later at paint time.
    Image(Box<BackgroundImage>),
}

impl ComputedImage {
    pub(crate) fn image(image: BackgroundImage) -> Self {
        Self::Image(Box::new(image))
    }

    pub(crate) const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub(crate) const fn is_image(&self) -> bool {
        matches!(self, Self::Image(_))
    }

    pub(crate) const fn as_image(&self) -> Option<&BackgroundImage> {
        match self {
            Self::Image(image) => Some(image),
            Self::None | Self::Invalid => None,
        }
    }

    pub(crate) fn as_image_mut(&mut self) -> Option<&mut BackgroundImage> {
        match self {
            Self::Image(image) => Some(image),
            Self::None | Self::Invalid => None,
        }
    }

    /// Select a concrete `image-set()` option, preserving the CSS distinction
    /// between `none` and a valid value that represents an invalid image.
    #[cfg(test)]
    pub(crate) fn select_image_set(&mut self, device_resolution_dppx: f32) {
        let Some(image) = self.as_image_mut() else {
            return;
        };
        if !image.select_image_set(device_resolution_dppx) {
            *self = Self::Invalid;
        }
    }

    /// Resolve image forms whose computed value depends on the element's
    /// used color scheme or the rendering environment.
    ///
    /// CSS Color 5 selects `light-dark()` before CSS Images selects an
    /// `image-set()` candidate, so a selected light/dark branch can itself be
    /// an image set.
    /// <https://drafts.csswg.org/css-color-5/#light-dark>
    /// <https://drafts.csswg.org/css-images-4/#image-set-notation>
    pub(crate) fn resolve_for_context(&mut self, context: ImageSelectionContext) {
        let Some(image) = self.as_image_mut() else {
            return;
        };
        if !image.resolve_for_context(context) {
            *self = Self::Invalid;
        }
    }
}

/// The element and rendering-environment inputs used to resolve CSS images.
///
/// Keeping the color-scheme and device-resolution inputs together preserves
/// CSS Color 5's required resolution order for `light-dark()` and
/// `image-set()`.
/// <https://drafts.csswg.org/css-color-5/#light-dark>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ImageSelectionContext {
    pub(crate) used_color_scheme: UsedColorScheme,
    pub(crate) resolution_dppx: f32,
}

/// Computed single concrete CSS image.
///
/// CSS Images defines gradients as generated images. The renderer supports URL
/// images and CSS Images Level 3 linear and radial gradients:
/// <https://www.w3.org/TR/css-images-3/#gradients>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BackgroundImage {
    /// A CSS Color 5 image whose concrete value depends on the owning
    /// element's used color scheme.
    /// <https://drafts.csswg.org/css-color-5/#light-dark>
    LightDark(LightDarkImage),
    /// An `image-set()` before the renderer has selected its concrete option.
    ///
    /// CSS Images requires MIME filtering and duplicate-resolution removal to
    /// happen before the user agent chooses an option. Retaining the complete
    /// list until that boundary also keeps stylesheet and inline declarations
    /// on the same selection path.
    /// <https://drafts.csswg.org/css-images-4/#image-set-notation>
    ImageSet(ImageSet),
    /// The concrete option selected from an `image-set()` together with its
    /// resolution. CSS Images uses that resolution to scale the selected
    /// raster image's intrinsic dimensions.
    /// <https://drafts.csswg.org/css-images-4/#image-set-notation>
    SelectedImageSet {
        image: Box<BackgroundImage>,
        resolution: f32,
    },
    /// A direct external image URL.
    Url(ImageUrl),
    /// CSS Images Level 5's `image()` notation.
    ///
    /// Unlike a plain URL, the source can fall back to a dimensionless color
    /// image after resource selection. The direction tag belongs to the
    /// source image and is intentionally retained until paint, where the
    /// consuming element's logical inline axis is known.
    /// <https://drafts.csswg.org/css-images-5/#image-notation>
    ImageFunction(ImageFunction),
    LinearGradient(LinearGradient),
    RadialGradient(RadialGradient),
    ConicGradient(ConicGradient),
    CssColor(ColorImageColor),
}

/// A CSS external image reference shared by `url()` and `image()`.
///
/// Keeping request modifiers and stylesheet URL context with the source makes
/// resource loading independent of the CSS function that contained it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ImageUrl {
    pub(crate) href: String,
    pub(crate) base_url: Option<url::Url>,
    pub(crate) root_url: Option<url::Url>,
    pub(crate) request_modifiers: crate::css::RequestUrlModifiers,
}

/// CSS Images Level 5's `image()` source/fallback value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ImageFunction {
    pub(crate) source: Option<ImageUrl>,
    pub(crate) fallback_color: Option<ColorImageColor>,
    pub(crate) directionality: Option<ImageDirectionality>,
}

/// The authored directionality of an `image()` source.
///
/// This is intentionally distinct from the element's CSS `Direction`: the
/// latter selects whether this source needs an inline-axis reflection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageDirectionality {
    Ltr,
    Rtl,
}

/// The light and dark image branches of CSS Color 5's `light-dark()`.
///
/// The parser normalizes a `none` branch to `image(transparent)`, so both
/// fields are always concrete CSS images rather than optional values.
/// <https://drafts.csswg.org/css-color-5/#light-dark>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LightDarkImage {
    pub(crate) light: Box<BackgroundImage>,
    pub(crate) dark: Box<BackgroundImage>,
}

/// Parsed CSS Images `image-set()` candidates before UA selection.
///
/// Source order is semantically significant: after unsupported MIME types are
/// removed, later candidates with an already-seen resolution are discarded.
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ImageSet {
    pub(crate) options: Vec<ImageSetOption>,
}

/// One candidate in an [`ImageSet`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ImageSetOption {
    pub(crate) image: Box<BackgroundImage>,
    /// Canonical CSS dots-per-pixel value. Resolution descriptors have no
    /// percentage basis and therefore compute before candidate negotiation.
    pub(crate) resolution_dppx: f32,
    pub(crate) mime_type: Option<String>,
}

/// The color argument to CSS Images Level 4's `image()` function.
///
/// `currentcolor` remains symbolic until the generated image is used, because
/// its value is the element's computed `color`, not a parser-global default.
/// <https://drafts.csswg.org/css-color-4/#currentcolor-color>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ColorImageColor {
    CssColor(CssColor),
    CurrentColor,
}

impl ColorImageColor {
    pub(crate) fn resolve(self, current_color: CssColor) -> CssColor {
        match self {
            Self::CssColor(color) => color,
            Self::CurrentColor => current_color,
        }
    }
}

impl BackgroundImage {
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        match self {
            Self::LightDark(branches) => {
                branches.light.requires_selected_font_metrics()
                    || branches.dark.requires_selected_font_metrics()
            }
            Self::ImageSet(set) => set
                .options
                .iter()
                .any(|option| option.image.requires_selected_font_metrics()),
            Self::SelectedImageSet { image, .. } => image.requires_selected_font_metrics(),
            Self::LinearGradient(gradient) => gradient.requires_selected_font_metrics(),
            Self::RadialGradient(gradient) => gradient.requires_selected_font_metrics(),
            Self::ConicGradient(gradient) => gradient.requires_selected_font_metrics(),
            Self::CssColor(_) | Self::Url(_) | Self::ImageFunction(_) => false,
        }
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        match self {
            Self::LightDark(branches) => {
                branches.light.resolve_selected_font_metric_lengths(basis);
                branches.dark.resolve_selected_font_metric_lengths(basis);
            }
            Self::ImageSet(set) => {
                for option in &mut set.options {
                    option.image.resolve_selected_font_metric_lengths(basis);
                }
            }
            Self::SelectedImageSet { image, .. } => {
                image.resolve_selected_font_metric_lengths(basis)
            }
            Self::LinearGradient(gradient) => gradient.resolve_selected_font_metric_lengths(basis),
            Self::RadialGradient(gradient) => gradient.resolve_selected_font_metric_lengths(basis),
            Self::ConicGradient(gradient) => gradient.resolve_selected_font_metric_lengths(basis),
            Self::CssColor(_) | Self::Url(_) | Self::ImageFunction(_) => {}
        }
    }

    /// Resolve `lh` components after the consuming element's computed line
    /// height is available.  Generated images inherit the same local
    /// font-relative basis as their owning property.
    /// <https://drafts.csswg.org/css-values-4/#lh>
    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        match self {
            Self::LightDark(branches) => {
                branches
                    .light
                    .resolve_line_height_relative_lengths(line_height);
                branches
                    .dark
                    .resolve_line_height_relative_lengths(line_height);
            }
            Self::ImageSet(set) => {
                for option in &mut set.options {
                    option
                        .image
                        .resolve_line_height_relative_lengths(line_height);
                }
            }
            Self::SelectedImageSet { image, .. } => {
                image.resolve_line_height_relative_lengths(line_height)
            }
            Self::LinearGradient(gradient) => {
                gradient.resolve_line_height_relative_lengths(line_height)
            }
            Self::RadialGradient(gradient) => {
                gradient.resolve_line_height_relative_lengths(line_height)
            }
            Self::ConicGradient(gradient) => {
                gradient.resolve_line_height_relative_lengths(line_height)
            }
            Self::CssColor(_) | Self::Url(_) | Self::ImageFunction(_) => {}
        }
    }

    /// Resolve every environment-dependent image form before layout, asset
    /// loading, and paint consume the concrete image.
    pub(crate) fn resolve_for_context(&mut self, context: ImageSelectionContext) -> bool {
        self.resolve_light_dark(context.used_color_scheme);
        self.select_image_set(context.resolution_dppx)
    }

    /// Return the selected image after unwrapping any nested `image-set()`
    /// candidates.
    pub(crate) fn selected_image(&self) -> &Self {
        match self {
            Self::SelectedImageSet { image, .. } => image.selected_image(),
            Self::ImageSet(_) => {
                debug_assert!(false, "image-set candidates must be selected before layout");
                self
            }
            Self::LightDark(_) => {
                debug_assert!(false, "light-dark() must resolve before layout");
                self
            }
            image => image,
        }
    }

    /// Return the product of selected `image-set()` resolutions.
    pub(crate) fn intrinsic_resolution(&self) -> f32 {
        match self {
            Self::LightDark(_) => 1.0,
            Self::SelectedImageSet { image, resolution } => {
                resolution * image.intrinsic_resolution()
            }
            Self::ImageSet(_) => 1.0,
            _ => 1.0,
        }
    }

    /// Resolve an `image-set()` using Spindrift's deterministic quality-first
    /// static-rendering policy. Unsupported MIME options are removed before
    /// duplicate-resolution elimination, exactly as required by CSS Images.
    pub(crate) fn select_image_set(&mut self, device_resolution_dppx: f32) -> bool {
        let Self::ImageSet(set) = self else {
            return true;
        };
        let mut retained = Vec::new();
        for option in &set.options {
            if !option.resolution_dppx.is_finite()
                || option.resolution_dppx <= 0.0
                || option.mime_type.as_ref().is_some_and(|mime| {
                    !crate::image_store::supports_declared_image_mime_type(mime)
                })
                || retained.iter().any(|candidate: &&ImageSetOption| {
                    candidate.resolution_dppx == option.resolution_dppx
                })
            {
                continue;
            }
            retained.push(option);
        }
        let selected = retained
            .iter()
            .filter(|option| option.resolution_dppx >= device_resolution_dppx)
            .min_by(|left, right| left.resolution_dppx.total_cmp(&right.resolution_dppx))
            .or_else(|| {
                retained
                    .iter()
                    .max_by(|left, right| left.resolution_dppx.total_cmp(&right.resolution_dppx))
            });
        let Some(selected) = selected else {
            return false;
        };
        *self = Self::SelectedImageSet {
            image: selected.image.clone(),
            resolution: selected.resolution_dppx,
        };
        true
    }

    /// Select the used `light-dark()` image branch throughout this image.
    ///
    /// Branch selection happens before `image-set()` candidate negotiation;
    /// callers perform that second step after this traversal.
    /// <https://drafts.csswg.org/css-color-5/#light-dark>
    pub(crate) fn resolve_light_dark(&mut self, used_color_scheme: UsedColorScheme) {
        match self {
            Self::LightDark(branches) => {
                let selected = match used_color_scheme {
                    UsedColorScheme::Light => (*branches.light).clone(),
                    UsedColorScheme::Dark => (*branches.dark).clone(),
                };
                *self = selected;
                self.resolve_light_dark(used_color_scheme);
            }
            Self::ImageSet(set) => {
                for option in &mut set.options {
                    option.image.resolve_light_dark(used_color_scheme);
                }
            }
            Self::SelectedImageSet { image, .. } => image.resolve_light_dark(used_color_scheme),
            Self::Url(_)
            | Self::ImageFunction(_)
            | Self::LinearGradient(_)
            | Self::RadialGradient(_)
            | Self::ConicGradient(_)
            | Self::CssColor(_) => {}
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        match self {
            Self::LightDark(branches) => {
                branches.light.resolve_font_metric_lengths(ch_advance);
                branches.dark.resolve_font_metric_lengths(ch_advance);
            }
            Self::ImageSet(set) => {
                for option in &mut set.options {
                    option.image.resolve_font_metric_lengths(ch_advance);
                }
            }
            Self::SelectedImageSet { image, .. } => image.resolve_font_metric_lengths(ch_advance),
            Self::LinearGradient(gradient) => gradient.resolve_font_metric_lengths(ch_advance),
            Self::RadialGradient(gradient) => gradient.resolve_font_metric_lengths(ch_advance),
            Self::ConicGradient(gradient) => gradient.resolve_font_metric_lengths(ch_advance),
            Self::CssColor(_) | Self::Url(_) | Self::ImageFunction(_) => {}
        }
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        match self {
            Self::LightDark(branches) => {
                branches.light.resolve_em_relative_lengths(font_size);
                branches.dark.resolve_em_relative_lengths(font_size);
            }
            Self::ImageSet(set) => {
                for option in &mut set.options {
                    option.image.resolve_em_relative_lengths(font_size);
                }
            }
            Self::SelectedImageSet { image, .. } => image.resolve_em_relative_lengths(font_size),
            Self::LinearGradient(gradient) => gradient.resolve_em_relative_lengths(font_size),
            Self::RadialGradient(gradient) => gradient.resolve_em_relative_lengths(font_size),
            Self::ConicGradient(gradient) => gradient.resolve_em_relative_lengths(font_size),
            Self::CssColor(_) | Self::Url(_) | Self::ImageFunction(_) => {}
        }
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        match self {
            Self::LightDark(branches) => {
                branches
                    .light
                    .resolve_root_font_relative_lengths(root_font_size);
                branches
                    .dark
                    .resolve_root_font_relative_lengths(root_font_size);
            }
            Self::ImageSet(set) => {
                for option in &mut set.options {
                    option
                        .image
                        .resolve_root_font_relative_lengths(root_font_size);
                }
            }
            Self::SelectedImageSet { image, .. } => {
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
            Self::CssColor(_) | Self::Url(_) | Self::ImageFunction(_) => {}
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::LightDark(branches) => {
                branches.light.resolve_root_font_metric_lengths(basis);
                branches.dark.resolve_root_font_metric_lengths(basis);
            }
            Self::ImageSet(set) => {
                for option in &mut set.options {
                    option.image.resolve_root_font_metric_lengths(basis);
                }
            }
            Self::SelectedImageSet { image, .. } => image.resolve_root_font_metric_lengths(basis),
            Self::LinearGradient(gradient) => gradient.resolve_root_font_metric_lengths(basis),
            Self::RadialGradient(gradient) => gradient.resolve_root_font_metric_lengths(basis),
            Self::ConicGradient(gradient) => gradient.resolve_root_font_metric_lengths(basis),
            Self::CssColor(_) | Self::Url(_) | Self::ImageFunction(_) => {}
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::LightDark(branches) => {
                branches.light.requires_root_font_metrics()
                    || branches.dark.requires_root_font_metrics()
            }
            Self::ImageSet(set) => set
                .options
                .iter()
                .any(|option| option.image.requires_root_font_metrics()),
            Self::SelectedImageSet { image, .. } => image.requires_root_font_metrics(),
            Self::LinearGradient(gradient) => gradient.requires_root_font_metrics(),
            Self::RadialGradient(gradient) => gradient.requires_root_font_metrics(),
            Self::ConicGradient(gradient) => gradient.requires_root_font_metrics(),
            Self::CssColor(_) | Self::Url(_) | Self::ImageFunction(_) => false,
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        match self {
            Self::LightDark(branches) => {
                branches.light.requires_ch_advance() || branches.dark.requires_ch_advance()
            }
            Self::ImageSet(set) => set
                .options
                .iter()
                .any(|option| option.image.requires_ch_advance()),
            Self::SelectedImageSet { image, .. } => image.requires_ch_advance(),
            Self::LinearGradient(gradient) => gradient.requires_ch_advance(),
            Self::RadialGradient(gradient) => gradient.requires_ch_advance(),
            Self::ConicGradient(gradient) => gradient.requires_ch_advance(),
            Self::CssColor(_) | Self::Url(_) | Self::ImageFunction(_) => false,
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
    pub image: ComputedImage,
    pub position: BackgroundPosition,
    pub size: BackgroundSize,
    pub repeat: BackgroundRepeat,
    pub attachment: BackgroundAttachment,
    pub origin: BackgroundBox,
    pub clip: BackgroundBox,
}

impl BackgroundLayer {
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        self.image
            .as_image()
            .is_some_and(BackgroundImage::requires_selected_font_metrics)
            || self.size.requires_selected_font_metrics()
            || self.position.requires_selected_font_metrics()
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        if let Some(image) = self.image.as_image_mut() {
            image.resolve_selected_font_metric_lengths(basis);
        }
        self.size.resolve_selected_font_metric_lengths(basis);
        self.position.resolve_selected_font_metric_lengths(basis);
    }

    pub(crate) const fn initial() -> Self {
        Self {
            image: ComputedImage::None,
            position: BackgroundPosition::INITIAL,
            size: BackgroundSize::AUTO,
            repeat: BackgroundRepeat::Repeat,
            attachment: BackgroundAttachment::Scroll,
            origin: BackgroundBox::Padding,
            clip: BackgroundBox::Border,
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Some(image) = self.image.as_image_mut() {
            image.resolve_font_metric_lengths(ch_advance);
        }
        self.size.resolve_font_metric_lengths(ch_advance);
        self.position.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        if let Some(image) = self.image.as_image_mut() {
            image.resolve_em_relative_lengths(font_size);
        }
        self.size.resolve_em_relative_lengths(font_size);
        self.position.resolve_em_relative_lengths(font_size);
    }

    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        if let Some(image) = self.image.as_image_mut() {
            image.resolve_root_font_relative_lengths(root_font_size);
        }
        self.size.resolve_root_font_relative_lengths(root_font_size);
        self.position
            .resolve_root_font_relative_lengths(root_font_size);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Some(image) = self.image.as_image_mut() {
            image.resolve_root_font_metric_lengths(basis);
        }
        self.size.resolve_root_font_metric_lengths(basis);
        self.position.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.image
            .as_image()
            .is_some_and(BackgroundImage::requires_root_font_metrics)
            || self.size.requires_root_font_metrics()
            || self.position.requires_root_font_metrics()
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.image
            .as_image()
            .is_some_and(BackgroundImage::requires_ch_advance)
            || self.size.requires_ch_advance()
            || self.position.requires_ch_advance()
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        if let Some(image) = self.image.as_image_mut() {
            image.resolve_line_height_relative_lengths(line_height);
        }
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
    /// The CSS CssColor 4 coordinate system used between color stops.
    ///
    /// Until CSS Images 4 default-color-space support is enabled, unqualified
    /// gradients use CSS Images 3's premultiplied sRGB interpolation. An
    /// explicit `in <color-space>` prelude retains its requested CSS Color 4
    /// method.
    ///
    /// CSS Images 4 instead defaults an omitted prelude to Oklab; that
    /// intentional divergence is tracked in `SPEC_DIVERGENCES.md`.
    /// <https://www.w3.org/TR/css-images-3/#coloring-gradient-line>
    /// <https://drafts.csswg.org/css-images-4/#coloring-gradient-line>
    pub interpolation: GradientInterpolationMethod,
    pub repeating: bool,
    pub stops: Vec<GradientColorStop>,
    pub hints: Vec<GradientColorHint>,
}

impl LinearGradient {
    /// Resolves the element-dependent `currentcolor` token at the gradient's
    /// used-value boundary. CSS CssColor defines `currentcolor` in terms of the
    /// consuming element, rather than the element that supplied the image.
    pub(crate) fn resolve_current_color(&self, current_color: CssColor) -> Self {
        let mut resolved = self.clone();
        for stop in &mut resolved.stops {
            stop.color = stop.color.resolve_current_color(current_color);
        }
        resolved
    }
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
    pub interpolation: GradientInterpolationMethod,
    pub repeating: bool,
    pub stops: Vec<GradientColorStop>,
    pub hints: Vec<GradientColorHint>,
}

impl RadialGradient {
    pub(crate) fn resolve_current_color(&self, current_color: CssColor) -> Self {
        let mut resolved = self.clone();
        for stop in &mut resolved.stops {
            stop.color = stop.color.resolve_current_color(current_color);
        }
        resolved
    }
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
    pub interpolation: GradientInterpolationMethod,
    pub repeating: bool,
    pub stops: Vec<ConicGradientStop>,
}

impl ConicGradient {
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        self.position.requires_selected_font_metrics()
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        self.position.resolve_selected_font_metric_lengths(basis);
    }

    pub(crate) fn resolve_current_color(&self, current_color: CssColor) -> Self {
        let mut resolved = self.clone();
        for stop in &mut resolved.stops {
            stop.color = stop.color.resolve_current_color(current_color);
        }
        resolved
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ConicGradientStop {
    pub color: GradientColor,
    pub position: Option<f32>,
}

/// A gradient stop keeps `currentcolor` symbolic until the consuming element's
/// used style is known. Unlike an ordinary solid paint, a gradient can be
/// reused by generated content and border images, so resolving it during CSS
/// token parsing would bind it to the wrong element.
/// <https://drafts.csswg.org/css-color-4/#currentcolor-color>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum GradientColor {
    CssColor(CssColor),
    /// A specified color whose `none` components remain missing until CSS
    /// CssColor interpolation has copied analogous components from the other
    /// endpoint. The concrete color is still kept for ordinary paint paths.
    ColorWithMissing {
        color: CssColor,
        missing: GradientMissingComponents,
        source: GradientMissingComponentSpace,
    },
    CurrentColor,
}

/// Bitset for the first three color components and alpha, in source order.
/// CSS CssColor's missing-component fixup happens after conversion into the
/// interpolation space, immediately before premultiplied interpolation.
/// <https://drafts.csswg.org/css-color-4/#interpolation-missing>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct GradientMissingComponents(u8);

/// The specified coordinate family that carried `none`. CSS CssColor only
/// propagates missing components into analogous coordinates; an RGB channel
/// is not, for example, an Oklab lightness coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GradientMissingComponentSpace {
    Rgb,
    Xyz,
    Lab,
    Oklab,
    Hsl,
    Hwb,
    Lch,
    Oklch,
}

impl GradientMissingComponents {
    pub(crate) const fn new(bits: u8) -> Self {
        Self(bits & 0b1111)
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl GradientColor {
    pub(crate) const fn resolve(self, current_color: CssColor) -> CssColor {
        match self {
            Self::CssColor(color) | Self::ColorWithMissing { color, .. } => color,
            Self::CurrentColor => current_color,
        }
    }

    pub(crate) const fn is_current_color(self) -> bool {
        matches!(self, Self::CurrentColor)
    }

    pub(crate) const fn as_color(self) -> Option<CssColor> {
        match self {
            Self::CssColor(color) | Self::ColorWithMissing { color, .. } => Some(color),
            Self::CurrentColor => None,
        }
    }

    pub(crate) const fn missing_components_for(
        self,
        interpolation: GradientInterpolationMethod,
    ) -> GradientMissingComponents {
        let Self::ColorWithMissing {
            missing, source, ..
        } = self
        else {
            return GradientMissingComponents::new(0);
        };
        let analogous = match source {
            GradientMissingComponentSpace::Rgb => matches!(
                interpolation.space,
                GradientInterpolationSpace::Srgb
                    | GradientInterpolationSpace::SrgbLinear
                    | GradientInterpolationSpace::DisplayP3
                    | GradientInterpolationSpace::DisplayP3Linear
                    | GradientInterpolationSpace::A98Rgb
                    | GradientInterpolationSpace::ProphotoRgb
                    | GradientInterpolationSpace::Rec2020
            ),
            GradientMissingComponentSpace::Xyz => matches!(
                interpolation.space,
                GradientInterpolationSpace::XyzD50 | GradientInterpolationSpace::XyzD65
            ),
            GradientMissingComponentSpace::Lab => {
                matches!(interpolation.space, GradientInterpolationSpace::Lab)
            }
            GradientMissingComponentSpace::Oklab => {
                matches!(interpolation.space, GradientInterpolationSpace::Oklab)
            }
            GradientMissingComponentSpace::Hsl => {
                matches!(interpolation.space, GradientInterpolationSpace::Hsl)
            }
            GradientMissingComponentSpace::Hwb => {
                matches!(interpolation.space, GradientInterpolationSpace::Hwb)
            }
            GradientMissingComponentSpace::Lch => {
                matches!(interpolation.space, GradientInterpolationSpace::Lch)
            }
            GradientMissingComponentSpace::Oklch => {
                matches!(interpolation.space, GradientInterpolationSpace::Oklch)
            }
        };
        if analogous {
            missing
        } else {
            GradientMissingComponents::new(0)
        }
    }

    pub(crate) const fn resolve_current_color(self, current_color: CssColor) -> Self {
        match self {
            Self::CurrentColor => Self::CssColor(current_color),
            color => color,
        }
    }
}

/// CSS Images 4's gradient-specific color interpolation method.
///
/// This intentionally lives with gradients rather than `CssColorSpace`: the
/// latter denotes retained PDF component spaces, while these values describe
/// both the CSS conversion coordinates and, for polar spaces, a hue path.
/// <https://drafts.csswg.org/css-color-4/#interpolation>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GradientInterpolationMethod {
    pub space: GradientInterpolationSpace,
    pub hue: HueInterpolationMethod,
}

impl GradientInterpolationMethod {
    /// CSS Images Level 3's default gradient interpolation method.
    ///
    /// CSS Images 3 interpolates gradient stops in premultiplied RGBA. Named
    /// and hexadecimal CSS colors are sRGB colors, so the stored RGB
    /// components are interpolated directly.
    /// <https://www.w3.org/TR/css-images-3/#coloring-gradient-line>
    pub(crate) const CSS_IMAGES_3: Self = Self {
        space: GradientInterpolationSpace::Srgb,
        hue: HueInterpolationMethod::Shorter,
    };

    pub(crate) const OKLAB: Self = Self {
        space: GradientInterpolationSpace::Oklab,
        hue: HueInterpolationMethod::Shorter,
    };

    pub(crate) const fn is_polar(self) -> bool {
        self.space.is_polar()
    }
}

impl Default for GradientInterpolationMethod {
    fn default() -> Self {
        Self::OKLAB
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GradientInterpolationSpace {
    Srgb,
    SrgbLinear,
    DisplayP3,
    DisplayP3Linear,
    A98Rgb,
    ProphotoRgb,
    Rec2020,
    XyzD50,
    XyzD65,
    Lab,
    Oklab,
    Hsl,
    Hwb,
    Lch,
    Oklch,
}

impl GradientInterpolationSpace {
    pub(crate) const fn is_polar(self) -> bool {
        matches!(self, Self::Hsl | Self::Hwb | Self::Lch | Self::Oklch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HueInterpolationMethod {
    Shorter,
    Longer,
    Increasing,
    Decreasing,
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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.position.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.position.requires_root_font_metrics()
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.position.requires_ch_advance()
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        self.position
            .resolve_line_height_relative_lengths(line_height);
    }
}

impl RadialGradient {
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        self.size.requires_selected_font_metrics()
            || self.position.requires_selected_font_metrics()
            || self
                .stops
                .iter()
                .any(GradientColorStop::requires_selected_font_metrics)
            || self
                .hints
                .iter()
                .any(GradientColorHint::requires_selected_font_metrics)
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        self.size.resolve_selected_font_metric_lengths(basis);
        self.position.resolve_selected_font_metric_lengths(basis);
        for stop in &mut self.stops {
            stop.resolve_selected_font_metric_lengths(basis);
        }
        for hint in &mut self.hints {
            hint.resolve_selected_font_metric_lengths(basis);
        }
    }

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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.size.resolve_root_font_metric_lengths(basis);
        self.position.resolve_root_font_metric_lengths(basis);
        for stop in &mut self.stops {
            stop.resolve_root_font_metric_lengths(basis);
        }
        for hint in &mut self.hints {
            hint.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.size.requires_root_font_metrics()
            || self.position.requires_root_font_metrics()
            || self
                .stops
                .iter()
                .any(GradientColorStop::requires_root_font_metrics)
            || self
                .hints
                .iter()
                .any(GradientColorHint::requires_root_font_metrics)
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

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        self.size.resolve_line_height_relative_lengths(line_height);
        self.position
            .resolve_line_height_relative_lengths(line_height);
        for stop in &mut self.stops {
            stop.resolve_line_height_relative_lengths(line_height);
        }
        for hint in &mut self.hints {
            hint.resolve_line_height_relative_lengths(line_height);
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
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        match self {
            Self::CircleRadius(radius) => radius.requires_selected_font_metrics(),
            Self::EllipseRadii { x, y } => {
                x.requires_selected_font_metrics() || y.requires_selected_font_metrics()
            }
            Self::Extent(_) => false,
        }
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        match self {
            Self::CircleRadius(radius) => radius.resolve_selected_font_metric_lengths(basis),
            Self::EllipseRadii { x, y } => {
                x.resolve_selected_font_metric_lengths(basis);
                y.resolve_selected_font_metric_lengths(basis);
            }
            Self::Extent(_) => {}
        }
    }

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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::CircleRadius(radius) => radius.resolve_root_font_metric_lengths(basis),
            Self::EllipseRadii { x, y } => {
                x.resolve_root_font_metric_lengths(basis);
                y.resolve_root_font_metric_lengths(basis);
            }
            Self::Extent(_) => {}
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::CircleRadius(radius) => radius.requires_root_font_metrics(),
            Self::EllipseRadii { x, y } => {
                x.requires_root_font_metrics() || y.requires_root_font_metrics()
            }
            Self::Extent(_) => false,
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        match self {
            Self::CircleRadius(radius) => radius.requires_ch_advance(),
            Self::EllipseRadii { x, y } => x.requires_ch_advance() || y.requires_ch_advance(),
            Self::Extent(_) => false,
        }
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        match self {
            Self::CircleRadius(radius) => radius.resolve_line_height_relative_lengths(line_height),
            Self::EllipseRadii { x, y } => {
                x.resolve_line_height_relative_lengths(line_height);
                y.resolve_line_height_relative_lengths(line_height);
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
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        self.stops
            .iter()
            .any(GradientColorStop::requires_selected_font_metrics)
            || self
                .hints
                .iter()
                .any(GradientColorHint::requires_selected_font_metrics)
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        for stop in &mut self.stops {
            stop.resolve_selected_font_metric_lengths(basis);
        }
        for hint in &mut self.hints {
            hint.resolve_selected_font_metric_lengths(basis);
        }
    }

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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        for stop in &mut self.stops {
            stop.resolve_root_font_metric_lengths(basis);
        }
        for hint in &mut self.hints {
            hint.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.stops
            .iter()
            .any(GradientColorStop::requires_root_font_metrics)
            || self
                .hints
                .iter()
                .any(GradientColorHint::requires_root_font_metrics)
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

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        for stop in &mut self.stops {
            stop.resolve_line_height_relative_lengths(line_height);
        }
        for hint in &mut self.hints {
            hint.resolve_line_height_relative_lengths(line_height);
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GradientColorStop {
    pub color: GradientColor,
    pub position: Option<ComputedLengthPercentage>,
}

impl GradientColorStop {
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        self.position
            .as_ref()
            .is_some_and(ComputedLengthPercentage::requires_selected_font_metrics)
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        if let Some(position) = &mut self.position {
            position.resolve_selected_font_metric_lengths(basis);
        }
    }

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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Some(position) = &mut self.position {
            position.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.position
            .as_ref()
            .is_some_and(ComputedLengthPercentage::requires_root_font_metrics)
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.position
            .as_ref()
            .is_some_and(ComputedLengthPercentage::requires_ch_advance)
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        if let Some(position) = &mut self.position {
            position.resolve_line_height_relative_lengths(line_height);
        }
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
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        self.position.requires_selected_font_metrics()
    }

    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        self.position.resolve_selected_font_metric_lengths(basis);
    }

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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.position.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.position.requires_root_font_metrics()
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.position.requires_ch_advance()
    }

    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        self.position
            .resolve_line_height_relative_lengths(line_height);
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
            Self::LightDark(branches) => {
                branches.light.resolve_viewport_lengths(basis);
                branches.dark.resolve_viewport_lengths(basis);
            }
            Self::ImageSet(set) => {
                for option in &mut set.options {
                    option.image.resolve_viewport_lengths(basis);
                }
            }
            Self::SelectedImageSet { image, .. } => image.resolve_viewport_lengths(basis),
            Self::LinearGradient(gradient) => gradient.resolve_viewport_lengths(basis),
            Self::RadialGradient(gradient) => gradient.resolve_viewport_lengths(basis),
            Self::ConicGradient(gradient) => gradient.resolve_viewport_lengths(basis),
            Self::CssColor(_) | Self::Url(_) | Self::ImageFunction(_) => {}
        }
    }
}

impl ResolveViewportLengths for BackgroundLayer {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Some(image) = self.image.as_image_mut() {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundRepeatAxis {
    Repeat,
    Space,
    Round,
    NoRepeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundRepeat {
    Repeat,
    NoRepeat,
    RepeatX,
    RepeatY,
    Axes {
        x: BackgroundRepeatAxis,
        y: BackgroundRepeatAxis,
    },
}

impl BackgroundRepeat {
    pub(crate) fn new(x: BackgroundRepeatAxis, y: BackgroundRepeatAxis) -> Self {
        match (x, y) {
            (BackgroundRepeatAxis::Repeat, BackgroundRepeatAxis::Repeat) => Self::Repeat,
            (BackgroundRepeatAxis::NoRepeat, BackgroundRepeatAxis::NoRepeat) => Self::NoRepeat,
            (BackgroundRepeatAxis::Repeat, BackgroundRepeatAxis::NoRepeat) => Self::RepeatX,
            (BackgroundRepeatAxis::NoRepeat, BackgroundRepeatAxis::Repeat) => Self::RepeatY,
            (x, y) => Self::Axes { x, y },
        }
    }

    pub(crate) fn x_axis(self) -> BackgroundRepeatAxis {
        match self {
            Self::Repeat | Self::RepeatX => BackgroundRepeatAxis::Repeat,
            Self::NoRepeat | Self::RepeatY => BackgroundRepeatAxis::NoRepeat,
            Self::Axes { x, .. } => x,
        }
    }

    pub(crate) fn y_axis(self) -> BackgroundRepeatAxis {
        match self {
            Self::Repeat | Self::RepeatY => BackgroundRepeatAxis::Repeat,
            Self::NoRepeat | Self::RepeatX => BackgroundRepeatAxis::NoRepeat,
            Self::Axes { y, .. } => y,
        }
    }

    /// Returns whether the background image repeats on the physical x axis.
    ///
    /// CSS Backgrounds and Borders defines `repeat`, `space`, and `round` as
    /// repeated styles; only `no-repeat` suppresses additional tiles:
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
    pub(crate) fn repeats_x(self) -> bool {
        self.x_axis() != BackgroundRepeatAxis::NoRepeat
    }

    /// Returns whether the background image repeats on the physical y axis.
    ///
    /// CSS Backgrounds and Borders defines `repeat`, `space`, and `round` as
    /// repeated styles; only `no-repeat` suppresses additional tiles:
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
    pub(crate) fn repeats_y(self) -> bool {
        self.y_axis() != BackgroundRepeatAxis::NoRepeat
    }
}
