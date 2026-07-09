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

/// Expands simple modeled shorthands into their longhands before rollback.
///
/// CSS Cascade Level 5 says shorthands set all of their longhands. Expanding
/// them before `revert-layer` lets a longhand rollback remove only that
/// longhand from an earlier shorthand while preserving unaffected sides:
/// <https://www.w3.org/TR/css-cascade-5/#shorthand>.
pub(in crate::css) fn expand_modeled_shorthands<'a>(
    declarations: &'a [CascadedDeclaration<'a>],
    direction: Direction,
    writing_mode: WritingMode,
) -> Vec<CascadedDeclaration<'a>> {
    let mut expanded = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        if contains_css_variable_reference(&declaration.value)
            || declaration_is_revert(&declaration.value)
            || declaration_is_revert_layer(&declaration.value)
        {
            expanded.push(declaration.clone());
            continue;
        }
        if let Some(parts) = expand_box_edge_shorthand(&declaration.name, &declaration.value) {
            for (name, value) in parts {
                let mut longhand = declaration.clone();
                longhand.name = Cow::Owned(name.to_string());
                longhand.value = Cow::Owned(value);
                expanded.push(longhand);
            }
        } else if let Some(parts) = expand_simple_modeled_shorthand(
            &declaration.name,
            &declaration.value,
            direction,
            writing_mode,
        ) {
            for (name, value) in parts {
                let mut longhand = declaration.clone();
                longhand.name = Cow::Owned(name.to_string());
                longhand.value = Cow::Owned(value);
                expanded.push(longhand);
            }
        } else {
            expanded.push(declaration.clone());
        }
    }
    expanded
}

