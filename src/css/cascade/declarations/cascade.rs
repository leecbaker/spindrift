use super::*;

/// A declaration after selector matching and cascade ordering, with its origin
/// and URL base preserved for computed-value application.
///
/// CSS Cascade Level 5 orders declarations by origin, importance, layer order,
/// specificity, scoped proximity, and source order before computed-value
/// resolution:
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
#[derive(Debug, Clone)]
pub(crate) struct CascadedDeclaration<'a> {
    pub name: Cow<'a, str>,
    pub value: Cow<'a, str>,
    pub origin: StylesheetOrigin,
    pub base_url: Option<&'a url::Url>,
    pub root_url: Option<&'a url::Url>,
    pub important: bool,
    pub layer_order: Option<usize>,
    pub specificity: u32,
    pub scope_proximity: usize,
    pub stylesheet_index: usize,
    pub rule_order: usize,
    pub declaration_order: usize,
}

pub(in crate::css) fn cascaded_declarations_from(
    declarations: &Declarations,
    origin: StylesheetOrigin,
) -> Vec<CascadedDeclaration<'_>> {
    declarations
        .iter()
        .enumerate()
        .map(|(declaration_order, (name, value))| CascadedDeclaration {
            name: Cow::Borrowed(name.as_str()),
            value: Cow::Borrowed(value.as_str()),
            origin,
            base_url: declarations.base_url(),
            root_url: declarations.root_url(),
            important: declaration_is_important(value),
            layer_order: None,
            specificity: 0,
            scope_proximity: usize::MAX,
            stylesheet_index: 0,
            rule_order: 0,
            declaration_order,
        })
        .collect()
}

