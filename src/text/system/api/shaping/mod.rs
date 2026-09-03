//! Text preparation and Parley-to-Spindrift shaping conversion.

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

/// A source-neutral request for one unwrapped document-font shaping pass.
///
/// HTML line layout and SVG text layout have different positioning models,
/// but selecting faces, shaping, fallback, and constructing document-font
/// glyph records is the same operation.  This request is the boundary between
/// those source-specific adapters and [`FontSystem`]: it owns no HTML
/// line-box or SVG user-coordinate state.
///
/// The complete resolved typography style remains intentionally concrete.
/// It makes every shaping-affecting CSS value available to the existing font
/// selection and Parley adapters, without an incomplete `TextStyle` trait or
/// a second SVG font-rendering path.
/// <https://drafts.csswg.org/css-fonts-4/#font-matching-algorithm>
/// <https://www.w3.org/TR/SVG2/text.html#TextLayoutAlgorithm>
#[derive(Clone, Copy)]
pub(crate) struct TextShapingRequest<'a> {
    text: &'a str,
    style: &'a ComputedStyle,
    line_height: f32,
}

impl<'a> TextShapingRequest<'a> {
    /// Construct a request from already-resolved font and text properties.
    /// SVG uses this after adapting its normalized text span; HTML uses
    /// [`Self::from_html_computed_style`] at its existing computed-style
    /// boundary.
    pub(crate) const fn new(text: &'a str, style: &'a ComputedStyle, line_height: f32) -> Self {
        Self {
            text,
            style,
            line_height,
        }
    }

    /// Adapt an ordinary HTML/CSS computed style to a shaping request while
    /// preserving HTML's resolved line-height context.
    pub(crate) const fn from_html_computed_style(
        text: &'a str,
        style: &'a ComputedStyle,
        line_height: f32,
    ) -> Self {
        Self::new(text, style, line_height)
    }
}

impl ShapingLetterSpacing {
    pub(in crate::text) fn requested_for(self, style: &ComputedStyle) -> f32 {
        match self {
            Self::Computed => style.used_letter_spacing().points(),
            Self::Suppressed => 0.0,
        }
    }
}

pub(crate) use controls::*;
pub(in crate::text) use normalization::*;
pub(in crate::text) use tabs::*;
