use std::borrow::Cow;

use super::declaration_values::*;
use super::supports::strip_enclosing_parentheses;
use super::*;
use crate::css::cascade::CascadedProperty;
use crate::css::component_values::parse_var_function_arguments;
use crate::css::{
    ComputedColorScheme, parse_font_palette, parse_font_synthesis,
    parse_font_synthesis_subproperty, parse_object_fit,
};

/// A declaration accepted by Quire's specified-value-time declaration parser.
///
/// Keeping this operation separate from the cascade order lets normal rules
/// and CSS Conditional feature queries share the same property grammar.
/// CSS Conditional Rules evaluates the declaration at specified-value time,
/// before a `var()` reference is substituted:
/// <https://www.w3.org/TR/css-conditional-3/#at-supports>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::css) struct DeclarationOperation<'a> {
    pub(in crate::css) name: Cow<'a, str>,
    pub(in crate::css) value: Cow<'a, str>,
}

/// The specified-value-time result of parsing one declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::css) enum DeclarationParseResult<'a> {
    UnsupportedProperty,
    InvalidValue,
    Valid(DeclarationOperation<'a>),
}

/// Parses one normal declaration using the grammar shared by the cascade and
/// `@supports (property: value)`.
///
/// This is intentionally the one property/value acceptance boundary. The
/// cascade consumes the `Valid` operation, while a declaration feature query
/// only observes whether it is valid.
pub(in crate::css) fn parse_canonical_declaration<'a>(
    raw_name: &'a str,
    raw_value: &'a str,
) -> DeclarationParseResult<'a> {
    let raw_name = raw_name.trim();
    // Validate the original component stream before removing declaration
    // whitespace. A newline after an unterminated quote is a BadString token,
    // not ignorable trailing whitespace.
    let Some(contains_variable_reference) = validate_component_values(raw_value, false, true)
    else {
        return DeclarationParseResult::InvalidValue;
    };
    let value = raw_value.trim();
    if raw_name.is_empty()
        || (value.is_empty() && !is_custom_property_name(raw_name))
        || !declaration_priority_is_valid(raw_value)
    {
        return DeclarationParseResult::InvalidValue;
    }
    // Custom properties accept any sequence of component values, except for
    // the declaration-level syntax restrictions. Their names are
    // case-sensitive and use the `<dashed-ident>` grammar, so do not
    // canonicalize them with ordinary property names.
    //
    // CSS Variables Level 1 defines `var()` as syntactically valid at
    // specified-value time even when the resulting value is not valid for a
    // consuming property. Consequently, a syntactically valid `var()` makes
    // a declaration feature query true for every supported property.
    // <https://www.w3.org/TR/css-variables-1/#using-variables>
    // <https://www.w3.org/TR/css-conditional-3/#at-supports>
    if is_custom_property_name(raw_name) {
        return DeclarationParseResult::Valid(DeclarationOperation {
            name: Cow::Borrowed(raw_name),
            value: Cow::Borrowed(value),
        });
    }
    let lower_case_name = if raw_name.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(raw_name.to_ascii_lowercase())
    } else {
        Cow::Borrowed(raw_name)
    };
    let name = match lower_case_name.as_ref() {
        // CSSOM-compatible alias; the cascade has one canonical flex-basis
        // slot, so feature queries must ask about that same operation.
        "-webkit-flex-basis" => Cow::Borrowed("flex-basis"),
        _ => lower_case_name,
    };
    // The typed cascade registry is the single ownership boundary for
    // ordinary declarations.  Keep this check before value-specific parsing:
    // accepting a name here without a typed identity would otherwise allow it
    // to panic later while constructing a cascaded declaration.
    if !supported_property_name(&name) {
        return DeclarationParseResult::UnsupportedProperty;
    }
    if contains_variable_reference {
        return declaration_operation_for_modeled_property(name, value);
    }

    let value = trim_css_value(value);
    if value.is_empty() {
        return DeclarationParseResult::InvalidValue;
    }
    if matches!(
        value.to_ascii_lowercase().as_str(),
        "initial" | "inherit" | "unset" | "revert" | "revert-layer"
    ) {
        return declaration_operation_for_modeled_property(name, value);
    }
    let valid = match name.as_ref() {
        "display" => supports_display_value(value),
        "direction" => matches!(value.to_ascii_lowercase().as_str(), "ltr" | "rtl"),
        "unicode-bidi" => matches!(
            value.to_ascii_lowercase().as_str(),
            "normal" | "embed" | "isolate" | "bidi-override" | "isolate-override" | "plaintext"
        ),
        "writing-mode" => parse_writing_mode(value).is_some(),
        "text-orientation" => matches!(
            value.to_ascii_lowercase().as_str(),
            "mixed" | "upright" | "sideways"
        ),
        "text-combine-upright" => parse_text_combine_upright(value).is_some(),
        "text-align" => supports_text_align_value(value),
        "text-align-all" => supports_text_align_all_value(value),
        "text-align-last" => supports_text_align_last_value(value),
        "text-autospace" => supports_text_autospace_value(value),
        "text-spacing-trim" => supports_text_spacing_trim_value(value),
        "text-spacing" => supports_text_spacing_value(value),
        "ruby-align" => matches!(
            value.to_ascii_lowercase().as_str(),
            "start" | "center" | "space-between" | "space-around"
        ),
        "ruby-overhang" => matches!(
            value.to_ascii_lowercase().as_str(),
            "auto" | "spaces" | "none"
        ),
        "word-space-transform" => supports_word_space_transform_value(value),
        "initial-letter" => parse_initial_letter(value).is_some(),
        "initial-letter-align" => parse_initial_letter_align(value).is_some(),
        "initial-letter-wrap" => parse_initial_letter_wrap(value, 12.0).is_some(),
        "text-transform" => supports_text_transform_value(value),
        "text-wrap" => text_wrap_value_is_valid(value),
        "text-wrap-mode" => matches!(value.to_ascii_lowercase().as_str(), "wrap" | "nowrap"),
        "text-wrap-style" => matches!(
            value.to_ascii_lowercase().as_str(),
            "auto" | "balance" | "stable"
        ),
        "wrap-inside" => matches!(value.to_ascii_lowercase().as_str(), "auto" | "avoid"),
        "tab-size" => parse_tab_size(value, 12.0).is_some(),
        "text-decoration" => supports_text_decoration_value(value),
        "text-decoration-line" => supports_text_decoration_line_value(value),
        "text-decoration-style" => supports_text_decoration_style_value(value),
        "text-decoration-color" => parse_color(value).is_some(),
        "text-decoration-thickness" => supports_text_decoration_thickness_value(value),
        "text-decoration-inset" => supports_text_decoration_inset_value(value),
        "text-decoration-skip" => matches!(
            trim_css_value(value).to_ascii_lowercase().as_str(),
            "auto" | "none"
        ),
        "text-decoration-skip-ink" => matches!(
            trim_css_value(value).to_ascii_lowercase().as_str(),
            "auto" | "all" | "none"
        ),
        "text-decoration-skip-self" => supports_text_decoration_skip_self_value(value),
        "text-decoration-skip-box" => matches!(
            trim_css_value(value).to_ascii_lowercase().as_str(),
            "none" | "all"
        ),
        "text-decoration-skip-spaces" => supports_text_decoration_skip_spaces_value(value),
        "text-underline-offset" => {
            value.eq_ignore_ascii_case("auto")
                || parse_computed_length_percentage(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "text-underline-position" => supports_text_underline_position_value(value),
        "text-emphasis-style" => supports_text_emphasis_style_value(value),
        "text-emphasis" => supports_text_emphasis_value(value),
        "text-emphasis-color" => parse_color(value).is_some(),
        "text-emphasis-position" => supports_text_emphasis_position_value(value),
        "text-emphasis-skip" => supports_text_emphasis_skip_value(value),
        "text-shadow" => supports_text_shadow_value(value),
        "box-shadow" => supports_box_shadow_value(value),
        "border-spacing" => parse_border_spacing(value, crate::css::ROOT_FONT_SIZE_PT).is_some(),
        "letter-spacing" => {
            value.eq_ignore_ascii_case("normal") || parse_letter_spacing(value, 12.0).is_some()
        }
        "word-spacing" => {
            value.eq_ignore_ascii_case("normal") || parse_word_spacing(value, 12.0).is_some()
        }
        "animation" => crate::css::cascade::parse_animation_snapshot_shorthand(value).is_some(),
        "animation-name" => crate::css::component_values::split_css_top_level_delimiter(value, ',')
            .first()
            .and_then(|value| crate::css::cascade::parse_animation_snapshot_name(value))
            .is_some(),
        "animation-duration" | "animation-delay" => {
            crate::css::component_values::split_css_top_level_delimiter(value, ',')
                .first()
                .and_then(|value| crate::css::cascade::parse_animation_snapshot_time(value))
                .is_some()
        }
        "font" => parse_font_shorthand(value, 12.0, FontWeight::NORMAL).is_some(),
        "font-feature-settings" => parse_font_feature_settings(value).is_some(),
        "font-variation-settings" => parse_font_variation_settings(value).is_some(),
        "font-palette" => parse_font_palette(value).is_some(),
        "font-synthesis" => parse_font_synthesis(value).is_some(),
        "font-synthesis-weight"
        | "font-synthesis-style"
        | "font-synthesis-small-caps"
        | "font-synthesis-position" => parse_font_synthesis_subproperty(value).is_some(),
        "font-kerning" => parse_font_kerning(value).is_some(),
        "font-weight" => parse_font_weight(value, FontWeight::NORMAL).is_some(),
        "font-style" => parse_font_style(value).is_some(),
        "font-width" | "font-stretch" => parse_font_width(value).is_some(),
        "font-size-adjust" => parse_font_size_adjust(value).is_some(),
        "font-language-override" => parse_font_language_override(value).is_some(),
        "font-variant" => parse_font_variant(value).is_some(),
        "font-variant-ligatures" => parse_font_variant_ligatures(value).is_some(),
        "font-variant-position" => parse_font_variant_position(value).is_some(),
        "object-fit" => parse_object_fit(value).is_some(),
        "object-position" => {
            crate::css::cascade::parse_background_position(value, crate::css::ROOT_FONT_SIZE_PT)
                .is_some()
        }
        "image-orientation" => crate::css::parse_image_orientation(value).is_some(),
        "image-rendering" => crate::css::parse_image_rendering(value).is_some(),
        "font-variant-caps" => parse_font_variant_caps(value).is_some(),
        "font-variant-numeric" => parse_font_variant_numeric(value).is_some(),
        "font-variant-alternates" => parse_font_variant_alternates(value).is_some(),
        "font-variant-east-asian" => parse_font_variant_east_asian(value).is_some(),
        "font-variant-emoji" => parse_font_variant_emoji(value).is_some(),
        "text-indent" => parse_text_indent(value, 12.0).is_some(),
        "hanging-punctuation" => parse_hanging_punctuation(value).is_some(),
        "vertical-align" => parse_vertical_align(value, 12.0).is_some(),
        "dominant-baseline" => parse_dominant_baseline(value).is_some(),
        "alignment-baseline" => parse_alignment_baseline(value).is_some(),
        "baseline-source" => parse_baseline_source(value).is_some(),
        "baseline-shift" => parse_baseline_shift(value, 12.0).is_some(),
        "margin-block" | "margin-inline" => supports_box_edge_axis_value(value, true),
        "padding-block" | "padding-inline" => supports_box_edge_axis_value(value, false),
        "color" | "background-color" => {
            parse_color(value).is_some()
                || crate::css::parse_color_from_currentcolor(value, CssColor::BLACK).is_some()
        }
        "color-scheme" => ComputedColorScheme::parse(value).is_some(),
        "background-origin" => background_box_list_is_valid(value, false),
        "background-clip" => background_box_list_is_valid(value, true),
        "border-color" => parse_border_colors(value).is_some(),
        "border-top-color"
        | "border-right-color"
        | "border-bottom-color"
        | "border-left-color"
        | "border-block-start-color"
        | "border-block-end-color"
        | "border-inline-start-color"
        | "border-inline-end-color" => parse_border_color(value).is_some(),
        "border-block-color" | "border-inline-color" => {
            component_list_is_valid(value, 1..=2, |part| parse_border_color(part).is_some())
        }
        "font-size" => parse_deferred_font_size(value).is_some(),
        "width" | "height" | "inline-size" | "block-size" | "min-width" | "max-width"
        | "min-height" | "max-height" | "min-inline-size" | "max-inline-size"
        | "min-block-size" | "max-block-size" => {
            value.eq_ignore_ascii_case("none")
                || parse_computed_box_size(
                    value,
                    crate::css::ROOT_FONT_SIZE_PT,
                    crate::css::ROOT_FONT_SIZE_PT,
                )
                .is_some()
        }
        "left" | "top" | "right" | "bottom" => {
            parse_computed_length_percentage_auto(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "margin" => component_list_is_valid(value, 1..=4, |part| {
            parse_computed_length_percentage_auto(part, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }),
        "margin-top"
        | "margin-right"
        | "margin-bottom"
        | "margin-left"
        | "margin-block-start"
        | "margin-block-end"
        | "margin-inline-start"
        | "margin-inline-end" => {
            parse_computed_length_percentage_auto(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "padding" => component_list_is_valid(value, 1..=4, |part| {
            parse_computed_length_percentage(part, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }),
        "padding-top"
        | "padding-right"
        | "padding-bottom"
        | "padding-left"
        | "padding-block-start"
        | "padding-block-end"
        | "padding-inline-start"
        | "padding-inline-end" => {
            parse_computed_length_percentage(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "outline-offset" => parse_computed_length_percentage(value, crate::css::ROOT_FONT_SIZE_PT)
            .is_some_and(|length| !length.contains_percentage()),
        "border-shape" => parse_border_shape(value, crate::css::ROOT_FONT_SIZE_PT).is_some(),
        "outline"
        | "border"
        | "border-top"
        | "border-right"
        | "border-bottom"
        | "border-left"
        | "border-block"
        | "border-inline"
        | "border-block-start"
        | "border-block-end"
        | "border-inline-start"
        | "border-inline-end" => border_shorthand_is_valid(value),
        "border-width" => supports_border_width_list(value, 4),
        "border-block-width" | "border-inline-width" => supports_border_width_list(value, 2),
        "outline-width" => supports_border_width_value(value),
        "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width"
        | "border-block-start-width"
        | "border-block-end-width"
        | "border-inline-start-width"
        | "border-inline-end-width" => supports_border_width_value(value),
        "gap" | "row-gap" | "column-gap" | "grid-gap" | "grid-row-gap" | "grid-column-gap" => {
            supports_gap_value(value)
        }
        "column-rule" | "row-rule" | "rule" => {
            parse_gap_rule_shorthand(value, crate::css::ROOT_FONT_SIZE_PT, CssColor::BLACK)
                .is_some()
        }
        "column-rule-width" | "row-rule-width" | "rule-width" => {
            parse_gap_rule_width_list(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "column-rule-style" | "row-rule-style" | "rule-style" => {
            parse_gap_rule_style_list(value).is_some()
        }
        "column-rule-color" | "row-rule-color" | "rule-color" => {
            parse_gap_rule_color_list(value, CssColor::BLACK).is_some()
        }
        "column-rule-break" | "row-rule-break" | "rule-break" => {
            parse_gap_rule_break(value).is_some()
        }
        "column-rule-visibility-items" | "row-rule-visibility-items" | "rule-visibility-items" => {
            parse_gap_rule_visibility_items(value).is_some()
        }
        "rule-overlap" => parse_gap_rule_overlap(value).is_some(),
        "column-rule-inset"
        | "row-rule-inset"
        | "rule-inset"
        | "column-rule-inset-start"
        | "column-rule-inset-end"
        | "row-rule-inset-start"
        | "row-rule-inset-end"
        | "rule-inset-start"
        | "rule-inset-end"
        | "column-rule-inset-cap"
        | "column-rule-inset-junction"
        | "row-rule-inset-cap"
        | "row-rule-inset-junction"
        | "rule-inset-cap"
        | "rule-inset-junction" => supports_gap_rule_inset_shorthand(value),
        "column-rule-inset-cap-start"
        | "column-rule-inset-cap-end"
        | "column-rule-inset-junction-start"
        | "column-rule-inset-junction-end"
        | "row-rule-inset-cap-start"
        | "row-rule-inset-cap-end"
        | "row-rule-inset-junction-start"
        | "row-rule-inset-junction-end" => {
            parse_gap_rule_inset_value(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "position" => {
            parse_running_position(value).is_some()
                || matches!(
                    value.to_ascii_lowercase().as_str(),
                    "static" | "relative" | "absolute" | "fixed" | "sticky"
                )
        }
        "isolation" => matches!(value.to_ascii_lowercase().as_str(), "auto" | "isolate"),
        "mix-blend-mode" => matches!(
            value.to_ascii_lowercase().as_str(),
            "normal"
                | "multiply"
                | "screen"
                | "overlay"
                | "darken"
                | "lighten"
                | "color-dodge"
                | "color-burn"
                | "hard-light"
                | "soft-light"
                | "difference"
                | "exclusion"
                | "hue"
                | "saturation"
                | "color"
                | "luminosity"
        ),
        "filter" | "clip-path" | "mask" | "mask-image" | "will-change" => true,
        "mask-border-source" => crate::css::parse_border_image_source(value).is_some(),
        "mask-border" => {
            crate::css::parse_mask_border_source(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "clip" => {
            crate::css::cascade::parse_legacy_clip(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "transform" => crate::css::parse_transform(value, crate::css::ROOT_FONT_SIZE_PT).is_some(),
        "translate" => {
            crate::css::parse_individual_translate(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "rotate" => crate::css::parse_individual_rotate(value).is_some(),
        "scale" => crate::css::parse_individual_scale(value).is_some(),
        "transform-origin" => {
            crate::css::parse_transform_origin(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "transform-box" => crate::css::parse_transform_box(value).is_some(),
        "object-view-box" => {
            crate::css::parse_object_view_box(value, crate::css::ROOT_FONT_SIZE_PT).is_some()
        }
        "contain-intrinsic-size" => {
            let values = value.split_ascii_whitespace().collect::<Vec<_>>();
            value.eq_ignore_ascii_case("none")
                || matches!(values.as_slice(), [size]
                    if crate::css::values::parse_computed_length_percentage(size, crate::css::ROOT_FONT_SIZE_PT).is_some())
                || matches!(values.as_slice(), [width, height]
                    if crate::css::values::parse_computed_length_percentage(width, crate::css::ROOT_FONT_SIZE_PT).is_some()
                    && crate::css::values::parse_computed_length_percentage(height, crate::css::ROOT_FONT_SIZE_PT).is_some())
        }
        "contain-intrinsic-inline-size" | "contain-intrinsic-block-size" => {
            value.eq_ignore_ascii_case("none")
                || crate::css::values::parse_computed_length_percentage(
                    value,
                    crate::css::ROOT_FONT_SIZE_PT,
                )
                .is_some()
        }
        "contain" => {
            let value = value.to_ascii_lowercase();
            value == "none"
                || value == "strict"
                || value == "content"
                || value.split_whitespace().all(|token| {
                    matches!(token, "size" | "inline-size" | "layout" | "style" | "paint")
                })
        }
        "content-visibility" => matches!(
            value.to_ascii_lowercase().as_str(),
            "visible" | "auto" | "hidden"
        ),
        "float" => matches!(
            value.to_ascii_lowercase().as_str(),
            "left" | "right" | "inline-start" | "inline-end" | "footnote" | "none"
        ),
        "footnote-display" => matches!(
            value.to_ascii_lowercase().as_str(),
            "block" | "inline" | "compact"
        ),
        "footnote-policy" => matches!(
            value.to_ascii_lowercase().as_str(),
            "auto" | "line" | "block"
        ),
        "clear" => matches!(
            value.to_ascii_lowercase().as_str(),
            "left" | "right" | "both" | "inline-start" | "inline-end" | "none"
        ),
        _ => supported_property_name(&name),
    };
    if valid {
        DeclarationParseResult::Valid(DeclarationOperation {
            name,
            value: Cow::Borrowed(value),
        })
    } else if supported_property_name(&name) {
        DeclarationParseResult::InvalidValue
    } else {
        DeclarationParseResult::UnsupportedProperty
    }
}

fn declaration_operation_for_modeled_property<'a>(
    name: Cow<'a, str>,
    value: &'a str,
) -> DeclarationParseResult<'a> {
    if supported_property_name(&name) {
        DeclarationParseResult::Valid(DeclarationOperation {
            name,
            value: Cow::Borrowed(value),
        })
    } else {
        DeclarationParseResult::UnsupportedProperty
    }
}

/// Validates a declaration already classified by the cascade without copying
/// its property name or value.
///
/// Cascaded modeled properties already use their canonical spelling, while
/// custom-property names retain their authored case. Reusing the normal
/// specified-value parser keeps this path aligned with declaration feature
/// queries without rebuilding an owned [`DeclarationOperation`].
pub(in crate::css) fn cascaded_declaration_is_valid(
    property: &CascadedProperty<'_>,
    value: &str,
) -> bool {
    matches!(
        parse_canonical_declaration(property.css_name(), value),
        DeclarationParseResult::Valid(_)
    )
}

/// Validates a whitespace-separated component list without splitting CSS
/// functions, strings, or bracketed blocks.
fn component_list_is_valid(
    value: &str,
    allowed_count: std::ops::RangeInclusive<usize>,
    mut component_is_valid: impl FnMut(&str) -> bool,
) -> bool {
    let components = split_css_component_values(value);
    allowed_count.contains(&components.len()) && components.into_iter().all(&mut component_is_valid)
}

/// Validates the comma-separated box lists accepted by the background origin
/// and clip longhands. A positioning box excludes the Level 4 `border-area`
/// keyword, while clipping accepts it.
fn background_box_list_is_valid(value: &str, allow_border_area: bool) -> bool {
    let values = crate::css::component_values::split_nonempty_css_top_level_delimiter(value, ',');
    !values.is_empty()
        && values.into_iter().all(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "border-box" | "padding-box" | "content-box"
            ) || (allow_border_area && value.trim().eq_ignore_ascii_case("border-area"))
        })
}

/// Validates the unordered width/style/color components shared by `border`
/// and `outline`. The parser used by cascade application supplies omitted
/// components from their initial values, so an empty component list is the
/// only rejected form here.
fn border_shorthand_is_valid(value: &str) -> bool {
    let mut width = false;
    let mut style = false;
    let mut color = false;
    let components = split_css_component_values(value);
    !components.is_empty()
        && components.into_iter().all(|component| {
            if !width
                && parse_computed_border_width(component, crate::css::ROOT_FONT_SIZE_PT).is_some()
            {
                width = true;
                return true;
            }
            if !style && parse_border_style(component).is_some() {
                style = true;
                return true;
            }
            if !color && parse_border_color(component).is_some() {
                color = true;
                return true;
            }
            false
        })
}

/// Validates `text-wrap: <text-wrap-mode> || <text-wrap-style>`.
fn text_wrap_value_is_valid(value: &str) -> bool {
    let mut mode = false;
    let mut style = false;
    component_list_is_valid(value, 1..=2, |component| {
        match component.to_ascii_lowercase().as_str() {
            "wrap" | "nowrap" if !mode => {
                mode = true;
                true
            }
            "auto" | "balance" | "stable" if !style => {
                style = true;
                true
            }
            _ => false,
        }
    })
}

pub(in crate::css) fn supports_declaration_condition(condition: &str) -> bool {
    let declaration = strip_enclosing_parentheses(condition.trim());
    let Some((name, value)) = declaration.split_once(':') else {
        return false;
    };
    matches!(
        parse_canonical_declaration(name, value),
        DeclarationParseResult::Valid(_)
    )
}

/// Returns whether a name is a CSS custom-property name.
///
/// A custom property uses the `<dashed-ident>` grammar, whose two-dash prefix
/// must be followed by an identifier code point. This deliberately preserves
/// ASCII case: `--a` and `--A` are distinct names.
/// <https://www.w3.org/TR/css-variables-1/#defining-variables>
pub(crate) fn is_custom_property_name(name: &str) -> bool {
    name.starts_with("--") && name != "--"
}

/// Validates declaration-level `!important` syntax without interpreting the
/// declaration value. `!important` is permitted exactly once at the top level
/// and must terminate the value; nested component values are not priorities.
fn declaration_priority_is_valid(value: &str) -> bool {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    while !parser.is_exhausted() {
        let Ok(token) = parser.next() else {
            return false;
        };
        match token {
            cssparser::Token::Function(_)
            | cssparser::Token::ParenthesisBlock
            | cssparser::Token::SquareBracketBlock
            | cssparser::Token::CurlyBracketBlock => {
                if parser
                    .parse_nested_block(|input| {
                        while input.next_including_whitespace_and_comments().is_ok() {}
                        Ok::<_, cssparser::ParseError<'_, ()>>(())
                    })
                    .is_err()
                {
                    return false;
                }
            }
            cssparser::Token::Delim('!') => {
                let Ok(cssparser::Token::Ident(ident)) = parser.next() else {
                    return false;
                };
                if !ident.eq_ignore_ascii_case("important") {
                    return false;
                }
                return parser.is_exhausted();
            }
            _ => {}
        }
    }
    true
}

/// Validates component values and reports whether they contain a `var()`
/// reference. CSS Syntax tokenization is used here instead of string matching
/// so comments, escaped identifiers, strings, and nested blocks follow the
/// grammar used by stylesheet parsing.
fn validate_component_values(
    value: &str,
    reject_top_level_bang: bool,
    reject_top_level_semicolon: bool,
) -> Option<bool> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    validate_component_values_from_parser(
        &mut parser,
        reject_top_level_bang,
        reject_top_level_semicolon,
    )
}

/// Returns whether a custom property's token stream is valid at parse time.
///
/// Custom properties otherwise accept arbitrary component values, but malformed
/// `var()` functions and invalid declaration priorities invalidate the whole
/// declaration before the cascade.
pub(crate) fn custom_property_value_is_valid(value: &str) -> bool {
    declaration_priority_is_valid(value) && validate_component_values(value, false, true).is_some()
}

fn validate_component_values_from_parser(
    input: &mut Parser<'_, '_>,
    reject_top_level_bang: bool,
    reject_top_level_semicolon: bool,
) -> Option<bool> {
    let mut contains_variable_reference = false;
    while !input.is_exhausted() {
        let token = input.next().ok()?.clone();
        if token.is_parse_error() {
            return None;
        }
        match token {
            cssparser::Token::Function(name) => {
                let is_variable = name.eq_ignore_ascii_case("var");
                let nested_contains_variable = input
                    .parse_nested_block(|nested| -> Result<bool, cssparser::ParseError<'_, ()>> {
                        let result = if is_variable {
                            parse_var_function_arguments(nested).map(|_| true)
                        } else {
                            validate_component_values_from_parser(nested, false, false)
                        };
                        result.ok_or_else(|| nested.new_custom_error(()))
                    })
                    .ok()?;
                contains_variable_reference |= nested_contains_variable;
            }
            cssparser::Token::ParenthesisBlock
            | cssparser::Token::SquareBracketBlock
            | cssparser::Token::CurlyBracketBlock => {
                let nested_contains_variable = input
                    .parse_nested_block(|nested| -> Result<bool, cssparser::ParseError<'_, ()>> {
                        validate_component_values_from_parser(nested, false, false)
                            .ok_or_else(|| nested.new_custom_error(()))
                    })
                    .ok()?;
                contains_variable_reference |= nested_contains_variable;
            }
            cssparser::Token::Semicolon if reject_top_level_semicolon => return None,
            cssparser::Token::Delim('!') if reject_top_level_bang => {
                return None;
            }
            _ => {}
        }
    }
    Some(contains_variable_reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascaded_validation_matches_the_shared_declaration_parser() {
        for (property_name, value) in [
            ("color", "red"),
            ("color", "not-a-color"),
            ("color", "var(--theme-color)"),
            ("--ThemeColor", "calc(1px + 2px)"),
            ("flex-basis", "10px"),
            ("display", "revert-layer"),
        ] {
            let property = CascadedProperty::try_from_name(Cow::Borrowed(property_name))
                .expect("test property must be represented by the cascade");
            assert_eq!(
                cascaded_declaration_is_valid(&property, value),
                matches!(
                    parse_canonical_declaration(property.css_name(), value),
                    DeclarationParseResult::Valid(_)
                ),
                "{property_name}: {value}"
            );
        }
    }

    #[test]
    fn canonical_parser_preserves_alias_and_unsupported_property_outcomes() {
        assert!(matches!(
            parse_canonical_declaration("-webkit-flex-basis", "10px"),
            DeclarationParseResult::Valid(_)
        ));
        assert!(matches!(
            parse_canonical_declaration("unsupported-property", "value"),
            DeclarationParseResult::UnsupportedProperty
        ));
    }

    #[test]
    fn var_functions_are_validated_as_arbitrary_substitution_functions() {
        for value in [
            "var(1px)",
            "var(--)",
            "var(--name ())",
            "var(var(--name))",
            "var(--name,)",
        ] {
            assert!(
                validate_component_values(value, false, true).is_some(),
                "{value}"
            );
        }
        for value in [
            "var()",
            "var(, red)",
            "var({--name} extra)",
            "var(--name, !red)",
        ] {
            assert!(
                validate_component_values(value, false, true).is_none(),
                "{value}"
            );
        }
    }

    #[test]
    fn invalid_var_name_is_still_a_pending_substitution_at_specified_value_time() {
        assert!(matches!(
            parse_canonical_declaration("color", "var(--, green)"),
            DeclarationParseResult::Valid(_)
        ));
    }
}