pub(in crate::css) fn expand_box_edge_shorthand(
    name: &str,
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let names = match name {
        "margin" => ["margin-top", "margin-right", "margin-bottom", "margin-left"],
        "padding" => [
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
        ],
        "scroll-padding" => [
            "scroll-padding-top",
            "scroll-padding-right",
            "scroll-padding-bottom",
            "scroll-padding-left",
        ],
        "scroll-margin" => [
            "scroll-margin-top",
            "scroll-margin-right",
            "scroll-margin-bottom",
            "scroll-margin-left",
        ],
        "inset" => ["top", "right", "bottom", "left"],
        _ => return None,
    };
    let value = trim_css_value(value);
    let parts = split_css_component_values(value);
    let [top, right, bottom, left] = match parts.as_slice() {
        [all] => [*all, *all, *all, *all],
        [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
        [top, horizontal, bottom] => [*top, *horizontal, *bottom, *horizontal],
        [top, right, bottom, left] => [*top, *right, *bottom, *left],
        _ => return None,
    };
    Some(
        names
            .into_iter()
            .zip([top, right, bottom, left])
            .map(|(name, value)| (name, value.to_string()))
            .collect(),
    )
}

pub(in crate::css) fn expand_simple_modeled_shorthand(
    name: &str,
    value: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    match name {
        "gap" => expand_gap_shorthand(value),
        "grid-gap" => expand_gap_shorthand(value),
        "grid-row-gap" => parse_gap(value, ROOT_FONT_SIZE_PT)
            .map(|_| vec![("row-gap", trim_css_value(value).to_string())]),
        "grid-column-gap" => parse_gap(value, ROOT_FONT_SIZE_PT)
            .map(|_| vec![("column-gap", trim_css_value(value).to_string())]),
        "column-rule" => expand_gap_rule_shorthand(value, "column-rule"),
        "row-rule" => expand_gap_rule_shorthand(value, "row-rule"),
        "rule" => expand_rule_shorthand(value),
        "rule-width" => expand_rule_axis_shorthand(value, "width"),
        "rule-style" => expand_rule_axis_shorthand(value, "style"),
        "rule-color" => expand_rule_axis_shorthand(value, "color"),
        "rule-break" => expand_rule_axis_shorthand(value, "break"),
        "rule-visibility-items" => expand_rule_axis_shorthand(value, "visibility-items"),
        "rule-inset" => expand_rule_axis_shorthand(value, "inset"),
        "rule-inset-start" => expand_rule_axis_shorthand(value, "inset-start"),
        "rule-inset-end" => expand_rule_axis_shorthand(value, "inset-end"),
        "rule-inset-cap" => expand_rule_axis_shorthand(value, "inset-cap"),
        "rule-inset-junction" => expand_rule_axis_shorthand(value, "inset-junction"),
        "column-rule-inset" | "row-rule-inset" => expand_gap_rule_inset_shorthand(name, value),
        "column-rule-inset-start"
        | "column-rule-inset-end"
        | "row-rule-inset-start"
        | "row-rule-inset-end" => expand_gap_rule_inset_side_shorthand(name, value),
        "column-rule-inset-cap"
        | "column-rule-inset-junction"
        | "row-rule-inset-cap"
        | "row-rule-inset-junction" => expand_gap_rule_inset_kind_shorthand(name, value),
        "flex-flow" => expand_flex_flow_shorthand(value),
        "flex" => expand_flex_shorthand(value),
        "grid-row" => expand_grid_placement_shorthand(value, "grid-row-start", "grid-row-end"),
        "grid-column" => {
            expand_grid_placement_shorthand(value, "grid-column-start", "grid-column-end")
        }
        "grid" => expand_grid_shorthand(value),
        "grid-template" => expand_grid_template_shorthand(value),
        "grid-area" => expand_grid_area_shorthand(value),
        "place-content" | "place-items" | "place-self" => {
            expand_alignment_place_shorthand(name, value)
        }
        "columns" => expand_columns_shorthand(value),
        "list-style" => expand_list_style_shorthand(value),
        "inline-size" | "block-size" | "min-inline-size" | "max-inline-size" | "min-block-size"
        | "max-block-size" => expand_logical_size_value(name, value, writing_mode),
        "contain-intrinsic-inline-size" | "contain-intrinsic-block-size" => {
            expand_logical_contain_intrinsic_size_value(name, value, writing_mode)
        }
        "margin-block"
        | "margin-inline"
        | "margin-block-start"
        | "margin-block-end"
        | "margin-inline-start"
        | "margin-inline-end" => {
            expand_logical_box_edge_values(name, value, "margin", direction, writing_mode)
        }
        "padding-block"
        | "padding-inline"
        | "padding-block-start"
        | "padding-block-end"
        | "padding-inline-start"
        | "padding-inline-end" => {
            expand_logical_box_edge_values(name, value, "padding", direction, writing_mode)
        }
        "scroll-padding-block"
        | "scroll-padding-inline"
        | "scroll-padding-block-start"
        | "scroll-padding-block-end"
        | "scroll-padding-inline-start"
        | "scroll-padding-inline-end" => {
            expand_logical_box_edge_values(name, value, "scroll-padding", direction, writing_mode)
        }
        "scroll-margin-block"
        | "scroll-margin-inline"
        | "scroll-margin-block-start"
        | "scroll-margin-block-end"
        | "scroll-margin-inline-start"
        | "scroll-margin-inline-end" => {
            expand_logical_box_edge_values(name, value, "scroll-margin", direction, writing_mode)
        }
        "inset-block" | "inset-inline" | "inset-block-start" | "inset-block-end"
        | "inset-inline-start" | "inset-inline-end" => {
            expand_logical_box_edge_values(name, value, "inset", direction, writing_mode)
        }
        "scroll-padding" | "scroll-margin" => expand_scroll_edge_shorthand(name, value),
        "border" => expand_border_shorthand(value),
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            expand_border_side_shorthand(name, value)
        }
        "outline" => expand_outline_shorthand(value),
        "border-radius" => expand_border_radius_shorthand(value),
        "corner" => expand_corner_shorthand(value),
        "corner-shape" => expand_corner_shape_shorthand(value),
        "border-block" | "border-inline" => {
            expand_logical_border_shorthand(name, value, direction, writing_mode)
        }
        "border-block-start" | "border-block-end" | "border-inline-start" | "border-inline-end" => {
            expand_logical_border_side_shorthand(name, value, direction, writing_mode)
        }
        "border-block-width" | "border-inline-width" => {
            expand_logical_border_axis_values(name, value, "width", direction, writing_mode)
        }
        "border-block-style" | "border-inline-style" => {
            expand_logical_border_axis_values(name, value, "style", direction, writing_mode)
        }
        "border-block-color" | "border-inline-color" => {
            expand_logical_border_axis_values(name, value, "color", direction, writing_mode)
        }
        _ => None,
    }
}

