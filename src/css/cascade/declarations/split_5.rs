use super::*;
use crate::css::is_custom_property_name;

pub(crate) fn apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
    parent_ch_advance: LayoutLength,
    is_document_root: bool,
) {
    let (direction, writing_mode) =
        logical_mapping_context(style, declarations, inheritance_source);
    let declarations = declarations_after_css_wide_rollbacks(declarations, direction, writing_mode);
    // A declaration is ignored when its specified value does not match the
    // property's grammar.  Keep this boundary before the font-size/color
    // prepasses: CSS Conditional declaration queries and ordinary cascade
    // application must agree about the same specified-value operation.
    // <https://www.w3.org/TR/css-cascade-5/#filtering>
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
        if is_custom_property_name(name) {
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
        // `match-parent` resolves the root's alignment as `start`: the root
        // has no element parent from which to inherit a physical start/end
        // direction, while its own used `direction` still maps logical start
        // to the correct inline edge. Descendants continue to resolve the
        // keyword against their parent's direction in `parse_text_align_all`:
        // <https://www.w3.org/TR/css-text-3/#text-align-property>.
        if is_document_root && value.eq_ignore_ascii_case("match-parent") {
            match name {
                "text-align" => {
                    style.text_align = TextAlign::Start;
                    style.text_align_last = TextAlignLast::Auto;
                    continue;
                }
                "text-align-all" => {
                    style.text_align = TextAlign::Start;
                    continue;
                }
                _ => {}
            }
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
    normalize_overflow_axes(style);
    normalize_background_layers(style);
    // `em` and `rem` components remain deferred until layout has resolved a
    // viewport-dependent `font-size` chain. Resolving them here would freeze
    // dependent box values against the cascade's provisional font size.
    // <https://www.w3.org/TR/css-values-4/#em>
    // <https://www.w3.org/TR/css-values-4/#rem>
    style
        .row_gap
        .reduce_math_with_nonnegative_percentage_basis();
    style
        .column_gap
        .reduce_math_with_nonnegative_percentage_basis();
    if let Some(expression) = &style.background_color_current_color_expression {
        style.background_color = parse_color_from_currentcolor(expression, style.color);
    } else if style.background_color_is_current_color {
        style.background_color = Some(style.color);
    }
    // `zoom` itself is non-inherited, but its used value is the product of
    // the local computed value and all flat-tree ancestor values.  Keep that
    // layout-only quantity with the computed style so every later used-value
    // boundary receives the same semantic scale.
    // <https://drafts.csswg.org/css-viewport/#zoom-property>
    style.effective_zoom =
        EffectiveZoom::from_parent_and_local(inheritance_source.effective_zoom, style.zoom);
}

pub(in crate::css) fn canonical_cascaded_declaration<'a>(
    declaration: CascadedDeclaration<'a>,
) -> Option<CascadedDeclaration<'a>> {
    let (name, value) = crate::css::parse::declaration_operation(
        declaration.name.as_ref(),
        declaration.value.as_ref(),
    )?;
    Some(CascadedDeclaration {
        name: Cow::Owned(name),
        value: Cow::Owned(value),
        ..declaration
    })
}

/// Apply Overflow's cross-axis computed-value adjustment.
///
/// A scrollable or clipped axis establishes the used overflow behavior of a
/// scroll container, so the other axis cannot remain `visible` or `clip`.
/// This happens after the complete cascade rather than while applying either
/// longhand, because cascade order must not affect the paired computed value.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-properties>.
fn normalize_overflow_axes(style: &mut ComputedStyle) {
    let x_is_scroll_container = !matches!(style.overflow_x, Overflow::Visible | Overflow::Clip);
    let y_is_scroll_container = !matches!(style.overflow_y, Overflow::Visible | Overflow::Clip);
    if x_is_scroll_container {
        style.overflow_y = match style.overflow_y {
            Overflow::Visible => Overflow::Auto,
            Overflow::Clip => Overflow::Hidden,
            overflow => overflow,
        };
    }
    if y_is_scroll_container {
        style.overflow_x = match style.overflow_x {
            Overflow::Visible => Overflow::Auto,
            Overflow::Clip => Overflow::Hidden,
            overflow => overflow,
        };
    }
}

mod group_1;
pub(in crate::css) use self::group_1::*;
mod group_2;
pub(in crate::css) use self::group_2::*;
mod group_3;
pub(in crate::css) use self::group_3::*;
