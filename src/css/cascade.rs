use super::parse::parse_declarations;
use super::selector::{selector_chain, selector_matches_with_scope_proximity_in_chain};
use super::types::*;
use super::values::*;
use selectors::context::SelectorCaches;
use std::collections::HashMap;

mod background;
mod columns;
mod declarations;
mod properties;
mod style;
mod variables;

use background::*;
pub(crate) use background::{parse_background_image, parse_background_position};
use columns::*;
pub(crate) use declarations::{
    CascadedDeclaration, apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance,
    apply_cascaded_marker_declarations_with_inheritance_source_and_parent_ch_advance,
    apply_declarations, declaration_is_important, declarations_affect_same_property,
    origin_importance_rank, parse_individual_rotate, parse_individual_scale,
    parse_individual_translate, parse_object_view_box, parse_transform, parse_transform_box,
    parse_transform_origin, sort_cascaded_declarations,
};
pub(in crate::css) use declarations::{
    parse_initial_letter, parse_initial_letter_align, parse_initial_letter_wrap,
};
use properties::*;
pub(crate) use style::{
    anonymous_block_style, apply_pseudo_rules_with_parent_ch_advance, default_style_for_tag,
    style_for_element_with_signature, style_for_element_with_signature_and_parent_ch_advance,
};
use variables::*;
