use super::assets::{
    PaintBackgroundArea, background_image_primitives_for_style, paint_effects_for_box,
};
use super::page_generated::{
    PageContentResolveContext, PageMarginContentItem, ResolvedPageContent,
    resolve_page_content_parts,
};
use super::*;
use crate::layout::inline_collect::normalize_inline_whitespace_items;

mod paint;

use paint::{page_margin_box_paint_order, replay_page_margin_box_fragments};

mod split_1;
pub(in crate::layout) use self::split_1::*;
mod split_2;
pub(in crate::layout) use self::split_2::*;
mod split_3;
pub(in crate::layout) use self::split_3::*;
