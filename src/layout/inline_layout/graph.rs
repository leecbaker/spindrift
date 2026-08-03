use super::super::*;
use super::items::InlineLineSequence;
use crate::text::{
    TextBreakPolicy, character_blocks_atomic_inline_break,
    collect_auto_phrase_relaxed_wrap_opportunities, collect_grapheme_cluster_inner_boundaries,
    collect_keep_all_relaxed_wrap_opportunities, collect_measured_break_opportunities,
    is_css_preserved_document_space,
};

mod split_1;
pub(in crate::layout) use self::split_1::*;
mod split_2;
pub(in crate::layout) use self::split_2::*;
