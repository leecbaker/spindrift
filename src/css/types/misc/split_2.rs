use super::*;

impl ElementSignature {
    pub fn new(tag: impl Into<String>, attrs: HashMap<String, String>) -> Self {
        let namespace_attrs = local_attribute_signatures(&attrs);
        Self {
            tag: tag.into(),
            namespace_url: String::new(),
            document_is_html: true,
            attrs,
            namespace_attrs,
            opaque_id: next_element_signature_opaque_id(),
            sibling_index: None,
            sibling_signatures: ElementSiblingSignatureList::empty(),
            child_signatures: ElementSiblingSignatureList::empty(),
            has_text_child: false,
            is_target: false,
            has_target_descendant: false,
            document_direction: None,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_siblings<Sibling>(
        tag: impl Into<String>,
        attrs: HashMap<String, String>,
        sibling_index: usize,
        sibling_signatures: Vec<Sibling>,
    ) -> Self
    where
        Sibling: Into<ElementSiblingSignature>,
    {
        Self::with_sibling_list(
            tag,
            attrs,
            sibling_index,
            ElementSiblingSignatureList::from_vec(sibling_signatures),
        )
    }

    pub(crate) fn with_sibling_list(
        tag: impl Into<String>,
        attrs: HashMap<String, String>,
        sibling_index: usize,
        sibling_signatures: ElementSiblingSignatureList,
    ) -> Self {
        let fallback_namespace_attrs = local_attribute_signatures(&attrs);
        let opaque_id = sibling_signatures
            .get(sibling_index)
            .map(|sibling| Rc::clone(&sibling.opaque_id))
            .unwrap_or_else(next_element_signature_opaque_id);
        let is_target = sibling_signatures
            .get(sibling_index)
            .is_some_and(|sibling| sibling.is_target);
        let has_target_descendant = sibling_signatures
            .get(sibling_index)
            .is_some_and(|sibling| sibling.has_target_descendant);
        let document_direction = sibling_signatures
            .get(sibling_index)
            .and_then(|sibling| sibling.document_direction);
        Self {
            tag: tag.into(),
            namespace_url: sibling_signatures
                .get(sibling_index)
                .map(|sibling| sibling.namespace_url.clone())
                .unwrap_or_default(),
            document_is_html: sibling_signatures
                .get(sibling_index)
                .map(|sibling| sibling.document_is_html)
                .unwrap_or(true),
            attrs,
            namespace_attrs: sibling_signatures
                .get(sibling_index)
                .map(|sibling| sibling.namespace_attrs.clone())
                .unwrap_or(fallback_namespace_attrs),
            opaque_id,
            sibling_index: Some(sibling_index),
            sibling_signatures,
            child_signatures: ElementSiblingSignatureList::empty(),
            has_text_child: false,
            is_target,
            has_target_descendant,
            document_direction,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        }
    }

    pub(crate) fn with_child_list(
        mut self,
        children: ElementSiblingSignatureList,
        has_text_child: bool,
    ) -> Self {
        self.child_signatures = children;
        self.has_text_child = has_text_child;
        self.has_target_descendant = self
            .child_signatures
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

    /// Attach HTML/document directionality for selector matching.
    ///
    /// Selectors `:dir()` uses the host language's directionality, not CSS
    /// `direction`; undefined directionality inherits during selector matching:
    /// <https://drafts.csswg.org/selectors/#the-dir-pseudo> and
    /// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality>.
    pub(crate) fn with_document_direction(mut self, direction: Direction) -> Self {
        self.document_direction = Some(direction);
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
    /// This is the inherited/computed `direction` available during cascade
    /// construction for layout-facing style resolution. It is intentionally not
    /// used for Selectors `:dir()`, which matches document-language
    /// directionality rather than CSS `direction`:
    /// <https://drafts.csswg.org/selectors/#the-dir-pseudo>.
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
            document_is_html: sibling.document_is_html,
            attrs: sibling.attrs,
            namespace_attrs: sibling.namespace_attrs,
            opaque_id: sibling.opaque_id,
            sibling_index: Some(index),
            sibling_signatures: self.sibling_signatures.clone(),
            child_signatures: sibling.children,
            has_text_child: sibling.has_text_child,
            is_target: sibling.is_target,
            has_target_descendant: sibling.has_target_descendant,
            document_direction: sibling.document_direction,
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
            document_is_html: child.document_is_html,
            attrs: child.attrs,
            namespace_attrs: child.namespace_attrs,
            opaque_id: child.opaque_id,
            sibling_index: Some(index),
            sibling_signatures: self.child_signatures.clone(),
            child_signatures: child.children,
            has_text_child: child.has_text_child,
            is_target: child.is_target,
            has_target_descendant: child.has_target_descendant,
            document_direction: child.document_direction,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
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
    pub color: Option<CssColor>,
}

impl TextDecoration {
    pub(crate) fn with_propagated_lines(self, _parent: Self) -> Self {
        self
    }

    pub(crate) fn has_visible_line(&self) -> bool {
        self.underline
            || self.overline
            || self.line_through
            || self.spelling_error
            || self.grammar_error
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.thickness.resolve_font_metric_lengths(ch_advance);
        self.underline_offset
            .resolve_font_metric_lengths(ch_advance);
        self.inset.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.thickness.requires_ch_advance()
            || self.underline_offset.requires_ch_advance()
            || self.inset.requires_ch_advance()
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TextDecorationThickness {
    Auto,
    FromFont,
    LengthPercentage(ComputedLengthPercentage),
}

impl TextDecorationThickness {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
    }
}

/// Computed CSS `text-decoration-inset`.
///
/// CSS Text Decoration Level 4 trims or extends the start and end endpoints of
/// line decorations:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-inset-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TextDecorationInset {
    Auto,
    Lengths {
        start: ComputedLengthPercentage,
        end: ComputedLengthPercentage,
    },
}

impl TextDecorationInset {
    pub(crate) const ZERO: Self = Self::Lengths {
        start: ComputedLengthPercentage::ZERO,
        end: ComputedLengthPercentage::ZERO,
    };

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::Lengths { start, end } = self {
            start.resolve_font_metric_lengths(ch_advance);
            end.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Lengths { start, end } if start.requires_ch_advance() || end.requires_ch_advance())
    }

    pub(crate) fn used(self, font_size: f32) -> (f32, f32) {
        match self {
            Self::Auto => (font_size * 0.125, font_size * 0.125),
            Self::Lengths { start, end } => (
                start
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        font_size,
                    )))
                    .map(layout_points)
                    .unwrap_or(start.length_points()),
                end.used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                    font_size,
                )))
                .map(layout_points)
                .unwrap_or(end.length_points()),
            ),
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
                let shape = shape.unwrap_or(match writing_mode.typographic_mode() {
                    TypographicMode::Horizontal => TextEmphasisShape::Circle,
                    TypographicMode::Vertical => TextEmphasisShape::Sesame,
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

/// Computed CSS `box-shadow` layer.
///
/// CSS Backgrounds and Borders Level 3 defines each shadow as a box-shaped
/// image outside or inside the border box, with the same geometry as the
/// border box unless offset, blur, or spread modifies it:
/// <https://www.w3.org/TR/css-backgrounds-3/#box-shadow>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoxShadow {
    pub(crate) color: BoxShadowColor,
    pub(crate) offset_x: ComputedLengthPercentage,
    pub(crate) offset_y: ComputedLengthPercentage,
    pub(crate) blur_radius: ComputedLengthPercentage,
    pub(crate) spread: ComputedLengthPercentage,
    pub(crate) inset: bool,
}

impl BoxShadow {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.offset_x.resolve_font_metric_lengths(ch_advance);
        self.offset_y.resolve_font_metric_lengths(ch_advance);
        self.blur_radius.resolve_font_metric_lengths(ch_advance);
        self.spread.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.offset_x.requires_ch_advance()
            || self.offset_y.requires_ch_advance()
            || self.blur_radius.requires_ch_advance()
            || self.spread.requires_ch_advance()
    }
}

