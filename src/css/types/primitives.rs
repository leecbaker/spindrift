use super::*;
use crate::units::{LayoutLength, LayoutSize, layout_pt};
use cssparser::{BasicParseErrorKind, Parser, ParserInput};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// A decoded cascade-layer path.
///
/// CSS layer names are a sequence of identifiers, not a serialized dot
/// string. Keeping the segments separate prevents an escaped literal dot in
/// an identifier from becoming indistinguishable from the layer separator.
/// <https://www.w3.org/TR/css-cascade-5/#typedef-layer-name>
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LayerName(pub(in crate::css) Vec<LayerSegment>);

impl LayerName {
    pub(in crate::css) fn nested(&self, child: Self) -> Self {
        let mut segments = self.0.clone();
        segments.extend(child.0);
        Self(segments)
    }

    pub(in crate::css) fn anonymous() -> Self {
        Self(vec![LayerSegment::Anonymous(AnonymousLayerId::next())])
    }

    pub(in crate::css) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One segment in a [`LayerName`]. Anonymous layers are deliberately not
/// representable by any CSS identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::css) enum LayerSegment {
    Named(String),
    Anonymous(AnonymousLayerId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::css) struct AnonymousLayerId(u64);

impl AnonymousLayerId {
    fn next() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// A lexicographic position in the cascade-layer tree.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LayerOrder(pub(in crate::css) Vec<usize>);

/// The output medium used when evaluating CSS Media Queries.
///
/// Media Queries Level 4 media types select an output category; PDF rendering
/// defaults to `print`, while callers that render a screen snapshot can select
/// `screen`: <https://www.w3.org/TR/mediaqueries-4/#media-types>.
///
/// ```no_run
/// use quire::{Html, MediaType, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let mut render_options = RenderOptions::default();
/// render_options.media_type = MediaType::Screen;
/// let mut output = File::create("document.pdf")?;
/// Html::from_file("document.html")
///     .await?
///     .write_pdf(&mut output, &render_options, &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MediaType {
    /// A paged or print rendering medium.
    #[default]
    Print,
    /// A screen rendering medium.
    Screen,
}

/// CSS system colors supplied by the rendering environment.
///
/// CSS CssColor Adjustment requires an active forced-colors environment to make
/// its limited palette available through CSS system color keywords:
/// <https://www.w3.org/TR/css-color-adjust-1/#forced-colors-mode>.
///
/// ```no_run
/// use quire::{
///     ForcedColorPalette, ForcedColorsMode, Html, PdfOptions, RenderOptions,
/// };
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let mut render_options = RenderOptions::default();
/// render_options.forced_colors = ForcedColorsMode::Active(ForcedColorPalette::DARK);
/// let mut output = File::create("document.pdf")?;
/// Html::from_file("document.html")
///     .await?
///     .write_pdf(&mut output, &render_options, &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForcedColorPalette {
    /// `Canvas`.
    pub canvas: CssColor,
    /// `CanvasText`.
    pub canvas_text: CssColor,
    /// `LinkText`.
    pub link_text: CssColor,
    /// `VisitedText`.
    pub visited_text: CssColor,
    /// `ActiveText`.
    pub active_text: CssColor,
    /// `ButtonFace`.
    pub button_face: CssColor,
    /// `ButtonText`.
    pub button_text: CssColor,
    /// `ButtonBorder`.
    pub button_border: CssColor,
    /// `Field`.
    pub field: CssColor,
    /// `FieldText`.
    pub field_text: CssColor,
    /// `Highlight`.
    pub highlight: CssColor,
    /// `HighlightText`.
    pub highlight_text: CssColor,
    /// `Mark`.
    pub mark: CssColor,
    /// `MarkText`.
    pub mark_text: CssColor,
    /// `GrayText`.
    pub gray_text: CssColor,
    /// `AccentColor`.
    pub accent_color: CssColor,
    /// `AccentColorText`.
    pub accent_color_text: CssColor,
    /// `SelectedItem`.
    pub selected_item: CssColor,
    /// `SelectedItemText`.
    pub selected_item_text: CssColor,
}

impl ForcedColorPalette {
    /// Deterministic light high-contrast palette used by the command line and
    /// the WPT runner.
    pub const LIGHT: Self = Self {
        canvas: CssColor::WHITE,
        canvas_text: CssColor::BLACK,
        link_text: CssColor::BLACK,
        visited_text: CssColor::BLACK,
        active_text: CssColor::BLACK,
        button_face: CssColor::WHITE,
        button_text: CssColor::BLACK,
        button_border: CssColor::srgb_const(0.5, 0.5, 0.5, 1.0),
        field: CssColor::WHITE,
        field_text: CssColor::BLACK,
        highlight: CssColor::WHITE,
        highlight_text: CssColor::BLACK,
        mark: CssColor::WHITE,
        mark_text: CssColor::BLACK,
        gray_text: CssColor::BLACK,
        accent_color: CssColor::WHITE,
        accent_color_text: CssColor::BLACK,
        selected_item: CssColor::WHITE,
        selected_item_text: CssColor::BLACK,
    };

    /// Deterministic dark high-contrast palette.
    pub const DARK: Self = Self {
        canvas: CssColor::BLACK,
        canvas_text: CssColor::WHITE,
        link_text: CssColor::WHITE,
        visited_text: CssColor::WHITE,
        active_text: CssColor::WHITE,
        button_face: CssColor::BLACK,
        button_text: CssColor::WHITE,
        button_border: CssColor::srgb_const(0.5, 0.5, 0.5, 1.0),
        field: CssColor::BLACK,
        field_text: CssColor::WHITE,
        highlight: CssColor::BLACK,
        highlight_text: CssColor::WHITE,
        mark: CssColor::BLACK,
        mark_text: CssColor::WHITE,
        gray_text: CssColor::WHITE,
        accent_color: CssColor::BLACK,
        accent_color_text: CssColor::WHITE,
        selected_item: CssColor::BLACK,
        selected_item_text: CssColor::WHITE,
    };
}

/// Whether CSS forced-colors mode is active for this render.
///
/// ```no_run
/// use quire::{
///     ForcedColorPalette, ForcedColorsMode, Html, PdfOptions, RenderOptions,
/// };
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let mut render_options = RenderOptions::default();
/// render_options.forced_colors = ForcedColorsMode::Active(ForcedColorPalette::LIGHT);
/// let mut output = File::create("document.pdf")?;
/// Html::from_file("document.html")
///     .await?
///     .write_pdf(&mut output, &render_options, &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[allow(clippy::large_enum_variant)] // Public Copy render option avoids per-style indirection.
pub enum ForcedColorsMode {
    /// Preserve authors' colors.
    #[default]
    Inactive,
    /// Force used colors through the supplied system-color palette.
    Active(ForcedColorPalette),
}

/// Canonical CSS system-color keyword.
///
/// Legacy CSS CssColor aliases are normalized by the parser to one of these
/// values before the forced-colors used-value stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SystemColor {
    Canvas,
    CanvasText,
    LinkText,
    VisitedText,
    ActiveText,
    ButtonFace,
    ButtonText,
    ButtonBorder,
    Field,
    FieldText,
    Highlight,
    HighlightText,
    Mark,
    MarkText,
    GrayText,
    AccentColor,
    AccentColorText,
    SelectedItem,
    SelectedItemText,
}

impl ForcedColorPalette {
    pub(crate) const fn color(self, system: SystemColor) -> CssColor {
        match system {
            SystemColor::Canvas => self.canvas,
            SystemColor::CanvasText => self.canvas_text,
            SystemColor::LinkText => self.link_text,
            SystemColor::VisitedText => self.visited_text,
            SystemColor::ActiveText => self.active_text,
            SystemColor::ButtonFace => self.button_face,
            SystemColor::ButtonText => self.button_text,
            SystemColor::ButtonBorder => self.button_border,
            SystemColor::Field => self.field,
            SystemColor::FieldText => self.field_text,
            SystemColor::Highlight => self.highlight,
            SystemColor::HighlightText => self.highlight_text,
            SystemColor::Mark => self.mark,
            SystemColor::MarkText => self.mark_text,
            SystemColor::GrayText => self.gray_text,
            SystemColor::AccentColor => self.accent_color,
            SystemColor::AccentColorText => self.accent_color_text,
            SystemColor::SelectedItem => self.selected_item,
            SystemColor::SelectedItemText => self.selected_item_text,
        }
    }
}

