use super::*;
use crate::css::is_custom_property_name;

pub(crate) fn apply_cascaded_marker_declarations_with_inheritance_source_and_parent_ch_advance(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
    parent_ch_advance: LayoutLength,
) {
    let (direction, writing_mode) =
        logical_mapping_context(style, declarations, inheritance_source);
    let declarations = declarations_after_css_wide_rollbacks(declarations, direction, writing_mode);
    let declarations = declarations
        .into_iter()
        .filter_map(canonical_cascaded_declaration)
        .collect::<Vec<_>>();
    apply_cascaded_custom_property_declarations(style, &declarations);
    apply_cascaded_font_size_declarations_with_parent_ch_advance(
        style,
        &declarations,
        inheritance_source,
        parent_ch_advance,
    );
    apply_cascaded_color_declarations(style, &declarations, inheritance_source);
    let declarations = declarations_after_variable_substitution_and_shorthand_expansion(
        &declarations,
        &style.custom_properties,
        direction,
        writing_mode,
    );

    for (index, declaration) in declarations.iter().enumerate() {
        let name = declaration.name.as_ref();
        if is_custom_property_name(name) || name == "font-size" {
            continue;
        }
        if is_shadowed_by_later_var_declaration(&declarations, index, name) {
            continue;
        }
        let resolved_value;
        let value = trim_css_value(&declaration.value);
        let value = if contains_css_variable_reference(value) {
            let Some(resolved) = resolve_css_variables(value, &style.custom_properties) else {
                continue;
            };
            resolved_value = resolved;
            trim_css_value(&resolved_value)
        } else {
            value
        };
        if let Some(keyword) = CssWideDefaultKeyword::parse(value) {
            apply_css_wide_default_keyword(style, name, keyword, inheritance_source);
            continue;
        }
        match name {
            "color" => {
                // The shared color prepass has already selected and resolved
                // the winning `color` declaration for this marker style.
            }
            "-webkit-text-fill-color" => {
                if value.eq_ignore_ascii_case("currentcolor") {
                    style.text_fill_color = None;
                } else if let Some(color) = parse_color(value) {
                    style.text_fill_color = Some(color);
                }
            }
            "font-family" => {
                style.font_family =
                    parse_font_family(value).unwrap_or_else(|| style.font_family.clone());
            }
            "font-synthesis" => {
                if let Some(font_synthesis) = parse_font_synthesis(value) {
                    style.font_synthesis = font_synthesis;
                }
            }
            "font-synthesis-weight" => {
                if let Some(value) = parse_font_synthesis_subproperty(value) {
                    style.font_synthesis.weight = value;
                }
            }
            "font-synthesis-style" => {
                if let Some(value) = parse_font_synthesis_subproperty(value) {
                    style.font_synthesis.style = value;
                }
            }
            "font-synthesis-small-caps" => {
                if let Some(value) = parse_font_synthesis_subproperty(value) {
                    style.font_synthesis.small_caps = value;
                }
            }
            "font-synthesis-position" => {
                if let Some(value) = parse_font_synthesis_subproperty(value) {
                    style.font_synthesis.position = value;
                }
            }
            "font-feature-settings" => {
                if let Some(font_feature_settings) = parse_font_feature_settings(value) {
                    style.font_feature_settings = font_feature_settings;
                }
            }
            "font-variation-settings" => {
                if let Some(font_variation_settings) = parse_font_variation_settings(value) {
                    style.font_variation_settings = font_variation_settings;
                }
            }
            "font-size-adjust" => {
                if let Some(font_size_adjust) = parse_font_size_adjust(value) {
                    style.font_size_adjust = font_size_adjust;
                }
            }
            "font-kerning" => {
                if let Some(font_kerning) = parse_font_kerning(value) {
                    style.font_kerning = font_kerning;
                }
            }
            "font-variant" => {
                if let Some(font_variant) = parse_font_variant(value) {
                    style.font_variant_ligatures = font_variant.ligatures;
                    style.font_variant_position = font_variant.position;
                    style.font_variant_caps = font_variant.caps;
                    style.font_variant_numeric = font_variant.numeric;
                    style.font_variant_alternates = font_variant.alternates;
                    style.font_variant_east_asian = font_variant.east_asian;
                    style.font_variant_emoji = font_variant.emoji;
                }
            }
            "font-variant-ligatures" => {
                if let Some(font_variant_ligatures) = parse_font_variant_ligatures(value) {
                    style.font_variant_ligatures = font_variant_ligatures;
                }
            }
            "font-variant-position" => {
                if let Some(font_variant_position) = parse_font_variant_position(value) {
                    style.font_variant_position = font_variant_position;
                }
            }
            "font-variant-caps" => {
                if let Some(font_variant_caps) = parse_font_variant_caps(value) {
                    style.font_variant_caps = font_variant_caps;
                }
            }
            "font-variant-numeric" => {
                if let Some(font_variant_numeric) = parse_font_variant_numeric(value) {
                    style.font_variant_numeric = font_variant_numeric;
                }
            }
            "font-variant-alternates" => {
                if let Some(font_variant_alternates) = parse_font_variant_alternates(value) {
                    style.font_variant_alternates = font_variant_alternates;
                }
            }
            "font-variant-east-asian" => {
                if let Some(font_variant_east_asian) = parse_font_variant_east_asian(value) {
                    style.font_variant_east_asian = font_variant_east_asian;
                }
            }
            "font-variant-emoji" => {
                if let Some(font_variant_emoji) = parse_font_variant_emoji(value) {
                    style.font_variant_emoji = font_variant_emoji;
                }
            }
            "font-palette" => {
                if let Some(font_palette) = parse_font_palette(value) {
                    style.font_palette = font_palette;
                }
            }
            "font-weight" => {
                if let Some(weight) = parse_font_weight(value, style.font_weight) {
                    style.font_weight = weight;
                }
            }
            "font-style" => {
                if let Some(font_style) = parse_font_style(value) {
                    style.font_style = font_style;
                }
            }
            "font-width" | "font-stretch" => {
                if let Some(width) = parse_font_width(value) {
                    style.font_width = width;
                }
            }
            "white-space" => {
                let parsed = match value.to_ascii_lowercase().as_str() {
                    "normal" => Some((WhiteSpace::Normal, TextWrapMode::Wrap)),
                    "nowrap" => Some((WhiteSpace::NoWrap, TextWrapMode::NoWrap)),
                    "pre" => Some((WhiteSpace::Pre, TextWrapMode::NoWrap)),
                    "pre-wrap" => Some((WhiteSpace::PreWrap, TextWrapMode::Wrap)),
                    "pre-line" => Some((WhiteSpace::PreLine, TextWrapMode::Wrap)),
                    "break-spaces" => Some((WhiteSpace::BreakSpaces, TextWrapMode::Wrap)),
                    _ => None,
                };
                if let Some((white_space, text_wrap_mode)) = parsed {
                    style.white_space = white_space;
                    style.text_wrap_mode = text_wrap_mode;
                }
            }
            "text-wrap" => {
                let mut mode = TextWrapMode::Wrap;
                let mut wrap_style = TextWrapStyle::Auto;
                let mut saw_mode = false;
                let mut saw_style = false;
                let mut valid = true;
                for component in value.split_ascii_whitespace() {
                    match component.to_ascii_lowercase().as_str() {
                        "wrap" if !saw_mode => {
                            mode = TextWrapMode::Wrap;
                            saw_mode = true;
                        }
                        "nowrap" if !saw_mode => {
                            mode = TextWrapMode::NoWrap;
                            saw_mode = true;
                        }
                        "auto" if !saw_style => {
                            wrap_style = TextWrapStyle::Auto;
                            saw_style = true;
                        }
                        "balance" if !saw_style => {
                            wrap_style = TextWrapStyle::Balance;
                            saw_style = true;
                        }
                        "stable" if !saw_style => {
                            wrap_style = TextWrapStyle::Stable;
                            saw_style = true;
                        }
                        _ => valid = false,
                    }
                }
                if valid && (saw_mode || saw_style) {
                    style.text_wrap_mode = mode;
                    style.text_wrap_style = wrap_style;
                }
            }
            "text-wrap-mode" => match value.trim().to_ascii_lowercase().as_str() {
                "wrap" => style.text_wrap_mode = TextWrapMode::Wrap,
                "nowrap" => style.text_wrap_mode = TextWrapMode::NoWrap,
                _ => {}
            },
            "text-wrap-style" => match value.trim().to_ascii_lowercase().as_str() {
                "auto" => style.text_wrap_style = TextWrapStyle::Auto,
                "balance" => style.text_wrap_style = TextWrapStyle::Balance,
                "stable" => style.text_wrap_style = TextWrapStyle::Stable,
                _ => {}
            },
            "wrap-inside" => match value.trim().to_ascii_lowercase().as_str() {
                "auto" => style.wrap_inside = WrapInside::Auto,
                "avoid" => style.wrap_inside = WrapInside::Avoid,
                _ => {}
            },
            "line-clamp" | "-webkit-line-clamp" => {
                let value = value.trim().to_ascii_lowercase();
                if value == "none" {
                    style.line_clamp = None;
                } else if let Ok(value) = value.parse::<usize>()
                    && value > 0
                {
                    style.line_clamp = Some(LineClamp::new(value, name == "-webkit-line-clamp"));
                }
            }
            "text-transform" => {
                if let Some(transform) = parse_text_transform(value) {
                    style.text_transform = transform;
                }
            }
            "list-style-type" => {
                style.list_style_type =
                    parse_list_style_type(value).unwrap_or_else(|| style.list_style_type.clone());
            }
            "content" => {
                style.marker_content =
                    parse_marker_content(value).unwrap_or_else(|| style.marker_content.clone());
                if let Some(content) =
                    parse_content_property(value, declaration.base_url, declaration.root_url)
                {
                    style.content = content;
                }
            }
            "counter-reset" => {
                if let Some(values) = parse_counter_resets(value) {
                    style.counter_resets = values;
                }
            }
            "counter-increment" => {
                if let Some(values) =
                    parse_counter_changes(value, 1, CounterDuplicatePolicy::KeepAll)
                {
                    style.counter_increments = values;
                }
            }
            "counter-set" => {
                if let Some(values) =
                    parse_counter_changes(value, 0, CounterDuplicatePolicy::KeepLast)
                {
                    style.counter_sets = values;
                }
            }
            "quotes" => {
                if let Some(quotes) = parse_quotes(value, &inheritance_source.quotes) {
                    style.quotes = quotes;
                }
            }
            _ => {}
        }
    }
}
