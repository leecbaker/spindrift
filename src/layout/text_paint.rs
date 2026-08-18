use super::*;
use crate::css::{TextDecoration, TextEmphasisSkip, TextEmphasisStyle, TextOrientation};
use crate::text::trim_start_css_collapsible_whitespace;
use crate::text::{CursiveProtectedUnitRanges, character_is_text_decoration_spacer};

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
pub(in crate::layout) use self::positioning::*;
