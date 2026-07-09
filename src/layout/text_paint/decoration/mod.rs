use super::*;
use crate::css::{TextDecoration, TextDecorationSkipSelf};

mod glyphs;
mod paint;
mod prepare;
mod segments;

pub(in crate::layout) use self::glyphs::*;
pub(in crate::layout) use self::prepare::*;
pub(in crate::layout) use self::segments::*;

pub(in crate::layout) fn active_text_decoration_layers(
    style: &ComputedStyle,
) -> Vec<TextDecoration> {
    if !style.text_decoration_layers.is_empty() {
        return style.text_decoration_layers.clone();
    }
    if style.text_decoration.clone().has_visible_line() {
        return vec![style.text_decoration.clone()];
    }
    Vec::new()
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
