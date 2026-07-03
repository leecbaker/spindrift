use super::*;

pub(in crate::css) static NEXT_ELEMENT_SIGNATURE_OPAQUE_ID: AtomicUsize = AtomicUsize::new(1);

pub(in crate::css) fn next_element_signature_opaque_id() -> Rc<usize> {
    Rc::new(NEXT_ELEMENT_SIGNATURE_OPAQUE_ID.fetch_add(1, Ordering::Relaxed))
}

/// Computed value of CSS Sizing `aspect-ratio`.
///
/// CSS Sizing Level 4 defines `aspect-ratio` as `auto || <ratio>`, where the
/// ratio is width divided by height:
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AspectRatio {
    pub(crate) auto: bool,
    pub(crate) ratio: Option<f32>,
}

impl AspectRatio {
    pub(crate) const AUTO: Self = Self {
        auto: true,
        ratio: None,
    };

    pub(crate) fn from_ratio(ratio: f32) -> Self {
        Self {
            auto: false,
            ratio: Some(ratio),
        }
    }

    pub(crate) fn auto_with_ratio(ratio: f32) -> Self {
        Self {
            auto: true,
            ratio: Some(ratio),
        }
    }

    /// Returns the authored preferred ratio for non-replaced boxes.
    ///
    /// CSS Sizing Level 4 gives `auto && <ratio>` special replaced-element
    /// fallback behavior; non-replaced boxes use the authored ratio as their
    /// preferred aspect ratio:
    /// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
    pub(crate) fn preferred_ratio_for_non_replaced(self, is_replaced: bool) -> Option<f32> {
        if self.auto && is_replaced {
            None
        } else {
            self.ratio
        }
    }

