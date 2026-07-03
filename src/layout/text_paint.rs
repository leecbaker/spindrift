use super::*;
use crate::css::{
    TextDecoration, TextDecorationSkipSelf, TextEmphasisSkip, TextEmphasisStyle, TextOrientation,
};
use crate::text::trim_start_css_collapsible_whitespace;
use crate::text::{
    character_is_text_decoration_spacer, typographic_unit_is_upright_in_mixed_orientation,
    typographic_unit_ranges,
};

mod split_1;
pub(in crate::layout) use self::split_1::*;
mod split_2;
mod split_3;
pub(in crate::layout) use self::split_3::*;
mod split_4;
pub(in crate::layout) use self::split_4::*;
