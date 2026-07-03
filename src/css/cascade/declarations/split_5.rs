use super::*;

pub(crate) fn apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
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
        if name.starts_with("--") {
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
        if apply_cascaded_declaration_group_1(
            style,
            name,
            value,
            declaration,
            inheritance_source,
            parent_ch_advance,
        ) {
            continue;
        }
        if apply_cascaded_declaration_group_2(
            style,
            name,
            value,
            declaration,
            inheritance_source,
            parent_ch_advance,
        ) {
            continue;
        }
        if apply_cascaded_declaration_group_3(
            style,
            name,
            value,
            declaration,
            inheritance_source,
            parent_ch_advance,
        ) {
            continue;
        }
    }
}

mod group_1;
pub(in crate::css) use self::group_1::*;
mod group_2;
pub(in crate::css) use self::group_2::*;
mod group_3;
pub(in crate::css) use self::group_3::*;
