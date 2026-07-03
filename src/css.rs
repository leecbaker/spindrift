mod cascade;
mod html_form_state;
mod page;
mod parse;
mod selector;
mod types;
mod ua;
mod values;

pub(crate) use cascade::apply_declarations;
pub(crate) use cascade::apply_pseudo_rules_with_parent_ch_advance;
pub(crate) use cascade::declarations_affect_same_property;
pub(crate) use cascade::default_style_for_tag;
pub(crate) use cascade::origin_importance_rank;
pub(crate) use cascade::parse_background_image;
pub(crate) use cascade::style_for_element_with_signature;
pub(crate) use cascade::style_for_element_with_signature_and_parent_ch_advance;
#[cfg(test)]
pub(crate) use page::page_margins_from;
#[cfg(test)]
pub(crate) use page::page_margins_from_for_size;
pub(crate) use page::{
    apply_stylesheet_options, page_margins_from_for_size_and_edges_with_ch_advance,
    page_padding_from_for_size_with_ch_advance, page_rotation_from, page_size_from_with_ch_advance,
    page_style_for_declarations,
};
#[cfg(test)]
pub(crate) use page::{
    page_margins_from_for_size_and_edges, page_padding_from_for_size, page_size_from,
};
#[cfg(test)]
pub(crate) use parse::cascade_page_declarations;
pub(crate) use parse::media_rule_applies;
#[cfg(test)]
pub(crate) use parse::parse_declarations;
pub(crate) use parse::parse_stylesheet;
pub(crate) use parse::supports_condition_applies;
pub(crate) use types::*;
pub use types::{Color, Css};
pub(crate) use ua::html5_presentational_hints_stylesheet;
#[cfg(test)]
pub(crate) use ua::html5_user_agent_source;
pub(crate) use ua::html5_user_agent_stylesheet;
pub(crate) use values::{
    CSS_PX_TO_PT, known_font_family, parse_color, parse_list_style_type, trim_css_value,
};

#[cfg(test)]
mod tests;
