use super::ComputedLengthPercentage;
use crate::css::types::{
    FontRelativeLengthBasis, ResolveViewportLengths, RootFontMetricLengthBasis, ViewportLengthBasis,
};
use crate::units::{LayoutLength, PercentageBasis, SemanticLengthExt, layout_points, layout_pt};

pub(crate) const ROOT_FONT_SIZE_PT: f32 = 12.0;

/// A `font-size` whose CSS font-relative components have not yet received
/// their parent font's used metrics.
///
/// Unlike ordinary `em` and `ch` lengths, the font-relative units in
/// `font-size` are relative to the parent element's font. Keeping that basis
/// explicit prevents the structural style phase from selecting a font merely
/// to cascade a descendant:
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
/// <https://www.w3.org/TR/css-fonts-4/#font-size-prop>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DeferredFontSize {
    Absolute(f32),
    /// An inherited computed size. Its numeric value is deliberately not
    /// copied from the pre-font structural phase: it must become the
    /// immediate parent's resolved used size.
    Inherit,
    RelativeToParent(ComputedLengthPercentage),
}

impl DeferredFontSize {
    pub(crate) const INITIAL: Self = Self::Absolute(ROOT_FONT_SIZE_PT);

    /// Resolves this value against the already-used parent font size and its
    /// selected-font `ch` advance.
    pub(crate) fn resolve(&self, parent: FontRelativeLengthBasis) -> LayoutLength {
        self.resolve_with_viewport(parent, None)
    }

    /// Resolves this value after the initial containing block's viewport is
    /// known. Viewport units in `font-size` are computed against that viewport
    /// before descendants use their inherited font metrics.
    /// <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
    pub(crate) fn resolve_with_viewport(
        &self,
        parent: FontRelativeLengthBasis,
        viewport: Option<ViewportLengthBasis>,
    ) -> LayoutLength {
        self.resolve_with_viewport_and_root_metrics(parent, viewport, None)
    }

    /// Resolve a deferred `font-size`, using the document root's selected
    /// metrics when root-relative metric units occur on a descendant.
    ///
    /// Root-relative font units are not fixed ratios of the root font size:
    /// their basis is the used root font. The root snapshot is only available
    /// once structural font-metric resolution has selected that face.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn resolve_with_viewport_and_root_metrics(
        &self,
        parent: FontRelativeLengthBasis,
        viewport: Option<ViewportLengthBasis>,
        root_metrics: Option<RootFontMetricLengthBasis>,
    ) -> LayoutLength {
        let parent_font_size = parent.font_size().points();
        let parent_ch_advance = parent.ch_advance();
        layout_pt(match self {
            Self::Absolute(value) => *value,
            Self::Inherit => parent_font_size,
            Self::RelativeToParent(value) => {
                let mut value = value.clone();
                if let Some(basis) = viewport {
                    value.resolve_viewport_lengths(basis);
                }
                value.resolve_font_relative_lengths(parent);
                value.resolve_font_metric_lengths(parent_ch_advance);
                value.resolve_ic_relative_lengths(parent.ic_advance());
                value.resolve_ex_relative_lengths(parent.x_height().points());
                value.resolve_cap_relative_lengths(parent.cap_height().points());
                value.resolve_line_height_relative_lengths(parent.line_height());
                if let Some(root_metrics) = root_metrics {
                    value.resolve_root_font_relative_lengths(root_metrics.font_size.points());
                    value.resolve_root_font_metric_lengths(root_metrics);
                } else {
                    // Resolving the root style itself cannot use a root
                    // snapshot yet. Preserve the CSS initial-metric fallback
                    // for that bootstrap case.
                    value.resolve_root_font_relative_lengths(ROOT_FONT_SIZE_PT);
                }
                // `font-size` resolves its font-relative units against the
                // parent selected font. `FontRelativeLengthBasis` retains the
                // CSS metric fallbacks for cascade-time callers that have not
                // selected a font yet:
                // <https://www.w3.org/TR/css-values-4/#ex>.
                value
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        parent_font_size,
                    )))
                    .map(layout_points)
                    .unwrap_or(parent_font_size)
            }
        })
    }

    /// Returns whether resolving this `font-size` requires the parent's used
    /// `ch` advance.
    ///
    /// CSS Fonts resolves font-relative units in `font-size` against the
    /// parent font. Deferred math is inspected structurally so a `ch` term
    /// hidden by the currently selected `min()`, `max()`, or `clamp()` branch
    /// still receives its required metric:
    /// <https://www.w3.org/TR/css-fonts-4/#font-size-prop> and
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn requires_parent_ch_advance(&self, _parent_font_size: f32) -> bool {
        match self {
            Self::Absolute(_) | Self::Inherit => false,
            Self::RelativeToParent(value) => value.requires_ch_advance(),
        }
    }

    /// Whether this `font-size` must receive the document-root metric
    /// snapshot after cascade has produced its provisional value.
    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::Absolute(_) | Self::Inherit => false,
            Self::RelativeToParent(value) => value.requires_root_font_metrics(),
        }
    }

    /// Whether this deferred value needs the document root's used font size.
    ///
    /// This is intentionally separate from [`Self::requires_root_font_metrics`]:
    /// `rem` depends on the root size but does not require a metric measured
    /// from the root's selected font.
    pub(crate) fn requires_document_root_font_size(&self) -> bool {
        match self {
            Self::Absolute(_) | Self::Inherit => false,
            Self::RelativeToParent(value) => value.requires_document_root_font_size(),
        }
    }

    /// Whether this `font-size` needs the parent selected font's metrics.
    pub(crate) fn requires_parent_selected_font_metrics(&self) -> bool {
        matches!(self, Self::RelativeToParent(value) if value.requires_parent_selected_font_metrics())
    }
}
