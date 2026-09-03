//! CSS Text opportunity graph construction, selection, and materialization.
//!
//! The modules mirror the graph lifecycle: its domain model is built from an
//! inline paragraph, candidates are resolved according to CSS Text/UAX #14,
//! and a selected range is materialized into a paintable line.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
#[cfg(feature = "layout-profile")]
use std::time::Instant;

use super::super::*;
use super::items::InlineLineSequence;
use super::mixed::apply_visual_tracking_boundaries;
use crate::css::{BoxDecorationBreak, DiscretionaryHyphenationPolicy, Hyphens, TextSpacingTrim};
use crate::layout::inline_collect::{
    InlineBoxEdge, autospace_boundary_character_at_end, autospace_boundary_character_at_start,
    inline_box_edge_components, inline_box_edge_physical_side, inline_box_edge_width,
    text_autospace_boundary_needs_spacing,
};
use crate::text::{
    CursiveProtectedUnitRanges, DiscretionaryOpportunity, LanguageDiscretionaryReplacement,
    TextBreakPolicy, automatic_hyphenation_opportunities, character_is_css_other_space_separator,
    character_is_default_ignorable_code_point, character_is_first_letter_associated_space,
    character_is_first_letter_suffix_punctuation, character_is_unicode_first_letter_base,
    character_is_unicode_mark, character_is_unicode_punctuation,
    collect_auto_phrase_relaxed_wrap_opportunities, collect_grapheme_cluster_inner_boundaries,
    collect_keep_all_relaxed_wrap_opportunities, collect_measured_break_opportunities,
    hyphenator_for_language, is_css_preserved_document_space, manual_hyphenation_opportunities,
};
mod model;
pub(in crate::layout) use self::model::*;
mod bidi;
pub(in crate::layout) use self::bidi::*;
mod intrinsic;
pub(in crate::layout) use self::intrinsic::*;
mod construction;
pub(in crate::layout) use self::construction::*;
mod shaping;
pub(in crate::layout) use self::shaping::*;
mod first_letter;
pub(in crate::layout) use self::first_letter::*;
mod text_spacing;
use self::text_spacing::*;
mod breaks;
pub(in crate::layout) use self::breaks::*;
mod line_edges;
mod line_selection;
pub(in crate::layout) use self::line_edges::*;
mod line_geometry;
pub(in crate::layout) use self::line_geometry::*;

#[cfg(test)]
mod tests;