impl ForcedColorsMode {
    /// Whether forced colors are enabled.
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active(_))
    }

    /// Return the active palette, if any.
    pub const fn palette(self) -> Option<ForcedColorPalette> {
        match self {
            Self::Inactive => None,
            Self::Active(palette) => Some(palette),
        }
    }
}

/// Author control over forced-color used-value substitution.
///
/// <https://www.w3.org/TR/css-color-adjust-1/#forced-color-adjust-prop>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ForcedColorAdjust {
    #[default]
    Auto,
    None,
    PreserveParentColor,
}

/// CSS-pixel viewport coordinates used only by media-query evaluation.
/// They are deliberately distinct from PDF-point layout viewport lengths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssViewportSpace {}

/// A viewport size expressed in CSS pixels.
///
/// This type is used to construct a [`MediaEnvironment`]. PDF rendering derives
/// its viewport from the initial page box, which author CSS may replace through
/// the `@page` `size` descriptor.
///
/// ```
/// let viewport = quire::CssViewportSize::new(800.0, 600.0);
/// assert_eq!(viewport.width, 800.0);
/// ```
pub type CssViewportSize = euclid::Size2D<f32, CssViewportSpace>;

/// Physical and logical viewport bases for resolving CSS viewport units.
/// Physical dimensions are layout points; media-query CSS pixels use the
/// separate [`CssViewportSize`] type above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewportLengthBasis {
    physical: LayoutSize,
    writing_mode: WritingMode,
    /// Layout-time container-unit basis. When absent, container-relative
    /// lengths use the required small-viewport fallback.
    container_physical: Option<LayoutSize>,
}

impl ViewportLengthBasis {
    pub(crate) fn for_writing_mode(physical: LayoutSize, writing_mode: WritingMode) -> Self {
        Self {
            physical,
            writing_mode,
            container_physical: None,
        }
    }

    /// Supply the physical query-container axes selected during layout.
    ///
    /// The current element's writing mode remains the projection context for
    /// `cqi`/`cqb`; only the independently selected physical width and height
    /// bases come from ancestor query containers.
    /// <https://drafts.csswg.org/css-conditional-5/#container-lengths>
    pub(crate) fn with_container_physical(mut self, physical: LayoutSize) -> Self {
        self.container_physical = Some(physical);
        self
    }

    pub(crate) fn vw(self, percentage: f32) -> LayoutLength {
        layout_pt(percentage * self.physical.width / 100.0)
    }

    pub(crate) fn vh(self, percentage: f32) -> LayoutLength {
        layout_pt(percentage * self.physical.height / 100.0)
    }

    pub(crate) fn vmin(self, percentage: f32) -> LayoutLength {
        layout_pt(percentage * self.physical.width.min(self.physical.height) / 100.0)
    }

    pub(crate) fn vmax(self, percentage: f32) -> LayoutLength {
        layout_pt(percentage * self.physical.width.max(self.physical.height) / 100.0)
    }

    pub(crate) fn vi(self, percentage: f32) -> LayoutLength {
        match self.writing_mode {
            WritingMode::HorizontalTb => self.vw(percentage),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self.vh(percentage),
        }
    }

    pub(crate) fn vb(self, percentage: f32) -> LayoutLength {
        match self.writing_mode {
            WritingMode::HorizontalTb => self.vh(percentage),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self.vw(percentage),
        }
    }

    /// CSS container units with no eligible container fall back to the small
    /// viewport. Quire's fixed paged viewport makes that the active page area.
    /// <https://www.w3.org/TR/css-contain-3/#container-lengths>
    pub(crate) fn container_fallback(self) -> ContainerLengthBasis {
        ContainerLengthBasis::for_writing_mode(
            self.container_physical.unwrap_or(self.physical),
            self.writing_mode,
        )
    }
}

/// Physical and logical bases for CSS container-relative length units.
///
/// Selection of the eligible ancestor is a layout concern. Once selected, the
/// unit projection is purely a value operation and therefore stays typed here,
/// alongside the viewport equivalent.
/// <https://www.w3.org/TR/css-contain-3/#container-lengths>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContainerLengthBasis {
    physical: LayoutSize,
    writing_mode: WritingMode,
}

impl ContainerLengthBasis {
    pub(crate) fn for_writing_mode(physical: LayoutSize, writing_mode: WritingMode) -> Self {
        Self {
            physical,
            writing_mode,
        }
    }

    pub(crate) fn cqw(self, percentage: f32) -> LayoutLength {
        layout_pt(percentage * self.physical.width / 100.0)
    }

    pub(crate) fn cqh(self, percentage: f32) -> LayoutLength {
        layout_pt(percentage * self.physical.height / 100.0)
    }

    pub(crate) fn cqi(self, percentage: f32) -> LayoutLength {
        match self.writing_mode {
            WritingMode::HorizontalTb => self.cqw(percentage),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self.cqh(percentage),
        }
    }

    pub(crate) fn cqb(self, percentage: f32) -> LayoutLength {
        match self.writing_mode {
            WritingMode::HorizontalTb => self.cqh(percentage),
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr => self.cqw(percentage),
        }
    }
}

/// Used parent-font metrics for resolving font-relative `font-size` terms.
///
/// CSS Values resolves `em` against the element's used font size and `ch`
/// against the selected font's zero-glyph advance:
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FontRelativeLengthBasis {
    font_size: LayoutLength,
    ch_advance: LayoutLength,
    x_height: LayoutLength,
    cap_height: LayoutLength,
    ic_advance: LayoutLength,
    line_height: LayoutLength,
}

impl FontRelativeLengthBasis {
    pub(crate) fn new(font_size: LayoutLength, ch_advance: LayoutLength) -> Self {
        Self {
            font_size,
            ch_advance,
            // CSS Values defines these fallbacks when the parent selected
            // font has no corresponding metric.
            x_height: layout_pt(font_size.points() * 0.5),
            cap_height: layout_pt(font_size.points() * 0.7),
            ic_advance: font_size,
            line_height: layout_pt(font_size.points() * 1.2),
        }
    }

    /// Replaces fallback metrics with the parent selected font's used metrics.
    pub(crate) const fn with_selected_font_metrics(
        mut self,
        x_height: LayoutLength,
        cap_height: LayoutLength,
        ic_advance: LayoutLength,
    ) -> Self {
        self.x_height = x_height;
        self.cap_height = cap_height;
        self.ic_advance = ic_advance;
        self
    }

    /// Replaces the parent's fallback zero-glyph advance when a descendant
    /// needs its selected-font `ch` metric.
    pub(crate) const fn with_ch_advance(mut self, ch_advance: LayoutLength) -> Self {
        self.ch_advance = ch_advance;
        self
    }

    /// Records the computed line-height used by parent-relative `lh` terms
    /// in font-affecting properties.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) const fn with_line_height(mut self, line_height: LayoutLength) -> Self {
        self.line_height = line_height;
        self
    }

    pub(crate) const fn font_size(self) -> LayoutLength {
        self.font_size
    }

    pub(crate) const fn ch_advance(self) -> LayoutLength {
        self.ch_advance
    }

    pub(crate) const fn line_height(self) -> LayoutLength {
        self.line_height
    }

    pub(crate) const fn x_height(self) -> LayoutLength {
        self.x_height
    }

    pub(crate) const fn cap_height(self) -> LayoutLength {
        self.cap_height
    }

    pub(crate) const fn ic_advance(self) -> LayoutLength {
        self.ic_advance
    }
}

