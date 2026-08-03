/// A stroke thickness in page-local paint coordinates.
///
/// This is distinct from a point, size, or displacement in the same space:
/// a stroke's width is a scalar graphics-state property that applies
/// perpendicular to its path. PDF calls this the line width, while CSS and
/// SVG expose it as `stroke-width`:
/// <https://www.w3.org/TR/SVG2/painting.html#StrokeWidth> and
/// ISO 32000-2:2020, 8.4.3 "Line Width".
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintStrokeWidth(euclid::Length<f32, PaintSpace>);

impl PaintStrokeWidth {
    /// A paint primitive with no stroke thickness.
    pub const ZERO: Self = Self(euclid::Length::new(0.0));

    /// Construct a stroke width from page-local paint points.
    ///
    /// This deliberately preserves the supplied value. Source-specific CSS
    /// and SVG validation remains responsible for any non-negative
    /// constraints, matching the previous scalar representation.
    pub const fn new(points: f32) -> Self {
        Self(euclid::Length::new(points))
    }

    /// Return the numeric page-local paint-point value at a scalar boundary.
    pub fn points(self) -> f32 {
        self.0.get()
    }
}

/// Page-local paint coordinates before PDF serialization.
///
/// Paint primitives are expressed with a bottom-left origin and an upward
/// `y` axis. That matches PDF default user space for unrotated pages, but this
/// marker keeps the CSS painting model boundary distinct from final PDF
/// serialization and future page-rotation or form-XObject coordinate changes:
/// <https://www.w3.org/TR/css2/visuren.html#painting-order> and
/// ISO 32000-2:2020, 8.3 "Coordinate Systems".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintSpace {}

/// PDF default user-space coordinates.
///
/// For current unrotated page content streams this is numerically identical to
/// [`PaintSpace`]. Keeping a separate marker documents the serialization
/// boundary where page boxes, page rotation, and PDF form coordinate systems
/// would be applied:
/// ISO 32000-2:2020, 8.3 "Coordinate Systems".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PdfUserSpace {}

/// A point in page-local paint coordinates.
pub type PaintPoint = euclid::Point2D<f32, PaintSpace>;
/// A local physical displacement in page-local paint coordinates.
///
/// `x` moves right and `y` moves upward from the physical bottom-left of the
/// page. Use this for relative geometry such as shadow and box offsets; whole
/// paint-object relocation uses [`PaintTranslation`] instead:
/// <https://www.w3.org/TR/css2/visuren.html#painting-order>.
pub(crate) type PaintDisplacement = euclid::Vector2D<f32, PaintSpace>;
/// A same-space translation of a complete page-local paint object.
///
/// The source and destination markers make this an operation on paint
/// geometry, rather than a local distance vector:
/// <https://www.w3.org/TR/css2/visuren.html#painting-order>.
pub(crate) type PaintTranslation = euclid::Translation2D<f32, PaintSpace, PaintSpace>;
/// A size in page-local paint coordinates.
pub type PaintSize = euclid::Size2D<f32, PaintSpace>;
/// A bottom-left-origin rectangle in page-local paint coordinates.
pub type PaintRect = euclid::Rect<f32, PaintSpace>;
/// A point in PDF user space.
pub(crate) type PdfPoint = euclid::Point2D<f32, PdfUserSpace>;
/// A size in PDF user space.
pub(crate) type PdfSize = euclid::Size2D<f32, PdfUserSpace>;
/// A bottom-left-origin rectangle in PDF user space.
pub(crate) type PdfRect = euclid::Rect<f32, PdfUserSpace>;

/// Convert a paint-space rectangle into current PDF user-space coordinates.
///
/// This is intentionally identity today. It names the final boundary so future
/// page rotation, crop-box, or form-XObject conversions do not get embedded in
/// individual PDF drawing routines.
pub(crate) fn paint_rect_to_pdf(rect: PaintRect) -> PdfRect {
    PdfRect::new(
        PdfPoint::new(rect.origin.x, rect.origin.y),
        PdfSize::new(rect.size.width, rect.size.height),
    )
}

