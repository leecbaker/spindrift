use super::*;
use crate::text::character_is_unicode_typographic_letter;
use icu_casemap::{CaseMapper, TitlecaseMapper, options::TitlecaseOptions};
use icu_locale_core::LanguageIdentifier;
use icu_segmenter::{WordSegmenter, options::WordBreakInvariantOptions};

mod split_1;
pub(in crate::layout) use self::split_1::*;
mod split_2;
pub(in crate::layout) use self::split_2::*;
