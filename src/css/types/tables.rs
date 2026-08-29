use crate::css::types::{
    ComputedLengthPercentage, ResolveViewportLengths, RootFontMetricLengthBasis,
    ViewportLengthBasis,
};
use crate::units::LayoutLength;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BorderCollapse {
    Separate,
    Collapse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptionSide {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableLayout {
    Auto,
    Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmptyCells {
    Show,
    Hide,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderSpacing {
    pub horizontal: ComputedLengthPercentage,
    pub vertical: ComputedLengthPercentage,
}

/// Cascaded table border spacing together with the provenance HTML table
/// layout needs to decide whether a `cellspacing` attribute may supply its
/// compatibility fallback.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CascadedTableBorderSpacing {
    AuthorDeclared(BorderSpacing),
    NonAuthor(BorderSpacing),
}

impl CascadedTableBorderSpacing {
    pub(crate) const INITIAL: Self = Self::NonAuthor(BorderSpacing::ZERO);

    pub(crate) const fn is_author_declared(&self) -> bool {
        matches!(self, Self::AuthorDeclared(_))
    }

    pub(crate) const fn value(&self) -> &BorderSpacing {
        match self {
            Self::AuthorDeclared(value) | Self::NonAuthor(value) => value,
        }
    }

    pub(crate) fn value_mut(&mut self) -> &mut BorderSpacing {
        match self {
            Self::AuthorDeclared(value) | Self::NonAuthor(value) => value,
        }
    }

    pub(crate) fn from_declaration(value: BorderSpacing, author_declared: bool) -> Self {
        if author_declared {
            Self::AuthorDeclared(value)
        } else {
            Self::NonAuthor(value)
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.value_mut().resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.value_mut().resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.value().requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.value().requires_root_font_metrics()
    }

    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        self.value_mut().scale_fixed_length_components(factor);
    }
}

impl std::ops::Deref for CascadedTableBorderSpacing {
    type Target = BorderSpacing;

    fn deref(&self) -> &Self::Target {
        self.value()
    }
}

impl ResolveViewportLengths for CascadedTableBorderSpacing {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.value_mut().resolve_viewport_lengths(basis);
    }
}

impl BorderSpacing {
    pub(crate) const ZERO: Self = Self {
        horizontal: ComputedLengthPercentage::ZERO,
        vertical: ComputedLengthPercentage::ZERO,
    };

    pub(crate) fn from_lengths(horizontal: f32, vertical: f32) -> Self {
        Self {
            horizontal: ComputedLengthPercentage::from_points(horizontal),
            vertical: ComputedLengthPercentage::from_points(vertical),
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.horizontal.resolve_font_metric_lengths(ch_advance);
        self.vertical.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.horizontal.resolve_root_font_metric_lengths(basis);
        self.vertical.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.horizontal.requires_ch_advance() || self.vertical.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.horizontal.requires_root_font_metrics() || self.vertical.requires_root_font_metrics()
    }

    /// Scale fixed border-spacing components at the CSS `zoom` used-value
    /// boundary.
    ///
    /// Percentage components remain relative to the table's already zoomed
    /// used geometry.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        self.horizontal.scale_fixed_length_components(factor);
        self.vertical.scale_fixed_length_components(factor);
    }
}

impl ResolveViewportLengths for BorderSpacing {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.horizontal.resolve_viewport_lengths(basis);
        self.vertical.resolve_viewport_lengths(basis);
    }
}
