use crate::css::types::{ComputedLengthPercentage, RootFontMetricLengthBasis};

/// Computed CSS `overflow` value.
///
/// CSS Overflow defines `overflow` as a shorthand controlling whether content
/// that extends past the padding box is visible, clipped, or scrollable:
/// <https://www.w3.org/TR/css-overflow-3/#propdef-overflow>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Overflow {
    Visible,
    Hidden,
    Clip,
    Scroll,
    Auto,
}

/// Computed CSS Scroll Snap container policy.
///
/// Logical axes stay unresolved until layout maps them through the container's
/// writing mode.
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-snap-type>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScrollSnapType {
    #[default]
    None,
    X(ScrollSnapStrictness),
    Y(ScrollSnapStrictness),
    Block(ScrollSnapStrictness),
    Inline(ScrollSnapStrictness),
    Both(ScrollSnapStrictness),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollSnapStrictness {
    Mandatory,
    Proximity,
}

/// Per-logical-axis alignment contributed by a scroll snap area.
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-snap-align>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ScrollSnapAlign {
    pub(crate) block: ScrollSnapAlignment,
    pub(crate) inline: ScrollSnapAlignment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScrollSnapAlignment {
    #[default]
    None,
    Start,
    End,
    Center,
}

/// Directional scrolling trap policy. Static rendering retains it as a
/// computed value even though no directional operation occurs.
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-snap-stop>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScrollSnapStop {
    #[default]
    Normal,
    Always,
}

/// Whether an element establishes an explicit group of scroll-marker links.
///
/// <https://drafts.csswg.org/css-overflow-5/#propdef-scroll-target-group>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScrollTargetGroup {
    #[default]
    None,
    Auto,
}

/// A generated scroll-marker group owned by a scroll container.
///
/// The placement is intentionally part of the computed value: it determines
/// where the generated sibling participates in its parent's formatting
/// context, while the mode is retained for PDF metadata and future
/// interactive renderers.
/// <https://drafts.csswg.org/css-overflow-5/#propdef-scroll-marker-group>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollMarkerGroup {
    pub(crate) placement: ScrollMarkerGroupPlacement,
    pub(crate) mode: ScrollMarkerGroupMode,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollMarkerGroupPlacement {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScrollMarkerGroupMode {
    #[default]
    Links,
    Tabs,
}

/// One computed `scroll-padding-*` edge. `auto` remains distinct until used
/// values are resolved against a concrete scrollport.
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-padding>
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum ScrollPadding {
    #[default]
    Auto,
    LengthPercentage(ComputedLengthPercentage),
}

impl ScrollPadding {
    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_root_font_metrics())
    }
}

impl Overflow {
    pub(crate) fn clips_overflow(self) -> bool {
        !matches!(self, Self::Visible)
    }

    /// Returns whether this computed overflow value is scrollable.
    ///
    /// CSS Overflow classifies `hidden`, `scroll`, and `auto` as scrollable
    /// overflow values, while `visible` and `clip` are non-scrollable. CSS
    /// Flexbox uses that distinction when resolving automatic minimum sizes
    /// for flex items:
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-properties> and
    /// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
    pub(crate) fn is_scrollable(self) -> bool {
        matches!(self, Self::Hidden | Self::Scroll | Self::Auto)
    }
}