/// Used root-font metrics for CSS Values root-relative metric units.
///
/// The root-relative metric units are intentionally distinct from the
/// element-font basis: the selected root font, its writing mode, and its
/// computed line height remain the basis even in an orthogonal descendant.
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RootFontMetricLengthBasis {
    pub(crate) font_size: LayoutLength,
    pub(crate) ch_advance: LayoutLength,
    pub(crate) x_height: LayoutLength,
    pub(crate) cap_height: LayoutLength,
    pub(crate) ic_advance: LayoutLength,
    pub(crate) line_height: LayoutLength,
}

/// Static capabilities used to evaluate CSS Media Queries for one rendering.
///
/// Viewport dimensions are CSS pixels. They are render inputs, rather than
/// computed style values, because media conditions must be evaluated before
/// their declarations enter the cascade:
/// <https://www.w3.org/TR/mediaqueries-4/#media-features>.
///
/// `Html::render` derives this environment from [`crate::RenderOptions`], so
/// applications normally configure the public render options directly.
///
/// ```
/// use quire::{CssViewportSize, MediaEnvironment, MediaType};
///
/// let environment = MediaEnvironment::new(
///     MediaType::Screen,
///     CssViewportSize::new(1280.0, 720.0),
/// );
/// assert_eq!(environment.media_type, MediaType::Screen);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaEnvironment {
    /// Rendering target selected for media queries.
    pub media_type: MediaType,
    /// Immutable viewport dimensions used by media queries.
    pub viewport: CssViewportSize,
    /// Device resolution in dots per CSS pixel.
    pub resolution_dppx: f32,
    /// Forced-colors rendering state.
    pub forced_colors: ForcedColorsMode,
    /// The user's preferred color scheme. This is distinct from the UA's
    /// fallback scheme when the author has not declared support.
    pub color_scheme_preference: ColorSchemePreference,
}

/// User preference consulted when resolving CSS Color Adjustment's used color
/// scheme. `None` is intentionally the default: in that case the first
/// author-supported scheme wins.
/// <https://www.w3.org/TR/css-color-adjust-1/#color-scheme-preference>
///
/// ```no_run
/// use quire::{ColorSchemePreference, Html, PdfOptions, RenderOptions};
/// use std::fs::File;
///
/// # async fn render() -> quire::Result<()> {
/// let mut render_options = RenderOptions::default();
/// render_options.color_scheme_preference = ColorSchemePreference::Dark;
/// let mut output = File::create("document.pdf")?;
/// Html::from_file("document.html")
///     .await?
///     .write_pdf(&mut output, &render_options, &PdfOptions::default())
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorSchemePreference {
    /// The user has not expressed a preference; the first supported scheme wins.
    #[default]
    None,
    /// Prefer a light scheme when the author supports it.
    Light,
    /// Prefer a dark scheme when the author supports it.
    Dark,
    /// Require light unless the author explicitly used `only`.
    OverrideLight,
    /// Require dark unless the author explicitly used `only`.
    OverrideDark,
}

/// One of the color schemes whose rendering behavior Quire implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum UsedColorScheme {
    Light,
    Dark,
}

impl ColorSchemePreference {
    pub(crate) const fn preferred(self) -> Option<UsedColorScheme> {
        match self {
            Self::None => None,
            Self::Light | Self::OverrideLight => Some(UsedColorScheme::Light),
            Self::Dark | Self::OverrideDark => Some(UsedColorScheme::Dark),
        }
    }

    pub(crate) const fn is_override(self) -> bool {
        matches!(self, Self::OverrideLight | Self::OverrideDark)
    }

    /// The value exposed by the `prefers-color-scheme` media feature.
    ///
    /// Media Queries intentionally exposes the ordinary default as `light`,
    /// while `None` remains distinct for CSS Color Adjustment's used-scheme
    /// selection, where the first author-supported scheme wins.
    /// <https://www.w3.org/TR/mediaqueries-5/#prefers-color-scheme>
    pub(crate) const fn media_query_scheme(self) -> UsedColorScheme {
        match self {
            Self::Dark | Self::OverrideDark => UsedColorScheme::Dark,
            Self::None | Self::Light | Self::OverrideLight => UsedColorScheme::Light,
        }
    }
}

impl MediaEnvironment {
    /// Creates a media-query environment with no color-scheme preference.
    pub const fn new(media_type: MediaType, viewport: CssViewportSize) -> Self {
        Self {
            media_type,
            viewport,
            resolution_dppx: 1.0,
            forced_colors: ForcedColorsMode::Inactive,
            color_scheme_preference: ColorSchemePreference::None,
        }
    }

    /// Sets the device density exposed to resolution media queries.
    pub const fn with_resolution_dppx(mut self, resolution_dppx: f32) -> Self {
        self.resolution_dppx = resolution_dppx;
        self
    }

    /// Sets the forced-colors mode exposed to CSS Color Adjustment.
    pub const fn with_forced_colors(mut self, forced_colors: ForcedColorsMode) -> Self {
        self.forced_colors = forced_colors;
        self
    }

    /// Sets the user preference used for `color-scheme` and `light-dark()`.
    pub const fn with_color_scheme_preference(
        mut self,
        color_scheme_preference: ColorSchemePreference,
    ) -> Self {
        self.color_scheme_preference = color_scheme_preference;
        self
    }
}

impl Default for MediaEnvironment {
    fn default() -> Self {
        // CSS's initial A4 page box in Quire's default print environment.
        Self::new(MediaType::Print, CssViewportSize::new(793.7008, 1122.5197))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_length_basis_keeps_physical_and_logical_axes_distinct() {
        let physical = LayoutSize::new(300.0, 200.0);
        let horizontal = ViewportLengthBasis::for_writing_mode(physical, WritingMode::HorizontalTb);
        let vertical = ViewportLengthBasis::for_writing_mode(physical, WritingMode::VerticalRl);
        let sideways = ViewportLengthBasis::for_writing_mode(physical, WritingMode::SidewaysLr);

        assert_eq!(horizontal.vw(100.0), layout_pt(300.0));
        assert_eq!(horizontal.vh(100.0), layout_pt(200.0));
        assert_eq!(horizontal.vmin(100.0), layout_pt(200.0));
        assert_eq!(horizontal.vmax(100.0), layout_pt(300.0));
        assert_eq!(horizontal.vi(100.0), layout_pt(300.0));
        assert_eq!(horizontal.vb(100.0), layout_pt(200.0));
        assert_eq!(vertical.vi(100.0), layout_pt(200.0));
        assert_eq!(vertical.vb(100.0), layout_pt(300.0));
        assert_eq!(sideways.vi(100.0), layout_pt(200.0));
        assert_eq!(sideways.vb(100.0), layout_pt(300.0));
    }

    #[test]
    fn color_scheme_preference_builder_preserves_preference_and_override() {
        let light = ColorSchemePreference::Light;
        let dark = ColorSchemePreference::Dark;
        let override_light = ColorSchemePreference::OverrideLight;
        let override_dark = ColorSchemePreference::OverrideDark;

        assert_eq!(light.preferred(), Some(UsedColorScheme::Light));
        assert_eq!(dark.preferred(), Some(UsedColorScheme::Dark));
        assert_eq!(override_light.preferred(), Some(UsedColorScheme::Light));
        assert_eq!(override_dark.preferred(), Some(UsedColorScheme::Dark));
        assert!(!light.is_override());
        assert!(!dark.is_override());
        assert!(override_light.is_override());
        assert!(override_dark.is_override());

        let environment =
            MediaEnvironment::new(MediaType::Screen, CssViewportSize::new(800.0, 600.0))
                .with_color_scheme_preference(override_dark);
        assert_eq!(environment.color_scheme_preference, override_dark);
    }
}

/// A predefined CSS RGB component space.
///
/// CSS CssColor 4 keeps colors in their specified space until they are used by a
/// physical output device.  In particular, an out-of-sRGB Display-P3 color
/// must not be clipped merely because Quire's layout engine is not itself a
/// display device.  See <https://www.w3.org/TR/css-color-4/#color-conversion>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum RgbColorSpace {
    Srgb,
    DisplayP3,
    A98Rgb,
    ProphotoRgb,
    Rec2020,
}