pub(in crate::css) fn expand_logical_size_value(
    name: &str,
    value: &str,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    Some(vec![(
        logical_size_physical_longhand(name, writing_mode)?,
        trim_css_value(value).to_string(),
    )])
}

/// Expand a logical intrinsic-size override to the physical component stored
/// by the computed style.
///
/// CSS Containment's logical longhands follow the element's writing mode in
/// the same way as CSS Logical Properties sizing longhands:
/// <https://drafts.csswg.org/css-contain-3/#contain-intrinsic-size> and
/// <https://www.w3.org/TR/css-logical-1/#dimension-properties>.
fn expand_logical_contain_intrinsic_size_value(
    name: &str,
    value: &str,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    let axes = WritingModeAxes::new(writing_mode, Direction::Ltr);
    let axis = match name {
        "contain-intrinsic-inline-size" => axes.physical_axis(LogicalAxis::Inline),
        "contain-intrinsic-block-size" => axes.physical_axis(LogicalAxis::Block),
        _ => return None,
    };
    let physical_name = match axis {
        PhysicalAxis::Horizontal => "contain-intrinsic-width",
        PhysicalAxis::Vertical => "contain-intrinsic-height",
    };
    Some(vec![(physical_name, trim_css_value(value).to_string())])
}

/// Return the physical sizing longhand addressed by a logical size property.
///
/// CSS Logical Properties maps inline/block size longhands through the
/// element's writing mode:
/// <https://www.w3.org/TR/css-logical-1/#dimension-properties>.
pub(in crate::css) fn logical_size_physical_longhand(
    name: &str,
    writing_mode: WritingMode,
) -> Option<&'static str> {
    let axes = WritingModeAxes::new(writing_mode, Direction::Ltr);
    let inline_axis = axes.physical_axis(LogicalAxis::Inline);
    let block_axis = axes.physical_axis(LogicalAxis::Block);
    match name {
        "inline-size" => Some(size_longhand_for_axis(inline_axis)),
        "block-size" => Some(size_longhand_for_axis(block_axis)),
        "min-inline-size" => Some(min_size_longhand_for_axis(inline_axis)),
        "max-inline-size" => Some(max_size_longhand_for_axis(inline_axis)),
        "min-block-size" => Some(min_size_longhand_for_axis(block_axis)),
        "max-block-size" => Some(max_size_longhand_for_axis(block_axis)),
        _ => None,
    }
}

pub(in crate::css) fn size_longhand_for_axis(axis: PhysicalAxis) -> &'static str {
    match axis {
        PhysicalAxis::Horizontal => "width",
        PhysicalAxis::Vertical => "height",
    }
}

pub(in crate::css) fn min_size_longhand_for_axis(axis: PhysicalAxis) -> &'static str {
    match axis {
        PhysicalAxis::Horizontal => "min-width",
        PhysicalAxis::Vertical => "min-height",
    }
}

pub(in crate::css) fn max_size_longhand_for_axis(axis: PhysicalAxis) -> &'static str {
    match axis {
        PhysicalAxis::Horizontal => "max-width",
        PhysicalAxis::Vertical => "max-height",
    }
}

pub(in crate::css) struct ListStyleShorthandComponents {
    pub(in crate::css) style_type: String,
    pub(in crate::css) position: String,
    pub(in crate::css) image: String,
}

/// Expands the CSS Lists `list-style` shorthand into its three longhands.
///
/// CSS Lists Level 3 defines `list-style` as an unordered shorthand for
/// `list-style-type`, `list-style-position`, and `list-style-image`, with
/// ambiguous `none` tokens assigned to whichever of type/image are not
/// otherwise specified:
/// <https://www.w3.org/TR/css-lists-3/#propdef-list-style>.
pub(in crate::css) fn expand_list_style_shorthand(
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let components = parse_list_style_shorthand(value)?;
    Some(vec![
        ("list-style-type", components.style_type),
        ("list-style-position", components.position),
        ("list-style-image", components.image),
    ])
}

