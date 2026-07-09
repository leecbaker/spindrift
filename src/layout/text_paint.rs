use super::*;
use crate::css::{TextDecoration, TextEmphasisSkip, TextEmphasisStyle, TextOrientation};
use crate::text::trim_start_css_collapsible_whitespace;
use crate::text::{
    character_is_text_decoration_spacer, typographic_unit_is_upright_in_mixed_orientation,
    typographic_unit_ranges,
};

mod decoration;
mod effects;
mod model;
mod positioning;
mod preparation;
mod runs;
#[cfg(test)]
mod tests;
mod text_layout;

pub(in crate::layout) use self::decoration::*;
pub(in crate::layout) use self::effects::*;
pub(in crate::layout) use self::model::*;
#[allow(unused_imports)]
pub(in crate::layout) use self::positioning::*;
