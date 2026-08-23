use std::rc::Rc;

use super::*;

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

impl ResolveViewportLengths for TextUnderlineOffset {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

/// One text-decoration origin carried through eligible in-flow descendants.
///
/// CSS text decorations do not inherit, but their lines propagate through
/// in-flow descendants. The source font context must therefore accompany the
/// decoration: `auto`, `from-font`, and percentage values are resolved using
/// the decorating box, never a descendant that merely supplies glyphs.
///
/// The retained snapshot has an empty layer list, making this provenance
/// finite rather than a recursively nested computed-style tree.
/// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextDecorationLayer {
    pub(crate) decoration: TextDecoration,
    pub(crate) origin_style: Rc<ComputedStyle>,
}

/// The decoration origins available to a style at a layout boundary.
///
/// The origin established by the box itself is distinct from decorations
/// propagated from eligible in-flow ancestors. Keeping them separate means a
/// used-style update can replace its inherited paint state without discarding
/// the box's independently originating decoration.
/// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TextDecorationOrigins {
    propagated: Vec<TextDecorationLayer>,
    own: Option<TextDecorationLayer>,
}

impl TextDecorationOrigins {
    /// Origins in painting order: propagated ancestors, then this box.
    pub(crate) fn effective_layers(&self) -> impl Iterator<Item = &TextDecorationLayer> {
        self.propagated.iter().chain(self.own.iter())
    }

    /// Mutable origins in painting order.
    pub(crate) fn effective_layers_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut TextDecorationLayer> {
        self.propagated.iter_mut().chain(self.own.iter_mut())
    }

    pub(crate) fn effective_layers_vec(&self) -> Vec<TextDecorationLayer> {
        self.effective_layers().cloned().collect()
    }

    pub(crate) fn has_effective_layers(&self) -> bool {
        !self.propagated.is_empty() || self.own.is_some()
    }

    pub(crate) fn own_layer(&self) -> Option<&TextDecorationLayer> {
        self.own.as_ref()
    }

    pub(crate) fn clear(&mut self) {
        self.propagated.clear();
        self.own = None;
    }

    #[cfg(test)]
    pub(crate) fn set_propagated(&mut self, layers: Vec<TextDecorationLayer>) {
        self.propagated = layers;
    }

    /// Store effective layers received from a parent context without storing
    /// this style's own origin twice.
    pub(crate) fn set_propagated_from_effective(&mut self, layers: &[TextDecorationLayer]) {
        self.propagated = layers
            .iter()
            .filter(|layer| {
                self.own
                    .as_ref()
                    .is_none_or(|own| !Rc::ptr_eq(&layer.origin_style, &own.origin_style))
            })
            .cloned()
            .collect();
    }

    pub(crate) fn clear_own(&mut self) {
        self.own = None;
    }

    pub(crate) fn set_own(&mut self, layer: TextDecorationLayer) {
        self.own = Some(layer);
    }
}