/// The component model carried by a CSS color.
///
/// A D50 XYZ profile-connection-space value must not be mistaken for RGB
/// channels merely because both have three scalar components. CSS CssColor 4
/// conversion preserves this distinction until a concrete output boundary.
/// <https://www.w3.org/TR/css-color-4/#color-conversion>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CssColorCoordinates {
    Srgb(EncodedRgb<Srgb>),
    DisplayP3(EncodedRgb<DisplayP3>),
    A98Rgb(EncodedRgb<A98Rgb>),
    ProphotoRgb(EncodedRgb<ProphotoRgb>),
    Rec2020(EncodedRgb<Rec2020>),
    XyzD50(D50Xyz),
}

/// Encoded coordinates in one predefined CSS RGB space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssRgbCoordinates {
    pub(crate) red: f32,
    pub(crate) green: f32,
    pub(crate) blue: f32,
}

const fn rgb_components(coordinates: CssRgbCoordinates) -> [f32; 3] {
    [coordinates.red, coordinates.green, coordinates.blue]
}

/// Encoded components tagged with their CSS predefined RGB space.
///
/// The marker is zero-sized but prevents `DisplayP3` coordinates from being
/// constructed as an `EncodedRgb<Srgb>` by accident.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EncodedRgb<Space> {
    coordinates: CssRgbCoordinates,
    marker: std::marker::PhantomData<Space>,
}

impl<Space> EncodedRgb<Space> {
    const fn new(red: f32, green: f32, blue: f32) -> Self {
        Self {
            coordinates: CssRgbCoordinates { red, green, blue },
            marker: std::marker::PhantomData,
        }
    }

    const fn coordinates(self) -> CssRgbCoordinates {
        self.coordinates
    }
}

// Private markers for CSS's distinct encoded RGB component spaces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Srgb;
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DisplayP3;
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct A98Rgb;
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ProphotoRgb;
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rec2020;

/// Device-independent CIE XYZ coordinates chromatically adapted to D50.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct D50Xyz {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
}

/// The CSS component space in which a [`CssColor`] stores its coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CssColorSpace {
    Srgb,
    DisplayP3,
    A98Rgb,
    ProphotoRgb,
    Rec2020,
    XyzD50,
}

impl CssColorSpace {
    /// Stable discriminant for document-local cache keys.
    pub(crate) const fn cache_key(self) -> u8 {
        self as u8
    }
}

/// A CSS color with independent alpha and color-space-tagged coordinates.
///
/// Coordinates retain their semantic model until an output boundary. Alpha is
/// always clamped as required by CSS CssColor 4.
///
/// <https://www.w3.org/TR/css-color-4/#alpha-value>
#[derive(Debug, Clone, Copy)]
pub struct CssColor {
    coordinates: CssColorCoordinates,
    alpha: CssAlpha,
    pub(crate) system: Option<SystemColor>,
}

/// CSS alpha is independent of the coordinate system and always normalized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssAlpha(f32);

impl CssAlpha {
    const fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    pub(crate) const fn value(self) -> f32 {
        self.0
    }
}

// The system-color marker is cascade metadata, not part of a color's paint
// value. In particular, collapsed-border conflict resolution must consider a
// resolved `CanvasText` equal to the same concrete palette color.
impl PartialEq for CssColor {
    fn eq(&self, other: &Self) -> bool {
        self.coordinates == other.coordinates && self.alpha == other.alpha
    }
}

impl CssColor {
    pub const BLACK: Self = Self {
        coordinates: CssColorCoordinates::Srgb(EncodedRgb::new(0.0, 0.0, 0.0)),
        alpha: CssAlpha::new(1.0),
        system: None,
    };
    pub const WHITE: Self = Self {
        coordinates: CssColorCoordinates::Srgb(EncodedRgb::new(1.0, 1.0, 1.0)),
        alpha: CssAlpha::new(1.0),
        system: None,
    };
    pub const TRANSPARENT: Self = Self {
        coordinates: CssColorCoordinates::Srgb(EncodedRgb::new(0.0, 0.0, 0.0)),
        alpha: CssAlpha::new(0.0),
        system: None,
    };