pub(in crate::css) fn parse_list_style_shorthand(
    value: &str,
) -> Option<ListStyleShorthandComponents> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }

    let mut style_type = None;
    let mut position = None;
    let mut image = None;
    let mut none_count = 0usize;

    for part in parts {
        if part.eq_ignore_ascii_case("none") {
            none_count += 1;
            continue;
        }

        if image.is_none() && parse_list_style_image_component(part).is_some() {
            image = Some(part.to_string());
        } else if position.is_none() && parse_list_style_position(part).is_some() {
            position = Some(part.to_string());
        } else if style_type.is_none() && parse_list_style_type(part).is_some() {
            style_type = Some(part.to_string());
        } else {
            return None;
        }
    }

    if style_type.is_none() && none_count > 0 {
        style_type = Some("none".to_string());
        none_count -= 1;
    }
    if image.is_none() && none_count > 0 {
        image = Some("none".to_string());
        none_count -= 1;
    }
    if none_count > 0 {
        return None;
    }

    Some(ListStyleShorthandComponents {
        style_type: style_type.unwrap_or_else(|| "disc".to_string()),
        position: position.unwrap_or_else(|| "outside".to_string()),
        image: image.unwrap_or_else(|| "none".to_string()),
    })
}

pub(in crate::css) fn parse_list_style_image_component(value: &str) -> Option<Option<String>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    let (url, tail) = parse_css_url_token(value)?;
    tail.trim().is_empty().then_some(Some(url))
}

/// Expands logical margin and padding properties into physical longhands.
///
/// CSS Logical Properties defines flow-relative box edges as aliases for
/// physical margin and padding edges after resolving `writing-mode` and
/// `direction`:
/// <https://www.w3.org/TR/css-logical-1/#box>.
pub(in crate::css) fn expand_logical_box_edge_values(
    name: &str,
    value: &str,
    property: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    let longhand = |logical_side| {
        let side = logical_box_side(logical_side, direction, writing_mode)?;
        match property {
            "margin" => Some(physical_margin_side_longhand(side)),
            "padding" => Some(physical_padding_side_longhand(side)),
            "scroll-padding" => Some(physical_scroll_padding_side_longhand(side)),
            "scroll-margin" => Some(physical_scroll_margin_side_longhand(side)),
            "inset" => Some(physical_inset_side_longhand(side)),
            _ => None,
        }
    };
    if matches!(
        name,
        "margin-block"
            | "margin-inline"
            | "padding-block"
            | "padding-inline"
            | "scroll-padding-block"
            | "scroll-padding-inline"
            | "scroll-margin-block"
            | "scroll-margin-inline"
            | "inset-block"
            | "inset-inline"
    ) {
        let [start, end] = logical_box_axis_side_names(name)?;
        let parts = split_css_component_values(trim_css_value(value));
        let [start_value, end_value] = match parts.as_slice() {
            [all] => [*all, *all],
            [start, end] => [*start, *end],
            _ => return None,
        };
        return Some(vec![
            (longhand(start)?, start_value.to_string()),
            (longhand(end)?, end_value.to_string()),
        ]);
    }
    Some(vec![(longhand(name)?, trim_css_value(value).to_string())])
}

