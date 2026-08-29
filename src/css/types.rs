use std::collections::{HashMap, HashSet};

use ::selectors::parser::SelectorList;

use super::selector::QuireSelectorImpl;
use super::values::CSS_PX_TO_PT;
pub(crate) use crate::units::{
    LayoutLength, PercentageBasis, SemanticLengthExt, layout_points, layout_pt,
};

mod alignment;
mod background;
mod border_image;
mod borders;
mod box_model;
mod columns;
mod compositing;
mod computed;
mod containment;
mod counters;
mod display;
mod flex;
mod fonts;
mod gaps;
mod generated_content;
mod grid;
mod images;
mod inline;
mod language;
mod lengths;
mod line;
mod lists;
mod overflow;
mod paged_media;
mod positioning;
mod primitives;
mod selectors;
mod shadows;
mod shapes;
mod sides;
mod sizing;
mod source;
mod tables;
mod text;
mod text_decoration;
mod transforms;
mod writing_modes;

pub(crate) use alignment::*;
pub(crate) use background::*;
pub(crate) use border_image::*;
pub(crate) use borders::*;
pub(crate) use box_model::*;
pub(crate) use columns::*;
pub(crate) use compositing::*;
pub(crate) use computed::*;
pub(crate) use containment::*;
pub(crate) use counters::*;
pub(crate) use display::*;
pub(crate) use flex::*;
pub(crate) use fonts::*;
pub(crate) use gaps::*;
pub(crate) use generated_content::*;
pub(crate) use grid::*;
pub(crate) use images::*;
pub(crate) use inline::*;
pub(crate) use language::*;
pub(crate) use lengths::*;
pub(crate) use line::*;
pub(crate) use lists::*;
pub(crate) use overflow::*;
pub(crate) use paged_media::*;
pub(crate) use positioning::*;
pub use primitives::*;
pub(crate) use selectors::*;
pub(crate) use shadows::*;
pub(crate) use shapes::*;
pub(crate) use sides::*;
pub(crate) use sizing::*;
pub use source::*;
pub(crate) use tables::*;
pub(crate) use text::*;
pub(crate) use text_decoration::*;
pub(crate) use transforms::*;
pub(crate) use writing_modes::*;

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