    const fn srgb_const(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            coordinates: CssColorCoordinates::Srgb(EncodedRgb::new(r, g, b)),
            alpha: CssAlpha::new(a),
            system: None,
        }
    }

    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 1.0)
    }

    /// Create an sRGB color with alpha.
    ///
    /// CSS CssColor Level 4 defines alpha as a number in `[0, 1]` after
    /// clamping:
    /// <https://www.w3.org/TR/css-color-4/#alpha-value>.
    pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            coordinates: CssColorCoordinates::Srgb(EncodedRgb::new(
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
            )),
            alpha: CssAlpha::new(a),
            system: None,
        }
    }

    /// Create an sRGB color from normalized CSS color components.
    ///
    /// CSS CssColor Level 4 defines `color(srgb ...)` components as numbers or
    /// percentages in the sRGB color space, with alpha clamped to `[0, 1]`:
    /// <https://www.w3.org/TR/css-color-4/#predefined-sRGB>.
    pub(crate) fn srgb(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            coordinates: CssColorCoordinates::Srgb(EncodedRgb::new(
                r.clamp(0.0, 1.0),
                g.clamp(0.0, 1.0),
                b.clamp(0.0, 1.0),
            )),
            alpha: CssAlpha::new(a),
            system: None,
        }
    }

    /// Create a color in a CSS CssColor 4 predefined RGB space, retaining
    /// out-of-gamut coordinates for the eventual output conversion.
    pub(crate) fn rgb(space: RgbColorSpace, red: f32, green: f32, blue: f32, a: f32) -> Self {
        Self {
            coordinates: match space {
                RgbColorSpace::Srgb => CssColorCoordinates::Srgb(EncodedRgb::new(red, green, blue)),
                RgbColorSpace::DisplayP3 => {
                    CssColorCoordinates::DisplayP3(EncodedRgb::new(red, green, blue))
                }
                RgbColorSpace::A98Rgb => {
                    CssColorCoordinates::A98Rgb(EncodedRgb::new(red, green, blue))
                }
                RgbColorSpace::ProphotoRgb => {
                    CssColorCoordinates::ProphotoRgb(EncodedRgb::new(red, green, blue))
                }
                RgbColorSpace::Rec2020 => {
                    CssColorCoordinates::Rec2020(EncodedRgb::new(red, green, blue))
                }
            },
            alpha: CssAlpha::new(a),
            system: None,
        }
    }

    /// Create a D50 XYZ profile-connection-space color.
    pub(crate) fn xyz_d50(x: f32, y: f32, z: f32, a: f32) -> Self {
        Self {
            coordinates: CssColorCoordinates::XyzD50(D50Xyz { x, y, z }),
            alpha: CssAlpha::new(a),
            system: None,
        }
    }

    /// Construct a color from legacy three-component callers while retaining
    /// the coordinate model selected by CSS CssColor.
    ///
    /// This compatibility boundary is deliberately centralized during the
    /// renderer-wide typed-coordinate migration: RGB spaces use the typed RGB
    /// constructor and the profile-connection space uses the distinct XYZ
    /// constructor, so callers cannot accidentally label XYZ coordinates as
    /// RGB channels.
    pub(crate) fn in_space(
        space: CssColorSpace,
        first: f32,
        second: f32,
        third: f32,
        a: f32,
    ) -> Self {
        match space {
            CssColorSpace::Srgb => Self::rgb(RgbColorSpace::Srgb, first, second, third, a),
            CssColorSpace::DisplayP3 => {
                Self::rgb(RgbColorSpace::DisplayP3, first, second, third, a)
            }
            CssColorSpace::A98Rgb => Self::rgb(RgbColorSpace::A98Rgb, first, second, third, a),
            CssColorSpace::ProphotoRgb => {
                Self::rgb(RgbColorSpace::ProphotoRgb, first, second, third, a)
            }
            CssColorSpace::Rec2020 => Self::rgb(RgbColorSpace::Rec2020, first, second, third, a),
            CssColorSpace::XyzD50 => Self::xyz_d50(first, second, third, a),
        }
    }

    pub(crate) const fn space(self) -> CssColorSpace {
        match self.coordinates {
            CssColorCoordinates::Srgb(_) => CssColorSpace::Srgb,
            CssColorCoordinates::DisplayP3(_) => CssColorSpace::DisplayP3,
            CssColorCoordinates::A98Rgb(_) => CssColorSpace::A98Rgb,
            CssColorCoordinates::ProphotoRgb(_) => CssColorSpace::ProphotoRgb,
            CssColorCoordinates::Rec2020(_) => CssColorSpace::Rec2020,
            CssColorCoordinates::XyzD50(_) => CssColorSpace::XyzD50,
        }
    }

    /// Return the three coordinates in this color's declared CSS space.
    ///
    /// Callers that need RGB semantics must first use `rgb_coordinates`; this
    /// view exists for generic interpolation and backend adapters only.
    pub(crate) const fn components(self) -> [f32; 3] {
        match self.coordinates {
            CssColorCoordinates::Srgb(c) => rgb_components(c.coordinates()),
            CssColorCoordinates::DisplayP3(c) => rgb_components(c.coordinates()),
            CssColorCoordinates::A98Rgb(c) => rgb_components(c.coordinates()),
            CssColorCoordinates::ProphotoRgb(c) => rgb_components(c.coordinates()),
            CssColorCoordinates::Rec2020(c) => rgb_components(c.coordinates()),
            CssColorCoordinates::XyzD50(c) => [c.x, c.y, c.z],
        }
    }

    pub(crate) const fn alpha(self) -> f32 {
        self.alpha.value()
    }

    /// Return RGB coordinates only when this color is in an RGB space.
    pub(crate) const fn rgb_coordinates(self) -> Option<(RgbColorSpace, CssRgbCoordinates)> {
        match self.coordinates {
            CssColorCoordinates::Srgb(coordinates) => {
                Some((RgbColorSpace::Srgb, coordinates.coordinates()))
            }
            CssColorCoordinates::DisplayP3(coordinates) => {
                Some((RgbColorSpace::DisplayP3, coordinates.coordinates()))
            }
            CssColorCoordinates::A98Rgb(coordinates) => {
                Some((RgbColorSpace::A98Rgb, coordinates.coordinates()))
            }
            CssColorCoordinates::ProphotoRgb(coordinates) => {
                Some((RgbColorSpace::ProphotoRgb, coordinates.coordinates()))
            }
            CssColorCoordinates::Rec2020(coordinates) => {
                Some((RgbColorSpace::Rec2020, coordinates.coordinates()))
            }
            CssColorCoordinates::XyzD50(_) => None,
        }
    }

    /// Return D50 XYZ coordinates only when this color is in the PCS.
    pub(crate) const fn xyz_d50_coordinates(self) -> Option<D50Xyz> {
        match self.coordinates {
            CssColorCoordinates::Srgb(_)
            | CssColorCoordinates::DisplayP3(_)
            | CssColorCoordinates::A98Rgb(_)
            | CssColorCoordinates::ProphotoRgb(_)
            | CssColorCoordinates::Rec2020(_) => None,
            CssColorCoordinates::XyzD50(coordinates) => Some(coordinates),
        }
    }

    /// Convert this computed CSS color to its D50 profile-connection-space
    /// coordinates without clipping or output encoding.
    pub(crate) fn to_xyz_d50(self) -> D50Xyz {
        self.xyz_d50_coordinates().unwrap_or_else(|| {
            crate::css::color_to_xyz_d50(self)
                .xyz_d50_coordinates()
                .expect("CSS D50 conversion must produce PCS coordinates")
        })
    }

    /// Convert this computed CSS color to a requested predefined RGB space
    /// without clipping or quantization. The result remains a CSS color.
    pub(crate) fn to_rgb_space(self, space: RgbColorSpace) -> Self {
        let target = match space {
            RgbColorSpace::Srgb => CssColorSpace::Srgb,
            RgbColorSpace::DisplayP3 => CssColorSpace::DisplayP3,
            RgbColorSpace::A98Rgb => CssColorSpace::A98Rgb,
            RgbColorSpace::ProphotoRgb => CssColorSpace::ProphotoRgb,
            RgbColorSpace::Rec2020 => CssColorSpace::Rec2020,
        };
        crate::css::color_to_predefined_rgb(self, target)
            .expect("a predefined CSS RGB target must be convertible")
    }

    pub(crate) const fn system(system: SystemColor, color: Self) -> Self {
        Self {
            system: Some(system),
            ..color
        }
    }

    pub(crate) const fn system_color(self) -> Option<SystemColor> {
        self.system
    }

    pub(crate) fn with_alpha(self, alpha: f32) -> Self {
        Self {
            alpha: CssAlpha::new(alpha),
            ..self
        }
    }

    pub(crate) fn is_visible(self) -> bool {
        self.alpha.value() > 0.0
    }

    pub(crate) fn is_opaque(self) -> bool {
        (self.alpha.value() - 1.0).abs() < 0.001
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Stylesheet {
    pub origin: StylesheetOrigin,
    /// URL context inherited by declarations synthesized from this stylesheet.
    ///
    /// HTML presentational hints are generated during cascade, after parsing,
    /// but URL-valued hints such as a legacy table `background` attribute must
    /// resolve exactly like declarations in the owning document stylesheet.
    pub base_url: Option<url::Url>,
    pub root_url: Option<url::Url>,
    /// The renderer color-adjustment environment used while parsing this
    /// stylesheet. Cascade-time used-value adjustment reads the same immutable
    /// input as its nested `@media` rules.
    pub forced_colors: ForcedColorsMode,
    pub color_scheme_preference: ColorSchemePreference,
    /// Whether this is Quire's built-in HTML presentational-hints sheet.
    ///
    /// Static selector-expressible hints live in the stylesheet itself, while
    /// value-dependent hints are injected during element cascade with the same
    /// author-origin, zero-specificity priority:
    /// <https://html.spec.whatwg.org/multipage/rendering.html#presentational-hints>.
    pub html_presentational_hints: bool,
    /// Optional specificity used for all style rules in this stylesheet.
    ///
    /// HTML presentational hints are author-origin declarations with zero
    /// specificity, regardless of the selector syntax used to find matching
    /// elements:
    /// <https://html.spec.whatwg.org/multipage/rendering.html#presentational-hints>
    /// and <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
    pub specificity_override: Option<u32>,
    /// Cascade layer names in first-declared order for this stylesheet.
    ///
    /// CSS Cascade Level 5 defines layer ordering by first declaration, with
    /// unlayered normal declarations ordered after all layered declarations:
    /// <https://www.w3.org/TR/css-cascade-5/#layer-order>.
    pub layer_names: Vec<LayerName>,
    /// Prefix bindings declared by CSS `@namespace` rules.
    ///
    /// Selector parsing consumes these bindings immediately, but declaration
    /// values such as `attr(prefix|name)` also need the binding during
    /// computed-value resolution:
    /// <https://www.w3.org/TR/css-namespaces-3/#declaration> and
    /// <https://drafts.csswg.org/css-values-5/#attr-notation>.
    pub namespace_prefixes: HashMap<String, String>,
    pub rules: Vec<StyleRule>,
    /// Rules retained from CSS size-container `@container` at-rules. They are
    /// kept separate because matching depends on layout-time ancestor sizes.
    /// <https://www.w3.org/TR/css-contain-3/#container-queries>
    #[allow(
        dead_code,
        reason = "container-query matching is retained for layout-time implementation"
    )]
    pub container_rules: Vec<ContainerRule>,
    pub keyframes: Vec<KeyframesRule>,
    pub marker_rules: Vec<StyleRule>,
    pub before_marker_rules: Vec<StyleRule>,
    pub after_marker_rules: Vec<StyleRule>,
    pub before_rules: Vec<StyleRule>,
    pub after_rules: Vec<StyleRule>,
    pub footnote_call_rules: Vec<StyleRule>,
    pub footnote_marker_rules: Vec<StyleRule>,
    pub first_line_rules: Vec<StyleRule>,
    pub first_letter_rules: Vec<StyleRule>,
    pub page_rules: Vec<PageRule>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub page_declarations: Declarations,
    pub first_page_declarations: Declarations,
    pub font_faces: Vec<CssFontFace>,
    pub font_feature_values: FontFeatureValues,
    pub font_palette_values: FontPaletteValues,
    pub counter_styles: Vec<CounterStyleRule>,
    /// Active `@property` rules in source order for this stylesheet.
    pub property_registrations: Vec<PropertyRegistrationRule>,
}