/// CssColor component of a computed CSS `box-shadow`.
///
/// CSS CssColor defines `currentColor` as the element's own computed `color`.
/// `box-shadow` is not inherited, but `currentColor` still resolves against
/// the box that paints the shadow:
/// <https://www.w3.org/TR/css-color-3/#currentcolor>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BoxShadowColor {
    CurrentColor,
    CssColor(CssColor),
}

impl BoxShadowColor {
    pub(crate) fn resolve(self, current_color: CssColor) -> CssColor {
        match self {
            Self::CurrentColor => current_color,
            Self::CssColor(color) => color,
        }
    }
}

/// Computed CSS `text-shadow` layer.
///
/// CSS Text Decoration Level 4 follows the box-shadow grammar but applies
/// each shadow layer to text and decorations:
/// <https://drafts.csswg.org/css-text-decor-4/#text-shadow-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextShadow {
    pub(crate) color: TextShadowColor,
    pub(crate) offset_x: ComputedLengthPercentage,
    pub(crate) offset_y: ComputedLengthPercentage,
    pub(crate) blur_radius: ComputedLengthPercentage,
    pub(crate) spread: ComputedLengthPercentage,
    pub(crate) inset: bool,
}

impl TextShadow {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.offset_x.resolve_font_metric_lengths(ch_advance);
        self.offset_y.resolve_font_metric_lengths(ch_advance);
        self.blur_radius.resolve_font_metric_lengths(ch_advance);
        self.spread.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.offset_x.requires_ch_advance()
            || self.offset_y.requires_ch_advance()
            || self.blur_radius.requires_ch_advance()
            || self.spread.requires_ch_advance()
    }
}

/// CssColor component of a computed CSS `text-shadow`.
///
/// CSS CssColor defines `currentColor` as the element's own computed `color`.
/// Since `text-shadow` inherits, `currentColor` must remain symbolic until the
/// inheriting element paints the shadow:
/// <https://www.w3.org/TR/css-color-3/#currentcolor>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TextShadowColor {
    CurrentColor,
    CssColor(CssColor),
}

impl TextShadowColor {
    pub(crate) fn resolve(self, current_color: CssColor) -> CssColor {
        match self {
            Self::CurrentColor => current_color,
            Self::CssColor(color) => color,
        }
    }
}

/// Computed CSS `text-underline-offset`.
///
/// CSS Text Decoration Level 4 defines underline offset as `auto` or a
/// length/percentage:
/// <https://www.w3.org/TR/css-text-decor-4/#text-underline-offset-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TextUnderlineOffset {
    Auto,
    LengthPercentage(ComputedLengthPercentage),
}

impl TextUnderlineOffset {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
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

impl ResolveViewportLengths for TextDecoration {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.thickness.resolve_viewport_lengths(basis);
        self.underline_offset.resolve_viewport_lengths(basis);
        self.inset.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for TextDecorationThickness {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for TextDecorationInset {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::Lengths { start, end } = self {
            start.resolve_viewport_lengths(basis);
            end.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for BoxShadow {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.offset_x.resolve_viewport_lengths(basis);
        self.offset_y.resolve_viewport_lengths(basis);
        self.blur_radius.resolve_viewport_lengths(basis);
        self.spread.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for TextShadow {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.offset_x.resolve_viewport_lengths(basis);
        self.offset_y.resolve_viewport_lengths(basis);
        self.blur_radius.resolve_viewport_lengths(basis);
        self.spread.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for TextUnderlineOffset {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}
