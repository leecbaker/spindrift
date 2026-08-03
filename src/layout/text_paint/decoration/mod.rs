use super::*;
use crate::css::{TextDecorationLayer, TextDecorationSkipSelf};

mod glyphs;
mod paint;
mod prepare;
mod segments;

pub(in crate::layout) use self::glyphs::*;
pub(in crate::layout) use self::prepare::*;
pub(in crate::layout) use self::segments::*;

pub(in crate::layout) fn active_text_decoration_layers(
    style: &ComputedStyle,
) -> Vec<TextDecorationLayer> {
    // Every decoration origin is finalized when its computed style is
    // created.  Using the raw longhands as a fallback here would let an
    // ancestor's values leak across an atomic inline boundary.
    style.text_decoration_layers.clone()
}

pub(in crate::layout) fn text_decoration_skip_self_suppresses(
    style: &ComputedStyle,
    line: TextDecorationLineKind,
) -> bool {
    match style.text_decoration.skip_self {
        TextDecorationSkipSelf::Auto | TextDecorationSkipSelf::NoSkip => false,
        TextDecorationSkipSelf::SkipAll => true,
        TextDecorationSkipSelf::Lines {
            underline,
            overline,
            line_through,
        } => match line {
            TextDecorationLineKind::Underline => underline,
            TextDecorationLineKind::Overline => overline,
            TextDecorationLineKind::LineThrough => line_through,
        },
    }
}
