use super::*;

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
    /// Suppress intrinsic contributions only on the element's logical inline
    /// axis. This remains distinct from `size`, which suppresses both axes.
    /// <https://drafts.csswg.org/css-contain-3/#valdef-contain-inline-size>
    pub(crate) inline_size: bool,
    pub(crate) size: bool,
}

/// Computed physical fallback sizes supplied to a size-contained box.
///
/// CSS Sizing treats these as intrinsic contributions only while size
/// containment suppresses real descendants.
/// <https://drafts.csswg.org/css-sizing-4/#intrinsic-size-override>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContainIntrinsicSize {
    pub(crate) width: Option<ComputedLengthPercentage>,
    pub(crate) height: Option<ComputedLengthPercentage>,
}

impl ContainIntrinsicSize {
    pub(crate) const NONE: Self = Self {
        width: None,
        height: None,
    };

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Some(width) = &mut self.width {
            width.resolve_root_font_metric_lengths(basis);
        }
        if let Some(height) = &mut self.height {
            height.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.width
            .as_ref()
            .is_some_and(ComputedLengthPercentage::requires_root_font_metrics)
            || self
                .height
                .as_ref()
                .is_some_and(ComputedLengthPercentage::requires_root_font_metrics)
    }
}

impl Contain {
    pub(crate) const NONE: Self = Self {
        layout: false,
        paint: false,
        style: false,
        inline_size: false,
        size: false,
    };
}

/// Computed CSS Containment query-container capability.
///
/// `inline-size` containers expose only their logical inline axis; `size`
/// containers expose both axes. The layout pass additionally verifies that the
/// element generates an eligible principal box before using this declaration
/// as a query container.
/// <https://www.w3.org/TR/css-contain-3/#container-type>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ContainerType {
    #[default]
    Normal,
    InlineSize,
    Size,
}

/// Validated list of names advertised by a CSS query container.
///
/// The CSS-wide and `none` keywords do not name a container; parsing rejects
/// them rather than carrying an invalid identifier into container selection.
/// <https://www.w3.org/TR/css-contain-3/#container-name>
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ContainerNames(pub(crate) Vec<String>);

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