    /// Returns the preferred ratio after resolving replaced-element fallback.
    ///
    /// CSS Sizing Level 4 defines `aspect-ratio:auto` on replaced elements as
    /// using the natural aspect ratio, a bare `<ratio>` as overriding that
    /// ratio, and `auto && <ratio>` as falling back to the natural ratio when
    /// one exists:
    /// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
    pub(crate) fn preferred_ratio(
        self,
        is_replaced: bool,
        natural_ratio: Option<f32>,
    ) -> Option<f32> {
        let natural_ratio = natural_ratio.filter(|ratio| *ratio > 0.0);
        let specified_ratio = self.ratio.filter(|ratio| *ratio > 0.0);
        if !is_replaced {
            return specified_ratio;
        }
        if self.auto {
            natural_ratio.or(specified_ratio)
        } else {
            specified_ratio
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextTransform {
    pub(crate) case: TextTransformCase,
    pub(crate) full_width: bool,
    pub(crate) full_size_kana: bool,
}

impl TextTransform {
    pub(crate) const NONE: Self = Self {
        case: TextTransformCase::None,
        full_width: false,
        full_size_kana: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextTransformCase {
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Visibility {
    Visible,
    Hidden,
    Collapse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListStyleType {
    Disc,
    Circle,
    Square,
    DisclosureOpen,
    DisclosureClosed,
    Decimal,
    DecimalLeadingZero,
    Numeric(NumericCounterStyle),
    Additive(AdditiveCounterStyle),
    LowerAlpha,
    UpperAlpha,
    LowerGreek,
    Hiragana,
    HiraganaIroha,
    Katakana,
    KatakanaIroha,
    CjkEarthlyBranch,
    CjkHeavenlyStem,
    LowerRoman,
    UpperRoman,
    String(String),
    Anonymous(Box<CounterStyleRule>),
    Named(String),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumericCounterStyle {
    ArabicIndic,
    Bengali,
    Cambodian,
    CjkDecimal,
    Devanagari,
    Gujarati,
    Gurmukhi,
    Kannada,
    Lao,
    Malayalam,
    Mongolian,
    Myanmar,
    Oriya,
    Persian,
    Tamil,
    Telugu,
    Thai,
    Tibetan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdditiveCounterStyle {
    Armenian,
    LowerArmenian,
    Georgian,
    Hebrew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListStylePosition {
    Outside,
    Inside,
}

/// Side-selection mode for outside list markers.
///
/// CSS Lists Level 3 defines `marker-side` to choose whether an outside marker
/// is positioned from the list item's own directionality or its parent's:
/// <https://www.w3.org/TR/css-lists-3/#marker-side>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkerSide {
    MatchSelf,
    MatchParent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MarkerContent {
    Auto,
    None,
    Parts(Vec<MarkerContentPart>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MarkerContentPart {
    Text(String),
    Counter {
        name: String,
        style: Option<ListStyleType>,
    },
    Counters {
        name: String,
        separator: String,
        style: Option<ListStyleType>,
    },
}

/// Computed CSS `content` value.
///
/// CSS Generated Content Level 3 defines `content` as controlling whether an
/// element renders normal contents, generated anonymous inline contents, or a
/// replaced image:
/// <https://www.w3.org/TR/css-content-3/#content-property>.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Content {
    Normal,
    None,
    List {
        parts: GeneratedContent,
        alt: Option<GeneratedAltText>,
    },
    Replacement {
        image: GeneratedContentPart,
        alt: Option<GeneratedAltText>,
    },
}

impl Content {
    pub(crate) fn generated_parts(&self) -> Option<&[GeneratedContentPart]> {
        match self {
            Self::List { parts, .. } => Some(parts),
            Self::Replacement { image, .. } => Some(std::slice::from_ref(image)),
            Self::Normal | Self::None => None,
        }
    }

    pub(crate) fn is_generated(&self) -> bool {
        matches!(self, Self::List { .. } | Self::Replacement { .. })
    }

    pub(crate) fn alt(&self) -> Option<&[GeneratedAltTextPart]> {
        match self {
            Self::List { alt, .. } | Self::Replacement { alt, .. } => alt.as_deref(),
            Self::Normal | Self::None => None,
        }
    }
}

/// Computed generated `content` parts for elements and tree-abiding
/// pseudo-elements.
///
/// CSS Generated Content Level 3 defines `<content-list>` as a sequence of
/// strings, images, attributes, and counters that generates anonymous inline
/// content:
/// <https://www.w3.org/TR/css-content-3/#typedef-content-list>.
pub(crate) type GeneratedContent = Vec<GeneratedContentPart>;
pub(crate) type GeneratedAltText = Vec<GeneratedAltTextPart>;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GeneratedContentPart {
    Text(String),
    Contents,
    Attr {
        name: String,
        fallback: Option<String>,
    },
    Counter {
        name: String,
        style: Option<ListStyleType>,
    },
    Counters {
        name: String,
        separator: String,
        style: Option<ListStyleType>,
    },
    TargetCounter {
        target: String,
        name: String,
        style: Option<ListStyleType>,
    },
    TargetText {
        target: String,
        keyword: NamedStringTargetTextKeyword,
    },
    Image {
        image: BackgroundImage,
    },
    Quote(GeneratedQuote),
    Leader(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GeneratedAltTextPart {
    Text(String),
    Attr {
        name: String,
        fallback: Option<String>,
    },
    Counter {
        name: String,
        style: Option<ListStyleType>,
    },
    Counters {
        name: String,
        separator: String,
        style: Option<ListStyleType>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeneratedQuote {
    Open,
    Close,
    NoOpen,
    NoClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Quotes {
    Auto {
        language: Option<String>,
        resolved: bool,
    },
    None,
    Pairs(Vec<(String, String)>),
}

impl Quotes {
    pub(crate) fn auto() -> Self {
        Self::Auto {
            language: None,
            resolved: false,
        }
    }

    /// Return the value inherited by ordinary `quotes` inheritance.
    ///
    /// CSS Generated Content Level 3 defines `quotes: auto` as resolving from
    /// the parent content language, while `match-parent` reuses the parent's
    /// quote system:
    /// <https://www.w3.org/TR/css-content-3/#quotes-property>.
    pub(crate) fn inherited(&self) -> Self {
        match self {
            Self::Auto { .. } => Self::auto(),
            Self::None => Self::None,
            Self::Pairs(pairs) => Self::Pairs(pairs.clone()),
        }
    }

    pub(crate) fn resolve_auto_language(&mut self, language: Option<&str>) {
        if let Self::Auto {
            language: auto_language,
            resolved,
        } = self
            && !*resolved
        {
            *auto_language = language.map(str::to_string);
            *resolved = true;
        }
    }

    pub(crate) fn auto_language(&self) -> Option<&str> {
        match self {
            Self::Auto { language, .. } => language.as_deref(),
            Self::None | Self::Pairs(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

/// Computed `isolation`.
///
/// CSS Compositing and Blending defines `isolation:isolate` as creating an
/// isolated stacking context:
/// <https://www.w3.org/TR/compositing-1/#isolation>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Isolation {
    Auto,
    Isolate,
}

/// Computed `mix-blend-mode` subset.
///
/// Non-`normal` blend modes establish stacking contexts. The renderer currently
/// records the trigger and preserves isolation ordering; PDF blend-mode output
/// can be expanded from these variants:
/// <https://www.w3.org/TR/compositing-1/#mix-blend-mode>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MixBlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// Computed `filter` value retained for stacking and later effect emission.
///
/// Non-`none` filter function lists establish a stacking context:
/// <https://www.w3.org/TR/filter-effects-1/#FilterProperty>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FilterValue {
    None,
    Functions(String),
}

/// Computed mask image/source value retained for stacking and later masking.
///
/// Non-`none` masks establish an isolated paint effect in CSS Masking:
/// <https://www.w3.org/TR/css-masking-1/#the-mask-image>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MaskValue {
    None,
    Image(String),
}

/// Computed paint containment bits relevant to stacking.
///
/// `contain: paint`, `contain: strict`, and `contain: content` establish paint
/// containment and therefore a stacking context:
/// <https://www.w3.org/TR/css-contain-2/#containment-paint>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Contain {
    pub(crate) layout: bool,
    pub(crate) paint: bool,
    pub(crate) style: bool,
    pub(crate) size: bool,
}

impl Contain {
    pub(crate) const NONE: Self = Self {
        layout: false,
        paint: false,
        style: false,
        size: false,
    };
}

/// Computed `content-visibility`.
///
/// `auto` and `hidden` imply layout/style/paint containment in CSS Containment:
/// <https://www.w3.org/TR/css-contain-2/#content-visibility>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentVisibility {
    Visible,
    Auto,
    Hidden,
}

/// Computed `clip-path` support relevant to paint isolation.
///
/// Non-`none` clip paths establish stacking contexts. Geometry support is kept
/// intentionally coarse until clipping is represented as paths in paint
/// effects:
/// <https://www.w3.org/TR/css-masking-1/#the-clip-path>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipPath {
    None,
    Inset,
    Shape,
    Url,
}

/// Computed `will-change` features relevant to stacking-context prediction.
///
/// CSS Will Change requires the element to act as if specified features already
/// had their non-initial values for stacking-context creation:
/// <https://www.w3.org/TR/css-will-change-1/#will-change>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct WillChange {
    pub(crate) contents: bool,
    pub(crate) scroll_position: bool,
    pub(crate) opacity: bool,
    pub(crate) transform: bool,
    pub(crate) filter: bool,
    pub(crate) clip_path: bool,
    pub(crate) mask: bool,
    pub(crate) mix_blend_mode: bool,
    pub(crate) isolation: bool,
    pub(crate) contain: bool,
}

/// One computed CSS 2D transform function.
///
/// CSS Transforms Level 1 defines the 2D transform function list and applies it
/// to transformable elements as a matrix at used-value time:
/// <https://www.w3.org/TR/css-transforms-1/#transform-functions>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TransformFunction {
    Matrix(f32, f32, f32, f32, f32, f32),
    Translate(ComputedLengthPercentage, ComputedLengthPercentage),
    Scale(f32, f32),
    Rotate(f32),
    Skew(f32, f32),
}

impl TransformFunction {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Self::Translate(x, y) = self {
            x.resolve_font_metric_lengths(ch_advance);
            y.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Self::Translate(x, y) = self {
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
    }
}

pub(crate) type TransformList = Vec<TransformFunction>;

/// Computed `transform-origin` for the supported 2D transform model.
///
/// The third component is intentionally omitted until 3D transforms are
/// modeled. Percentages resolve against the border box:
/// <https://www.w3.org/TR/css-transforms-1/#transform-origin-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TransformOrigin {
    pub(crate) x: ComputedLengthPercentage,
    pub(crate) y: ComputedLengthPercentage,
}

impl TransformOrigin {
    pub(crate) const INITIAL: Self = Self {
        x: ComputedLengthPercentage {
            length: layout_pt(0.0),
            percent: 0.5,
            has_percentage: true,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            vmin: 0.0,
            vmax: 0.0,
            vi: 0.0,
            vb: 0.0,
            math: None,
        },
        y: ComputedLengthPercentage {
            length: layout_pt(0.0),
            percent: 0.5,
            has_percentage: true,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            vmin: 0.0,
            vmax: 0.0,
            vi: 0.0,
            vb: 0.0,
            math: None,
        },
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

/// Computed `float` value.
///
/// CSS 2.2 defines left and right floats as boxes shifted to the containing
/// block edge with following flow content shortened around them:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Float {
    None,
    Left,
    Right,
    InlineStart,
    InlineEnd,
}

/// Computed `clear` value.
///
/// CSS 2.2 defines clearance as moving a box below prior left and/or right
/// floats in the same block formatting context:
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clear {
    None,
    Left,
    Right,
    Both,
    InlineStart,
    InlineEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageBreak {
    Auto,
    Avoid,
    Page,
    Left,
    Right,
    Recto,
    Verso,
}

impl PageBreak {
    pub(crate) fn is_forced(self) -> bool {
        matches!(
            self,
            Self::Page | Self::Left | Self::Right | Self::Recto | Self::Verso
        )
    }

    pub(crate) fn avoids_page(self) -> bool {
        matches!(self, Self::Avoid)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxSizing {
    ContentBox,
    BorderBox,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BookmarkLabel {
    pub parts: Vec<BookmarkLabelPart>,
}

impl BookmarkLabel {
    pub fn content_text() -> Self {
        Self {
            parts: vec![BookmarkLabelPart::ContentText],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BookmarkLabelPart {
    String(String),
    ContentText,
    Attr(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NamedStringSet {
    pub name: String,
    pub parts: Vec<NamedStringPart>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NamedStringPart {
    String(String),
    ContentText,
    ContentFirstLetter,
    ContentMarker,
    BeforeContent,
    AfterContent,
    Attr {
        name: String,
        fallback: Option<String>,
    },
    Image(BackgroundImage),
    Quote(GeneratedQuote),
    Leader(String),
    Counter {
        name: String,
        style: Option<ListStyleType>,
    },
    Counters {
        name: String,
        separator: String,
        style: Option<ListStyleType>,
    },
    TargetCounter {
        target: String,
        name: String,
        style: Option<ListStyleType>,
    },
    TargetText {
        target: String,
        keyword: NamedStringTargetTextKeyword,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NamedStringTargetTextKeyword {
    Content,
    Before,
    After,
    FirstLetter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CssBookmarkState {
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementAttributeSignature {
    pub namespace_url: String,
    pub local_name: String,
    pub value: String,
}

impl ElementAttributeSignature {
    pub(crate) fn new(
        namespace_url: impl Into<String>,
        local_name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            namespace_url: namespace_url.into(),
            local_name: local_name.into(),
            value: value.into(),
        }
    }
}

pub(in crate::css) fn local_attribute_signatures(
    attrs: &HashMap<String, String>,
) -> Vec<ElementAttributeSignature> {
    attrs
        .iter()
        .map(|(name, value)| ElementAttributeSignature::new("", name.clone(), value.clone()))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementSiblingSignatureList(Rc<[ElementSiblingSignature]>);

impl ElementSiblingSignatureList {
    pub(crate) fn empty() -> Self {
        Self::from_vec(Vec::<ElementSiblingSignature>::new())
    }

    pub(crate) fn from_vec<Sibling>(siblings: Vec<Sibling>) -> Self
    where
        Sibling: Into<ElementSiblingSignature>,
    {
        Self(Rc::from(
            siblings
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ))
    }

    pub(crate) fn as_slice(&self) -> &[ElementSiblingSignature] {
        &self.0
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, ElementSiblingSignature> {
        self.as_slice().iter()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&ElementSiblingSignature> {
        self.as_slice().get(index)
    }

    pub(crate) fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}

impl std::ops::Deref for ElementSiblingSignatureList {
    type Target = [ElementSiblingSignature];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementSiblingSignature {
    pub tag: String,
    pub namespace_url: String,
    pub document_is_html: bool,
    pub attrs: HashMap<String, String>,
    pub namespace_attrs: Vec<ElementAttributeSignature>,
    pub opaque_id: Rc<usize>,
    pub children: ElementSiblingSignatureList,
    pub has_text_child: bool,
    pub is_target: bool,
    pub has_target_descendant: bool,
    /// HTML/document directionality known from the element itself.
    ///
    /// Selectors `:dir()` matches document-language directionality rather than
    /// CSS `direction`, so selector snapshots preserve explicit `dir`,
    /// `dir=auto`, and default `<bdi>` resolution for reconstructed descendants:
    /// <https://drafts.csswg.org/selectors/#the-dir-pseudo> and
    /// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality>.
    pub document_direction: Option<Direction>,
}

impl ElementSiblingSignature {
    pub(crate) fn new(tag: impl Into<String>, attrs: HashMap<String, String>) -> Self {
        let namespace_attrs = local_attribute_signatures(&attrs);
        Self {
            tag: tag.into(),
            namespace_url: String::new(),
            document_is_html: true,
            attrs,
            namespace_attrs,
            opaque_id: next_element_signature_opaque_id(),
            children: ElementSiblingSignatureList::empty(),
            has_text_child: false,
            is_target: false,
            has_target_descendant: false,
            document_direction: None,
        }
    }

    pub(crate) fn with_namespace(
        mut self,
        namespace_url: impl Into<String>,
        namespace_attrs: Vec<ElementAttributeSignature>,
    ) -> Self {
        self.namespace_url = namespace_url.into();
        self.namespace_attrs = namespace_attrs;
        self
    }

    pub(crate) fn with_document_is_html(mut self, document_is_html: bool) -> Self {
        self.document_is_html = document_is_html;
        self
    }

    pub(crate) fn with_child_list(
        mut self,
        children: ElementSiblingSignatureList,
        has_text_child: bool,
    ) -> Self {
        self.children = children;
        self.has_text_child = has_text_child;
        self.has_target_descendant = self
            .children
            .iter()
            .any(|child| child.is_target || child.has_target_descendant);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_children<Sibling>(self, children: Vec<Sibling>, has_text_child: bool) -> Self
    where
        Sibling: Into<ElementSiblingSignature>,
    {
        self.with_child_list(
            ElementSiblingSignatureList::from_vec(children),
            has_text_child,
        )
    }

    pub(crate) fn with_document_direction(mut self, direction: Direction) -> Self {
        self.document_direction = Some(direction);
        self
    }
}

impl From<&str> for ElementSiblingSignature {
    fn from(tag: &str) -> Self {
        Self::new(tag, HashMap::new())
    }
}

impl From<String> for ElementSiblingSignature {
    fn from(tag: String) -> Self {
        Self::new(tag, HashMap::new())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedLanguage {
    Unresolved,
    Unknown,
    Tag(String),
}

impl ResolvedLanguage {
    pub(crate) fn from_html_attribute(value: &str) -> Self {
        let value = value.trim();
        if value.is_empty() {
            Self::Unknown
        } else {
            Self::Tag(value.to_string())
        }
    }

    pub(crate) fn from_computed(value: Option<&str>) -> Self {
        value
            .map(Self::from_html_attribute)
            .unwrap_or(Self::Unknown)
    }

    pub(crate) fn as_computed_language(&self) -> Option<String> {
        match self {
            Self::Tag(language) => Some(language.clone()),
            Self::Unresolved | Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementSignature {
    pub tag: String,
    pub namespace_url: String,
    pub document_is_html: bool,
    pub attrs: HashMap<String, String>,
    pub namespace_attrs: Vec<ElementAttributeSignature>,
    pub opaque_id: Rc<usize>,
    pub sibling_index: Option<usize>,
    pub sibling_signatures: ElementSiblingSignatureList,
    pub child_signatures: ElementSiblingSignatureList,
    pub has_text_child: bool,
    pub is_target: bool,
    pub has_target_descendant: bool,
    /// HTML/document directionality known from the element itself.
    ///
    /// This is separate from `resolved_direction`: Selectors `:dir()` is based
    /// on host-language directionality and is unaffected by CSS `direction`.
    /// Undefined HTML directionality inherits through the selector chain:
    /// <https://drafts.csswg.org/selectors/#the-dir-pseudo> and
    /// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality>.
    pub document_direction: Option<Direction>,
    pub html_direction: Option<Direction>,
    pub resolved_direction: Option<Direction>,
    pub resolved_language: ResolvedLanguage,
}
