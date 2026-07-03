use super::*;

pub(crate) fn apply_cascaded_marker_declarations_with_inheritance_source_and_parent_ch_advance(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
    parent_ch_advance: f32,
) {
    let (direction, writing_mode) =
        logical_mapping_context(style, declarations, inheritance_source);
    let declarations = declarations_after_css_wide_rollbacks(declarations, direction, writing_mode);
    apply_cascaded_custom_property_declarations(style, &declarations);
    apply_cascaded_font_size_declarations_with_parent_ch_advance(
        style,
        &declarations,
        inheritance_source,
        parent_ch_advance,
    );
    apply_cascaded_color_declarations(style, &declarations, inheritance_source);

    for (index, declaration) in declarations.iter().enumerate() {
        let name = declaration.name.as_ref();
        if name.starts_with("--") || name == "font-size" {
            continue;
        }
        if is_shadowed_by_later_var_declaration(&declarations, index, name) {
            continue;
        }
        let resolved_value;
        let value = trim_css_value(&declaration.value);
        let value = if value.contains("var(") {
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
                if let Some(color) = parse_color(value) {
                    style.color = color;
                }
            }
            "font-family" => {
                style.font_family =
                    parse_font_family(value).unwrap_or_else(|| style.font_family.clone());
            }
            "font-feature-settings" => {
                if let Some(font_feature_settings) = parse_font_feature_settings(value) {
                    style.font_feature_settings = font_feature_settings;
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
                style.white_space = match value.to_ascii_lowercase().as_str() {
                    "normal" => WhiteSpace::Normal,
                    "nowrap" => WhiteSpace::NoWrap,
                    "pre" => WhiteSpace::Pre,
                    "pre-wrap" => WhiteSpace::PreWrap,
                    "pre-line" => WhiteSpace::PreLine,
                    "break-spaces" => WhiteSpace::BreakSpaces,
                    _ => style.white_space,
                };
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
            "quotes" => {
                if let Some(quotes) = parse_quotes(value, &inheritance_source.quotes) {
                    style.quotes = quotes;
                }
            }
            _ => {}
        }
    }
}
