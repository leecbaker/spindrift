use crate::css::types::{
    ComputedLengthPercentage, Direction, ResolveViewportLengths, RootFontMetricLengthBasis,
    ViewportLengthBasis,
};
use crate::units::{LayoutLength, layout_pt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextAlign {
    Start,
    End,
    Left,
    Center,
    Right,
    Justify,
    JustifyAll,
}

impl TextAlign {
    /// Resolves logical `text-align` keywords to physical alignment.
    ///
    /// CSS Text defines `start` and `end` relative to the inline base
    /// direction of the block container:
    /// <https://www.w3.org/TR/css-text-3/#text-align-property>.
    pub(crate) fn physical(self, direction: Direction) -> Self {
        match self {
            Self::Start => logical_start_align(direction),
            Self::End => logical_end_align(direction),
            align => align,
        }
    }

    /// Return whether this value distributes inline content to fill the line.
    ///
    /// CSS Text defines both `justify` and `justify-all` as justification
    /// values. `justify-all` additionally affects the last line through
    /// `text-align-last: auto`:
    /// <https://www.w3.org/TR/css-text-3/#text-align-property>.
    pub(crate) fn justifies(self) -> bool {
        matches!(self, Self::Justify | Self::JustifyAll)
    }
}

/// Computed CSS `tab-size` value.
///
/// CSS Text Level 3 defines preserved tab advances as periodic tab stops,
/// initially every 8 spaces. Numeric values are resolved from the selected
/// font's U+0020 advance, while length values are already computed CSS layout
/// lengths:
/// <https://www.w3.org/TR/css-text-3/#tab-size-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TabSize {
    Spaces(f32),
    Length(ComputedLengthPercentage),
}

impl TabSize {
    pub(crate) const INITIAL: Self = Self::Spaces(8.0);

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::Length(length) = self {
            length.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::Length(length) = self {
            length.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Length(length) if length.requires_ch_advance())
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::Length(length) if length.requires_root_font_metrics())
    }

    pub(crate) fn used_tab_stop_advance(&self, space_advance: f32) -> LayoutLength {
        match self {
            Self::Spaces(columns) => layout_pt(*columns * space_advance),
            Self::Length(length) => length.fixed_component(),
        }
        .max(layout_pt(0.0))
    }
}

/// Computed CSS `text-align-last`.
///
/// CSS Text defines `text-align-last` as the alignment used for the last line
/// of a block or a line before a forced break; `auto` defers to
/// `text-align`, except that `text-align: justify` falls back to logical
/// start for the affected line, while `justify-all` keeps final-line
/// justification:
/// <https://www.w3.org/TR/css-text-3/#text-align-last-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextAlignLast {
    Auto,
    Align(TextAlign),
}

/// Computed CSS `text-justify`.
///
/// CSS Text defines the justification method used when `text-align: justify`
/// distributes remaining inline space:
/// <https://www.w3.org/TR/css-text-3/#text-justify-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextJustify {
    Auto,
    InterWord,
    InterCharacter,
    None,
}

pub(crate) fn logical_start_align(direction: Direction) -> TextAlign {
    match direction {
        Direction::Ltr => TextAlign::Left,
        Direction::Rtl => TextAlign::Right,
    }
}

pub(crate) fn logical_end_align(direction: Direction) -> TextAlign {
    match direction {
        Direction::Ltr => TextAlign::Right,
        Direction::Rtl => TextAlign::Left,
    }
}

impl ResolveViewportLengths for TabSize {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::Length(length) = self {
            length.resolve_viewport_lengths(basis);
        }
    }
}
