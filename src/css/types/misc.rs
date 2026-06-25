use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ELEMENT_SIGNATURE_OPAQUE_ID: AtomicUsize = AtomicUsize::new(1);

fn next_element_signature_opaque_id() -> Arc<usize> {
    Arc::new(NEXT_ELEMENT_SIGNATURE_OPAQUE_ID.fetch_add(1, Ordering::Relaxed))
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    Image {
        url: String,
        base_url: Option<PathBuf>,
        root_url: Option<PathBuf>,
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
            length: 0.0,
            percent: 0.5,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            vmin: 0.0,
            vmax: 0.0,
            vi: 0.0,
            vb: 0.0,
        },
        y: ComputedLengthPercentage {
            length: 0.0,
            percent: 0.5,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            vmin: 0.0,
            vmax: 0.0,
            vi: 0.0,
            vb: 0.0,
        },
    };
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

impl Float {
    pub(crate) fn physical(self, direction: Direction) -> Self {
        match (self, direction) {
            (Self::InlineStart, Direction::Ltr) | (Self::InlineEnd, Direction::Rtl) => Self::Left,
            (Self::InlineStart, Direction::Rtl) | (Self::InlineEnd, Direction::Ltr) => Self::Right,
            _ => self,
        }
    }
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

impl Clear {
    pub(crate) fn matches_float(self, float: Float, direction: Direction) -> bool {
        let clear = match (self, direction) {
            (Self::InlineStart, Direction::Ltr) | (Self::InlineEnd, Direction::Rtl) => Self::Left,
            (Self::InlineStart, Direction::Rtl) | (Self::InlineEnd, Direction::Ltr) => Self::Right,
            _ => self,
        };
        let float = float.physical(direction);
        matches!(
            (clear, float),
            (Self::Both, Float::Left | Float::Right)
                | (Self::Left, Float::Left)
                | (Self::Right, Float::Right)
        )
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamedStringSet {
    pub name: String,
    pub parts: Vec<NamedStringPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NamedStringPart {
    String(String),
    ContentText,
    BeforeContent,
    AfterContent,
    Attr(String),
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

fn local_attribute_signatures(attrs: &HashMap<String, String>) -> Vec<ElementAttributeSignature> {
    attrs
        .iter()
        .map(|(name, value)| ElementAttributeSignature::new("", name.clone(), value.clone()))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementSiblingSignature {
    pub tag: String,
    pub namespace_url: String,
    pub attrs: HashMap<String, String>,
    pub namespace_attrs: Vec<ElementAttributeSignature>,
    pub opaque_id: Arc<usize>,
    pub children: Vec<ElementSiblingSignature>,
    pub has_text_child: bool,
    pub is_target: bool,
    pub has_target_descendant: bool,
}

impl ElementSiblingSignature {
    pub(crate) fn new(tag: impl Into<String>, attrs: HashMap<String, String>) -> Self {
        let namespace_attrs = local_attribute_signatures(&attrs);
        Self {
            tag: tag.into(),
            namespace_url: String::new(),
            attrs,
            namespace_attrs,
            opaque_id: next_element_signature_opaque_id(),
            children: Vec::new(),
            has_text_child: false,
            is_target: false,
            has_target_descendant: false,
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

    pub(crate) fn with_children(
        mut self,
        children: Vec<ElementSiblingSignature>,
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
    pub attrs: HashMap<String, String>,
    pub namespace_attrs: Vec<ElementAttributeSignature>,
    pub opaque_id: Arc<usize>,
    pub sibling_index: Option<usize>,
    pub sibling_signatures: Vec<ElementSiblingSignature>,
    pub child_signatures: Vec<ElementSiblingSignature>,
    pub has_text_child: bool,
    pub is_target: bool,
    pub has_target_descendant: bool,
    pub html_direction: Option<Direction>,
    pub resolved_direction: Option<Direction>,
    pub resolved_language: ResolvedLanguage,
}

impl ElementSignature {
    pub fn new(tag: impl Into<String>, attrs: HashMap<String, String>) -> Self {
        let namespace_attrs = local_attribute_signatures(&attrs);
        Self {
            tag: tag.into(),
            namespace_url: String::new(),
            attrs,
            namespace_attrs,
            opaque_id: next_element_signature_opaque_id(),
            sibling_index: None,
            sibling_signatures: Vec::new(),
            child_signatures: Vec::new(),
            has_text_child: false,
            is_target: false,
            has_target_descendant: false,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        }
    }

    pub fn with_siblings<Sibling>(
        tag: impl Into<String>,
        attrs: HashMap<String, String>,
        sibling_index: usize,
        sibling_signatures: Vec<Sibling>,
    ) -> Self
    where
        Sibling: Into<ElementSiblingSignature>,
    {
        let fallback_namespace_attrs = local_attribute_signatures(&attrs);
        let sibling_signatures = sibling_signatures
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>();
        let opaque_id = sibling_signatures
            .get(sibling_index)
            .map(|sibling| Arc::clone(&sibling.opaque_id))
            .unwrap_or_else(next_element_signature_opaque_id);
        let is_target = sibling_signatures
            .get(sibling_index)
            .is_some_and(|sibling| sibling.is_target);
        let has_target_descendant = sibling_signatures
            .get(sibling_index)
            .is_some_and(|sibling| sibling.has_target_descendant);
        Self {
            tag: tag.into(),
            namespace_url: sibling_signatures
                .get(sibling_index)
                .map(|sibling| sibling.namespace_url.clone())
                .unwrap_or_default(),
            attrs,
            namespace_attrs: sibling_signatures
                .get(sibling_index)
                .map(|sibling| sibling.namespace_attrs.clone())
                .unwrap_or(fallback_namespace_attrs),
            opaque_id,
            sibling_index: Some(sibling_index),
            sibling_signatures,
            child_signatures: Vec::new(),
            has_text_child: false,
            is_target,
            has_target_descendant,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        }
    }

    pub(crate) fn with_children<Sibling>(
        mut self,
        children: Vec<Sibling>,
        has_text_child: bool,
    ) -> Self
    where
        Sibling: Into<ElementSiblingSignature>,
    {
        self.child_signatures = children.into_iter().map(Into::into).collect();
        self.has_text_child = has_text_child;
        self.has_target_descendant = self
            .child_signatures
            .iter()
            .any(|child| child.is_target || child.has_target_descendant);
        self
    }

    /// Attach HTML's dynamically resolved directionality for cascade input.
    ///
    /// The HTML `dir=auto` and default `<bdi>` algorithms produce an element
    /// directionality value that the Rendering section maps through UA
    /// `direction` rules using `:dir()`:
    /// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality> and
    /// <https://html.spec.whatwg.org/multipage/rendering.html#bidi-rendering>.
    pub(crate) fn with_html_direction(mut self, direction: Direction) -> Self {
        self.html_direction = Some(direction);
        self
    }

    /// Attach the element direction known before selector matching.
    ///
    /// Selectors `:dir()` matches the element's directionality. During cascade
    /// construction the available value is HTML's dynamic directionality, an
    /// explicit `dir` attribute, or the inherited computed `direction`:
    /// <https://www.w3.org/TR/selectors-4/#the-dir-pseudo> and
    /// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality>.
    pub(crate) fn with_resolved_direction(mut self, direction: Direction) -> Self {
        self.resolved_direction = Some(direction);
        self
    }

    /// Attach the element language known before selector matching.
    ///
    /// Selectors `:lang()` matches the element's document language, including
    /// inherited language and explicit unknown language. CSS delegates the
    /// language range matching to RFC 4647 filtering:
    /// <https://www.w3.org/TR/selectors-4/#the-lang-pseudo> and
    /// <https://www.rfc-editor.org/rfc/rfc4647#section-3.3.2>.
    pub(crate) fn with_resolved_language(mut self, language: ResolvedLanguage) -> Self {
        self.resolved_language = language;
        self
    }

    pub(crate) fn sibling_at(&self, index: usize) -> Option<Self> {
        let sibling = self.sibling_signatures.get(index)?.clone();
        Some(Self {
            tag: sibling.tag,
            namespace_url: sibling.namespace_url,
            attrs: sibling.attrs,
            namespace_attrs: sibling.namespace_attrs,
            opaque_id: sibling.opaque_id,
            sibling_index: Some(index),
            sibling_signatures: self.sibling_signatures.clone(),
            child_signatures: sibling.children,
            has_text_child: sibling.has_text_child,
            is_target: sibling.is_target,
            has_target_descendant: sibling.has_target_descendant,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        })
    }

    pub(crate) fn child_at(&self, index: usize) -> Option<Self> {
        let child = self.child_signatures.get(index)?.clone();
        Some(Self {
            tag: child.tag,
            namespace_url: child.namespace_url,
            attrs: child.attrs,
            namespace_attrs: child.namespace_attrs,
            opaque_id: child.opaque_id,
            sibling_index: Some(index),
            sibling_signatures: self.child_signatures.clone(),
            child_signatures: child.children,
            has_text_child: child.has_text_child,
            is_target: child.is_target,
            has_target_descendant: child.has_target_descendant,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TextDecoration {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
    pub blink: bool,
    pub spelling_error: bool,
    pub grammar_error: bool,
    pub style: TextDecorationStyle,
    pub thickness: TextDecorationThickness,
    pub inset: TextDecorationInset,
    pub skip_ink: TextDecorationSkipInk,
    pub skip_self: TextDecorationSkipSelf,
    pub skip_box: TextDecorationSkipBox,
    pub skip_spaces: TextDecorationSkipSpaces,
    pub underline_offset: TextUnderlineOffset,
    pub underline_position: TextUnderlinePosition,
    pub color: Option<Color>,
}

impl TextDecoration {
    pub(crate) fn with_propagated_lines(self, _parent: Self) -> Self {
        self
    }

    pub(crate) fn has_visible_line(self) -> bool {
        self.underline
            || self.overline
            || self.line_through
            || self.spelling_error
            || self.grammar_error
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.thickness.resolve_font_metric_lengths(ch_advance);
        self.underline_offset
            .resolve_font_metric_lengths(ch_advance);
        self.inset.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.thickness.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.underline_offset.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.inset.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }
}

/// Computed CSS `text-decoration-style`.
///
/// CSS Text Decoration defines the visual line pattern used by text
/// decoration lines:
/// <https://www.w3.org/TR/css-text-decor-3/#text-decoration-style-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextDecorationStyle {
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

/// Computed CSS `text-decoration-thickness`.
///
/// CSS Text Decoration Level 4 adds `text-decoration-thickness` as `auto`,
/// `from-font`, or a length/percentage:
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-width-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TextDecorationThickness {
    Auto,
    FromFont,
    LengthPercentage(ComputedLengthPercentage),
}

impl TextDecorationThickness {
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

/// Computed CSS `text-decoration-inset`.
///
/// CSS Text Decoration Level 4 trims or extends the start and end endpoints of
/// line decorations:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-inset-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TextDecorationInset {
    Auto,
    Lengths { start: f32, end: f32 },
}

impl TextDecorationInset {
    pub(crate) const ZERO: Self = Self::Lengths {
        start: 0.0,
        end: 0.0,
    };

    pub(crate) fn resolve_font_metric_lengths(&mut self, _ch_advance: f32) {}

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        _viewport_width: f32,
        _viewport_height: f32,
        _viewport_inline: f32,
        _viewport_block: f32,
    ) {
    }

    pub(crate) fn used(self, font_size: f32) -> (f32, f32) {
        match self {
            Self::Auto => (font_size * 0.125, font_size * 0.125),
            Self::Lengths { start, end } => (start, end),
        }
    }
}

/// Computed CSS `text-decoration-skip-ink`.
///
/// CSS Text Decoration Level 4 controls whether decoration strokes skip glyph
/// ink. The current PDF painter stores this for cascade fidelity; skip shaping
/// is still a renderer TODO:
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-skip-ink-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextDecorationSkipInk {
    Auto,
    All,
    None,
}

/// Computed CSS `text-decoration-skip-self`.
///
/// CSS Text Decoration Level 4 lets a box suppress decorations that would be
/// drawn through its own contents:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-self-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextDecorationSkipSelf {
    Auto,
    SkipAll,
    NoSkip,
    Lines {
        underline: bool,
        overline: bool,
        line_through: bool,
    },
}

/// Computed CSS `text-decoration-skip-box`.
///
/// CSS Text Decoration Level 4 controls whether decoration lines skip atomic
/// inline boxes:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-box-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextDecorationSkipBox {
    None,
    All,
}

/// Computed CSS `text-decoration-skip-spaces`.
///
/// CSS Text Decoration Level 4 defines whether decoration lines skip spacer
/// characters at the line edges or throughout the line:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-spaces-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextDecorationSkipSpaces {
    pub start: bool,
    pub end: bool,
    pub all: bool,
}

impl TextDecorationSkipSpaces {
    pub(crate) const NONE: Self = Self {
        start: false,
        end: false,
        all: false,
    };
    pub(crate) const START_END: Self = Self {
        start: true,
        end: true,
        all: false,
    };
    pub(crate) const ALL: Self = Self {
        start: false,
        end: false,
        all: true,
    };

    pub(crate) fn skips_line_start(self) -> bool {
        self.all || self.start
    }

    pub(crate) fn skips_line_end(self) -> bool {
        self.all || self.end
    }

    pub(crate) fn skips_all(self) -> bool {
        self.all
    }
}

/// Computed CSS `text-emphasis-style`.
///
/// CSS Text Decoration defines emphasis marks as either `none`, a filled/open
/// shape, or an author-provided string. If `filled` or `open` is given without
/// a shape, the used shape depends on the typographic writing mode:
/// <https://www.w3.org/TR/css-text-decor-3/#text-emphasis-style-property>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextEmphasisStyle {
    None,
    Keywords {
        fill: TextEmphasisFill,
        shape: Option<TextEmphasisShape>,
    },
    String(String),
}

impl TextEmphasisStyle {
    pub(crate) fn mark_for_writing_mode(&self, writing_mode: WritingMode) -> Option<&str> {
        match self {
            Self::None => None,
            Self::String(mark) => (!mark.is_empty()).then_some(mark.as_str()),
            Self::Keywords { fill, shape } => {
                let shape = shape.unwrap_or(match writing_mode {
                    WritingMode::HorizontalTb => TextEmphasisShape::Circle,
                    WritingMode::VerticalRl | WritingMode::VerticalLr => TextEmphasisShape::Sesame,
                });
                Some(match (fill, shape) {
                    (TextEmphasisFill::Filled, TextEmphasisShape::Dot) => "\u{2022}",
                    (TextEmphasisFill::Open, TextEmphasisShape::Dot) => "\u{25E6}",
                    (TextEmphasisFill::Filled, TextEmphasisShape::Circle) => "\u{25CF}",
                    (TextEmphasisFill::Open, TextEmphasisShape::Circle) => "\u{25CB}",
                    (TextEmphasisFill::Filled, TextEmphasisShape::DoubleCircle) => "\u{25C9}",
                    (TextEmphasisFill::Open, TextEmphasisShape::DoubleCircle) => "\u{25CE}",
                    (TextEmphasisFill::Filled, TextEmphasisShape::Triangle) => "\u{25B2}",
                    (TextEmphasisFill::Open, TextEmphasisShape::Triangle) => "\u{25B3}",
                    (TextEmphasisFill::Filled, TextEmphasisShape::Sesame) => "\u{FE45}",
                    (TextEmphasisFill::Open, TextEmphasisShape::Sesame) => "\u{FE46}",
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextEmphasisFill {
    Filled,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextEmphasisShape {
    Dot,
    Circle,
    DoubleCircle,
    Triangle,
    Sesame,
}

/// Computed CSS `text-emphasis-position`.
///
/// CSS Text Decoration positions emphasis marks over/under horizontal text
/// and on the right/left side in vertical text:
/// <https://www.w3.org/TR/css-text-decor-3/#text-emphasis-position-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextEmphasisPosition {
    pub(crate) over: bool,
    pub(crate) right: bool,
}

impl Default for TextEmphasisPosition {
    fn default() -> Self {
        Self {
            over: true,
            right: true,
        }
    }
}

/// Computed CSS `text-emphasis-skip`.
///
/// CSS Text Decoration Level 4 controls which typographic character classes
/// omit emphasis marks:
/// <https://drafts.csswg.org/css-text-decor-4/#text-emphasis-skip-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextEmphasisSkip {
    pub(crate) spaces: bool,
    pub(crate) punctuation: bool,
    pub(crate) symbols: bool,
    pub(crate) narrow: bool,
}

impl Default for TextEmphasisSkip {
    fn default() -> Self {
        Self {
            spaces: true,
            punctuation: true,
            symbols: false,
            narrow: false,
        }
    }
}

/// Computed CSS `text-shadow` layer.
///
/// CSS Text Decoration Level 4 follows the box-shadow grammar but applies
/// each shadow layer to text and decorations:
/// <https://drafts.csswg.org/css-text-decor-4/#text-shadow-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TextShadow {
    pub(crate) color: TextShadowColor,
    pub(crate) offset_x: f32,
    pub(crate) offset_y: f32,
    pub(crate) blur_radius: f32,
    pub(crate) spread: f32,
    pub(crate) inset: bool,
}

/// Color component of a computed CSS `text-shadow`.
///
/// CSS Color defines `currentColor` as the element's own computed `color`.
/// Since `text-shadow` inherits, `currentColor` must remain symbolic until the
/// inheriting element paints the shadow:
/// <https://www.w3.org/TR/css-color-3/#currentcolor>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TextShadowColor {
    CurrentColor,
    Color(Color),
}

impl TextShadowColor {
    pub(crate) fn resolve(self, current_color: Color) -> Color {
        match self {
            Self::CurrentColor => current_color,
            Self::Color(color) => color,
        }
    }
}

/// Computed CSS `text-underline-offset`.
///
/// CSS Text Decoration Level 4 defines underline offset as `auto` or a
/// length/percentage:
/// <https://www.w3.org/TR/css-text-decor-4/#text-underline-offset-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TextUnderlineOffset {
    Auto,
    LengthPercentage(ComputedLengthPercentage),
}

impl TextUnderlineOffset {
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

/// Computed CSS `text-underline-position`.
///
/// CSS Text Decoration defines underline placement keywords, including the
/// horizontal-writing `under` value and vertical-writing side keywords:
/// <https://www.w3.org/TR/css-text-decor-3/#text-underline-position-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextUnderlinePosition {
    pub auto: bool,
    pub under: bool,
    pub left: bool,
    pub right: bool,
}

impl TextUnderlinePosition {
    pub(crate) const AUTO: Self = Self {
        auto: true,
        under: false,
        left: false,
        right: false,
    };
}