/// Expand the physical scroll padding and margin shorthands exactly as the
/// corresponding four-sided box shorthands.
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-padding>
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-margin>
fn expand_scroll_edge_shorthand(name: &str, value: &str) -> Option<Vec<(&'static str, String)>> {
    let values = split_css_component_values(trim_css_value(value));
    let [top, right, bottom, left] = match values.as_slice() {
        [all] => [*all, *all, *all, *all],
        [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
        [top, horizontal, bottom] => [*top, *horizontal, *bottom, *horizontal],
        [top, right, bottom, left] => [*top, *right, *bottom, *left],
        _ => return None,
    };
    let names = match name {
        "scroll-padding" => [
            "scroll-padding-top",
            "scroll-padding-right",
            "scroll-padding-bottom",
            "scroll-padding-left",
        ],
        "scroll-margin" => [
            "scroll-margin-top",
            "scroll-margin-right",
            "scroll-margin-bottom",
            "scroll-margin-left",
        ],
        _ => return None,
    };
    Some(vec![
        (names[0], top.to_string()),
        (names[1], right.to_string()),
        (names[2], bottom.to_string()),
        (names[3], left.to_string()),
    ])
}

pub(in crate::css) fn physical_scroll_padding_side_longhand(side: BoxSide) -> &'static str {
    match side {
        BoxSide::Top => "scroll-padding-top",
        BoxSide::Right => "scroll-padding-right",
        BoxSide::Bottom => "scroll-padding-bottom",
        BoxSide::Left => "scroll-padding-left",
    }
}

pub(in crate::css) fn physical_scroll_margin_side_longhand(side: BoxSide) -> &'static str {
    match side {
        BoxSide::Top => "scroll-margin-top",
        BoxSide::Right => "scroll-margin-right",
        BoxSide::Bottom => "scroll-margin-bottom",
        BoxSide::Left => "scroll-margin-left",
    }
}

pub(in crate::css) fn physical_inset_side_longhand(side: BoxSide) -> &'static str {
    match side {
        BoxSide::Top => "top",
        BoxSide::Right => "right",
        BoxSide::Bottom => "bottom",
        BoxSide::Left => "left",
    }
}

/// Expand the physical `border` shorthand into side longhands.
///
/// CSS Backgrounds and Borders defines `border` as setting the width, style,
/// and color of all four physical border sides, with omitted components
/// resetting to their initial values:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-shorthands>.
pub(in crate::css) fn expand_border_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let mut expanded = Vec::new();
    for side in ["border-top", "border-right", "border-bottom", "border-left"] {
        expanded.extend(expand_border_side_shorthand(side, value)?);
    }
    Some(expanded)
}

/// Expand one physical side border shorthand into width/style/color longhands.
///
/// This runs before Cascade 5 rollback handling so longhand `revert-layer`
/// declarations can remove only the affected width, style, or color component:
/// <https://www.w3.org/TR/css-cascade-5/#shorthand>.
pub(in crate::css) fn expand_border_side_shorthand(
    name: &str,
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let [width_name, style_name, color_name] = match name {
        "border-top" => ["border-top-width", "border-top-style", "border-top-color"],
        "border-right" => [
            "border-right-width",
            "border-right-style",
            "border-right-color",
        ],
        "border-bottom" => [
            "border-bottom-width",
            "border-bottom-style",
            "border-bottom-color",
        ],
        "border-left" => [
            "border-left-width",
            "border-left-style",
            "border-left-color",
        ],
        _ => return None,
    };
    let components = border_shorthand_components(value)?;
    Some(vec![
        (width_name, components.width),
        (style_name, components.style),
        (color_name, components.color),
    ])
}

pub(in crate::css) struct BorderShorthandComponents {
    pub(in crate::css) width: String,
    pub(in crate::css) style: String,
    pub(in crate::css) color: String,
}

pub(in crate::css) fn border_shorthand_components(
    value: &str,
) -> Option<BorderShorthandComponents> {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    for part in split_css_component_values(value) {
        let mut recognized = false;
        if width.is_none() && parse_computed_border_width(part, ROOT_FONT_SIZE_PT).is_some() {
            width = Some(part.to_string());
            recognized = true;
        }
        if style.is_none() && parse_border_style(part).is_some() {
            style = Some(part.to_string());
            recognized = true;
        }
        if color.is_none() && parse_border_color(part, Color::BLACK).is_some() {
            color = Some(part.to_string());
            recognized = true;
        }
        if !recognized {
            return None;
        }
    }
    Some(BorderShorthandComponents {
        width: width.unwrap_or_else(|| "medium".to_string()),
        style: style.unwrap_or_else(|| "none".to_string()),
        color: color.unwrap_or_else(|| "currentColor".to_string()),
    })
}

