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
pub enum LayoutUnit {}

/// Marker for decoded raster image pixels.
///
/// Raster source pixels are image buffer coordinates, not CSS px or PDF
/// points. Keep them typed at the boundary so natural image dimensions must be
/// explicitly converted before entering layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterPixelUnit {}

/// Marker for a CSS content-box length or size.
///
/// This is still stored in Quire's PDF-point layout scalar, but the marker
/// records the CSS box-model semantic space. Keeping content-box values
/// distinct from border-box values makes padding and border expansion explicit:
/// <https://www.w3.org/TR/css-box-3/#content-box> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentBoxUnit {}

/// Marker for a CSS border-box length or size.
///
/// This is a semantic coordinate space over Quire's PDF-point layout scalar,
/// not a different physical unit. Conversions to or from content-box values
/// must explicitly add or subtract padding and border widths:
/// <https://www.w3.org/TR/css-box-3/#border-box> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderBoxUnit {}

/// Marker for padding plus border extents used in box-model conversions.
///
/// Non-content lengths are CSS layout lengths measured in PDF points. They
/// intentionally do not share a unit marker with content-box or border-box
/// values, so callers must choose an explicit conversion helper:
/// <https://www.w3.org/TR/css-box-3/#box-model>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonContentUnit {}

/// A CSS computed absolute length in Quire's canonical layout unit.
pub type LayoutLength = euclid::Length<f32, LayoutUnit>;

/// A CSS computed size in Quire's canonical layout unit.
pub type LayoutSize = euclid::Size2D<f32, LayoutUnit>;

/// A decoded raster image size in source pixels.
pub type RasterPixelSize = euclid::Size2D<u32, RasterPixelUnit>;

/// A CSS content-box length in Quire's PDF-point layout scalar.
pub type ContentBoxLength = euclid::Length<f32, ContentBoxUnit>;

/// A CSS border-box length in Quire's PDF-point layout scalar.
pub type BorderBoxLength = euclid::Length<f32, BorderBoxUnit>;

/// Padding plus border extent in Quire's PDF-point layout scalar.
pub type NonContentLength = euclid::Length<f32, NonContentUnit>;

/// A CSS content-box size in Quire's PDF-point layout scalar.
pub type ContentBoxSize = euclid::Size2D<f32, ContentBoxUnit>;

/// A CSS border-box size in Quire's PDF-point layout scalar.
pub type BorderBoxSize = euclid::Size2D<f32, BorderBoxUnit>;

/// Construct a layout length from PDF points.
pub const fn layout_pt(value: f32) -> LayoutLength {
    LayoutLength::new(value)
}

/// Construct a layout length from CSS pixels.
pub const fn layout_px(value: f32) -> LayoutLength {
    layout_pt(value * crate::css::CSS_PX_TO_PT)
}

/// Construct a layout length from CSS inches.
pub const fn layout_in(value: f32) -> LayoutLength {
    layout_pt(value * 72.0)
}

/// Construct a content-box length from PDF points.
pub const fn content_box_pt(value: f32) -> ContentBoxLength {
    ContentBoxLength::new(value)
}

/// Construct a border-box length from PDF points.
pub const fn border_box_pt(value: f32) -> BorderBoxLength {
    BorderBoxLength::new(value)
}

/// Construct a padding-plus-border length from PDF points.
pub const fn non_content_pt(value: f32) -> NonContentLength {
    NonContentLength::new(value)
}

/// Construct a content-box size from PDF points.
pub const fn content_box_size_pt(width: f32, height: f32) -> ContentBoxSize {
    ContentBoxSize::new(width, height)
}

/// Construct a border-box size from PDF points.
pub const fn border_box_size_pt(width: f32, height: f32) -> BorderBoxSize {
    BorderBoxSize::new(width, height)
}

/// Return the numeric PDF-point value of a layout length.
pub fn layout_points(length: LayoutLength) -> f32 {
    length.get()
}

/// Extract the numeric PDF-point value from a typed layout length.
pub trait SemanticLengthExt {
    /// Return this typed length in Quire's canonical PDF-point layout scalar.
    fn points(self) -> f32;
}

impl<Unit> SemanticLengthExt for euclid::Length<f32, Unit> {
    fn points(self) -> f32 {
        self.get()
    }
}

/// Expand a content-box length by padding and border extents.
pub fn content_box_to_border_box_length(
    content: ContentBoxLength,
    extras: NonContentLength,
) -> BorderBoxLength {
    border_box_pt((content.points() + extras.points()).max(0.0))
}

/// Shrink a border-box length by padding and border extents, clamping at zero.
pub fn border_box_to_content_box_length(
    border: BorderBoxLength,
    extras: NonContentLength,
) -> ContentBoxLength {
    content_box_pt((border.points() - extras.points()).max(0.0))
}

/// Expand a content-box size by horizontal and vertical padding/border extents.
pub fn content_box_to_border_box_size(
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
pub fn border_box_to_content_box_size(
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
pub fn raster_natural_layout_size(size: RasterPixelSize) -> LayoutSize {
    LayoutSize::new(
        layout_points(layout_px(size.width as f32)),
        layout_points(layout_px(size.height as f32)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
