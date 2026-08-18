mod cascade;
pub(crate) mod component_values;
mod html_form_state;
mod page;
mod parse;
mod quotes;
mod selector;
mod types;
mod ua;
mod values;

pub(crate) use html_form_state::{auto_directionality_input_value, input_type};
pub(crate) use values::color_depends_on_currentcolor;
pub(crate) use values::parse_css_url_token;
pub(crate) use values::{CrossOriginRequestMode, RequestUrlModifiers};

pub(crate) use cascade::SvgPresentationAttributeDeclarations;
pub(crate) use cascade::anonymous_block_style;
pub(crate) use cascade::anonymous_text_style;
pub(crate) use cascade::apply_declarations;
pub(crate) use cascade::apply_declarations_with_inheritance_source;
pub(crate) use cascade::apply_pseudo_rules_with_parent_ch_advance;
pub(crate) use cascade::declarations_affect_same_property;
pub(crate) use cascade::default_display_is_block_level_for_tag;
pub(crate) use cascade::default_style_for_tag;
pub(crate) use cascade::origin_importance_rank;
pub(crate) use cascade::parse_background_image;
pub(crate) use cascade::style_for_element_with_signature;
pub(crate) use cascade::style_for_element_with_signature_and_parent_ch_advance;
pub(crate) use cascade::style_for_element_with_signature_and_svg_presentation;
pub(crate) use cascade::{ParsedImage, parse_css_image};
pub(crate) use cascade::{
    parse_individual_rotate, parse_individual_scale, parse_individual_translate,
    parse_object_view_box, parse_transform, parse_transform_box, parse_transform_origin,
};
pub(crate) use component_values::trim_css_value;
#[cfg(test)]
pub(crate) use page::page_margins_from;
#[cfg(test)]
pub(crate) use page::page_margins_from_for_size;
pub(crate) use page::{
    PageMarginResolutionContext, apply_stylesheet_options,
    page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style_and_root_metrics,
    page_padding_from_for_size_with_ch_advance_and_root_metrics, page_rotation_from,
    page_size_from_with_ch_advance_and_root_metrics,
};
#[cfg(test)]
pub(crate) use page::{
    page_margins_from_for_size_and_edges,
    page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style,
    page_padding_from_for_size, page_size_from,
};
#[cfg(test)]
pub(crate) use parse::cascade_page_declarations;
pub(crate) use parse::media_rule_applies;
pub(crate) use parse::media_rule_applies_in_environment;
pub(crate) use parse::parse_declarations;
pub(crate) use parse::parse_stylesheet;
pub(crate) use parse::parse_stylesheet_with_media_environment;
pub(crate) use parse::supports_condition_applies;
pub(crate) use parse::{custom_property_value_is_valid, is_custom_property_name};
pub(crate) use types::CssColorSpace;
pub(crate) use types::*;
pub use types::{
    ColorSchemePreference, Css, CssColor, CssViewportSize, ForcedColorPalette, ForcedColorsMode,
    MediaEnvironment, MediaType,
};
pub(crate) use ua::html_document_important_user_agent_stylesheet;
#[cfg(test)]
pub(crate) use ua::html5_presentational_hints_stylesheet;
pub(crate) use ua::html5_presentational_hints_stylesheet_with_urls;
#[cfg(test)]
pub(crate) use ua::html5_user_agent_source;
pub(crate) use ua::html5_user_agent_stylesheet;
pub(crate) use values::{
    CSS_PX_TO_PT, canonical_predefined_counter_style_name, fallback_ch_advance_for_style,
    parse_color_from_currentcolor_in_scheme, parse_font_palette, parse_font_synthesis,
    parse_font_synthesis_subproperty, parse_list_style_type,
};
pub(crate) use values::{color_to_predefined_rgb, color_to_xyz_d50, parse_color_from_currentcolor};
pub(in crate::css) use values::{parse_border_image_source, parse_mask_border_source};

/// Match an unprefixed CSS `attr()` name against a DOM attribute name.
///
/// CSS Values delegates `attr()` name matching to attribute-selector rules.
/// HTML then requires the requested name to be ASCII-lowercased only for
/// namespace-less attributes on HTML elements in HTML documents; every other
/// attribute-name comparison remains exact.
/// <https://drafts.csswg.org/css-values-5/#attr-notation>
/// <https://html.spec.whatwg.org/multipage/semantics-other.html#case-sensitivity-of-the-css-attr()-function>
pub(crate) fn unprefixed_attr_name_matches(
    element_namespace: &str,
    document_is_html: bool,
    attribute_namespace: &str,
    attribute_local_name: &str,
    requested_name: &str,
) -> bool {
    const HTML_NAMESPACE_URL: &str = "http://www.w3.org/1999/xhtml";

    if !attribute_namespace.is_empty() {
        return false;
    }
    let requested_name = if document_is_html
        && (element_namespace.is_empty() || element_namespace == HTML_NAMESPACE_URL)
    {
        requested_name.to_ascii_lowercase()
    } else {
        requested_name.to_string()
    };
    attribute_local_name == requested_name
}

#[cfg(test)]
mod tests;
