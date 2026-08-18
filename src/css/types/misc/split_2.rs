use super::*;

impl ElementSignature {
    pub fn new(tag: impl Into<String>, attrs: HashMap<String, String>) -> Self {
        Self {
            selector: ElementSiblingSignature::new(tag, attrs),
            sibling_index: None,
            sibling_signatures: ElementSiblingSignatureList::empty(),
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        }
    }

    pub(crate) fn from_selector_snapshot(selector: ElementSiblingSignature) -> Self {
        Self {
            selector,
            sibling_index: None,
            sibling_signatures: ElementSiblingSignatureList::empty(),
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        }
    }

    /// Return the null-namespace attribute addressed by an unprefixed CSS
    /// `attr()` name on this selector snapshot.
    ///
    /// This mirrors DOM lookup so computed-time typed `attr()` substitutions
    /// retain the host-language case semantics used by deferred generated
    /// content.
    /// <https://drafts.csswg.org/css-values-5/#attr-notation>
    pub(crate) fn unprefixed_css_attr(&self, name: &str) -> Option<&str> {
        self.namespace_attrs
            .iter()
            .find(|attribute| {
                crate::css::unprefixed_attr_name_matches(
                    &self.namespace_url,
                    self.document_is_html,
                    &attribute.namespace_url,
                    &attribute.local_name,
                    name,
                )
            })
            .map(|attribute| attribute.value.as_str())
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
            ElementSiblingSignatureList::from_vec(
                sibling_signatures.into_iter().map(Into::into).collect(),
            ),
        )
    }

    pub(crate) fn with_sibling_list(
        tag: impl Into<String>,
        attrs: HashMap<String, String>,
        sibling_index: usize,
        sibling_signatures: ElementSiblingSignatureList,
    ) -> Self {
        let selected_sibling = sibling_signatures.get(sibling_index);
        // A signature reconstructed from a sibling list can itself later be
        // an ancestor in a selector chain. Preserve its complete selector
        // snapshot, including children, so relational selectors such as
        // `:has(> .match)` inspect the source DOM rather than an empty shell.
        // <https://drafts.csswg.org/selectors-4/#relational>
        let mut selector = ElementSiblingSignature::new(tag, attrs);
        if let Some(selected_sibling) = selected_sibling {
            // Callers can intentionally supply selector-local tag/attribute
            // data that differs from a sibling template.  Retain that public
            // construction behavior while sharing the template's recursive
            // source metadata.
            selector.namespace_url = selected_sibling.namespace_url.clone();
            selector.document_is_html = selected_sibling.document_is_html;
            selector.namespace_attrs = selected_sibling.namespace_attrs.clone();
            selector.opaque_id = Rc::clone(&selected_sibling.opaque_id);
            selector.source_element_id = selected_sibling.source_element_id;
            selector.children = selected_sibling.children.clone();
            selector.has_text_child = selected_sibling.has_text_child;
            selector.is_target = selected_sibling.is_target;
            selector.has_target_descendant = selected_sibling.has_target_descendant;
            selector.link_state = selected_sibling.link_state;
            selector.document_direction = selected_sibling.document_direction;
        }
        Self {
            selector,
            sibling_index: Some(sibling_index),
            sibling_signatures,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        }
    }

    /// Reconstruct an element solely from a cached sibling snapshot.
    ///
    /// Source-DOM layout always has an entry for its sibling index, so this
    /// path avoids cloning tag, attribute, namespace, and child metadata.
    pub(crate) fn from_sibling_snapshot(
        sibling_index: usize,
        sibling_signatures: ElementSiblingSignatureList,
    ) -> Option<Self> {
        let selector = sibling_signatures.get(sibling_index)?.clone();
        Some(Self {
            selector,
            sibling_index: Some(sibling_index),
            sibling_signatures,
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_link_state(mut self, link_state: LinkState) -> Self {
        self.selector = self.selector.with_link_state(link_state);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_child_list(
        mut self,
        children: ElementSiblingSignatureList,
        has_text_child: bool,
    ) -> Self {
        self.selector = self.selector.with_child_list(children, has_text_child);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_children<Sibling>(self, children: Vec<Sibling>, has_text_child: bool) -> Self
    where
        Sibling: Into<ElementSiblingSignature>,
    {
        self.with_child_list(
            ElementSiblingSignatureList::from_vec(children.into_iter().map(Into::into).collect()),
            has_text_child,
        )
    }

    #[cfg(test)]
    pub(crate) fn with_namespace(
        mut self,
        namespace_url: impl Into<String>,
        namespace_attrs: Vec<ElementAttributeSignature>,
    ) -> Self {
        self.selector = self.selector.with_namespace(namespace_url, namespace_attrs);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_document_is_html(mut self, document_is_html: bool) -> Self {
        self.selector = self.selector.with_document_is_html(document_is_html);
        self
    }

    /// Attach HTML/document directionality for selector matching.
    ///
    /// Selectors `:dir()` uses the host language's directionality, not CSS
    /// `direction`; undefined directionality inherits during selector matching:
    /// <https://drafts.csswg.org/selectors/#the-dir-pseudo> and
    /// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality>.
    pub(crate) fn with_document_direction(mut self, direction: Direction) -> Self {
        self.selector = self.selector.with_document_direction(direction);
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
            selector: sibling,
            sibling_index: Some(index),
            sibling_signatures: self.sibling_signatures.clone(),
            html_direction: None,
            resolved_direction: None,
            resolved_language: ResolvedLanguage::Unresolved,
        })
    }

    pub(crate) fn child_at(&self, index: usize) -> Option<Self> {
        let child = self.children.get(index)?.clone();
        Some(Self {
            selector: child,
            sibling_index: Some(index),
            sibling_signatures: self.children.clone(),
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
    pub color: CssColorOrCurrentColor,
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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.thickness.resolve_root_font_metric_lengths(basis);
        self.underline_offset
            .resolve_root_font_metric_lengths(basis);
        self.inset.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.thickness.requires_ch_advance()
            || self.underline_offset.requires_ch_advance()
            || self.inset.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.thickness.requires_root_font_metrics()
            || self.underline_offset.requires_root_font_metrics()
            || self.inset.requires_root_font_metrics()
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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_root_font_metrics())
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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::Lengths { start, end } = self {
            start.resolve_root_font_metric_lengths(basis);
            end.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Lengths { start, end } if start.requires_ch_advance() || end.requires_ch_advance())
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::Lengths { start, end } if start.requires_root_font_metrics() || end.requires_root_font_metrics())
    }

    /// Resolve `text-decoration-inset` after the decorating box has supplied
    /// its logical inline-size percentage basis.
    ///
    /// Percentages do not refer to the font size: CSS Text Decoration uses
    /// the decorating box's total inline size for `slice`, or an individual
    /// box fragment's inline size for `clone`.
    /// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-inset-property>
    pub(crate) fn used(self, percentage_basis: LayoutLength, auto_font_size: f32) -> (f32, f32) {
        match self {
            Self::Auto => (auto_font_size * 0.125, auto_font_size * 0.125),
            Self::Lengths { start, end } => (
                start
                    .used_length_with_percentage_basis(PercentageBasis::definite(percentage_basis))
                    .map(layout_points)
                    .unwrap_or(start.length_points()),
                end.used_length_with_percentage_basis(PercentageBasis::definite(percentage_basis))
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
pub(crate) enum TextDecorationSkipSpaces {
    None,
    Start,
    End,
    StartEnd,
    All,
}

impl TextDecorationSkipSpaces {
    pub(crate) const NONE: Self = Self::None;
    pub(crate) const START_END: Self = Self::StartEnd;
    pub(crate) const ALL: Self = Self::All;

    pub(crate) fn skips_line_start(self) -> bool {
        matches!(self, Self::Start | Self::StartEnd | Self::All)
    }

    pub(crate) fn skips_line_end(self) -> bool {
        matches!(self, Self::End | Self::StartEnd | Self::All)
    }

    pub(crate) fn skips_all(self) -> bool {
        matches!(self, Self::All)
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
    /// Apply CSS `zoom` to the fixed components of one shadow layer.
    ///
    /// Percentages remain relative to the already zoomed box, while the
    /// length component of each shadow metric is multiplied at the used-value
    /// boundary.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        self.offset_x.scale_fixed_length_components(factor);
        self.offset_y.scale_fixed_length_components(factor);
        self.blur_radius.scale_fixed_length_components(factor);
        self.spread.scale_fixed_length_components(factor);
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.offset_x.resolve_font_metric_lengths(ch_advance);
        self.offset_y.resolve_font_metric_lengths(ch_advance);
        self.blur_radius.resolve_font_metric_lengths(ch_advance);
        self.spread.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.offset_x.resolve_root_font_metric_lengths(basis);
        self.offset_y.resolve_root_font_metric_lengths(basis);
        self.blur_radius.resolve_root_font_metric_lengths(basis);
        self.spread.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.offset_x.requires_ch_advance()
            || self.offset_y.requires_ch_advance()
            || self.blur_radius.requires_ch_advance()
            || self.spread.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.offset_x.requires_root_font_metrics()
            || self.offset_y.requires_root_font_metrics()
            || self.blur_radius.requires_root_font_metrics()
            || self.spread.requires_root_font_metrics()
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
    /// Apply CSS `zoom` to the fixed components of one shadow layer.
    ///
    /// Percentages remain relative to the already zoomed text metrics, while
    /// the length component of each shadow metric is multiplied at the
    /// used-value boundary.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        self.offset_x.scale_fixed_length_components(factor);
        self.offset_y.scale_fixed_length_components(factor);
        self.blur_radius.scale_fixed_length_components(factor);
        self.spread.scale_fixed_length_components(factor);
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.offset_x.resolve_font_metric_lengths(ch_advance);
        self.offset_y.resolve_font_metric_lengths(ch_advance);
        self.blur_radius.resolve_font_metric_lengths(ch_advance);
        self.spread.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.offset_x.resolve_root_font_metric_lengths(basis);
        self.offset_y.resolve_root_font_metric_lengths(basis);
        self.blur_radius.resolve_root_font_metric_lengths(basis);
        self.spread.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.offset_x.requires_ch_advance()
            || self.offset_y.requires_ch_advance()
            || self.blur_radius.requires_ch_advance()
            || self.spread.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.offset_x.requires_root_font_metrics()
            || self.offset_y.requires_root_font_metrics()
            || self.blur_radius.requires_root_font_metrics()
            || self.spread.requires_root_font_metrics()
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

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_ch_advance())
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_root_font_metrics())
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