/// Expand the CSS outline shorthand into width/style/color longhands.
///
/// CSS UI defines `outline` as a shorthand for `outline-width`,
/// `outline-style`, and `outline-color`. Unlike borders, outlines do not affect
/// box metrics, so only the paint properties are modeled here:
/// <https://www.w3.org/TR/css-ui-3/#outline-props>.
pub(in crate::css) fn expand_outline_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let components = border_shorthand_components(value)?;
    Some(vec![
        ("outline-width", components.width),
        ("outline-style", components.style),
        ("outline-color", components.color),
    ])
}

/// Expand logical border axis shorthands using computed flow direction.
///
/// CSS Logical Properties maps `border-block` and `border-inline` to physical
/// side border shorthands through `writing-mode` and `direction`:
/// <https://www.w3.org/TR/css-logical-1/#border-shorthands>.
pub(in crate::css) fn expand_logical_border_shorthand(
    name: &str,
    value: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    let logical_sides = match name {
        "border-block" => ["border-block-start", "border-block-end"],
        "border-inline" => ["border-inline-start", "border-inline-end"],
        _ => return None,
    };
    let mut expanded = Vec::new();
    for logical_side in logical_sides {
        let side = physical_border_side_shorthand(logical_border_side(
            logical_side,
            direction,
            writing_mode,
        )?);
        expanded.extend(expand_border_side_shorthand(side, value)?);
    }
    Some(expanded)
}

/// Expand one logical border side shorthand using computed flow direction.
///
/// The logical side properties are flow-relative aliases for physical side
/// border shorthands:
/// <https://www.w3.org/TR/css-logical-1/#border-properties>.
pub(in crate::css) fn expand_logical_border_side_shorthand(
    name: &str,
    value: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    let side = physical_border_side_shorthand(logical_border_side(name, direction, writing_mode)?);
    expand_border_side_shorthand(side, value)
}

/// Expand logical border width/style/color axis shorthands.
///
/// CSS Logical Properties lets the axis shorthands take one or two values for
/// start/end. Expansion happens before cascade rollback so physical and
/// logical declarations affect the same modeled longhands:
/// <https://www.w3.org/TR/css-logical-1/#border-shorthands>.
pub(in crate::css) fn expand_logical_border_axis_values(
    name: &str,
    value: &str,
    component: &'static str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    let logical_sides = match name {
        "border-block-width" | "border-block-style" | "border-block-color" => {
            ["border-block-start", "border-block-end"]
        }
        "border-inline-width" | "border-inline-style" | "border-inline-color" => {
            ["border-inline-start", "border-inline-end"]
        }
        _ => return None,
    };
    let sides = logical_sides.map(|logical_side| {
        physical_border_side_component(
            logical_border_side(logical_side, direction, writing_mode).unwrap(),
            component,
        )
    });
    let parts = split_css_component_values(value)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let values = match parts.as_slice() {
        [all] => [all.clone(), all.clone()],
        [start, end] => [start.clone(), end.clone()],
        _ => return None,
    };
    Some(vec![
        (sides[0], values[0].clone()),
        (sides[1], values[1].clone()),
    ])
}

pub(in crate::css) fn physical_border_side_shorthand(side: BorderSide) -> &'static str {
    match side {
        BorderSide::Top => "border-top",
        BorderSide::Right => "border-right",
        BorderSide::Bottom => "border-bottom",
        BorderSide::Left => "border-left",
    }
}

pub(in crate::css) fn physical_border_side_component(
    side: BorderSide,
    component: &str,
) -> &'static str {
    match (side, component) {
        (BorderSide::Top, "width") => "border-top-width",
        (BorderSide::Right, "width") => "border-right-width",
        (BorderSide::Bottom, "width") => "border-bottom-width",
        (BorderSide::Left, "width") => "border-left-width",
        (BorderSide::Top, "style") => "border-top-style",
        (BorderSide::Right, "style") => "border-right-style",
        (BorderSide::Bottom, "style") => "border-bottom-style",
        (BorderSide::Left, "style") => "border-left-style",
        (BorderSide::Top, "color") => "border-top-color",
        (BorderSide::Right, "color") => "border-right-color",
        (BorderSide::Bottom, "color") => "border-bottom-color",
        (BorderSide::Left, "color") => "border-left-color",
        _ => unreachable!("invalid border side component"),
    }
}