/// Parsed legacy body margins supplied by the immediate container frame.
///
/// The HTML rendering rules use these only after the child body's own
/// `marginwidth`/`marginheight` and `leftmargin`/`topmargin` attributes have
/// been considered. Keeping the axes paired prevents a frame's horizontal
/// hint from being mistaken for a vertical one at the cascade boundary:
/// <https://html.spec.whatwg.org/multipage/rendering.html#the-page>.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HtmlContainerFrameBodyMargins {
    pub(crate) horizontal: Option<i32>,
    pub(crate) vertical: Option<i32>,
}

impl HtmlContainerFrameBodyMargins {
    pub(crate) fn from_iframe_attributes(attrs: &HashMap<String, String>) -> Self {
        Self {
            horizontal: attrs
                .get("marginwidth")
                .and_then(|value| parse_html_non_negative_integer(value)),
            vertical: attrs
                .get("marginheight")
                .and_then(|value| parse_html_non_negative_integer(value)),
        }
    }
}

/// Parse HTML's non-negative-integer microsyntax.
///
/// This follows HTML's integer parser, including leading ASCII whitespace,
/// optional `+`, and trailing unparsed input. The resulting invariant is used
/// by presentational hints that map a legacy integer to a CSS pixel length:
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-non-negative-integers>.
pub(crate) fn parse_html_non_negative_integer(value: &str) -> Option<i32> {
    let value = value.trim_start_matches(is_html_space);
    let (sign, rest) = match value.as_bytes().first().copied() {
        Some(b'+') => (1, &value[1..]),
        Some(b'-') => (-1, &value[1..]),
        _ => (1, value),
    };
    let digit_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digit_len == 0 {
        return None;
    }
    let integer = rest[..digit_len].parse::<i32>().ok()? * sign;
    (integer >= 0).then_some(integer)
}

fn is_html_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\u{0c}' | '\r')
}

/// Ordered stylesheets used to cascade one document.
///
/// The HTML user-agent stylesheets are process-wide immutable data, whereas
/// presentational hints and author stylesheets belong to one document. Keeping
/// the two storage classes distinct lets layout borrow the built-in sheets
/// without cloning their parsed selector and declaration trees.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stylesheets<'a> {
    user_agent: Option<&'static Stylesheet>,
    html_important: Option<&'static Stylesheet>,
    document: &'a [Stylesheet],
    /// Legacy physical body margins supplied by the immediate embedding frame.
    ///
    /// HTML's rendering rules allow a child document's body to fall back to
    /// its container frame's `marginwidth` and `marginheight` attributes.
    /// This is document-scoped cascade input rather than an iframe layout
    /// metric: the values become zero-specificity author-origin declarations
    /// only when cascading the child HTML body.
    /// <https://html.spec.whatwg.org/multipage/rendering.html#the-page>
    html_container_frame_body_margins: Option<HtmlContainerFrameBodyMargins>,
    /// Static-rendering input used for CSS Images `image-set()` negotiation.
    image_set_resolution_dppx: f32,
    // Direct cascade tests occasionally model a user-origin sheet without
    // manufacturing an owned document stylesheet. Keep that test fixture out
    // of production layout state.
    #[cfg(test)]
    borrowed: &'a [&'a Stylesheet],
}

pub(crate) static EMPTY_STYLESHEETS: Stylesheets<'static> = Stylesheets::document_only(&[]);

/// Inputs that can be viewed as an ordered stylesheet collection.
///
/// Layout carries [`Stylesheets`] so the process-wide UA sheets stay borrowed.
/// This trait keeps low-level unit tests and isolated CSS callers ergonomic when
/// they intentionally provide only document-owned stylesheets.
pub(crate) trait StylesheetCollection {
    fn stylesheet_view(&self) -> Stylesheets<'_>;
}

impl StylesheetCollection for Stylesheets<'_> {
    fn stylesheet_view(&self) -> Stylesheets<'_> {
        *self
    }
}

impl StylesheetCollection for [Stylesheet] {
    fn stylesheet_view(&self) -> Stylesheets<'_> {
        Stylesheets::document_only(self)
    }
}

impl StylesheetCollection for Vec<Stylesheet> {
    fn stylesheet_view(&self) -> Stylesheets<'_> {
        Stylesheets::document_only(self)
    }
}

impl<const LENGTH: usize> StylesheetCollection for [Stylesheet; LENGTH] {
    fn stylesheet_view(&self) -> Stylesheets<'_> {
        Stylesheets::document_only(self)
    }
}

impl<'a> Stylesheets<'a> {
    pub(crate) const fn document_only(document: &'a [Stylesheet]) -> Self {
        Self {
            user_agent: None,
            html_important: None,
            document,
            html_container_frame_body_margins: None,
            image_set_resolution_dppx: 1.0,
            #[cfg(test)]
            borrowed: &[],
        }
    }

    pub(crate) const fn for_document(
        user_agent: &'static Stylesheet,
        html_important: Option<&'static Stylesheet>,
        document: &'a [Stylesheet],
    ) -> Self {
        Self {
            user_agent: Some(user_agent),
            html_important,
            document,
            html_container_frame_body_margins: None,
            image_set_resolution_dppx: 1.0,
            #[cfg(test)]
            borrowed: &[],
        }
    }