/// Convert a paint-space point into current PDF user-space coordinates.
///
/// Like [`paint_rect_to_pdf`], this is identity for current unrotated pages
/// but names the boundary for path, stroke, text, and annotation emission.
pub(crate) fn paint_point_to_pdf(point: PaintPoint) -> PdfPoint {
    PdfPoint::new(point.x, point.y)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintTransform(euclid::Transform2D<f32, PaintSpace, PaintSpace>);

impl PaintTransform {
    pub(crate) fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Self(euclid::Transform2D::new(a, b, c, d, e, f))
    }

    /// Adopt an affine transform which has already been resolved into
    /// bottom-left page paint coordinates.
    pub(crate) fn from_transform(
        transform: euclid::Transform2D<f32, PaintSpace, PaintSpace>,
    ) -> Self {
        Self(transform)
    }

    pub(crate) fn identity() -> Self {
        Self(euclid::Transform2D::identity())
    }

    /// Build a paint-space translation transform.
    ///
    /// CSS Transforms applies translation functions in the element's current
    /// painting coordinate system; by this point Quire has already projected
    /// layout geometry into [`PaintSpace`]:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-functions>.
    pub(crate) fn translate(offset: PaintTranslation) -> Self {
        Self(offset.to_transform())
    }

    pub(crate) fn multiply(self, right: Self) -> Self {
        // `Transform2D::then` applies its argument after its receiver. CSS
        // matrix multiplication applies `right` first, then `self`.
        Self(right.0.then(&self.0))
    }

    pub(crate) fn scale(x: f32, y: f32) -> Self {
        Self(euclid::Transform2D::scale(x, y))
    }

    pub(crate) fn a(self) -> f32 {
        self.0.m11
    }
    pub(crate) fn b(self) -> f32 {
        self.0.m12
    }
    pub(crate) fn c(self) -> f32 {
        self.0.m21
    }
    pub(crate) fn d(self) -> f32 {
        self.0.m22
    }
    pub(crate) fn e(self) -> f32 {
        self.0.m31
    }
    pub(crate) fn f(self) -> f32 {
        self.0.m32
    }

    pub(crate) fn pdf_components(self) -> [f32; 6] {
        [self.a(), self.b(), self.c(), self.d(), self.e(), self.f()]
    }

    /// Whether this transform keeps axis-aligned rectangles axis-aligned.
    ///
    /// PDF underpaint elimination may only reason about a transformed CSS
    /// rectangle as another rectangle when there is no rotation or skew.  A
    /// negative scale is still safe: [`Self::apply_clip_to_aabb`] preserves
    /// the resulting rectangle's bounds.
    pub(crate) fn preserves_axis_aligned_rectangles(self) -> bool {
        self.a().is_finite()
            && self.b().is_finite()
            && self.c().is_finite()
            && self.d().is_finite()
            && self.e().is_finite()
            && self.f().is_finite()
            && self.b() == 0.0
            && self.c() == 0.0
    }

    /// Returns whether this 2D matrix can establish a CSS current
    /// transformation matrix. Non-invertible matrices suppress painting.
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
    pub(crate) fn is_invertible(self) -> bool {
        let determinant = self.a() * self.d() - self.b() * self.c();
        determinant.is_finite() && determinant != 0.0
    }

    /// Apply this transform to a page-local paint point.
    ///
    /// CSS Transforms maps already-painted geometry into the parent painting
    /// coordinate system. Keeping the input and output as [`PaintPoint`]
    /// prevents transform effects from crossing into layout top-edge or PDF
    /// user-space coordinates by accident:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
    pub(crate) fn apply_point(self, point: PaintPoint) -> PaintPoint {
        self.0.transform_point(point)
    }

    /// Transform a paint-space clip rectangle and return its axis-aligned bounds.
    ///
    /// CSS rectangular clips are transformed with the element, while PDF
    /// annotation bounds and effect isolation need a conservative axis-aligned
    /// paint rectangle after transform application:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
    pub(crate) fn apply_clip_to_aabb(self, clip: PaintClip) -> PaintClip {
        let points = [
            self.apply_point(clip.bottom_left()),
            self.apply_point(clip.bottom_right()),
            self.apply_point(clip.top_left()),
            self.apply_point(clip.top_right()),
        ];
        let min_x = points
            .iter()
            .map(|point| point.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = points
            .iter()
            .map(|point| point.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = points
            .iter()
            .map(|point| point.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = points
            .iter()
            .map(|point| point.y)
            .fold(f32::NEG_INFINITY, f32::max);
        PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(min_x, min_y),
            PaintSize::new((max_x - min_x).max(0.0), (max_y - min_y).max(0.0)),
        ))
    }

    /// Express a parent paint-space clip in this transform's local coordinate
    /// system, returning conservative axis-aligned bounds.
    ///
    /// A PDF clip is fixed when it is installed. Axis-selective CSS clips use
    /// this conversion so their unbounded physical axis reaches the actual
    /// page boundary even when the element establishes a transformed local
    /// coordinate system.
    pub(crate) fn inverse_apply_clip_to_aabb(self, clip: PaintClip) -> Option<PaintClip> {
        let inverse = self.0.inverse()?;
        Some(Self::from_transform(inverse).apply_clip_to_aabb(clip))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintClip {
    pub(in crate::document) rect: PaintRect,
}

/// Retained union of disjoint rectangular clip regions. Table cells rarely
/// span many visible row fragments; preserving up to 8 regions avoids
/// approximating an interior collapsed row as a single bounding rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintClipUnion {
    clips: [PaintClip; Self::MAX_REGIONS],
    len: u8,
}

impl PaintClipUnion {
    pub(crate) const MAX_REGIONS: usize = 8;

    pub(crate) fn from_clips(clips: &[PaintClip]) -> Option<Self> {
        let first = *clips.first()?;
        let mut union = Self {
            clips: [first; Self::MAX_REGIONS],
            len: 0,
        };
        for clip in clips.iter().copied().take(Self::MAX_REGIONS) {
            union.clips[usize::from(union.len)] = clip;
            union.len += 1;
        }
        Some(union)
    }

    pub(crate) fn clips(&self) -> &[PaintClip] {
        &self.clips[..usize::from(self.len)]
    }

    pub(crate) fn translated(mut self, offset: PaintTranslation) -> Self {
        for clip in &mut self.clips[..usize::from(self.len)] {
            *clip = clip.translated(offset);
        }
        self
    }
}

impl PaintClip {
    pub(crate) fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self::from_paint_rect(PaintRect::new(
            PaintPoint::new(x, y),
            PaintSize::new(width.max(0.0), height.max(0.0)),
        ))
    }

    pub(crate) fn from_paint_rect(rect: PaintRect) -> Self {
        Self { rect }
    }

    pub(crate) fn x(self) -> f32 {
        self.rect.origin.x
    }

    pub(crate) fn y(self) -> f32 {
        self.rect.origin.y
    }

    pub(crate) fn width(self) -> f32 {
        self.rect.size.width
    }

    pub(crate) fn height(self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(self) -> PaintRect {
        self.rect
    }

    pub(crate) fn bottom_left(self) -> PaintPoint {
        self.rect.origin
    }

    pub(crate) fn bottom_right(self) -> PaintPoint {
        PaintPoint::new(self.x() + self.width(), self.y())
    }

    pub(crate) fn top_left(self) -> PaintPoint {
        PaintPoint::new(self.x(), self.y() + self.height())
    }

    pub(crate) fn top_right(self) -> PaintPoint {
        PaintPoint::new(self.x() + self.width(), self.y() + self.height())
    }

    pub(in crate::document) fn translated(self, offset: PaintTranslation) -> Self {
        Self::from_paint_rect(offset.transform_rect(&self.rect))
    }

    pub(crate) fn intersect(self, other: Self) -> Option<Self> {
        self.rect
            .intersection(&other.rect)
            .map(Self::from_paint_rect)
    }

    /// Exact closed-edge containment for an axis-aligned paint rectangle.
    ///
    /// This intentionally does not use a layout epsilon: the caller uses it
    /// to decide whether a PDF clip can be omitted without changing edge
    /// coverage.
    pub(crate) fn contains(self, other: Self) -> bool {
        other.x() >= self.x()
            && other.y() >= self.y()
            && other.x() + other.width() <= self.x() + self.width()
            && other.y() + other.height() <= self.y() + self.height()
    }
}

/// A CSS overflow clip whose physical axes remain independently bounded.
///
/// `PaintClip` is intentionally always a finite rectangle: it is also used
/// for page bounds, fragment slicing, and generic PDF clipping. CSS
/// `overflow-x:clip; overflow-y:visible` instead establishes an infinite
/// vertical strip. Keeping that distinction here prevents deferred paint
/// effects from accidentally turning the visible axis into a finite clip.
/// <https://drafts.csswg.org/css-overflow-3/#overflow-properties>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AxisSelectivePaintClip {
    bounds: PaintClip,
    clips_x: bool,
    clips_y: bool,
}

impl AxisSelectivePaintClip {
    pub(crate) const fn new(bounds: PaintClip, clips_x: bool, clips_y: bool) -> Self {
        Self {
            bounds,
            clips_x,
            clips_y,
        }
    }

    pub(crate) const fn clips_x(self) -> bool {
        self.clips_x
    }

    pub(crate) const fn clips_y(self) -> bool {
        self.clips_y
    }

    pub(crate) const fn bounds(self) -> PaintClip {
        self.bounds
    }

    pub(crate) fn translated(self, offset: PaintTranslation) -> Self {
        Self::new(self.bounds.translated(offset), self.clips_x, self.clips_y)
    }

    /// Lower this CSS clip into a finite PDF rectangle using page bounds only
    /// for axes CSS leaves unbounded. The caller installs it after the local
    /// transform, so both the clip and descendants share the same CTM.
    pub(crate) fn resolved_against_page(self, page: PaintClip) -> PaintClip {
        debug_assert!(self.clips_x() || self.clips_y());
        let left = if self.clips_x {
            self.bounds.x()
        } else {
            page.x()
        };
        let right = if self.clips_x {
            self.bounds.x() + self.bounds.width()
        } else {
            page.x() + page.width()
        };
        let bottom = if self.clips_y {
            self.bounds.y()
        } else {
            page.y()
        };
        let top = if self.clips_y {
            self.bounds.y() + self.bounds.height()
        } else {
            page.y() + page.height()
        };
        PaintClip::new(left, bottom, right - left, top - bottom)
    }
}

pub(in crate::document) fn rect_bounds(rect: PaintRect) -> Option<PaintClip> {
    (rect.size.width > 0.0 && rect.size.height > 0.0).then_some(PaintClip::from_paint_rect(rect))
}

/// Accumulates the page-local bounds of paint geometry, including points and
/// line segments with zero area.
///
/// [`PaintRect::union`] deliberately ignores empty rectangles, which is right
/// for rectangle geometry but would discard path endpoints while calculating
/// paint bounds. This accumulator instead extends its typed extrema with every
/// included point before producing a rectangle at the bounds boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::document) struct PaintBounds {
    min: PaintPoint,
    max: PaintPoint,
}

