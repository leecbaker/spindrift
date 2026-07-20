mod cascade;
mod component_values;
mod html_form_state;
mod page;
mod parse;
mod selector;
mod types;
mod ua;
mod values;

pub(crate) use html_form_state::auto_directionality_input_value;
pub(crate) use values::{CrossOriginRequestMode, RequestUrlModifiers};

pub(crate) use cascade::anonymous_block_style;
pub(crate) use cascade::apply_declarations;
pub(crate) use cascade::apply_pseudo_rules_with_parent_ch_advance;
pub(crate) use cascade::declarations_affect_same_property;
pub(crate) use cascade::default_style_for_tag;
pub(crate) use cascade::origin_importance_rank;
pub(crate) use cascade::parse_background_image;
pub(crate) use cascade::style_for_element_with_signature;
pub(crate) use cascade::style_for_element_with_signature_and_parent_ch_advance;
pub(crate) use cascade::{ParsedImage, parse_css_image};
pub(crate) use cascade::{
    parse_individual_rotate, parse_individual_scale, parse_individual_translate,
    parse_object_view_box, parse_transform, parse_transform_box, parse_transform_origin,
};
#[cfg(test)]
pub(crate) use page::page_margins_from;
#[cfg(test)]
pub(crate) use page::page_margins_from_for_size;
pub(crate) use page::{
    apply_stylesheet_options, page_margins_from_for_size_and_edges_with_ch_advance,
    page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style,
    page_padding_from_for_size_with_ch_advance, page_rotation_from, page_size_from_with_ch_advance,
};
#[cfg(test)]
pub(crate) use page::{
    page_margins_from_for_size_and_edges, page_padding_from_for_size, page_size_from,
};
#[cfg(test)]
pub(crate) use parse::cascade_page_declarations;
pub(crate) use parse::media_rule_applies;
#[cfg(test)]
pub(crate) use parse::media_rule_applies_in_environment;
#[cfg(test)]
pub(crate) use parse::parse_declarations;
pub(crate) use parse::parse_stylesheet;
pub(crate) use parse::parse_stylesheet_with_media_environment;
pub(crate) use parse::supports_condition_applies;
pub(crate) use parse::{custom_property_value_is_valid, is_custom_property_name};
pub(crate) use types::CssColorSpace;
pub(crate) use types::*;
pub use types::{
    Css, CssColor, CssViewportSize, ForcedColorPalette, ForcedColorsMode, MediaEnvironment,
    MediaType,
};
pub(crate) use ua::html_document_important_user_agent_stylesheet;
#[cfg(test)]
pub(crate) use ua::html5_presentational_hints_stylesheet;
pub(crate) use ua::html5_presentational_hints_stylesheet_with_urls;
#[cfg(test)]
pub(crate) use ua::html5_user_agent_source;
pub(crate) use ua::html5_user_agent_stylesheet;
pub(crate) use values::normalize_css_comments;
pub(crate) use values::{
    CSS_PX_TO_PT, fallback_ch_advance_for_style, parse_font_palette, parse_font_synthesis,
    parse_font_synthesis_subproperty, parse_list_style_type, trim_css_value,
};
pub(crate) use values::{color_to_predefined_rgb, color_to_xyz_d50, parse_color_from_currentcolor};

#[cfg(test)]
mod tests;
