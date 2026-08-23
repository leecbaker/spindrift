use icu_casemap::options::TitlecaseOptions;
use icu_casemap::{CaseMapper, TitlecaseMapper};
use icu_locale_core::LanguageIdentifier;
use icu_segmenter::WordSegmenter;
use icu_segmenter::options::WordBreakInvariantOptions;

use super::*;
use crate::text::character_is_unicode_typographic_letter;

mod alignment;
pub(in crate::layout) use self::alignment::*;
mod generated_content;
pub(in crate::layout) use self::generated_content::*;
mod indent;
pub(in crate::layout) use self::indent::*;
mod text_transform;
pub(in crate::layout) use self::text_transform::*;
mod vertical_align;
pub(in crate::layout) use self::vertical_align::*;
mod whitespace;
pub(in crate::layout) use self::whitespace::*;