impl PaintBounds {
    #[cfg(test)]
    pub(in crate::document) fn from_paint_point(point: PaintPoint) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    pub(in crate::document) fn from_paint_rect(rect: PaintRect) -> Self {
        Self {
            min: rect.origin,
            max: PaintPoint::new(rect.max_x(), rect.max_y()),
        }
    }

    pub(in crate::document) fn include_paint_point(&mut self, point: PaintPoint) {
        self.min.x = self.min.x.min(point.x);
        self.min.y = self.min.y.min(point.y);
        self.max.x = self.max.x.max(point.x);
        self.max.y = self.max.y.max(point.y);
    }

    pub(in crate::document) fn include_paint_rect(&mut self, rect: PaintRect) {
        self.include_paint_point(rect.origin);
        self.include_paint_point(PaintPoint::new(rect.max_x(), rect.max_y()));
    }

    pub(in crate::document) fn paint_rect(self) -> PaintRect {
        PaintRect::new(
            self.min,
            PaintSize::new(self.max.x - self.min.x, self.max.y - self.min.y),
        )
    }

    pub(in crate::document) fn into_paint_clip(self) -> PaintClip {
        PaintClip::from_paint_rect(self.paint_rect())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AxisSelectivePaintClip, PaintBounds, PaintClip, PaintPoint, PaintRect, PaintSize,
        PaintTransform, PaintTranslation, PdfPoint, PdfRect, PdfSize, paint_point_to_pdf,
        paint_rect_to_pdf,
    };

