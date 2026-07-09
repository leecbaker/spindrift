use super::selector::QuireSelectorImpl;
use super::values::CSS_PX_TO_PT;
pub(crate) use crate::units::{
    LayoutLength, PercentageBasis, SemanticLengthExt, layout_points, layout_pt,
};
use selectors::parser::SelectorList;
use std::collections::HashMap;
use std::collections::HashSet;

mod background;
mod border_image;
mod box_model;
mod columns;
mod computed;
mod counters;
mod display;
mod fonts;
mod gaps;
mod grid;
mod lengths;
mod line;
mod misc;
mod primitives;
mod sides;
mod source;

pub(crate) use background::*;
pub(crate) use border_image::*;
pub(crate) use box_model::*;
pub(crate) use columns::*;
pub(crate) use computed::*;
pub(crate) use counters::*;
pub(crate) use display::*;
pub(crate) use fonts::*;
pub(crate) use gaps::*;
pub(crate) use grid::*;
pub(crate) use lengths::*;
pub(crate) use line::*;
pub(crate) use misc::*;
pub use primitives::*;
pub(crate) use sides::*;
pub use source::*;

/// Projects deferred viewport-relative CSS lengths into layout lengths.
///
/// Viewport-percentage lengths resolve against the initial containing block;
/// paged layout supplies that basis from the active page area:
/// <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
pub(crate) trait ResolveViewportLengths {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis);
}

impl<T: ResolveViewportLengths> ResolveViewportLengths for Option<T> {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Some(value) = self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

impl<T: ResolveViewportLengths> ResolveViewportLengths for Vec<T> {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        for value in self {
            value.resolve_viewport_lengths(basis);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counter(usize);

    impl ResolveViewportLengths for Counter {
        fn resolve_viewport_lengths(&mut self, _: ViewportLengthBasis) {
            self.0 += 1;
        }
    }

    #[test]
    fn viewport_resolution_recurses_through_optional_and_list_values() {
        let basis = ViewportLengthBasis::for_writing_mode(
            crate::units::LayoutSize::new(100.0, 100.0),
            WritingMode::HorizontalTb,
        );
        let mut optional = Some(Counter(0));
        let mut values = vec![Counter(0), Counter(0)];

        optional.resolve_viewport_lengths(basis);
        values.resolve_viewport_lengths(basis);

        assert_eq!(optional.unwrap().0, 1);
        assert_eq!(values.iter().map(|value| value.0).sum::<usize>(), 2);
    }
}
