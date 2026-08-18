use super::*;

pub(crate) fn apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
    parent_ch_advance: LayoutLength,
    is_document_root: bool,
    color_scheme_preference: ColorSchemePreference,
) {
    let (direction, writing_mode) =
        logical_mapping_context(style, declarations, inheritance_source);
    let mut declarations =
        declarations_after_css_wide_rollbacks(declarations, direction, writing_mode);
    // A declaration is ignored when its specified value does not match the
    // property's grammar.  Keep this boundary before the font-size/color
    // prepasses: CSS Conditional declaration queries and ordinary cascade
    // application must agree about the same specified-value operation.
    // <https://www.w3.org/TR/css-cascade-5/#filtering>
    declarations.retain(cascaded_declaration_is_canonical);
    apply_cascaded_custom_property_declarations(style, &declarations, inheritance_source);
    apply_cascaded_color_scheme_declarations(
        style,
        &declarations,
        inheritance_source,
        color_scheme_preference,
    );
    // Registered `<color>` values resolve `light-dark()` in the color scheme
    // selected by their owning element.  Keep their token form until the
    // color-scheme prepass has established that scheme, then substitute the
    // typed computed value into ordinary declarations.
    compute_registered_custom_property_values(style);
    let declarations = declarations_after_variable_substitution_and_shorthand_expansion(
        &declarations,
        &style.custom_properties,
        direction,
        writing_mode,
    );
    apply_cascaded_font_size_declarations_with_parent_ch_advance(
        style,
        &declarations,
        inheritance_source,
        parent_ch_advance,
    );
    apply_cascaded_color_declarations(style, &declarations, inheritance_source);

    // Font shorthand expansion emits consecutive components with their source
    // cascade metadata intact. Reuse one parsed shorthand for that group.
    let mut parsed_font_component = None;
    for (index, declaration) in declarations.iter().enumerate() {
        let Some(property) = declaration.property.modeled() else {
            continue;
        };
        let name = property.css_name();
        // These properties are fully resolved by dependency-ordered prepasses
        // above, including their CSS-wide keywords. Letting the generic
        // handler process an earlier defaulting declaration again would
        // overwrite the prepass's later winning longhand.
        if matches!(
            property,
            ModeledProperty::Longhand(
                ModeledLonghand::ColorScheme | ModeledLonghand::FontSize | ModeledLonghand::Color
            ) | ModeledProperty::FontComponent(ModeledLonghand::FontSize)
        ) {
            continue;
        }
        if is_shadowed_by_later_var_declaration(&declarations, index, &declaration.property) {
            continue;
        }
        let resolved_value;
        let value = trim_css_value(&declaration.value);
        let value = if contains_css_variable_reference(value) {
            let Some(resolved) = resolve_css_variables(value, &style.custom_properties) else {
                if matches!(
                    property,
                    ModeledProperty::Longhand(
                        ModeledLonghand::AnimationName
                            | ModeledLonghand::AnimationDuration
                            | ModeledLonghand::AnimationDelay
                    )
                ) {
                    // A winning unresolved variable makes this non-inherited
                    // animation longhand invalid at computed-value time. It
                    // therefore computes to its initial value; an earlier
                    // declaration must not revive.
                    apply_css_wide_default_keyword(
                        style,
                        property,
                        CssWideDefaultKeyword::Initial,
                        inheritance_source,
                    );
                }
                continue;
            };
            resolved_value = resolved;
            trim_css_value(&resolved_value)
        } else {
            value
        };
        if let Some(keyword) = CssWideDefaultKeyword::parse(value) {
            apply_css_wide_default_keyword(style, property, keyword, inheritance_source);
            continue;
        }
        if let Some(component) = property.font_component() {
            let source = (
                declaration.stylesheet_index,
                declaration.rule_order,
                declaration.declaration_order,
            );
            if parsed_font_component
                .as_ref()
                .is_none_or(|(cached_source, _)| *cached_source != source)
            {
                parsed_font_component = parse_font_shorthand_with_line_height_font_size(
                    value,
                    inheritance_source.font_size,
                    parent_ch_advance,
                    style.font_weight,
                    Some(style.font_size),
                    layout_pt(inheritance_source.line_height),
                )
                .map(|font| (source, font));
            }
            if let Some((_, font)) = &parsed_font_component {
                apply_font_shorthand_component(style, component, font);
            }
            continue;
        }
        parsed_font_component = None;
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
        if apply_cascaded_property(
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
    style.resolve_legacy_webkit_line_clamp();
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
    // `zoom` itself is non-inherited, but its used value is the product of
    // the local computed value and all flat-tree ancestor values.  Keep that
    // layout-only quantity with the computed style so every later used-value
    // boundary receives the same semantic scale.
    // <https://drafts.csswg.org/css-viewport/#zoom-property>
    style.effective_zoom =
        EffectiveZoom::from_parent_and_local(inheritance_source.effective_zoom, style.zoom);
}

/// Routes a canonical declaration to its owning feature handler.
///
/// The handlers retain the former mutually exclusive property match arms, in
/// their original cascade order. Keeping this dispatch boundary centralized
/// makes the cascade driver independent from how feature handlers are split
/// across files.
fn apply_cascaded_property(
    style: &mut ComputedStyle,
    name: &str,
    value: &str,
    declaration: &CascadedDeclaration<'_>,
    inheritance_source: &ComputedStyle,
    parent_ch_advance: LayoutLength,
) -> bool {
    if super::super::style::apply_animation_snapshot_longhand(style, name, value) {
        return true;
    }
    if name == "color-scheme" {
        return true;
    }
    apply_cascaded_layout_declaration(
        style,
        name,
        value,
        declaration,
        inheritance_source,
        parent_ch_advance,
    ) || apply_cascaded_sizing_declaration(
        style,
        name,
        value,
        declaration,
        inheritance_source,
        parent_ch_advance,
    ) || apply_cascaded_positioning_declaration(
        style,
        name,
        value,
        declaration,
        inheritance_source,
        parent_ch_advance,
    ) || apply_cascaded_visual_declaration(
        style,
        name,
        value,
        declaration,
        inheritance_source,
        parent_ch_advance,
    ) || apply_cascaded_text_declaration(
        style,
        name,
        value,
        declaration,
        inheritance_source,
        parent_ch_advance,
    )
}

/// Applies the modeled text properties that can affect a `::marker`'s text.
///
/// Marker boxes do not accept ordinary layout properties, but CSS Lists allows
/// text properties to cascade to their generated text. Keep the applicability
/// boundary here and reuse the ordinary declaration dispatcher so marker and
/// element values cannot drift apart.
/// <https://drafts.csswg.org/css-lists-3/#marker-properties>
pub(in crate::css) fn apply_cascaded_marker_text_property(
    style: &mut ComputedStyle,
    name: &str,
    value: &str,
    declaration: &CascadedDeclaration<'_>,
    inheritance_source: &ComputedStyle,
    parent_ch_advance: LayoutLength,
) -> bool {
    if !matches!(
        name,
        "direction"
            | "unicode-bidi"
            | "text-orientation"
            | "text-combine-upright"
            | "letter-spacing"
            | "word-spacing"
            | "tab-size"
            | "word-break"
            | "overflow-wrap"
            | "word-wrap"
            | "line-break"
            | "hyphens"
            | "text-decoration"
            | "text-decoration-line"
            | "text-decoration-style"
            | "text-decoration-color"
            | "text-decoration-thickness"
            | "text-decoration-inset"
            | "text-decoration-skip"
            | "text-decoration-skip-ink"
            | "text-decoration-skip-self"
            | "text-decoration-skip-box"
            | "text-decoration-skip-spaces"
            | "text-underline-offset"
            | "text-underline-position"
            | "text-emphasis"
            | "text-emphasis-style"
            | "text-emphasis-color"
            | "text-emphasis-position"
            | "text-emphasis-skip"
            | "text-shadow"
    ) {
        return false;
    }

    apply_cascaded_property(
        style,
        name,
        value,
        declaration,
        inheritance_source,
        parent_ch_advance,
    )
}

/// Whether a cascaded declaration survives specified-value validation.
///
/// Font shorthand components retain the original shorthand token stream until
/// their font-relative values are resolved, so their component token stream is
/// intentionally exempt from ordinary property grammar validation here.
pub(in crate::css) fn cascaded_declaration_is_canonical(
    declaration: &CascadedDeclaration<'_>,
) -> bool {
    // `font` is checked at specified-value time before it is split into
    // component declarations. A component retains the original shorthand
    // token stream, which is intentionally not valid as the component's own
    // grammar until its font-relative values are resolved below.
    if declaration
        .property
        .modeled()
        .is_some_and(|property| property.font_component().is_some())
    {
        return true;
    }
    crate::css::parse::cascaded_declaration_is_valid(
        &declaration.property,
        declaration.value.as_ref(),
    )
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

mod layout;
pub(in crate::css) use self::layout::*;
mod positioning;
pub(in crate::css) use self::positioning::*;
mod sizing;
pub(in crate::css) use self::sizing::*;
mod visual;
pub(in crate::css) use self::visual::*;
mod text;
pub(in crate::css) use self::text::*;