    fn paint_rect(x: f32, y: f32, width: f32, height: f32) -> PaintRect {
        PaintRect::new(PaintPoint::new(x, y), PaintSize::new(width, height))
    }

    #[test]
    fn typed_affine_composition_keeps_css_matrix_order() {
        let transform = PaintTransform::translate(PaintTranslation::new(10.0, 20.0))
            .multiply(PaintTransform::scale(2.0, 3.0));

        assert_eq!(
            transform.apply_point(PaintPoint::new(1.0, 2.0)),
            PaintPoint::new(12.0, 26.0)
        );
    }

    #[test]
    fn paint_clip_round_trips_through_typed_rect() {
        let rect = paint_rect(10.0, 20.0, 30.0, 40.0);
        let clip = PaintClip::from_paint_rect(rect);

        assert_eq!(clip, PaintClip::new(10.0, 20.0, 30.0, 40.0));
        assert_eq!(clip.paint_rect(), rect);
    }

    #[test]
    fn paint_clip_translation_and_intersection_delegate_to_typed_rects() {
        let clip = PaintClip::new(10.0, 20.0, 30.0, 40.0);

        assert_eq!(
            clip.translated(PaintTranslation::new(-5.0, 7.0))
                .paint_rect(),
            paint_rect(5.0, 27.0, 30.0, 40.0),
        );
        assert_eq!(
            clip.intersect(PaintClip::new(25.0, 30.0, 30.0, 40.0))
                .unwrap()
                .paint_rect(),
            paint_rect(25.0, 30.0, 15.0, 30.0),
        );
        assert!(
            clip.intersect(PaintClip::new(40.0, 20.0, 10.0, 10.0))
                .is_none()
        );
    }

