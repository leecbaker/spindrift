use super::parse::parse_declarations;
use super::selector::selector_matches_with_scope_proximity;
use super::types::*;
use super::values::*;
use std::collections::HashMap;

mod background;
mod columns;
mod declarations;
mod style;
mod variables;

use background::*;
pub(crate) use background::{parse_background_position, parse_background_size};
use columns::*;
pub(crate) use declarations::{
    CascadedDeclaration, apply_cascaded_declarations_with_inheritance_source,
    apply_cascaded_marker_declarations_with_inheritance_source, apply_declarations,
    declaration_is_important, declarations_affect_same_property, origin_importance_rank,
    sort_cascaded_declarations,
};
pub(crate) use style::{default_style_for_tag, style_for_element_with_signature};
use variables::*;
