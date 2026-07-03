use super::*;
use crate::text::{
    character_is_autospace_alpha, character_is_autospace_ideograph, character_is_autospace_numeric,
    trim_css_collapsible_whitespace,
};

mod split_1;
pub(in crate::layout) use self::split_1::*;
mod split_2;
mod split_3;
pub(in crate::layout) use self::split_3::*;