    #[test]
    fn axis_selective_clip_keeps_the_visible_axis_unbounded() {
        let clip = AxisSelectivePaintClip::new(PaintClip::new(10.0, 20.0, 30.0, 40.0), false, true);
        assert!(!clip.clips_x());
        assert!(clip.clips_y());
        assert_eq!(
            clip.resolved_against_page(PaintClip::new(0.0, 0.0, 100.0, 100.0)),
            PaintClip::new(0.0, 20.0, 100.0, 40.0)
        );
    }

    #[test]
    fn axis_selective_clip_uses_page_bounds_in_the_transformed_local_space() {
        let page = PaintClip::new(0.0, 0.0, 100.0, 100.0);
        let transform = PaintTransform::translate(PaintTranslation::new(-10.0, 0.0));
        let local_page = transform
            .inverse_apply_clip_to_aabb(page)
            .expect("translation is invertible");
        let clip = AxisSelectivePaintClip::new(PaintClip::new(20.0, 30.0, 40.0, 10.0), false, true);

        assert_eq!(
            clip.resolved_against_page(local_page),
            PaintClip::new(10.0, 30.0, 100.0, 10.0)
        );
    }

    #[test]
    fn paint_bounds_retains_degenerate_path_geometry() {
        let mut bounds = PaintBounds::from_paint_point(PaintPoint::new(10.0, 20.0));
        bounds.include_paint_point(PaintPoint::new(30.0, 20.0));
        bounds.include_paint_rect(paint_rect(5.0, 2.0, 0.0, 5.0));

        assert_eq!(bounds.paint_rect(), paint_rect(5.0, 2.0, 25.0, 18.0));
    }

    #[test]
    fn paint_rect_to_pdf_is_identity_for_unrotated_pages() {
        let rect = paint_rect(7.0, 8.0, 9.0, 10.0);

        assert_eq!(
            paint_rect_to_pdf(rect),
            PdfRect::new(PdfPoint::new(7.0, 8.0), PdfSize::new(9.0, 10.0))
        );
    }

    #[test]
    fn paint_point_to_pdf_is_identity_for_unrotated_pages() {
        assert_eq!(
            paint_point_to_pdf(PaintPoint::new(11.0, 12.0)),
            PdfPoint::new(11.0, 12.0)
        );
    }

    #[test]
    fn paint_transform_maps_typed_points_and_clips() {
        let transform = PaintTransform::translate(PaintTranslation::new(5.0, -2.0));

        assert_eq!(
            transform.apply_point(PaintPoint::new(10.0, 20.0)),
            PaintPoint::new(15.0, 18.0)
        );
        assert_eq!(
            transform.apply_clip_to_aabb(PaintClip::from_paint_rect(paint_rect(
                10.0, 20.0, 30.0, 40.0,
            ))),
            PaintClip::from_paint_rect(paint_rect(15.0, 18.0, 30.0, 40.0))
        );
    }
}
