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

pub(crate) use cascade::{
    ModeledLonghandSet, ParsedImage, SvgPresentationAttributeDeclarations, anonymous_block_style,
    anonymous_text_style, apply_declarations, apply_declarations_with_inheritance_source,
    apply_pseudo_rules_with_parent_ch_advance, copy_modeled_longhand,
    declarations_affect_same_property, default_display_is_block_level_for_tag,
    default_style_for_tag, modeled_longhand_has_same_source, origin_importance_rank,
    parse_background_image, parse_css_image, parse_individual_rotate, parse_individual_scale,
    parse_individual_translate, parse_object_view_box, parse_transform, parse_transform_box,
    parse_transform_origin, style_for_element_with_signature,
    style_for_element_with_signature_and_parent_ch_advance,
    style_for_element_with_signature_and_svg_presentation,
};
pub(crate) use component_values::{split_css_component_values, trim_css_value};
pub(crate) use html_form_state::{auto_directionality_input_value, input_type};
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
pub(crate) use parse::{
    custom_property_value_is_valid, is_custom_property_name, media_rule_applies,
    media_rule_applies_in_environment, parse_declarations, parse_stylesheet,
    parse_stylesheet_with_media_environment, supports_condition_applies,
};
pub use types::{
    ColorSchemePreference, Css, CssColor, CssViewportSize, ForcedColorPalette, ForcedColorsMode,
    MediaEnvironment, MediaType,
};
pub(crate) use types::{CssColorSpace, *};
#[cfg(test)]
pub(crate) use ua::html5_presentational_hints_stylesheet;
#[cfg(test)]
pub(crate) use ua::html5_user_agent_source;
pub(crate) use ua::{
    html_document_important_user_agent_stylesheet, html5_presentational_hints_stylesheet_with_urls,
    html5_user_agent_stylesheet,
};
pub(crate) use values::{
    CSS_PX_TO_PT, CrossOriginRequestMode, RequestUrlModifiers,
    canonical_predefined_counter_style_name, color_depends_on_currentcolor,
    color_to_predefined_rgb, color_to_xyz_d50, fallback_ch_advance_for_style, parse_color,
    parse_color_from_currentcolor, parse_color_from_currentcolor_in_scheme,
    parse_computed_length_percentage, parse_css_url_token, parse_font_palette,
    parse_font_synthesis, parse_font_synthesis_subproperty, parse_list_style_type,
};
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
