use super::*;
use crate::css::LayerOrder;
use crate::css::is_custom_property_name;

/// A declaration after selector matching and cascade ordering, with its origin
/// and URL base preserved for computed-value application.
///
/// CSS Cascade Level 5 orders declarations by origin, importance, layer order,
/// specificity, scoped proximity, and source order before computed-value
/// resolution:
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
#[derive(Debug, Clone)]
pub(crate) struct CascadedDeclaration<'a> {
    /// Typed property identity for cascade mechanics. Only custom properties
    /// retain an authored name; modeled properties serialize from their
    /// canonical enum identity at legacy parser boundaries.
    pub(in crate::css) property: CascadedProperty<'a>,
    pub value: Cow<'a, str>,
    pub origin: StylesheetOrigin,
    pub base_url: Option<&'a url::Url>,
    pub root_url: Option<&'a url::Url>,
    pub important: bool,
    pub layer_order: Option<LayerOrder>,
    pub specificity: u32,
    pub scope_proximity: usize,
    pub stylesheet_index: usize,
    pub rule_order: usize,
    pub declaration_order: usize,
}

/// The property kind carried by an ordinary cascade declaration.
///
/// Custom properties keep their original case-sensitive spelling in the
/// `Custom` variant, while all supported ordinary properties are classified
/// once into the typed syntax model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::css) enum CascadedProperty<'a> {
    Modeled(ModeledProperty),
    Custom(Cow<'a, str>),
}

impl<'a> CascadedProperty<'a> {
    /// Classify a declaration that has survived CSS parsing for the typed
    /// cascade. Unknown non-custom properties are ignored at this boundary,
    /// as required by CSS Syntax's declaration error handling.
    /// <https://www.w3.org/TR/css-syntax-3/#declaration>
    pub(in crate::css) fn try_from_name(name: Cow<'a, str>) -> Option<Self> {
        if is_custom_property_name(&name) {
            Some(Self::Custom(name))
        } else {
            ModeledProperty::parse(&name).map(Self::Modeled)
        }
    }

    pub(in crate::css) fn from_name(name: Cow<'a, str>) -> Self {
        Self::try_from_name(name)
            .expect("internal cascade construction must use a modeled or custom property")
    }

    pub(in crate::css) fn modeled(&self) -> Option<&ModeledProperty> {
        match self {
            Self::Modeled(property) => Some(property),
            Self::Custom(_) => None,
        }
    }

    pub(in crate::css) fn css_name(&self) -> &str {
        match self {
            Self::Modeled(property) => property.css_name(),
            Self::Custom(name) => name,
        }
    }

    pub(in crate::css) fn custom_name(&self) -> Option<&str> {
        match self {
            Self::Modeled(_) => None,
            Self::Custom(name) => Some(name),
        }
    }
}

pub(in crate::css) fn cascaded_declarations_from(
    declarations: &Declarations,
    origin: StylesheetOrigin,
) -> Vec<CascadedDeclaration<'_>> {
    declarations
        .iter()
        .enumerate()
        .filter_map(|(declaration_order, (name, value))| {
            Some(CascadedDeclaration {
                property: CascadedProperty::try_from_name(Cow::Borrowed(name.as_str()))?,
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
    declarations.sort_by(|left, right| {
        origin_importance_rank(left.origin, left.important)
            .cmp(&origin_importance_rank(right.origin, right.important))
            .then_with(|| {
                compare_layer_order(
                    left.layer_order.as_ref(),
                    right.layer_order.as_ref(),
                    left.important,
                )
            })
            .then_with(|| left.specificity.cmp(&right.specificity))
            .then_with(|| {
                scope_proximity_rank(left.scope_proximity)
                    .cmp(&scope_proximity_rank(right.scope_proximity))
            })
            .then_with(|| left.stylesheet_index.cmp(&right.stylesheet_index))
            .then_with(|| left.rule_order.cmp(&right.rule_order))
            .then_with(|| left.declaration_order.cmp(&right.declaration_order))
    });
}

/// Returns a weakest-to-strongest layer rank within an origin/importance band.
///
/// CSS Cascade Level 5 says normal unlayered declarations outrank all layered
/// normal declarations, while important declarations reverse layer order and
/// place unlayered important declarations before layered important declarations:
/// <https://www.w3.org/TR/css-cascade-5/#layering>.
pub(in crate::css) fn compare_layer_order(
    left: Option<&LayerOrder>,
    right: Option<&LayerOrder>,
    important: bool,
) -> std::cmp::Ordering {
    match (important, left, right) {
        (false, Some(left), Some(right)) => left.cmp(right),
        (false, Some(_), None) => std::cmp::Ordering::Less,
        (false, None, Some(_)) => std::cmp::Ordering::Greater,
        (false, None, None) => std::cmp::Ordering::Equal,
        (true, Some(left), Some(right)) => right.cmp(left),
        (true, Some(_), None) => std::cmp::Ordering::Greater,
        (true, None, Some(_)) => std::cmp::Ordering::Less,
        (true, None, None) => std::cmp::Ordering::Equal,
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
    property: &CascadedProperty,
) -> bool {
    declarations[index + 1..].iter().any(|declaration| {
        (declaration.property == *property
            || matches!(
                (declaration.property.modeled(), property.modeled()),
                (
                    Some(ModeledProperty::Longhand(left)),
                    Some(ModeledProperty::FontComponent(right))
                ) | (
                    Some(ModeledProperty::FontComponent(left)),
                    Some(ModeledProperty::Longhand(right))
                ) if left == right
            ))
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
                    &candidate.property,
                    &declaration.property,
                    direction,
                    writing_mode,
                ) || !same_or_stronger_reverted_origin(candidate, declaration)
            });
        } else if declaration_is_revert_layer(&declaration.value) {
            output.retain(|candidate: &CascadedDeclaration<'_>| {
                !declarations_affect_same_property_in_context(
                    &candidate.property,
                    &declaration.property,
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
    let Some(left) = ModeledProperty::parse(left) else {
        return false;
    };
    let Some(right) = ModeledProperty::parse(right) else {
        return false;
    };
    declarations_affect_same_property_in_context(
        &CascadedProperty::Modeled(left),
        &CascadedProperty::Modeled(right),
        Direction::Ltr,
        WritingMode::HorizontalTb,
    )
}

pub(in crate::css) fn declarations_affect_same_property_in_context(
    left: &CascadedProperty,
    right: &CascadedProperty,
    direction: Direction,
    writing_mode: WritingMode,
) -> bool {
    let (Some(left), Some(right)) = (left.modeled(), right.modeled()) else {
        return false;
    };
    let left_longhands = affected_longhands(left, direction, writing_mode);
    let right_longhands = affected_longhands(right, direction, writing_mode);
    left_longhands
        .into_iter()
        .any(|left| right_longhands.into_iter().any(|right| left == right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_non_custom_declarations_are_ignored_at_the_cascade_boundary() {
        let declarations = crate::css::parse_declarations(
            "-weasy-anchor: attr(id); color: green; --preserved-custom: value",
        );

        let cascaded = cascaded_declarations_from(&declarations, StylesheetOrigin::Author);

        assert_eq!(cascaded.len(), 2);
        assert_eq!(cascaded[0].property.css_name(), "color");
        assert_eq!(
            cascaded[1].property.custom_name(),
            Some("--preserved-custom")
        );
    }
}
