use super::*;
use crate::text::{
    character_is_css_other_space_separator, character_is_css_word_separator,
    inter_character_gap_allowed_between_text, is_css_preserved_document_space,
    line_end_letter_spacing_width, trim_css_collapsible_whitespace,
    trim_end_css_collapsible_whitespace, trim_start_css_collapsible_whitespace,
    typographic_unit_count, typographic_unit_ranges,
};

mod split_1;
pub(in crate::layout) use self::split_1::*;
mod split_2;
pub(in crate::layout) use self::split_2::*;
