use super::parse::parse_declarations;
use super::selector::selector_matches_with_scope_proximity;
use super::types::*;
use super::values::*;
use std::collections::HashMap;

mod background;
mod columns;
mod declarations;
mod properties;
mod style;
mod variables;

use background::*;
use columns::*;
pub(crate) use declarations::{
    CascadedDeclaration, apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance,
    apply_cascaded_marker_declarations_with_inheritance_source_and_parent_ch_advance,
    apply_declarations, declaration_is_important, declarations_affect_same_property,
    origin_importance_rank, sort_cascaded_declarations,
};
use properties::*;
pub(crate) use style::{
    apply_pseudo_rules_with_parent_ch_advance, default_style_for_tag,
    style_for_element_with_signature, style_for_element_with_signature_and_parent_ch_advance,
};
use variables::*;
