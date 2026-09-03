use std::collections::HashMap;

use selectors::context::SelectorCaches;

use super::component_values::{
    css_function_list, css_leading_function_matching, css_single_ident, parse_css_string_token,
    split_css_component_values, trim_css_value, try_split_css_component_values,
    try_split_css_top_level_delimiter,
};
use super::parse::parse_declarations;
use super::selector::selector_chain;
use super::types::*;
use super::values::*;

mod background;
mod columns;
mod declarations;

mod properties;
mod style;
pub(in crate::css) mod variables;

use background::*;
pub(crate) use background::{
    ParsedImage, parse_background_image, parse_background_position, parse_css_image,
};
use columns::*;
pub(crate) use declarations::{
    CascadedDeclaration, SvgTransformAttributeValue,
    apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance,
    apply_cascaded_marker_declarations_with_inheritance_source_and_parent_ch_advance,
    apply_declarations, apply_declarations_with_inheritance_source, declaration_is_important,
    declarations_affect_same_property, origin_importance_rank, parse_individual_rotate,
    parse_individual_scale, parse_individual_translate, parse_object_view_box,
    parse_svg_transform_attribute, parse_transform, parse_transform_box, parse_transform_origin,
    sort_cascaded_declarations, svg_transform_origin_presentation_declaration,
};
pub(in crate::css) use declarations::{
    CascadedProperty, parse_border_shape, parse_initial_letter, parse_initial_letter_align,
    parse_initial_letter_wrap, parse_legacy_clip, parse_running_position,
    parse_text_combine_upright, parse_writing_mode,
};
pub(in crate::css) use properties::is_modeled_property_name;
use properties::*;
pub(crate) use properties::{
    ModeledLonghand, ModeledLonghandSet, copy_modeled_longhand,
    inherit_modeled_longhand_provenance, mark_modeled_longhand_specified,
    modeled_longhand_has_same_source,
};
pub(crate) use style::{
    SvgPresentationAttributeDeclarations, anonymous_block_style, anonymous_text_style,
    apply_pseudo_rules_with_parent_ch_advance, default_display_is_block_level_for_tag,
    default_style_for_tag, style_for_element_with_signature,
    style_for_element_with_signature_and_parent_ch_advance,
    style_for_element_with_signature_and_svg_presentation,
};
pub(in crate::css) use style::{
    parse_animation_snapshot_name, parse_animation_snapshot_shorthand,
    parse_animation_snapshot_time,
};
use variables::*;
