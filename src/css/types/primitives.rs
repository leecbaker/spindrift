use super::*;
use crate::units::{LayoutLength, LayoutSize, layout_pt};

/// The output medium used when evaluating CSS Media Queries.
///
/// Media Queries Level 4 media types select an output category; PDF rendering
/// defaults to `print`, while callers that render a screen snapshot can select
/// `screen`: <https://www.w3.org/TR/mediaqueries-4/#media-types>.
///
/// ```
/// let medium = quire::MediaType::Print;
/// assert_eq!(medium, quire::MediaType::Print);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MediaType {
    /// A paged or print rendering medium.
    #[default]
    Print,
    /// A screen rendering medium.
    Screen,
}

/// CSS-pixel viewport coordinates used only by media-query evaluation.
/// They are deliberately distinct from PDF-point layout viewport lengths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssViewportSpace {}

pub type CssViewportSize = euclid::Size2D<f32, CssViewportSpace>;

/// Physical and logical viewport bases for resolving CSS viewport units.
/// Physical dimensions are layout points; media-query CSS pixels use the
/// separate [`CssViewportSize`] type above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewportLengthBasis {
    physical: LayoutSize,
    writing_mode: WritingMode,
}

impl ViewportLengthBasis {
    pub(crate) fn for_writing_mode(physical: LayoutSize, writing_mode: WritingMode) -> Self {
        Self {
            physical,
            writing_mode,
        }
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
        ContainerLengthBasis::for_writing_mode(self.physical, self.writing_mode)
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

/// Used element-font bases for resolving `em` and `ch` components.
///
/// CSS Values resolves `em` against the element's used font size and `ch`
/// against the selected font's zero-glyph advance:
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FontRelativeLengthBasis {
    font_size: LayoutLength,
    ch_advance: LayoutLength,
}

impl FontRelativeLengthBasis {
    pub(crate) const fn new(font_size: LayoutLength, ch_advance: LayoutLength) -> Self {
        Self {
            font_size,
            ch_advance,
        }
    }

    pub(crate) const fn font_size(self) -> LayoutLength {
        self.font_size
    }

    pub(crate) const fn ch_advance(self) -> LayoutLength {
        self.ch_advance
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaEnvironment {
    pub media_type: MediaType,
    pub viewport: CssViewportSize,
    pub resolution_dppx: f32,
}

impl MediaEnvironment {
    pub const fn new(media_type: MediaType, viewport: CssViewportSize) -> Self {
        Self {
            media_type,
            viewport,
            resolution_dppx: 1.0,
        }
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
}

/// The CSS color space in which a [`Color`] stores its three coordinates.
///
/// CSS Color 4 keeps colors in their specified space until they are used by a
/// physical output device.  In particular, an out-of-sRGB Display-P3 color
/// must not be clipped merely because Quire's layout engine is not itself a
/// display device.  See <https://www.w3.org/TR/css-color-4/#color-conversion>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum ColorSpace {
    Srgb,
    DisplayP3,
    A98Rgb,
    ProphotoRgb,
    Rec2020,
    /// Device-independent CIE XYZ, chromatically adapted to D50.
    XyzD50,
}

impl ColorSpace {
    /// Stable discriminant for document-local cache keys.
    pub(crate) const fn cache_key(self) -> u8 {
        self as u8
    }
}

/// A CSS color with independent alpha and color-space-tagged coordinates.
///
/// `r`, `g`, and `b` are historic field names retained while the renderer is
/// migrated; they are generic three-component coordinates, not necessarily
/// sRGB channels. Alpha is always clamped as required by CSS Color 4.
///
/// <https://www.w3.org/TR/css-color-4/#alpha-value>
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    pub(crate) space: ColorSpace,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
        space: ColorSpace::Srgb,
    };
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
        space: ColorSpace::Srgb,
    };
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
        space: ColorSpace::Srgb,
    };

    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 1.0)
    }

    /// Create an sRGB color with alpha.
    ///
    /// CSS Color Level 4 defines alpha as a number in `[0, 1]` after
    /// clamping:
    /// <https://www.w3.org/TR/css-color-4/#alpha-value>.
    pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a.clamp(0.0, 1.0),
            space: ColorSpace::Srgb,
        }
    }

    /// Create an sRGB color from normalized CSS color components.
    ///
    /// CSS Color Level 4 defines `color(srgb ...)` components as numbers or
    /// percentages in the sRGB color space, with alpha clamped to `[0, 1]`:
    /// <https://www.w3.org/TR/css-color-4/#predefined-sRGB>.
    pub(crate) fn srgb(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: r.clamp(0.0, 1.0),
            g: g.clamp(0.0, 1.0),
            b: b.clamp(0.0, 1.0),
            a: a.clamp(0.0, 1.0),
            space: ColorSpace::Srgb,
        }
    }

    /// Create a color in a CSS Color 4 predefined RGB space, retaining
    /// out-of-gamut coordinates for the eventual output conversion.
    pub(crate) fn in_space(space: ColorSpace, r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r,
            g,
            b,
            a: a.clamp(0.0, 1.0),
            space,
        }
    }

    pub(crate) const fn space(self) -> ColorSpace {
        self.space
    }

    pub(crate) fn with_alpha(self, alpha: f32) -> Self {
        Self {
            a: alpha.clamp(0.0, 1.0),
            ..self
        }
    }

    pub(crate) fn is_visible(self) -> bool {
        self.a > 0.0
    }

    pub(crate) fn is_opaque(self) -> bool {
        (self.a - 1.0).abs() < 0.001
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Stylesheet {
    pub origin: StylesheetOrigin,
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
    pub layer_names: Vec<String>,
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
    pub first_line_rules: Vec<StyleRule>,
    pub first_letter_rules: Vec<StyleRule>,
    pub page_rules: Vec<PageRule>,
    pub page_declarations: Declarations,
    pub first_page_declarations: Declarations,
    pub font_faces: Vec<CssFontFace>,
    pub font_feature_values: FontFeatureValues,
    pub font_palette_values: FontPaletteValues,
    pub counter_styles: Vec<CounterStyleRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    pub order: usize,
    /// Resolved cascade layer order for this page rule, if declared in `@layer`.
    ///
    /// CSS Paged Media delegates page-context cascading to normal cascade
    /// mechanics plus page-selector specificity, and CSS Cascade Level 5 adds
    /// cascade layers to that ordering:
    /// <https://www.w3.org/TR/css-page-3/#cascading-and-page-context> and
    /// <https://www.w3.org/TR/css-cascade-5/#layering>.
    pub layer_order: Option<usize>,
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
    pub weight: FontWeight,
    /// `font-weight: auto` (the descriptor initial value) or a variable range.
    /// In both cases the registered face keeps its intrinsic `wght` axis.
    pub weight_is_variable: bool,
    pub style: FontStyle,
    pub width: FontWidth,
    /// `font-stretch: auto` (the descriptor initial value) or a variable range.
    /// In both cases the registered face keeps its intrinsic `wdth` axis.
    pub width_is_variable: bool,
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
    pub specificity: u32,
    pub order: usize,
    pub layer_name: Option<String>,
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

/// One named CSS keyframes rule.
///
/// CSS Animations stores keyframes separately from ordinary style rules: a
/// keyframe selector supplies declarations only when an animation instance
/// selects an interval from this rule.
/// <https://www.w3.org/TR/css-animations-1/#keyframes>
#[derive(Debug, Clone)]
pub(crate) struct KeyframesRule {
    pub(crate) name: String,
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
    pub root: SelectorList<QuireSelectorImpl>,
    pub limit: Option<SelectorList<QuireSelectorImpl>>,
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
