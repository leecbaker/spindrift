//! Typed scalar units used across CSS computation, layout, paint, and PDF output.
//!
//! Quire's canonical layout scalar is a CSS computed absolute length measured
//! in PDF points. CSS Values and Units defines `1in = 96px`, while PDF default
//! user space uses 72 points per inch, so `1px = 0.75pt`:
//! <https://www.w3.org/TR/css-values-4/#absolute-lengths> and
//! ISO 32000-2:2020, 8.3 "Coordinate Systems".

/// Marker for Quire's canonical computed/layout length unit.
///
/// Values are stored numerically as PDF points. This names the scalar unit
/// separately from coordinate spaces such as paint space and PDF user space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutUnit {}

/// Marker for decoded raster image pixels.
///
/// Raster source pixels are image buffer coordinates, not CSS px or PDF
/// points. Keep them typed at the boundary so natural image dimensions must be
/// explicitly converted before entering layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RasterPixelUnit {}

/// Marker for a CSS content-box length or size.
///
/// This is still stored in Quire's PDF-point layout scalar, but the marker
/// records the CSS box-model semantic space. Keeping content-box values
/// distinct from border-box values makes padding and border expansion explicit:
/// <https://www.w3.org/TR/css-box-3/#content-box> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentBoxUnit {}

/// Marker for a CSS border-box length or size.
///
/// This is a semantic coordinate space over Quire's PDF-point layout scalar,
/// not a different physical unit. Conversions to or from content-box values
/// must explicitly add or subtract padding and border widths:
/// <https://www.w3.org/TR/css-box-3/#border-box> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BorderBoxUnit {}

/// Marker for padding plus border extents used in box-model conversions.
///
/// Non-content lengths are CSS layout lengths measured in PDF points. They
/// intentionally do not share a unit marker with content-box or border-box
/// values, so callers must choose an explicit conversion helper:
/// <https://www.w3.org/TR/css-box-3/#box-model>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NonContentUnit {}

/// A CSS computed absolute length in Quire's canonical layout unit.
pub(crate) type LayoutLength = euclid::Length<f32, LayoutUnit>;

/// A CSS computed size in Quire's canonical layout unit.
pub(crate) type LayoutSize = euclid::Size2D<f32, LayoutUnit>;

/// A decoded raster image size in source pixels.
pub(crate) type RasterPixelSize = euclid::Size2D<u32, RasterPixelUnit>;

/// A CSS content-box length in Quire's PDF-point layout scalar.
pub(crate) type ContentBoxLength = euclid::Length<f32, ContentBoxUnit>;

/// A CSS border-box length in Quire's PDF-point layout scalar.
pub(crate) type BorderBoxLength = euclid::Length<f32, BorderBoxUnit>;

/// Padding plus border extent in Quire's PDF-point layout scalar.
pub(crate) type NonContentLength = euclid::Length<f32, NonContentUnit>;

/// A CSS content-box size in Quire's PDF-point layout scalar.
pub(crate) type ContentBoxSize = euclid::Size2D<f32, ContentBoxUnit>;

/// A CSS border-box size in Quire's PDF-point layout scalar.
pub(crate) type BorderBoxSize = euclid::Size2D<f32, BorderBoxUnit>;

/// Construct a layout length from PDF points.
pub(crate) const fn layout_pt(value: f32) -> LayoutLength {
    LayoutLength::new(value)
}

/// Construct a layout length from CSS pixels.
pub(crate) const fn layout_px(value: f32) -> LayoutLength {
    layout_pt(value * crate::css::CSS_PX_TO_PT)
}

/// Construct a content-box length from PDF points.
pub(crate) const fn content_box_pt(value: f32) -> ContentBoxLength {
    ContentBoxLength::new(value)
}

/// Construct a border-box length from PDF points.
pub(crate) const fn border_box_pt(value: f32) -> BorderBoxLength {
    BorderBoxLength::new(value)
}

/// Construct a padding-plus-border length from PDF points.
pub(crate) const fn non_content_pt(value: f32) -> NonContentLength {
    NonContentLength::new(value)
}

/// Construct a content-box size from PDF points.
pub(crate) const fn content_box_size_pt(width: f32, height: f32) -> ContentBoxSize {
    ContentBoxSize::new(width, height)
}

/// Construct a border-box size from PDF points.
pub(crate) const fn border_box_size_pt(width: f32, height: f32) -> BorderBoxSize {
    BorderBoxSize::new(width, height)
}

/// Return the numeric PDF-point value of a layout length.
pub(crate) fn layout_points(length: LayoutLength) -> f32 {
    length.get()
}

/// Re-label a generic layout length as a content-box length without changing
/// its PDF-point value.
///
/// The caller establishes that the value denotes a content-box extent; this
/// helper does not remove padding or border.
pub(crate) fn layout_to_content_box_length(length: LayoutLength) -> ContentBoxLength {
    length.cast_unit()
}

/// Re-label a generic layout length as a border-box length without changing
/// its PDF-point value.
///
/// The caller establishes that the value denotes a border-box extent; this
/// helper does not add padding or border.
pub(crate) fn layout_to_border_box_length(length: LayoutLength) -> BorderBoxLength {
    length.cast_unit()
}

/// Extract the numeric PDF-point value from a typed layout length.
pub(crate) trait SemanticLengthExt {
    /// Return this typed length in Quire's canonical PDF-point layout scalar.
    fn points(self) -> f32;
}

impl<Unit> SemanticLengthExt for euclid::Length<f32, Unit> {
    fn points(self) -> f32 {
        self.get()
    }
}