    #[cfg(test)]
    pub(crate) const fn borrowed(stylesheets: &'a [&'a Stylesheet]) -> Self {
        Self {
            user_agent: None,
            html_important: None,
            document: &[],
            html_container_frame_body_margins: None,
            image_set_resolution_dppx: 1.0,
            borrowed: stylesheets,
        }
    }

    /// Attach the immediate embedding frame's legacy body-margin values.
    ///
    /// The context is deliberately copied with the stylesheet view, so a
    /// nested document cannot accidentally observe its grandparent frame.
    pub(crate) const fn with_html_container_frame_body_margins(
        mut self,
        margins: Option<HtmlContainerFrameBodyMargins>,
    ) -> Self {
        self.html_container_frame_body_margins = margins;
        self
    }

    /// Attach the static device density used to choose image-set candidates.
    pub(crate) const fn with_image_set_resolution_dppx(mut self, resolution_dppx: f32) -> Self {
        self.image_set_resolution_dppx = resolution_dppx;
        self
    }

    pub(crate) const fn image_set_resolution_dppx(self) -> f32 {
        self.image_set_resolution_dppx
    }

    pub(crate) const fn html_container_frame_body_margins(
        self,
    ) -> Option<HtmlContainerFrameBodyMargins> {
        self.html_container_frame_body_margins
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &Stylesheet> + Clone {
        let stylesheets = self
            .user_agent
            .iter()
            .copied()
            .chain(self.html_important.iter().copied())
            .chain(self.document.iter());
        #[cfg(test)]
        {
            stylesheets.chain(self.borrowed.iter().copied())
        }
        #[cfg(not(test))]
        {
            stylesheets
        }
    }

    pub(crate) const fn len(&self) -> usize {
        let len = self.user_agent.is_some() as usize
            + self.html_important.is_some() as usize
            + self.document.len();
        #[cfg(not(test))]
        {
            len
        }
        #[cfg(test)]
        {
            len + self.borrowed.len()
        }
    }

    pub(crate) fn get(&self, index: usize) -> Option<&Stylesheet> {
        self.iter().nth(index)
    }

    pub(crate) fn color_scheme_preference(self) -> ColorSchemePreference {
        self.iter()
            .rev()
            .find_map(|stylesheet| {
                (stylesheet.color_scheme_preference != ColorSchemePreference::None)
                    .then_some(stylesheet.color_scheme_preference)
            })
            .unwrap_or(ColorSchemePreference::None)
    }

    pub(crate) fn registered_custom_properties(self) -> RegisteredCustomProperties {
        RegisteredCustomProperties::from_rules(self.iter())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum StylesheetOrigin {
    UserAgent,
    User,
    Author,
}

#[derive(Debug, Clone)]
pub(crate) struct PageRule {
    pub origin: StylesheetOrigin,
    pub selectors: Vec<PageSelector>,
    pub declarations: Declarations,
    pub margin_boxes: HashMap<String, Declarations>,
    /// Cascaded declarations for GCPM's page-footnote area, if this page rule
    /// defines one:
    /// <https://www.w3.org/TR/css-gcpm-3/#footnote-area>.
    pub footnote_area: Option<Declarations>,
    pub order: usize,
    /// Resolved cascade layer order for this page rule, if declared in `@layer`.
    ///
    /// CSS Paged Media delegates page-context cascading to normal cascade
    /// mechanics plus page-selector specificity, and CSS Cascade Level 5 adds
    /// cascade layers to that ordering:
    /// <https://www.w3.org/TR/css-page-3/#cascading-and-page-context> and
    /// <https://www.w3.org/TR/css-cascade-5/#layering>.
    pub layer_order: Option<LayerOrder>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PageSelector {
    pub page_type: Option<String>,
    pub pseudos: Vec<PagePseudo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PagePseudo {
    First,
    Left,
    Right,
    Blank,
    Nth { a: i32, b: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PageSpecificity {
    pub page_type_names: u16,
    pub first_or_blank: u16,
    pub left_or_right: u16,
}

impl PageSelector {
    pub fn matches(
        &self,
        page_number: usize,
        page_name: Option<&str>,
        is_blank: bool,
        page_progression_direction: Direction,
    ) -> bool {
        if let Some(page_type) = &self.page_type
            && page_name != Some(page_type.as_str())
        {
            return false;
        }
        self.pseudos.iter().all(|pseudo| match pseudo {
            PagePseudo::First => page_number == 1,
            PagePseudo::Left => page_is_left(page_number, page_progression_direction),
            PagePseudo::Right => !page_is_left(page_number, page_progression_direction),
            PagePseudo::Blank => is_blank,
            PagePseudo::Nth { a, b } => nth_page_matches(*a, *b, page_number),
        })
    }

    // CSS Paged Media 3 computes page selector specificity as (f,g,h):
    // page type names, :first/:blank pseudo-classes, then :left/:right.
    // https://www.w3.org/TR/css-page-3/#cascading-and-page-context
    pub fn specificity(&self) -> PageSpecificity {
        let mut specificity = PageSpecificity {
            page_type_names: u16::from(self.page_type.is_some()),
            first_or_blank: 0,
            left_or_right: 0,
        };
        for pseudo in &self.pseudos {
            match pseudo {
                PagePseudo::First | PagePseudo::Blank | PagePseudo::Nth { .. } => {
                    specificity.first_or_blank = specificity.first_or_blank.saturating_add(1);
                }
                PagePseudo::Left | PagePseudo::Right => {
                    specificity.left_or_right = specificity.left_or_right.saturating_add(1);
                }
            }
        }
        specificity
    }
}

/// Match a one-based page number against CSS `:nth(<an-plus-b>)`.
///
/// GCPM page selectors reuse Selectors' `an+b` sequence with `n` starting at
/// zero; page numbers themselves are one-based:
/// <https://www.w3.org/TR/css-gcpm-3/#document-page-selectors>.
fn nth_page_matches(a: i32, b: i32, page_number: usize) -> bool {
    let page_number = i32::try_from(page_number).unwrap_or(i32::MAX);
    if a == 0 {
        return page_number == b;
    }
    let delta = page_number - b;
    if a > 0 {
        delta >= 0 && delta % a == 0
    } else {
        delta <= 0 && delta % a == 0
    }
}

impl PageRule {
    pub fn matching_specificity(
        &self,
        page_number: usize,
        page_name: Option<&str>,
        is_blank: bool,
        page_progression_direction: Direction,
    ) -> Option<PageSpecificity> {
        if self.selectors.is_empty() {
            return Some(PageSpecificity {
                page_type_names: 0,
                first_or_blank: 0,
                left_or_right: 0,
            });
        }
        self.selectors
            .iter()
            .filter(|selector| {
                selector.matches(page_number, page_name, is_blank, page_progression_direction)
            })
            .map(PageSelector::specificity)
            .max()
    }
}

/// Returns whether a page is a left page for spread pseudo-class matching.
///
/// CSS Paged Media defines `:left` and `:right` by page progression. In
/// left-to-right progression the first page is a right page; in right-to-left
/// progression the first page is a left page:
/// <https://www.w3.org/TR/css-page-3/#spread-pseudos>.
fn page_is_left(page_number: usize, page_progression_direction: Direction) -> bool {
    match page_progression_direction {
        Direction::Ltr => page_number.is_multiple_of(2),
        Direction::Rtl => !page_number.is_multiple_of(2),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CounterStyleRule {
    pub name: String,
    pub system: CounterStyleSystem,
    pub symbols: Vec<String>,
    pub additive_symbols: Vec<(i32, String)>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub negative: Option<(String, String)>,
    pub pad: Option<(usize, String)>,
    pub range: Option<CounterStyleRange>,
    pub fallback: Option<String>,
    pub speak_as: Option<String>,
}

/// The `range` descriptor for `@counter-style`.
///
/// CSS Counter Styles Level 3 allows `auto` or one or more integer/infinite
/// intervals:
/// <https://www.w3.org/TR/css-counter-styles-3/#descdef-counter-style-range>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CounterStyleRange {
    Auto,
    Intervals(Vec<CounterStyleRangeInterval>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CounterStyleRangeInterval {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CounterStyleSystem {
    Cyclic,
    Numeric,
    Alphabetic,
    Symbolic,
    Fixed(i32),
    Additive,
    Extends(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CssFontFace {
    pub family: String,
    pub sources: Vec<FontFaceSource>,
    pub unicode_range: Option<Vec<UnicodeRange>>,
    /// Scale applied to this face before glyph selection and metrics use.
    /// CSS Fonts Level 5 `size-adjust` is distinct from the element-level
    /// `font-size-adjust` property and therefore remains face metadata.
    pub size_adjust: Option<u32>,
    /// CSS Fonts metric override descriptors, stored as percentage factors of
    /// the selected face's em square.
    pub ascent_override: Option<u32>,
    pub descent_override: Option<u32>,
    pub line_gap_override: Option<u32>,
    pub weight: FontWeight,
    /// `font-weight: auto` (the descriptor initial value) or a variable range.
    /// In both cases the registered face keeps its intrinsic `wght` axis.
    pub weight_is_variable: bool,
    pub style: FontStyle,
    pub width: FontWidth,
    /// `font-stretch: auto` (the descriptor initial value) or a variable range.
    /// In both cases the registered face keeps its intrinsic `wdth` axis.
    pub width_is_variable: bool,
    /// Default OpenType axis coordinates supplied by the @font-face
    /// `font-variation-settings` descriptor.
    pub font_variation_settings: FontVariationSettings,
    pub font_feature_settings: FontFeatureSettings,
    pub font_variant_ligatures: FontVariantLigatures,
    pub font_variant_position: FontVariantPosition,
    pub font_variant_caps: FontVariantCaps,
    pub font_variant_numeric: FontVariantNumeric,
    pub font_variant_alternates: FontVariantAlternates,
    pub font_variant_east_asian: FontVariantEastAsian,
}

/// One inclusive CSS `@font-face unicode-range` interval.
///
/// CSS Fonts defines `unicode-range` as a font-face descriptor that limits the
/// characters for which a downloaded face participates in font matching:
/// <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnicodeRange {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

impl UnicodeRange {
    pub(crate) const ALL: Self = Self {
        start: 0,
        end: 0x10ffff,
    };

    pub(crate) fn contains(self, character: char) -> bool {
        let scalar = character as u32;
        self.start <= scalar && scalar <= self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FontFaceSource {
    Url {
        value: String,
        base_url: Option<url::Url>,
        root_url: Option<url::Url>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct StyleRule {
    pub selector_text: String,
    pub selector: SelectorList<QuireSelectorImpl>,
    pub declarations: Declarations,
    /// Maximum selector-list specificity, retained for parser diagnostics and
    /// tests. The cascade uses the specificity of the branch that matched.
    #[allow(dead_code)]
    pub specificity: u32,
    pub order: usize,
    pub layer_name: Option<LayerName>,
    pub scopes: Vec<ScopeRule>,
}

/// A conditional rule set selected by a layout-time query container.
#[allow(
    dead_code,
    reason = "container-query matching is retained for layout-time implementation"
)]
#[derive(Debug, Clone)]
pub(crate) struct ContainerRule {
    /// Raw prelude retained until the selected container supplies its logical
    /// axes and computed style for threshold resolution.
    pub prelude: String,
    pub rules: Vec<StyleRule>,
    /// Nested container queries retain their own condition instead of being
    /// flattened into the outer query before layout-time evaluation.
    pub nested: Vec<ContainerRule>,
}

#[allow(
    dead_code,
    reason = "container-query matching is retained for layout-time implementation"
)]
impl ContainerRule {
    /// Returns the optional query name followed by the parenthesized condition
    /// text. CSS Containment evaluates this only after container selection.
    /// <https://www.w3.org/TR/css-contain-3/#container-rule>
    pub(crate) fn name_and_condition(&self) -> (Option<&str>, &str) {
        let prelude = self.prelude.trim();
        let Some(condition_start) = prelude.find('(') else {
            return (None, prelude);
        };
        let name = prelude[..condition_start].trim();
        (
            (!name.is_empty()).then_some(name),
            prelude[condition_start..].trim(),
        )
    }

    pub(crate) fn rules(&self) -> &[StyleRule] {
        &self.rules
    }

    pub(crate) fn nested(&self) -> &[ContainerRule] {
        &self.nested
    }
}

impl Stylesheet {
    #[allow(
        dead_code,
        reason = "container-query matching is retained for layout-time implementation"
    )]
    pub(crate) fn container_rules(&self) -> &[ContainerRule] {
        &self.container_rules
    }
}

/// A CSS Animations `<keyframes-name>`.
///
/// The name has already been decoded from an identifier or string token. Its
/// equality is intentionally case-sensitive, as required when matching an
/// `animation-name` against an `@keyframes` rule:
/// <https://www.w3.org/TR/css-animations-1/#keyframes>
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyframesName(String);

impl KeyframesName {
    /// Parses one complete `<keyframes-name>` from CSS tokens.
    ///
    /// CSS Animations permits either a `<custom-ident>` or a `<string>`. The
    /// unquoted form excludes CSS-wide keywords, `default`, and `none`; a
    /// quoted string remains valid for every decoded value.
    /// <https://www.w3.org/TR/css-animations-1/#typedef-keyframes-name>
    pub(in crate::css) fn parse<'i, 't>(
        input: &mut Parser<'i, 't>,
    ) -> Result<Self, cssparser::ParseError<'i, BasicParseErrorKind<'i>>> {
        if let Ok(ident) = input.try_parse(|input| input.expect_ident_cloned()) {
            if Self::unquoted_identifier_is_reserved(&ident) {
                return Err(input.new_custom_error(BasicParseErrorKind::QualifiedRuleInvalid));
            }
            input.expect_exhausted()?;
            return Ok(Self(ident.to_string()));
        }

        let string = input.expect_string_cloned()?;
        input.expect_exhausted()?;
        Ok(Self(string.to_string()))
    }

    /// Parses a complete keyframes name retained as a declaration-value slice.
    pub(in crate::css) fn parse_css(value: &str) -> Option<Self> {
        let mut input = ParserInput::new(value);
        let mut parser = Parser::new(&mut input);
        Self::parse(&mut parser).ok()
    }

    /// Serializes the decoded keyframes name as a CSS string token. Keeping
    /// the snapshot state decoded makes keyframe-name comparison correct;
    /// quoting here provides an unambiguous declaration value when a
    /// shorthand is expanded into `animation-name`.
    pub(in crate::css) fn to_css_string(&self) -> String {
        let mut output = String::with_capacity(self.0.len() + 2);
        output.push('"');
        for character in self.0.chars() {
            match character {
                '"' | '\\' => {
                    output.push('\\');
                    output.push(character);
                }
                '\n' => output.push_str("\\a "),
                '\r' => output.push_str("\\d "),
                '\u{000C}' => output.push_str("\\c "),
                character => output.push(character),
            }
        }
        output.push('"');
        output
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn unquoted_identifier_is_reserved(value: &str) -> bool {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default" | "none"
        )
    }
}

/// One named CSS keyframes rule.
///
/// CSS Animations stores keyframes separately from ordinary style rules: a
/// keyframe selector supplies declarations only when an animation instance
/// selects an interval from this rule.
/// <https://www.w3.org/TR/css-animations-1/#keyframes>
#[derive(Debug, Clone)]
pub(crate) struct KeyframesRule {
    pub(crate) name: KeyframesName,
    pub(crate) steps: Vec<KeyframeStep>,
}

/// Declarations at one normalized keyframe offset in a [`KeyframesRule`].
#[derive(Debug, Clone)]
pub(crate) struct KeyframeStep {
    /// The normalized offset in the inclusive interval `[0, 1]`.
    pub(crate) offset: f32,
    pub(crate) declarations: Declarations,
}

/// Parsed CSS `@scope` root and optional lower boundary selectors.
///
/// CSS Cascade Level 5 places scoped proximity after specificity and before
/// source order in the cascade. The scope root/limit selectors define whether a
/// scoped rule applies to an element and how many ancestor hops its declaration
/// is from the nearest scoping root:
/// <https://www.w3.org/TR/css-cascade-5/#scoped-styles>.
#[derive(Debug, Clone)]
pub(crate) struct ScopeRule {
    pub root: ScopeRoot,
    pub limit: Option<SelectorList<QuireSelectorImpl>>,
}

/// The upper boundary of a CSS `@scope` rule.
#[derive(Debug, Clone)]
pub(crate) enum ScopeRoot {
    Explicit(SelectorList<QuireSelectorImpl>),
    Owner(StylesheetScopeAnchor),
}

/// The DOM context used by an empty `@scope` prelude.
///
/// CSS scopes an empty-prelude rule to the parent of the owning stylesheet
/// node, or to the containing tree root when it has no parent element:
/// <https://drafts.csswg.org/css-cascade-6/#scope-atrule>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum StylesheetScopeAnchor {
    DocumentRoot,
    Element(crate::dom::ElementId),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Declarations {
    items: Vec<(String, String)>,
    base_url: Option<url::Url>,
    root_url: Option<url::Url>,
}

impl Declarations {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            base_url: None,
            root_url: None,
        }
    }

    pub fn with_urls(mut self, base_url: Option<&url::Url>, root_url: Option<&url::Url>) -> Self {
        self.base_url = base_url.cloned();
        self.root_url = root_url.cloned();
        self
    }

    pub fn base_url(&self) -> Option<&url::Url> {
        self.base_url.as_ref()
    }

    pub fn root_url(&self) -> Option<&url::Url> {
        self.root_url.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&String> {
        self.items
            .iter()
            .rev()
            .find_map(|(key, value)| (key == name).then_some(value))
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, (String, String)> {
        self.items.iter()
    }

    pub fn extend(&mut self, declarations: Declarations) {
        self.items.extend(declarations.items);
    }
}

impl FromIterator<(String, String)> for Declarations {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        Self {
            items: iter.into_iter().collect(),
            base_url: None,
            root_url: None,
        }
    }
}

impl<'a> IntoIterator for &'a Declarations {
    type Item = &'a (String, String);
    type IntoIter = std::slice::Iter<'a, (String, String)>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionalEdges<T> {
    pub top: Option<T>,
    pub right: Option<T>,
    pub bottom: Option<T>,
    pub left: Option<T>,
}

impl<T> OptionalEdges<T> {
    pub const NONE: Self = Self {
        top: None,
        right: None,
        bottom: None,
        left: None,
    };
}
