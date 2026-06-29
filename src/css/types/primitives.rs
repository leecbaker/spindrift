use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
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
    pub rules: Vec<StyleRule>,
    pub marker_rules: Vec<StyleRule>,
    pub before_rules: Vec<StyleRule>,
    pub after_rules: Vec<StyleRule>,
    pub first_line_rules: Vec<StyleRule>,
    pub first_letter_rules: Vec<StyleRule>,
    pub page_rules: Vec<PageRule>,
    pub page_declarations: Declarations,
    pub first_page_declarations: Declarations,
    pub page_margin_boxes: HashMap<String, Declarations>,
    pub font_faces: Vec<CssFontFace>,
    pub font_feature_values: FontFeatureValues,
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
    pub weight: FontWeight,
    pub style: FontStyle,
    pub width: FontWidth,
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
        base_url: Option<PathBuf>,
        root_url: Option<PathBuf>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct StyleRule {
    pub selector_text: String,
    pub selector: SelectorList<ReasySelectorImpl>,
    pub declarations: Declarations,
    pub specificity: u32,
    pub order: usize,
    pub layer_name: Option<String>,
    pub scopes: Vec<ScopeRule>,
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
    pub root: SelectorList<ReasySelectorImpl>,
    pub limit: Option<SelectorList<ReasySelectorImpl>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Declarations {
    items: Vec<(String, String)>,
    base_url: Option<PathBuf>,
    root_url: Option<PathBuf>,
}

impl Declarations {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            base_url: None,
            root_url: None,
        }
    }

    pub fn with_urls(mut self, base_url: Option<&Path>, root_url: Option<&Path>) -> Self {
        self.base_url = base_url.map(Path::to_path_buf);
        self.root_url = root_url.map(Path::to_path_buf);
        self
    }

    pub fn base_url(&self) -> Option<&Path> {
        self.base_url.as_deref()
    }

    pub fn root_url(&self) -> Option<&Path> {
        self.root_url.as_deref()
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