/// Re-label a semantic CSS length as a generic layout length without changing
/// its PDF-point value.
///
/// This is only a unit-marker conversion. It never performs box-model
/// arithmetic; use the named content-box/border-box helpers when padding and
/// border must be added or removed.
pub(crate) trait IntoLayoutLength {
    /// Return this length as Quire's generic layout length.
    fn into_layout_length(self) -> LayoutLength;
}

impl IntoLayoutLength for LayoutLength {
    fn into_layout_length(self) -> LayoutLength {
        self
    }
}

impl IntoLayoutLength for ContentBoxLength {
    fn into_layout_length(self) -> LayoutLength {
        self.cast_unit()
    }
}

impl IntoLayoutLength for BorderBoxLength {
    fn into_layout_length(self) -> LayoutLength {
        self.cast_unit()
    }
}

impl IntoLayoutLength for NonContentLength {
    fn into_layout_length(self) -> LayoutLength {
        self.cast_unit()
    }
}

/// A CSS percentage-resolution basis that may or may not be definite.
///
/// CSS Sizing resolves percentage components only when the relevant containing
/// block axis is definite. The source records why a definite basis exists at
/// the layout boundary:
/// <https://www.w3.org/TR/css-sizing-3/#definite>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PercentageBasis<T, Source = ()> {
    Definite { value: T, source: Source },
    Indefinite,
}

impl<T> PercentageBasis<T, ()> {
    pub(crate) fn definite(value: T) -> Self {
        Self::Definite { value, source: () }
    }
}

impl<T, Source> PercentageBasis<T, Source> {
    pub(crate) fn definite_from(value: T, source: Source) -> Self {
        Self::Definite { value, source }
    }

    pub(crate) fn indefinite() -> Self {
        Self::Indefinite
    }

    pub(crate) fn is_definite(&self) -> bool {
        matches!(self, Self::Definite { .. })
    }

    pub(crate) fn value(self) -> Option<T> {
        match self {
            Self::Definite { value, .. } => Some(value),
            Self::Indefinite => None,
        }
    }

    pub(crate) fn map_value<U>(self, map: impl FnOnce(T) -> U) -> PercentageBasis<U, Source> {
        match self {
            Self::Definite { value, source } => PercentageBasis::Definite {
                value: map(value),
                source,
            },
            Self::Indefinite => PercentageBasis::Indefinite,
        }
    }
}

impl<T, Source> PercentageBasis<T, Source>
where
    T: SemanticLengthExt,
{
    pub(crate) fn points(self) -> Option<f32> {
        self.value().map(SemanticLengthExt::points)
    }
}

/// Expand a content-box length by padding and border extents.
pub(crate) fn content_box_to_border_box_length(
    content: ContentBoxLength,
    extras: NonContentLength,
) -> BorderBoxLength {
    border_box_pt((content.points() + extras.points()).max(0.0))
}

/// Shrink a border-box length by padding and border extents, clamping at zero.
pub(crate) fn border_box_to_content_box_length(
    border: BorderBoxLength,
    extras: NonContentLength,
) -> ContentBoxLength {
    content_box_pt((border.points() - extras.points()).max(0.0))
}

/// Expand a content-box size by horizontal and vertical padding/border extents.
pub(crate) fn content_box_to_border_box_size(
    content: ContentBoxSize,
    horizontal_extras: NonContentLength,
    vertical_extras: NonContentLength,
) -> BorderBoxSize {
    border_box_size_pt(
        (content.width + horizontal_extras.points()).max(0.0),
        (content.height + vertical_extras.points()).max(0.0),
    )
}

/// Shrink a border-box size by horizontal and vertical padding/border extents.
///
/// The result clamps at zero on each axis, matching CSS used-value behavior for
/// over-constrained content boxes.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn border_box_to_content_box_size(
    border: BorderBoxSize,
    horizontal_extras: NonContentLength,
    vertical_extras: NonContentLength,
) -> ContentBoxSize {
    content_box_size_pt(
        (border.width - horizontal_extras.points()).max(0.0),
        (border.height - vertical_extras.points()).max(0.0),
    )
}

/// Convert decoded raster source pixels into natural CSS layout dimensions.
///
/// CSS Values fixes `1px = 1/96in`, while Quire's layout unit is PDF points,
/// so each natural raster pixel contributes `0.75pt`:
/// <https://www.w3.org/TR/css-values-4/#absolute-lengths>.
pub(crate) fn raster_natural_layout_size(size: RasterPixelSize) -> LayoutSize {
    LayoutSize::new(
        layout_points(layout_px(size.width as f32)),
        layout_points(layout_px(size.height as f32)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_relabeling_preserves_points_without_box_model_arithmetic() {
        let layout: LayoutLength = content_box_pt(42.0).into_layout_length();
        let content: ContentBoxLength = layout_to_content_box_length(layout);

        assert_eq!(layout, layout_pt(42.0));
        assert_eq!(content, content_box_pt(42.0));
        assert_eq!(border_box_pt(42.0).into_layout_length(), layout_pt(42.0));
        assert_eq!(non_content_pt(42.0).into_layout_length(), layout_pt(42.0));
    }

    #[test]
    fn content_box_size_expands_to_border_box_size() {
        let border = content_box_to_border_box_size(
            content_box_size_pt(150.0, 150.0),
            non_content_pt(150.0),
            non_content_pt(150.0),
        );
        assert_eq!(border.width, 300.0);
        assert_eq!(border.height, 300.0);
    }

    #[test]
    fn border_box_size_shrinks_to_non_negative_content_box_size() {
        let content = border_box_to_content_box_size(
            border_box_size_pt(100.0, 100.0),
            non_content_pt(150.0),
            non_content_pt(150.0),
        );
        assert_eq!(content.width, 0.0);
        assert_eq!(content.height, 0.0);
    }
}