/// Expand `border-radius` into physical corner radius longhands.
///
/// CSS Cascade Level 5 treats shorthands as declarations for all longhands
/// before cascade-wide rollback keywords are applied, and CSS Backgrounds and
/// Borders Level 3 defines the slash-separated horizontal/vertical corner
/// grammar:
/// <https://www.w3.org/TR/css-cascade-5/#shorthand> and
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-radius>.
pub(in crate::css) fn expand_border_radius_shorthand(
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let (horizontal, vertical) = split_border_radius_groups(value)?;
    let horizontal = expand_four_radius_components(&horizontal)?;
    let vertical = if vertical.is_empty() {
        horizontal.clone()
    } else {
        expand_four_radius_components(&vertical)?
    };
    Some(vec![
        (
            "border-top-left-radius",
            radius_pair(&horizontal[0], &vertical[0]),
        ),
        (
            "border-top-right-radius",
            radius_pair(&horizontal[1], &vertical[1]),
        ),
        (
            "border-bottom-right-radius",
            radius_pair(&horizontal[2], &vertical[2]),
        ),
        (
            "border-bottom-left-radius",
            radius_pair(&horizontal[3], &vertical[3]),
        ),
    ])
}

/// Split the horizontal and vertical `border-radius` component groups.
///
/// The slash separator is only valid at the top level; function arguments such
/// as `calc()` must remain intact as component values:
/// <https://www.w3.org/TR/css-syntax-3/#component-value>.
pub(in crate::css) fn split_border_radius_groups(
    value: &str,
) -> Option<(Vec<String>, Vec<String>)> {
    let (horizontal, vertical) = split_top_level_once(value, '/').unwrap_or((value, ""));
    let horizontal = split_css_component_values(horizontal)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let vertical = split_css_component_values(vertical)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    (!horizontal.is_empty()).then_some((horizontal, vertical))
}

pub(in crate::css) fn split_css_top_level_slashes(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = value;
    while let Some((left, right)) = split_top_level_once(rest, '/') {
        parts.push(left.trim());
        rest = right;
    }
    parts.push(rest.trim());
    parts
}

/// Find a top-level delimiter without splitting nested CSS component values.
///
/// CSS Syntax Level 3 models function bodies and bracketed values as nested
/// component values:
/// <https://www.w3.org/TR/css-syntax-3/#component-value>.
pub(in crate::css) fn split_top_level_once(value: &str, delimiter: char) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if ch == delimiter && depth == 0 => {
                return Some((&value[..index], &value[index + ch.len_utf8()..]));
            }
            _ => {}
        }
    }
    None
}

/// Expand one-to-four corner radius values using CSS box-edge ordering.
///
/// CSS Backgrounds and Borders Level 3 uses top, right, bottom, left expansion
/// for the horizontal and vertical radius groups:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-radius>.
pub(in crate::css) fn expand_four_radius_components(values: &[String]) -> Option<Vec<String>> {
    match values {
        [all] => Some(vec![all.clone(), all.clone(), all.clone(), all.clone()]),
        [vertical, horizontal] => Some(vec![
            vertical.clone(),
            horizontal.clone(),
            vertical.clone(),
            horizontal.clone(),
        ]),
        [top, horizontal, bottom] => Some(vec![
            top.clone(),
            horizontal.clone(),
            bottom.clone(),
            horizontal.clone(),
        ]),
        [top, right, bottom, left] => Some(vec![
            top.clone(),
            right.clone(),
            bottom.clone(),
            left.clone(),
        ]),
        _ => None,
    }
}

/// Serialize one physical corner radius longhand from horizontal/vertical radii.
///
/// A corner radius longhand accepts one value when both radii match, otherwise
/// two values for horizontal then vertical radius:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-radius>.
pub(in crate::css) fn radius_pair(horizontal: &str, vertical: &str) -> String {
    if horizontal == vertical {
        horizontal.to_string()
    } else {
        format!("{horizontal} {vertical}")
    }
}
