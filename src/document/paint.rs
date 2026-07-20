use super::Page;
use crate::{CssColor, Error, Result};
use std::borrow::Cow;

/// A stroke thickness in page-local paint coordinates.
///
/// This is distinct from a point, size, or displacement in the same space:
/// a stroke's width is a scalar graphics-state property that applies
/// perpendicular to its path. PDF calls this the line width, while CSS and
/// SVG expose it as `stroke-width`:
/// <https://www.w3.org/TR/SVG2/painting.html#StrokeWidth> and
/// ISO 32000-2:2020, 8.4.3 "Line Width".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintStrokeWidth(euclid::Length<f32, PaintSpace>);

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

mod split_1;
pub(crate) use self::split_1::*;
mod split_2;
pub(crate) use self::split_2::*;
mod split_3;
pub(crate) use self::split_3::*;
mod split_4;
pub use self::split_4::LinkAnnotation;
pub(crate) use self::split_4::*;
