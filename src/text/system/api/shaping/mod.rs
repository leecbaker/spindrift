//! Text preparation and Parley-to-Quire shaping conversion.

use crate::css::ComputedStyle;
use crate::units::SemanticLengthExt;

mod controls;
mod lines;
mod normalization;
mod styled;
mod tabs;

/// Selects whether a shaping pass forwards CSS letter spacing to Parley.
///
/// Inline layout shapes graph fragments without backend-owned tracking, then
/// represents the used inter-character advances on graph boundaries. Keeping
/// that choice separate from [`ComputedStyle`] avoids copying the complete
/// computed style solely to replace its `letter-spacing` value.
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>
#[derive(Clone, Copy)]
pub(crate) enum ShapingLetterSpacing {
    Computed,
    Suppressed,
}

impl ShapingLetterSpacing {
    fn requested_for(self, style: &ComputedStyle) -> f32 {
        match self {
            Self::Computed => style.used_letter_spacing().points(),
            Self::Suppressed => 0.0,
        }
    }
}

pub(crate) use controls::*;
pub(crate) use lines::*;
pub(crate) use normalization::*;
pub(crate) use styled::*;
pub(crate) use tabs::*;