/// Sorts declarations into winning cascade order before computed-value resolution.
///
/// CSS Cascade Level 5 sorts by origin/importance, layer order, specificity,
/// scoped proximity, and source order. Quire currently models UA, user,
/// and author origins:
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
pub(crate) fn sort_cascaded_declarations(declarations: &mut [CascadedDeclaration<'_>]) {
    declarations.sort_by_key(|declaration| {
        (
            origin_importance_rank(declaration.origin, declaration.important),
            layer_precedence_rank(declaration.layer_order, declaration.important),
            declaration.specificity,
            scope_proximity_rank(declaration.scope_proximity),
            declaration.stylesheet_index,
            declaration.rule_order,
            declaration.declaration_order,
        )
    });
}

/// Returns a weakest-to-strongest layer rank within an origin/importance band.
///
/// CSS Cascade Level 5 says normal unlayered declarations outrank all layered
/// normal declarations, while important declarations reverse layer order and
/// place unlayered important declarations before layered important declarations:
/// <https://www.w3.org/TR/css-cascade-5/#layering>.
pub(in crate::css) fn layer_precedence_rank(layer_order: Option<usize>, important: bool) -> usize {
    match (important, layer_order) {
        (false, Some(order)) => order,
        (false, None) => usize::MAX,
        (true, None) => 0,
        (true, Some(order)) => usize::MAX.saturating_sub(1).saturating_sub(order),
    }
}

/// Converts Cascade 5 scoped proximity to weakest-to-strongest sort rank.
///
/// Smaller ancestor distance to the scoping root is stronger, while unscoped
/// declarations sort as the least proximate in the scoped-proximity step:
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
pub(in crate::css) fn scope_proximity_rank(scope_proximity: usize) -> usize {
    usize::MAX.saturating_sub(scope_proximity)
}

/// Detects a declaration's `!important` priority flag.
///
/// CSS Cascade Level 5 treats importance as part of cascade sorting, not as
/// part of the property value:
/// <https://www.w3.org/TR/css-cascade-5/#importance>.
pub(crate) fn declaration_is_important(value: &str) -> bool {
    value
        .trim_end()
        .to_ascii_lowercase()
        .ends_with("!important")
}

/// Returns the Cascade Level 5 origin/importance rank from weakest to strongest.
///
/// Quire currently has no transition or animation origin, so the modeled
/// origin ladder is UA normal, user normal, author normal, author important,
/// user important, then UA important:
/// <https://www.w3.org/TR/css-cascade-5/#cascade-origin>.
pub(crate) fn origin_importance_rank(origin: StylesheetOrigin, important: bool) -> u8 {
    match (origin, important) {
        (StylesheetOrigin::UserAgent, false) => 0,
        (StylesheetOrigin::User, false) => 1,
        (StylesheetOrigin::Author, false) => 2,
        (StylesheetOrigin::Author, true) => 3,
        (StylesheetOrigin::User, true) => 4,
        (StylesheetOrigin::UserAgent, true) => 5,
    }
}

/// Returns whether an earlier exact property must be suppressed by a later
/// custom-property-using declaration for the same property.
///
/// CSS Cascade Level 5 requires the winning specified value to be substituted
/// at computed-value time. If that substitution is invalid, the UA must not
/// roll back to an earlier cascaded declaration:
/// <https://www.w3.org/TR/css-cascade-5/#invalid-at-computed-value-time>.
pub(in crate::css) fn is_shadowed_by_later_var_declaration(
    declarations: &[CascadedDeclaration<'_>],
    index: usize,
    name: &str,
) -> bool {
    declarations[index + 1..].iter().any(|declaration| {
        declaration.name.as_ref() == name
            && contains_css_variable_reference(trim_css_value(&declaration.value))
    })
}

/// Applies CSS-wide cascade rollback keywords after shorthand expansion.
///
/// CSS Cascade Level 5 defines `revert` as rolling a property back to the
/// previous cascade origin and `revert-layer` as rolling it back to the layer
/// below. This pass runs after cascade sorting and before computed-value
/// application, removing earlier declarations that the rollback makes
/// inapplicable:
/// <https://www.w3.org/TR/css-cascade-5/#revert> and
/// <https://www.w3.org/TR/css-cascade-5/#revert-layer>.
pub(in crate::css) fn declarations_after_css_wide_rollbacks<'a>(
    declarations: &'a [CascadedDeclaration<'a>],
    direction: Direction,
    writing_mode: WritingMode,
) -> Vec<CascadedDeclaration<'a>> {
    let declarations = expand_modeled_shorthands(declarations, direction, writing_mode);
    let mut output = Vec::with_capacity(declarations.len());
    for declaration in &declarations {
        if declaration_is_revert(&declaration.value) {
            output.retain(|candidate: &CascadedDeclaration<'_>| {
                !declarations_affect_same_property_in_context(
                    &candidate.name,
                    &declaration.name,
                    direction,
                    writing_mode,
                ) || !same_or_stronger_reverted_origin(candidate, declaration)
            });
        } else if declaration_is_revert_layer(&declaration.value) {
            output.retain(|candidate: &CascadedDeclaration<'_>| {
                !declarations_affect_same_property_in_context(
                    &candidate.name,
                    &declaration.name,
                    direction,
                    writing_mode,
                ) || !same_cascade_layer(candidate, declaration)
            });
        } else {
            output.push(declaration.clone());
        }
    }
    output
}

/// Returns whether two parsed declarations affect at least one same longhand.
///
/// CSS Cascade Level 5 applies CSS-wide keywords such as `revert-layer` to the
/// longhands represented by a shorthand, so cascade rollback has to compare
/// affected longhands instead of only exact serialized declaration names:
/// <https://www.w3.org/TR/css-cascade-5/#shorthand> and
/// <https://www.w3.org/TR/css-cascade-5/#revert-layer>.
pub(crate) fn declarations_affect_same_property(left: &str, right: &str) -> bool {
    declarations_affect_same_property_in_context(
        left,
        right,
        Direction::Ltr,
        WritingMode::HorizontalTb,
    )
}

pub(in crate::css) fn declarations_affect_same_property_in_context(
    left: &str,
    right: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> bool {
    if left.eq_ignore_ascii_case(right) {
        return true;
    }
    let Some(left_longhands) = affected_longhands(left, direction, writing_mode) else {
        return false;
    };
    let Some(right_longhands) = affected_longhands(right, direction, writing_mode) else {
        return false;
    };
    left_longhands
        .iter()
        .any(|left| right_longhands.iter().any(|right| left == right))
}
